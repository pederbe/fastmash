# 0012: Separate release delivery from performance acceptance

The Linux executable-pair references below are superseded by
[ADR 0014: Native sorting in one executable](0014-native-sorting-everywhere.md).
The release and performance evidence boundaries remain unchanged.

Status: accepted. Supersedes the Linux performance-manifest prerequisite and
manual publication step in [ADR 0011](0011-native-release-delivery.md). Its
native archive reproducibility and installed-delivery guarantees remain.

Performance-affecting runtime, dependency and build-recipe changes retain their
predecessor guards during normal change review. New speed claims require dated
measurements identifying the actual executable and conditions. Weekly versioning
does not start a whole-release benchmark campaign.

Cargo package-version changes can alter generated code and layout. A new release
therefore does not inherit a claim that its bytes were measured in an earlier
campaign. Published results remain scoped to their recorded versions.

For delivery, build each platform executable twice and require identical bytes.
Freeze the actual Linux pair in `release/delivery.sha256` and the native macOS
archive by its workflow SHA-256. Test those bytes before and after upload,
verify provenance, and publish automatically only after all required checks pass.
Historical qualified manifests and benchmark records remain unchanged.

Requiring fresh controlled measurements of every exact version would require
scheduled access to measurement hosts and their retained inputs. Keeping
performance acceptance with the changes that can affect it supports weekly
hosted releases without turning delivery checks into performance claims.
