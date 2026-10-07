# Nix package

`package.nix` builds the published 0.1.0 source for Linux x86-64. The source
archive and vendored Cargo dependencies have verified hashes. Both executables
are installed in `bin/`, with project licenses and generated dependency
licenses, copyright and notices in `share/doc/fastmash/`.
Python is a build dependency only. The source-built programs carry no
performance claim from the release binaries.

NixOS has no `/usr/bin/sort`. The recipe replaces the release's one system-sort
constant with the immutable path of Nixpkgs coreutils, making it a runtime
dependency of both programs. It uses neither a wrapper nor a caller-supplied
`PATH`; the Sort supervisor still sits beside `fastmash`.

## Build and check

With a current Nixpkgs channel and its Rust toolchain, run from the repository
root:

```sh
nix-build --expr '(import <nixpkgs> {}).callPackage ./packaging/nix/package.nix {}'
nix-env -i ./result
fastmash --version
printf '1\n2\n3\n' | fastmash sum 1 mean 1
nix-env -e fastmash
```

The build runs the conversion and numerical library tests. The release's
other command tests assume host paths such as `/bin/sh` and `/usr/bin/sort`,
so the recipe instead checks the installed command's version, arithmetic,
transcendental result, both executables and license files. The installed
system Sort route must produce the paired-operation result and exact route
trace with an empty environment apart from the test controls. The route test
requires Linux 5.11 or later with pidfds and `close_range` permitted.

After removal, confirm that both executables and `share/doc/fastmash/` are
absent from the active profile. Nix retains older profile generations and store
objects for rollback; removing the active package does not delete those.

Follow Nixpkgs'
[Rust packaging guidance](https://nixos.org/manual/nixpkgs/unstable/#compiling-rust-applications-with-cargo)
when updating the version, source hash and `cargoHash`. Keep the path replacement
guarded with `--replace-fail`, so a changed source layout stops the build.

## Submission

Submit to Nixpkgs only with the maintainer's approval. Copy `package.nix` to
the by-name package directory, add its maintainer metadata, and run Nixpkgs'
formatting and package checks against the submission revision. After acceptance,
install from the published Nixpkgs revision in a fresh profile and verify the
same commands, route trace, licenses and removal before advertising `nix`
installation in the guide.
