# 0007: Linux first

Status: superseded by [ADR 0011: Native release delivery](0011-native-release-delivery.md), which published macOS support in 0.2.0.

The initial release supported Linux with glibc and Windows through WSL2.
Although software arithmetic was already portable, process supervision,
temporary storage and signal handling used Linux-specific interfaces.

Limiting the first release to Linux allowed native command and installation
checks against a defined compatibility baseline. Cross-compiling alone would
not establish runtime support on another platform. Native macOS support followed
through [0010](0010-native-apple-silicon.md) and 0011. Native Windows remains
outside the product scope.
