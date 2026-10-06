# Table health reports

`health` inspects the entire accepted input and reports inconsistent record
widths, blank or duplicated header labels, and absent, empty or candidate-missing
field values. It counts lexical value types and shows inferred expectations with
their supporting counts. Inspection leaves your input unchanged.

```sh
fastmash health < table.tsv
fastmash --header-in health examples 2 < table.tsv
fastmash -t , -C health examples 0 < table.txt
fastmash --header-in health type price number nonmissing id width 3 validate < table.tsv
fastmash --header-in health tsv type price number nonmissing id validate < table.tsv > health.tsv
```

Readable reports use bold headings, yellow `advisory` markers and red `violation`
markers. Finding categories, their inferred or declared basis, counts, Field
names, observations and examples keep the default foreground. The wording carries
the same meaning without color. An advisory can appear in a successful
`validate` report; only violations of supplied Health expectations make validation
fail.

Styling is automatic on eligible stdout terminals. Pipes and redirected reports
stay plain by default; `TERM=dumb` and a nonempty `NO_COLOR` also suppress automatic
styling. Use `--color=always` to force it, or `--color=never` or `--no-color` to
suppress every style, including bold. These global controls work with `health`;
see [terminal color](terminal-color.md) for detection, precedence and examples.
Styles reset immediately after each heading or marker. Removing the generated
style sequences reproduces the complete plain report, including escaped bytes
and example limits.

The Table health report states the data-record, header-record and accepted-record
counts, effective input settings, numeric decimal separator, baseline width,
maximum data-record width and example limits. `width N` supplies the baseline and
checks both data records and an existing Input header. N can be zero; width does
not declare individual fields. Without a supplied width, an Input header supplies
labels and the baseline rather than data values. Without a header, the baseline
is the most frequent data-record width across the complete input, choosing the
smaller width on a tie. With neither a supplied width, data nor a header, the
baseline is unavailable.

`width_mismatch` counts data records that differ from the selected baseline.
For a supplied width it is a declared violation; otherwise it is advisory.
`header_width_mismatch` separately counts an Input header disagreeing with a
supplied width, including when there are no data records.
`blank_header` identifies each empty header position. `duplicate_header` identifies
every position sharing a nonempty exact-byte label. These findings are advisory.
A complete report succeeds even when findings exist. Record and header-field
counts and cell counts can overlap; adding them does not give a count of distinct
bad records.

Every field appearing in the Input header, a data record or a positional
declaration has `absent_values`, `empty_values`, `missing_values`, `integer_values`,
`other_number_values` and `text_values` counters, including zero counts. The six
categories partition the data-record count for each tracked field. An absent
field is a position beyond a record's width; an empty field is present with
zero-length bytes. `missing_values` counts exact whole-field `NA`, `N/A` and `NaN`
tokens in any letter case, without trimming. Whitespace-only values and tokens
with extra characters are separate values.

An integer encoding allows the numeric grammar's leading ASCII whitespace, an
optional sign and decimal digits consuming the whole field. Fractional, exponent
and hexadecimal encodings are other numbers even when their mathematical value
would be integral. Numbers must consume the whole field under the effective
locale's numeric grammar, including recognized infinity and NaN encodings such
as `-inf` and `nan(payload)`. A numeric prefix followed by extra bytes, trailing
whitespace or an embedded NUL is text. Classification uses syntax, without
conversion, rounding or arithmetic magnitude limits.

Absent, empty and candidate-missing values do not participate in inference.
If numeric values outnumber text, the inferred Health expectation is `integer`
when all numbers have integer encodings, otherwise `number`. If text outnumbers
numbers, it is `text`. A tie or no eligible values leaves it `undetermined`.
The report labels these expectations as inferred and shows their counts;
undetermined is an observation. These guesses cannot establish the intended
meaning of an identifier or distinguish a legitimate code from a missing token.

Positive counters produce `absent_field`, `empty_field` and `missing_indicator`
findings in that order, each counting data-record cells at its field position.
These are observations: an NA-like token can be a legitimate code. Inspection
does not change values or remove records. The Input header is excluded from cell
counts, and header-only input has zero observations. Fields appearing late include
earlier absent positions; positions outside the header have an empty label.

`mixed_types` follows the missingness findings and counts one field whenever
numeric and text encodings coexist. `type_mismatch` follows it and counts text
cells incompatible with an inferred integer or number expectation. Inferred text
does not reject numbers, and mixed integer/fractional numbers can share a number
expectation. Both findings remain advisory, including an inconclusive mixed-type
observation when numeric and text counts tie.

`type FIELD integer`, `type FIELD number` and `type FIELD text` supply a known
Health expectation, replacing inference for that field. Integer and number use
the lexical rules above; text permits arbitrary field bytes. Type checks exempt
absent, empty and candidate-missing values. A supplied integer rule rejects other
numeric encodings as well as text, even when their mathematical value is integral.
Mismatches are declared `type_mismatch` violations. The field's inferred mismatch
is suppressed, while its counters and `mixed_types` observation remain visible.

`required FIELD` rejects absent and empty cells. `nonmissing FIELD` additionally
rejects candidate-missing indicators and works without `required`. When both are
supplied, nonmissing is the one effective presence rule. One declared
`presence_mismatch` finding per field counts failed cells without duplicating
the rules. A literal `NA` code can pass required/text checks and fail nonmissing.

FIELD follows the existing single-selector spelling, without lists, ranges or
pairs. In ordinary input, bare positive decimal integers are 1-based positions;
identifiers select exact Input-header names. Existing backslash escaping permits
special bytes and numeric names, such as `\01` for a header named `01`.
Under vnlog, numeric selectors remain names and never fall back to positions.
Named rules require an Input header and a unique full-byte match. Missing and
duplicated names are command errors before data processing or report output.
Ordinary positions remain usable with duplicated headers. Names and positions
resolving to the same field share their declarations and conflict checks.
Never-observed positional declarations are tracked without inventing intervening
fields or allocating through the largest declared position.

All controls may occur in any order. Identical declarations are idempotent;
conflicting types, widths or example limits are command errors. Repeated
`validate` switches are harmless. `validate` requires at least one supplied
type, presence or width rule. It emits the same complete report as ordinary
inspection and returns status 1 for declared violations, otherwise 0.
Observations and inferred suspicions alone do not fail validation. Empty input
has zero data records; header-only input has no per-cell or data-width violations,
while a supplied width still checks its header. There is no minimum-record rule.

`examples N` retains the earliest N examples per finding, with three by default.
Mixed types reserve the earliest numeric and earliest text witnesses when N is at
least two, then fill remaining slots with the earliest unused eligible cells.
With N equal to one, they retain the earliest eligible cell. Examples are printed
in input order. Mixed-type omissions count eligible numeric/text cells minus
retained examples, separately from the finding's count of one field.
N is a nonnegative decimal integer; zero suppresses examples. Repeating the same
limit is harmless, while conflicting limits are command errors. Each sample
retains at most 128 raw bytes. The report discloses retained and omitted examples
and marks truncated samples. Header labels remain complete metadata.

Locations are 1-based accepted-record ordinals, including the Input header and
excluding filtered comments and annotations. They are not physical line numbers.
Header findings include their field position; width samples contain the raw
record without its terminator. Header-width samples preserve the raw accepted
header, including vnlog's prefix and trailing blanks, within the same byte limit.
Field observations use their accepted-record ordinal and field position;
absent and empty samples contain no bytes and state
which condition was observed. Backslash, TAB, LF, CR and NUL are escaped as
`\\`, `\t`, `\n`, `\r` and `\0`. Other bytes outside printable ASCII use
`\xHH` with uppercase hexadecimal digits. This keeps binary bytes visible.

Input follows the existing Record intake and field separation: TAB by default,
literal `-t` separators, `-W` whitespace, `-z` NUL terminators, `-C` comment
filtering, Input headers and vnlog. `-H`/`--headers` contributes its Input-header
meaning; the report supplies its own headings. Empty records can have zero
fields, trailing empty fields are retained, and a final unterminated record is
accepted. Pipes and regular files produce the same report for the same input.

Sorting, grouping, full rows, numerical formatting, missing-value removal,
standalone output headers, result names and result delimiters are calculation
controls and fail in `health`. Quoted CSV input and output remain unavailable.
Unknown or incomplete controls are command errors.

## TSV for scripts

`tsv` selects the same collected report in schema version 1. Readable output is
the default. The switch can occur anywhere among health controls, and repeating
it is harmless. Validation emits identical report bytes with or without
`validate`; declared violations determine its exit status.
TSV stays plain under every color control, including `--color=always` on a
terminal, so scripts receive the same schema and data bytes.

The mandatory header has 13 columns in this order:

```text
row_kind	category	basis	field_index	field_name	count	count_unit	record_index	expected	observed	sample	sample_truncated	examples_omitted
```

Report records always use TAB separators and LF endings, including with `-z`
input. Inapplicable cells are empty. Zero counters are `0`, integers use unsigned
decimal spelling and field positions are 1-based. String cells use the byte
escapes described above; decoding them recovers labels and retained samples
without confusing literal escape-looking text with byte escapes. Field names
retain the complete header bytes; positions beyond the header have an empty name.

The report begins with exactly these summary rows, in this order. Cells not
listed are empty.

| Category | Populated cells |
| --- | --- |
| schema_version | observed `1` |
| data_records | basis `observation`; count; count_unit `data_records` |
| header_records | basis `observation`; count; count_unit `header_records` |
| accepted_records | basis `observation`; count; count_unit `records` |
| baseline_width | basis `declared` or `inferred`; count; count_unit `fields`; all three empty when unavailable |
| baseline_width_source | observed `declared`, `header`, `modal` or `unavailable` |
| maximum_observed_width | basis `observation`; count; count_unit `fields`; count empty with no data records |
| separator_kind | observed `literal` or `whitespace` |
| separator | observed literal separator bytes, empty for whitespace separation |
| record_terminator | observed `lf` or `nul` |
| numeric_decimal_separator | observed effective locale's decimal-separator byte |
| input_header | observed `yes` or `no` for the selected setting |
| comment_filter | observed `none`, `comments` or `vnlog` |
| vnlog | observed `yes` or `no` |
| record_numbering | observed `accepted_records_1based` |
| example_limit | basis `observation`; count; count_unit `examples` |
| sample_byte_limit | basis `observation`; count `128`; count_unit `bytes` |

Next, each tracked field in ascending position has six `field` rows in order:
`absent_values`, `empty_values`, `missing_values`, `integer_values`,
`other_number_values` and `text_values`. Each repeats field_index and field_name,
with basis `observation`, count and count_unit `cells`. These counters partition
the data records exactly, including zeros. A `type_expectation` row follows with
basis `declared` or `inferred` and expected `integer`, `number` or `text`;
undetermined instead has basis `observation` and expected `undetermined`.
A supplied presence rule adds one `presence_requirement` row with basis
`declared` and expected `required` or `nonmissing`.

Positive `finding` rows follow in this category order, then basis order
observation/inferred/declared, then ascending field position:

| Category | Basis | count_unit | expected |
| --- | --- | --- | --- |
| header_width_mismatch | declared | header_records | declared width |
| width_mismatch | inferred or declared | data_records | baseline width |
| blank_header | observation | header_fields | empty |
| duplicate_header | observation | header_fields | empty |
| absent_field | observation | cells | empty |
| empty_field | observation | cells | empty |
| missing_indicator | observation | cells | empty |
| mixed_types | observation | fields | empty |
| type_mismatch | inferred or declared | cells | integer or number |
| presence_mismatch | declared | cells | required or nonmissing |

Finding rows populate basis, applicable field identifiers, count, count_unit,
expected and examples_omitted. Their record_index, observed, sample and
sample_truncated are empty. Width findings have no field identifiers.
Do not add overlapping findings into a count of distinct bad records.

Each finding is immediately followed by its retained `example` rows in input
order. They repeat category, basis, applicable field identifiers and expected,
then populate record_index, observed, sample and sample_truncated (`yes` or `no`).
Their count, count_unit and examples_omitted are empty. Width observed values
are field counts, with a raw-record sample. Header observations use `blank` or
`duplicate`, with a header-field sample. Field and presence observations use
`absent`, `empty` or `missing_indicator`; absent/empty samples are empty.
Mixed/type examples use `integer`, `other_number` or `text`.
Samples retain at most 128 raw bytes, independently of the full scan and labels.
Mixed-type omissions count eligible cells rather than the finding's one field.
No other row categories occur in schema version 1; future schema changes must
identify their version.

Field counters, width summaries, header bytes and bounded examples remain in
memory. Storage can grow with field count, distinct widths and the chosen example
limit; there is no fixed memory guarantee or spill backend. Checked allocation
or counter capacity failures refuse the command before report emission. A read
failure prevents a report, and a failed report write returns failure. No numerical arithmetic is
performed during inspection.
