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

Download individual `.deb` and `.rpm` packages or a platform archive from the
[latest release](https://github.com/pederbe/fastmash/releases/latest).
Use the [quick installer](#quick-install) to download, verify and install the
archive for your system automatically.

## With Cargo

If you already use Rust, Cargo offers two routes.

[cargo-binstall](https://github.com/cargo-bins/cargo-binstall) downloads the
same prebuilt executables as the release archives, without compiling.
On Linux or WSL2, run:

```sh
cargo binstall fastmash
```

On macOS 15 or later, select its single executable:

```sh
cargo binstall fastmash --bin fastmash
```

Other platforms have no prebuilt Fastmash executable. If the download is not
available, cargo-binstall offers to compile instead. Add
`--strategies crate-meta-data` to stop with an error instead.

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

The quick installer removes an old helper during an upgrade when its checksum
matches the earlier installation's ownership receipt. An installation without
a receipt, or with a changed helper, leaves that file in place. Current releases
do not use it. You can remove it if it belongs to the installation you upgraded
or uninstalled:

```sh
rm ~/.local/bin/fastmash-sort-supervisor
```

See [System requirements](requirements.md) for building from source and
[verifying release archives](requirements.md#verifying-a-release-archive).
