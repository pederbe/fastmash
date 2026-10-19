#!/bin/bash
# Check installed release bytes and commands with the prepared host tool.
# usage: bash release/check-installed.sh VERSION BIN_DIR QUALIFIED_CHECKSUMS
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
target=${CARGO_TARGET_DIR:-$here/../target}
checker=${FASTMASH_INSTALLED_CHECKER:-$target/debug/fastmash-check-installed}
if [ ! -x "$checker" ]; then
    echo 'prepare the installed checker with cargo build --locked --manifest-path release/publication-test-server/Cargo.toml --target-dir target --bins' >&2
    exit 1
fi
exec "$checker" "$@"
