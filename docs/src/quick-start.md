# Quick start

This tour takes about five minutes. It assumes Fastmash is
[installed](install.md). Every example reads from standard input, so you can
paste it straight into a terminal.

> **Locale.** The examples use decimal points. If your shell uses a locale with
> decimal commas or one Fastmash doesn't know (see
> [Locales](guide/output.md#locales)), run `export LC_ALL=C.UTF-8` first.

## Your first summary

Fastmash reads records (lines) of fields separated by tabs, and applies one
or more **operations** to selected fields:

```console
$ printf '1\t2\n3\t4\n' | fastmash sum 1 mean 2
4	3
```

`sum 1` adds up field 1; `mean 2` averages field 2. Results are printed in the
order you ask for them, separated by tabs.

## Several fields at once

A field list or range applies an operation to each field:

```console
$ printf '1\t2\t3\n4\t5\t6\n' | fastmash sum 1-3
5	7	9
```

## Grouping

`-g` groups records by a key field and summarizes each group:

```console
$ printf 'A\t1\nA\t2\nB\t4\n' | fastmash -g 1 sum 2 count 2
A	3	2
B	4	1
```

Groups are made from **adjacent** records with the same key. If equal keys
can be scattered through the input, add `-s` to sort first:

```console
$ printf 'b\t2\na\t1\na\t3\n' | fastmash -s -g 1 sum 2
a	4
b	2
```

## Headers and named fields

With a header line, use `-H` to name fields and label the output:

```console
$ printf 'site\treading\nwest\t2\neast\t4\nwest\t6\n' | fastmash -H -s -g site sum reading mean reading
GroupBy(site)	sum(reading)	mean(reading)
east	4	4
west	8	4
```

`--header-in` reads the names without printing an output header.

## Other separators

`-t` sets a single-byte field separator, and `-W` splits on runs of spaces
and tabs:

```console
$ printf '1,2\n3,4\n' | fastmash -t, sum 1-2
4,6
$ printf '1  2\n3   4\n' | fastmash -W sum 2
6
```

`-t,` splits on every comma, preserving datamash's literal separator behavior.
For quoted CSV, use `--csv` to read and write CSV or choose input/output
independently with `--csv-in` and `--csv-out`:

```console
$ printf 'region,revenue\n"west,coast",2\neast,4\n"west,coast",6\n' | fastmash --csv -H -s -g region --result-name=1:total sum revenue
GroupBy(region),total
east,4
"west,coast",8
```

Format switches do not infer headers. [Quoted CSV and result names](guide/csv-and-result-names.md)
describes the supported modes and quoting rules.

## Statistics

```console
$ printf '2\n4\n4\n4\n5\n5\n7\n9\n' | fastmash median 1 q1 1 q3 1 pstdev 1 perc:90 1
4.5	4	5.5	2	7.6
```

See [Operations](guide/operations.md) for the full list: means, variances,
quantiles, modes, skewness and kurtosis, normality tests, correlation and more.

## Weighted means

Give each value a contribution weight with `wmean VALUE:WEIGHT`:

```console
$ printf '10\t1\n20\t3\n' | fastmash wmean 1:2
17.5
```

Weights must be finite and nonnegative; a nonempty calculation needs positive
retained weight. See [Weighted mean](guide/weighted-mean.md).

## Keep the highest records

Top-N selection retains complete records and keeps earlier input records on ties:

```console
$ printf 'A\t8\nB\t10\nC\t10\nD\t9\n' | fastmash top:2 2
B	10
C	10
```

Use `bottom:N` for the lowest records, and `-s -g` for a cutoff in each group.
See [Highest and lowest records](guide/top-n-records.md).

## Per-row operations

Some operations transform each record instead of summarizing:

```console
$ printf '3.14159\thello\n' | fastmash round 1 md5 2
3	5d41402abc4b2a76b9719d911017c592
```

## Missing values

`--narm` skips `NA`, `N/A` and `NaN` values:

```console
$ printf '1\nNA\n3\n' | fastmash --narm mean 1
2
```

## Next steps

- [Fields and input](guide/input.md): selectors, separators, headers, comments
- [Grouping and sorting](guide/grouping.md): `-g`, `-s`, `crosstab`
- [Table health](guide/table-health.md): inspect a table or validate supplied rules
- [Dataset comparison](guide/dataset-comparison.md): before/after summaries and ranked changes
- [Migrating from GNU datamash](guide/migrating.md)
- `fastmash --help` lists every operation and option
