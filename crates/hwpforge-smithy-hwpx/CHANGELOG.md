# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
