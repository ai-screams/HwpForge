//! Bounded Transitional DOCX text decoding into Core.
//!
//! Saved field results and body paragraphs are retained. Formatting, objects,
//! and ancillary content are reported as losses. Tracked changes are rejected
//! throughout every selected WordprocessingML part, including excluded subtrees.
//! This is a structural import, with no promise of matching displayed pages.
#![deny(missing_docs)]
#![deny(unsafe_code)]

mod body;
mod error;
mod package;
mod warning;
mod xml;

use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_core::{
    document::{Document, Draft},
    image::ImageStore,
};

pub use error::{DocxError, DocxResult};
pub use warning::DocxWarning;

/// Decoded structure and its registry-local styles, before output validation.
#[derive(Debug)]
#[non_exhaustive]
pub struct DocxDecoded {
    /// Body paragraphs in a single section with Core's default page settings.
    pub document: Document<Draft>,
    /// One character/paragraph shape pair from Blueprint's built-in body style.
    pub registry: StyleRegistry,
    /// Empty until image import is supported.
    pub image_store: ImageStore,
    /// Aggregated losses in first-occurrence order.
    pub warnings: Vec<DocxWarning>,
}

/// Decodes a Transitional DOCX package into a neutral Core bundle.
///
/// Reads each selected WordprocessingML part once, sharing XML text limits.
/// Does not fetch external relationships or interpret style definitions.
///
/// # Errors
///
/// Returns [`DocxError`] for required-part failures, unsafe or malformed XML,
/// resource limits, ambiguous field boundaries, or any detected tracked change.
/// Missing optional parts produce warnings. Opaque entries, missing or mismatched
/// content types or external targets on selected WordprocessingML parts, and ZIP
/// read errors are fatal.
///
/// ```no_run
/// use hwpforge_smithy_docx::decode_docx;
/// let bytes = std::fs::read("document.docx").unwrap();
/// let decoded = decode_docx(&bytes).unwrap();
/// for warning in &decoded.warnings {
///     eprintln!("{}: {}", warning.code(), warning.message());
/// }
/// ```
pub fn decode_docx(bytes: &[u8]) -> DocxResult<DocxDecoded> {
    body::decode(bytes)
}
