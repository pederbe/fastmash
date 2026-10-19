#!/bin/bash
# Check real offline Debian, Ubuntu and AlmaLinux repository installation,
# optional previous-version upgrade, qualified programs, notices and removal.
# usage: bash release/test-package-repository.sh REPOSITORY VERSION QUALIFIED [PREVIOUS] [--expected-docs DOC_DIR]
# DOC_DIR supplies independently verified same-version Linux documents.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
target=${CARGO_TARGET_DIR:-$here/../target}
checker=${FASTMASH_PACKAGE_INSTALLATION_CHECKER:-$target/debug/fastmash-test-package-installation}
if [ ! -x "$checker" ]; then
    echo 'prepare the package checker with cargo build --locked --manifest-path release/publication-test-server/Cargo.toml --target-dir target --bins' >&2
    exit 1
fi
exec "$checker" repository "$@"
