#!/usr/bin/env bash
# Register this repository's two publishing workflows as Trusted Publishers on
# crates.io for every published crate, so a publish can authenticate with a
# short-lived OIDC token instead of a stored API token. The nightly pipeline
# publishes each night and publish.yml publishes by hand, both through
# common-ci's shared publish, and crates.io checks the workflow that started
# the run, so each needs its own registration.
#
# Run this again every time a crate is added to ci/crates.txt. An unauthorised
# crate does not fail quietly, because the publish workflow reaches it, is
# refused with 403, and stops, leaving the rest of the release unpublished.
#
# A brand new crate name also has to be published by hand once before this can
# authorise it, because Trusted Publishing tokens are not allowed to create
# crates. Publish it with `cargo publish -p <crate>` from an account that owns
# the others, then run this.
# See https://crates.io/docs/trusted-publishing.
#
# Run with a crates.io API token in CRATES_IO_TOKEN belonging to an owner of the
# crates:
#
#   CRATES_IO_TOKEN=cio_xxx bash ci/setup-trusted-publishing.sh
#
# A crate that already has a trusted publisher for a workflow is skipped for
# that workflow, so running this more than once changes nothing.
# After this has run, the publish workflow no longer needs CARGO_REGISTRY_TOKEN.
set -uo pipefail

: "${CRATES_IO_TOKEN:?Set CRATES_IO_TOKEN to a crates.io API token that owns the crates}"

OWNER="51Degrees"
REPO="51degrees-rust"
WORKFLOWS=("nightly-pipeline.yml" "publish.yml")
API="https://crates.io/api/v1/trusted_publishing/github_configs"
UA="51degrees-trusted-publishing-setup (support@51degrees.com)"

# The crates to authorise, read from the single list this script and
# ci/publish-package.ps1 share, so a crate can never be published without also
# being authorised. Blank lines and comments are skipped.
CRATES_FILE="$(dirname "$0")/crates.txt"
if [ ! -f "$CRATES_FILE" ]; then
  echo "Crate list not found at $CRATES_FILE" >&2
  exit 1
fi
mapfile -t CRATES < <(grep -vE '^[[:space:]]*(#|$)' "$CRATES_FILE")
if [ "${#CRATES[@]}" -eq 0 ]; then
  echo "Crate list $CRATES_FILE names no crates" >&2
  exit 1
fi

# Returns 0 when one of the crate in $1's github configs names this owner,
# repository and the workflow in $2, 1 when none does, and 2 when the configs
# cannot be listed. Each config is put on its own line so the three are matched
# within one config. The owner and repository ignore case, as crates.io does
# when it matches them.
already_configured() {
  local configs
  configs=$(curl -fsS -H "User-Agent: $UA" -H "Authorization: $CRATES_IO_TOKEN" \
    "$API?crate=$1" 2>/dev/null) || return 2
  printf '%s' "$configs" | tr '{' '\n' \
    | grep -i "\"repository_owner\":\"$OWNER\"" \
    | grep -i "\"repository_name\":\"$REPO\"" \
    | grep "\"workflow_filename\":\"$2\"" >/dev/null
}

failures=0
for crate in "${CRATES[@]}"; do
  for workflow in "${WORKFLOWS[@]}"; do
    already_configured "$crate" "$workflow"
    case $? in
      0)
        echo "= $crate ($workflow): trusted publisher already configured, skipping"
        continue
        ;;
      2)
        echo "! $crate ($workflow): FAILED -> could not list its trusted publishers"
        failures=$((failures + 1))
        continue
        ;;
    esac
    body=$(printf '{"github_config":{"crate":"%s","repository_owner":"%s","repository_name":"%s","workflow_filename":"%s","environment":null}}' \
      "$crate" "$OWNER" "$REPO" "$workflow")
    resp=$(curl -sS -X POST -H "User-Agent: $UA" -H "Authorization: $CRATES_IO_TOKEN" \
      -H "Content-Type: application/json" -d "$body" "$API" 2>&1)
    if echo "$resp" | grep -q '"github_config"'; then
      echo "+ $crate ($workflow): trusted publisher registered"
    else
      echo "! $crate ($workflow): FAILED -> $resp"
      failures=$((failures + 1))
    fi
  done
done

echo
if [ "$failures" -gt 0 ]; then
  echo "Trusted publishing setup finished with $failures failure(s)."
  exit 1
fi
echo "Trusted publishing configured for all crates."
