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

You need Linux x86-64 (or WSL2), stable Rust, Python 3.10+ and GNU or uutils coreutils.

```sh
git clone https://github.com/pederbe/fastmash
cd fastmash
cargo build
cargo test --release --workspace
./target/debug/fastmash --help
```

Having GNU datamash 1.9 installed (`apt install datamash`, `pacman -S datamash`)
makes it easy to compare behavior. Check `datamash --version`, because
distributions ship different versions.

## Before you open a pull request

1. Read [ARCHITECTURE.md](ARCHITECTURE.md) for how the code fits together, and
   [CONTEXT.md](CONTEXT.md) for the project's vocabulary. Use its terms in code,
   comments, issues and commit messages.
2. Keep each pull request to one change. Separate refactoring from behavior changes.
3. Add tests. Bug fixes need a test that fails without the fix. Behavior
   changes need regression cases. See [TESTING.md](TESTING.md).
4. Run the checks:

   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --release --workspace
   cargo build --release
   python3 scripts/check_regressions.py --binary target/release/fastmash
   python3 scripts/check_regressions.py --binary target/release/fastmash --stdin-file
   ```

   The last two commands run the regression corpus, with its input piped and
   from a file, as CI does.

5. Update documentation in the same pull request when user-visible behavior
   changes: the user guide under `docs/`, `--help` text, and `CHANGELOG.md`.

### Changes that need extra care

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
when that is not obvious. Reference issues as `#123`.

## Tools

[AGENTS.md](AGENTS.md) summarizes the project's rules in the form coding tools
read. Whatever tools you use, you are responsible for every line you submit:
review it, test it and be able to explain it.

## Releases

Maintainers release as described in [RELEASING.md](RELEASING.md). As a
contributor, add a line to `CHANGELOG.md` under `[Unreleased]` for any change
users will notice.

## Code of conduct

Everyone taking part in the project is expected to follow the
[code of conduct](CODE_OF_CONDUCT.md).

## License

By contributing, you agree that your contributions are dual licensed under
[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at the user's option,
without any additional terms or conditions.
