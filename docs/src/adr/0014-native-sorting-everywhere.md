# 0014: Native sorting in one executable

Status: accepted. Supersedes [ADR 0006: Supervised external sort](0006-supervised-external-sort.md).

Use built-in stable sorting for every calculation and sorted duplicate removal
on Linux and macOS. It preserves complete records, contribution order, seeded
randomness and checked spill storage.

Keeping an external route would retain a second executable, a process protocol
and platform-specific failure paths for work the native sorter already covers.
One implementation keeps those behaviors together and removes external-sort
installation requirements.

The numerical profile is unchanged. Input and storage failures use Fastmash's
diagnostics and existing exit statuses. The
[architecture](../contributing/architecture.md#sorting) describes the native
sorter. Upgrade handling belongs in the
[installation guide](../install.md#older-linux-installations).
