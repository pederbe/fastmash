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
Source2:        https://raw.githubusercontent.com/pederbe/fastmash/3da66cc2b825e175535ace106edbd8f73bd5b2fa/packaging/linux-source-test.rs#/fastmash-linux-source-test.rs
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
# Keep this standalone test's unsafe-code prohibition despite distro lint caps.
%{__rustc} %{build_rustflags} --cap-lints=forbid --edition=2024 \
    --crate-name fastmash_source_package_test --target x86_64-unknown-linux-gnu \
    -C linker=%{__cc} %{SOURCE2} -o fastmash-test-linux-source
python3 scripts/third_party_licenses.py > THIRD-PARTY-LICENSES.md
cargo2rpm --path crates/cli/Cargo.toml license-summary
cargo2rpm --path crates/cli/Cargo.toml license-breakdown > LICENSE.dependencies
%cargo_vendor_manifest

%install
%{__cargo} install --locked --offline --profile rpm --no-track \
    --path crates/cli --root %{buildroot}%{_prefix}

%check
%cargo_test -- --locked --workspace
bin=%{buildroot}%{_bindir}
./fastmash-test-linux-source fedora "$bin" '%{version}'

%files
%license LICENSE-MIT LICENSE-APACHE THIRD-PARTY-LICENSES.md LICENSE.dependencies cargo-vendor.txt
%doc README.md CHANGELOG.md
%{_bindir}/fastmash
%{_bindir}/fastmash-sort-supervisor

%changelog
* Tue Oct 06 2026 Peder Bergan - 0.1.0-1
- Initial source package
