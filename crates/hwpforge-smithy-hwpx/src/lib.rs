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
//! - Text runs with character shapes, paragraph shapes, styles
//! - Tables (nested), images (binary + path), text boxes
//! - Headers, footers, page numbers, footnotes, endnotes
//! - Shapes: line, ellipse, polygon, arc, curve, connect line
//! - Equations (HancomEQN), charts (18 types, OOXML)
//! - Multi-column layouts, captions, bookmarks, fields, memos
//! - Page settings (size, margins, landscape, gutter, master pages)
//!
//! Not yet supported:
//! - OLE objects, form controls, change tracking

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(unsafe_code)]

mod color;
pub mod decoder;
pub mod default_styles;
mod encoder;
pub mod error;
pub mod exchange;
mod inline_text;
mod list_bridge;
mod patch;
pub mod presets;
mod schema;
mod section_workflow;
mod style_lookup_bridge;
pub mod style_store;

pub use decoder::package::{PackageEntryInfo, PackageReader};
pub use decoder::{HwpxDecoder, HwpxDocument};
pub use default_styles::{DefaultStyleEntry, HancomStyleSet};
pub use encoder::HwpxEncoder;
pub use error::{HwpxError, HwpxErrorCode, HwpxResult};
pub use exchange::{
    ExportedDocument, ExportedSection, PreservedTextSlot, SectionPreservation, TextLocator,
    SECTION_PRESERVATION_VERSION,
};
pub use patch::HwpxPatcher;
pub use presets::{builtin_presets, style_store_for_preset, PresetInfo};
pub use section_workflow::{
    SectionExportOutcome, SectionPatchOutcome, SectionWorkflowError, SectionWorkflowWarning,
};
pub use style_lookup_bridge::HwpxStyleLookup;
pub use style_store::{
    HwpxCharShape, HwpxFont, HwpxFontRef, HwpxParaShape, HwpxStyle, HwpxStyleStore,
};
