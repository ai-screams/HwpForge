# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.20.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.19.1...hwpforge-smithy-md-v0.20.0) - 2026-11-07

### Changed

- 워크스페이스 버전을 맞춘다. 이 크레이트 자체의 구현 변경은 없다.

## [0.19.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.19.0...hwpforge-smithy-md-v0.19.1) - 2026-11-03

### Changed

- 워크스페이스 버전을 맞춘다. 이 크레이트 자체의 구현 변경은 없다.

## [0.19.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.18.0...hwpforge-smithy-md-v0.19.0) - 2026-11-02

### Changed (BREAKING)

- Markdown 변환 결과의 문단이 한컴 기본 스타일 대신 템플릿 스타일(`body`, `heading1`–`heading6`, `list_item` 등)을 가리킨다. 형식 중립 `StyleRef`를 쓰도록 바뀐 Core 문단 참조에 맞춘 변경이다(`hwpforge-core` 항목을 본다). 제목·목록 충돌은 stderr 출력 대신 `MdWarning::HeadingListConflict`로 돌려준다. 이행: stderr의 충돌 메시지를 읽던 코드는 경고 값을 읽는다. ([#295](https://github.com/ai-screams/HwpForge/pull/295))
- Markdown 목록을 `- ` 글자가 붙은 본문 문단으로 쓰던 것을 한컴의 글머리표·번호·체크 문단으로 쓴다. 한컴에서 기호와 번호가 문서 기능으로 보이고, 중첩 수준이 HWPX에 남고, 둘째 줄이 항목 글 시작에 맞으며, 항목 안의 문단·코드·표도 그 항목에 남는다. 번호 목록은 목록마다 1부터 다시 센다. 같은 Markdown의 HWPX 출력이 달라지며 이전 산출물과의 호환은 없다. 새 경고는 다음과 같다: 9보다 깊은 수준은 `LIST_LEVEL_CLAMPED`, 1이 아닌 시작 번호는 1부터 쓰고 `LIST_START_NORMALIZED`, 항목 안 제목·구분선은 `LIST_SEMANTICS_LOST`, 템플릿에 해당 역할이 없으면 `LIST_DEFINITION_MISSING`, 번호 정의 복제 한도(1024)를 넘으면 `NUMBERING_PRESENTATION_LOST`. 한컴이 항목의 코드 문단에 표·그림·컨트롤을 넣었다면 코드 블록 대신 일반 문단으로 써서 내용을 잃지 않는다. ([#303](https://github.com/ai-screams/HwpForge/pull/303))
- `StyleLookup`의 목록 조회가 `para_list` 하나로 합쳐졌다(`hwpforge-core` 항목을 본다). 이에 맞춰 lossy 보기가 원본 스타일을 받는 `MdEncoder::encode_lossy_with_styles(document, styles)`를 더했고, lossy `to-md`의 목록·제목·체크 상태가 원본 문단 모양을 따른다. 목록 문단을 목록 항목으로 쓰지 못하면 문단마다 한 번 `MdWarning::ListSemanticsLost`(`LIST_SEMANTICS_LOST`)를 낸다. 사유는 `container_projection`, `body_carrier_projection`, `markup_fallback`이다. 표나 그림 하나만 있는 목록 문단은 그 블록을 품은 항목으로 쓰고, 코드 글꼴 문단에 표나 그림이 있으면 일반 문단으로 쓴다. ([#301](https://github.com/ai-screams/HwpForge/pull/301))

### Added

- `to-md`가 쓴 HTML을 `convert`가 다시 읽는다. styled 출력의 `<strong>`·`<em>`·`<u>`·`<sup>`·`<sub>`와 HTML 표, lossy 출력의 표 칸 `<br>`이 대상이다. 디코더는 인코더가 실제로 쓰는 HTML만 받아 같은 서식으로 읽고, 그 밖의 HTML은 전과 같이 거부한다. `<!-- chart -->`·`<!-- memo: … -->` 주석은 되살릴 데이터가 없으므로 건너뛰고 `CONTENT_LOST` 경고를 낸다. 전에는 이런 출력을 `convert`에 넣으면 `MD_DECODE_FAILED`로 실패했다. ([#298](https://github.com/ai-screams/HwpForge/pull/298))
- `StyleLookup::style_char_shape` 질의를 쓰도록 styled 인코더를 바꾼다(`hwpforge-core` 항목을 본다). ([#304](https://github.com/ai-screams/HwpForge/pull/304))

### Changed

- lossless 출력은 `metadata.format: lossless` 표지를 메타데이터와 관계없이 늘 쓴다. 일반 디코드가 이 표지를 만나면 원인과 대안(`to-json`/`from-json`)을 알려 주며 `UnsupportedStructure`로 거부한다. 전에는 메시지가 `raw HTML`이라 원인을 알 수 없었다. `to-md --mode lossless`의 도움말과 문서는 "Round-trip-safe"가 아니라 보존 범위를 확인하는 기계용 보기라고 설명한다. 이 출력을 되읽는 코드는 Rust API `MdDecoder::decode_lossless` 하나뿐이며 CLI·MCP·Python의 `convert`는 부르지 않는다. 이행: 문서 구조와 스타일의 왕복 편집은 `to-json`/`from-json`을 쓴다. ([#299](https://github.com/ai-screams/HwpForge/pull/299))

### Fixed

- 중첩 목록을 수준마다 두 칸씩 들여 써서 번호 목록의 하위 항목이 형제가 되고 더 깊은 줄이 들여쓴 코드 블록으로 읽히던 것을 고친다. 각 항목을 부모 표지 뒤 내용이 시작하는 열에 맞춰 들여 쓰고(`- `는 2칸, `1. `은 3칸, `10. `은 4칸), 수준을 건너뛰어도 한 단계만 깊어진다. 목록 글 안에서 표지처럼 보이는 글자는 글자로 남고, 빈 목록 항목도 자식을 유지한다. ([#300](https://github.com/ai-screams/HwpForge/pull/300))
- 기본 템플릿의 제목이 `# **장 제목 하나**`처럼 아무도 쓰지 않은 강조가 붙어 나오던 것을 고친다. 문단 스타일이 이미 주는 강조(굵게, 기울임, 취소선, 밑줄, 위 첨자, 아래 첨자)는 쓰지 않고 그 스타일을 넘는 강조만 쓴다. 스타일 기준을 모르거나 글자가 스타일의 속성을 끄는 경우는 전과 같이 절대 표기로 쓴다. 본문의 일반 강조는 그대로다. ([#304](https://github.com/ai-screams/HwpForge/pull/304))
- Markdown으로 읽을 때 목록의 왼쪽 여백과 첫 줄 들여쓰기가 `PARAGRAPH_MEASURE_LOST` 손실로 잘못 보고되던 것을 고친다. 인코더가 실제로 쓴 목록 구조와 기본 템플릿 값을 대조해 다시 돌아오는 여백만 경고에서 뺀다. 내용이 사라지던 문제가 아니라 경고가 많아 진짜 손실이 묻히던 문제였다. ([#312](https://github.com/ai-screams/HwpForge/pull/312))
- 코드 블록 마지막 줄 끝의 줄바꿈 하나가 `hp:lineBreak`가 되어 코드 아래에 빈 줄이 보이던 것을 고친다. 디코더는 마지막 줄 끝 줄바꿈 하나만 빼고, 공백·탭·중간의 빈 줄은 그대로 둔다. 빈 줄 하나뿐인 코드 블록은 빈 코드 블록과 같은 문단이 된다. ([#313](https://github.com/ai-screams/HwpForge/pull/313))
- 번호·글머리표 목록의 표지에서 글까지를 20pt로 고정하고, 이어지는 문단도 같은 자리에서 시작하게 한다. 글머리표의 글 시작은 15pt에서 20pt로 옮겨 간다. Markdown 표의 아래 여백은 2.83pt에서 10pt로 늘어나므로 표가 많은 문서는 쪽 흐름이 달라질 수 있다. 한계: 목록 기호 폭은 20pt 고정이며 글자 크기 10pt인 기본 템플릿에만 맞춰 있다. ([#313](https://github.com/ai-screams/HwpForge/pull/313))

### Performance

- 각주 정의가 비었는지 판정할 때와 각주·미주 정의의 전개 비용을 계산할 때 정의 본문 전체를 임시 문자열로 복사하던 것을 run 단위 처리로 바꾼다. 수용·거부되는 입력과 비용 값, 상한(8 MiB) 동작은 그대로이고, 큰 각주 정의를 읽을 때 정의 크기만큼 생기던 순간 메모리 상승이 사라진다. ([#302](https://github.com/ai-screams/HwpForge/pull/302), [#309](https://github.com/ai-screams/HwpForge/pull/309))

## [0.18.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.17.1...hwpforge-smithy-md-v0.18.0) - 2026-10-25

### Changed (BREAKING)

- `MdDocument` 가 `#[non_exhaustive]` 가 되고 `warnings: Vec<MdWarning>` 필드를 추가한다. 대상이 빈 이미지(`![x]()`)는 그림을 넣지 않고 이 필드에 `ImageEmbedSkipped`(`src` 빈 문자열, 이유 `Unreadable`)로 남긴다. 이행: `MdDocument { document, style_registry }` 로 구조 분해하던 코드는 `MdDocument { document, style_registry, .. }` 로 바꾸고, 경고가 필요하면 `warnings` 를 읽는다. 이 구조체는 디코더만 만든다.

### Changed

- styled 출력은 메모를 펼친 문단 사본을 문서 처리가 끝날 때까지 보관하므로, 메모가 많은 큰 문서에서 메모리를 더 사용한다.
- styled 출력은 `ContainerTooDeep` 경고를 내지 않는다. 0.17.1 부터 Core 검증이 깊이 32단계를 넘는 문서를 거부해(#267) 검증된 문서는 styled 가 그리는 깊이 한도에 닿지 않으므로, 그 뒤로는 나지 않던 경고다. 변종은 lossy 출력의 표 중첩 경고로 그대로 쓰인다.

### Fixed

- Markdown 의 대상이 빈 이미지(`![x]()`)가 0.17.1 에서 경고 없이 50×30mm 빈 그림 틀로 남던 것을, 0.17.0 처럼 "넣지 못함" 경고(`image "" not embedded (file unreadable) — image dropped`)와 함께 뺀다. 판정은 일반 Markdown 디코더에서 하므로, 한컴이 자산을 잃은 채 저장한 그림 틀(#266)은 HWPX·lossless 디코드와 두 자산 진입점(`load_referenced_images`, `collect_asset_plan`→`finish_assets`)에서 그대로 남는다.
- styled Markdown 출력에서 본문 문단과 표 칸의 글자가 줄 머리에서 `- `·`1. `·`: `·`===`·표 구분 행처럼 Markdown 구조로 다시 읽히지 않도록 그 표지 앞에 `\` 를 붙인다. 목록·제목·캡션 출력은 이전과 같다(정의 목록이 생기던 블록만 아래처럼 바뀐다). 각주와 lossy 출력에서 바뀐 점은 아래에 따로 적는다.
- 문단 안 표 바로 뒤의 글이 표에 흡수되던 문제를 고친다. 표 뒤에 빈 줄을 넣어 뒤 글이 새 문단으로 남고, GFM 표 행에서 버려지던 글자도 보존된다.
- styled 본문 문단에서 줄머리의 메모·차트 주석 뒤에 글이 이어지면 주석 뒤에서 줄을 바꾼다. 뒤 글은 HTML 주석 블록에 흡수되지 않고 새 문단으로 남으며, 메모 주석도 남는다. 목록 continuation 등 다른 출력은 이전과 같다.
- 위 규칙으로 막을 수 없는 드문 본문 문단은 서식 없는 글로 바꾸고 `MarkupWrittenAsText` 경고를 남긴다. 상호참조·필드·알 수 없는 컨트롤은 대체 글에서도 정상 출력과 같은 표시값을 쓴다.
- 출력에 정의 목록이 남지 않아, 그 때문에 from-md 가 실패하던 경우가 없어진다.
- 본문 문단이나 표 칸을 서식 없는 글로 바꿀 때(styled·lossy) 그림 참조, 허용된 링크, styled 의 각주 참조를 그대로 남긴다. 이전에는 그림 참조가 사라지고, 링크는 글자만, 각주는 본문 글로 남았다. 그리지 않은 각주(GFM 표가 잘라 낸 칸의 각주 등)는 이전처럼 본문 글로 쓴다.
- styled 각주 본문의 줄이 `-`·`+`·`1.`·`1)` 표지와 탭으로 시작하면 각주 안에 목록이 생겨 from-md 가 실패하던 문제를 고친다. 표지에 `\` 를 붙여(`\-\t`, 순서 목록은 숫자 뒤 `1\.\t`) 글자 그대로 읽힌다. 10자리 이상 숫자는 목록 표지가 아니므로 그대로 쓴다.
- lossy 출력을 다시 읽어 각주 정의(`[^r]: y`)가 생기면, 참조 정의처럼 그 문단을 서식 없는 글로 바꾸고 `MarkupWrittenAsText` 경고를 남긴다. 이전에는 목록 항목 뒤에 탭으로 들여쓴 각주 정의 꼴 글(`- x` 다음 줄의 `\t[^r]: y` 등)이 경고 없이 각주 정의로 남아 from-md 가 실패했다. lossy 의 이스케이프 규칙 자체는 이전과 같다.
- styled 본문 문단·캡션·표 칸에서 굵게·기울임·취소선이 맞붙거나(`**a****1**`) 유니코드 문장부호 뒤에서 이어지면 다시 읽을 때 글자나 서식이 바뀌던 문제를 고친다. 강조 표지를 문단 단위로 배치한 뒤 최종 출력을 다시 읽어 확인하고, 맞는 배치가 없으면 그 칸이나 문단을 서식 없는 글로 쓰며 `MarkupWrittenAsText` 경고를 문단당 하나 남긴다. 목록·제목 출력은 이전과 같다.

## [0.17.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.17.0...hwpforge-smithy-md-v0.17.1) - 2026-10-24

### Changed

- 인코더의 자체 중첩 깊이 계산을 지우고, 검증된 문서가 깊이 32단계 이하라는 Core 의 보장에 맡긴다. 검증된 문서의 출력은 같다 (#267)

### Fixed

- 이미지 자산이 없는 그림(빈 경로)은 styled·lossy 출력에서 빈 링크를 내지 않고 캡션만 남기며, 이미지 자산 계획에서도 뺀다 (#266)

## [0.17.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.16.11...hwpforge-smithy-md-v0.17.0) - 2026-10-23

### Changed (BREAKING)

- `MdOutput` 에 `warnings: Vec<MdWarning>` 가 생겨 `#[non_exhaustive]` 가 되었다. `MdWarning::ContainerTooDeep` 이 생겼다.

### Changed

- 읽기용 출력의 이스케이프를 하나의 타입 있는 계층으로 모으고, CommonMark 가 마크업으로 읽을 글자만 이스케이프한다. 링크는 `http://`, `https://`, `mailto:` 만 링크로 쓴다. 표가 8겹을 넘으면 `(중첩 표 생략)` 으로 쓰고 경고한다. 컨테이너가 32겹을 넘으면 읽기용 출력은 오류로 거부하고, 스타일 출력은 그 아래를 생략하며 경고한다.
- 글상자·각주·도형 안의 표와 그림, 묶음, 표·그림 캡션, 한컴이 저장한 필드의 표시값이 출력에 나온다.
- 무손실 Markdown 이 묶음과 그 그림, 도형 여덟 가지, 본문 그림의 배치와 캡션, 개체 틀, 표 캡션 속성·셀 구역·고정 셀 크기·셀 줄 바꿈, 그림 하이퍼링크를 왕복한다. 이전 판은 이 요소들을 읽지 못한다.

### Fixed

- 중첩 표 출력이 층마다 두 배로 불어나던 문제, 코드 펜스 검사의 이차 비용, 묶음 안 그림 링크와 캡션 속 그림 바이트가 빠지던 문제.

## [0.16.10](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.16.9...hwpforge-smithy-md-v0.16.10) - 2026-10-04

### Documentation

- correct the statements a fact check found wrong

- *(readme)* match the READMEs to the code and drop volatile counts


### Fixed

- *(md)* `to-md` 의 수식 변환이 깊게 중첩된 수식에서 스택을 넘쳐 프로세스가 죽던 문제를 고친다. 중첩 32단에서 멈추고, 그보다 깊은 수식은 `[수식 변환 생략: 중첩 깊이 초과] ` 뒤에 공백을 접은 원문을 그 자리의 일반 텍스트처럼 이스케이프해 쓴다. 실제 문서 수식(측정 코퍼스 최대 3단)의 출력은 그대로다


## [0.16.8](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.16.7...hwpforge-smithy-md-v0.16.8) - 2026-10-01

### Fixed

- *(md)* 무손실 Markdown 을 다시 읽을 때 `&amp;`·`&lt;`·`&gt;`·`&quot;`·`&#39;` 같은 참조를 글자로 풀어 읽는다. 이전에는 `A & B` 가 `A  B` 로 돌아왔다. 모르는 엔티티나 잘못된 숫자 참조는 `LosslessParse` 오류 (#197)


## [0.16.7](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.16.6...hwpforge-smithy-md-v0.16.7) - 2026-10-01

### Documentation

- *(readme)* link the docs site from the root and MCP crate READMEs


### Fixed

- *(hwpx)* quick-xml 0.42 로 올린다. lossless Markdown 디코더를 0.42 의 문자열 API 로 옮긴다


## [0.16.6](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.16.5...hwpforge-smithy-md-v0.16.6) - 2026-09-22

### Added

- *(bindings-cli)* `validate` 명령 신설 — MCP·Python 에만 있던 검증 연산을 CLI 에도


### Documentation

- PDF 렌더의 전제를 "한컴 저장본" 이 아니라 "조판 캐시" 로 적는다

- README 퀵스타트와 Rust 예시가 적힌 그대로 실행되게 한다

- *(python)* Python 바인딩 0.16.5 첫 wheel 배포를 문서에 반영


## [0.16.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.16.4...hwpforge-smithy-md-v0.16.5) - 2026-09-17

### Added

- *(smithy-md)* AssetOutcome 을 kind 내부 태그 snake_case 로 직렬화

- *(smithy-md)* 이미지 자산 해석을 계획·파일 읽기·임베드 3단계 assets 계약으로 분리


### Documentation

- 브랜치 전체 리뷰 반영 — 릴리스 이력 포인터·architecture mermaid·MSRV 예외 정정

- 공개 문서를 0.16.4 실측에 맞게 정정 — CLI 22·MCP 19·smithy-pdf·설치 명령·정책 문서


### Fixed

- *(smithy-md)* finish_assets 가 호출자의 계획과 재수집 계획을 전건 대조


## [0.16.4](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.16.3...hwpforge-smithy-md-v0.16.4) - 2026-08-28

### Added

- *(smithy-hwpx)* 각주/미주 본문 autoNum 번호 머리 방출 (W3.5)

- *(smithy-md)* 각주 다문단 방출 + 행 시작 escape — 왕복 폐곡선 (W2)

- *(smithy-md)* 각주·미주 MD 디코딩 — [^N]/[^eN] 왕복 (W1)

- *(smithy-md)* 인라인 서식 스택 — 굵게/기울임/취소선 왕복 (W0)


### Fixed

- *(smithy-hwpx)* 각주 번호 머리 주입의 리뷰 지적 7건 상환


## [0.16.3](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.16.2...hwpforge-smithy-md-v0.16.3) - 2026-08-26

### Fixed

- *(smithy-md)* 임베드 리뷰 상환 — 단독 문단 보존·bare 경로 base·BMP 구조검사

- *(smithy-md)* md→hwpx 이미지 참조를 BinData 로 실제 임베드


## [0.14.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.13.1...hwpforge-smithy-md-v0.14.0) - 2026-08-13

### Added

- pghd 감추기 carry + 명시적 쪽나눔 렌더 존중 (W3)

- nwno 새 번호 지정 carry — HWP5→Core→HWPX→PDF 전 구간 (W2)


## [0.11.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.11.4...hwpforge-smithy-md-v0.11.5) - 2026-07-23

### Changed

- *(md)* styled 인코더를 core 공유 분류기로 재기저


## [0.11.3](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.11.2...hwpforge-smithy-md-v0.11.3) - 2026-07-21

### Added

- lossy MD 병합셀 평탄화 경고 + convert 병합 격자 회귀 잠금 (E3 Wave 4)


## [0.11.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.10.0...hwpforge-smithy-md-v0.11.0) - 2026-07-12

### Added

- *(hwpx)* 누름틀 본문을 preserve-first 텍스트 슬롯으로 등록


## [0.8.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.7.0...hwpforge-smithy-md-v0.8.0) - 2026-06-27

### Added

- *(core)* **BREAKING** shape text vertical alignment (ellipse/polygon/textbox)


### Documentation

- sync README/mdbook/CLAUDE for hwpforge-convert (E5) + refresh metrics


## [0.7.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.6.0...hwpforge-smithy-md-v0.7.0) - 2026-06-19

### Added

- *(core)* **BREAKING** Wave 12p Step 2 — Core breaking: Image/Table/Equation 에 inst_id (cross-ref target carry)

- *(md)* Wave 12m Phase 2 Step 5 — Markdown CrossRef carry visible body text

- *(core)* **BREAKING** Wave 12m Phase 2 Step 3 — foundation/core API breaking (RefType + RefContentType + RefTarget + Control::CrossRef target)

- *(hwp5)* **BREAKING** Wave 12j — 글자겹침(compose) carry + char_pr_ids fidelity + packed-variant 지원

- *(hwp5)* **BREAKING** Wave 12i — 덧말(dutmal) option carry + flat-path control_iter 필터

- *(hwp5)* **BREAKING** Wave 12f-h — 메모 anchor 위치 수정 + 7 parameters wire 메타 carry 완성

- *(hwp5)* **BREAKING** Wave 12e — 메모 본문 carry + 본문 텍스트 덮어쓰기 버그 수정


### Documentation

- Wave 12l + Phase 12 series 완료 반영 (CLAUDE/MEMORY/README)


### Fixed

- *(md)* capture GFM table header row (was silently dropped)

- *(hwpx)* Wave 12p task #124 — SUMMERY editable per FieldType + Wave 12p Step 4 visual gate + fmt fallout


## [0.5.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.4.0...hwpforge-smithy-md-v0.5.0) - 2026-03-22

### Added

- *(list)* **BREAKING** add checkable bullet semantics

- *(list)* **BREAKING** add shared list semantics


### Documentation

- refresh readme and fix docs lint


### Fixed

- *(md)* normalize ordered task lists

- *(md)* preserve task list continuations

- *(list)* restore markdown task lists and tighten bullet semantics

- *(hwpx)* bridge registry-local style ids


## [0.4.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.3.0...hwpforge-smithy-md-v0.4.0) - 2026-03-19

### Chore

- *(release)* **BREAKING** prepare v0.4.0 for tab semantics


## [0.3.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.2.1...hwpforge-smithy-md-v0.3.0) - 2026-03-18

### Chore

- *(release)* **BREAKING** prepare v0.3.0 for preserving section API changes


## [0.2.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.2.0...hwpforge-smithy-md-v0.2.1) - 2026-03-17

### Fixed

- *(docs)* unescape HTML entities in details/summary tags


## [0.2.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.1.7...hwpforge-smithy-md-v0.2.0) - 2026-03-17

### Changed

- Align the Markdown codec crate with the workspace-wide `0.2.0` release line.
- Update internal table construction to the `hwpforge-core` `0.2.0` builder-based contract.

## [0.1.7](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.1.6...hwpforge-smithy-md-v0.1.7) - 2026-03-12

### Added

- HWPX→Markdown styled conversion pipeline


### Documentation

- update HWPX→MD examples and README with to-md/from-json CLI commands


## [0.1.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.1.4...hwpforge-smithy-md-v0.1.5) - 2026-03-10

### Fixed

- *(dist)* improve user experience for npm installation


## [0.1.4](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.1.3...hwpforge-smithy-md-v0.1.4) - 2026-03-09

### Added

- *(examples)* add HWPX → Markdown conversion example


### Documentation

- change Anvil emoji from ⚒️ to ⚙️ for better semantic match

- add Bindings branding (Hammer/Anvil/Tongs), MCP multi-platform install guide, SKILL.md agent rules

- update README with MCP server section, badges, and project stats


## [0.1.3](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.1.2...hwpforge-smithy-md-v0.1.3) - 2026-03-09

### Added

- *(examples)* reorganize examples and add 16 HWPX showcase files


## [0.1.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.1.0...hwpforge-smithy-md-v0.1.1) - 2026-03-07

### Documentation

- *(readme)* add supported Hancom versions table and cargo install instructions

- update LICENSE-APACHE to full text and add README badges


### Fixed

- *(readme)* replace broken Buy Me a Coffee button with stable CDN image

- use absolute URLs for README images (crates.io compatibility)
