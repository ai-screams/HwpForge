.PHONY: help install-tools check check-features test test-ci clippy fmt fmt-fix lint-md lint-md-fix doc cov deny machete msrv msrv-pdf ci ci-fast ci-full clean audit-hwp5 audit-hwp5-baseline audit-hwp5-gate skill-test

AUDIT_HWP5_FIXTURE_DIRS ?= tests/fixtures crates/hwpforge-smithy-hwp5/tests/fixtures crates/hwpforge-smithy-hwpx/tests/fixtures
AUDIT_HWP5_BASELINE   ?= .audit/hwp5_baseline.json
AUDIT_HWP5_CURRENT    ?= .audit/hwp5_current.json

MDBOOK_VERSION ?= 0.4.52
MDBOOK_ADMONISH_VERSION ?= 1.20.0
MDBOOK_MERMAID_VERSION ?= 0.16.2

# Use sccache as the compiler cache when it is on PATH (graceful no-op when
# absent — contributors without sccache build normally, nothing breaks). This
# speeds up the repeated full compiles in `make ci` (clippy -> test) and across
# runs. Scoped to make targets on purpose: release tooling (release-plz runs
# `cargo publish` via its own action, not make) is intentionally unaffected.
# Install for the speedup: `cargo install sccache` (or `brew install sccache`).
SCCACHE := $(shell command -v sccache 2>/dev/null)
ifneq ($(SCCACHE),)
export RUSTC_WRAPPER := $(SCCACHE)
endif

help:
	@echo "HwpForge Development Commands"
	@echo ""
	@echo "Setup:"
	@echo "  make install-tools    Install development tools"
	@echo ""
	@echo "Development:"
	@echo "  make check            Cargo check (workspace)"
	@echo "  make check-features   Cargo check every hwpforge feature combination"
	@echo "  make test             Run tests (cargo-nextest, workspace)"
	@echo "  make test-ci          Run tests with CI profile (nextest + junit)"
	@echo "  make clippy           Run clippy linter (workspace)"
	@echo "  make fmt              Check code formatting (rustfmt)"
	@echo "  make fmt-fix          Fix code formatting (rustfmt)"
	@echo "  make lint-md          Lint Markdown/TOML/JSON (dprint + markdownlint)"
	@echo "  make lint-md-fix      Fix Markdown/TOML/JSON formatting"
	@echo "  make doc              Generate documentation (opens browser)"
	@echo "  make cov              Code coverage (llvm-cov, fail-under-lines=90)"
	@echo "  make deny             Dependency license/advisory check"
	@echo "  make machete          Find unused dependencies"
	@echo "  make msrv             MSRV compatibility check (Rust 1.88)"
	@echo "  make msrv-pdf         MSRV check for the 1.92 crates + fuzz smoke"
	@echo ""
	@echo "CI:"
	@echo "  make ci-fast          Fast CI checks (fmt/clippy/test/deny/lint-md)"
	@echo "  make ci-full          Full CI checks (+coverage/msrv)"
	@echo "  make ci               Alias of ci-fast"
	@echo ""
	@echo "Cleanup:"
	@echo "  make clean            Remove build artifacts"

install-tools:
	@echo "Installing Rust development tools..."
	cargo install --locked cargo-nextest
	cargo install --locked cargo-llvm-cov
	cargo install --locked bacon
	cargo install --locked cargo-deny
	cargo install --locked cargo-machete
	cargo install --locked dprint
	cargo install --locked --version $(MDBOOK_VERSION) mdbook
	cargo install --locked --version $(MDBOOK_ADMONISH_VERSION) mdbook-admonish
	cargo install --locked --version $(MDBOOK_MERMAID_VERSION) mdbook-mermaid
	@echo "Installing Python build tool (maturin via uv)..."
	@if command -v uv >/dev/null 2>&1; then \
		uv tool install 'maturin>=1.15,<2'; \
	else \
		echo "⚠ uv not found — skipping maturin (install uv first: https://docs.astral.sh/uv/)"; \
	fi
	@echo "Installing lint/format tools..."
	@if command -v npm >/dev/null 2>&1; then \
		npm install -g markdownlint-cli2; \
	else \
		echo "⚠ npm not found — skipping markdownlint-cli2 (install Node.js first)"; \
	fi
	@if command -v pipx >/dev/null 2>&1; then \
		pipx install pre-commit; \
	elif command -v pip3 >/dev/null 2>&1; then \
		pip3 install --user pre-commit; \
	else \
		echo "⚠ pipx/pip3 not found — skipping pre-commit (install Python first)"; \
	fi
	@if command -v pre-commit >/dev/null 2>&1; then \
		pre-commit install; \
	fi
	@echo "Done!"

check:
	cargo check --workspace --all-targets --all-features

# The umbrella crate is the one place where feature wiring can break a
# consumer silently: a default-feature user must still compile, and the ops
# layer must not leak into builds that did not ask for it. Each combination
# starts from --no-default-features so nothing is enabled by accident.
check-features:
	cargo check -p hwpforge --no-default-features
	cargo check -p hwpforge --no-default-features --features hwpx
	cargo check -p hwpforge --no-default-features --features md
	cargo check -p hwpforge --no-default-features --features ops-hwpx
	cargo check -p hwpforge --no-default-features --features ops-md
	cargo check -p hwpforge --no-default-features --features ops
	cargo check -p hwpforge --no-default-features --features schemars
	cargo check -p hwpforge --no-default-features --features full

test:
	cargo nextest run --workspace --all-features

test-ci:
	cargo nextest run --workspace --all-features --profile ci

clippy:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

fmt:
	cargo fmt --all -- --check

fmt-fix:
	cargo fmt --all

lint-md:
	dprint check
	npx markdownlint-cli2 "**/*.md"

lint-md-fix:
	dprint fmt
	npx markdownlint-cli2 --fix "**/*.md"

doc:
	cargo doc --workspace --all-features --no-deps --open

cov:
	cargo llvm-cov nextest --workspace --all-features --fail-under-lines 90 --html

deny:
	cargo deny --all-features check

machete:
	cargo machete

# ci.yml `Verify › MSRV (1.88)` 와 같은 제외 목록 — 1.92 를 선언한 네 크레이트는
# 전부 publish=false 라 MSRV 소비자 계약이 없다. 목록이 어긋나면 로컬만 통과하고
# 큐에서 깨지므로 두 곳을 함께 고친다.
msrv:
	cargo +1.88 check --workspace --all-features \
	  --exclude hwpforge-smithy-pdf \
	  --exclude hwpforge-bindings-cli \
	  --exclude hwpforge-convert \
	  --exclude hwpforge-bindings-py

# 위에서 제외한 넷의 실질 MSRV 레인 + fuzz 스모크 (fuzz 는 별도 워크스페이스라
# --workspace 가 닿지 않는데 convert 를 path 로 의존한다).
msrv-pdf:
	cargo +1.92 check --all-features \
	  -p hwpforge-smithy-pdf \
	  -p hwpforge-bindings-cli \
	  -p hwpforge-convert \
	  -p hwpforge-bindings-py
	cargo +1.92 check --manifest-path fuzz/Cargo.toml

ci-fast: fmt clippy test deny lint-md
	@echo "✅ Fast CI checks passed!"

ci-full: ci-fast cov msrv msrv-pdf
	@echo "✅ Full CI checks passed!"

ci: ci-fast
	@echo "✅ CI checks passed!"

audit-hwp5:
	@mkdir -p .audit
	cargo run -q -p hwpforge-convert --example audit_batch -- $(AUDIT_HWP5_FIXTURE_DIRS) > $(AUDIT_HWP5_CURRENT)
	@echo "audit-hwp5 → $(AUDIT_HWP5_CURRENT)"

audit-hwp5-baseline: audit-hwp5
	cp $(AUDIT_HWP5_CURRENT) $(AUDIT_HWP5_BASELINE)
	@echo "audit-hwp5 baseline refreshed → $(AUDIT_HWP5_BASELINE)"

audit-hwp5-gate: audit-hwp5
	python3 scripts/audit_hwp5_gate.py --baseline $(AUDIT_HWP5_BASELINE) --current $(AUDIT_HWP5_CURRENT)

skill-test:
	bash scripts/skill-smoke.sh

clean:
	cargo clean
