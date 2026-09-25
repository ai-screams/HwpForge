#!/usr/bin/env bash
# =============================================================================
# Build the documentation site: mdBook + rustdoc (under book/api) + postprocess
# =============================================================================
# Shared by .github/workflows/pages.yml, the CI "Docs Build" lane and
# `make site-check`, so all three build the same tree the same way.
#
#   1. fail fast unless the docs toolchain matches the pins exactly
#   2. rustdoc goes to its own target dir (target/site-build); only its doc/
#      is cleaned, the default target/doc is left alone
#   3. mdbook build -> cargo doc -> copy to book/api -> postprocess -> verify
# =============================================================================
set -euo pipefail

# Keep these equal to MDBOOK_*_VERSION in ci.yml / pages.yml / Makefile.
MDBOOK_PIN="0.4.52"
MDBOOK_ADMONISH_PIN="1.20.0"
MDBOOK_MERMAID_PIN="0.16.2"
RUSTC_LINE="1.93"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$repo_root"

die() {
  echo "build_site: $*" >&2
  exit 1
}

check_pin() { # <env var name> <script pin>
  local name="$1" pin="$2" value="${!1:-}"
  if [[ -n "$value" && "$value" != "$pin" ]]; then
    die "$name=$value in the environment, but this script pins $pin"
  fi
}

check_tool() { # <tool> <exact --version output>
  local tool="$1" want="$2" got
  command -v "$tool" >/dev/null 2>&1 || die "$tool not found on PATH (need: $want)"
  got="$("$tool" --version 2>/dev/null)" || die "$tool --version failed (need: $want)"
  [[ "$got" == "$want" ]] || die "$tool version mismatch: found '$got', need '$want'"
}

check_pin MDBOOK_VERSION "$MDBOOK_PIN"
check_pin MDBOOK_ADMONISH_VERSION "$MDBOOK_ADMONISH_PIN"
check_pin MDBOOK_MERMAID_VERSION "$MDBOOK_MERMAID_PIN"
check_tool mdbook "mdbook v$MDBOOK_PIN"
check_tool mdbook-admonish "mdbook-admonish $MDBOOK_ADMONISH_PIN"
check_tool mdbook-mermaid "mdbook-mermaid $MDBOOK_MERMAID_PIN"
rustc_version="$(rustc --version)" || die "rustc not found"
[[ "$rustc_version" =~ ^rustc\ ${RUSTC_LINE//./\\.}\.[0-9]+(\ |$) ]] \
  || die "rustc version mismatch: found '$rustc_version', need $RUSTC_LINE.x"
command -v python3 >/dev/null 2>&1 || die "python3 not found on PATH"

# Set unconditionally so an inherited CARGO_TARGET_DIR cannot move the clean step.
export CARGO_TARGET_DIR="$repo_root/target/site-build"
mkdir -p "$CARGO_TARGET_DIR"
physical="$(cd "$CARGO_TARGET_DIR" && pwd -P)"
[[ "$physical" == "$repo_root/target/site-build" ]] \
  || die "target dir resolves to '$physical', expected '$repo_root/target/site-build'"
rm -rf "$physical/doc"

mdbook build

# Same crate set as the published site has always had.
cargo doc --workspace --no-deps --all-features \
  --exclude hwpforge-bindings-py \
  --exclude hwpforge-bindings-cli \
  --exclude hwpforge-smithy-hwp5

rm -rf book/api
cp -R "$physical/doc" book/api

python3 scripts/site_postprocess.py book
python3 scripts/site_postprocess.py --verify book
