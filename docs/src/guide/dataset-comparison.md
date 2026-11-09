# Dataset comparison

Compare statistical summaries from two exports, including categories that were
added or removed. For example, create two small sales exports:

```sh
printf 'category,revenue\nwest,100\neast,50\n' > before.csv
printf 'category,revenue\neast,75\nwest,80\nnorth,20\n' > after.csv
```

Compare revenue by category:

```sh
fastmash --csv -H -g category compare before.csv after.csv sum revenue
```

```csv
key(category),presence,rank,rank_state,before(sum(revenue)),after(sum(revenue)),difference(sum(revenue)),difference_state(sum(revenue)),percentage(sum(revenue)),percentage_state(sum(revenue))
east,matched,"",not_requested,50,75,25,available,50,available
north,added,"",not_requested,"",20,"",not_matched,"",not_matched
west,matched,"",not_requested,100,80,-20,available,-20,available
```

The `east` summary rises from 50 to 75, a change of 25 or 50%. `west` falls by
20%, and `north` appears as an added category with no before value. Without `-g`, the command
compares one summary for each whole dataset.

## Source files and supported operations

BEFORE and AFTER are literal paths. A standalone `-` supplies stdin on one side:

```console
producer | fastmash compare before.tsv - sum 2
```

Files are opened independently, even when both paths are identical. The before
source is read first, followed by after. Files are read as supplied, without a
snapshot or locking. Use the ordinary `--` option terminator for paths beginning
with `-`. The same separator, whitespace and comment controls, record terminator,
numeric locale, missing-value policy and operations apply to both sides.

Comparison supports whole datasets and positional or named comparison keys with
single, list, range and paired operation selectors. All numerical aggregates, existing
aliases and parameters are supported, including `wmean VALUE:WEIGHT`.
Text-result aggregates such as `first`, `unique` and `collapse` are refused.

## Input and output formats

Input and output formats are explicit and independent. The same input format
applies to both sources. Comparison does not infer a format or support different
formats on the two sides.

```console
fastmash --csv-in -g 1 compare before.csv after.csv sum 2
fastmash --csv-out -g 1 compare before.tsv after.tsv sum 2
fastmash --csv -H -g category compare before.csv after.csv rank 1 percent limit 10 sum reading
```

`--csv-in` reads strict comma-separated byte fields and retains ordinary text
output. `--csv-out` encodes each complete output field as CSV, with commas and
LF record endings. `--csv` enables both. These switches are exact-only,
repeatable and header-neutral. Text input still supports its ordinary separator,
whitespace, comment filtering and NUL record termination where allowed.

CSV uses the [shared syntax and encoding rules](csv-and-result-names.md#csv-syntax-and-bytes).
Names, numeric values and comparison keys use decoded fields, so different
quote spellings of the same key consolidate. Every field is checked, including
unused content, omitted pairs and records below a ranking cutoff. Output encodes
each complete report field independently, quoting empty cells and locale decimal
commas.

CSV input conflicts with `-t`, `-W` and `-C`. Either CSV direction conflicts
with `-z` and `--vnlog`. CSV output also conflicts with an explicit
`--output-delimiter`. These are command errors regardless of option order.
CSV-to-text output permits an explicit delimiter, but embedded delimiters or
line breaks can make text ambiguous. Use CSV output to preserve field boundaries.

## Input and output headers

Use `--header-in` to consume the first accepted input-header record on each
source. `-H` also requests an output header. A header-only source is valid and
has no summary. An entirely empty source, including one containing only filtered
comments, is an error when input headers are requested. Named fields must resolve
on both headers even when a source has no data. Headers are never inferred.

Named selectors bind independently, so the same names can occupy different
positions in the two exports:

```console
fastmash -H -g category compare before.tsv after.tsv sum reading
fastmash -H -g category compare before.tsv after.tsv wmean reading:weight
```

Names use ordinary exact, case-sensitive lookup. The first duplicate label wins
and backslash escapes one byte, for example `metric\,one`. Positional and mixed
selectors retain the requested positions on both sides. Unselected header fields
may differ. With only `--header-in`, positional data fields may extend beyond the
header labels. When producing an output header, selected labels in its preferred
source must exist under the ordinary header renderer's rules. Unused after
labels do not constrain positional data.

`--header-out` or `-H` requests exactly one fixed output header, including when
both sources have no data. There is no implicit comparison header. Labels use
the before input header when available, otherwise ordinary `field-N` labels.
Keys are labeled `key(FIELD_LABEL)`, followed by `presence`, `rank` and
`rank_state`. For each ordinary generated summary label L, the six labels are
`before(L)`, `after(L)`, `difference(L)`, `difference_state(L)`, `percentage(L)`
and `percentage_state(L)`. Label display retains ordinary conventions, including
stopping at NUL. This never truncates comparison-key identity or spelling.

Result names replace L throughout one summary's complete block:

```console
fastmash -H --result-name=1:total compare before.tsv after.tsv sum reading mean reading
fastmash --header-out --result-name=2:total compare before.tsv after.tsv sum 2,1
```

Indexes count expanded summary results in request order, once per result. They
do not count the six derived columns, rename keys or control columns, or identify
a ranking result by name. Names must be nonempty. Each index can be supplied
once, while different results may share a name. Text names must not contain the
output delimiter or record terminator. CSV output permits and encodes those
bytes. Numerical formatting applies to summary and change values independently
of their labels.

Both source headers, required named binding, every input and all calculations
must succeed before this output header is written.

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

Presence is `matched`, `added` or `removed`. An empty source has no summary. An absent side and both changes have empty cells with `not_matched` change
states. Two empty sources produce no data. Accepted records still establish a
summary when `--narm` omits every value. Those operations keep their existing
empty-sample results or completion errors, including weighted mean's requirement
for positive retained weight. Paired operations retain their established
independent compaction. Weighted mean validates partners and omits whole pairs.

## Comparison keys

Use the existing `-g` selectors to identify corresponding categories:

```console
fastmash -g 1 compare before.tsv after.tsv sum 2 mean 2
fastmash -g 1,2 compare before.tsv after.tsv wmean 3:4
```

Every distinct key has one summary per source, even when its records are
interleaved. Each repeated raw record contributes normally. All operations keep
the original encounter order within each key, including rounding-sensitive
weighted mean calculations. `-s` is accepted and gives exactly the same report.
This consolidation differs from ordinary adjacent groups. It does not change
their equality or the locale ordering of ordinary sorted groups.

A key is an ordered tuple of complete field byte strings. Field boundaries,
lengths and bytes after NUL all matter: `('a', 'bc')` differs from `('ab', 'c')`,
and `01` differs from `1`. Whitespace is retained. `-i` folds only ASCII A
through Z across complete fields. Non-ASCII bytes remain unchanged. Empty key
text is valid, while a missing required key field is an error. Keys are extracted
before any operation omits missing values, so all-omitted categories remain in
the report and keep their ordinary completion results or errors.

Reports align the complete union of keys after both sources finish. Keys precede
the report columns above. A matched or removed key displays its first before
spelling. An added key displays its first after spelling, including under `-i`.
Unranked reports follow lexicographic order over normalized key tuples, using
unsigned bytes and shorter prefixes first. This order is independent of locale
collation and input adjacency.

For a finite nonzero baseline, percentage is `(after - before) / before * 100`.
The denominator retains its sign: -100 changing to -50 has difference 50 and
percentage -50. A change from -100 to -150 has difference -50 and percentage 50.
Both zero signs make percentage unavailable. Reasons are checked in this order:
`not_matched`, `zero_baseline`, `nonfinite_baseline`, `nonfinite_after`.
NaNs and infinities remain visible in the before and after summary cells.
NaN differences, including equal-sign infinities, are `unordered`. Other infinite
differences remain available.

## Ranking and limits

Place `rank RESULT [absolute|percent]` after the source paths and before all
operations. The default measure is absolute. `RESULT` selects one 1-based
expanded summary result, counting list and range expansion in request order. Result names and the six derived columns do not affect its index. All requested
result blocks remain in the report:

```console
fastmash -g 1 compare before.tsv after.tsv rank 1 sum 2
fastmash -g 1 compare before.tsv after.tsv rank 2 percent limit 10 sum 2-3
fastmash -H -g category --result-name=1:total compare before.tsv after.tsv rank 1 percent sum reading
```

Ranking orders matched keys by descending magnitude of the selected available
difference or percentage. Signed cells keep their signs, including negative
baseline percentages. For example, A changing from 100 to 130 ranks ahead of B
changing from 10 to 20 by absolute change (30 versus 10). Percentage ranking
puts B first (100 versus 30). Matched zero changes are eligible. Available
infinite differences rank above finite magnitudes in absolute ranking. NaNs and
unavailable changes never participate.

Equal magnitudes, including opposite signs, both zero signs and infinities, tie
by the normalized complete comparison-key order. Input order and displayed
rounding cannot change winners. Ranks are consecutive plain unsigned decimal
numbers, even with hexadecimal or other numerical formats.

An optional `limit N` follows ranking and keeps at most N eligible keys, without
padding. Eligible keys below the cutoff are omitted. Every excluded key still
appears after the ranked portion in canonical key order, with an empty rank and
rank state `one_sided` for added/removed keys or `unavailable` otherwise. The
selected result's change-state columns provide the exact reason. Thus N bounds
ranked keys, and total report records can exceed N. With no limit, every
rankable key appears. An entirely unrankable dataset still reports every key.

RESULT and N accept leading zeros, reject signs and overflow, and must be
positive. N can be as large as 18446744073709551615, without reserving N entries
in advance. Ranking and limit occur once in that order. Limit requires ranking,
and neither modifier can follow or interrupt operations. Invalid modifiers or
result indexes fail before sources are opened. Every source, key, requested
summary and change must finish before cutoff selection and output, so a limit
cannot hide a later error or a failure in an unselected result.

## Precision and completion

Changes use completed binary80 numerical values. Subtraction, division by the
signed baseline and multiplication by exact 100 each round nearest-even,
independently. Displayed values are never used as calculation inputs. Current
numeric formatting and rounding controls apply to every numerical cell.
Finite overflow in any new arithmetic stage refuses with status 77 and names
the result and stage. Subnormal differences and exact cancellation are allowed.
Distinct completed binary80 summaries have a nonzero relative difference of
at least 2^-64, so percentage division and multiplication cannot underflow. Equal summaries produce exact zero. The ordered trace can refuse extreme pairs
even when their final mathematical percentage would fit, and it does not promise
one correctly rounded evaluation of the full formula. Existing summary
operations retain their own numerical limits.

Every requested summary and change, both source scans and input completion must
succeed before comparison writes stdout, including headers. Source errors identify
before or after and the supplied path. CSV input errors retain the original
logical record and its starting physical line. Syntax failures also retain the
detected physical line and byte when available. Completion/change failures name
the side where applicable, result and key without inventing an input record.
Output write or finalization errors prevent success and can leave partial output.
Key text, per-key operation state and required statistical samples remain in
memory under existing checked resource limits. Comparison streams intake without
retaining complete raw records for consolidation. It does not spill samples,
approximate summaries or promise a fixed total-memory bound.

Comparison refuses full rows, other modes, `--vnlog`, `--no-strict`, explicit
filler, collapse delimiter, random seed and `--sort-cmd`. Malformed commands and
conflicting format controls are errors with status 1.
