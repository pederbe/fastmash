# Benchmark method

## Principles

- **One build.** Every number on the results page comes from the same release
  artifact. Results from different versions are never combined.
- **Exact output first.** A timing counts only if Fastmash's output matches
  the expected output for that job.
- **Same conditions.** GNU datamash and Fastmash run on the same host, with the
  same input files, the same locale, output to `/dev/null`, and alternating order.
- **Repeated sessions.** Each job runs in two separate sessions, each with one
  warm-up and six measured repetitions, alternating the programs. We report
  medians and keep the minimum and maximum. Files are in the page cache
  (warm-cache measurements).
- **Resources as well as time.** CPU time and peak charged memory (from cgroups)
  are recorded for every job.

## Representative jobs

Jobs are grouped into **job families**, each representing a kind of work:

| Job family | What it measures | Example job |
| --- | --- | --- |
| Startup | Fixed per-process cost | A tiny input |
| Decimal accumulation | Parsing and summing many numbers | Sum and mean of a million decimals |
| Retained numerical summaries | Operations that keep every value | Quantiles by group |
| Grouped numerical analysis | Grouping with numerical operations | Exon statistics by gene |
| Grouped text and counts | Grouping with text operations | Unique values by key |

Datasets include public real-world data (RefGene genome annotations, the UCI
Wine Quality data) and generated data with controlled shapes. The
[benchmark kit](https://github.com/pederbe/fastmash/tree/main/bench) lists
the core jobs' exact arguments, downloads the public data and regenerates
the synthetic inputs from the same fixed seed.

## Acceptance rules

The rules are fixed before a release candidate is measured:

| Rule | Threshold |
| --- | --- |
| A job counts as a win | At least 20% lower median elapsed time and at least 5 ms saved per run, in both sessions and in at least five of six paired rounds |
| Fastmash is "faster" overall | On each host: wins in at least three of the four non-startup job families, including real-data wins in at least two; no unresolved material slowdowns; small commands within their limits |
| Material slowdown | Median elapsed time more than 10% **and** more than 5 ms longer |
| Small commands | At most 5 ms slower than GNU datamash, and at most 20 ms in total |
| CPU slowdown | More than 20% **and** more than 5 ms extra CPU time |
| Memory review | Peak memory more than twice GNU datamash's **and** 64 MiB more |

A job that fails, times out or gives a wrong answer counts as a failure, not
a result. Every Fastmash version is also compared with the previous version on every
existing job (the *predecessor guards*); a slowdown beyond these limits needs
an explicit, published justification.

## Hosts

| Host | CPU | OS | Notes |
| --- | --- | --- | --- |
| Laptop | Intel Core i7 | CachyOS, native Linux | |
| Desktop | AMD Ryzen 7 | Ubuntu under WSL2 on Windows 11 | |

## Full results

Median elapsed time over the two sessions, for every job and setting in the
0.1.0 measurement (release binary `da311d13…`, 2026-09-27; for sets that were
repeated, the last undisturbed attempt). Every job gave
the expected output on both hosts. Sets: `core` and `supplement` are the
representative jobs; `locale-C`, `locale-en` and `locale-de` repeat
locale-sensitive jobs under `C`, `en_US.UTF-8` and `de_DE.UTF-8`; `guards`
are small edge-case jobs; the `spill` sets sort inputs larger than the sort
memory target. CPU time and peak memory are in the measurement record.

### Intel laptop, native Linux

| Set | Job | GNU datamash 1.9 (ms) | Fastmash 0.1.0 (ms) | GNU / Fastmash |
| --- | --- | ---: | ---: | ---: |
| core | `decimal-1000` | 1.4 | 1.3 | 1.08× |
| core | `decimal-100000` | 35.2 | 14.4 | 2.44× |
| core | `decimal-1000000` | 326.2 | 125.7 | 2.60× |
| core | `disk-spill` | 638.0 | 1363.8 | 0.47× |
| core | `dominant-group` | 21.9 | 20.1 | 1.09× |
| core | `gene-many` | 163.4 | 157.4 | 1.04× |
| core | `gene-tiny` | 2.5 | 1.1 | 2.32× |
| core | `genes-example` | 7.5 | 3.9 | 1.91× |
| core | `grouped-decimal-100000` | 70.1 | 59.0 | 1.19× |
| core | `refgene-exons` | 159.6 | 110.5 | 1.44× |
| core | `refgene-quantiles` | 111.8 | 27.2 | 4.11× |
| core | `refgene-transcripts` | 112.2 | 66.1 | 1.70× |
| core | `scores-by-major` | 2.6 | 1.2 | 2.17× |
| core | `tiny-count` | 1.2 | 1.2 | 0.96× |
| core | `tiny-decimal` | 1.1 | 1.1 | 0.96× |
| core | `wine-by-quality` | 6.4 | 4.4 | 1.45× |
| core | `wine-red-summary` | 1.7 | 1.4 | 1.21× |
| core | `wine-white-summary` | 4.6 | 2.6 | 1.77× |
| disk-spill | `disk-spill` | 581.9 | 1368.8 | 0.43× |
| guards | `distinct-20000-geomean` | 4.8 | 5.7 | 0.84× |
| guards | `distinct-200000-geomean` | 37.1 | 43.2 | 0.86× |
| guards | `extreme-unique-128` | 1.1 | 1.4 | 0.76× |
| guards | `extreme-unique-20000` | 5.0 | 6.8 | 0.73× |
| guards | `missing-pairs` | 3.0 | 3.5 | 0.86× |
| guards | `moments-all-100000` | 68.5 | 28.6 | 2.39× |
| guards | `moments-alternating-100000` | 69.2 | 51.4 | 1.35× |
| guards | `paired-all-100000` | 105.7 | 65.5 | 1.61× |
| guards | `subnormal-unique-128` | 1.1 | 1.4 | 0.82× |
| guards | `subnormal-unique-20000` | 10.4 | 8.2 | 1.27× |
| locale-C | `locale-prepared` | 22.1 | 23.8 | 0.93× |
| locale-C | `locale-scientific` | 41.5 | 43.7 | 0.95× |
| locale-de | `locale-prepared` | 22.3 | 23.7 | 0.94× |
| locale-de | `locale-scientific` | 100.5 | 96.5 | 1.04× |
| locale-en | `locale-prepared` | 22.4 | 23.7 | 0.94× |
| locale-en | `locale-scientific` | 99.6 | 96.2 | 1.04× |
| many-keys | `many-keys` | 781.7 | 1583.6 | 0.49× |
| spill-guards | `alternating-original` | 312.4 | 118.0 | 2.65× |
| spill-guards | `alternating-packed` | 310.8 | 160.2 | 1.94× |
| spill-guards | `gene-many` | 162.1 | 159.6 | 1.02× |
| spill-guards | `refgene-exons` | 159.2 | 109.5 | 1.45× |
| spill-guards | `tiny-count` | 1.1 | 1.2 | 0.92× |
| spill-guards | `wide-original` | 451.4 | 206.2 | 2.19× |
| spill-guards | `wide-packed` | 451.4 | 261.9 | 1.72× |
| spill-half | `spill-half` | 301.5 | 615.0 | 0.49× |
| spill-large | `spill-large` | 893.1 | 2058.6 | 0.43× |
| supplement | `base64-1000` | 1.6 | 1.4 | 1.07× |
| supplement | `base64-100000` | 52.0 | 32.2 | 1.61× |
| supplement | `cross-dense` | 40.7 | 31.6 | 1.29× |
| supplement | `cross-small` | 2.6 | 1.2 | 2.17× |
| supplement | `cross-sparse` | 5.2 | 3.5 | 1.45× |
| supplement | `extract-1000` | 2.0 | 1.6 | 1.25× |
| supplement | `extract-100000` | 97.7 | 52.0 | 1.88× |
| supplement | `hash-1000` | 2.0 | 1.9 | 1.05× |
| supplement | `hash-100000` | 90.9 | 77.8 | 1.17× |
| supplement | `mixed-1000` | 1.8 | 1.9 | 0.95× |
| supplement | `mixed-100000` | 76.4 | 59.2 | 1.29× |
| supplement | `path-1000` | 1.4 | 1.4 | 1.04× |
| supplement | `path-100000` | 44.1 | 29.6 | 1.49× |
| supplement | `rounding-1000` | 3.1 | 2.5 | 1.22× |
| supplement | `rounding-100000` | 189.9 | 127.8 | 1.49× |
| supplement | `table-small` | 1.1 | 1.2 | 0.92× |
| supplement | `table-tall` | 7.2 | 5.2 | 1.36× |
| supplement | `table-wide` | 5.7 | 4.8 | 1.18× |
| supplement | `wine-group-complete` | 7.2 | 7.8 | 0.93× |
| supplement | `wine-group-prepared` | 4.2 | 4.2 | 0.99× |
| supplement | `wine-means` | 3.6 | 2.7 | 1.33× |
| supplement | `wine-moments` | 4.1 | 2.8 | 1.46× |
| supplement | `wine-paired` | 4.2 | 3.5 | 1.20× |
| supplement | `wine10-means` | 25.6 | 14.7 | 1.74× |
| supplement | `wine10-moments` | 30.5 | 16.8 | 1.82× |
| supplement | `wine10-paired` | 32.0 | 24.5 | 1.31× |

### AMD desktop, WSL2

| Set | Job | GNU datamash 1.9 (ms) | Fastmash 0.1.0 (ms) | GNU / Fastmash |
| --- | --- | ---: | ---: | ---: |
| core | `decimal-1000` | 1.2 | 1.2 | 1.00× |
| core | `decimal-100000` | 12.1 | 6.3 | 1.91× |
| core | `decimal-1000000` | 110.7 | 52.1 | 2.12× |
| core | `disk-spill` | 751.6 | 688.7 | 1.09× |
| core | `dominant-group` | 18.5 | 12.9 | 1.44× |
| core | `gene-many` | 161.6 | 74.6 | 2.17× |
| core | `gene-tiny` | 3.5 | 1.1 | 3.23× |
| core | `genes-example` | 9.9 | 2.6 | 3.83× |
| core | `grouped-decimal-100000` | 56.2 | 29.8 | 1.89× |
| core | `refgene-exons` | 158.8 | 62.9 | 2.53× |
| core | `refgene-quantiles` | 57.5 | 15.4 | 3.72× |
| core | `refgene-transcripts` | 108.7 | 36.7 | 2.96× |
| core | `scores-by-major` | 3.5 | 1.1 | 3.09× |
| core | `tiny-count` | 1.1 | 1.1 | 1.00× |
| core | `tiny-decimal` | 1.1 | 1.1 | 1.00× |
| core | `wine-by-quality` | 8.3 | 2.8 | 2.98× |
| core | `wine-red-summary` | 1.3 | 1.3 | 1.00× |
| core | `wine-white-summary` | 2.5 | 1.9 | 1.32× |
| disk-spill | `disk-spill` | 714.4 | 685.6 | 1.04× |
| guards | `distinct-20000-geomean` | 2.8 | 3.0 | 0.92× |
| guards | `distinct-200000-geomean` | 18.2 | 19.6 | 0.93× |
| guards | `extreme-unique-128` | 1.1 | 1.2 | 0.96× |
| guards | `extreme-unique-20000` | 3.0 | 3.8 | 0.78× |
| guards | `missing-pairs` | 3.8 | 5.8 | 0.65× |
| guards | `moments-all-100000` | 28.1 | 11.8 | 2.38× |
| guards | `moments-alternating-100000` | 29.4 | 21.0 | 1.40× |
| guards | `paired-all-100000` | 45.1 | 34.2 | 1.32× |
| guards | `subnormal-unique-128` | 1.1 | 1.3 | 0.81× |
| guards | `subnormal-unique-20000` | 2.4 | 4.2 | 0.57× |
| locale-C | `locale-prepared` | 8.6 | 11.3 | 0.77× |
| locale-C | `locale-scientific` | 32.1 | 31.9 | 1.01× |
| locale-de | `locale-prepared` | 8.9 | 11.1 | 0.80× |
| locale-de | `locale-scientific` | 40.7 | 47.8 | 0.85× |
| locale-en | `locale-prepared` | 8.8 | 11.1 | 0.78× |
| locale-en | `locale-scientific` | 40.3 | 48.2 | 0.84× |
| many-keys | `many-keys` | 973.8 | 970.5 | 1.00× |
| spill-guards | `alternating-original` | 343.9 | 160.6 | 2.14× |
| spill-guards | `alternating-packed` | 331.0 | 177.5 | 1.86× |
| spill-guards | `gene-many` | 173.6 | 81.3 | 2.14× |
| spill-guards | `refgene-exons` | 165.5 | 65.7 | 2.52× |
| spill-guards | `tiny-count` | 1.1 | 1.1 | 1.05× |
| spill-guards | `wide-original` | 465.0 | 195.7 | 2.38× |
| spill-guards | `wide-packed` | 483.5 | 232.5 | 2.08× |
| spill-half | `spill-half` | 413.0 | 408.0 | 1.01× |
| spill-large | `spill-large` | 1136.5 | 1712.8 | 0.66× |
| supplement | `base64-1000` | 1.3 | 1.3 | 1.00× |
| supplement | `base64-100000` | 27.4 | 20.6 | 1.34× |
| supplement | `cross-dense` | 23.5 | 17.2 | 1.37× |
| supplement | `cross-small` | 3.5 | 1.1 | 3.09× |
| supplement | `cross-sparse` | 4.8 | 2.3 | 2.11× |
| supplement | `extract-1000` | 1.4 | 1.3 | 1.08× |
| supplement | `extract-100000` | 39.5 | 26.6 | 1.48× |
| supplement | `hash-1000` | 1.5 | 1.2 | 1.20× |
| supplement | `hash-100000` | 44.1 | 18.8 | 2.35× |
| supplement | `mixed-1000` | 1.4 | 1.5 | 0.90× |
| supplement | `mixed-100000` | 32.5 | 27.2 | 1.19× |
| supplement | `path-1000` | 1.2 | 1.2 | 1.04× |
| supplement | `path-100000` | 22.2 | 17.9 | 1.24× |
| supplement | `rounding-1000` | 1.9 | 1.8 | 1.08× |
| supplement | `rounding-100000` | 87.8 | 72.5 | 1.21× |
| supplement | `table-small` | 1.1 | 1.1 | 1.00× |
| supplement | `table-tall` | 4.6 | 3.3 | 1.37× |
| supplement | `table-wide` | 3.7 | 3.2 | 1.14× |
| supplement | `wine-group-complete` | 8.4 | 9.1 | 0.93× |
| supplement | `wine-group-prepared` | 2.3 | 2.6 | 0.90× |
| supplement | `wine-means` | 2.0 | 1.8 | 1.17× |
| supplement | `wine-moments` | 2.2 | 1.9 | 1.16× |
| supplement | `wine-paired` | 2.3 | 2.3 | 1.00× |
| supplement | `wine10-means` | 10.0 | 6.8 | 1.48× |
| supplement | `wine10-moments` | 12.5 | 8.5 | 1.47× |
| supplement | `wine10-paired` | 13.3 | 12.1 | 1.10× |
