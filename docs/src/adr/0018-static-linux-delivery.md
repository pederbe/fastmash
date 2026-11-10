# 0018: Deliver static Linux binaries with musl

Status: accepted for Linux delivery. Supersedes the glibc 2.28 build baseline
in ADRs 0011 and 0015. The startup adaptation remains subject to the audited
boundary in ADR 0017.

Publish the Linux executable as `x86_64-unknown-linux-musl`, statically linked.
The same bytes go into the archive and Debian and RPM packages. They run on
both glibc and musl distributions without a system glibc requirement. Keep
current stable Rust, the baseline x86-64 CPU target, two-build reproducibility,
installed checks and reuse of tested artifacts.

Musl's initial grouping and sorting costs were material. Reusing Rayon workers,
using safe memx comparison interfaces and selecting musl's portable copy
routines brought eight sampled operations within about 8% of the previous
glibc build. Four sorting and grouped-statistics workloads remained about
15–24% slower, or 17–29% slower than glibc with the shared Rust improvements.
Peak resident memory was lower in every sampled operation. These observations
support the delivery tradeoff, not a universal performance claim.

The portable copy routines are unmodified musl 1.2.6 library sources, bundled
with their copyright. A safe Rust build script compiles them with the ordinary
C compiler and selects them through linker wrapping. Application comparisons
call memx's safe API. No custom allocator is needed.

Keep glibc source builds working with their existing allocator controls.
Supporting musl delivery does not establish that those controls can be removed
from glibc builds. The quick installer tries the static archive first and can
install older glibc archives on glibc systems. A checksum failure never selects
another archive.

The alternative was to retain glibc delivery and use the shared Rust
improvements for faster sorting. Static deployment across distributions and
removal of glibc-specific tuning from the distributed executable justify the
remaining measured cost. Mimalloc and jemalloc trials did not justify their
additional memory or build cost. Worker counts and allocator limits remain
implementation choices, assessed through useful command speed and memory use.
