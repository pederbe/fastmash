#!/bin/bash
# Resume the product upload without republishing an immutable version.
# usage: bash release/publish-crates.sh VERSION (dry-run|publish)
set -euo pipefail
usage() {
    printf '%s\n' 'usage: bash release/publish-crates.sh VERSION (dry-run|publish)' \
        'Check registry state and upload the fastmash package if missing.' \
        'dry-run checks the pending upload without uploading. publish uploads a missing version.' \
        'VERSION must be numeric MAJOR.MINOR.PATCH without a leading v.'
}
if [ "${1:-}" = --help ] || [ "${1:-}" = -h ]; then usage; exit 0; fi
if [ "$#" -ne 2 ]; then usage >&2; exit 1; fi
version=$1
mode=$2
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] ||
    { echo 'version must be numeric MAJOR.MINOR.PATCH without a leading v' >&2; exit 1; }
[[ "$mode" = dry-run || "$mode" = publish ]] ||
    { echo 'mode must be dry-run or publish' >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -r "$work"' EXIT
package=fastmash
status=$(curl --proto '=https' --retry 2 --silent --show-error --location \
    --user-agent 'fastmash-release (https://github.com/pederbe/fastmash)' \
    --output "$work/version.json" --write-out '%{http_code}' \
    "https://crates.io/api/v1/crates/$package/$version")
case "$status" in
    200)
        checksum=$(jq -er --arg version "$version" '.version | select(.num == $version and .yanked == false) | .checksum' "$work/version.json")
        cargo package --locked --registry crates-io --no-verify -p "$package"
        printf '%s  %s\n' "$checksum" "${CARGO_TARGET_DIR:-target}/package/$package-$version.crate" | sha256sum -c -
        echo "$package $version is already published with matching source bytes."
        exit 0
        ;;
    404) ;;
    *) echo "Could not determine registry state for $package $version (HTTP $status)." >&2; exit 1 ;;
esac
options=()
[ "$mode" = publish ] || options+=(--dry-run)
cargo publish --locked --registry crates-io -p fastmash "${options[@]}"
