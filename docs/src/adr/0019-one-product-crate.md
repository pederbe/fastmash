# 0019: One product binary crate

Status: accepted. Supersedes the five-package version and publication arrangement
in [0016](0016-release-plz.md).

Fastmash's supported interface is its command. Numeric conversion, canonical
binary80 storage, software arithmetic and native Spill support are private
modules of the `fastmash` binary crate in `crates/cli`. Keep checked constructors,
field privacy, refusal behavior and independent numerical fixtures at those
module boundaries.

Separate product libraries require coordinated versions and uploads without
providing another supported product. A combined library and binary would retain
a public Rust interface solely for direct tests. Module tests exercise the same
implementation without that interface. The numeric representation keeps its
rules but no longer has a separate `no_std` compilation boundary.

Retain unpublished maintenance packages for corpus checks, locale generation,
license notices and publication checks. Profiling examples and other development
targets remain useful. Locale generation compares its emitted source with a
fixture that the product tests compile against the real profile type.

Weekly release preparation and artifact qualification remain unchanged. Future
Cargo uploads handle only `fastmash`, preserving immutable-version checksum
checks and retry behavior. Previously published helper packages and historical
release artifacts remain available.

This changes source and publication boundaries without changing algorithms,
command behavior or the Numerical profile. Crate boundaries can affect compiler
optimization, so performance acceptance retains representative predecessor
guards under the existing release settings.
