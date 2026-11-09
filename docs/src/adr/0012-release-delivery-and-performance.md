# 0012: Separate release delivery from performance acceptance

Status: accepted performance and publication decision. Supersedes the
performance-manifest prerequisite and manual publication step in
[0011](0011-native-release-delivery.md). Delivery details were updated by
[0013](0013-reuse-tested-release-artifacts.md) and [0014](0014-native-sorting-everywhere.md).

Assess performance when runtime code, dependencies or build recipes change.
Compare affected workloads with the previous accepted build. New public speed
claims require dated measurements identifying the executable and conditions.
Routine versioning does not start another whole-release benchmark campaign.

A package-version change can alter generated code and layout, so a later
release does not inherit a claim that its exact bytes were measured. Published
results remain attached to their recorded versions.

Requiring a new full campaign for every version would couple scheduled releases
to measurement-host access without identifying which changes need investigation.
Release delivery instead establishes reproducibility, correctness, installation
and provenance, then publishes automatically after its required checks pass.
See the [benchmark method](../benchmarks/method.md) and
[release guide](https://github.com/pederbe/fastmash/blob/main/RELEASING.md).
