# System requirements

Most Linux systems from 2018 onward need nothing beyond the
[install](install.md) steps. This page lists the details.

## Platform

| Requirement | Why |
| --- | --- |
| Linux x86-64, glibc 2.28 or later (RHEL 8, Debian 10, Ubuntu 20.04 and newer) | Supported platform for the prebuilt binaries |
| Linux x86-64 with glibc (`x86_64-unknown-linux-gnu`) | Building from source; musl and other targets are refused at compile time |
| `/proc` mounted | Needed for the system `sort` route below; without it, those jobs sort in process, and output is written in 8 KiB blocks, even to a terminal |
| A writable `TMPDIR` (default `/tmp`) | Temporary data for large sorted jobs |

The published 0.1.0 artifacts remain Linux-only. Native development builds
also admit **Apple Silicon macOS 15 or later** (`aarch64-apple-darwin`), with
native checks on macOS 15 and 26. Intel macOS, older macOS versions and native
Windows are outside the scope. See the [development archive instructions](install.md#native-macos-development-archive).

macOS uses its system libraries and Fastmash's built-in sorting with Spill.
It needs no `/proc`, GNU coreutils or Sort supervisor. A writable temporary
directory is still required for large sorted jobs. The same built-in locale
rules and Numerical profile apply; native GNU results do not redefine Portable
results. Missing Linux memory information does not impose a record limit.

Under an address-space limit (`ulimit -v`, as some batch schedulers set),
Fastmash reduces the address space that its memory allocator reserves; see
[Address-space limits](guide/errors.md#address-space-limits).

A few sorted (`-s`) jobs in the `C` locales, listed in
[Grouping and sorting](guide/grouping.md#large-inputs), use the system `sort`
when all of these hold: `/proc` is mounted and `fastmash-sort-supervisor` is
beside `fastmash`; `/usr/bin/sort` is an executable from GNU coreutils or
uutils coreutils (not BusyBox or Toybox); the field separator is ASCII; `TMPDIR` is writable; Linux
is 5.11 or later, with pidfds and `close_range` allowed; and the `HUP`, `INT`
and `TERM` signals are neither blocked nor ignored. Otherwise (for example
without the supervisor or `/usr/bin/sort`, under `nohup`, for `command &` in a
script, on an older kernel or in a restrictive container), those jobs sort
inside Fastmash instead, with the same output but more slowly on large input.
The system `sort` chooses its own buffer size and threads, as it does for GNU
datamash (Fastmash's memory policy and `FASTMASH_SORT_MEMORY_BYTES` apply
only to its own sorting).

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

To build the current development source on Apple Silicon macOS 15 or later:

```sh
MACOSX_DEPLOYMENT_TARGET=15.0 cargo build --release --locked --bin fastmash --target aarch64-apple-darwin
# the program is target/aarch64-apple-darwin/release/fastmash
```

The macOS build needs Rust 1.88 or later and Apple's Command Line Tools.
Installing the archive needs neither. The Linux supervisor is not a macOS
runtime component.

`cargo install` and source builds use your own compiler and settings. They
omit the prebuilt binaries' branch-alignment tuning for some Intel processors,
so their speed can differ slightly. Keep `--locked`: it builds with the
tested dependency versions.
