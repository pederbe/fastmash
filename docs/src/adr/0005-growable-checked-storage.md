# 0005: Growable, checked storage instead of fixed limits

Status: accepted.

Early prototypes used fixed caps (record length, number of operations, number
of samples) to make resource use easy to reason about. Real workloads exceeded
them. Fastmash now has no such caps: storage grows with the job, every
allocation is checked, and allocation failure becomes a refusal where Rust and
the operating system allow it. Sorting spills to disk to limit memory.

**Consequence:** memory use grows with the largest group for operations that
need every value, such as quantiles. This is documented, together with which
data spills and which does not. A few narrow limits remain (for example on the
length of a single number) and are documented.
