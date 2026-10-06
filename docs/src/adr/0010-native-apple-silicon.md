# 0010: Native Apple Silicon macOS

Status: accepted for development. The published 0.1.0 platform scope remains
[ADR 0007: Linux x86-64 first](0007-linux-first.md).

Add native `aarch64-apple-darwin` support with macOS 15 as the minimum, verified
on native hosted macOS 15 and 26 runners. Cross-compilation cannot establish
runtime support, and the maintainer has no macOS hardware. Support requires
the complete applicable Command contract and Numerical profile checks, plus
installation and execution of the same checksummed archive on both baselines.
Once delivered in a release, this platform scope supersedes ADR 0007.

Use the built-in Sort route, including Spill, on macOS. Keep Linux external
supervision and its helper confined to Linux. Native OS primitives implement
standard streams, signals, randomness, terminal detection and private temporary
storage; software arithmetic and built-in locale rules preserve Portable
results under [ADR 0003: Portable results](0003-portable-results.md).

Build the archive once with `MACOSX_DEPLOYMENT_TARGET=15.0`, record source,
compiler, flags and hashes, and install those bytes outside the checkout.
The archive contains one executable with target-specific license notices.
The installer uses macOS's checksum tool and retains an existing installation
when a download or checksum fails. CI artifacts are development evidence;
publishing a release remains a separate maintainer action.

Intel macOS, macOS before 15, native Windows and macOS external supervision
remain outside scope. Hosted GNU comparisons describe the tested workloads
and runners, without carrying Linux benchmark claims over to macOS.
