# Install

Fastmash 0.2.0 runs on **Linux x86-64**, **Windows through WSL2** and
**Apple Silicon macOS 15 and later**. It installs next to GNU datamash
without changing it. Native Windows is outside the current product scope.

## Quick install

```sh
curl -fsSL https://fastmash.io/install.sh | sh
```

The [script](https://fastmash.io/install.sh) downloads the latest release,
checks its checksum and installs `fastmash` and, on Linux, `fastmash-sort-supervisor`
into `~/.local/bin`. Set `FASTMASH_INSTALL_DIR` to install elsewhere, or
`FASTMASH_VERSION` for a specific release.

On Linux the script uses `sha256sum`. A macOS archive uses the system
`shasum -a 256`, and installs only `fastmash`. Rust and GNU coreutils are
not required to install an archive. Download and checksum failures leave an
existing installation unchanged.

The script checks the download against its published SHA-256 checksum. For a
stronger check that the archive was built by this project's release workflow,
download it by hand and verify its GitHub build attestation:

```sh
gh attestation verify fastmash-v0.2.0-x86_64-unknown-linux-gnu.tar.gz --repo pederbe/fastmash
```

## Prebuilt binary, by hand

On Linux or WSL2, download a release from the
[releases page](https://github.com/pederbe/fastmash/releases), check it and
copy the two programs into a directory on your `PATH`. Run this block in Bash;
invoke `bash` first if using Fish. It stops if a download or checksum fails:

```bash
bash -eu <<'EOF'
version=0.2.0
name=fastmash-v$version-x86_64-unknown-linux-gnu
curl -fLO https://github.com/pederbe/fastmash/releases/download/v$version/$name.tar.gz
curl -fLO https://github.com/pederbe/fastmash/releases/download/v$version/$name.tar.gz.sha256
sha256sum -c $name.tar.gz.sha256
tar -xzf $name.tar.gz
mkdir -p ~/.local/bin
cp $name/fastmash $name/fastmash-sort-supervisor ~/.local/bin/
EOF
```

Replace `0.2.0` with the release you want, and keep `fastmash` and
`fastmash-sort-supervisor` from the same release in the same directory. If the
supervisor is missing, some large sorted jobs are slower; if it comes from
another release, the sorted jobs that use it stop with a message saying so.

## With Cargo

If you have a [Rust toolchain](https://rustup.rs), on Linux or WSL2:

```sh
cargo install --locked fastmash
```

On Apple Silicon macOS 15 or later, select its single executable:

```sh
cargo install --locked --bin fastmash fastmash
```

With [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), download
the prebuilt release binaries instead of compiling. On Linux or WSL2:

```sh
cargo binstall fastmash
```

On Apple Silicon macOS 15 or later:

```sh
cargo binstall --bin fastmash fastmash
```

## Homebrew

With [Homebrew](https://brew.sh) on Linux x86-64 or Apple Silicon macOS 15
or later:

```sh
brew install pederbe/tap/fastmash
```

The [maintainer's tap](https://github.com/pederbe/homebrew-tap) builds the
published 0.2.0 source with its locked Rust dependencies. On Linux it installs
both programs; on macOS it installs only `fastmash`. Both include their
licenses and notices. Rust and Python are build dependencies;
these source builds do not carry the performance qualification of the
downloadable binaries.

Run `brew test pederbe/tap/fastmash` to check the installation. On Linux the
test includes the optional system-sort route and needs Linux 5.11 or later
with GNU or uutils coreutils at `/usr/bin/sort`. Older kernels can use
Fastmash's built-in sorting. On macOS the test checks built-in sorting and
confirms that no Sort supervisor is installed.
Remove the package with `brew uninstall pederbe/tap/fastmash`.

## Native macOS archive

For Apple Silicon with macOS 15 or later, the published archive contains
`fastmash`, licenses and build provenance. It uses built-in sorting with Spill
and has no Sort supervisor. The same archive is checked on macOS 15 and 26.

The quick installer selects `aarch64-apple-darwin` automatically. To download
the 0.2.0 archive by hand:

```sh
curl -fLO https://github.com/pederbe/fastmash/releases/download/v0.2.0/fastmash-v0.2.0-aarch64-apple-darwin.tar.gz
curl -fLO https://github.com/pederbe/fastmash/releases/download/v0.2.0/fastmash-v0.2.0-aarch64-apple-darwin.tar.gz.sha256
```

For a stronger check before installation, verify its GitHub build attestation:

```sh
gh attestation verify fastmash-v0.2.0-aarch64-apple-darwin.tar.gz --repo pederbe/fastmash --signer-workflow pederbe/fastmash/.github/workflows/release.yml
```

Run this block in Bash; invoke `bash` first if using Fish. It stops on a
failed checksum before changing an existing installation. It defaults to 0.2.0;
set `FASTMASH_VERSION` to install a different downloaded archive version:

```bash
bash -eu <<'EOF'
version=${FASTMASH_VERSION:-0.2.0}
name=fastmash-v$version-aarch64-apple-darwin
shasum -a 256 -c "$name.tar.gz.sha256"
tar -xzf "$name.tar.gz"
(cd "$name" && shasum -a 256 -c binary.sha256)
cat "$name/build-provenance.txt"
mkdir -p ~/.local/bin
staging=$(mktemp -d "$HOME/.local/bin/.fastmash.XXXXXX")
trap 'rm -rf "$staging"' EXIT
cp "$name/fastmash" "$staging/fastmash"
chmod 755 "$staging/fastmash"
mv -f "$staging/fastmash" ~/.local/bin/fastmash
printf '1\n2\n' | ~/.local/bin/fastmash sum 1
EOF
```

The final command prints `3`. The archive checksum, embedded executable hash
and build provenance identify the delivered bytes. The release's installed
command checks and both regression corpus modes cover those same bytes on
macOS 15 and 26.

The archive is not Apple-notarized. Native CI records extended attributes,
code-signing metadata and the assessment result for a terminal download,
then executes the installed command without changing macOS security controls.
Browser downloads can have different quarantine behavior and are outside
these terminal checks. A checksum checks bytes, not Apple approval.

## Debian and Ubuntu

```sh
curl -fLO https://github.com/pederbe/fastmash/releases/download/v0.2.0/fastmash_0.2.0_amd64.deb
sudo apt install ./fastmash_0.2.0_amd64.deb
```

For Debian 10, Ubuntu 20.04 and newer. Remove it with `sudo apt remove fastmash`.

## Fedora, RHEL and similar

```sh
sudo dnf install https://github.com/pederbe/fastmash/releases/download/v0.2.0/fastmash-0.2.0-1.x86_64.rpm
```

For Fedora, RHEL 8 and newer. Remove it with `sudo dnf remove fastmash`.

## Check that it works

```sh
printf '1\t2\n3\t4\n' | fastmash sum 1 mean 2
```

This prints `4` and `3`, separated by a tab. Next: the
[quick start](quick-start.md).

## On Windows

Install [WSL2](https://learn.microsoft.com/en-us/windows/wsl/install), then
install Fastmash inside your Linux distribution as above. Keep your data in
the Linux file system (such as your home directory) rather than under
`/mnt/c`: it is much faster.

## Uninstall

```sh
rm ~/.local/bin/fastmash
# On Linux, also remove the supervisor:
rm ~/.local/bin/fastmash-sort-supervisor
# or, if installed with Cargo or cargo-binstall:
cargo uninstall fastmash
```

Fastmash keeps no configuration files, caches or background services.

Building from source, verifying release archives and the full system
requirements are described in [System requirements](requirements.md).
