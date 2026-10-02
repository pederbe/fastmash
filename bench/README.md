# Benchmark kit

`fastmash-bench.py` runs a subset of the [published benchmark
jobs](https://fastmash.io/benchmarks/) on your machine with GNU datamash and
Fastmash, checks that both print the same output, and prints a Markdown table
you can share.

```sh
curl -fsSLO https://raw.githubusercontent.com/pederbe/fastmash/main/bench/fastmash-bench.py
python3 fastmash-bench.py
```

You need Python 3.8 or later, `datamash` and `fastmash` on your `PATH` (or
`--datamash PATH` and `--fastmash PATH`), and about 60 MB of disk for the
data. Nothing else is installed.

On first use it downloads the public datasets into `fastmash-bench-data/`
and generates the synthetic ones there:

| Data | Source | Size |
| --- | --- | --- |
| RefGene gene annotations (hg19) | [UCSC](https://hgdownload.soe.ucsc.edu/goldenPath/hg19/database/refGene.txt.gz) | 8 MB download, 24 MB unpacked |
| White wine measurements | [UCI Wine Quality](https://archive.ics.uci.edu/dataset/186/wine+quality) (CC BY 4.0) | 260 kB |
| Synthetic decimals, 100,000 and 1,000,000 rows | generated from a fixed seed | 3 MB and 30 MB |

UCSC updates RefGene now and then; the kit says so when your copy differs
from the snapshot behind the published results. Your comparison of the two
programs is still valid.

## Options

| Option | Meaning |
| --- | --- |
| `--list` | Show the jobs and their arguments |
| `--job NAME` | Run only this job (repeatable) |
| `--runs N` | Timed runs per program (default 10) |
| `--locale L` | `LC_ALL` for both programs (default `C`) |
| `--custom 'ARGS' --input FILE` | Time your own command instead |
| `--no-hyperfine` | Use the built-in timer even if hyperfine is installed |

If [hyperfine](https://github.com/sharkdp/hyperfine) (1.16 or later) is
installed, it does the timing. Otherwise each program runs once to warm up,
then `--runs` times, alternating the two, and the median is reported. Close
other busy programs first: a quiet machine gives stable numbers.

## Example output

From the AMD desktop under WSL2, using the 0.1.0 release binary on
1 October 2026. The kit uses a simpler timing method and no cgroup limit,
so its values need not match the controlled release measurements:

```text
Fastmash benchmark kit: fastmash 0.1.0 vs datamash (GNU datamash) 1.9
CPU: (your CPU model); Linux (your kernel); LC_ALL=C; median of 3 runs (built-in timer)

| Job | Arguments | datamash (ms) | fastmash (ms) | datamash / fastmash |
| --- | --- | ---: | ---: | ---: |
| refgene-quantiles | `q1 9 median 9 q3 9 iqr 9` | 56.6 | 14.4 | 3.92x |
| refgene-transcripts | `-s -g 13 count 2 collapse 2` | 112.7 | 31.1 | 3.62x |
| refgene-exons | `-s -g 13 count 9 min 9 max 9 mean 9 median 9` | 163.5 | 40.5 | 4.03x |
| decimal-1000000 | `-H sum 3 mean 3` | 108.0 | 49.8 | 2.17x |
| decimal-100000 | `-H sum 3 mean 3` | 11.7 | 6.0 | 1.97x |
| grouped-decimal-100000 | `-H -s -g 2 count 3 mean 3 median 3` | 54.8 | 16.8 | 3.27x |
| wine-by-quality | `-t ';' -H -s -g 12 count 1 mean 11 median 11` | 8.0 | 1.8 | 4.43x |
| wine-white-summary | `-t ';' -H mean 9 median 9 min 9 max 9` | 2.0 | 1.6 | 1.29x |
```

## Sharing your results

Paste the table into a [discussion](https://github.com/pederbe/fastmash/discussions)
or, if Fastmash is slower or prints something different on a job you care
about, [open an issue](https://github.com/pederbe/fastmash/issues/new/choose)
with the table and the command. Slow real-world jobs are what we optimize
next.
