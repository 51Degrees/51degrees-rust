#!/usr/bin/env bash
# Checks what three crates would publish, because each compiles in files that
# come from a submodule.
#
# - fiftyone-derived-properties packages its scripts and cases from the
#   derived-properties submodule at derived-properties/vendor, and the commit
#   ci/record-derived-properties-commit.ps1 writes to SOURCE.txt.
# - fiftyone-javascript-builder packages its template from the
#   javascript-templates submodule at javascript-builder/javascript-templates.
# - fiftyone-ip-intelligence-translation packages the translations
#   ci/copy-translations.ps1 copies from the data repository inside cxx.
#
# Run both of those scripts first. For each crate this lists what cargo would
# publish, checks the shared files are all there and that nothing else from
# the submodules is, then unpacks the crate and builds it on its own, with no
# git and no submodule, as crates.io and docs.rs build it. The derived
# property crate's tests are run from its package too, which reads the cases
# and the recorded commit from the package. The workspace crates each one
# depends on are given as patches, so the package builds against this
# checkout rather than against what crates.io holds.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

failed=0
fail() {
  echo "::error::$1"
  failed=1
}

# The files cargo would publish for a crate, one per line, in order.
listing() {
  cargo package --list --allow-dirty -p "$1" | sort
}

# Every file under a folder of the checkout, relative to that folder, that
# git tracks there.
tracked() {
  git -C "$1" ls-files | sort
}

echo "::group::fiftyone-derived-properties"
list="$(listing fiftyone-derived-properties)"
echo "$list"
want="$( (tracked derived-properties/vendor \
  | grep -E '^(scripts/[^/]+\.yaml|tests/[^/]+\.yaml|tests/invalid/[^/]+\.yaml)$' \
  | sed 's|^|vendor/|') | sort)"
got="$(echo "$list" | grep '^vendor/' || true)"
if [ "$got" != "$want" ]; then
  fail "fiftyone-derived-properties does not package exactly the submodule's scripts and cases"
  diff <(echo "$want") <(echo "$got") || true
fi
for file in vendor/scripts/HumanConfidence.yaml vendor/scripts/IsGdpr.yaml \
  vendor/tests/HumanConfidence.cases.yaml vendor/tests/IsGdpr.cases.yaml \
  SOURCE.txt build.rs; do
  grep -qx "$file" <<< "$list" || fail "fiftyone-derived-properties does not package $file"
done
recorded="$(sed -n 's/^commit=//p' derived-properties/SOURCE.txt)"
submodule="$(git -C derived-properties/vendor rev-parse HEAD)"
if [ "$recorded" != "$submodule" ]; then
  fail "derived-properties/SOURCE.txt records $recorded but the submodule is at $submodule"
fi
echo "::endgroup::"

echo "::group::fiftyone-javascript-builder"
list="$(listing fiftyone-javascript-builder)"
echo "$list"
got="$(echo "$list" | grep '^javascript-templates/' || true)"
if [ "$got" != "javascript-templates/JavaScriptResource.mustache" ]; then
  fail "fiftyone-javascript-builder does not package exactly the template from its submodule"
  echo "$got"
fi
echo "::endgroup::"

echo "::group::fiftyone-ip-intelligence-translation"
list="$(listing fiftyone-ip-intelligence-translation)"
echo "$list"
want="$( (cd cxx/data/ip-intelligence/Translations && find . -name '*.yml' -type f \
  | sed 's|^\./|translations/|'; echo translations/SOURCE.txt) | sort)"
got="$(echo "$list" | grep '^translations/' || true)"
if [ "$got" != "$want" ]; then
  fail "fiftyone-ip-intelligence-translation does not package exactly the data repository's translations"
  diff <(echo "$want") <(echo "$got") || true
fi
echo "::endgroup::"

# The 51Degrees licence is the root LICENSE alone, and the manifests declare
# EUPL-1.2, so no package carries a licence file of its own.
for crate in fiftyone-derived-properties fiftyone-javascript-builder \
  fiftyone-ip-intelligence-translation; do
  if grep -qiE '(^|/)(LICEN[CS]E|COPYING)[^/]*$' <<< "$(listing "$crate")"; then
    fail "$crate packages a licence file"
  fi
done

if [ "$failed" -ne 0 ]; then
  exit 1
fi

# Package the three, with fiftyone-pipeline-translation, which the
# translation crate depends on and crates.io does not yet hold, so cargo
# resolves it from this checkout.
echo "::group::cargo package"
cargo package --no-verify --allow-dirty \
  -p fiftyone-derived-properties \
  -p fiftyone-javascript-builder \
  -p fiftyone-pipeline-translation \
  -p fiftyone-ip-intelligence-translation
echo "::endgroup::"

# Every workspace crate as a patch, so an unpacked crate builds against this
# checkout. Cargo warns about each patch a crate does not use.
patches=()
while IFS= read -r line; do
  patches+=(--config "patch.crates-io.${line%%=*}.path=\"${line#*=}\"")
done < <(cargo metadata --no-deps --format-version 1 \
  | jq -r '.packages[] | "\(.name)=\(.manifest_path | rtrimstr("/Cargo.toml"))"')

unpacked="${RUNNER_TEMP:-$(mktemp -d)}/packages"
rm -rf "$unpacked"
mkdir -p "$unpacked"
version() {
  cargo metadata --no-deps --format-version 1 \
    | jq -r ".packages[] | select(.name==\"$1\") | .version"
}
unpack() {
  local crate="$1" file
  file="target/package/$crate-$(version "$crate").crate"
  tar -xzf "$file" -C "$unpacked"
  echo "$unpacked/$crate-$(version "$crate")"
}

# Nothing above the unpacked folders is a git checkout, and nothing beside
# them is a cxx folder, so each crate can only find what its package holds.
if git -C "$unpacked" rev-parse --git-dir > /dev/null 2>&1; then
  echo "::error::$unpacked is inside a git checkout"
  exit 1
fi

echo "::group::fiftyone-derived-properties from its package"
dir="$(unpack fiftyone-derived-properties)"
(cd "$dir" && cargo test "${patches[@]}")
echo "::endgroup::"

echo "::group::fiftyone-javascript-builder from its package"
dir="$(unpack fiftyone-javascript-builder)"
(cd "$dir" && cargo build --no-default-features "${patches[@]}")
echo "::endgroup::"

echo "::group::fiftyone-ip-intelligence-translation from its package"
dir="$(unpack fiftyone-ip-intelligence-translation)"
(cd "$dir" && cargo build "${patches[@]}")
echo "::endgroup::"

echo "Each package carries the shared files and builds on its own."
