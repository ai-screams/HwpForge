.PHONY: help install-tools check test test-ci clippy fmt fmt-fix lint-md lint-md-fix doc cov deny msrv ci ci-fast ci-full clean

help:
	@echo "HwpForge Development Commands"
	@echo ""
	@echo "Setup:"
	@echo "  make install-tools    Install development tools"
	@echo ""
	@echo "Development:"
	@echo "  make check            Cargo check"
	@echo "  make test             Run tests (cargo-nextest)"
	@echo "  make test-ci          Run tests with CI profile (nextest + junit)"
	@echo "  make clippy           Run clippy linter"
	@echo "  make fmt              Format code (rustfmt)"
	@echo "  make lint-md          Lint & format Markdown/TOML/JSON"
	@echo "  make doc              Generate documentation"
	@echo "  make cov              Code coverage (llvm-cov, fail-under-lines=90)"
	@echo "  make msrv             MSRV compatibility check (Rust 1.75)"
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
	@echo "Installing lint/format tools..."
	brew install dprint pre-commit
	npm install -g markdownlint-cli2
	pre-commit install
	@echo "Done!"

check:
	cargo check --all-targets --all-features

test:
	cargo nextest run --all-features

test-ci:
	cargo nextest run --all-features --profile ci --junit-report junit.xml

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

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
	cargo doc --all-features --no-deps --open

cov:
	cargo llvm-cov nextest --all-features --fail-under-lines 90 --html

deny:
	cargo deny check

msrv:
	cargo +1.75 check --workspace --all-targets --all-features

ci-fast: fmt clippy test deny lint-md
	@echo "✅ Fast CI checks passed!"

ci-full: ci-fast cov msrv
	@echo "✅ Full CI checks passed!"

ci: ci-fast
	@echo "✅ CI checks passed!"

clean:
	cargo clean
	rm -rf target/
	find . -name "Cargo.lock" -delete
