#!/bin/bash
# Upload a prepared repository to R2. Storage credentials use the AWS environment.
# usage: bash release/publish-package-repository.sh REPOSITORY BUCKET
set -euo pipefail
usage() {
    printf '%s\n' 'usage: bash release/publish-package-repository.sh REPOSITORY BUCKET' \
        'Upload a prepared local package repository to the supplied R2 bucket.' \
        'Uses AWS environment credentials and preserves earlier immutable snapshots.'
}
if [ "${1:-}" = --help ] || [ "${1:-}" = -h ]; then usage; exit 0; fi
if [ "$#" -ne 2 ]; then usage >&2; exit 1; fi
cd "$1"
bucket=$2
# Upload immutable packages, by-hash indexes and complete dnf snapshots first.
# Keep every earlier object for clients that fetched an older signed index or
# mirrorlist. Do not delete or overwrite an older snapshot during publication.
aws s3 sync --only-show-errors . "s3://$bucket" \
    --exclude 'apt/dists/stable/main/binary-amd64/Packages*' \
    --exclude 'apt/dists/stable/Release*' --exclude 'apt/dists/stable/InRelease' \
    --exclude 'rpm/mirrorlist' --exclude 'fastmash.*' \
    --cache-control 'public, max-age=31536000, immutable'
for path in fastmash.asc fastmash.gpg fastmash.repo \
    apt/dists/stable/main/binary-amd64/Packages apt/dists/stable/main/binary-amd64/Packages.gz \
    apt/dists/stable/Release apt/dists/stable/Release.gpg apt/dists/stable/InRelease \
    rpm/mirrorlist; do
    aws s3 cp --only-show-errors "$path" "s3://$bucket/$path" --cache-control 'no-cache'
done
