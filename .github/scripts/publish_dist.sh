#!/usr/bin/env bash
# Publish every artifact in dist/ to one index, through Trusted Publishing.
#
# Both publish jobs call this with a different TARGET_NAME/PUBLISH_URL/CHECK_URL,
# so the two paths cannot drift apart. `--check-url` is the only idempotency
# mechanism: PyPI never allows a filename to be reused, so uv skips a file that
# is already there with the same hash and fails if the contents differ. There is
# deliberately no version-level pre-check that would skip the whole run.
set -euo pipefail

: "${TARGET_NAME:?TARGET_NAME is required}"
: "${PUBLISH_URL:?PUBLISH_URL is required}"
: "${CHECK_URL:?CHECK_URL is required}"

# Trusted Publishing needs an OIDC token. Without `permissions: id-token: write`
# or with the GitHub environment missing, we say which of the two is missing
# instead of falling back to a token or exiting green.
if [ -z "${ACTIONS_ID_TOKEN_REQUEST_URL:-}" ]; then
  echo "::error::no OIDC token available — the job needs permissions.id-token: write and the '${TARGET_NAME}' environment"
  exit 1
fi

files=()
while IFS= read -r file; do
  files+=("$file")
done < <(find dist -maxdepth 1 -type f \( -name '*.whl' -o -name '*.tar.gz' \) | sort)

if [ "${#files[@]}" -eq 0 ]; then
  echo "::error::dist/ holds no wheel or sdist to publish"
  exit 1
fi

echo "publishing ${#files[@]} files to ${TARGET_NAME} (${PUBLISH_URL})"
printf '  %s\n' "${files[@]}"

log="${RUNNER_TEMP:-/tmp}/uv-publish.log"
set +e
uv publish \
  --trusted-publishing always \
  --publish-url "$PUBLISH_URL" \
  --check-url "$CHECK_URL" \
  "${files[@]}" 2>&1 | tee "$log"
status="${PIPESTATUS[0]}"
set -e

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  {
    echo "### ${TARGET_NAME}: ${#files[@]} files offered, check URL \`${CHECK_URL}\`"
    echo
    echo '```'
    grep -iE 'upload|skip' "$log" || echo "(uv printed no upload/skip lines; see the job log)"
    echo '```'
  } >> "$GITHUB_STEP_SUMMARY"
fi

# uv fails on its own for the interesting cases — no Trusted Publishing record
# for this owner/repository/workflow/environment, or a filename that already
# exists with different bytes — but it does so without a GitHub annotation, so
# the failure is easy to miss in a long log. Say it in the job's error channel
# and keep uv's status as this script's status.
if [ "$status" -ne 0 ]; then
  echo "::error::uv publish failed against ${TARGET_NAME} (exit ${status}); the usual causes, most likely first: the artifacts differ from what is already on the index because this run rebuilt them (re-run the publish job of the run that built the published artifacts instead of dispatching again), a missing or mismatched Trusted Publishing record (owner, repository, workflow file, environment), or a file that already exists with different contents"
  exit "$status"
fi
