# Install

Fastmash runs on **Linux**, **Windows through WSL2** and
**macOS 15 and later**.

## Choose a method

Use your system's package manager when you can. It installs prebuilt
programs and brings you later versions with your other updates.

| Your system | Recommended | Updates |
| --- | --- | --- |
| Debian, Ubuntu | [apt repository](#debian-and-ubuntu) | `sudo apt upgrade` |
| Fedora, RHEL and similar | [dnf repository](#fedora-rhel-and-similar) | `sudo dnf upgrade` |
| macOS, or Linux with Homebrew | [Homebrew](#homebrew) | `brew upgrade` |
| Any supported system | [Quick install](#quick-install) | Run it again |

All of these install prebuilt programs. If you prefer to compile Fastmash
yourself, see [Cargo](#with-cargo).

## Debian and Ubuntu

The signed apt repository provides updates through apt on Linux. Install
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
The key is trusted only for this repository.

For Debian 10, Ubuntu 20.04 and newer. Remove it with `sudo apt remove fastmash`.

## Fedora, RHEL and similar

The signed dnf repository provides updates on Linux, with package and
repository signature checks enabled:

```sh
sudo curl -fsSLo /etc/yum.repos.d/fastmash.repo https://packages.fastmash.io/fastmash.repo
sudo dnf install fastmash
```

dnf asks to trust the signing key on first use. Before confirming, check that
its fingerprint is `76C1 7E23 B7D3 F8FD ED36 D31A 7BAC 7AF6 644B D290`,
the same key used by the apt repository above. Update with
`sudo dnf upgrade fastmash`.

For Fedora, RHEL 8 and newer. Remove it with `sudo dnf remove fastmash`.

## Homebrew

With [Homebrew](https://brew.sh) on macOS 15 or later, or on Linux:

```sh
brew install pederbe/tap/fastmash
```

Homebrew downloads the same tested programs as the Fastmash release archives.
Installation needs no Rust compiler and does not rebuild Fastmash.

Update with `brew upgrade fastmash` and remove it with
`brew uninstall pederbe/tap/fastmash`.

## Quick install

```sh
curl -fsSL https://fastmash.io/install.sh | sh
```

Downloads the latest release archive for your system, checks its checksum and
installs it into `~/.local/bin`. Add that directory to your `PATH` if needed.
Run the same command again to update.

Set `FASTMASH_INSTALL_DIR` to choose another installation directory.

## Single packages and archives

These downloads install the latest release without adding a repository. You
update them yourself.

### Arch Linux package

Use the [Arch package recipe](https://github.com/pederbe/fastmash/blob/main/packaging/aur/README.md)
to install the prebuilt executable through makepkg and pacman. No Rust compiler
is needed. To update, rebuild the package from the current recipe.

### Debian or RPM package

Each command looks up the latest release, then downloads and installs its
package. On Debian or Ubuntu:

```sh
tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' https://github.com/pederbe/fastmash/releases/latest)
tag=${tag##*/}
curl -fLO "https://github.com/pederbe/fastmash/releases/download/$tag/fastmash_${tag#v}_amd64.deb"
sudo apt install "./fastmash_${tag#v}_amd64.deb"
```

On Fedora, RHEL and similar:

```sh
tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' https://github.com/pederbe/fastmash/releases/latest)
tag=${tag##*/}
sudo dnf install "https://github.com/pederbe/fastmash/releases/download/$tag/fastmash-${tag#v}-1.x86_64.rpm"
```

### Linux archive

On Linux or WSL2, download an archive from the
[latest release](https://github.com/pederbe/fastmash/releases/latest), or run this in
Bash (enter `bash` first if using Fish):

```bash
bash -eu <<'EOF'
tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' https://github.com/pederbe/fastmash/releases/latest)
tag=${tag##*/}
name=fastmash-$tag-x86_64-unknown-linux-musl
if ! curl -fLO https://github.com/pederbe/fastmash/releases/download/$tag/$name.tar.gz; then
  if command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; then
    printf '%s\n' 'The latest release has no static Linux archive.' >&2
    exit 1
  fi
  name=fastmash-$tag-x86_64-unknown-linux-gnu
  curl -fLO https://github.com/pederbe/fastmash/releases/download/$tag/$name.tar.gz
fi
curl -fLO https://github.com/pederbe/fastmash/releases/download/$tag/$name.tar.gz.sha256
sha256sum -c $name.tar.gz.sha256
tar -xzf $name.tar.gz
mkdir -p ~/.local/bin
cp "$name/fastmash" ~/.local/bin/
EOF
```

### macOS archive

The quick installer chooses the macOS archive automatically. To download
the latest archive by hand, run this in an empty directory:

```sh
tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' https://github.com/pederbe/fastmash/releases/latest)
tag=${tag##*/}
curl -fLO "https://github.com/pederbe/fastmash/releases/download/$tag/fastmash-$tag-aarch64-apple-darwin.tar.gz"
curl -fLO "https://github.com/pederbe/fastmash/releases/download/$tag/fastmash-$tag-aarch64-apple-darwin.tar.gz.sha256"
```

Then install it from the same directory in Bash (enter `bash` first if using
Fish):

```bash
bash -eu <<'EOF'
set -- fastmash-v*-aarch64-apple-darwin.tar.gz
name=${1%.tar.gz}
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
same prebuilt executables as the release archives, without compiling.
On Linux or WSL2, run:

```sh
cargo binstall fastmash --strategies crate-meta-data
```

On macOS 15 or later, select its single executable:

```sh
cargo binstall fastmash --strategies crate-meta-data --bin fastmash
```

These commands install the published binaries and stop with an error if the
download is unavailable. Other platforms have no prebuilt Fastmash executable.

To compile Fastmash from source with `cargo install`, use the latest stable
[Rust toolchain](https://rustup.rs). On Linux or WSL2:

```sh
rustup target add x86_64-unknown-linux-musl
cargo install --locked --target x86_64-unknown-linux-musl fastmash
```

On macOS 15 or later, select its single executable:

```sh
cargo install --locked --bin fastmash fastmash
```

## Check that it works

```sh
printf '1\t2\n3\t4\n' | fastmash sum 1 mean 2
```

The summary shows `4` for the sum and `3` for the mean. Next: the
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
```

If you installed with Cargo or cargo-binstall:

```sh
cargo uninstall fastmash
```

Fastmash keeps no configuration files, caches or background services.

## Older Linux installations

Early releases installed a second executable, `fastmash-sort-supervisor`,
which Fastmash no longer uses.

The quick installer removes an old helper during an upgrade when its checksum
matches the earlier installation's ownership receipt. An installation without
a receipt, or with a changed helper, leaves that file in place. You can
remove it if it belongs to the installation you upgraded
or uninstalled:

```sh
rm ~/.local/bin/fastmash-sort-supervisor
```

See [System requirements](requirements.md) for building from source and
[verifying release archives](requirements.md#verifying-a-release-archive).
