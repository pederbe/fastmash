# 0002: Write everything in Rust, including the numerics

Status: accepted.

Fastmash is written entirely in Rust. The alternative was to keep the
numerical kernels in C, using the C library's `long double` functions, which
would match GNU datamash's arithmetic most directly. Fastmash implements them
in Rust instead, using C only as a comparison baseline during development, so
that the most delicate code stays within Rust's safety checks and the project
stays a single-language codebase.

**Consequence:** Fastmash implements its own 80-bit arithmetic, parsing,
formatting and transcendental functions, verified against independent
high-precision references. That choice is also what made
[portable results](0003-portable-results.md) possible.
