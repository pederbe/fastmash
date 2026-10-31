# Contributing to Fastmash

Thank you for your interest in Fastmash. Contributions of every size are
welcome, and some of the most valuable ones involve no code at all.

## Ways to help

- **Try it on your own data.** Run Fastmash alongside `datamash` in a real
  pipeline and tell us what happened, good or bad.
- **Report a compatibility difference.** If Fastmash and GNU datamash give
  different output, status or errors for the same command, and the difference
  is not in [Differences from GNU datamash](https://fastmash.io/guide/differences.html),
  please [open an issue](https://github.com/pederbe/fastmash/issues/new/choose).
- **Share a benchmark.** A slow job, with a description of the data (or a
  generator for similar data), helps us work on what matters.
- **Improve the documentation.** Unclear wording, missing examples and broken
  links are all worth fixing.
- **Write code.** Issues labelled
  [`good first issue`](https://github.com/pederbe/fastmash/labels/good%20first%20issue)
  are a good start. For anything larger, please open an issue to discuss the
  approach before you invest a lot of time.

Please report security problems privately, as described in
[SECURITY.md](SECURITY.md), not in a public issue.

## Development setup

To develop Fastmash, use Linux x86-64 (or WSL2), or Apple Silicon macOS 15 or
later, with stable Rust. Website checks also need Python 3.10 or later.
Native macOS development is verified by the
[native checks](TESTING.md#native-apple-silicon-development-gate).

```sh
git clone https://github.com/pederbe/fastmash
cd fastmash
cargo build
cargo build --locked --manifest-path release/publication-test-server/Cargo.toml --target-dir target --bins
cargo test --release --workspace
./target/debug/fastmash --help
```

Having GNU datamash 1.9 installed (`apt install datamash`, `pacman -S datamash`)
makes it easy to compare behavior. Check `datamash --version`, because
distributions ship different versions.

## Development workflow

All changes enter `main` through a pull request, including the maintainer's
changes. For planned work, agree on the scope and acceptance criteria in a
public issue before implementing it.

The maintainer creates a `contrib/` branch in this repository. Other contributors
create a branch in their fork. Open the pull request against `main` after the
local checks below pass, and link the issue when there is one.

The maintainer reviews and merges the pull request after the required CI checks
pass. Update the branch with `main` when intervening changes could affect it.
A branch does not need an update solely because unrelated changes have merged.
Validation runs before merge. Merging the tested result does not start another
validation suite. Development-only dependency changes run checks for the affected
workspace packages. Shared dependency or feature changes retain application checks.
Workflow-only changes run the check-selection tests. Use the manual CI run when
a workflow change also needs application or platform execution.
Before closing an implementation ticket, update its acceptance checklist and
check whether its parent issue is complete.

Renovate prepares one combined daily dependency update, including stable major
releases. GitHub automatically merges the batch after the required CI result
passes, covering every applicable platform, documentation and package check.
The daily self-hosted run updates Cargo, npm, Actions, container digests and the
custom tools in `.github/dependency-pins.json`. The synchronization helper keeps
release archive hashes and compiler versions together before pushing the PR.
The independent Unicode 15 reference crates in `crates/locale-tools` stay
fixed. Reconsider those pins when the retained reference contract changes.
Urgent security fixes may produce separate pull requests outside the daily batch.
Existing Dependabot security alerts and updates remain enabled.
Minimum Rust tests and the macOS 15 archive builder remain compatibility
baselines. The same archive must run unchanged on macOS 15 and 26. Release identities and
qualification evidence change through the release process, not dependency updates.

Use current stable dependencies with compatible version ranges and commit the
lockfiles. The remaining exact external pins, `unicode-case-mapping` 0.5.0,
`icu_properties` 1.3.2 and its data crate at 1.3.4, reproduce the independent
Unicode 15 producer. New casing changes mixed strings containing U+019B or
U+0264, and newer properties assign previously unassigned characters such as
U+1C89. Reconsider these pins when an upgrade preserves the reference behavior
or the reference is deliberately migrated.

Review `icu_collator` and `icu_collator_data` updates against the locale-order
verification in [TESTING.md](TESTING.md). The engine and data determine production
sort keys, and the ordinary corpus covers only part of the supported alphabets.

The npm KaTeX and lodash-es overrides select patched releases that the diagram
tool's dependency ranges exclude. Keep compatible ranges on these overrides and
remove them when the upstream tree resolves patched versions without them.
Action commit hashes and checksummed build tools identify reproducible inputs.
Exact versions between published Fastmash crates keep their release contracts
aligned.

## Before you open a pull request

1. Read [ARCHITECTURE.md](ARCHITECTURE.md) for how the code fits together, and
   [GLOSSARY.md](GLOSSARY.md) for the project's vocabulary. Use its terms in code,
   comments, issues and commit messages.
2. Keep each pull request to one change. Separate refactoring from behavior changes.
3. Add tests. Bug fixes need a test that fails without the fix. Behavior
   changes need regression cases. See [TESTING.md](TESTING.md).
4. Run the checks for what changed. Documentation uses the
   [website checks](TESTING.md#website-checks), application code uses the
   [application checks](TESTING.md#quick-start), and build or CI tooling needs
   checks of the affected tooling and workflow filters.

5. Update the user guide under `docs/` and `--help` text when user-visible
   behavior changes. Follow the [changelog guidance](#releases) in the same PR.

### Changes that need extra care

The maintainer runs release qualification on the measurement hosts for each
release. Pull requests run CI and the regression corpus; additional performance
qualification on those hosts is needed for pull requests that change performance.

- **Numerical output.** Anything that can change a printed number, however
  slightly, needs a new numerical profile version and independent verification.
  Please discuss it in an issue first.
- **Compatibility.** Matching GNU datamash is the default. An intentional
  difference needs a reason (usually safety or a GNU defect) and an entry in
  the differences page.
- **Performance.** Include before-and-after measurements on the affected jobs.
  A change that slows down an existing representative job needs a strong reason.
- **`unsafe` code.** Avoid it. If it is unavoidable, keep it minimal, add a
  `// SAFETY:` comment explaining why it is sound, and expect close review.
- **New dependencies.** Explain why the dependency is needed. Fastmash keeps
  its dependency tree small and every dependency must be compatible with
  `MIT OR Apache-2.0`.

## Commit messages

Write a short summary line in the imperative mood ("Add trimmean parameter
validation", not "Added…"), followed by a blank line and an explanation of *why*
when that is not obvious. Reference public issues as `#123` in commits and
discussions. Use full public issue links in source comments so their repository
is explicit. CI checks tracked files for private references.

## Tools

For local performance diagnosis, use [the profiling workflow](PROFILING.md).

[AGENTS.md](AGENTS.md) summarizes the project's rules in the form coding tools
read. Whatever tools you use, you are responsible for every line you submit:
review it, test it and be able to explain it.

## Releases

Maintainers release as described in [RELEASING.md](RELEASING.md), including its
comparison of the notes with merged changes before release preparation.

Include a concise entry under `[Unreleased]` in
[CHANGELOG.md](https://github.com/pederbe/fastmash/blob/main/CHANGELOG.md) in
the same pull request as a user-visible change. Cover capabilities, fixes,
installation and changes to command-line behavior or output. Explain
compatibility consequences and any action users need to take. Numerical-output
changes name their Numerical profile. Keep performance claims tied to their
measured workloads. If no entry is needed, explain why in the pull request's
Changelog section.

## Code of conduct

Everyone taking part in the project is expected to follow the
[code of conduct](CODE_OF_CONDUCT.md).

## License

By contributing, you agree that your contributions are dual licensed under
[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at the user's option,
without any additional terms or conditions.
