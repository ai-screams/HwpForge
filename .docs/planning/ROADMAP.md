# HwpForge — Development Roadmap

> **Status**: Phase 4.5 Wave 1-6 완료 + Bug Fix + Chart 71종 완전 호환 + TOC/Positioning + Wave 7-14 완료 + Test Coverage Boost (92.65%)
> **Updated**: 2026-03-06
> **Source of truth**: 이 문서가 최신 로드맵의 SSoT

---

## 전체 로드맵 요약

```
v1.0 (First Cycle — 핵심 파이프라인 완성)

Phase 0: Foundation (HwpUnit, Color, ID)                    ✅ 완료 (2026-02)
Phase 1: Core (Document, Paragraph, Table, Image)           ✅ 완료 (2026-02)
Phase 2: Blueprint (YAML templates, StyleRegistry)          ✅ 완료 (2026-02)
Phase 3: Smithy-HWPX Decoder (HWPX → Core)                 ✅ 완료 (2026-02)
Phase 4: Smithy-HWPX Encoder (Core → HWPX)                 ✅ 완료 (2026-02)
  └─ Phase 4.1: Encoder 6가지 개선 (deep research 반영)    ✅ 완료 (2026-02-17)
  └─ Phase 4.2: Table 완전 호환 (한글 정상 오픈)           ✅ 완료 (2026-02-17)
Phase 5: Smithy-MD (Markdown ↔ Core)                        ✅ 완료 (2026-02-17)
Phase 4.5: HWPX Write API 완성                              ✅ 완료
  └─ Wave 1: 이미지/머리글/바닥글/페이지번호               ✅ 완료 (2026-02-17)
  └─ Wave 2: 각주/미주/글상자                              ✅ 완료 (2026-02-17)
  └─ Wave 3: 다단/도형 (선/타원/다각형)                    ✅ 완료 (2026-02-18)
  └─ Wave 4: 캡션                                          ✅ 완료 (2026-02-18)
  └─ Wave 5: 수식                                          ✅ 완료 (2026-02-19)
  └─ Wave 6: 차트                                          ✅ 완료 (2026-02-19)
  └─ Bug Fix: colPr/polygon/chart_offset                   ✅ 완료 (2026-02-19)
  └─ Bug Fix: chart_offset multi-section / lineseg overlap  ✅ 완료 (2026-02-27)
  └─ Chart 71종 완전 호환 (9 sub-options + VHLC 4축)         ✅ 완료 (2026-02-28)
  └─ Line/Polygon 절대 위치 (horz_offset/vert_offset)        ✅ 완료 (2026-02-28)
  └─ TOC titleMark (Paragraph.heading_level)                 ✅ 완료 (2026-02-28)
  └─ Bug Fix: VHLC 4축 combo layout (3축→4축)               ✅ 완료 (2026-02-28)
Phase 5.5: Write API Zero-Config 편의 생성자                  ✅ 완료 (2026-02-27)
  └─ T1: Control 편의 생성자 (textbox/ellipse/line/polygon/equation/footnote/endnote/image)
  └─ T2: Section/HeaderFooter/PageNumber 편의 생성자
  └─ T3: Image format 자동 추론
  └─ T4: Builder 패턴 (선택적)
  └─ T5: full_report.rs 리팩토링
  └─ 상세: .docs/planning/BACKLOG_WRITE_API.md
Wave 7-13: HWPX Write API 완전 구현                           ✅ 완료
  └─ Wave 7:  Style Infrastructure (~1,500 LOC)              ✅ 완료 (2026-03-04)
  └─ Wave 8:  Paragraph Features (~600 LOC)                  ✅ 완료 (2026-03-05)
  └─ Wave 9:  Page Layout Completion (~800 LOC)              ✅ 완료 (2026-03-06)
  └─ Wave 10: Character Enhancements (~400 LOC)              ✅ 완료 (2026-03-05)
  └─ Wave 11: Shape Completions (~600 LOC)                   ✅ 완료 (2026-03-06)
  └─ Wave 12: References & Annotations (~500 LOC)            ✅ 완료 (2026-03-06)
  └─ Wave 13: Remaining Content (~400 LOC)                   ✅ 완료 (2026-03-05)
  └─ Wave 14: Final Features (~200 LOC)                      ✅ 완료 (2026-03-06)
  └─ Test Coverage Boost: 85.91% → 92.65% (~270 tests)      ✅ 완료 (2026-03-06)
  └─ 상세: .docs/planning/ROADMAP_WRITE_API_COMPLETION.md
  └─ 미구현 (v2.0 이동): 11.4 TextArt, 11.5 Container/Group, 11.7 FillBrush 스키마 확장
Phase 6: Bindings (Python + CLI)                             📋 예정
Phase 7: MCP Integration                                    📋 예정
Phase 8: Testing + Release v1.0                              📋 예정

v2.0 (Second Cycle — 레거시 호환)

Phase 9:  HWPX Full (OLE/양식/변경추적/TextArt/Group)      📋 예정
Phase 10: HWP5 Reader (레거시 → Core DOM)                   📋 예정
```

---

## 현재 상태 스냅샷 (2026-03-06)

| 지표 | 수치 |
|------|------|
| 총 소스 LOC | ~49,215 |
| 총 테스트 수 | 1,494 |
| 크레이트 수 | 8 |
| 소스 파일 수 | 92 |
| Golden 테스트 파일 | 13 (HWPX 8 + MD 5) |
| unsafe 코드 | 0 |
| clippy 경고 | 0 |
| Chart 변형 지원 | 71/71 (100%) |
| Wave 7-14 | 8/8 완료 (100%) |
| 테스트 커버리지 | 92.65% (CI threshold 90%) |

---

## Phase 상세

### Phase 0: Foundation ✅ (90+/100)

| 항목 | 값 |
|------|-----|
| 크레이트 | `hwpforge-foundation` |
| 테스트 | 185 |
| LOC | 4,610 |
| 핵심 | HwpUnit (정수 기반), Color (BGR), Index<T> (브랜드 인덱스), ErrorCode |

### Phase 1: Core ✅ (94/100)

| 항목 | 값 |
|------|-----|
| 크레이트 | `hwpforge-core` |
| 테스트 | 291 |
| LOC | 8,421 src |
| 핵심 | Document, Section, Paragraph, Table, Image, Hyperlink, Metadata |
| 패턴 | Typestate (Draft → Validated), 스타일 참조만 (ID only) |

### Phase 2: Blueprint ✅ (90/100)

| 항목 | 값 |
|------|-----|
| 크레이트 | `hwpforge-blueprint` |
| 테스트 | 191 |
| LOC | 4,647 src + 538 test |
| 핵심 | YAML Template, PartialCharShape → CharShape (Two-Type), StyleRegistry |
| 패턴 | Figma Design Token 컨셉, 상속/머지, 인덱스 할당 |

### Phase 3: Smithy-HWPX Decoder ✅ (96/100)

| 항목 | 값 |
|------|-----|
| 크레이트 | `hwpforge-smithy-hwpx` (decoder/) |
| 테스트 | 110 |
| LOC | 3,757 (section 1754 + header 652 + chart 590 + package 405 + mod 356) |
| 핵심 | HWPX ZIP → XML → Schema → Core, 5 golden decode tests |
| 패턴 | Pure serde, ZIP bomb 방어 (50MB/500MB/10k) |

### Phase 4: Smithy-HWPX Encoder ✅ (95/100)

| 항목 | 값 |
|------|-----|
| 크레이트 | `hwpforge-smithy-hwpx` (encoder/) |
| 테스트 | 248 (encoder + golden roundtrip) |
| LOC | 5,648 (section 2534 + header 1092 + chart 869 + mod 676 + package 477) |
| Schema LOC | 1,376 (header 806 + section 558 + mod 11) |
| Golden 테스트 | 8 roundtrip (실제 한글 파일) |
| 핵심 | Core → Schema → XML → ZIP, dual serde rename, xmlns template wrapping |

**Phase 4.1 개선사항** (2026-02-17):
- `charPr`: ratio/spacing/relSz/offset 요소 (7개 언어별 기본값: 100/0/100/0)
- `paraPr`: heading/breakSetting/autoSpacing/border 요소 + 기본값
- `borderFill id=2`: fillBrush/winBrush + Crooked/isCounter 속성
- `tabProperties`: 2nd entry (id=1, autoTabLeft=1)
- `numberings`: 1 numbering + 7 paraHead 레벨 (한국어 개요 형식)
- `section colPr`: 단일 컬럼 NEWSPAPER 레이아웃

**Phase 4.2 개선사항** (2026-02-17):
- `<hp:tbl>`: 14개 속성 + sz/pos/outMargin/inMargin 서브 요소
- `<hp:tc>`: header/hasMargin/protect/editable/dirty/borderFillIDRef + cellAddr/cellMargin
- `<hp:subList>`: 10개 속성 (textDirection, lineWrap, vertAlign 등)
- `borderFill id=3`: SOLID 0.12mm 테두리 (표 셀용)
- 한글에서 테이블 포함 문서 정상 열림 확인

### Phase 5: Smithy-MD ✅ (91/100)

| 항목 | 값 |
|------|-----|
| 크레이트 | `hwpforge-smithy-md` |
| 테스트 | 73 (56 unit + 10 E2E + 5 golden) |
| LOC | 3,836 src + 483 test |
| 핵심 | GFM decoder (pulldown-cmark), lossy/lossless encoder, YAML frontmatter |

지원 요소:
- YAML frontmatter (title, author, date → Metadata)
- Heading H1-H6 → Core Heading paragraphs
- Bold, Italic, Strikethrough, Code → Core TextRun attributes
- Ordered/Unordered lists (중첩 포함) → Core List items
- Blockquote → Core indented paragraphs
- Code block → Core monospace paragraphs
- Table (GFM) → Core Table
- Hyperlink → Core Hyperlink
- Horizontal rule → Core page break
- Section markers: `<!-- hwpforge:section -->` → 다중 섹션 분리
- MdDocument → Document + StyleRegistry
- HwpxStyleStore::from_registry() 브릿지

**Full Pipeline 확인**: MD → Core → HWPX → 한글에서 정상 열림 ✅

---

## Phase 4.5: HWPX Write API 완성 ✅ 완료 (Wave 1-6 + Bug Fix)

### 배경

Phase 4까지 구현된 HWPX Write는 T1 전체와 T2-T3의 일부만 커버합니다.
LLM 에이전트가 공문서/제안서를 생성하려면 이미지, 머리글/바닥글, 페이지 번호, 캡션, 수식, 차트가 필수입니다.
Bindings(Phase 6)를 만들기 전에 Write API를 완성하여 API 안정성을 확보합니다.

### 현재 HWPX Write 기능 매트릭스

#### T1: 텍스트 기본 — ✅ 100% 완료

| 요소 | 상태 | 비고 |
|------|------|------|
| 문단 (`<hp:p>`) | ✅ | 일반, 제목 H1-H6 |
| 텍스트 런 (`<hp:run>` + `<hp:t>`) | ✅ | Bold, Italic, Strikethrough, Underline |
| 글자 모양 (`charProperties`) | ✅ | 7개 언어별 ratio/spacing/relSz/offset |
| 문단 모양 (`paraProperties`) | ✅ | heading/breakSetting/autoSpacing/border |
| 스타일 (`styles`) | ✅ | YAML template → StyleRegistry → HWPX |
| 폰트 (`fontfaces`) | ✅ | 한글, 영문, 한자 등 7개 언어 |
| 페이지 설정 (`secPr`) | ✅ | 용지 크기, 여백, 컬럼 레이아웃 |

#### T2: 구조 요소 — ✅ 100% 완료

| 요소 | 상태 | 완료 시점 |
|------|------|----------|
| 표 (`<hp:tbl>`) | ✅ | Phase 4.2 |
| 번호 매기기 (`numberings`) | ✅ | Phase 4.1 |
| 글머리 기호 (`bullets`) | ✅ | 기본 불릿 |
| 목록 (ordered/unordered) | ✅ | 중첩 목록 포함 |
| 머리글 (`<hp:header>`) | ✅ | Phase 4.5 Wave 1 |
| 바닥글 (`<hp:footer>`) | ✅ | Phase 4.5 Wave 1 |
| 페이지 번호 (`<hp:autoNum>`) | ✅ | Phase 4.5 Wave 1 |
| 각주 (`<hp:footNote>`) | ✅ | Phase 4.5 Wave 2 |
| 미주 (`<hp:endNote>`) | ✅ | Phase 4.5 Wave 2 |
| 페이지 나누기 | ✅ | 수평선 → 구역 분리 |

#### T3: 미디어/객체 — ✅ 95% 완료

| 요소 | 상태 | 완료 시점 / 계획 |
|------|------|-----------------|
| 하이퍼링크 (fieldBegin/fieldEnd) | ✅ | Phase 4 |
| 이미지 경로 참조 | ✅ | Phase 4 |
| 이미지 바이너리 삽입 | ✅ | Phase 4.5 Wave 1 |
| 글상자 (`<hp:rect>` + drawText) | ✅ | Phase 4.5 Wave 2 |
| **캡션** (`<hp:caption>`) | ✅ | Phase 4.5 Wave 4 |
| **선/타원/다각형** (도형) | ✅ | Phase 4.5 Wave 3 |
| **호/곡선/연결선** (도형) | ✅ | Wave 11 (2026-03-06) |
| **책갈피** (`<hp:bookmark>`) | ✅ | Wave 12 (2026-03-06) |
| OLE 객체 | ❌ | v2.0 |

#### T4: 고급 기능 — ✅ 대부분 완료

| 요소 | 상태 | 비고 |
|------|------|------|
| **다단** (복수 컬럼) | ✅ | Phase 4.5 Wave 3 |
| **수식** (`<hp:equation>`) | ✅ | Phase 4.5 Wave 5 (2026-02-19) |
| **차트** (`<hp:chart>`) | ✅ | Phase 4.5 Wave 6 (2026-02-19) |
| **쪽 테두리/배경** (pageBorderFill) | ✅ | Wave 9 (2026-03-06) |
| **바탕쪽** (MasterPage) | ✅ | Wave 9 (2026-03-06) |
| **줄 번호** (lineNumberShape) | ✅ | Wave 9 (2026-03-06) |
| **메모/주석** (Memo) | ✅ | Wave 12 (2026-03-06) |
| **상호참조** (CrossRef) | ✅ | Wave 12 (2026-03-06) |
| **덧말/글자겹치기** (Dutmal/Compose) | ✅ | Wave 13 (2026-03-05) |
| 양식 컨트롤 | ❌ | v2.0 Phase 9 |
| 변경 추적 | ❌ | v2.0 Phase 9 |

### Phase 4.5 세부 구현 계획

```
완료:
  Phase 4.5a: 이미지 바이너리 삽입              ✅ Wave 1 (2026-02-17)
  Phase 4.5b: 머리글/바닥글                     ✅ Wave 1 (2026-02-17)
  Phase 4.5c: 페이지 번호 (autoNum)             ✅ Wave 1 (2026-02-17)
  Phase 4.5d: 각주/미주                         ✅ Wave 2 (2026-02-17)
  Phase 4.5e: 글상자 (TextBox → rect+drawText)  ✅ Wave 2 (2026-02-17)

  Phase 4.5f: 다단 (복수 컬럼)                  ✅ Wave 3 (2026-02-18)
  Phase 4.5g: 도형 (선/타원/다각형)              ✅ Wave 3 (2026-02-18)

  Phase 4.5h: 캡션                              ✅ Wave 4 (2026-02-18)

  Phase 4.5i: 수식                              ✅ Wave 5 (2026-02-19)

완료:
  Phase 4.5j: 차트                              ✅ Wave 6 (2026-02-19)
  Bug Fix: colPr/polygon/chart_offset            ✅ (2026-02-19)
```

#### 완료된 Wave 상세

**Wave 1 (2026-02-17)**: 이미지 바이너리 + 머리글/바닥글 + 페이지 번호
- Core: ImageStore (바이너리/파일경로), Section.header/footer/page_number
- Schema: HxHeader, HxFooter, HxPageNum, HxImage 확장
- Encoder: BinData/ ZIP 삽입, manifest.xml 업데이트
- 커밋: `e70331c`

**Wave 2 (2026-02-17)**: 각주/미주 + 글상자
- Core: Control::Endnote 추가, Footnote에 inst_id, TextBox에 offsets
- Schema: HxFootNote, HxEndNote, HxRect, HxDrawText, HxPoint
- 핵심 발견: 글상자는 ctrl이 아닌 `<hp:rect>` + `<hp:drawText>` 구조
- 커밋: `ee815b5`

**Wave 3 (2026-02-18)**: 다단 + 도형
- Core: Section column settings (column_count, column_gap), Shape enum (Line, Ellipse, Polygon)
- Schema: HxLine, HxEllipse, HxPolygon, extended HxColPr with multi-column support
- 12 code review findings addressed (commit 94355da)
- 커밋: `eded748`, `94355da`

**Wave 4 (2026-02-18)**: 캡션
- Caption support for all 6 shape types: Table, Image, TextBox, Line, Ellipse, Polygon
- Core: `caption.rs` with `Caption` + `CaptionSide` types
- Schema: `HxCaption` with SubList reuse pattern
- Encoder/Decoder: `build_hx_caption` / `convert_hx_caption` helpers
- MD: lossy/lossless caption encoding
- Showcase: `examples/showcase.rs` exercising all 13 APIs
- `HwpxFont::new()` constructor for API usability
- Code review fixes: M1 (error propagation), M2 (safe i32→u32 cast)

**Line Shape Fix (2026-02-19)**: Line 도형 한글 호환성 수정
- Schema: `hc:startPt`/`hc:endPt` namespace 수정 (was `hp:`)
- Schema: 필드 순서 수정 (geometry before sizing)
- Schema: `switches: Vec<HxSwitch>` (real 한글 has multiple `<hp:switch>` per paraPr)
- Schema: `HxShapeComment` 타입 추가
- Decoder: `decode_shape_style()` 공유 헬퍼 (line/ellipse/polygon DRY)
- Golden: 3 new tests + line.hwpx fixture
- Example: `line_styles.rs` — 10가지 선 스타일 쇼케이스
- 커밋: `49d1f03`

**Wave 5 (2026-02-19)**: 수식 (Equation)
- Core: `Control::Equation` variant (script, width, height, base_line, text_color, font)
- Core: `is_equation()` helper, `EmptyEquation` validation error (code 2012)
- Schema: `HxEquation` (13 attrs + 5 children), `HxScript` ($text serde capture)
- Decoder: `decode_equation()` — no depth param, no shape common block
- Encoder: `encode_equation_to_hx()` — NO `build_shape_common()` call
  - flowWithText=1, outMargin left/right=56, version="Equation Version 60"
- Golden: `decode_equations` + `roundtrip_equations` + equations.hwpx fixture
- Example: `equation_styles.rs` — 12 categories of HancomEQN equations (분수, 루트, 적분, 행렬, 색상 등)
- 핵심 발견: HancomEQN 자체 스크립트 포맷 (NOT MathML)
- 커밋: `23fb0a3`

**Wave 6 (2026-02-19)**: 차트 (Chart)
- Core: `chart.rs` — ChartType(18종), ChartData(Category/Xy), ChartSeries, XySeries, convenience constructors
- Core: `Control::Chart` variant, `EmptyChartData` validation error (code 2013)
- Schema: `HxRunSwitch` + `HxRunCase` + `HxChart` — switch/case wrapper (NOT direct element)
- Encoder: `encoder/chart.rs` — OOXML chart XML 생성 (18 ChartType, pie/doughnut/3D 특수화)
- Decoder: `decoder/chart.rs` — OOXML chart XML 파싱 (quick-xml Reader)
- Package: Chart/chartN.xml ZIP 직접 삽입 (content.hpf manifest 등록 시 한글 크래시)
- Golden: 12-chart fixture decode + roundtrip tests
- Example: `chart_styles.rs` — 9종 차트 데모
- 커밋: `e086dc0`

**Bug Fix Session (2026-02-19)**: colPr/polygon/chart_offset 수정
- `find_ctrl_injection_point`: self-closing `<hp:colPr .../>` 대응 (`</hp:colPr>` → `<hp:colPr` 검색)
- Polygon namespace: `hp:pt` → `hc:pt` (KS X 6101: `type="hc:PointType"`)
- `chart_offset` parameter: multi-section 문서 차트 인덱스 충돌 방지
- Chart `text_wrap`: `SQUARE` → `TOP_AND_BOTTOM`
- TextBox 검증 완료 (한글 golden fixture 기반)
- Polygon vertex closure: 첫 꼭짓점 마지막 반복 필수
- `full_report.rs` 완전 재작성: HWPX 포맷 분석 보고서 (4 섹션)
- `feature_isolation.rs` polygon 다양화 (4종)
- 커밋: `f9a935c`

**HWPX Write API 완성 (2026-02-28)**: 3개 잔여 기능 + VHLC 버그 수정
- **Line/Polygon 절대 위치**: `horz_offset: i32, vert_offset: i32` → `hp:pos` + `treatAsChar="0"`
- **Chart 71종 완전 호환**: 9개 sub-option 필드 (bar_shape, explosion, of_pie_type, radar_style, wireframe, bubble_3d, scatter_style, show_markers, stock_variant)
  - 새 enum: `StockVariant(Hlc/Ohlc/Vhlc/Vohlc)`, `BarShape`, `ScatterStyle`, `RadarStyle`, `OfPieType`
  - Stock VHLC/VOHLC: barChart + stockChart 복합 plotArea
  - Series overflow guard: max 25 category / 13 XY series
- **TOC titleMark**: `Paragraph.heading_level: Option<u8>` (1-7) → `<hp:titleMark>` 자동 주입
- **VHLC 4축 combo layout 버그 수정**:
  - 3축 (barChart+stockChart가 catAx 공유) → 한글 렌더링 깨짐 (수평선+괄호만 표시)
  - 4축 (각 차트 독립 축 쌍) → 정상 렌더링 ✅
  - stockChart: catAx(1) + valAx(2, left, 가격), barChart: catAx(3, hidden) + valAx(4, right, 거래량)
- 커밋: `3201fb4`

#### 4.5f: 다단 (복수 컬럼)

**현재**: 단일 컬럼 NEWSPAPER 레이아웃만 (Phase 4.1)
**목표**: 2단, 3단 등 복수 컬럼 레이아웃 지원

구현 범위:
- `secPr > colPr` 확장: colCount, sameSz, type 속성
- 각 컬럼별 width/gap 지정
- Core Section에 column_count/column_gap 필드 추가

#### 4.5g: 도형 (선/타원/다각형)

**현재**: rect만 지원 (글상자용)
**목표**: 선(`<hp:line>`), 타원(`<hp:ellipse>`), 다각형(`<hp:polygon>`) 추가

구현 범위:
- HxRect 패턴 확장 (AbstractDrawingObjectType 공통 구조)
- HxLine, HxEllipse, HxPolygon 스키마 타입
- Core에 DrawingObject enum 또는 Shape 타입 추가
- lineShape, fillBrush 공통 스타일링

#### 4.5h: 캡션

**현재**: 미구현
**목표**: 이미지/표/글상자에 캡션 텍스트 부착

구현 범위:
- `<hp:caption>` 요소 (pic/tbl/rect 하위)
- side 속성 (TOP, BOTTOM, LEFT, RIGHT)
- 내부 subList + 문단 (기존 패턴 재활용)
- Core Image/Table에 caption 필드 추가

#### 4.5i: 수식 ✅ 완료 (2026-02-19)

**구현 완료**: HancomEQN 수식 전체 파이프라인

구현 결과:
- `<hp:equation>` 요소 파싱/생성 (NO shape common block)
- HancomEQN 자체 스크립트 포맷 (`{a+b} over {c+d}`, `root {2} of {x}`, `sum`, `int`, `matrix`)
- Core: `Control::Equation` (script, width, height, base_line, text_color, font)
- Validation: `EmptyEquation` (code 2012), dimension checks
- Golden: 2 tests (decode + roundtrip) + equations.hwpx fixture (18 equations)
- Example: `equation_styles.rs` — 50+ equations across 12 categories

#### 4.5j: 차트 ✅ 완료 (2026-02-19)

**구현 완료**: OOXML 차트 전체 파이프라인 (18종 ChartType)

구현 결과:
- `<hp:chart>` 요소: switch/case wrapper 구조 (NOT direct element)
- 차트 데이터 모델: ChartData(Category/Xy), ChartSeries, XySeries, convenience constructors
- 차트 타입: 18종 ChartType (bar, column, line, pie, doughnut, area, scatter, bubble, radar + 3D + stacked variants)
- Core: `Control::Chart` variant, `EmptyChartData` validation error (code 2013)
- Encoder: `encoder/chart.rs` — OOXML chart XML generation (pie/doughnut/3D specialization)
- Decoder: `decoder/chart.rs` — OOXML chart XML parsing (quick-xml Reader)
- Package: Chart/chartN.xml ZIP 직접 삽입 (content.hpf 등록 시 한글 크래시)
- Golden: 12-chart fixture decode + roundtrip tests
- Example: `chart_styles.rs` — 9종 차트 데모

### Phase 4.5 의존 관계

```
완료:
  4.5a (이미지) ✅ ──────── 독립
  4.5b (머리글/바닥글) ✅ ── 독립
  4.5c (페이지 번호) ✅ ──── 4.5b 선행
  4.5d (각주/미주) ✅ ────── 독립
  4.5e (글상자) ✅ ────────── 독립

  4.5f (다단) ✅ ──────────── 독립
  4.5g (도형) ✅ ──────────── 4.5e 패턴 확장

  4.5h (캡션) ✅ ──────────── 독립 (subList 패턴 재활용)

완료:
  4.5i (수식) ✅ ──────────── 완료 (2026-02-19)
  4.5j (차트) ✅ ──────────── 완료 (2026-02-19)

전체 Phase 4.5 완료: Wave 1-6 + Bug Fix (2026-02-19)
```

---

## Wave 8-13: HWPX Write API 완전 구현 ✅ 완료

### Wave 8: Paragraph Features ✅ (2026-03-05)

- Core: `NumberingIndex`, `TabIndex` 브랜드 인덱스 타입
- Core: `NumberingDef` (10 레벨 동적 번호 매기기), `TabDef` (3 entry 탭 속성)
- Header encoder: `<hh:numberings>`, `<hh:tabProperties>` 동적 직렬화
- Schema: `HwpxParaShape` heading 속성 (headingType, headingLevel, headingIdRef)
- Encoder: outline paraPr 수정 (개요 레벨별 올바른 paraPr 참조)
- ~600 LOC

### Wave 9: Page Layout Completion ✅ (2026-03-06)

- Core: `Gutter` (type/width), `Visibility` (header/footer/pageNum/border 숨김), `LineNumberShape`, `PageBorderFillEntry`, `BeginNum`, `MasterPage`
- Encoder/Decoder: `<hp:pageBorderFill>`, `<hp:visibility>`, `<lineNumberShape>`, `<masterPage>`, `<beginNum>` 요소
- `mirror_margins` 는 HWPX pagePr에 대응 속성 없음 (lossy — 항상 false로 decode)
- `landscape` 속성 = 페이지 방향 (**NARROWLY=가로, WIDELY=세로** — 스펙과 반대!), mirror_margins 아님
- `PageSettings.landscape: bool` 명시적 필드 추가 — width/height 비교로 추론 금지
- MasterPage: prefix 없는 `<masterPage>` 루트 + 15개 xmlns 전체 선언 필수
- ~800 LOC

### Wave 10: Character Enhancements ✅ (2026-03-05)

- Foundation: `EmphasisType` enum (13종, `#[non_exhaustive]`)
- Core: CharShape 7개 필드 확장 (emphasis, ratio, spacing, rel_sz, offset, use_kerning, use_font_space)
- Encoder/Decoder: charPr emphasis/ratio/spacing/relSz/offset 동적 값 반영
- ~400 LOC

### Wave 11: Shape Completions ✅ (2026-03-06)

- Core: `Shape::Arc`, `Shape::Curve`, `Shape::ConnectLine` 변형
- Core: `ShapeStyle`에 `rotation`, `flip`, `head_arrow`/`tail_arrow` 필드 추가
- Core: `Fill` enum (Solid/Gradient/Pattern/Image), `CurveSegmentType` enum
- Schema: `HxArc`, `HxCurve`, `HxConnectLine` 구조체
- Encoder/Decoder: `<hp:arc>`, `<hp:curve>`, `<hp:connectLine>` 요소
- 핵심: ArrowType 기하 도형은 EMPTY_ 형태만 사용 (headfill/tailfill로 채움 제어)
- **미구현 (v2.0 이동)**: 11.4 TextArt, 11.5 Container/Group, 11.7 FillBrush 스키마 확장
- ~600 LOC

### Wave 12: References & Annotations ✅ (2026-03-06)

- Core: `Control::Bookmark`, `Control::CrossRef`, `Control::Field`, `Control::Memo`, `Control::IndexMark`
- Field 하위 타입: CLICK_HERE (누름틀), SUMMERY (요약), autoNum (자동 번호)
- Encoder: fieldBegin/fieldEnd 패턴 활용 (hyperlink과 동일 구조)
- SUMMERY 필드 인코딩 수정, autoNum 페이지 번호 지원
- ~500 LOC

### Wave 13: Remaining Content ✅ (2026-03-05)

- Core: `Control::Dutmal` (덧말/윗주) + `Control::Compose` (글자 겹치기)
- Foundation: `DutmalPosition`, `DutmalAlign` enum
- Schema: `HxDutmal`, `HxCompose`
- Encoder/Decoder: `<hp:dutmal>`, `<hp:compose>` 요소
- MD lossless: data-position, data-align 속성 보존
- ~400 LOC

### Wave 14: Final Features ✅ (2026-03-06)

- Core: `TextDirection` enum (Horizontal/Vertical/VerticalAll) on `Section`
- Core: `DropCapStyle` enum (None/DoubleLine/TripleLine/Margin) on `ShapeStyle`
- Encoder: `page_break` encoding fix (was hardcoded to 0)
- Encoder: `char_border_fill_id` dynamic `borderFillIDRef` (was hardcoded)
- ~200 LOC

### Test Coverage Boost ✅ (2026-03-06)

- Coverage: 85.91% → 92.65% (CI threshold 90%)
- ~270 new unit tests across 7 files
- shapes decoder: ~40% → 91%
- shapes encoder: ~40% → 96%
- section decoder: 82% → 93%
- section encoder: 77% → 95%
- md lossless encoder: 58% → 90%
- md lossless decoder: 73% → 99%

### Wave 8-13 파일 분할

- `schema/section.rs` → `schema/section.rs` + `schema/shapes.rs` 서브모듈 분리
- `encoder/section.rs` → `encoder/section.rs` + `encoder/shapes.rs` 서브모듈 분리
- `decoder/section.rs` → `decoder/section.rs` + `decoder/shapes.rs` 서브모듈 분리

---

## Phase 6-8: 이후 계획

### Phase 6: Bindings (Python + CLI)

- `hwpforge-bindings-py`: PyO3 래퍼 → `pip install hwpforge`
- `hwpforge-bindings-cli`: clap CLI → `hwpforge convert`, `hwpforge template`
- Phase 4.5 완료 후 API 고정 상태에서 래핑

### Phase 7: MCP Integration

- Python SDK 기반 MCP Server
- 도구: `create_document`, `apply_template`, `convert_md_to_hwpx`
- LLM 에이전트가 자연어로 문서 생성

### Phase 8: Testing + Release v1.0

- Golden test 대폭 보강 (실제 한글 문서 30+ 유형)
- 성능 벤치마크 (1000 페이지 문서)
- crates.io + PyPI 배포
- 문서화 완성 (README, API docs, 튜토리얼)

---

## v2.0 계획

### Phase 9: HWPX Full Compatibility (나머지)

- OLE 객체
- 양식 컨트롤 (체크박스, 라디오, 입력란)
- 변경 추적
- ~~책갈피 + 상호참조~~ ✅ Wave 12에서 완료
- 그리기 객체 묶음/그룹 (Container)
- 글맵시 (TextArt)
- 도형 채우기 확장 (Gradient/Pattern/Image FillBrush 스키마)

### Phase 10: HWP5 Reader

- HWP5 바이너리 → Core DOM
- T1~T2 범위 (텍스트, 표)
- HWP5 → HWPX 변환 파이프라인

---

## Scope Matrix

| Version | HWPX Read | HWPX Write | HWP5 | MD | 비고 |
|---------|-----------|------------|------|-----|------|
| v1.0 현재 | T1~T3 | **T1~T4 전체** | ❌ | ✅ R/W | Wave 1-13 완료 (페이지레이아웃/도형완성/참조주석 포함) |
| v1.0 목표 | T1~T3 | T1~T4 전체 | ❌ | ✅ R/W | ✅ 달성 — Wave 13 완료 |
| v2.0 | Full | Full | T1~T2 Reader | ✅ R/W | Phase 9-10 (OLE/양식/변경추적/TextArt/Group) |

---

## 계획 변경 이력

| 날짜 | 변경 내용 | 사유 |
|------|-----------|------|
| 2026-02-07 | 초기 로드맵 작성 | 프로젝트 킥오프 |
| 2026-02-10 | HWP5를 v2.0으로 이동 | HWPX 우선 전략 |
| 2026-02-17 | Phase 5 완료, Phase 4.5 삽입 | HWPX Write API 완성 후 Bindings |
| 2026-02-18 | v1.0 스코프 확장: 캡션/수식/차트/도형/다단 추가 | Write API 완전 안정화 후 Bindings |
| 2026-02-18 | Phase 4.5 Wave 3 완료: 다단 + 도형 (선/타원/다각형) | 커밋 eded748, 94355da |
| 2026-02-18 | Phase 4.5 Wave 4 완료: 캡션 (Table/Image/TextBox/Line/Ellipse/Polygon 전체 지원) | — |
| 2026-02-19 | Line shape fix: namespace(hc:), field order, multi-switch Vec, decode_shape_style DRY, line_styles example | line.hwpx ground truth 기반 팩트 체크 |
| 2026-02-19 | Phase 4.5 Wave 5 완료: 수식 (Equation) — HancomEQN 스크립트, NO shape common, flowWithText=1 | equations.hwpx ground truth 기반 |
| 2026-02-19 | Phase 4.5 Wave 6 완료: 차트 (18종 ChartType, OOXML encode/decode, 12-chart golden test) | charts.hwpx ground truth 기반 |
| 2026-02-19 | Bug Fix: colPr self-closing, polygon hc:pt namespace, chart_offset multi-section, TextBox 검증 완료 | full_report.rs 한글 호환 검증 |
| 2026-02-25 | 의존성 마이그레이션: schemars 1.2, quick-xml 0.39, zip 8.1, pulldown-cmark 0.13; MSRV 1.88; 통계 갱신 (988 tests, 37,052 LOC, 75 .rs files) | CI/CD 강화 브랜치 (cicd-hardening) |
| 2026-02-27 | Phase 5.5 완료: 28개 편의 생성자, 하이퍼링크 인코딩, breakNonLatinWord 수정, WordBreakType 추가 | API 안정화 |
| 2026-02-28 | Write API 잔여 3건 완료: Line/Polygon 절대위치, Chart 71종 완전호환 (9 sub-options + StockVariant enum), TOC titleMark (heading_level) | HWPX Write API 100% |
| 2026-02-28 | VHLC 4축 combo layout 버그 수정: 3축(catAx 공유) → 4축(독립 축 쌍). CLAUDE.md gotcha #23 추가 | 한글 차트 호환성 |
| 2026-03-04 | Wave 7 완료: Style Infrastructure — Distribute/DistributeFlush alignment, Paragraph.style_id, dynamic BorderFill encoding, per-style charPr/paraPr formatting | HWPX Write API 스타일 인프라 완성 |
| 2026-03-05 | Wave 8+10+13 완료: Paragraph Features (NumberingIndex/TabIndex, dynamic numberings/tabs, heading attributes), Character Enhancements (EmphasisType 13종, CharShape 7필드 확장), Remaining Content (Dutmal/Compose encode/decode) | 병렬 구현 (3 Waves 동시) |
| 2026-03-05 | Wave 8/10/13 감사: HeadingType enum, EmphasisType Display PascalCase, parse_number_format DRY, Dutmal lossless MD, wave8_10_13_test.rs 예제 | 코드 리뷰 반영 |
| 2026-03-06 | Wave 9+11+12 완료: Page Layout (Gutter, Visibility, LineNumberShape, PageBorderFillEntry, BeginNum, MasterPage), Shape Completions (Arc/Curve/ConnectLine, rotation/flip/arrow, Fill enum), References (Bookmark/CrossRef/Field/Memo/IndexMark) | 병렬 구현 (3 Waves 동시) |
| 2026-03-06 | 핵심 수정: ArrowType EMPTY_ 규칙, MasterPage namespace (prefix 없는 루트 + 15 xmlns), SUMMERY 필드 인코딩, autoNum 페이지 번호 | 한글 호환성 검증 |
| 2026-03-06 | 미구현 v2.0 이동: 11.4 TextArt, 11.5 Container/Group, 11.7 FillBrush 스키마 확장 (Core types만 준비됨) | 우선순위 기반 연기 |
| 2026-03-06 | Wave 14 완료: TextDirection, DropCapStyle, page_break 인코딩 수정, char_border_fill_id 동적 처리 (~200 LOC) | 최종 기능 마무리 |
| 2026-03-06 | Test Coverage Boost: 85.91% → 92.65% (~270 신규 테스트, 7 파일, shapes/section/md lossless 집중 보강) | CI threshold 90% 달성 |

### 2026-02-18 변경 상세

**변경 전**:
```
Phase 4.5 (a-e) → Phase 6 → Phase 7 → Phase 8
v2.0 Phase 9: 수식, 차트, 캡션, 도형, 다단, OLE, 양식, 변경추적, 책갈피
```

**변경 후**:
```
Phase 4.5 (a-j, Wave 1-6) → Phase 6 → Phase 7 → Phase 8
v1.0으로 이동: 캡션, 수식, 차트, 도형, 다단
v2.0 유지: OLE, 양식 컨트롤, 변경 추적, 책갈피
```

**사유**:
1. **관공서 제안서 필수 기능**: 캡션(표/그림 번호), 수식, 차트는 제안서에서 자주 사용
2. **도형**: rect 패턴 이미 구현됨 — 선/타원/다각형은 확장 비용 낮음
3. **다단**: secPr colPr 확장만으로 구현 가능 (난이도 하)
4. **API 안정성**: Write API를 최대한 완성한 후 Phase 6(Bindings) 착수

---

## Risk Management

| 리스크 | 완화 전략 |
|--------|-----------|
| 스펙과 실파일 불일치 | 구현체 교차검증 + golden test 기반 (37개 불일치 문서화 완료) |
| 수식 포맷 복잡성 | 한글 실파일 리버스 + MathML 대안 검토 |
| ~~차트 구조 복잡성~~ | ~~최소 3종 MVP~~ → ✅ 71종 전체 완료 (VHLC 4축 포함) |
| 이미지 포맷 다양성 | PNG/JPG만 v1.0, GIF/BMP는 v2.0 |
| API 변경 시 Bindings 재작업 | Phase 4.5 Wave 6까지 API 고정 후 Phase 6 착수 |
| MCP 디버깅 난이도 | 도구를 작게 시작하고 점진 확장 |

---

## Next Actions

1. ~~Phase 5 (smithy-md) 구현~~ ✅ 완료
2. ~~Phase 4.5a-e (Wave 1-2)~~ ✅ 완료
3. ~~Phase 4.5f+4.5g (Wave 3: 다단 + 도형)~~ ✅ 완료
4. ~~Phase 4.5h (Wave 4: 캡션)~~ ✅ 완료
5. ~~Phase 4.5i (Wave 5: 수식)~~ ✅ 완료
6. ~~Phase 4.5j (Wave 6: 차트)~~ ✅ 완료
7. ~~Bug Fix (colPr/polygon/chart_offset)~~ ✅ 완료
8. ~~TextBox 디버깅 (한글 golden fixture 기반 검증)~~ ✅ 완료
9. ~~Chart 71종 완전 호환 (9 sub-options + VHLC 4축)~~ ✅ 완료
10. ~~Line/Polygon 절대 위치 + TOC titleMark~~ ✅ 완료
11. ~~Wave 7: Style Infrastructure (StyleIndex, BorderFill, per-style 서식, Distribute 정렬)~~ ✅ 완료 (2026-03-04)
12. ~~Wave 8: Paragraph Features (번호 매기기, 탭 설정, 개요 paraPr)~~ ✅ 완료 (2026-03-05)
13. ~~Wave 10: Character Enhancements (EmphasisType, CharShape 확장)~~ ✅ 완료 (2026-03-05)
14. ~~Wave 13: Remaining Content (Dutmal, Compose)~~ ✅ 완료 (2026-03-05)
15. ~~Wave 9: Page Layout Completion (Gutter, Visibility, LineNumberShape, PageBorderFill, BeginNum, MasterPage)~~ ✅ 완료 (2026-03-06)
16. ~~Wave 11: Shape Completions (Arc, Curve, ConnectLine, rotation/flip/arrow, Fill enum)~~ ✅ 완료 (2026-03-06)
17. ~~Wave 12: References & Annotations (Bookmark, CrossRef, Field, Memo, IndexMark)~~ ✅ 완료 (2026-03-06)
18. ~~Wave 14: Final Features (TextDirection, DropCapStyle, page_break fix, char_border_fill_id)~~ ✅ 완료 (2026-03-06)
19. ~~Test Coverage Boost: 85.91% → 92.65% (~270 tests, CI threshold 90%)~~ ✅ 완료 (2026-03-06)
20. **Phase 6 바인딩 API surface 고정**
21. **Phase 7 MCP tool 스키마 확정**
22. **v1.0 release gate용 golden test 보강**
