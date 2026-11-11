# Errors, exit statuses and resources

## Exit statuses

| Status | Meaning |
| --- | --- |
| `0` | Success |
| `1` | **Error**: a problem with the command or its input, such as an unknown option, a non-numeric value, a missing field or a write failure |
| `77` | **Refusal**: Fastmash deliberately declined to produce a result, because a feature or locale is unsupported or a checked resource limit was reached |
| `70` | **Internal failure**: Fastmash detected a violation of its numerical invariants. Please [report it](https://github.com/pederbe/fastmash/issues/new/choose) |

When one failure follows another, such as a failure to write the output after
an error, the status is the more serious one: 70 over 77 over 1. If a
diagnostic itself cannot be written, the status is 1. Internal numerical or
table-identity failures indicate a bug: please report them. The legacy
"internal error: input field too long" diagnostic instead identifies the
[numeric field-length limit](output.md#limits). `SIGABRT` can indicate a bug or
an allocation failure that the process could not report as a refusal.

Diagnostics go to standard error, in the form:

```text
fastmash: invalid numeric value in line 3 field 2: 'n/a'
```

Some are followed by a `hint:` line suggesting a fix.

## Find and fix the cause

The diagnostic identifies the first problem encountered. Check the named field,
record, source file or option before rerunning the command.

| Message or situation | What to check |
| --- | --- |
| `invalid numeric value` | Confirm that the selected field contains numbers and that the field separator is correct. If the first record names the fields, add `--header-in` |
| A requested field is missing | Field numbers start at 1. Tab is the default separator. Use `-W` for runs of spaces and tabs, `-t,` for literal commas, or `--csv` for quoted CSV |
| A header name is not found | Names are exact and case-sensitive. Check the spelling, separator and whether the first record really is a header |
| `invalid CSV` | Use double quotes around a complete field. Write a quote inside it as two double quotes. The reported record and physical line locate the problem |
| A CSV option conflicts with another option | Remove text-input controls such as `-t` or `-W` when reading CSV. CSV output chooses its own comma separator, so omit `--output-delimiter` |
| A weighted mean has no positive retained weight | At least one retained value/weight pair must have a positive weight. Check zero weights and pairs omitted by `--narm` |
| `sort temporary I/O error` | Check that `TMPDIR`, or `/tmp` when it is unset, exists, is writable and has free space. Choose another writable directory if needed |
| Memory allocation failed | Check available memory and process limits. Sorting can spill to disk, but retained samples and complete tables stay in memory |
| A read or write failed | Check the input/output paths, access permissions and free disk space. A closed or incorrectly opened standard stream must be fixed by the shell or program launching Fastmash |

`--narm` skips only whole-field `NA`, `N/A` and `NaN` tokens, ignoring case. It
does not skip arbitrary text, empty fields or absent fields. `--round` and
`--format` change display only and cannot repair a numerical-range refusal.

The warning about `-f/--full` with "non-linewise operations" concerns aggregate
operations such as `sum`. It does not stop the command or change its exit
status. Omit `--full` if you only need the calculated results. Per-row operations
such as `round` do not produce this warning.

## Always check the exit status

Fastmash writes results progressively through a small output buffer, so a
command that fails part-way may already have printed earlier groups. In scripts, check the exit
status rather than whether output appeared, and in pipelines enable
`set -o pipefail` so an earlier command's failure isn't hidden:

```sh
bash -c '
set -o pipefail
if ! fastmash -s -g 1 sum 2 < data.tsv > totals.tsv; then
  echo "summary failed" >&2
  exit 1
fi
'
```

Dataset comparison completes both input scans and all calculations before
writing its report, including the header. Output write failures can still leave
partial bytes. Table health likewise completes inspection before report emission;
`health ... validate` can emit a complete report and return 1 for declared
violations. Advisory findings alone do not make validation fail.

## Why refusals exist

Fastmash prefers a clear refusal to a doubtful answer. Examples:

- A non-integer percentile such as `perc:2.5`. GNU datamash 1.9 reads an
  unset parameter for it and reports a misleading error.
- Multiple key fields for `rmdup`, which GNU datamash 1.9 aborts on.
- An unsupported locale, where numbers might be read with the wrong decimal
  separator.
- A calculation that would need more memory than is available.

A refusal is not a claim that GNU datamash would reject the same input.

## Memory

Fastmash has no fixed limits on record length, field count, number of
operations or number of values. (The few fixed limits that remain are GNU
datamash's own, on `--format` strings, names in a command and, in one narrow
case, numeric fields; see [Numbers](output.md#limits).) Data storage grows with
checked allocations. If memory runs out, Fastmash refuses with status 77 where
it can. Process initialization, including arguments and configuration, uses
standard Rust allocation behavior and can terminate before reporting a refusal.
The operating system may also stop a process that exhausts memory. In those
cases, the shell or job runner reports the process's termination status.

What grows with the input:

- Operations that need every value (quantiles, dispersion, paired statistics,
  `unique`, `collapse`) keep the values of the current group.
- `rmdup`, `transpose` and `crosstab` keep their tables in memory.
- Top-N selection keeps at most N candidates for the active group or dataset;
  N limits record count, not bytes.
- Dataset comparison keeps keys, per-key calculation state and required samples
  in memory; these do not spill.
- Table health keeps field counters, widths, header labels and bounded examples
  in memory, with no fixed total-memory guarantee.
- `-s` keeps a sort buffer, spilling to disk beyond the chunk target. Sorted
  numerical jobs keep the original records until they are processed.

### Address-space limits

Some systems limit a process's address space rather than its memory, for
example `ulimit -v` or a batch scheduler's virtual-memory limit such as
`h_vmem`. The static Linux download uses musl and needs no glibc allocator
settings. If a sorted job refuses under the limit, try `OMP_NUM_THREADS=1`
to reduce worker memory (see [Large inputs](grouping.md#large-inputs)).

For glibc source builds, the C library's memory allocator gives each thread
that allocates an arena of its own, which reserves 64 MiB of address space,
more as it grows, and sorts in language locales, like the heavier
calculations in the C locales, run on up to eight threads.
Under such a limit, Fastmash therefore keeps the allocator to one arena for
each 512 MiB of the limit, from one (below 1 GiB) to eight. Threads that share
an arena wait for each other, so a sort that would also fit without the cap
can take somewhat longer. A number of arenas that `GLIBC_TUNABLES`
(`glibc.malloc.arena_max`) or `MALLOC_ARENA_MAX` sets is used instead, higher
or lower. If a sorted job still refuses under the limit:

- `GLIBC_TUNABLES=glibc.malloc.arena_max=1` keeps the allocator to one arena
  at a limit of 1 GiB or more too;
- `OMP_NUM_THREADS=1` sorts on one thread (see
  [Large inputs](grouping.md#large-inputs)).

## Disk

Sorting large inputs with `-s` writes temporary data to `TMPDIR` (default
`/tmp`), using anonymous files that the operating system removes automatically.
Where the filesystem has no anonymous files (`O_TMPFILE`), such as NFS or a
container's `/tmp` on Linux before 6.10, Fastmash creates private files with
random names and removes their names at once, so they also disappear when
Fastmash exits, including when it is interrupted. A sort that has to spill to
disk without a usable `TMPDIR` stops with "sort temporary I/O error" (status 1).

## Interruption

An interrupted command can leave partial output. Treat output as complete only
when the exit status is 0.
