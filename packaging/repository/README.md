# Signed apt and dnf repository

The signed repository at `packages.fastmash.io` distributes Fastmash's release
`.deb` and `.rpm` packages. See the
[installation guide](../../docs/src/install.md) for user setup.

The [Package repository workflow](../../.github/workflows/package-repository.yml)
verifies published packages, their checksums and build attestations without
rebuilding the executable. Debian packages remain unchanged. RPM header signing
must preserve header and payload digests. Installation, upgrade and removal
checks compare installed executables with the release's delivery manifest.

To retry delivery for a published tag:

```sh
gh workflow run package-repository.yml --ref vX.Y.Z -f tag=vX.Y.Z
```

Wait for the previous run to finish before dispatching another. Existing
packages and metadata snapshots remain available throughout publication.
Do not prune old indexes or snapshots as part of an update.

## Layout

```text
fastmash.asc, fastmash.gpg   release signing key, armored and binary
fastmash.repo                dnf repository definition
apt/dists/stable/            InRelease, Release, Release.gpg, main/binary-amd64/Packages
apt/dists/stable/main/binary-amd64/by-hash/   retained SHA256 and SHA512 indexes
apt/pool/main/f/fastmash/    every published .deb
rpm/                         every published .rpm, signed
rpm/snapshots/<sha256>/repodata/   permanent metadata, with repomd.xml.asc
rpm/mirrorlist               URL of the current dnf snapshot
```

Package files, apt By-Hash indexes and dnf snapshots are immutable. Current apt
metadata, dnf mirrorlist, keys and `fastmash.repo` use `Cache-Control: no-cache`.
APT fetches the index identified by its signed `InRelease`. DNF fetches the
snapshot named by its mirrorlist. Earlier snapshots remain available for
clients already reading them. An interrupted publication can leave apt and dnf
showing different versions while each remains internally consistent.

## Signing and verification

The committed [RSA public key](../../release/package-signing-key.asc) has this
fingerprint:

```text
76C1 7E23 B7D3 F8FD ED36 D31A 7BAC 7AF6 644B D290
```

Verify it with:

```sh
gpg --show-keys --fingerprint release/package-signing-key.asc
```

RSA keeps RHEL and AlmaLinux 8 able to verify RPM signatures. The key has no
expiry because apt users keep their own copy and would not receive an extended
expiry automatically. Local tests use a separate throwaway key. Required
workflow credentials and permissions are defined in the
[publication workflow](../../.github/workflows/package-repository.yml).

## Checking locally

Run from the repository root on Linux with Rust, Docker, curl, the GitHub CLI,
`jq`, GnuPG, `dpkg-deb`, `apt-ftparchive`, `rpm`, `rpmsign` and `createrepo_c`.
On Debian/Ubuntu, the repository tools come from `apt-utils`, `createrepo-c`
and `rpm`, as in the maintained workflow. Use `CONTAINER_ENGINE=podman` for
Podman instead of Docker.

This Bash example downloads and verifies the latest published archive and both
packages. It signs a disposable local repository with a throwaway key, then
checks installed bytes and documentation in disposable clients. It does not
upload packages or modify the published repository.

```bash
bash -euo pipefail <<'EOF_CHECK'
work=$(mktemp -d /var/tmp/fastmash-repository-check.XXXXXX)
trap 'gpgconf --kill gpg-agent; rm -rf -- "$work"' EXIT
export GNUPGHOME="$work/gnupg"
mkdir -m 700 "$GNUPGHOME"
mkdir "$work/assets" "$work/repository"
assets="$work/assets"
tag=$(gh release view --repo pederbe/fastmash --json tagName --jq .tagName)
version=${tag#v}
gh api "repos/pederbe/fastmash/releases/tags/$tag" > "$work/release.json"
target=x86_64-unknown-linux-gnu
if jq -e --arg name "fastmash-$tag-x86_64-unknown-linux-musl.tar.gz" \
  '.assets | any(.name == $name)' "$work/release.json" >/dev/null; then
  target=x86_64-unknown-linux-musl
fi
name="fastmash-$tag-$target"
for asset in "$name.tar.gz" "fastmash_${version}_amd64.deb" "fastmash-${version}-1.x86_64.rpm"; do
  for file in "$asset" "$asset.sha256"; do
    curl --proto '=https' -fsSLo "$assets/$file" \
      "https://github.com/pederbe/fastmash/releases/download/$tag/$file"
  done
  (cd "$assets" && sha256sum -c "$asset.sha256")
  gh attestation verify "$assets/$asset" --repo pederbe/fastmash \
    --source-ref "refs/tags/$tag" --signer-workflow pederbe/fastmash/.github/workflows/release.yml
done
tar -xzf "$assets/$name.tar.gz" --no-same-owner -C "$assets" \
  "$name/README.md" "$name/CHANGELOG.md" "$name/LICENSE-MIT" \
  "$name/LICENSE-APACHE" "$name/THIRD-PARTY-LICENSES.md"
if tar -tzf "$assets/$name.tar.gz" "$name/binary.sha256" >/dev/null 2>&1; then
  tar -xOf "$assets/$name.tar.gz" "$name/binary.sha256" > "$work/delivery.sha256"
else
  git fetch --depth 1 https://github.com/pederbe/fastmash.git "refs/tags/$tag:refs/tags/$tag"
  manifest=release/delivery.sha256
  git cat-file -e "$tag:$manifest" 2>/dev/null || manifest=release/qualified.sha256
  git show "$tag:$manifest" > "$work/delivery.sha256"
fi
gpg --batch --pinentry-mode loopback --passphrase '' --quick-gen-key \
  'Repository test <test@example.invalid>' rsa2048 sign 1d
gpg --batch --armor --export > "$work/test-key.asc"
cargo build --locked --manifest-path release/publication-test-server/Cargo.toml --target-dir target --bins
bash release/package-repository.sh "$work/repository" "$work/test-key.asc" \
  "$assets/fastmash_${version}_amd64.deb" "$assets/fastmash-${version}-1.x86_64.rpm"
bash release/test-package-repository.sh "$work/repository" "$version" "$work/delivery.sha256" \
  --expected-docs "$assets/$name"
EOF_CHECK
```

`--expected-docs` compares all five installed documents with independently
verified release bytes. If an existing local repository also contains an earlier
version, pass it after `QUALIFIED` and before `--expected-docs` to check an upgrade.

Set `INSTALLATION_TEST_EVIDENCE` to a new directory to retain command output,
statuses and container receipts. Each run needs its own directory.

## Publication regression checks

These checks use fixture packages and a throwaway key. They need the repository
tools above and the Debian 12 and AlmaLinux 8 container images.

```sh
cargo build --locked --manifest-path release/publication-test-server/Cargo.toml --target-dir target --bins
bash release/package-publication-fixtures.sh /var/tmp/publication-fixtures
bash release/test-package-publication.sh /var/tmp/publication-fixtures/old /var/tmp/publication-fixtures/new
```

The test runs the real upload script against local HTTP storage. It checks both
package managers after every object write and interleaves publication with an
active client. It requires no production storage credentials.

Set `PUBLICATION_TEST_EVIDENCE` to a new directory to retain server logs, stage
output and `completed.tsv`, including completed stages. Without it, cleanup
removes temporary logs when the test exits.
