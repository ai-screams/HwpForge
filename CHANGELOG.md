# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0] - 2026-03-20

### Changed

- Promote the workspace release line to `0.4.0` for the breaking tab semantics contract in `hwpforge-core` and `hwpforge-blueprint`.
- Add shared tab-stop semantics across the IR stack so HWPX/HWP5 codecs can preserve explicit tab definitions and paragraph tab references.

### Migration

- `hwpforge_core::TabDef` now includes explicit `stops`; downstream struct literals must initialize the new field.
- `hwpforge_blueprint::Template`, `ParaShape`, and `PartialParaShape` now carry tab definition references/collections directly.
- Consumers matching on `BlueprintErrorCode` should handle the new tab-related error codes explicitly.

## [0.3.0] - 2026-03-19

### Changed

- Promote the workspace release line to `0.3.0` for the breaking HWPX section editing contract update.
- Preserve-first section editing now requires preservation metadata on `ExportedSection` and rejects stale or legacy section exports explicitly.

## [0.2.0] - 2026-03-17

### Changed

- Adopt the `hwpforge-core` v0.2.0 public DOM contract for richer table and image semantics.
- Align the workspace release line and internal crate pins on `0.2.0`.

### Migration

- `Table`, `TableRow`, `TableCell`, and `Image` are now `#[non_exhaustive]` and should be constructed via `new`/`with_*` builders instead of struct literals.
- Table DOM now carries page-break, repeat-header, cell-spacing, border/fill, header-row, cell margin, and vertical-alignment semantics directly in `hwpforge-core`.
- Image DOM now carries placement metadata directly in `hwpforge-core`.
- Validation now exposes `CoreErrorCode::NonLeadingTableHeaderRow`; downstream code that inspects validation codes should handle it explicitly.

## [0.1.0] - 2026-03-06

### Added

- **hwpforge**: Umbrella crate with feature flags (`hwpx`, `md`, `full`)
- **hwpforge-foundation**: Primitive types (HwpUnit, Color BGR, branded Index<T>, enums, error codes)
- **hwpforge-core**: Format-independent document model with typestate validation (Draft/Validated)
  - Document, Section, Paragraph, Run, Table, Image
  - Controls: TextBox, Footnote, Endnote, Equation, Chart (18 types)
  - Shapes: Line, Ellipse, Polygon, Arc, Curve, ConnectLine
  - References: Bookmark, CrossRef, Field, Memo, IndexMark
  - Layout: Multi-column, captions, headers/footers, page numbers, master pages
  - Annotations: Dutmal, compose characters
- **hwpforge-blueprint**: YAML-based style template system
  - Template inheritance with DFS merge
  - StyleRegistry with deduplicated fonts, char shapes, para shapes
  - Built-in default template (Hancom 한컴바탕)
  - BorderFill support
- **hwpforge-smithy-hwpx**: Full HWPX codec (KS X 6101)
  - Decoder: HWPX ZIP+XML -> Core Document
  - Encoder: Core Document -> HWPX ZIP+XML
  - Lossless roundtrip for all supported content
  - HancomStyleSet support (Classic/Modern/Latest)
  - 22 default styles with per-style charPr/paraPr
  - ZIP bomb defense (50MB/500MB/10k limits)
  - OOXML chart generation (18 chart types)
  - Golden fixture tests with real Hancom 한글 files
- **hwpforge-smithy-md**: Markdown codec
  - GFM decoder (pulldown-cmark) with YAML frontmatter
  - Lossy encoder (readable GFM) and lossless encoder (HTML+YAML)
  - Full pipeline: MD -> Core -> HWPX verified in Hancom 한글

[0.1.0]: https://github.com/ai-screams/HwpForge/releases/tag/v0.1.0
