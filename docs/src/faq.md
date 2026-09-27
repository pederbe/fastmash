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

Fastmash has the number rules of every UTF-8 glibc locale (without an `@`
modifier) built in, but sorts only in the language locales where its Unicode
collation was checked to match glibc (194 of them, such as `fr_FR`, `es_ES`
and `pl_PL`). In other locales, such as `cs_CZ` or `nb_NO`, it refuses to
sort rather than order keys differently from GNU. It also refuses numbers in
locales that aren't UTF-8 (a name without `.UTF-8` is accepted when glibc's
default for it is UTF-8, such as `hi_IN`). Set `LC_COLLATE=C.UTF-8` (or
`LC_ALL=C.UTF-8`) for byte ordering.

## Can it read CSV files?

It splits on any single byte, so `-t,` works for simple comma-separated data.
It does not interpret quoted fields such as `"Smith, John"`, just like GNU
datamash. Convert such files first, for example with
[Miller](https://miller.readthedocs.io/) (`mlr --icsv --otsv cat`),
[qsv](https://github.com/dathere/qsv) or
[csvtk](https://bioinf.shenwei.me/csvtk/) (`csvtk csv2tab`). See
[Compared with other tools](comparison.md).

## Why are there two executables?

`fastmash-sort-supervisor` runs the system `sort` safely for the sorted jobs
that Fastmash's own sorter doesn't handle yet (some statistics in the C
locales). Keep it in the same
directory as `fastmash`.

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

On Windows, use WSL2. macOS on Apple Silicon and native Windows are planned;
see the [roadmap](roadmap.md).

## How does Fastmash relate to GNU datamash?

Fastmash is an independent reimplementation of GNU datamash's command
language, written in Rust. It contains no GNU code and is not affiliated with
the GNU Project. GNU datamash, by Assaf Gordon and contributors, defined the
interface that Fastmash follows.
