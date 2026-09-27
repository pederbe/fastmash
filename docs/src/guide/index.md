# User guide

A Fastmash command has this shape:

```text
fastmash [OPTIONS] OPERATION FIELDS [OPERATION FIELDS ...]
fastmash [OPTIONS] -g KEYS OPERATION FIELDS ...
fastmash [OPTIONS] MODE ...
```

Fastmash reads **records** from standard input, splits each into **fields**,
and applies the requested **operations**. It writes results to standard output
and diagnostics to standard error, and reports success or failure through its
[exit status](errors.md).

| Page | Covers |
| --- | --- |
| [Fields and input](input.md) | Field selectors, separators, headers, record terminators, comments, missing values |
| [Operations](operations.md) | Every summary statistic, with parameters and edge cases |
| [Grouping and sorting](grouping.md) | `-g`, `-s`, case-insensitive grouping, `crosstab` |
| [Per-row operations and modes](modes.md) | Rounding, binning, checksums, paths, `cut`, `transpose`, `check`, `rmdup` |
| [Numbers, output and locales](output.md) | Number formats, `--format`, `--round`, locales, portable results |
| [Errors, exit statuses and resources](errors.md) | What can fail, how it is reported, memory and disk use |
| [Differences from GNU datamash](differences.md) | Every intentional difference |

`fastmash --help` prints a complete summary of operations and options.
