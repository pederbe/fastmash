# Differences from GNU datamash

Fastmash aims to behave exactly like GNU datamash 1.9. This page lists the
known intentional differences and explicit extensions. Other differences in
GNU-compatible commands are bugs:
please [report it](https://github.com/pederbe/fastmash/issues/new/choose).

Comparisons were made against GNU datamash 1.9 on x86-64 Linux. Other GNU
versions and builds can differ from each other as well.

## Fastmash extensions

These features extend the command language through explicit options, Operations
or Modes. Ordinary GNU-compatible commands keep the compatibility rules on this
page. The linked guides describe each extension's supported combinations and
limits; no comparative workflow speed or memory claim is implied.

- [Quoted CSV and custom result names](csv-and-result-names.md) add explicit
  quoted input/output formats and supplied labels for calculation results.
- [Table health](table-health.md) inspects structural and missing-value
  observations, inferred types and supplied expectations, with readable and TSV
  reports and optional validation.
- [Top-N selection](top-n-records.md) returns the highest or lowest complete
  Records by one numeric Field, retaining original input order for ties.
- [Weighted mean](weighted-mean.md) adds `wmean VALUE:WEIGHT` with finite,
  nonnegative contribution weights and checked numerical range limits.
- [Dataset comparison](dataset-comparison.md) summarizes two raw datasets and
  reports their numerical changes, with complete Comparison keys and optional
  ranking.
- [Terminal color](terminal-color.md) styles help, failure prefixes and readable
  health reports on eligible terminals, or under explicit controls. Data output,
  health TSV and version bytes remain plain, even when color is forced.

## Numbers

| Area | GNU datamash 1.9 | Fastmash |
| --- | --- | --- |
| Arithmetic | The CPU's x87 unit and the C math library | Software arithmetic, including `log` and `exp`; identical on every supported machine |
| Last digits | Depend on the build and the math library | Can differ from GNU in the last printed digit for some transcendental results |
| NaN from invalid arithmetic | Prints `-nan` on x86-64 | Always positive `nan` |
| NaN between two NaN inputs | Chosen by the x87 unit | The one with the larger payload, positive on a tie |
| `round`, `floor`, `ceil`, `trunc`, `frac`, `bin` of `-nan` | Gives `0` | Keeps `-nan` |
| `log` and `exp` at extreme arguments | Computed by the C library | A few boundary cases that cannot be settled within fixed limits are refused (status 77) |

## Safety

| Area | GNU datamash 1.9 | Fastmash |
| --- | --- | --- |
| `perc:100` | Can read past the end of the values | Returns the largest value |
| Non-integer percentile, such as `perc:2.5` | Reads an unset parameter; on x86-64 builds reports the misleading "invalid percentile value 0" | Refused, status 77, with a clear message |
| `rmdup` with several key fields | Aborts on an assertion | Refused, status 77 |
| Empty operation name, a pair whose second field is a range (`1:2-3`), `strbin:2.0`, numeric `getnum` type | Assertion failures or undefined behavior | Error, status 1, with a message |
| The long `--seed` option | Mishandles its argument | Requires a value, like `-S` |
| `crosstab` labels longer than 511 bytes | Truncated | Kept in full |
| Unseeded `rand` when the kernel's random source fails | Prints `Error N` and continues | Refused, status 77 |
| Running out of memory | "memory exhausted", status 1 | Refused, status 77, naming what could not be stored; for an input record, "record memory allocation failed" |

## Features

| Area | Fastmash |
| --- | --- |
| `--sort-cmd` | Not supported; sort the input separately |
| Explicit `line` mode | Not supported; per-row operations work without it |
| Hidden test options (`---print-inf` and others) | Not supported |
| Locales | Built in from glibc 2.43 rather than read from the system: numbers in `C`, `POSIX`, `C.UTF-8` and every glibc locale with a one-byte decimal separator, `@` modifier locales included (in a character set other than UTF-8, where its separators are ASCII); sorting in `C`, `POSIX`, `C.UTF-8` and 194 language locales checked against glibc. Other locales refuse commands that read or print numbers, or sort. A name glibc has no locale for behaves as `C` in both. Where a locale is not installed, GNU datamash falls back to `C` and Fastmash does not. Messages are in English |
| Sorted key order in language locales | As GNU `sort` orders them: letters and digits by Unicode collation, and the punctuation, symbols and spaces that GNU ignores except to break ties between otherwise equal keys by glibc's own table, in its order (so `a-1` before `a_1` before `a1`, and `aa` before `a+b`); in the Spanish locales, `gl_ES` and `pl_PL` a space sorts before every letter and digit, as in GNU (`San José` before `Sanabria`). Some differences remain. glibc skips one character at the accent level in some runs of digits, punctuation and symbols that end before a letter, so keys that differ only in punctuation or spaces next to digits can come in another order: GNU sorts `12 A`, `12-A`, `12A`, `1-2A` and `file1.txt`, `File1.txt`, `file(1).txt`, where Fastmash sorts `1-2A`, `12 A`, `12-A`, `12A` and `file(1).txt`, `file1.txt`, `File1.txt`. A few symbols that glibc weighs rather than ignores sort differently, such as a vulgar fraction and the digits it stands for (`¼` and `1/4`), and in `tk_TM` the numero sign. Letters from outside the language's alphabet can be placed differently, notably Hangul syllables, Georgian capitals, some Arabic letters, CJK Extension A/B and compatibility ideographs, and characters newer than glibc's table. A letter written as one character sorts after the same letter written as a base and combining marks, as in GNU (`e` and U+0301 before `é`, `Е` and U+0308 before `Ё`). glibc departs from that for some letters with two marks, such as Vietnamese `ổ` and `ǖ`, which it puts first at the end of a key or before a letter, and Fastmash does not: it puts them after there too, and also where glibc decides by the second mark before an earlier difference (`éǖ`). Where glibc treats two spellings as the same letter (`и` and a combining breve, and `й`; `L` and a middle dot, and `Ŀ`; some Arabic, Indic, Thai, Lao and Tibetan vowel forms), the next key decides, and tied keys stay in input order, a Group per run of one spelling, as in GNU. Combining marks written in another order than Unicode's (`a` with U+0301 then U+0323) sort as the Unicode collator reads them, not where glibc puts them. In `cy_GB`, `sq_AL` and `sq_MK`, `ll` before a middle dot sorts as `l` and `ŀ`, where glibc reads the locale's `ll` and then the dot, so such a key ties with `lŀ` and the two form a Group per run in the input, where GNU orders them apart. Keys must be valid UTF-8 without NUL bytes, or the command is refused. `rmdup -s --vnlog` sorts `#` comment records with the data, as GNU datamash does (one can become the second header), so a comment whose key field is not valid UTF-8 is refused too |
| Quotation marks in messages | Curly quotes in UTF-8 locales; messages are never translated |
| `-i` | Folds ASCII letters only; does not apply to column names or `rmdup` keys; refused under the Turkic character types `az_AZ`, `crh_UA`, `ku_TR`, `tr_CY`, `tr_TR` and `tt_RU@iqtelif`, where glibc does not fold `i` and `I` |
| Unwritable `TMPDIR` | A sorted job fails only if the sort has to spill to disk ("sort temporary I/O error", status 1), as in GNU datamash, but the two spill at different sizes: Fastmash only when memory does not allow it to hold the input (its sort starts at 64 MiB and grows within the available memory and any cgroup limit, or stays at `FASTMASH_SORT_MEMORY_BYTES` when that is set), and not at all when it groups input from a file by key instead of sorting it; GNU `sort` sizes its buffer from an input file and the host's available memory, ignoring cgroup limits, and spills much sooner on piped input |
| Size limits | No fixed limits on records, operations, values or results |

## Diagnostics

Error messages use the same wording as GNU datamash where it matters to
scripts, but some messages differ, and they begin with the program name, `fastmash:`. A few
failure paths also differ in detail: for example, a read error in input sorted
with `-s` can give just "read error: Input/output error", where GNU datamash
prints the system `sort`'s message and "read error (on close)"; when standard
input cannot be read at all (opened for writing only), GNU datamash's `-s`
prints an error from the shell that runs `sort` but ends with status 0, where
Fastmash reports the read error with status 1; and `--help` or `--version` into a closed pipe exits with status
77 rather than by `SIGPIPE`. Scripts should rely on exit statuses, not the
exact text of error messages.
