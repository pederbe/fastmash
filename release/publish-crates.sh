#!/bin/bash
# Resume a dependency-ordered workspace upload without republishing immutable versions.
# usage: bash release/publish-crates.sh VERSION (dry-run|publish)
set -euo pipefail
version=${1:?expected version}
mode=${2:?expected dry-run or publish}
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
[[ "$mode" = dry-run || "$mode" = publish ]]
work=$(mktemp -d)
trap 'rm -r "$work"' EXIT
remaining=()
for package in fastmash-numeric-contract fastmash-conversion fastmash-portable-numerics fastmash-sort-process fastmash; do
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
            ;;
        404) remaining+=(-p "$package") ;;
        *) echo "Could not determine registry state for $package $version (HTTP $status)." >&2; exit 1 ;;
    esac
done
if [ "${#remaining[@]}" = 0 ]; then
    echo 'All five crate versions are already published.'
    exit 0
fi
options=()
[ "$mode" = publish ] || options+=(--dry-run)
cargo publish --locked --registry crates-io "${remaining[@]}" "${options[@]}"
