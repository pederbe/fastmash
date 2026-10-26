# 0013: Reuse tested release artifacts

Status: accepted. Supersedes the repeated installation checks and tag-stored
Linux delivery manifest in ADR 0012. Performance acceptance is unchanged.

Build each platform executable twice for reproducibility, then package one
of those builds. Generate the Linux executable manifest in the release job
and include it inside the attested archive. Do not rebuild merely to reproduce
a manifest prepared before tagging.

Run complete installation checks once per artifact and supported baseline.
Linux checks run before upload. Compare the downloaded assets with the build's
retained checksums and verify provenance before publication. macOS release
installation checks run against the downloaded draft on macOS 15 and 26.
Branch CI checks installation through workflow artifacts instead.

Website deployment follows package delivery in a separate workflow with an
independent result and manual retry. A website build failure must not require
rebuilding or republishing packages. Cloudflare remains the website builder
and reports its deployment result.

Repeated installation of identical bytes adds release latency without testing
a different artifact. Hash and provenance checks preserve the connection between
tested and uploaded files while keeping the delivery process smaller.
