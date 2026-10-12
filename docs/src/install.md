# Install

Fastmash runs on **Linux x86-64** (glibc), **Windows through WSL2** and
**Apple Silicon macOS 15 and later**.

## Quick install

```sh
curl -fsSL https://fastmash.io/install.sh | sh
```

Installs the latest release into `~/.local/bin`. Add that directory to your
`PATH` if needed.

## Prebuilt binary, by hand

On Linux or WSL2, download an archive from the
[releases page](https://github.com/pederbe/fastmash/releases), or run this in
Bash (enter `bash` first if using Fish):

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

Replace `0.2.0` with the version you want. Keep `fastmash` and
`fastmash-sort-supervisor` from the same release in the same directory.

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

Homebrew builds from source. Remove it with
`brew uninstall pederbe/tap/fastmash`.

## Native macOS archive

The quick installer chooses the macOS archive automatically. To download
version 0.2.0 by hand:

```sh
curl -fLO https://github.com/pederbe/fastmash/releases/download/v0.2.0/fastmash-v0.2.0-aarch64-apple-darwin.tar.gz
curl -fLO https://github.com/pederbe/fastmash/releases/download/v0.2.0/fastmash-v0.2.0-aarch64-apple-darwin.tar.gz.sha256
```

Then install it in Bash (enter `bash` first if using Fish). Set
`FASTMASH_VERSION` if you downloaded a different version:

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

The final command prints `3`.

## Debian and Ubuntu

The signed apt repository provides updates through apt on Linux x86-64. Install
the tools needed to download and inspect its key:

```sh
sudo apt install ca-certificates curl gnupg
curl -fsSLo fastmash.gpg https://packages.fastmash.io/fastmash.gpg
gpg --show-keys --fingerprint fastmash.gpg
```

Check that the key fingerprint is:

```text
76C1 7E23 B7D3 F8FD ED36 D31A 7BAC 7AF6 644B D290
```

Then add the repository and install Fastmash:

```sh
sudo install -m 644 fastmash.gpg /usr/share/keyrings/fastmash.gpg
echo "deb [arch=amd64 signed-by=/usr/share/keyrings/fastmash.gpg] https://packages.fastmash.io/apt stable main" |
  sudo tee /etc/apt/sources.list.d/fastmash.list
sudo apt update
sudo apt install fastmash
```

Later versions arrive through `sudo apt update` and `sudo apt upgrade`.
The key is trusted only for this repository. Both 0.1.0 and 0.2.0 remain available.

To install a single release without adding the repository:

```sh
curl -fLO https://github.com/pederbe/fastmash/releases/download/v0.2.0/fastmash_0.2.0_amd64.deb
sudo apt install ./fastmash_0.2.0_amd64.deb
```

For Debian 10, Ubuntu 20.04 and newer. Remove it with `sudo apt remove fastmash`.

## Fedora, RHEL and similar

The signed dnf repository provides updates on Linux x86-64, with package and
repository signature checks enabled:

```sh
sudo curl -fsSLo /etc/yum.repos.d/fastmash.repo https://packages.fastmash.io/fastmash.repo
sudo dnf install fastmash
```

dnf asks to trust the signing key on first use. Before confirming, check that
its fingerprint is `76C1 7E23 B7D3 F8FD ED36 D31A 7BAC 7AF6 644B D290`,
the same key used by the apt repository above. Update with
`sudo dnf upgrade fastmash`. Both 0.1.0 and 0.2.0 remain available.

To install a single release without adding the repository:

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

## Installer options

Set `FASTMASH_INSTALL_DIR` to choose another installation directory, or
`FASTMASH_VERSION` to choose a release.

## Uninstall

```sh
rm ~/.local/bin/fastmash
# On Linux, also remove the supervisor:
rm ~/.local/bin/fastmash-sort-supervisor
# or, if installed with Cargo or cargo-binstall:
cargo uninstall fastmash
```

Fastmash keeps no configuration files, caches or background services.

See [System requirements](requirements.md) for building from source and
[verifying release archives](requirements.md#verifying-a-release-archive).
