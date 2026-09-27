# Benchmarks

Speed is Fastmash's reason to exist, so every release is measured against GNU
datamash 1.9 on the same machines, with the same data, and the results are
published in full: wins and losses.

## Fastmash 0.1.0

{{#include core-jobs.svg}}

Median elapsed time in milliseconds, lower is better, measured with the
0.1.0 release binaries themselves (`fastmash` sha256 `da311d13…`), the
same bytes you download. The chart is generated from
[core-jobs.tsv](core-jobs.tsv) by `scripts/benchmark_chart.py`.

| Job | Data | Intel laptop, native Linux<br>GNU → Fastmash | AMD desktop, WSL2<br>GNU → Fastmash |
| --- | --- | --- | --- |
| Quartiles of exon counts | RefGene annotations | 111.8 → 27.2 (**4.1×**) | 57.5 → 15.4 (**3.7×**) |
| Transcripts per gene | RefGene annotations | 112.2 → 66.1 (**1.7×**) | 108.7 → 36.7 (**3.0×**) |
| Exon statistics per gene | RefGene annotations | 159.6 → 110.5 (1.4×) | 158.8 → 62.9 (**2.5×**) |
| Many small groups | Gene annotations | 163.4 → 157.4 (1.04×) | 161.6 → 74.6 (**2.2×**) |
| Gene example from the datamash manual | Gene annotations | 7.5 → 3.9 (**1.9×**) | 9.9 → 2.6 (**3.8×**) |
| Sum and mean of a million decimals | Synthetic | 326.2 → 125.7 (**2.6×**) | 110.7 → 52.1 (**2.1×**) |
| Sum and mean of 100,000 decimals | Synthetic | 35.2 → 14.4 (**2.4×**) | 12.1 → 6.3 (**1.9×**) |
| Grouped decimals, 100,000 rows | Synthetic | 70.1 → 59.0 (1.2×) | 56.2 → 29.8 (**1.9×**) |
| One dominant group | Synthetic | 21.9 → 20.1 (1.09×) | 18.5 → 12.9 (1.4×) |
| Wine quality by grade | UCI Wine Quality | 6.4 → 4.4 (**1.5×**) | 8.3 → 2.8 (**3.0×**) |
| Large sort with disk spill | Synthetic | 638.0 → 1363.8 (0.47×) | 751.6 → 688.7 (1.09×) |

Over the whole catalog of 71 jobs, Fastmash is faster than GNU datamash on
49 on the Intel laptop and 48 on the WSL2 desktop, and even on nearly all the
others: they differ by less than 3 ms, which is within measurement noise for
commands this short. It gives the expected output on every job.
Full per-host tables, including CPU time and peak memory, are on the
[method](method.md) page.

## Where GNU datamash is still faster

- **Sorting data far larger than memory.** Fastmash sorts in memory and
  spills sorted runs to disk; on the Intel laptop this is about 2 times slower
  than GNU datamash with GNU `sort`. On WSL2 the disk-spill job is even and
  the largest one about 1.5 times slower. This is the one exception accepted
  for 0.1.0, and the next performance priority.
- **The geometric mean of 200,000 distinct values** on the Intel laptop:
  about 1.2 times slower (6 ms).
- **On WSL2 only**: sorting scientific-notation input in the language
  locales, about 1.2 times slower (8 ms).

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
