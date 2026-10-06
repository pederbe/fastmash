#!/bin/bash
# Check the installed Linux release bytes, version, arithmetic and Sort supervisor.
# usage: bash release/check-installed.sh VERSION BIN_DIR QUALIFIED_CHECKSUMS
set -euo pipefail
if [ "$#" -ne 3 ]; then
    echo 'usage: check-installed.sh VERSION BIN_DIR QUALIFIED_CHECKSUMS' >&2
    exit 1
fi
version=$1 bin=$(realpath "$2") checksums=$(realpath "$3")
export LC_ALL=C
for program in fastmash fastmash-sort-supervisor; do
    test -x "$bin/$program" || { echo "$program is not installed and executable" >&2; exit 1; }
done
(cd "$bin" && sha256sum -c "$checksums")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

same() {
    local expected_hash actual_hash
    expected_hash=$(sha256sum "$1")
    actual_hash=$(sha256sum "$2")
    if [ "${expected_hash%% *}" != "${actual_hash%% *}" ]; then
        echo "unexpected installed output: $2" >&2
        exit 1
    fi
}

check() {
    local input=$1 expected=$2
    shift 2
    printf '%s' "$input" | env -i PATH=/usr/bin:/bin LC_ALL=C "$bin/fastmash" "$@" \
        > "$work/actual" 2> "$work/diagnostic"
    printf '%s' "$expected" > "$work/expected"
    same "$work/expected" "$work/actual"
    test ! -s "$work/diagnostic"
}
check '' "fastmash $version"$'\n' --version
check $'1\t2\n3\t4\n' $'4\t3\n' sum 1 mean 2
check $'1\n2\n3\n' $'2\n' median 1

# Correct output alone can hide fallback to the built-in sorter. Require the
# system route as well, so a missing or incompatible supervisor cannot pass.
printf 'b\t2\t3\na\t1\t2\na\t3\t6\n' | \
    env -i PATH=/usr/bin:/bin LC_ALL=C FASTMASH_GROUPING=sort FASTMASH_SORT_TRACE=1 \
    "$bin/fastmash" -sg1 pcov 2:3 > "$work/actual" 2> "$work/diagnostic"
printf 'a\t2\nb\t0\n' > "$work/expected"
same "$work/expected" "$work/actual"
printf 'sort route: system sort\n' > "$work/expected"
same "$work/expected" "$work/diagnostic"
