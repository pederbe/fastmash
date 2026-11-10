#!/bin/bash
# Build the tagged version twice and retain one executable with its delivery hashes.
set -euo pipefail
: "${RUNNER_TEMP:?}"
engine=${CONTAINER_ENGINE:-docker}
work="$RUNNER_TEMP/weekly-linux"
mkdir -p "$work/builder" "$work/out"
cp release/Containerfile "$work/builder/"
# Do not reuse an installed stable toolchain from a previous image build.
"$engine" build --pull --no-cache -f "$work/builder/Containerfile" -t fastmash-weekly-builder "$work/builder"
{
    printf 'source_revision=%s\n' "$(git rev-parse HEAD)"
    "$engine" run --rm fastmash-weekly-builder bash -c \
        'for tool in rustc cargo targets cc ld; do printf "\n%s:\n" "$tool"; cat "/builder/$tool.txt"; done'
} > "$work/out/build-provenance.txt"
for build in first second; do
    "$engine" run --rm -v "$PWD:/source:ro" -v "$work/out:/out" \
        -e CARGO_TARGET_DIR="/out/$build" fastmash-weekly-builder bash /source/release/build.sh
done
for binary in fastmash; do
    cmp "$work/out/first/x86_64-unknown-linux-musl/release/$binary" "$work/out/second/x86_64-unknown-linux-musl/release/$binary"
done
(cd "$work/out/first/x86_64-unknown-linux-musl/release" && sha256sum fastmash) > release/delivery.sha256
