# Compared with other tools

Fastmash runs GNU datamash commands with portable results, and extends that
workflow with quoted CSV, weighted means, record selection, table health and
dataset comparison. Other good tools cover much more ground. This page
says honestly where each one fits. Facts about the other tools come from their
own documentation (checked September 2026); tell us if something has changed.

## At a glance

| | Fastmash | [GNU datamash](https://www.gnu.org/software/datamash/) | [Miller](https://miller.readthedocs.io/) | [qsv](https://github.com/dathere/qsv) | [csvtk](https://bioinf.shenwei.me/csvtk/) |
| --- | --- | --- | --- | --- | --- |
| Command language | datamash's | datamash's | Its own verbs and DSL | Its own subcommands | Its own subcommands |
| Written in | Rust | C | Go | Rust | Go |
| Licence | MIT or Apache-2.0 | GPL-3.0-or-later | BSD-2-Clause | MIT | MIT |
| Input | Delimited text and explicit quoted CSV | Tab or single-byte delimited text | CSV (RFC 4180), TSV, JSON and more | CSV, TSV, Excel, Parquet and more | CSV and TSV |
| Quoted CSV fields | Yes, in supported calculation, selection and comparison modes | No | Yes | Yes | Yes |
| Grouped statistics | Yes | Yes | Yes (`stats1 -g`) | Through `pivotp` or `sqlp` | Yes (`summary -g`) |
| Joins, filters, reshaping | No | No | Yes | Yes | Yes |
| Platforms | Linux x86-64 | Linux, macOS, Windows and others | Linux, macOS, Windows, BSD | Linux, macOS, Windows | Linux, macOS, Windows, BSD |

## GNU datamash

The original, and the reference Fastmash is measured against. It runs almost
everywhere and is packaged by every Linux distribution, conda-forge and
Homebrew.

**Choose Fastmash when** you run datamash commands on Linux and want them
faster (up to 4 times on the published [benchmarks](benchmarks/index.md)),
want the same digits on every machine, or want clear errors where datamash
1.9 aborts or reads past its data. Your commands don't change; the
[differences](guide/differences.md) are listed on one page.

**Stay with GNU datamash when** you need macOS, native Windows or a non-x86
CPU, or when you need a GNU-maintained tool. Some disk-spilling jobs still
favor GNU on native Linux; the [benchmarks](benchmarks/index.md) show the
costs and measurement conditions.

## Miller

A complete toolkit for record-oriented data: it reads and writes CSV (with
quoting), TSV, JSON and several other formats, and has a full expression
language for filtering, computing new fields, joining, sorting, sampling and
reshaping. Its `stats1` verb computes grouped statistics (count, sum, mean,
median and any percentile, mode, variance, skewness and more) without
sorting the input first.

**Choose Miller when** your data is JSON, or the job needs more
than statistics: filtering, joining or transforming records.
**Choose Fastmash when** the job is a datamash-style summary of delimited
text and speed or exact datamash output matters.

## qsv

A very fast, very broad CSV toolkit in Rust: indexing, joins (through Polars),
SQL queries, validation, sampling, format conversion to Parquet, Excel and
databases, and whole-column statistics. Grouped aggregation goes through
`pivotp` (count, sum, mean, median, quantiles, first, last) or `sqlp`.

**Choose qsv when** you work with large CSV files and want one tool for
exploring, querying and converting them.
**Choose Fastmash when** you want datamash's operations and syntax, for
example sample and population standard deviations, skewness and kurtosis,
mode, collapse or crosstab per group, with datamash-identical output.

## csvtk

A cross-platform CSV/TSV toolkit popular in bioinformatics, with subcommands
for selecting, filtering, joining, sorting, reshaping, converting
and plotting. `csvtk summary` computes grouped statistics (count, sum, mean,
median, quartiles, standard deviation, first, last, unique values, collapse),
printing two decimals by default.

**Choose csvtk when** you want a single, familiar toolkit for everyday table
work, including quoted CSV and compressed files.
**Choose Fastmash when** you need datamash's full set of statistics and exact
datamash output, or its speed on large inputs.

## Using them together

These tools combine well. Fastmash can read quoted CSV directly:

```sh
fastmash --csv -H -s -g region mean price < data.csv
```

Use another tool to filter or join records, then Fastmash to summarize them.
For ordinary text workflows, a quote-aware conversion remains useful:

```sh
mlr --icsv --otsv cat data.csv | fastmash -H -s -g region mean price
```
