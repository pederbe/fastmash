# Dataset comparison

Fastmash compares numerical summaries of two raw datasets:

```console
fastmash compare before.tsv after.tsv sum 2 mean 3
```

BEFORE and AFTER are literal paths. A standalone `-` supplies stdin on one side:

```console
producer | fastmash compare before.tsv - sum 2
```

Files are opened independently, even when both paths are identical. The before
source is read first, followed by after. Files are read as supplied, without a
snapshot or locking. Use the ordinary `--` option terminator for paths beginning
with `-`. The same separator, whitespace and comment controls, Record terminator,
numeric locale, Missing-value policy and Operations apply to both sides.

Comparison supports whole datasets and positional or named Comparison keys with
single, list, range and paired Operation Selectors. All numerical aggregates, existing
aliases and parameters are supported, including `wmean VALUE:WEIGHT`.
Text-result aggregates such as `first`, `unique` and `collapse` are refused.

## Input and output formats

Input and output formats are explicit and independent. The same input format
applies to both sources; comparison does not infer a format or support different
formats on the two sides.

```console
fastmash --csv-in -g 1 compare before.csv after.csv sum 2
fastmash --csv-out -g 1 compare before.tsv after.tsv sum 2
fastmash --csv -H -g category compare before.csv after.csv rank 1 percent limit 10 sum reading
```

`--csv-in` reads strict comma-separated byte Fields and retains ordinary text
output. `--csv-out` encodes each complete output Field as CSV, with commas and
LF Record endings. `--csv` enables both. These switches are exact-only,
repeatable and header-neutral. Text input still supports its ordinary separator,
whitespace, comment filtering and NUL Record termination where allowed.

CSV quotes open only at a Field's start; doubled quotes decode to one quote.
Quoted Fields may contain commas, quotes, CR and LF. LF or CRLF ends a Record,
and a complete final Record may omit its ending. A blank Record has one empty
Field; a trailing comma adds another empty Field. Empty input has no Records.
Bytes are retained without trimming, BOM stripping, transcoding or newline
normalization, including NUL and non-UTF-8 bytes. Numeric conversion, names and
Comparison keys use these decoded Fields. Equal decoded key bytes consolidate
even when their original quote spellings differ.

Every CSV Field and Record is checked, including unused content, omitted pairs,
eligible keys below a ranking cutoff and content after a nonfinite result.
Malformed quoting and bare CR outside quotes are input Errors. CSV input
conflicts with explicitly supplied `-t`, `-W` and `-C`; either CSV direction
conflicts with `-z` and `--vnlog`; CSV output conflicts with an explicit
`--output-delimiter`. These conflicts are Errors regardless of option order.
CSV output from text input permits the text separator and comment controls.

CSV output individually encodes keys, labels, control/state columns, ranks and
numerical cells. Fields containing comma, quote, CR or LF are quoted, with
quotes doubled; empty cells print as `""`. A locale decimal comma remains inside
one quoted numeric Field. NUL and non-UTF-8 bytes remain intact. CSV-to-text
output supports an explicit Output delimiter but keeps ordinary unescaped text
framing: embedded delimiter or Record-ending bytes can make such text ambiguous.

## Input and output headers

Use `--header-in` to consume the first accepted Input-header Record on each
source. `-H` also requests an Output header. A header-only source is valid and
has no summary; an entirely empty source, including one containing only filtered
comments, is an Error when input headers are requested. Named Fields must resolve
on both headers even when a source has no data. Headers are never inferred.

Named Selectors bind independently, so the same names can occupy different
positions in the two exports:

```console
fastmash -H -g category compare before.tsv after.tsv sum reading
fastmash -H -g category compare before.tsv after.tsv wmean reading:weight
```

Names use ordinary exact, case-sensitive lookup; the first duplicate label wins
and backslash escapes one byte, for example `metric\,one`. Positional and mixed
Selectors retain the requested positions on both sides. Unselected header Fields
may differ. With only `--header-in`, positional data Fields may extend beyond the
header labels. When producing an Output header, selected labels in its preferred
source must exist under the ordinary header renderer's rules; unused after
labels do not constrain positional data.

`--header-out` or `-H` requests exactly one fixed Output header, including when
both sources have no data. There is no implicit comparison header. Labels use
the before Input header when available, otherwise ordinary `field-N` labels.
Keys are labeled `key(FIELD_LABEL)`, followed by `presence`, `rank` and
`rank_state`. For each ordinary generated summary label L, the six labels are
`before(L)`, `after(L)`, `difference(L)`, `difference_state(L)`, `percentage(L)`
and `percentage_state(L)`. Label display retains ordinary conventions, including
stopping at NUL; this never truncates Comparison-key identity or spelling.

Result names replace L throughout one summary's complete block:

```console
fastmash -H --result-name=1:total compare before.tsv after.tsv sum reading mean reading
fastmash --header-out --result-name=2:total compare before.tsv after.tsv sum 2,1
```

Indexes count expanded summary results in request order, once per result. They
do not count the six derived columns, rename keys or control columns, or identify
a ranking result by name. Names must be nonempty; each index can be supplied
once, while different results may share a name. Text names must not contain the
output delimiter or Record terminator; CSV output permits and encodes those
bytes. Numerical formatting applies to summary and change values independently
of their labels.

Both source headers, required named Binding, every input and all calculations
must succeed before this Output header is written.

## Report columns

Without keys, each nonempty dataset has one summary. Output has `presence`,
`rank` and `rank_state`, followed by six columns per expanded result in request
order. Without ranking, every rank is empty and every rank state is
`not_requested`:

| Column | Meaning |
| --- | --- |
| before | Completed numerical summary from BEFORE |
| after | Completed numerical summary from AFTER |
| difference | Signed after minus before |
| difference_state | `available`, `unordered` or `not_matched` |
| percentage | Signed ordinary percentage change, without a percent suffix |
| percentage_state | `available`, `not_matched`, `zero_baseline`, `nonfinite_baseline` or `nonfinite_after` |

Presence is `matched`, `added` or `removed`. An empty source has no summary;
an absent side and both changes have empty cells with `not_matched` change
states. Two empty sources produce no data. Accepted Records still establish a
summary when `--narm` omits every value. Those Operations keep their existing
empty-sample results or completion errors, including Weighted mean's requirement
for positive retained weight. Paired Operations retain their established
independent compaction; Weighted mean validates partners and omits whole pairs.

## Comparison keys

Use the existing `-g` Selectors to identify corresponding categories:

```console
fastmash -g 1 compare before.tsv after.tsv sum 2 mean 2
fastmash -g 1,2 compare before.tsv after.tsv wmean 3:4
```

Every distinct key has one summary per source, even when its Records are
interleaved. Each repeated raw Record contributes normally. All Operations keep
the original encounter order within each key, including rounding-sensitive
Weighted mean calculations. `-s` is accepted and gives exactly the same report.
This consolidation differs from ordinary adjacent Groups; it does not change
their equality or the locale ordering of ordinary sorted Groups.

A key is an ordered tuple of complete Field byte strings. Field boundaries,
lengths and bytes after NUL all matter: `('a', 'bc')` differs from `('ab', 'c')`,
and `01` differs from `1`. Whitespace is retained. `-i` folds only ASCII A
through Z across complete Fields; non-ASCII bytes remain unchanged. Empty key
text is valid, while a missing required key Field is an Error. Keys are extracted
before any Operation omits Missing values, so all-omitted categories remain in
the report and keep their ordinary completion results or errors.

Reports align the complete union of keys after both sources finish. Keys precede
the report columns above. A matched or removed key displays its first before
spelling; an added key displays its first after spelling, including under `-i`.
Unranked reports follow lexicographic order over normalized key tuples, using
unsigned bytes and shorter prefixes first. This order is independent of locale
collation and input adjacency.

For a finite nonzero baseline, percentage is `(after - before) / before * 100`.
The denominator retains its sign: -100 changing to -50 has difference 50 and
percentage -50; -100 changing to -150 has difference -50 and percentage 50.
Both zero signs make percentage unavailable. Reasons are checked in this order:
`not_matched`, `zero_baseline`, `nonfinite_baseline`, `nonfinite_after`.
NaNs and infinities remain visible in the before and after summary cells.
NaN differences, including equal-sign infinities, are `unordered`; other infinite
differences remain available.

## Ranking and limits

Place `rank RESULT [absolute|percent]` after the source paths and before all
Operations. The default measure is absolute. `RESULT` selects one 1-based
expanded summary result, counting list and range expansion in request order;
Result names and the six derived columns do not affect its index. All requested
result blocks remain in the report:

```console
fastmash -g 1 compare before.tsv after.tsv rank 1 sum 2
fastmash -g 1 compare before.tsv after.tsv rank 2 percent limit 10 sum 2-3
fastmash -H -g category --result-name=1:total compare before.tsv after.tsv rank 1 percent sum reading
```

Ranking orders matched keys by descending magnitude of the selected available
difference or percentage. Signed cells keep their signs, including negative
baseline percentages. For example, A changing from 100 to 130 ranks ahead of B
changing from 10 to 20 by absolute change (30 versus 10); percentage ranking
puts B first (100 versus 30). Matched zero changes are eligible. Available
infinite differences rank above finite magnitudes in absolute ranking. NaNs and
unavailable changes never participate.

Equal magnitudes, including opposite signs, both zero signs and infinities, tie
by the normalized complete Comparison-key order. Input order and displayed
rounding cannot change winners. Ranks are consecutive plain unsigned decimal
numbers, even with hexadecimal or other numerical formats.

An optional `limit N` follows ranking and keeps at most N eligible keys, without
padding. Eligible keys below the cutoff are omitted. Every excluded key still
appears after the ranked portion in canonical key order, with an empty rank and
rank state `one_sided` for added/removed keys or `unavailable` otherwise. The
selected result's change-state columns provide the exact reason. Thus N bounds
ranked keys, and total report records can exceed N. With no limit, every
rankable key appears; an entirely unrankable dataset still reports every key.

RESULT and N accept leading zeros, reject signs and overflow, and must be
positive. N can be as large as 18446744073709551615, without reserving N entries
in advance. Ranking and limit occur once in that order; limit requires ranking,
and neither modifier can follow or interrupt Operations. Invalid modifiers or
result indexes fail before sources are opened. Every source, key, requested
summary and change must finish before cutoff selection and output, so a limit
cannot hide a later Error or a failure in an unselected result.

## Precision and completion

Changes use completed binary80 numerical values. Subtraction, division by the
signed baseline and multiplication by exact 100 each round nearest-even,
independently. Displayed values are never used as calculation inputs. Current
numeric formatting and rounding controls apply to every numerical cell.
Finite overflow in any new arithmetic stage refuses with status 77 and names
the result and stage. Subnormal differences and exact cancellation are allowed.
Distinct completed binary80 summaries have a nonzero relative difference of
at least 2^-64, so percentage division and multiplication cannot underflow;
equal summaries produce exact zero. The ordered trace can refuse extreme pairs
even when their final mathematical percentage would fit, and it does not promise
one correctly rounded evaluation of the full formula. Existing summary
Operations retain their own numerical limits.

Every requested summary and change, both source scans and input completion must
succeed before comparison writes stdout, including headers. Source errors identify
before or after and the supplied path. CSV input Errors retain the original
logical Record and its starting physical line; syntax failures also retain the
detected physical line and byte when available. Completion/change failures name
the side where applicable, result and key without inventing an input Record.
Output write or finalization errors prevent success and can leave partial output.
Key text, per-key Operation state and required statistical samples remain in
memory under existing checked resource limits. Comparison streams intake without
retaining complete raw Records for consolidation. It does not spill samples,
approximate summaries or promise a fixed total-memory bound.

Comparison refuses Full rows, other Modes, `--vnlog`, `--no-strict`, explicit
Filler, Collapse delimiter, random Seed and `--sort-cmd`. Malformed commands and
conflicting format controls are Errors with status 1.
