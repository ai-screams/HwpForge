# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
