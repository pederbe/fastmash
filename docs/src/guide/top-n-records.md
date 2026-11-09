# Highest and lowest records

Use `top:N FIELD` to keep the N records with the highest values in one numeric
field. `bottom:N FIELD` keeps the lowest. Each selected record retains all of
its fields.

## Select the highest records

```console
$ printf 'A\t8\tfirst observation\nB\t10\tsecond observation\nC\t10\tthird observation\nD\t9\tfourth observation\n' | fastmash top:2 2
Top-N selection
────────────────────────────────
  field-1  field-2  field-3
        B       10  second observation
        C       10  third observation
✔ Completed
```

B and C tie at 10. Earlier input records win ties, including at the cutoff.
The command emits at most N records in rank order. Duplicates remain separate
observations, and fewer eligible records produce fewer results.

Use a field number or, with `-H` or `--header-in`, a field name. Lists, ranges
and field pairs are not accepted for ranking. Selection cannot be combined
with calculations or other modes.

## Ranking and missing values

Ranking uses the active numerical profile and supported locale, including its
binary80 precision. Numbers that convert to the same value tie, regardless of
their spelling. Both zero signs tie. Infinities sort beyond finite values in
their numeric direction. Any unskipped NaN is an error because it is unordered.
Only the ranking field must be numeric. Accompanying fields are copied without
numeric checks.

With `--narm`, records whose ranking field is exactly `NA`, `N/A` or `NaN`, in
any letter case, are omitted. Matching does not trim whitespace. Empty or absent
fields, malformed numbers, range failures, signed NaNs and NaN payloads remain
errors. Empty and all-missing datasets emit no data records. Every accepted
record is checked, including late records that cannot enter the selection.

## Copied fields and CSV

Selection copies complete logical fields in their original order, retaining
source numeric spelling, trailing empty fields, ragged widths and arbitrary
accompanying bytes. It adds no score, rank or result column. Ordinary separators,
whitespace input, output delimiters, comment filtering and NUL-terminated records
use the existing controls. Whitespace separator runs become the effective output
delimiter, and a final unterminated record receives an output terminator.
Ordinary output does not escape embedded output delimiters or record terminators
in copied fields.
Use `--csv-out` to encode selected text records as CSV, including sorted text
groups. Use `--csv-in` to select decoded CSV records with ordinary output, or
`--csv` for CSV input and output together. These exact-only switches are
idempotent and do not enable headers.

CSV follows the [shared input and output rules](csv-and-result-names.md#csv-syntax-and-bytes).
Every field is checked, including fields on omitted records or candidates
that cannot win. Output preserves complete decoded fields and numeric spelling,
with canonical CSV quoting.

`--full` is accepted with the same output and no deprecation warning. `-s`
without grouping keys leaves this whole-dataset selection unchanged.

## Selection within groups

With `-g FIELD[,FIELD...]`, each consecutive group has its own cutoff. Groups
appear in encounter order, and rank order applies within each group. For
example, `fastmash -H -g category top:2 reading` selects the highest two
records in each adjacent category, binding `category` and `reading` from the
input header. A later repeated category remains a separate group. Selected
records retain every field once, without an extra grouping-key prefix.
Multiple keys and `-i` follow the existing group equality and supported locale
controls. No-key `-i` leaves numeric ranking unchanged. Empty key text is a key
value, while an absent required key is an error. Every required key and group
boundary is checked before `--narm` omission, so an all-missing intervening
group still separates equal-key runs on either side.
CSV keys compare decoded field values, so different quote spellings of the
same text belong to the same adjacent group.

Add `-s` to prepare interleaved categories under the existing sorted group
workflow, for example `fastmash -s -H -g category top:2 reading`. Groups then
follow the existing key order, with ranking applied inside each group. Sorting
keeps complete records and original input identity through memory and spill,
so earlier source records still win equal-rank ties. Sorting order and group
equality retain their existing distinct meanings: whitespace key separators,
case controls, language-locale spellings and NUL-terminated input can affect
the prepared runs without introducing global key normalization. Selection uses
the existing native sort for text and decoded CSV sort for quoted CSV.
Pipe and regular-file input use the same selection route for each format.

For example, `fastmash --csv -s -H -g category top:2 reading` selects the
highest two records per category from an interleaved CSV export. Sorting uses
decoded key bytes, so quoted and unquoted spellings of the same key sort
together. Each selected record keeps its complete decoded fields, including
multiline notes, through sorting and spill. CSV output encodes them again.
Locale ordering still does not merge differently spelled keys that existing
group equality treats separately. No global key normalization is introduced.

## Headers

`--header-in` consumes the first accepted record as the input header and
excludes it from ranking. Names resolve to the first matching label when a
header contains duplicates. `--header-out` emits one copied-field output header
for the whole command, without operation wrappers or appended group labels.
`-H` selects both controls. With an input header, copied labels follow the
existing display convention, stopping at the first NUL byte. Without one,
labels are `field-1`, `field-2` and so on, using the first accepted source data
record's width even when that record is omitted or does not win. Later ragged
records do not revise the header, and sorting does not change its source schema.
Positional ranking fields may extend beyond
the input header's labels when the data supplies them. Empty input invents no
header. Header-only input with both controls and all-missing nonempty input can
emit a requested header without data. Name lookup fails before header output.

## Completion and failures

The entire input must be inspected before ungrouped data is emitted. Completed
adjacent groups can emit as the next group begins. A read failure or malformed
record does not emit the incomplete current selection, but earlier groups and
the output header may remain. Output writes and finalization must succeed. A
later output failure can leave partial bytes.
Sorted grouping finishes input intake before emitting selected data. A read or
temporary-sort failure emits no incomplete sorted group. Later ranking or key
errors can leave already completed groups. Diagnostics retain original accepted
record numbers, including the input header and excluding filtered comments.
CSV input errors identify the original logical record and its starting physical
line. Syntax errors also retain their detected location.

## Memory and limits

N is a mandatory positive decimal integer, at most 18446744073709551615.
Leading zeros are accepted. At most N candidate records are retained for the current group or whole dataset,
with storage growing as records arrive and candidate state released between
groups. A large requested N does not allocate N records in advance. N limits
candidate count, not bytes or total process memory: large records or a large N
can still exhaust memory. Checked resource failures refuse with status 77.
Fastmash does not truncate fields, reduce N or approximate the winners.
Sorted grouping has a separate sort-memory target, controlled by
`FASTMASH_SORT_MEMORY_BYTES`, and spills input when needed under the maintained
sort policy. Candidate retention remains bounded by N for the active group
and does not spill. Neither limit is a guarantee of total process memory.

## Option compatibility

CSV input conflicts with explicit text-input separators and
comment filtering. Any CSV format conflicts with NUL termination and `--vnlog`.
CSV output also conflicts with an explicit output delimiter. Format conflicts
are errors with status 1, independently of option order. CSV input with ordinary
output can use an explicit output delimiter, subject to ordinary framing limits.
A named field without `-H` or `--header-in` is a command error.
Explicit result names, numeric presentation controls, collapse delimiter,
filler, random seed, `--no-strict`, `--vnlog` and `--sort-cmd` are unsupported
for selection and refuse with status 77. Malformed options and conflicting CSV
controls retain error status 1. Ordinary commands keep their existing behavior.
