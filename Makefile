.PHONY: help install-tools check test clippy fmt doc cov clean

help:
	@echo "HwpForge Development Commands"
	@echo ""
	@echo "Setup:"
	@echo "  make install-tools    Install development tools"
	@echo ""
	@echo "Development:"
	@echo "  make check            Cargo check"
	@echo "  make test             Run tests (cargo-nextest)"
	@echo "  make clippy           Run clippy linter"
	@echo "  make fmt              Format code (rustfmt)"
	@echo "  make doc              Generate documentation"
	@echo "  make cov              Code coverage (llvm-cov)"
	@echo ""
	@echo "CI:"
	@echo "  make ci               Run all CI checks"
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
	@echo "Installing pre-commit..."
	pip install pre-commit
	pre-commit install
	@echo "Done!"

check:
	cargo check --all-targets --all-features

test:
	cargo nextest run --all-features

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

fmt:
	cargo fmt --all -- --check

fmt-fix:
	cargo fmt --all

doc:
	cargo doc --all-features --no-deps --open

cov:
	cargo llvm-cov nextest --all-features --html

deny:
	cargo deny check

ci: fmt clippy test deny
	@echo "✅ All CI checks passed!"

clean:
	cargo clean
	rm -rf target/
	find . -name "Cargo.lock" -delete
