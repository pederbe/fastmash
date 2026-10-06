# Benchmarks

Speed is Fastmash's reason to exist, so every release is measured against GNU
datamash 1.9 on the same machines, with the same data, and the results are
published in full: wins and losses.

## Fastmash 0.1.0

{{#include core-jobs.svg}}

Mean of the two sessions' median elapsed times, in milliseconds; lower is
better. Measured with the
0.1.0 release binaries themselves (`fastmash` sha256 `1f1fec94…`), the
same bytes you download. The chart is generated from
[core-jobs.tsv](core-jobs.tsv) by `scripts/benchmark_chart.py`.

The animated demo replays the Intel laptop's RefGene quartile job below.
Playback is slowed 20 times so that both runs are visible; its clocks show
the measured times.

| Job | Data | Intel laptop, native Linux<br>GNU → Fastmash | AMD desktop, native Linux<br>GNU → Fastmash |
| --- | --- | --- | --- |
| Quartiles of exon counts | RefGene annotations | 112.8 → 26.6 (**4.2×**) | 50.3 → 13.1 (**3.8×**) |
| Transcripts per gene | RefGene annotations | 112.0 → 61.0 (**1.8×**) | 45.6 → 26.5 (**1.7×**) |
| Exon statistics per gene | RefGene annotations | 159.4 → 83.1 (**1.9×**) | 62.2 → 36.5 (**1.7×**) |
| Many small groups | Synthetic grouping data | 160.4 → 105.0 (**1.5×**) | 55.0 → 38.7 (**1.4×**) |
| Gene example from the datamash manual | Gene annotations | 7.3 → 3.8 (**1.9×**) | 3.0 → 1.6 (**1.9×**) |
| Sum and mean of a million decimals | Synthetic | 324.7 → 122.5 (**2.7×**) | 105.9 → 46.8 (**2.3×**) |
| Sum and mean of 100,000 decimals | Synthetic | 36.8 → 14.6 (**2.5×**) | 10.9 → 5.3 (**2.1×**) |
| Grouped decimals, 100,000 rows | Synthetic | 69.9 → 38.5 (**1.8×**) | 25.6 → 14.8 (**1.7×**) |
| One dominant group | Synthetic | 21.9 → 11.9 (**1.8×**) | 9.0 → 4.5 (**2.0×**) |
| Wine quality by grade | UCI Wine Quality | 6.4 → 3.2 (**2.0×**) | 2.8 → 1.4 (**2.0×**) |
| Large sort with disk spill | Synthetic | 572.4 → 694.1 (0.82×) | 180.0 → 309.7 (0.58×) |

Across 71 combinations of jobs and settings, 28 on the Intel laptop and 19
on the native AMD desktop meet the full speed-win rule: at least 20% and 5 ms saved
in both sessions, with consistent paired runs. Both hosts have wins in all
four non-startup workload families. Every job gives the expected output.
The named slower and inconclusive cases below retain their individual
dispositions. Full tables and the rules are on the [method](method.md) page.

## Where GNU datamash is still faster

These jobs cross the material elapsed slowdown rule in both sessions. Each
cell shows the two session medians in milliseconds:

| Host | Set and job | GNU datamash | Fastmash | Extra time per run |
| --- | --- | ---: | ---: | ---: |
| AMD desktop | Core, disk spill | 176.0 / 184.0 | 313.4 / 306.0 | 122–137 ms |
| AMD desktop | Dedicated disk spill | 177.0 / 175.6 | 303.8 / 319.5 | 127–144 ms |
| AMD desktop | Larger disk spill | 264.6 / 261.5 | 457.8 / 477.3 | 193–216 ms |
| Intel laptop | Core, disk spill | 573.7 / 571.0 | 694.5 / 693.8 | 121–123 ms |
| Intel laptop | Many keys | 788.6 / 772.0 | 959.4 / 970.9 | 171–199 ms |
| Intel laptop | Geometric mean, 200,000 distinct values | 37.3 / 37.5 | 44.7 / 44.9 | 7.4 ms |

These individual costs are accepted for 0.1.0. The AMD spill medians reach
1.83 times GNU's time on these invocations. Lower CPU work and charged peak
memory in the spill measurements are separate observations; the elapsed
verdicts remain regressions. The geometric mean keeps the existing numerical
precision and range checks.

Three further comparisons are inconclusive under the two-session rule:

| Host | Set and job | GNU datamash (ms) | Fastmash (ms) |
| --- | --- | ---: | ---: |
| AMD desktop | Many keys | 248.6 / 380.9 | 423.2 / 388.5 |
| Intel laptop | Dedicated disk spill | 580.7 / 961.9 | 714.8 / 832.3 |
| Intel laptop | Larger disk spill | 1943.6 / 892.4 | 1088.4 / 1069.7 |

These uncertain results are also accepted as named limitations. They establish
neither a GNU competitiveness pass nor a win. All samples remain in the record,
including an Intel dedicated-spill Fastmash run of 4.887 seconds. Session
medians give no tail-latency guarantee or upper bound for other input sizes.

The decision retains the measured implementation: every predecessor elapsed
and CPU guard passes, both hosts meet the workload-family and small-command
rules, and all checked outputs match. The numbers concern the recorded
warm-cache, disk-backed, 512 MiB command cgroup conditions on these two native
Linux hosts. Untimed observations confirm actual spilling and correct output;
they do not establish an I/O bottleneck.

## Run them yourself

The [benchmark kit](https://github.com/pederbe/fastmash/tree/main/bench)
runs eight of these jobs on your machine with both programs, checks that
they agree, and prints a table you can share:

```sh
curl -fsSLO https://raw.githubusercontent.com/pederbe/fastmash/main/bench/fastmash-bench.py
python3 fastmash-bench.py
```

The best benchmark is your own job. Compare both programs on your data:

```sh
export LC_ALL=C.UTF-8
time datamash -s -g 1 median 2 < your-data.tsv > /dev/null
time fastmash -s -g 1 median 2 < your-data.tsv > /dev/null
```

For stable numbers, repeat each command several times, alternate the two
programs, and use a quiet machine. [hyperfine](https://github.com/sharkdp/hyperfine)
does this for you:

```sh
hyperfine --warmup 2 \
  'datamash -s -g 1 median 2 < your-data.tsv' \
  'fastmash -s -g 1 median 2 < your-data.tsv'
```

If Fastmash is slower on a job you care about, please
[tell us](https://github.com/pederbe/fastmash/issues/new/choose): slow
real-world jobs are what we optimize next.
