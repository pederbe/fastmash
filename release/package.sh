#!/bin/bash
# Build the .deb and .rpm packages from the qualified release binaries.
#
# usage: release/package.sh VERSION BIN_DIR DOC_DIR OUT_DIR
#
# BIN_DIR holds fastmash and fastmash-sort-supervisor, DOC_DIR the README,
# changelog, licenses and THIRD-PARTY-LICENSES.md. Both packages install the
# programs into /usr/bin, unchanged: the script checks that the binaries in
# each package are byte-identical to BIN_DIR's. Needs dpkg-deb, and rpmbuild
# and rpm2cpio for the .rpm (the rpm package on Debian and Ubuntu).
set -euo pipefail

version=$1 bin=$(realpath "$2") docs=$(realpath "$3") out=$(realpath "$4")
summary="Fast command-line statistics on text data, compatible with GNU datamash"
homepage="https://fastmash.io"
programs=(fastmash fastmash-sort-supervisor)
documents=(README.md CHANGELOG.md THIRD-PARTY-LICENSES.md LICENSE-MIT LICENSE-APACHE)
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$(stat -c %Y "$bin/fastmash")}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"

same() { # PACKAGE_ROOT: the packaged programs equal the qualified ones
    for program in "${programs[@]}"; do
        cmp -s "$bin/$program" "$1/usr/bin/$program" ||
            { echo "$program changed during packaging" >&2; exit 1; }
    done
}

# Debian and Ubuntu
deb=$work/deb
for program in "${programs[@]}"; do install -Dm755 "$bin/$program" "$deb/usr/bin/$program"; done
for document in "${documents[@]}"; do install -Dm644 "$docs/$document" "$deb/usr/share/doc/fastmash/$document"; done
cat > "$deb/usr/share/doc/fastmash/copyright" <<EOF
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: fastmash
Source: https://github.com/pederbe/fastmash

Files: *
Copyright: Peder Bergan
License: MIT or Apache-2.0
 See /usr/share/doc/fastmash/LICENSE-MIT and LICENSE-APACHE; bundled
 dependencies are listed in THIRD-PARTY-LICENSES.md.
EOF
mkdir -p "$deb/DEBIAN"
cat > "$deb/DEBIAN/control" <<EOF
Package: fastmash
Version: $version
Architecture: amd64
Maintainer: Peder Bergan <https://github.com/pederbe/fastmash/issues>
Depends: libc6 (>= 2.28)
Section: utils
Priority: optional
Installed-Size: $(du -sk --exclude=DEBIAN "$deb" | cut -f1)
Homepage: $homepage
Description: $summary
 Fastmash summarizes, groups and reshapes tab-separated or delimited text
 with the GNU datamash command language.
EOF
same "$deb"
dpkg-deb --root-owner-group -Zxz --build "$deb" "$out/fastmash_${version}_amd64.deb" >/dev/null
check=$work/deb-check
dpkg-deb -x "$out/fastmash_${version}_amd64.deb" "$check"
same "$check"
echo "$out/fastmash_${version}_amd64.deb"

# Fedora, RHEL and openSUSE
if ! command -v rpmbuild >/dev/null; then
    echo "rpmbuild not found; skipping the .rpm" >&2
    exit 0
fi
top=$work/rpm
mkdir -p "$top"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
cat > "$top/SPECS/fastmash.spec" <<EOF
# Prebuilt, qualified binaries: no stripping, debug packages or other rewriting.
%global debug_package %{nil}
%global __os_install_post %{nil}
%global _build_id_links none

Name:           fastmash
Version:        $version
Release:        1
Summary:        $summary
License:        MIT OR Apache-2.0
URL:            $homepage
BuildArch:      x86_64
AutoReqProv:    no
Requires:       glibc >= 2.28

%description
Fastmash summarizes, groups and reshapes tab-separated or delimited text
with the GNU datamash command language.

%install
install -Dm755 $bin/fastmash %{buildroot}/usr/bin/fastmash
install -Dm755 $bin/fastmash-sort-supervisor %{buildroot}/usr/bin/fastmash-sort-supervisor
mkdir -p %{buildroot}%{_docdir}/fastmash %{buildroot}%{_licensedir}/fastmash
install -m644 $docs/README.md $docs/CHANGELOG.md %{buildroot}%{_docdir}/fastmash/
install -m644 $docs/LICENSE-MIT $docs/LICENSE-APACHE $docs/THIRD-PARTY-LICENSES.md %{buildroot}%{_licensedir}/fastmash/

%files
/usr/bin/fastmash
/usr/bin/fastmash-sort-supervisor
%{_docdir}/fastmash
%{_licensedir}/fastmash
EOF
rpmbuild --quiet -bb --define "_topdir $top" --define "use_source_date_epoch_as_buildtime 1" \
    --define "clamp_mtime_to_source_date_epoch 1" "$top/SPECS/fastmash.spec"
rpm=$top/RPMS/x86_64/fastmash-$version-1.x86_64.rpm
check=$work/rpm-check
mkdir -p "$check"
(cd "$check" && rpm2cpio "$rpm" | cpio -idm --quiet)
same "$check"
cp "$rpm" "$out/"
echo "$out/fastmash-$version-1.x86_64.rpm"
