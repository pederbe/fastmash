# Quoted CSV and custom result names

Use `--csv` to read and write quoted CSV. Add `-H` when the first record names
the fields. For example, summarize revenue by region:

```console
$ printf 'region,revenue\nwest,2\neast,4\nwest,6\n' | fastmash --csv -H -s -g region sum revenue
GroupBy(region),sum(revenue)
east,4
west,8
```

`--csv-in` selects only CSV input, and `--csv-out` selects only CSV output.
Ordinary delimited text remains the default. Use
[custom result names](#custom-result-names) to give calculated columns stable
labels, such as `total` instead of `sum(revenue)`.

## Supported workflows

| Workflow | CSV input/output | Result names |
| --- | --- | --- |
| Aggregate and per-row reports | Yes, including supported grouping and full copied fields | Calculated output columns with an explicit output header |
| [Weighted mean](weighted-mean.md) | Yes, including adjacent and sorted groups | One name per value/weight result |
| [Top-N selection](top-n-records.md) | Yes, including adjacent and sorted groups | No: selection copies the source fields |
| [Dataset comparison](dataset-comparison.md) | Yes. Both sources use the same input format | One name throughout each summary's before/after/change columns |
| Crosstab, field modes and legacy table modes | No | No |
| [Table health](table-health.md) | No. Ordinary text input and readable or TSV output | No |

## Calculations on quoted CSV

Use `--csv-in` for strict quoted CSV input with ordinary text output, or `--csv`
for CSV input and output together. `--csv` is equivalent to `--csv-in --csv-out`.
Repeating these exact-only switches is idempotent. None enables headers.
[Top-N selection](top-n-records.md) supports decoded CSV input for whole-dataset,
adjacent-group and sorted-group selection with either output format. Sorted
selection retains complete decoded records and original-input ties through
bounded spill. Its copied output header uses the original source header or
first accepted source record, independently of sorted encounter order.
Its ranking field uses the decoded field-local numerical
contract, and copied data preserves complete bytes.
Aggregate and per-row calculations support their existing positional
lists, numeric ranges, escaped named selectors, paired selectors and parameters.
Aggregate grouping and `--full` work with decoded CSV fields. Add `-s` to bring
equal decoded grouping keys together, using bounded spill when the input exceeds
the maintained sort-memory target. Per-row operations retain their ordinary
conflict with grouping keys. With no grouping keys, `-s` keeps the streaming
workflow and source order, including per-row and `--full` reports. Output-only CSV retains sorted
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

### Grouping decoded fields

Grouping keys compare decoded values, so `west` and `"west"` belong to the same
adjacent group. Repeated keys in a later run stay separate without sorting:

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
numeric input and output keep their existing rules. A language locale does not
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
separate adjacent groups. Headers bind before sorting. Without an input header,
a generated output header takes its field width from the first sorted record.
Copied records keep their actual widths and original logical record/physical-line
locations through reordering and representative selection.

CSV sorting uses the same sort-memory policy as ordinary text, including
`FASTMASH_SORT_MEMORY_BYTES`, and spills sorted runs to temporary files when
needed. Complete decoded fields and their original input locations survive sorting.
The memory target covers sort chunks, not the whole process. An oversized record,
merge heads and retained statistical or text samples have their own storage needs.
Samples are not spilled. Temporary I/O errors fail with status 1, and checked
allocation failures refuse with status 77. Later failures may leave earlier output.

### Copying complete records

Full reports copy each decoded source field before the calculation results.
For example, a per-row report can retain a comma in a source field while
selecting its numeric field with a custom result label:

```sh
printf 'place,value\n"west,coast",2\neast,4\n' | LC_ALL=C fastmash --csv -H --full --result-name=1:reading cut value
```

```csv
place,value,reading
"west,coast",2,2
east,4,4
```

For an aggregate, the representative starts as the group's first record.
`first`, `last`, `rand` and improving extrema may select another record under
their existing rules. Extrema ties retain the earlier selection. With multiple
operations the representative need not supply every result. Aggregate `--full`
retains its existing GNU compatibility warning. Per-row `--full` does not warn.
Copied-field labels replace GroupBy labels, and result-name indices still count
only operation-produced results. Without sorting, header width comes from the
first logical record. Each copied data record keeps its actual width, without padding or
filler. Required fields must still exist, including with `--no-strict` or
`--filler`. Copied fields preserve complete bytes, including NUL, independently
of an operation's display rules.

### CSV syntax and bytes

A double quote opens quoting only at the start of a field. Inside quotes,
commas, CR and LF are data, and two double quotes decode to one. After a closing
quote, only comma, LF, CRLF or end of input is allowed. Quotes within unquoted
fields, stray bytes after closing quotes, unfinished quoted fields and bare CR
outside quotes are errors, including in unused fields. Spaces are data, with no
implicit trimming. LF and CRLF endings may be mixed. A complete final record
may omit its ending.

An empty file has no records. Each blank LF or CRLF record has one empty field. Quoted and unquoted empty fields decode identically. A trailing comma supplies
a final empty field, while a terminal record ending supplies no extra record.
Different widths are permitted when all requested fields exist. An absent field
still errors. An existing empty field is valid text but invalid numeric input.
`--narm` retains the existing exact, case-insensitive NA/N/A/NaN matching and
independent paired-side compaction. It does not skip empty fields or trim spaces.

Decoding preserves bytes, including tabs, NUL, non-UTF-8 and internal CRLF.
Operations keep their existing display and transformation rules: `cut` stops
display at NUL, while base64 and checksums consume the complete decoded field.
The active numerical locale and presentation settings apply to each isolated
decoded numeric field. An unused numeric neighbor cannot affect conversion.
Ordinary text numerical conversion keeps its existing behavior.

A leading UTF-8 BOM is data. Before an unquoted input header it becomes part of
the name and generated label, following the normal byte-escaped selector rules.
Before a quoted first field it is data followed by an illegal quote, so that
export fails. There is no BOM stripping or transcoding. This dialect has stated
byte and blank-record extensions. It is not a claim of full RFC 4180 conformance.

With `--csv-in`, text results use the normal tab default and accept an explicit
`--output-delimiter`. Decoded delimiters or line breaks in result fields may make
that text report ambiguous. Use `--csv` when field boundaries must survive output.
CSV input conflicts with explicit `-t`, `-W` and `-C`, including their GNU
abbreviations and redundant comma delimiters. Either CSV direction conflicts
with `-z` and `--vnlog`, in either option order, with error status 1. Legacy modes
refuse CSV before input. Health rejects all CSV switches and remains text-only.

### Errors and locations

CSV validation errors identify the original logical record number, counting an
input header, and its starting physical line. Syntax errors also identify the
detected physical line and byte position, counting raw bytes including quotes. Unfinished quotes identify unexpected end of input. LF inside quotes advances
the physical line. CRLF counts as one line break. Header lookup errors include
the header's location when it exists. I/O failures keep their I/O context without
inventing a record or turning a read failure into a quoted-field EOF error.
Malformed CSV uses status 1, checked capacity refusals use 77, and output
finalization retains normal precedence. Later failures can leave partial output.

## CSV output from delimited text

Use `--csv-out` to produce quoted CSV from ordinary delimited-text input.
It works with aggregate and per-row operations, including adjacent or sorted
groups, explicit headers and copied `--full` fields. Repeating the switch is
idempotent. It does not enable either input or output headers.
[Top-N selection](top-n-records.md) also supports CSV output from complete
selected text records, including sorted groups. It adds no calculation columns.

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
custom names, grouping keys, formatted results and each copied `--full` field.
Commas separate fields and LF ends each emitted record. A field containing a
comma, double quote, CR or LF is quoted, with internal double quotes doubled.
An empty field prints as `""`. Embedded line breaks retain their bytes. Output
is canonical. It does not retain an input field's quoting style or line ending.

Decimal-comma formatted numbers and `collapse` or `unique` lists stay within one
quoted result field. The collapse delimiter is still the list's internal
separator. CSV adds no escaping scheme inside those lists. Encoding preserves
the result bytes, including non-UTF-8 and NUL bytes, under the existing operation
and header display rules. For example, raw keys and copied fields retain NUL,
while selected text results and generated field-name displays stop at NUL as
they do in ordinary output. A leading UTF-8 BOM remains field data.

Output-only CSV allows text-input `-t`, `-W` and `-C`. An explicit
`--output-delimiter`, `-z` or `--vnlog` conflicts with CSV output and fails with
status 1, regardless of option order or a redundant delimiter value. Crosstab,
legacy Table modes and field modes refuse CSV with status 77 before input.
Health reports remain text-only and reject CSV switches using their status-1
calculation-control diagnostic. Only the exact CSV switch spellings are accepted. Existing GNU option abbreviations retain their meanings.

CSV output retains empty-input and header-only timing and the normal failure
policy. A later input, calculation or transport failure can leave partial output.


## Custom result names

Give calculated output columns stable names in your reports. Select output headers with
`--header-out` or `-H`, then repeat `--result-name=INDEX:NAME` for the labels you
want to replace. The separate form `--result-name INDEX:NAME` works too.

For example, a report can give its sum a stable schema name while retaining the
generated label for its mean:

```sh
printf 'reading\n2\n4\n' | LC_ALL=C fastmash -H --result-name=1:total sum reading mean reading
```

```text
Summary
────────────────────────────────
  total  mean(reading)
      6              3
✔ Completed
```

Indices follow operation-produced output order after selector expansion, starting
at 1. A list or range contributes each expanded result, repeated operations
contribute their results again, and a paired operation contributes one result.
Each field selected by `cut` also contributes a result. Grouping keys and the
copied fields from `--full` do not consume indices or receive replacement labels.

In this example, `sum 2,1-2` produces three results in the order 2, 1, 2. Names
can be supplied out of order, and unnamed results keep their generated labels:

```sh
printf '1\t2\n3\t4\n' | LC_ALL=C fastmash --header-out --result-name=3:again --result-name=1:total sum 2,1-2
```

```text
Summary
────────────────────────────────
  total  sum(field-1)  again
      6             4      6
✔ Completed
```

Names replace labels only. They do not change selector lookup, required-field
checks, calculation values, output order, or when headers appear. Empty input
still produces no invented header. With `-H`, header-only input retains its
normal output header without a result row. Later input or output failures can
leave partial output, as with ordinary generated headers.

`INDEX` must be an unsigned, nonzero decimal integer within the expanded result
count. Repeating a target, omitting a name, or supplying an empty name is an error.
Duplicate name values are allowed. Split at the first colon, so a name can itself
contain colons. Name bytes are preserved as supplied by the operating system.
For text output, names cannot contain the effective output delimiter or record
terminator. CSV output allows comma, quote, colon, CR and LF bytes in names and
encodes them as one field. Invalid names fail before any output.

Result names work for ordinary aggregate and per-row operations, including
adjacent and sorted groups. Crosstab, table modes (`transpose`, `check`, `rmdup`)
and field modes (`reverse`, `noop`) refuse naming before reading input. Only the
exact `--result-name` spelling is accepted. Existing GNU option abbreviations
retain their normal meanings. `--vnlog` alone does not satisfy the requirement
for explicit output headers. Add `--header-out` or `-H`.

GNU datamash 1.9 generates labels such as `sum(reading)`. Stable custom report
labels otherwise need a wrapper or a downstream renaming step. Naming inside
the command avoids that extra step and follows the existing header timing.
