# 0006: Supervise the system `sort` for the remaining sort routes

Status: accepted.

Fastmash sorts in process, with disk spill, for the common operations and
locales. For the remaining combinations it uses the system `sort`, as GNU
datamash does, but never directly: a small supervisor process starts `sort`
with a fixed argument list in a private temporary directory, watches the main
process through a Linux pidfd, and cleans up even if the main process is killed.

**Consequences:** the external route needs Linux 5.11 or later and a GNU
coreutils `sort`, and Fastmash ships two executables that must be installed
together. The in-process sorter is expected to take over the remaining routes
over time.
