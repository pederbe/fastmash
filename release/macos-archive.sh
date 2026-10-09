#!/bin/bash
# Build a native archive; release mode compares two clean executable builds.
# Usage: bash release/macos-archive.sh OUTPUT_DIRECTORY [development|release]
# Install and test the same archive bytes on macOS 15 and 26 before delivery.
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo 'usage: macos-archive.sh OUTPUT_DIRECTORY [development|release]' >&2
    exit 1
fi
mode=${2:-development}
case $mode in
    development|release) ;;
    *) echo 'unknown archive mode' >&2; exit 1 ;;
esac

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
test "$(uname -s)" = Darwin && test "$(uname -m)" = arm64 ||
    { echo 'build this archive on native Apple Silicon macOS' >&2; exit 1; }
os_version=$(sw_vers -productVersion)
test "${os_version%%.*}" -ge 15 ||
    { echo 'the archive builder needs macOS 15 or later' >&2; exit 1; }
test -z "$(git status --porcelain)" ||
    { echo 'build from a clean source checkout so provenance identifies the source' >&2; exit 1; }

source_revision=$(git rev-parse HEAD)
version=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' crates/cli/Cargo.toml | head -n 1)
label=$version-macos-test-${source_revision:0:12}
kind=development-test
if [ "$mode" = release ]; then
    label=$version
    kind=release-candidate
fi
target=aarch64-apple-darwin
name=fastmash-v$label-$target
out=$1
mkdir -p "$out"
out=$(cd "$out" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

export MACOSX_DEPLOYMENT_TARGET=15.0 CARGO_INCREMENTAL=0
if [ "$mode" = release ]; then
    # Stable source and dependency paths make separate clean target trees
    # comparable. The hosted compiler is pinned by the calling workflow.
    export RUSTFLAGS="--remap-path-prefix=$root=/fastmash --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}/registry/src=/deps"
    unset CARGO_ENCODED_RUSTFLAGS
    export CARGO_TARGET_DIR="$work/first"
fi
cargo build --locked --release -p fastmash --bin fastmash --target "$target"
binary=${CARGO_TARGET_DIR:-$root/target}/$target/release/fastmash
if [ "$mode" = release ]; then
    CARGO_TARGET_DIR="$work/second" cargo build --locked --release -p fastmash --bin fastmash --target "$target"
    cmp "$binary" "$work/second/$target/release/fastmash"
fi
stage=$work/$name
mkdir "$stage"
cp "$binary" "$stage/fastmash"
chmod 755 "$stage/fastmash"
cmp "$binary" "$stage/fastmash"
deployment=$(xcrun vtool -show-build "$stage/fastmash")
printf '%s\n' "$deployment" | grep -Eq '^[[:space:]]*minos 15\.0(\.0)?$' ||
    { echo 'the binary does not record the macOS 15 deployment target' >&2; exit 1; }
cp README.md CHANGELOG.md LICENSE-MIT LICENSE-APACHE "$stage/"
python3 scripts/third_party_licenses.py --target "$target" > "$stage/THIRD-PARTY-LICENSES.md"
(
    cd "$stage"
    shasum -a 256 fastmash > binary.sha256
)
{
    printf 'artifact_kind=%s\narchive_label=%s\nbinary_version=%s\n' "$kind" "$label" "$version"
    builds=1
    if [ "$mode" = release ]; then builds=2; fi
    printf 'clean_builds=%s\n' "$builds"
    printf 'source_revision=%s\ntarget=%s\nMACOSX_DEPLOYMENT_TARGET=15.0\n' "$source_revision" "$target"
    printf 'CARGO_INCREMENTAL=0\nRUSTFLAGS=%s\nCARGO_ENCODED_RUSTFLAGS=%s\n' "${RUSTFLAGS:-}" "${CARGO_ENCODED_RUSTFLAGS:-}"
    printf 'command=cargo build --locked --release -p fastmash --bin fastmash --target %s\n' "$target"
    printf '\nOperating system:\n'
    sw_vers
    uname -a
    printf '\nCompiler:\n'
    rustc -Vv
    printf '\nMach-O deployment metadata:\n'
    printf '%s\n' "$deployment"
    printf '\nBinary identity:\n'
    cat "$stage/binary.sha256"
} > "$stage/build-provenance.txt"
COPYFILE_DISABLE=1 tar -czf "$out/$name.tar.gz" -C "$work" "$name"
(
    cd "$out"
    shasum -a 256 "$name.tar.gz" > "$name.tar.gz.sha256"
)
cp "$stage/build-provenance.txt" "$out/"
cp "$stage/binary.sha256" "$out/"
printf '%s\n' "$label" > "$out/archive-version.txt"
printf '%s\n' "$kind" > "$out/archive-kind.txt"
printf '%s\n' "$out/$name.tar.gz"
