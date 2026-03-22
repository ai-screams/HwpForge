# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.4.0...hwpforge-core-v0.5.0) - 2026-03-22

### Added

- *(list)* **BREAKING** add checkable bullet semantics

- *(list)* **BREAKING** add shared list semantics


### Fixed

- *(md)* preserve task list continuations


## [0.4.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.3.0...hwpforge-core-v0.4.0) - 2026-03-20

### Changed

- Extend `TabDef` with explicit `TabStop` semantics so tab definitions can carry stop position, alignment, and leader data through the shared IR.
- Add shared helpers for default-tab merging, reference validation, and tab-position clamping used by HWPX/HWP5 bridges.

### Migration

- `TabDef` struct literals must now initialize the `stops` field.
- Consumers that duplicated tab-default merge or reference-validation logic should move to the shared helpers on `TabDef`.

## [0.3.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.2.1...hwpforge-core-v0.3.0) - 2026-03-18

### Chore

- *(release)* **BREAKING** prepare v0.3.0 for preserving section API changes


## [0.2.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.1.7...hwpforge-core-v0.2.0) - 2026-03-17

### Changed

- Extend the public table DOM with page-break, repeat-header, cell-spacing, border/fill, row-header, cell height, margin, and vertical-alignment semantics.
- Extend the public image DOM with placement metadata.
- Move `ValidationError::NonLeadingTableHeaderRow` to the tail of the enum to avoid unnecessary discriminant drift for existing variants.

### Migration

- `Table`, `TableRow`, `TableCell`, and `Image` are now `#[non_exhaustive]`. Construct them with `new`/`with_*` builders instead of struct literals.
- New builder methods are available on `Table`, `TableCell`, and `Image` to cover the v0.2.0 public fields without direct field construction.
- Validation code consumers should handle `CoreErrorCode::NonLeadingTableHeaderRow`.

## [0.1.7](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.1.6...hwpforge-core-v0.1.7) - 2026-03-12

### Added

- HWPX→Markdown styled conversion pipeline


## [0.1.4](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.1.3...hwpforge-core-v0.1.4) - 2026-03-09

### Changed

- extract shared types into smithy-hwpx to eliminate CLI/MCP duplication
