# Homebrew formula

`fastmash.rb` builds the published 0.2.0 source on Linux x86-64 and
Apple Silicon macOS 15 or later with the locked Cargo dependencies.
Linux installs `fastmash` and its Sort supervisor together; macOS installs
only `fastmash`. Both include the project licenses and generated dependency
licenses, copyright and notice texts for their target. Python and Rust are
build dependencies only.
The retained formula compiles its checksum-pinned Rust test resource during the
build and installs the checker under `libexec`. `brew test` runs this checker
without a compiler in its test dependencies. The published source archive and
its checksum remain unchanged.
The source-built programs carry no performance claim from the release binaries.

The formula is published in the [maintainer's tap](https://github.com/pederbe/homebrew-tap):

```sh
brew install pederbe/tap/fastmash
```

Homebrew's
[acceptance policy](https://docs.brew.sh/Package-Acceptance-Policy) includes
repository age and public-interest requirements for `homebrew/core`.
Review those requirements before proposing a core submission.

## Build and check

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
On Linux it requires the Sort supervisor and an exact `system sort` trace;
a correct result from fallback sorting cannot pass it. Testing this optional
route requires Linux 5.11 or later and GNU or uutils coreutils at `/usr/bin/sort`.
Older kernels can install Fastmash and use its built-in sorting, but cannot
pass this route check. On macOS the test requires a `native sort` trace and
confirms that no Sort supervisor is installed.

After removal, confirm that `fastmash` and, on Linux, `fastmash-sort-supervisor`
are absent from the prefix's `bin/` and that the formula's Cellar directory is gone.

## Maintenance

Keep this recipe and `Formula/fastmash.rb` in the tap in sync when updating
the published source version. After publishing an update, install from that
tap in a fresh environment, run `brew test`, check the installed licenses,
and verify removal before updating the installation guide.
