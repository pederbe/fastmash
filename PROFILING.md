# Profiling complete commands

Use hotpath-rs for local diagnosis of startup, decimal calculations, grouping
and spill. Ordinary builds have no profiling dependency or instrumentation.

The integration follows upstream [setup](https://hotpath.rs/),
[static profiling modes](https://hotpath.rs/profiling_modes),
[allocation profiling](https://hotpath.rs/functions) and
[CPU profiling](https://hotpath.rs/cpu_profiling). It uses a guard whose lifetime
includes command parsing, execution and stream completion. Controlled errors
and refusals flush reports before the process exits. Forced termination and
aborts cannot promise a final report.

## Build and capture

Use the latest stable Rust for ordinary builds and profiling.
Reuse installed tools and the normal build cache. These commands run from the
repository root on Linux or macOS 15 or later.

```sh
cargo +stable build --profile profiling --features hotpath-alloc --bin fastmash --example profile-jobs
cargo +stable run --profile profiling --features hotpath-alloc --example profile-jobs -- /var/tmp/fastmash-profiling
```

Choose a fresh destination. The Rust example creates repeatable datasets and
runs complete commands with exact expected stdout, stderr and exit statuses.
It covers a small sum, decimal accumulation, unsorted grouping, forced native
spill, a command error and a refusal. It retains inputs, command streams,
statuses, binary identity and JSON reports in that destination. The spill job
uses a one-byte sort-memory target and verifies that private spill runs were
opened. This target forces a route for diagnosis, and is not a realistic
performance comparison.

The decimal job sums ten million `0.125` records. Each value is one eighth, so
the independently known exact result is `1250000`. The larger workload gives
the CPU sampler more time to attach before the command finishes.

The `language-cache` job counts 8,192 records in 16 groups with repeated
195-byte accented keys in `en_US.UTF-8`. It checks count 512 and RMS 1 for each
group. It uses one projection thread so allocation reports include all key
construction. Capture allocation counts with:

```sh
HOTPATH_ALLOC_METRIC=count cargo +stable run --profile profiling --features hotpath-alloc --example profile-jobs -- /var/tmp/fastmash-language-cache language-cache
```

Use `--features hotpath` instead for timing without allocation tracking.
For a single job, add its name after the destination, for example `decimal`
or `spill`. The other names are `small`, `grouping`, `error` and `refusal`.

The calculation jobs are `pskew`, `jarque`, `pcov` and `mixed`. Each sorts
200,000 records into two groups and checks its exact output. Balanced signals
of -1 and +1 with factors of -2 and +2 give skewness 0, covariance 2 and RMS 1.
At this group size the Jarque-Bera probability rounds to 0. The mixed job also
checks stable contribution order with `last`. For example:

```sh
cargo +stable run --profile profiling --features hotpath-alloc --example profile-jobs -- /var/tmp/fastmash-pskew pskew
```

Reports distinguish preparation and final emission from the group's moment
and paired summaries. Moment reports include the ordered mean separately.
Use the retained input with the CPU or Callgrind commands below to attribute
individual arithmetic primitives without instrumenting every sample.

The CSV decoding jobs are `csv-short`, `csv-long`, `csv-quoted` and
`csv-escaped`. Each counts 4,096 records and checks the exact result. They cover
short fields, 1,024-byte fields, quoted fields with embedded CRLF, and escaped
quotes with empty fields. Use their retained `.input` files with the Callgrind
command below, replacing `sum 1` with `--csv-in count 2`.

The CSV copying jobs isolate decoding, retention and selection over 4,096
records. The first five use the same input: `a,1,"PAYLOAD"`, with 1,024 `x`
bytes in the quoted payload. Output uses the ordinary tab separator.

| Job | Command | Expected output |
| --- | --- | --- |
| `csv-copy` | `--csv-in count 2` | `4096` |
| `csv-copy-sort` | `--csv-in -s -g1 count 2` | `a` and `4096` |
| `csv-copy-spill` | `--csv-in -s -g1 count 2` | `a` and `4096` |
| `csv-copy-top` | `--csv-in top:1 2` | The first complete decoded record |
| `csv-copy-sort-top` | `--csv-in -s -g1 top:1 2` | The first complete decoded record |
| `csv-copy-wide` | `--csv-in count 2` | `4096` |

`csv-copy-spill` sets `FASTMASH_SORT_MEMORY_BYTES=65536` and checks that a
private run was opened. The other jobs use the runner's 64 MiB target. This
forced spill isolates run encoding and merge-head allocation costs. It does
not represent ordinary sort-memory selection. `csv-copy-wide` has 128 quoted
one-byte fields followed by `1`, exposing repeated small appends and field-end
writes. The top jobs have tied ranks, so only the first record wins. They
isolate rejecting candidates from copying accepted candidates, rather than
measuring a workload in which every record replaces the winner.

Build with `hotpath-alloc` and run a job twice in fresh destinations, once with
the default allocation-byte metric and once with `HOTPATH_ALLOC_METRIC=count`.
Allocation bytes include successive buffer growth. They are not a count of
bytes copied, live memory or peak resident memory. Use the raw Callgrind call
graph to distinguish copying inside decoding from copying into sort chunks.
Keep per-record helpers uninstrumented to avoid adding a measurement to every
append.

The example configures `HOTPATH_OUTPUT_PATH`, `HOTPATH_OUTPUT_FORMAT=json`, and
`HOTPATH_METRICS_SERVER_OFF=1` on each child. No static capture starts a network
listener. Without `HOTPATH_OUTPUT_PATH`, an instrumented binary runs without a
report. With that path set, the binary requires the metrics server to be off
before starting the guard. Keep the path on a local regular file, separate
from stdout, stderr and input. Upstream report settings can be adjusted on the
child when investigating a particular question.

Inspect `JOB.json` with a JSON viewer or `jq . JOB.json`. Function timings
include instrumented callees, so nested totals cannot be added. Allocation
reports attribute Rust allocations exclusively by default. They are neither
peak resident memory nor total process memory.
Instrumentation stays at command, sort and group-summary boundaries rather
than individual records or arithmetic primitives.

## CPU sampling

Reuse `samply` 0.13.x, `hotpath-samply` 0.31.2 and the hotpath CLI/TUI when
installed. If missing, upstream installation commands are:

```sh
cargo install samply --locked
cargo install hotpath --version 0.31.2 --bin hotpath-samply
cargo install hotpath --version 0.31.2 --features tui --bin hotpath
```

Build with symbols and CPU support, then capture only the decimal job. On Linux
use a separate session as required by upstream. This block explicitly uses Bash.

```bash
bash -c '
set -euo pipefail
cargo +stable build --profile profiling --features hotpath-cpu --bin fastmash --example profile-jobs
setsid -w target/profiling/examples/profile-jobs /var/tmp/fastmash-cpu decimal
'
```

On macOS, run `samply setup` if needed and omit `setsid -w`. Linux CPU sampling
needs permission to attach through the kernel perf and ptrace policies. Follow
the upstream permission instructions only when authorized to change the host.
An empty CPU section, missing worker, attachment failure or short capture is
missing CPU evidence. Retain it as a limitation rather than inferring no CPU
cost. Hotpath stores the underlying `hp.json.gz` in its reported session under
`/tmp/hotpath`. Inspect it separately with `samply load PATH` when an interactive
viewer is wanted. Loading that viewer is separate from static capture.

CPU instrumentation prevents measured functions from being inlined. Use its
samples to locate work, then verify the hypothesis in an ordinary build.

## Instruction counts on Linux

Hotpath can attribute executed instructions to the existing instrumented
functions using Valgrind's Callgrind tool. This helps distinguish extra work
from noisy elapsed times when investigating a small command. Install Valgrind
in the development environment, then use a fresh directory for each capture.
This block explicitly uses Bash.

```bash
bash -c '
set -euo pipefail
cargo +stable build --profile profiling --features hotpath-callgrind --bin fastmash
capture=$(mktemp -d)
printf "1\n2\n3\n" > "$capture/input"
env HOTPATH_METRICS_SERVER_OFF=1 \
  HOTPATH_OUTPUT_PATH="$capture/report.json" \
  HOTPATH_CALLGRIND_OUT_FILE="$capture/callgrind.%p" \
  valgrind --tool=callgrind --callgrind-out-file="$capture/callgrind.%p" \
  target/profiling/fastmash sum 1 < "$capture/input" > "$capture/stdout"
test "$(cat "$capture/stdout")" = 6
jq -e '\''.functions_icount.tool | startswith("callgrind-")'\'' "$capture/report.json"
jq -e '\''.functions_icount.data | any(.total | tonumber > 0)'\'' "$capture/report.json"
printf "Capture: %s\n" "$capture"
'
```

Keep the JSON report and raw Callgrind files together. Instruction counts are
diagnostic evidence. Hotpath disables function timing in Callgrind mode because
Valgrind changes execution time. Use an ordinary build for performance claims.

## Validation and measurements

Run the ordinary application and website checks in [TESTING.md](TESTING.md).
Also check the profiling configuration on Linux and native macOS:

```sh
cargo +stable clippy -p fastmash --all-targets --features hotpath-alloc --locked -- -D warnings
cargo +stable test --release -p fastmash --features hotpath-alloc --test profiling --locked
cargo +stable check -p fastmash --all-targets --features hotpath-cpu --locked
cargo deny check licenses
```

Profiling-enabled tests cover the command boundary. Ordinary release workspace
tests and the frozen corpus remain the numerical and compatibility evidence.
Native macOS runtime checks require an actual supported Mac. A cross-target
compile is useful build coverage and cannot replace them.

All captures above are shared diagnostics. Retain source revision and patch,
binary hash, compiler and build flags, dataset and command, host identity,
background activity, exit status and report. The runner records reproducible
job inputs and streams. Record the remaining host and source facts alongside
the destination. Follow the maintained [measurement method](docs/src/benchmarks/method.md)
for final before-and-after acceptance comparisons, with profiling disabled in
both ordinary builds. Diagnostic timings cannot establish a speedup or satisfy
Predecessor guards.
