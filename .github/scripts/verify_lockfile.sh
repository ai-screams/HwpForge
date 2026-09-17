#!/usr/bin/env bash
# Assert Cargo.lock is still the one resolution the `resolve` job produced.
#
# Cargo.lock is untracked in this repository, so the resolve job generates it
# once and every matrix job builds from that same file. This runs twice per job:
# once before the build, so a corrupted download cannot become a wheel, and once
# after, because a maturin invocation that regenerates or updates the lockfile
# would otherwise break the one-resolution invariant while the pre-build check
# still looked green (`maturin sdist` takes no `--locked`, so this is the only
# guard on that job).
set -euo pipefail

: "${LOCK_SHA256:?LOCK_SHA256 is required}"

actual="$(
  uv run --no-project python -c \
    'import hashlib; print(hashlib.sha256(open("Cargo.lock", "rb").read()).hexdigest())' | tail -n 1
)"

echo "${WHEN:-lockfile} expected ${LOCK_SHA256}"
echo "${WHEN:-lockfile} actual   ${actual}"

if [ "$actual" != "$LOCK_SHA256" ]; then
  echo "::error::Cargo.lock ${WHEN:-check} differs from the resolution the resolve job froze"
  exit 1
fi
