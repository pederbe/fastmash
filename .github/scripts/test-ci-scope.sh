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
mkdir -p crates/app/src crates/helper/src .github/workflows release/publication-test-server/tests scripts/demo site packaging/homebrew
printf '[workspace]\nmembers = ["crates/app", "crates/helper"]\n' > Cargo.toml
printf '[package]\nname = "fastmash"\nversion = "1.0.0"\n[dependencies]\ncommon = "1"\n' > crates/app/Cargo.toml
printf '[package]\nname = "helper"\nversion = "1.0.0"\n[dependencies]\ncommon = "1"\ndevlib = "1"\n' > crates/helper/Cargo.toml
touch crates/app/src/lib.rs crates/helper/src/lib.rs .github/workflows/ci.yml release/publication-test-server/tests/release_delivery.rs scripts/build_site.py
touch packaging/homebrew/fastmash.rb release/publication-test-server/Cargo.toml
touch .github/workflows/homebrew.yml .github/workflows/macos.yml
touch release-plz.toml renovate.json release/changelog.sh .github/workflows/release-plz.yml
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
website_paths=(
  .github/ISSUE_TEMPLATE/bug_report.yml .github/ISSUE_TEMPLATE/config.yml
  ARCHITECTURE.md GLOSSARY.md TESTING.md
  docs/book.toml docs/descriptions.json docs/theme/head.hbs docs/theme/fastmash.css
  docs/theme/navigation.js docs/theme/footer.js docs/theme/favicon.svg
  site/index.html site/landing.js site/landing.css site/landing-noscript.css
  site/demo.gif site/demo.mp4 site/demo.webm site/demo-poster.png site/og.png
  site/_headers site/release-version.txt site/diagrams/dark.json site/diagrams/rendered/example.svg
  scripts/demo/demo.html scripts/demo/record.mjs scripts/demo/og.html
  scripts/build_site.sh scripts/build_site.py scripts/test_build_site.py
  scripts/check_public_references.py scripts/test_check_public_references.py
  scripts/benchmark_chart.py scripts/test_landing_page.mjs scripts/test_docs.mjs
  package.json package-lock.json wrangler.jsonc
)
for path in "${website_paths[@]}"; do
  mkdir -p "$(dirname "$path")"
  touch "$path"
done
touch scripts/application-tool.sh install.sh
printf '# repository fixture\n' > release/package-repository.sh
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
printf '# installer workflow change\n' > .github/workflows/homebrew.yml
expect application=false configuration=false packages=
printf '# installer workflow change\n' > .github/workflows/homebrew.yml
printf '// application change\n' > crates/app/src/lib.rs
expect application=true configuration=false packages=fastmash
printf '# installer workflow change\n' > .github/workflows/homebrew.yml
printf '# archive builder change\n' > .github/workflows/macos.yml
expect application=false configuration=true packages=
printf '// release tooling change\n' > release/publication-test-server/tests/release_delivery.rs
expect application=false configuration=true packages=
printf '# release configuration change\n' > release-plz.toml
expect application=false configuration=true packages=
printf '# changelog formatter change\n' > release/changelog.sh
expect application=false configuration=true packages=
printf '{}\n' > renovate.json
expect application=false configuration=true packages=
printf '# release workflow change\n' > .github/workflows/release-plz.yml
expect application=false configuration=true packages=
printf '# package signing change\n' > release/package-repository.sh
expect application=false configuration=true packages=
printf '# release-file installation recipe\n' > packaging/homebrew/fastmash.rb
expect application=false configuration=false packages=
printf '# independent tool manifest change\n' > release/publication-test-server/Cargo.toml
expect application=false configuration=true packages=
printf '# website change\n' > scripts/build_site.py
expect application=false configuration=false packages=
printf '# release configuration change\n' > release-plz.toml
printf '// application change\n' > crates/app/src/lib.rs
expect application=true configuration=true packages=fastmash
# Website inputs share one route, without suppressing mixed application or workflow edits.
for path in "${website_paths[@]}"; do
  printf 'website change\n' > "$path"
  expect application=false configuration=false packages=
done
printf 'website change\n' > scripts/test_docs.mjs
printf '// application change\n' > crates/app/src/lib.rs
expect application=true configuration=false packages=fastmash
printf 'website change\n' > site/diagrams/dark.json
printf '# workflow change\n' > .github/workflows/ci.yml
expect application=false configuration=true packages=
# Shared installation behavior and unrelated scripts still select application checks.
printf 'website change\n' > site/index.html
printf '# installer change\n' > install.sh
expect application=true configuration=false packages=
printf '# application tool change\n' > scripts/application-tool.sh
expect application=true configuration=false packages=
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
