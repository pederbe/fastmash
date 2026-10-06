# Homebrew formula

`fastmash.rb` builds the published 0.1.0 source on Linux x86-64 with the
locked Cargo dependencies. It installs `fastmash` and its Sort supervisor
together, plus both project licenses and the generated dependency licenses,
copyright and notice texts. Python and Rust are build dependencies only.
The source-built programs carry no performance claim from the release binaries.

The formula is published in the [maintainer's tap](https://github.com/pederbe/homebrew-tap):

```sh
brew install pederbe/tap/fastmash
```

Homebrew's
[acceptance policy](https://docs.brew.sh/Package-Acceptance-Policy) includes
repository age and public-interest requirements for `homebrew/core`.
Review those requirements before proposing a core submission.
The Linux restriction follows the published release's platform scope.

## Build and check

Use Homebrew's supported Linux prefix, `/home/linuxbrew/.linuxbrew`, with
the normal [Linux build prerequisites](https://docs.brew.sh/Homebrew-on-Linux).
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

The test requires nonempty licenses and notices, the exact version,
arithmetic and a transcendental result, and a paired operation with an exact
`system sort` trace. A correct result from fallback sorting cannot pass it.
Testing this optional route requires Linux 5.11 or later and GNU or uutils
coreutils at `/usr/bin/sort`. Older kernels can install Fastmash and use its
built-in sorting, but cannot pass this route check.

After removal, confirm that `fastmash` and `fastmash-sort-supervisor` are
absent from the prefix's `bin/` and that the formula's Cellar directory is gone.

## Maintenance

Keep this recipe and `Formula/fastmash.rb` in the tap in sync when updating
the published source version. After publishing an update, install from that
tap in a fresh environment, run `brew test`, check the installed licenses,
and verify removal before updating the installation guide.
