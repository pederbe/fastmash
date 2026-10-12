<h1 align="center">Fastmash</h1>

<p align="center">
  <strong>Fast command-line statistics for delimited text.</strong><br>
  A Rust implementation of the GNU datamash command language.
</p>

<p align="center">
  <a href="https://github.com/pederbe/fastmash/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/pederbe/fastmash/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/pederbe/fastmash/releases"><img alt="Latest release" src="https://img.shields.io/github/v/release/pederbe/fastmash"></a>
  <a href="https://github.com/pederbe/fastmash/blob/main/LICENSE-MIT"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-blue"></a>
  or
  <a href="https://github.com/pederbe/fastmash/blob/main/LICENSE-APACHE"><img alt="Apache 2.0 license" src="https://img.shields.io/badge/license-Apache--2.0-blue"></a>
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

- **Speed.** Up to four times faster than GNU datamash 1.9 in
  [published Linux benchmarks](https://fastmash.io/benchmarks/).
- **Familiar commands.** Over 70 operations and modes, with grouping, headers,
  field selectors, percentiles, `crosstab` and `transpose`.
- **CSV and table tools.** Read and write quoted CSV, name result columns,
  calculate weighted means, select top records, compare datasets and check
  table health.
- **Repeatable results.** Software arithmetic gives identical numerical results
  across supported machines for the same input, settings and Fastmash version.

## Install

Fastmash runs on **Linux x86-64** (glibc), **Windows through WSL2** and
**Apple Silicon macOS 15 and later**.

Download a prebuilt binary from the
[releases page](https://github.com/pederbe/fastmash/releases), or install with
Cargo on Linux or WSL2.

```sh
cargo install --locked fastmash
```

On Apple Silicon macOS

```sh
cargo install --locked --bin fastmash fastmash
```

The [installation guide](https://fastmash.io/install.html) also covers Homebrew,
Linux packages and the quick installer.

## Quick start

```sh
# Sum CSV revenue by region and name the result column
fastmash --csv -H -s -g region --result-name=1:total sum revenue < sales.csv

# Weighted mean and the highest five complete records
fastmash -H wmean price:units < sales.tsv
fastmash --csv -H top:5 revenue < sales.csv

# Check table health and compare summaries of two exports
fastmash --header-in health type revenue number nonmissing id validate < sales.tsv
fastmash --csv -H -g region compare before.csv after.csv sum revenue
```

Use `--csv` for quoted CSV and `-H` to select fields by header name.
The [user guide](https://fastmash.io/guide/) covers all operations and options.

## Compatibility

Fastmash implements the GNU datamash 1.9 command language. See the
[migration guide](https://fastmash.io/guide/migrating.html) to switch existing
scripts and the [documented differences](https://fastmash.io/guide/differences.html)
for compatibility details.

## Documentation

- [User guide](https://fastmash.io/guide/) for commands and examples
- [Architecture](https://github.com/pederbe/fastmash/blob/main/ARCHITECTURE.md) for how Fastmash is built
- [Changelog](https://github.com/pederbe/fastmash/blob/main/CHANGELOG.md) for changes between releases

## Contributing

Found a bug or have a workload to share?
[Open an issue](https://github.com/pederbe/fastmash/issues/new/choose) or read the
[contributing guide](https://github.com/pederbe/fastmash/blob/main/CONTRIBUTING.md).
Report vulnerabilities through the
[security policy](https://github.com/pederbe/fastmash/blob/main/SECURITY.md).

---

<p align="center">Created by <a href="https://pederbe.dev">Peder Bergan</a></p>
