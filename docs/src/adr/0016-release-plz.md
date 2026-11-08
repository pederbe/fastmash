# 0016: Version releases with release-plz

Release-plz prepares one weekly pull request with Cargo versions and dated
notes from conventional pull request titles. It merges after required CI,
then creates the binary tag and draft. All five published crates participate
in versioning and the binary's combined changelog. A shared Cargo workspace
version keeps all five crates and their exact dependency requirements aligned.
An unchanged library still receives the release version, preserving same-source
checksum comparisons during partial upload recovery.

Keep release-plz Cargo publication disabled. The existing Release workflow
qualifies the archives and packages before publishing the GitHub release and
uploading crates. Its checksum-aware uploader resumes partial Cargo uploads.
Moving uploads into release-plz would publish source packages before those
delivery checks, changing the established release boundary.

This supersedes ADR 0011's download-version file as the deployed website's
authority. The published `release` branch's workspace Cargo manifest supplies its
version, so no separate version-file updater is needed. Local previews retain
the last recorded published version as their fallback. ADRs 0012 and 0013's
qualification and performance boundaries remain unchanged.
