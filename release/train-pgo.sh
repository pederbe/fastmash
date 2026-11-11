#!/bin/bash
# Train one fresh compiler profile in the same builder as the release executable.
set -euo pipefail
cd /source
export CARGO_HOME=/tmp/cargo CARGO_INCREMENTAL=0 LC_ALL=C TZ=UTC
cargo build --locked --release --manifest-path release/publication-test-server/Cargo.toml \
  --target-dir /out/pgo-tools --bin fastmash-pgo-train
timeout --kill-after=5s 300s /out/pgo-tools/release/fastmash-pgo-train \
  /out/instrumented/x86_64-unknown-linux-musl/release/fastmash /out/pgo/training /out/pgo/raw
llvm-profdata merge -o /out/pgo/merged.profdata /out/pgo/raw
llvm-profdata show /out/pgo/merged.profdata > /out/pgo/profile-summary.txt
