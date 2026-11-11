#!/usr/bin/env bash
# Build the fastmash.io website into docs/book: the build command of the
# Cloudflare Workers Builds deployment (its deploy command, `npx wrangler
# deploy`, then uploads docs/book; see wrangler.jsonc) and of the docs CI.
# Needs bash, curl, Python 3 and npm; no Rust and no browser, because diagram
# renders are committed (scripts/build_site.py). Set MDBOOK to use an
# installed mdBook instead of the pinned release binary.
set -euo pipefail
if [ "${1:-}" = --help ] || [ "${1:-}" = -h ]; then
    printf '%s\n' 'usage: bash scripts/build_site.sh [local|--cloudflare]' \
        'Build the documentation website into docs/book without deploying it.' \
        'local is the default. --cloudflare uses published-release download links.' \
        'Set MDBOOK to use an installed mdBook executable.'
    exit 0
fi
if [ "$#" -gt 1 ]; then echo 'usage: bash scripts/build_site.sh [local|--cloudflare]' >&2; exit 2; fi
cd "$(dirname "$0")/.."
mode=${1:-local}
case "$mode" in local|--cloudflare) ;; *) echo 'usage: bash scripts/build_site.sh [local|--cloudflare]' >&2; exit 2 ;; esac

version=0.5.4
digest=3f28de05dafca9d0f2eab99c662116b0e37b89b1d96a08f8f430b9eeae958cd7
if [ -z "${MDBOOK:-}" ]; then
    tools=$(mktemp -d)
    trap 'rm -rf "$tools"' EXIT
    archive="$tools/mdbook.tar.gz"
    curl -fsSL --proto '=https' --retry 3 -o "$archive" \
        "https://github.com/rust-lang/mdBook/releases/download/v$version/mdbook-v$version-x86_64-unknown-linux-gnu.tar.gz"
    echo "$digest  $archive" | sha256sum -c --quiet
    tar -xzf "$archive" -C "$tools" mdbook
    MDBOOK=$tools/mdbook
fi

# Wrangler for the deploy step, at the version package-lock.json pins; no
# install scripts, so Puppeteer fetches no browser.
npm ci --ignore-scripts --no-audit --no-fund
python3 -m unittest discover -s scripts -p 'test_build_site.py'
if [ "$mode" = --cloudflare ]; then
    # Release publication advances this pointer. Documentation still builds
    # from main, independently of the version used by the download links.
    release_ref=$(git ls-remote origin refs/heads/release)
    if [ -n "$release_ref" ]; then
        git fetch --depth=1 origin refs/heads/release:refs/remotes/origin/release
        FASTMASH_SITE_RELEASE_VERSION=$(git show refs/remotes/origin/release:Cargo.toml |
            sed -n 's/^version = "\([^"]*\)"$/\1/p' | head -1)
        # Published tags before shared workspace versioning keep it in the CLI.
        if [ -z "$FASTMASH_SITE_RELEASE_VERSION" ]; then
            FASTMASH_SITE_RELEASE_VERSION=$(git show refs/remotes/origin/release:crates/cli/Cargo.toml |
                sed -n 's/^version = "\([^"]*\)"$/\1/p' | head -1)
        fi
        export FASTMASH_SITE_RELEASE_VERSION
    fi
fi
"$MDBOOK" build docs
FASTMASH_DIAGRAMS=committed python3 scripts/build_site.py docs/book
