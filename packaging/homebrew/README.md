# Homebrew installation

Homebrew installs the same Linux and macOS programs as the published Fastmash
release. It does not compile Fastmash or create another packaged build.

```sh
brew install pederbe/tap/fastmash
```

Existing installations receive updates with `brew upgrade`. Revision 1 of the
0.4.0 recipe moves existing Homebrew-built installations to the release files.
The next Fastmash version starts at revision zero again.

## Maintenance

`fastmash.rb` supplies the download layout and installation instructions. Its
URLs and checksums name the current published release, so the recipe works on
its own. Keep it compatible with the archives already produced by the release
pipeline. The archives contain the executable, documentation and licenses.

The installation repository's daily `Update` workflow reads this recipe from
Fastmash's `main`, selects the latest published release and replaces the URLs
and checksums with that release's asset metadata. It checks the installation
and upgrade before committing the recipe. A failed check leaves the existing
installation recipe available and can be retried without rebuilding Fastmash
or republishing a release.

Release qualification remains responsible for application correctness. The
small `check-install.sh` helper checks only the Homebrew delivery path. It
checks the version, compares the installed executable with the downloaded
archive and verifies that licenses are installed. The installation repository
also uses it to check upgrades from the previous recipe. Both repositories
check removal afterwards.

Changes to this recipe or helper run the existing Homebrew workflow on macOS
15, macOS 26 and Linux. These jobs install the published archives, not programs
rebuilt from the pull request. Application changes continue through the
ordinary application and release checks.

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
