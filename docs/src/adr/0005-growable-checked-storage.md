# 0005: Growable, checked storage instead of fixed limits

Status: accepted.

Fixed caps (record length, number of operations, number of samples) make
resource use easy to reason about, but real workloads exceed them. Fastmash
uses growable, checked storage. Allocation failure becomes a refusal where Rust and
the operating system allow it. Sorting spills to disk to limit memory.

**Consequence:** memory use grows with the largest group for operations that
need every value, such as quantiles. Sorting spills, but retained samples do
not. The remaining compatibility limits, including 99-byte `--format` strings
and 511-byte command names, are listed in [the guide](../guide/output.md#limits).
