# Changelog

All notable changes to Fastmash are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Fastmash uses
[Semantic Versioning](https://semver.org/). Before 1.0, a minor version (0.x)
may include incompatible changes; they are always listed under **Changed**.

A change to any numerical output is always listed, together with the
numerical profile version that introduced it.

## [Unreleased]

## [0.1.0] - 2026-09-29

First public release.

### Added

- The GNU datamash 1.9 command language on Linux x86-64 and WSL2: aggregate
  statistics, grouping (`-g`, `groupby`) with optional sorting (`-s`),
  `crosstab`, field modes (`cut`, `reverse`, `noop`), table modes (`check`,
  `transpose`, `rmdup`) and per-row operations (rounding, binning, `getnum`,
  `base64`, checksums, path names).
- Named fields with input headers, generated output headers, custom field and
  output separators, whitespace splitting, NUL-terminated records, comment
  skipping, `--vnlog` input, missing-value removal (`--narm`), `--format` and
  `--round`.
- Portable results: software 80-bit arithmetic, independent of the CPU and
  the C math library, with identical results on every supported machine.
- Built-in locale rules from glibc 2.43, needing no installed locale data:
  numbers in every UTF-8 locale with a one-byte decimal separator, and
  sorting in 194 language locales checked against glibc.
- In-process sorting with disk spill; supervised use of the system `sort` for
  the remaining routes.
  Spill works on any writable `TMPDIR`, using anonymous files where the
  filesystem supports them and private unlinked files elsewhere (NFS, a
  container's `/tmp` before Linux 6.10).

### Differences from GNU datamash 1.9

See [Differences from GNU datamash](https://fastmash.io/guide/differences.html).
