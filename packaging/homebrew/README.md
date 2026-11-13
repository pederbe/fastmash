# Homebrew installation

Homebrew installs the same Linux and macOS programs as the published Fastmash
release. It does not compile Fastmash or create another packaged build.

```sh
brew install pederbe/tap/fastmash
```

Check the installation with:

```sh
printf '1\n2\n' | fastmash sum 1
```

The command prints `3`. Update Fastmash with `brew upgrade fastmash`, or remove
it with `brew uninstall pederbe/tap/fastmash`.

## Maintenance

`fastmash.rb` supplies the download layout and installation instructions. Its
URLs and checksums identify release archives, so the recipe works on its own.
Keep it compatible with archives produced by the release pipeline. The archives
contain the executable, documentation and licenses. The
[installation repository](https://github.com/pederbe/homebrew-tap) uses this
recipe as its template and selects published asset URLs and checksums there.

Release qualification remains responsible for application correctness. The
small `check-install.sh` helper checks only the Homebrew delivery path. It
checks the version, compares the installed executable with the downloaded
archive and verifies that licenses are installed. The installation repository
also uses it to check upgrades from the previous recipe. Both repositories
check removal afterwards.

The [Homebrew checks](../../.github/workflows/homebrew.yml) install published
archives, not programs rebuilt from the pull request. Application changes
continue through ordinary application and release checks.

For a local check, create a disposable test repository with
`brew tap-new --no-git OWNER/fastmash-test`, copy `fastmash.rb` to its `Formula`
directory, then run:

```sh
brew style OWNER/fastmash-test/fastmash
brew audit --except=installed OWNER/fastmash-test/fastmash
bash packaging/homebrew/check-install.sh OWNER/fastmash-test/fastmash
brew uninstall OWNER/fastmash-test/fastmash
```

See the [installation repository's maintenance guide](https://github.com/pederbe/homebrew-tap/blob/main/CONTRIBUTING.md)
for automatic updates and recovery.
