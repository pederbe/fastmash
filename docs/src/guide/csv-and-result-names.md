# Quoted CSV and custom result names

Fastmash supports quoted CSV input and output, and optional names for calculated
output columns. CSV is selected explicitly; ordinary delimited text stays the default.

| Workflow | CSV input/output | Result names |
| --- | --- | --- |
| Aggregate and per-row reports | Yes, including supported grouping and full copied fields | Calculated output columns with an explicit output header |
| [Weighted mean](weighted-mean.md) | Yes, including adjacent and sorted groups | One name per value/weight result |
| [Top-N selection](top-n-records.md) | Yes, including adjacent and sorted groups | No: selection copies the source fields |
| [Dataset comparison](dataset-comparison.md) | Yes; both sources use the same input format | One name throughout each summary's before/after/change columns |
| Crosstab, field modes and legacy table modes | No | No |
| [Table health](table-health.md) | No; ordinary text input and readable or TSV output | No |

## Custom result names

Give calculated output columns stable names in your reports. Select Output headers with
`--header-out` or `-H`, then repeat `--result-name=INDEX:NAME` for the labels you
want to replace. The separate form `--result-name INDEX:NAME` works too.

For example, a report can give its sum a stable schema name while retaining the
generated label for its mean:

```sh
printf 'reading\n2\n4\n' | LC_ALL=C fastmash -H --result-name=1:total sum reading mean reading
```

```text
total	mean(reading)
6	3
```

Indices follow Operation-produced output order after Selector expansion, starting
at 1. A list or range contributes each expanded result, repeated Operations
contribute their results again, and a paired Operation contributes one result.
Each field selected by `cut` also contributes a result. Grouping keys and the
copied fields from `--full` do not consume indices or receive replacement labels.

In this example, `sum 2,1-2` produces three results in the order 2, 1, 2. Names
can be supplied out of order, and unnamed results keep their generated labels:

```sh
printf '1\t2\n3\t4\n' | LC_ALL=C fastmash --header-out --result-name=3:again --result-name=1:total sum 2,1-2
```

```text
total	sum(field-1)	again
6	4	6
```

Names replace labels only. They do not change Selector lookup, required-field
checks, calculation values, output order, or when headers appear. Empty input
still produces no invented header; with `-H`, header-only input retains its
normal Output header without a result row. Later input or output failures can
leave partial output, as with ordinary generated headers.

`INDEX` must be an unsigned, nonzero decimal integer within the expanded result
count. Repeating a target, omitting a name, or supplying an empty name is an error.
Duplicate name values are allowed. Split at the first colon, so a name can itself
contain colons. Name bytes are preserved as supplied by the operating system.
For text output, names cannot contain the effective Output delimiter or Record
terminator. CSV output allows comma, quote, colon, CR and LF bytes in names and
encodes them as one field. Invalid names fail before any output.

Result names work for ordinary aggregate and Per-row operations, including
adjacent and sorted groups. Crosstab, table modes (`transpose`, `check`, `rmdup`)
and field modes (`reverse`, `noop`) refuse naming before reading input. Only the
exact `--result-name` spelling is accepted; existing GNU option abbreviations
retain their normal meanings. `--vnlog` alone does not satisfy the requirement
for explicit Output headers; add `--header-out` or `-H`.

GNU datamash 1.9 generates labels such as `sum(reading)`. Stable custom report
labels otherwise need a wrapper or a downstream renaming step. Naming inside
the Command avoids that extra step and follows the existing header timing.
This is a convenience for report schemas, with no claim of faster computation.

## CSV output from delimited text

Use `--csv-out` to produce quoted CSV from ordinary delimited-text input.
It works with aggregate and Per-row operations, including adjacent or sorted
groups, explicit headers and copied `--full` fields. Repeating the switch is
idempotent. It does not enable either Input or Output headers.
[Top-N selection](top-n-records.md) also supports CSV output from complete
selected text Records, including sorted Groups; it adds no calculation columns.

For example, tab-separated observations can become a CSV report with a stable
result label, including keys and names containing commas or quotation marks:

```sh
printf 'place\treading\nlab,west\t2\nlab,west\t4\n' | LC_ALL=C fastmash --csv-out -H -g place --result-name='1:total,"daily"' sum reading
```

```csv
GroupBy(place),"total,""daily"""
"lab,west",6
```

Every logical output field is encoded separately: complete generated labels,
custom names, Grouping keys, formatted results and each copied `--full` field.
Commas separate fields and LF ends each emitted Record. A field containing a
comma, double quote, CR or LF is quoted, with internal double quotes doubled.
An empty field prints as `""`. Embedded line breaks retain their bytes. Output
is canonical; it does not retain an input field's quoting style or line ending.

Decimal-comma formatted numbers and `collapse` or `unique` lists stay within one
quoted result field. The Collapse delimiter is still the list's internal
separator; CSV adds no escaping scheme inside those lists. Encoding preserves
the result bytes, including non-UTF-8 and NUL bytes, under the existing Operation
and header display rules. For example, raw keys and copied fields retain NUL,
while selected text results and generated field-name displays stop at NUL as
they do in ordinary output. A leading UTF-8 BOM remains field data.

Output-only CSV allows text-input `-t`, `-W` and `-C`. An explicit
`--output-delimiter`, `-z` or `--vnlog` conflicts with CSV output and fails with
status 1, regardless of option order or a redundant delimiter value. Crosstab,
legacy Table modes and Field modes refuse CSV with status 77 before input.
Health reports remain text-only and reject CSV switches using their status-1
calculation-control diagnostic. Only the exact CSV switch spellings are accepted;
existing GNU option abbreviations retain their meanings.

CSV output retains empty-input and header-only timing and the normal failure
policy. A later input, calculation or transport failure can leave partial output.

## Calculations on quoted CSV

Use `--csv-in` for strict quoted CSV input with ordinary text output, or `--csv`
for CSV input and output together. `--csv` is equivalent to `--csv-in --csv-out`.
Repeating these exact-only switches is idempotent; none enables headers.
[Top-N selection](top-n-records.md) supports decoded CSV input for whole-dataset,
adjacent-Group and sorted-Group selection with either output format. Sorted
selection retains complete decoded Records and original-input ties through
bounded Spill. Its copied Output header uses the original source header or
first accepted source Record, independently of sorted encounter order.
Its ranking Field uses the decoded field-local numerical
contract, and copied data preserves complete bytes.
Aggregate and Per-row calculations support their existing positional
lists, numeric ranges, escaped named Selectors, paired Selectors and parameters.
Aggregate grouping and `--full` work with decoded CSV fields. Add `-s` to bring
equal decoded Grouping keys together, using bounded Spill when the input exceeds
the maintained sort-memory target. Per-row operations retain their ordinary conflict with
Grouping keys. With no Grouping keys, `-s` keeps the streaming workflow and source
order, including Per-row and `--full` reports. Output-only CSV retains sorted
text-input workflows.

For example, a quoted export can be summed directly, without a quote-aware
preprocessing step, while preserving a comma in its decoded column name:

```sh
printf '"amount,raw",note\r\n"2","two\nlines"\n4,"a""b"' | LC_ALL=C fastmash --csv -H --result-name='1:total,"daily"' sum 'amount\,raw'
```

```csv
"total,""daily"""
6
```

Grouping keys compare decoded values, so `west` and `"west"` belong to the same
adjacent Group. Repeated keys in a later run stay separate without sorting:

```sh
printf '"region,name",value\n"west,coast",2\n"west,coast",4\neast,3\n"west,coast",5\n' | LC_ALL=C fastmash --csv -H -g 'region\,name' --result-name=1:total sum value
```

```csv
"GroupBy(region,name)",total
"west,coast",6
east,3
"west,coast",5
```

Multiple and named keys retain their normal request order, exact name lookup and
first-duplicate-name rules. Adjacent equality is byte-based, with ASCII folding
under `-i` where the selected locale supports it. Equal-length keys compare up
to NUL, while displayed keys retain their complete byte spans. Locale-aware
numeric input and output keep their existing rules; a language locale does not
silently merge differently spelled adjacent keys.

Sorting the same export merges the separate `west,coast` runs:

```sh
printf '"region,name",value\n"west,coast",2\n"west,coast",4\neast,3\n"west,coast",5\n' | LC_ALL=C fastmash --csv -H -s -g 'region\,name' --result-name=1:total sum value
```

```csv
"GroupBy(region,name)",total
east,3
"west,coast",11
```

Sorting compares complete decoded keys with the existing locale and case policy,
then preserves source order for tied keys. Group equality remains distinct from
locale ordering: differently spelled keys which the locale ties can still form
separate adjacent Groups. Headers bind before sorting; without an Input header,
a generated Output header takes its field width from the first sorted Record.
Copied Records keep their actual widths and original logical Record/physical-line
locations through reordering and representative selection.

CSV sorting uses the same sort-memory policy as ordinary text, including
`FASTMASH_SORT_MEMORY_BYTES`, and spills sorted runs to temporary files when
needed. Complete decoded fields and their original input locations survive sorting.
The memory target covers sort chunks, not the whole process; an oversized Record,
merge heads and retained statistical or text samples have their own storage needs.
Samples are not spilled. Temporary I/O errors fail with status 1, and checked
allocation failures refuse with status 77. Later failures may leave earlier output.

Full reports copy each decoded source field before the calculation results.
For example, a Per-row report can retain a comma in a source field while
selecting its numeric field with a custom result label:

```sh
printf 'place,value\n"west,coast",2\neast,4\n' | LC_ALL=C fastmash --csv -H --full --result-name=1:reading cut value
```

```csv
place,value,reading
"west,coast",2,2
east,4,4
```

For an aggregate, the representative starts as the Group's first Record;
`first`, `last`, `rand` and improving extrema may select another Record under
their existing rules. Extrema ties retain the earlier selection. With multiple
Operations the representative need not supply every result. Aggregate `--full`
retains its existing GNU compatibility warning; Per-row `--full` does not warn.
Copied-field labels replace GroupBy labels, and Result-name indices still count
only Operation-produced results. Without sorting, header width comes from the
first logical Record; each copied data Record keeps its actual width, without padding or
filler. Required fields must still exist, including with `--no-strict` or
`--filler`. Copied fields preserve complete bytes, including NUL, independently
of an Operation's display rules.

A double quote opens quoting only at the start of a field. Inside quotes,
commas, CR and LF are data, and two double quotes decode to one. After a closing
quote, only comma, LF, CRLF or end of input is allowed. Quotes within unquoted
fields, stray bytes after closing quotes, unfinished quoted fields and bare CR
outside quotes are errors, including in unused fields. Spaces are data, with no
implicit trimming. LF and CRLF endings may be mixed; a complete final Record
may omit its ending.

An empty file has no Records. Each blank LF or CRLF Record has one empty field;
quoted and unquoted empty fields decode identically. A trailing comma supplies
a final empty field, while a terminal Record ending supplies no extra Record.
Different widths are permitted when all requested fields exist. An absent field
still errors; an existing empty field is valid text but invalid numeric input.
`--narm` retains the existing exact, case-insensitive NA/N/A/NaN matching and
independent paired-side compaction. It does not skip empty fields or trim spaces.

Decoding preserves bytes, including tabs, NUL, non-UTF-8 and internal CRLF.
Operations keep their existing display and transformation rules: `cut` stops
display at NUL, while base64 and checksums consume the complete decoded field.
The active numerical locale and presentation settings apply to each isolated
decoded numeric field. An unused numeric neighbor cannot affect conversion.
Ordinary text numerical conversion keeps its existing behavior.

A leading UTF-8 BOM is data. Before an unquoted Input header it becomes part of
the name and generated label, following the normal byte-escaped Selector rules.
Before a quoted first field it is data followed by an illegal quote, so that
export fails. There is no BOM stripping or transcoding. This dialect has stated
byte and blank-Record extensions; it is not a claim of full RFC 4180 conformance.

With `--csv-in`, text results use the normal tab default and accept an explicit
`--output-delimiter`. Decoded delimiters or line breaks in result fields may make
that text report ambiguous; use `--csv` when field boundaries must survive output.
CSV input conflicts with explicit `-t`, `-W` and `-C`, including their GNU
abbreviations and redundant comma delimiters. Either CSV direction conflicts
with `-z` and `--vnlog`, in either option order, with Error status 1. Legacy modes
refuse CSV before input; Health rejects all CSV switches and remains text-only.

Unsorted input is read incrementally, with checked growth for decoded Record and
Field storage. Adjacent Grouping and Per-row execution retain owned decoded Records without
losing field boundaries or original locations when representative and spare
storage swap. Statistical sample retention keeps
its existing memory policy; parsing does not make samples spillable.

CSV validation errors identify the original logical Record number, counting an
Input header, and its starting physical line. Syntax errors also identify the
detected physical line and byte position, counting raw bytes including quotes;
unfinished quotes identify unexpected end of input. LF inside quotes advances
the physical line; CRLF counts as one line break. Header lookup errors include
the header's location when it exists. I/O failures keep their I/O context without
inventing a Record or turning a read failure into a quoted-field EOF error.
Malformed CSV uses status 1, checked Capacity refusals use 77, and output
finalization retains normal precedence. Later failures can leave partial output.
