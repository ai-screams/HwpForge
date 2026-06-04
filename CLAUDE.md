# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

---

## Project Overview

HwpForge is a Rust library for programmatic control of Korean HWP/HWPX document formats, designed with LLM-first principles. The goal is to enable AI agents (like Claude Code) to generate Korean government proposal documents using natural language + Markdown + YAML style templates.

**Current Status**:

- HWPX codec: read/write shipped
- Markdown bridge: read/write shipped
- HWP5 converter path: active with style/layout fidelity line in progress
- CLI bindings: shipped
- MCP bindings: shipped
- Python bindings: stub
- Shared tab semantics: landed on `main`
- Shared `ordered / bullet / outline` semantics: implemented on local `feat/list-shared-semantics`
- Checkable bullet semantics: implemented on local `feat/list-shared-semantics`
- HWP5 checkable support: all three gotcha-#8 truth locations now carry end-to-end — `bullet.checkedChar`, `bullet.paraHead.checkable` (definition-level), and `paraPr.checked` (paragraph-level)
- Markdown task lists normalize to HWPX-first checkable semantics; ordered task lists intentionally lose numbering
- HWP5 char/para style bridge now preserves the main supported style surface
- HWP5 layout hint patch injects `linesegarray` and safe table height hints for better visual parity
- `convert-hwp5` / `audit-hwp5` warning counts are aligned for style projection fallbacks
- HWP5/HWPX char effects now preserve `emboss`, `engrave`, `superscript`, and `subscript`
- HWP5/HWPX paragraph `breakLatinWord=HYPHENATION` is now carried end-to-end (Wave 1d)
- HWP5 chart objects (OLE-backed BinData) now carry end-to-end as `Control::EmbeddedChart` passthrough — emits `Chart/chartN.xml` + `BinData/oleN.ole` + `<hp:switch>` block (Wave 4c, closes `DroppedControl:ole_object`)
- HWP5 tab fidelity end-to-end: inline `<hp:tab width/leader/type>` attributes carried via new `RunContent::InlineText` variant (Wave 4 tab Phase 2+3, additive `#[non_exhaustive]`)
- HWP5 header/footer per-ctrl `applyPageType` (BOTH/ODD/EVEN) now carry as multiple `<hp:header>` / `<hp:footer>` elements (Wave 5 gap A; **breaking** ADR-002: `Section.header: Option<HeaderFooter>` → `Section.headers: Vec<HeaderFooter>`, footer 동일)
- HWP5 `secd` ctrl property bits (0/1/2/5/19) carry into `Section.visibility.hide_first_*` (Wave 5 gap B)
- **Phase 12 HWP5 carry series complete** (active branch `feat/phase12-hwp5-gso-shapes`, 21 commits ahead of `main`, 2026-06-02):
  - Wave 12a: GSO `Ellipse`/`Arc`/`Curve` carry (`0x50`/`0x53` sub-records, arc inferred from ellipse)
  - Wave 12b: GSO `ConnectLine` carry (`$col` discriminator on `ShapeComponentLine`)
  - Wave 12d: `Equation` carry (`eqed` + `HWPTAG_EQEDIT` script pairing)
  - Wave 12e/f/g/h: `Memo` carry — body content + anchor positioning + 7 wire parameters (`editable`/`dirty`/`zorder`/`field_id`/`begin_id_ref`/`name`/`meta_tag`)
  - Wave 12i: `Dutmal` (덧말) `option` carry + flat-path `control_iter` filter (Core breaking: `option_raw`)
  - Wave 12j: `Compose` (글자겹침) carry + `char_pr_ids` fidelity + packed-variant 지원 (Core breaking)
  - Wave 12k: `IndexMark` (찾아보기) carry + `0x16` 인라인 marker 보존
  - Wave 12l: `Control::Field` (CLICK_HERE / 누름틀) carry + form-mode `name` 메타 carry — `Control::Field`에 `name: Option<String>` 추가 (Core breaking), HWP5 `%clk` ctrl_id + 트레일링 `0x57` (`TagId::CtrlData`) sub-record 페어링, HWPX encoder `Clickhere:set:N:` 자기 참조 N 공식 empirical 도출 (이전 하드코딩 `43` 수정), 32K command + 2K name allocation cap (DoS 방어)
  - Wave 12o (latest, 2026-06-04): Document metadata end-to-end carry — `Core::Metadata`에 `description`/`last_saved_by`/`extras: BTreeMap` 추가 + `#[non_exhaustive]` (Core breaking, builder API `Metadata::new().with_*()`), HWPX encoder가 `content.hpf <opf:metadata>` 9-slot byte-parity 형식 emit, HWPX decoder가 `Contents/content.hpf` 파싱하고 `Document.metadata` 채움 (XXE/billion-laughs/depth-cap 방어), HWP5 decoder가 `\x05HwpSummaryInformation` OLE2 PropertySet 디코딩 (VT_LPSTR/VT_LPWSTR/VT_FILETIME + Hancom custom PID 0x14/0x15, FILETIME→ISO 8601 hand-rolled). 검증: `$title`/`$author`/`$createtime`/`$modifiedtime`/`$lastsaveby` 모두 한컴 저장 시 재평가되어 native 값 표시 확인 (G6/G7 PASS). **G2 (native byte-parity diff), G4 (HWP5→HWPX e2e 자동 게이트), G8b (저장 후 fallback 거동) 미평가 — 후속 wave에서 자동 게이트로 승격 예정 (Codex(architect) Wave 12o-fixup §Top-5 종료 정직성).**
  - Wave 12o-fixup (2026-06-04, Codex architect 리뷰 반영): (1) `summary_info.rs::parse_summary_information` `sec_start+8` unchecked add → `checked_add` + `bytes.get(..)` (32-bit/wasm DoS panic 차단, Top-1 P0), (2) HWPX encoder가 `extras["date"]` 를 9-slot 위치에서 그대로 carry (typed collision guard 에서 `date` 예외, Hancom recompute 가정 깨질 때 silent data loss 차단, Top-2 P0), (3) `Hwp5Decoder::decode` 가 `intermediate.metadata` → `document.set_metadata` forward (canonical entry API 일관성, Top-4 P1), (4) `decoder/metadata.rs::enforce_namespace` 가 unprefixed `<title>` 같은 prefix 누락 요소도 reject (S3 guard 실효화, P1 신규 발견).
- Known still-deferred: richer strike/underline line families; Wave 5 gap C (masterPage carry — macOS 한컴 fixture 비대칭, PC 한컴 fixture 대기, task #33); multi-section / page-border-fill (fixture 미작성); 한컴-authored .hwpx의 multi-run span 디코딩 (편집 알고리즘 prerequisite, task #96); Wave 12n leftover ($P$F PATH 필드, outline level off-by-1, linesegarray drop, editable bit propagation — tasks #120~#124, 한컴 '낮은 보안수준 복구' 경고의 보조 트리거)

**Workspace Facts (code-grounded, 2026-06-04 Wave 12o)**:

- Cargo packages: `10`
- Workspace version: `0.6.0` (Unreleased — bumped from `0.5.2` for ADR-002 + Wave 12 series + Wave 12o `#[non_exhaustive] Metadata` breaking)
- Tracked Rust `src` files under `crates/`: `147` (+1: `summary_info.rs`)
- Workspace nextest count: `2,444` (passed) + `2` skipped (+76 Wave 12o + 3 Wave 12o-fixup 회귀 게이트)
- Example artifact files under `examples/`: `67`+ (gitignored `examples/hwp5_review/` 리뷰 영역 별도)
- GitHub workflow files: `5`
- MSRV: `1.88`
- Dev toolchain: Rust `1.93`

Treat these as code-derived facts, not roadmap promises.

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

> **상세 내용 (코드 예제 포함)**: `.docs/references/gotchas.md` (35항목)

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
