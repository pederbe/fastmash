# 0010: Native macOS

Status: accepted platform decision. The development-only release boundary was
superseded by [ADR 0011](0011-native-release-delivery.md). Linux external sorting
was superseded by [ADR 0014](0014-native-sorting-everywhere.md).

Support `aarch64-apple-darwin` on macOS 15 and later. Establish runtime support
through native command, numerical and installed-archive checks on macOS 15 and
26. Cross-compilation alone cannot establish those behaviors.

Use native interfaces for streams, signals, randomness and temporary storage.
Software arithmetic and bundled locale rules preserve
[portable results](0003-portable-results.md). Built-in stable sorting includes
disk spill, so macOS needs no external sorter.

The platform scope excludes Intel macOS, macOS before 15 and native Windows.
Linux benchmark results do not become macOS performance claims. The
[testing guide](../../../TESTING.md#native-macos-development-gate) and
[release guide](https://github.com/pederbe/fastmash/blob/main/RELEASING.md#native-macos-release-delivery)
describe the checks.
