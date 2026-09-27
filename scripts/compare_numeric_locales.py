#!/usr/bin/env python3
"""Compare fastmash's number reading and printing with GNU datamash per locale.

usage: compare_numeric_locales.py FASTMASH DATAMASH LOCPATH TABLE_JSON [--all]

For one representative locale per distinct (decimal point, thousands
separator, grouping) rule in TABLE_JSON (glibc's LC_NUMERIC values from
scripts/generate_numeric_locales.py), or every locale with --all, both programs
read values written with the locale's decimal point and print each of them
through `-g 2 sum 1` (one group per value, so each value is formatted on its
own) with default output and a set of --format directives, including the
grouping flag with e, f, g and a conversions, widths, zero and space padding,
left alignment, signs and the L modifier. Aggregates and a parameter in the
locale's decimal notation are compared too. Locales whose default character
set is UTF-8 are named without a codeset, the others with `.UTF-8`. GNU
datamash runs with LOCPATH so glibc supplies the reference; stdout, stderr
(program name normalized) and status must match byte for byte.
"""
import json
import re
import subprocess
import sys

FORMATS = ["%'.2f", "%'14.2f", "%'014.2f", "%-'16.3f", "%+'.0f", "%' 025.3f", "%'Lf", "%'g",
           "%'.10g", "%'.20g", "%'e", "%'a", "%.3f", "%g", "%'20.4f", "%'.0f"]
VALUES = ['0', '1', '-1', '12', '123', '1234', '12345', '123456', '1234567', '12345678',
          '123456789', '1234567890', '98765432109876', '-4321987.5', '0.125', '1e20', '-2.5e-7',
          '999999.995', '1000000', '-0.5', '123456789012345678901234567890']


def run(binary, name, args, stdin, locale, locpath):
    env = {'PATH': '/usr/bin:/bin', 'LC_ALL': locale, 'LOCPATH': locpath, 'TZ': 'UTC'}
    done = subprocess.run([name, *args], executable=binary, input=stdin, env=env,
                          capture_output=True, timeout=30)
    return done.returncode, done.stdout, re.sub(rb'\b(datamash|fastmash)\b', b'PROG', done.stderr)


def main(fastmash, datamash, locpath, table_json, every=False):
    data = json.load(open(table_json, encoding='utf-8'))
    table, bare = data['locales'], set(data.get('bare_utf8', []))
    chosen, seen = [], set()
    for name, rules in sorted(table.items()):
        if len(rules['decimal_point'].encode()) != 1:
            continue
        key = (rules['decimal_point'], rules['thousands_sep'], tuple(rules['grouping']))
        if every or key not in seen:
            seen.add(key)
            chosen.append((name, rules))
    cases = failures = 0

    def compare(locale, args, stdin):
        nonlocal cases, failures
        fast = run(fastmash, 'fastmash', args, stdin, locale, locpath)
        gnu = run(datamash, 'datamash', args, stdin, locale, locpath)
        cases += 1
        if fast != gnu:
            failures += 1
            if failures <= 25:
                print(f'MISMATCH {locale} {args}\n  fastmash {fast}\n  datamash {gnu}')

    for name, rules in chosen:
        locale = name if name in bare else name + '.UTF-8'
        point = rules['decimal_point']
        values = [v.replace('.', point) for v in VALUES]
        each = ''.join(f'{v}\t{i}\n' for i, v in enumerate(values)).encode()
        for fmt in [None, *FORMATS]:
            compare(locale, (['--format', fmt] if fmt else []) + ['-g', '2', 'sum', '1'], each)
        column = ''.join(v + '\n' for v in values[:-1]).encode()
        for args in (['sum', '1', 'mean', '1'], [f'trimmean:0{point}2', '1'], ['perc:90', '1'],
                     ['--format', "%'.3f", 'sum', '1']):
            compare(locale, args, column)
    print(f'{len(chosen)} locales, {cases} cases, {failures} mismatches')
    return 1 if failures else 0


if __name__ == '__main__':
    if len(sys.argv) < 5:
        sys.exit(__doc__)
    sys.exit(main(*sys.argv[1:5], every='--all' in sys.argv))
