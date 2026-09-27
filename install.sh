#!/bin/sh
# Install Fastmash from its GitHub releases.
#
#   curl -fsSL https://fastmash.io/install.sh | sh
#
# Downloads the latest release for Linux x86-64, checks its SHA-256 checksum,
# and installs `fastmash` and `fastmash-sort-supervisor` into ~/.local/bin.
# It needs curl or wget, tar, gzip and sha256sum, and changes nothing else.
#
# Settings (environment variables):
#   FASTMASH_VERSION      a release such as 0.1.0 (default: the latest)
#   FASTMASH_INSTALL_DIR  where to install (default: ~/.local/bin)
#   FASTMASH_RELEASES     release download base URL, for mirrors
#                         (default: https://github.com/pederbe/fastmash/releases)
set -eu

releases=${FASTMASH_RELEASES:-https://github.com/pederbe/fastmash/releases}
target=x86_64-unknown-linux-gnu
# HTTPS only, unless a custom FASTMASH_RELEASES says otherwise.
case $releases in
    https://*) secure="--proto =https --tlsv1.2" ;;
    *) secure="" ;;
esac

say() { printf '%s\n' "$*"; }
fail() { printf 'fastmash install: %s\n' "$*" >&2; exit 1; }
has() { command -v "$1" >/dev/null 2>&1; }

main() {
    [ "$(uname -s)" = Linux ] ||
        fail "Fastmash runs on Linux x86-64 (and Windows through WSL2); this is $(uname -s)."
    [ "$(uname -m)" = x86_64 ] ||
        fail "prebuilt binaries are for x86-64; this machine is $(uname -m). Fastmash supports only x86-64 Linux with glibc for now: https://fastmash.io/requirements.html"
    if has ldd && ldd --version 2>&1 | grep -qi musl; then
        fail "the prebuilt binaries need glibc; this system uses musl. Fastmash supports only x86-64 Linux with glibc for now: https://fastmash.io/requirements.html"
    fi
    for tool in tar gzip sha256sum; do
        has "$tool" || fail "please install $tool first."
    done
    if has curl; then
        fetch() { curl $secure -fsSL -o "$2" "$1"; }
    elif has wget; then
        fetch() { wget -q -O "$2" "$1"; }
    else
        fail "please install curl or wget first."
    fi

    version=${FASTMASH_VERSION:-$(latest_version)}
    version=${version#v}
    [ -n "$version" ] || fail "could not find the latest release; set FASTMASH_VERSION, such as 0.1.0."
    dir=${FASTMASH_INSTALL_DIR:-$HOME/.local/bin}
    name=fastmash-v$version-$target

    tmp=$(mktemp -d)
    # An interrupt exits (a trapped INT alone would resume the script); the
    # EXIT trap then removes the download.
    trap 'rm -rf "$tmp"' EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    say "Downloading Fastmash $version..."
    fetch "$releases/download/v$version/$name.tar.gz" "$tmp/$name.tar.gz" ||
        fail "could not download release $version from $releases."
    fetch "$releases/download/v$version/$name.tar.gz.sha256" "$tmp/$name.tar.gz.sha256" ||
        fail "could not download the checksum for release $version."
    (cd "$tmp" && sha256sum -c --quiet "$name.tar.gz.sha256") ||
        fail "the download does not match its checksum; nothing was installed."
    tar -xzf "$tmp/$name.tar.gz" -C "$tmp"

    mkdir -p "$dir"
    for program in fastmash fastmash-sort-supervisor; do
        # Copy then rename, so a running copy is never overwritten in place.
        cp "$tmp/$name/$program" "$dir/.$program.new"
        chmod 755 "$dir/.$program.new"
        mv -f "$dir/.$program.new" "$dir/$program"
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
        url=$(curl $secure -fsSLI -o /dev/null -w '%{url_effective}' "$releases/latest") || return 0
    else
        url=$(wget -q -S --spider "$releases/latest" 2>&1 | sed -n 's/^ *[Ll]ocation: *//p' | tail -n 1) || return 0
    fi
    case $url in
        */tag/v*) printf '%s\n' "${url##*/tag/v}" | tr -d '\r' ;;
    esac
}

main "$@"
