#!/bin/bash
# Build the tagged version twice and retain one executable with its delivery hashes.
set -euo pipefail
: "${RUNNER_TEMP:?}"
work="$RUNNER_TEMP/weekly-linux"
mkdir -p "$work/builder" "$work/out"
cp release/Containerfile "$work/builder/"
# Do not reuse an installed stable toolchain from a previous image build.
docker build --pull --no-cache -f "$work/builder/Containerfile" -t fastmash-weekly-builder "$work/builder"
{
    printf 'source_revision=%s\n' "$(git rev-parse HEAD)"
    docker run --rm fastmash-weekly-builder bash -c \
        'for tool in rustc cargo glibc cc ld; do printf "\n%s:\n" "$tool"; cat "/builder/$tool.txt"; done'
} > "$work/out/build-provenance.txt"
for build in first second; do
    docker run --rm -v "$PWD:/source:ro" -v "$work/out:/out" \
        -e CARGO_TARGET_DIR="/out/$build" fastmash-weekly-builder bash /source/release/build.sh
done
for binary in fastmash; do
    cmp "$work/out/first/release/$binary" "$work/out/second/release/$binary"
done
(cd "$work/out/first/release" && sha256sum fastmash) > release/delivery.sha256
