# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] — targeted as `0.6.0`

### Known Issues — HWP5→HWPX 옵션 손실 (P2/P3, Core breaking 필요, 후속 슬라이스)

옵션 단위 전수조사에서 식별된 잔여 갭. **공유 Core/Foundation 모델이 해당
정보를 담을 자리가 없어** 수정에 public API breaking 이 필요하므로 별도 슬라이스로
보류 (P0/P1 은 매핑 수정만으로 해소돼 완료). 착수 전 각 항목의 macOS 한글 저작
가능 여부 확인 필요 (P0-2 가운데밑줄처럼 Windows-blocked 일 수 있음). 상세 표·코드
위치: `.docs/audit/2026-06-17_hwp5_hwpx_option_gaps.md` (§P2/§P3).

**P2 — 경고는 나가지만 손실 (`ProjectionFallback` 동반, 값을 알지만 Core 가 못 담음):**

- 글자: 외곽선/그림자 종류 단일화, **그림자 색·위치 미carry**, **스크립트별 자간/장평/상대크기(7스크립트) 붕괴**
- 문단: 세로정렬, 줄높이 플래그, **한영/한숫자 자동 간격(한국어 조판 영향)**, 테두리 오프셋·연결 플래그, 줄간격 kind≥4
- 기타: 반복 헤더행 강등, 필드 내부 tab 속성, TextArt enum 미상값, page_border_fill 레코드≠3 매핑

**P3 — Core enum 천장 (경고도 없이 raw 초과분이 기본값으로):**

- `UnderlineShape` raw 12–15 · `StrikeoutShape` ≥13 · `EmphasisType` ≥7 · `DutmalPosition` ≥4 · 이미지 fill effect · border line kind · border width ≥16 → 각 기본값
- (일부는 P1-1 처럼 스펙에 실재하지 않는 값일 수 있음 — 착수 시 reference/fixture 로 도달성부터 확인)

**그 외**: non-chart OLE · masterPage(gap C) · 양식컨트롤 · 쪽 테두리/배경 hatch 페이지 경로 · 가운데 밑줄 — 전부 **Windows 한컴 fixture 대기** (macOS 저작 불가, 상세는 `CLAUDE.md` Still-deferred).

### Fixed — HWP5→HWPX 채우기 "색 없음" → `faceColor="none"` (P1-5, 옵션 전수조사)

HWP5 는 "배경색 없음" 을 None fill 이 아니라 **Color fill + `background_color =
0xFFFFFFFF`** (Windows COLORREF null sentinel)로 인코딩한다. `colorref_to_hwpx_color`
가 `(raw>>24)!=0` 검사를 먼저 통과시켜 `faceColor="#FFFFFFFF"` (흰색)을 emit했고,
한컴 native 는 `faceColor="none"` 을 쓴다.

- 신규 fill 전용 헬퍼 `colorref_to_hwpx_fill_color`: `0xFFFFFFFF → "none"`,
  그 외는 기존 매핑. face/hatch 색에만 적용 — 테두리 선 색은 native 가 실제
  `#RRGGBB` 를 쓰므로 공유 함수 그대로 둠 (검증 안 된 거동 도입 방지).
- 검증: 기존 native `sample-cell-diagonal` (borderFill id=2 background=0xFFFFFFFF)
  변환 결과가 `faceColor="none"` 으로 일치, 출력에서 `#FFFFFFFF` 완전 제거.
- 테스트: `fill_color_maps_no_color_sentinel_to_none`.

### Changed — 미상 무늬/그러데이션 type warning-first (P1-3/4, 옵션 전수조사)

`border_fill` 의 채우기 무늬·그러데이션 type 이 알 수 없는 raw 값일 때 조용히
기본값(무늬→솔리드, 그러데이션→LINEAR)으로 떨어지던 것을 `ProjectionFallback`
경고로 노출. (알려진 6개 무늬·4개 그러데이션 type 은 정상 carry 되며, 무늬
`None`=무늬 없음 은 경고 대상 아님.)

- 신규 경고 subject: `style.border_fill.fill_pattern`,
  `style.border_fill.gradation_type` (각각 `border_fill_id` + raw 값 포함).
- 정상 저작으론 도달하지 않는 방어적 경로라 native fixture 불필요.
- 테스트: `unknown_hatch_pattern_warns_but_known_and_none_stay_silent`,
  `unknown_gradation_type_warns_but_known_stays_silent`.

### Fixed — HWP5→HWPX 이미지 채우기 모드 12종 (P1-2, 옵션 전수조사)

배경 그림 채우기(`<hc:imgBrush mode>`)의 16개 모드 중 4개(TILE/TOTAL/CENTER/
ZOOM)만 매핑되고 **나머지 12개가 무음으로 투명 처리(fill 드롭)** 되던 문제.
바둑판 가로/세로, 가운데 위/아래, 좌/우 정렬 채우기가 전부 사라졌다.

- `hwp5_image_fill_mode_to_hwpx` 를 16개 모드 전부로 완성. HWP5 raw 0-15 는
  OWPML `ImageBrushMode` enum 과 동일 순서 1:1 (raw 1=TILE_HORZ_TOP, 7=
  CENTER_TOP, 10=LEFT_TOP, 14=RIGHT_BOTTOM, `*Middle`→`*_CENTER` 등).
- 검증: 사용자 작성 native `sample-cell-image-fill` (4 family 대표 모드
  TILE_HORZ_TOP/CENTER_TOP/LEFT_TOP/RIGHT_BOTTOM) 변환 결과가 한컴 native
  와 일치, 변환 경고 4→0.
- 테스트: `image_fill_mode_maps_all_16_ks_x_6101_modes` (전 모드 단위).

### Fixed — HWP5→HWPX 쪽 번호 형식 carry (P0-3, 옵션 전수조사)

`pgnp`(쪽 번호 위치) 컨트롤의 **번호 모양 바이트를 전혀 읽지 않아** 모든 쪽
번호가 `formatType="DIGIT"` 로만 나가던 문제. 로마자/한글/알파벳 쪽 번호가
조용히 아라비아 숫자로 변환됨.

- `parse_page_number_control` 이 property bits 0-7 (`header_data[4]`) =
  번호 모양(`HWPNumberShape`)을 읽어 `NumberFormatType::try_from` 으로 매핑
  (위치는 기존대로 bits 8-11 = `header_data[5]`). HWP5 shape 코드는
  `NumberFormatType` 과 1:1 (0=Digit, 2=RomanCapital, …).
- 검증: 사용자 작성 native `sample-pagenu-roman` (로마자 대문자) 변환 결과가
  한컴 native 와 byte-identical — `<hp:pageNum pos="INSIDE_TOP"
  formatType="ROMAN_CAPITAL" sideChar="-"/>`.
- 테스트: `parse_page_number_control_reads_number_shape_from_property` (단위),
  `..._user_sample_page_number_format_matches_native` (native e2e).

### Fixed — HWP5→HWPX 문단 번호매기기 형식 5종 (P0-1, 옵션 전수조사)

문단 번호 형식(`<hh:paraHead numFormat>`) 디코딩 매핑이 KS X 6101 OWPML
`NumberType1` enum 과 어긋나 일부 형식이 손실/오변환됐다. 옵션 단위 전수조사
중 발견.

- **code 11**: `"HANJA_DIGIT"` (OWPML 에 없는 무효 문자열) → `CIRCLED_HANGUL_JAMO`
  (원 ㄱ,ㄴ,ㄷ) 로 정정. 한자 숫자는 실제로 code 13 = `IDEOGRAPH`.
- **code 6** (`CIRCLED_LATIN_CAPTION`, 원 알파벳 대문자 Ⓐ), **code 12**
  (`HANGUL_PHONETIC` 일,이,삼), **code 13** (`IDEOGRAPH` 한자), **code 14**
  (`CIRCLED_IDEOGRAPH`) 추가 — 이전엔 전부 무음으로 `DIGIT` 로 붕괴.
- code 0~10 은 사용자 작성 native fixture `sample-numbering-hangul`(10수준,
  9형식)로 byte 단위 일치 확인 — 가/나/다(`HANGUL_SYLLABLE`), ㄱ/ㄴ/ㄷ
  (`HANGUL_JAMO`) 등 정상 carry 회귀 잠금. (audit 의 "가/나/다가 아라비아로
  나간다" 주장은 거짓이었음 — 코드 정독 한계, fixture 로 반증.)
- 테스트: `numbering_num_format_covers_full_ks_x_6101_enum` (전 코드 단위),
  `..._user_sample_numbering_formats_match_native` (native e2e 게이트).

### Verified — HWP5→HWPX 빗금무늬(hatch fill) carry + gotcha #21 방향 스왑

빗금/역빗금 등 무늬 채우기(`<hc:winBrush hatchStyle>`)의 HWP5→HWPX carry 를
native 한컴 fixture 로 실측 검증. **코드 변경 없음** — 디코드/projection/인코더
경로는 이미 완성돼 있었고, 한컴 HWPX writer 의 빗금↔역빗금 문자열 스왑
(gotcha #21: 시각 빗금(/) → `"BACK_SLASH"`, 시각 역빗금(\) → `"SLASH"`) 도
이미 올바르게 반영돼 있었음.

- **fixture**: 사용자 작성 native `sample-charbg-hatch-{slash,backslash}.{hwp,hwpx}`
  (글자 배경 무늬 — hatch fill 은 page/char/table border fill 에 공유되므로
  동일 코드 경로를 검증). 우리 출력 winBrush 가 한컴 native 와 byte-identical
  (`faceColor="#E5E5E5" hatchColor="#CA56A7" hatchStyle="BACK_SLASH"/"SLASH"`).
- **회귀 게이트**: `..._charbg_hatch_carries_swapped_pattern_direction`.
- **Windows-deferred**: **쪽 테두리/배경**(페이지) 무늬 fixture 는 macOS 한글
  [쪽] 메뉴에 항목 자체가 없어 작성 불가 → **Windows 한글 fixture 대기**
  (masterPage gap C / non-chart OLE 와 동일 차단). 단 hatch fill 자체는 위에서
  공유 검증됨.

### Added — HWP5→HWPX 다단(multi-column / `<hp:colPr>`) carry

한 구역을 2단/3단 신문형으로 나눈 다단을 HWP5→HWPX 변환에서 보존; 이전엔
`cold` ctrl 을 마커로만 필터하고 단 개수를 항상 `colCount="1"` 로
하드코딩해 단 정보가 손실됐다.

- **HWP5 디코더**: `cold`(`CTRL_ID_COLUMN_DEF`) ctrl payload 파싱 —
  `[4..6]` u16 property (bits 2-9 = 단 개수), `[6..8]` u16 단 간격(HWPUNIT).
  `secd` 사이드카 캡처 패턴 미러로 `SectionResult.column_def` 에 저장.
- **Projection**: `col_count >= 2` 면 `Section.column_settings =
  ColumnSettings::equal_columns(count, gap)`. 단일 단은 `None` 유지(인코더
  기본값).
- **인코더/Core**: 변경 없음 — `Section.column_settings` 와
  `build_col_pr_xml` 가 이미 multi-column emit + HWPX round-trip 지원.
  HWP5 leg 만 비어 있던 것.
- **검증**: 사용자 작성 `sample-multicolumn.hwp` (2단) 변환 결과 colPr 가
  native 와 byte-identical (`colCount="2" sameSz="1" sameGap="2268"`),
  0 warnings. 디코더 단위 테스트(`ctrl_header_cold_captures_column_def`)
  추가. nextest/clippy clean, 한컴 시각 게이트 PASS.

### Fixed — Blueprint 저작 시 밑줄 선종류(underline shape) 손실

Blueprint(YAML 템플릿 / 빌더 API)로 만든 문서의 밑줄이 선종류와 무관하게
항상 `SOLID` 로 나가던 문제. **Core breaking (additive)**: `CharShape` /
`PartialCharShape` 에 `underline_shape` 필드 추가 (`strikeout_shape` 는
이미 있었으나 `underline_shape` 만 누락돼 있었음).

- **근본 원인**: `style_store.rs` 의 Blueprint→`HwpxCharShape` 브리지가
  `underline_shape: UnderlineShape::Solid` 를 하드코딩 — Blueprint 에
  읽을 필드가 없었기 때문.
- **수정**: `blueprint/style.rs` 에 `underline_shape` 추가 (`PartialCharShape`
  은 `Option`, `CharShape` 는 `UnderlineShape` 기본 `Solid`) + `merge()` /
  `resolve()` thread, 하드코딩을 `cs.underline_shape` 로 교체.
- **범위 메모**: HWP5→HWPX 변환과 HWPX→HWPX round-trip 경로는 이미
  밑줄/취소선 12종 선종류를 완전히 carry 하고 있었음 (Foundation
  `UnderlineShape`/`StrikeoutShape` + HWP5 decoder + HWPX encoder/decoder
  모두 완비). 이번 수정은 Blueprint 저작 경로 전용.
- **검증**: nextest 통과 (+ 비-Solid 밑줄 carry 회귀 테스트 +
  serde round-trip 테스트). 생성기 `examples/underline_shapes.rs` 산출물이
  SOLID/DASH/DOT/DASH_DOT/DASH_DOT_DOT/LONG_DASH/DOUBLE_SLIM/WAVE 밑줄 +
  SOLID/DASH/DOUBLE_SLIM/WAVE 취소선을 distinct 하게 emit, 한컴 시각 게이트
  PASS (이중선 2줄, 물결 곡선 정상 렌더링).

### Added — HWP5↔HWPX TextArt(글맵시 / `<hp:textart>`) carry

한컴 글맵시(워프된 장식 문자)를 HWP5→HWPX 변환에서 보존 (이전엔 통째로
drop). **Core breaking (additive)**: `Control::TextArt { text, shape,
font_name, font_style, align, line_spacing, char_spacing, width, height,
horz_offset, vert_offset, fill_color, inst_id }` 신설 + `validate.rs` 가
TextArt 를 도형 family 로 검증/그룹 자식 허용.

- **Wire 발견**: TextArt 는 `gso` `ShapeComponent(0x4C)` 의 comp_type
  판별자 `"$tat"` 가 `ShapeTextArt(0x5A)` sub-record 를 감싸는 구조
  (`0x5A` 는 dead 가 아니라 TextArt 전용으로 사용됨 — WIRE_SPEC §10 정정).
  `0x5A` 레이아웃: `pt0..3` (32B) + BSTR text/fontName/fontStyle + u32
  fontType(1=TTF)/textShape(0..54)/lineSpacing/charSpacing/align(0=LEFT) +
  20B shadow tail.
- **textShape enum (55종)**: HWP5 정수 = 글맵시 모양 그리드 위치, HWPX
  문자열은 native 출력에서 추출. 사용자가 작성한 56-textart fixture 로
  전체 테이블 (`PARALLELOGRAM`=0 … `WAVE2`=17 … `DOUBLE_LINE_CIRCLE`=54)
  을 한 번에 확정 (`TEXTART_SHAPE_NAMES`).
- **HWP5 디코더**: `Hwp5ShapeTextArt` 스키마 + `$tat`/`0x5A` 를 4개 gso
  컨텍스트에 ellipse 미러로 thread, `classify_gso_control` 에서
  `Hwp5Control::TextArt` 조기 분류.
- **Projection** → `Control::TextArt` (`textart_shape_name`/`textart_align_name`
  으로 정수→문자열, 범위 밖이면 경고+fallback).
- **HWPX 인코더**: `<hp:textart>` 직접 emit (renderingInfo scaMatrix =
  curSz/orgSz 계산, lineShape NONE, fillBrush, pt0-3, textartPr, shapeComment).
- **HWPX 디코더**: `<hp:textart>` → `Control::TextArt` round-trip (`HxTextArt`).
- **검증**: workspace nextest 통과(+신규 textart 스키마/인코더/테이블 테스트),
  clippy `-D warnings` clean. 사용자 작성 `sample-gso-textart-all.hwp`
  (56 글맵시, 55 distinct shape) 변환 결과 textShape 55종 전부 보존, 0 drop,
  HWP5→HWPX→JSON round-trip 56개 유지. scaMatrix/orgSz/curSz native 일치.

### Added — HWP5↔HWPX 묶음 객체(group / `<hp:container>`) carry (Wave A, flat groups)

한컴 "개체 묶기"로 묶인 그리기 객체(group)를 HWP5→HWPX 변환에서 보존
(이전엔 통째로 drop). **Core breaking (additive)**: 재귀
`Control::Group { children: Vec<Control>, width, height, horz_offset,
vert_offset, inst_id }` 신설 + `validate.rs` 가 비-도형 자식을
`ValidationError::InvalidGroupChild` 로 거부.

- **Wire 발견**: container 는 별도 TagId(0x56) 가 아니라 `gso`
  `ShapeComponent(0x4C)` 의 comp_type 판별자 `"$con"` (line `"$col"` /
  rect `"$rec"` / ellipse `"$ell"` 와 동일 메커니즘). 자식은 한 단계 깊은
  `ShapeComponent` 들이고 각자 기존 shape sub-record(0x4F/0x50/…) 보유.
- **HWP5 디코더**: gso 스코프를 단일-`Option` flat 상태머신에서
  `GsoGroupBuilder`/`GsoChildBuilder` 스택으로 재구성 (기존 `table_stack`
  패턴, architect 리뷰 P0). 자식 geometry 는 자식 ShapeComponent common
  header 에서 파싱 (`Hwp5ShapeComponentGeometry::parse_from_shape_component`:
  x=[4..8], y=[8..12], w=[16..20], h=[20..24] — native 바이트 대조로 도출).
  Wave A 는 flat 만; 중첩 `$con` 은 depth cap(`GSO_GROUP_MAX_DEPTH`) 으로
  warn+degrade.
- **HWPX 인코더**: `<hp:container>` 재귀 emit. 자식 위치는 한컴이
  `<hc:transMatrix>` translation(e3=x, e6=y)으로 잡으므로 그것을 설정
  (identity matrix 면 전부 원점에 겹침 — 시각 검증으로 확인); `<hp:offset>`
  도 동일 값으로 mirror, top-level `<hp:sz>`/`<hp:pos>` 는 자식에서 제거,
  `<hp:curSz>` 는 0×0 (native 일치).
- **HWPX 디코더**: `<hp:container>` → `Control::Group` round-trip.
- **검증**: workspace nextest 통과, clippy clean, 변환 산출물 geometry 가
  native `sample-gso-group.hwpx` 와 byte 일치 (offset/transMatrix/orgSz/
  curSz/container sz·pos), 한컴 시각 게이트 PASS (직사각형+타원 나란히,
  텍스트 "사각형"/"타원" 보존, 겹침 없음). 중첩 그룹 재귀는 Wave B.

### Added — HWP5↔HWPX 중첩 묶음 객체(`$con`-in-`$con`) 재귀 carry (Wave B)

Wave A 가 flat 그룹만 보존한 데 이어, 그룹 안의 그룹(`$con` 이 `$con` 을
다시 품는 구조)을 재귀적으로 보존. Core 변경 없음(`Control::Group` 가
이미 재귀형 `Vec<Control>`); 4개 레이어가 재귀를 타도록 확장.

- **HWP5 디코더**: `GsoGroupBuilder.current_child` 를
  `Option<GsoChildBuilder>` → `Option<GsoActiveChild>` 로 교체.
  `GsoActiveChild::{Leaf(GsoChildBuilder), Nested(Box<GsoGroupBuilder>)}`
  로 중첩 `$con` 은 자식 `GsoGroupBuilder` 를 열어 재귀 (Box 로
  `GsoGroupBuilder → GsoActiveChild → GsoGroupBuilder` 타입 cycle 차단).
  `GSO_GROUP_MAX_DEPTH` 초과 시에만 leaf 로 degrade → Unknown(경고).
- **Projection**: `project_group_child` 에 `Hwp5Control::Group(nested) =>
  project_group_run(...)` arm 추가 (중첩 그룹 자식 재귀).
- **레이아웃 힌트**: `collect_group_child_layout_hints` 가 자식이 중첩
  그룹이면 재귀 — 그러지 않으면 inner 그룹의 텍스트 자식 `<hp:p>` 가
  hint 없이 emit 되어 `paragraph layout hint count underflow` 발생.
- **HWPX 인코더**: `encode_group_child_xml` 에 `Control::Group` early-return
  추가 — `encode_group_to_xml` 로 재귀 후 transMatrix/offset/curSz/sz·pos
  를 다른 자식과 동일하게 처리 (groupLevel 은 재귀 내부에서 +1 baked).
- **HWPX 디코더**: `HxContainer.containers: Vec<HxContainer>` 필드 +
  `decode_container` 가 중첩 container 재귀, `MAX_NESTING_DEPTH`(32) depth
  guard.
- **검증**: workspace nextest 통과, clippy clean. 변환 산출물이 native
  `sample-gso-group-nested.hwpx` 와 구조·geometry byte 일치 (groupLevel
  0/1/2/2/1, inner container identity matrix, ellipse e3=1164/e6=6512, line
  e3=525/e6=13422, 전 자식 curSz 0×0 + top-level sz/pos 없음). 한컴 시각
  게이트 PASS.

### Fixed — SUMMERY/PATH 필드 빈 본문 → 한컴 "낮은 보안 수준 복구" 경고 (#120/#136)

HWP5 → HWPX 변환 시 문서정보(SUMMERY: `$author`/`$title`/`$createtime`/
`$modifiedtime`/`$lastsaveby`)·경로(PATH: `$P$F`) 필드의
`<hp:fieldBegin>…<hp:fieldEnd>` 본문이 빈 `<hp:t/>` 로 나가
한컴이 필드를 미해결로 보고 "낮은 보안 수준으로 복구" 경고 + 첫 열기
빈 placeholder 를 남기던 문제.

- **진단**: byte-diff 로 빈 본문이 유일 구조 차이임을 확인하고, 우리
  출력에 native 값만 주입한 격리 파일로 한컴 실측 (경고 없음) 하여
  본문 값이 원인임을 확정. 한컴 native HWPX 는 본문에 resolved 값을
  캐싱하며 그 값은 HWP5 원본 `BodyText/Section0` ParaText 의
  FieldBegin..FieldEnd span 에 이미 존재. Wave 12n Step 6.6 이 "빈 본문이
  경고를 회피한다" 던 주석은 틀렸음 — 당시 거부된 건 **합성 ISO 날짜**
  (locale 불일치) 였지 verbatim 값이 아니었다.
- **Core (breaking, additive)**: `Control::Field`/`UnknownSummery`/
  `DateCodeField`/`PathField` 에 `display_text: String` 추가
  (`CrossRef::display_text` 와 동일 형태/의미, 빈 문자열 = 없음).
  ClickHere 는 빈 값 유지 (visible placeholder 는 `hint_text`).
- **HWP5 projection**: FieldBegin..FieldEnd span 을 `display_text` 로 누적
  (Hyperlink/CrossRef 와 동일 경로) + DoS cap
  (`MAX_FIELD_DISPLAY_TEXT_UNITS = 4096`, body 가 BSTR command cap 우회).
- **HWPX encoder/decoder**: 본문에 캐싱값 emit + round-trip 복원
  (디코더가 run body 텍스트를 `display_text` 로 환원).
- **검증**: workspace nextest 2,469 통과, 변환 산출물 field body 가 native
  안정값과 일치 (날짜/경로는 출처 파일 차이만), 한컴 시각 게이트
  (경고 없음 + 첫 열기 값 표시) PASS.

### Fixed — HWP5→HWPX 차트 편집 시나리오 (크기 + custom title)

HWP5 → HWPX 변환본의 차트를 `to-json` → 값 편집 → `from-json` 으로
재생성하는 체인을 end-to-end 검증하며 발견된 2건:

- **차트 표시 크기**: projection 이 `ShapeComponentOle` extent (내부
  캔버스, 상수 7200×7200) 를 `hp:sz` 로 사용해 변환 차트가 ≈2.5cm 로
  축소되던 버그. 표시 프레임은 `gso ` CtrlHeader geometry `[16..24]`
  (HWPUNIT) — 한컴 native HWPX 쌍의 `hp:sz 32250×18750` 과 byte 일치
  (`probe_chart_geometry` 로 확정). geometry 우선 + extent fallback 으로
  수정; encoder 의 `orgSz`/`extent` 7200 hardcode 는 native 와 일치라
  유지. e2e 게이트에 native 크기 단언 추가.
- **custom title 형태**: `write_title` 의 최소형
  (`<a:bodyPr/><a:lstStyle/>` + 빈 `<a:rPr lang="ko-KR"/>`) 은 표준
  OOXML 로는 유효하지만 한컴 native 형태와 다름. 사용자 작성
  `sample-chart-title.hwpx` fixture 에서 한컴의 실제 custom title 형태를
  probe 해 byte-convention 미러링으로 교체: fully-attributed `bodyPr`,
  `lstStyle` 제거, `pPr/defRPr`+`rPr` 에 `sz=1400`+함초롬돋움 4종,
  trailing `endParaRPr`. 단위 게이트
  `write_title_mirrors_hancom_native_form` 추가.

시각 검증: 변환 → 판매 값 `[10,3.5,1.5,1.2]`→`[1,2,3,4]` 편집 + 제목
추가 → 재생성본이 한컴에서 정상 크기 프레임 + 제목 + 분리형 파이
(explosion 25% 보존) 로 렌더링 확인.

### Refactor (tasks #88/#89/#90/#91/#92/#94/#95) — Wave 12 누적 부채 정리

기능 영향 0 (모든 step 에서 workspace nextest 전체 통과로 검증),
코드 구조만 개선:

- **#94** CTRL_ID 상수 33개 선언 (4개 파일, 9개 중복 그룹) →
  `smithy-hwp5/src/ctrl_ids.rs` 단일 모듈 25개 canonical 상수.
  drift 이름 정리: `SECTION_DEF`→`SECD`, `CROSSREF`→`FIELD_CROSSREF`,
  `FIELD_INLINE_PAGE`/`INLINE_AUTONUM`→`ATNO`,
  `INDEXMARK_INLINE`→`INDEXMARK`.
- **#95** Wave 12i/12k 공유 split-leader / length-prefixed UTF-16 BSTR
  파서를 `parse_split_leader_utf16` / `parse_length_prefixed_utf16`
  helper 로 통합 (Dutmal main/sub, IndexMark primary/secondary).
  Compose (12j) 는 length prefix 가 없는 다른 wire 라 의도적으로 제외.
- **#88** `parse_name_subrecord` 의 `Option<Option<String>>` →
  `ClickHereNameSubrecord { Named / Unnamed / Malformed }` named enum.
- **#89** `Hwp5ClickHereControl::parse` → `Result<_, ClickHereParseError>`
  (`TruncatedHeader` / `CommandTooLong` / `TruncatedCommand` /
  `CommandSyntax`) — DroppedControl warning 에 구체 사유 표기.
- **#90** `handle_top_level_record` ~425줄 → 71줄 dispatcher + 4 helper
  (`try_intercept_memo_cluster`, `try_attach_clickhere_name`,
  `handle_ctrl_header`, `handle_eqedit_record`).
- **#91** `project_paragraph_with_images_structural` ~225줄 → 135줄 +
  3 helper (`project_visible_text_segment`, `start_field_from_marker`,
  `drain_unconsumed_paragraph_queues`).
- **#92** `smithy-hwpx/encoder/section.rs` 5,481줄 → 3,765줄 (잔여 중
  ~2,760줄은 root 테스트 모듈). 기존 `section/table.rs` 선례 패턴
  (`mod X;` + `use super::*;` + `pub(super) fn`) 으로 8개 family 모듈
  신설: `field`(694) / `section_pr`(280) / `header_footer`(245) /
  `memo`(169) / `chart`(152) / `picture`(139) / `equation`(66) /
  `typography`(64). root 재수입으로 호출처/테스트 무변경, 본문
  verbatim 이동. 같은 crate 내 모듈 분리라 runtime 성능 영향 0.

### Docs (task #72) — HWP5 wire spec (HwpForge-internal)

Added `crates/hwpforge-smithy-hwp5/HWP5_WIRE_SPEC.md` — code-grounded
HWP5 binary format documentation capturing Wave 12 series discoveries
that the public KS X 6101 / Hancom spec omits:

- ParaHeader `[18..22]` `instance_id` field (Wave 12p Step 1)
- Family-aware CtrlHeader `instance_id` offsets (fn/en at 16,
  gso/tbl/eqed at 36, Wave 12p Step 5)
- ParaShape `property1` bits 25-27 are **zero-based ordinal** cap=6
  (Wave 12p #121)
- Style "개요 N" / "Outline N" outline-level override for levels 7~9
  (Wave 12q #122)
- Default `linesegarray` synthesis values for paragraphs lacking
  `ParaLineSeg` (Wave 12p #123)
- `editable` attribute per `FieldType` (Wave 12p #124)
- BSTR length-prefix allocation caps (`MAX_*_UNITS` family, Wave 12 +
  task #86)
- CTRL_ID magic constant inventory across 24 entries (3 source files,
  prelude to task #94 consolidation)
- SummaryInformation OLE2 PropertySet layout incl. Hancom custom PIDs
  `0x14`/`0x15` (Wave 12o)
- Cross-reference Command 8-parameter Hancom-canonical wire (Wave 12m,
  ADR-004)

Each topic links back to the canonical source file/symbol so future
contributors can grep this file before re-running probes.

Linked from `crates/hwpforge-smithy-hwp5/AGENTS.md` for discoverability.

### Wave 12q (task #122) — outline level 7~9 recovery via Style "개요 N" linkage

HWP5 `ParaShape.property1` bit 25-27 은 3 bits 만 표현 가능 (cap=6).
한컴 native 는 outline level 7~9 를 Style record 의 한국어 이름
"개요 N" (N-1 = HWPX level) 으로 표현. 변환 후 paraPr 의 heading_level
을 Style "개요 N" 매핑으로 override 하여 native parity 달성.

#### Fixed

- `hwpforge-smithy-hwp5`: `apply_outline_style_level_overrides()` —
  ParaShape 변환 후 Style 테이블에서 "개요 N" / "Outline N" 패턴 매칭
  → para_shape_id 가 가리키는 paraPr 의 heading_level 을 N-1 로
  override. 한컴이 wire cap=6 으로 저장한 level 7/8/9 가 정확히 emit.
- override 는 silent (warning 없음) — semantic loss 가 아닌 lossless
  level recovery 이므로 audit warning_count 에 영향 없음.

#### Added — `hwpforge-smithy-hwpx`

- `HwpxStyleStore::para_shape_mut()` — Wave 12q level override 용
  mutable accessor.

#### 검증

- `sample-outline-9levels.hwp` (사용자 작성 native fixture) 변환 결과:
  - paraPr id 2~8 level 0~6: native parity ✅
  - paraPr id 18/16/17 level 7/8/9: hp10 namespace switch wrap 없이도
    한컴이 정상 인식 ✅
- 한컴 시각 검증: 1~7수준 `1./가./1)/가)/(1)/(가)/①`, 8수준 `㉠`,
  9~10수준 marker 없음 (native 와 byte-identical 매칭).
- Codex(architect) 검토 반영: paraPr id 자체에 의존 안 함, heading_type
  이 Outline 인 paraPr 만 override.

ADR 참조: `.docs/architecture/adr/ADR-006-wave12p-task122-outline-level-7-9-diagnostics.md`

### Wave 12p tasks #121/#123/#124 — outline/linesegarray/editable fidelity

#### Fixed (#121) — `hwpforge-smithy-hwp5`

- `Hwp5RawParaShape::heading_level()` 이 `saturating_sub(1)` 로 wire 를
  1-based 로 잘못 가정 → 0-based 그대로 사용. HWP5 spec bit 25-27 (3
  bits) 가 zero-based ordinal 0~7. HWPX `<hh:heading level>` 도
  zero-based. Codex(architect) 검토 확인.
- 검증: native `sample-outline-9levels.hwpx` 의 paraPr id 2~8 level
  0~6 가 우리 emit 와 완전 일치.

#### Fixed (#123) — `hwpforge-smithy-hwp5`

- `layout_hint_patch::write_linesegarray()`: HWP5 source 의
  `ParaLineSeg` (tag 0x45) record 가 없는 paragraph 도 default lineseg
  로 `<hp:linesegarray>` 항상 emit. 이전엔 누락 → 한컴이 "낮은 보안
  수준 복구" 경고.
- Default 값 (native fixture 도출): `vertsize=1000`, `textheight=1000`,
  `baseline=850`, `spacing=600`, `horzsize=42520` (A4 content width),
  `flags=393216`. `vertpos=0` — 한컴이 paragraph 순서 따라 재계산.

#### Fixed (#124) — `hwpforge-foundation` + `hwpforge-smithy-hwpx`

- `FieldType::hwpx_editable()` 신규 method — `Author`/`Title` → `false`,
  `LastSavedBy`/`CreatedTime`/`ModifiedTime` → `true` (한컴 native
  fixture 도출). 이전엔 모든 SUMMERY field 가 `editable="1"` 강제됨.
- `build_summery_run_xml_raw()` 시그니처 확장 (`editable: bool` 파라미터).
- 검증: `sample-field-docsummary.hwp` 변환 결과 8/8 SUMMERY field 가
  native editable 값과 일치.

### Wave 12p — cross-ref target element instance ID carry (BREAKING, partial)

Wave 12m 시각 검증의 잔존 이슈 (HWP5 변환본의 footnote/endnote/figure/
table/equation cross-ref 가 한컴에서 `?` 표시) 해소 작업. **Steps 1-4
완료**, Step 5 (HWP5 → HWPX target ID 매칭 알고리즘) 는 후속 작업.

ADR 참조: `.docs/architecture/adr/ADR-005-wave12p-target-element-instance-id-carry.md`

#### Breaking — `hwpforge-foundation`

- `RefContentType::BookmarkName` variant **부활** (Wave 12m fixup
  regression revert). Bookmark N2 매핑 native 일치 보정:
  - N2=0 → Page, N2=1 → Number (책갈피 본문/번호)
  - N2=2 → BookmarkName (책갈피 이름), N2=3 → UpDownPos
- `Display` 는 `Contents` 와 `BookmarkName` 모두 `OBJECT_TYPE_CONTENTS`
  emit (한컴 wire 일치, 의미는 RefType + N2 wire code 에서 결정).

#### Breaking — `hwpforge-core`

- `Image` 에 `pub inst_id: Option<u64>` 신규.
- `Table` 에 `pub inst_id: Option<u64>` 신규.
- `Control::Equation` variant 에 `inst_id: Option<u64>` 신규.
- 모두 `#[non_exhaustive]` + `Default::default()` 호환이라 builder
  사용 시 caller 코드 변경 불필요.

#### Added — `hwpforge-smithy-hwp5`

- `Hwp5ParaHeader.instance_id: u32` (Step 1a) — HWP5 ParaHeader `[18..22]`
  의 u32 LE 추출. outline cross-ref target ID source.
- `Hwp5NestedSubtree.instance_id: u32` (Step 1b) — Header/Footer/Footnote/
  Endnote CtrlHeader trailer.
- `Hwp5Table.instance_id: u32` (Step 1c-1) — Table CtrlHeader trailer.
- `Hwp5EquationControl.instance_id: u32` (Step 1c-2) — `eqed` CtrlHeader
  trailer.
- `Hwp5ImageControl.instance_id: u32` (Step 1c-3) — `gso ` CtrlHeader
  trailer (via NestedSubtreeContext + InlineGsoContext).
- Helper `extract_ctrl_header_trailer_instance_id(&[u8]) -> u32` —
  마지막 8 bytes 중 first 4 의 u32 LE.

#### Changed — projection / encoder

- HWP5 projection: 6종 target element 의 instance_id 를 Core inst_id
  로 통과 (`0` 은 unset 으로 `None` 정규화).
- HWPX encoder: `<hp:footNote instId="N">`, `<hp:endNote instId="N">`,
  `<hp:equation id="N">`, `<hp:tbl id="N">`, `<hp:pic id="N">` 모두
  Core inst_id 가 있으면 사용, 없으면 sequential `generate_instid`
  fallback.

#### Deferred (Step 5, 후속 작업)

Probe 결과 (`crates/hwpforge-smithy-hwp5/examples/probe_instance_id.rs`):
HWP5 binary 의 footnote/endnote CtrlHeader payload 는 **자기 자신의
instance ID 를 carry 하지 않음** (한컴이 save 시 session counter 로
synthesize). cross-ref Command 는 target ID 를 carry 하지만 target
element 의 ID 는 wire 에 없음.

결과:
- **Forged HWPX path** (caller 가 `Image::new(...).inst_id = Some(N)`
  설정): ✅ 정상 동작 (encoder 가 wire 에 emit)
- **HWP5 → HWPX 변환 path**: ❌ cross-ref 가 여전히 `?` 표시 (target
  element inst_id = None, encoder skip)

Step 5 알고리즘 (ADR-005 §"Step 5 Algorithm" 에 plan 상세):
1. Section 별 cross-ref Command target_id 들을 pre-scan + RefType
   별 grouping
2. Doc 순서대로 target element 에 그 IDs 할당 (1:N dedup 처리)
3. cross-ref Command 는 as-is (HWP5 wire 그대로)

Step 5 land 시 HWP5 conversion path 도 cross-ref 정상 동작.

#### 신규 examples / probe

- `crates/hwpforge-smithy-hwp5/examples/probe_instance_id.rs` — debug:
  HWP5 fixture 의 projection inst_id 확인

#### 검증

- `cargo nextest run --workspace --all-features`: 2,463 passed, 2 skipped
- `cargo clippy --workspace --all-features --all-targets -D warnings`: clean
- 한컴 시각 검증: forged path 만 동작 확인 (HWP5 conversion path 는
  Step 5 대기)

### Wave 12m fixup — fieldid `%xrf` magic constant + `RefContentType::BookmarkName` 폐기 (BREAKING)

Wave 12m Phase 2 시각 검증 (한컴오피스 한글) 에서 cross-ref body 가 `?`
로 표시되는 회귀 발견 → 2단계 research (1차 + 재검증) 후 두 결정적
차이 fix.

#### Breaking — `hwpforge-foundation`

- `RefContentType::BookmarkName` variant **폐기**. OWPML 표 156 은
  ContentType 을 4종 (PAGE / NUMBER / CONTENTS / UPDOWNPOS) 만 정의
  하며, 책갈피의 경우 `Contents` 가 "책갈피 내용/이름" 의미를 내포함
  (spec 명시: "참조 대상의 제시 내용 또는 책갈피의 경우, 책갈피 내용").
  Wave 12m Phase 2 Step 3 의 `BookmarkName` enum + `OBJECT_TYPE_BOOKMARK_NAME`
  emit 은 spec 외 invented string 으로 한컴 인식 실패. HWP5 N2 code 2
  for Bookmark → `RefContentType::Contents` 로 매핑.

#### Fixed — `hwpforge-smithy-hwpx`

- `build_crossref_run_xml` 의 `fieldid` 가 sequential counter
  (`1_828_000_000 + N`) 가 아니라 Hancom `%xrf` ASCII magic constant
  (`0x25787266` = `628_650_598`) 로 emit. native HanCom-authored HWPX
  11 sample 전수 분석 (n=11) 에서 모든 CROSSREF 가 동일 magic
  constant 사용 확인. per-instance identity 는 `id` (begin_id) 가
  carry. HwpForge 다른 native byte-identical 검증된 field 와 일관:
  ClickHere=`%clk`, SummeryField=`%smr`, PathField=`%pat`.
- HWP5 projection boundary `decode_hwp5_crossref_content_type`: Bookmark
  + N2=2 → `Contents` (이전 `BookmarkName`). 의미는 RefType-상대적
  ("책갈피 이름").
- HWPX encoder reverse boundary `ref_content_type_wire_code`: Contents
  → 모든 RefType 에서 N2=2 (Bookmark="책갈피 이름", Figure/Table/Eq/
  Outline="캡션 내용").

#### Regression gates

- `crossref_builders_emit_nonzero_matching_fieldid`: fieldid 기댓값
  `1828000000` → `628650598` 변경. assertion 메시지 "non-zero derived"
  → "`%xrf` magic constant" 로 강화.

#### 시각 검증 결과 (한컴오피스 한글 직접 열기)

| 케이스 | 이전 | 현재 |
|--------|-----|-----|
| Bookmark+Page (Point) | `?` | `1` ✅ |
| Bookmark+Page+Hyperlink (Point) | `?` | `1` ✅ |
| Bookmark+Contents (Span) | (미검증) | 본문 표시 ✅ |
| Bookmark+Contents (Point) | `?` | `?` (정상 — 본문 없음) |

Point bookmark 의 Contents reference 는 본문이 없어 한컴이 `?` 표시 —
우리 wire 형식은 정상. caller 가 SpanStart/SpanEnd 책갈피를 만들면 한컴
이 범위 안 본문을 표시.

#### 추가 발견 (별도 task)

- `<hp:endNote>` / `<hp:footNote>` / `<hp:figure>` / `<hp:table>` 등
  cross-ref target 이 될 수 있는 elements 에 `instId` attribute 가
  emit 되지 않음. 한컴 cross-ref Command 의 `?#<id>` target lookup 이
  실패하여 endnote/footnote/caption cross-ref 가 `?` 표시. Wave 12m
  범위 밖 — task #134 로 분리.

#### 새 examples (시각 검증용)

- `crossref_matrix.rs` — RefType × ContentType 8-permutation HWPX
- `crossref_with_target.rs` — Point bookmark + cross-ref 4종
- `crossref_span_bookmark.rs` — Span bookmark + Contents reference 검증

#### 검증

- `cargo nextest run --workspace --all-features`: 2,463 passed, 2 skipped
- `cargo clippy --workspace --all-features --all-targets -D warnings`: clean

### Wave 12m Phase 2 — `Control::CrossRef` HWP5 leg structured upgrade (BREAKING)

HWP5 `%xrf` 상호참조 필드를 `Control::Unknown(tag="hwp5.crossref")`
surrogate (Wave 12 이전 lossy 경로) 대신 typed `Hwp5CrossRefControl`
schema → boundary functions → native `Control::CrossRef` 로 end-to-end
carry. 사용자 시각 검증 가능한 12 한컴 native fixture
(`tests/fixtures/hwp5/crossref/`) e2e 회귀 게이트 동반.

ADR 참조: `.docs/architecture/adr/ADR-004-wave12m-crossref-structured-upgrade.md`
(내부 문서).

#### Breaking — `hwpforge-foundation`

- `RefType` (enum) — `#[repr(u8)]` 제거 + `#[non_exhaustive]` 추가.
  신규 variants: `Footnote`, `Endnote`, `Outline`, `Unknown(u8)`.
  기존 `TryFrom<u8>` impl 제거 (boundary 책임을 smithy-hwp5 로 이동).
  `RefType` 크기 1 byte → 2 bytes (variant + discriminant).
- `RefContentType` (enum) — `#[repr(u8)]` 제거 + `#[non_exhaustive]`
  추가. 신규 variants: `BookmarkName`, `Unknown(u8)`. 동일 크기 변화.

#### Breaking — `hwpforge-core`

- `Control::CrossRef` — `target_name: String` 필드를 type-safe
  `target: RefTarget` 로 교체. 신규 `RefTarget` 열거형 (`Name(String)`
  for Bookmark, `SystemId(u64)` for `#<id>` refs, `Raw(String)` for
  unparseable fallback). `Control::cross_ref(...)` 생성자 시그니처도
  연동 변경.
- `Control::CrossRef` 에 `display_text: String` 필드 추가. HWPX wire 는
  `<hp:fieldBegin>` / `<hp:fieldEnd>` 사이 visible run 으로 display
  text 를 embedding 한다. HWP5 `%xrf` wire 는 display text 를 직접
  carry 하지 않고 ParaText 본문에 풀어 두지만, projection 이
  FieldBegin..FieldEnd span 을 읽어 이 필드에 채워 넣는다. 빈 문자열은
  "display text 없음" 의미.
- JSON 호환성: 의도된 clean break. 기존 `target_name` / surrogate
  JSON 도 마이그레이션 없이 변경.

#### Added — `hwpforge-smithy-hwp5`

- `schema::section::Hwp5CrossRefControl` — 9-field wire-fidelity 구조
  (`ctrl_id`, `command_raw`, `target_raw`, `ref_type_code`,
  `content_type_code`, `hyperlink_code`, `param4_raw`,
  `header_flag_raw`, `trailer_begin_id`, `trailer_field_id`).
  `MAX_CROSSREF_COMMAND_UNITS = 1024` cap 으로 할당 전 차단. 13 unit
  tests (security gates 포함).
- `decoder::section::Hwp5Control::CrossRef(Hwp5CrossRefControl)` variant
  + `CTRL_ID_FIELD_CROSSREF = 0x2578_7266` ("%xrf") dispatch. malformed
  payload 는 `Hwp5Warning::DroppedControl{control: "crossref"}` 로
  떨어진다 (silent skip 금지).
- `projection::` Boundary functions: `decode_hwp5_crossref_ref_type`
  (0~6 + `Unknown(u8)`), `decode_hwp5_crossref_content_type`
  (RefType-relative: Bookmark=1→Contents/2→BookmarkName, 그 외는
  1→Number/2→Contents, 3→UpDownPos), `decode_hwp5_crossref_target`
  (Bookmark→Name, `#<u64>`→SystemId, 실패→Raw).
- `semantic::Hwp5SemanticControlKind::CrossRef` 신규 variant +
  `semantic_adapter` 매핑.

#### Added — `hwpforge-smithy-hwpx`

- Reverse boundary helpers: `ref_type_wire_code(&RefType) -> u8`,
  `ref_content_type_wire_code(&RefType, &RefContentType) -> u8`,
  `crossref_target_for_command(&RefTarget, &RefType) -> String`.
- `build_crossref_run_xml` 가 Hancom-canonical 8-parameter form
  (`Fiexde=1` / `Prop=0` / `Command=?{target};N1;N2;N3;0;` /
  `RefPath=?{target};` / `RefType` / `RefContentType` / `RefHyperLink` /
  `RefOpenType=HWPHYPERLINK_JUMP_CURRENTTAB`) 직접 emit. 시그니처:
  1번 인자 `target_name: &str` → `target: &RefTarget` (target 종류별
  정확한 emit).

#### Changed — `hwpforge-smithy-md`

- Markdown encoder 의 `Control::CrossRef` arm 이 `display_text` 가
  비어 있지 않으면 그대로 emit (사용자가 본 visible 문자열). 비어
  있을 때만 `target.as_display()` 로 fallback (Name → "bookmark1",
  SystemId → "#5") 하고 `[...]` anchor brackets 로 wrap.

#### Removed (clean break)

- `hwpforge-smithy-hwp5::projection::HWP5_CROSSREF_UNKNOWN_TAG`
  constant (`"hwp5.crossref"`)
- `parse_crossref_target_name`, `encode_hwp5_crossref_unknown_data`
  (smithy-hwp5 projection)
- `parse_hwp5_crossref_unknown_data`, `Hwp5CrossRefUnknownPayload`
  (smithy-hwpx encoder), `HWP5_CROSSREF_UNKNOWN_TAG` const
- HWPX encoder 의 `Control::Unknown { tag == HWP5_CROSSREF_UNKNOWN_TAG }`
  branch — projection 이 더 이상 emit 하지 않음.
- `build_hwp5_crossref_run_xml` 는 fieldid 회귀 게이트 목적으로
  `#[cfg(test)]` 유지.

#### Decoder — HWPX (Wave 12m Phase 2 Step 4)

- `<hp:fieldBegin type="CROSSREF">` 가 `RefPath` 의 `#<u64>` 를
  `RefTarget::SystemId` 로 정규화, Bookmark refs (`ref_type ==
  Bookmark`) 는 `RefTarget::Name`, 그 외는 `RefTarget::Raw` fallback.
  `display_text` capture 는 후속 wave (FieldBegin/FieldEnd 사이
  sibling run 누적).

#### Regression gates

- 신규 (Step 2): 12 fixture parse 회귀 게이트
  (`schema::section::tests::crossref_parse_*`) — 모든 wire 변형의
  Command 파싱 검증.
- 신규 (Step 6): `hwp5_to_hwpx_crossref_12_fixture_matrix_emits_typed_control_and_canonical_wire`
  — 12 한컴 native fixture 의 end-to-end 변환 검증
  (RefType / Hancom-canonical 8-param form / fieldid 양수).

#### 검증

- `cargo nextest run --workspace --all-features`: 2,463 passed, 2 skipped
  (Wave 12n Step 6 baseline 2,445 → 2,463, Wave 12m Phase 2 +18 신규
  게이트).
- `cargo clippy --workspace --all-features --all-targets -D warnings`:
  clean.

### Wave 12n Step 6 — `%pat` PATH 필드 lossless HWPX carry

Wave 12n Step 3 에서 placeholder 로 남겨두었던 `Control::PathField` 의
HWPX wire 매핑을 native 형식으로 완성. #120 (한컴 "낮은 보안수준 복구"
경고의 가장 강한 트리거) 해소.

#### Fixed — HWPX encoder

- `Control::PathField { command }` arm 이 더 이상 LOSSY SUMMERY
  surrogate 를 emit 하지 않고 `<hp:fieldBegin type="PATH" name=""
  editable="0" dirty="0" zorder="-1" fieldid="628121972" metaTag="">`
  + `<hp:stringParam name="Format">$P|$F|$P$F</hp:stringParam>` 를
  직접 emit. 한컴 native 와 byte-identical (단 `id` 카운터 제외).
  Body 는 empty — 한컴이 저장 시 `$P` / `$F` / `$P$F` 를 실제
  on-disk 경로 / 파일명으로 재평가.

#### Added — HWPX decoder

- `<hp:fieldBegin type="PATH">` 인식 분기 신설. `Format` param 우선,
  없으면 `Command` 로 fallback. `PathFieldCommand::from_wire(&cmd)` 로
  typed variant (`Path` / `FileName` / `PathAndFileName`) 또는
  `Unknown(s)` 로 carry.

#### Regression gates (+2, -1)

- 신규: `pathfield_emits_native_path_wire` — encoder wire 형식 검증
  (`type="PATH"` / `fieldid="628121972"` / `editable="0"` / `Format` 사용
  / `Property` 미사용)
- 신규: `pathfield_roundtrip_preserves_command_lossless` —
  `PathAndFileName` / `Path` / `FileName` 세 variant 모두 lossless
  round-trip 검증
- 신규: `pathfield_unknown_command_roundtrips_as_unknown` — 비표준
  `$X` Command 도 `PathFieldCommand::Unknown("$X")` 로 carry
- 제거: 기존 `lossy_roundtrip_pathfield_becomes_unknown_summery` —
  이제 lossless 이므로 의도 상충. lossy 사실을 확정하던 단언이 모두
  lossless 단언으로 대체됨.
- 제거: 기존 `lossy_pathfield_emits_summery_with_raw_command` —
  마찬가지 이유.

검증:
- workspace nextest: 2,444 → **2,445 passed** + 2 skipped (+3 신규, -2 폐기)
- end-to-end: `sample-field-docsummary.hwp` 변환 결과의 PATH 필드 wire
  가 한컴 native `sample-field-docsummary.hwpx` 와 byte-identical
  (single field id 카운터 차이만 존재).

#### Note — #120 보안 경고 보조 트리거 잔존

PATH 필드 오분류는 #120 의 가장 강한 트리거이나 #121 (outline level
off-by-1), #123 (linesegarray 누락), #124 (editable bit 강제) 등 다른
트리거가 잔존. Step 6 만으로 보안 경고가 완전 사라지는지는 사용자
시각 검증 (한컴 열기 후 확인) 에 의존.

### Known Issues — Wave 12p/12q 종료 후 잔존

#### #120 — "낮은 보안 수준 복구" 경고 (HWP5 → HWPX SUMMERY 필드)

**증상** (2026-06-09 시각 검증, `converted-field-docsummary-wave12p-final.hwpx`):

1. 한컴에서 변환 결과 파일을 열 때 "현재의 낮은 보안 수준으로 문서에
   손상을 줄 수 있는 내용을 복구하였습니다" 경고 dialog 표시.
2. 처음 열 때 모든 SUMMERY 필드 (`$author`, `$title`,
   `$modifiedtime`, `$lastsaveby`, `$createtime`) 의 body 가 빈 placeholder
   (`[문서 정보 시작][문서 정보 끝]`) 로 표시.
3. 사용자가 **저장하면** 한컴이 자체적으로 metadata 를 evaluate 해서
   `hanyul` / 날짜 등 실제 값으로 채워 다시 표시.

**Native fixture (`sample-field-docsummary.hwpx`) 와의 차이**:
- native: 경고 없음 + 처음 열 때부터 SUMMERY 값 표시.
- ours: 경고 + 처음 열 때 빈 표시 → 저장 후 update.

**Wire 비교 결과 (`/tmp/docsum_diff` 의 grep)**:

native 와 우리 emit 모두 `<hp:fieldBegin>` 와 `<hp:fieldEnd>` 사이의
`<hp:t>` body 에 실제 값을 채움 (예: native field #2 `hanyul`, ours
field #2 `hanyul`). **body 자체에는 차이 없음**. 그럼에도 한컴이 우리
파일은 빈 placeholder 로 표시 — 한컴이 다른 구조적 차이를 트리거로
잡는 듯.

**알려지지 않은 추가 트리거 (Wave 12p/12q 후 잔존)**:
- `<hp:fieldBegin>` 의 `id`/`fieldid`/`metaTag` attribute 의 값/조합이
  native 와 미세하게 다른가? (예: hardcoded `fieldid="628321650"` vs
  native 의 paragraph-별 다른 값)
- ParaText (paragraph 본문 text) 의 byte sequence 가 다른가?
- `dirty="0"` 이지만 한컴이 dirty 로 인식하는 다른 signal?
- 단일 paragraph 안에 여러 SUMMERY field 가 연속될 때 우리 emit 의
  paragraph 구조가 다른가? (native field #5 는 paragraph 안에 여러
  `<hp:t>` 가 split 되어 있음)

**완화책 — 후속 wave 에서 진단 필요**:
1. native `sample-field-docsummary.hwpx` 와 우리 emit 의 `section0.xml`
   byte-level diff 로 트리거 specific 식별.
2. 한컴 "낮은 보안 수준" warning 의 정확한 트리거 조건 reverse-engineering.
3. ZIP container 의 file 순서 / mimetype 처리 차이 가능성도 검토.

**user impact**: 사용자가 변환 결과를 저장 한 번 하면 모든 값이
올바르게 표시되므로 **functional impact 는 크지 않음**. 단 첫 인상이
"빈 필드 + 경고" 라 confusing — 후속 wave 에서 trigger specific 한 fix
필요.

상태: **investigation deferred** — 후속 wave 에서 byte-level diff 진단
필요.

#### #33 — Wave 5 gap C: 별도 `<hm:masterPage>` element carry

**상태**: **investigation deferred** — Windows 한컴 fixture 필요.

**컨텍스트** (2026-06-09 macOS 한컴 fixture 진단):

macOS 한컴은 페이지 테두리 / 머리말 / 꼬리말 / 쪽 배경 등을 모두
`<hp:secPr>` 안에 `<hp:pageBorderFill>` + `<hp:header>` + `<hp:footer>`
로 inline emit. 별도 `<hm:masterPage>` element 는 만들지 못함
(`masterPageCnt="0"`, 별도 `MasterPage/` 디렉토리 / `Contents/masterPageN.xml`
파일 없음).

진단 결과: **현재 HwpForge 변환이 이미 macOS 한컴 시나리오 (페이지
테두리 + 머리말/꼬리말) 를 정확히 처리** — `<hp:pageBorderFill>` 3종
(BOTH/EVEN/ODD), `<hp:header applyPageType="BOTH">`, `<hp:footer ...>`
모두 native parity (id 번호만 +1 차이).

#33 의 진짜 scope 인 **별도 `<hm:masterPage>` element** (페이지별로
다른 master template 같은 advanced 기능) 는 macOS 한컴이 만들지 못해
fixture 확보 불가. Windows 한컴 환경에서 `<hm:masterPage>` 정의된
.hwp + native .hwpx fixture 확보 후 진단/구현 가능.

**user impact 작음**: 일반 사용자가 사용하는 페이지 테두리/머리말/꼬리말
시나리오는 이미 동작. advanced `<hm:masterPage>` 시나리오는 한컴 자체
가 흔한 use case 가 아닌 듯 (macOS 빌드에서 누락 가능).

### Wave 12o-fixup — Codex(architect) Top-5 리뷰 4건 + 종료 정직성

Codex(architect) Wave 12o post-completion 리뷰에서 발견된 P0/P1 4건의
fast-follow fix + 종료 노트 정직성 보정. Wave 12o 종료 commit (`8aa47f9`)
은 revert/amend 하지 않음 (감사 추적성 보존).

#### Security (Top-1 P0)

- `hwpforge-smithy-hwp5::schema::summary_info::parse_summary_information`
  의 `sec_start + 8` unchecked `usize` add 가 32-bit / wasm 타깃에서
  `u32`-derived `sec_start >= 0xFFFFFFF8` 일 때 wrap 후 panicking
  슬라이스 인덱스로 진입할 수 있었음. `checked_add` +
  `bytes.get(..)` 패턴으로 항상 `Hwp5Error::RecordParse` 경로 보장.
  악성 .hwp 만으로 트리거 가능한 DoS 봉쇄.

#### Fixed (Top-2 P0 — data carry)

- HWPX decoder 가 한컴 emit `<opf:meta name="date">2026년 …</opf:meta>`
  값을 `extras["date"]` 로 carry 하지만 HWPX encoder 의 typed-collision
  guard 가 이를 drop 해서 HWPX → HwpForge → HWPX 1-cycle round-trip
  에서 silent data loss 발생. typed-collision 리스트에서 `date` 만
  예외 처리하고 9-slot canonical 위치에서 `extras["date"]` 값을 그대로
  emit. `creator` / `subject` 등 진짜 typed slot 은 계속 drop.

#### Fixed (Top-4 P1 — API 일관성)

- `Hwp5Decoder::decode` (canonical public entry) 가 `decode_intermediate`
  에서 생성한 `intermediate.metadata` 를 받았지만 projection 결과
  `document` 에 `set_metadata` 하지 않아 silently drop. 한 줄 추가로
  `decode_hwp5_with_images` / `hwp5_to_hwpx_bytes` 와 일관성 확보.

#### Fixed (S3 namespace guard — P1 신규 발견)

- `hwpforge-smithy-hwpx::decoder::metadata::enforce_namespace` 가
  `if let Some(p) = name.prefix()` 패턴이어서 unprefixed `<title>` /
  `<meta>` (prefix 누락) 가 S3 (namespace confusion) 가드를 통과.
  `match` 표현으로 변환해서 prefix 누락 케이스도 reject. 하스타일
  metadata 슬롯 shadow 가능성 차단.

#### Regression gates (+3 tests)

- `summary_info::section_start_past_eof_rejected` — Top-1
- `decoder::metadata::date_extras_roundtrip_preserves_value` — Top-2
- `decoder::metadata::unprefixed_element_inside_metadata_rejected` — S3

Workspace nextest: 2,441 → **2,444 passed** + 2 skipped.

#### Process integrity (Top-5)

- Wave 12o 종료 commit 의 G6/G7 PASS 표기 뒤에 G2 (native byte-parity
  xmllint diff), G4 (HWP5→HWPX e2e 자동 게이트), G8b (저장 후 fallback
  거동) 가 미평가 상태로 묻혀 있던 사실을 CLAUDE.md 종료 노트에
  명시. 후속 wave 에서 자동 게이트로 승격 약속.

#### Visual verification example

- 신규 `crates/hwpforge-smithy-hwpx/examples/probe_date_carry_wave12o_fixup.rs`
  — 한컴 저장본 HWPX 를 HwpForge 가 decode → encode → 재decode 한 결과
  metadata 가 동일한지 자동 검증 + content.hpf 비교 명령 출력.

### Wave 12o Phase 3 — HWP5 SummaryInformation OLE2 PropertySet decoder

#### Added — HWP5 decoder

- New module `crate::schema::summary_info` parses the
  `\x05HwpSummaryInformation` OLE2 PropertySet stream (standard MS
  Office layout) into a populated [`Metadata`](Phase 0) struct.
- `PackageReader` now reads the summary stream alongside the other
  OLE2 sub-streams. Missing or undecodable summary downgrades to
  `Metadata::default()` with a `ParserFallback` warning (never fatal).
- `DecodedHwp5Intermediate` gains a `metadata` field; both
  `decode_hwp5_with_images` and `hwp5_to_hwpx_bytes` forward it into
  the projected `Document` via `Document::set_metadata`.

#### Wire mapping (PropertySet PIDs → Core Metadata)

- PID 2 (TITLE) → `title`
- PID 3 (SUBJECT) → `subject`
- PID 4 (AUTHOR) → `author`
- PID 5 (KEYWORDS) → `keywords` (semicolon-split)
- PID 6 (COMMENTS) → `description`
- PID 8 (LASTAUTHOR) → `last_saved_by`
- PID 0x0C (LASTSAVEDTIME, VT_FILETIME) → `modified` (ISO 8601)
- PID 0x0D (CREATEDTIME, VT_FILETIME) → `created` (ISO 8601)
- PID 0x14 (Hancom custom date display string) → `extras["date"]`
- PID 0x15 (Hancom custom app name) → `extras["appname"]`
- Unknown PIDs → `extras["pid_<hex>"]` (preserves wire bytes)

#### Security (Wave 12o architect review §11.2 M3/M4 + §11.4 S2/S7)

- M3 — slots into Schema stage (no new pipeline stage).
- M4 — FILETIME → ISO 8601 hand-rolled (no `chrono` dependency added).
  Sub-second precision discarded to match Hancom wire (seconds).
  Year > 9999 and FILETIME = 0 yield `None`.
- S2 — property table monotonic-offset enforcement rejects classic
  PropertySet payload-cycle DoS.
- S7 — explicit UTF-16LE BOM strip from VT_LPWSTR payloads.
- Allocation caps: per-property body ≤ 64 KiB, property count ≤ 256.

### Wave 12o Phase 2 — HWPX decoder `content.hpf` metadata parser + XXE/DoS defenses

#### Added — HWPX decoder

- New module `crate::decoder::metadata` parses
  `Contents/content.hpf` `<opf:metadata>` into the Core
  [`Metadata`](Phase 0) struct, symmetric with Phase 1's encoder
  output.
- `HwpxDecoder::decode` now reads metadata before constructing the
  `Document`, so a Hancom-emitted file's `$title`/`$author`/etc.
  values resolve correctly on the next encode.
- Missing or malformed `content.hpf` metadata downgrades to
  `Metadata::default()` (third-party HWPX authors may omit the
  block).

#### Security (Wave 12o architect review §11.1 B3 / §11.4 S1–S3)

- `<!DOCTYPE>` events are rejected explicitly — defeats XXE and
  billion-laughs vectors before quick-xml's default parser path.
- CDATA inside `<opf:metadata>` is rejected.
- Allocation caps: depth ≤ 16, `<opf:meta>` count ≤ 256,
  per-text body ≤ 64 KiB, attribute value ≤ 64 KiB.
- S3 namespace confusion guard: any child of `<opf:metadata>` must
  use the `opf` prefix; `<hp:title>` and friends are rejected.
- S1 illegal-XML-char sanitizer is applied to all decoded text
  before it enters `Metadata`.

### Wave 12o Phase 1 — HWPX encoder `content.hpf` metadata emit

#### Changed (BREAKING — public API)

- `HwpxEncoder::encode` now consults `document.metadata()` and
  forwards it to the package writer.
- `package::generate_content_hpf` / `PackageWriter::write_hwpx`
  signatures gain a leading `metadata: &Metadata` parameter
  (internal `pub(crate)`; not part of the public surface).

#### Added — wire format

- `content.hpf` `<opf:metadata>` is always emitted with the full
  nine-slot Hancom byte-parity layout
  (`title`/`language`/`creator`/`subject`/`description`/`lastsaveby`/
  `CreatedDate`/`ModifiedDate`/`date`/`keyword`) plus alphabetical
  `extras`. `None` slots self-close; populated slots use the
  `content="text"` attribute.
- `date` always self-closes (Hancom recomputes on save —
  Wave 12o §11.3 Q2). `keywords` are joined with `;` into a
  single element (§11.3 Q3).
- Defense in depth: every text body flows through
  `sanitize_xml_text` (Wave 12o §11.4 S1) before `escape_xml`.

### Wave 12o Phase 0 — Document Metadata Core breaking

#### Changed (BREAKING — public API)

- `core::metadata::Metadata` gains three new fields and is now annotated
  `#[non_exhaustive]`. This is ADR-003 in the 0.6.0 break window. External
  callers can no longer construct `Metadata` with a struct literal; use the
  new `Metadata::new()` seed plus the chainable
  `.with_title()`/`.with_author()`/`.with_subject()`/`.with_description()`/
  `.with_last_saved_by()`/`.with_keywords()`/`.with_created()`/
  `.with_modified()`/`.with_extra()` builder methods.
- New `Metadata::description: Option<String>` — corresponds to HWPX
  `<opf:meta name="description">`. Distinct from `subject` (Hancom stores
  them in separate slots).
- New `Metadata::last_saved_by: Option<String>` — corresponds to HWPX
  `<opf:meta name="lastsaveby">` and the `$lastsaveby` SUMMERY auto-field
  (Wave 12n). Distinct from `author` (`creator`).
- New `Metadata::extras: BTreeMap<String, String>` — lossless carry slot
  for `<opf:meta>` keys not yet promoted to typed fields. Deterministic
  ordering for byte-stable encoder output.
- Rationale: HwpForge-emitted `content.hpf` had hardcoded `<opf:title/>`
  with all other metadata fields empty, causing Hancom Office to overwrite
  SUMMERY auto-field values (e.g. `$title`) with fallback text on save.
  Wave 12n's `Control::Field` (Author/LastSavedBy/CreatedTime/
  ModifiedTime/Title) auto-fields need a populated metadata source to
  resolve to user-intended values. Phases 1–4 (HWPX encoder/decoder,
  HWP5 SummaryInformation) wire the carry end-to-end.

### Phase 12l — Control::Field `name` carry (Core + HWPX + HWP5)

#### Changed (BREAKING — public API)

- `Control::Field` gains a `name: Option<String>` field carrying the form-mode
  identifier (HWPX `<hp:fieldBegin name="...">`). Existing callers that
  construct `Control::Field` with a field-record literal must add `name:
  None`. Pattern matches with `..` are unaffected. Required because the
  prior model silently dropped the form-mode identifier on every
  HWPX↔Core↔HWP5 roundtrip.
- `Control::field()` convenience constructor unchanged in signature; the
  produced `Control::Field` now has `name: None`.
- `Display` impl for `Control::Field` now prints `name="…"` only when present.
- `Serialize`/`Deserialize`/`JsonSchema` representations of `Control::Field`
  gain the new field; consumers that round-trip JSON of `Control::Field`
  must accept the new key (it is optional / defaults to `null`).

#### Fixed — HWPX encoder

- `build_field_run_xml` now emits the real `name` attribute on
  `<hp:fieldBegin>` for both `CLICK_HERE` and `SUMMERY` (automatic) fields
  instead of the hardcoded `name=""`.
- `Clickhere:set:N:` now uses a fixed-point computed `N` (self-referential
  UTF-16 code unit count) instead of the literal `43` that did not match
  the real command string length and was a wire-truth lie even before name
  carry.
- `build_field_run_xml` signature gains `name: &str`.

#### Fixed — HWPX decoder

- Both `CLICK_HERE`/automatic-field branches and the `SUMMERY` branch in
  `decode_field_control` now carry `fb.name` into `Control::Field.name`
  (with `Some("") -> None` normalisation, matching the indexmark/dutmal
  pattern).

#### Added — HWP5 leg

- HWP5 `%clk` CtrlHeader parser (`Hwp5ClickHereControl::parse`) carries
  the press-field's `hint_text` / `help_text` / `field_unique_id` from
  the wire's UTF-16LE Command string. Parser is length-driven (no
  `:`-delimiter split) and UTF-16 code unit cursor based, so embedded
  colons in hint/help and surrogate-pair input do not corrupt decoding.
- HWP5 `0x57` (`TagId::CtrlData`) sub-record parser
  (`Hwp5ClickHereControl::parse_name_subrecord`) carries the form-mode
  `name`. The `%clk` → `0x57` pairing is held by
  `BodyTextParserState.pending_clickhere` (mirrors the `eqed` →
  `EqEdit` pattern used in Wave 12d) and flushed at orphan boundaries
  with a targeted `ProjectionFallback` warning.
- Projection has a dedicated `clickhere_controls` queue + new
  `ActiveField::ClickHere` variant. `start_active_field` /
  `finish_active_field` emit a single `Control::Field` Run with all
  four metadata fields populated; the HWPX encoder rebuilds the visible
  placeholder from `hint_text`, so the span between the `FIELD_BEGIN` /
  `FIELD_END` markers is intentionally not double-emitted.
- Audit semantic model gains `Hwp5SemanticControlKind::ClickHere` so
  press-field counts attribute correctly in the audit-batch output.

#### Fixed — HWPX encoder Command N

- `Clickhere:set:43:` was a wire-truth lie (the literal `43` did not
  match any real-fixture command length, even before name carry).
  Replaced by `clickhere_command_string`, which uses the
  empirically-derived formula `N = utf16_len(rest_of_command) - 1`
  (verified against the seven press-field instances across five
  fixtures). The formula is `digits(N)`-independent, so no fixed-point
  iteration is required.

#### Notes

- See `.docs/algorithms/2026-06-02_clickhere_field_carry.md` for the
  full algorithm + Codex review resolution table.
- See `.docs/research/2026-06-02_clickhere_wire_dump.md` for the wire
  layout derivation.

### Phase 12 (HWP5 drawing-object carry)

Continues the Phase 11 line: HWP5 drawing objects the decoder previously
skipped now carry through Core to HWPX instead of silently emptying their
host paragraph. No Core or HWPX API changes — these shape variants already
existed in the shared model; only the HWP5 leg was missing.

#### Added — Wave 12a (GSO ellipse / arc / curve)

- Decode `gso ` `ShapeComponentEllipse` (`0x50`) and `ShapeComponentCurve`
  (`0x53`) sub-records and project them to `Control::Ellipse`,
  `Control::Arc`, and `Control::Curve`. Previously these fell through to
  `Hwp5Control::Unknown` and were dropped, emptying the host paragraph.
- 한컴 stores arcs inside the ellipse (`0x50`) record with arc fields set —
  it does **not** emit a separate `ShapeComponentArc` (`0x51`). An arc is
  now distinguished by content and carried as `Control::Arc` →
  `<hp:ellipse hasArcPr="1">`.
- Classify ellipse/arc/curve in the audit semantic model
  (`Hwp5SemanticControlKind::{Ellipse, Arc, Curve}`) so source-side control
  counts match converted output.
- Binary layouts confirmed empirically from 한컴 truth fixtures
  (`sample-gso-{ellipse,arc,curve}`); golden tests assert end-to-end carry.
- Known limitation: arcs carry as the `Normal` arc type sized from the
  bounding box. Pie/chord arc types and exact arc-sweep endpoints are
  deferred until dedicated fixtures exist.

#### Added — Wave 12b (GSO connect line)

- Carry connectors as `Control::ConnectLine` → `<hp:connectLine>` instead of
  demoting them to a plain `<hp:line>`. 한컴 stores a connector in the **same**
  `ShapeComponentLine` (`0x4E`) sub-record as a plain line; the only
  discriminator is the `ShapeComponent` (`0x4C`) type tag `"$col"` (confirmed
  against `$rec`/`$ell`/`$cur`). A conservative guard upgrades **only** an
  exact `"$col"` match, so plain lines are never reclassified.
- Classify connectors in the audit semantic model
  (`Hwp5SemanticControlKind::ConnectLine`).
- Confirmed end to end against a natively-drawn 한컴 connector fixture
  (`sample-gso-connectline-native`: two rectangles + one connector).
- Known limitation: only a straight connector with its endpoints is carried;
  the source connector's object-link references have no `<hp:connectLine>`
  representation and are dropped.

#### Fixed — floating ellipse/arc/curve/connect-line positioning (HWPX encoder)

- `<hp:ellipse>`, `<hp:curve>`, and `<hp:connectLine>` hardcoded inline
  positioning (`numberingType="NONE"`, `textWrap="TOP_AND_BOTTOM"`,
  `vertRelTo/horzRelTo="PARA"`) instead of using the shared offset-aware
  helpers that `<hp:line>`/`<hp:rect>` already used. A **floating** shape
  (non-zero offset) was therefore mis-anchored to the paragraph and rendered
  in the wrong place in 한컴. They now route through `shape_position` /
  `shape_numbering_type` / `shape_text_wrap`, so a floating shape anchors to
  `PAPER` as `PICTURE`/`IN_FRONT_OF_TEXT` (matching 한컴) while inline shapes
  (zero offset) are unchanged. Exposed by Wave 12b's first floating connector.

#### Added — Wave 12d (equation)

- Carry equations as `Control::Equation` → `<hp:equation>` with the HancomEQN
  script preserved. The `eqed` ctrl used to fall through to
  `Hwp5Control::Unknown` and was dropped. The decoder now recognizes the
  `eqed` ctrl, parses the script from its child `HWPTAG_EQEDIT` (`0x58`)
  record (`UINT32` property, then a `UINT16` WCHAR-count length prefix + UTF-16
  script), and projects it to `Control::Equation` sized from the ctrl-header
  geometry.
- Classify equations in the audit semantic model
  (`Hwp5SemanticControlKind::Equation`).
- Confirmed end to end against a 한컴 equation fixture
  (`sample-equation-basic`: `{a + b} over {c + d}`); golden test asserts both
  `<hp:equation>` emission and verbatim `<hp:script>` carry.

#### Added — Wave 12e-Memo (memo annotation carry + body corruption fix)

- Carry memo annotations as `Control::Memo` → HWPX `<hp:fieldBegin type="MEMO">`
  with the body paragraphs in `<hp:subList>`. The HWP5 `%unk` ctrl with command
  `"MEMO/{shapeId}/{memo_id}/{instId}"` used to fall through to
  `Hwp5Control::Unknown`, so the matching `HWPTAG_MEMO_LIST` (`0x5D`) cluster's
  level-2 `ParaText` records would fall into the body-paragraph `ParaText` arm
  and **overwrite the body text** — the visible "memo content replaces body
  text" corpus bug.
- Decoder now recognises the `%unk MEMO/...` placeholder, captures the matching
  cluster region at the end of the section's last body paragraph (records at
  level 1/2: `MemoList`, `ListHeader`, content `ParaHeader`, `ParaText`,
  `CharShape`, …), and joins clusters back to placeholders by `memo_id` (not
  by document position) during `BodyTextParserState::finish`. Multi-memo
  fixtures confirmed both happy path and id-keyed matching.
- Projection adds an `ActiveField::MemoAnchor` so the anchor text inside the
  `FieldBegin %unk MEMO` / `FieldEnd` span flows into `runs` (no drop) and the
  memo `Run` is emitted at the anchor's start position.
- `layout_hint_patch` now folds memo body paragraphs into the body scope so the
  HWPX patcher does not underflow the paragraph-hint queue.
- Classify memos in the audit semantic model (`Hwp5SemanticControlKind::Memo`).
- Golden tests confirmed against two 한컴 fixtures: `sample-memo-basic` (one
  memo, body anchor + body content preservation) and `sample-memo-multiple`
  (two memos, id-keyed cluster matching).

#### Changed — Wave 12e-Memo (Core API breaking, semver-deliberate)

- `Control::Memo` no longer carries `author` / `date` — the variant is now
  `Control::Memo { content: Vec<Paragraph> }`. Neither field was actually
  populated by any wire path: the HWPX `<hp:fieldBegin type="MEMO">` only
  exposes `MemoShapeID` / `MemoType` parameters, and HWP5's `%unk MEMO/...`
  command exposes no author/date metadata. Holding the fields encouraged
  callers to pass dummy values that never round-tripped.
- `Control::memo(content)` helper drops the corresponding `author`/`date`
  parameters; call sites in `smithy-hwpx` examples (`shapes_and_references`,
  `hwpx_complete_guide_parts/section2`) and tests
  (`smithy-hwpx/src/registry_bridge.rs`,
  `smithy-hwpx/src/encoder/section.rs`) updated.
- `smithy-md` markdown encoder emits `<!-- memo: body -->` instead of
  `<!-- memo(author): body -->` (the author segment was always blank in
  practice).

#### Fixed — Wave 12f (memo anchor position)

- HWP5 stores the memo *inline* `FieldBegin` marker with `extra[0..4] = %%me`
  (`0x2525_6D65`), which is **not** the same id as the `CtrlHeader` ctrl_id
  `%unk` (`0x2575_6E6B`). Wave 12e matched only the latter, so the inline
  anchor was never recognised and the memo `Run` ended up drained at the
  end of the paragraph as a point-anchored field — 한컴 rendered it as
  `메모 대상 문장입니다.[메모 시작][필드 끝]`.
- `projection.rs::start_active_field` now recognises both ids: the inline
  marker activates `ActiveField::MemoAnchor`, which positions the memo
  `Run` at the correct anchor offset (`vis=2` in the basic fixture).

#### Changed — Wave 12g (Core API breaking, semver-deliberate)

- `Control::Memo` gains `anchor_runs: Vec<Run>`. The anchor body sits
  *inside* the same `<hp:run>` as `fieldBegin`/`fieldEnd` in 한컴
  truth fixtures; the previous Wave-12f layout placed the anchor as a
  separate Run before the memo and 한컴 mis-rendered the end marker as
  generic `[필드 끝]`.
- HWPX encoder now serializes memos as a flat
  `[fieldBegin][anchor_xml][fieldEnd]` `<hp:run>` via
  `build_memo_anchor_xml`. See
  `.docs/algorithms/2026-06-01_memo_anchor_serialization.md` for the
  anchor-body collapse heuristic (single `<hp:t>` per anchor) and why
  we accept the per-run char-shape fidelity loss.
- `Control::memo_with_anchor(content, anchor_runs)` helper added.

#### Added / Changed — Wave 12h (full `<hp:parameters>` carry)

- HWPX `<hp:parameters>` now emits the full 한컴-standard `cnt="7"`
  block (`Prop`, `Command`, `ID`, `Number`, `Author`, `MemoShapeIDRef`,
  `CreateDateTime`) plus `editable="1" dirty="1" zorder="1"`. The
  pre-12h `cnt="2"` block (`MemoShapeID` + `MemoType` only) made
  한컴 mis-classify the field — the memo body rendered correctly, but
  the end marker fell back to generic `[필드 끝]` in 조판부호 view.
- New schema struct `Hwp5MemoCommand` parses the wire's
  `"MEMO/{shape_id}/{memo_id}/{hancom_inst_a}/{hancom_inst_b}/{author}/{terminator}"`
  command string. Reusable wire-string utilities
  (`parse_ctrl_header_command_string`, `split_slash_command`) added to
  `schema/section.rs` for future `%hlk` / `%xrf` / `%bmk` work.
- New `Control::Memo { metadata: MemoMetadata, … }` field (Core API
  breaking, semver-deliberate). `MemoMetadata` carries `shape_id_ref`,
  `number`, `id`, `author`, `create_datetime`, `command` — wire
  values flow through `projection.rs` verbatim.
- `CreateDateTime` has no HWP5 wire source (verified via DOC_PROPERTIES
  `0x10`, TRACKCHANGE `0x20`, MemoShape `0x5E` dumps). Encoder fills it
  with `iso8601_utc_now()` (std-only Howard-Hinnant civil-from-days) at
  write time — matching 한컴's own behaviour on fresh saves.
- `build_memo_parameters_xml` extracted as a re-usable HWPX
  `<hp:parameters>` builder for future field types that need the same
  structure (hyperlink / cross-reference fidelity work).

#### Added / Changed — Wave 12i (dutmal carry + flat-path control filter)

- Carry dutmal (덧말) annotations as `Control::Dutmal` →
  `<hp:dutmal>` with `main_text` / `sub_text` / `posType` /
  **`option`**. The decoder now reads `option_raw` from
  `tail[8..12]` of the `tdut` ctrl payload (`Hwp5DutmalControl`); the
  HWPX encoder previously hard-coded `option="0"` and the HWPX
  decoder discarded the value, so any non-default `option=4` 한컴
  fixture lost fidelity end to end. Both legs now mirror the integer
  verbatim. Semantics of `option` are intentionally not pinned —
  the bit/enum meaning is undocumented and produces no visible
  rendering difference in our truth fixture; see
  `.docs/algorithms/2026-06-01_dutmal_carry.md`.
- New `Control::Dutmal { metadata: DutmalMetadata, … }` field
  (Core API breaking, semver-deliberate). `DutmalMetadata` carries
  `option: u32` and is `#[non_exhaustive]` so future
  `sz_ratio` / `align` / `style_id_ref` decode work is additive.

#### Fixed — Wave 12i flat-path projection `control_iter` filter

- `project_paragraph_with_images_flat` used to iterate **every**
  control in `Hwp5Paragraph.controls` (including the
  `secd` / `cold` / `%bmk` / `%hlk` / `%xrf` / `bokm` / `pgnp`
  Unknown markers that lead a first-section paragraph) when matching
  inline `\u{FFFC}` `ControlRef` positions to runs. Each FFFC
  popped the *wrong* control — the marker-header Unknowns returned
  `None` from `project_control_run` and got dropped, while the real
  inline controls leaked to the end-of-paragraph drain. The
  observable symptom on Wave 12i's two-dutmals-with-space fixture
  was the body space `<hp:t> </hp:t>` getting pulled in front of
  both dutmals (`한국어 韓字` → `한국어韓字`). The structural
  projection path was unaffected because it already separated those
  controls into `marker_headers` vs. `object_controls` queues. The
  flat path now applies the same filter so its FFFC iterator only
  sees object controls. Any first-section paragraph that combines
  `secd` / `cold` with **any** inline shape (rect, polygon,
  ellipse, image, table, equation, dutmal) is covered, not just
  dutmal. See `.docs/algorithms/2026-06-01_dutmal_carry.md`
  (companion-fix section) for the full root-cause + rationale.

#### Added / Changed — Wave 12j (compose carry + char_pr_ids fidelity)

- Carry compose (글자겹침) annotations end-to-end through the HWP5 leg
  — `Hwp5ComposeControl` schema struct, `tcps` CtrlHeader decode,
  `Hwp5Control::Compose` variant, and a `project_compose_run` that
  emits `Control::Compose` with circleType / composeType mapped from
  raw enum bytes to the OWPML attribute strings (14 `SHAPECIRCLETYPE`
  values + 2 `COMPOSETYPE` values, including the spec-typo
  `SHAPE_REVERSAL_TIRANGLE`). HWPX→Core and Core→HWPX both already
  existed prior to this wave; only HWP5→Core was missing and `tcps`
  was silently dropped to `Hwp5Control::Unknown`.
- New `Control::Compose { char_pr_ids: Vec<u32>, … }` field (Core API
  breaking, semver-deliberate). HWPX schema fixes `<hp:compose
  charPrCnt>` at 10, but the existing encoder hard-coded all 10 slots
  to `u32::MAX` ("no override" sentinel) and the existing decoder
  discarded `<hp:charPr prIDRef>` children entirely. The new field
  threads the 10 LE u32 charPr IDs verbatim through Core so a `HWPX
  → Core → HWPX` round-trip preserves which slots carry a real
  `prIDRef` override (e.g. `7`) vs. the `u32::MAX` placeholder.
  Encoder pads / truncates to 10 slots.
- Compose wire layout discriminator. The `tcps` CtrlHeader payload
  has two empirically-observed forms, discriminated by the low half
  of `properties` (`data[4..6]` as LE u16):
  - `0x0003` (unpacked) — `composeText` is fully in `data[8..]`,
    body trailer carries the 4 metadata bytes + 10 × u32 charPrs.
    27 of 28 round-tripped variants and every native 한컴 fixture
    use this layout.
  - `0x0002` (packed) — `composeText[0]` is in `properties[2..4]`,
    `composeText[1..N]` is at the start of the body; the body
    trailer layout is unchanged. Observed exclusively on the
    `CHAR + OVERLAP` combination when 한컴 saved an HWPX → HWP5.
    The parser detects this via `properties.low == 0x0002` and
    prepends the packed char to the body region before decoding.
- Any other `properties.low` value falls through to
  `Hwp5Control::Unknown`. No clamp; no guessing. See
  `.docs/algorithms/2026-06-01_compose_carry.md` for the full
  discriminator table, the shape-glyph table for `properties.high`,
  and the validation policy rationale (Codex-reviewed).
- The packed variant was discovered only through a 14 × 2 = 28
  `circleType × composeType` combinatorial fixture
  (`gen_compose_variants` example) — single-shape native fixtures
  never trigger it. The lesson is captured in
  `.docs/learnings/2026-06-01_hwp5_ctrl_header_properties_overloaded.md`:
  HWP5's CtrlHeader `properties` word is **not** a generic bitfield —
  each ctrl_id can repurpose it for ctrl-specific data, and the same
  ctrl can switch layouts within a single document.

#### Added — Wave 12k (IndexMark carry + `0x16` inline marker promotion)

- Carry IndexMark (찾아보기 표시) annotations end-to-end through the
  HWP5 leg — `Hwp5IndexMarkControl` schema struct, `idxm` CtrlHeader
  decode, `Hwp5Control::IndexMark` variant, and a
  `project_indexmark_run` that emits `Control::IndexMark { primary,
  secondary }`. The HWPX encoder + decoder + Core variant all
  existed prior to Wave 12k; only the HWP5 leg was missing and
  `idxm` was silently dropped to `Hwp5Control::Unknown`.
- **`0x16` ParaText inline-marker promotion (Codex 🟠 HIGH).**
  `Hwp5ParaText::parse` used to consume the entire `0x0E..=0x16`
  byte range silently. Adding a CtrlHeader dispatch alone would
  have left every IndexMark `Run` drained to end-of-paragraph
  instead of anchored at the inline marker. The fix carves `0x16`
  out of the silent-consume range and promotes it to
  `TextSegment::ControlRef` only when `extra[0..4]` matches the
  LE-stored ctrl_id for `idxm`. The discrimination keeps every
  unknown `0x16` owner falling through the silent path so we don't
  alias unrelated control families.
- Targeted parser warning. Malformed `idxm` payloads emit
  `Hwp5Warning::DroppedControl { control: "indexmark", … }` rather
  than the generic `UnsupportedTag(0x47)` fallback — audit
  baselines can attribute the loss to the IndexMark code path
  specifically.
- Trailer 4 bytes deliberately discarded. The trailer is
  `0xFFFF_FFFF` on native 한컴 authoring and `0x00000000` on HWPX→
  HWP5 round-trip via 한컴 save; HWPX has no corresponding
  attribute, so carrying it into Core would be a format leak. The
  4 bytes are still required to be present — a truncated trailer
  means the record boundary is no longer trustworthy.
- Empty-secondary normalization. The source `Some("")` for one
  fixture entry comes back from 한컴 as `secondary_units_len = 0`;
  the decoder reflects 한컴's intent by returning `None`. HWP5
  wire cannot distinguish the two states once 한컴 has saved.
- Golden tests cover (a) the native two-IndexMark fixture, (b) the
  8-IndexMark combinatorial fixture (every primary-only and
  primary+secondary combination + the `CPU` / `GPU` same-paragraph
  order regression gate that Codex specifically called out). The
  schema layer adds five focused unit tests (real native bytes,
  real multi bytes with secondary, `primary_units_len == 1` edge
  case, `primary_units_len == 0` rejection, truncated-trailer
  rejection). See
  `.docs/algorithms/2026-06-02_indexmark_carry.md` for the full
  layout table, Codex-review rationale, and validation policy
  notes.

### Phase 11 (HWP5 → HWPX silent-gap closure)

This release closes the largest batch of "HWP5 decoder has the bytes, but
projection or shared model can't carry them" gaps measured against truth
HWPX fixtures from 한컴 Office. All work is HWP5-leg only — HWPX
encode/decode paths were already verified by existing golden tests.

#### Added — Wave 0 (audit infrastructure)

- `audit-hwp5` warning taxonomy (`SilentGap`, `DroppedControl`, …) and
  `--strict` mode so silently dropped semantics are surfaced as build
  signals instead of accumulating as invisible technical debt.

#### Added — Wave 1 (character style line family + word break)

- Carry `underline` (1b) and `strikeout` (1c) line family (`DOT`, `DASH`,
  `DOT_DASH`, etc.) through `HwpxCharShape` instead of collapsing to
  `SOLID`.
- Carry `breakLatinWord = HYPHENATION` (1d) through `WordBreakType` and
  HWP5 projection so HWPX `breakLatinWord` is no longer always
  `KEEP_WORD`.
- Surface silent `shadow` decode gap as warning (1a) until carry lands.

#### Added — Wave 2 (paragraph layout fidelity)

- Carry `lineSpacingType = AtLeast` (2a).
- Verify all 6 alignment variants (2b), `indent` + `pageBreakBefore`
  (2cd), and `border` + `shading` (2e) carry against truth HWPX
  fixtures.

#### Added — Wave 3 (paragraph-level checked decode)

- HWP5 paragraph-level `paraPr.checked` decode so the third location of
  checkable-bullet truth (the per-item checked state) is no longer lost.
  Closes the legacy R1 line.

#### Added — Wave 4 (Field / Object parity)

- Wave 4a: carry HWP5 `Rect` control through Core → HWPX.
- Wave 4b: carry HWP5 footnote control through Core → HWPX.
- Wave 4c: carry HWP5 chart (OLE-backed BinData) end-to-end as
  `Control::EmbeddedChart` passthrough — emits `Chart/chartN.xml` +
  `BinData/oleN.ole` + `<hp:switch>` block. Closes the
  `DroppedControl:ole_object` measurement.
- Carry HWP5 fixed-width space and non-breaking space through Core to
  HWPX inline text.
- Carry HWP5 checkable bullet `checkedChar` + `paraHead.checkable`
  through bullet conversion.
- Tab fidelity end-to-end: carry inline `<hp:tab width / leader / type>`
  attributes through a new `RunContent::InlineText` variant; HWPX
  encoder/decoder updated symmetrically; fill-type → leader mapping
  rebuilt against openhwp truth. See debug doc
  `.docs/debug/2026-05-26_tab_fidelity_bugs.md` and
  `.docs/debug/2026-05-27_hwpx_decoder_inline_tab_attrs_lost.md`.
- Field controls: emit non-zero `fieldid` for `CROSSREF` and keep
  `fieldBegin id` within signed 32-bit range.

#### Added — Wave 5 (page-level features)

- Wave 5 gap A: carry per-ctrl `applyPageType` (`BOTH` / `ODD` / `EVEN`)
  through HWP5 `head` / `foot` ctrl property word into multiple
  `<hp:header>` / `<hp:footer>` elements. Verified against
  `sample-header-footer-odd-even` truth fixture.
- Wave 5 gap B: carry `secd` ctrl property bits (0/1/2/5/19) into
  `Section.visibility.hide_first_header / footer / page_num /
  empty_line / master_page`. Verified against
  `sample-header-footer-hide-first` truth fixture. New
  `crates/hwpforge-smithy-hwp5/examples/probe_secd.rs` reusable probe.
- Wave 5 gap C (`masterPage` carry): **deferred**. Diagnosed as
  fixture-asymmetric on macOS 한컴 (truth HWPX has `masterpage0.xml`
  but the paired HWP5 has no master-page sub-records). See debug doc
  `.docs/debug/2026-05-27_hwp5_page_features_lost.md` for full probe
  results. Resume when a PC-한컴 fixture is available.

#### Fixed — Wave 6 (corpus-driven conversion robustness)

Measured by extending the `audit-hwp5` signal source from synthetic
fixtures to the real government-document corpus and clustering the
pre-categorized conversion failures. The two real bugs recovered all
29 `hwp5_convert_failed` documents (plus one more from `탈락`); the
remaining failures are inputs that are genuinely not HWP5.

- Schemeless hyperlink URLs (e.g. `www.motie.go.kr`) are normalized to
  `http://` instead of aborting the whole conversion. The HWPX encoder
  previously rejected any URL outside the `http://` / `https://` /
  `mailto:` allowlist; explicit unsafe schemes (`javascript:`, `data:`,
  `file:`, …) are still rejected. (16 corpus docs)
- Non-leading table header rows are demoted to normal rows (with a
  warning) in HWP5 projection instead of failing Core validation with
  `NonLeadingTableHeaderRow`. Real 한글 tables sometimes restate a
  header row mid-table; the leading header block is preserved and the
  stray header row is demoted so the document still converts.
  (14 corpus docs)
- `.hwp` inputs that are actually ZIP (`PK..`, i.e. an HWPX saved with a
  `.hwp` extension) or a Hancom secured/DRM container (`SCDS..`) now
  fail with an actionable message instead of a raw CFB byte dump.

### Changed — Breaking

- **ADR-002**: `hwpforge_core::Section.header: Option<HeaderFooter>` →
  `Section.headers: Vec<HeaderFooter>` (and `footer` → `footers`). This
  changes the public `Section` shape so that multiple page-type-scoped
  headers (`ODD` / `EVEN` / `BOTH`) can coexist as required by HWPX
  wire format. Empty `Vec` means "no header". JSON dump now emits a
  list instead of an object (or omits when empty). Patch / MD encoder
  / CLI / MCP consumers updated to iterate the slot. See
  `.docs/architecture/adr/ADR-002-section-multi-header-footer-cardinality.md`.

### Added — Public API

- `hwpforge_core::RunContent::InlineText(InlineText)` variant for
  mixed-content runs that include `<hp:tab>` (and is forward-compatible
  with `<hp:lineBreak>` / `<hp:nbSpace>` / `<hp:fwSpace>`). `InlineText`
  / `InlineSegment` / `InlineTabAttr` are `#[non_exhaustive]`.
- `Section` constructors continue to be `new()` / `with_paragraphs()`;
  push headers/footers via `section.headers.push(HeaderFooter::…)`.

### Migration

- Replace `section.header = Some(hf)` with `section.headers.push(hf)`.
- Replace `section.header.as_ref()` with
  `section.headers.first()` (single-slot consumers) or iterate
  `section.headers` (general).
- `serde` shape now uses `headers` / `footers` arrays; persisted JSON
  using the old keys must be migrated.
- Match arms on `RunContent` may need a new `InlineText(_)` arm. Use
  `RunContent::carries_text()` / `plain_text()` for read-only consumers
  that don't care about inline attribute fidelity.

## [0.5.2] - 2026-05-13

### Added

- `hwpforge` CLI gains `to-md` lossy / lossless modes for
  HWPX → Markdown export choice.
- HWP5 → HWPX projection preserves field controls and checkable state
  (precursor to Phase 11 Wave 3/4 closure).

### Fixed

- Dependency hygiene updates (`sha2`, GitHub Actions pinning, suppress
  `RUSTSEC-2026-0097` non-applicable advisory).

## [0.5.1] - 2026-04-13

### Added

- HWP5 → HWPX style fidelity bridge improvements (more char/para style
  surface preserved end-to-end).
- HWPX char effects: preserve `emboss`, `engrave`, `superscript`,
  `subscript` (also covered later under Wave 1 audit).

### Fixed

- Warn on conflicting vertical-position bits instead of silently
  normalizing.
- HWP5 paragraph layout hints (linesegarray, safe table height) carried
  to HWPX so visual diff matches truth better.
- `convert-hwp5` / `audit-hwp5` / `inspect` summary share the same
  style-projection warning source.

## [0.5.0] - 2026-03-20

### Changed — Breaking

- Adopt shared `ordered` / `bullet` / `outline` list semantics across
  `core`, `blueprint`, and `smithy-hwpx`. Markdown bridge integrated.
- Add **checkable bullet** semantics (HWPX `heading(type="BULLET")` +
  `bullet.checkedChar` + `bullet.paraHead.checkable` +
  `paraPr.checked`). Markdown task lists normalize to this shared HWP
  semantic; ordered task lists intentionally drop numbering on the way
  in.
- Tighten bullet semantics: paragraph `heading_level` is no longer a
  catch-all for list semantics (see gotcha #7 in `CLAUDE.md`).

### Added

- Markdown bridge: preserve task list continuation paragraphs;
  normalize ordered task lists.
- Fixtures: reorganized under `examples/` and `tests/fixtures/`.

### Fixed

- HWPX style id bridging for registry-local style ids.
- Outline contract hardening (golden tests).

## [0.4.0] - 2026-03-20

### Changed

- Promote the workspace release line to `0.4.0` for the breaking tab semantics contract in `hwpforge-core` and `hwpforge-blueprint`.
- Add shared tab-stop semantics across the IR stack so HWPX/HWP5 codecs can preserve explicit tab definitions and paragraph tab references.

### Migration

- `hwpforge_core::TabDef` now includes explicit `stops`; downstream struct literals must initialize the new field.
- `hwpforge_blueprint::Template`, `ParaShape`, and `PartialParaShape` now carry tab definition references/collections directly.
- Consumers matching on `BlueprintErrorCode` should handle the new tab-related error codes explicitly.

## [0.3.0] - 2026-03-19

### Changed

- Promote the workspace release line to `0.3.0` for the breaking HWPX section editing contract update.
- Preserve-first section editing now requires preservation metadata on `ExportedSection` and rejects stale or legacy section exports explicitly.

## [0.2.0] - 2026-03-17

### Changed

- Adopt the `hwpforge-core` v0.2.0 public DOM contract for richer table and image semantics.
- Align the workspace release line and internal crate pins on `0.2.0`.

### Migration

- `Table`, `TableRow`, `TableCell`, and `Image` are now `#[non_exhaustive]` and should be constructed via `new`/`with_*` builders instead of struct literals.
- Table DOM now carries page-break, repeat-header, cell-spacing, border/fill, header-row, cell margin, and vertical-alignment semantics directly in `hwpforge-core`.
- Image DOM now carries placement metadata directly in `hwpforge-core`.
- Validation now exposes `CoreErrorCode::NonLeadingTableHeaderRow`; downstream code that inspects validation codes should handle it explicitly.

## [0.1.0] - 2026-03-06

### Added

- **hwpforge**: Umbrella crate with feature flags (`hwpx`, `md`, `full`)
- **hwpforge-foundation**: Primitive types (HwpUnit, Color BGR, branded Index<T>, enums, error codes)
- **hwpforge-core**: Format-independent document model with typestate validation (Draft/Validated)
  - Document, Section, Paragraph, Run, Table, Image
  - Controls: TextBox, Footnote, Endnote, Equation, Chart (18 types)
  - Shapes: Line, Ellipse, Polygon, Arc, Curve, ConnectLine
  - References: Bookmark, CrossRef, Field, Memo, IndexMark
  - Layout: Multi-column, captions, headers/footers, page numbers, master pages
  - Annotations: Dutmal, compose characters
- **hwpforge-blueprint**: YAML-based style template system
  - Template inheritance with DFS merge
  - StyleRegistry with deduplicated fonts, char shapes, para shapes
  - Built-in default template (Hancom 한컴바탕)
  - BorderFill support
- **hwpforge-smithy-hwpx**: Full HWPX codec (KS X 6101)
  - Decoder: HWPX ZIP+XML -> Core Document
  - Encoder: Core Document -> HWPX ZIP+XML
  - Lossless roundtrip for all supported content
  - HancomStyleSet support (Classic/Modern/Latest)
  - 22 default styles with per-style charPr/paraPr
  - ZIP bomb defense (50MB/500MB/10k limits)
  - OOXML chart generation (18 chart types)
  - Golden fixture tests with real Hancom 한글 files
- **hwpforge-smithy-md**: Markdown codec
  - GFM decoder (pulldown-cmark) with YAML frontmatter
  - Lossy encoder (readable GFM) and lossless encoder (HTML+YAML)
  - Full pipeline: MD -> Core -> HWPX verified in Hancom 한글

[0.1.0]: https://github.com/ai-screams/HwpForge/releases/tag/v0.1.0
