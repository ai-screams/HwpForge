//! HwpForge Core: format-independent Document Object Model.
//!
//! This crate defines the universal document structure used across
//! all HwpForge format conversions. It is the **Anvil** in the Forge
//! metaphor -- the surface on which all documents are shaped.
//!
//! # Architecture
//!
//! Core sits one layer above Foundation:
//!
//! ```text
//! foundation (primitives: HwpUnit, Color, Index<T>)
//!     |
//!     v
//! core (this crate: Document, Section, Paragraph, Run)
//!     |
//!     v
//! blueprint (styles: CharShape, ParaShape, Template)
//!     |
//!     v
//! smithy-* (format codecs: HWPX, HWP5, Markdown)
//! ```
//!
//! Core has zero knowledge of XML, binary formats, or Markdown.
//! It references style definitions by branded indices (Foundation's
//! [`CharShapeIndex`](hwpforge_foundation::CharShapeIndex),
//! [`ParaShapeIndex`](hwpforge_foundation::ParaShapeIndex)) without
//! depending on Blueprint.
//!
//! # Document Lifecycle (Typestate)
//!
//! ```text
//! Document<Draft>  --(validate)-->  Document<Validated>
//!    (mutable)                         (immutable)
//! ```
//!
//! - [`Draft`] documents can be modified (add sections, set metadata).
//! - [`Validated`] documents are structurally sound and ready for export.
//! - Deserialization always produces `Draft` (must re-validate).
//!
//! # DOM Hierarchy
//!
//! ```text
//! Document
//!   +-- Metadata
//!   +-- Section[]
//!         +-- PageSettings
//!         +-- Paragraph[]
//!               +-- Run[]
//!                     +-- RunContent
//!                           +-- Text(String)
//!                           +-- Table(Box<Table>)
//!                           +-- Image(Image)
//!                           +-- Control(Box<Control>)
//! ```
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::*;
//! use hwpforge_core::run::Run;
//! use hwpforge_core::section::Section;
//! use hwpforge_core::paragraph::Paragraph;
//! use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
//!
//! let mut doc = Document::new();
//! doc.add_section(Section::with_paragraphs(
//!     vec![Paragraph::with_runs(
//!         vec![Run::text("Hello, HwpForge!", CharShapeIndex::new(0))],
//!         ParaShapeIndex::new(0),
//!     )],
//!     PageSettings::a4(),
//! ));
//!
//! let validated = doc.validate().unwrap();
//! assert_eq!(validated.section_count(), 1);
//! ```

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(clippy::all)]

pub mod caption;
pub mod chart;
pub mod column;
pub mod control;
pub mod document;
pub mod error;
pub mod image;
pub mod metadata;
pub mod page;
pub mod paragraph;
pub mod run;
pub mod section;
pub mod table;

mod validate;

// ---------------------------------------------------------------------------
// Re-exports for convenience
// ---------------------------------------------------------------------------

pub use caption::{Caption, CaptionSide};
pub use chart::{ChartData, ChartGrouping, ChartSeries, ChartType, LegendPosition, XySeries};
pub use column::{ColumnDef, ColumnLayoutMode, ColumnSettings, ColumnType};
pub use control::{Control, ShapePoint, ShapeStyle};
pub use document::{Document, Draft, Validated};
pub use error::{CoreError, CoreErrorCode, CoreResult, ValidationError};
pub use image::{Image, ImageFormat, ImageStore};
pub use metadata::Metadata;
pub use page::PageSettings;
pub use paragraph::Paragraph;
pub use run::{Run, RunContent};
pub use section::{HeaderFooter, PageNumber, Section};
pub use table::{Table, TableCell, TableRow};
