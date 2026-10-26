# Releasing Fastmash

The website keeps its existing automatic builds from `main`. After publication
advances the `release` branch, the release workflow calls the fixed-`main`
Cloudflare deploy hook stored in `CLOUDFLARE_DEPLOY_HOOK`. The build resolves the
download version from that release branch while rendering current documentation.

How and when Fastmash is released. The release itself is automated; this is
the checklist around it.

## Rhythm

Every Monday at 06:17 UTC, the Weekly release workflow checks for product,
dependency and packaging changes since the last published release. Documentation
and website changes alone do not create a release. Run it manually for an urgent
fix, selecting a minor or major increment when needed. Disable its schedule to
stop recurring publication.

The workflow prepares a versioned metadata commit and tag without changing
`main`. Every published crate, internal version pin, lockfile and executable
version stays aligned. Release notes combine new authored changelog entries with
commits grouped into breaking changes, additions, fixes and other changes.
Breaking commit markers (`type!:` or `BREAKING CHANGE:`) select a minor version
before 1.0 and a major version afterward. New authored **Added**, **Changed** or
**Removed** entries also select a minor version before 1.0.

Publication waits for exact-version qualification, reproducible Linux builds, package
checks and installation checks against uploaded Linux and macOS assets. A failed
run retains its frozen tag and draft. The next weekly run retries only failed
jobs. If a source fix is needed, merge it into `main`, then dispatch Weekly release
with `supersede_failed=true`. That creates a higher version while preserving the
failed tag and its evidence.
Patch releases (0.x.**y**) fix bugs and change no documented behavior or
numerical output. Before 1.0, a minor release (0.**x**) may change behavior;
every such change is listed under **Changed**.

## Changelog practice

[CHANGELOG.md](CHANGELOG.md) is written for users, in the pull request that
makes the change, under `## [Unreleased]`:

- **Added** for new operations, options and platforms.
- **Changed** for behavior changes, including faster jobs (name the job and
  the measured gain: "Grouped medians on sorted input are about 1.4 times
  faster").
- **Fixed** for bugs, naming the symptom a user would have seen.
- **Removed** and **Security** as in [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

A change to any numerical output is always listed, with the numerical profile
version that introduced it. Internal refactoring, tests and CI are not
listed unless users notice them.

Release dates record when a version becomes available, not a target date.
Keep upcoming changes under the undated `[Unreleased]` heading until the
release is being tagged.

## Checklist

1. **Freeze.** Everything for the release is on `main` and the checks pass.
   For the initial release, while the repository is private, run the CI
   checks locally against the exported checkout: formatting, Clippy,
   workspace tests, both regression-corpus input modes, the minimum Rust
   version, dependency licenses and advisories, and the site and link checks.
   Run hosted CI after the repository becomes public, before tagging. Standard
   public hosted runners are free; extra private Actions minutes are not a
   prerequisite. Local checks do not test GitHub's tag dispatch, permissions,
   attestations or draft-release uploads.
2. **Version.** Set the version in all five crates' `Cargo.toml` (including
   the exact `=` pins between them) and update `Cargo.lock`. Keep the
   changelog entries under `[Unreleased]` during qualification.
3. **Build the candidate** with the release builder (`release/Containerfile`
   and `release/build.sh`), twice from a clean checkout. The two builds must
   be byte-identical.
4. **Qualify the candidate.** Run the regression corpus
   (`target/release/fastmash-corpus --binary ...`, both with and without
   `--stdin-file`) and the benchmark catalog on
   both measurement hosts, against GNU datamash 1.9 and the previous release,
   with the [acceptance rules](https://fastmash.io/benchmarks/method.html)
   fixed before measuring. A slowdown beyond the limits needs a published
   justification or a fix.
5. **Record the Linux bytes.** Put the qualified `fastmash` and
   `fastmash-sort-supervisor` checksums in `release/qualified.sha256`. The
   release workflow refuses to publish any other Linux bytes. The macOS archive
   is frozen and qualified within the native workflow run as described below.
6. **Update the published results** when the numbers changed:
   - `docs/src/benchmarks/core-jobs.tsv` and the chart, with
     `scripts/benchmark_chart.py results` and `svg`;
   - the per-host tables on the method page and the "still faster" list on
     the benchmarks page;
   - the demo GIF and video exports (`scripts/demo/record.mjs`; see
     `scripts/demo/README.md`) if the demo job's times moved
     noticeably, and any claims on the landing page and link-preview image.
7. **Record exact-version qualification.** Set the planned release version
   before building and measuring. After both measurement hosts accept that
   executable pair, prepare the release helper and record their evidence:

   ```sh
   cargo build --locked --manifest-path release/publication-test-server/Cargo.toml --target-dir target --bin fastmash-release
   target/debug/fastmash-release record-qualification SOURCE_SHA AMD_EVIDENCE_URL INTEL_EVIDENCE_URL
   ```

   Commit the versioned source, `release/qualification.tsv` and both accepted
   hashes in `release/qualified.sha256` together through the normal review and
   CI route. The record binds the accepted source and distinct host evidence
   to the exact runtime crates, package versions, lockfile, compiler configuration
   and build recipe. Package-version changes can alter Cargo crate identities,
   generated code and executable layout, so they require new qualification.
   Weekly release rebuilds twice and verifies the existing accepted hashes.
   It never replaces them with hashes from an unqualified build.

   Qualification on the two measurement hosts is still a separate prerequisite.
   The weekly workflow publishes an already qualified planned version. It stops
   before tagging when that exact version or its accepted evidence is missing.
8. **Automatic publication.** Weekly release creates the dated changelog and
   immutable tag, then explicitly dispatches Release. Release checks the exact-source
   binding and frozen Linux hashes, builds both platforms, creates attestations
   and a draft, downloads its uploaded assets and checks installation. Only then
   does it publish the GitHub release. It dry-runs the five published source
   packages before obtaining a temporary crates.io token and publishing them in
   dependency order. Registry failures remain visible and are retried without
   selecting another source revision.
9. **Packages.** Release explicitly dispatches Package repository at the same
   tag, adding its signed packages to `packages.fastmash.io`. The registry and
   signing environments retain their tag restrictions. Additional distribution
   submissions remain separate work. After delivery succeeds, Release advances the `release` branch to the
   published tag. The website builds `main` with `bash scripts/build_site.sh
   --cloudflare`, reading the published version from that branch. A failed
   finalizer is retried before preparing another version.
10. **Announcements.** Social posts and Discussions announcements remain separate
    from scheduled releases. Link measurements when making performance claims.

## Patch releases

Use this procedure when a report warrants a fix to a published release.
It does not schedule a patch release. Product fixes and regression tests belong
in this repository; the maintainer retains qualification plans, logs, source
bindings and measurements in the private measurement repository. Published
claims keep their supporting results here.

### Triage and select the fix

Record the reported version and executable checksum, complete command, input
bytes, locale and other explicit settings, platform, stdout, diagnostic and
exit status. Minimize the input while preserving the symptom, then reproduce
it with the delivered release and the current source. For compatibility
questions, check GNU datamash 1.9 and the
[documented differences](docs/src/guide/differences.md). For numerical questions,
use the retained independent expectations described in [TESTING.md](TESTING.md).

Classify the report before choosing a release:

- A defect that breaks a documented command, produces a wrong result, loses
  data or creates a security problem may warrant an urgent patch.
- A new operation, platform, intentional compatibility change or performance
  improvement can wait for a minor release.
- A documented difference or unsupported condition needs an explanation or
  documentation correction unless the maintainer chooses to change the contract.

A patch preserves the released documented behavior and Numerical profile.
A deliberate change to printed numbers requires a new Numerical profile,
independent verification and a changelog entry, and goes into a minor release.
If a correctness fix would change printed numbers, follow that same rule rather
than silently treating it as a patch. Restoring other documented behavior needs
a regression test that fails on the released version and passes with the fix.

Develop the fix on a `contrib/` branch and submit it through the normal pull
request checks. Select only the accepted fixes for the release. If `main`
already contains minor-release work, start the patch branch at the last published
tag and cherry-pick the fixes with their tests and necessary prerequisites.
Do not include the unreleased macOS feature in a Linux 0.1.x patch. Review the
complete diff from the previous tag, including dependency and builder changes.

### Build and qualify the exact candidate

Apply checklist steps 1–5 to the selected patch source. Set all five crate
versions and exact internal dependency pins to the patch version, update the
lockfile, and keep only its user-visible fixes under `[Unreleased]`. Record the
full source revision, lockfile hash, builder identity, compiler and flags.
Use the pinned `release/Containerfile` and `release/build.sh` recipe already used
by the release workflow. Build twice from the same clean source into separate
empty target directories and compare **both** executables byte for byte. Normal
Cargo output is suitable for development checks but is not the release artifact.

Run formatting, warnings-denied Clippy, release workspace tests, minimum Rust
version, license/advisory and documentation checks on that source. Retain the
independent primitive, exact conversion and MPFR challenge checks. Then run the
command tests and both corpus input modes against the candidate pair, with
`fastmash-sort-supervisor` beside `fastmash`. The command tests include the
independent numerical CLI fixtures and affected-feature tests; they must execute
the candidate rather than a separately built executable.

For an already built Linux candidate, this Bash block runs the maintained
command boundary and freezes the corpus except for the explicit release version:

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

Run any affected feature's opt-in tests whose prerequisites are available, and
record the remaining exclusions with their reasons. The independent numerical
library tests still execute the test build; the CLI fixtures and corpus exercise
the candidate. A source, dependency, toolchain, flag or executable change
invalidates the affected qualification and needs fresh checks. Retain the
previous release and its records unchanged.

When a change can affect performance, compare this candidate with the delivered
predecessor and GNU datamash 1.9 under the published
[benchmark method](docs/src/benchmarks/method.md). Fix the jobs and acceptance
limits before measuring. Include affected jobs and representative startup,
streaming, retained-summary and sorting guards on both measurement hosts, using
the same inputs, settings and build recipe. Check output before timing, alternate
the programs, and retain both sessions, CPU time, peak memory and variation.
Resolve any material regression before proceeding. For a change with no runtime
effect, record why new performance measurements are unnecessary. A patch that
leaves published timing claims unchanged need not regenerate their charts.

### Verify delivery and prepare the draft

After qualification, update `release/qualified.sha256` to the **two candidate
hashes**, keeping the previous release's manifest at its tag. Move the selected
changelog entries into the dated patch section when tagging. The generated release metadata must reproduce its frozen executable hashes
at the final tagged revision. Any mismatch stops the release. Verify that the tag's version, crate
versions, notes and executable version agree.

Use checklist step 7 to prepare the tag and explicitly dispatch the maintained
Release workflow. It checks the qualified hashes, runs the workspace and candidate
command checks plus both corpus modes, and produces a draft with the archive,
Debian and RPM packages, checksums and provenance. Its archive and installed
package checks use:

```bash
bash release/check-installed.sh X.Y.Z /absolute/path/to/installed/bin release/qualified.sha256
```

This check verifies both installed executable hashes, version, arithmetic and
an actual system Sort route. Correct output from a built-in fallback does not
prove that the installed Sort supervisor works. The workflow installs and removes
the packages in clean Debian 12 and AlmaLinux 8 containers and verifies that both
executables are removed. Package construction also checks that packaging did not
rewrite either executable. Keep these checks passing before accepting the draft.

Review the draft's notes and complete asset list. Download its delivered assets,
verify their checksum sidecars and provenance, and repeat extraction/installation
checks against the qualified manifest. Build-side checks alone do not verify an
uploaded asset. Retain asset hashes and installation logs with qualification.
The authorized weekly workflow publishes the release and crates after these
checks pass, then follows checklist steps 8–9. Additional distribution submissions
and announcements remain separate work.

## Native macOS release delivery

Apple Silicon macOS 15 or later is the main addition in the published 0.2.0 release.
The 0.1.0 release and its qualified Linux files remain unchanged.
Intel macOS is outside this scope. See [ADR 0011: Qualify and deliver the same
native archive](docs/src/adr/0011-native-release-delivery.md).

### Qualify a candidate before tagging

The Native macOS workflow still checks the complete applicable workspace on
macOS 15 and 26. Its ordinary runs build a development archive. To exercise
the release layout and double-build check without creating a tag or draft:

```sh
gh workflow run macos.yml --ref contrib/macos-release-delivery -f release-archive=true
```

Use the actual candidate branch in place of that example. Record the source
revision and successful workflow run. The pinned compiler builds twice into
separate empty target trees with `MACOSX_DEPLOYMENT_TARGET=15.0` and remapped
paths; the native builder refuses differing executable bytes. It packages one
executable, target-specific notices, `binary.sha256` and build provenance.
The candidate uses the final `fastmash-vX.Y.Z-aarch64-apple-darwin.tar.gz` name,
with `artifact_kind=release-candidate` recorded in its provenance.

The archive job freezes its SHA-256. Both native baselines download and verify
that hash, install outside the checkout through the documented method and
shell installer, check the installed executable hash and version, and run the
applicable command tests and both frozen corpus input modes. Sorting, actual
Spill, signals and temporary-file cleanup stay covered. Representative hosted
GNU comparisons remain scoped to those runners and workloads.

A successful branch run is candidate evidence, not a release or permission to
publish. Complete Linux qualification and update `release/qualified.sha256`
for the exact final candidate through checklist steps 1–5 before tagging.
Keep the Numerical profile unchanged unless the numerical-change rules apply.

### Deliver through the Release workflow

On the qualified tag, the manually dispatched Release workflow calls the same
native build and installation workflows. Linux's committed checksum gate and
archive/package checks remain required. Only after both platforms pass does
it stage the same macOS archive, attest both platform archives and Linux
packages, and create a draft release.

The workflow then downloads the draft's uploaded macOS archive on macOS 15
and 26, verifies the release-workflow attestation and the frozen archive hash,
and repeats documented installation, installed executable checks, applicable
command tests and both corpus input modes. Those checks use the delivered
bytes; rebuilding a source executable cannot substitute for them. Evidence
from candidate and delivered installations has separate artifact names.
A failure leaves the draft unpublished. Successful delivered-asset checks
allow the workflow to publish automatically.

The macOS checksum is frozen per workflow run rather than stored in the Linux
manifest. Two clean builds establish within-run executable reproducibility;
hosted SDK updates do not imply that a later run can rebuild the same bytes.
The archive's checksum, embedded executable manifest, source/compiler/build
provenance and GitHub attestation bind the tested bytes to their delivery.
Never substitute a development CI archive or rewrite 0.1.0 evidence.

### Publish accurate platform claims

After publication, update `site/release-version.txt`, the homepage's installation
text and `operatingSystem` metadata, README, installation, requirements,
comparison, FAQ and roadmap pages, `docs/descriptions.json` and the platform
text emitted into `llms.txt`. Remove the candidate qualifications only when the
release is available. Keep the existing benchmark claims scoped to their
measured Linux 0.1.0 binaries.

Rebuild and preview the site, check links and the published macOS installation
steps, and verify deployed pages before announcing the release. The terminal
download route is tested without altering macOS security policy. Apple
notarization and browser-quarantine behavior are not promised by these checks.
