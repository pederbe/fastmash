#!/bin/bash
# Smoke test for the installed package: both programs, arithmetic, grouping,
# and the portable-arithmetic promise (a transcendental result's digits).
set -euo pipefail
fastmash --version
test -x "$(dirname "$(command -v fastmash)")/fastmash-sort-supervisor"
check() {
    local expected="$1" actual="$2"
    if [ "$actual" != "$expected" ]; then
        printf 'expected %q, got %q\n' "$expected" "$actual" >&2
        exit 1
    fi
}
check "$(printf '6\t2')" "$(printf '1\n2\n3\n' | fastmash sum 1 mean 1)"
check "$(printf 'a\t4\nb\t2')" "$(printf 'b\t2\na\t1\na\t3\n' | fastmash -s -g 1 sum 2)"
check "1.8171205928321" "$(printf '1\n2\n3\n' | fastmash geomean 1)"
