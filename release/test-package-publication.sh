#!/bin/bash
# Exercise the real uploader against local storage with actual apt/dnf clients.
# usage: bash release/test-package-publication.sh OLD_REPOSITORY NEW_REPOSITORY
# Requires Rust, Docker (or CONTAINER_ENGINE), Debian 12 and AlmaLinux 8 images.
set -euo pipefail
if [ "${1:-}" = --help ] || [ "${1:-}" = -h ]; then
    printf '%s\n' 'usage: bash release/test-package-publication.sh OLD_REPOSITORY NEW_REPOSITORY' \
        'Exercise the uploader locally using signed fixture repositories and disposable package-manager containers.' \
        'Requires Rust, Docker or CONTAINER_ENGINE, and prepared Debian/AlmaLinux images.' \
        'Set PUBLICATION_TEST_EVIDENCE to a new directory to retain logs and completed.tsv.'
    exit 0
fi
if [ "$#" -ne 2 ]; then
    echo 'usage: bash release/test-package-publication.sh OLD_REPOSITORY NEW_REPOSITORY' >&2
    exit 1
fi
here=$(dirname "$(realpath -- "$0")")
old=$(realpath -- "$1") new=$(realpath -- "$2")
target=${CARGO_TARGET_DIR:-$here/../target}
cargo build --quiet --locked --manifest-path "$here/publication-test-server/Cargo.toml" --target-dir "$target" --bins
exec "$target/debug/fastmash-test-package-publication" \
    "$old" "$new" "$here/publish-package-repository.sh"
