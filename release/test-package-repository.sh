#!/bin/bash
# Install, check and remove Fastmash from a local copy of the package
# repository, as users would, in offline Debian 12, Ubuntu 24.04 and
# AlmaLinux 8 containers with signature checks on.
#
# usage: bash release/test-package-repository.sh REPOSITORY VERSION QUALIFIED [PREVIOUS]
#
# QUALIFIED is release/qualified.sha256 from VERSION's tag; the installed
# programs are checked with this directory's check-installed.sh. With
# PREVIOUS, each container first installs that version from the repository
# and then runs a normal upgrade, which must arrive at VERSION; without it,
# VERSION is installed directly. Needs docker.
set -euo pipefail

repository=$(realpath "$1") version=$2 qualified=$(realpath "$3") previous=${4:-}
here=$(dirname "$(realpath "$0")")
release=$(mktemp -d)
trap 'rm -rf "$release"' EXIT
cp "$here/check-installed.sh" "$release/"
cp "$qualified" "$release/qualified.sha256"
chmod -R a+rX "$release"

# Check the installed pair and its removal; shared by both families.
read -r -d '' finish <<'EOF' || true
bash /release/check-installed.sh "$VERSION" /usr/bin /release/qualified.sha256
remove
test ! -e /usr/bin/fastmash
test ! -e /usr/bin/fastmash-sort-supervisor
EOF

apt_test() { # IMAGE
    docker run --rm --network=none -v "$repository:/repo:ro" -v "$release:/release:ro" \
        -e VERSION="$version" -e PREVIOUS="$previous" "$1" bash -c '
set -euo pipefail
rm -f /etc/apt/sources.list /etc/apt/sources.list.d/*
cp /repo/fastmash.gpg /usr/share/keyrings/fastmash.gpg
echo "deb [signed-by=/usr/share/keyrings/fastmash.gpg] file:/repo/apt stable main" \
    > /etc/apt/sources.list.d/fastmash.list
apt-get -q update
if [ -n "$PREVIOUS" ]; then
    apt-get -qy install "fastmash=$PREVIOUS"
    test "$(fastmash --version)" = "fastmash $PREVIOUS"
    apt-get -qy upgrade
else
    apt-get -qy install "fastmash=$VERSION"
fi
remove() { apt-get -qy remove fastmash; }
'"$finish"
}

dnf_test() { # IMAGE
    docker run --rm --network=none -v "$repository:/repo:ro" -v "$release:/release:ro" \
        -e VERSION="$version" -e PREVIOUS="$previous" "$1" bash -c '
set -euo pipefail
rm -f /etc/yum.repos.d/*
sed -e "s|https://packages.fastmash.io/rpm|file:///repo/rpm|" \
    -e "s|https://packages.fastmash.io/fastmash.asc|file:///repo/fastmash.asc|" \
    /repo/fastmash.repo > /etc/yum.repos.d/fastmash.repo
if [ -n "$PREVIOUS" ]; then
    dnf -qy install "fastmash-$PREVIOUS"
    test "$(fastmash --version)" = "fastmash $PREVIOUS"
    dnf -qy upgrade fastmash
else
    dnf -qy install "fastmash-$VERSION"
fi
rpm -q --qf "%{RSAHEADER:pgpsig}\n" fastmash | grep -q "Key ID"
remove() { dnf -qy remove fastmash; }
'"$finish"
}

apt_test debian:12-slim
apt_test ubuntu:24.04
dnf_test almalinux:8
echo "repository installs of $version${previous:+ (upgraded from $previous)} passed"
