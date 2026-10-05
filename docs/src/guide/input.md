# Fields and input

## Records and fields

Ordinary delimited-text input is a sequence of **records**, each ended by a
newline (or by a NUL byte with `-z`). The last record may omit its terminator.
Each record is split into **fields**, by default at every tab.

Carriage returns are kept as data in ordinary text, so files with Windows line
endings may need converting first (for example with `tr -d '\r'`). Explicit
[quoted CSV input](csv-and-result-names.md) accepts LF and CRLF record endings
and permits line breaks inside quoted fields.

## Selecting fields

Operations take a **selector** naming the fields to use:

| Selector | Meaning |
| --- | --- |
| `3` | Field 3 (fields are numbered from 1) |
| `1,4` | Fields 1 and 4 |
| `2-5` | Fields 2 through 5 |
| `1,3-4` | Field 1, then fields 3 and 4 |
| `price` | The field named `price` in the input header |
| `2:3` | The pair of fields 2 and 3, for paired statistics |
| `1:2,3:4` | Two pairs |

A list or range applies the operation to each field in turn, producing one
result per field. Repeating a field repeats its result. Ranges must be
ascending, and a range cannot be combined with pairs.

## Field separators

| Option | Effect |
| --- | --- |
| *(default)* | Fields are separated by a tab |
| `-t X`, `--field-separator=X` | Fields are separated by the single byte `X` |
| `-W`, `--whitespace` | Fields are separated by runs of spaces and tabs; leading whitespace is skipped |
| `--output-delimiter=X` | Results are separated by `X` |

The output separator follows the input separator unless `--output-delimiter`
is given. With `-W`, the output separator is a tab.

```console
$ printf 'a;1\nb;2\n' | fastmash -t ';' sum 2
3
```

`-t,` does not understand CSV quoting: a comma inside a quoted field still
splits it. Use `--csv-in` to read quoted CSV, or `--csv` for both input and
output. Format switches do not imply headers. CSV input conflicts with `-t`,
`-W` and `-C`; either CSV direction conflicts with `-z` and `--vnlog`.
See [Quoted CSV and result names](csv-and-result-names.md).

## Headers

| Option | Effect |
| --- | --- |
| `--header-in` | The first record names the fields; it is not treated as data |
| `--header-out` | Print a header line naming each result |
| `-H`, `--headers` | Both of the above |

With an input header, selectors can use field names. Names are exact and
case-sensitive; if two fields share a name, the first one is used.

```console
$ printf 'price\tunits\n3\t2\n5\t4\n' | fastmash -H sum units mean price
sum(units)	mean(price)
6	4
```

## NUL-terminated records

`-z` ends records with NUL bytes instead of newlines, for input from tools
such as `find -print0`. Output records are NUL-terminated too.

## Comments

`-C` (`--skip-comments`) skips records whose first non-blank character is `#`
or `;`. Comment markers later in a record are data. Skipped lines do not count
towards line numbers in error messages.

`--vnlog` reads [vnlog](https://github.com/dkogan/vnlog) files: whitespace-separated
columns with a `# name name …` header line and `#` comments. In vnlog data,
everything after a `#` is ignored, even inside a field, and output uses spaces
with a `# ` header.

## Missing values

`--narm` skips fields whose exact text is `NA`, `N/A` or `NaN`, in any letter
case, for every operation:

```console
$ printf '1\nNA\n3\n' | fastmash --narm sum 1 count 1
4	2
```

Only these exact words count as missing. An empty field is not missing: a
numerical operation reports it as an invalid value. A record that lacks the
selected field is an error.

For paired operations, each side skips its missing values independently, so
the remaining values may pair up across different records.

## Other options

| Option | Effect |
| --- | --- |
| `-f`, `--full` | Print a complete input record before each group's results. The record is the group's first, unless `first`, `last`, `rand` or an extremum selects another. GNU datamash's deprecation warning for this option is kept |
| `-S N`, `--seed N` | Make `rand` reproducible. Without a seed, `rand` is not reproducible. Its selection, like GNU datamash's, is slightly biased and not cryptographic |
| `--no-strict`, `--filler X` | Accept ragged rows and set the missing-cell text for `transpose` and `crosstab`. They don't fill in missing fields for summary operations |
| `--` | End option processing |

Output headers name each result after its operation and field, such as
`sum(price)`, and grouping keys as `GroupBy(site)`.

Options can appear after operations, and short options can be combined
(`-sg1`) as in GNU datamash. If the `POSIXLY_CORRECT` environment variable is
set, option processing stops at the first operation.
