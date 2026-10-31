# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.19.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.18.0...hwpforge-smithy-hwpx-v0.19.0) - 2026-10-31

### Changed

- *(core)* **BREAKING** [#296] read paragraph lists as one typed reference ([#301](https://github.com/ai-screams/HwpForge/pull/301))

- *(core)* **BREAKING** [#279] make paragraph style references format-neutral and link Markdown to template styles ([#295](https://github.com/ai-screams/HwpForge/pull/295))

- *(smithy-hwpx)* [#280] define repeated HWPX wire values in one place ([#292](https://github.com/ai-screams/HwpForge/pull/292))

- *(smithy-hwpx)* [#282] drop the unused MAX_STYLE_INDEX checks ([#290](https://github.com/ai-screams/HwpForge/pull/290))


### Fixed

- *(smithy-hwpx)* [#286] read XML references in chart values and CLI text records ([#289](https://github.com/ai-screams/HwpForge/pull/289))

- *(smithy-hwpx)* [#282] drop only the metadata value that is lost and report it ([#287](https://github.com/ai-screams/HwpForge/pull/287))


## [0.18.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.17.1...hwpforge-smithy-hwpx-v0.18.0) - 2026-10-25

### Changed (BREAKING)

- `CellEditError`·`StamperError`·`StructuralEditError`의 `Codec(String)`을 `Codec(CodecFailure)`로 바꾼다. stage·leaf code·detail을 보존하고 기존 Clone/Eq를 유지한다. 세 enum에 `Invariant { detail }`, 구조 편집에 `ProtectedFieldRange { section, index, detail }`를 추가한다.
- preserve-first patch 의 오류 타입 `PatchError` 를 새로 둔다. variant 는 `Codec(CodecFailure)`·`Rejected(HwpxError)`·`SectionOutOfRange`·`ProtectedFieldRange`·`Invariant` 이고 `#[non_exhaustive]` 다. `patch_section_preserving`·`patch_section_preserving_with_diagnostics`·`patch_section_preserving_with_codec_diagnostics`·`export_section_preservation` 의 오류 반환형이 `HwpxError` 에서 `PatchError` 로 바뀌고, `SectionWorkflowError::PreservingPatch` 는 `PatchError` 를 담는다. `Codec` 은 실패 단계(`CodecStage`)와 원래 오류의 코드·메시지만 담고 원래 오류의 source chain 은 담지 않는다.
- `FillError` 에 `Codec(CodecFailure)`·`Patch(PatchError)`·`Invariant { detail }` 을 더하고 `From<HwpxError> for FillError` 를 지운다. 입력 decode 실패는 `Codec`, 섹션 patch 실패는 `Patch`, 필드 목록 진단은 계속 `Workflow` 다. `FillError` 는 계속 exhaustive 다.

- 본문의 CR(`\r`)을 줄바꿈으로 다룬다. 한컴은 본문의 CR을 줄바꿈으로 보여 주지만 다시 저장하면 지워 앞뒤 글을 붙인다. 인코더는 CR을 `<hp:lineBreak/>`로 쓰고 한 글 조각 안의 CRLF는 줄바꿈 하나로 접으며, HWPX 디코더는 CR 참조를 LF 하나로 읽는다. 수식 스크립트·글맵시·속성값은 기존 규칙을 유지한다. CRLF를 접느라 줄 배치 위치가 어긋나는 문단은 줄 배치 캐시를 버리고 `LayoutCacheDropped` 경고로 알린다. `fill`과 보존 `patch`는 같은 접기를 거친 값으로 비교하므로 같은 CR 글을 두 번 써도 바뀌지 않는다. 이행: HWPX에서 읽은 본문이 `\r` 대신 `\n`을 돌려주므로 `\r`에 기대던 코드를 고치고, 0.17에서 내보낸 보존 metadata 중 CR이 든 칸이 있으면 다시 내보낸 뒤 patch한다.

### Changed

- stamp·cell 코덱 오류 메시지에서 `output decode:`, `fixed-point decode:`, `reference decode:`, `layout carry:`, `zip:` 같은 호출 문맥 접두를 제거한다. `CodecFailure.detail`은 원래 오류의 문자열을 한 번만 보존하며 실패 단계는 `stage`로 구분한다.

- Draft 셀 편집 깊이 초과는 `NestingTooDeep { section: usize, reason }`로 반환하며 모든 구역을 편집 전에 검사한다. Display는 `nesting depth exceeds the limit: {reason}`다. 입력 바이트의 깊이 초과는 decode 단계다.
- admission helper는 호출 목적에 따라 validate·encode 오류를 구분한다. layout carry는 최종 ZIP 작성만 Encode, 기존 나머지 오류는 Verification이며 기존 MissingFile·텍스트 읽기·정렬 실패의 무음 복구 정책을 유지한다.

- 공개 decode는 구역 조립 직후 본문·머리말·꼬리말·바탕쪽의 Core 깊이를 검사하고 32 초과를 `HwpxError::Core`(E4008, `nesting_depth`)로 거절한다. 파서가 통과하던 묶음·글상자 및 그림 캡션 경로도 포함하며, 이런 입력의 stamp-plan도 이제 실패한다. 다른 Core 구조 검증은 수행하지 않는다.
- 표·subList·묶음 파서 가드(E4006)는 열려던 층을 1부터 세고 실제 part 경로를 메시지에 포함한다. 파서 가드(E4006)·XML(E4003) leaf는 유지하며 편집기에서도 연산 코드는 `DECODE_FAILED`다.
- 줄바꿈·묶음 빈칸·고정폭 빈칸이 든 글 칸을 `patch`·`fill`로 고칠 수 있다. 한컴에서 Tab 키로 넣은 탭은 너비 정보가 있어 계속 거부한다.
- 본문 run, 글 칸(`<hp:t>`), 컨트롤(`<hp:ctrl>`), `<hp:subList>` 안에서 읽지 못해 조용히 버리던 내용을 `CONTENT_LOST` 경고로 알린다. 종류는 OLE 개체 `ole`, 모르는 요소 `unknown_element`, 차트가 아닌 `switch`, 되살리지 못한 차트 `chart`(차트 XML 이 없는 본문 차트 포함), run 바로 아래 글 `run_text`, 글 칸 안 모르는 요소 `inline_element`, 컨트롤 안 모르는 내용 `ctrl_content`, subList 안 문단 아닌 내용 `sublist_content` 다. 본문 최상위뿐 아니라 머리말·꼬리말·표 칸·각주·미주·글상자에서도 알린다. 미지 내용 때문에 각주·미주 번호 머리 인식에서 빠진 경우(이때 번호 뒤 간격 글이 남는다)를 제외하면 읽은 문서 내용은 같다.
- 이 경고가 있는 문서는 내용을 다시 쓰는 편집(`restyle`·`set-cell`·`stamp`)이 문서 전체를 거부한다. 원문을 보존하는 `fill`·`patch` 와 `insert-para`·`delete-para` 는 전과 같다. 측정한 HWPX 372개 중 새 경고가 생긴 문서는 5개(`ole` 4개, `unknown_element` 1개)이고, 그중 새로 거부되는 문서는 1개다.
- patch 의 깊이 거부·보호 필드 거부 메시지에서 앞에 붙던 `Invalid HWPX structure:` 가 빠지고, 내부 불변식 실패 메시지는 `internal invariant violated:` 로 시작한다. 그 밖의 거부·decode 메시지는 그대로다.
- 구역(`<hs:sec>`) 바로 아래의 글·모르는 요소·`<hp:t>` 처럼 문단이 아닌 내용도 `CONTENT_LOST` 경고(종류 `section_content`, 위치는 그 구역)로 알리고, 위 편집기들은 그런 문서를 거부한다. 문단 사이의 들여쓰기 공백은 알리지 않는다. 측정한 HWPX 372개 중 이 경고가 새로 생긴 문서는 없다.
- 각주·미주 첫 문단의 번호 머리 run 에 위 내용이 섞이면 번호 머리로 보고 떼어 내지 않고, 경고와 함께 일반 run 으로 읽는다. 형광펜·제목 표지만 섞인 번호 머리는 전처럼 떼어 낸다.

- stamp `apply`·`apply_cells`, 격자 주소 함수 네 개, registry rebind는 중첩이 32단계를 넘는 문서를 순회하기 전에 기존 오류 variant로 거부한다. 오류를 반환할 수 없는 `stamp::plan`은 대신 반복 방식으로 순회한다.

### Added

- 편집기 오류가 싣는 `CodecStage`(`Decode`·`Admission`·`Validation`·`Encode`·`Verification`)와 `CodecFailure`(`stage`·`code`·`detail`)를 공개한다. `CodecStage`는 일부러 `#[non_exhaustive]`가 아니어서 단계를 더하면 breaking이다.

## [0.17.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.17.0...hwpforge-smithy-hwpx-v0.17.1) - 2026-10-24

### Changed

- HWPX 파트(`header.xml`·`section*.xml`)를 읽을 때 XML 이벤트 재생 버퍼가 65,536 이벤트를 넘게 필요하면 `more than 65536 buffered XML events; HwpForge does not read documents that require more` 로 거부한다. 흩어진 목록 항목을 모으느라 쌓는 버퍼에 한도가 없어, 조작한 파트 하나가 디코드 한 번에 이벤트 수백만 개를 붙잡을 수 있었다. 측정한 실제 HWPX 3,671개 중 가장 많이 필요한 문서가 90 이벤트라 해당하는 문서는 없다. 보존형 `patch` 경로는 기존 오류 코드 `PATCH_FAILED` 를 낸다 (#254, #205)
- 보존형 patch, preservation export, 셀 편집(`apply_set_cells`), provenance 확인·복원이 사용자가 넘긴 `Section` 의 중첩 깊이를 먼저 검사해 32단계를 넘으면 각 입구의 기존 오류로 거부한다. 전에는 수천 단계 이상 중첩한 값에서 스택이 넘쳐 프로세스가 죽을 수 있었다 (#267)

### Fixed

- 이미지 자산을 잃은 채 저장된 그림(`binaryItemIDRef=""`)을 버리지 않고 빈 그림 틀로 읽고 다시 쓴다. 한컴은 이런 그림을 원래 크기의 빈 틀로 보여 준다. 크기·위치·감싸기·자르기 같은 틀 속성은 그대로이고, 매니페스트에 가짜 항목을 만들지 않으며, 새 경고도 없다. `<hc:img>` 자체가 없는 그림과 비어 있지 않은 끊긴 참조는 전과 같다 (#266)
- 글꼴을 하나만 등록한 템플릿으로 HWPX 를 만들면 기본 글자 모양이 존재하지 않는 글꼴 슬롯 1을 가리켜 인코드가 멈추던 문제를 고친다. 글꼴이 하나뿐이면 그 글꼴을 쓴다. 글꼴이 둘 이상인 템플릿과 프리셋의 출력은 바이트까지 그대로다 (#255)
- `<hp:p>` 바로 아래에 `<hp:run>` 밖으로 놓인 `<hp:t>`·글·모르는 요소를 조용히 버리던 것을 `CONTENT_LOST` (`paragraph_content`) 경고로 알린다. 읽은 문서 내용은 바뀌지 않고, 버려진 글을 추측해 읽지도 않는다. 측정한 HWPX 372개에는 새 경고가 생긴 문서가 없다 (#263)
- `<hp:run>`·`<hp:t>` 없이 줄 조판 정보(`<hp:linesegarray>`)만 든 빈 문단이 있는 section 도 보존형 `patch` 로 편집할 수 있다. 전에는 `to-json --section` 이 "no original slot" 경고를 내고 그 section 전체에서 보존형 patch 를 쓸 수 없었다. 편집하지 않은 patch 는 패키지를 바이트 그대로 돌려주고, 다른 문단을 고치면 빈 문단의 줄 캐시도 함께 지워 자식 없는 `<hp:p ...></hp:p>` 로 나간다. 그런 빈 문단에 글자를 넣는 편집은 구조 변경이라 기존 `InvalidStructure` 로 거부한다 (#260)
- 본문 글자의 CR(`\r`)을 모든 text 경로에서 `&#13;` 로 쓴다. 탭·줄바꿈이 섞인 run, InlineText 조각, 필드·메모 본문, `patch` 로 고친 칸이 쓰던 공유 경로는 날 CR 바이트를 써서, 읽을 때 LF 로 바뀌어 같은 글자가 경로에 따라 `\r` 또는 `\n` 으로 돌아왔다. 이 때문에 한 run 에 CR 과 탭이 함께 든 문서를 `set-cell` 이 거부하던 문제도 풀린다. 편집하지 않은 칸은 원본 바이트를 그대로 둔다 (#262, Refs #207)

### Tests

- 공백 하나나 CR 하나만 든 run 이 있어도 보존형 `patch` 가 원본 바이트를 지키는 동작을 회귀 시험으로 고정한다. 0.17.0 에서 이미 고쳐졌고 제품 코드는 바뀌지 않는다 (#257, Closes #206)

## [0.17.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.16.11...hwpforge-smithy-hwpx-v0.17.0) - 2026-10-23

### Changed (BREAKING)

- 헤더 참조를 정의의 `id` 가 아니라 목록 위치로 해석한다(한컴 방식). `push_border_fill` 의 반환값과 `border_fill` 의 인자가 1부터 세는 위치가 되었고, 글자·문단 모양의 `source_id` 가 없어졌다. 인코더는 빈칸 없이 다시 번호를 매긴다.
- `HwpxCharShape.shadow_type` 이 `shadow: CharShadow` 가 되었다. 이전 `shadow_type` JSON 은 계속 읽힌다.
- 문단 여백과 탭을 단위 그대로(`ParagraphMeasure`) 읽고 쓴다. 물리 줄 간격이 홀수인 고정 줄 간격은 쓸 때 오류다.
- 교환 JSON(`exchange`)이 `format_version: 2` 와 `{"unit", "value"}` 여백·탭을 쓴다. 버전 없는 JSON 은 탭 위치가 있으면 `LegacyJsonTabs` 로 거부하고, 없으면 여백을 풀어 읽으며 경고한다.

### Changed

- 패키지 읽기를 하나의 검사된 진입점으로 통일했다. 중복·비정상 이름, 한컴 이름 변환으로 바뀌는 이름, 대소문자만 다른 항목 둘 이상을 거부하고, 한컴이 여는 패키지(끝 레코드 뒤 바이트, `application/hwp+zip` 접두 mimetype, UTF-16 패키지 XML, 쓰이지 않는 비정형 항목)는 받아들인다.
- 새 경고: `CONTENT_LOST`(디코드·인코드), 개체·manifest id 충돌, 풀리지 않는 그림 참조, 쪽 시작 번호 보정. 편집 표면은 XML 1.0 금지 문자를 제거하고 알린다.
- 쪽 시작 번호를 1로 쓰고, 인코딩에 현재 시각을 쓰지 않으며 개체 id 를 1부터 매긴다.

### Fixed

- 필드 시작과 같은 run 의 글자가 필드 본문으로 접혀 사라지던 문제, 상호참조 표시 본문, run 사이 공백.
- 표 구역·`noAdjust`·배치, 캡션 번호와 속성, 글꼴 종류, 글자 그림자, 그림 하이퍼링크와 자르기, 개체 틀, 묶음 안 그림·글맵시·글상자 속 표와 그림, 표 머리말 행의 셀별 제목 셀 표시.
- 묶음 도형의 행렬 체인 디코드, 회전과 대칭이 함께 있는 도형, 바탕쪽의 범위·겹침·본문 영역, 모양 없음 도형과 두께 0 선.
- `from-json --base` 가 변경하지 않은 문단의 줄 캐시를 유지하고, 편집 명령은 움직인 문단의 캐시를 함께 버린다. 의미가 같은 탭 정의를 내용으로 비교한다.
- zip 9 로 올려 RUSTSEC-2026-0367, -0368, -0369 를 해소한다.

## [0.16.10](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.16.9...hwpforge-smithy-hwpx-v0.16.10) - 2026-10-04

### Documentation

- correct the statements a fact check found wrong

- *(readme)* match the READMEs to the code and drop volatile counts


### Fixed

- *(hwpx)* 편집 경로(`fill`·`patch`·`insert-para`·`delete-para`·`set-cell`·`stamp`·섹션 export)가 HWPX 엔트리를 크기 제한 없이 풀던 문제를 고친다. 디코더와 같은 한도(엔트리 50 MiB, 전체 500 MiB)를 적용하고, ZIP 헤더에 적힌 크기로 메모리를 미리 잡지 않는다. 편집 경로는 디코더가 읽지 않는 엔트리까지 모두 세므로, 50 MiB 를 넘는 엔트리가 있는 패키지는 편집이 거부된다(일반 읽기에는 영향 없음)


## [0.16.9](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.16.8...hwpforge-smithy-hwpx-v0.16.9) - 2026-10-03

### Fixed

- *(hwpx)* HWP 를 HWPX 로 변환할 때 여러 줄 글맵시(TextArt)의 줄바꿈이 공백으로 합쳐지던 문제를 고친다. 줄바꿈은 한컴처럼 `␍␊`(U+240D U+240A)로 쓰고 읽을 때 `\r\n` 으로 푼다. 이전 버전이 만든 HWPX 는 줄바꿈이 이미 공백으로 저장돼 있으므로 원본 HWP 에서 다시 변환해야 한다 (#199)

- *(hwpx)* `to-json`·MCP·Python 에서 한컴 HWPX 의 글맵시 `text` 가 `␍␊` 대신 `\r\n` 으로 보인다. 예전 JSON 의 `␍␊` 도 계속 줄바꿈으로 읽는다 (#199)

- *(hwpx)* 다른 도구가 글맵시 줄바꿈을 `&#10;`·`&#13;` 로 쓴 HWPX 도 `\r\n` 으로 읽어, 다시 쓰고 읽어도 글자가 바뀌지 않는다 (#199)


## [0.16.8](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.16.7...hwpforge-smithy-hwpx-v0.16.8) - 2026-10-01

### Fixed

- *(hwpx)* 메모 본문·묶음 도형 글상자 안의 탭·줄바꿈·하이퍼링크가 내부 치환 문자열(`__HWPTXT_…`)로 바뀌어 저장되던 문제를 고친다. 중첩된 치환을 끝까지 풀고, 그래도 남으면 깨진 파일 대신 `InvalidStructure` 오류로 멈춘다 (#195)

- *(hwpx)* serde 로 쓰는 part(`header.xml`·`section*.xml`)에서 XML 1.0 이 금지하는 제어 문자(U+0001 등)를 지우고 `XML_FORBIDDEN_CHARS_REMOVED` 경고를 낸다. 이전에는 표준 XML parser 가 거부하는 파일이 경고 없이 나왔다. TAB·LF·CR 은 그대로 (#198)

- *(hwpx)* 묶음 도형이 33겹 이상 중첩되면 묶음 전체가 경고 없이 사라지던 것을, 표와 같이 `InvalidStructure` (`group nesting depth 32 exceeds limit of 32`) 로 거부한다 (#196)


## [0.16.7](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.16.6...hwpforge-smithy-hwpx-v0.16.7) - 2026-10-01

### Changed

- *(hwpx)* `header.xml`·`section*.xml` 이 XML 224 단계보다 깊게 중첩되거나 한 지점에서 namespace 바인딩이 128개를 넘으면 `DECODE_FAILED` 로 거부한다. 224 는 잠정값이고, 표·글상자 32겹은 계속 읽힌다. namespace 한도를 넘으면 HwpForge 의 한도라고 알리는 문장으로 오류를 낸다

- *(hwpx)* 최적화 빌드에서 디코드 스레드의 스택은 2 MiB 를 권장한다 (잰 최소값 1 MiB)


### Documentation

- *(readme)* link the docs site from the root and MCP crate READMEs


### Fixed

- *(hwpx)* quick-xml 0.42 로 올린다. 한컴이 읽는 결과가 0.41 과 같도록 아래 셋을 맞춘다

- *(hwpx)* 속성 값의 탭·줄바꿈·CR 은 공백으로 쓴다. 0.42 는 이를 `&#9;`·`&#10;`·`&#13;` 로 써서 한컴에서 스타일 이름이 잘렸다. 0.41 출력을 XML 파서가 읽은 값과 같다

- *(hwpx)* 공백 문자 참조 (`&#13;` 등) 만 든 텍스트 run 을 읽을 때 버리지 않는다. 0.42 는 CR 하나를 이렇게 쓴다. 공백만 든 run 이 서식 run 사이에 있는 문단의 줄 조판 캐시도 이제 유지한다 (이전에는 `LAYOUT_CACHE_DROPPED` 와 함께 버려졌다)

- *(hwpx)* 글꼴 이름 (`hh:font@face`, 수식 글꼴 `hp:equation@font`) 은 쓸 때와 읽을 때 모두 제어 문자를 지우고 앞뒤 ASCII 공백을 자른다. 가운데 공백은 지킨다. 경고는 내지 않는다


## [0.16.6](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.16.5...hwpforge-smithy-hwpx-v0.16.6) - 2026-09-22

### Added

- *(bindings-cli)* `validate` 명령 신설 — MCP·Python 에만 있던 검증 연산을 CLI 에도


### Documentation

- PDF 렌더의 전제를 "한컴 저장본" 이 아니라 "조판 캐시" 로 적는다

- README 퀵스타트와 Rust 예시가 적힌 그대로 실행되게 한다

- *(python)* Python 바인딩 0.16.5 첫 wheel 배포를 문서에 반영


## [0.16.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.16.4...hwpforge-smithy-hwpx-v0.16.5) - 2026-09-17

### Added

- *(smithy-hwpx)* fill·insert·delete 진단 쌍둥이, stamp 의 코덱 양방향 경고, umbrella 배선

- *(smithy-hwpx)* 진단 보존 진입점 — 성공 경로의 인코더·디코더 경고를 돌려주는 *_with_diagnostics 쌍둥이

- *(smithy-hwpx)* partition_semantic_loss 공개, PresetInfo 에 Deserialize·JsonSchema 파생

- *(smithy-hwpx)* 의미 손상 fail-closed 를 typed SemanticLoss 오류로 표면화

- *(smithy-hwpx)* EncodeWarning::is_semantic_loss 를 fail-closed 집합의 단일 정의로


### Documentation

- 브랜치 전체 리뷰 반영 — 릴리스 이력 포인터·architecture mermaid·MSRV 예외 정정

- 공개 문서를 0.16.4 실측에 맞게 정정 — CLI 22·MCP 19·smithy-pdf·설치 명령·정책 문서


## [0.16.4](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.16.3...hwpforge-smithy-hwpx-v0.16.4) - 2026-08-28

### Added

- *(smithy-hwpx)* 각주/미주 본문 autoNum 번호 머리 방출 (W3.5)


### Fixed

- *(smithy-hwpx)* 각주 번호부여 7차 평결 상환 — 편집기 의미 손상 fail-closed

- *(smithy-hwpx)* 각주 번호부여 6차 평결 상환 — 정확 일치 가드·경고 표면 확대

- *(smithy-hwpx)* 각주 번호부여 5차 평결 상환 — 마커 가드 전종화·경고 관통

- *(smithy-hwpx)* 각주 번호부여 4차 평결 상환 — 마커 유출 차단·주입 안전화

- *(smithy-hwpx)* 각주 번호부여 3차 평결 상환 — 명시적 시작번호 보존

- *(smithy-hwpx)* 각주 번호부여 재평결 지적 상환 — 합성 begin_num 오독 제거

- *(smithy-hwpx)* 각주 번호 머리 주입의 리뷰 지적 7건 상환

- *(smithy-hwpx)* 공백 전용 <hp:t> run 무음 소실 수정


## [0.16.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.16.0...hwpforge-smithy-hwpx-v0.16.1) - 2026-08-26

### Fixed

- *(smithy-hwpx)* 문단부호 sentinel(wire끝+1) lineseg 좌표 수용


## [0.15.2](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.15.1...hwpforge-smithy-hwpx-v0.15.2) - 2026-08-18

### Fixed

- *(hwpx)* wire 축약점 to_wire 를 earliest-preimage 로 정정


## [0.15.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.15.0...hwpforge-smithy-hwpx-v0.15.1) - 2026-08-18

### Added

- *(smithy-pdf)* W2a 렌더러 이미지 배관 — Paint Image·스니퍼·preflight·브리지 결선


### Fixed

- *(smithy-hwpx)* pic 표시 크기를 sz-우선 선택으로 — 재저장 curSz=(0,0) 관례 반영


## [0.15.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.14.0...hwpforge-smithy-hwpx-v0.15.0) - 2026-08-14

### Added

- *(smithy-hwpx)* **BREAKING** W1b wire↔Core 좌표 ledger — 3경계 정규화·allowlist·진단 전파

- HxRun 자식 문서 순서 보존 — 순서 사이드카 + 디코더·patch 동시 전환 (이미지 에픽 W1a)


### Fixed

- *(smithy-hwpx)* W1b 독립 리뷰 상환 — folded field tab 폭·절대값 oracle·미래 variant 방어


## [0.14.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.13.1...hwpforge-smithy-hwpx-v0.14.0) - 2026-08-13

### Added

- pghd 감추기 carry + 명시적 쪽나눔 렌더 존중 (W3)

- nwno 새 번호 지정 carry — HWP5→Core→HWPX→PDF 전 구간 (W2)


### Fixed

- 독립 리뷰 상환 — marker 큐 정렬·이벤트 유실·secPr 계약 (게이트 6)


## [0.13.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.13.0...hwpforge-smithy-hwpx-v0.13.1) - 2026-08-12

### Added

- *(core)* 쪽번호 전용 CHAR 스타일 조회 char_style_shape 추가

- *(smithy-hwpx)* char_shape_mut 추가 + W5 머리말·쪽번호 시각 게이트 fixture


## [0.13.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.12.1...hwpforge-smithy-hwpx-v0.13.0) - 2026-08-09

### Added

- *(core)* **BREAKING** W5-α 머리말/꼬리말 subList 기하 승격 — vertAlign·textWidth·textHeight

- *(smithy-hwpx)* W5-α 디코드 경고 채널 — 미지 enum 폴백 표면화

- *(core)* **BREAKING** W5-α pageStartsOn 승격 — 스키마 폐기 교정 + 인코더 BOTH 고정 제거

- *(smithy-hwpx)* **BREAKING** W5-α 다중 머리말/꼬리말 cardinality 보존 — first-wins 무음 폐기 제거

- *(smithy-pdf)* W4c 스타일 선택·언어축 검사 — 기본 fatal + Degraded 옵트인

- *(smithy-pdf)* W3c 표 배치소스 — 검증된 프로파일 재생 + 계산 페이지네이션 이중 검산

- *(core)* StyleLookup borderFill 렌더 표면 + HWPX 브리지 (PDF W3b)

- *(smithy-hwpx)* 표 margin 원본값 왕복 + hasMargin 기반 셀 margin 승격 (PDF W3a-2)

- *(core)* 표 outMargin/inMargin 구조 승격 + sz height decode-only 캐시 (PDF W3a-1)


## [0.12.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.12.0...hwpforge-smithy-hwpx-v0.12.1) - 2026-08-06

### Added

- *(smithy-pdf)* W2d 렌더 파이프라인 + krilla 백엔드 — bbox 게이트 전축 100%


### Fixed

- *(smithy-pdf)* 독립 리뷰 상환 — 선행 컨트롤 가드·정규화 정밀화·왕복 대칭


## [0.12.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.11.7...hwpforge-smithy-hwpx-v0.12.0) - 2026-08-05

### Added

- *(smithy-hwpx)* 인코더 opt-in emit_layout_cache (기본 off, PDF 재생 파이프라인 전용)

- *(core)* **BREAKING** 줄 조판 캐시 LayoutCache 승격(decode-only) + HWPX 디코더 승격·admission 캐시 정규화


## [0.11.7](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.11.6...hwpforge-smithy-hwpx-v0.11.7) - 2026-07-25

### Added

- 문단 배치 삽입 insert_paragraphs + 삭제 경고 스캔 scan_delete_warnings (E4 후속)


### Fixed

- lenient 격자 배치에 span-폭주 사전 가드 (E3 L3)


## [0.11.6](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.11.5...hwpforge-smithy-hwpx-v0.11.6) - 2026-07-24

### Added

- *(hwpx)* E4 W3 — 문단 insert + G2 linesegarray 처리

- *(hwpx)* E4 W2 — 문단 delete (바이트 스플라이스 + reverse-delta 검증)

- *(hwpx)* E4 W1 — 문단 참조 스캔 + 최상위 span 스캐너 승격


### Changed

- refactor(hwpx)+test: E4 구조 편집 admission/self-verify DRY + 안전망 커버


### Fixed

- *(hwpx)* E4 독립 리뷰 상환 — 문단 id 재번호(H1) + strip depth-aware(M1) 외


## [0.11.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.11.4...hwpforge-smithy-hwpx-v0.11.5) - 2026-07-23

### Added

- *(hwpx)* HwpxDiffer — 두 문서 diff (semantic+package 2채널)

- *(hwpx)* read 부분 projection — 문단범위/표격자/필드

- *(hwpx)* outline 항법 지도 projection (HwpxReader)


### Fixed

- *(hwpx)* diff 필드 스트립을 field 축 커버리지로 한정 + 리뷰 상환

- *(hwpx)* outline 섹션 요약 표 카운트를 ordinal 목록과 정합


## [0.11.4](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.11.3...hwpforge-smithy-hwpx-v0.11.4) - 2026-07-22

### Added

- *(cli,mcp)* stamp v2 배선 — 셀 후보 plan·v2 맵·클래스-B 스탬핑 표면

- *(smithy-hwpx)* 클래스-B 셀 스탬핑 apply + stamp_v2 (통합 preflight·역치환 delta·manifest v2)

- *(smithy-hwpx)* 클래스-B 셀 후보 탐지 plan_cells (+stampable-empty 실측 확장)

- *(smithy-hwpx)* stamp map v2 versioned envelope (+클래스-B 셀 스펙 계약)

- *(smithy-hwpx)* 클래스-B 셀 스탬핑 기본 술어 (canonical-empty + shared-boundary 인접)


### Fixed

- *(smithy-hwpx)* 미편집 문단의 linesegarray(줄 조판 캐시) 선별 보존

- *(smithy-hwpx)* 빈 문단 placeholder 가 authored charPrIDRef 를 보존하도록 수정


## [0.11.3](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.11.2...hwpforge-smithy-hwpx-v0.11.3) - 2026-07-21

### Added

- set-cell — 논리 격자 주소 기반 표 셀 편집 (E3 Wave 3)

- JSON export 에 표 셀 논리 격자 주소(addr) 노출 + import 검증-후-폐기


### Changed

- 표 셀 주소 occupancy 계산 2중복을 core grid_placements 로 통합


### Fixed

- 완전 피복 빈 표 행을 진실하게 수용하고 HWP5 유령 셀 폴백 제거


## [0.11.2](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.11.1...hwpforge-smithy-hwpx-v0.11.2) - 2026-07-21

### Added

- MCP hwpforge_stamp_plan/hwpforge_stamp + 스킬 STAMP 경로

- CLI stamp-plan/stamp — E6 스탬핑 커맨드 + 게이트 fixture

- E6 HwpxStamper — admission 게이트 + manifest (fail-closed bytes 파사드)

- E6 스탬핑 apply 단계 — 전량 preflight + run 분할 승격 (all-or-nothing)

- E6 스탬핑 plan 단계 — patch 슬롯 워커 재사용 + 문단·셀 스코프 가드

- E6 스탬핑 클래스-A 마커 탐지기 — 닫힌 패턴 리스트 + 안내문 가드


### Fixed

- 후속 검증 잔여 Low 상환 — MCP escape 일관성·manifest 경로 충돌 가드

- 리뷰 확정 이슈 상환 — 탐지기 O(n²) DoS·부분 산출물·엔트리명 escape

- CrossRef RefPath envelope 언랩 — 왕복 누적 발산·Object 승격 불발 수정


## [0.11.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.11.0...hwpforge-smithy-hwpx-v0.11.1) - 2026-07-13

### Added

- *(hwpx)* 이름 기반 누름틀 채우기 델타 API (HwpxFiller)


## [0.11.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.10.0...hwpforge-smithy-hwpx-v0.11.0) - 2026-07-12

### Added

- *(hwpx)* 누름틀 본문을 preserve-first 텍스트 슬롯으로 등록

- *(core)* **BREAKING** 누름틀 display_text 가 채워진 본문을 보존하도록 의미 변경

- *(hwpx)* 누름틀 본문에 채워진 값을 인코딩


### Fixed

- *(hwpx)* 병합-run 누름틀 본문은 미채움으로 다운그레이드 (무모호 게이트)


## [0.10.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.9.0...hwpforge-smithy-hwpx-v0.10.0) - 2026-07-02

### Added

- *(core)* **BREAKING** carry multi-column separator line (colLine)


### Documentation

- reconcile README/CLAUDE.md/CHANGELOG with 0.9.0 workspace state


## [0.9.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.8.0...hwpforge-smithy-hwpx-v0.9.0) - 2026-06-28

### Changed

- *(core)* **BREAKING** link cross-ref to target via shared ObjectId (E6 M2)


## [0.8.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.7.0...hwpforge-smithy-hwpx-v0.8.0) - 2026-06-27

### Added

- *(core)* **BREAKING** shape text vertical alignment (ellipse/polygon/textbox)


### Changed

- *(core)* **BREAKING** rename Summery typo to Summary in IR identifiers (E6 slice A)


### Documentation

- *(example)* author footnote/endnote inline in full_report (gotcha #12)

- sync README/mdbook/CLAUDE for hwpforge-convert (E5) + refresh metrics


### Performance

- *(hwpx)* single-allocation marker substitution (E4 #1)

- *(hwpx)* single-pass enrich_sec_pr splice (E4 #4)

- *(hwpx)* collect section results in one move-pass, drop clones (E4 #5)


## [0.7.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.6.0...hwpforge-smithy-hwpx-v0.7.0) - 2026-06-19

### Added

- *(hwp5)* TextArt (글맵시 / <hp:textart>) carry — HWP5↔HWPX

- *(hwp5)* Wave B — nested group ($con-in-$con) recursive carry

- *(hwp5)* carry group/묶음 객체 (<hp:container>) HWP5↔HWPX — Wave A (flat)

- *(hwpx)* Wave 12p Step 4 — HWPX encoder emit inst_id 를 id/instId attribute 로

- *(core)* **BREAKING** Wave 12p Step 2 — Core breaking: Image/Table/Equation 에 inst_id (cross-ref target carry)

- *(hwp5)* **BREAKING** Wave 12m Phase 2 Step 4 — Control::CrossRef vertical slice

- *(core)* **BREAKING** Wave 12m Phase 2 Step 3 — foundation/core API breaking (RefType + RefContentType + RefTarget + Control::CrossRef target)

- *(hwpx)* **BREAKING** Wave 12n Step 6 — %pat PATH 필드 lossless HWPX carry (#120 핵심 해소)

- *(hwpx)* **BREAKING** Wave 12o Phase 2 — content.hpf metadata decoder + XXE/DoS 방어

- *(hwpx)* **BREAKING** Wave 12o Phase 1 — content.hpf metadata emit (Document.metadata → <opf:metadata>)

- *(core)* **BREAKING** Wave 12o Phase 0 — Metadata에 description/last_saved_by/extras + non_exhaustive (ADR-003)

- *(core)* **BREAKING** Wave 12n — 자동 필드 의미 분할 + HWPX carry

- *(hwp5)* **BREAKING** Wave 12l — 누름틀(ClickHere) carry + name 메타 carry

- *(hwp5)* **BREAKING** Wave 12j — 글자겹침(compose) carry + char_pr_ids fidelity + packed-variant 지원

- *(hwp5)* **BREAKING** Wave 12i — 덧말(dutmal) option carry + flat-path control_iter 필터

- *(hwp5)* **BREAKING** Wave 12f-h — 메모 anchor 위치 수정 + 7 parameters wire 메타 carry 완성

- *(hwp5)* **BREAKING** Wave 12e — 메모 본문 carry + 본문 텍스트 덮어쓰기 버그 수정


### Changed

- *(hwpx)* remove orphaned days_to_ymd date-synthesis helper

- *(hwpx)* task #92 Step 4 — split section_pr/header_footer out of encoder/section.rs

- *(hwpx)* task #92 Step 3 — split field family out of encoder/section.rs

- *(hwpx)* task #92 Step 2 — split memo/picture out of encoder/section.rs

- *(hwpx)* task #92 Step 1 — split equation/typography/chart out of encoder/section.rs


### Documentation

- *(refactor)* task #92 Step 5 — module-layout doc + CHANGELOG + CLAUDE.md sync

- *(metadata)* Wave 12o-fixup CHANGELOG + probe_date_carry example

- Wave 12l + Phase 12 series 완료 반영 (CLAUDE/MEMORY/README)


### Fixed

- *(blueprint)* **BREAKING** non_exhaustive on ParaShape/PartialParaShape/PartialStyle (B)

- *(blueprint)* carry underline_shape through Blueprint char styles

- *(hwp5)* carry SUMMERY/PATH field cached value to close 한컴 recovery warning (#120/#136)

- *(hwpx)* chart custom title mirrors 한컴-native form

- *(hwp5)* task #73 — carry dutmal sz_ratio + align from pinned tail offsets

- *(hwp5)* Wave 12q (task #122) — apply outline level overrides from Style 개요 N

- *(hwpx)* Wave 12p task #124 — SUMMERY editable per FieldType + Wave 12p Step 4 visual gate + fmt fallout

- *(foundation)* **BREAKING** RefContentType::BookmarkName 부활 + Bookmark N2 매핑 native 일치 (Wave 12m fixup regression)

- *(hwpx)* **BREAKING** Wave 12m fixup — fieldid `%xrf` magic + RefContentType::BookmarkName 폐기 (시각 검증 통과)

- *(hwpx)* #87 hardening — escape_xml에 C0/illegal-char strip 통합

- *(hwpx)* Wave 12n Step 6.6 — SUMMERY body 빈 emit (한컴 "복구" 경고 우회)

- *(hwpx)* Wave 12n Step 6.5 — fieldBegin/fieldEnd trailing <hp:t/> 추가

- *(metadata)* Wave 12o-fixup — Codex review 4건 (Top-1/Top-2/Top-4/S3) + Top-5 종료 노트 정직성

- *(hwpx)* floating ellipse/arc/curve/connectLine use shared positioning


## [0.5.2](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.5.1...hwpforge-smithy-hwpx-v0.5.2) - 2026-05-13

### Added

- *(hwp5)* preserve fields and checkable state in hwpx projection


## [0.5.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.5.0...hwpforge-smithy-hwpx-v0.5.1) - 2026-03-24

### Added

- *(hwpx)* preserve relief and vertical char effects

- *(hwp5)* improve style fidelity bridge


### Documentation

- refresh public docs and doc tooling


### Fixed

- *(style)* warn on conflicting vertical position bits

- *(hwp5)* preserve paragraph layout hints


## [0.5.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.4.0...hwpforge-smithy-hwpx-v0.5.0) - 2026-03-22

### Added

- *(list)* **BREAKING** add checkable bullet semantics

- *(list)* **BREAKING** add shared list semantics


### Documentation

- refresh readme and fix docs lint


### Fixed

- *(md)* preserve task list continuations

- *(list)* restore markdown task lists and tighten bullet semantics

- *(hwpx)* bridge registry-local style ids


## [0.4.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.3.0...hwpforge-smithy-hwpx-v0.4.0) - 2026-03-19

### Added

- *(tab)* **BREAKING** implement shared tab semantics across hwpx and hwp5


### Changed

- *(tab)* tighten tab semantics and shared encoder helpers


### Chore

- *(release)* **BREAKING** prepare v0.4.0 for tab semantics


## [0.3.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.2.1...hwpforge-smithy-hwpx-v0.3.0) - 2026-03-19

### Changed

- Add preservation metadata to `ExportedSection` so section JSON exports can drive byte-preserving patch workflows across CLI and MCP.
- Harden preserving section patching with canonical preservation validation, stale metadata rejection, and explicit mixed-content fail-fast behavior.

### Migration

- Downstream code that constructs `ExportedSection` with a struct literal must initialize the new `preservation` field.
- Consumers should re-export section JSON with the current `to-json --section` flow before patching; legacy exports are intentionally rejected.

## [0.2.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.2.0...hwpforge-smithy-hwpx-v0.2.1) - 2026-03-17

### Fixed

- *(docs)* unescape HTML entities in details/summary tags


## [0.2.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.1.7...hwpforge-smithy-hwpx-v0.2.0) - 2026-03-17

### Changed

- Align the HWPX codec crate with the workspace-wide `0.2.0` release line.
- Update internal table and image construction to the `hwpforge-core` `0.2.0` builder-based contract.

## [0.1.7](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.1.6...hwpforge-smithy-hwpx-v0.1.7) - 2026-03-12

### Added

- HWPX→Markdown styled conversion pipeline


### Documentation

- update HWPX→MD examples and README with to-md/from-json CLI commands


## [0.1.6](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.1.5...hwpforge-smithy-hwpx-v0.1.6) - 2026-03-10

### Added

- *(mcp)* Phase 7c MCP Extended — 3 tools + 4 resources + 3 prompts


### Documentation

- *(readme)* update stats and MCP tool list for Phase 7c

- *(readme)* simplify MCP setup and update AI tool list


### Fixed

- *(mcp)* fix restyle index mismatch and convert font override bugs


## [0.1.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.1.4...hwpforge-smithy-hwpx-v0.1.5) - 2026-03-10

### Fixed

- *(dist)* improve user experience for npm installation


## [0.1.4](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.1.3...hwpforge-smithy-hwpx-v0.1.4) - 2026-03-09

### Changed

- extract shared types into smithy-hwpx to eliminate CLI/MCP duplication


### Documentation

- change Anvil emoji from ⚒️ to ⚙️ for better semantic match

- add Bindings branding (Hammer/Anvil/Tongs), MCP multi-platform install guide, SKILL.md agent rules

- update README with MCP server section, badges, and project stats


## [0.1.3](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.1.2...hwpforge-smithy-hwpx-v0.1.3) - 2026-03-09

### Added

- *(examples)* add hwpx_complete_guide to JSON round-trip

- *(examples)* add HWPX↔JSON round-trip examples

- *(examples)* reorganize examples and add 16 HWPX showcase files

- *(smithy-hwpx)* add gradient fill support for shapes


### Fixed

- *(smithy-hwpx)* fix JSON round-trip crash and improve codec fidelity

- *(encoder)* use DrawingML namespace for chart title

- *(encoder)* add pattern fill (hatchStyle) support and fix BACK_SLASH/SLASH swap

- *(encoder)* fix rotation encoding to match 한글 convention

- *(encoder)* encode flip in rotMatrix instead of scaMatrix

- *(encoder)* apply scaMatrix + transMatrix for shape flip rendering

- *(encoder)* add unique id to fieldBegin and fix table cellAddr for merged cells


## [0.1.2](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.1.1...hwpforge-smithy-hwpx-v0.1.2) - 2026-03-08

### Added

- *(cli)* implement Phase 6 AI-first CLI with 7 commands


### Documentation

- *(readme)* add CLI quick start section and update project stats


## [0.1.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-hwpx-v0.1.0...hwpforge-smithy-hwpx-v0.1.1) - 2026-03-07

### Documentation

- *(readme)* add supported Hancom versions table and cargo install instructions

- update LICENSE-APACHE to full text and add README badges


### Fixed

- *(readme)* replace broken Buy Me a Coffee button with stable CDN image

- use absolute URLs for README images (crates.io compatibility)
