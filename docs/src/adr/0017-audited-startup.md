# 0017: Keep one audited startup module

Status: accepted. Records the maintainer decision of 24 October 2026 and its
runtime-configuration clarification of 10 November 2026.

Keep pre-runtime capture of closed standard streams and the GNU Linux process
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

GNU Linux builds retain their direct glibc process entry to avoid Rust's per-process
stack-guard lookup on tiny Commands. It supplies the initial ignored SIGPIPE
disposition that Rust's runtime start would otherwise install. Musl and macOS
use Rust's normal runtime entry and argument initialization after their
constructors capture streams. [ADR 0018](https://github.com/pederbe/fastmash/blob/main/docs/src/adr/0018-static-linux-delivery.md)
records the musl startup decision.

Capture uses no allocation or Rust I/O. Each unsafe operation explains its
safety requirements locally. Ordinary stream reads, writes, seeking and
descriptor completion stay outside this module and use safe interfaces.
The exception does not cover allocator tuning, environment reads, platform
admission or later signal-policy changes.

Runtime configuration uses ordinary safe Rust interfaces. Color preferences
use standard environment reads, as locale and other settings already do.
Requiring allocation-free color reads would add machinery for one startup
failure case while other configuration reads still allocate. Checked data
storage retains its separate allocation-failure behavior.

The macOS deployment target and native archive tests define the supported
platform. A runtime product-version query adds an early refusal on older or
uninspectable systems without selecting a different calculation path, so it is
removed. Unsupported systems have no guaranteed startup diagnostic.

Normal SIGPIPE restoration uses the small MIT-licensed `sigpipe` dependency,
which reuses libc and does not report restoration errors. Signal-mask observation
and blocked-SIGPIPE refusals remain.
Preserving ordinary pipeline behavior does not require a new native wrapper or
a broader unsafe-code exception. Allocator tuning remains a separate migration.
