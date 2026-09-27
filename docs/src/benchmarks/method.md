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
0.1.0 measurement (release binary `574da941…`, 2026-09-26). Every job gave
the expected output on both hosts. Sets: `core` and `supplement` are the
representative jobs; `locale-C`, `locale-en` and `locale-de` repeat
locale-sensitive jobs under `C`, `en_US.UTF-8` and `de_DE.UTF-8`; `guards`
are small edge-case jobs; the `spill` sets sort inputs larger than the sort
memory target. CPU time and peak memory are in the measurement record.

### Intel laptop, native Linux

| Set | Job | GNU datamash 1.9 (ms) | Fastmash 0.1.0 (ms) | GNU / Fastmash |
| --- | --- | ---: | ---: | ---: |
| core | `decimal-1000` | 1.6 | 1.4 | 1.18× |
| core | `decimal-100000` | 37.1 | 14.1 | 2.62× |
| core | `decimal-1000000` | 322.4 | 122.4 | 2.63× |
| core | `disk-spill` | 569.7 | 1311.0 | 0.43× |
| core | `dominant-group` | 21.9 | 19.9 | 1.10× |
| core | `gene-many` | 162.4 | 154.4 | 1.05× |
| core | `gene-tiny` | 2.6 | 1.1 | 2.36× |
| core | `genes-example` | 7.3 | 3.8 | 1.92× |
| core | `grouped-decimal-100000` | 69.3 | 58.6 | 1.18× |
| core | `refgene-exons` | 158.4 | 107.8 | 1.47× |
| core | `refgene-quantiles` | 110.8 | 26.9 | 4.12× |
| core | `refgene-transcripts` | 111.9 | 65.9 | 1.70× |
| core | `scores-by-major` | 2.5 | 1.2 | 2.12× |
| core | `tiny-count` | 1.2 | 1.3 | 0.92× |
| core | `tiny-decimal` | 1.2 | 1.3 | 0.92× |
| core | `wine-by-quality` | 6.3 | 4.3 | 1.48× |
| core | `wine-red-summary` | 1.6 | 1.4 | 1.18× |
| core | `wine-white-summary` | 4.7 | 2.6 | 1.79× |
| disk-spill | `disk-spill` | 575.9 | 1341.2 | 0.43× |
| guards | `distinct-20000-geomean` | 4.8 | 5.7 | 0.84× |
| guards | `distinct-200000-geomean` | 36.8 | 43.5 | 0.85× |
| guards | `extreme-unique-128` | 1.1 | 1.4 | 0.76× |
| guards | `extreme-unique-20000` | 4.8 | 6.8 | 0.71× |
| guards | `missing-pairs` | 3.0 | 3.3 | 0.91× |
| guards | `moments-all-100000` | 68.0 | 28.3 | 2.40× |
| guards | `moments-alternating-100000` | 68.9 | 50.8 | 1.36× |
| guards | `paired-all-100000` | 105.8 | 65.2 | 1.62× |
| guards | `subnormal-unique-128` | 1.1 | 1.4 | 0.79× |
| guards | `subnormal-unique-20000` | 10.4 | 8.1 | 1.28× |
| locale-C | `locale-prepared` | 22.1 | 23.7 | 0.93× |
| locale-C | `locale-scientific` | 41.6 | 44.5 | 0.93× |
| locale-de | `locale-prepared` | 22.3 | 23.7 | 0.94× |
| locale-de | `locale-scientific` | 100.7 | 96.4 | 1.04× |
| locale-en | `locale-prepared` | 22.2 | 23.8 | 0.93× |
| locale-en | `locale-scientific` | 99.6 | 95.8 | 1.04× |
| many-keys | `many-keys` | 869.6 | 2617.8 | 0.33× |
| spill-guards | `alternating-original` | 307.5 | 118.8 | 2.59× |
| spill-guards | `alternating-packed` | 309.1 | 155.7 | 1.99× |
| spill-guards | `gene-many` | 162.9 | 157.0 | 1.04× |
| spill-guards | `refgene-exons` | 158.9 | 108.4 | 1.47× |
| spill-guards | `tiny-count` | 1.1 | 1.2 | 0.92× |
| spill-guards | `wide-original` | 447.2 | 200.3 | 2.23× |
| spill-guards | `wide-packed` | 446.9 | 260.2 | 1.72× |
| spill-half | `spill-half` | 308.0 | 604.7 | 0.51× |
| spill-large | `spill-large` | 1136.3 | 3167.2 | 0.36× |
| supplement | `base64-1000` | 1.5 | 1.4 | 1.07× |
| supplement | `base64-100000` | 52.5 | 32.8 | 1.60× |
| supplement | `cross-dense` | 40.3 | 31.5 | 1.28× |
| supplement | `cross-small` | 2.5 | 1.2 | 2.12× |
| supplement | `cross-sparse` | 5.2 | 3.5 | 1.45× |
| supplement | `extract-1000` | 2.0 | 1.6 | 1.25× |
| supplement | `extract-100000` | 97.3 | 51.8 | 1.88× |
| supplement | `hash-1000` | 1.9 | 1.9 | 1.00× |
| supplement | `hash-100000` | 90.0 | 77.6 | 1.16× |
| supplement | `mixed-1000` | 1.8 | 1.9 | 0.97× |
| supplement | `mixed-100000` | 76.2 | 58.0 | 1.31× |
| supplement | `path-1000` | 1.4 | 1.4 | 1.04× |
| supplement | `path-100000` | 43.5 | 30.1 | 1.44× |
| supplement | `rounding-1000` | 3.1 | 2.5 | 1.27× |
| supplement | `rounding-100000` | 188.8 | 127.1 | 1.49× |
| supplement | `table-small` | 1.1 | 1.1 | 0.96× |
| supplement | `table-tall` | 7.1 | 5.2 | 1.37× |
| supplement | `table-wide` | 5.8 | 4.8 | 1.22× |
| supplement | `wine-group-complete` | 7.2 | 7.5 | 0.95× |
| supplement | `wine-group-prepared` | 4.2 | 4.2 | 0.99× |
| supplement | `wine-means` | 3.5 | 2.7 | 1.31× |
| supplement | `wine-moments` | 4.1 | 2.8 | 1.46× |
| supplement | `wine-paired` | 4.2 | 3.5 | 1.19× |
| supplement | `wine10-means` | 25.5 | 14.6 | 1.75× |
| supplement | `wine10-moments` | 30.4 | 16.6 | 1.83× |
| supplement | `wine10-paired` | 31.9 | 24.2 | 1.32× |

### AMD desktop, WSL2

| Set | Job | GNU datamash 1.9 (ms) | Fastmash 0.1.0 (ms) | GNU / Fastmash |
| --- | --- | ---: | ---: | ---: |
| core | `decimal-1000` | 1.3 | 1.3 | 1.00× |
| core | `decimal-100000` | 12.4 | 6.7 | 1.87× |
| core | `decimal-1000000` | 115.4 | 53.0 | 2.18× |
| core | `disk-spill` | 795.0 | 795.8 | 1.00× |
| core | `dominant-group` | 20.2 | 13.1 | 1.54× |
| core | `gene-many` | 174.6 | 83.7 | 2.09× |
| core | `gene-tiny` | 3.9 | 1.2 | 3.12× |
| core | `genes-example` | 11.4 | 2.9 | 3.93× |
| core | `grouped-decimal-100000` | 59.7 | 32.4 | 1.84× |
| core | `refgene-exons` | 165.6 | 71.8 | 2.31× |
| core | `refgene-quantiles` | 61.8 | 15.8 | 3.91× |
| core | `refgene-transcripts` | 112.8 | 39.9 | 2.83× |
| core | `scores-by-major` | 3.8 | 1.2 | 3.17× |
| core | `tiny-count` | 1.2 | 1.3 | 0.92× |
| core | `tiny-decimal` | 1.2 | 1.2 | 1.00× |
| core | `wine-by-quality` | 8.9 | 2.9 | 3.07× |
| core | `wine-red-summary` | 1.4 | 1.3 | 1.08× |
| core | `wine-white-summary` | 2.6 | 2.0 | 1.30× |
| disk-spill | `disk-spill` | 716.0 | 720.9 | 0.99× |
| guards | `distinct-20000-geomean` | 2.9 | 3.2 | 0.92× |
| guards | `distinct-200000-geomean` | 18.6 | 20.2 | 0.92× |
| guards | `extreme-unique-128` | 1.2 | 1.4 | 0.89× |
| guards | `extreme-unique-20000` | 3.0 | 4.0 | 0.76× |
| guards | `missing-pairs` | 4.1 | 5.0 | 0.82× |
| guards | `moments-all-100000` | 28.9 | 12.0 | 2.40× |
| guards | `moments-alternating-100000` | 31.4 | 21.4 | 1.47× |
| guards | `paired-all-100000` | 46.4 | 35.2 | 1.32× |
| guards | `subnormal-unique-128` | 1.2 | 1.3 | 0.92× |
| guards | `subnormal-unique-20000` | 2.5 | 4.3 | 0.58× |
| locale-C | `locale-prepared` | 9.4 | 12.4 | 0.76× |
| locale-C | `locale-scientific` | 36.3 | 74.7 | 0.49× |
| locale-de | `locale-prepared` | 8.9 | 11.4 | 0.79× |
| locale-de | `locale-scientific` | 42.4 | 51.0 | 0.83× |
| locale-en | `locale-prepared` | 8.9 | 11.3 | 0.79× |
| locale-en | `locale-scientific` | 41.0 | 47.8 | 0.86× |
| many-keys | `many-keys` | 911.4 | 895.5 | 1.02× |
| spill-guards | `alternating-original` | 309.2 | 106.2 | 2.91× |
| spill-guards | `alternating-packed` | 301.8 | 153.2 | 1.97× |
| spill-guards | `gene-many` | 164.2 | 76.5 | 2.15× |
| spill-guards | `refgene-exons` | 153.4 | 65.2 | 2.35× |
| spill-guards | `tiny-count` | 1.2 | 1.2 | 1.00× |
| spill-guards | `wide-original` | 432.4 | 155.4 | 2.78× |
| spill-guards | `wide-packed` | 440.4 | 218.6 | 2.01× |
| spill-half | `spill-half` | 390.5 | 350.0 | 1.12× |
| spill-large | `spill-large` | 1122.0 | 1278.1 | 0.88× |
| supplement | `base64-1000` | 1.6 | 1.8 | 0.91× |
| supplement | `base64-100000` | 34.7 | 27.2 | 1.28× |
| supplement | `cross-dense` | 30.2 | 22.9 | 1.32× |
| supplement | `cross-small` | 4.3 | 1.4 | 3.19× |
| supplement | `cross-sparse` | 5.7 | 2.8 | 2.02× |
| supplement | `extract-1000` | 1.9 | 1.7 | 1.09× |
| supplement | `extract-100000` | 51.1 | 35.0 | 1.46× |
| supplement | `hash-1000` | 1.7 | 1.6 | 1.06× |
| supplement | `hash-100000` | 52.8 | 21.9 | 2.42× |
| supplement | `mixed-1000` | 1.8 | 1.9 | 0.95× |
| supplement | `mixed-100000` | 38.2 | 32.9 | 1.16× |
| supplement | `path-1000` | 1.6 | 1.6 | 0.97× |
| supplement | `path-100000` | 29.8 | 23.3 | 1.28× |
| supplement | `rounding-1000` | 2.6 | 2.2 | 1.18× |
| supplement | `rounding-100000` | 110.3 | 85.1 | 1.30× |
| supplement | `table-small` | 1.2 | 1.4 | 0.86× |
| supplement | `table-tall` | 6.2 | 4.0 | 1.58× |
| supplement | `table-wide` | 5.2 | 4.0 | 1.30× |
| supplement | `wine-group-complete` | 10.9 | 10.3 | 1.05× |
| supplement | `wine-group-prepared` | 2.8 | 3.1 | 0.89× |
| supplement | `wine-means` | 2.5 | 2.2 | 1.14× |
| supplement | `wine-moments` | 2.9 | 2.3 | 1.26× |
| supplement | `wine-paired` | 2.9 | 2.5 | 1.14× |
| supplement | `wine10-means` | 11.2 | 7.8 | 1.43× |
| supplement | `wine10-moments` | 16.2 | 9.9 | 1.64× |
| supplement | `wine10-paired` | 15.8 | 15.3 | 1.03× |
