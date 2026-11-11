#!/bin/bash
# Check installed release bytes and commands with the prepared host tool.
# usage: bash release/check-installed.sh VERSION BIN_DIR QUALIFIED_CHECKSUMS
set -euo pipefail
usage() {
    printf '%s\n' 'usage: bash release/check-installed.sh VERSION BIN_DIR QUALIFIED_CHECKSUMS' \
        'Verify installed executable bytes, version and command behavior against a qualified checksum manifest.' \
        'Set INSTALLATION_TEST_EVIDENCE to a new directory to retain detailed receipts.'
}
if [ "${1:-}" = --help ] || [ "${1:-}" = -h ]; then usage; exit 0; fi
if [ "$#" -ne 3 ]; then usage >&2; exit 1; fi
here=$(cd "$(dirname "$0")" && pwd)
target=${CARGO_TARGET_DIR:-$here/../target}
checker=${FASTMASH_INSTALLED_CHECKER:-$target/debug/fastmash-check-installed}
if [ ! -x "$checker" ]; then
    echo 'prepare the installed checker with cargo build --locked --manifest-path release/publication-test-server/Cargo.toml --target-dir target --bins' >&2
    exit 1
fi
exec "$checker" "$@"
