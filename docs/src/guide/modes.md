# Per-row operations and modes

## Per-row operations

Per-row operations produce one output record for each input record. They can
be combined with each other and with `cut`, but not with summary operations
in the same command.

### Numbers

| Operation | Result |
| --- | --- |
| `round` | Nearest integer, halves away from zero |
| `floor`, `ceil` | Round down, round up |
| `trunc` | Round toward zero |
| `frac` | Fractional part, keeping the sign |
| `bin[:WIDTH]` | Lower bound of the bucket of width `WIDTH` (default 100) containing the value |
| `strbin[:COUNT]` | Hash the field's bytes into a bucket number from 0 to `COUNT`−1 (default 10) |
| `getnum[:TYPE]` | Extract the first number found in a text field |

`getnum` types: `n` digits, `i` signed integer, `p` digits and dots (the
default), `d` signed digits and dots, `h` hexadecimal, `o` octal. Text with no
number gives `0`.

```console
$ printf 'item-42.5kg\n' | fastmash getnum 1
42.5
$ printf '17\n183\n' | fastmash bin:50 1
0
150
```

### Text

| Operation | Result |
| --- | --- |
| `base64`, `debase64` | Encode or decode the field as Base64 |
| `md5`, `sha1`, `sha224`, `sha256`, `sha384`, `sha512` | Lowercase hexadecimal checksum of the field's bytes |
| `dirname`, `basename` | Directory part, or last component, of a path |
| `extname`, `barename` | File extension, or file name without it |

MD5 and SHA-1 are provided for compatibility and checksums, not for security.

Path operations work on the text alone; they don't touch the filesystem.

## Field modes

| Mode | Effect |
| --- | --- |
| `cut FIELDS`, `echo FIELDS` | Print the selected fields of each record |
| `reverse` | Reverse the order of fields in each record |
| `noop`, `nop` | Read the input and print nothing (useful for validation); with `--full`, print each record |

```console
$ printf 'a\tb\tc\n' | fastmash cut 3,1
c	a
$ printf 'a\tb\tc\n' | fastmash reverse
c	b	a
```

## Table modes

| Mode | Effect |
| --- | --- |
| `transpose` | Swap rows and columns |
| `check [N lines] [N fields]` | Verify every record has the same number of fields, and optionally the expected counts |
| `rmdup FIELD`, `dedup FIELD` | Keep the first record for each distinct value of one key field |

```console
$ printf 'a\tb\n1\t2\n' | fastmash transpose
a	1
b	2
$ printf 'x\t1\ny\t2\nx\t3\n' | fastmash rmdup 1
x	1
y	2
```

`transpose` and `reverse` require every record to have the same number of
fields. With `--no-strict` they accept ragged input, and `transpose` fills
missing cells with the `--filler` text (default `N/A`).

`check` prints a short confirmation and exits with status 0 when the table is
consistent, or reports the first inconsistent record and exits with status 1.
