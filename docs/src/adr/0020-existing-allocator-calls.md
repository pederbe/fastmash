# 0020: Retain the existing GNU Linux allocator calls

Status: accepted. Partly supersedes the exclusive unsafe-code scope in
[0017](0017-audited-startup.md). Its audited startup boundary remains unchanged.

Retain only the existing `mallopt(M_ARENA_MAX, arenas)` and `malloc_trim(0)`
calls in `crates/cli/src/malloc_arenas.rs` under a narrow unsafe-code exception
for GNU Linux. This preserves allocator behavior while the product modules
consolidate into one binary crate.

The arena cap runs before application threads start, respects an explicitly
configured arena count and retains its existing address-space-limit calculation.
Trimming retains its existing arguments and execution points. Neither call
changes numerical rules, allocation-failure handling or the behavior on musl
and macOS.

The binary package denies unsafe code. Allow it only on the audited startup
module and the two existing allocator blocks, each with its local safety
explanation. The private conversion, representation, arithmetic and Spill
modules continue to forbid unsafe code. This exception permits no additional
allocator calls or other native behavior.

Removing allocator controls could change memory use and refusals under resource
limits. Replacing the allocator or introducing another wrapper would be a
separate change needing its own justification and validation. Retaining the
existing calls avoids coupling that work to crate consolidation.
