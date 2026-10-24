#!/bin/bash
# Prepare the checksummed release source and its locked dependencies for RPM.
set -euo pipefail
if [ "$#" -ne 1 ]; then
    echo 'usage: prepare-sources.sh OUT_DIR' >&2
    exit 1
fi
mkdir -p "$1"
out=$(realpath "$1")
work=$(mktemp -d "${TMPDIR:-/var/tmp}/fastmash-fedora.XXXXXX")
trap 'rm -rf "$work"' EXIT
archive=fastmash-0.2.0.tar.gz
curl -fL --retry 2 https://github.com/pederbe/fastmash/archive/refs/tags/v0.2.0.tar.gz \
    -o "$work/$archive"
printf '%s  %s\n' e331d2d9499dc502f16e253ab4c408ceed4cca251420c53718ea8f70595e0302 \
    "$work/$archive" | sha256sum -c -
tar -xzf "$work/$archive" -C "$work"
cd "$work/fastmash-0.2.0"
cargo vendor --locked vendor > /dev/null
# Reproducible archive metadata; Cargo verifies each vendored crate's checksum.
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -cf - vendor | \
    gzip -n > "$work/fastmash-0.2.0-vendor.tar.gz"
checker=fastmash-linux-source-test.rs
curl -fL --retry 2 \
    https://raw.githubusercontent.com/pederbe/fastmash/3da66cc2b825e175535ace106edbd8f73bd5b2fa/packaging/linux-source-test.rs \
    -o "$work/$checker"
printf '%s  %s\n' 71b6715633d14f654a84d7f27c9f27851bdacc0a2ece0e1ac6cb6c831ce469dc \
    "$work/$checker" | sha256sum -c -
cp "$work/$archive" "$work/fastmash-0.2.0-vendor.tar.gz" "$work/$checker" "$out/"
cd "$out"
sha256sum "$archive" fastmash-0.2.0-vendor.tar.gz "$checker" > sources.sha256
