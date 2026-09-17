#!/usr/bin/env bash
# Re-verify that the tag still points at the commit the resolve job froze.
#
# `resolve` dereferences `refs/tags/<tag>` to a commit SHA once and every later
# job checks out that SHA, so a tag moved mid-run cannot change what is built.
# This closes the other half: it proves the tag the operator named still means
# that commit at publish time, so we never upload bytes from one commit under a
# tag that now points somewhere else.
set -euo pipefail

: "${TAG:?TAG is required}"
: "${EXPECTED_SHA:?EXPECTED_SHA is required}"

listing="$(mktemp)"
trap 'rm -f "$listing"' EXIT
git ls-remote --tags origin "refs/tags/${TAG}" "refs/tags/${TAG}^{}" > "$listing"

if [ ! -s "$listing" ]; then
  echo "::error::refs/tags/${TAG} no longer exists on the remote"
  exit 1
fi
cat "$listing"

# An annotated tag lists both the tag object and its peeled commit; the peeled
# line is the commit, so it wins when present.
actual="$(
  awk -v plain="refs/tags/${TAG}" -v peeled="refs/tags/${TAG}^{}" \
    '$2 == peeled { p = $1 } $2 == plain { t = $1 } END { print (p != "" ? p : t) }' "$listing"
)"

if [ -z "$actual" ]; then
  echo "::error::could not read a commit for refs/tags/${TAG} from the remote"
  exit 1
fi
if [ "$actual" != "$EXPECTED_SHA" ]; then
  echo "::error::refs/tags/${TAG} now points at ${actual}, but this run resolved and built ${EXPECTED_SHA}"
  exit 1
fi
echo "refs/tags/${TAG} still points at ${EXPECTED_SHA}"
