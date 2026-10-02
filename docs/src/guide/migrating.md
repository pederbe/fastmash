# Migrating from GNU datamash

Fastmash accepts GNU datamash 1.9's command language. In most scripts,
switching is a matter of replacing `datamash` with `fastmash`.

## A safe way to switch

1. [Install Fastmash](../install.md) alongside `datamash`. Don't alias or
   replace `datamash`; keep both available while you compare.
2. Run your existing command with both programs on the same input and with the
   same locale settings, and compare the output and the exit status:

   ```sh
   export LC_ALL=C.UTF-8
   datamash -s -g 1 sum 2 < data.tsv > gnu.out;  echo "datamash: $?"
   fastmash -s -g 1 sum 2 < data.tsv > fast.out; echo "fastmash: $?"
   cmp gnu.out fast.out && echo identical
   ```

3. If the outputs differ, check [Differences from GNU datamash](differences.md).
   If the difference isn't listed there, please
   [report it](https://github.com/pederbe/fastmash/issues/new/choose).
4. Change the script, and keep checking exit statuses as before.

## What works unchanged

- Every GNU datamash 1.9 operation, mode and alias, except the explicit `line`
  mode (write `fastmash base64 1`, not `fastmash line base64 1`)
- Field numbers, lists, ranges, pairs and header names
- All options except `--sort-cmd`, including short-option clusters (`-sg1`),
  abbreviated long options (`--whitesp`), options after operands, and `--`
- `POSIXLY_CORRECT` handling
- Output layout, header names and the `--full` output
- Exit statuses 0 and 1 for success and ordinary errors

## What to check

**Locale settings.** Fastmash has the glibc 2.43 locale rules built in:
numbers work in `C`, `POSIX` and every glibc locale with a one-byte decimal
separator, `@` modifier locales included, and sorting in `C`, `POSIX`,
`C.UTF-8` and 194 language locales checked against glibc. A name glibc has no
locale for behaves as `C`, as in GNU datamash. Commands that read or print
numbers exit with status 77 in `ps_AF`, and in a character set other than
UTF-8 whose thousands separator is not ASCII (such as `fr_FR` in Latin-1);
sorted commands do outside the sorting locales. GNU datamash falls back to `C`
where a locale is not installed, while Fastmash does not. Set
`LC_ALL=C.UTF-8` (or `C`) in scripts. In language locales, Fastmash orders
sorted keys as GNU `sort` does under glibc, letters and digits by Unicode
collation and punctuation, symbols and spaces by glibc's own table; keys that
differ only in punctuation next to digits (`12-A` and `1-2A`), vulgar fractions
such as `¼` and some letters from outside the language's alphabet can still
sort differently. [Locales](output.md#locales).

**Last digits of results.** Fastmash's portable arithmetic can differ from a
local GNU build in the last printed digit of some results, or in the sign of
a `nan`. If a script compares numbers textually, compare with a tolerance or
fix the precision with `--round`. [Numbers](differences.md#numbers).

**Exit status 77.** Fastmash refuses some inputs that GNU datamash handles
unsafely, such as `perc:2.5` or `rmdup` with several keys. A script that
treats any nonzero status as failure needs no change.

**`--sort-cmd`.** Not supported. To use a different sorter, sort the input
first and leave out `-s`:

```sh
LC_ALL=C sort -s -t "$(printf '\t')" -k1,1 data.tsv | fastmash -g 1 sum 2
```

**Test suites.** GNU datamash's hidden testing options (`---print-inf`,
`---print-nan`, `---print-progname`, `---rmdup-test`) are not provided.

## Remember

`-t,` splits on commas and does not parse quoted CSV, in both programs.
`spearson` is the sample Pearson correlation, in both programs.
