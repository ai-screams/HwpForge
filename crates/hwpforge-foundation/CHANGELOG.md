# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
