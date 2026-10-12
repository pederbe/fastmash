# Signed apt and dnf repository

This directory prepares the release `.deb` and `.rpm` packages for a signed
apt repository (Debian, Ubuntu) and a signed dnf repository (RHEL, AlmaLinux,
Fedora) at `packages.fastmash.io`. The repository is not yet published.
Complete setup and verify the first publication before adding it to the
installation guide. Once available, users who add it receive new versions
through `apt upgrade` and `dnf upgrade`, with signature verification enabled.

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
   against the tag's qualified checksums and removes it. When the version is
   the newest, it installs the previous version first and upgrades;
4. uploads the packages followed by the metadata, and compares the published
   metadata with what it signed.

It starts on its own when a release is published, if the tagged commit
contains the workflow. Otherwise, run it from `main` with the tag:
`gh workflow run "Package repository" --ref main -f tag=vX.Y.Z`. Both need the
maintainer's approval of the `packages` environment. Runs share one
concurrency group, and GitHub keeps only the newest pending run, so start
each dispatch after the previous run has finished.

## Layout

```text
fastmash.asc, fastmash.gpg   release signing key, armored and binary
fastmash.repo                dnf repository definition
apt/dists/stable/            InRelease, Release, Release.gpg, main/binary-amd64/Packages
apt/pool/main/f/fastmash/    every published .deb
rpm/                         every published .rpm, signed
rpm/repodata/                metadata, with repomd.xml.asc
```

Package files are cached as immutable. Metadata, keys and `fastmash.repo` are
served with `Cache-Control: no-cache`.

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
   environment with the maintainer as required reviewer, limited to the `main`
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

With Docker, a throwaway key in `GNUPGHOME` and its public key in a file:

```sh
GPG_PASSPHRASE=... bash release/package-repository.sh /tmp/repository test-key.asc \
  fastmash_0.1.0_amd64.deb fastmash-0.1.0-1.x86_64.rpm
git show v0.1.0:release/qualified.sha256 > /tmp/qualified.sha256
bash release/test-package-repository.sh /tmp/repository 0.1.0 /tmp/qualified.sha256
```

## For users

Add to the installation guide once the repository is live and checked. After
verifying the key fingerprint:

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
