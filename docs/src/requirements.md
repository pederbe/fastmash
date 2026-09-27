# System requirements

Most Linux systems from 2018 onward need nothing beyond the
[install](install.md) steps. This page lists the details.

## Platform

| Requirement | Why |
| --- | --- |
| Linux x86-64, glibc 2.28 or later (RHEL 8, Debian 10, Ubuntu 20.04 and newer) | Supported platform for the prebuilt binaries |
| Linux x86-64 with glibc (`x86_64-unknown-linux-gnu`) | Building from source; musl and other targets are refused at compile time |
| `/proc` mounted | Locating the sort supervisor and checking standard streams |
| A writable `TMPDIR` (default `/tmp`) | Temporary data for large sorted jobs; the jobs that use the system `sort` also need it writable |

A few sorted (`-s`) jobs in the `C` locales use the system `sort`, listed in
[Grouping and sorting](guide/grouping.md#large-inputs). Those also need:

- Linux 5.11 or later, and `/usr/bin/sort` from GNU coreutils or uutils
  coreutils (the default on recent Ubuntu);
- the `HUP`, `INT` and `TERM` signals neither blocked nor ignored, and a
  container or security policy that allows pidfds and `close_range`. `nohup`
  ignores `HUP`, and a script's `command &` starts with `INT` ignored, so these
  jobs stop with status 77 there; run them in the foreground, for example in
  `tmux` or `screen`;
- memory for the system `sort`, which chooses its own buffer size and threads
  as it does for GNU datamash (`FASTMASH_SORT_MEMORY_BYTES` applies only to
  Fastmash's own sorting).

All other sorted jobs sort inside Fastmash and need none of these.

## Locales

Fastmash has the glibc 2.43 locale rules built in, so you don't need to
install locale data. See [Numbers, output and locales](guide/output.md#locales).

## Verifying a release archive

Every release archive has a `.sha256` checksum file, checked by the install
steps, and a GitHub build provenance attestation:

```sh
gh attestation verify fastmash-v0.1.0-x86_64-unknown-linux-gnu.tar.gz --repo pederbe/fastmash
```

The published [benchmarks](benchmarks/index.md) measure these exact binaries.

## Building from source

```sh
git clone https://github.com/pederbe/fastmash
cd fastmash
cargo build --release --locked
# the programs are target/release/fastmash and target/release/fastmash-sort-supervisor
```

`cargo install` and source builds use your own compiler and settings. They
omit the prebuilt binaries' branch-alignment tuning for some Intel processors,
so their speed can differ slightly. Keep `--locked`: it builds with the
tested dependency versions.
