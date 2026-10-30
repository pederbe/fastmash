# Releasing Fastmash

## Weekly releases

Every Monday at 06:17 UTC, the Weekly release workflow checks for product,
dependency and packaging changes since the last published release. Documentation
and website changes alone do not create a release. Run it manually for an urgent
fix, selecting an automatic, minor or major version increment. Disable its
schedule to stop recurring publication.

The workflow creates a versioned metadata commit and tag without advancing
`main`. Published crates, exact internal dependency pins, the lockfile and the
executable version stay aligned. Before 1.0, a minor version may change behavior.
Breaking commit markers (`type!:` or `BREAKING CHANGE:`), and new authored
**Added**, **Changed** or **Removed** entries, select a minor version before 1.0.

[CHANGELOG.md](CHANGELOG.md) keeps user-facing entries under `[Unreleased]`.
Use **Added**, **Changed**, **Fixed**, **Removed** and **Security** headings.
A numerical-output change must name its new Numerical profile and receive
independent verification. The release notes combine new authored entries with
commits grouped into breaking changes, additions, fixes and other changes.
Each tagged changelog keeps the preceding release history.

The release tool replaces the shipping Unreleased heading with the version and
UTC release date. It accepts a changelog with only dated sections and retains
the frozen date on retries. It does not create an empty pending section.

After publication, `Release changelog` records the dated notes on `main` through
a pull request and the maintained documentation checks. It preserves later
pending entries and historical edits. Publication events and completed Release
workflows trigger this job. Draft and check-only runs do not update `main`.
Use the workflow's manual check-only mode to inspect a published tag or retry
its recording without rebuilding or republishing the release.

Before preparing a release, compare the Unreleased entries with all merged
changes since the previous published release. Check capabilities, fixes,
installation, support boundaries, Numerical profile changes and upgrade
instructions. Resolve omissions before freezing the notes. Generated commit
history supplements the user-facing summary rather than replacing it. For the
scheduled workflow, account for this coverage in each change review so it can
reuse the maintained entries without a separate manual approval.

## Performance acceptance

Performance-affecting runtime, dependency and build-recipe changes keep the
existing predecessor guards during normal change review. Select the affected
workloads and representative regression guards before measuring, follow the
[acceptance rules](docs/src/benchmarks/method.md), and reuse valid evidence for
unchanged inputs. A material regression needs a fix or an explicit disposition.
New public speed claims need measurements identifying their executable, input,
host and conditions.

Routine release versioning does not start another full benchmark campaign.
Changing Cargo package versions can alter generated code and executable layout,
so a new version is not claimed to be byte-equivalent to an earlier measured
build. Published benchmark results remain attached to their named version and
date. The existing charts describe the measured Linux 0.1.0 binaries and do not
become performance claims for later releases or macOS.

[ADR 0012](docs/src/adr/0012-release-delivery-and-performance.md) records this
separation between performance acceptance and release delivery.
[ADR 0013](docs/src/adr/0013-reuse-tested-release-artifacts.md) removes repeated
builds and installation checks and separates website deployment.

## Build, test and publish

The GitHub-hosted workflows perform these steps for the frozen version:

1. Build the tagged Linux pair twice with `release/Containerfile` and
   `release/build.sh`, using empty target directories. Both executable pairs
   must be byte-identical. Record their hashes in the run's
   `release/delivery.sha256` and package one of those pairs directly. Include
   the manifest as `binary.sha256` inside the archive. Historical manifests
   remain unchanged.
2. Run workspace tests and both regression-corpus input modes against that
   executable and its Sort supervisor. Package the same bytes into the archive,
   `.deb` and `.rpm`, then test extraction, installation, Commands and removal.
3. Build the Apple Silicon executable twice on macOS 15 with the pinned compiler,
   remapped paths and deployment target 15.0. Freeze one archive and its SHA-256.
4. Attest the platform archives and Linux packages and upload a draft release.
   Verify downloaded Linux assets against the build's retained checksums and
   release provenance. Install the downloaded macOS archive once on macOS 15
   and 26 and run the applicable command tests and both corpus input modes.
   Dry-run the five published source packages before publishing anything.
5. Publish the GitHub release after those checks pass. Explicitly dispatch
   Package repository at the same tag and wait for its signed apt/dnf update and
   installed-client checks. Publish the remaining crates in dependency order
   using the existing crates.io trusted publisher. A separate job then
   installs the release with cargo-binstall on Linux x86-64 and Apple Silicon
   macOS and checks the installed bytes against the release archives. Its
   failure is reported without holding up the next step.
6. Advance the `release` branch after package delivery succeeds. A separate
   Website deployment workflow requests the fixed-`main` Cloudflare build.
   Website failures do not fail package delivery. Retry that workflow manually
   without rebuilding or republishing a release. Cloudflare reports the actual
   website build result after accepting the request.
7. The Homebrew tap's daily `Update` workflow notices the published release.
   It builds, tests and publishes bottles from the release tag's
   [recipe](packaging/homebrew/README.md), then checks an upgrade on each
   platform. The release does not wait for it, and a tap failure is retried in
   the tap.

The workflow binds correctness and installation evidence to the delivered
bytes. It does not relabel a fresh rebuild as a benchmarked executable. Additional
distribution submissions, public announcements and new performance claims remain
separate work.

## Website updates

Cloudflare Workers Builds publishes website-input changes from `main` using
`scripts/build_site.sh --cloudflare`. The build checks the documentation,
static diagrams and generated routes before uploading its output. Pull requests
run the same website build through the required CI result. A selected website
check must succeed, and cannot pass as a skipped job.

The homepage and documentation share the committed 1200 by 630 `site/og.png`.
Its editable source is `scripts/demo/og.html`. Use the rendering command in
`scripts/demo/README.md` after editing, and review the image and its alternative
text together. The homepage and chapter heads provide Open Graph and Twitter
metadata. Chapter canonical URLs use the same clean routes as the sitemap.

Website build-tool changes alone do not start a binary release. Release
publication refreshes the displayed version through the separate deployment
workflow, whose queued requests do not replace pending requests. Cloudflare
reports the build and deployment result. Retry a failed website update there
without rebuilding published binaries.

## Failures and urgent fixes

A failed run retains its immutable tag and any draft assets. The next weekly run
retries only failed jobs before selecting another version. Existing draft assets
must match the frozen assets byte for byte. Already published crate versions
must match their source-package checksums before a partial upload resumes.

If a source fix is needed, merge it through the normal pull request checks, then
dispatch Weekly release with `supersede_failed=true`. It creates a higher version
while preserving the failed tag and its evidence. Older runs cannot move the
published version backward.

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
Review the complete diff from the previous tag, including dependency and builder changes.

### Build and verify the patch candidate

Apply the hosted build and delivery checks above to the selected patch source.
Set published crate versions and exact internal dependency pins to the patch
version, update the
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
invalidates the affected build and correctness evidence and needs fresh checks. Retain the
previous release and its records unchanged.

When a change can affect performance, compare the changed build with the
previous accepted Fastmash build under the existing change-review acceptance
rules. Use GNU datamash 1.9 when evaluating GNU-relative claims. Fix the jobs and
acceptance limits before measuring. Include affected jobs and representative
startup, streaming, retained-summary and sorting guards, with the same inputs,
settings and build recipe. Check output before timing, alternate the programs,
and retain both sessions, CPU time, peak memory and variation. Resolve material
regressions before merging. A routine release version bump does not repeat this
campaign, and unchanged dated claims do not need regenerated charts.

### Verify delivery and prepare the draft

After two identical clean builds, the Linux release job writes the actual
candidate hashes to its local `release/delivery.sha256` and includes that manifest
in the archive. Weekly preparation only versions metadata and creates the tag.
Preserve historical qualified manifests unchanged. Move selected changelog entries
into the dated patch section when tagging.
Verify that the tag, crate versions, notes and executable version agree.

The Release workflow checks those hashes, runs the workspace and candidate
Command checks plus both corpus modes, and produces a draft with the archive,
Debian and RPM packages, checksums and provenance. Its archive and installed
package checks use:

```bash
bash release/check-installed.sh X.Y.Z /absolute/path/to/installed/bin release/delivery.sha256
```

This check verifies both installed executable hashes, version, arithmetic and
an actual system Sort route. Correct output from a built-in fallback does not
prove that the installed Sort supervisor works. The workflow installs and removes
the packages in clean Debian 12 and AlmaLinux 8 containers and verifies that both
executables are removed. Package construction also checks that packaging did not
rewrite either executable. Keep these checks passing before accepting the draft.

Review the draft's notes and complete asset list. Download its delivered assets,
verify their checksum sidecars and provenance against the build records. Identical
Linux downloads reuse the completed installation evidence. macOS installation
checks run once against the downloaded draft archive. Retain asset hashes and
installation logs with the delivery evidence.
The authorized weekly workflow publishes the release and crates after these
checks pass, then updates the signed repositories and published website version. Additional distribution submissions
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
publish. The release job freezes the Linux delivery manifest for the
planned version during the release build, after creating its tag.
Keep the Numerical profile unchanged unless the numerical-change rules apply.

### Deliver through the Release workflow

On the frozen version tag, the Release workflow calls the native source checks
and archive build, then stages and attests that archive in a draft release.
It skips the earlier candidate-installation round. The workflow downloads the
draft archive on macOS 15 and 26, verifies its release-workflow attestation and
frozen hash, and runs documented installation, installed executable checks,
applicable command tests and both corpus modes once on each baseline. A failure
leaves the draft unpublished. Branch CI retains the candidate-installation round
because it has no draft release to download.

The macOS checksum is frozen per workflow run rather than stored in the Linux
manifest. Two clean builds establish within-run executable reproducibility;
hosted SDK updates do not imply that a later run can rebuild the same bytes.
The archive's checksum, embedded executable manifest, source/compiler/build
provenance and GitHub attestation bind the tested bytes to their delivery.
Never substitute a development CI archive or rewrite 0.1.0 evidence.

### Publish accurate platform claims

After publication, the workflow advances the published version pointer and
refreshes the website. New platform support still requires updating homepage
installation text and `operatingSystem` metadata, README, installation, requirements,
comparison, FAQ and roadmap pages, `docs/descriptions.json` and the platform
text emitted into `llms.txt`. Remove the candidate qualifications only when the
release is available. Keep the existing benchmark claims scoped to their
measured Linux 0.1.0 binaries.

Rebuild and preview the site, check links and the published macOS installation
steps, and verify deployed pages before announcing the release. The terminal
download route is tested without altering macOS security policy. Apple
notarization and browser-quarantine behavior are not promised by these checks.
