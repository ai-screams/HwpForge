# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.10.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.9.0...hwpforge-core-v0.10.0) - 2026-07-02

### Added

- *(core)* **BREAKING** carry multi-column separator line (colLine)


## [0.9.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.8.0...hwpforge-core-v0.9.0) - 2026-06-28

### Changed

- *(core)* **BREAKING** link cross-ref to target via shared ObjectId (E6 M2)


## [0.8.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.7.0...hwpforge-core-v0.8.0) - 2026-06-27

### Added

- *(core)* **BREAKING** shape text vertical alignment (ellipse/polygon/textbox)


### Changed

- *(core)* **BREAKING** rename Summery typo to Summary in IR identifiers (E6 slice A)

- *(core)* split control.rs into submodules (E7 #2)


## [0.7.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-core-v0.6.0...hwpforge-core-v0.7.0) - 2026-06-19

### Added

- *(hwp5)* TextArt (글맵시 / <hp:textart>) carry — HWP5↔HWPX

- *(hwp5)* carry group/묶음 객체 (<hp:container>) HWP5↔HWPX — Wave A (flat)

- *(core)* **BREAKING** Wave 12p Step 2 — Core breaking: Image/Table/Equation 에 inst_id (cross-ref target carry)

- *(hwp5)* **BREAKING** Wave 12m Phase 2 Step 4 — Control::CrossRef vertical slice

- *(core)* **BREAKING** Wave 12m Phase 2 Step 3 — foundation/core API breaking (RefType + RefContentType + RefTarget + Control::CrossRef target)

- *(core)* **BREAKING** Wave 12o Phase 0 — Metadata에 description/last_saved_by/extras + non_exhaustive (ADR-003)

- *(core)* **BREAKING** Wave 12n — 자동 필드 의미 분할 + HWPX carry

- *(hwp5)* **BREAKING** Wave 12l — 누름틀(ClickHere) carry + name 메타 carry

- *(hwp5)* **BREAKING** Wave 12j — 글자겹침(compose) carry + char_pr_ids fidelity + packed-variant 지원

- *(hwp5)* **BREAKING** Wave 12i — 덧말(dutmal) option carry + flat-path control_iter 필터

- *(hwp5)* **BREAKING** Wave 12f-h — 메모 anchor 위치 수정 + 7 parameters wire 메타 carry 완성

- *(hwp5)* **BREAKING** Wave 12e — 메모 본문 carry + 본문 텍스트 덮어쓰기 버그 수정


### Fixed

- *(hwp5)* carry SUMMERY/PATH field cached value to close 한컴 recovery warning (#120/#136)

- *(hwpx)* Wave 12p task #124 — SUMMERY editable per FieldType + Wave 12p Step 4 visual gate + fmt fallout


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
