# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
