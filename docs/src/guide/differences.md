# Differences from GNU datamash

Fastmash aims to behave exactly like GNU datamash 1.9. This page lists the
known intentional differences. Anything else that behaves differently is a bug:
please [report it](https://github.com/pederbe/fastmash/issues/new/choose).

Comparisons were made against GNU datamash 1.9 on x86-64 Linux. Other GNU
versions and builds can differ from each other as well.

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

## Features

| Area | Fastmash |
| --- | --- |
| `--sort-cmd` | Not supported; sort the input separately |
| Explicit `line` mode | Not supported; per-row operations work without it |
| Hidden test options (`---print-inf` and others) | Not supported |
| Locales | Built in from glibc 2.43 rather than read from the system: numbers in `C`, `POSIX`, `C.UTF-8` and every UTF-8 glibc locale without an `@` modifier and with a one-byte decimal separator; sorting in `C`, `POSIX`, `C.UTF-8` and 194 language locales checked against glibc. Other locales refuse commands that read or print numbers, or sort. Where a locale is not installed, GNU datamash falls back to `C` and Fastmash does not. Messages are in English |
| Sorted key order in language locales | Unicode collation, which orders punctuation differently from GNU `sort` (`a_1` before `a-1`), and can place letters from outside the language's alphabet differently, notably Hangul syllables, Georgian capitals, some Arabic letters and CJK Extension A/B and compatibility ideographs; keys must be valid UTF-8 without NUL bytes, or the command is refused |
| Quotation marks in messages | Curly quotes in UTF-8 locales; messages are never translated |
| `-i` | Folds ASCII letters only; does not apply to column names or `rmdup` keys; refused under the Turkic character types `az_AZ`, `crh_UA`, `ku_TR`, `tr_CY` and `tr_TR`, where glibc does not fold `i` and `I` |
| Unwritable `TMPDIR` | Stops the sorted jobs that use the system `sort` even on small input ("sort temporary I/O error", status 1); GNU datamash only fails if `sort` has to spill to disk |
| Ignored `HUP`, `INT` or `TERM` (`nohup`, `command &` in a script) | The sorted jobs that use the system `sort` stop with status 77 ("sort process runtime requirements are not met"); GNU datamash runs them. Run them in the foreground, for example in `tmux` or `screen` |
| Size limits | No fixed limits on records, operations, values or results; `--format` strings over 99 bytes are refused |

## Diagnostics

Error messages use the same wording as GNU datamash where it matters to
scripts, but some messages differ, and they begin with the program name, `fastmash:`. A few
failure paths also differ in detail: for example, invalid sorted input can give a
simpler read-error message, and `--help` into a closed pipe exits with status
77 rather than by `SIGPIPE`. Scripts should rely on exit statuses, not the
exact text of error messages.
