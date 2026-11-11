#!/bin/bash
# Check real offline Debian, Ubuntu and AlmaLinux repository installation,
# optional previous-version upgrade, qualified programs, notices and removal.
# usage: bash release/test-package-repository.sh REPOSITORY VERSION QUALIFIED [PREVIOUS] [--expected-docs DOC_DIR]
# DOC_DIR supplies independently verified same-version Linux documents.
set -euo pipefail
usage() {
    printf '%s\n' 'usage: bash release/test-package-repository.sh REPOSITORY VERSION QUALIFIED [PREVIOUS] [--expected-docs DOC_DIR]' \
        'Check installation, optional upgrade and removal in disposable package-manager containers.' \
        'QUALIFIED is the executable checksum manifest. DOC_DIR supplies independently verified release documents.' \
        'Set INSTALLATION_TEST_EVIDENCE to a new directory to retain receipts.'
}
if [ "${1:-}" = --help ] || [ "${1:-}" = -h ]; then usage; exit 0; fi
if [ "$#" -lt 3 ]; then usage >&2; exit 1; fi
here=$(cd "$(dirname "$0")" && pwd)
target=${CARGO_TARGET_DIR:-$here/../target}
checker=${FASTMASH_PACKAGE_INSTALLATION_CHECKER:-$target/debug/fastmash-test-package-installation}
if [ ! -x "$checker" ]; then
    echo 'prepare the package checker with cargo build --locked --manifest-path release/publication-test-server/Cargo.toml --target-dir target --bins' >&2
    exit 1
fi
exec "$checker" repository "$@"
