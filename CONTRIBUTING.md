# Contributing to Fastmash

You can help by reporting compatibility differences, sharing real workloads,
improving documentation or contributing code. Look for
[`good first issue`](https://github.com/pederbe/fastmash/labels/good%20first%20issue)
for a starting point. Discuss substantial changes in an issue before implementing
them.

Report bugs and feature requests through the
[issue templates](https://github.com/pederbe/fastmash/issues/new/choose).
Include the command, relevant input and observed output or errors. For a GNU
compatibility difference, compare with GNU datamash 1.9 and check the
[documented differences](https://fastmash.io/guide/differences.html).
Report security problems privately through [SECURITY.md](SECURITY.md).

## Development setup

Use Linux or WSL2, or macOS 15 or later, with the latest stable Rust.
Website checks also need Python 3.10 or later. From a clone of the repository:

```sh
cargo build
./target/debug/fastmash --help
```

Read [ARCHITECTURE.md](ARCHITECTURE.md) for the code map and
[GLOSSARY.md](GLOSSARY.md) for the vocabulary used in code, tests and issues.
[Design decisions](docs/src/adr/index.md) explain the project's constraints.

## Checks for your change

Use focused local checks while developing. [TESTING.md](TESTING.md) gives the
commands and prerequisites for application, website, native macOS and tooling
checks. It also explains how to prepare the test helpers before workspace tests.
A complete local replay of CI is not required. Use CI for platform coverage
unavailable locally. Applicable CI checks must pass before merging.

Add a regression test that fails without a bug fix, and matching expectations
for behavior changes. Update the user guide and `--help` when behavior changes.
Keep each pull request to one concern and separate refactoring from behavior
changes.

## Code and behavior rules

- Match GNU datamash 1.9 by default. An intentional compatibility difference
  needs a reason and an entry in the differences guide in the same change.
- Any change to printed numbers needs a new Numerical profile version and
  independent verification. Discuss it in an issue first and name the profile
  in the pull request.
- Performance-affecting runtime, dependency and build-recipe changes need
  before-and-after measurements against the previous accepted build on affected
  jobs and representative regression guards. A material regression needs a fix
  or an explicit disposition. Tie new speed claims to dated measurements. See the
  [measurement method](docs/src/benchmarks/method.md) and [PROFILING.md](PROFILING.md).
- First-party unsafe code is limited to the audited startup module and the two
  existing GNU Linux allocator calls. Startup covers pre-runtime capture of
  closed standard streams and the Linux process entry with its initial SIGPIPE
  setup. The allocator exception covers only `mallopt(M_ARENA_MAX, arenas)` and
  `malloc_trim(0)` in `malloc_arenas.rs`. Keep each operation minimal and explain
  its soundness with a `// SAFETY:` comment. Any extension needs a new maintainer
  decision. See [ADR 0017](docs/src/adr/0017-audited-startup.md) and
  [ADR 0020](docs/src/adr/0020-existing-allocator-calls.md).
- Justify new dependencies and use licenses compatible with `MIT OR Apache-2.0`.
  Use current stable dependencies with compatible ranges and commit lockfiles.
  Locale-reference updates have additional
  [qualification requirements](TESTING.md#locale-tools).

## Public issues and pull requests

Create a branch in your fork and open a pull request against `main` once the
implementation and local checks are settled. All changes enter `main` through
a pull request. The maintainer reviews and merges after applicable CI passes.
Update your branch when intervening changes could affect it.

Use **Problem**, **Solution** and **Validation** in the pull request description.
Explain the reason, behavior and compatibility effects, and meaningful checks.
Put migration or upgrade instructions in Solution. Link the issue when there is
one. Issue descriptions use **Problem**, **Desired outcome** and **Evidence**.
Keep small changes brief.

Public descriptions and comments cover software behavior, technical rationale
and evidence. Attribute checks accurately, keep private discussions out of public
records and preserve reporters' observations.
Before closing an implementation ticket, update its acceptance checklist and
check whether its parent is complete.

## Pull request titles and changelog

Use a conventional title with a short imperative summary, such as
`fix: Validate trimmean parameters` or `feat: Add a table operation`.
Use `perf:` for measured performance changes, `docs:` for documentation and
`fix(deps):` for user-facing dependency updates. Internal work uses `ci:`,
`build:`, `chore:`, `test:` or `refactor:`.

The title becomes the short changelog entry. Use `!` for a breaking change,
such as `feat!: Rename an option`, and explain compatibility consequences in
the description. Release notes are generated, so leave `[Unreleased]` empty
and do not add a release-note footer. Use public issue numbers in discussion
and full public issue links in source comments.

## Further guidance

- [Testing](TESTING.md) and [profiling](PROFILING.md)
- [Release tooling and recovery](RELEASING.md)
- [Code of conduct](CODE_OF_CONDUCT.md)

Contributions are dual licensed under [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE), at the user's option, without additional terms.
