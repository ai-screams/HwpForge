# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.18.0](https://github.com/ai-screams/HwpForge/compare/v0.17.1...v0.18.0) - 2026-10-25

### Added

- **BREAKING** 0.18 error codes, depth limits and styled Markdown fixes ([#272](https://github.com/ai-screams/HwpForge/pull/272))


### Changed (BREAKING)

- cell·stamp·structural 편집기의 입력 decode는 `DECODE_FAILED`, no-op validate·encode·재decode는 `INPUT_NOT_ROUNDTRIP_SAFE`, 편집 문서 검증은 `VALIDATION_FAILED`, 출력 생성은 `ENCODE_FAILED`, 산출물 검증은 `SELF_VERIFY_FAILED`로 분류한다. 보호 필드 침범은 `PROTECTED_FIELD_RANGE`, source-map·provenance 불변식은 `INTERNAL_INVARIANT`다. CLI·MCP·Python이 같은 코드를 반환한다.
- stamp의 delta 검증 코드는 `SELF_VERIFY_FAILED`, 같은 셀 중복 지정은 `CELL_TARGET_DUPLICATE`로 통일한다.
- CLI 종료 상태는 명령과 비공개 typed 코드로 결정하며, 같은 명령·코드의 종료 값은 오류 payload나 발생 경로에 따라 달라지지 않는다. 아래 네 편집기 변경군과 뒤의 I/O 두 변경군, validate 한 변경군(모두 일곱 변경군)을 적용한다.

| 명령·실패                                                      | 이전 종료 | 0.18 종료 |
| -------------------------------------------------------------- | --------: | --------: |
| delete-para·insert-para: no-op validate·encode·재decode        |         2 |         1 |
| stamp: no-op validate·encode·재decode                          |         2 |         1 |
| set-cell: 입력 decode·원본 패키지 읽기                         |         1 |         2 |
| delete-para·insert-para: 출력 decode·산출물 읽기·fragment 파싱 |         2 |         1 |

- Draft `apply_set_cells` 깊이 초과는 `CellEditError::NestingTooDeep { section: usize, reason }`이며 Display는 `nesting depth exceeds the limit: {reason}`다. 바이트 입력은 decode 관문에서 먼저 거부한다.

- `to_json` 은 파서는 통과하지만 Core 중첩 깊이 32단계를 넘는 HWPX 입력에서 `GRID_ADDR_PROJECTION_FAILED` 대신 읽기 단계의 `DECODE_FAILED` 를 반환한다.
- `stamp-plan` 은 이전에 성공하던 같은 깊이 초과 입력을 읽기 단계에서 `DECODE_FAILED` 로 거부한다.
- `fill`·`stamp`·`insert-para`·`delete-para`·`set-cell`·`patch` 는 같은 깊이 초과 입력에서 이제 decode 단계에서 실패하고, 메시지는 `Core error:` 를 포함한 깊이 초과 진단으로 바뀐다. 여섯 편집기 모두 아래 단계 분류에 따라 `DECODE_FAILED` 를 반환한다.
- CLI·MCP는 공통 `OpsCode::as_str()`를 출력한다. `DECODE_ERROR`, `UNKNOWN_PRESET`, `INDEX_OUT_OF_RANGE` 등의 창구별 별명은 제거한다. 정밀한 기존 도메인 코드와 의미 손실 코드도 그대로 전달한다.
- read의 구역·표·필드 부재는 `SECTION_OUT_OF_RANGE`, `TABLE_NOT_FOUND`, `FIELD_NOT_FOUND`로 통합한다. 관련 read variant 3개와 소비자가 없어진 `SectionWorkflowFailed`, `SetCellFailed`, `StampFailed`, `StructuralEditFailed` 및 옛 lookup을 삭제한다.
- 입력 부재/기타 읽기/크기 상한/출력 I/O는 `FILE_NOT_FOUND`/`FILE_READ_FAILED`/`INPUT_TOO_LARGE`/`FILE_WRITE_FAILED`이다. MCP 출력 경로 정책 거부는 `INVALID_OUTPUT_PATH`이다.
- `INVALID_SET`·`DUPLICATE_SET`과 진단용 네 코드, 총 여섯 CLI 전용 코드는 CLI 안에서 유지한다. 입력 부재·읽기 실패는 to-md·convert-hwp5 2, 나머지 1이며 출력 I/O는 구조 편집·convert-hwp5 2, 나머지 1이다. `INPUT_TOO_LARGE`는 일반 파일 사전 검사·FIFO 모두 1, convert-hwp5의 `FILE_WRITE_FAILED`는 쓰기·후속 stat 실패 모두 2이다.
- Python 파일 I/O는 `HwpForgeError`+`OSError` 계층과 입력 부재 하위 클래스를 사용한다. 광역 catch·OS 필드·copy·pickle을 보존하며 정확한 타입 비교·`PermissionError` 포착에는 변경이 있다. 부모 디렉터리가 없는 `Document.save`는 `FileNotFoundError` 대신 `HwpForgeIOError`로 포착해야 한다.
- to-pdf·convert-hwp5의 HWP5 디코드 실패는 `HWP5_CONVERT_FAILED` 대신 `HWP5_DECODE_FAILED`를 출력한다. convert-hwp5 출력 쓰기 실패는 `FILE_WRITE_FAILED`, to-md 입력 실패는 `FILE_NOT_FOUND`·`FILE_READ_FAILED`·`INPUT_TOO_LARGE`로 구분한다. 아래 두 I/O 변경군 외의 종료 값은 유지한다.
- MCP 직렬화 실패 `SERIALIZE_ERROR`·`STAMP_MANIFEST_SERIALIZE`는 `JSON_SERIALIZE_FAILED`로 통합한다.
- CLI 구조 편집의 `UNCARRIED_ZIP_ENTRIES`는 `INPUT_ENTRIES_NOT_CARRIED`, `STRUCTURAL_EDIT_FAILED`는 구체 도메인 코드 또는 `UPSTREAM_UNMAPPED`로 전달한다.
- set-cell·stamp의 의미 손실은 `SET_CELL_CODEC_FAILED`·`STAMP_CODEC_FAILED` 대신 `ENCODE_SEMANTIC_LOSS`이다.
- `patch`·`fill` 의 오류를 실패 단계와 원인으로 나눈다. 입구 검사를 통과한 뒤 원본을 읽거나 decode 하지 못하면 `PATCH_FAILED`·`FILL_FAILED` 대신 `DECODE_FAILED` 다. 편집한 문서의 검증 실패는 `VALIDATION_FAILED`, 출력 작성 실패는 `ENCODE_FAILED`, 보호 필드의 값·범위 변경은 `PROTECTED_FIELD_RANGE`, 내부 source map·위치 불변식 파손은 `INTERNAL_INVARIANT` 다. `PATCH_FAILED` 는 보존 patch 가 받지 않는 변경과 metadata 부재·불일치에만, `FILL_FAILED` 는 필드 목록 진단에만 남는다.
- `fill` 은 내부 patch 의 원인을 그대로 전달하므로 `PATCH_FAILED` 도 낼 수 있다(예: 다시 만들 수 없는 인라인 표지가 든 칸). 검사 순서는 그대로라 metadata 가 없고 XML 도 손상된 patch 는 계속 `PATCH_FAILED` 다. CLI 종료 값은 두 명령 모두 바뀐 코드에서도 이전 `PATCH_FAILED`·`FILL_FAILED` 와 같은 2 다(patch 의 구역 범위 거부 `SECTION_OUT_OF_RANGE` 1 은 그대로).
- `convert-hwp5`·`to-pdf`(CLI)와 Python `convert_hwp5`·`to_pdf` 의 HWP5 변환 실패도 단계별 코드를 낸다. 읽은 문서의 Core 검증 실패는 `VALIDATION_FAILED`, HWPX 출력 생성 실패는 `ENCODE_FAILED` 이고, 디코더가 스스로 거부한 스타일·테두리 참조(예: 정의가 없는 표 테두리 번호)는 `HWP5_CONVERT_FAILED` 대신 `HWP5_DECODE_FAILED` 다. `HWP5_CONVERT_FAILED` 는 생성한 패키지에 줄 배치 정보를 다시 입히는 단계에만 남는다. 두 CLI 명령의 종료 값은 바뀐 코드에서도 이전과 같은 2 이고, 힌트도 그대로다. Python `convert_hwp5`·`Document.to_pdf` 에서는 디코더가 거부한 참조의 힌트가 코드를 따라 `Check that the source is a supported HWP5 document` 에서 `Check that the file is a valid HWP5 document` 로 바뀐다(검증·출력 생성 실패의 힌트는 그대로).

I/O와 validate의 종료 값 변경은 다음과 같다. 편집기의 네 변경군은 위 표에 적었다.

| 명령·실패                                       | 이전 코드·종료                           | 0.18 코드·종료              |
| ----------------------------------------------- | ---------------------------------------- | --------------------------- |
| to-md·convert-hwp5의 크기 한도를 넘는 FIFO 입력 | `DECODE_FAILED`·`HWP5_DECODE_FAILED` / 2 | `INPUT_TOO_LARGE` / **1**   |
| convert-hwp5의 쓰기 성공 직후 stat 실패         | `FILE_WRITE_FAILED` / 1                  | `FILE_WRITE_FAILED` / **2** |
| validate: 파서는 통과하고 Core 중첩 깊이가 32 를 넘는 HWPX | 판정(`valid:false`) / 3 | `DECODE_FAILED` / **2** |

- `validate` 는 파서는 통과하지만 Core 중첩 깊이가 32 를 넘는 HWPX 에서 판정(`valid:false`) 대신 `DECODE_FAILED` 로 실패한다. HWPX decode 진입점이 이 깊이를 거부하기 때문이며(오류 E4008), 파서 한도(표 중첩·XML 예산)에 걸린 문서는 0.17.1 에서도 `DECODE_FAILED` 였으므로 그대로다. 이행: Rust ops 의 `validate` 는 `Ok(ValidateOutput { ok: false, .. })` 대신 `Err` 를 반환하고, MCP `hwpforge_validate` 는 `valid:false` 데이터 대신 도구 오류를 반환하며, Python `Document.validate()` 는 `ok: False` 를 돌려주지 않고 코드 `DECODE_FAILED` 의 `HwpForgeError` 를 일으킨다. CLI 는 stdout 판정과 종료 3 대신 stderr 의 `DECODE_FAILED` 와 종료 2 를 낸다. 깊이 초과를 판정으로 받던 호출부는 오류 경로에서 처리한다.
- HWPX에서 읽은 본문의 CR이 `\r` 대신 `\n`으로 나온다(to-json, Python, MCP, 무손실 Markdown). 이행: `\r`에 기대던 코드를 고치고, 0.17에서 내보낸 보존 metadata 중 CR이 든 칸이 있으면 다시 내보낸 뒤 patch한다. 자세한 것은 `hwpforge-smithy-hwpx` 항목을 본다.

### Changed

- `hwpforge-*` 크레이트끼리의 의존 요구가 `"0"`(0.x 아무 버전) 대신 릴리스 버전(예: `"0.18.0"`)을 최소 버전으로 요구한다. 그래서 cargo 가 이 의존에 다른 `hwpforge-*` 크레이트의 이전 minor 를 고르지 않는다. `hwpforge-*` 를 직접 여러 개 의존한다면 함께 올린다.
- `INPUT_NOT_ROUNDTRIP_SAFE` 힌트는 코덱 갭 수정 대기 안내 대신 원본을 보존하는 텍스트 `to-json(section) → patch`·누름틀 `fill` 경로를 안내한다. MCP stamp도 해당 도구 이름으로 같은 안내를 제공한다.

### Fixed

- `convert`(CLI·MCP·Python `convert_md`)가 Markdown 의 대상이 빈 이미지(`![x]()`)를 0.17.0 처럼 `IMAGE_EMBED_SKIPPED` 경고와 함께 뺀다. 0.17.1 에서는 경고 없이 50×30mm 빈 그림 틀로 남았다. 한컴이 자산을 잃은 채 저장한 그림 틀(#266)은 그대로 남는다.
- HWPX 구역(`<hs:sec>`) 바로 아래에 놓여 읽지 못하는 글·요소·`<hp:t>` 를 `CONTENT_LOST (section_content)` 경고로 알리고, `restyle`·`set-cell`·`stamp` 는 그런 문서를 거부한다. 전에는 경고 없이 그 글을 뺐다. 자세한 것은 `hwpforge-smithy-hwpx` 항목을 본다.
- HWP 묶음 안 그림의 테두리 손실 경고가 손실을 판정한 선 종류를 함께 적는다(`border: line type 2, thickness 0`). 전에는 두께만 적어, 두께 0 인 테두리가 왜 손실인지 보이지 않았다(#256 후속).

### 업그레이드 주의

- **HWP 변환의 깊이 초과**: HWP5 디코더는 컨테이너마다 깊이를 1씩 세어 32에서 멈추지만 Core 는 캡션·묶음 안 도형을 한 겹에 2 이상으로 센다. 그래서 디코더 상한 안의 HWP 문서도 Core 깊이 32단계를 넘으면 `convert-hwp5`·`to-pdf` 와 Python `convert_hwp5` 가 `VALIDATION_FAILED` 로 실패할 수 있다. 0.17.1 의 깊이 한도(#267)가 HWP 변환에도 걸린다는 뜻이다. 실제 HWP 문서에서 이렇게 실패하는 경우는 측정하지 않았다.
- **스킬 위치 (#270, 0.17.1)**: 저장소의 에이전트 스킬이 `.claude/skills/` 에서 `skills/` 로 옮겨졌다. 0.17.1 CHANGELOG 에 빠졌던 항목이다.

## [0.17.1](https://github.com/ai-screams/HwpForge/compare/v0.17.0...v0.17.1) - 2026-10-24

0.17.0 뒤에 나온 결함 수정과 입력 한도를 모은 패치 릴리스다. 공개 Rust API 는 바뀌지 않고, `Section::check_nesting_depth()` 하나가 더해진다. 새로 거부하는 입력과 출력이 바뀌는 경우는 아래 "업그레이드 주의" 에 모았다.

### Changed

- HWPX 파트를 읽을 때 XML 이벤트 재생 버퍼가 65,536 이벤트를 넘게 필요하면 한도를 밝힌 오류로 거부한다. 측정한 실제 HWPX 3,671개 중 해당하는 문서는 없다 (#254, #205)
- `Document::validate` 와 사용자 `Section` 을 받는 편집 입구(보존형 patch·preservation export·`set-cell`·provenance·JSON `from-json`·`patch`)가 중첩 깊이 32단계를 넘는 문서를 기존 오류로 거부한다. 전에는 아주 깊게 중첩한 값에서 스택이 넘쳐 프로세스가 죽을 수 있었다 (#267)
- HWP 파일의 다각형·곡선 레코드 하나가 점을 1,048,576개 넘게 선언하면 메모리를 잡기 전에 그 도형을 거부하고 기존 경고를 낸다. 실제 문서에서 측정한 최대는 33점이다 (#265)

### Fixed

- 이미지 자산을 잃은 채 저장된 그림(`binaryItemIDRef=""`)이 변환·재저장 뒤에도 빈 그림 틀로 남고 `inspect` 의 그림 수에 잡힌다. Markdown 은 빈 링크 없이 캡션만 쓰고, `to-pdf` 는 그 자리만 차지한 채 그리지 않는다 (#266)
- 문단 직속에 놓여 읽지 못하는 글·요소를 `CONTENT_LOST` 경고로 알린다. `validate`·`inspect`·`read`·`to-md`·`to-json` 이 그런 문서에서 경고를 함께 내고, `restyle`·`set-cell`·`stamp` 는 의미 손상으로 보고 `INPUT_NOT_ROUNDTRIP_SAFE` 로 거부한다. 전에는 그 글을 뺀 채 다시 썼다 (#263)
- HWP 를 HWPX 로 바꿀 때 여러 문단에 걸친 누름틀이 첫 문단 끝에서 닫히지 않고 끝까지 이어진다. 뒷 문단 글자가 누름틀 밖으로 빠지던 문제다. 표 칸의 계산식 필드는 여전히 변환에서 빠지고 경고만 남는다 (#258)
- HWP 의 묶음 안 그림이 테두리를 잃는다고 잘못 경고하던 문제를 고친다. 이 거짓 경고 때문에 그런 문서의 `stamp`·`set-cell` 이 거부됐다 (#256)
- 글꼴 하나짜리 템플릿 인코드, run 없는 빈 문단이 있는 section 의 보존형 `patch`, 본문 CR 표기 통일은 `hwpforge-smithy-hwpx` 항목을 따른다 (#255, #260, #262)

### 업그레이드 주의

- **새로 거부하는 입력**: XML 이벤트를 65,536개 넘게 버퍼링해야 읽히는 HWPX 파트(#254), 중첩 깊이 32단계를 넘는 문서(#267), 점 1,048,576개를 넘게 선언한 HWP 다각형·곡선(#265). 측정한 실제 문서에는 셋 다 해당하지 않는다.
- **`CONTENT_LOST` 증가 (#263)**: run 밖 글·요소가 문단 바로 아래에 있는 문서는 `CONTENT_LOST (paragraph_content)` 경고를 내고, `restyle`·`set-cell`·`stamp` 는 그 문서를 거부한다. 읽은 내용은 그대로다.
- **출력 바이트 변화**: 본문 CR 은 모든 경로에서 `&#13;` 로 나간다(#262). HWP→HWPX 변환에서 여러 문단 누름틀의 범위와 줄 캐시가 바뀐다(표본 259개 중 5개, #258). 자산 없는 그림이 출력에 남는다(#266). 한컴이 다시 저장할 때 CR 을 지우는 문제(#207)는 다음 minor 에서 다룬다.
- **MCP 입력 상한 (#264)**: `hwpforge-bindings-mcp` 항목을 본다.

### Tests

- 템플릿 여백과 BorderFill 길이 수정을 끝에서 끝까지 고정하는 시험을 더한다 (#253). 공백·CR 하나만 든 run 의 보존형 patch 회귀 시험을 더한다 (#257, Closes #206)

## [0.17.0](https://github.com/ai-screams/HwpForge/compare/v0.16.11...v0.17.0) - 2026-10-23

한컴이 저장한 HWPX 와 HWP5 문서를 변환하고 다시 쓸 때 조용히 사라지거나 바뀌던 값을 Core 에 담아 보존한다. 손실이 남는 자리는 경고(`CONTENT_LOST` 등)로 드러내고, 다시 쓰는 편집 명령은 그런 입력을 거부한다. 공개 Rust 타입이 여럿 바뀌므로 breaking 릴리스다 (#249).

### Changed (BREAKING)

- **Core 개체**: 그리기 개체 열 가지(TextBox, Line, Ellipse, Rect, Polygon, Arc, Curve, ConnectLine, Group, TextArt)와 `Image`·`Table` 에 `frame: ObjectFrame`(원본 크기, 크기 잠금)이 생겼다. `Image::original_size` 는 `Image::frame.original_size` 로 옮겼다. `Control::Group::children` 이 `Vec<GroupChild>` 가 되었고, `Control::Arc` 에 `definition: ArcDefinition` 이 생겼다.
- **Core 구조**: `LineSeg`(`Copy` 도 아님, `boundary` 추가)·`Caption`(`full_size`, `last_width`)·`ObjectPlacement`(정렬, `affect_line_spacing`, `hold_anchor_and_so`)·`MasterPage`(범위·겹침·앞쪽 표시·본문 영역)가 `#[non_exhaustive]` 가 되었다. `Paragraph::end_char_shape`, `TextBox::text_margin`, `ShapeStyle::corner_ratio`(0–50, 넘으면 검증 오류), `Fill::Gradient` 의 중심·단계, `HeaderFooter::anchor_paragraph`, `Table::placement` 가 생겼다. `CoreErrorCode` 에 새 코드가 늘어 전수 매칭에는 와일드카드가 필요하다.
- **글자 그림자**: `HwpxCharShape.shadow_type` 과 Blueprint `shadow` 가 종류·색·오프셋을 담는 `CharShadow` 가 되었다. 색을 주지 않은 그림자의 기본 색이 `#B2B2B2` 에서 한컴 새 문서와 같은 `#C0C0C0` 으로 바뀐다.
- **헤더 참조**: 정의의 `id` 속성이 아니라 목록 위치로 해석한다(한컴 방식). 글자 모양·문단 모양·탭·스타일·글꼴은 0부터, 테두리/배경·번호 매기기·글머리표는 1부터 센다. `push_border_fill` 의 반환값과 `border_fill` 의 인자가 1부터 세는 위치로 바뀌었고 글자·문단 모양의 `source_id` 가 없어졌다. 범위를 벗어난 참조는 오류다.
- **문단 여백·탭 단위**: 문단 여백 다섯 가지와 `TabStop.position` 이 `ParagraphMeasure`(HWPUNIT 또는 1/500 글자)가 되어 원래 단위를 잃지 않는다. `hwp5_style_store_to_hwpx` 는 `Result` 를 반환한다.
- **JSON**: 문서·구역·스타일 JSON 이 `"format_version": 2` 를 쓰고 여백·탭을 `{"unit", "value"}` 로 쓴다. 버전 없는 이전 JSON 은 여백은 `LEGACY_PARAGRAPH_MEASURES` 경고와 함께 읽고, 탭 위치가 들어 있으면 `LEGACY_JSON_TABS` 로 거부한다.
- **HWP5 변환 기본값**: `convert-hwp5`, `ConvertOptions`/`ConvertHwp5Options`, Python `convert_hwp5` 가 줄 배치 캐시를 기본으로 옮긴다. 이전 출력은 `--no-layout-cache`, `with_carry_layout_cache(false)`, `carry_layout_cache=False` 로 얻는다.
- **Markdown·PDF 타입**: `MdOutput` 에 `warnings` 가 생겨 `#[non_exhaustive]` 가 되었다(`MdWarning::ContainerTooDeep`). smithy-pdf 의 `LineImage`·`LaidAnchoredImage` 에 `crop` 이 생겼다.

### Migration

- 구조체 리터럴로 만들던 코드는 생성자와 `with_*` 빌더를 쓰거나 새 필드를 넣고, 패턴에는 `..` 를 붙인다. 예전 JSON 의 개체·그룹·바탕쪽·그림자는 그대로 읽힌다.
- 묶음 자식은 도형을 `GroupChild::Shape(..)` 로 감싸 만들고, 읽을 때는 `GroupChild` 가 non-exhaustive 이므로 와일드카드 팔을 둔다.
- `hwp5_style_store_to_hwpx` 와 `hwp5_style_store_to_hwpx_with_bin_data` 가 반환하는 `Result` 를 처리한다. 선언하지 않은 정의를 가리키는 저장소는 채워 주지 않고 거부한다.
- 그림자의 이전 색이 필요하면 `shadow: {kind: drop, color: '#B2B2B2'}` 처럼 색을 적는다.
- 버전 없는 JSON 으로 편집하던 흐름은 원본 HWPX/HWP5 에서 `to-json` 을 다시 실행한다.
- 테두리/배경을 위치로 조회할 때는 `push_border_fill` 의 반환값을 그대로 쓴다.

### Changed

- **새로 거부하는 입력**: 물리 줄 간격이 홀수인 고정 줄 간격(정확히 쓸 수 없음), 범위 밖 style 을 참조하는 본문, 잘렸거나 모순된 HWP5 IdMappings, 중복되거나 비정상인 ZIP 항목 이름, 한컴의 이름 변환을 거치면 다른 부품을 가리키게 되는 이름, 대소문자만 다른 항목이 둘 이상인 패키지. 다음 스타일이 범위 밖이면 거부하지 않고 자기 자신을 가리키게 하며 손실 경고를 낸다.
- **한컴이 여는 패키지를 받아들임**: 끝 레코드 뒤에 바이트가 붙은 ZIP, `application/hwp+zip` 으로 시작하는 mimetype, UTF-16 으로 쓴 `container.xml`·`content.hpf`, 쓰이지 않는 비정형 항목(바이트 그대로 옮기고, 그 항목을 버리게 되는 저장은 거부).
- **편집 표면**: `restyle`, `set-cell`, `stamp` 는 디코드가 내용을 잃은 입력(`CONTENT_LOST`)을 거부한다. `fill`, `patch`, `insert-para`, `set-cell`, `stamp` 는 XML 1.0 이 쓸 수 없는 문자를 제거하고 `XML_FORBIDDEN_CHARS_REMOVED` 로 알린다. 현재 값과 같은 값의 `fill` 은 변경 없음으로 처리한다.
- **새 경고**: 같은 id 를 쓰는 개체(`OBJECT_ID_CONFLICT`), 같은 manifest id 를 쓰는 항목(`PACKAGE_ID_CONFLICT`), 풀리지 않는 그림 참조(`IMAGE_REFERENCE_UNRESOLVED`), 쪽 시작 번호 0 을 1로 고쳐 씀(`BEGIN_NUM_REPAIRED`), 줄 시작을 정확히 옮길 수 없어 버린 줄 캐시(`LAYOUT_CACHE_DROPPED`).
- **출력이 바뀌는 경우**: 머리말·꼬리말이 컨트롤이 놓인 본문 문단에 고정되고 PDF 도 그 문단의 쪽부터 적용한다. 재인코딩한 HWPX 의 쪽 시작 번호를 1로 쓴다(0 이면 한컴이 0쪽부터 세고 홀짝 바탕쪽을 맞바꿨다). 인코딩에 현재 시각을 쓰지 않고 개체 id 를 1부터 매겨 같은 문서는 같은 바이트로 나온다. 의미가 같은 탭 정의를 내용으로 비교해 무편집 왕복이 원본 바이트를 돌려주는 범위가 넓어졌다. 선 화살표와 호 정의를 보존하므로 이런 선과 호가 든 문서는 JSON·Markdown·HWPX 출력이 달라진다.
- **읽기용 Markdown**: CommonMark 가 마크업으로 읽을 글자만 이스케이프하고, 표 칸 결합 전에 원문을 한 번 이스케이프해 표 칸 안 `<`·`~` 앞에 역슬래시가 붙는 문서가 많다. 링크는 `http://`, `https://`, `mailto:` 만 링크로 쓰고 나머지는 글자만 남긴다. 글상자·각주·도형 안의 표와 그림, 묶음, 표·그림 캡션, 한컴이 저장한 필드의 표시값이 새로 나온다. 표가 8겹을 넘으면 `(중첩 표 생략)` 으로 쓰고 `CONTAINER_TOO_DEEP` 로 알린다. 컨테이너가 32겹을 넘으면 읽기용 출력은 오류로 거부하고, 스타일 출력은 그 아래를 생략하며 같은 경고를 낸다.
- **무손실 Markdown**: 묶음과 그 그림, 글상자·선·타원·다각형·사각형·호·곡선·연결선, 본문 그림의 배치와 캡션, 개체 틀, 표 캡션 속성·셀 구역·고정 셀 크기·셀 줄 바꿈, 그림 하이퍼링크를 왕복한다. 이전에는 이런 문서를 거부했다.
- **PDF**: 자른 그림을 잘라 그리고, 기본 흐름 표의 캡션과 셀 구역 배경을 그린다. 둥근 모서리 글상자, 칠할 수 없는 글상자 채우기, 캡션 번호, 묶음이 든 문단, 위/왼쪽 정렬이 아닌 앵커 그림은 새로 거부한다.

### Fixed

- **HWP5 변환**:
  - 표 캡션, 셀 구역, 고정 셀 크기, 표 여백과 배치, 셀 줄 바꿈을 옮긴다. 이전에는 표 캡션이 경고 없이 사라졌다.
  - 머리말·꼬리말과 그 안의 표·도형, 표 셀 안 머리말·꼬리말을 읽는다. 셀·캡션·글상자·도형·묶음·메모·머리말·각주 안의 컨트롤이 본문과 같은 경로로 읽힌다(묶음 글상자 글자 소실, 쪽 번호가 엉뚱한 문단으로 옮겨져 쪽이 밀리던 문제, 셀 안 겹글자 소실).
  - 도형의 선·채우기(검은 실선과 흰 채우기 대신 원본 값), 선 화살표 모양과 크기, 묶음 자식의 선·채우기·모서리 곡률, 호 종류(NORMAL·PIE·CHORD)와 호 정의 방식을 옮긴다.
  - 묶음이 아닌 도형·그림의 회전과 대칭, 묶음 배치(글자처럼·감싸기·기준·오프셋), 묶음 자식 그림 채우기의 원본 상자를 옮긴다.
  - 개체 원본 크기·크기 잠금, 그림 자르기(이전에는 자른 그림이 전체 이미지로 나왔다), 둥근 모서리, 그러데이션 단계, 글자 그림자, 문단 끝 표시의 글자 모양, 글꼴 종류·대체 글꼴, 단일 줄 입력 모드, 글머리표 너비 조정을 옮긴다.
  - 바탕쪽이 첫 본문 문단을 덮어쓰지 않고 구역의 바탕쪽이 된다. 짧은 색 채우기 형식의 테두리/배경, 구형 14바이트와 체크 글리프 없는 23바이트 글머리표 레코드를 읽는다.
  - 글자 모양 경계를 텍스트 위치로 읽어 탭·개체 뒤 글자가 엉뚱한 모양을 받던 문제, 개체 감싸기 값을 공개 스펙 순서로 읽던 문제, 요약 정보의 생성 시각과 마지막 저장 시각이 뒤바뀐 문제를 고친다.
  - 장식 글자를 포함해 세 글자 이상인 글자 겹치기가 통째로 빠지던 문제를 고친다.
  - 클릭 필드(누름틀)의 채워진 본문과 필드 안 글자 모양들을 유지한다.
- **HWPX**:
  - 필드 시작과 같은 run 의 글자가 필드 본문으로 접혀 사라지던 문제를 고친다. 필드 범위를 한 곳에서 계산해 `fill`·`patch`·`stamp`·Markdown·PDF 가 같은 범위를 쓴다.
  - 상호참조의 표시 본문, run 사이 공백, 표 구역·`noAdjust`·표 배치, 캡션 번호와 속성, 글꼴 종류, 그림 하이퍼링크와 자르기, 개체 틀, 묶음 안 그림·글맵시·글상자 속 표와 그림을 읽고 쓴다.
  - 묶음 도형을 행렬 체인으로 디코드하고, 회전과 대칭이 함께 있는 도형을 한컴 방식으로 쓴다. 바탕쪽을 문단 디코더·인코더로 읽고 쓰며 범위·겹침·본문 영역을 보존한다.
  - 표 머리말 행의 셀별 제목 셀 표시를 읽는다(이전에는 그런 문서가 열리지 않았다). 모양 없음 도형·그림으로 채운 도형이 흰색으로 바뀌던 문제, 두께 0 선이 두꺼워지던 문제를 고친다.
  - `from-json --base` 가 변경하지 않은 문단의 줄 캐시를 유지하고, `fill`·`patch`·`set-cell`·`stamp` 는 편집이 움직인 문단의 캐시를 함께 버린다.
  - Core 가 담지 못하는 값(내장 글꼴, 글맵시 윤곽·그림자, 묶음 하이퍼링크, 묶음 안 수식·차트 등)은 `CONTENT_LOST` 로 알린다.
- **Markdown**: 읽기용 출력의 이스케이프를 하나의 계층으로 모으고 최종 출력을 다시 파싱해 확인한다. 중첩 표 출력이 층마다 두 배로 불어나던 문제와 묶음 안 그림 링크·캡션 그림 바이트가 빠지던 문제를 고친다.
- **PDF**: 머리말·꼬리말이 앵커 문단의 쪽부터 시작하고, 쪽마다 머리말·꼬리말 선택이 선형이 된다.
- **성능**: 글자 모양 경계, OLE 검사, 이미지 자산 복사, 줄 경계 조회, `fill` 캐시 무효화, 중첩 묶음 디코드가 이차 비용에서 선형이 되었다.

### Security

- zip 9 로 올려 zip 8 의 보안 권고 RUSTSEC-2026-0367, -0368, -0369 를 해소한다. 권고는 추출 경로에 있고 이 워크스페이스는 그 경로를 호출하지 않지만 의존성 감사가 실패했다.
- 패키지 읽기를 하나의 검사된 진입점으로 통일한다(이전에는 `patch`·`stamp` 와 레이아웃 힌트 패처가 검사를 건너뛰었다). 실패한 읽기의 바이트도 누적 상한에 센다. 바탕쪽을 디코드 크기 예산에 센다.
- HWP5 채우기 길이를 할당 전에 검사하고(수 바이트 레코드가 기가바이트를 요청하던 문제), 도형 안 도형을 16단계, 표 레코드가 선언한 셀 수를 예산으로 제한한다. 잘못된 입력에서 패닉하던 묶음 자식 비교를 고친다.

### Known limitations

- 패키지의 manifest 연결을 부품 찾기가 따르지 않는 경우가 있고, 대소문자만 다른 항목은 거부한다. 한컴과 같은 항목을 고르는 지원은 다음 판에서 다룬다.
- HWP5 변환이 여러 문단에 걸친 누름틀을 첫 문단 끝에서 닫는다. 화면은 같지만 필드 범위가 줄어든다.
- 서식이 섞인 누름틀(본문 run 이 둘 이상)은 `fill` 이 거부한다.
- `to-json` 다음 `from-json` 은 그림 채우기의 이미지를 패키지로 옮기지 않아 경고와 함께 그림이 빠진다.
- 내장 글꼴과 그림 글머리표의 이미지는 옮기지 않고 경고로 알린다. 묶음 안 차트와 글맵시는 원본 배치 속성을 따르지 않는다.
- 무손실 Markdown 은 안쪽 여백이 기본값(283)이 아닌 글상자, 둥근 모서리 글상자, 캡션 번호, 문단이 없는 글상자, 점이 세 개 미만인 다각형, 32겹을 넘는 컨테이너를 거부한다.
- 무손실 Markdown 은 이전 판이 읽지 못하는 요소를 쓰고, 머리말·꼬리말처럼 다루는 범위 밖의 값을 오류 없이 뺀다. `Unknown` 컨트롤은 바탕쪽 문단과 메모 앵커 run 에서 경고 없이 빠진다.
- 표 제목 칸 테두리 일부, 기울이기 개체, 일부 문서의 줄바꿈 위치와 쪽 번호 높이가 한컴 결과와 다르다.

## [0.16.11](https://github.com/ai-screams/HwpForge/compare/v0.16.10...v0.16.11) - 2026-10-13

### Fixed

- *(bindings)* MCP 서버가 `initialize` 응답에 알리는 제목을 패키지 설명과 같은 이름 `Vise — HwpForge MCP Server` 로 바꾼다(시작 로그도 같음). 식별자 `hwpforge-mcp` 는 그대로다. Python `help(hwpforge)` 첫 줄은 `Tongs — …` 로 시작한다 (#234)

### Documentation

- 한컴이 HWP 문서 파일 공개 문서를 이용할 때 요구하는 표기("본 제품은 한컴의 HWP 문서 파일(.hwp) 공개 문서를 참고하여 개발하였습니다.")를 README, 문서 사이트, `hwpforge --help`, MCP 서버 정보, Python README, HWP5 크레이트 rustdoc 에 넣는다 (#232)

- 계층과 창구의 대장간 비유 이름을 하나로 정한다. Core = Anvil(모루), CLI = Hammer, MCP 서버 = **Vise**(예전 Anvil), Python = Tongs. 패키지·바이너리 이름은 바뀌지 않고 crates.io·npm·PyPI 설명 문구만 바뀐다. 정본은 `branding/BRANDING.md` 의 "대장간 이름 체계" (#233)

## [0.16.10](https://github.com/ai-screams/HwpForge/compare/v0.16.9...v0.16.10) - 2026-10-04

### Fixed

- *(hwpx)* 편집 경로(`fill`·`patch`·`insert-para`·`delete-para`·`set-cell`·`stamp`·섹션 export)가 HWPX 엔트리를 크기 제한 없이 풀던 문제를 고친다. 디코더와 같은 한도(엔트리 50 MiB, 전체 500 MiB)를 적용하고, ZIP 헤더에 적힌 크기로 메모리를 미리 잡지 않는다. 편집 경로는 디코더가 읽지 않는 엔트리까지 모두 세므로, 50 MiB 를 넘는 엔트리가 있는 패키지는 편집이 거부된다(일반 읽기에는 영향 없음)

- *(hwp5)* HWP 를 HWPX 로 바꿀 때 차트 데이터를 제한 없이 풀던 문제를 고친다. 차트는 다른 HWP5 스트림과 같은 한도(스트림 500 MB, 압축비 100배)를 따르고, 이미지·차트 해제는 모두 문서 전체 예산(2 GiB)에 합산된다(이미지는 압축비 제외 — 정상 BMP 가 93배까지 나옴). 한도를 넘은 차트는 `DroppedControl` 경고와 함께 빠지고, 예산을 넘으면 문서 변환이 실패한다. 예전에 빠지던 zlib 형식 차트는 이제 변환되고, 손상된 차트의 경고 문구에 `Record parse error at offset 0: decompression failed:` 가 붙는다

- *(md)* `to-md` 의 수식 변환이 깊게 중첩된 수식에서 스택을 넘쳐 프로세스가 죽던 문제를 고친다. 중첩 32단에서 멈추고, 그보다 깊은 수식은 `[수식 변환 생략: 중첩 깊이 초과] ` 뒤에 공백을 접은 원문을 그 자리의 일반 텍스트처럼 이스케이프해 쓴다. 실제 문서 수식(측정 코퍼스 최대 3단)의 출력은 그대로다


## [0.16.9](https://github.com/ai-screams/HwpForge/compare/v0.16.8...v0.16.9) - 2026-10-03

### Fixed

- *(hwpx)* HWP 를 HWPX 로 변환할 때 여러 줄 글맵시(TextArt)의 줄바꿈이 공백으로 합쳐지던 문제를 고친다. 줄바꿈은 한컴처럼 `␍␊`(U+240D U+240A)로 쓰고 읽을 때 `\r\n` 으로 푼다. 이전 버전이 만든 HWPX 는 줄바꿈이 이미 공백으로 저장돼 있으므로 원본 HWP 에서 다시 변환해야 한다 (#199)

- *(hwpx)* `to-json`·MCP·Python 에서 한컴 HWPX 의 글맵시 `text` 가 `␍␊` 대신 `\r\n` 으로 보인다. 예전 JSON 의 `␍␊` 도 계속 줄바꿈으로 읽는다 (#199)

### Changed

- 워크스페이스 MSRV 를 Rust 1.89 로 올린다. 간접 의존성 `uuid` 1.27 이 rustc 1.89 를 요구한다 (krilla 경로의 네 크레이트는 그대로 1.92)


## [0.16.8](https://github.com/ai-screams/HwpForge/compare/v0.16.7...v0.16.8) - 2026-10-01

### Fixed

- *(hwpx)* 메모 본문·묶음 글상자 안의 탭·줄바꿈·하이퍼링크가 내부 치환 문자열로 저장되던 문제를 고친다 (#195)

- *(hwpx)* HWPX 를 쓸 때 XML 이 금지하는 제어 문자를 지우고 `XML_FORBIDDEN_CHARS_REMOVED` 경고를 낸다 (#198)

- *(hwpx)* 33겹 이상 중첩된 묶음 도형은 조용히 사라지지 않고 `InvalidStructure` 로 거부된다. 그런 문서는 편집 명령도 거부한다 (#196)

- *(md)* 무손실 Markdown 왕복에서 `&`·`<`·`>`·따옴표가 사라지던 문제를 고친다 (#197)

- *(core)* 이미지가 여럿인 문서도 HWPX 출력 바이트가 실행마다 같다 (#200)

- *(cli)* `census-hwp5 --companion` 이 깨진 section XML 에서 잘린 목록을 `status: ok` 로 돌려주던 것을 `HWPX_CENSUS_FAILED` 로 끝낸다. `inspect` 도 그런 section 에서 실패할 수 있다 (#201)


## [0.16.7](https://github.com/ai-screams/HwpForge/compare/v0.16.6...v0.16.7) - 2026-10-01

### Changed

- *(hwpx)* HWPX 의 `header.xml`·`section*.xml` 이 XML 224 단계보다 깊게 중첩되거나 namespace 바인딩이 128개를 넘으면 `DECODE_FAILED` 로 거부한다 (224 는 잠정값, 표·글상자 32겹은 계속 읽힌다). 최적화 빌드의 디코드 스레드 스택은 2 MiB 를 권장한다


### Documentation

- *(readme)* link the docs site from the root and MCP crate READMEs


### Fixed

- *(hwpx)* quick-xml 0.42 로 올리면서 한컴이 읽는 결과는 0.41 과 같게 맞춘다 — 속성 값의 탭·줄바꿈·CR 은 공백으로 쓴다

- *(hwpx)* 공백 문자 참조 (`&#13;` 등) 만 든 텍스트 run 이 읽을 때 사라지지 않고, 공백만 든 run 이 서식 run 사이에 있는 문단의 줄 조판 캐시가 유지된다

- *(hwpx)* 글꼴 이름은 읽고 쓸 때 제어 문자를 지우고 앞뒤 ASCII 공백을 자른다 (끝에 공백이 남으면 한컴이 글꼴을 찾지 못한다)


## [0.16.6](https://github.com/ai-screams/HwpForge/compare/v0.16.5...v0.16.6) - 2026-09-22

### Added

- *(bindings-cli)* `validate` 명령 신설 — MCP·Python 에만 있던 검증 연산을 CLI 에도

- *(hwpforge)* from_json 이 입력 JSON 의 layout_cache 를 버릴 때 LAYOUT_CACHE_DROPPED 경고를 낸다

- *(hwpforge)* ops 가 세 창구의 정보를 전부 담도록 — 프리셋 균일 적용, inspect 얕은 계수, from_json 기본 레지스트리, stamp·restyle·convert 상세


### Changed

- *(hwpforge,bindings-mcp,bindings-py,bindings-cli)* InspectSection 의 미릴리스 계수 필드 이름이 스코프를 말하게 한다

- *(bindings-mcp,hwpforge)* MCP 요청 스키마가 smithy-hwpx 의 serde 타입을 직접 노출하지 않는다

- *(hwpforge,bindings-cli,bindings-mcp,bindings-py)* 세 창구가 각자 갖던 크기 게이트·경고 DTO·매니페스트 경로 규칙을 ops 로 올린다

- *(hwpforge,bindings-cli)* inspect 가 디코드를 한 번만 한다 — CLI 전용 심층 계수를 ops 로 올린다


### Documentation

- PDF 렌더의 전제를 "한컴 저장본" 이 아니라 "조판 캐시" 로 적는다

- README 퀵스타트와 Rust 예시가 적힌 그대로 실행되게 한다

- *(hwpforge,bindings-cli,bindings-mcp)* 리뷰 상환 — 문서가 재는 것과 한도를 정확히 말한다

- *(python)* Python 바인딩 0.16.5 첫 wheel 배포를 문서에 반영


### Fixed

- *(hwpforge,bindings-cli)* 힌트가 실제로 되는 길을 말하게 하고 대체 기록 체계를 둔다

- *(hwpforge,bindings-cli)* read 거부 문구가 창구 공통 인자 이름을 쓰게 한다

- *(hwpforge,bindings-mcp)* 진단 정확화 — 거짓 NOTE 제거, Io 힌트 함정 수리, read 죽은 reason 단일화

- *(hwpforge)* 공유 입력 게이트 fs·OpsError::Io 를 ops-hwpx 에서 쓸 수 있게

- *(hwpforge)* read_bounded 의 상한 덧셈을 포화시켜 u64::MAX 상한에서 오버플로하지 않는다

- *(hwpforge,bindings-cli)* 리뷰 상환 — CLI 심층 계수는 raw 스캔으로 통일, 창구 합의 테스트를 도구별로, 순회 하나로

- *(bindings-cli)* validate 의 무효 문서를 종료코드 3 으로 분리, to_pdf 경고 렌더를 ops 에서 가져오고, 남은 감사 지적을 정리한다

- *(hwpforge,bindings-mcp)* restyle 가 디코드 경고를 잇고, MCP 구조 편집 도구가 디코드 경고를 걸러내지 않는다

- *(hwpforge,bindings-mcp)* 재평결 상환 — 메모 anchor_runs 캐시 탐지 복원, 그룹 자식 번호를 인코더 기준으로, 테스트 잠금

- *(hwpforge,bindings-mcp)* 리뷰 상환 — 캡션 캐시 탐지, 경고 경로 모양, 인라인 크기 게이트, 직렬화 단언

- *(hwpforge)* W2 에서 더한 Meta 필드에 serde 기본값 — 0.16.5 가 쓴 JSON 을 계속 읽는다


## [0.16.5](https://github.com/ai-screams/HwpForge/compare/v0.16.4...v0.16.5) - 2026-09-17

### Added

- *(smithy-hwpx)* fill·insert·delete 진단 쌍둥이, stamp 의 코덱 양방향 경고, umbrella 배선

- *(hwpforge)* ops 스타일·마크다운·스키마 작업 — templates·restyle·validate·to_md·convert_md·decode_md·schema

- *(hwpforge)* ops 편집·스탬프 작업 — fill·set_cell·insert_para·delete_para·stamp_plan·stamp

- *(hwpforge)* ops 조회·교환 작업 — outline·read·fields·to_json·export_section·from_json·patch·diff·InspectMeta

- *(hwpforge)* ops 오류 모델을 단계 구분으로 — Decode/Encode·MdDecode/MdEncode, 의미 손상 봉투, 인자 거부 코드

- *(hwpforge)* ops 층 골격 — 오류·경고 모델, feature, 파일 해석 헬퍼, inspect, 인벤토리 테스트


### Documentation

- 브랜치 전체 리뷰 반영 — 릴리스 이력 포인터·architecture mermaid·MSRV 예외 정정

- 공개 문서를 0.16.4 실측에 맞게 정정 — CLI 22·MCP 19·smithy-pdf·설치 명령·정책 문서


### Fixed

- *(hwpforge)* `fields` 의 wire 응답에 admission 디코더 경고를 싣는다

- *(hwpforge)* ops 가 성공 경로의 경고를 버리지 않도록 진단 보존 API 에 배선, Meta non_exhaustive, 인벤토리 정밀화


## [0.5.0](https://github.com/ai-screams/HwpForge/compare/v0.4.0...v0.5.0) - 2026-03-22

### Added

- *(list)* **BREAKING** add shared list semantics


### Documentation

- refresh readme and fix docs lint


### Fixed

- *(hwpx)* bridge registry-local style ids


## [0.4.0](https://github.com/ai-screams/HwpForge/compare/v0.3.0...v0.4.0) - 2026-03-20

### Changed

- Promote the workspace release line to `0.4.0` for the breaking tab semantics contract added across `hwpforge-core` and `hwpforge-blueprint`.
- Preserve explicit tab definitions and paragraph tab references through the HWP5/HWPX conversion path.

### Migration

- Downstream code constructing `hwpforge_core::TabDef` with struct literals must initialize the new `stops` field.
- Downstream code constructing blueprint templates or paragraph shapes with struct literals must initialize the new tab-related fields.
- Consumers matching exhaustively on `hwpforge_blueprint::BlueprintErrorCode` should handle the new tab error variants.

## [0.3.0](https://github.com/ai-screams/HwpForge/compare/v0.2.1...v0.3.0) - 2026-03-19

### Changed

- Promote the workspace release line to `0.3.0` to reflect the breaking `ExportedSection` contract in the HWPX section editing workflow.
- Align the preserving section export/patch path across CLI and MCP, including explicit warnings and stricter section edit validation.

### Migration

- Any downstream Rust code constructing `hwpforge_smithy_hwpx::ExportedSection` via struct literals must add the `preservation` field.
- Section editing clients should refresh their `to-json --section` exports before patching; stale and legacy preservation metadata is rejected by design.

## [0.2.1](https://github.com/ai-screams/HwpForge/compare/v0.2.0...v0.2.1) - 2026-03-17

### Fixed

- *(docs)* unescape HTML entities in details/summary tags


## [0.2.0](https://github.com/ai-screams/HwpForge/compare/v0.1.5...v0.2.0) - 2026-03-17

### Changed

- Adopt the `hwpforge-core` v0.2.0 contract for richer table and image semantics across the umbrella crate feature surface.
- Align workspace crate versions on the `0.2.0` release line.

### Migration

- Downstream code should stop constructing `Table`, `TableRow`, `TableCell`, and `Image` with struct literals and move to constructors/builders.
- Consumers that inspect validation codes should handle `CoreErrorCode::NonLeadingTableHeaderRow`.

## [0.1.5](https://github.com/ai-screams/HwpForge/compare/v0.1.4...v0.1.5) - 2026-03-10

### Fixed

- *(dist)* improve user experience for npm installation


## [0.1.3](https://github.com/ai-screams/HwpForge/compare/v0.1.2...v0.1.3) - 2026-03-09

### Added

- *(examples)* reorganize examples and add 16 HWPX showcase files


## [0.1.1](https://github.com/ai-screams/HwpForge/compare/v0.1.0...v0.1.1) - 2026-03-07

### Documentation

- *(readme)* add supported Hancom versions table and cargo install instructions

- update LICENSE-APACHE to full text and add README badges


### Fixed

- *(readme)* replace broken Buy Me a Coffee button with stable CDN image

- use absolute URLs for README images (crates.io compatibility)
