# Benchmarks

Fastmash is faster than GNU datamash on quantiles, grouped summaries and
number-heavy tables. These Linux benchmarks compare both programs on an Intel
laptop and an AMD desktop, using gene annotations, wine measurements and
synthetic tables.

{{#include core-jobs.svg}}

Large sorted summaries can be slower than GNU datamash.

Longer bars mean faster runs. The dashed line marks equal speed. See the
[method and timings](method.md) for the commands, elapsed times and machines.

The animated demo replays the laptop's RefGene quartile job. Playback is slowed
20 times so that both runs are visible. Its clocks show the measured times.

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
