# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.16.6](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.16.5...hwpforge-bindings-mcp-v0.16.6) - 2026-09-22

### Added

- *(bindings-mcp)* inspect 가 ops 의 객체·문단 계수 9 필드를 섹션마다 낸다

- *(bindings-mcp)* 경고 채널이 없던 11 도구 응답에 warnings 배열을 additive 로 더한다

- *(hwpforge)* from_json 이 입력 JSON 의 layout_cache 를 버릴 때 LAYOUT_CACHE_DROPPED 경고를 낸다


### Changed

- *(hwpforge,bindings-mcp,bindings-py,bindings-cli)* InspectSection 의 미릴리스 계수 필드 이름이 스코프를 말하게 한다

- *(bindings-mcp)* 인라인 응답 상한을 output.rs 상수 하나로

- *(bindings-mcp,hwpforge)* MCP 요청 스키마가 smithy-hwpx 의 serde 타입을 직접 노출하지 않는다

- *(hwpforge,bindings-cli,bindings-mcp,bindings-py)* 세 창구가 각자 갖던 크기 게이트·경고 DTO·매니페스트 경로 규칙을 ops 로 올린다

- *(bindings-cli,bindings-mcp)* 힌트 해석을 한 경로로, 두 창구의 코드 합의 테스트, 스냅샷을 동결/신규로 분리

- *(bindings-mcp)* 19 도구를 `hwpforge::ops` 호출로 이전, 레거시 코드·힌트는 호환 층으로 고정


### Documentation

- *(hwpforge,bindings-cli,bindings-mcp)* 리뷰 상환 — 문서가 재는 것과 한도를 정확히 말한다

- *(bindings-mcp)* to_json 인라인 게이트가 재는 것과 실효 상한


### Fixed

- *(bindings-mcp)* 채울 수 없는 필드의 힌트를 실제로 되는 길로, 대체 문구 기록을 등식으로

- *(bindings-mcp)* 생성·편집 프롬프트의 잘못된 안내를 고친다

- *(bindings-mcp)* next·도구 설명이 실제로 되는 호출을 가리키게 한다

- *(bindings-mcp)* 거부 힌트가 통하는 길을 말하게 한다

- *(bindings-mcp,bindings-cli)* 리뷰 상환 — 동결 문구와 문맥 의존 힌트를 제자리에

- *(hwpforge,bindings-mcp)* 진단 정확화 — 거짓 NOTE 제거, Io 힌트 함정 수리, read 죽은 reason 단일화

- *(bindings-cli,bindings-mcp)* 리뷰 상환 — convert-hwp5 단일 유계 읽기, to_json 게이트 무할당 측정, validate 종료코드 문구 통일, to_pdf 변형 전수 테스트

- *(bindings-mcp)* validate 가 디코드 실패를 오류로 보고하고, to_json 게이트가 warnings 를 포함해 재며, 이전으로 은퇴한 코드를 분리한다

- *(bindings-cli,bindings-mcp)* 입력을 상한까지만 읽는다 — metadata 길이만 믿던 100MB 게이트를 우회하는 FIFO·파이프 입력 차단

- *(hwpforge,bindings-mcp)* restyle 가 디코드 경고를 잇고, MCP 구조 편집 도구가 디코드 경고를 걸러내지 않는다

- *(hwpforge,bindings-mcp)* 재평결 상환 — 메모 anchor_runs 캐시 탐지 복원, 그룹 자식 번호를 인코더 기준으로, 테스트 잠금

- *(hwpforge,bindings-mcp)* 리뷰 상환 — 캡션 캐시 탐지, 경고 경로 모양, 인라인 크기 게이트, 직렬화 단언

- *(bindings-mcp)* 호환 표 정정 — 결합 arm 복원, admission 오류 행, 힌트 오타, to_json 디코드 경고, read 검증 순서


## [0.16.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.16.4...hwpforge-bindings-mcp-v0.16.5) - 2026-09-17

### Documentation

- 공개 문서를 0.16.4 실측에 맞게 정정 — CLI 22·MCP 19·smithy-pdf·설치 명령·정책 문서


### Fixed

- *(bindings-mcp)* restyle 을 의미 손상 fail-closed 로 바꾸고 SemanticLoss 를 명시 매핑

- *(bindings-mcp)* rmcp 3.4 에서 deprecated 된 ServerInfo 별칭 대신 InitializeResult 사용


## [0.16.4](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.16.3...hwpforge-bindings-mcp-v0.16.4) - 2026-08-28

### Fixed

- *(smithy-hwpx)* 각주 번호부여 6차 평결 상환 — 정확 일치 가드·경고 표면 확대

- *(smithy-hwpx)* 각주 번호부여 5차 평결 상환 — 마커 가드 전종화·경고 관통


## [0.16.3](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.16.2...hwpforge-bindings-mcp-v0.16.3) - 2026-08-26

### Fixed

- *(smithy-md)* 임베드 리뷰 상환 — 단독 문단 보존·bare 경로 base·BMP 구조검사

- *(smithy-md)* md→hwpx 이미지 참조를 BinData 로 실제 임베드


## [0.16.2](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.16.1...hwpforge-bindings-mcp-v0.16.2) - 2026-08-26

### Fixed

- *(bindings-mcp)* rmcp 3.1 마이그레이션 (MRTR outcome enum·SEP-2549 필드)


## [0.11.7](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.11.6...hwpforge-bindings-mcp-v0.11.7) - 2026-07-25

### Added

- insert-para 배치·delete-para 경고를 CLI/MCP 표면에 노출 (E4 후속)


## [0.11.6](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.11.5...hwpforge-bindings-mcp-v0.11.6) - 2026-07-24

### Added

- feat(mcp)+docs(skill): E4 W4b — insert_para/delete_para 툴 + 스킬


### Changed

- refactor(hwpx)+test: E4 구조 편집 admission/self-verify DRY + 안전망 커버


### Fixed

- *(hwpx)* E4 독립 리뷰 상환 — 문단 id 재번호(H1) + strip depth-aware(M1) 외


## [0.11.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.11.4...hwpforge-bindings-mcp-v0.11.5) - 2026-07-23

### Added

- *(mcp)* hwpforge_diff 툴 — 편집 검증

- *(mcp)* hwpforge_read 툴 — 표적 텍스트 읽기

- *(mcp)* hwpforge_outline 툴 — 문서 항법 지도


### Documentation

- docs(skill)+feat(mcp): 스킬 축소 — outline→read→편집→diff 흐름으로 국소 교체


### Fixed

- *(hwpx)* diff 필드 스트립을 field 축 커버리지로 한정 + 리뷰 상환


## [0.11.4](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.11.3...hwpforge-bindings-mcp-v0.11.4) - 2026-07-22

### Added

- *(cli,mcp)* stamp v2 배선 — 셀 후보 plan·v2 맵·클래스-B 스탬핑 표면


## [0.11.3](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.11.2...hwpforge-bindings-mcp-v0.11.3) - 2026-07-21

### Added

- set-cell — 논리 격자 주소 기반 표 셀 편집 (E3 Wave 3)

- JSON export 에 표 셀 논리 격자 주소(addr) 노출 + import 검증-후-폐기


## [0.11.2](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.11.1...hwpforge-bindings-mcp-v0.11.2) - 2026-07-21

### Added

- MCP hwpforge_stamp_plan/hwpforge_stamp + 스킬 STAMP 경로


### Fixed

- 후속 검증 잔여 Low 상환 — MCP escape 일관성·manifest 경로 충돌 가드

- 리뷰 확정 이슈 상환 — 탐지기 O(n²) DoS·부분 산출물·엔트리명 escape


## [0.11.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.11.0...hwpforge-bindings-mcp-v0.11.1) - 2026-07-13

### Added

- *(mcp)* fields/fill 표면 — CLI 명령·MCP 툴·스킬 결정 트리


## [0.10.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.9.0...hwpforge-bindings-mcp-v0.10.0) - 2026-07-02

### Fixed

- *(deps)* rmcp 2.0 + quick-xml 0.41 (RUSTSEC GHSA-89vp-x53w-74fx, Dependabot #90)


## [0.8.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.7.0...hwpforge-bindings-mcp-v0.8.0) - 2026-06-27

### Fixed

- *(mcp)* deny sensitive-path writes in write_output_file (E1 #4 HIGH-1)

- *(hwp5,mcp)* address E1 audit findings

- *(mcp)* reject path traversal in write_output_file (E1 #4)


## [0.5.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.4.0...hwpforge-bindings-mcp-v0.5.0) - 2026-03-22

### Added

- *(list)* **BREAKING** add shared list semantics


### Fixed

- *(hwpx)* bridge registry-local style ids


## [0.4.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.3.0...hwpforge-bindings-mcp-v0.4.0) - 2026-03-19

### Chore

- *(release)* **BREAKING** prepare v0.4.0 for tab semantics


## [0.3.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.2.1...hwpforge-bindings-mcp-v0.3.0) - 2026-03-18

### Fixed

- *(bindings)* align section edit validation and warnings

- *(hwpx)* harden and unify preserving section workflows

- *(hwpx)* harden preserving section patch fidelity


### Chore

- *(release)* **BREAKING** prepare v0.3.0 for preserving section API changes


## [0.2.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.1.7...hwpforge-bindings-mcp-v0.2.0) - 2026-03-17

### Changed

- Align the MCP binding crate with the workspace-wide `0.2.0` release line.
- Adopt the `hwpforge-core` `0.2.0` table and image construction contract in the shipped command surface.

## [0.1.7](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.1.6...hwpforge-bindings-mcp-v0.1.7) - 2026-03-12

### Added

- *(cli/mcp)* add to-md command for HWPX→Markdown conversion


### Documentation

- add metadata extraction guide and fix MCP inspect metadata gap


## [0.1.6](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.1.5...hwpforge-bindings-mcp-v0.1.6) - 2026-03-10

### Added

- *(mcp)* Phase 7c MCP Extended — 3 tools + 4 resources + 3 prompts


### Changed

- *(mcp)* extract shared I/O helpers and eliminate TOCTOU race condition


### Fixed

- *(mcp)* address quality review — font contract docs, extension guard, range format

- *(mcp)* apply PR review fixes — TOCTOU comment, tests, error handling

- *(mcp)* fix restyle index mismatch and convert font override bugs


## [0.1.4](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.1.3...hwpforge-bindings-mcp-v0.1.4) - 2026-03-09

### Added

- *(mcp)* implement 5 MCP tools (convert, inspect, to_json, patch, templates)

- *(mcp)* add hwpforge-bindings-mcp crate skeleton with rmcp


### Changed

- extract shared types into smithy-hwpx to eliminate CLI/MCP duplication


### Documentation

- add Bindings branding (Hammer/Anvil/Tongs), MCP multi-platform install guide, SKILL.md agent rules

- *(mcp)* add README with installation and platform setup guides


### Fixed

- *(mcp)* add workspace metadata and dep versions for crates.io publish

- *(mcp)* add missing #[tool_handler] macro for MCP tool discovery

- *(mcp)* fix duplicate step comments and CLI-style hint text

- *(mcp)* harden security and correctness from audit

- *(mcp)* address code review findings (P1-P3)
