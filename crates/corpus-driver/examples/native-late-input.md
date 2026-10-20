# Late input error probe

This safe Rust example exercises a real late input error in the supplied
Fastmash executable. It is a mechanism probe for native fault-test development.
The retained application tests still provide their existing coverage.

```sh
cargo run --release --locked -p fastmash-corpus-driver --example native-late-input -- \
  /path/to/fastmash /tmp/fastmash-late-input-captures
```

Use a new capture directory on Linux or Apple Silicon macOS. The probe first
checks ordinary input with a clean end of stream. It then creates a private
pseudo-terminal and a separate session through safe Nix and Rustix interfaces.
A coordinator places the candidate's process group in the terminal foreground.
The candidate receives the known input and prints a complete prefix while it
remains live. An additional known record makes the preceding output block
visible before the probe selects the fault.

The coordinator then reclaims the foreground. The candidate blocks `SIGTTIN`,
so its subsequent terminal read must report `EIO` under the
[POSIX terminal access rules](https://pubs.opengroup.org/onlinepubs/007904975/basedefs/xbd_chap11.html).
A calibrated comment wakes a read already pending at the transition. The
candidate ignores that comment, keeping one exact expected final output.
The probe requires all supplied data bytes, the expected input-error diagnostic,
exit status 1 and reaped processes. It records the visible prefix, process and
session identities, signal masks, output bytes and hashes.

Execution and cleanup have bounded waits. Cancellation asks the coordinator to
reap the candidate through a pre-established socket before stopping the
coordinator. It needs no new file writes on the capture filesystem.
Use `--cancel` before the executable path to check this cleanup path after a
live prefix, instead of selecting the input fault. That control requires both
processes to be reaped and the candidate to have received `SIGKILL`.
Failed setup or observations
leave partial captures and return a nonzero status.

A successful result establishes this terminal-based late-input mechanism.
Porting the retained fault cases and qualifying full-output faults remain
separate work. The research workflow checks macOS 15 and 26 with Rust 1.88 and
1.98.1 and retains their native observations.
