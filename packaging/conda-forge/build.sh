#!/bin/bash
set -euxo pipefail
# Licences of every Rust dependency compiled into the binaries.
cargo-bundle-licenses --format yaml --output THIRDPARTY.yml
# Install the native fastmash executable.
cargo install --locked --no-track --root "$PREFIX" --path crates/cli
# Retained only as test data, never installed into the package's runtime prefix.
rustc --edition=2024 --target "${CARGO_BUILD_TARGET:-x86_64-unknown-linux-gnu}" \
    -C "linker=${CC:-cc}" -C opt-level=2 -C strip=symbols \
    "$RECIPE_DIR/run_test.rs" -o "$SRC_DIR/fastmash-package-test"
