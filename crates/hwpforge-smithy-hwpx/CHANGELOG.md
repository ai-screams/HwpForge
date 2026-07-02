# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
