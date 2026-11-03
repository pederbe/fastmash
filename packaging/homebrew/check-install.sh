#!/usr/bin/env bash
# Check the package-manager integration, reusing the published executable.
set -euo pipefail
formula=$1
previous=${2:-}
name=${formula##*/}
tap=$(brew --repository "${formula%/*}")
recipe="$tap/Formula/$name.rb"
work=$(mktemp -d "${RUNNER_TEMP:-/var/tmp}/fastmash-homebrew.XXXXXX")
cp "$recipe" "$work/candidate.rb"
trap 'cp "$work/candidate.rb" "$recipe"; rm -r "$work"' EXIT
if [[ -n "$previous" ]] && ! cmp -s "$previous" "$recipe"; then
  cp "$previous" "$recipe"
  # Historical packages already have downloadable Homebrew builds. Never
  # compile an old version merely to check the upgrade to a release archive.
  if grep -q '^  bottle do$' "$recipe"; then
    brew install --force-bottle "$formula"
  else
    brew install "$formula"
  fi
  cp "$work/candidate.rb" "$recipe"
  brew upgrade "$formula"
else
  brew install "$formula"
fi
brew test "$formula"
version=$(brew info --json=v2 "$formula" | jq -er '.formulae[0].versions.stable')
prefix=$(brew --prefix "$formula")
test "$("$prefix/bin/$name" --version)" = "$name $version"
archive=$(brew --cache --build-from-source "$formula")
mkdir "$work/archive"
tar -xf "$archive" -C "$work/archive"
expected=$(find "$work/archive" -type f -name "$name")
cmp "$expected" "$prefix/bin/$name"
for document in LICENSE-MIT LICENSE-APACHE THIRD-PARTY-LICENSES.md; do
  test -s "$prefix/share/doc/$name/$document"
done
