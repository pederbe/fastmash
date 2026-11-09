# Terminal presentation

Fastmash makes terminal results easier to scan with purple headings, blue result
values, green completion marks, amber advisories and red failures. Secondary
metadata uses gray. All styles use your terminal's default background.

Ordinary calculations and Top-N selection show labeled, aligned fields and a
`✔ Completed` footer after successful processing. Headers appear even without
`--header-out`. Automatic headers use positional labels such as `sum(field-1)`.
Use `-H` or `--header-out` to request labels from an input header. Results still
stream as they become available. Alignment uses at most 32 columns per field,
while labels and values are always printed in full.
A failure can leave earlier results visible, followed by a diagnostic. It does
not receive a completion mark. Numerical values and their formatting are unchanged.

Readable table-health reports put the outcome and findings ahead of input
metadata. `⚠` identifies advisory findings, and `✗` identifies violations of
supplied expectations. Successful validation uses `✔` and explicitly refers to
the supplied expectations. Inferred findings remain advisory even when validation
passes. Field counts, examples, omissions and interpretation details remain in
the report.

## Files, pipes and explicit formats

Output sent to a file or another program keeps its existing plain format,
including readable health reports. It gains no headings, padding, symbols or
color codes, even with `--color=always`. Examples that write a report file show
its data format.

Explicit CSV output, NUL-terminated output, an explicit output delimiter, vnlog,
crosstab, dataset comparison, field and table transformations, health TSV and
version output retain their data formats on terminals too. This lets you inspect
those formats directly without a presentation layer changing their structure.

## Controls

```sh
fastmash --color=auto sum 1
fastmash --no-color sum 1
fastmash --color=always --help
```

`auto` is the default. Each destination is checked separately. Redirecting stdout
does not disable colored diagnostics on a terminal. `TERM=dumb`, a nonempty
`NO_COLOR`, or failed terminal detection disables automatic styling. An empty
`NO_COLOR` permits it. `never` and `--no-color` disable the terminal presentation,
including bold, added headers and completion marks.

`always` overrides environment suppression on a terminal. It can also force
color in redirected help and diagnostics, but never decorates redirected results
or health reports. Diagnostics style only their program prefix and failure mark,
leaving the explanation, hints and input bytes in the default foreground.

Repeated controls follow the last one reached by ordinary option scanning. `--`
ends scanning, and `POSIXLY_CORRECT` stops it at the first operand. Help and version
stop scanning immediately, so put color controls before them.
