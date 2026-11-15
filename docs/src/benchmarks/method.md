# Benchmark method

We compare Fastmash with GNU datamash on the same files and machine. The
benchmark kit checks that their output matches before timing either program.

Each job runs in two rounds, with one warm-up and seven timed runs per program
in each round. Program order alternates, and job order is shuffled between
rounds. The tables show the mean of the two median elapsed times in
milliseconds. Lower times are better.

Both programs read regular files with a warm file cache, use the `C` locale
and write timed output to `/dev/null`. Temporary files use disk-backed storage.
GNU datamash's `-s` commands include the time taken by the system `sort`.

## Machines

| Host | CPU | System |
| --- | --- | --- |
| Laptop | Intel Core i7-8550U | CachyOS, native Linux |
| Desktop | AMD Ryzen 7 9800X3D | CachyOS, native Linux |

## Data and commands

The [benchmark kit](https://github.com/pederbe/fastmash/tree/main/bench) downloads
RefGene genome annotations and UCI Wine Quality measurements, and generates
synthetic decimal tables from a fixed seed. The remaining jobs exercise small
groups, a dominant group and a large sorted summary.

Field numbers in each command refer to its input table. The commands below
are identical for both programs.

{{#include full-results.md}}
