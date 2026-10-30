# System requirements

Most Linux systems from 2018 onward need nothing beyond the
[install](install.md) steps. This page lists the details.

## Platform

| Requirement | Why |
| --- | --- |
| Linux x86-64, glibc 2.28 or later (RHEL 8, Debian 10, Ubuntu 20.04 and newer) | Supported platform for the prebuilt binaries |
| Linux x86-64 with glibc (`x86_64-unknown-linux-gnu`) | Building Linux source; musl and other Linux targets are refused at compile time |
| Apple Silicon macOS 15 or later (`aarch64-apple-darwin`) | Supported platform for the native archive, checked on macOS 15 and 26 |
| `/proc` mounted on Linux | Supplies memory and terminal information. Without it, Fastmash uses conservative sort memory and writes output in 8 KiB blocks, even to a terminal |
| A writable `TMPDIR` (default `/tmp`) | Temporary data for large sorted jobs |

Fastmash supports Linux x86-64, Windows through WSL2 and
**Apple Silicon macOS 15 or later** (`aarch64-apple-darwin`), with native checks
on macOS 15 and 26. Intel macOS, older macOS versions and native Windows are
outside the scope. See [Homebrew](install.md#homebrew) or the
[native archive instructions](install.md#macos-archive).

Linux and macOS use Fastmash's built-in sorting with Spill. Sorting needs no
system `sort`, helper executable, pidfds or `close_range`. A writable temporary
directory is required when a sorted job spills. The same built-in locale
rules and Numerical profile apply; native GNU results do not redefine Portable
results. Missing Linux memory information does not impose a record limit.

Under an address-space limit (`ulimit -v`, as some batch schedulers set),
Fastmash reduces the address space that its memory allocator reserves; see
[Address-space limits](guide/errors.md#address-space-limits).

Every sorted calculation and sorted `rmdup` uses this same native storage and
resource policy. See [Grouping and sorting](guide/grouping.md#large-inputs).

## Locales

Fastmash has the glibc 2.43 locale rules built in, so you don't need to
install locale data. See [Numbers, output and locales](guide/output.md#locales).

## Verifying a release archive

The installer checks the archive's SHA-256 checksum. To verify which GitHub
workflow built it, download the archive from the
[releases page](https://github.com/pederbe/fastmash/releases) and use the
[GitHub CLI](https://cli.github.com):

Linux:

```sh
gh attestation verify fastmash-v0.2.0-x86_64-unknown-linux-gnu.tar.gz --repo pederbe/fastmash
```

macOS:

```sh
gh attestation verify fastmash-v0.2.0-aarch64-apple-darwin.tar.gz --repo pederbe/fastmash --signer-workflow pederbe/fastmash/.github/workflows/release.yml
```

The macOS archive is not Apple-notarized.

The published [benchmarks](benchmarks/index.md) measure the qualified Linux 0.1.0 binaries. Hosted macOS checks do not extend those speed claims to macOS.

## Building from source

```sh
git clone https://github.com/pederbe/fastmash
cd fastmash
cargo build --release --locked
# the program is target/release/fastmash
```

To build on Apple Silicon macOS 15 or later, run this command in Bash:

```bash
MACOSX_DEPLOYMENT_TARGET=15.0 cargo build --release --locked --bin fastmash --target aarch64-apple-darwin
# the program is target/aarch64-apple-darwin/release/fastmash
```

The macOS build needs Rust 1.88 or later and Apple's Command Line Tools.
Installing the archive needs neither.

`cargo install` and source builds use your own compiler and settings. They
omit the prebuilt binaries' branch-alignment tuning for some Intel processors,
so their speed can differ slightly. Keep `--locked`: it builds with the
tested dependency versions.
