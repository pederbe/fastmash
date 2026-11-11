#!/bin/sh
# Install Fastmash from its GitHub releases.
#
#   curl -fsSL https://fastmash.io/install.sh | sh
#
# Downloads the latest release archive for this platform, checks its SHA-256
# checksum, and installs fastmash into ~/.local/bin. Older Linux archives also
# contain a sort supervisor, whose ownership is recorded for a later upgrade.
# It needs curl or wget, tar, gzip, and sha256sum (Linux) or shasum (macOS).
#
# Settings (environment variables):
#   FASTMASH_INSTALL_DIR  where to install (default: ~/.local/bin)
set -eu

# Internal test input for serving candidate archives from local fixture servers.
releases=${FASTMASH_RELEASES:-https://github.com/pederbe/fastmash/releases}
# HTTPS only, unless a custom FASTMASH_RELEASES says otherwise.
case $releases in
    https://*) secure="--proto =https --tlsv1.2" ;;
    *) secure="" ;;
esac

say() { printf '%s\n' "$*"; }
fail() { printf 'fastmash install: %s\n' "$*" >&2; exit 1; }
installation_failed() {
    failure_status=$?
    printf 'fastmash install: %s\n' "$*" >&2
    exit "$failure_status"
}
has() { command -v "$1" >/dev/null 2>&1; }

main() {
    if [ "$#" -ne 0 ]; then
        case "$1" in
            -h|--help)
                [ "$#" -eq 1 ] || fail "--help takes no other arguments."
                # Print the example's variable literally for the caller's shell.
                # shellcheck disable=SC2016
                printf '%s\n' \
                    'Install or update Fastmash from the latest release.' \
                    '' \
                    'Usage: sh install.sh [--help]' \
                    '' \
                    'The installer checks the download checksum and installs into ~/.local/bin.' \
                    'Run it again to update the same installation.' \
                    '' \
                    'Quick install:' \
                    '  curl -fsSL https://fastmash.io/install.sh | sh' \
                    '' \
                    'Choose a writable installation directory:' \
                    '  curl -fsSL https://fastmash.io/install.sh | env FASTMASH_INSTALL_DIR="$HOME/bin" sh' \
                    '' \
                    'Show this help without installing:' \
                    '  curl -fsSL https://fastmash.io/install.sh | sh -s -- --help' \
                    '' \
                    'More methods, PATH setup and removal: https://fastmash.io/install.html'
                return
                ;;
            *) fail "unsupported argument '$1'. This installer takes no arguments. Use --help for usage." ;;
        esac
    fi
    system=$(uname -s)
    machine=$(uname -m)
    case "$system:$machine" in
        Linux:x86_64)
            target=x86_64-unknown-linux-musl
            programs=fastmash
            checksum_tool=sha256sum
            checksum() { sha256sum -c "$1"; }
            ;;
        Darwin:arm64)
            target=aarch64-apple-darwin
            programs=fastmash
            checksum_tool=shasum
            checksum() { shasum -a 256 -c "$1"; }
            has sw_vers || fail "cannot determine the macOS version. Make sure /usr/bin is on PATH and try again."
            major=$(sw_vers -productVersion) || installation_failed "could not read the macOS version. Run sw_vers -productVersion to check it, then try again."
            major=${major%%.*}
            case $major in
                ''|*[!0-9]*) fail "cannot determine the macOS version. Run sw_vers -productVersion to check it, then try again." ;;
            esac
            [ "$major" -ge 15 ] || fail "the macOS archive needs macOS 15 or later. Upgrade macOS before installing Fastmash."
            ;;
        *) fail "no archive for $system $machine. Supported downloads are Linux x86-64 and Apple Silicon macOS 15 or later. On Windows, run this installer inside WSL2. See https://fastmash.io/requirements.html" ;;
    esac
    for tool in tar gzip "$checksum_tool"; do
        has "$tool" || fail "install $tool, then run this installer again."
    done
    if has curl; then
        # Split these fixed curl options into separate arguments.
        # shellcheck disable=SC2086
        fetch() { curl $secure -fsSL -o "$2" "$1"; }
    elif has wget; then
        fetch() { wget -q -O "$2" "$1"; }
    else
        fail "install curl or wget, then run this installer again."
    fi

    version=$(latest_version)
    [ -n "$version" ] || fail "could not find the latest release at $releases. Check your connection and try again."
    case $version in
        *[!0-9A-Za-z.-]*|.*) fail "the release server returned an invalid version '$version'. No installation was changed. See https://fastmash.io/install.html" ;;
    esac
    dir=${FASTMASH_INSTALL_DIR:-$HOME/.local/bin}
    name=fastmash-v$version-$target

    tmp=$(mktemp -d) || installation_failed "could not create a download directory. Check that TMPDIR is writable and has free space, then try again."
    # An interrupt exits (a trapped INT alone would resume the script); the
    # EXIT trap then removes the download.
    trap 'rm -rf "$tmp"; if [ -n "${staging:-}" ]; then rm -rf "$staging"; fi' EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    say "Downloading Fastmash $version..."
    if ! fetch "$releases/download/v$version/$name.tar.gz" "$tmp/$name.tar.gz" 2>/dev/null; then
        # Older releases provided GNU archives. Never fall back after a
        # checksum failure: the selected archive must pass its own checksum.
        [ "$system" = Linux ] || fail "could not download release $version from $releases. Check your connection and try again. Your existing installation was not changed."
        if has ldd && ldd --version 2>&1 | grep -qi musl; then
            fail "could not download the static Linux archive for release $version. Check your connection and try again. Your musl system cannot use the older glibc archive. To build from source, see https://fastmash.io/install.html#with-cargo"
        fi
        target=x86_64-unknown-linux-gnu
        name=fastmash-v$version-$target
        fetch "$releases/download/v$version/$name.tar.gz" "$tmp/$name.tar.gz" ||
            fail "could not download release $version from $releases. Check your connection and try again. Your existing installation was not changed."
    fi
    fetch "$releases/download/v$version/$name.tar.gz.sha256" "$tmp/$name.tar.gz.sha256" ||
        fail "could not download the checksum for release $version. Check your connection and try again. Your existing installation was not changed."
    (cd "$tmp" && checksum "$name.tar.gz.sha256") ||
        fail "the download does not match its checksum. Retry the download. Your existing installation was not changed."
    tar -xzf "$tmp/$name.tar.gz" -C "$tmp" ||
        fail "could not unpack the archive. Check free space and retry the download. Your existing installation was not changed."

    if [ "$system" = Linux ] && { [ -e "$tmp/$name/fastmash-sort-supervisor" ] || [ -L "$tmp/$name/fastmash-sort-supervisor" ]; }; then
        programs="fastmash fastmash-sort-supervisor"
    fi

    fingerprint() {
        if [ "$system" = Linux ]; then
            sum=$(sha256sum "$1")
        else
            sum=$(shasum -a 256 "$1")
        fi
        printf '%s\n' "${sum%% *}"
    }

    # Validate every program before changing an existing installation.
    for program in $programs; do
        [ -f "$tmp/$name/$program" ] && [ ! -L "$tmp/$name/$program" ] ||
            fail "the archive has no regular $program executable. Your existing installation was not changed. Please report this release at https://github.com/pederbe/fastmash/issues"
    done

    mkdir -p "$dir" || installation_failed "could not create $dir. Choose a writable directory with FASTMASH_INSTALL_DIR. See https://fastmash.io/install.html#quick-install"
    staging=$(mktemp -d "$dir/.fastmash-install.XXXXXX") || installation_failed "could not create installation files in $dir. Check write permissions and free space, then try again."
    for program in $programs; do
        cp "$tmp/$name/$program" "$staging/$program" || installation_failed "could not copy $program into $dir. Check write permissions and free space, then try again."
        chmod 755 "$staging/$program" || installation_failed "could not make $program executable in $dir. Check the directory's permissions and try again."
    done
    ownership=$dir/.fastmash-supervisor.sha256
    if [ "$programs" != fastmash ]; then
        fingerprint "$staging/fastmash-sort-supervisor" > "$staging/ownership"
    fi
    for program in $programs; do
        # Rename, so a running copy is never overwritten in place.
        mv -f "$staging/$program" "$dir/$program" || installation_failed "could not replace $dir/$program. Check write permissions and try again."
    done
    if [ "$programs" = fastmash ]; then
        # Only remove a regular file whose bytes match this installer's receipt.
        # Retire it only after the new command is in place. Older installations
        # without a receipt and user replacements stay put.
        old=$dir/fastmash-sort-supervisor
        if [ -f "$old" ] && [ ! -L "$old" ] && [ -f "$ownership" ] && [ ! -L "$ownership" ] &&
            [ "$(fingerprint "$old")" = "$(cat "$ownership")" ]; then
            rm "$old"
            rm "$ownership"
        fi
    fi
    if [ -f "$staging/ownership" ]; then
        mv -f "$staging/ownership" "$ownership"
    fi

    say "Installed Fastmash $version in $dir."
    case ":$PATH:" in
        *":$dir:"*) say "Try it: printf '1\\n2\\n' | fastmash sum 1"
                   say "The result should be 3." ;;
        *) say "Add $dir to PATH using your shell's settings."
           say "For now, run the installed command directly: \"$dir/fastmash\" --help"
           say "PATH setup: https://fastmash.io/install.html#command-not-found" ;;
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
