# Portable numerics test expectations

Exact expectations for the portable binary80 primitives, generated from an
independent multiple-precision oracle: `v2/` for the v2 profile and `v3/` for
the accepted v3 profile (finite and special operands, exact helper results).
`src/tests.rs` and `src/v3_tests.rs` compare every row. The files are kept
byte-identical to the oracle's output; do not edit them by hand.
