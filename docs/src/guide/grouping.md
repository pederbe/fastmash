# Grouping and sorting

## Groups

`-g KEYS` (also written `groupby KEYS`, `grouping KEYS` or `gb KEYS`) makes
Fastmash summarize each **group** of records that share the same key values.
Output rows start with the key values, followed by the results:

```console
$ printf 'A\tx\t1\nA\tx\t2\nA\ty\t5\nB\ty\t4\n' | fastmash -g 1,2 sum 3
A	x	3
A	y	5
B	y	4
```

A group is a run of **adjacent** records with equal keys. When the key changes,
the group ends and its results are printed. This makes grouping a single pass
over the input, with memory proportional to one group.

If equal keys are not adjacent, the same key appears in more than one group:

```console
$ printf 'b\t2\na\t1\nb\t3\n' | fastmash -g 1 sum 2
b	2
a	1
b	3
```

## Sorting first: `-s`

`-s` sorts the records by their keys before grouping, so every key forms one group:

```console
$ printf 'b\t2\na\t1\nb\t3\n' | fastmash -s -g 1 sum 2
a	1
b	5
```

The sort is stable: records with equal keys keep their input order, so
`first`, `last` and `collapse` give predictable results.

Key order depends on the [locale](output.md#locales). In `C` and `C.UTF-8`,
keys are ordered byte by byte. In the supported language locales, such as
`en_US.UTF-8`, `fr_FR.UTF-8` or `pl_PL.UTF-8`, keys are ordered alphabetically
using built-in Unicode collation rules.

If your input is already sorted, leave out `-s`: the result is the same and
the job is faster.

### Large inputs

Fastmash sorts in memory, and when the data is large it writes sorted chunks
to temporary files in `TMPDIR` (default `/tmp`) and merges them. The files are
anonymous: the operating system removes them automatically, even if Fastmash
is interrupted.

`FASTMASH_SORT_MEMORY_BYTES` sets the target chunk size (default 64 MiB). It
is a target for the sort buffer, not a limit on Fastmash's total memory use.

In the `C`, `POSIX` and `C.UTF-8` locales, some operations still use the
system `sort` command, through Fastmash's sort supervisor: `rand`, `geomean`,
`harmmean`, `ms`, `rms`, the skewness and kurtosis operations, `jarque`,
`dpo`, the paired statistics, and `rmdup` with `-s`. This route writes its
temporary files in a private directory in `TMPDIR`; see
[System requirements](../requirements.md) for what it needs. In language locales
every operation uses Fastmash's own sorter.

## Ignoring case: `-i`

`-i` compares keys ignoring ASCII letter case, for grouping and sorting. The
output shows each key as it first appeared. `-i` also affects `countunique`
and `unique`. Under the Turkic character types (`tr_TR`, `az_AZ` and a few
others), where glibc does not fold `i` and `I`, `-i` is refused; set
`LC_CTYPE=C.UTF-8` to fold ASCII letters.

## Pivot tables: `crosstab`

`crosstab KEY1,KEY2` (or `ct`) builds a two-way table with one row per value of the first
key and one column per value of the second. By default each cell is a count;
you can name one operation instead:

```console
$ printf 'a\tx\t1\na\ty\t2\nb\tx\t3\n' | fastmash -s crosstab 1,2 sum 3
	x	y
a	1	2
b	3	N/A
```

Missing combinations show the `--filler` text (default `N/A`). Rows and columns
are ordered byte by byte. Use `-s` unless each combination's records are already
adjacent.
