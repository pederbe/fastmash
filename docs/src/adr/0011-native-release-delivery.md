# 0011: Qualify and deliver the same native archive

Status: accepted for archive reproducibility and installed delivery. Supersedes
the development-only boundary in [0010](0010-native-apple-silicon.md) and the
Linux-only platform scope in [0007](0007-linux-first.md).

Build the macOS executable twice in clean target trees within one hosted run.
Require identical bytes, package one build, and test that same archive on each
supported baseline before delivering it. This establishes reproducibility
within the run, not identical future builds under different SDK images.

Rebuilding for upload would sever the connection between tested and delivered
bytes. Committing a future macOS checksum would assume that a later hosted SDK
can recreate the same executable. Neither is needed when the tested archive
can be retained. A development CI artifact also cannot establish that a public
release is available.

Later decisions replace parts of the original procedure: [0012](0012-release-delivery-and-performance.md)
separates performance acceptance and enables automatic publication,
[0013](0013-reuse-tested-release-artifacts.md) removes duplicate installation
checks and tag-stored Linux manifests, [0015](0015-stable-release-toolchain.md)
selects stable Rust, and [0016](0016-release-plz.md) defines download-version
authority. The current procedure is in the
[release guide](https://github.com/pederbe/fastmash/blob/main/RELEASING.md).
