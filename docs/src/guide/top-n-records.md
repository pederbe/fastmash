# Highest and lowest records

Fastmash supports Top-N selection from ordinary delimited text
or quoted CSV, across the whole dataset or separately within adjacent or sorted
Groups. Select one numeric Field with `top:N FIELD` for the highest
values or `bottom:N FIELD` for the lowest. N is mandatory, accepts leading
zeros, and must be an unsigned decimal integer from 1 through
18446744073709551615. The Field must be one positional or escaped named
selector, without lists, ranges or pairs. Named Fields require `-H` or
`--header-in`. Selection cannot be mixed with calculations or other Modes.

For example, given these tab-separated Records:

```text
A	8	first observation
B	10	second observation
C	10	third observation
D	9	fourth observation
```

`fastmash top:2 2` emits B then C with all their Fields.
`fastmash bottom:2 2` emits A then D. Output is in numeric rank order. Equal
values keep original input order, and earlier Records win ties at the cutoff.
Duplicates remain separate observations; ties never expand the cutoff beyond N.
Fewer eligible Records yield fewer output Records, without padding.

Ranking uses the active Numerical profile and Supported locale, including its
binary80 precision. Numbers that convert to the same value tie, regardless of
their spelling. Both zero signs tie. Infinities sort beyond finite values in
their numeric direction. Any unskipped NaN is an Error because it is unordered.
Only the ranking Field must be numeric; accompanying Fields are copied without
numeric checks.

With `--narm`, Records whose ranking Field is exactly `NA`, `N/A` or `NaN`, in
any letter case, are omitted. Matching does not trim whitespace. Empty or absent
Fields, malformed numbers, range failures, signed NaNs and NaN payloads remain
Errors. Empty and all-missing datasets emit no data Records. Every accepted
Record is checked, including late Records that cannot enter the selection.

Selection copies complete logical Fields in their original order, retaining
source numeric spelling, trailing empty Fields, ragged widths and arbitrary
accompanying bytes. It adds no score, rank or result column. Ordinary separators,
whitespace input, output delimiters, comment filtering and NUL-terminated Records
use the existing controls. Whitespace separator runs become the effective output
delimiter, and a final unterminated Record receives an output terminator.
Ordinary output does not escape embedded output delimiters or Record terminators
in copied Fields.
Use `--csv-out` to encode selected text Records as CSV, including sorted text
Groups. Use `--csv-in` to select decoded CSV Records with ordinary output, or
`--csv` for CSV input and output together. These exact-only switches are
idempotent and do not enable headers.

CSV input uses the existing strict comma/double-quote grammar, including doubled
quotes, multiline Fields, LF or CRLF Record endings, blank Records and a final
complete Record without a terminator. It does not trim values, strip a BOM,
transcode bytes or normalize decoded newlines. Ranking reads only the decoded
numeric Field, while ordinary text keeps its existing conversion across Field
boundaries. Every CSV Field is structurally validated, including accompanying
Fields on omitted Records and candidates that cannot win.

CSV output encodes each complete logical Field independently, preserving
numeric spelling, commas, quotes, CR/LF, NUL and non-UTF-8 bytes. It doubles
quotes, quotes empty Fields as `""`, and emits LF Record endings. Source quote
spelling and CRLF framing are not copied. Copied data remains complete even
where the separate header-label display convention stops at NUL.
`--full` is accepted with the same output and no deprecation warning. `-s`
without Grouping keys leaves this whole-dataset selection unchanged.

With `-g FIELD[,FIELD...]`, each consecutive Group has its own cutoff. Groups
appear in encounter order, and rank order applies within each Group. For
example, `fastmash -H -g category top:2 reading` selects the highest two
Records in each adjacent category, binding `category` and `reading` from the
Input header. A later repeated category remains a separate Group. Selected
Records retain every Field once, without an extra Grouping-key prefix.
Multiple keys and `-i` follow the existing Group equality and Supported locale
controls; no-key `-i` leaves numeric ranking unchanged. Empty key text is a key
value, while an absent required key is an Error. Every required key and Group
boundary is checked before `--narm` omission, so an all-missing intervening
Group still separates equal-key runs on either side.
CSV keys compare decoded Field values, so different quote spellings of the
same text belong to the same adjacent Group.

Add `-s` to prepare interleaved categories under the existing sorted Group
workflow, for example `fastmash -s -H -g category top:2 reading`. Groups then
follow the existing key order, with ranking applied inside each Group. Sorting
keeps complete Records and original input identity through memory and Spill,
so earlier source Records still win equal-rank ties. Sorting order and Group
equality retain their existing distinct meanings: whitespace key separators,
case controls, language-locale spellings and NUL-terminated input can affect
the prepared runs without introducing global key normalization. Selection uses
the existing native sort for text and decoded CSV sort for quoted CSV.
Pipe and regular-file input use the same selection route for each format.

For example, `fastmash --csv -s -H -g category top:2 reading` selects the
highest two Records per category from an interleaved CSV export. Sorting uses
decoded key bytes, so quoted and unquoted spellings of the same key sort
together. Each selected Record keeps its complete decoded Fields, including
multiline notes, through sorting and Spill; CSV output encodes them again.
Locale ordering still does not merge differently spelled keys that existing
Group equality treats separately. No global key normalization is introduced.

`--header-in` consumes the first accepted Record as the Input header and
excludes it from ranking. Names resolve to the first matching label when a
header contains duplicates. `--header-out` emits one copied-field Output header
for the whole Command, without operation wrappers or appended Group labels;
`-H` selects both controls. With an Input header, copied labels follow the
existing display convention, stopping at the first NUL byte. Without one,
labels are `field-1`, `field-2` and so on, using the first accepted source data
Record's width even when that Record is omitted or does not win. Later ragged
Records do not revise the header, and sorting does not change its source schema.
Positional ranking Fields may extend beyond
the Input header's labels when the data supplies them. Empty input invents no
header; header-only input with both controls and all-missing nonempty input can
emit a requested header without data. Name lookup fails before header output.

The entire input must be inspected before ungrouped data is emitted. Completed
adjacent Groups can emit as the next Group begins. A read failure or malformed
Record does not emit the incomplete current selection, but earlier Groups and
the Output header may remain. Output writes and finalization must succeed; a
later output failure can leave partial bytes.
Sorted grouping finishes input intake before emitting selected data; a read or
temporary-sort failure emits no incomplete sorted Group. Later ranking or key
Errors can leave already completed Groups. Diagnostics retain original accepted
Record numbers, including the Input header and excluding filtered comments.
CSV input Errors identify the original logical Record and its starting physical
line; syntax Errors also retain their detected location.

At most N candidate Records are retained for the current Group or whole dataset,
with storage growing as Records arrive and candidate state released between
Groups. A large requested N does not allocate N Records in advance. N limits
candidate count, not bytes or total process memory: large Records or a large N
can still exhaust memory. Checked resource failures refuse with status 77;
Fastmash does not truncate Fields, reduce N or approximate the winners.
Sorted grouping has a separate sort-memory target, controlled by
`FASTMASH_SORT_MEMORY_BYTES`, and Spills input when needed under the maintained
sort policy. Candidate retention remains bounded by N for the active Group
and does not Spill. Neither limit is a guarantee of total process memory.

CSV input conflicts with explicit text-input separators and
comment filtering. Any CSV format conflicts with NUL termination and `--vnlog`;
CSV output also conflicts with an explicit Output delimiter. Format conflicts
are Errors with status 1, independently of option order. CSV input with ordinary
output can use an explicit Output delimiter, subject to ordinary framing limits.
A named Field without `-H` or `--header-in` is a command Error.
Explicit result names, numeric presentation controls, Collapse delimiter,
Filler, random Seed, `--no-strict`, `--vnlog` and `--sort-cmd` are unsupported
for selection and refuse with status 77. Malformed options and conflicting CSV
controls retain Error status 1. Ordinary Commands keep their existing behavior.
