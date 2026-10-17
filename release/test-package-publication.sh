#!/bin/bash
# Exercise the real uploader against local storage with actual apt/dnf clients.
# usage: bash release/test-package-publication.sh OLD_REPOSITORY NEW_REPOSITORY
# Requires Rust, Docker (or CONTAINER_ENGINE), Debian 12 and AlmaLinux 8 images.
set -euo pipefail
here=$(dirname "$(realpath "$0")")
old=$(realpath "$1") new=$(realpath "$2")
engine=${CONTAINER_ENGINE:-docker}
work=$(mktemp -d)
server_pid=
cleanup() {
    local status=$?
    if [ "$status" -ne 0 ] && [ -f "$work/server.log" ]; then tail -n 30 "$work/server.log"; fi
    if [ -n "$server_pid" ]; then kill "$server_pid" 2>/dev/null || true; wait "$server_pid" || true; fi
    rm -rf "$work"
}
trap cleanup EXIT
mkdir "$work/bin" "$work/live"
export PUBLICATION_LIVE=$work/live PUBLICATION_COUNT=$work/count
export PATH="$work/bin:$PATH"
export PUBLICATION_SOURCE=$new PUBLICATION_UPLOADER=$here/publish-package-repository.sh
cat > "$work/action" <<'EOF'
#!/bin/bash
set -euo pipefail
bash "$PUBLICATION_UPLOADER" "$PUBLICATION_SOURCE" test-bucket
EOF
target=${CARGO_TARGET_DIR:-$here/../target}
cargo build --quiet --locked --manifest-path "$here/publication-test-server/Cargo.toml" --target-dir "$target" --bins
# The local adapter atomically exposes each object and can stop after any write.
# Package-manager requests still go through the existing HTTP fixture server.
cp "$target/debug/fastmash-publication-test-storage" "$work/bin/aws"
mkfifo "$work/ready"
"$target/debug/fastmash-publication-test-server" \
    "$work/live" "$work/ready" "$work/trigger" "$work/action" 2> "$work/server.log" &
server_pid=$!
read -r port < "$work/ready"
origin=http://127.0.0.1:$port
reset_repository() {
    rm -rf "$work/live"
    mkdir "$work/live"
    cp -a "$old/." "$work/live/"
    echo 0 > "$PUBLICATION_COUNT"
}
apt_client() {
    # shellcheck disable=SC2016 # Variables expand inside the container.
    "$engine" run --rm --network=host -v "$old:/repo:ro" -e ORIGIN="$origin" \
        "${APT_TEST_IMAGE:-debian:12-slim}" bash -c '
set -euo pipefail
rm -f /etc/apt/sources.list /etc/apt/sources.list.d/*
printf "deb [signed-by=/repo/fastmash.gpg] %s/apt stable main\n" "$ORIGIN" > /etc/apt/sources.list.d/fastmash.list
apt-get -q -o APT::Update::Error-Mode=any update
policy=$(apt-cache policy fastmash)
printf "%s\n" "$policy"
grep -q "Candidate: 0.[12].0" <<< "$policy"
'
}
dnf_client() {
    # shellcheck disable=SC2016 # Variables expand inside the container.
    "$engine" run --rm --network=host -v "$new:/repo:ro" -e ORIGIN="$origin" \
        "${DNF_TEST_IMAGE:-almalinux:8}" bash -c '
set -euo pipefail
rm -f /etc/yum.repos.d/*
sed "s|https://packages.fastmash.io|$ORIGIN|g" /repo/fastmash.repo > /etc/yum.repos.d/fastmash.repo
dnf -qy --refresh makecache
dnf -q --showduplicates list fastmash
'
}
clients() {
    apt_client > "$work/apt.log" 2>&1 || { cat "$work/apt.log"; return 1; }
    dnf_client > "$work/dnf.log" 2>&1 || { cat "$work/dnf.log"; return 1; }
}
reset_repository
clients
bash "$work/action"
writes=$(cat "$PUBLICATION_COUNT")
clients
for ((stop=1; stop<=writes; stop++)); do
    reset_repository
    status=0
    PUBLICATION_STOP_AFTER=$stop bash "$work/action" || status=$?
    test "$status" -eq 75
    clients || { echo "clients failed after object $stop of $writes" >&2; exit 1; }
    bash "$work/action"
    clients
done
echo "apt and dnf accept and resume every interrupted upload ($writes object boundaries)"
for path in /apt/dists/stable/InRelease /rpm/mirrorlist; do
    reset_repository
    printf '%s\n' "$path" > "$work/trigger"
    clients
    test ! -e "$work/trigger"
    clients
done
echo 'apt and dnf accept clients spanning publication'
grep -q '/by-hash/SHA512/' "$work/server.log"
grep -q '/rpm/snapshots/.*/repodata/repomd.xml.asc 200' "$work/server.log"
reset_repository
digest=$(sha512sum < "$old/apt/dists/stable/main/binary-amd64/Packages.gz" | cut -d' ' -f1)
printf 'corrupt index\n' > "$work/live/apt/dists/stable/main/binary-amd64/by-hash/SHA512/$digest"
if apt_client > "$work/apt.log" 2>&1; then
    echo 'apt accepted a corrupt index' >&2
    exit 1
fi
grep -Eq 'Hash Sum mismatch|unexpected size' "$work/apt.log"
reset_repository
snapshot=$(cat "$old/rpm/mirrorlist")
snapshot=${snapshot#https://packages.fastmash.io/}
printf 'corrupt signature\n' > "$work/live/$snapshot/repodata/repomd.xml.asc"
if dnf_client > "$work/dnf.log" 2>&1; then
    echo 'dnf accepted a corrupt signature' >&2
    exit 1
fi
grep -q 'GPG signature' "$work/dnf.log"
echo 'apt and dnf reject corrupt signed metadata'
