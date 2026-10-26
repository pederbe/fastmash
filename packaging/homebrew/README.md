# Homebrew formula

`fastmash.rb` is the recipe published in the
[maintainer's tap](https://github.com/pederbe/homebrew-tap):

```sh
brew install pederbe/tap/fastmash
```

It builds the published source on Linux x86-64 and Apple Silicon macOS 15 or
later with the locked Cargo dependencies. Linux installs `fastmash` and its
Sort supervisor together. macOS installs only `fastmash`. Both include the
project licenses and generated dependency licenses, copyright and notice texts
for their target. Rust is a build dependency only. The tap publishes bottles,
which are prebuilt Homebrew packages, for macOS 15, macOS 26 and Linux x86-64.
These Homebrew builds carry no performance claim from the release binaries.

The recipe compiles `run_test.rs` from the same source during the build and
installs the checker under `libexec`. `brew test` runs this checker without a
compiler in its test dependencies.

## How releases reach the tap

The tap's `Update` workflow runs daily. When the latest published Fastmash
release is newer than the tap's formula, it takes this recipe from the release
tag and sets the tag's source URL and checksum. It then builds and tests
bottles on GitHub-hosted runners, publishes them, and checks installation,
upgrade from the previous version and removal from the published tap.
It does nothing when the versions match. A failure leaves the tap unchanged,
and does not affect the Fastmash release or its other packages. See the
[tap's maintenance guide](https://github.com/pederbe/homebrew-tap/blob/main/CONTRIBUTING.md)
for retries.

Change the recipe here, in the same pull request as any source change it
depends on. The `Homebrew recipe checks` workflow builds the changed recipe
from the pull request's own source on macOS 15, macOS 26 and Linux x86-64, and
runs its test, audit, style check and removal check. The `url` and `sha256`
lines name the latest release that the tap published. The tap's workflow
replaces them for each new release.

Homebrew's
[acceptance policy](https://docs.brew.sh/Package-Acceptance-Policy) includes
repository age and public-interest requirements for `homebrew/core`.
Review those requirements before proposing a core submission.

## Build and check locally

Use Homebrew's supported prefix: `/home/linuxbrew/.linuxbrew` on Linux with
the normal [Linux build prerequisites](https://docs.brew.sh/Homebrew-on-Linux),
or `/opt/homebrew` on Apple Silicon macOS 15 or later.
Create a local test tap with `brew tap-new OWNER/fastmash-test`, then copy
`fastmash.rb` into its `Formula/` directory. Substitute your GitHub username
for `OWNER` in these commands:

```sh
brew install --build-from-source OWNER/fastmash-test/fastmash
brew test OWNER/fastmash-test/fastmash
brew audit --strict OWNER/fastmash-test/fastmash
brew style OWNER/fastmash-test/fastmash
brew uninstall OWNER/fastmash-test/fastmash
```

On both platforms the test requires nonempty licenses and notices, the exact
version, arithmetic and a transcendental result, and a paired operation.
On Linux it requires the Sort supervisor and an exact `system sort` trace.
A correct result from fallback sorting cannot pass it. Testing this optional
route requires Linux 5.11 or later and GNU or uutils coreutils at `/usr/bin/sort`.
Older kernels can install Fastmash and use its built-in sorting, but cannot
pass this route check. On macOS the test requires a `native sort` trace and
confirms that no Sort supervisor is installed.

After removal, confirm that `fastmash` and, on Linux, `fastmash-sort-supervisor`
are absent from the prefix's `bin/` and that the formula's Cellar directory is gone.
