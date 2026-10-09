"""Regression checks for locale queries and the generated Rust/JSON tables."""
from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import re
import tempfile
import unittest
from unittest import mock

import generate_numeric_locales as generator


LITERAL = r'b"(?:\\x[0-9a-f]{2})*"'
NUMERIC_ROW = re.compile(
    rf'    \(({LITERAL}), Profile::new\(({LITERAL})\[0\], '
    rf'({LITERAL}), ({LITERAL})\)\),')
NAME_ROW = re.compile(rf'    ({LITERAL}),')


def decode_literal(literal):
    return bytes.fromhex(literal[2:-1].replace('\\x', ''))


def decoded_tables(source):
    """Accept only escaped byte data in the emitted table entries."""
    tables = {}
    for name in ('NUMERIC', 'BARE_UTF8', 'REFUSED'):
        body = source.split(f'pub(super) static {name}:', 1)[1]
        body = body.split('= &[\n', 1)[1].split('\n];', 1)[0]
        rows = []
        for line in body.splitlines():
            pattern = NUMERIC_ROW if name == 'NUMERIC' else NAME_ROW
            match = pattern.fullmatch(line)
            if match is None:
                raise AssertionError(f'{name} contains source syntax instead of byte data: {line!r}')
            values = tuple(decode_literal(literal) for literal in match.groups())
            rows.append(values if name == 'NUMERIC' else values[0])
        tables[name] = rows
    return tables


class NumericLocaleGenerationTests(unittest.TestCase):
    def generate(self, profiles):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            locales = root / 'locales'
            locales.mkdir()
            for name in profiles:
                (locales / name).mkdir()

            def query(command, **options):
                if command == ['ldd', '--version']:
                    return 'fixture glibc 2.43\n'
                if command != ['locale', '-k', 'LC_NUMERIC']:
                    raise AssertionError(f'unexpected query: {command!r}')
                decimal, separator, grouping = profiles[options['env']['LC_ALL']]
                return (f'decimal_point={json.dumps(decimal)}\n'
                        f'thousands_sep={json.dumps(separator)}\n'
                        f'grouping={";".join(str(size) for size in grouping)}\n')

            out_json, out_rs = root / 'table.json', root / 'table.rs'
            with mock.patch.object(generator.subprocess, 'check_output', side_effect=query), \
                    mock.patch.object(generator.platform, 'machine', return_value='fixture-host'), \
                    redirect_stdout(io.StringIO()):
                generator.main(locales, out_json, out_rs)
            return json.loads(out_json.read_text(encoding='utf-8')), out_rs.read_text(encoding='utf-8')

    def test_untrusted_names_remain_byte_data_in_all_three_tables(self):
        names = [
            ('quote"name', b'quote"name'),
            ('back\\slash', b'back\\slash'),
            ('carriage\rreturn', b'carriage\rreturn'),
            ('line\nbreak', b'line\nbreak'),
            ('fr_é', b'fr_\xc3\xa9'),
            ('bad");compile_error!("injected', b'bad");compile_error!("injected'),
        ]
        for name, expected in names:
            with self.subTest(name=name):
                # A failed locale selection can still return these successful
                # default values. Its locale name must still be emitted as byte data.
                raw, source = self.generate({
                    name: ('.', '', []),
                    name + '@refused': ('٫', ',', [3, 0]),
                })
                tables = decoded_tables(source)
                self.assertEqual(tables['NUMERIC'], [(expected, b'.', b'', b'')])
                self.assertEqual(tables['BARE_UTF8'], [expected])
                self.assertEqual(tables['REFUSED'], [expected + b'@refused'])
                self.assertEqual(raw['locales'][name]['decimal_point'], '.')
                self.assertEqual(raw['bare_utf8'], [name, name + '@refused'])

    def test_existing_profiles_keep_decoded_values_ordering_and_json_meaning(self):
        raw, source = self.generate({
            'sr_RS@latin': (',', '.', [3, -1]),
            'nan_TW.UTF-8@latin': ('.', '\u00a0', [3, 0]),
            'de_DE.UTF-8': (',', '.', [3, 3, 0]),
            'fa_IR.UTF-8': ('٫', ',', [3, 0]),
            'aa_ER': ('.', '', []),
            'C.UTF-8': ('.', '', []),
        })
        self.assertEqual(decoded_tables(source), {
            'NUMERIC': [
                (b'aa_ER', b'.', b'', b''),
                (b'de_DE', b',', b'.', b'\x03\x03\x00'),
                (b'nan_TW@latin', b'.', b'\xc2\xa0', b'\x03\x00'),
                (b'sr_RS@latin', b',', b'.', b'\x03\x7f'),
            ],
            'BARE_UTF8': [b'aa_ER', b'sr_RS@latin'],
            'REFUSED': [b'fa_IR'],
        })
        self.assertEqual(raw, {
            'glibc': 'fixture glibc 2.43',
            'machine': 'fixture-host',
            'bare_utf8': ['aa_ER', 'sr_RS@latin'],
            'locales': {
                'aa_ER': {'decimal_point': '.', 'thousands_sep': '', 'grouping': []},
                'de_DE': {'decimal_point': ',', 'thousands_sep': '.', 'grouping': [3, 3, 0]},
                'fa_IR': {'decimal_point': '٫', 'thousands_sep': ',', 'grouping': [3, 0]},
                'nan_TW@latin': {'decimal_point': '.', 'thousands_sep': '\u00a0', 'grouping': [3, 0]},
                'sr_RS@latin': {'decimal_point': ',', 'thousands_sep': '.', 'grouping': [3, -1]},
            },
        })
        self.assertEqual(list(raw['locales']),
                         ['aa_ER', 'de_DE', 'fa_IR', 'nan_TW@latin', 'sr_RS@latin'])


if __name__ == '__main__':
    unittest.main()
