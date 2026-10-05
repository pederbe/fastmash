# Fastmash

**Fast command-line statistics for delimited text.**

Fastmash summarizes, groups and reshapes delimited text and quoted CSV. It
accepts the command language of [GNU datamash](https://www.gnu.org/software/datamash/),
so if you already use `datamash`, you already know how to use Fastmash.

It also reads quoted CSV directly, gives your result columns stable names,
calculates weighted means and selects the highest or lowest complete records.
Inspect a table's health or compare summaries from two exports with the same
field selectors and portable arithmetic.

```console
$ cat readings.tsv
site	reading
west	2
east	4
west	6
west	7
$ fastmash -H -s -g site count reading median reading < readings.tsv
GroupBy(site)	count(reading)	median(reading)
east	1	4
west	3	6
```

Install it with one command, or see [other ways](install.md):

```sh
curl -fsSL https://fastmash.io/install.sh | sh
```

## Fastmash is faster than GNU datamash<sup>*</sup>

Up to four times faster, on real jobs: quantiles, grouped summaries and
millions of decimals.

{{#include benchmarks/core-jobs.svg}}

<small>* In 0.1.0, some disk-spilling sorts and the distinct-value geometric-mean
job are slower on native Linux.
[All results and the method](benchmarks/index.md).</small>

## And better in other ways

| Feature | GNU datamash 1.9 | Fastmash |
| --- | --- | --- |
| Results | Depend on the CPU and the C math library | **Identical on every machine**: all arithmetic in software |
| Locales | Need locale data installed; silently fall back to `C` without it | **Built in**: numbers in 318 locales, sorting in 194 |
| `perc:100` | Can read past the end of the values | **Returns the largest value** |
| `rmdup` on several key fields | Crashes on an assertion | **A clear refusal** |
| Malformed parameters such as `strbin:2.0` | Misleading errors or undefined behavior | **An accurate error message** |
| `crosstab` labels over 511 bytes | Truncated | **Kept in full** |

Everything else works the same: over 70 operations and modes, the same
options, field selectors, grouping, headers and output. For most scripts,
switching means changing one word.
[Migrating from datamash](guide/migrating.md) ·
[All differences](guide/differences.md)

## Get started

- [Install Fastmash](install.md) on Linux x86-64 or WSL2
- [Quick start](quick-start.md): a five-minute tour
- [Operations](guide/operations.md): every statistic and transformation
- [Quoted CSV and result names](guide/csv-and-result-names.md): structured input and useful report labels
- [Highest and lowest records](guide/top-n-records.md): keep the records behind the numbers
- [Table health](guide/table-health.md) and [dataset comparison](guide/dataset-comparison.md): inspect data and understand changes

Fastmash is open source under the MIT or Apache-2.0 license.
[Source on GitHub](https://github.com/pederbe/fastmash).
