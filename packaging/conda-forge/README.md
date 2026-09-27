# conda-forge recipe

A draft recipe for [conda-forge](https://conda-forge.org/). Most
bioinformaticians install tools with conda, and GNU datamash itself is
packaged on conda-forge
([datamash-feedstock](https://github.com/conda-forge/datamash-feedstock)).

**Why conda-forge, not bioconda:** Fastmash is a general-purpose tool, and
bioconda's guidelines ask for domain-specific packages and send general ones
to conda-forge. Bioconda recipes can depend on conda-forge packages, so
bioinformatics pipelines can use it either way.

**Built from source:** conda-forge requires compiled code to be built in its
CI, so this package is compiled by conda-forge rather than repackaging the
release binaries. Its bytes therefore differ from the benchmarked release
binaries; the results are the same (the arithmetic is portable), but speed
can differ slightly.

## Submitting (after the first release)

1. Put the release tag's tarball checksum in `meta.yaml`:
   `curl -fsSL https://github.com/pederbe/fastmash/archive/refs/tags/v0.1.0.tar.gz | sha256sum`.
2. Fork [conda-forge/staged-recipes](https://github.com/conda-forge/staged-recipes),
   copy `meta.yaml`, `build.sh` and `run_test.sh` into `recipes/fastmash/`,
   and open a pull request. Their CI builds and runs `run_test.sh`.
3. After review and merge, conda-forge creates the `fastmash-feedstock`
   repository with the listed maintainers. Its bot opens a pull request for
   each new release; review and merge it as part of each release.

Then update the install page and the landing page's install tabs with
`conda install -c conda-forge fastmash`.
