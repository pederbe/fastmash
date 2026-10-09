# Signed apt and dnf repository

`packages.fastmash.io` serves the release `.deb` and `.rpm` packages as a
signed apt repository (Debian, Ubuntu) and a signed dnf repository (RHEL,
AlmaLinux, Fedora). Users who add it get new versions through `apt upgrade`
and `dnf upgrade`, and the package manager checks every signature.

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
4. uploads the packages, then each index in the order clients read them, with
   signatures last, and compares the published metadata with what it signed.

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

1. **Signing key.** On a trusted machine:

   ```sh
   gpg --quick-gen-key "Fastmash Release Signing <releases@fastmash.io>" rsa4096 sign never
   gpg --armor --export FINGERPRINT > release/package-signing-key.asc
   gpg --armor --export-secret-keys FINGERPRINT   # for the environment secret
   ```

   RSA keeps RHEL and AlmaLinux 8 able to check RPM signatures. The key does
   not expire, because apt users keep their own copy of it and would not
   receive an extended expiry. Keep an offline backup of the secret key and
   its revocation certificate (`openpgp-revocs.d/` in the GnuPG home), which is
   how the key is retired if it is ever exposed. Commit
   `release/package-signing-key.asc`; the script refuses to sign with any
   other key. Publish the fingerprint on fastmash.io as well as in the
   repository.
2. **Storage.** In Cloudflare, create the R2 bucket `fastmash-packages`,
   connect the custom domain `packages.fastmash.io` to it, and create an R2 API
   token with Object Read & Write access to that bucket only.
3. **Environment.** In the repository settings, create the `packages`
   environment with the maintainer as required reviewer, limited to the `main`
   branch and `v*` tags, with these secrets:

   | Secret | Value |
   | --- | --- |
   | `PACKAGE_SIGNING_KEY` | armored secret key |
   | `PACKAGE_SIGNING_PASSPHRASE` | its passphrase |
   | `R2_ACCOUNT_ID` | Cloudflare account ID |
   | `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY` | the R2 token's S3 credentials |

4. **First publication.** Run the workflow for `v0.1.0`, then for each later
   release in order, so the newest version is published last.

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
