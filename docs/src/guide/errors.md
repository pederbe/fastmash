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
diagnostic itself cannot be written, the status is 1. A message containing
"internal", or a process killed by `SIGABRT`, also indicates a bug: please
report it.

Diagnostics go to standard error, in the form:

```text
fastmash: invalid numeric value in line 3 field 2: 'n/a'
```

Some are followed by a `hint:` line suggesting a fix.

## Always check the exit status

Fastmash writes results progressively through a small output buffer, so a
command that fails part-way may already have printed earlier groups. In scripts, check the exit
status rather than whether output appeared, and in pipelines enable
`set -o pipefail` so an earlier command's failure isn't hidden:

```sh
set -o pipefail
if ! fastmash -s -g 1 sum 2 < data.tsv > totals.tsv; then
  echo "summary failed" >&2
  exit 1
fi
```

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
case, numeric fields; see [Numbers](output.md#limits).) Storage grows with the job and every
allocation is checked; if memory runs out, Fastmash refuses with status 77
where it can. The operating system may still stop a process that exhausts
memory before Fastmash can report it. When it stops the system `sort` that
some sorted jobs use (see [Large inputs](grouping.md#large-inputs)), the job
fails with status 1, naming the signal before "read error (on close)" as GNU
datamash does on Debian and Ubuntu:

```text
Killed
fastmash: read error (on close)
```

What grows with the input:

- Operations that need every value (quantiles, dispersion, paired statistics,
  `unique`, `collapse`) keep the values of the current group.
- `rmdup`, `transpose` and `crosstab` keep their tables in memory.
- `-s` keeps a sort buffer, spilling to disk beyond the chunk target. Sorted
  numerical jobs keep the original records until they are processed.

### Address-space limits

Some systems limit a process's address space rather than its memory, for
example `ulimit -v` or a batch scheduler's virtual-memory limit such as
`h_vmem`. The C library's memory allocator (glibc `malloc`) gives each thread
that allocates an arena of its own, which reserves 64 MiB of address space,
more as it grows, and sorts in language locales run on up to eight threads.
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
Fastmash exits. Sorted jobs that use the system `sort` (see
[Grouping and sorting](grouping.md#large-inputs)) write to a private
directory in `TMPDIR`, which the sort supervisor creates when the job starts
and removes when it ends, even if Fastmash is interrupted. Only killing both
processes at once, for example with `kill -9` on the whole process group, can
leave it behind. Where `TMPDIR` is not a writable directory, these jobs sort
inside Fastmash instead. A sort that has to spill to disk without a usable
`TMPDIR` stops with "sort temporary I/O error" (status 1).

## Interruption

An interrupted command can leave partial output. Treat output as complete only
when the exit status is 0.
