# Changelog

All notable changes to Fastmash are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Fastmash uses
[Semantic Versioning](https://semver.org/). Before 1.0, a minor version (0.x)
may include incompatible changes; they are always listed under **Changed**.

A change to any numerical output is always listed, together with the
numerical profile version that introduced it.

## [Unreleased]

### Fixed

- The installation guide's cargo-binstall command for macOS names the crate
  before `--bin fastmash`, which cargo-binstall otherwise rejects.

### Changed

- Use built-in sorting for every sorted calculation in the C locales, including
  mixed calculations, other means, higher moments, normality tests, paired
  statistics and `rand`, without requiring the system `sort` or Sort supervisor.
  Stable contribution order and the Numerical profile are preserved. Other
  means, higher moments, normality tests and paired statistics grouped by one
  key prepare records on several threads. On Linux, large numerical sorts can
  merge spilled records and calculate in parallel within the configured thread
  limits, and under a cgroup memory limit write their runs back to storage as
  they go. Sorted `rmdup` now reports its route under `FASTMASH_SORT_TRACE`.

- cargo-binstall no longer falls back to third-party QuickInstall builds, and
  each release checks that it installs the published executables on Linux
  x86-64 and Apple Silicon macOS.

- Homebrew installs prebuilt bottles that follow each release within a day, and
  the installation guide now leads with apt, dnf and Homebrew.

## [0.3.0] - 2026-10-26

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

- Animate the website's landing page with scroll entrances, a breathing hero
  glow and a pulsing terminal halo. Decorative motion runs only in view, a
  button pauses it, and reduced-motion settings keep the page static.

- Under a cgroup memory limit, let each built-in sort and hash grouping grow
  to half of what is free there, with growth shared among running Fastmash
  processes instead of one share per processor. Sorts and hash grouping now
  start at 16 MiB and check the limit before taking 64 MiB. A lone sort in a
  container or service with a memory limit of about 200 MiB or more spills
  less often, and sorts started beside it spill instead of running out of
  memory. Under smaller limits a sort may spill from 16 MiB chunks. Output is
  unchanged.

- Reduce decoding work for long quoted CSV fields while preserving strict
  validation and original error locations.

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

### Other changes

- Separate weekly delivery checks from performance acceptance (#154) ([3002d5f](https://github.com/pederbe/fastmash/commit/3002d5f))
- Require qualification of exact versioned release binaries (#153) ([9f7e277](https://github.com/pederbe/fastmash/commit/9f7e277))
- Preserve breaking changes declared in merge messages (#152) ([00b6cec](https://github.com/pederbe/fastmash/commit/00b6cec))
- Refresh the website after automated release delivery (#151) ([ab9de62](https://github.com/pederbe/fastmash/commit/ab9de62))
- Automate qualified weekly releases and changelogs (#150) ([2dbe17a](https://github.com/pederbe/fastmash/commit/2dbe17a))
- Update daily dependencies (#149) ([e38b8e8](https://github.com/pederbe/fastmash/commit/e38b8e8))
- Update daily dependencies (#147) ([c634248](https://github.com/pederbe/fastmash/commit/c634248))
- Animate the landing page with in-view entrances and ambient glows (#145) ([eb48291](https://github.com/pederbe/fastmash/commit/eb48291))
- Bump dompurify from 3.4.16 to 3.4.17 (#139) ([1fc4cdc](https://github.com/pederbe/fastmash/commit/1fc4cdc))
- Size native sort memory to cgroup limits without starving later sorts (#144) ([2e0aac5](https://github.com/pederbe/fastmash/commit/2e0aac5))
- Add focused CSV record copying profiling jobs (#141) ([7e2f849](https://github.com/pederbe/fastmash/commit/7e2f849))
- Update Fedora source package to 0.2.0 (#140) ([4c717d4](https://github.com/pederbe/fastmash/commit/4c717d4))
- Reduce scanning work for long quoted CSV fields (#138) ([7283e3a](https://github.com/pederbe/fastmash/commit/7283e3a))
- Reduce work in CSV conversion and ordered numerical samples (#137) ([ad12be8](https://github.com/pederbe/fastmash/commit/ad12be8))
- Update dependencies and enable instruction-count profiling (#136) ([10df460](https://github.com/pederbe/fastmash/commit/10df460))
- Configure Renovate dependency updates with automerge (#135) ([162c691](https://github.com/pederbe/fastmash/commit/162c691))
- Support current repository metadata compression and RPM digest tags ([9a7a42c](https://github.com/pederbe/fastmash/commit/9a7a42c))
- Preserve archive compatibility and synchronize tool guides ([f145247](https://github.com/pederbe/fastmash/commit/f145247))
- Preserve minimum Rust baseline and latest security updates ([2141700](https://github.com/pederbe/fastmash/commit/2141700))
- Batch dependency ecosystems and verified tool pins with Renovate ([80e2577](https://github.com/pederbe/fastmash/commit/80e2577))
- Scope dependency checks to affected packages and avoid duplicate CI ([971bb43](https://github.com/pederbe/fastmash/commit/971bb43))
- Keep only verified Unicode reference version constraints ([747299b](https://github.com/pederbe/fastmash/commit/747299b))
- Review production collation data updates against locale qualification ([2a09238](https://github.com/pederbe/fastmash/commit/2a09238))
- Allow reviewed package licenses across dependency updates ([2b08a03](https://github.com/pederbe/fastmash/commit/2b08a03))
- Allow current dependency updates with narrow reference data exceptions ([e5e7e86](https://github.com/pederbe/fastmash/commit/e5e7e86))
- Validate normalization data instead of library Unicode version ([86c6348](https://github.com/pederbe/fastmash/commit/86c6348))
- Bump unicode-normalization ([f7e5d2a](https://github.com/pederbe/fastmash/commit/f7e5d2a))
- Use plain wording for dependency pin comments ([5bb0c66](https://github.com/pederbe/fastmash/commit/5bb0c66))
- Auto-merge routine dependency updates while preserving data pins ([22a0fff](https://github.com/pederbe/fastmash/commit/22a0fff))
- Add GitHub Sponsors funding link (#118) ([430c00b](https://github.com/pederbe/fastmash/commit/430c00b))
- Bump the actions-patch group with 2 updates ([f8b9a1a](https://github.com/pederbe/fastmash/commit/f8b9a1a))
- Automate patch dependency updates and complete Dependabot coverage ([50d2dd2](https://github.com/pederbe/fastmash/commit/50d2dd2))
- Simplify the pull request template ([c3f05f4](https://github.com/pederbe/fastmash/commit/c3f05f4))
- Use checked safe native signal-mask queries ([7500203](https://github.com/pederbe/fastmash/commit/7500203))
- Project byte-order records directly into sort chunks ([fb3b3dd](https://github.com/pederbe/fastmash/commit/fb3b3dd))
- Expose native sort and merge stage profiling boundaries ([98857f9](https://github.com/pederbe/fastmash/commit/98857f9))
- Observe the macOS signal mask through a safe API ([a375920](https://github.com/pederbe/fastmash/commit/a375920))
- Use safe terminal and address-space limit queries ([804e6be](https://github.com/pederbe/fastmash/commit/804e6be))
- Replace raw signal test setup with safe native APIs ([d37079f](https://github.com/pederbe/fastmash/commit/d37079f))
- Use safe process setup for external sort tests ([2f80bcb](https://github.com/pederbe/fastmash/commit/2f80bcb))
- Reuse safe helpers in invocation and selection tests (#108) ([fffa3db](https://github.com/pederbe/fastmash/commit/fffa3db))
- Clarify native command test cleanup and retained evidence ([59242ee](https://github.com/pederbe/fastmash/commit/59242ee))
- Retire the unused native fault interposer and probe ([1b4988a](https://github.com/pederbe/fastmash/commit/1b4988a))
- Express interrupted cleanup retry as a while-let loop ([e526a59](https://github.com/pederbe/fastmash/commit/e526a59))
- Keep reaping interrupted native-sort cleanup waits ([2eee166](https://github.com/pederbe/fastmash/commit/2eee166))
- Use safe native-sort fixture ownership and child setup ([b73e4e0](https://github.com/pederbe/fastmash/commit/b73e4e0))
- Recover confirmed busy native image creation teardown (#109) ([e0733b9](https://github.com/pederbe/fastmash/commit/e0733b9))
- Use safe native bindings for diagnostic fault tests ([f6acb89](https://github.com/pederbe/fastmash/commit/f6acb89))
- Restore ordinary child SIGPIPE for descriptor references ([d8eb969](https://github.com/pederbe/fastmash/commit/d8eb969))
- Reuse safe spawn bindings in descriptor failure tests ([f32bb20](https://github.com/pederbe/fastmash/commit/f32bb20))
- Preserve failed native workspace volume receipts ([9e75737](https://github.com/pederbe/fastmash/commit/9e75737))
- Place corpus ownership regression after production items ([8a022cd](https://github.com/pederbe/fastmash/commit/8a022cd))
- Keep unconfirmed full-output state outside general scratch cleanup ([72842da](https://github.com/pederbe/fastmash/commit/72842da))
- Cover partial full-output retention outside general scratch cleanup ([0396c03](https://github.com/pederbe/fastmash/commit/0396c03))
- Retain failed image creation state before establishing the volume owner ([6a04716](https://github.com/pederbe/fastmash/commit/6a04716))
- Cover failed creation without unsupported native cleanup claims ([6cee61b](https://github.com/pederbe/fastmash/commit/6cee61b))
- Wait for native image transition postconditions before cleanup ([6dda407](https://github.com/pederbe/fastmash/commit/6dda407))
- Use owned full output for invocation information failures ([9c7eec8](https://github.com/pederbe/fastmash/commit/9c7eec8))
- Use safe child file-size limits in selection tests ([7a53f4a](https://github.com/pederbe/fastmash/commit/7a53f4a))
- Format retained fixture path ([bc5e0f9](https://github.com/pederbe/fastmash/commit/bc5e0f9))
- Retain combined native fault cleanup receipts ([7e69417](https://github.com/pederbe/fastmash/commit/7e69417))
- Handle exit transitions without releasing owned child groups ([cd9f8f1](https://github.com/pederbe/fastmash/commit/cd9f8f1))
- WIP: move remaining record faults to owned native transports ([b7596f1](https://github.com/pederbe/fastmash/commit/b7596f1))
- Preserve earlier command errors before late-input selection ([35fc77f](https://github.com/pederbe/fastmash/commit/35fc77f))
- Exercise buffered input errors through owned terminal job control ([2ae226d](https://github.com/pederbe/fastmash/commit/2ae226d))
- Share safe ignored-signal setup with CSV resource tests ([019d1be](https://github.com/pederbe/fastmash/commit/019d1be))
- Use safe native fixtures for weighted test faults ([a771e79](https://github.com/pederbe/fastmash/commit/a771e79))
- Format presentation test fixture setup ([4dac030](https://github.com/pederbe/fastmash/commit/4dac030))
- Use owned native output faults in report tests ([31247b7](https://github.com/pederbe/fastmash/commit/31247b7))
- Use owned native output faults in numerical summary tests ([e1476ce](https://github.com/pederbe/fastmash/commit/e1476ce))
- Preserve grouping resource and signal tests through safe child setup ([6a4a7b1](https://github.com/pederbe/fastmash/commit/6a4a7b1))
- Use owned full-output fixtures and safe bounds for quantile tests ([ab243fd](https://github.com/pederbe/fastmash/commit/ab243fd))
- Include prepared-candidate text controls in native qualification ([777dbe7](https://github.com/pederbe/fastmash/commit/777dbe7))
- Use owned native output faults in text-summary tests ([de1426d](https://github.com/pederbe/fastmash/commit/de1426d))
- Use safe native APIs for standard streams and owned descriptor closure ([c0b76af](https://github.com/pederbe/fastmash/commit/c0b76af))
- Use the existing deadline for native image detachment ([890f1c5](https://github.com/pederbe/fastmash/commit/890f1c5))
- Handle owned terminal-only macOS process groups ([38ad9bc](https://github.com/pederbe/fastmash/commit/38ad9bc))
- Qualify anchored corpus child cleanup on native macOS ([0484682](https://github.com/pederbe/fastmash/commit/0484682))
- Retain child ownership until corpus group cleanup completes ([aec0f5f](https://github.com/pederbe/fastmash/commit/aec0f5f))
- Preserve the qualified native executable pair across test builds ([5659b11](https://github.com/pederbe/fastmash/commit/5659b11))
- Use the maintained compiler for native qualification linting ([8183f85](https://github.com/pederbe/fastmash/commit/8183f85))
- Replace unsafe native stream setup with a child-only Rust launcher ([6bc0502](https://github.com/pederbe/fastmash/commit/6bc0502))
- Format the unsupported transport diagnostic ([0984c20](https://github.com/pederbe/fastmash/commit/0984c20))
- Clarify corpus fault calibration and Sort supervisor terminology ([00593dd](https://github.com/pederbe/fastmash/commit/00593dd))
- Use the complete Rust corpus driver in application and installation checks ([1b8798f](https://github.com/pederbe/fastmash/commit/1b8798f))
- Run acknowledged late-input errors in the Rust corpus driver ([aa28d70](https://github.com/pederbe/fastmash/commit/aa28d70))
- Describe held observations in the corpus command help ([7d7c6b3](https://github.com/pederbe/fastmash/commit/7d7c6b3))
- Use raw fixture arguments in the native buffer unit control ([e260023](https://github.com/pederbe/fastmash/commit/e260023))
- Observe held output through native files pipes and terminals ([4f1ba91](https://github.com/pederbe/fastmash/commit/4f1ba91))
- Preserve open stdin until early corpus commands exit ([93bd4f2](https://github.com/pederbe/fastmash/commit/93bd4f2))
- Verify native image ownership through partial detach cleanup ([10514f0](https://github.com/pederbe/fastmash/commit/10514f0))
- Document corpus-owner lifetime controls ([f296b97](https://github.com/pederbe/fastmash/commit/f296b97))
- Leave cleanup time for both full-output lifetime controls ([b1017e0](https://github.com/pederbe/fastmash/commit/b1017e0))
- Use calibrated real full destinations in the Rust corpus driver ([e97794e](https://github.com/pederbe/fastmash/commit/e97794e))
- Clarify corpus coverage and keep negative controls free of expected panics ([c2b4f31](https://github.com/pederbe/fastmash/commit/c2b4f31))
- Run directory and broken-pipe corpus cases through Rust ([f3505b1](https://github.com/pederbe/fastmash/commit/f3505b1))
- Use owned PTY handles for Linux late-input tests ([d4e74e3](https://github.com/pederbe/fastmash/commit/d4e74e3))
- Inspect Spill descriptors through temporary safe file handles ([cd875a9](https://github.com/pederbe/fastmash/commit/cd875a9))
- Qualify real full-stderr and unselected output controls ([4630e50](https://github.com/pederbe/fastmash/commit/4630e50))
- Force-detach only the owned full-output test volume ([5ed4f1a](https://github.com/pederbe/fastmash/commit/5ed4f1a))
- Check Spill file lifetime through safe descriptor metadata ([c299c34](https://github.com/pederbe/fastmash/commit/c299c34))
- Qualify full-volume cleanup through normal and unwind controls ([faa418e](https://github.com/pederbe/fastmash/commit/faa418e))
- Share safe closed-input process support across native validation tests ([61e4852](https://github.com/pederbe/fastmash/commit/61e4852))
- Read macOS output metadata through safe Rustix interfaces ([5e75faa](https://github.com/pederbe/fastmash/commit/5e75faa))
- Normalize native filesystem block widths before checking capacity ([ead2105](https://github.com/pederbe/fastmash/commit/ead2105))
- Probe actual full-output failures using bounded private volumes ([fe8062e](https://github.com/pederbe/fastmash/commit/fe8062e))
- Cancel late-input probes through an established socket and verify reaping ([26596a2](https://github.com/pederbe/fastmash/commit/26596a2))
- Probe actual late input EIO through safe terminal job control ([06c8244](https://github.com/pederbe/fastmash/commit/06c8244))
- Borrow sorting input through the safe Rustix interface ([38233df](https://github.com/pederbe/fastmash/commit/38233df))
- Generate glibc collation artifacts in safe Rust and retire Python ([f96b32e](https://github.com/pederbe/fastmash/commit/f96b32e))
- Prepare safe glibc collation reference probes ([b8896e6](https://github.com/pederbe/fastmash/commit/b8896e6))
- Keep public fixture paths distinct from private captures ([e390e21](https://github.com/pederbe/fastmash/commit/e390e21))
- Generate collation candidates with safe ICU reference tooling ([f0f397e](https://github.com/pederbe/fastmash/commit/f0f397e))
- Run Fedora and Nix recipe checks with pinned Rust helper ([83e45c0](https://github.com/pederbe/fastmash/commit/83e45c0))
- Add shared Rust checks for Linux source recipes ([3da66cc](https://github.com/pederbe/fastmash/commit/3da66cc))
- Keep raw filename fixture coverage on supported filesystems ([56ebaec](https://github.com/pederbe/fastmash/commit/56ebaec))
- Prepare and invoke the pinned Homebrew package checker ([992b10d](https://github.com/pederbe/fastmash/commit/992b10d))
- Add safe Rust checks for the retained Homebrew package ([e8ec726](https://github.com/pederbe/fastmash/commit/e8ec726))
- Make the native archive platform guard explicit ([34d91e7](https://github.com/pederbe/fastmash/commit/34d91e7))
- Verify native archive bytes and deployment metadata in Rust ([5785b89](https://github.com/pederbe/fastmash/commit/5785b89))
- Move retained source-package smoke checks to Rust ([1486845](https://github.com/pederbe/fastmash/commit/1486845))
- Verify packaged executable bytes with Rust ([ab165a0](https://github.com/pederbe/fastmash/commit/ab165a0))
- Move publication fixture repair checks into Rust ([6149505](https://github.com/pederbe/fastmash/commit/6149505))
- Move installed verification into Rust ([766e6c1](https://github.com/pederbe/fastmash/commit/766e6c1))
- Run package publication checks from Rust ([b00971d](https://github.com/pederbe/fastmash/commit/b00971d))
- Run collation verification from Rust ([a157bd5](https://github.com/pederbe/fastmash/commit/a157bd5))
- Preserve verification console output in Rust ([7fa1034](https://github.com/pederbe/fastmash/commit/7fa1034))
- Add Unicode 15 collation word planning ([ca1982f](https://github.com/pederbe/fastmash/commit/ca1982f))
- Fix native punctuation test paths ([b1abd05](https://github.com/pederbe/fastmash/commit/b1abd05))
- Add fixed collation punctuation comparison command ([6fee831](https://github.com/pederbe/fastmash/commit/6fee831))
- Support larger locale comparison pipe inputs ([5e6d611](https://github.com/pederbe/fastmash/commit/5e6d611))
- Retire superseded numeric Python tools ([1d87ae4](https://github.com/pederbe/fastmash/commit/1d87ae4))
- Use safe parent signal delivery in native tests ([bb94567](https://github.com/pederbe/fastmash/commit/bb94567))
- Use owned terminal descriptors in native tests ([4ab599e](https://github.com/pederbe/fastmash/commit/4ab599e))
- Use safe spawn actions for native descriptor tests ([58ef743](https://github.com/pederbe/fastmash/commit/58ef743))
- Use safe APIs for descriptor cloning and flags ([34c24b6](https://github.com/pederbe/fastmash/commit/34c24b6))
- Use Rust for local publication storage ([bc2bfb2](https://github.com/pederbe/fastmash/commit/bc2bfb2))
- Use safe native entropy interfaces ([a9f4200](https://github.com/pederbe/fastmash/commit/a9f4200))
- Add pure collation verification report updates ([7bbb52d](https://github.com/pederbe/fastmash/commit/7bbb52d))
- Add Rust punctuation comparison reports ([a6d7638](https://github.com/pederbe/fastmash/commit/a6d7638))
- Add fixed collation word plans and order comparison ([0c18b5f](https://github.com/pederbe/fastmash/commit/0c18b5f))
- Add Rust locale comparison and collation table commands ([82b0ee9](https://github.com/pederbe/fastmash/commit/82b0ee9))
- Keep invalid-byte directory fixture on Linux ([5d466d3](https://github.com/pederbe/fastmash/commit/5d466d3))
- Document additive Rust locale development tools ([ff671a5](https://github.com/pederbe/fastmash/commit/ff671a5))
- Plan numeric locale comparisons in Rust ([dfd1bb1](https://github.com/pederbe/fastmash/commit/dfd1bb1))
- Add native numeric locale generation command ([adfd86b](https://github.com/pederbe/fastmash/commit/adfd86b))
- Prepare deterministic Rust numeric locale generation ([cf7053e](https://github.com/pederbe/fastmash/commit/cf7053e))
- Retain safe cache hints and generate dependency notices in Rust ([6819e75](https://github.com/pederbe/fastmash/commit/6819e75))
- Use safe Rust for sorted record and spill buffer access ([444e521](https://github.com/pederbe/fastmash/commit/444e521))
- Preserve surrogateescaped bytes in frozen corpus arguments ([1349719](https://github.com/pederbe/fastmash/commit/1349719))
- Use the published testing guide link in architecture docs ([2b4d32a](https://github.com/pederbe/fastmash/commit/2b4d32a))
- Reject inherited Linux real-time signal masks ([65d0fce](https://github.com/pederbe/fastmash/commit/65d0fce))
- Add a safe Rust driver for ordinary regression commands ([82d81fe](https://github.com/pederbe/fastmash/commit/82d81fe))
- Use checked safe Rust for numerical storage and division ([715cb89](https://github.com/pederbe/fastmash/commit/715cb89))
- Use an exact decimal expectation for the longer capture ([9bdf08f](https://github.com/pederbe/fastmash/commit/9bdf08f))
- Verify CPU attribution on the disposable Linux runner ([cb180b1](https://github.com/pederbe/fastmash/commit/cb180b1))
- Identify Spill by its instrumented module path ([efb360a](https://github.com/pederbe/fastmash/commit/efb360a))
- Write the profiling binary digest as hex bytes ([081fe4a](https://github.com/pederbe/fastmash/commit/081fe4a))
- Use a stable invocation name in profiling jobs ([bd9129b](https://github.com/pederbe/fastmash/commit/bd9129b))
- Allow the CPU report decoder license ([fde0f5f](https://github.com/pederbe/fastmash/commit/fde0f5f))
- Preserve failed profiling diagnostic status ([b3aa4df](https://github.com/pederbe/fastmash/commit/b3aa4df))
- Add opt-in static Command profiling ([b35ea20](https://github.com/pederbe/fastmash/commit/b35ea20))
- Remove redundant license prose from the README ([e56afff](https://github.com/pederbe/fastmash/commit/e56afff))
- Make the README a concise product overview ([1c9b9ee](https://github.com/pederbe/fastmash/commit/1c9b9ee))
- Document the signed apt and dnf repository ([2115807](https://github.com/pederbe/fastmash/commit/2115807))
- Keep package repository metadata consistent during publication ([93f3121](https://github.com/pederbe/fastmash/commit/93f3121))
- Prepare package repository signing and setup ([cd5abd1](https://github.com/pederbe/fastmash/commit/cd5abd1))
- Trim installation docs and scope documentation checks ([f3bc01e](https://github.com/pederbe/fastmash/commit/f3bc01e))
- Isolate broken-pipe test fixture after fork ([79efdab](https://github.com/pederbe/fastmash/commit/79efdab))
- Update installation guidance for Fastmash 0.2.0 ([e3310fc](https://github.com/pederbe/fastmash/commit/e3310fc))
- Publish a signed apt and dnf repository ([1289d36](https://github.com/pederbe/fastmash/commit/1289d36))

[Full comparison](https://github.com/pederbe/fastmash/compare/v0.2.0...v0.3.0)

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
