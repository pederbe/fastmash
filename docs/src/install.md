# Install

Fastmash runs on **Linux**, **macOS 15 and later**, and **Windows 11**.
For Windows, use [PowerShell 7.4 or later](#on-windows).

## Choose a method

Use your system's package manager when you can. It installs prebuilt
programs and brings you later versions with your other updates.

| Your system | Recommended | Updates |
| --- | --- | --- |
| Debian, Ubuntu | [apt repository](#debian-and-ubuntu) | `sudo apt update`, then `sudo apt install --only-upgrade fastmash` |
| Fedora, RHEL and similar | [dnf repository](#fedora-rhel-and-similar) | `sudo dnf upgrade fastmash` |
| macOS, or Linux with Homebrew | [Homebrew](#homebrew) | `brew upgrade fastmash` |
| Linux, macOS, or WSL2 | [Quick install](#quick-install) | Run it again |
| Windows | [Native archive](#on-windows) | Replace the extracted folder |

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

On Linux, macOS or inside WSL2:

```sh
curl -fsSL https://fastmash.io/install.sh | sh
```

Downloads the latest release archive for your system, checks its checksum and
installs it into `~/.local/bin`. The installer prints a command to try when it
finishes. If your shell cannot find `fastmash`, see [Command not found](#command-not-found).
Run the same command again to update this installation.

Choose another writable directory with `FASTMASH_INSTALL_DIR`:

```sh
curl -fsSL https://fastmash.io/install.sh | env FASTMASH_INSTALL_DIR="$HOME/bin" sh
```

Use the same directory when updating. To read the installer's help without
installing anything:

```sh
curl -fsSL https://fastmash.io/install.sh | sh -s -- --help
```

If a download or checksum check fails, your existing installation stays in
place. Check your connection and retry. For a permission error, choose a
directory you can write to with `FASTMASH_INSTALL_DIR`.

## Single packages and archives

These downloads install the latest release without adding a repository. You
update them yourself.

### Arch Linux package

Use the [Arch package recipe](https://github.com/pederbe/fastmash/blob/main/packaging/aur/README.md)
to install the prebuilt executable through makepkg and pacman. No Rust compiler
is needed. To update, rebuild the package from the current recipe.

### Debian or RPM package

Each block looks up the latest release, then downloads and installs its
package. Run it in Bash, entering `bash` first if you use Fish.
On Debian or Ubuntu:

```bash
bash -eu <<'EOF'
tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' https://github.com/pederbe/fastmash/releases/latest)
tag=${tag##*/}
curl -fLO "https://github.com/pederbe/fastmash/releases/download/$tag/fastmash_${tag#v}_amd64.deb"
sudo apt install "./fastmash_${tag#v}_amd64.deb"
EOF
```

On Fedora, RHEL and similar:

```bash
bash -eu <<'EOF'
tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' https://github.com/pederbe/fastmash/releases/latest)
tag=${tag##*/}
sudo dnf install "https://github.com/pederbe/fastmash/releases/download/$tag/fastmash-${tag#v}-1.x86_64.rpm"
EOF
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
curl -fLO https://github.com/pederbe/fastmash/releases/download/$tag/$name.tar.gz
curl -fLO https://github.com/pederbe/fastmash/releases/download/$tag/$name.tar.gz.sha256
sha256sum -c $name.tar.gz.sha256
tar -xzf $name.tar.gz
if [ ! -f "$name/fastmash" ] || [ -L "$name/fastmash" ]; then
  printf '%s\n' 'The archive has no regular fastmash executable. No installation was changed.' >&2
  exit 1
fi
mkdir -p ~/.local/bin
staging=$(mktemp -d "$HOME/.local/bin/.fastmash.XXXXXX")
trap 'rm -rf "$staging"' EXIT
cp "$name/fastmash" "$staging/fastmash"
chmod 755 "$staging/fastmash"
mv -f "$staging/fastmash" "$HOME/.local/bin/fastmash"
printf '1\n2\n' | ~/.local/bin/fastmash sum 1
EOF
```

The final command prints `3`. To update, repeat these steps in an empty directory.

### macOS archive

The quick installer chooses the macOS archive automatically. To download
the latest archive by hand, run this in Bash in an empty directory. Enter
`bash` first if using Fish:

```sh
bash -eu <<'EOF'
tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' https://github.com/pederbe/fastmash/releases/latest)
tag=${tag##*/}
curl -fLO "https://github.com/pederbe/fastmash/releases/download/$tag/fastmash-$tag-aarch64-apple-darwin.tar.gz"
curl -fLO "https://github.com/pederbe/fastmash/releases/download/$tag/fastmash-$tag-aarch64-apple-darwin.tar.gz.sha256"
EOF
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
mkdir -p ~/.local/bin
staging=$(mktemp -d "$HOME/.local/bin/.fastmash.XXXXXX")
trap 'rm -rf "$staging"' EXIT
cp "$name/fastmash" "$staging/fastmash"
chmod 755 "$staging/fastmash"
mv -f "$staging/fastmash" ~/.local/bin/fastmash
printf '1\n2\n' | ~/.local/bin/fastmash sum 1
EOF
```

The final command prints `3`. To update, repeat the download and installation
steps in an empty directory.

## With Cargo

If you already use Rust, Cargo offers two routes.

[cargo-binstall](https://github.com/cargo-bins/cargo-binstall) downloads the
same prebuilt executables as the release archives, without compiling.
On Linux or WSL2, run:

```sh
cargo binstall fastmash --strategies crate-meta-data
```

On macOS or Windows, select its single executable:

```sh
cargo binstall fastmash --strategies crate-meta-data --bin fastmash
```

These commands install the published binaries and stop with an error if the
download is unavailable.

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

In Bash, or use the [PowerShell example](quick-start.md#powershell-74):

```sh
printf '1\t2\n3\t4\n' | fastmash sum 1 mean 2
```

The summary shows `4` for the sum and `3` for the mean. Next: the
[quick start](quick-start.md).

## Command not found

For a quick installation or an archive installed in the default directory,
try the executable directly:

```sh
~/.local/bin/fastmash --version
```

`PATH` lists the directories your shell searches for commands, in order.
If the direct command prints the version, add `~/.local/bin` to `PATH`. In Fish:

```fish
fish_add_path "$HOME/.local/bin"
```

In Bash, add this line to `~/.bashrc`. In Zsh, add it to `~/.zshrc`, then open
a new terminal:

```text
export PATH="$HOME/.local/bin:$PATH"
```

If Bash starts a login shell, add the line to `~/.bash_profile` instead.
Replace `~/.local/bin` with your chosen directory if you set
`FASTMASH_INSTALL_DIR`.

To see which installation your shell uses:

```sh
command -v fastmash
```

Update that installation with its original method, or put your preferred
installation directory first in `PATH`. In Fish, `fish_add_path --move`
moves an existing directory to the front.

## On Windows

Use [PowerShell 7.4 or later](https://learn.microsoft.com/en-us/powershell/scripting/install/installing-powershell-on-windows) on Windows 11.

1. Open the [release downloads](https://github.com/pederbe/fastmash/releases/latest)
   and choose the archive ending in `x86_64-pc-windows-msvc.tar.gz`.
2. Extract it and keep the extracted folder somewhere permanent.
3. Search Windows for **Edit environment variables for your account**. Edit
   **Path** and add the folder containing `fastmash.exe`.
4. Open a new PowerShell terminal and run:

   ```powershell
   fastmash --version
   ```

It prints the installed version. Continue with the
[PowerShell quick start](quick-start.md#powershell-74) for files and pipelines.
To update, replace the extracted folder and keep its location on `Path`.
To uninstall, remove that folder and its `Path` entry. No Rust compiler or
separate Visual C++ runtime installation is needed.

For Linux workflows, you can also use [WSL2](https://learn.microsoft.com/en-us/windows/wsl/install)
and follow the Linux instructions. Keep WSL2 data in the Linux file system rather
than under `/mnt/c`.

## Uninstall

Package managers remove Fastmash with their own commands, shown above.
For the quick installer or a downloaded archive:

```sh
rm ~/.local/bin/fastmash
```

If you chose another directory with `FASTMASH_INSTALL_DIR`, remove its
`fastmash` file instead. Removing the executable does not remove your data.

If you installed with Cargo or cargo-binstall:

```sh
cargo uninstall fastmash
```

Fastmash keeps no configuration files, caches or background services.

See [System requirements](requirements.md) for building from source and
[verifying release archives](requirements.md#verifying-a-release-archive).
