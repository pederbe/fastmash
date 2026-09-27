# Numbers, output and locales

## Reading numbers

Numerical operations accept decimal numbers (`42`, `-3.5`, `1e-9`),
hexadecimal floating-point (`0x1.8p3`), `inf` and `nan`. Leading blanks are
accepted. Anything else, including trailing blanks or an empty field, is an
error that names the line and field.

## Printing numbers

By default, results are printed with up to 14 significant digits, like
`printf "%.14g"`:

```console
$ printf '1\n2\n' | fastmash mean 1 geomean 1
1.5	1.4142135623731
```

Two options change how results are printed. Neither changes how they are computed.

| Option | Effect |
| --- | --- |
| `-R N`, `--round=N` | Print `N` digits after the decimal point (1 to 50) |
| `--format=FMT` | Use a `printf`-style format with one `%e`, `%f`, `%g` or `%a` conversion |

```console
$ printf '1\n2\n' | fastmash -R 3 mean 1
1.500
$ printf '1234567\n' | fastmash --format "%'.2f" sum 1
1234567.00
```

The `'` flag groups digits with the locale's thousands separator and grouping:
`1,234,567.00` in `en_US.UTF-8`, `1.234.567,00` in `de_DE.UTF-8`,
`12,34,567.00` in `as_IN.UTF-8`. In `C` it does nothing.

## Limits

`--format` strings longer than 99 bytes are refused. Numbers and results
have no fixed length limit.

## Precision

Fastmash computes with 80-bit extended precision (a 64-bit significand, about
19 significant decimal digits), the same precision GNU datamash uses on x86-64
Linux. Sums are accumulated in input order.

## Portable results

Fastmash implements its arithmetic in software, including its logarithms and
exponentials. As a result, one Fastmash version gives
**identical results on every supported machine**, independent of the CPU
model or the system's math library.

This also means Fastmash's output can occasionally differ from a GNU datamash
build in the last printed digit, or in the sign of a `nan`. See
[Differences from GNU datamash](differences.md#numbers).

The rules for reading, computing and printing numbers are versioned together
as a **numerical profile**. A Fastmash release that changes any numerical
output says so in its changelog.

## Locales

The locale decides how numbers are written and the order of sorted keys.
Fastmash has the rules of the glibc locales built in, taken from glibc 2.43,
so it does not need locale data installed on the system.

**Numbers.** In `C`, `POSIX` and `C.UTF-8`, and in the UTF-8 locales glibc
provides (such as `en_US.UTF-8`, `fr_FR.UTF-8`, `nb_NO.UTF-8` or
`hi_IN.UTF-8`), numbers are read and printed with that locale's decimal
separator, and the `'` format flag uses its thousands separator and grouping.
The name needs a `.UTF-8` (or `.utf8`) codeset, unless glibc's default for
it is UTF-8 (such as `hi_IN`), and no `@` modifier. The one
glibc locale whose decimal separator is not a single byte, `ps_AF`,
is not supported. Thousands separators are never accepted in input, as in GNU
datamash.

**Sorted keys.**

| Locale | Sorted key order |
| --- | --- |
| `C`, `POSIX`, `C.UTF-8` | Byte order |
| 194 glibc locales in 123 languages, such as `en_US`, `de_DE`, `fr_FR`, `es_ES`, `it_IT`, `pt_BR`, `nl_NL`, `pl_PL`, `ro_RO`, `hr_HR`, `ru_RU` and `he_IL` | Alphabetical (Unicode collation), checked against glibc |
| Other locales, such as `cs_CZ`, `da_DK`, `el_GR`, `fi_FI`, `hu_HU`, `ja_JP`, `nb_NO`, `sv_SE`, `tr_TR`, `uk_UA` and the Chinese locales | Sorting is refused |

A language locale is supported for sorting when Fastmash's order of its
alphabet (its standard and auxiliary letters in the Unicode CLDR data, alone,
in pairs and in each case, and digits) matches GNU `sort` under glibc exactly. `fastmash --help` lists the
languages. Punctuation is ordered differently from GNU `sort`; see
[Differences](differences.md).

Fastmash reads `LC_NUMERIC` (for numbers), `LC_COLLATE` (for sorting) and
`LC_CTYPE` (for quotation marks and `-i`) from the first non-empty value of
`LC_ALL`, that variable, and `LANG`, falling back to `C`.

With an unsupported locale, commands that
neither read nor print numbers (`transpose`, `cut`, `first`, `unique`,
checksums and the like, and `count`, `countunique` and `strbin` unless
`--format` or `--round` is given)
work normally. Commands that do are
refused with exit status 77 rather than risk reading numbers with the wrong
decimal separator, and the message tells you which setting to change:

```text
fastmash: unsupported numeric locale ‘ps_AF.UTF-8’ from LANG
hint: set LC_NUMERIC=C.UTF-8, or a UTF-8 locale whose decimal separator is one byte
```

Two situations make GNU datamash differ, because it uses the installed
locale data instead:

- If the locale is not installed, GNU datamash (and every other C program)
  falls back to `C` for everything, while Fastmash still uses the locale's
  rules. Check with `locale -a`.
- A different glibc version can have slightly different data: for example,
  glibc changed `fr_FR`'s thousands separator to a narrow no-break space.
  This only affects the `'` format flag and sorting.

For scripts, setting `LC_ALL=C.UTF-8` gives the same results everywhere.

Diagnostics are always in English.
