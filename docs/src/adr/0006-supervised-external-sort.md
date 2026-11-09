# 0006: Supervise the system `sort` for the remaining sort routes

Status: superseded by [ADR 0014: Native sorting in one executable](0014-native-sorting-everywhere.md).

Earlier releases used the system `sort` for combinations that the built-in
sorter did not yet cover. A separate Linux supervisor started it with fixed
arguments and cleaned up temporary files even when the main process was killed.

This reused an established sorter while preserving cleanup, but required two
executables, Linux process supervision and compatible external tools. Native
sorting replaced that route in 0.4.0. See [current requirements](../requirements.md).
