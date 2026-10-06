<h1 align="center">Fastmash</h1>

<p align="center">
  <strong>Fast command-line statistics for delimited text.</strong><br>
  A Rust implementation of the GNU datamash command language.
</p>

<p align="center">
  <a href="https://github.com/pederbe/fastmash/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/pederbe/fastmash/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/pederbe/fastmash/releases"><img alt="Latest release" src="https://img.shields.io/github/v/release/pederbe/fastmash"></a>
  <a href="#license"><img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue"></a>
</p>

<p align="center">
  <a href="https://fastmash.io">Documentation</a> ·
  <a href="https://fastmash.io/install.html">Install</a> ·
  <a href="https://fastmash.io/benchmarks/">Benchmarks</a> ·
  <a href="https://fastmash.io/guide/migrating.html">Migrating from datamash</a>
</p>

---

Fastmash summarizes, groups and reshapes tab-separated text and quoted CSV from
the command line. It accepts the command language of
[GNU datamash](https://www.gnu.org/software/datamash/), so most existing scripts can
switch by changing one word.

```console
$ printf 'site\treading\nwest\t2\neast\t4\nwest\t6\n' | fastmash -H -s -g site sum reading mean reading
GroupBy(site)	sum(reading)	mean(reading)
east	4	4
west	8	4
```

## Why Fastmash

- **Fast.** On representative real-world jobs Fastmash runs up to four times
  faster than GNU datamash 1.9, for example quantiles on genome annotations and
  grouped summaries per gene. Every release publishes its full results, including the jobs
  where GNU datamash is still faster: see the [benchmarks](https://fastmash.io/benchmarks/).
- **The datamash commands you already use.** Over 70 operations and modes,
  datamash's options and field selectors, grouping, headers, `crosstab`,
  `transpose` and more.
- **Useful table workflows.** Read and write quoted CSV, name result columns,
  calculate weighted means, select the highest or lowest complete records,
  compare two datasets and inspect or validate table health.
- **The same answer everywhere.** Arithmetic is implemented in software, so the
  same Fastmash version gives identical results on every supported machine,
  whatever its CPU or math library.
- **Refuses rather than guesses.** Where GNU datamash 1.9 can crash, truncate
  or print undefined values, Fastmash reports a clear error or a refusal
  (exit status 77). Memory and disk use grow with the job and are checked.

## Install

Fastmash runs on Linux x86-64 (glibc) and on Windows through WSL2.
The published 0.1.0 archives cover those platforms. Development builds add
Apple Silicon macOS 15 and later; the [native archive instructions](docs/src/install.md#native-macos-development-archive)
describe how to obtain and verify the CI artifact before a macOS release is available.

Download a prebuilt binary from the
[releases page](https://github.com/pederbe/fastmash/releases), or build it with Cargo:

```sh
cargo install --locked fastmash
```

See the [installation guide](https://fastmash.io/install.html) for
requirements, verification and removal. Installing Fastmash does not replace or
alter an installed `datamash`.

## Quick start

```sh
# Sum and mean of columns 1 and 2
printf '1\t2\n3\t4\n' | fastmash sum 1 mean 2
# 4	3

# Group by column 1 after sorting, then count and take the median of column 2
fastmash -s -g 1 count 2 median 2 < measurements.tsv

# Quoted CSV, named fields and a stable output label
fastmash --csv -H -s -g region --result-name=1:total sum revenue < sales.csv

# Weighted mean and the highest five complete records
fastmash -H wmean price:units < sales.tsv
fastmash --csv -H top:5 revenue < sales.csv

# Inspect a table, or compare numerical summaries of two exports
fastmash --header-in health type revenue number nonmissing id validate < sales.tsv
fastmash --csv -H -g region compare before.csv after.csv sum revenue

# Per-row transformations
fastmash round 3 sha256 1 < data.tsv
```

`--csv` reads and writes strict quoted CSV; `--csv-in` and `--csv-out` select
each direction separately. `-t,` retains literal comma splitting for existing
datamash commands. The [user guide](https://fastmash.io/guide/) covers every
operation, option and mode, including their format and resource limits.

## Compatibility

Fastmash targets the command language and results of GNU datamash 1.9. Where it
differs, the difference is intentional and listed in
[Differences from GNU datamash](https://fastmash.io/guide/differences.html).
The most visible ones:

- Fastmash's numerical results come from its own portable arithmetic. The
  final digit or the sign of a NaN can occasionally differ from a local GNU build.
- Inputs that crash GNU datamash 1.9 or give it undefined behavior are errors
  or refusals in Fastmash.
- `--sort-cmd` and the explicit `line` mode are not provided.

Quoted CSV, custom result names, weighted mean, Top-N selection, dataset
comparison and table health are Fastmash extensions. Help and readable health
reports use terminal color automatically; `--no-color` suppresses it. Calculation
results, CSV, health TSV and version output always keep plain data bytes.

## Status

Fastmash is at **0.1**, a preview release: usable today on Linux x86-64 and
WSL2, with the differences above. Before 1.0, minor releases may still change
behavior; every change is listed in the [changelog](CHANGELOG.md). Native Apple
Silicon macOS support is under development for 0.2.0, with checks on macOS 15
and 26. Windows users run the Linux version through WSL2;
native Windows is outside the current product scope.
See the [roadmap](https://fastmash.io/roadmap.html).

## Documentation

- [User guide](https://fastmash.io/guide/): operations, grouping, headers,
  numbers and output, errors and exit statuses
- [Architecture](ARCHITECTURE.md): how Fastmash is built
- [Testing](TESTING.md): how Fastmash is verified
- [Glossary](CONTEXT.md): the project's vocabulary

## Contributing

Bug reports, compatibility findings and benchmarks from real workloads are
especially welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) to get started,
and [SECURITY.md](SECURITY.md) to report a vulnerability.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in Fastmash by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

Fastmash is an independent implementation and is not affiliated with the GNU
Project. It contains no GNU datamash code; its behavior was matched by studying
GNU datamash's documentation, source code and observed output.

---

<p align="center">Created by <a href="https://pederbe.dev">Peder Bergan</a></p>
