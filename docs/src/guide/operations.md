# Operations

Operations summarize a field over all records, or over each
[group](grouping.md). Operation names are case-insensitive. Parameters follow a
colon: `perc:90`, `trimmean:0.1`.

For operations that transform each record instead, see
[Per-row operations and modes](modes.md).

## Counting and text

| Operation | Result |
| --- | --- |
| `count` | Number of values, including text and empty fields |
| `countunique` | Number of distinct values (case-sensitive unless `-i`) |
| `unique`, `uniq` | Sorted, comma-separated list of distinct values |
| `collapse` | Comma-separated list of all values, in input order |
| `first` | First value |
| `last` | Last value |
| `rand` | One value chosen at random (reproducible with `--seed`) |

`-c X` (`--collapse-delimiter`) changes the separator used by `unique` and `collapse`.

## Basic statistics

| Operation | Result |
| --- | --- |
| `sum` | Sum |
| `mean` | Arithmetic mean |
| `min`, `max` | Minimum, maximum |
| `absmin`, `absmax` | Value with the smallest or largest magnitude, keeping its sign |
| `range` | `max` minus `min` |

## Other means

| Operation | Result |
| --- | --- |
| `geomean` | Geometric mean |
| `harmmean` | Harmonic mean |
| `ms` | Mean of the squared values |
| `rms` | Root mean square |
| `trimmean[:P]` | Mean after removing the fraction `P` of values from each end (default 0; `0.5` gives the median) |

## Quantiles and order statistics

| Operation | Result |
| --- | --- |
| `median` | Middle value, or the mean of the two middle values |
| `q1`, `q3` | First and third quartiles |
| `iqr` | Interquartile range, `q3` minus `q1` |
| `perc[:N]` | `N`th percentile, for integer `N` from 1 to 100 (default 95) |
| `mode` | Most frequent value (the smallest, on ties) |
| `antimode` | Least frequent value (the smallest, on ties) |

Quartiles and percentiles use linear interpolation between order statistics
(Hyndman and Fan's type 7, the default in R and NumPy).

## Dispersion

| Operation | Result |
| --- | --- |
| `pvar`, `svar` | Population and sample variance |
| `pstdev`, `sstdev` | Population and sample standard deviation |
| `mad` | Median absolute deviation, scaled by 1.4826 |
| `madraw` | Median absolute deviation, unscaled |

## Shape and normality

| Operation | Result |
| --- | --- |
| `pskew`, `sskew` | Population and sample skewness |
| `pkurt`, `skurt` | Population and sample excess kurtosis |
| `jarque` | Jarque–Bera normality test p-value |
| `dpo` | D'Agostino–Pearson omnibus normality test p-value |

## Paired statistics

These take a pair of fields, `LEFT:RIGHT`:

| Operation | Result |
| --- | --- |
| `pcov`, `scov` | Population and sample covariance |
| `ppearson`, `spearson` | Population and sample Pearson correlation coefficient |
| `dotprod` | Dot product |

```console
$ printf '1\t2\n2\t4\n3\t7\n' | fastmash scov 1:2 spearson 1:2
2.5	0.99339926779878
```

`spearson` is the *sample* Pearson coefficient, not Spearman's rank correlation.

## Small samples and missing data

- `jarque` and `dpo` keep GNU datamash's formulas, including its tail
  cancellation: very small p-values can print as `0`.
- Sample statistics need enough values: `svar` and `sstdev` give `nan` for a
  single value; `sskew` needs at least three and `skurt` at least four.
- When every value in a group is missing (with `--narm`), `count` and `sum`
  give `0`; `mean`, `median` and most statistics give `nan`; `min` and `max`
  give `-inf` and `inf`; `first` and `last` give `N/A`.
- An empty input produces no output line.

## Memory use

Most operations keep a running total. Quantiles, modes, dispersion, shape,
normality and paired statistics must keep every value of a group, and text
operations like `unique` and `collapse` keep their text. Memory then grows
with the size of the largest group, and this storage is not spilled to disk.
Related operations on the same field share storage: quantiles, `mode` and
`antimode` share one sorted copy, the variance family another, and the
moment and normality statistics another.
