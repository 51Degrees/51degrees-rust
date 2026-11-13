#!/usr/bin/env bash
# Measure what CountriesTranslationEngine adds to a WebAssembly module.
#
# Builds the two programs in ci/wasm-size for wasm32-wasip1 in release, one
# with CountryCodeTranslationEngine alone and one with CountriesTranslationEngine
# as well, and prints the size of each module and the difference. The
# difference is the countries engine's code, its 16 languages' country names
# and the Unicode collation data icu_collator compiles in. The figure in
# ip-intelligence-translation/README.md comes from this script's output in the
# Pull Request workflow's wasm job.
#
# When wasmtime is on the path, both modules are also run for Swedish, and the
# script fails unless the larger one gives the 250 countries in Swedish order,
# ending with Åland, Österrike and Östtimor. Swedish sorts Å and Ö after Z,
# which the root collation does not, so the order shows that the module holds
# the collation data as well as choosing the locale.
#
# Needs the wasm32-wasip1 target (rustup target add wasm32-wasip1).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
crate="$root/ci/wasm-size"

# Resolve the same dependency versions as the library workspace. Cargo keeps
# the entries this crate uses and drops the rest.
cp "$root/Cargo.lock" "$crate/Cargo.lock"
cargo build --manifest-path "$crate/Cargo.toml" --release \
  --target wasm32-wasip1 --bins

out="$crate/target/wasm32-wasip1/release"
without=$(wc -c < "$out/without-countries.wasm")
with=$(wc -c < "$out/with-countries.wasm")
added=$((with - without))

summary=$(cat <<EOF
| Module | Bytes |
| --- | ---: |
| CountryCodeTranslationEngine alone | $without |
| With CountriesTranslationEngine | $with |
| Added by CountriesTranslationEngine | $added |
EOF
)
echo "$summary"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  {
    echo "### What CountriesTranslationEngine adds to a wasm32-wasip1 module"
    echo
    echo "$summary"
  } >> "$GITHUB_STEP_SUMMARY"
  echo "The table is in the run's summary."
fi

if command -v wasmtime > /dev/null; then
  # Each run is its own assignment so that set -e stops on a failed run.
  alone=$(wasmtime run "$out/without-countries.wasm" sv_SE)
  echo "without-countries sv_SE: $alone"
  sorted=$(wasmtime run "$out/with-countries.wasm" sv_SE)
  echo "with-countries sv_SE: $sorted"
  want=$(printf '%s\n' sv-SE 250 Åland Österrike Östtimor)
  if [ "$(echo "$sorted" | tail -n 5)" != "$want" ]; then
    echo "with-countries did not give the 250 countries in Swedish order" >&2
    exit 1
  fi
fi
