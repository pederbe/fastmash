# Benchmarks

Speed is Fastmash's reason to exist, so every release is measured against GNU
datamash 1.9 on the same machines, with the same data, and the results are
published in full: wins and losses.

## Fastmash 0.1.0

{{#include core-jobs.svg}}

Mean of the two sessions' median elapsed times, in milliseconds; lower is
better. Measured with the
0.1.0 release binaries themselves (`fastmash` sha256 `6717db00…`), the
same bytes you download. The chart is generated from
[core-jobs.tsv](core-jobs.tsv) by `scripts/benchmark_chart.py`.

The animated demo replays the Intel laptop's RefGene quartile job below.
Playback is slowed 20 times so that both runs are visible; its clocks show
the measured times.

| Job | Data | Intel laptop, native Linux<br>GNU → Fastmash | AMD desktop, WSL2<br>GNU → Fastmash |
| --- | --- | --- | --- |
| Quartiles of exon counts | RefGene annotations | 111.0 → 26.2 (**4.2×**) | 58.6 → 14.8 (**4.0×**) |
| Transcripts per gene | RefGene annotations | 111.6 → 59.3 (**1.9×**) | 112.1 → 33.2 (**3.4×**) |
| Exon statistics per gene | RefGene annotations | 158.9 → 79.8 (**2.0×**) | 161.3 → 42.7 (**3.8×**) |
| Many small groups | Gene annotations | 159.9 → 99.8 (**1.6×**) | 169.3 → 48.7 (**3.5×**) |
| Gene example from the datamash manual | Gene annotations | 7.3 → 3.8 (**1.9×**) | 10.7 → 3.0 (**3.6×**) |
| Sum and mean of a million decimals | Synthetic | 317.7 → 119.1 (**2.7×**) | 111.0 → 50.1 (**2.2×**) |
| Sum and mean of 100,000 decimals | Synthetic | 36.9 → 14.4 (**2.6×**) | 12.2 → 6.5 (**1.9×**) |
| Grouped decimals, 100,000 rows | Synthetic | 69.0 → 37.5 (**1.8×**) | 55.4 → 18.5 (**3.0×**) |
| One dominant group | Synthetic | 22.0 → 11.6 (**1.9×**) | 19.1 → 6.3 (**3.0×**) |
| Wine quality by grade | UCI Wine Quality | 6.3 → 3.1 (**2.0×**) | 8.6 → 2.5 (**3.4×**) |
| Large sort with disk spill | Synthetic | 569.5 → 680.2 (0.84×) | 771.4 → 432.3 (**1.8×**) |

Across 71 combinations of jobs and settings, 27 on the Intel laptop and 31
on the WSL2 desktop meet the full speed-win rule: at least 20% and 5 ms saved
in both sessions, with consistent paired runs. Both hosts have wins in all
four non-startup workload families. Every job gives the expected output.
The remaining jobs stay within the regression limits apart from the native
Linux cases below. Full tables and the rules are on the [method](method.md) page.

## Where GNU datamash is still faster

- **Disk-spilling sorts on the Intel laptop.** Under the benchmark's 512 MiB
  command memory limit, the large sort cases take about 1.2 times GNU's time.
  The largest case has slower session medians, with only four of six paired
  runs slower in one session; its regression result is inconclusive under the
  repeatability rule. These spill costs are accepted for 0.1.0.
  The corresponding WSL2 jobs are faster than GNU datamash.
- **The geometric mean of 200,000 distinct values** on the Intel laptop:
  about 1.2 times slower (7 ms), also accepted for 0.1.0.

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
