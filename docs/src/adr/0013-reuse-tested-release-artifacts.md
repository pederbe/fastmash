# 0013: Reuse tested release artifacts

Status: accepted. Supersedes repeated installation checks and the tag-stored
Linux delivery manifest in [0012](0012-release-delivery-and-performance.md).

Package one of the two reproducible executable builds and test installation
once per artifact and supported baseline. Keep its executable manifest inside
the archive. Verify downloaded assets against retained hashes and provenance.
Rebuilding solely to match a pre-tag manifest would introduce another artifact.
Reinstalling identical bytes would add latency without testing different code.

Deploy the website independently after package delivery. A website failure must
be retryable without rebuilding or republishing packages. Cloudflare remains
the website builder and reports the deployment result.

These choices preserve the link between tested and delivered bytes while
avoiding repeated work. Performance acceptance remains governed by 0012. The
[release guide](https://github.com/pederbe/fastmash/blob/main/RELEASING.md)
describes platform checks and recovery.
