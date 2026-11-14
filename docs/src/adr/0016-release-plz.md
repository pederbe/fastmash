# 0016: Version releases with release-plz

Status: accepted. Supersedes the download-version file in
[0011](0011-native-release-delivery.md).
The five-package version and publication arrangement below is superseded by
[0019](0019-one-product-crate.md). Weekly preparation and qualified delivery
remain in force.

Use release-plz for weekly version and changelog preparation. All five published
crates share a workspace version and exact internal dependency requirements.
Even an unchanged library receives that version, preserving same-source checksum
comparisons when recovering a partial upload.

Keep Cargo publication in the Release workflow, after archive and package
checks. Enabling release-plz uploads would publish source packages before those
checks, changing the release boundary. The existing uploader can resume partial
uploads by comparing source checksums.

The deployed website takes its download version from the published `release`
branch's workspace manifest. This avoids a separate version-file updater.
Local previews retain the last recorded published version as a fallback.
[0012](0012-release-delivery-and-performance.md) and
[0013](0013-reuse-tested-release-artifacts.md) retain their performance and
delivery guarantees. The
[release guide](https://github.com/pederbe/fastmash/blob/main/RELEASING.md#weekly-releases)
describes the workflow.
