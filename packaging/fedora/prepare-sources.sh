#!/bin/bash
# Prepare the checksummed release source and its locked dependencies for RPM.
set -euo pipefail
if [ "$#" -ne 1 ]; then
    echo 'usage: prepare-sources.sh OUT_DIR' >&2
    exit 1
fi
mkdir -p "$1"
out=$(realpath "$1")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
archive=fastmash-0.1.0.tar.gz
curl -fL --retry 2 https://github.com/pederbe/fastmash/archive/refs/tags/v0.1.0.tar.gz \
    -o "$work/$archive"
printf '%s  %s\n' aa6b74c7e760622e615b44c5dbeb623aad50e4e6dc34a3808a3f3b1777964b60 \
    "$work/$archive" | sha256sum -c -
tar -xzf "$work/$archive" -C "$work"
cd "$work/fastmash-0.1.0"
cargo vendor --locked vendor > /dev/null
# Reproducible archive metadata; Cargo verifies each vendored crate's checksum.
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -cf - vendor | \
    gzip -n > "$work/fastmash-0.1.0-vendor.tar.gz"
cp "$work/$archive" "$work/fastmash-0.1.0-vendor.tar.gz" "$out/"
cd "$out"
sha256sum "$archive" fastmash-0.1.0-vendor.tar.gz > sources.sha256
