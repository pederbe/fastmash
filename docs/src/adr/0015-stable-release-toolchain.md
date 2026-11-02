# 0015: Build releases with current stable Rust

Status: accepted. Supersedes the pinned compiler requirement in
[ADR 0011](0011-native-release-delivery.md).

Resolve current stable Rust through rustup for each release build. Rebuild the
Linux builder without cached installation layers so a new stable release is
selected automatically. Record the actual Rust and native build-tool versions in
the archive's build provenance.

An exact compiler pin required synchronized archive downloads and checksums
without improving the installation checks. Reproducibility still compares two
clean builds within the same run. Different future compiler or SDK versions may
produce different bytes.

Keep the glibc baseline, CPU target, artifact checksums, installed checks and
reuse of the tested artifacts. Historical qualification manifests identify past
measurements and remain unchanged. They do not select the compiler for a new
release.
