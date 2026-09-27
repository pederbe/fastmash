# Benchmarks

Speed is Fastmash's reason to exist, so every release is measured against GNU
datamash 1.9 on the same machines, with the same data, and the results are
published in full: wins and losses.

## Fastmash 0.1.0

{{#include core-jobs.svg}}

Median elapsed time in milliseconds, lower is better, measured with the
0.1.0 release candidate (`fastmash` sha256 `574da941…`); the released
binaries were checked against it with no difference beyond measurement
noise. The chart is generated from
[core-jobs.tsv](core-jobs.tsv) by `scripts/benchmark_chart.py`.

| Job | Data | Intel laptop, native Linux<br>GNU → Fastmash | AMD desktop, WSL2<br>GNU → Fastmash |
| --- | --- | --- | --- |
| Quartiles of exon counts | RefGene annotations | 110.8 → 26.9 (**4.1×**) | 61.8 → 15.8 (**3.9×**) |
| Transcripts per gene | RefGene annotations | 111.9 → 65.9 (**1.7×**) | 112.8 → 39.9 (**2.8×**) |
| Exon statistics per gene | RefGene annotations | 158.4 → 107.8 (**1.5×**) | 165.6 → 71.8 (**2.3×**) |
| Many small groups | Gene annotations | 162.4 → 154.4 (1.05×) | 174.6 → 83.7 (**2.1×**) |
| Gene example from the datamash manual | Gene annotations | 7.3 → 3.8 (**1.9×**) | 11.4 → 2.9 (**3.9×**) |
| Sum and mean of a million decimals | Synthetic | 322.4 → 122.4 (**2.6×**) | 115.4 → 53.0 (**2.2×**) |
| Sum and mean of 100,000 decimals | Synthetic | 37.1 → 14.1 (**2.6×**) | 12.4 → 6.7 (**1.9×**) |
| Grouped decimals, 100,000 rows | Synthetic | 69.3 → 58.6 (1.2×) | 59.7 → 32.4 (**1.8×**) |
| One dominant group | Synthetic | 21.9 → 19.9 (1.1×) | 20.2 → 13.1 (**1.5×**) |
| Wine quality by grade | UCI Wine Quality | 6.3 → 4.3 (**1.5×**) | 8.9 → 2.9 (**3.1×**) |
| Large sort with disk spill | Synthetic | 569.7 → 1311.0 (0.43×) | 795.0 → 795.8 (1.0×) |

Over the whole catalog of 71 jobs, Fastmash is faster than GNU datamash on
48 on the Intel laptop and 46 on the WSL2 desktop, and even on nearly all the
others: they differ by less than 3 ms, which is within measurement noise for
commands this short. It gives the expected output on every job.
Full per-host tables, including CPU time and peak memory, are on the
[method](method.md) page.

## Where GNU datamash is still faster

- **Sorting data far larger than memory.** Fastmash sorts in memory and
  spills sorted runs to disk; on the Intel laptop this is 2–3 times slower
  than GNU datamash with GNU `sort`, and on WSL2 about even. This is the one
  exception accepted for 0.1.0, and the next performance priority.
- **The geometric mean of 200,000 distinct values** on the Intel laptop:
  about 1.2 times slower (7 ms).
- **On WSL2 only**: sorting scientific-notation input, about 2 times slower in
  the `C` locale (through the system `sort`) and 1.2 times in the language
  locales.

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
