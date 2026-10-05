# User guide

A Fastmash command has this shape:

```text
fastmash [OPTIONS] OPERATION FIELDS [OPERATION FIELDS ...]
fastmash [OPTIONS] -g KEYS OPERATION FIELDS ...
fastmash [OPTIONS] MODE ...
fastmash [OPTIONS] top:N FIELD
fastmash [OPTIONS] compare BEFORE AFTER [rank RESULT [absolute|percent] [limit N]] OPERATION FIELDS ...
fastmash [INPUT OPTIONS] health [CONTROLS]
```

Fastmash reads **records** from standard input, splits each into **fields**,
and applies the requested **operations**. It writes results to standard output
and diagnostics to standard error, and reports success or failure through its
[exit status](errors.md).

| Page | Covers |
| --- | --- |
| [Fields and input](input.md) | Field selectors, separators, headers, record terminators, comments, missing values |
| [Quoted CSV and result names](csv-and-result-names.md) | Explicit CSV formats, quoting rules and stable output labels |
| [Operations](operations.md) | Every summary statistic, with parameters and edge cases |
| [Weighted mean](weighted-mean.md) | Contribution weights, complete-pair omission and numerical limits |
| [Grouping and sorting](grouping.md) | `-g`, `-s`, case-insensitive grouping, `crosstab` |
| [Highest and lowest records](top-n-records.md) | Complete-record selection, stable ties, groups and capacity limits |
| [Dataset comparison](dataset-comparison.md) | Before/after summaries, global key alignment and change ranking |
| [Table health reports](table-health.md) | Inspection, supplied validation rules and versioned TSV output |
| [Per-row operations and modes](modes.md) | Rounding, binning, checksums, paths, `cut`, `transpose`, `check`, `rmdup` |
| [Numbers, output and locales](output.md) | Number formats, `--format`, `--round`, locales, portable results |
| [Terminal color](terminal-color.md) | Help, health and failure styling, detection and suppression |
| [Errors, exit statuses and resources](errors.md) | What can fail, how it is reported, memory and disk use |
| [Differences from GNU datamash](differences.md) | Every intentional difference |

`fastmash --help` prints a complete summary of operations and options.
