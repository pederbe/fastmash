# Signed apt and dnf repository

This directory prepares the release `.deb` and `.rpm` packages for the signed
apt repository (Debian, Ubuntu) and a signed dnf repository (RHEL, AlmaLinux,
Fedora) at `packages.fastmash.io`. Published versions remain available when a
new release is added.
Public-domain installation, upgrade and removal are verified on Debian 12,
Ubuntu 24.04 and AlmaLinux 8 with signature checks enabled and installed programs
matching each release's delivery checksums. Users who add it receive new
versions through `apt upgrade` and `dnf upgrade`. See the
[installation guide](../../docs/src/install.md) for setup.

The [Package repository workflow](../../.github/workflows/package-repository.yml)
publishes it after a release is published. It downloads that release's
packages, checks their SHA-256 files and their build attestations from
`release.yml`, and never rebuilds the programs. Then it:

1. syncs the current repository from R2, so every earlier version stays
   available, and checks every `.deb` already there against its release's
   SHA-256 file (storage credentials alone cannot add a package);
2. runs `release/package-repository.sh`, which publishes the `.deb` unchanged,
   adds a header signature to the `.rpm` (checking that its header and payload
   digests stay the same), refuses any file that is not a Fastmash package
   matching its name, and regenerates and signs the apt and dnf metadata;
3. runs `release/test-package-repository.sh`, which installs the version from
   the generated repository in offline Debian 12, Ubuntu 24.04 and AlmaLinux 8
   containers with signature checks on, runs `release/check-installed.sh`
   against the tag's delivery checksums and removes it. When the version is
   the newest, it installs the previous version first and upgrades;
4. runs `release/publish-package-repository.sh` to upload packages, apt By-Hash
   indexes and complete dnf snapshots before advancing the signed apt index
   and dnf mirrorlist, then compares the published metadata with what it signed.

Release dispatches it at the published tag. To retry that delivery, run:
`gh workflow run package-repository.yml --ref vX.Y.Z -f tag=vX.Y.Z`.
The workflow uses the tag-restricted `packages` environment. Runs share one
concurrency group, and GitHub keeps only the newest pending run, so start each
dispatch after the previous run has finished.

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

Package files, apt By-Hash indexes and dnf snapshots are cached as immutable.
The current apt metadata, dnf mirrorlist, keys and `fastmash.repo` are served
with `Cache-Control: no-cache`.

APT advertises `Acquire-By-Hash: yes`, so clients fetch the exact index named
by the checksum in their signed `InRelease`. DNF reads a mirrorlist containing
one snapshot URL, then fetches that snapshot's index and signature. Its package
paths point back to the shared RPM pool. Earlier indexes and snapshots remain
available for clients already using them. An interrupted upload can leave apt
and dnf showing different release versions, but each remains consistent.

If publication fails, repeat the workflow for the same tag. Existing packages
and snapshots remain available, and advancing either current index is safe once
all its references have been uploaded. Do not prune old indexes or snapshots
as part of publication.

## One-time setup

1. **Signing key.** The dedicated RSA 4096 public key is committed in
   [`release/package-signing-key.asc`](../../release/package-signing-key.asc).
   Its fingerprint is:

   ```text
   76C1 7E23 B7D3 F8FD ED36 D31A 7BAC 7AF6 644B D290
   ```

   Verify the committed key with:

   ```sh
   gpg --show-keys --fingerprint release/package-signing-key.asc
   ```

   RSA keeps RHEL and AlmaLinux 8 able to check RPM signatures. The key does
   not expire, because apt users keep their own copy of it and would not
   receive an extended expiry. Keep an offline backup of the secret key and
   its revocation certificate (`openpgp-revocs.d/` in the GnuPG home), which is
   how the key is retired if it is ever exposed. Store the armored secret key
   and its passphrase in the protected environment below. The script refuses
   to sign with any other key. Publish the fingerprint on fastmash.io after
   the repository is live and checked.
2. **Storage.** In Cloudflare, activate R2 for the account if needed. This
   requires completing its dashboard billing setup, even when usage fits
   within the [Standard storage free tier](https://developers.cloudflare.com/r2/pricing/).
   Create the R2 bucket `fastmash-packages` with Standard storage, connect the
   custom domain `packages.fastmash.io` to it with TLS 1.2 or later, and create
   an R2 API token with Object Read & Write access to that bucket only. Use its
   Access Key ID and Secret Access Key for the environment secrets. Leave
   public `r2.dev` access disabled.
3. **Environment.** In the repository settings, create the `packages`
   environment without required reviewers, so verified releases publish
   automatically. Limit it to the `main`
   branch and `v*` tags, with these secrets:

   | Secret | Value |
   | --- | --- |
   | `PACKAGE_SIGNING_KEY` | armored secret key |
   | `PACKAGE_SIGNING_PASSPHRASE` | its passphrase |
   | `R2_ACCOUNT_ID` | Cloudflare account ID |
   | `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY` | the R2 token's S3 credentials |

4. **First publication.** First verify that clients cannot receive mismatched
   indexes and signatures during an interrupted publication. Then run the
   workflow for `v0.1.0`, followed by each later release in order, so the
   newest version is published last. Verify installation, upgrade and removal
   through the public domain with signature checks enabled before publishing
   the user instructions.

## Checking locally

With Rust, Docker, curl, the GitHub CLI, a throwaway key in `GNUPGHOME` and its
public key in a file, this example checks version 0.2.0. Its expected documents
come from the same version's verified Linux release archive.

```sh
bash -c '
set -euo pipefail
version=0.2.0
tag="v$version"
name="fastmash-${tag}-x86_64-unknown-linux-gnu"
assets=$(mktemp -d)
trap "rm -rf -- \"$assets\"" EXIT
for file in "$name.tar.gz" "$name.tar.gz.sha256"; do
  curl --proto "=https" -fsSLo "$assets/$file" \
    "https://github.com/pederbe/fastmash/releases/download/$tag/$file"
done
(cd "$assets" && sha256sum -c "$name.tar.gz.sha256")
gh attestation verify "$assets/$name.tar.gz" --repo pederbe/fastmash \
  --source-ref "refs/tags/$tag" --signer-workflow pederbe/fastmash/.github/workflows/release.yml
tar -xzf "$assets/$name.tar.gz" --no-same-owner -C "$assets" \
  "$name/README.md" "$name/CHANGELOG.md" "$name/LICENSE-MIT" \
  "$name/LICENSE-APACHE" "$name/THIRD-PARTY-LICENSES.md"
cargo build --locked --manifest-path release/publication-test-server/Cargo.toml --target-dir target --bins
GPG_PASSPHRASE=... bash release/package-repository.sh /tmp/repository test-key.asc \
  "fastmash_${version}_amd64.deb" "fastmash-${version}-1.x86_64.rpm"
manifest=release/delivery.sha256
git cat-file -e "$tag:$manifest" 2>/dev/null || manifest=release/qualified.sha256
git show "$tag:$manifest" > "$assets/delivery.sha256"
bash release/test-package-repository.sh /tmp/repository "$version" "$assets/delivery.sha256" \
  --expected-docs "$assets/$name"
'
```

`--expected-docs` compares all five installed documents with those independent
bytes and preserves only Fastmash's documentation in the disposable Debian and
Ubuntu clients. Calls without the option retain the executable, installation
and removal checks. If the repository also contains an earlier version, put it
after `QUALIFIED` and before `--expected-docs` to check its upgrade to the version
whose documents were supplied.

Publication regression checks use small fixture packages and a throwaway key.
They need the repository tools above, Rust, Docker and the Debian 12 and
AlmaLinux 8 container images:

```sh
cargo build --locked --manifest-path release/publication-test-server/Cargo.toml --target-dir target --bins
bash release/package-publication-fixtures.sh /tmp/publication-fixtures
bash release/test-package-publication.sh /tmp/publication-fixtures/old /tmp/publication-fixtures/new
```

The test runs the real upload script against local storage over HTTP. It checks
both package managers after every object write and publishes the next generation
between requests from an active client. The dedicated package-repository checks
workflow runs these tests for changes to the publication tooling, without R2
credentials. Set `CONTAINER_ENGINE=podman` to use Podman for either test script.

## For users

The [installation guide](../../docs/src/install.md) gives the verified setup
and key fingerprint. After verifying that fingerprint:

```sh
# Debian and Ubuntu
curl -fsSL https://packages.fastmash.io/fastmash.gpg | sudo tee /usr/share/keyrings/fastmash.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/fastmash.gpg] https://packages.fastmash.io/apt stable main" |
  sudo tee /etc/apt/sources.list.d/fastmash.list
sudo apt update && sudo apt install fastmash

# RHEL, AlmaLinux and Fedora
sudo curl -fsSLo /etc/yum.repos.d/fastmash.repo https://packages.fastmash.io/fastmash.repo
sudo dnf install fastmash
```

dnf shows the key's fingerprint and asks for confirmation the first time.
