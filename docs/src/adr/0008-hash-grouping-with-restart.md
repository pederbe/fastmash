# 0008: Hash grouping with restart

Status: accepted. The external-sort fallback was superseded by
[ADR 0014](0014-native-sorting-everywhere.md).

For eligible `-s` jobs with repeated keys, collect each key's operation state
and sort only the groups. Preserve the contribution order and results of a
stable sort of all records. Where that cannot be established, replay the input
through the native sorter before writing any result.

Replay preserves sorted behavior for ambiguous keys, collation ties and errors.
It also handles jobs whose group count or memory use makes hashing unsuitable.
Regular files can be reread. Eligible language-locale pipes retain input for
replay because their sorter would also retain complete records. C-locale pipes
normally sort directly to avoid that additional storage.

**Alternatives:** serializing partially accumulated groups into sort records
would avoid rereading, but would require a serialization contract for every
operation and reproduce sorted error order from stored state. Retaining every
pipe adds memory cost in C locales. Spilling retained input to a temporary file
does not reduce memory use when that filesystem is memory-backed.

**Consequences:** a late restart repeats work. A regular file that changes
between readings produces the result of the second reading. File and pipe
routes can use different resources while preserving the same data contract.
The [architecture](../contributing/architecture.md#sorting) describes eligibility
and replay. The [grouping guide](../guide/grouping.md#large-inputs) describes
resource use and controls.
