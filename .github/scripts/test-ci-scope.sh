#!/usr/bin/env bash
# Exercise dependency ownership, shared options and source/workflow changes.
set -euo pipefail
scope=$(realpath "$(dirname "$0")/ci-scope.sh")
fixture=$(mktemp -d)
trap 'rm -r "$fixture"' EXIT
cd "$fixture"
git init -q
git config user.email fixture@example.invalid
git config user.name fixture
mkdir -p crates/app/src crates/helper/src .github/workflows
printf '[workspace]\nmembers = ["crates/app", "crates/helper"]\n' > Cargo.toml
printf '[package]\nname = "fastmash"\nversion = "1.0.0"\n[dependencies]\ncommon = "1"\n' > crates/app/Cargo.toml
printf '[package]\nname = "helper"\nversion = "1.0.0"\n[dependencies]\ncommon = "1"\ndevlib = "1"\n' > crates/helper/Cargo.toml
touch crates/app/src/lib.rs crates/helper/src/lib.rs .github/workflows/ci.yml
cat > Cargo.lock <<'LOCK'
version = 4
[[package]]
name = "fastmash"
version = "1.0.0"
dependencies = ["common"]
[[package]]
name = "helper"
version = "1.0.0"
dependencies = ["common", "devlib"]
[[package]]
name = "common"
version = "1.0.0"
[[package]]
name = "devlib"
version = "1.0.0"
LOCK
git add .
git commit -qm fixture
base=$(git rev-parse HEAD)
expect() {
  local output
  output=$(bash "$scope" "$base")
  for line in "$@"; do
    if ! grep -Fxq "$line" <<< "$output"; then
      printf 'Expected %s, got:\n%s\n' "$line" "$output" >&2
      exit 1
    fi
  done
  git restore --source="$base" --staged --worktree .
}

# Any development-only dependency, not a list of Unicode package names.
sed -i '/name = "devlib"/{n;s/1.0.0/2.0.0/;}' Cargo.lock
expect application=false packages=helper
printf '// helper change\n' > crates/helper/src/lib.rs
expect application=false packages=helper
sed -i '/name = "common"/{n;s/1.0.0/2.0.0/;}' Cargo.lock
expect application=true
sed -i 's/common = "1"/common = { version = "1", features = ["extra"] }/' crates/helper/Cargo.toml
expect application=true
printf '// application change\n' > crates/app/src/lib.rs
expect application=true
printf '# workflow change\n' > .github/workflows/ci.yml
expect application=false configuration=true packages=
printf '# documentation\n' > README.md
expect application=false packages=
rm README.md
bash "$scope" | grep -Fxq application=true

# CLI test dependencies do not enter the shipping dependency closure.
printf '\n[dev-dependencies]\ndevlib = "1"\n' >> crates/app/Cargo.toml
sed -i '0,/dependencies = \["common"\]/s//dependencies = ["common", "devlib"]/' Cargo.lock
git add .
git commit -qm 'CLI test dependency'
base=$(git rev-parse HEAD)
sed -i '/name = "devlib"/{n;s/1.0.0/2.0.0/;}' Cargo.lock
sed -i 's/devlib = "1"/devlib = "2"/' crates/app/Cargo.toml
expect application=false 'packages=fastmash helper'

# Same package name, different resolved versions: only the helper uses v1.
sed -i 's/\["common", "devlib"\]/["common 2.0.0", "devlib"]/' Cargo.lock
# Keep the helper on v1 while the CLI uses v2.
sed -i '/name = "helper"/{n;n;s/common 2.0.0/common 1.0.0/;}' Cargo.lock
printf '\n[[package]]\nname = "common"\nversion = "2.0.0"\n' >> Cargo.lock
sed -i 's/common = "1"/legacy = { package = "common", version = "1" }/' crates/helper/Cargo.toml
git add .
git commit -qm 'Different resolved versions'
base=$(git rev-parse HEAD)
sed -i '/name = "common"/{n;s/1.0.0/1.1.0/;}' Cargo.lock
sed -i 's/common 1.0.0/common 1.1.0/' Cargo.lock
sed -i 's/version = "1"/version = "1.1"/' crates/helper/Cargo.toml
expect application=false packages=helper
printf '\n[features]\nprivate = ["devlib/extra"]\n' >> crates/helper/Cargo.toml
expect application=false packages=helper
printf 'CI scope cases passed\n'
