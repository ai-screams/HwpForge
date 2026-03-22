# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

---

## Project Overview

HwpForge is a Rust library for programmatic control of Korean HWP/HWPX document formats, designed with LLM-first principles. The goal is to enable AI agents (like Claude Code) to generate Korean government proposal documents using natural language + Markdown + YAML style templates.

**Current Status**:

- HWPX codec: read/write shipped
- Markdown bridge: read/write shipped
- HWP5 converter path: active (Phase 10 line)
- CLI bindings: shipped
- MCP bindings: shipped
- Python bindings: stub
- Shared tab semantics: landed on `main`
- Shared `ordered / bullet / outline` semantics: implemented on local `feat/list-shared-semantics`
- Checkable bullet semantics: implemented on local `feat/list-shared-semantics`
- HWP5 checkable support: definition-level parity only; paragraph item checked-state decode is still backlog
- Markdown task lists normalize to HWPX-first checkable semantics; ordered task lists intentionally lose numbering

**Workspace Facts (code-grounded)**:

- Cargo packages: `10`
- Workspace version: `0.4.0`
- Tracked Rust `src` files under `crates/`: `137`
- Tracked Rust `src` LOC under `crates/`: `83,962`
- Example artifact files under `examples/`: `47`
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

### 1. HWP5 TagID Offset

Section records have +16 offset from official spec: `PARA_HEADER` = 0x42 (66), not 0x32 (50).

### 2. HWPX landscape values (spec 반전!)

`WIDELY` = 세로(portrait), `NARROWLY` = 가로(landscape) — KS X 6101 스펙과 반대!
width/height는 항상 세로 기준 유지 (A4 = 210x297). **`PageSettings.landscape: bool` 사용**.

```rust
// ❌ width/height 교환 → 이중 회전 발생
// ✅ landscape: true, 치수는 세로 기준 유지
let landscape = PageSettings { landscape: true, ..PageSettings::a4() };
```

### 3. Geometry namespace: ALL use `hc:` (NOT `hp:`)

Line endpoints(`hc:startPt`), polygon vertices(`hc:pt`), textbox corners(`hc:pt0`~`hc:pt3`) 모두 `hc:` namespace. `hp:` 사용 시 한글 parse error 또는 "파일을 읽거나 저장하는데 오류".

### 4. TextBox = `hp:rect` + `hp:drawText` (NOT control element)

```xml
<hp:rect ...><hp:drawText>...</hp:drawText></hp:rect>
```

핵심 규칙:

1. Element order: shape-common → drawText → caption → hc:pt0-3 → sz → pos → outMargin → shapeComment
2. lastWidth = 전체 width (margin 차감 안 함), shadow alpha = 178
3. `<hp:shapeComment>사각형입니다.</hp:shapeComment>` 필수
4. Shape run 후 `<hp:t/>` marker 필수

### 5. Chart encoding rules

1. **No manifest**: Chart/*.xml은 ZIP에만 존재, content.hpf에 등록 금지 (한글 크래시)
2. **`<c:f>` 필수**: 더미 formula라도 포함 (`Sheet1!$A$2:$A$5`). 없으면 빈 차트.
3. **`<c:tx>` 직접값만**: `<c:tx><c:v>시리즈명</c:v></c:tx>` (strRef 사용 시 크래시)
4. **`dropcapstyle="None"` 필수**, `horzRelTo="COLUMN"` (PARA 아님)
5. **VHLC/VOHLC**: 4축 combo layout 필수 (각 chart type이 자체 catAx+valAx 쌍 보유, secondary catAx는 `delete="1"`)

### 6. Multiple switches per paraPr

`Vec<HxSwitch>` 사용 (NOT `Option<HxSwitch>`). 실제 한글 파일은 `<hh:paraPr>` 당 2개 이상 `<hp:switch>` 포함.

### 7. Equation: NO shape common block

도형과 달리 offset, orgSz, curSz, flip, rotation, lineShape, fillBrush, shadow가 없음.
`flowWithText="1"` (도형은 0), `outMargin` left/right=56 (도형은 0 또는 283).

### 8. Self-closing colPr

```rust
// ❌ xml.find("</hp:colPr>")  — self-closing 태그 누락
// ✅ xml.find("<hp:colPr")    — 양쪽 형태 모두 매칭
```

### 9. Polygon vertex closure

첫 꼭짓점을 마지막에 반복해야 path가 닫힘. 한글은 자동으로 닫지 않음.

### 10. breakNonLatinWord = KEEP_WORD

`BREAK_WORD` 사용 시 양쪽 정렬에서 글자 사이 공간이 균등 분배되어 퍼짐. `KEEP_WORD`(한글 기본값)가 자연스러움.

### 11. Field encoding patterns

- **하이퍼링크**: `fieldBegin type="HYPERLINK"` + `fieldEnd` pair (NOT `<hp:hyperlink>`)
- **날짜/시간/문서요약**: `type="SUMMERY"` (한글 내부 오타 14년간 유지), `Prop=8`, `fieldid=628321650`
- **CLICK_HERE**: `Prop=9`, `fieldid=627272811`, `editable="1"`
- **본문 쪽번호**: `<hp:autoNum numType="PAGE">` (NOT fieldBegin). 머리글/바닥글은 `<hp:pageNum>`.

### 12. 각주/미주: inline Run 필수

별도 문단으로 만들면 각주 번호가 단독 줄에 표시됨. 같은 문단의 Run에 포함해야 함.

```rust
Paragraph::with_runs(vec![
    Run::text("본문 텍스트.", cs),
    Run::control(Control::footnote(notes), cs),
], ps);
```

### 13. Style system gotchas

- **개요 8/9/10 paraPr**: Non-sequential (18/16/17, NOT 순차)
- **User paraShapes**: Modern style set에서 index 20부터 시작
- **DropCapStyle**: PascalCase (`DoubleLine`, NOT `DOUBLE_LINE`), 도형 속성 (문단 속성 아님)

### 14. ArrowType: EMPTY_ 형태만 사용

한글은 `FILLED_DIAMOND/CIRCLE/BOX`를 무시. `EMPTY_*` + `headfill="1"`로 채움 제어.

### 15. MasterPage XML

1. 루트: `<masterPage>` (prefix 없음, NOT `<hm:masterPage>`)
2. 15개 xmlns 전체 선언 필수
3. `<hp:subList>` 사용 (NOT `<hm:subList>`)

### 16. Dependency versions

- **schemars 1.x**: `schema_name()` → `Cow<'static, str>` (NOT `String`)
- **quick-xml 0.39**: `unescape()` 제거됨 → `decoder().decode()` 사용. `Event::GeneralRef` 처리 필수.

### 17. page_break encoding

`page_break: u32::from(para.page_break)` — hardcoded 0이 아닌 실제 필드값 사용.

### 18. Flip은 `rotMatrix`에 인코딩 — scaMatrix/transMatrix는 identity 유지

```xml
<!-- ❌ WRONG — scaMatrix에 flip 저장 → 드래그 잔영이 원본, 회전/대칭 메뉴 비활성화 -->
<hp:flip horizontal="1" vertical="0"/>
<hp:renderingInfo>
  <hc:transMatrix e1="1" e2="0" e3="{width}" e4="0" e5="1" e6="0"/>
  <hc:scaMatrix e1="-1" e2="0" e3="0" e4="0" e5="1" e6="0"/>
  <hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>
</hp:renderingInfo>

<!-- ✅ CORRECT — rotMatrix에 flip + 보정 이동, scaMatrix/transMatrix는 identity -->
<hp:flip horizontal="1" vertical="0"/>
<hp:renderingInfo>
  <hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>
  <hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>
  <hc:rotMatrix e1="-1" e2="0" e3="{width}" e4="0" e5="1" e6="0"/>
</hp:renderingInfo>
```

한글은 flip을 `rotMatrix`에서 읽음. `scaMatrix`에 넣으면 수학적으로 동일하지만:

- 드래그 시 잔영(ghost)이 원본(반전 전) 모양으로 표시됨
- 우클릭 메뉴의 회전/대칭 기능이 비활성화됨

### 19. fillBrush는 xs:choice — winBrush/gradation/imgBrush 중 하나만

```xml
<!-- ❌ WRONG — winBrush와 gradation 동시 출력 (xs:choice 위반) -->
<hc:fillBrush>
  <hc:winBrush faceColor="none" hatchColor="#000000" alpha="0"/>
  <hc:gradation type="LINEAR" angle="0" ...>
    <hc:color value="#FF0000"/><hc:color value="#0000FF"/>
  </hc:gradation>
</hc:fillBrush>

<!-- ✅ CORRECT — gradation만 (winBrush 없음) -->
<hc:fillBrush>
  <hc:gradation type="LINEAR" angle="0" centerX="0" centerY="0"
    step="255" colorNum="2" stepCenter="50" alpha="0">
    <hc:color value="#FF0000"/>
    <hc:color value="#0000FF"/>
  </hc:gradation>
</hc:fillBrush>
```

KS X 6101 스펙: "`<fillBrush>` 요소는 세 개의 하위 요소 중 **하나의 요소**를 가질 수 있다(choice)."
hwpxlib(Java)도 세 필드 모두 nullable. 도형(DrawingObject)과 borderFill이 동일한 `hc:FillBrushType` 사용.
`gradation` 필수 속성: type, angle, centerX, centerY, step, colorNum, stepCenter, alpha + `<hc:color>` 자식.

- **Horizontal flip**: `rotMatrix e1="-1"`, `e3=width`
- **Vertical flip**: `rotMatrix e5="-1"`, `e6=height`
- **Both**: 양쪽 모두 적용
- **Pipeline**: `point' = transMatrix × rotMatrix × scaMatrix × point`
- **검증**: `15_shapes_advanced.hwpx` Section 6 — 비대칭 깃발 도형으로 4방향 반전 확인
- **적용 대상**: 모든 도형 (Polygon, Ellipse, Line, Arc, Curve, ConnectLine, TextBox)

### 20. Rotation은 정수 degrees + CCW 방향 + 중심 이동 포함

```xml
<!-- ❌ WRONG — centidegrees, CW 방향, 이동 없음 → 도형이 원점 기준 회전 -->
<hp:rotationInfo angle="9000" centerX="3000" centerY="2000" rotateimage="1"/>
<hc:rotMatrix e1="0" e2="1" e3="0" e4="-1" e5="0" e6="0"/>

<!-- ✅ CORRECT — 정수 degrees, CCW 방향, 중심 기준 이동 포함 -->
<hp:rotationInfo angle="90" centerX="3000" centerY="2000" rotateimage="1"/>
<hc:rotMatrix e1="0" e2="-1" e3="5000" e4="1" e5="0" e6="-1000"/>
```

한글 회전 인코딩 규칙:

- **angle 단위**: 정수 degrees (NOT centidegrees). 90° = `angle="90"` (NOT `"9000"`)
- **rotMatrix 방향**: `[cos θ, -sin θ; sin θ, cos θ]` (CCW, 화면 좌표계에서 시계방향)
- **rotMatrix 이동**: 중심 기준 회전을 위한 보정 필수
  - `e3 = cx*(1-cos) + cy*sin`
  - `e6 = cy*(1-cos) - cx*sin`
  - `cx = width/2, cy = height/2`
- **이동 없으면**: 도형이 바운딩 박스 원점(0,0) 기준으로 회전 → 위치 이탈
- **scaMatrix, transMatrix**: 순수 회전 시 identity 유지

### 21. PatternType BACK_SLASH/SLASH 반전 (spec 반전!)

```rust
// ❌ WRONG — spec대로 매핑하면 한글에서 역사선(\)과 사선(/)이 반대로 렌더링됨
PatternType::BackSlash => "BACK_SLASH"  // 한글이 `/`로 렌더링
PatternType::Slash => "SLASH"           // 한글이 `\`로 렌더링

// ✅ CORRECT — 스왑하여 실제 렌더링과 일치
PatternType::BackSlash => "SLASH"       // 한글이 `\`로 렌더링 ✓
PatternType::Slash => "BACK_SLASH"      // 한글이 `/`로 렌더링 ✓
```

KS X 6101 XSD 문서에는 `BACK_SLASH = \\\\`, `SLASH = ////`이지만, 한글은 반대로 렌더링합니다.
landscape 반전(gotcha #2)과 동일한 패턴. `PatternType`의 `Display`/`FromStr`에서 스왑 처리됨.

### 22. 패턴 채우기는 winBrush + hatchStyle 필수

```xml
<!-- ❌ WRONG — hatchStyle 없으면 솔리드 채우기로 표시됨 -->
<hc:winBrush faceColor="#FFD700" hatchColor="#000000" alpha="0"/>

<!-- ✅ CORRECT — hatchStyle로 패턴 종류 지정 -->
<hc:winBrush faceColor="#FFD700" hatchColor="#000000" hatchStyle="HORIZONTAL" alpha="0"/>
```

패턴 채우기 시 `hatchStyle` 속성 필수. 없으면 한글이 솔리드 채우기로 렌더링.
유효값: `HORIZONTAL`, `VERTICAL`, `BACK_SLASH`, `SLASH`, `CROSS`, `CROSS_DIAGONAL`

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
