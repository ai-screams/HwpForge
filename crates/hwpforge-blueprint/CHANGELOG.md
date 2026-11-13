# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- 문단의 optional `east_asian_auto_spacing`(`EastAsianAutoSpacing { latin, digit }`, `EastAsianAutoSpacing::new`)을 추가한다. 한글과 영문·숫자 사이 자동 간격의 사용 여부를 두 값 한 묶음으로 지정하고 상속하며, 생략하면 출력 형식의 원본 보존 또는 조판 프로필 기본값을 따른다.

- 공유 목록 정의가 충돌하면 인코더가 문서 방문 순서상 첫 사용처의 유효 서식과 상대 margin 이동량을 선택하고 의미 손실을 보고할 수 있다. 미지정과 명시적 0의 구별을 유지한다.

- `ListMarkerAlign`, `ListTextStart`, `ListMarkerLayout::new` 및 문단의 optional `list_marker`를 추가한다. 거리는 정수 HwpUnit이며 0과 미지정을 구별한다. 목록 서식은 객체 전체를 상속하고 참조·범위를 출력 경계에서 다시 검증한다.

## [0.20.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-blueprint-v0.19.1...hwpforge-blueprint-v0.20.0) - 2026-11-07

### Changed

- 워크스페이스 버전을 맞춘다. 이 크레이트 자체의 구현 변경은 없다.

## [0.19.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-blueprint-v0.19.0...hwpforge-blueprint-v0.19.1) - 2026-11-03

### Changed

- 워크스페이스 버전을 맞춘다. 이 크레이트 자체의 구현 변경은 없다.

## [0.19.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-blueprint-v0.18.0...hwpforge-blueprint-v0.19.0) - 2026-11-02

### Changed (BREAKING)

- 스타일 항목이 자기 `StyleRef`를 들고 레지스트리가 그 순서를 관리한다. `StyleEntry::new`를 지우고 `StyleRegistry::register_style(name, char_shape_id, para_shape_id, font_id) -> BlueprintResult<StyleRef>`로 등록한다. 같은 이름을 다시 등록하면 참조는 유지하고 내용만 바꾼다. `StyleRegistry.style_entries` 필드는 비공개가 되고 `style_entries()` 접근자로 읽는다. `StyleEntry`의 직렬화 형식에도 필수 필드 `style_ref`가 더해져 이전 직렬화 데이터는 그대로 읽히지 않는다. 이행: `StyleEntry`를 직접 만들던 코드는 `register_style`로 바꾸고, 필드를 직접 읽던 코드는 접근자를 쓴다. 이전 직렬화 데이터는 원본 템플릿이나 등록 정보로 레지스트리를 다시 구성한다. ([#295](https://github.com/ai-screams/HwpForge/pull/295))
- `TemplateProvider`에 `is_builtin_template(&self, name) -> bool`을 더한다. 기본 구현은 `false`라 기존 구현은 그대로 컴파일된다. 내장 템플릿에만 호환 이름 후보를 허용하는 데 쓴다. ([#295](https://github.com/ai-screams/HwpForge/pull/295))
- 템플릿 상속에서 항목 순서가 루트부터 쌓이고, 값은 가장 가까운 부모가 이긴다. 전에는 "첫 항목을 본문으로 쓰는" 대체 선택이 이 순서에 기대었으므로, 다단 상속 템플릿에서 본문으로 고른 스타일이 달라질 수 있다. 이행: 이전 선택을 유지하려면 `markdown_mapping.body`를 명시한다. ([#295](https://github.com/ai-screams/HwpForge/pull/295))
- `MarkdownMapping`에 `numbered_item`, `check_item`, `bullet_levels` 필드가 늘어난다. 구조체를 리터럴로 만들던 코드는 컴파일되지 않으므로 `..Default::default()`를 붙인다. 수제 템플릿에 이 역할이 없으면 Markdown 쪽에서 그 종류의 목록을 지원하지 않는 것으로 다룬다(`hwpforge-smithy-md` 항목을 본다). ([#303](https://github.com/ai-screams/HwpForge/pull/303))

### Added

- 본문·제목·코드·인용·목록 역할의 스타일 이름 선택과 제목·목록 문단 모양 파생을 맡는 `markdown` 모듈을 더한다. `resolve_markdown_style_names`가 이름을 고르고 `apply_markdown_heading_levels`가 제목에 개요 수준을 붙인다. 제목이 본문이나 명시 목록과 같은 항목을 쓰면 원래 항목은 바꾸지 않고 제목용 문단 모양을 따로 만든다. ([#295](https://github.com/ai-screams/HwpForge/pull/295))
- 내장 템플릿 `default`와 `gov_proposal`에 `heading4`–`heading6`을 더한다. 모양은 `heading3`과 같고, 개요 4–6 수준이라 한컴 차례에 여섯 수준이 모두 들어간다. ([#295](https://github.com/ai-screams/HwpForge/pull/295))
- 기본 템플릿에 번호·체크 항목 역할과 수준별 글머리표·번호 정의를 넣고, `apply_markdown_list_levels`·`restart_markdown_numbering`으로 수준별 문단 모양을 파생한다. `restart_markdown_numbering`은 번호 정의를 복제해 번호 목록마다 새로 1부터 세게 한다. 수준 1–8의 번호 모양은 한컴 기본 형식(`1.`, `가.`, `1)`, `가)`, `(1)`, `(가)` 등)이다. ([#303](https://github.com/ai-screams/HwpForge/pull/303))

### Fixed

- 번호·글머리표 목록에서 표지 뒤 글의 시작이 번호마다 달라(`1.`은 13.2pt, `10.`은 18.7pt, `가.`는 17.5pt) 이어지는 문단과 어긋나던 것을 고친다. 항목의 이어지는 문단은 항목 여백에 기호 폭 20pt를 더한 자리에서 시작한다. 글머리표의 글 시작은 15pt에서 20pt로 옮겨 간다. 한계: 기호 폭 20pt는 글자 크기 10pt인 기본 템플릿에 맞춘 고정값이라, 본문 글자 크기를 바꾼 템플릿에서는 기호 뒤 글과 이어지는 문단이 어긋날 수 있다. ([#313](https://github.com/ai-screams/HwpForge/pull/313))

## [0.18.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-blueprint-v0.17.1...hwpforge-blueprint-v0.18.0) - 2026-10-25

### Changed

- 다른 `hwpforge-*` 크레이트를 같은 릴리스 버전(`0.18.0`)으로 요구한다. 이전에는 `0.x` 아무 버전이나 받아 다른 minor 와 섞일 수 있었다.

## [0.17.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-blueprint-v0.16.11...hwpforge-blueprint-v0.17.0) - 2026-10-23

### Changed (BREAKING)

- `CharShape.shadow` 와 `PartialCharShape.shadow` 가 `CharShadow`(종류·색·오프셋)가 되었다. YAML 은 종류 문자열(`shadow: drop`)과 `{kind, color, offset_x, offset_y}` 맵을 모두 받는다. 널 색, 모르는 키, -100–100 밖 오프셋은 오류다.
- 색을 주지 않은 그림자의 기본 색이 `#B2B2B2` 에서 한컴 새 문서와 같은 `#C0C0C0` 으로 바뀐다. 이전 색은 `shadow: {kind: drop, color: '#B2B2B2'}` 로 유지한다.

## [0.16.6](https://github.com/ai-screams/HwpForge/compare/hwpforge-blueprint-v0.16.5...hwpforge-blueprint-v0.16.6) - 2026-09-22

### Added

- *(bindings-cli)* `validate` 명령 신설 — MCP·Python 에만 있던 검증 연산을 CLI 에도


### Documentation

- PDF 렌더의 전제를 "한컴 저장본" 이 아니라 "조판 캐시" 로 적는다

- README 퀵스타트와 Rust 예시가 적힌 그대로 실행되게 한다

- *(python)* Python 바인딩 0.16.5 첫 wheel 배포를 문서에 반영


## [0.7.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-blueprint-v0.6.0...hwpforge-blueprint-v0.7.0) - 2026-06-19

### Documentation

- Wave 12l + Phase 12 series 완료 반영 (CLAUDE/MEMORY/README)


### Fixed

- *(blueprint)* **BREAKING** non_exhaustive on ParaShape/PartialParaShape/PartialStyle (B)

- *(blueprint)* **BREAKING** add #[non_exhaustive] to CharShape/PartialCharShape (B3)

- *(blueprint)* carry underline_shape through Blueprint char styles


## [0.5.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-blueprint-v0.4.0...hwpforge-blueprint-v0.5.0) - 2026-03-22

### Added

- *(list)* **BREAKING** add checkable bullet semantics

- *(list)* **BREAKING** add shared list semantics


### Documentation

- refresh readme and fix docs lint


### Fixed

- *(md)* preserve task list continuations

- *(list)* restore markdown task lists and tighten bullet semantics

- *(hwpx)* bridge registry-local style ids


## [0.4.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-blueprint-v0.3.0...hwpforge-blueprint-v0.4.0) - 2026-03-20

### Changed

- Extend blueprint templates and paragraph-shape IR with explicit tab definition collections and `tab_def_id` references.
- Add dedicated blueprint error codes for invalid, duplicate, and unknown tab references.

### Migration

- `Template`, `ParaShape`, and `PartialParaShape` struct literals must initialize the new tab-related fields.
- Exhaustive matches on `BlueprintErrorCode` must handle `InvalidTabReference`, `DuplicateTabDefinition`, and `InvalidTabDefinition`.

## [0.3.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-blueprint-v0.2.1...hwpforge-blueprint-v0.3.0) - 2026-03-18

### Chore

- *(release)* **BREAKING** prepare v0.3.0 for preserving section API changes


## [0.2.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-blueprint-v0.2.0...hwpforge-blueprint-v0.2.1) - 2026-03-17

### Fixed

- *(docs)* unescape HTML entities in details/summary tags


## [0.2.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-blueprint-v0.1.6...hwpforge-blueprint-v0.2.0) - 2026-03-17

### Changed

- Align the blueprint crate version with the workspace-wide `0.2.0` release line for consistent dependency pinning.

## [0.1.6](https://github.com/ai-screams/HwpForge/compare/hwpforge-blueprint-v0.1.5...hwpforge-blueprint-v0.1.6) - 2026-03-10

### Added

- *(mcp)* Phase 7c MCP Extended — 3 tools + 4 resources + 3 prompts


### Documentation

- *(readme)* update stats and MCP tool list for Phase 7c

- *(readme)* simplify MCP setup and update AI tool list


## [0.1.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-blueprint-v0.1.4...hwpforge-blueprint-v0.1.5) - 2026-03-10

### Fixed

- *(dist)* improve user experience for npm installation


## [0.1.3](https://github.com/ai-screams/HwpForge/compare/hwpforge-blueprint-v0.1.2...hwpforge-blueprint-v0.1.3) - 2026-03-09

### Added

- *(examples)* reorganize examples and add 16 HWPX showcase files


## [0.1.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-blueprint-v0.1.0...hwpforge-blueprint-v0.1.1) - 2026-03-07

### Documentation

- *(readme)* add supported Hancom versions table and cargo install instructions

- update LICENSE-APACHE to full text and add README badges


### Fixed

- *(readme)* replace broken Buy Me a Coffee button with stable CDN image

- use absolute URLs for README images (crates.io compatibility)
