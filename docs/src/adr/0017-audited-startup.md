# 0017: Keep one audited startup module

Status: accepted. Records the maintainer decision of 24 October 2026.

Keep pre-runtime capture of closed standard streams and the Linux process
entry with its initial SIGPIPE disposition in `crates/cli/src/startup.rs`.
These are the only approved exceptions to the first-party unsafe-code
prohibition. Any extension requires a new maintainer decision.

Rust repairs closed standard descriptors before ordinary application code can
observe them. Safe standard-stream interfaces cannot recover the original
state. The startup module captures it before that repair, then occupies each
closed descriptor with `/dev/null` opened in the wrong direction. Later opens
cannot take a standard descriptor's place, and the Command still reports an
invalid input or output when it uses that stream. Replacing a closed input
with a successful empty input would violate GNU compatibility.

Linux retains its direct glibc process entry to avoid Rust's per-process
stack-guard lookup on tiny Commands. It supplies the initial ignored SIGPIPE
disposition that Rust's runtime start would otherwise install. macOS keeps
Rust's normal runtime entry, after the Mach-O constructor captures streams.

Capture uses no allocation or Rust I/O. Each unsafe operation explains its
safety requirements locally. Ordinary stream reads, writes, seeking and
descriptor completion stay outside this module and use safe interfaces.
The exception does not cover allocator tuning, environment reads, platform
admission or later signal-policy changes. Remaining migrations must preserve
their failure and resource behavior rather than expanding this exception.
