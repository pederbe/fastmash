# System requirements

The Linux download is statically linked and runs on both glibc and musl
distributions, including Alpine. Follow the [install](install.md) steps.

## Platform

| Requirement | Why |
| --- | --- |
| Linux (`x86_64-unknown-linux-musl`) | Static prebuilt binaries, with no glibc version requirement |
| Linux (`x86_64-unknown-linux-gnu`) | Also supported for source builds using the system glibc |
| macOS 15 or later (`aarch64-apple-darwin`) | Supported platform for the native archive, checked on macOS 15 and 26 |
| `/proc` mounted on Linux | Supplies memory and terminal information. Without it, Fastmash uses conservative sort memory and writes output in 8 KiB blocks, even to a terminal |
| A writable `TMPDIR` (default `/tmp`) | Temporary data for large sorted jobs |

Fastmash supports Linux, Windows through WSL2 and
**macOS 15 or later** (`aarch64-apple-darwin`), with native checks
on macOS 15 and 26. Intel macOS, older macOS versions and native Windows are
outside the scope. See [Homebrew](install.md#homebrew) or the
[quick installer](install.md#quick-install).

Sorting runs inside Fastmash and needs no other program. `TMPDIR` must be
writable when a job spills to disk.

For commands under an address-space limit (`ulimit -v`, as some batch schedulers set), see
[Address-space limits](guide/errors.md#address-space-limits).

See [Grouping and sorting](guide/grouping.md#large-inputs) for memory use and
temporary storage.

## Locales

Fastmash has the glibc 2.43 locale rules built in, so you don't need to
install locale data. See [Numbers, output and locales](guide/output.md#locales).

## Verifying a release archive

The installer checks the archive's SHA-256 checksum. To verify which GitHub
workflow built the latest release, use the [GitHub CLI](https://cli.github.com)
to download and verify its archive:

Linux:

```sh
gh release download --repo pederbe/fastmash --pattern 'fastmash-*-x86_64-unknown-linux-*.tar.gz' --output fastmash.tar.gz --clobber
gh attestation verify fastmash.tar.gz --repo pederbe/fastmash
```

macOS:

```sh
gh release download --repo pederbe/fastmash --pattern 'fastmash-*-aarch64-apple-darwin.tar.gz' --output fastmash.tar.gz --clobber
gh attestation verify fastmash.tar.gz --repo pederbe/fastmash --signer-workflow pederbe/fastmash/.github/workflows/release.yml
```

The macOS archive is not Apple-notarized.

The published [benchmarks](benchmarks/index.md) measure the qualified Linux 0.1.0 binaries. Hosted macOS checks do not extend those speed claims to macOS.

## Building from source

Use the latest stable Rust and a C compiler (`cc`, or set `CC`).

```sh
git clone https://github.com/pederbe/fastmash
cd fastmash
rustup target add x86_64-unknown-linux-musl
cargo build --release --locked --target x86_64-unknown-linux-musl
# the program is target/x86_64-unknown-linux-musl/release/fastmash
```

To build on macOS 15 or later, run this command in Bash:

```bash
MACOSX_DEPLOYMENT_TARGET=15.0 cargo build --release --locked --bin fastmash --target aarch64-apple-darwin
# the program is target/aarch64-apple-darwin/release/fastmash
```

The ordinary Linux `cargo build --release --locked` command still builds
against the system glibc. The macOS build needs Apple's Command Line Tools.
Installing the archive needs no Rust compiler or Command Line Tools.

`cargo install` and source builds use your own compiler and settings. They
omit the prebuilt binaries' branch-alignment tuning for some Intel processors,
so their speed can differ slightly. Keep `--locked`: it builds with the
tested dependency versions.
