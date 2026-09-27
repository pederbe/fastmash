#!/usr/bin/env python3
"""Finish the website after `mdbook build docs`.

usage: build_site.py BOOK_DIR

The documentation is an mdBook in docs/; the landing page is a hand-written
static page in site/. This:

- puts the landing page at the site root (replacing the index.html mdBook
  writes for its first chapter, which stays available as introduction.html),
  inlines the generated benchmark chart (docs/src/benchmarks/core-jobs.svg)
  where the page says `<!-- chart -->`, fills `@VERSION@` with the fastmash
  package version (crates/cli/Cargo.toml), and copies its CSS and script;
- copies install.sh, served at https://fastmash.io/install.sh;
- writes a Markdown copy of every docs page next to its HTML (guide/output.md
  beside guide/output.html), with mdBook includes resolved, and links it from
  the page as its text/markdown alternate;
- renders each Mermaid diagram to static SVG, in a dark and a light variant
  (site/diagrams/*.json), and replaces the code block with a <figure> whose
  caption is the `<!-- description: ... -->` comment that must follow it; no
  diagram code is served. Renders are committed in site/diagrams/rendered/,
  named by a hash of the diagram and its theme, so the site builds without a
  browser. A diagram without a committed render is rendered with mermaid-cli
  from `npm ci --ignore-scripts` (package.json) and Puppeteer's Chrome (`npx
  puppeteer browsers install chrome`; FASTMASH_MMDC overrides the mmdc path)
  and must then be committed. With FASTMASH_DIAGRAMS=committed (the deploy
  build and CI) a missing or unused render is an error instead;
- writes llms.txt (https://llmstxt.org), sitemap.xml and robots.txt.
"""
import hashlib
import html
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from xml.sax.saxutils import escape

ROOT = Path(__file__).resolve().parents[1]
SITE = 'https://fastmash.io'
SRC = ROOT / 'docs' / 'src'
INCLUDE = re.compile(r'\{\{#include\s+([^}\s]+)\s*\}\}')
MERMAID = re.compile(r'<pre><code class="language-mermaid">(.*?)</code></pre>\s*'
                     r'(?:<!-- description: (.*?) -->)?', re.S)


def version():
    manifest = (ROOT / 'crates' / 'cli' / 'Cargo.toml').read_text(encoding='utf-8')
    return re.search(r'^version = "([^"]+)"', manifest, re.M).group(1)


def chapters():
    """(section, title, path) for each chapter in SUMMARY.md, in order."""
    section = 'Docs'
    for line in (SRC / 'SUMMARY.md').read_text(encoding='utf-8').splitlines():
        heading = re.match(r'#\s+(.+)', line)
        if heading and not line.startswith('# Summary'):
            section = heading.group(1).strip()
        link = re.search(r'\[([^\]]+)\]\(([^)]+\.md)\)', line)
        if link:
            yield section, link.group(1), link.group(2)


def markdown(path):
    """A chapter's Markdown with mdBook includes resolved."""
    source = SRC / path

    def include(match):
        target, *lines = match.group(1).split(':')
        included = (source.parent / target).resolve()
        if included.suffix == '.svg':
            return f'![Benchmark chart]({target})'
        text = included.read_text(encoding='utf-8').splitlines()
        start = int(lines[0]) - 1 if lines and lines[0] else 0
        end = int(lines[1]) if len(lines) > 1 and lines[1] else len(text)
        return '\n'.join(text[start:end])

    return INCLUDE.sub(include, source.read_text(encoding='utf-8'))


def landing(book):
    page = (ROOT / 'site' / 'index.html').read_text(encoding='utf-8')
    chart = (SRC / 'benchmarks' / 'core-jobs.svg').read_text(encoding='utf-8')
    if page.count('<!-- chart -->') != 1:
        sys.exit('site/index.html must contain exactly one <!-- chart --> marker')
    page = page.replace('<!-- chart -->', chart.strip()).replace('@VERSION@', version())
    (book / 'index.html').write_text(page, encoding='utf-8')
    for name in ('landing.css', 'landing.js', 'demo.gif'):
        shutil.copyfile(ROOT / 'site' / name, book / name)
    shutil.copyfile(ROOT / 'install.sh', book / 'install.sh')
    shutil.copyfile(ROOT / 'site' / 'og.png', book / 'og.png')
    shutil.copyfile(ROOT / 'site' / '_headers', book / '_headers')  # Cloudflare response headers
    # mdBook 0.5 names its static files with a content hash (favicon-1a2b.svg);
    # the landing page uses stable names, so copy each one under that name.
    for needed in ('favicon.svg', 'fonts/source-code-pro-v11-all-charsets-500.woff2'):
        target = book / needed
        if not target.is_file():
            stem, suffix = target.stem, target.suffix
            hashed = sorted(target.parent.glob(f'{stem}-*{suffix}'))
            if len(hashed) != 1:
                sys.exit(f'the landing page needs {needed} from the book build')
            shutil.copyfile(hashed[0], target)


def docs_markdown(book):
    for _, _, path in chapters():
        (book / path).parent.mkdir(parents=True, exist_ok=True)
        (book / path).write_text(markdown(path), encoding='utf-8')
        html = book / Path(path).with_suffix('.html')
        page = html.read_text(encoding='utf-8')
        url = f'{SITE}/{Path(path).with_suffix(".html").as_posix()}'
        alternate = (f'<link rel="alternate" type="text/markdown" '
                     f'href="{Path(path).name}">\n'
                     f'<link rel="canonical" href="{url}">\n'
                     f'<meta property="og:url" content="{url}">\n')
        if 'type="text/markdown"' not in page:
            html.write_text(page.replace('</head>', alternate + '</head>', 1), encoding='utf-8')


def render(source, theme, target):
    mmdc = os.environ.get('FASTMASH_MMDC') or str(ROOT / 'node_modules' / '.bin' / 'mmdc')
    config = ROOT / 'site' / 'diagrams'
    with tempfile.TemporaryDirectory() as tmp:
        diagram = Path(tmp) / 'diagram.mmd'
        diagram.write_text(source, encoding='utf-8')
        subprocess.run([mmdc, '-q', '-i', str(diagram), '-o', str(target), '-b', 'transparent',
                        '-c', str(config / f'{theme}.json'), '-p', str(config / 'puppeteer.json')],
                       check=True, timeout=120)
    # Mermaid sizes its SVG to its container (width="100%"); an <img> needs the
    # drawing's own size, from the viewBox.
    svg = target.read_text(encoding='utf-8')
    box = re.search(r'viewBox="[-\d.]+ [-\d.]+ ([\d.]+) ([\d.]+)"', svg)
    width, height = (round(float(v)) for v in box.groups())
    svg = re.sub(r'<svg([^>]*?) width="100%"', lambda m: f'<svg{m.group(1)} width="{width}" height="{height}"', svg, count=1)
    target.write_text(svg, encoding='utf-8')


RENDERED = ROOT / 'site' / 'diagrams' / 'rendered'


def diagrams(book):
    out = book / 'diagrams'
    out.mkdir(exist_ok=True)
    committed_only = os.environ.get('FASTMASH_DIAGRAMS') == 'committed'
    themes = {theme: (ROOT / 'site' / 'diagrams' / f'{theme}.json').read_bytes()
              for theme in ('dark', 'light')}
    used = set()
    for page_path in sorted(book.rglob('*.html')):
        page = page_path.read_text(encoding='utf-8')
        if 'language-mermaid' not in page:
            continue
        root = os.path.relpath(book, page_path.parent).replace(os.sep, '/')

        def figure(match):
            source = html.unescape(match.group(1))
            description = match.group(2)
            if not description:
                sys.exit(f'{page_path}: a Mermaid diagram needs a <!-- description: ... --> after it')
            images = []
            for theme, config in themes.items():
                key = hashlib.sha256(source.encode() + b'\0' + config).hexdigest()[:16]
                name = f'{key}-{theme}.svg'
                used.add(name)
                if not (RENDERED / name).exists():
                    if committed_only:
                        sys.exit(f'{page_path}: diagram {name} has no committed render; run '
                                 'scripts/build_site.py with mermaid-cli and commit site/diagrams/rendered/')
                    RENDERED.mkdir(exist_ok=True)
                    render(source, theme, RENDERED / name)
                shutil.copyfile(RENDERED / name, out / name)
                images.append(f'<img class="diagram-{theme}" src="{root}/diagrams/{name}" '
                              f'alt="" loading="lazy">')
            return (f'<figure class="diagram">{"".join(images)}'
                    f'<figcaption>{html.escape(description.strip())}</figcaption></figure>')

        page_path.write_text(MERMAID.sub(figure, page), encoding='utf-8')
    unused = sorted(p.name for p in RENDERED.glob('*.svg') if p.name not in used) if RENDERED.is_dir() else []
    if unused and committed_only:
        sys.exit(f'site/diagrams/rendered/ has unused renders: {", ".join(unused)}')


def llms_txt(book):
    lines = [
        '# Fastmash',
        '',
        '> Fast command-line statistics for delimited text: a Rust implementation of',
        '> the GNU datamash command language, up to four times faster on real jobs,',
        '> with identical results on every machine.',
        '',
        f'Version {version()}. Linux x86-64 and WSL2. Install:',
        '`curl -fsSL https://fastmash.io/install.sh | sh`, or `cargo install --locked fastmash`.',
        'Commands, options and output follow GNU datamash 1.9; the differences page',
        'lists every intentional difference. Created by Peder Bergan (https://pederbe.dev).',
    ]
    section = None
    for current, title, path in chapters():
        if current != section:
            section = current
            lines += ['', f'## {"Docs" if section == "Docs" else section}', '']
        lines.append(f'- [{title}]({SITE}/{path})')
    (book / 'llms.txt').write_text('\n'.join(lines) + '\n', encoding='utf-8')


def sitemap(book):
    pages = [''] + [str(Path(path).with_suffix('.html')).replace('\\', '/')
                    for _, _, path in chapters()]
    entries = ''.join(f'  <url><loc>{escape(SITE + "/" + page)}</loc></url>\n' for page in pages)
    (book / 'sitemap.xml').write_text(
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n'
        f'{entries}</urlset>\n', encoding='utf-8')
    (book / 'robots.txt').write_text(
        f'User-agent: *\nAllow: /\n\nSitemap: {SITE}/sitemap.xml\n', encoding='utf-8')


def main(book):
    book = Path(book)
    if not (book / 'introduction.html').is_file():
        sys.exit(f'{book} is not a built book (no introduction.html)')
    landing(book)
    diagrams(book)
    docs_markdown(book)
    llms_txt(book)
    sitemap(book)
    print(f'landing page, install.sh, diagrams, Markdown pages, llms.txt and sitemap.xml written to {book}')


if __name__ == '__main__':
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
