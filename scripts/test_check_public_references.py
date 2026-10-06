"""Regression coverage for the public source's private-reference check."""
from pathlib import Path
import tempfile
import unittest

from check_public_references import scan


class PublicScanTests(unittest.TestCase):
    def scan_source(self, text, name='fixture.rs'):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / name).write_text(text, encoding='utf-8')
            return scan(root, [name])

    def test_numeric_delimiter_after_format_placeholder(self):
        source = 'let long_run_on = format!("{}\x235", "1".repeat(600));'
        self.assertEqual(self.scan_source(source), [])

    def test_issue_reference_after_numeric_delimiter(self):
        source = 'let long_run_on = format!("{}\x235", "1".repeat(600)); // See \x23150.'
        hits = self.scan_source(source)
        self.assertEqual(len(hits), 1)
        self.assertIn('private issue number', hits[0])

    def test_issue_references_in_prose(self):
        for source in ('// See \x235.', '// Regression (\x23150).', '"Tracked in \x23150"'):
            with self.subTest(source=source):
                self.assertEqual(len(self.scan_source(source)), 1)

    def test_issue_reference_in_tabular_fixture_comment(self):
        source = '\x23 Directed logarithm challenges from \x2374.\n0001\t0002\n'
        hits = self.scan_source(source, 'log-mpfr.tsv')
        self.assertEqual(len(hits), 1)
        self.assertIn('private issue number', hits[0])


if __name__ == '__main__':
    unittest.main()
