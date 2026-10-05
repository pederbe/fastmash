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
a result. Each candidate is also compared with the previous accepted Fastmash
build on every existing job (the *predecessor guards*); a slowdown beyond these limits needs
an explicit, published justification.

## Hosts

| Host | CPU | OS | glibc | System `sort` |
| --- | --- | --- | --- | --- |
| Laptop | Intel Core i7-8550U | CachyOS, native Linux 7.2.9 | 2.44 | GNU coreutils 9.12 |
| Desktop | AMD Ryzen 7 9800X3D | CachyOS, native Linux 7.2.9 | 2.44 | GNU coreutils 9.12 |

GNU datamash's `-s` jobs sort with the system `sort`, so its speed is part of
their times.

Each complete command and its children run with a 512 MiB cgroup memory
limit, no swap, at most 64 tasks (processes and threads), a 30-second timeout
and a 512 MiB per-file output limit. Temporary files
are on disk-backed Linux storage. Both programs get the same limits;
Fastmash's sort memory override is unset. These conditions matter for large
sorts: the memory available to a command affects when it writes runs to disk.
Separate, untimed observations confirm that the `disk-spill` and `spill-large`
jobs write temporary runs on both hosts under these limits and still produce
the expected output.

## Full results

Mean of the two sessions' median elapsed times, for every job and setting in the
0.1.0 measurement (release binary `1f1fec94…`, 5 and 6 October 2026). Every job gave
the expected output on both hosts. Sets: `core` and `supplement` are the
representative jobs; `locale-C`, `locale-en` and `locale-de` repeat
locale-sensitive jobs under `C`, `en_US.UTF-8` and `de_DE.UTF-8`; `guards`
are small edge-case jobs; the `spill` sets exercise larger sorts and their
boundaries. CPU time and peak memory are in the measurement record.
The displayed means average the retained session medians, already rounded to
0.1 ms, then round that mean to 0.1 ms. Ratios use the displayed values.
Precise unrounded samples remain in the measurement record.

The elapsed verdict preserves each comparison's two-session result. A
regression crosses the material slowdown limits in both sessions, with
Fastmash slower in at least five of six paired rounds in each. A threshold
or direction disagreement is inconclusive; it does not establish a win or
a competitive pass. Within margins means both sessions stay within the
material regression limits. The [named limitations](index.md#where-gnu-datamash-is-still-faster)
retain the practical costs and uncertainty.

{{#include full-results.md}}
