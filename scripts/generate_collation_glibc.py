#!/usr/bin/env python3
"""Generate glibc's treatment of the characters it collates apart from letters.

usage: generate_collation_glibc.py LOCPATH [COMMON]

GNU sort in a glibc locale ignores whitespace, punctuation and most symbols at
the first three levels and compares them only at the fourth, by their order in
glibc's iso14651_t1_common (COMMON, default
/usr/share/i18n/locales/iso14651_t1_common), where letters and digits come
after them all. Some letters have no fourth-level weight: the Han block of
iso14651_t1, and letters a locale tailors (such as Croatian c-caron). Some
locales give an ignorable character the lowest first-level weight instead
(the space in the Spanish locales, gl_ES and pl_PL). Fastmash's Unicode
collation differs in all of these, so it adjusts its keys by this data.

The ignorable characters and their fourth-level order are read from COMMON.
Everything per locale is observed, not parsed: each of the sorting locales of
crates/cli/src/collation_locales.rs is loaded from LOCPATH (glibc's compiled
locales, as for scripts/verify_collation_locales.py) and probed with
strcoll_l. An ignorable character c keeps "aa" < "acb" < "ac" at the first
level; one that does not but sorts "acb" before "a0" is weighed lowest, and
at the fourth level must rank below every ignorable (it does where es, gl and
pl reorder the space; the generator fails otherwise); a letter or digit L has
no fourth-level weight when "aL" sorts before "a-L".
Every letter and digit of the Basic Multilingual Plane is probed. The
characters with a canonical decomposition, and its length, come from Python's
unicodedata.

COMMON also spells some letters in parts as collating elements with the
letter's weights (its "decomposition of" entries, such as CYRILLIC SMALL
LETTER I and a combining breve for SHORT I), which glibc ties with the letter
at every level, where Unicode's ties may differ or not apply (L and a middle
dot for L WITH MIDDLE DOT is only a compatibility decomposition). Each is read
from COMMON with the letter its comment names, and must tie with it, alone and
between other letters, in every sorting locale (the generator fails
otherwise).

Where the collator ties a letter with its canonical decomposition, glibc
orders the two by its fourth level, its table's order of their characters;
Fastmash orders them by code point. The letters glibc places after their
decomposition in every context probed, though they are coded below its first
character (such as CYRILLIC CAPITAL LETTER IO), are listed; they must be the
same in every locale. Where
glibc places a letter before its decomposition, it does so only in some
contexts (its second level drops a mark before an ignorable that ends a run),
so code point order is kept for those. A letter glibc ties with its
decomposition between other letters must be one of the contractions above.

Output: crates/cli/src/collation_glibc.rs, and the observations in
data/locales/collation-glibc.json.
"""
import ctypes
import functools
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import unicodedata

ROOT = Path(__file__).resolve().parents[1]
TABLE = ROOT / 'crates/cli/src/collation_locales.rs'
OUTPUT = ROOT / 'crates/cli/src/collation_glibc.rs'
DATA = ROOT / 'data/locales/collation-glibc.json'
LC_COLLATE_MASK = 1 << 3
# Where a letter is probed against its decomposition: alone, between letters,
# and before a space, punctuation, a digit or the end.
CONTEXTS = [('', ''), ('a', 'b'), ('', 'b'), ('', ' b'), ('', '-b'), ('', '.b'),
            ('', '1'), ('', ','), ('', '  b'), ('a', '')]


def ignorables(common):
    """The characters COMMON ignores at the first three levels, in its order."""
    order = []
    entry = re.compile(r'^<U([0-9A-F]{4,8})> IGNORE;IGNORE;IGNORE;<U\1>')
    for line in common.read_text(encoding='utf-8').splitlines():
        match = entry.match(line)
        if match:
            order.append(int(match.group(1), 16))
    if len(order) != len(set(order)):
        sys.exit('an ignorable character is listed twice')
    return order


def contractions(common):
    """COMMON's sequences that spell a letter in parts, and that letter."""
    entry = re.compile(r'^collating-element <[^>]+> from "((?:<U[0-9A-F]+>)+)"'
                       r' % decomposition of (.+)$', re.M)
    out = sorted((''.join(chr(int(u, 16)) for u in re.findall(r'<U([0-9A-F]+)>', parts)),
                  unicodedata.lookup(name.strip()))
                 for parts, name in entry.findall(common.read_text(encoding='utf-8')))
    if not out or len(out) != len({parts for parts, _ in out}):
        sys.exit('no contractions, or one listed twice')
    return out


def compose(text, contractions):
    """`text` with each contraction replaced by its letter, the longest at each
    position (as `locale::compose` does)."""
    letters = dict(contractions)
    out, at = [], 0
    while at < len(text):
        found = max((parts for parts in letters if text.startswith(parts, at)),
                    key=len, default=None)
        if found is None:
            out.append(text[at])
            at += 1
        else:
            out.append(letters[found])
            at += len(found)
    return ''.join(out)


def decomposable():
    """The characters whose canonical decomposition is more than one character,
    apart from the Hangul syllables."""
    return [c for a, b, _ in decomposed() for c in range(a, b + 1)]


def sorting_locales():
    """The sorting locales' glibc names and Unicode collation tags."""
    return re.findall(r'\(b"([^"]+)", "([^"]+)"\)', TABLE.read_text(encoding='utf-8'))


def letters():
    """The Basic Multilingual Plane's assigned letters and digits."""
    return [c for c in range(0x80, 0x10000)
            if unicodedata.category(chr(c))[0] in 'LN' and not 0xD800 <= c < 0xE000]


class Glibc:
    def __init__(self, locpath):
        os.environ['LOCPATH'] = locpath
        self.libc = ctypes.CDLL('libc.so.6')
        self.libc.newlocale.restype = ctypes.c_void_p
        self.libc.newlocale.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_void_p]
        self.libc.strcoll_l.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_void_p]
        self.libc.freelocale.argtypes = [ctypes.c_void_p]

    def probe(self, name, ignorable, candidates, contractions, decomposable):
        handle = self.libc.newlocale(LC_COLLATE_MASK, f'{name}.UTF-8'.encode(), None)
        if not handle:
            sys.exit(f'{name}.UTF-8 is not in LOCPATH')
        compare = self.libc.strcoll_l
        for parts, letter in contractions:
            for a, b in ((parts, letter), (f'a{parts}b', f'a{letter}b')):
                if compare(a.encode(), b.encode(), handle) != 0:
                    sys.exit(f'{name}: {a!r} does not tie with {b!r}')
        reordered = {}
        for c in decomposable:
            parts = unicodedata.normalize('NFD', chr(c))
            if compare(f'a{chr(c)}b'.encode(), f'a{parts}b'.encode(), handle) == 0:
                if compose(parts, contractions) != chr(c):
                    sys.exit(f'{name}: U+{c:04X} ties with its decomposition')
                continue
            if c > ord(parts[0]):
                continue
            orders = {compare(before.encode() + chr(c).encode() + after.encode(),
                              before.encode() + parts.encode() + after.encode(), handle) > 0
                      for before, after in CONTEXTS}
            if orders == {True}:
                reordered[c] = ord(parts[0])
        weighed, other, no_fourth = [], [], []
        for c in ignorable:
            if c == 0:
                continue
            s = f'a{chr(c)}b'.encode()
            if compare(s, b'aa', handle) > 0 and compare(s, b'ac', handle) < 0:
                continue
            (weighed if compare(s, b'a0', handle) < 0 else other).append(c)
        # At the fourth level, where "aFIb" and "aIFb" tie before it, a
        # character F weighed first ranks below each ignorable I (es, gl and
        # pl move the space into the symbols); order them among themselves.
        rest = [c for c in ignorable if c != 0 and c not in weighed]
        below = lambda f, i: compare(f'a{chr(f)}{chr(i)}b'.encode(),
                                     f'a{chr(i)}{chr(f)}b'.encode(), handle) < 0
        for f in weighed:
            above = [i for i in rest if not below(f, i)]
            if above:
                sys.exit(f'{name}: U+{f:04X} ranks above U+{above[0]:04X} at the fourth level')
        weighed.sort(key=functools.cmp_to_key(lambda f, g: -1 if below(f, g) else 1))
        for c in candidates:
            letter = chr(c).encode()
            if compare(b'a' + letter, b'a-' + letter, handle) < 0:
                no_fourth.append(c)
        self.libc.freelocale(handle)
        return weighed, other, no_fourth, reordered


def ranges(points):
    out = []
    for point in sorted(points):
        if out and out[-1][1] + 1 == point:
            out[-1][1] = point
        else:
            out.append([point, point])
    return out


def decomposed():
    """Ranges (first, last, length) of the characters whose canonical
    decomposition is `length` characters, where that is more than one, apart
    from the Hangul syllables, which have no fourth-level weight."""
    out = []
    for point in range(0x80, 0x110000):
        if 0xD800 <= point < 0xE000 or 0xAC00 <= point <= 0xD7A3:
            continue
        length = len(unicodedata.normalize('NFD', chr(point)))
        if length < 2:
            continue
        if out and out[-1][1] + 1 == point and out[-1][2] == length:
            out[-1][1] = point
        else:
            out.append([point, point, length])
    return out


def main():
    if len(sys.argv) not in (2, 3):
        sys.exit(__doc__)
    common = Path(sys.argv[2] if len(sys.argv) == 3 else
                  '/usr/share/i18n/locales/iso14651_t1_common')
    order = ignorables(common)
    rank = {c: at for at, c in enumerate(order)}
    glibc = Glibc(sys.argv[1])
    candidates = letters()
    composed = contractions(common)
    letters_in_parts = decomposable()
    observed, tags = {}, {}
    for name, tag in sorting_locales():
        weighed, other, no_fourth, reordered = glibc.probe(name, order, candidates, composed,
                                                          letters_in_parts)
        observed[name] = {'weighed': weighed, 'other_weighed': other, 'no_fourth': no_fourth,
                          'reordered': reordered}
        tags.setdefault(tag, []).append(name)
        print(f'{name}: {len(weighed)} weighed lowest, {len(other)} weighed otherwise, '
              f'{len(no_fourth)} letters without a fourth level', file=sys.stderr)
    common_no_fourth = set.intersection(*(set(v['no_fourth']) for v in observed.values()))
    common_reordered = next(iter(observed.values()))['reordered']
    if any(v['reordered'] != common_reordered for v in observed.values()):
        sys.exit('the letters placed after their decomposition differ between locales')
    def entry(c, base):
        return f'(0x{c:04X}, 0x{base:04X})'
    # Ranges of consecutive characters with consecutive ranks.
    ignored = []
    for c in sorted(order):
        if ignored and ignored[-1][1] + 1 == c and rank[ignored[-1][1]] + 1 == rank[c]:
            ignored[-1][1] = c
        else:
            ignored.append([c, c, rank[c]])
    lines = [
        '//! Generated by scripts/generate_collation_glibc.py from glibc\'s',
        '//! iso14651_t1_common and its compiled sorting locales; do not edit. How',
        '//! glibc collates the characters apart from letters, which the Unicode',
        '//! collator orders differently.',
        '',
        '/// Characters glibc ignores at the first three levels, by code point: ranges',
        '/// `(first, last, rank)` of consecutive characters whose fourth-level ranks',
        '/// (their order in iso14651_t1_common) are consecutive from `rank`.',
        '#[rustfmt::skip]',
        'pub(super) static IGNORED: &[(u32, u32, u16)] = &[',
        *(f'    (0x{a:04X}, 0x{b:04X}, {r}),' for a, b, r in ignored),
        '];',
        '',
        '/// Letters and digits without a fourth-level weight in every sorting locale.',
        '#[rustfmt::skip]',
        'pub(super) static NO_FOURTH: &[(u32, u32)] = &[',
        *(f'    (0x{a:04X}, 0x{b:04X}),' for a, b in ranges(common_no_fourth)),
        '];',
        '',
        '/// Characters whose canonical decomposition (Unicode ' + unicodedata.unidata_version + ') is more',
        '/// than one character: ranges `(first, last, length)`. The Unicode collator',
        '/// reads them decomposed, so the fourth level counts them so too.',
        '#[rustfmt::skip]',
        'pub(super) static DECOMPOSED: &[(u32, u32, u8)] = &[',
        *(f'    (0x{a:04X}, 0x{b:04X}, {n}),' for a, b, n in decomposed()),
        '];',
        '',
        '/// Sequences glibc collates as the letter they spell in parts, with that',
        '/// letter, sorted by sequence: iso14651_t1_common\'s "decomposition of"',
        '/// collating elements, which tie with the letter at every level in every',
        '/// sorting locale.',
        '#[rustfmt::skip]',
        'pub(super) static CONTRACTIONS: &[(&str, char)] = &[',
        *('    ("' + ''.join(f'\\u{{{ord(c):04X}}}' for c in parts)
          + f'", \'\\u{{{ord(letter):04X}}}\'),' for parts, letter in composed),
        '];',
        '',
        '/// Letters coded below the first character of their canonical decomposition',
        '/// that glibc places after it, where the collator ties the two, the same',
        '/// in every sorting locale: `(letter, base)`, by letter, where `base` is',
        '/// that first character.',
        '#[rustfmt::skip]',
        'pub(super) static REORDERED: &[(u32, u32)] = &[',
        *(f'    {entry(c, base)},' for c, base in sorted(common_reordered.items())),
        '];',
        '',
        '/// A sorting locale\'s Unicode collation tag, the ignorable characters it',
        '/// weighs first at the first level, before every letter and digit (in their',
        '/// fourth-level order, which is below every other ignorable), and its',
        '/// further letters and digits without a fourth-level weight.',
        'pub(super) type Locale = (&\'static str, &\'static [u32], &\'static [(u32, u32)]);',
        '',
        '/// Per sorting locale, sorted by the Unicode collation tag it sorts with',
        '/// (the locales that share a tag behave alike), where it has any.',
        '#[rustfmt::skip]',
        'pub(super) static LOCALES: &[Locale] = &[',
    ]
    for tag, names in sorted(tags.items()):
        shapes = {json.dumps([observed[name]['weighed'],
                              ranges(set(observed[name]['no_fourth']) - common_no_fourth)])
                  for name in names}
        if len(shapes) != 1:
            sys.exit(f'the locales sorting with {tag} ({", ".join(names)}) differ')
        weighed, extra = json.loads(shapes.pop())
        if weighed or extra:
            points = ', '.join(f'0x{c:04X}' for c in weighed)
            spans = ', '.join(f'(0x{a:04X}, 0x{b:04X})' for a, b in extra)
            lines.append(f'    ("{tag}", &[{points}], &[{spans}]),')
    lines.append('];')
    OUTPUT.write_text('\n'.join(lines) + '\n', encoding='utf-8')
    DATA.write_text(json.dumps({
        'source': str(common.name),
        'source_sha256': hashlib.sha256(common.read_bytes()).hexdigest(),
        'unicode_decompositions': unicodedata.unidata_version,
        'ignorable': len(order),
        'contractions': {' '.join(f'{ord(c):04X}' for c in parts): f'{ord(letter):04X}'
                         for parts, letter in composed},
        'no_fourth_everywhere': ranges(common_no_fourth),
        'reordered_everywhere': [f'{c:04X}' for c in sorted(common_reordered)],
        'locales': {name: {'weighed': d['weighed'], 'other_weighed': d['other_weighed'],
                           'no_fourth': ranges(set(d['no_fourth']) - common_no_fourth)}
                    for name, d in sorted(observed.items())},
    }, indent=1) + '\n', encoding='utf-8')
    print(f'{len(order)} ignorable characters in {len(ignored)} ranges; '
          f'{len(ranges(common_no_fourth))} ranges without a fourth level everywhere; '
          f'{sum(1 for d in observed.values() if d["weighed"])} locales weigh an ignorable '
          f'lowest, {sum(1 for d in observed.values() if d["other_weighed"])} otherwise')


if __name__ == '__main__':
    main()
