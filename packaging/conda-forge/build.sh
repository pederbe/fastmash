#!/bin/bash
set -euxo pipefail
# Licences of every Rust dependency compiled into the binaries.
cargo-bundle-licenses --format yaml --output THIRDPARTY.yml
# Installs both programs; fastmash-sort-supervisor must sit beside fastmash.
cargo install --locked --no-track --root "$PREFIX" --path crates/cli
