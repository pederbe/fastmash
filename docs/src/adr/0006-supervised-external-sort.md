# 0006: Supervise the system `sort` for the remaining sort routes

Status: accepted.

Fastmash sorts in process, with disk spill, for the common operations and
locales. For the remaining combinations it uses the system `sort`, as GNU
datamash does, but never directly: a small supervisor process starts `sort`
with a fixed argument list in a private temporary directory, watches the main
process through a Linux pidfd, and cleans up even if the main process is killed.

**Consequences:** Fastmash ships two executables. The external route is used
only where it works: Linux 5.11 or later, a GNU or uutils coreutils `sort`,
the supervisor installed beside `fastmash`, and the other conditions in
[System requirements](../requirements.md). Elsewhere those jobs sort in
process, with the same output. The in-process sorter is expected to take over
the remaining routes over time.
