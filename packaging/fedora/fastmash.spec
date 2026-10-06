Name:           fastmash
Version:        0.1.0
Release:        1%{?dist}
Summary:        Fast command-line statistics and table transformations
# Select MIT for dual-licensed crates; retain the LLVM exception and Unicode terms.
# The complete statically linked license breakdown is generated during build.
License:        MIT AND (Apache-2.0 WITH LLVM-exception) AND Unicode-3.0
URL:            https://fastmash.io
Source0:        https://github.com/pederbe/fastmash/archive/refs/tags/v%{version}.tar.gz#/fastmash-%{version}.tar.gz
# Generated from Source0 with prepare-sources.sh and cargo vendor --locked.
Source1:        fastmash-%{version}-vendor.tar.gz
ExclusiveArch:  x86_64

BuildRequires:  cargo-rpm-macros
BuildRequires:  cargo >= 1.88
BuildRequires:  rust >= 1.88
BuildRequires:  gcc
BuildRequires:  python3
BuildRequires:  coreutils
BuildRequires:  diffutils
Requires:       coreutils

%description
Fastmash computes statistics and table transformations over delimited text
using the GNU datamash command language. Software 80-bit arithmetic gives
portable numerical results. Both the command and its Sort supervisor are
installed together.

%prep
%autosetup -n fastmash-%{version}
tar -xzf %{SOURCE1}
%cargo_prep -v vendor

%build
%cargo_build -- --locked -p fastmash --bins
python3 scripts/third_party_licenses.py > THIRD-PARTY-LICENSES.md
cargo2rpm --path crates/cli/Cargo.toml license-summary
cargo2rpm --path crates/cli/Cargo.toml license-breakdown > LICENSE.dependencies
%cargo_vendor_manifest

%install
%{__cargo} install --locked --offline --profile rpm --no-track \
    --path crates/cli --root %{buildroot}%{_prefix}

%check
%cargo_test -- --locked --workspace
export LC_ALL=C
bin=%{buildroot}%{_bindir}
test -x "$bin/fastmash-sort-supervisor"
printf 'fastmash %{version}\n' > expected
"$bin/fastmash" --version > actual
cmp expected actual
printf '1\n2\n3\n' | "$bin/fastmash" sum 1 mean 1 > actual
printf '6\t2\n' > expected
cmp expected actual
printf '1\n2\n3\n' | "$bin/fastmash" geomean 1 > actual
printf '1.8171205928321\n' > expected
cmp expected actual
printf 'b\t2\t3\na\t1\t2\na\t3\t6\n' | \
    env -i PATH=/usr/bin:/bin LC_ALL=C FASTMASH_GROUPING=sort FASTMASH_SORT_TRACE=1 \
    "$bin/fastmash" -sg1 pcov 2:3 > actual 2> diagnostic
printf 'a\t2\nb\t0\n' > expected
cmp expected actual
printf 'sort route: system sort\n' > expected
cmp expected diagnostic

%files
%license LICENSE-MIT LICENSE-APACHE THIRD-PARTY-LICENSES.md LICENSE.dependencies cargo-vendor.txt
%doc README.md CHANGELOG.md
%{_bindir}/fastmash
%{_bindir}/fastmash-sort-supervisor

%changelog
* Tue Oct 06 2026 Peder Bergan - 0.1.0-1
- Initial source package
