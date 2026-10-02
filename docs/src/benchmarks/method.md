# Benchmark method

## Principles

- **One build.** Every number on the results page comes from the same release
  artifact. Results from different versions are never combined.
- **Exact output first.** A timing counts only if Fastmash's output matches
  the expected output for that job.
- **Same conditions.** GNU datamash and Fastmash run on the same host, with the
  same input files and locale, and alternating order. Each invocation's
  output is captured in files on the same filesystem and checked for equality.
  Input is redirected from regular files; piped input can perform differently.
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
[benchmark kit](https://github.com/pederbe/fastmash/tree/main/bench) runs
eight of the core jobs with their exact arguments, downloads the public data
and regenerates the synthetic inputs from the same fixed seed.

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

| Host | CPU | OS | System `sort` |
| --- | --- | --- | --- |
| Laptop | Intel Core i7 | CachyOS, native Linux | GNU coreutils 9.11 |
| Desktop | AMD Ryzen 7 | Ubuntu under WSL2 on Windows 11 | uutils coreutils 0.8.0 |

GNU datamash's `-s` jobs sort with the system `sort`, so its speed is part of
their times.

Each complete command and its children run with a 512 MiB cgroup memory
limit, no swap, at most 64 processes and a 30-second timeout. Temporary files
are on disk-backed Linux storage. Both programs get the same limits;
Fastmash's sort memory override is unset. These conditions matter for large
sorts: the memory available to a command affects when it writes runs to disk.
Separate, untimed observations confirm that the `disk-spill` and `spill-large`
jobs write temporary runs on both hosts under these limits and still produce
the expected output.

## Full results

Mean of the two sessions' median elapsed times, for every job and setting in the
0.1.0 measurement (release binary `6717db00…`, 2026-10-01). Every job gave
the expected output on both hosts. Sets: `core` and `supplement` are the
representative jobs; `locale-C`, `locale-en` and `locale-de` repeat
locale-sensitive jobs under `C`, `en_US.UTF-8` and `de_DE.UTF-8`; `guards`
are small edge-case jobs; the `spill` sets exercise larger sorts and their
boundaries. CPU time and peak memory are in the measurement record.
Times are rounded to 0.1 ms; the displayed ratios use those rounded values.

### Intel laptop, native Linux

| Set | Job | GNU datamash 1.9 (ms) | Fastmash 0.1.0 (ms) | GNU / Fastmash |
| --- | --- | ---: | ---: | ---: |
| core | `decimal-1000` | 1.6 | 1.6 | 1.00× |
| core | `decimal-100000` | 36.9 | 14.4 | 2.56× |
| core | `decimal-1000000` | 317.7 | 119.1 | 2.67× |
| core | `disk-spill` | 569.5 | 680.2 | 0.84× |
| core | `dominant-group` | 22.0 | 11.6 | 1.90× |
| core | `gene-many` | 159.9 | 99.8 | 1.60× |
| core | `gene-tiny` | 2.6 | 1.3 | 2.00× |
| core | `genes-example` | 7.3 | 3.8 | 1.92× |
| core | `grouped-decimal-100000` | 69.0 | 37.5 | 1.84× |
| core | `refgene-exons` | 158.9 | 79.8 | 1.99× |
| core | `refgene-quantiles` | 111.0 | 26.2 | 4.24× |
| core | `refgene-transcripts` | 111.6 | 59.3 | 1.88× |
| core | `scores-by-major` | 2.7 | 1.4 | 1.93× |
| core | `tiny-count` | 1.2 | 1.5 | 0.80× |
| core | `tiny-decimal` | 1.2 | 1.4 | 0.86× |
| core | `wine-by-quality` | 6.3 | 3.1 | 2.03× |
| core | `wine-red-summary` | 1.6 | 1.6 | 1.00× |
| core | `wine-white-summary` | 4.7 | 2.8 | 1.68× |
| disk-spill | `disk-spill` | 590.1 | 687.8 | 0.86× |
| guards | `distinct-20000-geomean` | 4.8 | 5.8 | 0.83× |
| guards | `distinct-200000-geomean` | 37.0 | 43.8 | 0.84× |
| guards | `extreme-unique-128` | 1.1 | 1.4 | 0.79× |
| guards | `extreme-unique-20000` | 4.9 | 7.1 | 0.69× |
| guards | `missing-pairs` | 3.0 | 1.7 | 1.76× |
| guards | `moments-all-100000` | 68.0 | 29.0 | 2.34× |
| guards | `moments-alternating-100000` | 68.8 | 51.3 | 1.34× |
| guards | `paired-all-100000` | 106.0 | 66.3 | 1.60× |
| guards | `subnormal-unique-128` | 1.1 | 1.6 | 0.69× |
| guards | `subnormal-unique-20000` | 10.4 | 8.7 | 1.20× |
| locale-C | `locale-prepared` | 22.0 | 23.9 | 0.92× |
| locale-C | `locale-scientific` | 41.7 | 29.8 | 1.40× |
| locale-de | `locale-prepared` | 22.4 | 23.9 | 0.94× |
| locale-de | `locale-scientific` | 101.0 | 29.9 | 3.38× |
| locale-en | `locale-prepared` | 22.4 | 23.9 | 0.94× |
| locale-en | `locale-scientific` | 101.0 | 30.0 | 3.37× |
| many-keys | `many-keys` | 777.6 | 904.9 | 0.86× |
| spill-guards | `alternating-original` | 302.8 | 26.5 | 11.43× |
| spill-guards | `alternating-packed` | 304.2 | 30.6 | 9.94× |
| spill-guards | `gene-many` | 158.6 | 98.2 | 1.62× |
| spill-guards | `refgene-exons` | 158.1 | 80.0 | 1.98× |
| spill-guards | `tiny-count` | 1.1 | 1.3 | 0.85× |
| spill-guards | `wide-original` | 440.1 | 62.4 | 7.05× |
| spill-guards | `wide-packed` | 442.2 | 242.7 | 1.82× |
| spill-half | `spill-half` | 296.1 | 243.6 | 1.22× |
| spill-large | `spill-large` | 936.7 | 1088.8 | 0.86× |
| supplement | `base64-1000` | 1.5 | 1.6 | 0.94× |
| supplement | `base64-100000` | 51.5 | 31.4 | 1.64× |
| supplement | `cross-dense` | 40.2 | 35.1 | 1.15× |
| supplement | `cross-small` | 2.6 | 1.4 | 1.86× |
| supplement | `cross-sparse` | 5.1 | 3.8 | 1.34× |
| supplement | `extract-1000` | 2.0 | 1.7 | 1.18× |
| supplement | `extract-100000` | 98.2 | 51.8 | 1.90× |
| supplement | `hash-1000` | 2.0 | 2.0 | 1.00× |
| supplement | `hash-100000` | 90.8 | 77.9 | 1.17× |
| supplement | `mixed-1000` | 1.8 | 2.0 | 0.90× |
| supplement | `mixed-100000` | 76.7 | 59.9 | 1.28× |
| supplement | `path-1000` | 1.4 | 1.6 | 0.87× |
| supplement | `path-100000` | 43.5 | 30.1 | 1.45× |
| supplement | `rounding-1000` | 3.1 | 2.6 | 1.19× |
| supplement | `rounding-100000` | 189.9 | 126.2 | 1.50× |
| supplement | `table-small` | 1.1 | 1.3 | 0.85× |
| supplement | `table-tall` | 7.0 | 4.9 | 1.43× |
| supplement | `table-wide` | 5.7 | 4.5 | 1.27× |
| supplement | `wine-group-complete` | 7.2 | 4.2 | 1.71× |
| supplement | `wine-group-prepared` | 4.2 | 3.9 | 1.08× |
| supplement | `wine-means` | 3.5 | 2.8 | 1.25× |
| supplement | `wine-moments` | 4.0 | 3.0 | 1.33× |
| supplement | `wine-paired` | 4.2 | 3.8 | 1.11× |
| supplement | `wine10-means` | 25.5 | 14.5 | 1.76× |
| supplement | `wine10-moments` | 30.4 | 16.6 | 1.83× |
| supplement | `wine10-paired` | 32.0 | 24.6 | 1.30× |

### AMD desktop, WSL2

| Set | Job | GNU datamash 1.9 (ms) | Fastmash 0.1.0 (ms) | GNU / Fastmash |
| --- | --- | ---: | ---: | ---: |
| core | `decimal-1000` | 1.4 | 1.5 | 0.93× |
| core | `decimal-100000` | 12.2 | 6.5 | 1.88× |
| core | `decimal-1000000` | 111.0 | 50.1 | 2.22× |
| core | `disk-spill` | 771.4 | 432.3 | 1.78× |
| core | `dominant-group` | 19.1 | 6.3 | 3.03× |
| core | `gene-many` | 169.3 | 48.7 | 3.48× |
| core | `gene-tiny` | 4.0 | 1.4 | 2.86× |
| core | `genes-example` | 10.7 | 3.0 | 3.57× |
| core | `grouped-decimal-100000` | 55.4 | 18.5 | 2.99× |
| core | `refgene-exons` | 161.3 | 42.7 | 3.78× |
| core | `refgene-quantiles` | 58.6 | 14.8 | 3.96× |
| core | `refgene-transcripts` | 112.1 | 33.2 | 3.38× |
| core | `scores-by-major` | 3.8 | 1.6 | 2.37× |
| core | `tiny-count` | 1.3 | 1.6 | 0.81× |
| core | `tiny-decimal` | 1.2 | 1.4 | 0.86× |
| core | `wine-by-quality` | 8.6 | 2.5 | 3.44× |
| core | `wine-red-summary` | 1.4 | 1.6 | 0.87× |
| core | `wine-white-summary` | 2.5 | 2.3 | 1.09× |
| disk-spill | `disk-spill` | 769.6 | 409.1 | 1.88× |
| guards | `distinct-20000-geomean` | 3.0 | 3.3 | 0.91× |
| guards | `distinct-200000-geomean` | 18.5 | 20.0 | 0.93× |
| guards | `extreme-unique-128` | 1.2 | 1.5 | 0.80× |
| guards | `extreme-unique-20000` | 3.1 | 4.3 | 0.72× |
| guards | `missing-pairs` | 4.2 | 1.6 | 2.62× |
| guards | `moments-all-100000` | 28.6 | 12.9 | 2.22× |
| guards | `moments-alternating-100000` | 29.9 | 22.5 | 1.33× |
| guards | `paired-all-100000` | 46.0 | 36.2 | 1.27× |
| guards | `subnormal-unique-128` | 1.2 | 1.6 | 0.75× |
| guards | `subnormal-unique-20000` | 2.5 | 4.7 | 0.53× |
| locale-C | `locale-prepared` | 8.7 | 11.8 | 0.74× |
| locale-C | `locale-scientific` | 31.2 | 14.6 | 2.14× |
| locale-de | `locale-prepared` | 8.9 | 11.8 | 0.75× |
| locale-de | `locale-scientific` | 38.7 | 14.6 | 2.65× |
| locale-en | `locale-prepared` | 8.9 | 11.8 | 0.75× |
| locale-en | `locale-scientific` | 39.2 | 14.7 | 2.67× |
| many-keys | `many-keys` | 868.7 | 533.1 | 1.63× |
| spill-guards | `alternating-original` | 311.7 | 16.2 | 19.24× |
| spill-guards | `alternating-packed` | 306.9 | 17.7 | 17.34× |
| spill-guards | `gene-many` | 156.6 | 46.8 | 3.35× |
| spill-guards | `refgene-exons` | 155.7 | 41.1 | 3.79× |
| spill-guards | `tiny-count` | 1.1 | 1.4 | 0.79× |
| spill-guards | `wide-original` | 436.8 | 32.2 | 13.57× |
| spill-guards | `wide-packed` | 443.8 | 218.8 | 2.03× |
| spill-half | `spill-half` | 442.6 | 202.6 | 2.18× |
| spill-large | `spill-large` | 1050.4 | 597.8 | 1.76× |
| supplement | `base64-1000` | 1.4 | 1.6 | 0.87× |
| supplement | `base64-100000` | 29.1 | 23.9 | 1.22× |
| supplement | `cross-dense` | 25.1 | 20.8 | 1.21× |
| supplement | `cross-small` | 4.2 | 1.5 | 2.80× |
| supplement | `cross-sparse` | 5.2 | 2.8 | 1.86× |
| supplement | `extract-1000` | 1.6 | 1.6 | 1.00× |
| supplement | `extract-100000` | 40.0 | 24.7 | 1.62× |
| supplement | `hash-1000` | 1.6 | 1.6 | 1.00× |
| supplement | `hash-100000` | 48.3 | 21.1 | 2.29× |
| supplement | `mixed-1000` | 1.4 | 1.8 | 0.78× |
| supplement | `mixed-100000` | 34.5 | 29.8 | 1.16× |
| supplement | `path-1000` | 1.4 | 1.6 | 0.87× |
| supplement | `path-100000` | 23.7 | 19.6 | 1.21× |
| supplement | `rounding-1000` | 2.2 | 2.1 | 1.05× |
| supplement | `rounding-100000` | 90.7 | 69.2 | 1.31× |
| supplement | `table-small` | 1.2 | 1.4 | 0.86× |
| supplement | `table-tall` | 5.3 | 4.0 | 1.32× |
| supplement | `table-wide` | 4.2 | 3.8 | 1.11× |
| supplement | `wine-group-complete` | 9.2 | 3.1 | 2.97× |
| supplement | `wine-group-prepared` | 2.5 | 2.8 | 0.89× |
| supplement | `wine-means` | 2.1 | 2.0 | 1.05× |
| supplement | `wine-moments` | 2.4 | 2.3 | 1.04× |
| supplement | `wine-paired` | 2.5 | 2.7 | 0.93× |
| supplement | `wine10-means` | 10.3 | 7.0 | 1.47× |
| supplement | `wine10-moments` | 13.1 | 9.1 | 1.44× |
| supplement | `wine10-paired` | 13.8 | 13.0 | 1.06× |
