# Releasing Fastmash

## Weekly releases

Every Monday at 06:17 UTC, [release-plz](https://release-plz.dev) opens or updates
one release pull request when there are unreleased user-facing changes. Required
CI checks must pass before it merges automatically. Pushes to `main` refresh an
already-open release pull request, without opening a new one on every merge.
Internal changes alone do not trigger a release. Disable the schedule to stop
recurring preparation. Manual dispatch prepares an additionally authorized release.

The release pull request updates Cargo versions, exact dependency requirements,
the lockfile and a dated [CHANGELOG.md](CHANGELOG.md) section from conventional
pull request titles. All five published crates participate in versioning, and
library commits appear in the binary's combined notes. A version group propagates
library features and breaking changes to the binary. Unchanged libraries can
retain their published versions. Before 1.0, new features and breaking changes
raise the minor version.

Notes reach `main` before tagging and stay under their dated heading. Keep
`[Unreleased]` empty for future changes. Existing pending notes from the previous
release machinery are retained for the first release-plz release. Describe
compatibility consequences and required actions in the pull request description.
Use an actual `BREAKING CHANGE:` paragraph at the end for upgrade instructions.
A numerical-output change still needs its new Numerical profile and independent
verification. Use `fix(security):` for security notes and `feat(remove)!:` or
`feat(deprecate):` for removals and deprecations.

After the release pull request merges, release-plz creates `vVERSION` and a
GitHub draft, then explicitly dispatches Release at that tag. Workflow-token
events do not start other workflows. Only the binary gets a tag and GitHub
release. Cargo uploads remain disabled in release-plz and occur through the
existing qualified delivery below, using the unchanged trusted publisher in
`release.yml` and environment `crates-io`. No routine manual approval is needed.
See [ADR 0016](docs/src/adr/0016-release-plz.md).

The repository needs auto-merge enabled and the required `CI` check on `main`.
`RELEASE_PLZ_TOKEN` is a repository-scoped token with Contents and Pull requests
write access, allowing release pull requests to run CI. Squash defaults must use
the pull request title and body so conventional titles and upgrade instructions
survive merging. Release-plz uses the built-in workflow token for tags and drafts.

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

1. Build the tagged Linux executable twice with `release/Containerfile` and
   `release/build.sh`, using empty target directories. Both executable builds
   must be byte-identical. Record their hashes in the run's
   `release/delivery.sha256` and package one of those builds directly. Include
   the manifest as `binary.sha256` inside the archive. Historical manifests
   remain unchanged.
2. Run workspace tests and both regression-corpus input modes against that
   executable. Package the same bytes into the archive,
   `.deb` and `.rpm`, then test extraction, installation, Commands and removal.
3. Build the macOS executable twice on macOS 15 with the pinned compiler,
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
   installs the release with cargo-binstall on Linux and macOS and checks the
   installed bytes against the release archives. Its
   failure is reported without holding up the next step.
6. Advance the `release` branch after package delivery succeeds. A separate
   Website deployment workflow requests the fixed-`main` Cloudflare build.
   Website failures do not fail package delivery. Retry that workflow manually
   without rebuilding or republishing a release. Cloudflare reports the actual
   website build result after accepting the request.
7. The Homebrew tap's daily `Update` workflow notices the published release.
   It updates the [installation recipe](packaging/homebrew/README.md) to use
   the published archives and their checksums, then checks installation,
   upgrade and removal without rebuilding Fastmash. The release does not wait
   for it, and an installation update failure is retried in the tap.

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

A failed delivery retains its immutable tag and draft assets. Rerun the failed
Release jobs at the original tag after fixing the reported cause. Existing draft
assets must match the frozen assets byte for byte. Already published crate
versions must match their source-package checksums before a partial upload
resumes. A green release-plz run does not establish successful delivery.

Tag creation, draft creation and workflow dispatch are separate operations.
Release-plz skips an existing tag on retry. If its tag exists but draft creation
failed, restore only the missing draft with `gh release create --draft
--verify-tag --notes-file`, using the dated notes from that tag's exact source.
Then dispatch `release.yml` on that tag. Also dispatch it explicitly if the
original dispatch failed after creating the draft. Do not move tags or overwrite
published releases. Cargo uploads are still handled by Release, not release-plz.

If source changes are needed, merge them through the normal checked pull request
route and let release-plz prepare a higher version. Preserve the failed tag and
its evidence. Older runs cannot move the published version backward. After a
manual recovery finishes package delivery, advance `release` to the unchanged
version tag and refresh the website.

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
Prepare the patch versions, exact internal dependency requirements, lockfile and
dated release notes with release-plz on the selected source. Record the
full source revision, lockfile hash, builder identity, compiler and flags.
Use the pinned `release/Containerfile` and `release/build.sh` recipe already used
by the release workflow. Build twice from the same clean source into separate
empty target directories and compare the executable byte for byte. Normal
Cargo output is suitable for development checks but is not the release artifact.

Run formatting, warnings-denied Clippy, release workspace tests,
license/advisory and documentation checks on that source. Retain the
independent primitive, exact conversion and MPFR challenge checks. Then run the
command tests and both corpus input modes against the candidate executable.
The command tests include the
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
in the archive. Release-plz versions metadata and creates the tag and draft.
Preserve historical qualified manifests unchanged. Verify that the tag, binary
crate version, notes and executable version agree, and that internal dependency
requirements resolve to the intended library versions.

The Release workflow checks those hashes, runs the workspace and candidate
Command checks plus both corpus modes, and produces a draft with the archive,
Debian and RPM packages, checksums and provenance. Its archive and installed
package checks use:

```bash
bash release/check-installed.sh X.Y.Z /absolute/path/to/installed/bin release/delivery.sha256
```

This check verifies the installed executable hash, version, arithmetic, sorted
calculations and duplicate removal. The workflow installs and removes the
packages in clean Debian 12 and AlmaLinux 8 containers and verifies removal.
Upgrade checks cover earlier two-executable installations, removing their stale
supervisor without touching unrelated files. Package construction also checks
that packaging did not rewrite the executable. Keep these checks passing before
accepting the draft.

Review the draft's notes and complete asset list. Download its delivered assets,
verify their checksum sidecars and provenance against the build records. Identical
Linux downloads reuse the completed installation evidence. macOS installation
checks run once against the downloaded draft archive. Retain asset hashes and
installation logs with the delivery evidence.
The authorized weekly workflow publishes the release and crates after these
checks pass, then updates the signed repositories and published website version. Additional distribution submissions
and announcements remain separate work.

## Native macOS release delivery

macOS 15 or later is the main addition in the published 0.2.0 release.
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
