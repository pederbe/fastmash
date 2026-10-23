# Changelog

All notable changes to Fastmash are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Fastmash uses
[Semantic Versioning](https://semver.org/). Before 1.0, a minor version (0.x)
may include incompatible changes; they are always listed under **Changed**.

A change to any numerical output is always listed, together with the
numerical profile version that introduced it.

## [Unreleased]

### Added

- Optional local timing, allocation and CPU profiling with reproducible Command
  jobs and separate static reports, plus Callgrind instruction counts on Linux
  x86-64. Ordinary builds keep profiling disabled.

- Signed apt and dnf repositories at `packages.fastmash.io`, with package-manager
  updates on Linux x86-64. Installation, upgrade from 0.1.0 to 0.2.0 and removal
  are verified on Debian 12, Ubuntu 24.04 and AlmaLinux 8.

### Fixed

- Refuse safely when the operating system denies the runtime signal-mask query.
- Prepare apt and dnf repository updates without replacing metadata that
  clients are still reading, using retained apt By-Hash indexes and signed
  dnf snapshots.

### Changed

- Reduce decoding work for long CSV fields while preserving strict validation
  and original error locations.
- Speed up plain-number conversion in CSV calculations and reduce sorting work
  and memory for already ordered numerical samples, preserving numerical results.
- Advance the Numerical profile to `portable-binary80-v4` with ICU4X 2.4's
  Unicode 18 and CLDR 49 collation. Updated Group ordering can change seeded
  `rand` selections. Arithmetic, conversion and the random sequence retain
  their v3 rules.
- Use checked Rust storage for numerical workspaces and safe integer division
  for decimal parsing, preserving the v3 arithmetic rules inherited by v4.
- Read sorted Records and Spill buffers through checked Rust slices, retaining
  x86-64 cache hints through a safe interface.

## [0.2.0] - 2026-10-12

### Added

- Homebrew installation on Linux x86-64 through `pederbe/tap/fastmash`,
  with both executables, license notices and installed system-sort checks.
  Source recipes for Nix and Fedora are also prepared; their repository
  admission remains pending.
- Native Apple Silicon macOS 15 and later, with built-in sorting and disk spill.
  Release archives include license notices, checksums and build provenance.
  Installation and command checks run against the same archive on macOS 15
  and 26 before and after release upload.

### Fixed

- Keep the website's installation panel stable during loading and defer its
  script, while retaining every installation command without JavaScript.
- Keep documentation code examples readable in light mode without JavaScript.

### Changed

- Native sorted calculations and sorted `rmdup` on Linux and macOS report
  missing named-key input headers and read failures themselves, without
  starting the system `sort` or Sort supervisor. Missing headers now use the
  native diagnostic on Linux; read errors take precedence, while a command
  containing `wmean` still succeeds without output at clean empty input.
- Give the documentation clearer headings and navigation, distinct example,
  table and diagram panels, and a card layout for the existing starting links.

## [0.1.0] - 2026-10-06

### Added

- The GNU datamash 1.9 command language on Linux x86-64 and WSL2: aggregate
  statistics, grouping (`-g`, `groupby`) with optional sorting (`-s`),
  `crosstab`, field modes (`reverse`, `noop`), table modes (`check`,
  `transpose`, `rmdup`) and per-row operations (`cut`, rounding, binning,
  `getnum`, `base64`, checksums, path names).
- Named fields with input headers, generated output headers, custom field and
  output separators, whitespace splitting, NUL-terminated records, comment
  skipping, `--vnlog` input, missing-value removal (`--narm`), `--format` and
  `--round`.
- Strict quoted CSV input and output, selected independently with `--csv-in`
  and `--csv-out`, or together with `--csv`. Aggregate and per-row reports,
  grouping, headers and full copied fields retain decoded field boundaries;
  sorted CSV uses checked in-process storage with disk spill.
- Custom output labels with `--result-name=INDEX:NAME` for calculated columns,
  including dataset comparison's before, after and change columns.
- Table health inspection with bounded examples and readable or versioned TSV
  reports. Supplied type, presence and width rules can fail validation while
  inferred findings remain advisory. Health accepts ordinary delimited text.
- `top:N FIELD` and `bottom:N FIELD`: highest or lowest complete records,
  globally or per group, with stable original-input ties, explicit headers,
  text/CSV input and output, and sorted spill support.
- `wmean VALUE:WEIGHT` with finite nonnegative contribution weights, complete-pair
  missing-value removal, headers, grouping and checked binary80 range limits.
- `compare BEFORE AFTER`: numerical summaries aligned by complete keys, signed
  differences and percentage changes, optional change ranking, independent
  header binding and text/CSV formats. Both inputs finish before report output.
- Structured help and stream-specific terminal color for help, failure prefixes
  and readable health reports, with `--color=auto|always|never` and `--no-color`.
  Calculation data, CSV, health TSV and version output remain plain.
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
