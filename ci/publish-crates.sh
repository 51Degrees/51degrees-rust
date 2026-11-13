#!/usr/bin/env bash
# Publish the named crates to crates.io, in the dependency order of
# ci/crates.txt whatever order they are named in.
#
#   bash ci/publish-crates.sh fiftyone-pipeline-core fiftyone-caching
#
# The publish workflow names the crates its release plan releases (see
# ci/get-release-plan.ps1), and ci/set-crate-versions.ps1 has already written
# each crate's own version into its manifest. Naming no crate publishes
# nothing. A crate whose version is already on crates.io is skipped, so a
# rerun finishes what a failed run left undone.
#
# After publishing a crate the script waits until the crates.io API lists the
# new version, and carries on after 300 seconds even if it does not. The API
# lists a version once the upload is recorded, which can be before the index
# that Cargo resolves from has it, so the wait that lets the next, dependent
# crate resolve the version is cargo publish's own. That waits up to 60 seconds
# for the index and then carries on with a warning. If the index is slower, the
# next crate's verification build fails to find the version and the script
# stops, and a rerun skips what was published and finishes the release.
#
# `fodid` and `fiftyone-fodid-cloud` need no OWID crate from any registry. The
# OWID source is compiled into `fodid` from the owid-rust submodule by
# ci/copy-owid-source.ps1, which the publish workflow runs before this script,
# and the `fodid` manifest lists the copied directory in its `include` so that
# `cargo publish` packages it even though git ignores it. The three -sys crates
# carry their C sources the same way, copied from the cxx submodule by
# ci/copy-cxx-sources.ps1, which the publish workflow also runs first.
set -euo pipefail

# Publishing needs a crates.io token. When it is absent (for example a normal
# push before the secret is configured) skip cleanly rather than fail, so the
# workflow only acts once a token is in place.
if [ -z "${CARGO_REGISTRY_TOKEN:-}" ]; then
  echo "CARGO_REGISTRY_TOKEN is not set; skipping crates.io publish."
  exit 0
fi

# The crates that may be published, read from the single list both this
# script and ci/setup-trusted-publishing.sh use, so a crate can never be
# published without also being authorised. Blank lines and comments are
# skipped.
CRATES_FILE="$(dirname "$0")/crates.txt"
if [ ! -f "$CRATES_FILE" ]; then
  echo "Crate list not found at $CRATES_FILE" >&2
  exit 1
fi
mapfile -t LISTED < <(grep -vE '^[[:space:]]*(#|$)' "$CRATES_FILE")
if [ "${#LISTED[@]}" -eq 0 ]; then
  echo "Crate list $CRATES_FILE names no crates" >&2
  exit 1
fi

for named in "$@"; do
  found=0
  for listed in "${LISTED[@]}"; do
    if [ "$named" = "$listed" ]; then found=1; fi
  done
  if [ "$found" -eq 0 ]; then
    echo "$named is not in $CRATES_FILE, so it is not authorised to publish" >&2
    exit 1
  fi
done

# The named crates, in the order of the list.
CRATES=()
for listed in "${LISTED[@]}"; do
  for named in "$@"; do
    if [ "$named" = "$listed" ]; then CRATES+=("$listed"); fi
  done
done
if [ "${#CRATES[@]}" -eq 0 ]; then
  echo "No crate named, so nothing is published."
  exit 0
fi

UA="51degrees-rust-publish (support@51degrees.com)"

# True when the given crate version is already on crates.io. The API returns 200
# for a published version and 404 otherwise; a User-Agent is required.
is_published() {
  curl -sf -H "User-Agent: $UA" \
    "https://crates.io/api/v1/crates/$1/$2" >/dev/null 2>&1
}

crate_version() {
  cargo metadata --no-deps --format-version 1 \
    | jq -r ".packages[] | select(.name==\"$1\") | .version"
}

# Publish one crate, waiting out crates.io's new-crate rate limit. crates.io
# limits how many brand-new crate names one account may publish in a short
# window and replies 429 with a "try again after <date>" once the burst is
# spent. On a 429 this sleeps until that time (plus a margin) and retries the
# same crate; any other failure is fatal. Once every crate has been published
# once, later version bumps are not new-crate publishes and are not throttled.
publish_crate() {
  local crate="$1" attempt out when target now wait
  for attempt in $(seq 1 40); do
    # --allow-dirty because the OWID source under fodid/src/owid, the C
    # sources under each -sys crate's cxx folder, the translations under
    # ip-intelligence-translation/translations and
    # derived-properties/SOURCE.txt are written by ci/copy-owid-source.ps1,
    # ci/copy-cxx-sources.ps1, ci/copy-translations.ps1 and
    # ci/record-derived-properties-commit.ps1 and deliberately never
    # committed, and cargo otherwise refuses to publish a package that
    # includes files git does not track. The workflow starts from a clean
    # checkout, so those files are the only thing the flag lets through.
    if out="$(cargo publish -p "$crate" --allow-dirty 2>&1)"; then
      echo "$out"
      return 0
    fi
    echo "$out"
    if echo "$out" | grep -qiE "429|too many requests|too many .* crates"; then
      when="$(echo "$out" | grep -oiE "try again after [^.]*" \
        | head -1 | sed -E 's/[Tt]ry again after //')"
      target="$(date -u -d "$when" +%s 2>/dev/null || echo "")"
      now="$(date -u +%s)"
      if [ -n "$target" ] && [ "$target" -gt "$now" ]; then
        wait=$(( target - now + 15 ))
      else
        wait=130
      fi
      echo ">> rate limited; sleeping ${wait}s then retrying $crate (attempt $attempt)"
      sleep "$wait"
      continue
    fi
    echo ">> $crate failed to publish for a non-rate-limit reason; stopping."
    return 1
  done
  echo ">> gave up on $crate after $attempt attempts."
  return 1
}

for crate in "${CRATES[@]}"; do
  version="$(crate_version "$crate")"
  if [ -z "$version" ]; then
    echo "ERROR: could not resolve a version for $crate"
    exit 1
  fi
  if is_published "$crate" "$version"; then
    echo "== $crate $version already on crates.io; skipping."
    continue
  fi
  echo "== publishing $crate $version"
  publish_crate "$crate"
  # Wait until the crates.io API lists the new version. This is not the index
  # the next crate's verification build resolves from, which cargo publish
  # has already waited for, as the header says.
  for attempt in $(seq 1 30); do
    if is_published "$crate" "$version"; then
      echo "   crates.io lists $crate $version."
      break
    fi
    echo "   waiting for crates.io to list $crate $version ($attempt/30)..."
    sleep 10
  done
done

echo "Done."
