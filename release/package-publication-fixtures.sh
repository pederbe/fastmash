#!/bin/bash
# Build two small signed repositories for publication tests, without release assets.
# usage: bash release/package-publication-fixtures.sh DIRECTORY
set -euo pipefail
here=$(dirname "$(realpath "$0")")
mkdir -p "$1"
work=$(realpath "$1")
export GNUPGHOME="$work/gnupg"
mkdir -m 700 "$GNUPGHOME"
trap 'gpgconf --kill gpg-agent; rm -rf "$GNUPGHOME" "$work/build"' EXIT
gpg --batch --pinentry-mode loopback --passphrase '' --quick-gen-key \
    'Publication test <test@example.invalid>' rsa2048 sign 1d >/dev/null 2>&1
gpg --batch --armor --export > "$work/test-key.asc"
mkdir -p "$work/old" "$work/new" "$work/build/rpm"
for version in 0.1.0 0.2.0; do
    deb=$work/build/deb
    mkdir -p "$deb/DEBIAN"
    cat > "$deb/DEBIAN/control" <<EOF
Package: fastmash
Version: $version
Architecture: amd64
Maintainer: Publication test <test@example.invalid>
Description: Package repository publication fixture
EOF
    dpkg-deb --build "$deb" "$work/build/fastmash_${version}_amd64.deb" >/dev/null
    cat > "$work/build/fixture.spec" <<EOF
Name: fastmash
Version: $version
Release: 1
Summary: Package repository publication fixture
License: MIT OR Apache-2.0
%description
Package repository publication fixture.
%install
mkdir -p %{buildroot}/usr/share/fastmash
echo $version > %{buildroot}/usr/share/fastmash/publication-fixture
%files
/usr/share/fastmash/publication-fixture
EOF
    rpmbuild --quiet -bb --define "_topdir $work/build/rpm" "$work/build/fixture.spec"
    if [ "$version" = 0.1.0 ]; then
        destination=$work/old
    else
        cp -a "$work/old/." "$work/new/"
        destination=$work/new
    fi
    bash "$here/package-repository.sh" "$destination" "$work/test-key.asc" \
        "$work/build/fastmash_${version}_amd64.deb" \
        "$work/build/rpm/RPMS/x86_64/fastmash-${version}-1.x86_64.rpm"
done
# Check the same-generation repair while the disposable signing key is live.
target=${CARGO_TARGET_DIR:-$here/../target}
checker=${FASTMASH_FIXTURE_REPAIR_CHECKER:-$target/debug/fastmash-test-package-fixture-repair}
if [ ! -x "$checker" ]; then
    echo 'prepare the fixture checker with cargo build --locked --manifest-path release/publication-test-server/Cargo.toml --target-dir target --bins' >&2
    exit 1
fi
"$checker" "$work" "$here/package-repository.sh"
