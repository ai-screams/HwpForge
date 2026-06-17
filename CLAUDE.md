# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

---

## Project Overview

HwpForge is a Rust library for programmatic control of Korean HWP/HWPX document formats, designed with LLM-first principles. The goal is to enable AI agents (like Claude Code) to generate Korean government proposal documents using natural language + Markdown + YAML style templates.

**Current Status** (snapshot — 2026-06-17):

- HWPX codec: read/write shipped · Markdown bridge: read/write shipped
- HWP5 → HWPX converter path: active, style/layout fidelity line in progress
- CLI bindings: shipped · MCP bindings: shipped · Python bindings: stub
- Shared `tab` / `ordered·bullet·outline` / checkable-bullet semantics wired through core → blueprint → smithy. HWP5 checkable carries all three gotcha-#8 truth locations (`bullet.checkedChar`, `bullet.paraHead.checkable`, `paraPr.checked`).
- Active branch `feat/phase12-hwp5-gso-shapes` (ahead of `main`): Phase 12 HWP5→HWPX carry series — GSO shapes, equation, memo, dutmal, compose, indexmark, click-here/auto fields, cross-ref instId, document metadata, outline levels 1–10 — plus 옵션 단위 enum 전수조사 (번호/쪽번호/이미지 채우기 형식 버그 수정).

> **이 섹션은 짧은 상태 스냅샷으로만 유지한다 (wave-by-wave 이력을 여기 다시 쌓지 말 것).**
> Wave별 상세 이력 + breaking change: **`CHANGELOG.md`** (canonical) 와 memory `MEMORY.md` / `phase11_wave_history.md`.
> Enum/wire 레이아웃 표 (번호·쪽번호·이미지채우기·대각선 등): **`crates/hwpforge-smithy-hwp5/HWP5_WIRE_SPEC.md`** (특히 §22).

**Still-deferred (Windows 한컴 fixture 대기)**:

- **non-chart OLE passthrough** — 전 구간 구현됐으나 standalone `<hp:ole>` 가 macOS 한컴 crash, macOS는 생성 자체 불가 → `git stash` 보존 (memory `non-chart-ole-deferred.md`)
- **masterPage carry** (Wave 5 gap C, task #33) · **쪽 테두리/배경 hatch _페이지_ 경로** (char/table hatch 는 byte-verified 완료, 공유 코드 — macOS [쪽] 메뉴에 항목 없음)
- **양식컨트롤(form controls)** — 완전 무음 드롭, `b"form"` + `HWPTAG_FORM_OBJECT(0x5B)` 미구현 (memory `form-controls-deferred.md`)
- **가운데 밑줄** (macOS 한글 밑줄 위치에 "가운데" 옵션 없음) · 한컴-authored multi-run span 디코딩 (편집 prerequisite, task #96)

**Workspace Facts** (code-grounded — 카운트는 drift하니 인용 전 확인):

- Cargo packages `10` · Workspace version `0.6.0` (Unreleased, ADR-002 + Wave 12 series breaking) · MSRV `1.88` · Dev toolchain Rust `1.93`
- `crates/` 추적 src 파일 ~`157` · nextest ~`2,489` passed + `2` skipped · `examples/` 산출물 `67`+ (gitignored `examples/hwp5_review/` 리뷰 영역 별도) · GitHub workflows `5`

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
- 용량 큰 이미지 임베드 fixture(~MB)는 gitignore된 `examples/hwp5_review/`에만 두고, 회귀 방지는 **단위 테스트로 잠금**(수 MB fixture를 커밋하지 말 것).

### Documentation & Coverage

```bash
make doc
make cov
```

---

## Crate Dependency Graph

```
foundation (NO dependencies except serde/thiserror)
    ↓
core (foundation only)
    ↓
blueprint (foundation + core)
    ↓
smithy-hwpx, smithy-hwp5, smithy-md (foundation + core + blueprint)
    ↓
bindings-py, bindings-cli, bindings-mcp (all smithy crates)
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
   - `tests/golden/hwpx/*.hwpx`
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
cargo test --test golden            # Golden tests only
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
- ❌ derive_more, strum (manual implementations = better error messages)

**Principle**: Add complexity only when proven necessary.

---

## Important Files & Directories

### `crates/`

Actual implementation lives here. Read `crates/AGENTS.md` and any crate-local `AGENTS.md` before changing a crate boundary.

### `examples/`

Generated artifacts and sample converters live here. `hwpx2md/images/` is a helper output directory for Markdown conversion artifacts.

### `tests/`

Root `tests/` is primarily a fixture warehouse. It is not itself the main Rust integration-test crate.

### `.docs/`

Local planning and research workspace. It may be git-excluded in this repository setup, so never assume "not in git status" means "does not exist".

### Reference docs

- `.docs/references/openhwp/docs/hwpx/` — local KS X 6101 markdownized reference
- `.docs/research/` — local research logs and workstream notes
- `.docs/architecture/` — crate-role and design notes when present

---

## Current Engineering State

- Phase 10 HWP5 line is active.
- Shared tab semantics already landed on `main`.
- Table integration gates are concentrated in `crates/hwpforge-bindings-cli/tests/cli_integration.rs`.
- Stress or real-world table fixtures are not the same thing as committed regression gates.
- On local `feat/list-shared-semantics`, shared `ordered / bullet / outline` semantics are wired through `core -> blueprint -> smithy-hwpx`, with Markdown bridge integration.
- Checkable bullet semantics are also wired on the local branch.
- HWP5 remains the partial leg for this slice:
  - bullet/checkable definition parity is present
  - paragraph-level checked item state is not fully decoded yet
- Do not confuse local branch completion with `main` branch state.

---

## Working on a New Slice

### Before Starting

1. Read root `AGENTS.md`.
2. Read `crates/AGENTS.md` and the target crate's local `AGENTS.md` if present.
3. Check code, manifests, and entrypoints before trusting roadmap prose.
4. If HWP5 reveals a new semantic, confirm the shared model can carry it before wiring format-specific code.
5. If the change may break public API or semver, stop and get approval first.

### During Implementation

1. **TDD**: Edge cases first
2. **Atomic commits**: One logical change per commit
3. **Documentation**: 100% rustdoc (enforced by `#![deny(missing_docs)]`)
4. **Zero warnings**: `cargo clippy -- -D warnings`

### After Implementation

1. Run `make ci-fast` (or stricter checks if the slice warrants it)
2. Re-check public API / semver impact before release-facing actions
3. Update local research or Serena memory only if it materially changes the working model

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
12. 각주/미주: 같은 문단의 inline Run에 포함 (별도 문단 금지)
13. Style: 개요 8/9/10 paraPr 비순차(18/16/17), DropCapStyle은 PascalCase
14. ArrowType: `EMPTY_*` + `headfill` 조합만 (FILLED_* 무시됨)
15. MasterPage: prefix 없는 `<masterPage>` 루트, 15개 xmlns, `<hp:subList>`
16. schemars 1.x: `Cow<'static, str>` 반환. quick-xml 0.39: `decoder().decode()` 사용
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
