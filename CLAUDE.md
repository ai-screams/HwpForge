# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

---

## Project Overview

HwpForge is a Rust library for programmatic control of Korean HWP/HWPX document formats, designed with LLM-first principles. The goal is to enable AI agents (like Claude Code) to generate Korean government proposal documents using natural language + Markdown + YAML style templates.

**Current Status** (snapshot — 2026-07-25):

- HWPX codec: read/write shipped · Markdown bridge: read/write shipped
- HWP5 → HWPX converter path: active, style/layout fidelity line in progress
- CLI bindings: shipped · MCP bindings: shipped · Python bindings: stub
- Shared `tab` / `ordered·bullet·outline` / checkable-bullet semantics wired through core → blueprint → smithy. HWP5 checkable carries all three gotcha-#8 truth locations (`bullet.checkedChar`, `bullet.paraHead.checkable`, `paraPr.checked`).
- Phase 12 HWP5→HWPX carry series (GSO shapes/equation/memo/dutmal/compose/indexmark/click-here·auto fields/cross-ref instId/document metadata/outline 1–10) `main` 머지 완료.
- **E6 IR 와이어-누출 상환 완료** (`0.9.0`): Summery rename(A) · BookmarkName collapse(B) · raw wire 필드 제거(C) · `inst_id`/`SystemId`→공유 `ObjectId`(M2). **H1(display_text)=Won't-do** (ADR-009 §CLOSURE, memory `e6-wire-leak-status-h1-wontdo.md`).
- **0.10.0 릴리스** (2026-07-02): colLine(다단 구분선) HWPX+HWP5 carry(breaking) + rmcp 2.0 보안(GHSA-89vp-x53w-74fx)·quick-xml 0.41 + 문서 정합.
- **AI 편집(에이전트 편집) 전 에픽 배포 완료** (2026-07-12~24, `0.11.0`→`0.11.6`): E1 누름틀 채우기(`0.11.0`, `feat!:` `display_text` 의미 변경) · E2 `fill` 델타 API·`fields`(`0.11.1`) · E6 템플릿 스탬핑 W1(`0.11.2`) · E3 표 격자 주소+`set-cell`(`0.11.3`) · E6 W2 클래스-B 셀 스탬핑+`layout_carry` linesegarray 보존(`0.11.4`) · E5 outline/read/diff 읽기 3표면(`0.11.5`) · E4 문단 구조 편집 `insert-para`/`delete-para`(`0.11.6`, preserve-first 바이트 스플라이스 + reverse-delta self-verify). 설계/로드맵 = `.docs/planning/2026-07-10-agent-editing-architecture.md` (남은 후속: E4b 표 행 편집 · `<hp:t>` 줄 경계 분할 보존 등 backlog).

> **이 섹션은 짧은 상태 스냅샷으로만 유지한다 (wave-by-wave 이력을 여기 다시 쌓지 말 것).**
> Wave별 상세 이력 + breaking change: **`CHANGELOG.md`** (canonical) 와 memory `MEMORY.md` / `phase11_wave_history.md`.
> Enum/wire 레이아웃 표 (번호·쪽번호·이미지채우기·대각선 등): **`crates/hwpforge-smithy-hwp5/HWP5_WIRE_SPEC.md`** (특히 §22).

**Still-deferred (Windows 한컴 fixture 대기)**:

- **non-chart OLE passthrough** — 전 구간 구현됐으나 standalone `<hp:ole>` 가 macOS 한컴 crash, macOS는 생성 자체 불가 → `git stash` 보존 (memory `non-chart-ole-deferred.md`)
- **masterPage carry** (Wave 5 gap C, task #33) · **쪽 테두리/배경 hatch _페이지_ 경로** (char/table hatch 는 byte-verified 완료, 공유 코드 — macOS [쪽] 메뉴에 항목 없음)
- **양식컨트롤(form controls)** — 완전 무음 드롭, `b"form"` + `HWPTAG_FORM_OBJECT(0x5B)` 미구현 (memory `form-controls-deferred.md`)
- **가운데 밑줄** (macOS 한글 밑줄 위치에 "가운데" 옵션 없음) · 한컴-authored multi-run span 디코딩 (편집 prerequisite, task #96)

**Known lossy (Core breaking 필요, 후속 슬라이스)**: 글자 그림자 색·위치, 스크립트별 자간/장평, 한영 자동 간격, 문단 세로정렬·테두리 오프셋 등 (P2 — 경고는 나감) + enum 천장 (P3 — UnderlineShape/StrikeoutShape/EmphasisType 등 raw 초과분). 상세 backlog: `.docs/planning/BACKLOG_SMITHY_HWPX.md` + 분석 `.docs/audit/2026-06-17_hwp5_hwpx_option_gaps.md`.

**Workspace Facts** (code-grounded — 카운트는 drift하니 인용 전 확인):

- Cargo packages `11` · crates.io published `0.11.6` (E4 문단 구조 편집, 2026-07-24) · MSRV `1.88` · Dev toolchain Rust `1.93`
- `crates/` 추적 src 파일 ~`177` · nextest ~`2,688` passed + `2` skipped · `examples/` 산출물 `68`+ (미추적 `examples/hwp5_review/` 리뷰 영역 별도 — gitignore 아님) · GitHub workflows `5`

---

## Architecture (Forge Metaphor)

The codebase follows a **blacksmith workshop** metaphor with clear separation of concerns:

```
Foundation (🔩 primitives)
  → Core (🔨 pure document structure, no style definitions)
  → Blueprint (📐 YAML style templates, centralized like Figma Design Tokens)
  → Smithy (🔥 format-specific compilers: HWPX, HWP5, Markdown)
  → Bindings (🐍⚒️🤖 Python/CLI/MCP interfaces)
```

**Key Principle**: **Structure and Style are separate** (like HTML + CSS).

- Core contains document structure with style **references** (IDs only)
- Blueprint contains style **definitions** (fonts, sizes, colors)
- Smithy compilers fuse Core + Blueprint → final format

This enables:

- One YAML template applied to multiple documents
- Format-agnostic document manipulation
- Easy addition of new formats (smithy-odt, smithy-pdf, etc.)

---

## Development Commands

### Build & Test

```bash
cargo build --workspace
cargo nextest run --workspace --all-features
cargo test -p hwpforge-foundation
make test
make ci-fast
make ci-full
```

### Lint & Format

```bash
cargo clippy -p hwpforge-foundation -- -D warnings
make clippy
make fmt
make fmt-fix
```

### Watch Mode

```bash
bacon         # Auto-run clippy on file changes
bacon test    # Auto-run tests
```

### Tooling Gotchas (pre-commit / test)

- **dprint + 한글(CJK) 마크다운 표**: 한글이 든 `.md` 표(예: `HWP5_WIRE_SPEC.md`, `CHANGELOG.md`)를 편집하면 dprint pre-commit 훅이 거부함(CJK 글자 폭 재계산으로 표 정렬 불일치 판단). `dprint fmt <파일>` 수동 실행 → 재-stage → 재커밋.
- **`cargo nextest run -p <crate> <filter>`** 의 필터는 정규식이 아니라 **부분일치(substring)** — `'a|b'` 는 아무것도 안 잡음. 공통 substring 하나(예: `warns`)로 필터하거나 따로 실행.
- 용량 큰 이미지 임베드 fixture(~MB)는 리뷰 산출물 영역 `examples/hwp5_review/`(미추적 — `git add -A` 주의. **디렉터리 통째 gitignore 금지**: tracked 리뷰 샘플 39개가 있어 cargo/release-plz 가 "committed and in .gitignore" 로 실패, PR #92)에만 두고, 회귀 방지는 **단위 테스트로 잠금**(수 MB fixture를 커밋하지 말 것).
- **pre-commit `cargo fmt` 훅**: 스테이지된 Rust(특히 테스트의 다줄 배열/`assert!`)를 재포맷하며 커밋을 **거부**함 → `cargo fmt` 수동 실행 → 재-`git add` → 재커밋 (dprint 표와 동일 패턴).
- **pre-commit/pre-push 훅이 workspace clippy(+`make ci`)를 돌림** → 다파일 커밋·push는 **2분+**, cold/contended 빌드 땐 **20분+** 까지 감. 항상 `run_in_background`로 commit/push 후 폴링.
- **docs-only 커밋/push 는 빠름**: pre-commit clippy·fmt 는 staged 에 Rust 파일이 있을 때만 실행(`no files to check` skip) — "훅 2분+" 은 Rust 변경 시에만 해당 (백그라운드 실행 원칙은 동일).
- **`git push` 를 파이프에 연결 금지**(`| tail` 등) — pre-push 훅의 대량 테스트 출력이 `BlockingIOError [Errno 35]` 로 push 자체를 죽임. run_in_background(파일 리다이렉트)로 실행하고, 성공 판정은 `git ls-remote --heads origin <branch> | grep -q .` 로.
- pre-push `cargo deny` 가 RustSec advisory DB fetch 네트워크 오류로 간헐 실패 → 재시도로 해결.
- **전체 `cargo nextest run --workspace`는 cold 빌드 시 15분+** (foreground 한계 초과) → 변경 영향 크레이트만 `-p <crate>`로 돌리고 byte-중립 게이트만 골라 검증. **테스트 실행 중 소스 편집 금지**(rebuild 유발로 더 느려짐).
- **nextest 통합 테스트 파일 필터**: substring 은 테스트 _이름_ 만 매칭 (파일명 안 잡힘) — 파일 단위는 `-E 'binary(<파일명>)'`.
- **commit 출력도 `| tail` 로 자르지 말 것** — 실패한 훅 라인·exit code 가 사라져 "커밋됐다" 오판. 파일 리다이렉트 후 grep (push 파이프 금지 규칙과 동일 계열).
- **커밋 전 touched 크레이트만 `cargo clippy --all-targets -- -D warnings` 사전 점검** — 훅 거부 1회 = 2분+ 재사이클 (nextest/build 는 clippy lint 를 안 잡음).
- `rm` 은 대화형 alias — stale `.git/index.lock`(0바이트·git 프로세스 없음 확인 후) 등 스크립트 삭제는 `rm -f`.
- 대용량 정리: `target/`(수백 GB 가능)·`fuzz/target`·`.docs/papers/EAAI/eval/oracle-rs/target` 은 재생성 가능 빌드 산출물. `.docs/papers`(corpus·논문)·`fuzz/corpus` 는 자산 — 삭제 금지. **디스크 고갈 시 우선 삭제 = `target/debug/incremental`(94GB 실사고)·`target/llvm-cov-target`** — `target/debug/deps`(warm 의존성 캐시)는 보존해 cold 재빌드를 피한다.
- pre-commit 은 **미스테이지 변경을 stash 하고 staged 트리만 검사** — 다파일 수정에서 하나라도 `git add` 누락하면 staged 트리가 컴파일 실패로 거부됨 (원인이 "숨은 미스테이지 파일"이라 오진하기 쉬움). 커밋 전 `git status --short` 로 관련 파일 전부 staged(`M`) 확인.
- **zsh 는 미인용 변수를 word-split 하지 않음** — `CMD="node /x.mjs"; $CMD status` 는 전체가 하나의 명령명 (조용한 command-not-found → 루프/조건 오탐). 스크립트에서 명령을 변수에 담지 말고 인라인 전체 경로로 (`for x in $VAR` 미분리와 동계열).
- Bash 작업 디렉터리는 **호출 간 지속** — 앞서 `cd` 한 상태에서 레포-루트 상대 경로(git add 등)를 쓰면 pathspec fatal. 커밋/스테이지 명령은 절대 경로 또는 루트 복귀 후 실행.

### Documentation & Coverage

```bash
make doc
make cov
```

---

## Crate Dependency Graph

```
foundation (NO HwpForge crate deps; external only: serde/schemars/thiserror)
    ↓
core (foundation only)
    ↓
blueprint (foundation + core)
    ↓
smithy-hwpx, smithy-md (foundation + core + blueprint) · smithy-hwp5 (foundation + core only)
    ↓
convert (core + foundation + smithy-hwp5 + smithy-hwpx — HWP5→HWPX 오케스트레이터)
    ↓
bindings-py, bindings-cli (+ convert), bindings-mcp
```

**Important**: Foundation is the root. If you modify foundation, ALL crates rebuild. Keep it minimal.

---

## Critical Design Patterns

### Working Principles

- **Warning-first for unknowns**: if source truth is missing or a value is unsupported, emit a warning or validation signal first.
- **No fake support**: do not silently normalize unknown semantics into arbitrary defaults just to keep output green.
- **Unhandled enum ≠ bug**: an unmatched enum arm is a real gap only if that value actually exists in the reference enum (hwpxlib/libhwp) or a native fixture. Verify existence first — otherwise `_ => default` is correct and a guessed mapping is fake support. (See `HWP5_WIRE_SPEC.md §22`; 번호 code 11 / 이미지 채우기 모드 = real bugs, 가나다 · 대각선 1/4/5 = false positives.)
- **Shared-model first**: if HWP5 discovers a semantic that Core/HWPX cannot carry, extend the shared representation first and wire HWP5 after.
- **Semver-first for public API**: if a design touches public structs, enums, or externally constructible types, surface the breakage before implementation and get approval first.

### 1. Color is BGR (NOT RGB!)

```rust
// ❌ WRONG — This is BLUE in BGR!
Color::from_raw(0xFF0000)

// ✅ CORRECT — red → 0x0000FF internally
Color::from_rgb(255, 0, 0)
```

HWP format uses BGR (Blue-Green-Red) byte order. Always use `from_rgb()` constructor.

### 2. HwpUnit Integer-Based Units

```rust
HwpUnit::from_pt(12.0)  // 12pt → HwpUnit(1200)
// 1pt = 100 HWPUNIT, 1mm ≈ 283 HWPUNIT
```

Integer-based to avoid floating-point precision errors. Valid range: ±100M.

### 3. Branded Index Types

```rust
CharShapeIndex::new(0)   // ✅ OK
let idx: ParaShapeIndex = CharShapeIndex::new(0);  // ❌ Compile error!
```

`Index<T>` uses phantom types. Cannot mix char/para/font indices.

### 4. Typestate Pattern (Core)

```rust
let doc = Document::<Draft>::new();
// doc.save_hwpx(...);  // ❌ Compile error! Draft cannot be saved
let validated = doc.validate()?;
// validated.save_hwpx(...);  // ✅ OK
```

### 5. Two-Type Pattern (Blueprint)

```rust
// PartialCharShape: all fields Option (for YAML/inheritance merge)
let partial = PartialCharShape { font: Some("Batang".into()), size: Some(unit), ..Default::default() };
// CharShape: all fields required (after resolution)
let resolved: CharShape = partial.resolve("style_name")?;
```

### 6. StyleRegistry Pipeline (Blueprint → Smithy)

```rust
let template = Template::from_yaml(yaml_str)?;
let resolved = resolve_template(&template, &provider)?;
let registry = StyleRegistry::from_template(&resolved)?;
let entry = registry.get_style("body").unwrap();
```

### 7. Paragraph heading vs list semantics are NOT the same axis

- `Paragraph.heading_level` is currently closer to `titleMark` / TOC marker semantics.
- HWPX ordered / bullet / outline lists live in `paraPr/heading(type,idRef,level)`.
- Do not stuff list semantics into `Paragraph.heading_level` just because the names are similar.

### 8. Checkable bullet is still `BULLET`, not a new heading kind

In HWPX, checkable bullet still lowers as:

```text
heading(type="BULLET", idRef="...", level="...")
```

with three separate truth locations:

- `bullet.checkedChar` → definition-level checked glyph
- `bullet.paraHead.checkable` → checkable family marker
- `paraPr.checked` → per-item checked state

Wire only one of those and you did not implement checkable bullet. You painted the dashboard and left the engine block open.

### 9. Bullet `level` and glyph selection are different axes

- `level` controls nesting depth
- bullet glyph is selected by `bullet_id`

So leveled bullet glyph switching is not automatic numbered-style behavior. If a caller wants `level -> glyph` changes, that mapping must be explicit.

### 10. Markdown task lists are normalized to HWP semantics

- unordered task list (`- [ ] foo`) → `CheckBullet`
- ordered task list (`1. [ ] foo`) → numbering is intentionally discarded and normalized to `CheckBullet`

Do not invent `CheckNumber` or preserve Markdown-only semantics unless the shared HWP model can actually carry them.

### 11. Multi-paragraph task item continuation is a bridge concern

Markdown task items can contain continuation paragraphs. That does **not** mean HWPX/HWP gained a new list kind.

The correct interpretation is:

- first paragraph = actual `CheckBullet` item
- following paragraphs = same item continuation paragraphs

This is decoder/encoder bridge logic, not shared list-kind proliferation.

---

## Testing Strategy

### 3-Tier Approach

1. **Golden Tests** (most important): Real HWPX/HWP5 files from 한글 program
   - Fixtures in `tests/fixtures/`; golden test lives in `crates/hwpforge-smithy-hwpx/tests/golden.rs`
   - Load → Save → Load → assert equality

2. **Unit Tests**: Edge cases first (TDD)
   - Boundary values (MIN, MAX, zero)
   - Invalid inputs (INFINITY, NAN, empty string)
   - Normal cases last

3. **Property Tests**: `proptest` for invariants
   - Round-trip: `pt → HwpUnit → pt`
   - Round-trip: `RGB → BGR → RGB`

### Running Tests

```bash
cargo test --lib                    # Unit tests only
cargo test -p hwpforge-smithy-hwpx --test golden   # Golden tests only
cargo test -p hwpforge-foundation   # Specific crate
cargo llvm-cov --html               # Coverage report
```

Target: **90% line coverage in CI**.

---

## TDD Workflow

```
1. 🔴 RED: Write edge case tests FIRST (they should fail)
2. 🟢 GREEN: Minimal implementation to pass tests
3. 🔵 REFACTOR: Optimize/clean code (tests still pass)
4. ✅ COMMIT: Atomic commit per component
```

Example checklist for new type:

- [ ] 0, MIN, MAX boundary tests
- [ ] Overflow/underflow tests
- [ ] Invalid inputs (empty, null, special chars)
- [ ] Round-trip tests
- [ ] Normal cases

---

## YAGNI Removals (Learn from Phase 0)

These were planned but **removed as unnecessary** (keep it simple):

- ❌ SIMD Color operations (no batch processing yet)
- ❌ HwpUnit typestate (doubles size for minimal benefit)
- ❌ String interning (profile first, optimize second)
- ❌ miette diagnostics (heavy dependency)
- ❌ derive_more, strum (manual implementations = better error messages; still declared in `[workspace.dependencies]` but no crate opts in)

**Principle**: Add complexity only when proven necessary.

---

## Important Files & Directories

### `crates/`

Actual implementation lives here. Read `crates/AGENTS.md` and any crate-local `AGENTS.md` before changing a crate boundary.

### `examples/`

Generated artifacts and sample converters live here, organized into `showcase/`, `interop/`, `hwp5_review/`, `hwpx_roundtrip_review/`. `interop/hwpx_md_convert/hwpx2md/images/` is a helper output directory for Markdown conversion artifacts.

### `tests/`

Root `tests/` is primarily a fixture warehouse. It is not itself the main Rust integration-test crate.

### `.docs/`

Local planning and research workspace. It may be git-excluded in this repository setup, so never assume "not in git status" means "does not exist".

**Rule — 계획 문서는 `.docs/planning/` 에 작성한다** (내부 문서, git 미커밋). 마스터 플랜(epic 단위 전체 작업) + epic별 상세 실행 계획(TDD 단계/수용 기준)을 여기 둔다. 리서치/감사 산출물은 `.docs/audit/`, `.docs/research/`.

### Reference docs

- `.docs/references/openhwp/docs/hwpx/` — local KS X 6101 markdownized reference
- `.docs/research/` — local research logs and workstream notes
- `.docs/architecture/` — crate-role and design notes when present

---

## Current Engineering State

- 릴리스/기능 상태의 canonical = 상단 **Current Status** 스냅샷 (여기 중복 서술 금지).
- Table integration gates are concentrated in `crates/hwpforge-bindings-cli/tests/cli_integration.rs`.
- Stress or real-world table fixtures are not the same thing as committed regression gates.
- colLine (다단 구분선) HWPX + HWP5→HWPX legs shipped in `0.10.0` (PR #91, 2026-07-02).
- **Nightly › Fuzz Build 복구됨** (PR #101, 2026-07-19 — 2026-06-29부터 실패했었음): 원인 2겹 = ① prebuilt cargo-fuzz 가 musl 을 기본 타깃으로 골라 ASAN 과 충돌 → `security.yml` 에 `--target x86_64-unknown-linux-gnu` 명시 ② fuzz 타깃 bit-rot (`hwp5_to_hwpx_bytes` 가 convert 크레이트로 이사). fuzz/ 는 standalone 워크스페이스라 메인 CI 가 컴파일을 안 잡음 — API 이동 시 fuzz 타깃도 함께 갱신할 것.
- Always confirm `main` state from code + manifests + git; do not trust stale branch prose.

---

## Working on a New Slice

> **정식 에픽 워크플로우 = `.claude/rules/epic-workflow.md`** (자동 로드 — E3~E6·E4 로 검증된 프로세스, 그쪽이 canonical).
> 요약: 사전 확인(ground truth) → 연구·설계(`.docs/planning/`, 실측 근거) → Codex 적대 리뷰 → **확정 계획 보고·사용자 승인** → TDD 웨이브 → 시각 게이트(사용자 판정·PDF 대조) → 독립 리뷰 상환 → CI·merge queue → release-plz 릴리스 실측 검증 → 기록.

구현 중 상시 규칙: **TDD edge-first** · **atomic conventional commits** · **100% rustdoc** (`#![deny(missing_docs)]`) · **zero clippy warnings**.

---

## Releasing (release-plz 소유)

> **상세 절차·다이어그램·체크리스트**: `RELEASING.md` (canonical).

릴리스는 **release-plz** 가 소유한다 (`.github/workflows/release-plz.yml` + `release-plz.toml`). 규칙:

- **버전/태그를 손으로 만들지 말 것.** `cargo publish`·`git tag`·Cargo.toml 버전 수동 bump 금지. `v0.6.0` 및 per-crate 태그는 전부 release-plz 산출물 — 손대면 자기비교·중복 publish 사고.
- **흐름은 2단계**: feature PR 머지(버전 안 올림) → release-plz가 **Release PR** 생성/갱신(버전 bump + CHANGELOG) → 사람이 **Release PR 머지** → 그때서야 crates.io publish·태그·GitHub Release·npm·문서 배포가 일어남.
- **conventional commit 으로 릴리스가 결정**됨: `feat|fix|perf|refactor`(+ `type!:`)만 트리거. breaking 은 **반드시 `type!:` 또는 `BREAKING CHANGE:`** 로 표기(안 하면 0.x에서 patch로 오판). 0.x에서 breaking = **마이너** bump(0.6→0.7), **비파괴 feat/fix = 패치** bump(0.11.0→0.11.1 — "다음은 0.12.0" 오판 주의).
- **SemVer 검사는 release-plz가 소유** (`semver_check = true`). ci.yml 에 standalone cargo-semver-checks 게이트를 **다시 넣지 말 것** — feature PR은 버전을 안 올리는 모델이라 breaking PR마다 영원히 빨강이 됨 (이 이유로 PR #78에서 제거).
- **배포 대상**: crates.io = `hwpforge`(umbrella)·foundation·core·blueprint·smithy-hwpx·smithy-md·bindings-mcp. **제외**(`publish=false`) = smithy-hwp5·convert·bindings-cli·bindings-py. **umbrella 만 GitHub Release 생성** → npm(`@hwpforge/mcp`)·pages 배포가 거기 매달림.
- **다음 릴리스 주의**: ① **npm publish 복구됨 (2026-07-13, v0.11.0 전 패키지 배포)** — 실패 원인은 granular 토큰 90일 만료(npm 은 인증 실패를 **E404 로 위장**) + 재발급 토큰의 **"Bypass 2FA" 필수**(없으면 E403). ⚠️ 현 토큰 **~2026-10-10 재만료** — 영구 해결은 npm Trusted Publishing(OIDC) 전환(워크플로에 `id-token: write` 추가) · ② CHANGELOG 한글 표는 `dprint fmt CHANGELOG.md` 수동 후 재-stage · ③ 태그 기반 로컬 검증 전 `git fetch --tags`(stale 태그 → 거짓 통과 함정).
- **Merge queue 활성** — `gh pr merge --squash`(특히 `--delete-branch`) 거부됨(`Auto merge is not allowed` / `Cannot use --delete-branch when merge queue enabled`). 대신 GraphQL `enqueuePullRequest(input:{pullRequestId})` mutation 으로 큐에 넣을 것 (`mergeStateStatus=CLEAN` 이후에만 성공 — BLOCKED/UNSTABLE 중엔 대기). 큐가 PR당 CI 재실행 후 자동 머지(머지 방식은 큐 설정 소유 — `gh --squash` 무시되고 merge-commit). 큐 상태 = `repository.mergeQueue(branch:"main").entries`.
- **inter-crate 의존성은 `version = "0"` 유지 — 정확 핀으로 "고치지" 말 것.** 통합버전(`version.workspace = true`) 워크스페이스에서 release-plz 는 커밋 없는 베이스 crate 를 못 올려, 정확 핀이면 breaking bump 시 `failed to select a version` 으로 Release PR 생성이 죽음 (PR #94 로 해결; `version_group`·`release_always` 는 무효 — 로컬 전수검증, memory `release-plz-unified-version-workspace.md`).
- **release-plz 디버깅은 로컬 프리빌트로 재현** (CI 머지 사이클로 추측 금지): `gh release download release-plz-v0.3.159 --repo release-plz/release-plz` + 깨끗한 clone 에서 `release-plz update`. `{{ release_link }}` 는 로컬 렌더 실패 → 임시 제거 후 실험 (cargo install 은 rustc 1.94 요구로 로컬 1.93 에서 실패).
- **publish 검증**: crates.io API(`crates.io/api`) 는 샌드박스에서 막힐 수 있음 → sparse index `index.crates.io/hw/pf/<crate>`(`dangerouslyDisableSandbox`)로 published 버전 확인.
- **릴리스 완주 판정**: GitHub Release 는 release-plz 실행 **도중** 먼저 게시되고 그 이벤트가 npm-publish 를 트리거 → release-plz·npm-publish **둘 다 success** + npm 레지스트리(`npm view @hwpforge/mcp version`)·sparse index 실측까지 확인해야 완료 (Release 게시만 보고 완료 오판 금지).

---

## Gotchas & Common Mistakes

> **상세 내용 (코드 예제 포함)**: `.docs/references/gotchas.md` (40항목)

1. HWP5 TagID +16 오프셋 — `PARA_HEADER` = 0x42 (66), not 0x32 (50)
2. landscape 스펙 반전 — `WIDELY`=세로, `NARROWLY`=가로. width/height 교환 금지
3. 기하 좌표는 모두 `hc:` namespace (`hp:` 사용 시 한글 parse error)
4. TextBox = `hp:rect` + `hp:drawText` (control 요소 아님). 요소 순서/shapeComment 필수
5. Chart: manifest 등록 금지, `<c:f>` 필수, `<c:tx>`는 직접값만, `dropcapstyle="None"` 필수
6. paraPr 당 `Vec<HxSwitch>` (NOT `Option`) — 2개 이상 switch 가능
7. Equation: shape common 블록 없음 (`flowWithText="1"`, `outMargin` left/right=56)
8. colPr self-closing 태그 — `xml.find("<hp:colPr")` 로 매칭
9. Polygon 꼭짓점 닫힘 — 첫 꼭짓점을 마지막에 반복 필수
10. `breakNonLatinWord` = `KEEP_WORD` (BREAK_WORD 시 글자 퍼짐)
11. Field: 하이퍼링크=`fieldBegin/End`, 날짜=`type="SUMMERY"` (오타), 쪽번호=`autoNum`
12. 각주/미주: 같은 문단의 inline Run에 포함 (별도 문단 금지). HWP5 decode 시 ParaText `0x11` 마커(ctrl_id `fn`/`en`)를 `ControlRef`로 승격해야 inline 유지 — 안 하면 문단 꼬리로 drain되어 마커가 단독 줄에 표시
13. Style: 개요 8/9/10 paraPr 비순차(18/16/17), DropCapStyle은 PascalCase
14. ArrowType: `EMPTY_*` + `headfill` 조합만 (FILLED_* 무시됨)
15. MasterPage: prefix 없는 `<masterPage>` 루트, 15개 xmlns, `<hp:subList>`
16. schemars 1.x: `Cow<'static, str>` 반환. quick-xml 0.41: `decoder().decode()` 사용
17. `page_break`: `u32::from(para.page_break)` — hardcoded 0 금지
18. Flip은 `rotMatrix`에 인코딩 — scaMatrix/transMatrix는 identity 유지
19. `fillBrush`는 xs:choice — winBrush/gradation/imgBrush 중 하나만
20. Rotation: 정수 degrees + CCW 방향 + 중심 이동 보정 필수
21. PatternType `BACK_SLASH`/`SLASH` 스펙 반전 — Display/FromStr에서 스왑
22. 패턴 채우기: `hatchStyle` 속성 필수 (없으면 솔리드로 렌더링)
23. fieldid = ctrl_id ASCII magic constant (`%xrf`/`%clk`/`%smr`/`%pat`) — type tag, instance ID 아님
24. CROSSREF wire = 8-param Hancom-canonical (`Fiexde`/`Prop`/`Command` 포함, 5-param spec form 금지)
25. cross-ref target element (endNote/footNote/figure/table) 에 `instId` attribute 필수
26. Bookmark Contents reference 는 SpanStart/SpanEnd 책갈피 필요 (Point 는 본문 없음 → `?`)
27. ContentType 의미는 RefType-상대적 (Bookmark+Contents = 책갈피 이름, Figure+Contents = 캡션 본문) — invented enum 금지
28. 도형/글상자 텍스트 **세로정렬 = HWP5 ListHeader 속성 bits 5-6** (`(props>>5)&0x03`, 0/1/2=Top/Center/Bottom). 표 셀 디코드(`smithy-hwp5/src/decoder/section/mod.rs`)가 ground truth — openhwp `(props>>2)`는 우리 wire와 불일치
29. 도형 drawText `<hp:subList textWidth/textHeight>` = **`0`이 Hancom-정답** (렌더러는 `<hp:sz>`−`<hp:textMargin>`(기본 283)으로 텍스트 영역 계산). 계산값으로 "고치지" 말 것 — 한컴 fixture·KS X 6101 샘플 141 확인
30. 누름틀(ClickHere) 본문 = `fieldBegin`~`fieldEnd` 사이 평범한 `<hp:t>` (미채움 = 힌트와 동일 문자열). `display_text` 빈 문자열 = 미채움/모호 sentinel — patch 슬롯·redact·fill 이 전부 ClickHere-gated 로 이 불변식 공유. 한컴 재저장은 라벨 run 을 필드 run 에 병합 → run 에 `<hp:t>` 1개일 때만 본문 무모호 귀속 (HxRun 은 자식 순서 미보존)

---

## Key References

When implementing HWPX:

- openhwp/docs/hwpx/ (9,054 lines) — **KS X 6101 spec in markdown**
- No need to buy KS X 6101 standard document

When implementing HWP5:

- `.docs/research/ANALYSIS_hwpers.md` — Rust HWP5 patterns
- HWP_5_0_FORMAT_COMPLETE_GUIDE.md — 6 critical gotchas

When designing APIs:

- Follow foundation patterns (Newtype, Branded Index, ErrorCode)
- Separation: structure (Core) vs style (Blueprint)
