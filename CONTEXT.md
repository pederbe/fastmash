# Fastmash

Fastmash is a fast Rust command-line tool for Linux that computes statistics and
table transformations over delimited text. It implements GNU datamash's command
language so that datamash workflows can move to it with few or no changes.

Use **Fastmash** for the product in prose and display text, and `fastmash`
for commands and identifiers, including package and repository names.

This glossary covers current behavior and selected post-release concepts.
The user guide describes implemented capabilities.

## Command language

**Command**:
One complete `fastmash` invocation: its options, an optional mode or grouping,
and its operations.
_Avoid_: Using "command" for a single operation

**Operation**:
One requested calculation or transformation, named with an optional `:parameter`
and applied to one or more fields, such as `sum 2` or `perc:90 3`.
_Avoid_: Op, function, command

**Operation parameter**:
The optional value after an operation's colon that adjusts it, such as the 90
in `perc:90`.

**Operation family**:
A set of related operations that are documented, implemented or measured
together, such as the quantiles or the paired statistics.
_Avoid_: "Family" on its own (see **Job family**)

**Mode**:
The overall way a command processes its input: aggregating (optionally in
groups), Dataset comparison, crosstab, per-row operations, Top-N selection, or one of the field modes
and table modes.
_Avoid_: Using "mode" for the `mode` statistic

**Per-row operation**:
An operation that produces one output value for each input record rather than
one summary, such as `round`, `base64`, `getnum` or `cut` (which prints the
selected fields).
_Avoid_: Linewise operation, line mode, per-line operation

**Field mode**:
A mode that rearranges or passes through the fields of each record: `reverse`
and `noop`. `cut` is a per-row operation, as in GNU datamash, not a field mode.

**Table mode**:
A mode that treats the input as a whole table: `check`, `transpose`, `rmdup`
and `health`, which produces a Table health report.

**Crosstab**:
A pivot table of one calculation over every pair of values of two grouping keys.
_Avoid_: Cross-tabulation

## Input

**Record**:
One logical input unit composed of fields. In ordinary delimited text, it ends
at a newline or, with `-z`, a NUL byte; quoted CSV permits line breaks within fields.
_Avoid_: Row (except for per-row operations), line

**Field**:
One value within a record, addressed by its 1-based position or, with an input
header, by its name.
_Avoid_: Column (except when talking about headers and tables)

**Field separator**:
In ordinary delimited text, the single byte, or run of whitespace with `-W`,
that splits an input record into fields. Tab by default.
_Avoid_: Input delimiter

**Quoted CSV**:
An explicitly selected input or output format with comma-separated fields,
where double quoting permits commas, quotes and line breaks within a field.

**Output delimiter**:
The byte written between fields of an ordinary delimited-text output record,
following the Field separator unless set explicitly. CSV output uses commas
between individually encoded fields.

**Collapse delimiter**:
The byte written between the values listed by `unique` and `collapse`. Comma by
default.

**Selector**:
The field argument within a Command, such as an Operation's input or a Top-N
ranking field. Operations accept a single field, a list or range of fields,
or a `LEFT:RIGHT` field pair for paired operations; selection accepts one field.
_Avoid_: Field spec

**Binding**:
The assignment of a Selector's Fields and requested keys to positions in a
dataset. Names use that dataset's Input header; positional Fields keep their
requested positions.

**Input header**:
A first record that names the fields instead of carrying data (`--header-in`, `-H`).

**Output header**:
A first output record that labels calculation results, such as `sum(reading)`,
or copied fields in Top-N selection (`--header-out`, `-H`).

**Result name**:
A supplied label for an Operation-produced output column, including a selected
field from `cut`. It replaces the generated label, excluding Grouping keys and
the copied-field prefix from `--full`. In a Dataset comparison, it names one
summary result throughout that result's before, after and change columns.

**Full row**:
A complete input record's fields copied before calculation results with `--full`,
including Per-row results. CSV copies decoded field values, not their original
quote spelling.

**Filler**:
The text printed for a missing cell in crosstab and ragged `transpose` output
(`--filler`, `N/A` by default).

**Missing value**:
A field whose exact text is `NA`, `N/A` or `NaN`, in any letter case, which
`--narm` skips. An empty field is not a missing value.
_Avoid_: Null, blank

## Grouping and sorting

**Grouping key**:
The field or fields whose equal values define a group (`-g`).
_Avoid_: Group field, sort key

**Group**:
A run of consecutive records with equal grouping keys. Records with equal keys
form one group only when they are adjacent, or when `-s` sorts them together first.

**Top-N selection**:
Selecting at most N records by the highest or lowest values of one numeric
ranking field, across a dataset or within each Group. Complete records appear
in rank order, with earlier original input records winning ties.

**Sort route**:
The way Fastmash brings equal Grouping keys together for `-s`, by sorting in
memory with Spill as needed or, for ordinary delimited text, by another eligible
route such as Hash grouping or the system `sort`.

**Hash grouping**:
A Sort route for eligible ordinary delimited-text input whose Grouping keys
repeat: each Group's records are collected as they arrive, and only the Groups
are sorted. Where that cannot give the sorted result exactly, Fastmash reads
the input again and sorts it, retaining piped input for that replay.
_Avoid_: Hash mode, hash aggregation

**Spill**:
Temporarily writing sorted runs to disk so that sorting large input does not
need to hold all of it in memory. Only sorting spills; retained samples and
tables stay in memory.

**Sort supervisor**:
The helper executable, `fastmash-sort-supervisor`, installed beside `fastmash`,
that runs the system `sort` for the external sort route and cleans up after it.
_Avoid_: Companion, sorter process

## Results

**Table health report**:
A report of identified or suspected data-quality issues in a dataset, assessed
against expectations for its fields and records.

**Health expectation**:
A requirement for a field or record, supplied by the user or inferred from
patterns in the data. Supplied requirements take precedence over inference.

**Health finding**:
A reported observation, suspected inconsistency with an inferred expectation,
or violation of a supplied expectation.

**Health validation**:
Assessment of a dataset against supplied health expectations, with violations
treated as validation failures. Inferred findings alone are advisory.

**Weighted mean**:
An average in which each value contributes according to an associated weight:
the sum of weighted values divided by the sum of their weights.

**Contribution weight**:
A finite nonnegative number describing how much an observation contributes to
a weighted mean. Zero contributes nothing; a valid mean needs positive total weight.

**Dataset comparison**:
A report of differences between statistical summaries of two datasets.

**Comparison key**:
An ordered tuple of complete Field byte strings, decoded for Quoted CSV,
identifying corresponding summaries in a Dataset comparison. Each distinct key
has at most one summary per dataset, independent of adjacency or quote spelling;
its Records retain their encounter order.

**Comparison ranking**:
Ordering matched Comparison keys by the magnitude of one selected result's
available difference or percentage. A limit caps ranked keys; every one-sided
or unavailable key remains visible after them.

**Portable results**:
The product property that one Fastmash version gives identical results
(standard output and exit status) for the same input and explicit settings on
every supported platform, independent of the host's CPU arithmetic, math library
or installed locale data. Diagnostics produced by the host's own `sort` are
outside it.

**Numerical profile**:
A named, versioned set of rules for how Fastmash reads, computes and prints
numbers, such as `portable-binary80-v3`. It implements **Portable results**.
_Avoid_: "Profile" on its own

**Supported locale**:
A locale whose number and text-ordering rules are built into Fastmash rather than
read from the host. Other locales are refused when they would change a result.

**Default output format**:
How numbers print when neither `--format` nor `--round` is given: up to 14
significant digits.
_Avoid_: Default14 (the code identifier) in user-facing text

**Refusal**:
Fastmash deliberately stopping instead of producing a result it cannot stand
behind, with a diagnostic and exit status 77: for an unsupported feature, locale
or condition, or a checked resource limit. Earlier output may already have been
written.
_Avoid_: Crash, "unsupported" as the general term

**Capacity refusal**:
A refusal because a checked resource limit, such as memory or numerical
precision, would be exceeded.

**Error**:
A failure caused by the command or its input, such as a malformed selector,
non-numeric data or a full disk, with exit status 1.
_Avoid_: Refusal

**Internal failure**:
A detected violation of Fastmash's own invariants, never expected in normal use.
Numerical invariant and table-identity failures exit with status 70; other
internal failures exit with status 77 or abort the process. When one failure
follows another, the status is the more serious: 70 over 77 over 1. A diagnostic
that cannot be written makes the status 1.

## Compatibility

**GNU compatibility**:
Accepting GNU datamash 1.9's options, operations and selectors and producing the
same observable results as its reference profiles, except for **Documented
differences**. Different GNU builds can disagree with each other; they do not
override **Portable results**.

**Documented difference**:
A specific, published behavior where Fastmash intentionally differs from GNU
datamash, such as a refusal where GNU prints an unreliable value.
_Avoid_: Deviation, mismatch (for intended differences)

**Improvement**:
A measured or demonstrated advantage of Fastmash over GNU datamash, such as
speed, a useful behavior or safety, for a stated workload or input.

**Reference profile**:
A named GNU datamash build and host environment used to observe GNU's behavior
for comparison.
_Avoid_: "Profile" on its own

## Performance evaluation

**Representative job**:
A concrete dataset and command pair that stands for a real workload and is used
to compare Fastmash with GNU datamash and with earlier Fastmash builds.
_Avoid_: Benchmark case, test job

**Job family**:
A named group of representative jobs that exercise the same kind of work, such as
startup, decimal accumulation or grouped text.
_Avoid_: "Family" on its own, workload

**Candidate**:
A specific Fastmash build, identified by its exact source and binary, that is
being evaluated.

**Host profile**:
The exact hardware, operating system and toolchain identity of a machine whose
measurements count as evidence.
_Avoid_: "Profile" on its own

**Predecessor guard**:
A check that a candidate is not materially slower or more resource-hungry than
the previous accepted Fastmash build on a given representative job.
_Avoid_: Regression guard

**Qualification**:
The evidence process that decides whether a candidate meets a stated acceptance
criterion on a named host profile.

**Admission**:
The check that a specific binary and host match an expected identity before
their results count as evidence.
_Avoid_: Using it for the broader **Qualification**

## Releases

**Preview release**:
A public, installable Fastmash release before the replacement release, with its
supported operations, platforms and known differences stated.
_Avoid_: Installable preview, development candidate

**Replacement release**:
A release intended to replace GNU datamash in normal workflows, with broad
compatibility, a performance advantage and a short explicit list of differences.
