#!/bin/bash
# Check source package contents and build all five crates as they will publish.
# usage: bash release/check-crate-packages.sh VERSION
set -euo pipefail
usage() {
    printf '%s\n' 'usage: bash release/check-crate-packages.sh VERSION' \
        'Check the five source packages and run their publication dry-run without uploading.' \
        'VERSION is the expected workspace package version.'
}
if [ "${1:-}" = --help ] || [ "${1:-}" = -h ]; then usage; exit 0; fi
if [ "$#" -ne 1 ]; then usage >&2; exit 1; fi
version=$1
cargo metadata --no-deps --format-version 1 --locked |
    jq -e --arg version "$version" 'all(.packages[] | select(.publish != []); .version == $version)' > /dev/null ||
    { echo "crate versions differ from $version" >&2; exit 1; }
for package in fastmash-numeric-contract fastmash-conversion fastmash-portable-numerics fastmash-sort-process fastmash; do
    files=$(cargo package --list --locked --registry crates-io -p "$package")
    printf '%s package contents:\n%s\n' "$package" "$files"
    for required in Cargo.toml Cargo.lock LICENSE-MIT LICENSE-APACHE; do
        grep -qxF "$required" <<< "$files" || { echo "$package lacks $required" >&2; exit 1; }
    done
done
# Cargo handles dependencies between unpublished workspace crates, including
# verification of their packaged sources. No upload or registry token is needed.
bash release/publish-crates.sh "$version" dry-run
