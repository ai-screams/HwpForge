# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

---

## Project Overview

HwpForge is a Rust library for programmatic control of Korean HWP/HWPX document formats, designed with LLM-first principles. The goal is to enable AI agents (like Claude Code) to generate Korean government proposal documents using natural language + Markdown + YAML style templates.

**Current Status**: Phase 0-5 + Wave 1-14 + Phase 6 CLI 완료
**Stats**: ~50,700 LOC, 1,592 tests (nextest), 103 .rs files, 9 crates, 92.65% coverage

---

## Architecture (Forge Metaphor)

The codebase follows a **blacksmith workshop** metaphor with clear separation of concerns:

```
Foundation (🔩 primitives)
  → Core (🔨 pure document structure, no style definitions)
  → Blueprint (📐 YAML style templates, centralized like Figma Design Tokens)
  → Smithy (🔥 format-specific compilers: HWPX, HWP5, Markdown)
  → Bindings (🐍⚒️ Python/CLI interfaces)
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
cargo build                                  # Build all crates
cargo test -p hwpforge-foundation            # Test specific crate
make test                                    # cargo-nextest (parallel)
make ci                                      # Full CI: fmt + clippy + test + deny
```

### Lint & Format

```bash
cargo clippy -p hwpforge-foundation -- -D warnings  # Specific crate
make clippy                                         # All crates
make fmt                                            # Check formatting
make fmt-fix                                        # Auto-fix
```

### Watch Mode

```bash
bacon         # Auto-run clippy on file changes
bacon test    # Auto-run tests
```

### Documentation & Coverage

```bash
make doc      # Generate rustdoc (opens in browser)
make cov      # Code coverage HTML report (llvm-cov)
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
bindings-py, bindings-cli (all smithy crates)
```

**Important**: Foundation is the root. If you modify foundation, ALL crates rebuild. Keep it minimal.

---

## Critical Design Patterns

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

Target: **95%+ coverage** per crate.

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

### Internal Docs (.docs/ - git excluded)

- `.docs/architecture/CRATE_ROLES.md` — Each crate's responsibility
- `.docs/architecture/TDD_GUIDELINES.md` — Edge-first TDD process
- `.docs/architecture/ADVANCED_TYPE_SYSTEM.md` — Type innovations
- `.docs/research/SYNTHESIS.md` — Analysis of 5 reference projects

### Plans (.docs/planning/)

- `ROADMAP.md` — 로드맵 SSoT (최신 Phase 상태)
- `phase1_core_detailed.md` ~ `phase4_smithy_hwpx_encoder_detailed.md` — Phase별 상세 계획
- `v1.0_decisions.md`, `v1.0_learnings.md` — 초기 의사결정/학습 기록
- `BACKLOG_SMITHY_MD.md` — Phase 5 백로그

### Reference Projects (.docs/references/ - git excluded)

- `openhwp/` (Rust) — Architecture inspiration
- `hwpxlib/` (Java) — Most mature HWPX implementation
- `hwpx-owpml-model/` (C++) — Official Hancom model
- `hwp.js/` (TypeScript) — HWP5 format gotchas
- `hwpers/` (Rust) — HWP5 Rust patterns

---

## Working on a New Phase

### Before Starting

1. Read `.docs/planning/phaseN_*_detailed.md` (if exists)
2. Read `crates/hwpforge-{crate}/AGENTS.md` (if exists)
3. Read `.docs/architecture/CRATE_ROLES.md` for role definition
4. Review completed phases for patterns (e.g., Phase 3 decoder architecture)

### During Implementation

1. **TDD**: Edge cases first
2. **Atomic commits**: One logical change per commit
3. **Documentation**: 100% rustdoc (enforced by `#![deny(missing_docs)]`)
4. **Zero warnings**: `cargo clippy -- -D warnings`

### After Implementation

1. Run `make ci` (fmt + clippy + test + deny)
2. Request Oracle review for 90+/100 score
3. Update Serena memory: `phase{N}_completion_summary`

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

### 18. Flip은 `<hp:flip>` 속성만으로 부족 — scaMatrix + transMatrix 필수

```xml
<!-- ❌ WRONG — flip 속성만 설정, 렌더링 행렬은 identity → 한글이 반전 무시 -->
<hp:flip horizontal="1" vertical="0"/>
<hp:renderingInfo>
  <hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>
  <hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>
</hp:renderingInfo>

<!-- ✅ CORRECT — flip 속성 + scaMatrix(반전) + transMatrix(보정 이동) -->
<hp:flip horizontal="1" vertical="0"/>
<hp:renderingInfo>
  <hc:transMatrix e1="1" e2="0" e3="{width}" e4="0" e5="1" e6="0"/>
  <hc:scaMatrix e1="-1" e2="0" e3="0" e4="0" e5="1" e6="0"/>
</hp:renderingInfo>
```

한글은 `<hp:flip>` 요소를 상태 표시로만 사용하고, 실제 렌더링은 `scaMatrix`로 수행합니다.

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

- **Horizontal flip**: `scaMatrix e1="-1"` + `transMatrix e3=width` (x축 반전 후 보정)
- **Vertical flip**: `scaMatrix e5="-1"` + `transMatrix e6=height` (y축 반전 후 보정)
- **Both**: 양쪽 모두 적용
- **Pipeline**: `point' = transMatrix × rotMatrix × scaMatrix × point`
- **검증**: `15_shapes_advanced.hwpx` Section 6 — 비대칭 깃발 도형으로 4방향 반전 확인
- **적용 대상**: 모든 도형 (Polygon, Ellipse, Line, Arc, Curve, ConnectLine, TextBox)

---

## Phase Status

### v1.0 (First Cycle: Core Pipeline)

| Phase         | Crate                                             | Status            | Tests | LOC    |
| ------------- | ------------------------------------------------- | ----------------- | ----- | ------ |
| 0             | foundation                                        | ✅ Done (90+/100) | 224   | 4,432  |
| 1             | core                                              | ✅ Done (94/100)  | 331   | 5,554  |
| 2             | blueprint                                         | ✅ Done (90/100)  | 200   | 4,647  |
| 3             | smithy-hwpx decoder                               | ✅ Done (96/100)  | 110   | 3,666  |
| 4             | smithy-hwpx encoder                               | ✅ Done (95/100)  | 226   | 10,349 |
| 4.1           | encoder improvements                              | ✅ Done           | —     | +104   |
| 4.2           | table 한글 호환                                   | ✅ Done           | —     | +198   |
| 5             | smithy-md                                         | ✅ Done (91/100)  | 73    | 3,757  |
| 4.5 Wave 1    | 이미지/머리글/바닥글/페이지번호                   | ✅ Done           | —     | —      |
| 4.5 Wave 2    | 각주/미주/글상자                                  | ✅ Done           | —     | —      |
| 4.5 Wave 3    | 다단/도형 (선/타원/다각형)                        | ✅ Done           | —     | —      |
| 4.5 Wave 4    | 캡션 (Caption on 6 shapes)                        | ✅ Done           | —     | —      |
| 4.5 Wave 5    | 수식 (Equation)                                   | ✅ Done           | —     | —      |
| 4.5 Wave 6    | 차트 (Chart)                                      | ✅ Done           | —     | —      |
| —             | Bug fix (colPr/polygon/chart_offset)              | ✅ Done           | —     | —      |
| —             | Linter setup (dprint + markdownlint)              | ✅ Done           | —     | —      |
| Style Phase F | breakNonLatinWord fix                             | ✅ Done           | —     | —      |
| Style Phase A | HancomStyleSet + default styles                   | ✅ Done           | —     | —      |
| 5.5           | Write API Zero-Config 편의 생성자                 | ✅ Done           | —     | —      |
| 5.5b          | Write API 100% Coverage                           | ✅ Done           | —     | —      |
| 5.5c          | Hyperlink encoding (fieldBegin/End)               | ✅ Done           | —     | —      |
| 5.5d          | Chart sub-variants + positioning + TOC            | ✅ Done           | —     | —      |
| Wave 7        | Style Infrastructure                              | ✅ Done           | —     | ~1,750 |
| Wave 8        | Paragraph Features (numbering/tabs/outline)       | ✅ Done           | —     | ~600   |
| Wave 9        | Page Layout Completion                            | ✅ Done           | —     | ~800   |
| Wave 10       | Character Enhancements (emphasis/charshape)       | ✅ Done           | —     | ~400   |
| Wave 11       | Shape Completions (Arc/Curve/ConnectLine)         | ✅ Done           | —     | ~600   |
| Wave 12       | References & Annotations                          | ✅ Done           | —     | ~500   |
| Wave 13       | Remaining Content (Dutmal/Compose)                | ✅ Done           | —     | ~400   |
| Wave 14       | Final Features (TextDirection/DropCap/page_break) | ✅ Done           | —     | ~200   |
| 6 (CLI)       | bindings-cli (AI-first CLI, 79 integration tests) | ✅ Done           | 80    | 1,035  |
| 6 (Python)    | bindings-py (PyO3)                                | 📋 Ready          | —     | —      |
| 7             | MCP integration                                   | 📋 Ready          | —     | —      |
| 8             | Testing + Release v1.0                            | 📋 Ready          | —     | —      |

**Totals**: ~50,700 LOC, 1,592 tests (nextest), 103 .rs files, 9 crates

### v2.0 (Second Cycle: Full Compatibility)

| Phase | Crate                                      | Status   |
| ----- | ------------------------------------------ | -------- |
| 9     | HWPX Full (OLE/양식컨트롤/변경추적/책갈피) | 📋 Ready |
| 10    | smithy-hwp5 (HWP5 읽기)                    | 📋 Ready |

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
