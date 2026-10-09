#!/bin/bash
# Check installed release bytes and commands using the platform's archive layout.
# usage: bash release/check-installed.sh VERSION BIN_DIR QUALIFIED_CHECKSUMS
set -euo pipefail
if [ "$#" -ne 3 ]; then
    echo 'usage: check-installed.sh VERSION BIN_DIR QUALIFIED_CHECKSUMS' >&2
    exit 1
fi
version=$1
bin=$(cd "$2" && pwd)
checksums=$(cd "$(dirname "$3")" && pwd)/$(basename "$3")
export LC_ALL=C
case $(uname -s) in
    Linux) programs=(fastmash fastmash-sort-supervisor); checksum=(sha256sum) ;;
    Darwin) programs=(fastmash); checksum=(shasum -a 256) ;;
    *) echo 'unsupported release platform' >&2; exit 1 ;;
esac
for program in "${programs[@]}"; do
    test -x "$bin/$program" || { echo "$program is not installed and executable" >&2; exit 1; }
done
(cd "$bin" && "${checksum[@]}" -c "$checksums")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

same() {
    local expected_hash actual_hash
    expected_hash=$("${checksum[@]}" "$1")
    actual_hash=$("${checksum[@]}" "$2")
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

if [ "${#programs[@]}" -eq 1 ]; then
    test ! -e "$bin/fastmash-sort-supervisor"
    check $'b\t2\t3\na\t1\t2\na\t3\t6\n' $'a\t2\nb\t0\n' -sg1 pcov 2:3
    exit 0
fi

# Correct output alone can hide fallback to the built-in sorter. Require the
# system route as well, so a missing or incompatible supervisor cannot pass.
printf 'b\t2\t3\na\t1\t2\na\t3\t6\n' | \
    env -i PATH=/usr/bin:/bin LC_ALL=C FASTMASH_GROUPING=sort FASTMASH_SORT_TRACE=1 \
    "$bin/fastmash" -sg1 pcov 2:3 > "$work/actual" 2> "$work/diagnostic"
printf 'a\t2\nb\t0\n' > "$work/expected"
same "$work/expected" "$work/actual"
printf 'sort route: system sort\n' > "$work/expected"
same "$work/expected" "$work/diagnostic"
