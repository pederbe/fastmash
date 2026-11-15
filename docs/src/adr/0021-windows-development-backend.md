# 0021: Add a safe native Windows development backend

Status: accepted for development. Partly supersedes [0010](0010-native-apple-silicon.md)
by admitting Windows 11 x64 builds with the MSVC target. Windows release delivery
remains unqualified under [0011](0011-native-release-delivery.md) and
[0013](0013-reuse-tested-release-artifacts.md).

## Context

Windows standard handles differ from Unix descriptors. Cloning a null standard
handle can succeed, and ordinary Rust streams can then silently accept I/O.
Pipe metadata can identify input as a file and permit a seek that does not replay
its bytes. Ignoring these differences would permit silent success or lost input.

## Decision

Keep the backend private within the product binary. Reject null and invalid
standard-stream sentinels before safe handle cloning. Use owned copies for raw
file and pipe I/O, explicitly checking application-buffer completion and the
consuming close of each used copy. Leave inherited original handles to process
cleanup. Windows completion therefore promises neither early pipe EOF before
exit nor a checked final close of the originals. Unix completion stays unchanged.

Use Windows-only `io-close` for checked consuming close and `winapi-util` for
native file-type admission. Both offer safe interfaces and compatible licenses.
Require native disk type and nonempty regular-file metadata before file replay.
Pipes and unknown types use the existing streaming routes.

Keep redirected data binary. Use Rust's Unicode console interfaces for terminals,
retaining incomplete UTF-8 scalars between application-buffer writes and refusing
invalid console text. Accept Unicode command arguments and native paths without
lossy replacement. Refuse arguments containing unpaired UTF-16 code units with
status 77. Dataset bytes and escaped byte selectors retain their existing rules.

Classify Windows errors by their native meaning, including broken pipes reported
as error 232. Failed reads, writes and completion remain visible in final status.
Keep entropy failures as failures. Create spill files exclusively with random
names, restrictive sharing and delete-on-close so termination also removes them.
Initially use a fixed 64 MiB sort-chunk target, with the existing explicit memory
override. This is neither a record limit nor a total-process memory bound.

No new first-party unsafe code is permitted. The exceptions in
[0017](0017-audited-startup.md) and [0020](0020-existing-allocator-calls.md)
do not expand. Numerical rules and the Numerical profile remain unchanged.

## Consequences and alternatives

Focused native tests qualify the backend mechanisms. Full Windows command,
console, PowerShell 7.4+ and exact-archive installation qualification are still
required before advertising support or adding Windows release artifacts.

Closing the original standard handles would require an additional safe ownership
handover mechanism or a separately approved unsafe exception. Owning copies
avoids that exception at the cost of the explicit lifetime limit above. Disabling
file replay entirely would avoid a file-type dependency but lose the existing
file-specific grouping route. WSL2 remains the supported Windows workflow while
native release qualification is incomplete.
