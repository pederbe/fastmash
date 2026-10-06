# 0008: Group sorted input by hash, and sort again when unsure

Status: accepted.

GNU datamash sorts every record for `-s`, with a stable `sort -s`, so each of
its groups is exactly the records with one key, in input order. Typical inputs
have far fewer groups than records. When standard input is a regular file,
Fastmash therefore collects each key's records into that group's own
operation state as they arrive and sorts only the keys, reusing the sort's key
order and the group writer, so the output is the sort's. In a language
locale, it does the same for piped input, holding what it reads in memory so
that the input can be read again: the language-locale sort keeps whole
records, so the held input takes about as much memory as the sort would.

Where that could give a different result, Fastmash does not try to reproduce
the sort's behaviour: before writing anything, it reads the input again from
its start and sorts it, in process or through the system sort, as the job
would have been sorted without hash grouping. That happens on a missing or
NUL key, a key the collation refuses, any operation error (so error messages
and line numbers are exactly the sort's), too few records per group, or too
little memory. With `-W`, GNU's sort keys keep the blanks before each field,
which groups ignore, so a group is keyed by its fields with those blanks, and
two keys that differ only in them may be one group or two: Fastmash gives up
there, and on a newline in a `-z` record, which the sort takes for a blank.
In a language locale, glibc ties some keys spelled differently at every
level (`й`, and `и` with a combining breve), which the stable sort keeps in
input order, a group per run of each spelling: Fastmash gives up when a new
key ties so with one already collected.
`--vnlog` and `rand`, whose results depend on more than each group's own
records, always sort.

**Considered:** turning the collected groups into pre-aggregated sort records
when hashing stops paying, which reads the input once and also covers pipes,
but needs a serialized form of every operation's state and must reproduce the
sort's first error from stored state; its surface for mistakes is much
larger. Holding piped input in every locale: in the `C` locales the held
input takes several times the memory of a sort that keeps only the selected
fields, and some jobs are slower (`FASTMASH_PIPE_GROUPING=hash` still does
it). Writing held input beyond a budget to a temporary file: no gain where
the temporary directory is in memory.

**Consequences:** sorted jobs on files are usually several times faster and
use memory for the groups only, and piped jobs in a language locale three to
four times faster with the sort's memory; piped input in the `C` locales
keeps the sort, so `< file` and `cat file |` can differ in time and memory,
not in output. A piped job that gives up late takes about a third longer
than the sort alone. A job that restarts
reads its input twice, and its output is that of the second reading, so a file
that changes while it is read gives the result of reading it later than a
program that reads once would. `FASTMASH_GROUPING=sort` always sorts.
