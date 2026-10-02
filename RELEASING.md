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
   - the demo GIF (`scripts/demo/record.mjs`) if the demo job's times moved
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
