# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed (BREAKING)

- `Paragraph`와 `Control`이 `Drop`을 구현해, 100,000단계로 중첩된 Draft(메모·묶음·글상자·캡션·표 칸)를 스택 넘침 없이, 패닉 unwinding 중에도 해제한다. `validate`는 거절한 문서를 더 이상 직접 해체하지 않는다. 이행: `Paragraph`·`Control`에서 `Copy`가 아닌 필드를 밖으로 옮기는 코드(`let runs = paragraph.runs;`, `let Control::Group { children, .. } = control`, `*boxed_control` 매칭)는 E0509로 컴파일되지 않는다. 필드를 빌리거나 mutable 바인딩에서 `std::mem::take`·`Option::take`로 꺼낸다. `Run`·`Table`·`Image` 등은 전처럼 분해할 수 있고, clone·Debug·동등 비교·직렬화는 여전히 재귀다.

### Changed

- `FieldInventory`가 필드 없는 문서에서도 모든 run의 경로 키와 흐름 이름을 저장해 D단계 중첩 Draft에 O(D^2) 바이트를 쓰던 것을, 경로를 하나의 커서에 두고 필드 관련 run이 있는 흐름만 주소를 기록하게 바꾼다. 필드 없는 글상자 사슬 깊이 10,000에서 inventory 최대 메모리가 4,018 MB에서 12 MB로, `stamp::plan`이 7,983 MB에서 14 MB로 줄었다. 질의 결과·순서·문자열 형태는 같다. 필드가 있는 문서의 깊은 경로 질의는 질의마다 O(D^2) 시간이 걸린다.

### Added

- Core 의미 깊이 상한 `MAX_NESTING_DEPTH`(32)를 공개한다. MD·HWPX·HWP5 파서의 총량 예산도 이 값을 참조하지만 카운터의 의미가 같다는 뜻은 아니다.
### Fixed

- 문단·표·이미지 순회(와 그 아래 크레이트 순회)를 명시적 스택으로 돌려, 100,000단계로 중첩된 Draft를 순회해도 스택이 넘치지 않는다. 방문 순서, 캡션 제외, 자식을 콜백 뒤에 나열하는 규칙은 그대로다.
- `FieldInventory`가 깊이 32단계를 넘는 문서도 끝까지 읽고 깊이 진단을 더한다. 이런 문서에서 replace·insert·delete는 거부되고 clear는 아무것도 하지 않는다.
- ZIP 중앙 헤더 예산이 `zip` 9.1에서 헤더 읽기를 세지 못해, 고정 레코드 상한이 `zip`의 대체 경로에서 작업량을 제한하지 못하던 것을 고친다. 이제 정확히 46바이트이고 중앙 헤더 서명으로 시작하는 읽기를 레코드 하나로 센다. 상한(4 x `MAX_ENTRIES`)은 그대로이며 `zip` 9.0과 9.1 모두에서 적용된다. 바깥 항목의 주석 안에 숨긴 디렉터리는 `zip` 9.0의 대체 경로에서 전부 파싱하지 않고 40,001개째 레코드에서 멈추고, `zip` 9.1에서는 디렉터리 구조 검사에서 거부한다. 서명으로 시작하는 46바이트 이름·extra·주석도 레코드로 세므로 그런 archive는 상한에 더 일찍 닿는다. 일반 HWPX는 그런 모양이 아니다.

## [0.17.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.17.0...hwpforge-core-v0.17.1) - 2026-10-24

### Added

- `Section::check_nesting_depth()` 로 직접 만든 `Section` 의 중첩 깊이(상한 32)를 미리 검사한다 (#267)

### Fixed

- `Document::validate` 가 문서를 재귀로 걷기 전에 중첩 깊이를 반복문으로 재고, 32단계를 넘으면 기존 `CoreError::InvalidStructure` 로 거부한다. 거부한 문서도 재귀 없이 해제한다. 전에는 공개 API 로 수천 단계 이상 중첩한 값(묶음 안 묶음, 메모 안 메모 등)을 검증할 때 스택이 넘쳐 프로세스가 오류 없이 죽었다. 세는 규칙은 Markdown 인코더가 쓰던 것과 같아 그림·도형 캡션을 두 단계로 센다. 그래서 전에 HWPX 디코더를 통과하던 경계의 문서가 거부될 수 있다. 측정한 HWPX 3,828개의 최대 깊이는 7이다 (#267)
- 묶음 안 그림의 빈 이미지 경로를 본문 그림과 같은 규칙으로 허용한다. 자산을 잃은 그림을 빈 틀로 보존하기 위한 것이고, 크기 검사는 그대로다 (#266)

## [0.17.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.16.11...hwpforge-core-v0.17.0) - 2026-10-23

### Changed (BREAKING)

- 그리기 개체 열 가지와 `Image`·`Table` 에 `frame: ObjectFrame`(원본 크기, 크기 잠금)이 생겼다. `Image::original_size` 는 `Image::frame.original_size` 로 옮겼다.
- `Control::Group::children` 이 `Vec<GroupChild>` 가 되어 묶음이 그림을 담는다. `Control::Arc` 에 `definition: ArcDefinition` 이 생겼다(없는 JSON 은 `EllipseSweep`).
- `LineSeg`(`boundary` 추가, `Copy` 아님)·`Caption`(`full_size`, `last_width`)·`ObjectPlacement`(정렬, `affect_line_spacing`, `hold_anchor_and_so`)·`MasterPage`(범위·겹침·앞쪽 표시·본문 영역)가 `#[non_exhaustive]` 가 되었다. 생성자와 `with_*` 빌더를 쓴다.
- `Paragraph::end_char_shape`, `TextBox::text_margin`, `ShapeStyle::corner_ratio`(0–50, 넘으면 검증 오류), `Fill::Gradient` 의 `center_x`·`center_y`·`step`·`step_center`, `HeaderFooter::anchor_paragraph`, `Table::placement` 가 생겼다.
- 문단 여백 다섯 가지와 `TabStop.position` 이 `ParagraphMeasure`(HWPUNIT 또는 1/500 글자)가 되었다.
- `CoreErrorCode` 에 `InvalidMasterPageScope`, `HeaderFooterAnchorOutOfRange` 등이 늘어 전수 매칭에는 와일드카드가 필요하다.

### Added

- `Control::AutoNumber`(캡션 번호), `Table::cell_zones`, `Table::no_adjust`(고정 셀 크기), 셀 줄 바꿈, 개체 바깥 여백, 그림의 자르기·이미지 크기·하이퍼링크, 채우기 없음을 나타내는 도형 스타일, 형식에 묶이지 않는 도형 기하 정의.

## [0.16.9](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.16.8...hwpforge-core-v0.16.9) - 2026-10-03

### Fixed

- *(core)* `Control::TextArt.text` 의 줄바꿈 형식(`\r\n`)과 HWPX 왕복 규칙을 문서화한다. `Display` 는 글맵시 글자를 이스케이프해 여러 줄 글맵시도 진단 출력 한 줄로 나온다 (#199)


## [0.16.8](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.16.7...hwpforge-core-v0.16.8) - 2026-10-01

### Fixed

- *(core)* `ImageStore` 를 key 순서로 순회한다. 같은 문서를 여러 번 HWPX 로 저장해도 `content.hpf` 의 이미지 항목과 ZIP 안 `BinData/*` 순서가 같아 출력 바이트가 재현된다 (#200). 순서는 key 문자열 순서 (`image10` 이 `image2` 보다 앞)


## [0.16.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.16.4...hwpforge-core-v0.16.5) - 2026-09-17

### Added

- *(core)* 불변 문단 순회기 for_each_paragraph 추가


### Documentation

- *(core)* 문단 순회기의 정확한 재귀 집합과 이미지 캡션 갭을 명시


## [0.16.4](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.16.3...hwpforge-core-v0.16.4) - 2026-08-28

### Fixed

- *(smithy-hwpx)* 각주 번호부여 5차 평결 상환 — 마커 가드 전종화·경고 관통

- *(smithy-hwpx)* 각주 번호부여 3차 평결 상환 — 명시적 시작번호 보존


## [0.16.3](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.16.2...hwpforge-core-v0.16.3) - 2026-08-26

### Fixed

- *(smithy-md)* 임베드 리뷰 상환 — 단독 문단 보존·bare 경로 base·BMP 구조검사

- *(smithy-md)* md→hwpx 이미지 참조를 BinData 로 실제 임베드


## [0.16.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.15.2...hwpforge-core-v0.16.0) - 2026-08-25

### Added

- *(core)* **BREAKING** ObjectPlacement 공용화 릴리스 트리거 재선언 (파일 변경 동반)

- feat!(core): 도형 placement 공용화 — ObjectPlacement 로 hp:pos 무음 소실 제거


## [0.14.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.13.1...hwpforge-core-v0.14.0) - 2026-08-13

### Added

- pghd 감추기 carry + 명시적 쪽나눔 렌더 존중 (W3)

- nwno 새 번호 지정 carry — HWP5→Core→HWPX→PDF 전 구간 (W2)


## [0.13.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.13.0...hwpforge-core-v0.13.1) - 2026-08-12

### Added

- *(core)* 쪽번호 전용 CHAR 스타일 조회 char_style_shape 추가


## [0.13.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.12.1...hwpforge-core-v0.13.0) - 2026-08-09

### Added

- *(core)* **BREAKING** W5-α 머리말/꼬리말 subList 기하 승격 — vertAlign·textWidth·textHeight

- *(core)* **BREAKING** W5-α pageStartsOn 승격 — 스키마 폐기 교정 + 인코더 BOTH 고정 제거

- *(smithy-pdf)* W4c 스타일 선택·언어축 검사 — 기본 fatal + Degraded 옵트인

- *(smithy-pdf)* W3c 표 배치소스 — 검증된 프로파일 재생 + 계산 페이지네이션 이중 검산

- *(core)* StyleLookup borderFill 렌더 표면 + HWPX 브리지 (PDF W3b)

- *(core)* 표 outMargin/inMargin 구조 승격 + sz height decode-only 캐시 (PDF W3a-1)


## [0.12.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.11.7...hwpforge-core-v0.12.0) - 2026-08-05

### Added

- *(core)* **BREAKING** 줄 조판 캐시 LayoutCache 승격(decode-only) + HWPX 디코더 승격·admission 캐시 정규화


### Fixed

- *(core)* non_exhaustive Metadata rustdoc 예제 수리 (doctest bit-rot)


## [0.11.7](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.11.6...hwpforge-core-v0.11.7) - 2026-07-25

### Fixed

- lenient 격자 배치에 span-폭주 사전 가드 (E3 L3)


## [0.11.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.11.4...hwpforge-core-v0.11.5) - 2026-07-23

### Added

- *(core)* Control::kind_name — 컨트롤 종류의 안정 snake_case 이름

- *(core)* 문단 outline/list 분류기 공유 모듈 추가


## [0.11.3](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.11.2...hwpforge-core-v0.11.3) - 2026-07-21

### Added

- *(core)* 표 논리 격자 파생 모듈 table::grid 신설


### Fixed

- 완전 피복 빈 표 행을 진실하게 수용하고 HWP5 유령 셀 폴백 제거


## [0.11.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.10.0...hwpforge-core-v0.11.0) - 2026-07-12

### Added

- *(core)* **BREAKING** 누름틀 display_text 가 채워진 본문을 보존하도록 의미 변경


## [0.10.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.9.0...hwpforge-core-v0.10.0) - 2026-07-02

### Added

- *(core)* **BREAKING** carry multi-column separator line (colLine)


## [0.9.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.8.0...hwpforge-core-v0.9.0) - 2026-06-28

### Changed

- *(core)* **BREAKING** link cross-ref to target via shared ObjectId (E6 M2)


## [0.8.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.7.0...hwpforge-core-v0.8.0) - 2026-06-27

### Added

- *(core)* **BREAKING** shape text vertical alignment (ellipse/polygon/textbox)


### Changed

- *(core)* **BREAKING** rename Summery typo to Summary in IR identifiers (E6 slice A)

- *(core)* split control.rs into submodules (E7 #2)


## [0.7.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.6.0...hwpforge-core-v0.7.0) - 2026-06-19

### Added

- *(hwp5)* TextArt (글맵시 / <hp:textart>) carry — HWP5↔HWPX

- *(hwp5)* carry group/묶음 객체 (<hp:container>) HWP5↔HWPX — Wave A (flat)

- *(core)* **BREAKING** Wave 12p Step 2 — Core breaking: Image/Table/Equation 에 inst_id (cross-ref target carry)

- *(hwp5)* **BREAKING** Wave 12m Phase 2 Step 4 — Control::CrossRef vertical slice

- *(core)* **BREAKING** Wave 12m Phase 2 Step 3 — foundation/core API breaking (RefType + RefContentType + RefTarget + Control::CrossRef target)

- *(core)* **BREAKING** Wave 12o Phase 0 — Metadata에 description/last_saved_by/extras + non_exhaustive (ADR-003)

- *(core)* **BREAKING** Wave 12n — 자동 필드 의미 분할 + HWPX carry

- *(hwp5)* **BREAKING** Wave 12l — 누름틀(ClickHere) carry + name 메타 carry

- *(hwp5)* **BREAKING** Wave 12j — 글자겹침(compose) carry + char_pr_ids fidelity + packed-variant 지원

- *(hwp5)* **BREAKING** Wave 12i — 덧말(dutmal) option carry + flat-path control_iter 필터

- *(hwp5)* **BREAKING** Wave 12f-h — 메모 anchor 위치 수정 + 7 parameters wire 메타 carry 완성

- *(hwp5)* **BREAKING** Wave 12e — 메모 본문 carry + 본문 텍스트 덮어쓰기 버그 수정


### Fixed

- *(hwp5)* carry SUMMERY/PATH field cached value to close 한컴 recovery warning (#120/#136)

- *(hwpx)* Wave 12p task #124 — SUMMERY editable per FieldType + Wave 12p Step 4 visual gate + fmt fallout


## [0.5.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.4.0...hwpforge-core-v0.5.0) - 2026-03-22

### Added

- *(list)* **BREAKING** add checkable bullet semantics

- *(list)* **BREAKING** add shared list semantics


### Fixed

- *(md)* preserve task list continuations


## [0.4.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.3.0...hwpforge-core-v0.4.0) - 2026-03-20

### Changed

- Extend `TabDef` with explicit `TabStop` semantics so tab definitions can carry stop position, alignment, and leader data through the shared IR.
- Add shared helpers for default-tab merging, reference validation, and tab-position clamping used by HWPX/HWP5 bridges.

### Migration

- `TabDef` struct literals must now initialize the `stops` field.
- Consumers that duplicated tab-default merge or reference-validation logic should move to the shared helpers on `TabDef`.

## [0.3.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.2.1...hwpforge-core-v0.3.0) - 2026-03-18

### Chore

- *(release)* **BREAKING** prepare v0.3.0 for preserving section API changes


## [0.2.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.1.7...hwpforge-core-v0.2.0) - 2026-03-17

### Changed

- Extend the public table DOM with page-break, repeat-header, cell-spacing, border/fill, row-header, cell height, margin, and vertical-alignment semantics.
- Extend the public image DOM with placement metadata.
- Move `ValidationError::NonLeadingTableHeaderRow` to the tail of the enum to avoid unnecessary discriminant drift for existing variants.

### Migration

- `Table`, `TableRow`, `TableCell`, and `Image` are now `#[non_exhaustive]`. Construct them with `new`/`with_*` builders instead of struct literals.
- New builder methods are available on `Table`, `TableCell`, and `Image` to cover the v0.2.0 public fields without direct field construction.
- Validation code consumers should handle `CoreErrorCode::NonLeadingTableHeaderRow`.

## [0.1.7](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.1.6...hwpforge-core-v0.1.7) - 2026-03-12

### Added

- HWPX→Markdown styled conversion pipeline


## [0.1.4](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.1.3...hwpforge-core-v0.1.4) - 2026-03-09

### Changed

- extract shared types into smithy-hwpx to eliminate CLI/MCP duplication
