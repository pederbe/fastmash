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
`en_US.UTF-8`, `fr_FR.UTF-8` or `pl_PL.UTF-8`, keys are ordered alphabetically,
as GNU `sort` orders them, using built-in Unicode collation rules and glibc's
table of the punctuation and symbols it ignores.

If your input is already sorted, leave out `-s`: the result is the same and
the job is faster.

### Large inputs

When standard input is a file (`fastmash -s -g 1 sum 2 < data.tsv`) and its
keys repeat, Fastmash does not sort the records at all: it collects each
group's records as they arrive and sorts only the groups, which is faster and
needs memory only for the groups (hash grouping). The result is the same as
sorting's. Where it could differ, for example when a record lacks a key field
or a value is invalid, or where the groups turn out to be too many, Fastmash
reads the file again and sorts it, so the output and any error are exactly
what the sort gives.

Input from a pipe (`cut -f 1,3 data.tsv | fastmash -s -g 1 sum 2`) is grouped
the same way in a language locale such as `en_US.UTF-8`: Fastmash keeps what
it reads in memory, as the sort would, so that it can sort it if it has to. A
job that gives up late, near the end of a large input, then takes somewhat
longer than sorting would have. In `C` and `C.UTF-8`, piped input is sorted,
which needs less memory there. `FASTMASH_PIPE_GROUPING=sort` sorts piped
input in every locale, and `=hash` groups it in every locale where Fastmash
sorts in process. With `-W`, input whose columns are separated by the same
blanks for each value is grouped this way too; where the same key comes with
different blanks before it, Fastmash sorts. `--vnlog` and `rand` always sort;
`FASTMASH_GROUPING=sort` makes every job sort.

Otherwise Fastmash sorts in memory, and when the data is large it writes sorted chunks
to temporary files in `TMPDIR` (default `/tmp`) and merges them. The files are
anonymous: the operating system removes them automatically, even if Fastmash
is interrupted.

A chunk holds each record's grouping keys and the fields its operations use
(the whole record with `--full`, in language locales, or when the field
separator is a letter, a digit or the decimal point, which a number could run
on into) and 32 bytes per record for sorting. A sort starts with a 64 MiB
chunk. When it fills, Fastmash holds the whole input in memory instead, if
that fits: within the buffer GNU `sort` would use for an input file, a quarter
of the available memory, a share of what is free under any cgroup memory limit
(one per processor) or on the host (one per running Fastmash), and with less
than half the swap in use. For piped input it doubles the chunk each time it
fills, within the same bounds. Otherwise it spills. In language locales the
records are prepared for sorting on several threads, whose batches take a
sixteenth of the first 64 MiB.

`FASTMASH_SORT_MEMORY_BYTES` fixes the chunk's memory target instead, and
`FASTMASH_SORT_TRACE` (set to anything) reports each decision on standard
error. Neither is a limit on Fastmash's total memory use.

In language locales, records are prepared for sorting on up to 8 threads,
chosen as GNU `sort` chooses its default: the processors available to
Fastmash, or `OMP_NUM_THREADS` if it is set, capped by `OMP_THREAD_LIMIT`.
Set `OMP_NUM_THREADS=1` to sort on one thread.

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
