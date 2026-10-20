# Regression commands

`fastmash-corpus` checks commands in the existing compressed regression corpus,
through either piped or regular-file standard input. Its default run also checks
directory input, output to a pipe whose reader has closed and full-output errors. It
verifies the frozen fixture and provenance, records executable identities and
compares exact bytes and status. It preserves byte arguments, the isolated
environment, the eight-second execution deadline and the 524,288-byte output
guard. Each invocation owns a process group that is cancelled on completion or
failure.

Linux admission reads the current thread's kernel `SigBlk` mask, including
real-time signals, from `/proc/thread-self/status`. macOS uses the native signal
mask API. Both require an unblocked inherited mask. The kernel
[thread identity](https://man7.org/linux/man-pages/man5/proc_thread-self.5.html)
and [blocked-signal mask](https://docs.kernel.org/filesystems/proc.html)
documentation define the Linux observation.

```sh
cargo build --release -p fastmash-corpus-driver --bin fastmash-corpus
target/release/fastmash-corpus --binary target/release/fastmash --expected-version 0.2.0 --evidence-dir /tmp/ordinary-pipe
target/release/fastmash-corpus --binary target/release/fastmash --expected-version 0.2.0 --stdin-file --evidence-dir /tmp/ordinary-file
```

The default run executes 3,134 cases: 2,857 ordinary cases, 22 directory-input
cases, 51 closed-output-pipe cases, 72 full-output cases and 65 early commands
with stdin held open, plus 67 held-output observations. It records three late-input cases
as deferred. Regular-file mode executes the same 2,857 ordinary cases as before
and records why each other case belongs to the default Rust run or the full
Python run. Both modes account for all 3,137 case IDs. Keep the complete Python
corpus gate until the remaining transports have migrated.
Native macOS uses the same
expectations and the existing two named missing-header diagnostic adaptations.
Running this tool on Linux supplies Linux evidence only.

Early-command cases retain an empty stdin pipe until the actual executable
exits. Their evidence records whether the writer remained open when exit was
observed. A timeout fails the case and cancels the owned process group.

Held-output cases keep stdin open for two seconds after delivering input,
record the actual output prefix and whether the process is still alive, then
close stdin and compare the completed output. Destinations are regular files,
pipes or raw terminals. Native macOS preserves the four existing buffer-size
adaptations using frozen bytes and observed destination facts.

Full-output cases use Linux's `/dev/full` or a private 8 MiB HFS+ image on macOS,
created with the system's `hdiutil`. Ordinary, selected and unselected I/O controls
first calibrate the actual executable. Each full-output case requires real ENOSPC
and an empty sink before and after execution. The sink closes after candidates
are reaped and before the owned image detaches. Calibration, destination facts
and cleanup records are retained below the run's evidence directory. A run that
selects no full-output cases creates no image.

An empty evidence directory receives fixture, provenance and executable hashes,
every selected case's disposition, attempted outcomes, native observations and
complete mismatch bytes. `--keep-going` continues after mismatches. A nonempty
evidence directory is rejected to preserve the identity of each run.

The unpublished crate's Rust test command exercises deliberately incorrect
output, exit status, overflow and timeout behavior. It also verifies actual
directory reads and broken-pipe writes, including successful output on the
unselected stream. It is a development helper
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
