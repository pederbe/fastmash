# Profiling complete Commands

Use hotpath-rs for local diagnosis of startup, decimal calculations, Grouping
and Spill. Ordinary builds have no profiling dependency or instrumentation.
The optional dependency is pinned to MIT-licensed hotpath 0.28.6. Its transitive
dependencies are checked by the repository's `cargo deny check licenses` policy.
CPU report decoding also brings in the permissive Zlib-licensed `zlib-rs`.

The integration follows upstream [setup](https://hotpath.rs/),
[static profiling modes](https://hotpath.rs/profiling_modes),
[allocation profiling](https://hotpath.rs/functions) and
[CPU profiling](https://hotpath.rs/cpu_profiling). It uses a guard whose lifetime
includes command parsing, execution and stream completion. Controlled errors
and Refusals flush reports before the process exits. Forced termination and
aborts cannot promise a final report.

## Build and capture

Normal builds retain Rust 1.88 compatibility. Use Rust 1.98.1 for profiling.
Reuse installed tools and the normal build cache. These commands run from the
repository root on Linux x86-64 or Apple Silicon macOS 15 or later.

```sh
cargo +1.98.1 build --profile profiling --features hotpath-alloc --bin fastmash --example profile-jobs
measure-lock build -- cargo +1.98.1 run --profile profiling --features hotpath-alloc --example profile-jobs -- /tmp/fastmash-profiling
```

Choose a fresh destination. The Rust example creates repeatable datasets and
runs complete Commands with exact expected stdout, stderr and exit statuses.
It covers a small sum, decimal accumulation, unsorted Grouping, forced native
Spill, a command error and a Refusal. It retains inputs, command streams,
statuses, binary identity and JSON reports in that destination. The Spill job
uses a one-byte sort-memory target and verifies that private Spill runs were
opened. This target forces a route for diagnosis, and is not a realistic
performance comparison.

The decimal job sums ten million `0.125` Records. Each value is one eighth, so
the independently known exact result is `1250000`. The larger workload gives
the CPU sampler more time to attach before the Command finishes.

Use `--features hotpath` instead for timing without allocation tracking.
For a single job, add its name after the destination, for example `decimal`
or `spill`. The other names are `small`, `grouping`, `error` and `refusal`.

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
peak resident memory nor allocation totals for an external system sort.
Instrumentation stays at command and sort boundaries rather than individual
records or arithmetic primitives.

## CPU sampling

Reuse `samply` 0.13.x, `hotpath-samply` 0.28.6 and the hotpath CLI/TUI when
installed. If missing, upstream installation commands are

```sh
cargo install samply --locked
cargo install hotpath --version 0.28.6 --bin hotpath-samply
cargo install hotpath --version 0.28.6 --features tui --bin hotpath
```

Build with symbols and CPU support, then capture only the decimal job. On Linux
use a separate session as required by upstream. This block explicitly uses Bash.

```bash
bash -c '
set -euo pipefail
cargo +1.98.1 build --profile profiling --features hotpath-cpu --bin fastmash --example profile-jobs
measure-lock build -- setsid -w target/profiling/examples/profile-jobs /tmp/fastmash-cpu decimal
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

## Validation and measurements

Run the ordinary application and website checks in [TESTING.md](TESTING.md).
Also check the profiling configuration on Linux and native macOS

```sh
cargo +1.88 check --workspace --locked
cargo +1.98.1 clippy -p fastmash --all-targets --features hotpath-alloc --locked -- -D warnings
cargo +1.98.1 test --release -p fastmash --features hotpath-alloc --test profiling --locked
cargo +1.98.1 check -p fastmash --all-targets --features hotpath-cpu --locked
cargo deny check licenses
```

Profiling-enabled tests cover the command boundary. Ordinary release workspace
tests and the frozen corpus remain the numerical and compatibility evidence.
Native macOS runtime checks require an actual supported Mac. A cross-target
compile is useful build coverage and cannot replace them.

The static profiling CI matrix covers Linux and macOS 15/26. Its disposable
Linux runner also captures attributed CPU samples with pinned sampling tools.
Only that runner relaxes its perf and ptrace settings. Reports are retained as
CI artifacts for seven days. Desktop settings are configured separately.

All captures above are shared diagnostics. Retain source revision and patch,
binary hash, compiler and build flags, dataset and command, host identity,
background activity, exit status and report. The runner records reproducible
job inputs and streams. Record the remaining host and source facts alongside
the destination. Follow the maintained [measurement method](docs/src/benchmarks/method.md)
for final before-and-after acceptance comparisons, with profiling disabled in
both ordinary builds. Diagnostic timings cannot establish a speedup or satisfy
Predecessor guards.
