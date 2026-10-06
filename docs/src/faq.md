# FAQ

## Is Fastmash a drop-in replacement for GNU datamash?

For most commands, yes: it accepts the same operations, options and selectors
and aims to produce the same output. Check your scripts against
[Differences from GNU datamash](guide/differences.md), especially the locale
settings and numerical last digits, and keep `datamash` installed while you compare.

## Is it always faster?

Not yet. Fastmash is faster on many common jobs and slower on some others,
and each release publishes both in the [benchmarks](benchmarks/index.md).
Try it on your own data, and tell us about jobs where it is slower.

## Why does Fastmash refuse my locale?

Fastmash has the number rules of every glibc locale built in, `@` modifier
locales included, but sorts only in the language locales where its Unicode
collation was checked to order letters and digits as glibc does (194 of them,
such as `fr_FR`, `es_ES` and `pl_PL`); punctuation and symbols follow glibc's
own table there, with a few exceptions, such as some keys with punctuation
next to digits (see [Differences](guide/differences.md)). In other locales, such as `cs_CZ` or `nb_NO`, it refuses to
sort. It refuses numbers only where it cannot write a locale's separators:
`ps_AF`, whose decimal separator is not one byte, and a character set other
than UTF-8 when the thousands separator is not ASCII (such as `fr_FR` in
Latin-1). A name glibc has no locale for behaves as `C`, as in GNU datamash.
Set `LC_COLLATE=C.UTF-8` (or `LC_ALL=C.UTF-8`) for byte ordering.

## Can it read CSV files?

Yes. `--csv-in` reads strict quoted CSV, `--csv-out` encodes output fields, and
`--csv` selects both. Quoted fields can contain commas, quotes and line breaks;
format switches do not infer headers. Aggregates, per-row operations, Top-N
selection and dataset comparison support CSV, including their supported grouped
workflows. Table health and legacy table/field modes remain ordinary-text only.
`-t,` still means literal comma splitting, as in GNU datamash. See
[Quoted CSV and result names](guide/csv-and-result-names.md).

## Can I check a table before calculating?

`fastmash --header-in health < data.tsv` reports inconsistent widths, missing
values, mixed lexical types and examples. Add known rules such as
`type price number nonmissing id width 3 validate` to fail on declared
violations. Inferred findings stay advisory. A versioned TSV report is available
for scripts. See [Table health reports](guide/table-health.md).

## Can I compare two exports with different column orders?

Yes. With `-H`, named fields bind independently to each input header.
`fastmash -H -g category compare before.tsv after.tsv sum revenue` reports each
key's before, after, signed difference and percentage, including added and
removed keys. Keys consolidate across each whole dataset; they need not be
adjacent. See [Dataset comparison](guide/dataset-comparison.md).

## Will terminal color change data in a pipeline?

No. Only help, the program prefix on failures and readable health reports use
color. Calculation results, CSV, health TSV and version output remain plain even
with `--color=always`. Styling defaults to automatic terminal detection;
`--no-color` disables it. See [Terminal color](guide/terminal-color.md).

## Why are there two executables?

`fastmash-sort-supervisor` runs the system `sort` safely for some sorted jobs
in the C locales, where the system `sort` is faster on large input. Keep it in
the same directory as `fastmash`: without it, those jobs sort inside Fastmash,
with the same output.

## Why exit status 77?

Status 77 means Fastmash deliberately declined to produce a result: an
unsupported feature, locale or condition, or a resource limit. It is distinct
from status 1, which means the command or the input is wrong. See
[Errors, exit statuses and resources](guide/errors.md).

## Why do results sometimes differ from GNU datamash in the last digit?

Fastmash computes with the same 80-bit precision, but in software, including
its logarithms and exponentials. Its results are the same on
every machine; GNU datamash's depend on the hardware and C library. See
[Portable results](guide/output.md#portable-results).

## Does it work on macOS or Windows?

On Windows, use WSL2. macOS on Apple Silicon is a later target; see the
[roadmap](roadmap.md). Native Windows is outside the current product scope.

## How does Fastmash relate to GNU datamash?

Fastmash is an independent reimplementation of GNU datamash's command
language, written in Rust. It contains no GNU code and is not affiliated with
the GNU Project. GNU datamash, by Assaf Gordon and contributors, defined the
interface that Fastmash follows.
