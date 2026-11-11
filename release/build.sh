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
  --remap-path-prefix=/source=/fastmash
  --remap-path-prefix=/source/crates/vendor=/deps
  --remap-path-prefix=/tmp/cargo/registry/src/index.crates.io-1949cf8c6b5b557f=/deps
)
case "${FASTMASH_RELEASE_PGO:-off}" in
  off) ;;
  generate) flags+=(-Cprofile-generate=/out/pgo/raw) ;;
  use)
    test -s /out/pgo/merged.profdata
    flags+=(-Cprofile-use=/out/pgo/merged.profdata -Cllvm-args=-pgo-warn-missing-function)
    ;;
  *) echo 'FASTMASH_RELEASE_PGO must be off, generate or use' >&2; exit 1 ;;
esac
mkdir -p "$CARGO_TARGET_DIR"
printf '%s\n' "${flags[@]}" > "$CARGO_TARGET_DIR/build-flags.txt"
CARGO_ENCODED_RUSTFLAGS=$(IFS=$'\x1f'; printf '%s' "${flags[*]}")
export CARGO_ENCODED_RUSTFLAGS
cargo build --locked --release --target x86_64-unknown-linux-musl -p fastmash --bins
path="$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/fastmash"
if readelf -d "$path" | grep -q '(NEEDED)' || readelf -l "$path" | grep -q INTERP; then
  echo "fastmash must be statically linked without a shared-library interpreter" >&2
  exit 1
fi
