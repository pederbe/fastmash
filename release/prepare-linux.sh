#!/bin/bash
# Freeze delivery hashes after two identical clean builds of the planned version.
set -euo pipefail
: "${RUNNER_TEMP:?}"
work="$RUNNER_TEMP/weekly-linux"
mkdir -p "$work/builder" "$work/out"
cp release/Containerfile "$work/builder/"
for component in rustc cargo rust-std; do
    curl --proto '=https' -fsSLo "$work/builder/$component-1.99.1-x86_64-unknown-linux-gnu.tar.xz" \
        "https://static.rust-lang.org/dist/$component-1.99.1-x86_64-unknown-linux-gnu.tar.xz"
done
docker build -f "$work/builder/Containerfile" -t fastmash-weekly-builder "$work/builder"
for build in first second; do
    docker run --rm -v "$PWD:/source:ro" -v "$work/out:/out" \
        -e CARGO_TARGET_DIR="/out/$build" fastmash-weekly-builder bash /source/release/build.sh
done
for binary in fastmash fastmash-sort-supervisor; do
    cmp "$work/out/first/release/$binary" "$work/out/second/release/$binary"
done
(cd "$work/out/first/release" && sha256sum fastmash fastmash-sort-supervisor) > release/delivery.sha256
