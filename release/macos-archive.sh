#!/bin/bash
# Build one development archive on native Apple Silicon macOS.
# Usage: bash release/macos-archive.sh OUTPUT_DIRECTORY
# This prepares a CI artifact; it never publishes a release or changes the
# qualified Linux files. Install and test these same bytes on macOS 15 and 26.
set -euo pipefail

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
target=aarch64-apple-darwin
name=fastmash-v$label-$target
out=$1
mkdir -p "$out"
out=$(cd "$out" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

export MACOSX_DEPLOYMENT_TARGET=15.0 CARGO_INCREMENTAL=0
cargo build --locked --release -p fastmash --bin fastmash --target "$target"
binary=${CARGO_TARGET_DIR:-$root/target}/$target/release/fastmash
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
    printf 'artifact_kind=development-test\narchive_label=%s\nbinary_version=%s\n' "$label" "$version"
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
printf '%s\n' "$out/$name.tar.gz"
