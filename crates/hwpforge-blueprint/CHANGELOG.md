# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
