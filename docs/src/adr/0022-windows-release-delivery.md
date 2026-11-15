# 0022: Qualify native Windows release archives

Status: accepted. Supersedes the Windows exclusion in [0010](0010-native-apple-silicon.md)
and the development-only Windows boundary in [0021](0021-windows-development-backend.md).
Extends [0011](0011-native-release-delivery.md). The exact-artifact requirement in
[0013](0013-reuse-tested-release-artifacts.md) remains in force.

## Decision

Build native Windows archives for Windows 11 x64 with the MSVC target. Support
PowerShell 7.4 and later. Use the existing archive naming and tar.gz layout,
including the executable suffix used by cargo-binstall. Do not add a second
installer format, package-manager catalog or signing service.

Link the Microsoft C runtime statically and inspect the PE machine type and
normal and delayed DLL imports. Permit only the named Windows system libraries.
Record the compiler, MSVC and SDK versions, source revision, flags, executable
hash and build kind. Generate notices from the Windows normal dependency graph.

Require two byte-identical optimized executable builds in separate empty target
trees, then package one. Install the frozen archive outside the checkout and
compare the executable and every packaged document with independently retained
build files. Run Windows stream, path, spill and console checks, both PowerShell
workflows and both applicable corpus input modes against that installed executable.
Verify its identity again after testing. Deliver those archive bytes without rebuilding.

The first qualification installs that same hosted archive on Windows 11 using
the PowerShell 7.4 baseline and current stable shell. Repeat native Windows 11
qualification before merging changes to Windows runtime behavior, streams,
paths, spill, installation, compiler/SDK configuration, native dependencies,
the PowerShell baseline or the documented file-input recipe.

Each weekly archive receives hosted Windows installation and command evidence.
This does not mean every later weekly archive was executed on Windows 11.
Unlike macOS, Windows has no per-release desktop-VM dispatch dependency.
Hosted images and the stable toolchain can advance between native qualifications.
The Windows 11 receipt states the exact archive, source and toolchain it covers.

Keep the existing numerical profile and safe-Rust boundaries. Inherited original
standard handles remain with process cleanup as recorded in 0021. Unix-specific
corpus mechanisms retain explicit exclusions and are never counted as Windows
coverage. Windows speed claims need their own measurements.

## Consequences

The static runtime avoids a separate Visual C++ runtime installation for the
archive. It remains Windows software using system DLLs, rather than a fully
self-contained operating environment. The import audit and native execution
check this boundary without removing developer tools or changing Windows protections.

A Windows 11 gate on every weekly archive would require a new desktop dispatch
and export dependency. Initial and relevant-change qualification, together with
hosted checks of each exact archive, preserves unattended releases with the
existing machines. Native download instructions become applicable only to
published releases that actually contain a Windows archive. WSL2 remains valid
for earlier releases. Preparation does not itself publish a release.
