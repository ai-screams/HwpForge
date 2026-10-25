# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed (BREAKING)

- 공통 `OpsCode` 문자열을 별명 없이 출력하고 도구별 힌트는 유지한다. read의 구역·표·필드 부재는 `SECTION_OUT_OF_RANGE`·`TABLE_NOT_FOUND`·`FIELD_NOT_FOUND`다. 파일 오류는 공통 I/O 코드, 출력 경로 거부는 `INVALID_OUTPUT_PATH`다.
- cell·stamp·문단 편집의 decode·admission·validation·encode·verification 단계를 구분한다. 보호 필드 침범은 `PROTECTED_FIELD_RANGE`, 내부 불변식 파손은 `INTERNAL_INVARIANT`다. 바이트 깊이 초과는 버그 보고 힌트 없는 `DECODE_FAILED`다. stamp의 delta 검증과 중복 셀 지정은 `SELF_VERIFY_FAILED`·`CELL_TARGET_DUPLICATE`다.
- `SERIALIZE_ERROR`·`STAMP_MANIFEST_SERIALIZE`를 `JSON_SERIALIZE_FAILED`로 통합한다. set-cell·stamp의 의미 손실은 `ENCODE_SEMANTIC_LOSS`다. 구조 편집의 범위 실패는 `SECTION_OUT_OF_RANGE`·`PARAGRAPH_OUT_OF_RANGE`, ZIP 미보존은 `INPUT_ENTRIES_NOT_CARRIED`다.
- 19개 도구의 필드 역직렬화 실패를 구조화된 `INVALID_PARAMS`로 반환하고 메시지에 필드 경로를 담는다. 빠진 필드는 소속 객체의 경로(루트는 `$`)를 알린다. 입력 스키마와 프로토콜 오류는 유지한다.
- `hwpforge_patch`·`hwpforge_fill`은 실패 단계와 원인을 구분한다. 입력 decode는 `DECODE_FAILED`, 문서 검증은 `VALIDATION_FAILED`, 출력 생성은 `ENCODE_FAILED`, 보호 필드 침범은 `PROTECTED_FIELD_RANGE`, 내부 불변식 파손은 `INTERNAL_INVARIANT`다. `PATCH_FAILED`는 보존 patch가 거부하는 변경·metadata에 남고 fill은 이 원인을 그대로 전달한다. fill 입력 decode의 힌트는 `Check that the file is valid HWPX.`를 유지한다.
- `convert`·`from_json`·`restyle`·`insert_para`·`delete_para`의 비어 있지 않은 `warnings`가 문자열 배열에서 `{code, message, hint?}` 객체 배열로 바뀐다. 문자열 대신 `message`를 읽는다. `insert_para`·`delete_para`의 decode·encode 메시지에서 코드 접두를 빼므로 `code`를 읽는다. 18개 도구의 빈 경고 생략과 `templates`의 경고 필드 부재는 유지한다.

### Changed

- `INPUT_NOT_ROUNDTRIP_SAFE` 힌트는 코덱 갭 수정 대기 안내 대신 원본을 보존하는 `hwpforge_to_json(section)` → `hwpforge_patch`, 누름틀 `hwpforge_fill` 경로를 안내한다.

## [0.17.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.17.0...hwpforge-bindings-mcp-v0.17.1) - 2026-10-24

### Changed

- 표준 입력 JSON-RPC 한 줄의 길이 상한을 32 MiB 로 둔다. 넘는 줄은 개행까지 버리고 오류 로그를 남기며, 연결은 다음 요청을 계속 받는다. 이 값은 줄 길이 상한이고 버퍼 할당이나 프로세스 메모리 상한이 아니다. 전에는 아주 긴 한 줄(또는 개행 없는 입력)이 메모리를 계속 쓰게 할 수 있었다. rmcp 최소 버전을 3.6 으로 올린다(MSRV 1.89 그대로) (#264)
- 도구의 인라인 입력 상한(`convert` Markdown, `from_json` 구조)을 50 MiB 에서 24 MiB 로 낮춘다. 더 큰 문서는 파일 기반 도구를 쓴다. 측정한 가장 큰 요청 한 줄은 14.7 MiB 였다 (#264)
- 도구 결과가 0.17.1 라이브러리의 변경(빈 그림 틀 보존, 깊이 관문, `paragraph_content` 경고, XML 이벤트 버퍼 한도)을 그대로 따른다.

## [0.17.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.16.11...hwpforge-bindings-mcp-v0.17.0) - 2026-10-23

### Changed (BREAKING)

- `to_json` 이 `format_version: 2` JSON 을 내보내고 문단 여백·탭을 `{"unit", "value"}` 로 쓴다. `from_json`·`patch` 는 버전 없는 JSON 에 탭 위치가 있으면 `LEGACY_JSON_TABS` 로 거부하고, 없으면 `LEGACY_PARAGRAPH_MEASURES` 경고와 함께 읽는다.

### Changed

- 도구 결과가 0.17.0 라이브러리의 새 경고(`CONTENT_LOST`, `OBJECT_ID_CONFLICT`, `PACKAGE_ID_CONFLICT`, `IMAGE_REFERENCE_UNRESOLVED`, `BEGIN_NUM_REPAIRED`, `CONTAINER_TOO_DEEP`, `XML_FORBIDDEN_CHARS_REMOVED`)를 그대로 전한다. `restyle`, `set_cell`, `stamp` 는 내용을 잃은 입력을 거부한다.

## [0.16.11](https://github.com/ai-screams/HwpForge/compare/hwpforge-bindings-mcp-v0.16.10...hwpforge-bindings-mcp-v0.16.11) - 2026-10-13

### Fixed

- *(bindings)* 서버가 `initialize` 응답에 알리는 제목을 `Vise — HwpForge MCP Server` 로 바꾼다(시작 로그도 같음). 식별자 `hwpforge-mcp` 는 그대로다 (#234)

### Documentation

- 서버 정보 설명과 README 에 한컴 HWP 문서 파일 공개 문서 이용 표기를 넣는다 (#232)

- MCP 서버의 별칭을 Anvil 에서 **Vise** 로 바꾼다. crate 설명과 npm README 문구가 바뀌고, 설치 명령과 패키지 이름은 그대로다 (#233)

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
