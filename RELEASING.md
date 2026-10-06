# Releasing Fastmash

How and when Fastmash is released. The release itself is automated; this is
the checklist around it.

## Rhythm

Small, frequent releases: whenever a user-visible improvement has landed and
been measured, and at least every two months while there are unreleased
changes. Each release should have one headline a user cares about, such as
"0.2: faster disk-spill sorts", which is also its announcement.

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
   (`scripts/check_regressions.py --binary ...`, both with and without
   `--stdin-file`) and the benchmark catalog on
   both measurement hosts, against GNU datamash 1.9 and the previous release,
   with the [acceptance rules](https://fastmash.io/benchmarks/method.html)
   fixed before measuring. A slowdown beyond the limits needs a published
   justification or a fix.
5. **Record the bytes.** Put the qualified `fastmash` and
   `fastmash-sort-supervisor` checksums in `release/qualified.sha256`. The
   release workflow refuses to publish any other bytes.
6. **Update the published results** when the numbers changed:
   - `docs/src/benchmarks/core-jobs.tsv` and the chart, with
     `scripts/benchmark_chart.py results` and `svg`;
   - the per-host tables on the method page and the "still faster" list on
     the benchmarks page;
   - the demo GIF and video exports (`scripts/demo/record.mjs`; see
     `scripts/demo/README.md`) if the demo job's times moved
     noticeably, and any claims on the landing page and link-preview image.
7. **Tag.** Move the `[Unreleased]` entries under
   `## [X.Y.Z] - YYYY-MM-DD` using the actual release date; the workflow
   refuses a tag without a dated entry. Tag the qualified commit `vX.Y.Z`
   and push the tag, then start the release explicitly:
   `gh workflow run Release --ref vX.Y.Z`. The tagged commit must contain
   the manual workflow; changing `main` does not update an existing tag.
   A dispatch on a branch skips the release job.
   The workflow builds, checks the checksums, runs the corpus, packages
   `.tar.gz`, `.deb` and `.rpm`, tests the packages on Debian 12 and
   AlmaLinux 8, attests provenance and creates a **draft** release with the
   changelog section as its notes.
8. **Publish.** Review the draft (assets, notes) and publish it. Then publish
   the crates to crates.io in dependency order:
   `fastmash-numeric-contract`, `fastmash-conversion`,
   `fastmash-portable-numerics`, `fastmash-sort-process`, `fastmash`.
   The website deploys from `main`; check that the install page and landing
   page show the new version.
9. **Packages.** Update any existing distribution packages when the release
   is available. Additional distribution channels, including conda-forge,
   follow the first release; they are not required to launch it.
10. **Announce** the headline: a Discussions announcement, and the social
    accounts. A release with a measured improvement links its benchmark.

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
cargo test --release --workspace --locked
python3 -B scripts/check_regressions.py --binary "$FASTMASH_TEST_BINARY" --expected-version "$version"
python3 -B scripts/check_regressions.py --binary "$FASTMASH_TEST_BINARY" --expected-version "$version" --stdin-file
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
changelog entries into the dated patch section when tagging. These metadata-only
changes must reproduce the qualified executable hashes at the final tagged
revision; any mismatch stops the release. Verify that the tag's version, crate
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
Publish the release and crates only with the maintainer's explicit approval,
then follow checklist steps 8–10 as authorized. Creating a draft does not authorize
publication, additional package submissions or announcements.

## Adding macOS support in 0.2.0

Native Apple Silicon macOS support is the main feature planned for 0.2.0.
The published 0.1.0 release has Linux artifacts only; its qualified files
and the Linux release recipe remain unchanged.

Use native GitHub-hosted macOS builds and tests against the public repository's
development source. Native checks run on macOS 15 and 26. The build check is:

```sh
MACOSX_DEPLOYMENT_TARGET=15.0 cargo build --release --locked --bin fastmash --target aarch64-apple-darwin
```

Record the source revision, macOS version, target, Rust toolchain and build
logs. Keep Linux build and behavior checks passing throughout the port, and
preserve the Numerical profile. [ADR 0010: Native Apple Silicon macOS](docs/src/adr/0010-native-apple-silicon.md)
sets the development platform boundary. Intel macOS is outside this scope.

`release/macos-archive.sh OUTPUT_DIRECTORY` builds one checksummed development
archive with a macOS 15 deployment target and records compiler, source,
flags, binary identity and deployment metadata. Its label includes
`macos-test` and the source revision; it is a downloadable CI artifact,
not a release asset. Both native baselines install those same bytes outside
the checkout through the installer, then run installed-command checks and the
applicable frozen corpus. The workflow retains native installation and
downloaded-file security observations without changing security policy.
Representative GNU comparisons record exact binaries, fixture identities,
repeated samples and output eligibility on each hosted runner.

The current tag-only release workflow continues to publish Linux assets only.
Before a macOS release, extend that explicitly authorized workflow to build,
qualify and attest the native archive, and decide its frozen-checksum process.
Do not treat a development CI artifact as a qualified release or modify
`release/qualified.sha256` to hold it.

Before releasing 0.2.0 with macOS support, complete these checks:

- Run the applicable numerical and command checks, including sorting,
  temporary-file cleanup and signal handling, on native macOS. Test startup
  and representative commands through the documented installation method.
  Keep the tested macOS version and architecture explicit.
- Provide working installation instructions. If distributing a macOS archive,
  include it in the release workflow, with checksums, provenance and an
  installation check of the delivered artifact. Describe source-build support
  explicitly if that is the validated delivery method.
- Update `site/index.html` (installation text and `operatingSystem` metadata),
  the README, and the installation, requirements, comparison, FAQ and roadmap
  pages. Update `docs/descriptions.json`, the platform text generated by
  `scripts/build_site.py` for `llms.txt`, and release notes. Supersede the
  platform ADR when support changes. Keep the existing benchmark claims
  scoped to their measured Linux hosts.
- Rebuild and preview the site, check its links and macOS installation steps,
  and verify the deployed pages before announcing the release.

For later releases, apply the same checks when adding or changing a supported
platform, and keep platform claims aligned with the delivered release.
