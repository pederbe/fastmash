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
- adds page descriptions, keyboard-accessible navigation and skip links;
- aligns HTML, menu, search, canonical and sitemap URLs with Cloudflare's
  extensionless routes, retaining the generated HTML files on disk;
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
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from urllib.parse import urlsplit, urlunsplit
from xml.sax.saxutils import escape

ROOT = Path(__file__).resolve().parents[1]
SITE = 'https://fastmash.io'
SRC = ROOT / 'docs' / 'src'
INCLUDE = re.compile(r'\{\{#include\s+([^}\s]+)\s*\}\}')
MERMAID = re.compile(r'<pre><code class="language-mermaid">(.*?)</code></pre>\s*'
                     r'(?:<!-- description: (.*?) -->)?', re.S)


def site_url(url):
    """Use Cloudflare's default HTML routes; leave other sites and assets alone."""
    parts = urlsplit(url)
    if parts.scheme and parts.scheme not in ('http', 'https'):
        return url
    if parts.netloc and parts.netloc != urlsplit(SITE).netloc:
        return url
    path = parts.path
    if path == 'index.html' or path.endswith('/index.html'):
        path = path[:-len('index.html')] or './'
    elif path.endswith('.html'):
        path = path[:-len('.html')]
    return urlunsplit(parts._replace(path=path))


def site_links(text):
    """Rewrite URL attributes, without changing code examples or external links."""
    def tag(match):
        def attribute(attr):
            url = html.unescape(attr.group(2))
            return attr.group(1) + html.escape(site_url(url), quote=True) + attr.group(3)
        return re.sub(r'((?:href|src)=["\'])(.*?)(["\'])', attribute, match.group(0))
    return re.sub(r'<(?:a|link|iframe)\b[^>]*>', tag, text)


def descriptions():
    values = json.loads((ROOT / 'docs' / 'descriptions.json').read_text(encoding='utf-8'))
    paths = {path for _, _, path in chapters()}
    if set(values) != paths or any(not isinstance(v, str) or not v.strip() for v in values.values()):
        sys.exit('docs/descriptions.json must contain one nonempty description per chapter')
    return values


def version():
    manifest = (ROOT / 'crates' / 'cli' / 'Cargo.toml').read_text(encoding='utf-8')
    return re.search(r'^version = "([^"]+)"', manifest, re.M).group(1)


def published_version():
    """Release used by public download links, independent of candidate source."""
    value = (ROOT / 'site' / 'release-version.txt').read_text(encoding='utf-8').strip()
    if not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', value):
        sys.exit('site/release-version.txt must name a published release')
    return value


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
    page = page.replace('<!-- chart -->', chart.strip()).replace('@VERSION@', published_version())
    (book / 'index.html').write_text(page, encoding='utf-8')
    for name in ('landing.css', 'landing-noscript.css', 'landing.js', 'demo.gif'):
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
    summaries = descriptions()
    for _, _, path in chapters():
        (book / path).parent.mkdir(parents=True, exist_ok=True)
        (book / path).write_text(markdown(path), encoding='utf-8')
        page_path = book / Path(path).with_suffix('.html')
        page = page_path.read_text(encoding='utf-8')
        url = site_url(f'{SITE}/{Path(path).with_suffix(".html").as_posix()}')
        summary = html.escape(summaries[path], quote=True)
        for attribute in ('name="description"', 'property="og:description"'):
            page, count = re.subn(r'<meta ' + attribute + r' content="[^"]*">',
                                  lambda _: f'<meta {attribute} content="{summary}">', page)
            if count != 1:
                sys.exit(f'{page_path}: expected one {attribute} meta tag')
        alternate = (f'<link rel="alternate" type="text/markdown" '
                     f'href="{Path(path).name}">\n'
                     f'<link rel="canonical" href="{url}">\n'
                     f'<meta property="og:url" content="{url}">\n')
        if 'type="text/markdown"' not in page:
            page = page.replace('</head>', alternate + '</head>', 1)
        page_path.write_text(page, encoding='utf-8')


def navigation(book):
    for page_path in book.rglob('*.html'):
        if page_path in (book / 'index.html', book / 'toc.html'):
            continue
        page = page_path.read_text(encoding='utf-8')
        root = os.path.relpath(book, page_path.parent).replace(os.sep, '/')

        def button(match):
            attrs = match.group(1).replace(' for="mdbook-sidebar-toggle-anchor"', '')
            return (f'<button type="button"{attrs}>{match.group(2)}</button>'
                    f'<noscript><a class="sidebar-contents-link" href="{root}/toc.html">'
                    'Contents</a></noscript>')

        page, count = re.subn(r'<label([^>]*\bid="mdbook-sidebar-toggle"[^>]*)>(.*?)</label>',
                              button, page, flags=re.S)
        if count != 1 or page.count('<main>') != 1 or page.count('<body>') != 1:
            sys.exit(f'{page_path}: mdBook navigation markup changed; update the site builder')
        page = page.replace('<body>', '<body>\n<a class="skip-link" href="#main-content">'
                            'Skip to content</a>', 1)
        page = page.replace('<main>', '<main id="main-content" tabindex="-1">', 1)
        # Long examples and tables must remain horizontally scrollable by keyboard.
        # Put focus on code itself: mdBook scrolls <code>, not its <pre> parent.
        page = re.sub(r'(<pre\b[^>]*>\s*<code\b)([^>]*)(>)',
                      lambda m: m.group(1) + m.group(2) + ' tabindex="0"' + m.group(3), page)
        page = page.replace('<div class="table-wrapper">',
                            '<div class="table-wrapper" tabindex="0">')
        page_path.write_text(page, encoding='utf-8')


def routes(book):
    # These generated scripts also contain links. Give changed contents a new
    # filename so a browser cannot reuse the old, content-hashed asset.
    renamed = {}
    for prefix in ('toc', 'searchindex'):
        assets = list(book.glob(f'{prefix}-*.js'))
        if len(assets) != 1:
            sys.exit(f'{book}: expected one generated {prefix} script')
        old = assets[0]
        text = old.read_text(encoding='utf-8')
        if prefix == 'toc':
            text = site_links(text)
            # Directory URLs already match the rewritten TOC links. mdBook's
            # index.html alias would otherwise prevent marking them active.
            text, count = re.subn(r"\s*if \(current_page\.endsWith\('/'\)\) \{\s*"
                                  r"current_page \+= 'index\.html';\s*\}", '', text)
            if count != 1:
                sys.exit(f'{old}: mdBook current-page matching changed; update the site builder')
        else:
            def search_urls(match):
                urls = json.loads(match.group(1))
                return '"doc_urls":' + json.dumps([site_url(url) for url in urls])
            text, count = re.subn(r'"doc_urls":(\[[^\]]*\])', search_urls, text)
            if count != 1:
                sys.exit(f'{old}: mdBook search index changed; update the site builder')
        new = book / f'{prefix}-{hashlib.sha256(text.encode()).hexdigest()[:8]}.js'
        if old != new:
            new.write_bytes(text.encode('utf-8'))
            old.unlink()
            renamed[old.name] = new.name
    for page_path in book.rglob('*.html'):
        page = site_links(page_path.read_text(encoding='utf-8'))
        for old, new in renamed.items():
            page = page.replace(old, new)
        page_path.write_text(page, encoding='utf-8')


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
        '> Fast command-line statistics and table workflows for text and quoted CSV.',
        '> GNU datamash commands, portable results, weighted means, complete-record',
        '> selection, table health and dataset comparison.',
        '',
        f'Published version {published_version()}: Linux x86-64 and WSL2. Install:',
        '`curl -fsSL https://fastmash.io/install.sh | sh`, or `cargo install --locked fastmash`.',
        f'Source candidate {version()} adds Apple Silicon macOS 15 or later; see the installation guide.',
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
    pages = [''] + [site_url(Path(path).with_suffix('.html').as_posix())
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
    navigation(book)
    routes(book)
    llms_txt(book)
    sitemap(book)
    print(f'landing page, install.sh, diagrams, Markdown pages, llms.txt and sitemap.xml written to {book}')


if __name__ == '__main__':
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
