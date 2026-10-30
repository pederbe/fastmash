# 0014: Native sorting in one executable

Status: accepted. Supersedes [ADR 0006: Supervise the system `sort` for the remaining sort routes](0006-supervised-external-sort.md).

Use built-in stable sorting for every calculation and sorted duplicate removal
on Linux and macOS. The native sorter already preserves complete records,
contribution order, seeded randomness and checked Spill storage. Keeping a second
sort implementation would retain an executable, a process protocol and
platform-specific failure paths for the same work.

Fastmash builds and installs as one executable. Keep the native temporary-file,
entropy, signal, memory and thread helpers still needed by sorting and other
commands. Remove the system-sort route, supervisor and their startup requirements.
The Numerical profile is unchanged. Input and storage failures use Fastmash's
own diagnostics and existing exit statuses.

Upgrades remove a stale supervisor only when it belongs to the installation
being replaced. Published releases, checksums and historical qualification evidence
remain unchanged. Release publication is separate from this development change.
