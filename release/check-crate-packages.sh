#!/bin/bash
# Check the product source package as it will publish.
# usage: bash release/check-crate-packages.sh VERSION
set -euo pipefail
usage() {
    printf '%s\n' 'usage: bash release/check-crate-packages.sh VERSION' \
        'Check the product source package and run its publication dry-run without uploading.' \
        'VERSION is the expected workspace package version.'
}
if [ "${1:-}" = --help ] || [ "${1:-}" = -h ]; then usage; exit 0; fi
if [ "$#" -ne 1 ]; then usage >&2; exit 1; fi
version=$1
cargo metadata --no-deps --format-version 1 --locked |
    jq -e --arg version "$version" '[.packages[] | select(.publish != [])] |
        length == 1 and .[0].name == "fastmash" and .[0].version == $version and
        ([.[0].targets[] | select(.kind == ["bin"]) | .name] == ["fastmash"]) and
        all(.[0].targets[]; .kind != ["lib"])' > /dev/null ||
    { echo "expected one fastmash product binary package at $version" >&2; exit 1; }
files=$(cargo package --list --locked --registry crates-io -p fastmash)
printf 'fastmash package contents:\n%s\n' "$files"
for required in Cargo.toml Cargo.lock LICENSE-MIT LICENSE-APACHE; do
    grep -qxF "$required" <<< "$files" || { echo "fastmash lacks $required" >&2; exit 1; }
done
# Verify packaged sources even when this version is already in the registry.
# Registry checksum comparison belongs to the resumable upload, not this check.
cargo publish --locked --registry crates-io -p fastmash --dry-run
