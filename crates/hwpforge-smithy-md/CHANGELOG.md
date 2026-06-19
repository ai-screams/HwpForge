# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
