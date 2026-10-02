# Project rules in brief

A compact summary of how to work on Fastmash, for contributors and the coding
tools they use. [CONTRIBUTING.md](CONTRIBUTING.md) is the full guide.

## Orientation

- Fastmash is a Rust command-line program for Linux x86-64 that implements the
  GNU datamash command language with faster, portable results.
- Read [CONTEXT.md](CONTEXT.md) before naming anything. Use its terms in code,
  tests, comments and issues, and avoid the synonyms it lists under _Avoid_.
- Read [ARCHITECTURE.md](ARCHITECTURE.md) for the crates, the request pipeline
  and the design principles. Decisions are recorded in `docs/src/adr/`. If a
  proposal conflicts with a decision, name the decision and explain the conflict.
- The user guide in `docs/src/` describes promised behavior. Treat a mismatch
  between the guide and the code as a bug in one of them, and say which.

## Commands

```sh
cargo build
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
python3 scripts/check_regressions.py --binary target/release/fastmash
python3 scripts/check_regressions.py --binary target/release/fastmash --stdin-file
```

Run the full set before declaring a change complete, and report any command
you could not run. Tests need Linux; on Windows, use WSL2 with the checkout on
the Linux filesystem.

## Rules

1. **Do not change numerical output silently.** A change that can alter any
   printed number requires a new numerical profile version, independent
   verification and a changelog entry. If you are unsure whether a change
   affects numbers, assume it does and say so.
2. **Match GNU datamash by default.** For a compatibility question, check what
   GNU datamash 1.9 actually does. Intentional differences go in
   `docs/src/guide/differences.md` in the same change.
3. **Refuse rather than guess.** Unsupported conditions exit with status 77
   and a clear diagnostic. Errors in the command or input exit with status 1.
   Never replace a refusal with a best-effort result.
4. **Keep storage growable and checked.** Do not add new fixed caps on records,
   fields, groups or samples. Handle allocation failure as a refusal.
5. **Measure performance claims.** Do not claim a speedup without
   before-and-after measurements on the same host and build settings.
6. **No `unsafe` without need.** New `unsafe` code requires a `// SAFETY:`
   comment and a reason in the pull request.
7. **No new dependencies without justification**, and only with licenses
   compatible with `MIT OR Apache-2.0`.
8. **No GNU code.** Fastmash reimplements behavior. Do not copy, translate or
   paraphrase code from GNU datamash or other GPL sources. Describing
   observable behavior is fine.
9. **Never write "datamash" in identifiers, crate names or program messages.**
   Documentation may compare the two programs.
10. **Tests accompany changes.** A bug fix adds a test that fails without it.

## Pull requests

Describe what changed, why, how it was tested, and any effect on output,
exit statuses, performance or documentation. Keep each pull request to one concern.
