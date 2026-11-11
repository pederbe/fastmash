#!/bin/bash
# Train once, then build the tagged version twice from the same compiler profile.
set -euo pipefail
: "${RUNNER_TEMP:?}"
engine=${CONTAINER_ENGINE:-docker}
work="$RUNNER_TEMP/weekly-linux"
mkdir -p "$work/builder" "$work/out" "$work/cargo"
# Refuse stale training or profile data, including after a failed local attempt.
mkdir "$work/out/pgo"
cp release/Containerfile "$work/builder/"
# Do not reuse an installed stable toolchain from a previous image build.
"$engine" build --pull --no-cache -f "$work/builder/Containerfile" -t fastmash-weekly-builder "$work/builder"
{
    printf 'source_revision=%s\n' "$(git rev-parse HEAD)"
    # shellcheck disable=SC2016 # Expand these paths inside the container.
    "$engine" run --rm fastmash-weekly-builder bash -c \
        'for tool in rustc cargo targets cc ld llvm-profdata; do printf "\n%s:\n" "$tool"; cat "/builder/$tool.txt"; done'
} > "$work/out/build-provenance.txt"
run=(run --rm -v "$PWD:/source:ro" -v "$work/out:/out" -v "$work/cargo:/tmp/cargo")
"$engine" "${run[@]}" -e CARGO_TARGET_DIR=/out/instrumented \
    -e FASTMASH_RELEASE_PGO=generate fastmash-weekly-builder bash /source/release/build.sh
"$engine" "${run[@]}" fastmash-weekly-builder bash /source/release/train-pgo.sh
for build in first second; do
    "$engine" "${run[@]}" -e CARGO_TARGET_DIR="/out/$build" \
        -e FASTMASH_RELEASE_PGO=use fastmash-weekly-builder bash /source/release/build.sh
done
cmp "$work/out/first/x86_64-unknown-linux-musl/release/fastmash" \
    "$work/out/second/x86_64-unknown-linux-musl/release/fastmash"
{
    printf '\nrelease_rustflags:\n'
    cat "$work/out/first/build-flags.txt"
    printf '\ncompiler_profile_and_training_sha256:\n'
    (cd "$work/out" && sha256sum pgo/merged.profdata pgo/training/manifest.tsv \
        instrumented/x86_64-unknown-linux-musl/release/fastmash)
} >> "$work/out/build-provenance.txt"
(cd "$work/out/first/x86_64-unknown-linux-musl/release" && sha256sum fastmash) > release/delivery.sha256
