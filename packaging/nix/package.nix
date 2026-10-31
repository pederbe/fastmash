{
  lib,
  fetchurl,
  rustPlatform,
  python3,
  coreutils,
}:

let
  packageTest = fetchurl {
    url = "https://raw.githubusercontent.com/pederbe/fastmash/3da66cc2b825e175535ace106edbd8f73bd5b2fa/packaging/linux-source-test.rs";
    sha256 = "71b6715633d14f654a84d7f27c9f27851bdacc0a2ece0e1ac6cb6c831ce469dc";
  };
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "fastmash";
  version = "0.1.0";

  src = fetchurl {
    url = "https://github.com/pederbe/fastmash/archive/refs/tags/v${finalAttrs.version}.tar.gz";
    sha256 = "aa6b74c7e760622e615b44c5dbeb623aad50e4e6dc34a3808a3f3b1777964b60";
  };
  cargoHash = "sha256-hQclGP4/bZRJj4knLR2w1qs1D4k/pMoKakc32L2gmYw=";

  nativeBuildInputs = [ python3 ];
  cargoBuildFlags = [
    "-p"
    "fastmash"
    "--bins"
  ];
  # Command tests assume an FHS host. Keep the numerical tests in the sandbox
  # and exercise the installed command in installCheckPhase.
  cargoTestFlags = [
    "-p"
    "fastmash-conversion"
    "-p"
    "fastmash-numeric-contract"
    "-p"
    "fastmash-portable-numerics"
    "--lib"
  ];

  postPatch = ''
    # Only historical source releases contain an external sort path.
    if [ -f crates/sort-process/src/lib.rs ] && grep -q '"/usr/bin/sort"' crates/sort-process/src/lib.rs; then
      substituteInPlace crates/sort-process/src/lib.rs \
        --replace-fail '"/usr/bin/sort"' '"${lib.getExe' coreutils "sort"}"'
    fi
  '';

  postBuild = ''
    rustc --edition=2024 --crate-name fastmash_source_package_test \
      --target x86_64-unknown-linux-gnu -C "linker=$CC" -C opt-level=2 \
      ${packageTest} -o fastmash-test-linux-source
  '';

  postInstall = ''
    python3 scripts/third_party_licenses.py > THIRD-PARTY-LICENSES.md
    install -Dm644 -t "$out/share/doc/fastmash" \
      README.md LICENSE-MIT LICENSE-APACHE THIRD-PARTY-LICENSES.md
  '';

  doInstallCheck = true;
  installCheckPhase = ''
    runHook preInstallCheck
    ./fastmash-test-linux-source nix "$out/bin" '${finalAttrs.version}' \
      "$out/share/doc/fastmash"
    runHook postInstallCheck
  '';

  meta = {
    description = "Fast command-line statistics and table transformations";
    homepage = "https://fastmash.io";
    changelog = "https://github.com/pederbe/fastmash/blob/v${finalAttrs.version}/CHANGELOG.md";
    license = with lib.licenses; [
      mit
      asl20
    ];
    platforms = [ "x86_64-linux" ];
    mainProgram = "fastmash";
  };
})
