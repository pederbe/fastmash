# 0005: Growable, checked storage instead of fixed limits

Status: accepted.

Fixed caps (record length, number of operations, number of samples) make
resource use easy to reason about, but real workloads exceed them. Fastmash
has no such caps: storage grows with the job, every
allocation is checked, and allocation failure becomes a refusal where Rust and
the operating system allow it. Sorting spills to disk to limit memory.

**Consequence:** memory use grows with the largest group for operations that
need every value, such as quantiles. This is documented, together with which
data spills and which does not. The only fixed limits left are GNU datamash's
own, such as 99-byte `--format` strings and 511-byte names, and they are
documented.
