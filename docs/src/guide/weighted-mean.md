# Weighted mean

Fastmash implements `wmean VALUE:WEIGHT` for ordinary
delimited-text or quoted CSV reports, across a whole dataset or within adjacent
or sorted Groups, with independently selected input and output formats.
Each pair produces one result: the sum of weight-times-value products divided
by the sum of retained weights. Contribution weights are finite, nonnegative
numbers. Fractions are valid, weights need not sum to one, and zero contributes
nothing. They describe relative influence, rather than requesting replicated
Records or weighted variance.

For example, values 10 and 20 with weights 1 and 3 produce 17.5:

```sh
printf '10\t1\n20\t3\n' | env LC_ALL=C fastmash wmean 1:2
```

```text
17.5
```

Weights 0.25 and 0.75 give the same result for this exact example. Negative or
nonfinite weights are input Errors with status 1. Both signed zeros are zero
weights. A zero-weight Record still needs valid supplied Fields, but its
product is not evaluated, even for an admitted infinite or NaN value.
Positive weights permit admitted infinite and NaN values under the ordinary
arithmetic rules. Later Records are still validated after such a result is known.

## Named reports and adjacent Groups

Use explicit Input and Output headers with `-H`, or select them independently
with `--header-in` and `--header-out`. Named Selectors use the existing
case-sensitive first match for duplicate names; a backslash escapes one byte
in a name. Positional and named Fields may be mixed. Pair lists, repeated pairs,
shared weight Fields and same-Field pairs each produce a result in request order.

```sh
printf 'site\tvalue\tweight\na\t10\t1\na\t20\t3\nb\t5\t2\n' | env LC_ALL=C fastmash -H -g site --result-name=1:average wmean value:weight count value
```

```text
GroupBy(site)	average	count(value)
a	17.5	2
b	5	1
```

Without a Result name, the generated label identifies both roles, for example
`wmean(value,weight)` or `wmean(field-1,field-2)`. Each expanded pair consumes
one Result-name index; Grouping keys and copied `--full` Fields consume none.
Headers remain explicit. No count or total-weight column is added automatically.
Empty input produces no report; header-only input retains its ordinary Output
header and emits no data result. This includes sorted reports with named Fields
when input ends cleanly before the requested header. A present header still has
to contain the requested names, and a failed read is an input error.

Grouping uses the existing Group definition: consecutive Records with equal
Grouping keys. Multiple keys and existing case and locale controls apply.
Separated runs of the same key remain separate. The calculation resets between
Groups. Every requested Grouping key must exist, including with `--full` and on
omitted pairs; an empty key is still present. A preceding Group completes before
the next Group's pair is collected, preserving its result or completion failure.
`wmean` leaves the ordinary `--full` representative unchanged, initially
the first Record; another requested Operation may replace it. It does not choose
the largest-weight Record. Aggregate `--full` retains its existing deprecation
warning.

For ordinary-text input, existing separators,
whitespace splitting, comment filtering, NUL Record termination, numeric locales,
`--format` and `--round` apply. Unused ragged or byte-oriented content remains
accepted when every required numeric and key Field exists; this calculation
does not inspect whole-table health.

## Sorted text reports

Use `-s` to bring interleaved Grouping keys together. Multiple keys, named Fields,
headers, Result names and other aggregate Operations retain their usual behavior:

```sh
printf 'site\tvalue\tweight\nb\t5\t2\na\t10\t1\nb\t7\t2\na\t20\t3\n' | env LC_ALL=C fastmash -H -s -g site --result-name=1:average wmean value:weight
```

```text
GroupBy(site)	average
a	17.5
b	6
```

The same calculation accepts a regular file, for example
`env LC_ALL=C fastmash -H -s -g site wmean value:weight < measurements.tsv`.
Eligible Sort routes, Hash grouping and its replay, and bounded Spill preserve
the encounter order of each Group's contributions. Equal-key sorting is stable;
it does not rearrange a Group's values or weights to improve the arithmetic.
Existing case and Supported-locale controls determine key ordering and equality.
With `-W`, blanks before a key can affect sorting before ordinary Group equality
is applied. With no Grouping keys, `-s` keeps the whole-dataset encounter order.

Both pair Fields are retained, including reversed pairs and shared Fields.
`--full` keeps complete representative Records using the existing selection rule.
Sort storage has its own memory policy and can Spill; the weighted accumulator
still keeps two totals per request. Ordinary sorted numeric errors use the
established sorted Record position, rather than claiming an original-input
location. Temporary-storage, replay, read, output and close failures prevent
successful status, and numerical-range Refusals remain unchanged.

Select `--csv-out` independently to encode results, keys, copied Fields and
generated or supplied labels through the existing output encoder:

```sh
printf 'site\tvalue\tweight\nb\t5\t2\na\t10\t1\na\t20\t3\n' | env LC_ALL=C fastmash --csv-out -H -s -g site wmean value:weight
```

```text
GroupBy(site),"wmean(value,weight)"
a,17.5
b,5
```

Commas, quotes and line breaks in labels or copied Fields are quoted as needed;
locale decimal commas are likewise encoded within one result Field. CSV output
retains its existing conflicts with NUL Record termination and an explicit
ordinary Output delimiter. It does not imply an Input or Output header.

## Quoted CSV reports

Use `--csv-in` for CSV input with ordinary text output, `--csv-out` for CSV
output from ordinary text, or `--csv` for both. The switches do not imply
headers. Numeric conversion and named Selectors see decoded Field bytes,
including quoted names with commas, quotes or line breaks:

```sh
printf 'site,"value,raw",weight\r\nb,5,2\na,"10",1\nb,7,2\n"a",20,3\n' | env LC_ALL=C fastmash --csv -H -s -g site --result-name=1:average wmean 'value\,raw:weight'
```

```text
GroupBy(site),average
a,17.5
b,6
```

Omit `-s` to report each adjacent run separately, or omit `-g site` to report
one whole-dataset mean. Regular-file input uses the same calculation. Sorting
compares decoded keys, so `a` and `"a"` name the same key. Multiple keys and
existing case and Supported-locale controls apply. Stable sorting and bounded
Spill retain complete needed Fields and each Group's contribution order.

CSV output individually encodes results, keys, copied `--full` Fields, generated
paired labels and supplied Result names. For example, the generated label
`wmean(value,raw,weight)` is quoted as one CSV Field. Copied Fields preserve
complete decoded bytes, including non-UTF-8 bytes, NULs, internal CR/LF and
trailing empty Fields; their original quote spelling is replaced by canonical
encoding. A locale decimal comma is likewise quoted within one result Field.
CSV-to-text output retains ordinary text framing, so a decoded tab or newline
can appear as an ordinary delimiter or Record break. Choose CSV output when
those bytes need unambiguous Field boundaries.

Strict syntax validation covers every Field and Record, including unused
content, omitted pairs and zero weights, even after an infinite or NaN result
is known. LF, CRLF, multiline Fields, doubled quotes and a final complete Record
without a terminator follow the existing [CSV grammar](csv-and-result-names.md).
CSV input retains its option conflicts with text separators, whitespace
splitting and comment filtering; either CSV direction conflicts with NUL Record
termination and Vnlog. No autodetection or extra dialect is selected.

Numeric, invalid-weight, missing-Field and header errors retain the original
logical Record and starting physical line through sorting and Spill. Syntax
errors also identify the detected physical line and byte position. Completion
errors identify the weighted pair and Group without assigning a source Record.
Read, temporary-storage, output and close failures prevent successful status;
earlier output can remain under the ordinary report rules.

## Missing pairs and failures

With `--narm`, an exact whole-field `NA`, `N/A` or `NaN` token, ignoring case,
omits the entire value/weight pair for that request. Supplied nonmissing Fields
are validated in value-then-weight order before deciding omission.
Thus value `NA` with weight 2 is omitted, while
value `NA` with weight `oops`, -1 or infinity fails. Empty Fields, absent Fields,
malformed numbers and conversion range failures remain Errors. Surrounding
spaces, signed NaNs and NaN payloads do not become Missing values. Without
`--narm`, the ordinary numeric conversion rules apply.

Omission is local to each request. Another weighted pair or an unweighted
Operation can still use the same Record. Grouping keys and Group changes are
observed before omission. A nonempty dataset or Group with only zero or omitted
weights fails with status 1, identifying the weighted pair and, when grouped,
its keys. It does not silently disappear or produce a placeholder mean.
Completion errors do not invent a source Record location. Earlier completed
Groups, a header or part of the failing output Record may already have been
written. Input, output and close failures also prevent successful status.

## Numerical trace and limits

The calculation uses the current admitted binary80 arithmetic and presentation,
without narrowing operands to binary64. It starts both totals at positive zero.
For each retained positive-weight pair in encounter order it rounds the
weight-times-value product, adds and rounds that product into the weighted
total, then adds and rounds the weight into the weight total. Completion divides
the totals with one rounding. Each step uses nearest-even rounding. There is
no fused product/addition or rescaling. Results can depend on Record order;
arbitrary weight scaling is not promised to preserve every rounded bit.

A numerical Capacity refusal with status 77 identifies a finite product that
overflows or rounds a nonzero value to zero, an overflowing finite weighted
total, an overflowing weight total, or an overflowing finite final division.
The check happens when the intermediate is formed, even if later Records might
cancel it. Deliberate infinite value inputs are distinguished from finite
overflow. Finite subnormal intermediates, ordinary rounding and cancellation,
and final-result underflow remain permitted.

These limits can refuse finite inputs whose mathematical mean is representable.
Value 2 with weight `1e4932` refuses at its product; value `1e-4000` with weight
`1e-4000` refuses when its nonzero product rounds to zero. Scaling weights in
advance may change the evaluation trace and rounded result.

Unsorted `wmean` retains two numeric totals per request, independent of input
Record count. Full row context and other requested Operations keep their own
storage policies. This is not a total-process-memory or performance comparison.
