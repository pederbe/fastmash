# 0011: Qualify and deliver the same native archive

Status: accepted for 0.2.0 release delivery. Supersedes the development-only
archive boundary in [ADR 0010: Native Apple Silicon macOS](0010-native-apple-silicon.md).
Published 0.1.0 support remains Linux-only until 0.2.0 is published. At that
point this also supersedes the Linux-only scope of [ADR 0007: Linux x86-64
first](0007-linux-first.md).

## Decision

Build the Apple Silicon executable twice in separate clean target trees on
macOS 15, using the pinned Rust compiler, remapped source paths and a macOS 15
deployment target. Require identical executable bytes, then package one copy
with target-specific notices, its executable checksum and build provenance.
This proves reproducibility within that hosted build; it does not promise
identical output from different future hosted SDK images.

Freeze the resulting archive's SHA-256 as an output of that workflow run.
Install those exact bytes outside the source checkout on macOS 15 and 26.
Run the applicable installed Command contract, Numerical profile fixtures,
Spill, cleanup, signal and both corpus input modes before accepting it.
The release uses this archive directly instead of rebuilding it after testing.

The Release workflow also requires Linux's existing committed qualified
checksums and package checks. Only after both platforms pass does it attest
the archives and Linux packages and prepare a draft release. Download the
draft's macOS archive on both native versions, verify its release-workflow
attestation and frozen hash, and repeat the installed checks. A failed delivery
check leaves an unpublished draft for investigation. The maintainer publishes
only after the complete workflow succeeds and the draft is reviewed.

Keep public download links and release metadata tied to
`site/release-version.txt`, which names the published release. Candidate source
versions and platform documentation can advance without pointing users at
unavailable downloads. Update that file and published platform claims only
when the corresponding release is available.

## Considered options

- Committing a future macOS executable hash would require rebuilding on a
  later hosted SDK image and assuming those bytes can be recreated. Reusing
  the tested archive avoids that assumption. Linux retains its controlled
  container build and committed qualification manifest.
- Rebuilding the macOS archive for upload would break the binding between
  native installation evidence and delivered bytes.
- Treating a CI artifact as published support would give users a download
  promise the latest release cannot yet meet.

Intel macOS, earlier macOS, native Windows and Apple notarization remain
outside this platform scope. Hosted GNU comparisons retain their measured
conditions; Linux performance claims do not become macOS claims.
