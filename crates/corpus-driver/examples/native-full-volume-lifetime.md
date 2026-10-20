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

Cleanup resolves the owned image's current whole device from `hdiutil info
-plist`, after the probe and its candidates have exited and been reaped. A busy
detach can already have removed the mount association. Cleanup therefore checks
the image binding afresh before each attempt and verifies its absence before
removing the image. At most three attempts share one 15-second cleanup budget.
Unknown bindings, other errors and an image that remains attached fail the
control. All captures remain outside the image.

The [hdiutil manual](https://keith.github.io/xcode-man-pages/hdiutil.1.html)
distinguishes image attachment from filesystem mounting. The pinned MIT-licensed
[plist parser](https://docs.rs/crate/plist/1.10.1/source/Cargo.toml.orig) reads
the native image/device associations, including non-JSON property-list values.
Its Rust 1.88 minimum matches this workspace's supported minimum. Text parsing
or a cached disk identifier cannot establish the same current ownership.

The native workflow runs on macOS 15 and 26 with Rust 1.88 and 1.98.1. This
qualifies resource ownership for a development mechanism. Existing full-output
test transports remain in place until their retained fault cases are ported
and qualified.
