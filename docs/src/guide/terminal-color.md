# Terminal color

Fastmash uses a small terminal palette to make human-readable output easier to
scan. Help headings use bold default foreground; Commands, options and Operation
names use cyan. Advisory findings use yellow. Errors, Refusals, Internal failures
and violations of supplied Health expectations use red. Descriptions, values and
examples keep the default foreground. Every style uses the terminal's default
background, and wording carries the meaning when styling is disabled.

The controls select whether eligible presentation is styled:

```sh
fastmash --color=auto --help
fastmash --color always --help
fastmash --no-color --help
fastmash --color=never --help
```

`auto` is the default. Each destination is checked separately: redirecting stdout
does not disable eligible stderr styling, and redirecting stderr does not disable
eligible stdout styling. Redirected output, failed terminal detection, `TERM=dumb`
and a nonempty `NO_COLOR` disable automatic styling, including bold. An empty
`NO_COLOR` permits automatic styling. `always` overrides these checks, including
for redirected presentation. `never` and `--no-color` suppress every generated
style.

Fastmash-authored failure diagnostics style only the existing program prefix,
such as `fastmash:`, in red. The prefix resets before the diagnostic body, so
multiline messages, hints and input bytes retain their default foreground.
Errors, Refusals and Internal failures share this failure role while keeping
their distinct text and exit statuses. Stderr eligibility controls the prefix
independently of stdout; a calculation piped to another program can still show
colored diagnostics in a terminal. Diagnostics forwarded from child programs
retain their existing bytes. Styling uses fixed sequences and does not require
allocating a new diagnostic message, including the low-memory fallback.
If argument collection fails before option scanning begins, no invocation control
has taken effect; that early fallback uses the default automatic stderr policy.

Readable Table health reports style their existing headings in bold, `advisory`
markers in yellow and declared `violation` markers in red. A finding's structured
basis selects its marker; labels or samples containing those words do not receive
color. Each heading and marker resets immediately, leaving counts, observations,
Field names and examples plain. Color preserves the report contents and validation
status: advisory findings can remain yellow when `validate` succeeds. See
[Table health reports](table-health.md) for the distinction between inference and
supplied Health expectations.

Only help, Fastmash's failure prefix and readable Table health reports are eligible
presentation surfaces. Calculation results, Dataset comparison reports, copied
Records, CSV, health TSV and version output retain their original bytes even with
`--color=always`. Color never decorates data or changes exit status.

Color controls require their exact spellings. Repeated controls follow the last
one reached by ordinary option scanning. As with existing options, `--` ends
scanning and `POSIXLY_CORRECT` stops it at the first operand. Help and version
terminate scanning immediately, so place color controls before them:

```sh
fastmash --no-color --color=always --help
fastmash --color=always --help > help.txt
```

The palette and stream handling follow the
[CLI Guidelines](https://clig.dev/#output), and environment suppression follows
the [NO_COLOR convention](https://no-color.org/), with explicit invocation controls
taking precedence. Fastmash also suppresses bold whenever styling is disabled.
