# Releasing Fastmash

## Weekly releases

[Release-plz](.github/workflows/release-plz.yml) prepares a checked
version and changelog pull request, then creates the binary's version tag and
draft release. The [Release workflow](.github/workflows/release.yml) performs
artifact delivery and Cargo publication. See the
[release-plz workflow](.github/workflows/release-plz.yml) for its schedule and
entry points, and [release-plz.toml](release-plz.toml) for versioning and notes.

Release pull requests need repository auto-merge and the required `CI` check.
`RELEASE_PLZ_TOKEN` needs repository Contents and Pull requests write access
so those pull requests can run CI. Tags and drafts use the workflow token.
Cargo publication uses the existing trusted publisher after artifact checks.
Only the `fastmash` product package is published. Maintenance tools remain
unpublished. [ADR 0019](docs/src/adr/0019-one-product-crate.md) records the package
boundary.

The PR title supplies the short changelog entry and breaking marker. Keep
compatibility consequences and upgrade instructions in the PR's Solution
section. Leave `[Unreleased]` empty. Use `fix(security):` for security fixes,
`feat(remove)!:` for removals and `feat(deprecate):` for deprecations.
Numerical-output changes still require a new Numerical profile and independent
verification. See [ADR 0016](docs/src/adr/0016-release-plz.md).

## Performance acceptance

Performance-affecting runtime, dependency and build-recipe changes keep the
[predecessor guards](docs/src/benchmarks/method.md#acceptance-rules) during
change review. Select affected workloads and representative regression guards
before measuring. A material regression needs a fix or an explicit disposition.
New speed claims identify the executable, input, host and conditions.

Routine versioning does not repeat a full benchmark campaign. Changing package
versions can change executable layout, so a later release is not claimed to be
byte-equivalent to an earlier measured build. Published results remain attached
to their named version and date. The existing charts describe the measured
Linux 0.1.0 binaries and do not become claims for later releases or macOS.
See [ADR 0012](docs/src/adr/0012-release-delivery-and-performance.md).

## Build, test and publish

The [Linux builder](.github/workflows/linux-release.yml) and
[native builder](.github/workflows/macos.yml) bind correctness and installation
evidence to the bytes delivered by [Release](.github/workflows/release.yml).
Their scripts and workflow inputs define the current build commands.

- Linux uses `release/Containerfile`, `release/build.sh` and
  `release/train-pgo.sh`. Train a fresh compiler profile for each new source or
  compiler build. Use that same frozen profile for two clean optimized builds
  in empty target directories and require byte-identical executables.
  Failed training commands or a stale profile directory stop the build.
- The static Linux executable must have no shared-library dependency or ELF
  interpreter. Include musl's copyright with Cargo dependency notices.
  Archive and package construction must preserve executable bytes.
- Retain the compiler profile, training inputs and build provenance with the
  archive. Reproducibility uses that recorded profile as an input. Retraining
  need not reproduce identical counts or bytes.
- Native macOS release builds use a supported deployment target, remapped paths
  and two clean builds. Both executable builds must match. The same archive
  must pass installation and command checks on both supported macOS baselines.
- Run workspace tests, independent numerical fixtures and both corpus input
  modes against the selected executable. Test archive/package extraction,
  installation, commands and removal. Dry-run the product source package before
  authenticating for Cargo publication.
- Retain executable hashes in the build's `release/delivery.sha256` and the
  archive's `binary.sha256`. Downloaded draft assets must match their frozen
  checksums and release-workflow provenance before publication. Existing assets
  cannot be replaced with different bytes. Historical manifests remain unchanged.

The [package-repository guide](packaging/repository/README.md) covers signed
apt/dnf delivery. The [Homebrew guide](packaging/homebrew/README.md) covers the
release-archive recipe. Additional distribution submissions and announcements
are separate from the delivery pipeline.

## Website updates

Website changes use [the website checks](TESTING.md#website-checks).
`scripts/build_site.sh --cloudflare` builds from `main` and reads the published
`release` pointer for download links. A website-tool change alone does not
require a binary release.

After package delivery, the separate
[Website deployment workflow](.github/workflows/website-deploy.yml) requests a
Cloudflare build. A failed website update can be retried there without
rebuilding or republishing binaries. Cloudflare reports the actual deployment
result after accepting the request.

For the shared social-preview image and demo assets, use the editable sources
and rendering commands in [the demo guide](scripts/demo/README.md).

## Failures and urgent fixes

A failed delivery retains its immutable tag and draft assets. Rerun failed
Release jobs at the original tag after fixing the reported cause. Existing
draft assets must match the frozen assets byte for byte. Already published
versions must match their source-package checksums before an upload retry
completes. A green release-plz result alone does not establish successful delivery.

Tag creation, draft creation and delivery dispatch are separate operations.
Release-plz skips an existing tag on retry. If the tag exists but draft creation
failed, restore only the missing draft with `gh release create --draft
--verify-tag --notes-file`, using dated notes from that tag's exact source.
Then dispatch `release.yml` on the tag. Dispatch it explicitly as well if the
original dispatch failed after creating the draft. Do not move tags or overwrite
published releases. Cargo uploads remain in Release, not release-plz.

If source changes are needed, submit them through the normal checked PR route
and prepare a higher version. Preserve the failed tag and its evidence. Older
runs cannot move the published version backward. After manual recovery finishes
package delivery, advance `release` to the unchanged version tag and refresh
the website.

## Patch releases

### Triage and select the fix

Record the reported version and executable checksum, complete command, input
bytes, locale/settings, platform, stdout, diagnostic and exit status. Reproduce
the symptom with the delivered release and current source. For compatibility
questions, check GNU datamash 1.9 and the
[documented differences](docs/src/guide/differences.md). For numerical questions,
use the independent expectations described in [TESTING.md](TESTING.md).

A defect that breaks a documented command, produces a wrong result, loses data
or creates a security problem may warrant an urgent patch. New operations,
platforms and intentional compatibility or performance changes can wait for a
minor release. A documented difference or unsupported condition needs an
explanation unless the contract is deliberately changed.

A patch preserves the released behavior and Numerical profile. A deliberate
change to printed numbers, including a correctness fix, requires a new profile,
independent verification and a changelog entry in a minor release. Restoring
other documented behavior needs a test that fails on the released version.

If `main` already contains minor-release work, start the patch branch at the
last published tag and select the accepted fixes, their tests and necessary
prerequisites. Review the complete diff, including dependencies and builders.

### Build and verify the patch candidate

Use the build and delivery requirements above on the selected source. Prepare
the product version, lockfile and dated notes with
release-plz. Record the full source revision, lockfile hash, builder, compiler
and flags. Normal Cargo output is suitable for development checks but is not
the release artifact.

Run formatting, warnings-denied Clippy, release workspace tests, license/advisory
and documentation checks on that source. Use [TESTING.md](TESTING.md) and the
[CI workflow](.github/workflows/ci.yml) for the maintained commands.

For an already built Linux candidate, this Bash block directs the maintained
command tests and both corpus modes to that executable. Numerical module tests
still execute their own test build.

```bash
bash -c '
set -euo pipefail
candidate=$(realpath "$1")
version=$2
export FASTMASH_TEST_BINARY="$candidate/fastmash"
cargo build --locked --manifest-path release/publication-test-server/Cargo.toml --target-dir target --bins
cargo test --release --workspace --locked
cargo build --release --locked -p fastmash-corpus-driver --bin fastmash-corpus
target/release/fastmash-corpus --binary "$FASTMASH_TEST_BINARY" --expected-version "$version"
target/release/fastmash-corpus --binary "$FASTMASH_TEST_BINARY" --expected-version "$version" --stdin-file
' -- /absolute/path/to/candidate-binaries X.Y.Z
```

Run relevant opt-in tests whose prerequisites are available and record material
exclusions. A source, dependency, compiler, flag or executable change invalidates
affected evidence. Retain previous releases and their records unchanged.
Keep plans, logs and unpublished measurements with their existing working
records. Results supporting published claims remain in the public repository.

### Verify delivery and prepare the draft

Verify that the tag, workspace version, product package, notes and executable
version agree. The archive
and installed-package checks use:

```bash
bash release/check-installed.sh X.Y.Z /absolute/path/to/installed/bin release/delivery.sha256
```

Prepare the checker as described in [TESTING.md](TESTING.md#quick-start).
It verifies executable bytes, version, arithmetic, sorted calculations and
duplicate removal. Package checks include installation, upgrades from older
two-executable releases, removal and preservation of unrelated files.

Set `INSTALLATION_TEST_EVIDENCE` to a new directory to retain raw installed-check
receipts. Publication regression tests use `PUBLICATION_TEST_EVIDENCE` similarly.
Without it, their temporary logs are removed on exit. Use a fresh directory
for each run.

Review the draft's notes and complete asset list. Download delivered assets and
verify checksum sidecars and provenance against retained build records. Identical
Linux downloads reuse completed installation evidence. macOS installation checks
run against the downloaded draft archive. Retain hashes and installation logs.

## Native macOS release delivery

### Qualify a candidate before tagging

To exercise the release layout and double-build check without a tag or draft,
replace `YOUR_BRANCH` with the candidate branch:

```sh
gh workflow run macos.yml --ref YOUR_BRANCH -f release-archive=true
```

Record the source revision and successful run. The candidate uses the final
archive layout with `artifact_kind=release-candidate` in its provenance.
Installation checks use the same archive on the supported native baselines,
including command tests, both corpus modes, sorting, spill and cleanup.
A successful branch run is candidate evidence, not a published release.

### Deliver through the Release workflow

Release stages and attests the native archive in a draft, then checks downloaded
bytes once on each supported native baseline. Branch CI retains its separate
candidate-installation round because no draft exists to download.

The macOS checksum is frozen per run. Two clean builds establish within-run
reproducibility. Hosted SDK updates do not imply that a later run rebuilds the
same bytes. Checksums, the executable manifest, provenance and the attestation
bind tested bytes to delivery. See [ADR 0011](docs/src/adr/0011-native-release-delivery.md).

### Publish accurate platform claims

New platform support requires updating installation text and `operatingSystem`
metadata, README, installation, requirements, comparison and FAQ pages,
`docs/descriptions.json` and the platform text in `llms.txt`. Remove candidate
qualifications only when the release is available. Existing benchmark claims
remain scoped to their measured executable and platform.

Run website checks and verify deployed installation steps before announcing a
release. The terminal download route does not promise Apple notarization or
browser-quarantine behavior.
