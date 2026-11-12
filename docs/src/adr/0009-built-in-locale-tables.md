# 0009: Built-in locale tables

Status: accepted.

Bundle numeric locale rules and verified text-ordering rules instead of reading
the host's installed locale data. This gives a bare container and a workstation
the same behavior, supporting [portable results](0003-portable-results.md).

Reading host locales would match each host's GNU behavior, including fallback
to `C` for missing locales. It would also make results depend on installed data
and require a second collation path. Fastmash instead uses its bundled rules
for known names, falls back to `C` for unknown names, and refuses unsupported
sorting or numeric separators.

The numeric rules follow glibc 2.43. Supported sorting locales combine Unicode
collation with verified glibc ordering rules. Differences from a host's locale
data and remaining collation differences are
[documented](../guide/differences.md#features).

Adding a locale requires extending and verifying the bundled data. Regeneration
and verification commands belong in the
[testing guide](../../../TESTING.md#locale-tools).
