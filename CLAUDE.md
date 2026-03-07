# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

---

## Project Overview

HwpForge is a Rust library for programmatic control of Korean HWP/HWPX document formats, designed with LLM-first principles. The goal is to enable AI agents (like Claude Code) to generate Korean government proposal documents using natural language + Markdown + YAML style templates.

**Current Status**: Phase 0-5 완료 + Phase 4.5 Wave 1-13 완료 + Wave 14 완료 + 버그 수정 + Wave 7 Style Infrastructure 완료 (foundation → core → blueprint → smithy-hwpx → smithy-md)
**Stats**: ~49,215 LOC, 1,494 tests (nextest), 92 .rs files, 9 crates at 90+/100 Oracle score

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
// ❌ WRONG
Color::from_raw(0xFF0000)  // This is BLUE in BGR!

// ✅ CORRECT
Color::from_rgb(255, 0, 0)  // This is red → 0x0000FF internally
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
// validated.save_hwpx(...);  // ✅ OK (Phase 3)
```

Compile-time state validation prevents invalid operations.

### 5. Two-Type Pattern (Blueprint)

```rust
// PartialCharShape: all fields Option (for YAML/inheritance merge)
let partial = PartialCharShape { font: Some("Batang".into()), size: Some(unit), ..Default::default() };

// CharShape: all fields required (after resolution)
let resolved: CharShape = partial.resolve("style_name")?;
// resolved.font is String, not Option<String>
```

Invalid states (missing font/size) are unrepresentable after resolution.

### 6. StyleRegistry Pipeline (Blueprint → Smithy)

```rust
let template = Template::from_yaml(yaml_str)?;      // Parse YAML
let resolved = resolve_template(&template, &provider)?; // Inherit + merge
let registry = StyleRegistry::from_template(&resolved)?; // Allocate indices

// Index-based access (branded types prevent mixing)
let entry = registry.get_style("body").unwrap();
let cs = registry.char_shape(entry.char_shape_id);   // CharShapeIndex
let font = registry.font(entry.font_id);             // FontIndex (deduplicated)
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

### 1. Color byte order

```rust
// HWP uses BGR, not RGB!
let red_bgr = 0x0000FF;  // ✅ Correct
let red_rgb = 0xFF0000;  // ❌ This is BLUE in HWP!
```

### 2. HWP5 TagID Offset

Section records have +16 offset from official spec:

- Spec: `PARA_HEADER = 0x32` (50)
- Reality: `PARA_HEADER = 0x42` (66)

See: `.docs/research/SPEC_VS_REALITY.md`

### 3. HWPX landscape values (⚠️ 반전 + 명시적 필드 필수)

Per validation spreadsheet: `landscape` attribute values are **reversed** in actual files vs spec.

- **한글 실제 동작**: `WIDELY` = 세로(portrait), `NARROWLY` = 가로(landscape)
- **KS X 6101 스펙**: `WIDELY` = 가로, `NARROWLY` = 세로 (반대!)
- **width/height는 항상 세로 기준 유지** (예: A4 = 210x297). 한글이 내부적으로 회전 처리.
- **절대 width > height 비교로 orientation 추론하지 말 것** — `PageSettings.landscape: bool` 사용.

```rust
// ❌ WRONG — width/height 교환으로 가로 설정 (이중 회전 발생)
let landscape = PageSettings {
    width: HwpUnit::from_mm(297.0).unwrap(),
    height: HwpUnit::from_mm(210.0).unwrap(),
    ..PageSettings::a4()
};

// ✅ CORRECT — landscape: true, 치수는 세로 기준 유지
let landscape = PageSettings {
    landscape: true,
    ..PageSettings::a4()
};
```

### 4. TextBox is NOT a control element in HWPX

```rust
// ❌ WRONG (HWPX는 control 요소가 아님)
Control::TextBox(...)

// ✅ CORRECT (HWPX는 hp:rect + hp:drawText 구조)
// <hp:rect ...><hp:drawText>...</hp:drawText></hp:rect>
```

TextBox는 `<hp:rect>` 도형 안에 `<hp:drawText>`를 내포한 구조입니다. Control element가 아닙니다.

### 5. Dependency hygiene

Foundation is the root. **Keep it minimal**. Phase 0 removed 3 unused deps during Oracle review.

### 7. Line shape namespace

```xml
<!-- hc: (core) namespace for geometry, NOT hp: (paragraph) -->
<hc:startPt x="0" y="0"/>  <!-- ✅ CORRECT -->
<hp:startPt x="0" y="0"/>  <!-- ❌ WRONG — 한글 parse error -->
```

### 8. Multiple switches per paraPr

Real 한글 files have 2+ `<hp:switch>` per `<hh:paraPr>` (e.g., one for heading, one for margin/lineSpacing).
Schema must use `Vec<HxSwitch>`, NOT `Option<HxSwitch>`.

### 9. Equation has NO shape common block

```xml
<!-- ❌ WRONG — equation은 shape common이 없음 -->
<hp:equation><hp:offset .../><hp:orgSz .../></hp:equation>

<!-- ✅ CORRECT — sz + pos + outMargin + script만 -->
<hp:equation><hp:sz .../><hp:pos .../><hp:outMargin .../><hp:script>...</hp:script></hp:equation>
```

Equation은 도형(Line/Ellipse/Polygon)과 달리 offset, orgSz, curSz, flip, rotation, lineShape, fillBrush, shadow가 없습니다.
`flowWithText="1"` (도형은 0), `outMargin` left/right=56 (도형은 0 또는 283).

### 10. Chart XML manifest 등록 금지

```xml
<!-- ❌ WRONG — 한글 크래시 유발 -->
<opf:item id="chart1" href="Chart/chart1.xml" media-type="application/xml"/>

<!-- ✅ CORRECT — Chart/*.xml은 ZIP에만 존재, content.hpf에 등록하지 않음 -->
```

### 11. Chart `<c:f>` formula 참조 필수

```xml
<!-- ❌ WRONG — 차트 열리지만 데이터 표시 안 됨 (빈 차트) -->
<c:cat><c:strRef><c:strCache>...</c:strCache></c:strRef></c:cat>

<!-- ✅ CORRECT — 더미 formula라도 반드시 포함 -->
<c:cat><c:strRef><c:f>Sheet1!$A$2:$A$5</c:f><c:strCache>...</c:strCache></c:strRef></c:cat>
```

한글은 `<c:f>` 존재 여부를 cache 데이터 읽기의 전제조건으로 사용합니다.

### 12. Chart `<c:tx>` series 이름은 직접값만

```xml
<!-- ❌ WRONG — 한글 크래시 -->
<c:tx><c:strRef><c:strCache>...</c:strCache></c:strRef></c:tx>

<!-- ✅ CORRECT -->
<c:tx><c:v>시리즈명</c:v></c:tx>
```

### 13. Chart `<hp:chart>` dropcapstyle 필수

`dropcapstyle="None"` 속성이 없으면 한글 크래시. `horzRelTo="COLUMN"` (PARA 아님).

### 14. Polygon vertex namespace

```xml
<!-- hc: (core) namespace for polygon vertices, NOT hp: (paragraph) -->
<hc:pt x="0" y="0"/>  <!-- ✅ CORRECT (KS X 6101: type="hc:PointType") -->
<hp:pt x="0" y="0"/>  <!-- ❌ WRONG — 한글 "파일을 읽거나 저장하는데 오류" -->
```

Same pattern as line shape (gotcha #7). All geometry elements use `hc:` namespace.

### 15. Self-closing colPr and ctrl element ordering

```xml
<!-- secPr 내 ctrl 요소 순서: secPr → colPr → header → footer → pageNum -->
<!-- ❌ WRONG — search for </hp:colPr> misses self-closing tags -->
xml.find("</hp:colPr>")

<!-- ✅ CORRECT — matches both <hp:colPr .../> and <hp:colPr>...</hp:colPr> -->
xml.find("<hp:colPr")
```

`build_col_pr_xml`은 self-closing `<hp:colPr ... />`를 생성하므로 `</hp:colPr>` 검색은 실패합니다.

### 16. TextBox (hp:rect) encoding 주의사항

TextBox는 `<hp:rect>` + `<hp:drawText>` 구조입니다. 6가지 핵심 규칙:

1. **Corner points는 `hc:` namespace**: `<hc:pt0>` ~ `<hc:pt3>` (NOT `hp:pt0`)
2. **Element order**: shape-common → drawText → caption → hc:pt0-3 → sz → pos → outMargin → shapeComment
3. **lastWidth = 전체 width**: margin 차감하지 않음
4. **Shadow alpha = 178**: 기본값 0이 아님
5. **shapeComment 필수**: `<hp:shapeComment>사각형입니다.</hp:shapeComment>`
6. **Shape run 후 `<hp:t/>` marker 필수**: 모든 shape 포함 run에 빈 `<hp:t/>` 추가

Golden fixture 검증 완료: `tests/fixtures/textbox.hwpx` (decode + roundtrip)

### 17. Polygon vertex closure (첫 꼭짓점 반복)

```xml
<!-- ❌ WRONG — 한글은 자동으로 path를 닫지 않음 (삼각형이 2변만 표시) -->
<hc:pt x="0" y="100"/><hc:pt x="50" y="0"/><hc:pt x="100" y="100"/>

<!-- ✅ CORRECT — 첫 꼭짓점을 마지막에 반복하여 닫기 -->
<hc:pt x="0" y="100"/><hc:pt x="50" y="0"/><hc:pt x="100" y="100"/><hc:pt x="0" y="100"/>
```

### 18. schemars 1.x: schema_name() return type

```rust
// ❌ WRONG (schemars 0.8 API)
fn schema_name() -> String { "MyType".to_owned() }

// ✅ CORRECT (schemars 1.x API)
fn schema_name() -> Cow<'static, str> { Cow::Borrowed("MyType") }
```

schemars 1.x changed `schema_name()` to return `Cow<'static, str>` instead of `String`.

### 19. quick-xml 0.39: unescape() removed

```rust
// ❌ WRONG (quick-xml 0.36 API — removed in 0.39)
let text = event.unescape()?;

// ✅ CORRECT (quick-xml 0.39)
let text = reader.decoder().decode(event.as_ref())?;
```

Also: `Event::GeneralRef` variant was added in 0.39 and must be handled in match arms that exhaustively cover `Event`.

### 20. breakNonLatinWord must be KEEP_WORD (양쪽 정렬 글자 퍼짐 원인)

- Location: `crates/hwpforge-smithy-hwpx/src/encoder/header.rs` `build_para_pr()`
- `BREAK_WORD` causes character-level space distribution in justified text → unnatural wide spacing
- `KEEP_WORD` (한글 default) preserves word boundaries → natural spacing
- `HwpxParaShape` has `break_latin_word` and `break_non_latin_word` fields (`WordBreakType` enum)

```rust
// ❌ WRONG (was in encoder)
break_non_latin_word: "BREAK_WORD"  // 양쪽 정렬 시 글자 사이 공간 균등 분배 → 퍼짐

// ✅ CORRECT (한글 기본값)
break_non_latin_word: "KEEP_WORD"  // 단어 단위 공간 분배 → 자연스러움
```

### 21. HWPX 하이퍼링크는 fieldBegin/fieldEnd 패턴 (NOT `<hp:hyperlink>`)

```xml
<!-- ❌ WRONG — 이런 요소 없음 -->
<hp:hyperlink href="...">...</hp:hyperlink>

<!-- ✅ CORRECT — KS X 6101 field pair -->
<hp:run charPrIDRef="0">
  <hp:ctrl>
    <hp:fieldBegin type="HYPERLINK" fieldid="0">
      <hp:parameters cnt="4">
        <hp:stringParam name="Path">https://url.com</hp:stringParam>
        <hp:stringParam name="Category">HWPHYPERLINK_TYPE_URL</hp:stringParam>
        <hp:stringParam name="TargetType">HWPHYPERLINK_TARGET_DOCUMENT_DONTCARE</hp:stringParam>
        <hp:stringParam name="DocOpenType">HWPHYPERLINK_JUMP_NEWTAB</hp:stringParam>
      </hp:parameters>
    </hp:fieldBegin>
  </hp:ctrl>
  <hp:t>링크 텍스트</hp:t>
  <hp:ctrl><hp:fieldEnd beginIDRef="0" fieldid="0"/></hp:ctrl>
</hp:run>
```

### 22. 각주/미주는 같은 문단에 인라인 Run으로 삽입해야 함

```rust
// ❌ WRONG — 별도 문단으로 만들면 각주 번호가 단독 줄에 표시됨
paras.push(p("본문 텍스트."));
paras.push(ctrl_para(Control::footnote(notes), CS_NORMAL, PS_JUSTIFY)); // 별도 줄에 "1)"

// ✅ CORRECT — 같은 문단의 Run에 포함
paras.push(Paragraph::with_runs(
    vec![
        Run::text("본문 텍스트.", CharShapeIndex::new(0)),
        Run::control(Control::footnote(notes), CharShapeIndex::new(0)),
    ],
    ParaShapeIndex::new(0),
));
```

### 23. VHLC/VOHLC 주식 차트는 4축 combo layout 필수

```xml
<!-- ❌ WRONG — 3축 layout (barChart+stockChart가 catAx 공유) → 한글 렌더링 깨짐 -->
<c:barChart>...<c:axId val="1"/><c:axId val="3"/></c:barChart>
<c:stockChart>...<c:axId val="1"/><c:axId val="2"/></c:stockChart>
<c:catAx><c:axId val="1"/>...</c:catAx>  <!-- 공유 catAx -->
<c:valAx><c:axId val="2"/>...</c:valAx>  <!-- price -->
<c:valAx><c:axId val="3"/>...</c:valAx>  <!-- volume -->

<!-- ✅ CORRECT — OOXML 표준 4축 combo layout -->
<c:barChart>...<c:axId val="3"/><c:axId val="4"/></c:barChart>  <!-- secondary axes -->
<c:stockChart>...<c:axId val="1"/><c:axId val="2"/></c:stockChart>  <!-- primary axes -->
<c:catAx><c:axId val="1"/><c:crossAx val="2"/>...</c:catAx>  <!-- primary cat (bottom) -->
<c:valAx><c:axId val="2"/><c:crossAx val="1"/>...</c:valAx>  <!-- primary val (left, price) -->
<c:catAx><c:axId val="3"/><c:crossAx val="4"/><c:delete val="1"/>...</c:catAx>  <!-- secondary cat (hidden) -->
<c:valAx><c:axId val="4"/><c:crossAx val="3"/><c:crosses val="max"/>...</c:valAx>  <!-- secondary val (right, volume) -->
```

각 차트 타입은 자체 축 쌍(catAx+valAx)을 가져야 합니다. secondary catAx는 `delete="1"`로 숨깁니다.

### 24. 개요 8/9/10 paraPr index is NON-SEQUENTIAL (Modern style set)

Modern(22) style set에서 개요 8/9/10의 paraPr 인덱스는 순차적이지 않습니다:

- 개요 8 (style ID 9) → paraPr group 18
- 개요 9 (style ID 10) → paraPr group 16
- 개요 10 (style ID 11) → paraPr group 17

User paraShapes start at index 20 in Modern (after the 20 default paraShapes).
Golden fixture `tests/fixtures/textbox.hwpx` verified.

### 25. ArrowType 기하 도형은 반드시 EMPTY_ 형태 사용 (headfill/tailfill로 채움 제어)

```xml
<!-- ❌ WRONG — 한글이 FILLED_* 를 인식하지 않음 (화살촉 안 보임) -->
<hp:lineShape headStyle="FILLED_DIAMOND" headfill="1" .../>

<!-- ✅ CORRECT — EMPTY_* + headfill="1" = 채워진 다이아몬드 -->
<hp:lineShape headStyle="EMPTY_DIAMOND" headfill="1" .../>

<!-- ✅ CORRECT — EMPTY_* + headfill="0" = 빈 다이아몬드 -->
<hp:lineShape headStyle="EMPTY_DIAMOND" headfill="0" .../>
```

KS X 6101 스키마에는 `FILLED_DIAMOND`, `FILLED_CIRCLE`, `FILLED_BOX`가 유효한 값으로 정의되어 있지만,
실제 한글은 `EMPTY_*` 형태만 인식하고 `headfill`/`tailfill` 속성(0 or 1)으로 채움 여부를 결정합니다.

- **검증**: `SimpleLine.hwpx` 참조 — `headStyle="EMPTY_BOX"` + `headfill="1"` = 채워진 사각형
- **적용 대상**: Diamond(`EMPTY_DIAMOND`), Circle(`EMPTY_CIRCLE`), Box(`EMPTY_BOX`)
- **비기하 도형은 그대로**: `NORMAL`, `ARROW`, `SPEAR`, `CONCAVE_ARROW` — fill 속성 무관

### 26. MasterPage XML은 namespace prefix 없는 루트 + 전체 xmlns 선언 필수

```xml
<!-- ❌ WRONG — 한글 크래시 (응용 프로그램이 예기치 않게 종료) -->
<hm:masterPage xmlns:hp="..." xmlns:hm="...">
  <hm:subList>...</hm:subList>
</hm:masterPage>

<!-- ✅ CORRECT — prefix 없는 루트 + 15개 xmlns 전체 선언 + hp:subList -->
<masterPage xmlns="http://www.hancom.co.kr/hwpml/2011/master"
            xmlns:hp="..." xmlns:hh="..." xmlns:hc="..." ...>
  <hp:subList id="" textDirection="HORIZONTAL" ...>
    ...
  </hp:subList>
</masterPage>
```

3가지 핵심 규칙:

1. **루트 요소**: `<masterPage>` (prefix 없음, `<hm:masterPage>` 아님)
2. **xmlns 선언**: header/section과 동일한 15개 namespace 전부 선언 필수
3. **subList prefix**: `<hp:subList>` 사용 (`<hm:subList>` 아님)

### 27. 날짜/시간/문서요약 필드는 type="SUMMERY" (NOT "DATE"/"TIME")

```xml
<!-- ❌ WRONG — 한글이 인식하지 않음 (아무것도 표시되지 않음) -->
<hp:fieldBegin type="DATE" ...>

<!-- ✅ CORRECT — 한글 내부 "Summary" 오타 14년간 유지 -->
<hp:fieldBegin type="SUMMERY" fieldid="628321650" ...>
  <hp:parameters cnt="3" name="">
    <hp:integerParam name="Prop">8</hp:integerParam>
    <hp:stringParam name="Command">$modifiedtime</hp:stringParam>
    <hp:stringParam name="Property">$modifiedtime</hp:stringParam>
  </hp:parameters>
</hp:fieldBegin>
```

Command 매핑: `$modifiedtime`=날짜, `$createtime`=시간, `$author`=작성자, `$lastsaveby`=최종수정자.
CLICK_HERE와의 차이: `Prop=8` (CLICK_HERE는 9), `fieldid=628321650` (CLICK_HERE는 627272811).

### 28. 본문 쪽번호는 `<hp:autoNum>` (NOT fieldBegin/fieldEnd)

```xml
<!-- ❌ WRONG — PAGE_NUM은 유효한 fieldBegin 타입이 아님 -->
<hp:fieldBegin type="PAGE_NUM" ...>

<!-- ✅ CORRECT — autoNum 메커니즘 사용 -->
<hp:ctrl>
  <hp:autoNum num="1" numType="PAGE">
    <hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar="" supscript="0"/>
  </hp:autoNum>
</hp:ctrl>
```

3가지 쪽번호 메커니즘 혼동 주의:

- `<hp:pageNum>`: secPr 내 ctrl (머리글/바닥글 자동 배치)
- `<hp:autoNum numType="PAGE">`: 본문 텍스트 인라인 삽입
- ~~`type="PAGE_NUM"` fieldBegin~~: 존재하지 않음

### 29. page_break는 paragraph 속성으로 직접 인코딩

```rust
// ❌ WRONG (was hardcoded)
page_break: 0,

// ✅ CORRECT
page_break: u32::from(para.page_break),
```

encoder/section.rs의 build_paragraph()에서 pageBreak 속성을 para.page_break 필드에서 읽어야 합니다.

### 30. DropCapStyle은 PascalCase (shape-level attribute)

```xml
<!-- ❌ WRONG — SCREAMING_SNAKE_CASE -->
dropcapstyle="DOUBLE_LINE"

<!-- ✅ CORRECT — PascalCase per KS X 6101 XSD -->
dropcapstyle="DoubleLine"
```

DropCapStyle은 문단 속성이 아닌 도형(AbstractShapeObjectType)의 속성입니다.
Valid values: None, DoubleLine, TripleLine, Margin

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
| 6 (CLI)       | bindings-cli (AI-first CLI)                       | ✅ Done           | 28    | 1,021  |
| 6 (Python)    | bindings-py (PyO3)                                | 📋 Ready          | —     | —      |
| 7             | MCP integration                                   | 📋 Ready          | —     | —      |
| 8             | Testing + Release v1.0                            | 📋 Ready          | —     | —      |

**Totals (Phase 0-5 + 4.5 Wave 1-14 + Wave 7-14 + Phase 6 CLI)**: ~50,581 LOC, 1,541 tests (nextest), 103 .rs files, 9 crates

### v2.0 (Second Cycle: Full Compatibility)

| Phase | Crate                                      | Status   |
| ----- | ------------------------------------------ | -------- |
| 9     | HWPX Full (OLE/양식컨트롤/변경추적/책갈피) | 📋 Ready |
| 10    | smithy-hwp5 (HWP5 읽기)                    | 📋 Ready |

### Phase 3+4: smithy-hwpx (HWPX Full Codec)

**Status**: ✅ Complete (95-96/100)
**Scope**: T1 full + T2 full + T3/T4 partial (text, tables, images, headers/footers, footnotes/endnotes, textboxes)
**Encoder LOC Breakdown**: header.rs (1,092) + section.rs (2,239) + mod.rs (499) + package.rs (476) + chart.rs (471) = 4,777 LOC
**Total smithy-hwpx LOC**: ~15,823 src (updated after Wave 7)

Key achievements:

- Pure serde approach (zero manual XML parsing)
- ZIP bomb defense (50MB/500MB/10k limits)
- Full codec: decode + encode (Core ↔ HWPX ZIP+XML)
- 246 tests (110 decoder + 136 encoder/spike), 5 golden decode + 8 golden roundtrip with real HWPX files
- Dual serde rename for namespace prefixes (hh:, hp:, hc:, hs:)
- xmlns template wrapping (hand-craft root, serialize inner content)
- Styles roundtrip (hh:styles, char shapes, para shapes)
- Empty paragraph normalization for lossless roundtrip
- Complete table encoding (14 attrs + sub-elements, 한글 호환 확인)
- Zero unsafe code, 100% rustdoc coverage

**Architecture**:

- `decoder/` — HWPX → Core (package → header → section)
- `encoder/` — Core → HWPX (header → section → package)
- `schema/` — XML types (Hx* prefix, private, serde-based)
- `style_store` — HWPX-specific style storage (fonts, char shapes, para shapes, styles)
- `error` — HwpxError (4000-4099 range)

### Phase 5: smithy-md (MD Full Codec)

**Status**: ✅ Complete (91/100)
**Scope**: GFM decoder + lossy/lossless encoder + YAML frontmatter
**LOC**: 3,757 src + 475 test, 9 files

Key achievements:

- Full MD ↔ Core codec (pulldown-cmark GFM parser)
- YAML frontmatter (title, author, date → Metadata)
- Section markers: `<!-- hwpforge:section -->` splits sections
- MdDocument → Document + StyleRegistry
- HwpxStyleStore::from_registry() bridge (Blueprint → HWPX)
- Full pipeline verified: MD → Core → HWPX → 한글에서 정상 열림
- 73 tests (58 unit + 10 E2E + 5 golden roundtrip)

### Phase 4.5: HWPX Write API 완성

**Wave 1-6 완료 (2026-02-17-19)**: 이미지 바이너리, 머리글/바닥글, 페이지 번호, 각주/미주, 글상자, 다단, 도형 (선/타원/다각형), 캡션, 수식, 차트

**Wave 1 achievements**:

- ImageStore: 바이너리 데이터 또는 파일 경로 지원
- BinData/ ZIP 삽입 with manifest.xml 업데이트
- Section.header/footer: 머리글/바닥글 컨테이너
- PageNumber with autoNum 자동 번호 매기기
- Schema: HxHeader, HxFooter, HxPageNum, HxImage 확장

**Wave 2 achievements**:

- Footnote/Endnote: inst_id 기반 참조 시스템
- TextBox: offsets (left/top/right/bottom) 위치 제어
- **Critical pattern discovered**: 글상자는 `<hp:rect>` + `<hp:drawText>` 구조 (NOT control element)
- Schema: HxFootNote, HxEndNote, HxRect, HxDrawText, HxPoint

**Wave 3 achievements**:

- Multi-column layout: colCount, sameSz, type, per-column width/gap (다단)
- Shape encoding/decoding: line, ellipse, polygon support (도형)
- Core: Section column settings, Shape types added
- Schema: HxLine, HxEllipse, HxPolygon, extended HxColPr
- 12 code review findings addressed (commit 94355da)

**Wave 4 achievements**:

- Caption support for 6 shape types: Table, Image, TextBox, Line, Ellipse, Polygon
- Core: caption.rs with Caption + CaptionSide types
- HwpxFont::new() constructor added to style_store
- Showcase example: examples/showcase.rs demonstrating all 13 APIs

**Line Shape Fix (2026-02-19)**:

- Schema: `hc:` namespace for startPt/endPt, field order fix, HxShapeComment
- Header: `switches: Vec<HxSwitch>` (multiple switches per paraPr)
- Decoder: `decode_shape_style()` DRY helper for line/ellipse/polygon
- Golden: 3 tests + line.hwpx fixture, line_styles.rs example (10 variations)

**Wave 5 achievements (2026-02-19)**: 수식 (Equation)

- Core: `Control::Equation` variant with script/width/height/baseLine/textColor/font
- Schema: `HxEquation` + `HxScript` structs (NO shape common block)
- Decoder: `decode_equation()` — sz/pos/outMargin/script extraction
- Encoder: `encode_equation_to_hx()` — hardcoded constants (flowWithText=1, outMargin=56)
- Validation: `EmptyEquation` error (code 2012)
- Golden: `decode_equations` + `roundtrip_equations` with equations.hwpx fixture
- Example: `equation_styles.rs` — 12 categories of equations
- HancomEQN script format (NOT MathML): `{a+b} over {c+d}`, `root {2} of {x}`

**Wave 6 achievements (2026-02-19)**: 차트 (Chart)

- Core: `chart.rs` — ChartType(18), ChartData(Category/Xy), ChartSeries, XySeries, convenience constructors
- Core: `Control::Chart` variant with chart_type, data, title, legend, grouping, width, height
- Core: `EmptyChartData` validation error (code 2013)
- Schema: `HxRunSwitch` + `HxRunCase` + `HxChart` — switch/case wrapper (NOT direct element)
- Encoder: `encoder/chart.rs` — OOXML chart XML generation (18 types, write!() templates)
- Decoder: `decoder/chart.rs` — quick-xml Reader OOXML parsing
- Package: Chart/chartN.xml in ZIP (NO manifest registration — crashes 한글)
- Pie/Doughnut: varyColors=1, holeSize, firstSliceAng; 3D: view3D block
- Golden: 12-chart fixture decode + roundtrip tests
- Example: `chart_styles.rs` — 9 chart types demo
- 71-variant full compatibility deferred to backlog

**Wave 6 plan** (detailed in `.docs/planning/ROADMAP.md`):

- Wave 6: 차트 — ✅ DONE (18종 ChartType, OOXML 인코드/디코드)

**Bug Fix Session (2026-02-19)**: colPr/polygon/chart_offset 수정

- Encoder `find_ctrl_injection_point`: self-closing `<hp:colPr .../>` 대응 (`</hp:colPr>` → `<hp:colPr` 검색)
- Schema: polygon vertex namespace 수정 (`hp:pt` → `hc:pt`, KS X 6101 PointType)
- Encoder: `chart_offset` parameter 추가 (multi-section 문서에서 차트 인덱스 충돌 방지)
- Encoder: chart `text_wrap` 수정 (`SQUARE` → `TOP_AND_BOTTOM`)
- TextBox 미검증 확인 → 예제에서 제외 (별도 디버깅 세션 예정)
- Polygon vertex closure: 첫 꼭짓점 마지막 반복 필수
- `full_report.rs` 완전 재작성: HWPX 포맷 분석 보고서 (4 섹션, TextBox 제외 전 기능 활용)
- `feature_isolation.rs` polygon 다양화: 삼각형/마름모/오각형/화살표 (4종)

**Wave 7 achievements (2026-03-04)**: Style Infrastructure

- Foundation: `Distribute = 4`, `DistributeFlush = 5` added to `Alignment` enum
- Core: `Paragraph.style_id: Option<StyleIndex>` with `with_style()` builder
- Encoder/Decoder: dynamic `styleIDRef` (was hardcoded to 0)
- Schema: `HxBorderFill` + 7 structs; dynamic serde replaces BORDER_FILLS_XML constant
- Per-style: 7 default charShapes + 20 default paraShapes from golden fixture
- Non-sequential paraPr: 개요 8/9/10 use paraPr 18/16/17 (NOT sequential)
- Phase D: `HancomStyleSet::style_id_for_name()`, MD H1-H6 → 개요 1-6 mapping
- Example: `wave7_style_test.rs` — roundtrip verification for all features

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
