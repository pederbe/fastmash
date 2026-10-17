# Ordinary regression commands

`fastmash-corpus` checks the ordinary commands in the existing compressed
regression corpus, through either piped or regular-file standard input. It
verifies the frozen fixture and provenance, records executable identities and
compares exact bytes and status. It preserves byte arguments, the isolated
environment, the eight-second execution deadline and the 524,288-byte output
guard. Each invocation owns a process group that is cancelled on completion or
failure.

```sh
cargo build --release -p fastmash-corpus-driver --bin fastmash-corpus
target/release/fastmash-corpus --binary target/release/fastmash --expected-version 0.2.0 --evidence-dir /tmp/ordinary-pipe
target/release/fastmash-corpus --binary target/release/fastmash --expected-version 0.2.0 --stdin-file --evidence-dir /tmp/ordinary-file
```

Both modes execute 2,857 ordinary cases. Their evidence accounts for all 3,137
case IDs, including 280 deferred transport cases. The complete Python corpus
gate remains required for those transports. Native macOS uses the same ordinary
expectations and the existing two named missing-header diagnostic adaptations.
Running this tool on Linux supplies Linux evidence only.

An empty evidence directory receives fixture, provenance and executable hashes,
every selected case's disposition, attempted outcomes, native observations and
complete mismatch bytes. `--keep-going` continues after mismatches. A nonempty
evidence directory is rejected to preserve the identity of each run.

The unpublished crate's Rust test command exercises deliberately incorrect
output, exit status, overflow and timeout behavior. It is a development helper
and does not enter product archives.

The pinned MIT-licensed Nix dependency supplies safe Unix signal-mask, session,
resource-limit and process-group APIs. Its declared Rust 1.69 minimum is below
this workspace's Rust 1.88 minimum. The private launcher applies limits in a
fresh process and then replaces itself with the selected executable, preserving
its exit and signal status without a first-party unsafe pre-exec hook. The
upstream [package declaration](https://docs.rs/crate/nix/0.31.3/source/Cargo.toml),
[signal-mask API](https://docs.rs/nix/0.31.3/nix/sys/signal/fn.pthread_sigmask.html),
[resource-limit API](https://docs.rs/nix/0.31.3/nix/sys/resource/fn.setrlimit.html),
[session API](https://docs.rs/nix/0.31.3/nix/unistd/fn.setsid.html) and
[process-group signal implementation](https://github.com/nix-rust/nix/blob/v0.31.3/src/sys/signal.rs)
describe those contracts.
