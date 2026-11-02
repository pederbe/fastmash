#!/usr/bin/env bash
# Keep version duplicates and verified download hashes in the same updater commit.
set -euo pipefail
cd "$(dirname "$0")/../.."
old_pins=$(git show HEAD:.github/dependency-pins.json)
pins=.github/dependency-pins.json
replace() {
  OLD=$1 NEW=$2 perl -pi -e 's/\Q$ENV{OLD}\E/$ENV{NEW}/g' "${@:3}"
}
for key in mdbook samply hotpath; do
  old=$(jq -er --arg key "$key" '.[$key]' <<< "$old_pins")
  new=$(jq -er --arg key "$key" '.[$key]' "$pins")
  [[ $new =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
  if [[ $old == "$new" ]]; then continue; fi
  case $key in
    mdbook)
      digest=$(curl --proto '=https' --tlsv1.2 -fsSL "https://api.github.com/repos/rust-lang/mdBook/releases/tags/v$new" | jq -er --arg name "mdbook-v$new-x86_64-unknown-linux-gnu.tar.gz" '.assets[] | select(.name == $name) | .digest | select(test("^sha256:[0-9a-f]{64}$")) | ltrimstr("sha256:")')
      replace "$old" "$new" TESTING.md
      sed -i -E "s/^version=.*/version=$new/; s/^digest=.*/digest=$digest/" scripts/build_site.sh
      ;;
    samply)
      digest=$(curl --proto '=https' --tlsv1.2 -fsSL "https://api.github.com/repos/mstange/samply/releases/tags/samply-v$new" | jq -er '.assets[] | select(.name == "samply-x86_64-unknown-linux-gnu.tar.xz") | .digest | select(test("^sha256:[0-9a-f]{64}$")) | ltrimstr("sha256:")')
      replace "samply-v$old" "samply-v$new" .github/workflows/ci.yml
      DIGEST=$digest perl -pi -e 'if (/samply\.tar\.xz.*sha256sum/) { s/[0-9a-f]{64}/$ENV{DIGEST}/ }' .github/workflows/ci.yml
      ;;
    hotpath)
      replace "$old" "$new" PROFILING.md
      replace "cargo install hotpath --version $old" "cargo install hotpath --version $new" .github/workflows/ci.yml
      ;;
  esac
done
