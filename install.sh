#!/bin/sh
# Install Fastmash from its GitHub releases.
#
#   curl -fsSL https://fastmash.io/install.sh | sh
#
# Downloads the selected platform archive, checks its SHA-256 checksum,
# and installs its programs into ~/.local/bin. macOS archives contain only
# `fastmash`; Linux archives also contain `fastmash-sort-supervisor`.
# It needs curl or wget, tar, gzip, and sha256sum (Linux) or shasum (macOS).
#
# Settings (environment variables):
#   FASTMASH_VERSION      a release such as 0.1.0 (default: the latest)
#   FASTMASH_INSTALL_DIR  where to install (default: ~/.local/bin)
#   FASTMASH_RELEASES     release download base URL, for mirrors
#                         (default: https://github.com/pederbe/fastmash/releases)
set -eu

releases=${FASTMASH_RELEASES:-https://github.com/pederbe/fastmash/releases}
# HTTPS only, unless a custom FASTMASH_RELEASES says otherwise.
case $releases in
    https://*) secure="--proto =https --tlsv1.2" ;;
    *) secure="" ;;
esac

say() { printf '%s\n' "$*"; }
fail() { printf 'fastmash install: %s\n' "$*" >&2; exit 1; }
has() { command -v "$1" >/dev/null 2>&1; }

main() {
    system=$(uname -s)
    machine=$(uname -m)
    case "$system:$machine" in
        Linux:x86_64)
            target=x86_64-unknown-linux-gnu
            programs="fastmash fastmash-sort-supervisor"
            checksum_tool=sha256sum
            checksum() { sha256sum -c "$1"; }
            if has ldd && ldd --version 2>&1 | grep -qi musl; then
                fail "the Linux prebuilt binaries need glibc; this system uses musl. See https://fastmash.io/requirements.html"
            fi
            ;;
        Darwin:arm64)
            target=aarch64-apple-darwin
            programs=fastmash
            checksum_tool=shasum
            checksum() { shasum -a 256 -c "$1"; }
            has sw_vers || fail "cannot determine the macOS version."
            major=$(sw_vers -productVersion)
            major=${major%%.*}
            case $major in
                ''|*[!0-9]*) fail "cannot determine the macOS version." ;;
            esac
            [ "$major" -ge 15 ] || fail "the macOS archive needs macOS 15 or later."
            ;;
        *) fail "no archive for $system $machine; supported targets are Linux x86-64 with glibc and Apple Silicon macOS 15 or later. See https://fastmash.io/requirements.html" ;;
    esac
    for tool in tar gzip "$checksum_tool"; do
        has "$tool" || fail "please install $tool first."
    done
    if has curl; then
        # Split these fixed curl options into separate arguments.
        # shellcheck disable=SC2086
        fetch() { curl $secure -fsSL -o "$2" "$1"; }
    elif has wget; then
        fetch() { wget -q -O "$2" "$1"; }
    else
        fail "please install curl or wget first."
    fi

    version=${FASTMASH_VERSION:-$(latest_version)}
    version=${version#v}
    [ -n "$version" ] || fail "could not find the latest release; set FASTMASH_VERSION, such as 0.1.0."
    case $version in
        *[!0-9A-Za-z.-]*|.*) fail "invalid release version: $version" ;;
    esac
    dir=${FASTMASH_INSTALL_DIR:-$HOME/.local/bin}
    name=fastmash-v$version-$target

    tmp=$(mktemp -d)
    # An interrupt exits (a trapped INT alone would resume the script); the
    # EXIT trap then removes the download.
    trap 'rm -rf "$tmp"; if [ -n "${staging:-}" ]; then rm -rf "$staging"; fi' EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    say "Downloading Fastmash $version..."
    fetch "$releases/download/v$version/$name.tar.gz" "$tmp/$name.tar.gz" ||
        fail "could not download release $version from $releases."
    fetch "$releases/download/v$version/$name.tar.gz.sha256" "$tmp/$name.tar.gz.sha256" ||
        fail "could not download the checksum for release $version."
    (cd "$tmp" && checksum "$name.tar.gz.sha256") ||
        fail "the download does not match its checksum; nothing was installed."
    tar -xzf "$tmp/$name.tar.gz" -C "$tmp" ||
        fail "could not unpack the archive; nothing was installed."

    # Validate every program before changing an existing installation.
    for program in $programs; do
        [ -f "$tmp/$name/$program" ] && [ ! -L "$tmp/$name/$program" ] ||
            fail "the archive has no regular $program executable; nothing was installed."
    done

    mkdir -p "$dir"
    staging=$(mktemp -d "$dir/.fastmash-install.XXXXXX")
    for program in $programs; do
        cp "$tmp/$name/$program" "$staging/$program"
        chmod 755 "$staging/$program"
    done
    for program in $programs; do
        # Rename, so a running copy is never overwritten in place.
        mv -f "$staging/$program" "$dir/$program"
    done

    say "Installed Fastmash $version in $dir."
    case ":$PATH:" in
        *":$dir:"*) say "Try it: printf '1\\n2\\n' | fastmash sum 1" ;;
        *) say "Add $dir to your PATH, for example:"
           say "  echo 'export PATH=\"$dir:\$PATH\"' >> ~/.profile && . ~/.profile" ;;
    esac
}

# The newest release, from the tag that $releases/latest redirects to.
latest_version() {
    if has curl; then
        # Split the same fixed HTTPS options used by fetch.
        # shellcheck disable=SC2086
        url=$(curl $secure -fsSLI -o /dev/null -w '%{url_effective}' "$releases/latest") || return 0
    else
        url=$(wget -q -S --spider "$releases/latest" 2>&1 | sed -n 's/^ *[Ll]ocation: *//p' | tail -n 1) || return 0
    fi
    case $url in
        */tag/v*) printf '%s\n' "${url##*/tag/v}" | tr -d '\r' ;;
    esac
}

main "$@"
