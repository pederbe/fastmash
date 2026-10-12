#!/bin/bash
# Add release packages to the signed apt and dnf repository and re-sign it.
#
# usage: bash release/package-repository.sh REPOSITORY PUBLIC_KEY PACKAGE...
#
# REPOSITORY is a local copy of the published repository (empty the first
# time). PUBLIC_KEY is the committed release signing key; its secret key must
# be in the caller's GnuPG keyring, and GPG_PASSPHRASE holds its passphrase
# when it has one. Each PACKAGE is a verified .deb or .rpm from a release.
#
# The .deb is published unchanged. The .rpm gets a header signature; its
# header and payload digests are checked to be unchanged by signing. Versions
# already in the repository are kept, and a package that differs from the one
# already published under the same name is refused. Needs gpg, gpgv,
# apt-ftparchive, rpm, rpmsign and createrepo_c.
set -euo pipefail

repository=$(realpath "$1") public_key=$(realpath "$2")
shift 2
test $# -gt 0 || { echo "no packages given" >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# The signing key is the committed one, and its secret part is available.
mapfile -t fingerprints < <(gpg --show-keys --with-colons "$public_key" |
    awk -F: '$1 == "pub" {primary = 1; next} $1 == "fpr" && primary {print $10; primary = 0}')
test "${#fingerprints[@]}" -eq 1 ||
    { echo "$public_key must hold exactly one primary key" >&2; exit 1; }
fingerprint=${fingerprints[0]}
gpg --batch --list-secret-keys "$fingerprint" >/dev/null 2>&1 ||
    { echo "the secret key for $fingerprint is not in the keyring" >&2; exit 1; }

sign() { # GPG_ARGUMENTS...: sign with the release key, unattended
    if [ -n "${GPG_PASSPHRASE:-}" ]; then
        gpg --batch --yes --pinentry-mode loopback --passphrase-fd 3 \
            --local-user "$fingerprint" --digest-algo SHA512 "$@" 3<<<"$GPG_PASSPHRASE"
    else
        gpg --batch --yes --local-user "$fingerprint" --digest-algo SHA512 "$@"
    fi
}

# An rpm database that trusts only the release key, to check RPM signatures.
rpmdb=$work/rpmdb
mkdir "$rpmdb"
rpmkeys --dbpath "$rpmdb" --import "$public_key"
rpm_signed() { # RPM: signed by the release key, with intact digests
    rpmkeys --dbpath "$rpmdb" --checksig "$1" | grep -q ': digests signatures OK$'
}
rpm_identity() { # RPM: the digests that header signing must not change
    rpm --dbpath "$rpmdb" -qp --nosignature --qf '%{SHA256HEADER} %{PAYLOADDIGEST}\n' "$1"
}

pool=$repository/apt/pool/main/f/fastmash
mkdir -p "$pool" "$repository/rpm"
for package in "$@"; do
    name=$(basename "$package")
    case $name in
    fastmash_*_amd64.deb)
        if [ -e "$pool/$name" ]; then
            cmp -s "$package" "$pool/$name" ||
                { echo "$name differs from the published package" >&2; exit 1; }
        else
            cp "$package" "$pool/$name"
        fi
        ;;
    fastmash-*.x86_64.rpm)
        cp "$package" "$work/$name"
        before=$(rpm_identity "$work/$name")
        if [ -n "${GPG_PASSPHRASE:-}" ]; then
            rpmsign --addsign --define "_gpg_name $fingerprint" \
                --define "_gpg_sign_cmd_extra_args --batch --pinentry-mode loopback --passphrase-fd 3" \
                "$work/$name" 3<<<"$GPG_PASSPHRASE" >/dev/null
        else
            rpmsign --addsign --define "_gpg_name $fingerprint" \
                --define "_gpg_sign_cmd_extra_args --batch" "$work/$name" >/dev/null
        fi
        test "$(rpm_identity "$work/$name")" = "$before" ||
            { echo "signing changed the contents of $name" >&2; exit 1; }
        rpm_signed "$work/$name" || { echo "$name signature check failed" >&2; exit 1; }
        if [ -e "$repository/rpm/$name" ]; then
            test "$(rpm_identity "$repository/rpm/$name")" = "$before" ||
                { echo "$name differs from the published package" >&2; exit 1; }
        else
            mv "$work/$name" "$repository/rpm/$name"
        fi
        ;;
    *)
        echo "$name is not a Fastmash .deb or .rpm" >&2
        exit 1
        ;;
    esac
done

# The indexers below list every package file they find, so the repository may
# hold only Fastmash packages whose metadata matches their names, and every
# RPM must carry a valid release-key signature.
version_pattern='[0-9]+\.[0-9]+\.[0-9]+'
while IFS= read -r -d '' file; do
    name=${file#"$repository"/}
    if [[ $name =~ ^apt/pool/main/f/fastmash/fastmash_($version_pattern)_amd64\.deb$ ]]; then
        test "$(dpkg-deb -f "$file" Package Version Architecture | tr '\n' ' ')" = \
            "Package: fastmash Version: ${BASH_REMATCH[1]} Architecture: amd64 " ||
            { echo "$name does not match its control fields" >&2; exit 1; }
    elif [[ $name =~ ^rpm/fastmash-($version_pattern)-1\.x86_64\.rpm$ ]]; then
        test "$(rpm --dbpath "$rpmdb" -qp --nosignature --qf '%{NAME} %{VERSION} %{ARCH}' "$file")" = \
            "fastmash ${BASH_REMATCH[1]} x86_64" ||
            { echo "$name does not match its header" >&2; exit 1; }
        rpm_signed "$file" || { echo "$name is not signed by the release key" >&2; exit 1; }
    elif [[ ! $name =~ ^(apt/dists|rpm/repodata)/ ]] &&
        [[ ! $name =~ ^fastmash\.(asc|gpg|repo)$ ]]; then
        echo "unexpected file in the repository: $name" >&2
        exit 1
    fi
done < <(find "$repository" -type f -print0)

# apt: Packages lists every version in the pool; InRelease and Release.gpg
# sign the Release file that holds the Packages digests.
apt=$repository/apt
dists=$apt/dists/stable
mkdir -p "$dists/main/binary-amd64"
(cd "$apt" && apt-ftparchive packages pool) > "$dists/main/binary-amd64/Packages"
gzip -9nkf "$dists/main/binary-amd64/Packages"
rm -f "$dists/Release" "$dists/InRelease" "$dists/Release.gpg"
apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=Fastmash \
    -o APT::FTPArchive::Release::Label=Fastmash \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Codename=stable \
    -o APT::FTPArchive::Release::Architectures=amd64 \
    -o APT::FTPArchive::Release::Components=main \
    -o "APT::FTPArchive::Release::Description=Fastmash release packages" \
    release "$dists" > "$work/Release"
mv "$work/Release" "$dists/Release"
sign --clearsign --output "$dists/InRelease" "$dists/Release"
sign --armor --detach-sign --output "$dists/Release.gpg" "$dists/Release"

# dnf: repomd.xml.asc signs the metadata index.
createrepo_c --quiet --update "$repository/rpm"
sign --armor --detach-sign --output "$repository/rpm/repodata/repomd.xml.asc" \
    "$repository/rpm/repodata/repomd.xml"

# The public key in both forms, and the dnf repository definition.
cp "$public_key" "$repository/fastmash.asc"
gpg --batch --yes --dearmor --output "$repository/fastmash.gpg" "$public_key"
cat > "$repository/fastmash.repo" <<'EOF'
[fastmash]
name=Fastmash
baseurl=https://packages.fastmash.io/rpm
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=https://packages.fastmash.io/fastmash.asc
EOF

# Check the signatures as a client would, with only the published key.
gpgv --keyring "$repository/fastmash.gpg" "$dists/InRelease" 2>/dev/null
gpgv --keyring "$repository/fastmash.gpg" "$dists/Release.gpg" "$dists/Release" 2>/dev/null
gpgv --keyring "$repository/fastmash.gpg" "$repository/rpm/repodata/repomd.xml.asc" \
    "$repository/rpm/repodata/repomd.xml" 2>/dev/null
echo "$repository: $(find "$pool" -name '*.deb' | wc -l) .deb and $(find "$repository/rpm" -maxdepth 1 -name '*.rpm' | wc -l) .rpm, signed by $fingerprint"
