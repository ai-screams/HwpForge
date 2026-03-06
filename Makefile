.PHONY: help install-tools check test test-ci clippy fmt fmt-fix lint-md lint-md-fix doc cov deny machete msrv ci ci-fast ci-full clean

help:
	@echo "HwpForge Development Commands"
	@echo ""
	@echo "Setup:"
	@echo "  make install-tools    Install development tools"
	@echo ""
	@echo "Development:"
	@echo "  make check            Cargo check (workspace)"
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
	cargo install cargo-nextest
	cargo install cargo-llvm-cov
	cargo install bacon
	cargo install cargo-deny
	cargo install cargo-machete
	cargo install dprint
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

msrv:
	cargo +1.88 check --workspace --all-features

ci-fast: fmt clippy test deny lint-md
	@echo "✅ Fast CI checks passed!"

ci-full: ci-fast cov msrv
	@echo "✅ Full CI checks passed!"

ci: ci-fast
	@echo "✅ CI checks passed!"

clean:
	cargo clean
