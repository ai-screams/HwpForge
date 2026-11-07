# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.20.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.19.1...hwpforge-foundation-v0.20.0) - 2026-11-07

### Changed

- 워크스페이스 버전을 맞춘다. 이 크레이트 자체의 구현 변경은 없다.

## [0.19.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.19.0...hwpforge-foundation-v0.19.1) - 2026-11-03

### Changed

- 워크스페이스 버전을 맞춘다. 이 크레이트 자체의 구현 변경은 없다.

## [0.19.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.18.0...hwpforge-foundation-v0.19.0) - 2026-11-02

### Changed (BREAKING)

- `StyleIndex`를 `StyleRef`로 바꾼다. 문서 안에서만 뜻이 있는 스타일 참조이며 HWPX 출력 번호가 아니다. 출력 번호는 HWPX 스타일 저장소만 정한다. 이행: `StyleIndex`를 `StyleRef`로 고쳐 쓴다. 생성(`StyleRef::new(n)`)과 값 접근은 전과 같다. ([#295](https://github.com/ai-screams/HwpForge/pull/295))

### Added

- 공통 `OpsCode`에 `Hwp5Input`(`HWP5_INPUT`)과 `Hwp5ConversionLoss`(`HWP5_CONVERSION_LOSS`)를 더한다. 앞의 것은 HWPX만 받는 연산에 HWP5(`.hwp`) 파일이 들어왔다는 뜻이고, 뒤의 것은 `.hwp`를 변환하는 중 문서 의미가 사라져 편집을 거부했다는 뜻이다. `OpsCode`는 `#[non_exhaustive]`이므로 `match`에 이미 기본 갈래가 있으면 컴파일은 그대로 된다. ([#310](https://github.com/ai-screams/HwpForge/pull/310))

## [0.18.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.17.1...hwpforge-foundation-v0.18.0) - 2026-10-25

### Changed (BREAKING)

- `SetCellCodecFailed`·`StampCodecFailed`·`StructuralCodec`·`StampDeltaMismatch`·`StampCellTargetDuplicate`와 옛 문자열 lookup을 삭제한다. 실패 단계에 맞는 코드, `InternalInvariant`·`SelfVerifyFailed`·`CellTargetDuplicate`를 사용한다.
- `ReadSectionOutOfRange`·`ReadTableOutOfRange`·`ReadFieldNotFound`·`SectionWorkflowFailed`·`SetCellFailed`·`StampFailed`·`StructuralEditFailed`와 옛 문자열 lookup을 삭제한다. 해당 도메인 코드나 `UpstreamUnmapped`를 사용한다.

### Added

- 공통 `OpsCode`에 `InvalidParams`·`FileNotFound`·`FileReadFailed`·`FileWriteFailed`·`InvalidOutputPath`·`InvalidExtension`·`OutputTooLarge`·`ManifestPathConflict`·`ProtectedFieldRange`를 더한다. 문자열은 `INVALID_PARAMS`·`FILE_NOT_FOUND`처럼 변형 이름의 대문자 snake 형이다.

## [0.17.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.16.11...hwpforge-foundation-v0.17.0) - 2026-10-23

### Added

- 글자 그림자를 종류·색·오프셋으로 담는 `CharShadow`, `ShadowOffset` 과 연속(`Continuous`) 그림자 종류.
- 진단 코드 `LEGACY_JSON_TABS` 등 0.17.0 의 JSON 교환과 경고에 쓰이는 코드.

## [0.16.6](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.16.5...hwpforge-foundation-v0.16.6) - 2026-09-22

### Added

- *(bindings-cli)* `validate` 명령 신설 — MCP·Python 에만 있던 검증 연산을 CLI 에도


### Documentation

- PDF 렌더의 전제를 "한컴 저장본" 이 아니라 "조판 캐시" 로 적는다

- README 퀵스타트와 Rust 예시가 적힌 그대로 실행되게 한다

- *(python)* Python 바인딩 0.16.5 첫 wheel 배포를 문서에 반영


## [0.16.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.16.4...hwpforge-foundation-v0.16.5) - 2026-09-17

### Added

- *(hwpforge)* ops 오류 모델을 단계 구분으로 — Decode/Encode·MdDecode/MdEncode, 의미 손상 봉투, 인자 거부 코드

- *(foundation)* OpsCode 에 IO_FAILED·INPUT_TOO_LARGE·NO_FONTS·ASSET_PLAN_MISMATCH·ASSET_IDENTITY_CONFLICT 추가

- *(foundation)* diagnostics 모듈 — OpsCode 정본 코드 표와 WarningInfo


### Documentation

- 브랜치 전체 리뷰 반영 — 릴리스 이력 포인터·architecture mermaid·MSRV 예외 정정

- 공개 문서를 0.16.4 실측에 맞게 정정 — CLI 22·MCP 19·smithy-pdf·설치 명령·정책 문서


## [0.9.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.8.0...hwpforge-foundation-v0.9.0) - 2026-06-28

### Changed

- *(foundation)* **BREAKING** collapse RefContentType::BookmarkName into Contents (E6 slice B)


## [0.8.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.7.0...hwpforge-foundation-v0.8.0) - 2026-06-27

### Added

- *(core)* **BREAKING** shape text vertical alignment (ellipse/polygon/textbox)


### Changed

- *(core)* **BREAKING** rename Summery typo to Summary in IR identifiers (E6 slice A)

- *(foundation)* split enums.rs into domain submodules (E7 #1)


### Documentation

- sync README/mdbook/CLAUDE for hwpforge-convert (E5) + refresh metrics


## [0.7.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.6.0...hwpforge-foundation-v0.7.0) - 2026-06-19

### Added

- *(core)* **BREAKING** Wave 12m Phase 2 Step 3 — foundation/core API breaking (RefType + RefContentType + RefTarget + Control::CrossRef target)

- *(core)* **BREAKING** Wave 12n — 자동 필드 의미 분할 + HWPX carry


### Documentation

- Wave 12l + Phase 12 series 완료 반영 (CLAUDE/MEMORY/README)


### Fixed

- *(hwpx)* Wave 12p task #124 — SUMMERY editable per FieldType + Wave 12p Step 4 visual gate + fmt fallout

- *(foundation)* **BREAKING** RefContentType::BookmarkName 부활 + Bookmark N2 매핑 native 일치 (Wave 12m fixup regression)

- *(hwpx)* **BREAKING** Wave 12m fixup — fieldid `%xrf` magic + RefContentType::BookmarkName 폐기 (시각 검증 통과)


## [0.5.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.4.0...hwpforge-foundation-v0.5.0) - 2026-03-22

### Added

- *(list)* **BREAKING** add shared list semantics


### Documentation

- refresh readme and fix docs lint


### Fixed

- *(hwpx)* bridge registry-local style ids


## [0.4.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.3.0...hwpforge-foundation-v0.4.0) - 2026-03-19

### Added

- *(tab)* **BREAKING** implement shared tab semantics across hwpx and hwp5


## [0.2.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.2.0...hwpforge-foundation-v0.2.1) - 2026-03-17

### Fixed

- *(docs)* unescape HTML entities in details/summary tags


## [0.2.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.1.5...hwpforge-foundation-v0.2.0) - 2026-03-17

### Changed

- Align the foundation crate version with the workspace-wide `0.2.0` release line for a consistent dependency surface.

## [0.1.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.1.4...hwpforge-foundation-v0.1.5) - 2026-03-10

### Fixed

- *(dist)* improve user experience for npm installation


## [0.1.3](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.1.2...hwpforge-foundation-v0.1.3) - 2026-03-09

### Added

- *(examples)* reorganize examples and add 16 HWPX showcase files


### Fixed

- *(encoder)* add pattern fill (hatchStyle) support and fix BACK_SLASH/SLASH swap


## [0.1.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-foundation-v0.1.0...hwpforge-foundation-v0.1.1) - 2026-03-07

### Documentation

- *(readme)* add supported Hancom versions table and cargo install instructions

- update LICENSE-APACHE to full text and add README badges


### Fixed

- *(readme)* replace broken Buy Me a Coffee button with stable CDN image

- use absolute URLs for README images (crates.io compatibility)
