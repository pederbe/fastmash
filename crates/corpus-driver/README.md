# Regression commands

`fastmash-corpus` checks commands in the existing compressed regression corpus,
through either piped or regular-file standard input. Its default run also checks
directory input, output to a pipe whose reader has closed and full-output errors. It
verifies the frozen fixture and provenance, records executable identities and
compares exact bytes and status. It preserves byte arguments, the isolated
environment, the eight-second execution deadline and the 524,288-byte output
guard. On Unix, each invocation owns a process group that is cancelled on completion or
failure. Candidate processes must retain inherited credentials and remain in
that group and session. Exit is observed without reaping the leader, so its PID
stays owned until group cancellation finishes. On macOS, signalling a group with
only zombies can return EPERM. That result is accepted only after a fresh
terminal-status observation of the still-owned leader. Live-leader permission
errors remain failures.

Linux admission reads the current thread's kernel `SigBlk` mask, including
real-time signals, from `/proc/thread-self/status`. macOS uses the native signal
mask API. Both require an unblocked inherited mask. The kernel
[thread identity](https://man7.org/linux/man-pages/man5/proc_thread-self.5.html)
and [blocked-signal mask](https://docs.kernel.org/filesystems/proc.html)
documentation define the Linux observation.

```sh
cargo build --release -p fastmash -p fastmash-corpus-driver --bins
```

Use the candidate's package version and fresh directories for the two input modes:

```bash
bash -c '
set -eu
version=$(cargo pkgid -p fastmash)
version=${version##*@}
target/release/fastmash-corpus --binary target/release/fastmash --expected-version "$version" --evidence-dir /var/tmp/ordinary-pipe
target/release/fastmash-corpus --binary target/release/fastmash --expected-version "$version" --stdin-file --evidence-dir /var/tmp/ordinary-file
'
```

On Unix, the default run executes all 3,137 frozen cases: 2,857 ordinary cases, 22 directory-input
cases, 51 closed-output-pipe cases, 72 full-output cases and 65 early commands
with stdin held open, plus 67 held-output observations and three late-input errors. Regular-file mode executes the same 2,857 ordinary cases as before
and records why each other case belongs to the default Rust run. Both modes
account for all 3,137 case IDs. Application CI, contributor commands and native
archive installation checks use this driver.
Native macOS keeps the two existing missing-header diagnostic adaptations and
the four buffer-size adaptations described below.
Running this tool on Linux supplies Linux evidence only.

Early-command cases retain an empty stdin pipe until the actual executable
exits. Their evidence records whether the writer remained open when exit was
observed. A timeout fails the case and cancels the owned process group.

Held-output cases keep stdin open for two seconds after delivering input,
record the actual output prefix and whether the process is still alive, then
close stdin and compare the completed output. Destinations are regular files,
pipes or raw terminals. Native macOS preserves the four existing buffer-size
adaptations using frozen bytes and observed destination facts.

Late-input cases first check ordinary completion against the actual executable.
They then retain exact input, observe a live acknowledgement through a raw
terminal, stop the candidate, reclaim its input terminal foreground and resume
it. The next read produces a real EIO without added wake bytes. Captures include
the acknowledgement, terminal/session identities, signal masks and reaping.
The coordinator survives terminal hangup to cancel its group when the driver
disappears. The candidate keeps its normal hangup signal setting.

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
resource-limit and process-group APIs. The private launcher applies limits in a
fresh process and then replaces itself with the selected executable, preserving
its exit and signal status without a first-party unsafe pre-exec hook. The
upstream [package declaration](https://docs.rs/crate/nix/0.31.3/source/Cargo.toml),
[signal-mask API](https://docs.rs/nix/0.31.3/nix/sys/signal/fn.pthread_sigmask.html),
[resource-limit API](https://docs.rs/nix/0.31.3/nix/sys/resource/fn.setrlimit.html),
[session API](https://docs.rs/nix/0.31.3/nix/unistd/fn.setsid.html) and
[process-group signal implementation](https://github.com/nix-rust/nix/blob/v0.31.3/src/sys/signal.rs)
describe those contracts.

`fastmash-test-process` prepares direct native stream tests in a fresh process.
It sets an alarm and a selected signal mask, creates a real broken output pipe
when requested, then replaces itself with the actual executable. It preserves
raw argument and environment bytes, PID, descriptors and final signal status.
Private setup variables are removed from the candidate environment. Ordinary
execution uses `Command::exec`. The inherited-ignored-SIGPIPE control uses safe
POSIX `execve` to retain Rust's ignored disposition, calibrated by an actual
failed pipe write. The launcher and migrated stream suite forbid unsafe code.

The stack-limit fixture lowers the soft limit while preserving the inherited
hard limit. On Linux, the pinned Apache-2.0 OR BSD-3-Clause
[seccompiler dependency](https://docs.rs/seccompiler/0.5.0/seccompiler/fn.apply_filter.html)
installs a filter in the fresh launcher that makes the actual `close_range`
syscall return `EINVAL` or `ENOSYS`. This exercises native fallback on older
kernels without affecting other syscalls. The filter survives target execution
and applies to its descendants. It belongs to this unpublished development
crate and does not enter product archives.

## Windows transports

The Windows driver uses the same frozen fixture, exact numerical bytes and
command interface. Default input runs 3,088 cases, including native directory,
closed-pipe, full-volume, early-exit and held-output checks. Regular-file input
runs 2,843 ordinary cases. Every run records a disposition for all 3,137 IDs.
Fourteen raw Unix-byte argument cases, 32 Unix PTY cases and three POSIX
controlling-terminal read failures are excluded with explicit reasons. Separate
ConPTY tests check console Unicode and color. Exclusions are not Windows coverage.

Safe `process-wrap` and `win32job` interfaces suspend each child, assign it to
owned Jobs before its code runs and resume it. Completion or failure cancels the
whole Job. An independent Job membership query must show no remaining processes
before cleanup is confirmed. Pipe reader and input writer threads have bounded
completion waits. The eight-second per-command deadline and 524,288-byte output
guard remain in force. The output guard limits test captures, not product data.
Three held-output cases retain a real regular-file destination, with bounded
prefix reads. The other admitted held-output cases use actual pipes.

Windows reports a nonzero broken-pipe write failure instead of Unix SIGPIPE.
Where Unix terminates before a primary command diagnostic, Windows reports that
diagnostic followed by output completion failure. The fixed 8,192-byte Windows
output buffer changes four held-prefix observations and one full-destination
finalization diagnostic. Each adaptation is named in the driver and recorded in
`dispositions.jsonl`. The frozen fixture and its provenance remain unchanged.

Full-output tests require `FASTMASH_TEST_FULL_VOLUME`, pointing to the private
mounted volume prepared by [windows-volume.ps1](../../.github/scripts/windows-volume.ps1).
The maintained setup creates a disposable 64 MiB NTFS image. Calibration fills
one newly created file, stops after at most 128 MiB of writes, then verifies that
a single byte fails with native disk-full status before and after each candidate.
The driver removes its fill file. The setup owner detaches and removes the image
even after a failed test. Calibration and volume receipts identify the actual sink.

PowerShell workflow tests run the product under the 7.4 baseline and current
stable shell. They distinguish raw native pipelines and stdout redirection from
PowerShell string pipelines. Their file-input route launches `cmd.exe /d /v:off`
with environment-variable indirection for executable and input paths, so the
shell streams the file without decoding it or loading it into memory. Tests
include spaces, Unicode, single quotes, literal `%NAME%`, ampersands and `!`,
also from a parent with delayed expansion enabled. See
[Testing](../../TESTING.md#native-windows-command-validation) for entrypoints.

Use `--shard PART/TOTAL` with zero-based `PART` when a runner needs bounded
phases. Shards partition the unchanged fixture order and retain their selection
in `identity.json`. All shards are required for complete default-mode coverage.
