# Grouping and sorting

## Groups

`-g KEYS` (also written `groupby KEYS`, `grouping KEYS` or `gb KEYS`) makes
Fastmash summarize each **group** of records that share the same key values.
Output rows start with the key values, followed by the results:

```console
$ printf 'A\tx\t1\nA\tx\t2\nA\ty\t5\nB\ty\t4\n' | fastmash -g 1,2 sum 3
Grouped summary
────────────────────────────────
  field-1  field-2  sum(field-3)
        A        x             3
        A        y             5
        B        y             4
✔ Completed
```

A group is a run of **adjacent** records with equal keys. When the key changes,
the group ends and its results are printed. This makes grouping a single pass
over the input, with memory proportional to one group.

If equal keys are not adjacent, the same key appears in more than one group:

```console
$ printf 'b\t2\na\t1\nb\t3\n' | fastmash -g 1 sum 2
Grouped summary
────────────────────────────────
  field-1  sum(field-2)
        b             2
        a             1
        b             3
✔ Completed
```

## Sorting first: `-s`

`-s` sorts the records by their keys before grouping, so every key forms one group:

```console
$ printf 'b\t2\na\t1\nb\t3\n' | fastmash -s -g 1 sum 2
Grouped summary
────────────────────────────────
  field-1  sum(field-2)
        a             1
        b             5
✔ Completed
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

Fastmash sorts inside its own executable. It holds a sort buffer in memory and
writes sorted runs to temporary files when needed. `TMPDIR` selects their
directory, with `/tmp` as the default. The operating system removes the files
when Fastmash exits, including after interruption.

For eligible input with repeated keys, hash grouping collects operation state
per key and sorts only the groups. If this cannot preserve sorted behavior, it
rereads the input and sorts it before writing results. Language-locale pipes
can retain input for that replay. C-locale pipes normally sort directly.
File and pipe input can therefore have different time and memory costs.
A file that changes between readings contributes its second-reading contents.

Sorting can spill, but the values retained by statistics such as quantiles stay
in memory. A sort-memory target is not a total-process memory limit. See
[memory and disk requirements](errors.md#memory).

### Resource controls

`FASTMASH_SORT_MEMORY_BYTES` sets the sort-buffer memory target, and
`FASTMASH_SORT_TRACE` (set to anything) reports each decision on standard
error. Neither is a limit on Fastmash's total memory use.

`FASTMASH_GROUPING=sort` always sorts. For piped input,
`FASTMASH_PIPE_GROUPING=sort` selects sorting and `=hash` permits hash grouping
in every supported locale. `--vnlog` and `rand` always sort. These controls
change the route, not the result.

When the selected calculations support parallel sorting, records are prepared
on up to 8 threads. On Linux, numerical calculations
can merge spilled records and calculate in parallel on two threads. Both use
the processors available to Fastmash, or `OMP_NUM_THREADS` if it is set,
capped by `OMP_THREAD_LIMIT`. Set `OMP_NUM_THREADS=1` to sort on one thread.

Sorted `rmdup` keeps complete records and stable key order, with the same
temporary-storage policy. Case folding affects ordering, while duplicate keys
are compared exactly. The [architecture](https://github.com/pederbe/fastmash/blob/main/ARCHITECTURE.md#sorting)
describes buffer sizing, parallel preparation and replay eligibility.

## Ignoring case: `-i`

`-i` compares keys ignoring ASCII letter case, for grouping and sorting. The
output shows each key as it first appeared. `-i` also affects `countunique`
and `unique`. Under the Turkic character types (`tr_TR`, `az_AZ` and a few
others), where glibc does not fold `i` and `I`, `-i` is refused. Set
`LC_CTYPE=C.UTF-8` to fold ASCII letters.

## Pivot tables: `crosstab`

`crosstab KEY1,KEY2` (or `ct`) builds a two-way table with one row per value of the first
key and one column per value of the second. By default each cell is a count. You can name one operation instead:

```console
$ printf 'a\tx\t1\na\ty\t2\nb\tx\t3\n' | fastmash -s crosstab 1,2 sum 3
	x	y
a	1	2
b	3	N/A
```

Missing combinations show the `--filler` text (default `N/A`). Rows and columns
are ordered byte by byte. Use `-s` unless each combination's records are already
adjacent.
