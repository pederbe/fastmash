# Fedora source package

`fastmash.spec` compiles the published 0.1.0 source for Linux x86-64, using
Fedora's Rust RPM macros and an offline copy of the locked Cargo dependencies.
This is a source package proposal for Fedora, separate from the prebuilt RPM
on the releases page. Fedora's compiler and build flags produce different
program bytes, with no performance claim from the release binaries.

Both programs install in `/usr/bin`. Both project licenses, full dependency
licenses and copyright notices, and the generated linked-license breakdown
are installed as license files. The vendor manifest is also installed there,
where Fedora's RPM generator reads it to identify bundled crates in package
metadata. The package requires coreutils for the system
Sort route. Rust, Python and the packaging macros are build dependencies only.

## Build and check

Run this Bash block from the repository root on Fedora with `rpm-build`,
`cargo-rpm-macros`, Rust 1.88 or newer, GCC, Python 3, coreutils and diffutils
installed. Source preparation needs Cargo, curl, GNU tar and gzip and can use
the normal Cargo download cache. It verifies the public release archive before
vendoring dependencies; all compilation and package tests then run offline.

```bash
bash -eu <<'EOF'
top=$(mktemp -d)
mkdir -p "$top/SOURCES" "$top/SPECS"
bash packaging/fedora/prepare-sources.sh "$top/SOURCES"
(cd "$top/SOURCES" && sha256sum -c sources.sha256)
cp packaging/fedora/fastmash.spec "$top/SPECS/"
rpmbuild -ba --define "_topdir $top" "$top/SPECS/fastmash.spec"
printf 'Source and binary packages: %s\n' "$top"
EOF
```

The package build runs the release's workspace tests, then checks the installed
pair's version, arithmetic, a transcendental result and a paired operation
with an exact `system sort` trace. This route check needs Linux 5.11 or later
and permitted pidfds and `close_range`; successful built-in sorting cannot
conceal a broken supervisor.

In a disposable Fedora environment, install the resulting RPM with
`dnf install ./fastmash-0.1.0-1.fcVERSION.x86_64.rpm`, substituting its actual
filename. Inspect `rpm -ql fastmash`, `rpm -q --licensefiles fastmash` and
`rpm -q --provides fastmash` for the bundled-crate entries,
and repeat the version, arithmetic and system-sort trace checks against
`/usr/bin/fastmash`. Remove it with `dnf remove fastmash`; confirm that both
programs and their documentation and license directories are gone.

## Repository admission

Submission requires the maintainer's approval and Fedora's
[new-package review and packager process](https://docs.fedoraproject.org/en-US/package-maintainers/Joining_the_Package_Maintainers/).
A Fedora packager must review the source package, dependency bundling, combined
license expression, generated bundled-crate metadata and maintenance ownership.
Check the [current Rust guidelines](https://docs.fedoraproject.org/en-US/packaging-guidelines/Rust/)
before submission. A local RPM build does not establish official acceptance.
Advertise repository installation only after an accepted package has been
installed, tested and removed from the published repository.
