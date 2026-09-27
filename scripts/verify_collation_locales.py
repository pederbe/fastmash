#!/usr/bin/env python3
"""Choose the locales whose sorting fastmash reproduces, by comparison with GNU.

usage:
  verify_collation_locales.py candidates GNU_SORT LOCPATH
  verify_collation_locales.py table [--variant N]
  verify_collation_locales.py verify N FASTMASH DATAMASH GNU_SORT LOCPATH [LOCALE...]

`candidates` maps every locale of data/locales/glibc-lc-numeric.json to two
candidate Unicode (BCP 47) collation locales, in order of preference: the same
language and territory, whose CLDR tailoring may differ from glibc's (CLDR
sorts a language's own script first; glibc keeps Latin first), and the root
collation `und`, which matches glibc's untailored table. Both get
`-u-kf-upper` where glibc sorts uppercase first (probed with GNU_SORT). It
also records each locale's alphabet: the standard and
auxiliary CLDR exemplar letters of the host's ICU (libicu, same CLDR
generation as the collator data; auxiliary letters appear in loanwords and
names, such as ć and đ in Slovenian), or none when ICU has no data for the
language. Output:
data/locales/collation-candidates.json.

`table` writes crates/cli/src/collation_locales.rs with each locale's first
verified candidate from data/locales/collation-verification.json, or with
--variant N every locale's Nth candidate (a measurement build to verify that
variant; never commit it).

`verify N` checks candidate N with a measurement build of it: it runs both
programs with `-s -g 1 count 1` over word lists and
compares the order of the output groups. GNU datamash sorts through GNU_SORT
(`--sort-cmd`), so glibc's collation from LOCPATH is the reference. Words are
letters and digits only: glibc and the shifted Unicode collator both ignore
punctuation and spaces at the first levels, and their order is a documented
difference. A locale is verified when:

- its alphabet list orders exactly as GNU's: every standard and auxiliary
  exemplar letter and ASCII letter in both cases, alone, before and after a
  filler letter, and (for standard alphabets up to 120 letters) every ordered
  pair of standard and ASCII letters, lower and capitalized, which covers
  contractions such as ch, cs, ll, ny and dz. For languages written with Han
  characters (by their exemplars, or cmn, hak, lzh and nan, which ICU has no
  data for) the list adds samples of CJK Extension A, Extension B and the
  compatibility ideographs;
- its digits list (digits alone, in pairs and after a letter) orders exactly;
- and, for a language ICU has no data for, no script block (all Latin-1,
  Latin Extended-A and national letters, and the other scripts) displaces a
  word that en_US's does not. Displaced words are those outside a longest
  common subsequence of the two orders; en_US's (Arabic, Georgian capitals,
  Hangul syllables, letters newer than glibc's table) are shipped and
  documented.

Output: data/locales/collation-verification.json, which keeps the results of
each variant.
"""
from bisect import bisect_left
from concurrent.futures import ThreadPoolExecutor
import ctypes
import json
from pathlib import Path
import subprocess
import sys
import unicodedata

CANDIDATES = Path('data/locales/collation-candidates.json')
VERIFICATION = Path('data/locales/collation-verification.json')
TABLE = Path('crates/cli/src/collation_locales.rs')
ICU = '78'
ASCII = 'abcdefghijklmnopqrstuvwxyz'

BLOCKS = {
    'latin': [(0x41, 0x5a), (0x61, 0x7a), (0xc0, 0x17f), (0x218, 0x21b), (0x18f, 0x18f),
              (0x259, 0x259)],
    'vietnamese': [(0x1a0, 0x1a1), (0x1af, 0x1b0), (0x1ea0, 0x1ef9)],
    'greek': [(0x386, 0x3ce)],
    'cyrillic': [(0x400, 0x4ff)],
    'armenian': [(0x531, 0x586)],
    'hebrew': [(0x5d0, 0x5ea)],
    'arabic': [(0x620, 0x64a), (0x671, 0x6d3)],
    'devanagari': [(0x904, 0x939), (0x958, 0x961)],
    'bengali': [(0x985, 0x9b9)],
    'gurmukhi': [(0xa05, 0xa39)],
    'gujarati': [(0xa85, 0xab9)],
    'oriya': [(0xb05, 0xb39)],
    'tamil': [(0xb85, 0xbb9)],
    'telugu': [(0xc05, 0xc39)],
    'kannada': [(0xc85, 0xcb9)],
    'malayalam': [(0xd05, 0xd3a)],
    'sinhala': [(0xd85, 0xdc6)],
    'thai': [(0xe01, 0xe2e)],
    'lao': [(0xe81, 0xeae)],
    'tibetan': [(0xf40, 0xf6c)],
    'myanmar': [(0x1000, 0x102a)],
    'georgian': [(0x10d0, 0x10fa)],
    'ethiopic': [(0x1200, 0x1248)],
    'khmer': [(0x1780, 0x17b3)],
    'thaana': [(0x780, 0x7a5)],
    'hiragana': [(0x3041, 0x3096)],
    'katakana': [(0x30a1, 0x30fa)],
    'hangul': [(0xac00, 0xac00 + 588 * 4)],
    'han': [(0x4e00, 0x4e00 + 600)],
}


def run(argv, stdin, locale, locpath, name=None):
    env = {'PATH': '/usr/bin:/bin', 'LC_ALL': locale, 'LOCPATH': locpath, 'TZ': 'UTC'}
    return subprocess.run([name or argv[0], *argv[1:]], executable=argv[0], input=stdin, env=env,
                          capture_output=True, timeout=300)


def icu_function(library, name, restype=ctypes.c_int):
    function = getattr(ctypes.CDLL(f'lib{library}.so.{ICU}'), f'{name}_{ICU}')
    function.restype = restype
    return function


def icu_alphabet(name, kind):
    """Exemplar letters (kind 0 standard, 1 auxiliary), or None without ICU data."""
    pointer = ctypes.c_void_p
    status = ctypes.c_int(0)
    bundle = icu_function('icuuc', 'ures_open', pointer)(None, name.encode(), ctypes.byref(status))
    actual = icu_function('icuuc', 'ures_getLocaleByType', ctypes.c_char_p)(
        pointer(bundle), 1, ctypes.byref(status))
    icu_function('icuuc', 'ures_close')(pointer(bundle))
    if actual.decode().split('_')[0] != name.split('_')[0]:
        return None
    status = ctypes.c_int(0)
    data = icu_function('icui18n', 'ulocdata_open', pointer)(name.encode(), ctypes.byref(status))
    uset = icu_function('icui18n', 'ulocdata_getExemplarSet', pointer)(
        pointer(data), None, 0, kind, ctypes.byref(status))
    letters, buffer = [], (ctypes.c_uint16 * 64)()
    for index in range(icu_function('icuuc', 'uset_getItemCount')(pointer(uset))):
        start, end, error = ctypes.c_int(), ctypes.c_int(), ctypes.c_int(0)
        length = icu_function('icuuc', 'uset_getItem')(
            pointer(uset), index, ctypes.byref(start), ctypes.byref(end), buffer, 64,
            ctypes.byref(error))
        if length == 0:
            letters += [chr(point) for point in range(start.value, end.value + 1)]
        else:
            letters.append(bytes(buffer)[:2 * length].decode('utf-16-le'))
    icu_function('icuuc', 'uset_close')(pointer(uset))
    icu_function('icui18n', 'ulocdata_close')(pointer(data))
    return letters


def candidates(gnu_sort, locpath):
    table = json.load(open('data/locales/glibc-lc-numeric.json', encoding='utf-8'))
    out = {'icu': ICU, 'glibc': table['glibc'], 'locales': {}}
    for name in sorted(table['locales']):
        probe = run([gnu_sort], b'a\nA\n', name + '.UTF-8', locpath).stdout
        upper = probe == b'A\na\n'
        suffix = '-u-kf-upper' if upper else ''
        out['locales'][name] = {'tags': [name.replace('_', '-') + suffix, 'und' + suffix],
                                'alphabet': icu_alphabet(name, 0),
                                'auxiliary': icu_alphabet(name, 1)}
    CANDIDATES.write_text(json.dumps(out, indent=1, ensure_ascii=False) + '\n', encoding='utf-8')
    print(f'{len(out["locales"])} candidates, '
          f'{sum(1 for v in out["locales"].values() if v["tags"][0].endswith("upper"))} uppercase first, '
          f'{sum(1 for v in out["locales"].values() if v["alphabet"] is None)} without ICU data')


def write_table(variant):
    entries = json.load(open(CANDIDATES, encoding='utf-8'))['locales']
    if variant is None:
        verified = json.load(open(VERIFICATION, encoding='utf-8'))['locales']
        chosen = {name: entry['tag'] for name, entry in verified.items() if entry['tag']}
    else:
        chosen = {name: entry['tags'][variant] for name, entry in entries.items()}
    rows = '\n'.join(f'    (b"{name}", "{tag}"),' for name, tag in sorted(chosen.items()))
    header = (f'//! MEASUREMENT BUILD of candidate {variant}; do not commit.\n'
              if variant is not None else
              '//! Generated by scripts/verify_collation_locales.py from\n'
              '//! data/locales/collation-verification.json; do not edit. glibc locales whose\n'
              '//! sorting the Unicode collator reproduces, with that collator\'s locale.\n')
    TABLE.write_text(header + '/// Sorted by glibc name for binary search.\n'
                     'pub(super) static COLLATION: &[(&[u8], &str)] = &[\n' + rows + '\n];\n',
                     encoding='utf-8')
    print(f'{len(chosen)} locales in {TABLE}')


def word_char(ch):
    return len(ch) == 1 and unicodedata.category(ch)[0] in 'LN'


def with_filler(letters, filler):
    out = set()
    for letter in letters:
        for variant in {letter, letter.lower(), letter.upper(), letter.capitalize()}:
            if all(word_char(ch) for ch in variant):
                out.update([variant, variant + filler, filler + variant])
    return out


HAN_EXTENSIONS = [(0x3400, 0x3400 + 100), (0x20000, 0x20000 + 100), (0xf900, 0xf900 + 100)]
# Chinese languages that ICU has no exemplar data for.
HAN_LANGUAGES = {'cmn', 'hak', 'lzh', 'nan'}


def alphabet_words(alphabet, auxiliary, language):
    letters = sorted(set(alphabet or []) | set(ASCII))
    extra = set(auxiliary or [])
    if language in HAN_LANGUAGES or any(
            len(ch) == 1 and unicodedata.name(ch, '').startswith('CJK UNIFIED') for ch in letters):
        extra.update(chr(p) for low, high in HAN_EXTENSIONS for p in range(low, high + 1)
                     if word_char(chr(p)))
    words = with_filler(sorted(set(letters) | extra), 'a')
    if len(letters) <= 120:
        for x in letters:
            for y in letters:
                words.update([x + y, (x + y).capitalize()])
    return sorted(words)


def digit_words():
    digits = '0123456789'
    return sorted({*digits, *(x + y for x in digits for y in digits),
                   *('a' + d for d in digits), *(d + 'a' for d in digits), 'a', 'b', 'z'})


def block_words(block):
    chars = [chr(p) for low, high in BLOCKS[block] for p in range(low, high + 1) if word_char(chr(p))]
    words = with_filler(chars, chars[0])
    if block == 'latin':
        words.update(x + y for x in ASCII for y in ASCII)
    return sorted(words)


def displaced(fast, gnu):
    """Words outside a longest common subsequence of two orders of one set."""
    rank = {word: i for i, word in enumerate(gnu)}
    tails, tail_at, parent = [], [], {}
    for word in fast:
        r = rank[word]
        at = bisect_left(tails, r)
        parent[word] = tail_at[at - 1] if at else None
        if at == len(tails):
            tails.append(r)
            tail_at.append(word)
        else:
            tails[at] = r
            tail_at[at] = word
    keep, word = set(), tail_at[-1] if tail_at else None
    while word is not None:
        keep.add(word)
        word = parent[word]
    return {word for word in fast if word not in keep}


def orders(fastmash, datamash, gnu_sort, locpath, locale, words):
    stdin = ''.join(w + '\n' for w in words).encode()
    fast = run([fastmash, '-s', '-g', '1', 'count', '1'], stdin, locale, locpath, 'fastmash')
    gnu = run([datamash, '--sort-cmd', gnu_sort, '-s', '-g', '1', 'count', '1'], stdin, locale,
              locpath, 'datamash')
    if fast.returncode or gnu.returncode:
        return None, [fast.stderr.decode(errors='replace'), gnu.stderr.decode(errors='replace')]
    parse = lambda out: [line.split(b'\t')[0].decode() for line in out.splitlines()]
    return parse(fast.stdout), parse(gnu.stdout)


def verify_one(tools, name, entry, blocks):
    locale = name + '.UTF-8'
    result = {}
    alphabet = alphabet_words(entry['alphabet'], entry['auxiliary'], name.split('_')[0])
    for label, words in (('alphabet', alphabet), ('digits', digit_words())):
        fast, gnu = orders(*tools, locale, words)
        if fast is None:
            result[label] = {'error': gnu}
        elif fast != gnu:
            words = sorted(displaced(fast, gnu))
            result[label] = {'displaced': len(words), 'first': words[:40]}
    shifted = {}
    for block, words in blocks.items():
        fast, gnu = orders(*tools, locale, words)
        shifted[block] = None if fast is None else displaced(fast, gnu)
    return name, result, shifted


def verify(variant, fastmash, datamash, gnu_sort, locpath, *chosen):
    variant = int(variant)
    candidates = json.load(open(CANDIDATES, encoding='utf-8'))
    entries = candidates['locales']
    names = list(chosen) or sorted(entries)
    if 'en_US' not in names:
        names.append('en_US')
    blocks = {block: block_words(block) for block in BLOCKS}
    tools = (fastmash, datamash, gnu_sort, locpath)
    with ThreadPoolExecutor(8) as pool:
        raw = list(pool.map(lambda n: verify_one(tools, n, entries[n], blocks), names))
    baseline = next(shifted for name, _, shifted in raw if name == 'en_US')
    report = {}
    for name, result, shifted in sorted(raw):
        extra = {block: len(words - baseline[block]) if words is not None else 'error'
                 for block, words in shifted.items()
                 if words is None or words - baseline[block]}
        if entries[name]['alphabet'] is None:
            result.update({f'block:{block}': v for block, v in extra.items()})
        report[name] = {'verified': not result, 'tag': entries[name]['tags'][variant],
                        'failures': result, 'other_scripts': extra}
        failures = ' '.join(f'{k}:{v["displaced"] if isinstance(v, dict) and "displaced" in v else v}'
                            for k, v in result.items())
        print(f'{name}: {"verified" if not result else "differs " + failures}')
    output = (json.load(open(VERIFICATION, encoding='utf-8')) if VERIFICATION.exists() else
              {'variants': {}})
    output.update(icu=candidates['icu'], glibc=candidates['glibc'])
    variant_results = output['variants'].setdefault(str(variant), {'locales': {}})
    variant_results['baseline_displaced'] = {b: sorted(v) for b, v in baseline.items()}
    variant_results['locales'].update(report)
    # Each locale's first verified candidate.
    output['locales'] = {}
    for name, entry in sorted(entries.items()):
        results = [output['variants'].get(str(n), {}).get('locales', {}).get(name)
                   for n in range(len(entry['tags']))]
        chosen = next((r['tag'] for r in results if r and r['verified']), None)
        output['locales'][name] = {'tag': chosen, 'alphabet_data': entry['alphabet'] is not None}
    VERIFICATION.write_text(json.dumps(output, indent=1, ensure_ascii=False) + '\n',
                            encoding='utf-8')
    print(f'{sum(v["verified"] for v in report.values())} of {len(report)} locales verified '
          f'with candidate {variant}; {sum(1 for v in output["locales"].values() if v["tag"])} '
          f'locales verified overall')


if __name__ == '__main__':
    command, args = (sys.argv[1], sys.argv[2:]) if len(sys.argv) > 1 else (None, [])
    if command == 'candidates' and len(args) == 2:
        candidates(*args)
    elif command == 'table':
        write_table(int(args[args.index('--variant') + 1]) if '--variant' in args else None)
    elif command == 'verify' and len(args) >= 5:
        verify(*args)
    else:
        sys.exit(__doc__)
