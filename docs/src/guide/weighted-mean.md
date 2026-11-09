# Weighted mean

Use `wmean VALUE:WEIGHT` to average values according to their contribution
weights. The result is the sum of weighted values divided by the total weight.
Weights must be finite and nonnegative. They may be fractions and need not sum
to one. Zero contributes nothing.

For example, values 10 and 20 with weights 1 and 3 produce 17.5:

```sh
printf '10\t1\n20\t3\n' | env LC_ALL=C fastmash wmean 1:2
```

```text
Summary
────────────────────────────────
  wmean(field-1,field-2)
                    17.5
✔ Completed
```

## Named reports and adjacent groups

Use explicit input and output headers with `-H`, or select them independently
with `--header-in` and `--header-out`. Named selectors use the existing
case-sensitive first match for duplicate names. A backslash escapes one byte
in a name. Positional and named fields may be mixed. Pair lists, repeated pairs,
shared weight fields and same-field pairs each produce a result in request order.

```sh
printf 'site\tvalue\tweight\na\t10\t1\na\t20\t3\nb\t5\t2\n' | env LC_ALL=C fastmash -H -g site --result-name=1:average wmean value:weight count value
```

```text
Grouped summary
────────────────────────────────
  site  average  count(value)
     a     17.5             2
     b        5             1
✔ Completed
```

Without a result name, the generated label identifies both roles, for example
`wmean(value,weight)` or `wmean(field-1,field-2)`. Each expanded pair consumes
one result-name index. Grouping keys and copied `--full` fields consume none.
Use `-H` for named result labels. No count or total-weight column is added
automatically.
Empty input produces no report. Header-only input retains its ordinary output
header and emits no data result. This includes sorted reports with named fields
when input ends cleanly before the requested header. A present header still has
to contain the requested names, and a failed read is an input error.

Grouping uses the existing group definition: consecutive records with equal
grouping keys. Multiple keys and existing case and locale controls apply.
Separated runs of the same key remain separate. The calculation resets between
groups. Every requested grouping key must exist, including with `--full` and on
omitted pairs. An empty key is still present. A preceding group completes before
the next group's pair is collected, preserving its result or completion failure.
`wmean` leaves the ordinary `--full` representative unchanged, initially
the first record. Another requested operation may replace it. It does not choose
the largest-weight record. Aggregate `--full` retains its existing deprecation
warning.

For ordinary-text input, existing separators,
whitespace splitting, comment filtering, NUL record termination, numeric locales,
`--format` and `--round` apply. Unused ragged or byte-oriented content remains
accepted when every required numeric and key field exists. This calculation
does not inspect whole-table health.

## Sorted text reports

Use `-s` to bring interleaved grouping keys together. Multiple keys, named fields,
headers, result names and other aggregate operations retain their usual behavior:

```sh
printf 'site\tvalue\tweight\nb\t5\t2\na\t10\t1\nb\t7\t2\na\t20\t3\n' | env LC_ALL=C fastmash -H -s -g site --result-name=1:average wmean value:weight
```

```text
Grouped summary
────────────────────────────────
  site  average
     a     17.5
     b        6
✔ Completed
```

The same calculation accepts a regular file, for example
`env LC_ALL=C fastmash -H -s -g site wmean value:weight < measurements.tsv`.
Sorting, including hash grouping and replay, preserves the encounter order of
each group's contributions. It does not rearrange values or weights to improve
the arithmetic.
Existing case and supported-locale controls determine key ordering and equality.
With `-W`, blanks before a key can affect sorting before ordinary group equality
is applied. With no grouping keys, `-s` keeps the whole-dataset encounter order.

Both pair fields are retained, including reversed pairs and shared fields.
`--full` keeps complete representative records using the existing selection rule.
Sort storage has its own memory policy and can spill. The weighted accumulator
still keeps two totals per request. Ordinary sorted numeric errors use the
established sorted record position, rather than claiming an original-input
location. Temporary-storage, replay, read, output and close failures prevent
successful status, and numerical-range refusals remain unchanged.

Select `--csv-out` independently to encode results, keys, copied fields and
generated or supplied labels through the existing output encoder:

```sh
printf 'site\tvalue\tweight\nb\t5\t2\na\t10\t1\na\t20\t3\n' | env LC_ALL=C fastmash --csv-out -H -s -g site wmean value:weight
```

```text
GroupBy(site),"wmean(value,weight)"
a,17.5
b,5
```

CSV output follows the [shared encoding rules](csv-and-result-names.md#csv-output-from-delimited-text),
including quoting generated labels such as `wmean(value,weight)` as one field.

## Quoted CSV reports

Use `--csv-in` for CSV input with ordinary text output, `--csv-out` for CSV
output from ordinary text, or `--csv` for both. The switches do not imply
headers. Numeric conversion and named selectors see decoded field bytes,
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
existing case and supported-locale controls apply. Stable sorting and bounded
spill retain complete needed fields and each group's contribution order.

CSV output preserves complete decoded keys and copied `--full` fields using
canonical encoding. CSV-to-text output retains ordinary text framing, so a
decoded tab or newline can appear as a delimiter or record break. Choose CSV
output when those bytes need unambiguous field boundaries.

Strict syntax validation covers every field and record, including unused
content, omitted pairs and zero weights, even after an infinite or NaN result
is known. The shared [CSV grammar and option conflicts](csv-and-result-names.md#csv-syntax-and-bytes)
apply. Fastmash does not infer the input format.

Numeric, invalid-weight, missing-field and header errors retain the original
logical record and starting physical line through sorting and spill. Syntax
errors also identify the detected physical line and byte position. Completion
errors identify the weighted pair and group without assigning a source record.
Read, temporary-storage, output and close failures prevent successful status.
Earlier output can remain under the ordinary report rules.

## Missing pairs and failures

Values 10 and 20 with weights 0.25 and 0.75 also produce 17.5. Negative or
nonfinite weights are input errors with status 1. Both signed zeros are zero
weights. A zero-weight record still needs valid supplied fields, but its
product is not evaluated, even for an admitted infinite or NaN value.
Positive weights permit admitted infinite and NaN values under the ordinary
arithmetic rules. Later records are still validated after such a result is known.

With `--narm`, an exact whole-field `NA`, `N/A` or `NaN` token, ignoring case,
omits the entire value/weight pair for that request. Supplied nonmissing fields
are validated in value-then-weight order before deciding omission.
Thus value `NA` with weight 2 is omitted, while
value `NA` with weight `oops`, -1 or infinity fails. Empty fields, absent fields,
malformed numbers and conversion range failures remain errors. Surrounding
spaces, signed NaNs and NaN payloads do not become missing values. Without
`--narm`, the ordinary numeric conversion rules apply.

Omission is local to each request. Another weighted pair or an unweighted
operation can still use the same record. Grouping keys and group changes are
observed before omission. A nonempty dataset or group with only zero or omitted
weights fails with status 1, identifying the weighted pair and, when grouped,
its keys. It does not silently disappear or produce a placeholder mean.
Completion errors do not invent a source record location. Earlier completed
groups, a header or part of the failing output record may already have been
written. Input, output and close failures also prevent successful status.

## Numerical trace and limits

The calculation uses 80-bit software arithmetic, without narrowing operands to
64-bit floating point. It starts both totals at positive zero.
For each retained positive-weight pair in encounter order it rounds the
weight-times-value product, adds and rounds that product into the weighted
total, then adds and rounds the weight into the weight total. Completion divides
the totals with one rounding. Each step uses nearest-even rounding. There is
no fused product/addition or rescaling. Results can depend on record order.
Arbitrary weight scaling is not promised to preserve every rounded bit.

A numerical capacity refusal with status 77 identifies a finite product that
overflows or rounds a nonzero value to zero, an overflowing finite weighted
total, an overflowing weight total, or an overflowing finite final division.
The check happens when the intermediate is formed, even if later records might
cancel it. Deliberate infinite value inputs are distinguished from finite
overflow. Finite subnormal intermediates, ordinary rounding and cancellation,
and final-result underflow remain permitted.

These limits can refuse finite inputs whose mathematical mean is representable.
Value 2 with weight `1e4932` refuses at its product. Value `1e-4000` with weight
`1e-4000` refuses when its nonzero product rounds to zero. Scaling weights in
advance may change the evaluation trace and rounded result.

Unsorted `wmean` retains two numeric totals per request, independent of input
record count. Full row context and other requested operations keep their own
storage policies. This is not a total-process-memory or performance comparison.
