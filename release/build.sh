#!/bin/bash
# Build the release binaries inside the release builder (release/Containerfile)
# with the source mounted at /source and CARGO_TARGET_DIR set. These flags and
# paths define the measured and released artifact: the remaps give vendored
# (offline) and crates.io builds identical dependency paths.
set -euo pipefail
cd /source
: "${CARGO_TARGET_DIR:?}"
export CARGO_HOME=/tmp/cargo CARGO_INCREMENTAL=0 LC_ALL=C TZ=UTC
flags=(
  -Ctarget-cpu=x86-64
  -Cpanic=abort
  -Coverflow-checks=yes
  -Cllvm-args=-x86-branches-within-32B-boundaries
  -L/opt/fastmash-link
  --remap-path-prefix=/source=/fastmash
  --remap-path-prefix=/source/crates/vendor=/deps
  --remap-path-prefix=/tmp/cargo/registry/src/index.crates.io-1949cf8c6b5b557f=/deps
)
CARGO_ENCODED_RUSTFLAGS=$(IFS=$'\x1f'; printf '%s' "${flags[*]}")
export CARGO_ENCODED_RUSTFLAGS
test -f /opt/fastmash-link/libgcc_s.a
cargo build --locked --release -p fastmash --bins
for binary in fastmash fastmash-sort-supervisor; do
  if readelf -d "$CARGO_TARGET_DIR/release/$binary" | grep -q libgcc_s; then
    echo "$binary still needs libgcc_s: the release link shim did not apply" >&2
    exit 1
  fi
done
