#!/bin/bash
# Internal fixture for checking an unpublished macOS archive in CI.
# The public installer discovers the latest release instead.
set -eu
archive=$1
name=${archive%.tar.gz}
shasum -a 256 -c "$archive.sha256"
tar -xzf "$archive"
(cd "$name" && shasum -a 256 -c binary.sha256)
cat "$name/build-provenance.txt"
mkdir -p ~/.local/bin
staging=$(mktemp -d "$HOME/.local/bin/.fastmash.XXXXXX")
trap 'rm -rf "$staging"' EXIT
cp "$name/fastmash" "$staging/fastmash"
chmod 755 "$staging/fastmash"
mv -f "$staging/fastmash" ~/.local/bin/fastmash
printf '1\n2\n' | ~/.local/bin/fastmash sum 1
