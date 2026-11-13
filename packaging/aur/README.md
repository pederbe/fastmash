# Arch Linux package

Build a local pacman package from Fastmash's prebuilt Linux executable.
No Rust compiler or AUR account is needed.

Install Arch's `base-devel` tools and `curl`, then download the recipe into
an empty directory:

```sh
sudo pacman -S --needed base-devel curl
mkdir fastmash-package
cd fastmash-package
curl -fsSLo PKGBUILD https://raw.githubusercontent.com/pederbe/fastmash/main/packaging/aur/PKGBUILD
makepkg --syncdeps --install
```

The recipe checks the download's checksum and runs a small command check.
Pacman installs the executable, documentation and licenses. Check it with:

```sh
printf '1\n2\n' | fastmash sum 1
```

The command prints `3`.

To update, download the current recipe into the same directory and run
`makepkg --syncdeps --install` again. These local packages do not receive
updates through `pacman -Syu`. Remove Fastmash with `sudo pacman -R fastmash-bin`.

## Maintenance

For a published release, update `pkgver`, the archive name in `_name` and
`sha256sums` from its actual Linux asset, then reset `pkgrel` to `1`. Keep the
archive URL, target suffix and checksum together so makepkg verifies the exact
release it packages.

Run makepkg, install the resulting package with pacman, check the command and
installed documentation, then check removal before committing the update.
