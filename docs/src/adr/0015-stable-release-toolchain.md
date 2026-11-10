# 0015: Build releases with current stable Rust

Status: accepted. Supersedes the pinned compiler requirement in
[ADR 0011](0011-native-release-delivery.md).

Build releases with current stable Rust and record the actual compiler and
native build tools in the archive's provenance.

An exact compiler pin required synchronized archive downloads and checksums
without improving the installation checks. Reproducibility still compares two
clean builds within the same run. Different future compiler or SDK versions may
produce different bytes.

The glibc baseline is superseded by [ADR 0018](0018-static-linux-delivery.md).
Keep the CPU target, artifact checksums, installed checks and
reuse of the tested artifacts. Historical qualification manifests identify past
measurements, not the compiler for a new release. The
[release guide](https://github.com/pederbe/fastmash/blob/main/RELEASING.md)
describes toolchain resolution and artifact checks.
