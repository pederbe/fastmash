{
  lib,
  fetchurl,
  rustPlatform,
  python3,
  coreutils,
}:

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
  # and exercise the installed command and supervisor in installCheckPhase.
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
    substituteInPlace crates/sort-process/src/lib.rs \
      --replace-fail '"/usr/bin/sort"' '"${lib.getExe' coreutils "sort"}"'
  '';

  postInstall = ''
    python3 scripts/third_party_licenses.py > THIRD-PARTY-LICENSES.md
    install -Dm644 -t "$out/share/doc/fastmash" \
      README.md LICENSE-MIT LICENSE-APACHE THIRD-PARTY-LICENSES.md
  '';

  doInstallCheck = true;
  installCheckPhase = ''
    runHook preInstallCheck
    export LC_ALL=C
    test -x "$out/bin/fastmash"
    test -x "$out/bin/fastmash-sort-supervisor"
    for document in LICENSE-MIT LICENSE-APACHE THIRD-PARTY-LICENSES.md; do
      test -s "$out/share/doc/fastmash/$document"
    done
    printf 'fastmash ${finalAttrs.version}\n' > expected
    "$out/bin/fastmash" --version > actual
    cmp expected actual
    printf '1\n2\n3\n' | "$out/bin/fastmash" sum 1 mean 1 > actual
    printf '6\t2\n' > expected
    cmp expected actual
    printf '1\n2\n3\n' | "$out/bin/fastmash" geomean 1 > actual
    printf '1.8171205928321\n' > expected
    cmp expected actual
    printf 'b\t2\t3\na\t1\t2\na\t3\t6\n' | \
      env -i LC_ALL=C FASTMASH_GROUPING=sort FASTMASH_SORT_TRACE=1 \
      "$out/bin/fastmash" -sg1 pcov 2:3 > actual 2> diagnostic
    printf 'a\t2\nb\t0\n' > expected
    cmp expected actual
    printf 'sort route: system sort\n' > expected
    cmp expected diagnostic
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
