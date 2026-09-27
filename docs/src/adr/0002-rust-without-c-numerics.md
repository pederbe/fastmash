# 0002: Write everything in Rust, including the numerics

Status: accepted.

Fastmash is written entirely in Rust. The first technical recommendation was
the shortcut: keep numerical kernels in C, using the C library's `long double`
functions, which would have matched GNU datamash's arithmetic most directly.
The project instead required a pure-Rust implementation, keeping C only as a
comparison baseline during development, so that the most delicate code stays
within Rust's safety checks and the project stays a single-language codebase.

**Consequence:** Fastmash implements its own 80-bit arithmetic, parsing,
formatting and transcendental functions, verified against independent
high-precision references. That requirement is also what made
[portable results](0003-portable-results.md) possible.
