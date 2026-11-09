# 0002: Rust for the program and numerical libraries

Status: accepted.

Fastmash's executable and numerical libraries are written in Rust. Keeping the
numerical kernels in C with the host's `long double` functions would match GNU
datamash's arithmetic more directly. Implementing them in Rust keeps the
numerical code within Rust's safety checks and avoids a second implementation
language in the product.

**Consequence:** Fastmash implements its own 80-bit arithmetic, parsing,
formatting and transcendental functions, verified against independent
high-precision references. That choice is also what made
[portable results](0003-portable-results.md) possible.
