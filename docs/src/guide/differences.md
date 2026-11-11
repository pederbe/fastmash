# Differences from GNU datamash

Fastmash aims to behave exactly like GNU datamash 1.9. This page lists the
known intentional differences and explicit extensions. Other differences in
GNU-compatible commands are bugs:
please [report it](https://github.com/pederbe/fastmash/issues/new/choose).

Comparisons were made against GNU datamash 1.9 on x86-64 Linux. Other GNU
versions and builds can differ from each other as well.

## Fastmash extensions

These features extend the command language through explicit options, operations
or modes. Ordinary GNU-compatible commands keep the compatibility rules on this
page. The linked guides describe each extension's supported combinations and
limits. No comparative workflow speed or memory claim is implied.

- [Quoted CSV and custom result names](csv-and-result-names.md) add explicit
  quoted input/output formats and supplied labels for calculation results.
- [Table health](table-health.md) inspects structural and missing-value
  observations, inferred types and supplied expectations, with readable and TSV
  reports and optional validation.
- [Top-N selection](top-n-records.md) returns the highest or lowest complete
  records by one numeric field, retaining original input order for ties.
- [Weighted mean](weighted-mean.md) adds `wmean VALUE:WEIGHT` with finite,
  nonnegative contribution weights and checked numerical range limits.
- [Dataset comparison](dataset-comparison.md) summarizes two raw datasets and
  reports their numerical changes, with complete comparison keys and optional
  ranking.
- [Terminal presentation reference](terminal-color.md) describes interactive
  output behavior.

## Numbers

| Area | GNU datamash 1.9 | Fastmash |
| --- | --- | --- |
| Arithmetic | The CPU's x87 unit and the C math library | Software arithmetic, including `log` and `exp`. Identical on every supported machine |
| Last digits | Depend on the build and the math library | Can differ from GNU in the last printed digit for some transcendental results |
| NaN from invalid arithmetic | Prints `-nan` on x86-64 | Always positive `nan` |
| NaN between two NaN inputs | Chosen by the x87 unit | The one with the larger payload, positive on a tie |
| `round`, `floor`, `ceil`, `trunc`, `frac`, `bin` of `-nan` | Gives `0` | Keeps `-nan` |
| `log` and `exp` at extreme arguments | Computed by the C library | A few boundary cases that cannot be settled within fixed limits are refused (status 77) |

## Safety

| Area | GNU datamash 1.9 | Fastmash |
| --- | --- | --- |
| `perc:100` | Can read past the end of the values | Returns the largest value |
| Non-integer percentile, such as `perc:2.5` | Reads an unset parameter. On x86-64 builds reports the misleading "invalid percentile value 0" | Refused, status 77, with a clear message |
| `rmdup` with several key fields | Aborts on an assertion | Refused, status 77 |
| Empty operation name, a pair whose second field is a range (`1:2-3`), `strbin:2.0`, numeric `getnum` type | Assertion failures or undefined behavior | error, status 1, with a message |
| The long `--seed` option | Mishandles its argument | Requires a value, like `-S` |
| `crosstab` labels longer than 511 bytes | Truncated | Kept in full |
| Unseeded `rand` when the kernel's random source fails | Prints `Error N` and continues | Refused, status 77 |
| Running out of memory | "memory exhausted", status 1 | Refused, status 77, naming what could not be stored. For an input record, "record memory allocation failed" |

## Features

| Area | Fastmash |
| --- | --- |
| `--sort-cmd` | Unsupported. Sort the input separately |
| Explicit `line` mode | Unsupported. Write the per-row operation directly |
| Hidden test options (`---print-inf` and others) | Unsupported |
| Locales | Bundled rules, with the [availability and ordering differences](#locale-and-sorting-differences) below |
| Quotation marks in messages | Curly quotes in UTF-8 locales. Messages are always in English |
| `-i` | ASCII case folding, excluding column names and duplicate-removal keys. Refused under the Turkic character types `az_AZ`, `crh_UA`, `ku_TR`, `tr_CY`, `tr_TR` and `tt_RU@iqtelif`, where glibc does not fold `i` and `I` |
| Unwritable `TMPDIR` | Fails only when sorting needs to spill, with status 1. Spill thresholds differ from GNU sort |
| Size limits | Growable records, operation lists and samples, with the [documented compatibility limits](output.md#limits) |

### Temporary storage

Fastmash accounts for available memory and cgroup limits when sizing its sort
buffer. GNU sort sizes from input files and host memory, ignoring cgroup limits,
and spills sooner on piped input. Hash grouping can avoid sorting records.
See [grouping and sorting](grouping.md#large-inputs) for resource controls and
the [architecture](../contributing/architecture.md#sorting) for buffer sizing.

## Locale and sorting differences

### Locale availability

Fastmash bundles glibc 2.43 numeric rules. It supports `C`, `POSIX`,
`C.UTF-8` and every glibc locale with a one-byte decimal separator, including
known `@` modifiers. Non-UTF-8 character sets work where their separators are
ASCII. Other numeric separators are refused when a command needs numbers.

Sorting supports the C locales and 194 verified language locales. Unverified
sorting locales are refused. Unknown locale names behave as `C` in both
programs. For a known locale missing on the host, GNU datamash falls back to
`C`, while Fastmash uses its bundled rules.

### Key order

In supported language locales, letters and digits follow Unicode collation.
Punctuation, symbols and spaces use glibc's ordering table, including characters
ignored until otherwise equal keys need a tie-breaker. For example, `a-1`
sorts before `a_1` before `a1`, and `aa` before `a+b`.

In Spanish locales, `gl_ES` and `pl_PL`, spaces sort before letters and
digits, as in GNU sort. This puts `San José` before `Sanabria`.

### Punctuation beside digits

glibc skips a character at the accent level in some digit-and-punctuation runs
ending before a letter. These keys can therefore appear in different orders:

| Program | Order |
| --- | --- |
| GNU sort | `12 A`, `12-A`, `12A`, `1-2A` |
| Fastmash | `1-2A`, `12 A`, `12-A`, `12A` |
| GNU sort | `file1.txt`, `File1.txt`, `file(1).txt` |
| Fastmash | `file(1).txt`, `file1.txt`, `File1.txt` |

A few symbols that glibc weighs rather than ignores also differ, including
`¼` versus `1/4`, and the numero sign in `tk_TM`.

### Letters and combining marks

Letters outside the language's alphabet can differ in order. These include
Hangul syllables, Georgian capitals, some Arabic letters, CJK Extension A/B,
compatibility ideographs and characters newer than glibc's table.

A precomposed letter sorts after its base plus combining marks, as in GNU:
`e` with U+0301 precedes `é`, and `Е` with U+0308 precedes `Ё`.
glibc makes exceptions for some letters with two marks, including Vietnamese
`ổ` and `ǖ`, putting them first at the end of a key or before a letter.
Fastmash keeps them after the decomposed spelling. They also differ where glibc
uses a second mark before an earlier difference, as in `éǖ`.

Combining marks written outside Unicode order, such as `a` with U+0301 then
U+0323, follow Unicode collation in Fastmash rather than glibc's order.

### Tied spellings and grouping

Where glibc treats two spellings as the same letter, the next key decides.
Fully tied keys stay in input order, with one group per run of a spelling.
Examples include `и` plus a combining breve versus `й`, `L` plus a middle
dot versus `Ŀ`, and some Arabic, Indic, Thai, Lao and Tibetan vowel forms.

In `cy_GB`, `sq_AL` and `sq_MK`, `ll` followed by a middle dot sorts as
`l` and `ŀ` in Fastmash. glibc instead reads the locale's `ll` and then
the dot. Such a key ties with `lŀ` in Fastmash, producing a group per input
run, while GNU orders them apart.

When group order differs, seeded `rand` can select a different number because
groups share one random stream.

### Invalid sorted keys

Language-locale keys must be valid UTF-8 without NUL bytes. Other keys are
refused. With `rmdup -s --vnlog`, comment records sort with the data, as in
GNU datamash, and a comment can become the second header. A comment whose key is
invalid UTF-8 is therefore refused too.

## Diagnostics

Error messages use the same wording as GNU datamash where it matters to
scripts, but some messages differ, and they begin with the program name, `fastmash:`.
Errors for Fastmash extensions include their own field-selection guidance,
supported modes and examples of valid option syntax.

Common command errors also include a separate `hint:` line with valid syntax
or a recovery step. The original error wording and exit status are preserved.
Top-N selection errors use their own wording and include a ranking example.
For byte-order recovery, `LC_ALL` must be unset before `LC_COLLATE=C` takes
effect. Copy its value to `LC_NUMERIC` first to keep the same number format.

A few failure paths also differ in detail: for example, a read error in input sorted
with `-s` can give just "read error: Input/output error", where GNU datamash
prints the system `sort`'s message and "read error (on close)". When standard
input cannot be read at all (opened for writing only), GNU datamash's `-s`
prints an error from the shell that runs `sort` but ends with status 0, where
Fastmash reports the read error with status 1. And `--help` or `--version` into a closed pipe exits with status
77 rather than by `SIGPIPE`. Scripts should rely on exit statuses, not the
exact text of error messages.

Ordinary commands use normal Unix broken-pipe handling. Blocked SIGPIPE and
failed signal-mask queries cause a refusal.

With native sorting on Linux and macOS, a command with a named grouping key
or sorted `rmdup` key and no input header
reports `fastmash: missing input header for named grouping key` and exits with
status 1, including when comment-only input leaves no header. These failure
paths run inside Fastmash on both platforms. A failed input read still reports
the read error and exits with status 1. The missing-header diagnostic does not
hide it. A command containing `wmean` still succeeds without output when no
input header arrives at clean EOF. OS-owned diagnostic wording is checked
separately from portable results.
