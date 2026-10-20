# Full-output error probe

This safe Rust development example runs the supplied Fastmash executable against
an output destination that actually reports `ENOSPC`. It checks ordinary
successful input first, then requires the existing write-error diagnostic and
exit status 1 for `count`, full `noop` and help output. It calibrates the full
destination before and after each invocation. Candidate execution and cleanup
have bounded waits. The whole probe has a 60-second deadline, with a 10-second
limit for each candidate and bounded child cleanup.

The probe also rejects diagnostic writes to a full stderr destination in plain
and forced-color modes. Ordinary diagnostic controls establish the expected
bytes first. A full stdout destination must preserve those stderr bytes, and
a full stderr destination must preserve ordinary version output on stdout.
The probe checks the full destination before and after each control and records
which descriptor receives it. These controls exercise the supplied executable.

On Linux, use the native full device:

```sh
cargo run --release --locked -p fastmash-corpus-driver --example native-full-output -- \
  /path/to/fastmash /tmp/fastmash-full-output-captures
```

The capture directory must be new. On macOS, supply a separate private mounted
volume as the third argument. The probe refuses volumes outside 1 to 16 MiB.
It creates its own sink and filler, checks that the sink initially accepts
ordinary bytes, fills that volume within a 16 MiB bound and requires a real
one-byte `ENOSPC` before running the candidate. The sink must stay empty after
every failed output. Captures remain outside the full volume.

The research workflow creates an 8 MiB HFS+ disk image, attaches it at a private
mount point and passes that mount point to the probe. Its exit trap detaches the
image on failure. Successful execution also detaches it and removes the image.
It retains setup, calibration, candidate and cleanup observations on macOS 15
and 26 with Rust 1.88 and 1.98.1.

This establishes a development mechanism. Existing full-output transports remain
in place until the retained fault cases have been ported and qualified. It does
not change production startup or qualify a release.
