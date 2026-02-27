//! HWPX format codec for HwpForge.
//!
//! This crate reads and writes HWPX files (ZIP archives containing XML,
//! per KS X 6101), converting between HwpForge Core's document types
//! and the HWPX on-disk format.
//!
//! # Architecture
//!
//! **Decoding** (HWPX → Core):
//! 1. Open ZIP, validate mimetype, enumerate section files
//! 2. Parse `Contents/header.xml` → [`HwpxStyleStore`]
//! 3. Parse `Contents/section*.xml` → paragraphs + page settings
//! 4. Assemble `Document<Draft>` with sections
//!
//! **Encoding** (Core → HWPX):
//! 1. Serialize [`HwpxStyleStore`] → `header.xml`
//! 2. Serialize each section → `section{N}.xml`
//! 3. Package into ZIP with metadata files
//!
//! # Quick Start
//!
//! ```no_run
//! use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};
//!
//! // Decode
//! let result = HwpxDecoder::decode_file("document.hwpx").unwrap();
//! println!("Sections: {}", result.document.sections().len());
//!
//! // Round-trip: decode → validate → encode
//! let validated = result.document.validate().unwrap();
//! let output = HwpxEncoder::encode(&validated, &result.style_store, &result.image_store).unwrap();
//! std::fs::write("output.hwpx", &output).unwrap();
//! ```
//!
//! # Supported Content
//!
//! - Text runs, tables (nested), images
//! - Fonts, character shapes, paragraph shapes from `header.xml`
//! - Page settings (size, margins) from `<secPr>` in sections
//!
//! Not yet supported:
//! - OLE objects, form controls, change tracking, bookmarks
//! - Field codes

#![deny(missing_docs)]
#![deny(unsafe_code)]

pub mod decoder;
pub mod default_styles;
mod encoder;
pub mod error;
mod schema;
pub mod style_store;

pub use decoder::{HwpxDecoder, HwpxDocument};
pub use default_styles::{DefaultStyleEntry, HancomStyleSet};
pub use encoder::HwpxEncoder;
pub use error::{HwpxError, HwpxErrorCode, HwpxResult};
pub use style_store::{
    HwpxCharShape, HwpxFont, HwpxFontRef, HwpxParaShape, HwpxStyle, HwpxStyleStore,
};
