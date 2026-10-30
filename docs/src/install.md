# Install

Fastmash runs on **Linux x86-64** (glibc), **Windows through WSL2** and
**Apple Silicon macOS 15 and later**. The current release is @VERSION@.

## Choose a method

Use your system's package manager when you can. It installs prebuilt
programs and brings you later versions with your other updates.

| Your system | Recommended | Updates |
| --- | --- | --- |
| Debian, Ubuntu | [apt repository](#debian-and-ubuntu) | `sudo apt upgrade` |
| Fedora, RHEL and similar | [dnf repository](#fedora-rhel-and-similar) | `sudo dnf upgrade` |
| Apple Silicon macOS, or Linux with Homebrew | [Homebrew](#homebrew) | `brew upgrade` |
| Any supported system | [Quick install](#quick-install) | Run it again |

All of these install prebuilt programs. If you prefer to compile Fastmash
yourself, see [Cargo](#with-cargo).

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
The key is trusted only for this repository. Earlier versions remain available.

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
`sudo dnf upgrade fastmash`. Earlier versions remain available.

For Fedora, RHEL 8 and newer. Remove it with `sudo dnf remove fastmash`.

## Homebrew

With [Homebrew](https://brew.sh) on Apple Silicon macOS 15 or later, or on
Linux x86-64:

```sh
brew install pederbe/tap/fastmash
```

Homebrew downloads a prebuilt bottle for macOS 15, macOS 26 and Linux x86-64,
so you do not need Rust. If no bottle matches your setup, such as a Homebrew
installed outside its default location, it builds from source automatically.
This takes a few minutes and needs no action from you. Bottles are built by
Homebrew from the published source, separately from the release archives.

Update with `brew upgrade fastmash` and remove it with
`brew uninstall pederbe/tap/fastmash`.

## Quick install

```sh
curl -fsSL https://fastmash.io/install.sh | sh
```

Downloads the latest release archive for your system, checks its checksum and
installs it into `~/.local/bin`. Add that directory to your `PATH` if needed.
Run the same command again to update.

When upgrading from a two-executable Linux release, the installer removes the
old Sort supervisor only if its checksum matches the ownership receipt from
an earlier installation. An older installation without that receipt, or a
changed helper, stays in place. After upgrading, you may remove that unused
`fastmash-sort-supervisor` yourself if it belongs to this installation.

Set `FASTMASH_INSTALL_DIR` to choose another installation directory, or
`FASTMASH_VERSION` to choose a release.

## Single packages and archives

These downloads install one release without adding a repository. You update
them yourself.

### Debian or RPM package

```sh
curl -fLO https://github.com/pederbe/fastmash/releases/download/v@VERSION@/fastmash_@VERSION@_amd64.deb
sudo apt install ./fastmash_@VERSION@_amd64.deb
```

```sh
sudo dnf install https://github.com/pederbe/fastmash/releases/download/v@VERSION@/fastmash-@VERSION@-1.x86_64.rpm
```

### Linux archive

On Linux or WSL2, download an archive from the
[releases page](https://github.com/pederbe/fastmash/releases), or run this in
Bash (enter `bash` first if using Fish):

```bash
bash -eu <<'EOF'
version=@VERSION@
name=fastmash-v$version-x86_64-unknown-linux-gnu
curl -fLO https://github.com/pederbe/fastmash/releases/download/v$version/$name.tar.gz
curl -fLO https://github.com/pederbe/fastmash/releases/download/v$version/$name.tar.gz.sha256
sha256sum -c $name.tar.gz.sha256
tar -xzf $name.tar.gz
mkdir -p ~/.local/bin
cp "$name/fastmash" ~/.local/bin/
if [ -f "$name/fastmash-sort-supervisor" ]; then
  cp "$name/fastmash-sort-supervisor" ~/.local/bin/
fi
EOF
```

Replace `@VERSION@` with the version you want. Existing published Linux archives
may also contain `fastmash-sort-supervisor`. When present, keep that helper
beside `fastmash` from the same release. Archives built with native sorting
everywhere contain only `fastmash`.

### macOS archive

The quick installer chooses the macOS archive automatically. To download
version @VERSION@ by hand:

```sh
curl -fLO https://github.com/pederbe/fastmash/releases/download/v@VERSION@/fastmash-v@VERSION@-aarch64-apple-darwin.tar.gz
curl -fLO https://github.com/pederbe/fastmash/releases/download/v@VERSION@/fastmash-v@VERSION@-aarch64-apple-darwin.tar.gz.sha256
```

Then install it in Bash (enter `bash` first if using Fish). Set
`FASTMASH_VERSION` if you downloaded a different version:

```bash
bash -eu <<'EOF'
version=${FASTMASH_VERSION:-@VERSION@}
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

## With Cargo

If you already use Rust, Cargo offers two routes.

[cargo-binstall](https://github.com/cargo-bins/cargo-binstall) downloads the
same prebuilt executables as the release archives, without compiling. On Linux
x86-64 or WSL2, run:

```sh
cargo binstall fastmash
```

Existing published Linux releases may also install their Sort supervisor.
Releases containing the native sorting migration install only `fastmash`.

On Apple Silicon macOS 15 or later, select its single executable:

```sh
cargo binstall fastmash --bin fastmash
```

Other platforms have no prebuilt Fastmash executable. If the download is not
available, cargo-binstall offers to compile instead. Add
`--strategies crate-meta-data` to stop with an error instead.

`cargo install` compiles Fastmash from source with your
[Rust toolchain](https://rustup.rs). On Linux or WSL2:

```sh
cargo install --locked fastmash
```

On Apple Silicon macOS 15 or later, select its single executable:

```sh
cargo install --locked --bin fastmash fastmash
```

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

Package managers remove Fastmash with their own commands, shown above.
For the quick installer or a downloaded archive:

```sh
rm ~/.local/bin/fastmash
# If installed by an earlier Linux release, also remove its helper:
rm ~/.local/bin/fastmash-sort-supervisor
```

If you installed with Cargo or cargo-binstall:

```sh
cargo uninstall fastmash
```

Fastmash keeps no configuration files, caches or background services.

See [System requirements](requirements.md) for building from source and
[verifying release archives](requirements.md#verifying-a-release-archive).
