# Install

Fastmash runs on **Linux x86-64** and on **Windows through WSL2**. macOS on
Apple Silicon is a [later target](roadmap.md). It installs next to GNU datamash
without changing it. Native Windows is outside the current product scope.

## Quick install

```sh
curl -fsSL https://fastmash.io/install.sh | sh
```

The [script](https://fastmash.io/install.sh) downloads the latest release,
checks its checksum and installs `fastmash` and `fastmash-sort-supervisor`
into `~/.local/bin`. Set `FASTMASH_INSTALL_DIR` to install elsewhere, or
`FASTMASH_VERSION` for a specific release.

The script checks the download against its published SHA-256 checksum. For a
stronger check that the archive was built by this project's release workflow,
download it by hand and verify its GitHub build attestation:

```sh
gh attestation verify fastmash-v0.1.0-x86_64-unknown-linux-gnu.tar.gz --repo pederbe/fastmash
```

## Prebuilt binary, by hand

Download a release from the
[releases page](https://github.com/pederbe/fastmash/releases), check it and
copy the two programs into a directory on your `PATH`:

```sh
version=0.1.0
name=fastmash-v$version-x86_64-unknown-linux-gnu
curl -LO https://github.com/pederbe/fastmash/releases/download/v$version/$name.tar.gz
curl -LO https://github.com/pederbe/fastmash/releases/download/v$version/$name.tar.gz.sha256
sha256sum -c $name.tar.gz.sha256
tar -xzf $name.tar.gz
mkdir -p ~/.local/bin
cp $name/fastmash $name/fastmash-sort-supervisor ~/.local/bin/
```

Replace `0.1.0` with the release you want, and keep `fastmash` and
`fastmash-sort-supervisor` from the same release in the same directory. If the
supervisor is missing, some large sorted jobs are slower; if it comes from
another release, the sorted jobs that use it stop with a message saying so.

## With Cargo

If you have a [Rust toolchain](https://rustup.rs):

```sh
cargo install --locked fastmash
```

With [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), Cargo
downloads the prebuilt release binaries instead of compiling:

```sh
cargo binstall fastmash
```

## Debian and Ubuntu

```sh
curl -LO https://github.com/pederbe/fastmash/releases/download/v0.1.0/fastmash_0.1.0_amd64.deb
sudo apt install ./fastmash_0.1.0_amd64.deb
```

For Debian 10, Ubuntu 20.04 and newer. Remove it with `sudo apt remove fastmash`.

## Fedora, RHEL and similar

```sh
sudo dnf install https://github.com/pederbe/fastmash/releases/download/v0.1.0/fastmash-0.1.0-1.x86_64.rpm
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
rm ~/.local/bin/fastmash ~/.local/bin/fastmash-sort-supervisor
# or, if installed with Cargo or cargo-binstall:
cargo uninstall fastmash
```

Fastmash keeps no configuration files, caches or background services.

Building from source, verifying release archives and the full system
requirements are described in [System requirements](requirements.md).
