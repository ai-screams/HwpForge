.PHONY: help install-tools check check-features test test-ci clippy fmt fmt-fix lint-md lint-md-fix doc site-check cov deny machete msrv msrv-pdf ci ci-fast ci-full clean audit-hwp5 audit-hwp5-baseline audit-hwp5-gate skill-test py-dev py-test py-cov py-lint py-check py-rust-test py-all

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
	@echo "  make site-check       Build the docs site like Pages (mdBook 0.4.52 on PATH) + postprocess check"
	@echo "  make cov              Code coverage (llvm-cov, fail-under-lines=90)"
	@echo "  make deny             Dependency license/advisory check"
	@echo "  make machete          Find unused dependencies"
	@echo "  make msrv             MSRV compatibility check (Rust 1.88)"
	@echo "  make msrv-pdf         MSRV check for the 1.92 crates + fuzz smoke"
	@echo ""
	@echo "Python bindings (uv):"
	@echo "  make py-dev           Build + install the extension into the uv env"
	@echo "  make py-test          py-dev, then pytest"
	@echo "  make py-cov           py-dev, then coverage (fail-under=90)"
	@echo "  make py-lint          ruff check + ruff format --check"
	@echo "  make py-check         ty check (type check)"
	@echo "  make py-rust-test     Rust unit tests of the binding crate (no extension-module)"
	@echo "  make py-all           py-lint -> py-check -> py-rust-test -> py-test -> py-cov"
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

# bindings-py 제외: `--all-features` 는 `extension-module` 을 켜고, 그 상태의
# 테스트 바이너리는 libpython 을 링크하지 않는다. 지금은 유닛 테스트가 pyo3 C API
# 를 참조하지 않아 링커가 잘라내지만(실측: 미해결 `_Py*` 심볼 0) 테스트 하나가
# `PyErr`·`Python` 을 건드리면 Linux 링크가 깨진다 — 그때 워크스페이스 게이트가
# 빨개지지 않게 미리 제외한다. 그 크레이트의 Rust 유닛 테스트는 `py-rust-test`,
# 층 자체는 `py-test`(pytest) 가 본다. ci.yml 의 `Verify › Test`·`Verify › Coverage`
# 와 같은 목록이므로 두 곳을 함께 고친다. clippy·check 는 링크하지 않아 제외가 없다.
test:
	cargo nextest run --workspace --all-features \
	  --exclude hwpforge-bindings-py

test-ci:
	cargo nextest run --workspace --all-features --profile ci \
	  --exclude hwpforge-bindings-py

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

# Pages 와 같은 사이트 빌드 (mdBook + rustdoc + canonical/홈 링크 후처리). `make ci` 에는 넣지 않는다.
site-check:
	scripts/build_site.sh

# 제외 사유는 `test` 와 같다. 순수 Python 층의 90% 게이트는 `make py-cov`.
cov:
	cargo llvm-cov nextest --workspace --all-features --fail-under-lines 90 --html \
	  --exclude hwpforge-bindings-py

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

# Python 바인딩 (계획 부록 A-3·A-6 의 canonical 명령). 환경·실행·인터프리터는
# 전부 uv 가 소유한다 — pip·venv·mypy·pyright 경로는 쓰지 않는다. 인터프리터를
# 바꾸려면 `UV_PYTHON=3.9 UV_PROJECT_ENVIRONMENT=.venv/3.9 make py-test` 처럼
# (형제 이름 `.venv-3.9` 는 gitignore·ruff 기본 제외에 걸리지 않아 `ruff check .`
# 이 그 안을 훑는다 — `.venv/` 아래로 넣는다)
# 환경변수로 넘긴다 (CI 의 `Verify › Python` 루프가 그렇게 부른다).
PY_DIR ?= crates/hwpforge-bindings-py

py-dev:
	cd $(PY_DIR) && uv run maturin develop --release

py-test: py-dev
	cd $(PY_DIR) && uv run pytest tests -q

py-cov: py-dev
	cd $(PY_DIR) && uv run coverage run -m pytest tests
	cd $(PY_DIR) && uv run coverage report --fail-under=90

py-lint:
	cd $(PY_DIR) && uv run ruff check .
	cd $(PY_DIR) && uv run ruff format --check .

py-check:
	cd $(PY_DIR) && uv run ty check

# 워크스페이스 nextest·coverage 에서 제외된 크레이트의 Rust 유닛 테스트. 여기서
# 돌지 않으면 어디서도 돌지 않는다. `extension-module` 을 끄고(기본 feature 아님)
# 돌리는 것이 지원되는 조합 — pyo3 가 libpython 을 정상 링크하므로 테스트가 앞으로
# pyo3 C API 를 참조해도 깨지지 않는다. uv 가 설치한 인터프리터는 공유 libpython 을
# 포함한다. release 프로파일은 `py-dev` 가 만든 의존성 아티팩트를 재사용하기 위한 것.
# 테스트 바이너리는 uv 가 설치한 인터프리터의 공유 libpython 을 링크하는데, 그 lib
# 디렉터리는 로더 경로에 없다(Linux 러너 실측: `libpython3.11.so.1.0: cannot open shared
# object file`). 인터프리터가 아는 `LIBDIR` 를 로더 경로 앞에 붙인다 — macOS 는 rpath 로
# 이미 찾지만 같은 변수를 두어도 무해하다.
py-rust-test:
	PY="$$(uv python find 3.11)"; \
	LIBDIR="$$("$$PY" -c 'import sysconfig; print(sysconfig.get_config_var("LIBDIR"))')"; \
	PYO3_PYTHON="$$PY" \
	LD_LIBRARY_PATH="$$LIBDIR$${LD_LIBRARY_PATH:+:$$LD_LIBRARY_PATH}" \
	DYLD_LIBRARY_PATH="$$LIBDIR$${DYLD_LIBRARY_PATH:+:$$DYLD_LIBRARY_PATH}" \
	cargo nextest run -p hwpforge-bindings-py --cargo-profile release

py-all: py-lint py-check py-rust-test py-test py-cov
	@echo "✅ Python binding checks passed!"

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
