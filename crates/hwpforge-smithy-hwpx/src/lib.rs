//! HWPX format decoder for HwpForge.
//!
//! This crate reads HWPX files (ZIP archives containing XML, per KS X 6101)
//! and converts them into HwpForge Core's `Document<Draft>` representation.
//!
//! # Architecture
//!
//! The decoding pipeline has four stages:
//!
//! 1. **Package** — Open ZIP, validate mimetype, enumerate section files
//! 2. **Header** — Parse `Contents/header.xml` → [`HwpxStyleStore`]
//! 3. **Section** — Parse `Contents/section*.xml` → paragraphs + page settings
//! 4. **Assembly** — Combine into `Document<Draft>` with sections
//!
//! # Quick Start
//!
//! ```no_run
//! use hwpforge_smithy_hwpx::HwpxDecoder;
//!
//! let result = HwpxDecoder::decode_file("document.hwpx").unwrap();
//! println!("Sections: {}", result.document.sections().len());
//! println!("Fonts: {}", result.style_store.font_count());
//! ```
//!
//! # Phase 3 Scope
//!
//! Currently supports:
//! - Text runs, tables, images
//! - Fonts, character shapes, paragraph shapes from `header.xml`
//! - Page settings (size, margins) from `<secPr>` in sections
//!
//! Does **not** yet support:
//! - Encoding (write-back to HWPX)
//! - Footnotes, endnotes, bookmarks, field codes
//! - Drawing objects, OLE, equations

#![deny(missing_docs)]
#![deny(unsafe_code)]

pub mod decoder;
pub mod error;
mod schema;
pub mod style_store;

pub use decoder::{HwpxDecoder, HwpxDocument};
pub use error::{HwpxError, HwpxErrorCode, HwpxResult};
pub use style_store::{
    HwpxCharShape, HwpxFont, HwpxFontRef, HwpxParaShape, HwpxStyleStore,
};
