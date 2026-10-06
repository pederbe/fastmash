"""Checks for the site builder's routing and mdBook navigation boundaries."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

import build_site


class SiteBuildTests(unittest.TestCase):
    def test_html_routes_keep_queries_fragments_and_relative_paths(self):
        cases = {
            'introduction.html': 'introduction',
            '../guide/index.html#grouping': '../guide/#grouping',
            'index.html': './',
            '/index.html': '/',
            '/print.html?search=sum#results': '/print?search=sum#results',
            'https://fastmash.io/guide/output.html#numbers':
                'https://fastmash.io/guide/output#numbers',
        }
        for before, after in cases.items():
            with self.subTest(url=before):
                self.assertEqual(build_site.site_url(before), after)

    def test_external_urls_and_downloads_are_unchanged(self):
        for url in ('https://example.org/guide.html', '//example.org/index.html',
                    'mailto:maintainer@example.org', 'guide/output.md',
                    'core-jobs.svg', 'install.sh', '#main-content'):
            with self.subTest(url=url):
                self.assertEqual(build_site.site_url(url), url)

    def test_link_rewrite_preserves_examples_and_attributes(self):
        source = ('<a href="guide/index.html?x=1&amp;y=2#keys" aria-label="Guide">Guide</a>'
                  '<iframe src="toc.html"></iframe>'
                  '<pre>&lt;a href="example.html"&gt;</pre>'
                  '<a href="https://example.org/index.html">External</a>')
        expected = source.replace('guide/index.html?', 'guide/?').replace('src="toc.html"', 'src="toc"')
        self.assertEqual(build_site.site_links(source), expected)

    def test_generated_assets_get_new_hashes_and_references(self):
        with tempfile.TemporaryDirectory() as directory:
            book = Path(directory)
            (book / 'toc-old.js').write_text(
                'const toc = \'<a href="guide/index.html">Guide</a>\';\n'
                "if (current_page.endsWith('/')) {\n"
                "  current_page += 'index.html';\n}\n")
            (book / 'searchindex-old.js').write_text(
                'window.search = {"doc_urls":["guide/operations.html#sum"],"body":"example.html"};')
            (book / 'index.html').write_text(
                '<script src="toc-old.js"></script>'
                '<script>window.path_to_searchindex_js="searchindex-old.js";</script>'
                '<a href="guide/index.html">Guide</a>')
            build_site.routes(book)
            page = (book / 'index.html').read_text()
            for prefix in ('toc', 'searchindex'):
                assets = list(book.glob(f'{prefix}-*.js'))
                self.assertEqual(len(assets), 1)
                asset = assets[0]
                digest = hashlib.sha256(asset.read_bytes()).hexdigest()[:8]
                self.assertEqual(asset.name, f'{prefix}-{digest}.js')
                self.assertIn(asset.name, page)
                self.assertNotIn('.html#', asset.read_text())
            self.assertIn('"body":"example.html"', next(book.glob('searchindex-*.js')).read_text())
            self.assertNotIn("current_page += 'index.html'", next(book.glob('toc-*.js')).read_text())
            self.assertIn('href="guide/"', page)

    def test_docs_get_native_menu_button_and_first_skip_link(self):
        with tempfile.TemporaryDirectory() as directory:
            book = Path(directory)
            page_path = book / 'guide' / 'operations.html'
            page_path.parent.mkdir()
            page_path.write_text(
                '<body><label id="mdbook-sidebar-toggle" class="icon-button" '
                'for="mdbook-sidebar-toggle-anchor" aria-controls="mdbook-sidebar">'
                '<span>Menu</span></label><main><h1>Operations</h1></main></body>')
            build_site.navigation(book)
            page = page_path.read_text()
            self.assertTrue(page.startswith('<body>\n<a class="skip-link" href="#main-content">'))
            self.assertIn('<button type="button" id="mdbook-sidebar-toggle"', page)
            self.assertNotIn(' for=', page)
            self.assertIn('<main id="main-content" tabindex="-1">', page)
            self.assertIn('<noscript><a class="sidebar-contents-link" href="../toc.html">', page)

    def test_changed_mdbook_markup_fails_visibly(self):
        with tempfile.TemporaryDirectory() as directory:
            book = Path(directory)
            (book / 'introduction.html').write_text('<body><main>Unexpected template</main></body>')
            with self.assertRaisesRegex(SystemExit, 'mdBook navigation markup changed'):
                build_site.navigation(book)

    def test_each_chapter_has_its_own_description(self):
        values = build_site.descriptions()
        self.assertEqual(len(values), len(set(values.values())))


if __name__ == '__main__':
    unittest.main()
