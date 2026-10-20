# Full-volume lifetime control

This safe Rust development example checks that an owned macOS disk image is
detached and removed after ordinary completion and panic unwinding. Each control
runs the actual full-output probe against its own 8 MiB HFS+ image. The probe
requires successful ordinary output, real `ENOSPC`, the write-error diagnostic
and exit status 1. Captures stay outside the full volume.

Build the candidate and full-output probe with the ordinary release profile.
Build only this lifetime control with panic unwinding enabled:

```sh
cargo build --release --locked -p fastmash --bin fastmash
cargo build --release --locked -p fastmash-corpus-driver --example native-full-output
env CARGO_PROFILE_RELEASE_PANIC=unwind cargo build --release --locked \
  -p fastmash-corpus-driver --example native-full-volume-lifetime
target/release/examples/native-full-volume-lifetime \
  /path/to/fastmash /tmp/fastmash-full-volume-lifetimes
```

The capture directory must be new. An abort-profile lifetime control refuses
execution before creating a disk image. The research workflow records the
separate panic settings and checks that building the lifetime control leaves
the candidate executable unchanged.

The inner probe has a 60-second overall deadline, with a 10-second limit for
each candidate. It kills and reaps a timed-out candidate. The lifetime control
allows 90 seconds for that probe and 15 seconds for each image utility. Cleanup
failure fails the control and preserves its logs and image path. Successful
normal and unwind controls each retain a `cleanup.ok` receipt.

A second pair exercises the corpus driver's `FullOutput` owner directly. Its
selected sink must close before the image detaches, on both completion and
unwinding. These controls retain `volume/cleanup.ok` below `owned-normal` and
`owned-unwind`. The native workflow also runs the frozen corpus through that
owner with both input modes, retaining the calibration and cleanup records.

Cleanup force-detaches only the private mount created by that control, after
the probe and its candidates have exited and been reaped. Ordinary detach can
report a busy volume at this point on macOS. All captures remain outside the
image, and a failed forced detach still fails qualification.

The native workflow runs on macOS 15 and 26 with Rust 1.88 and 1.98.1. This
qualifies resource ownership for a development mechanism. Existing full-output
test transports remain in place until their retained fault cases are ported
and qualified.
