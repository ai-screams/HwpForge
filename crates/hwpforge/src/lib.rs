//! # HwpForge
//!
//! Programmatic control of Korean HWP/HWPX documents.
//!
//! HwpForge lets you read, write, and convert [Hancom 한글](https://www.hancom.com/)
//! documents from Rust. It targets the HWPX format (ZIP + XML, KS X 6101) used by
//! modern versions of 한글.
//!
//! # Feature Flags
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `hwpx`  | ✅      | HWPX encoder/decoder |
//! | `md`    | —       | Markdown ↔ Core conversion |
//! | `full`  | —       | All features |
//!
//! # Quick Start
//!
//! ```no_run
//! use hwpforge::core::{Document, Draft, PageSettings, Paragraph, Run, Section};
//! use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};
//!
//! // Build a document programmatically
//! let mut doc = Document::<Draft>::new();
//! let paragraph = Paragraph::with_runs(
//!     vec![Run::text("Hello, 한글!", CharShapeIndex::new(0))],
//!     ParaShapeIndex::new(0),
//! );
//! let section = Section::with_paragraphs(vec![paragraph], PageSettings::a4());
//! doc.add_section(section);
//! ```
//!
//! ## Encode to HWPX
//!
//! ```no_run
//! # use hwpforge::core::{Document, Draft};
//! # let doc = Document::<Draft>::new();
//! use hwpforge::hwpx::{HwpxEncoder, HwpxStyleStore};
//! use hwpforge::core::ImageStore;
//!
//! let validated = doc.validate().unwrap();
//! let style_store = HwpxStyleStore::with_default_fonts("함초롬바탕");
//! let image_store = ImageStore::new();
//! let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store).unwrap();
//! std::fs::write("output.hwpx", &bytes).unwrap();
//! ```
//!
//! ## Decode from HWPX
//!
//! ```no_run
//! use hwpforge::hwpx::HwpxDecoder;
//!
//! let result = HwpxDecoder::decode_file("input.hwpx").unwrap();
//! println!("Sections: {}", result.document.sections().len());
//! ```

/// Foundation types: [`HwpUnit`](foundation::HwpUnit), [`Color`](foundation::Color),
/// branded indices, and core enums.
pub use hwpforge_foundation as foundation;

/// Format-independent document model: [`Document`](core::Document),
/// [`Section`](core::Section), [`Paragraph`](core::Paragraph),
/// [`Table`](core::Table), [`Control`](core::Control).
pub use hwpforge_core as core;

/// YAML-based style template system with inheritance and merge.
pub use hwpforge_blueprint as blueprint;

/// HWPX format codec (encoder + decoder).
#[cfg(feature = "hwpx")]
pub use hwpforge_smithy_hwpx as hwpx;

/// Markdown codec (GFM decoder + lossy/lossless encoder).
#[cfg(feature = "md")]
pub use hwpforge_smithy_md as md;
