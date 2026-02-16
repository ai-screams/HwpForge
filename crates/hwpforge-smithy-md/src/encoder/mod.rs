//! Core -> Markdown encoders.

mod lossless;
mod lossy;

use hwpforge_blueprint::template::Template;
use hwpforge_core::{Document, Validated};

use crate::error::MdResult;

/// Markdown encoder entrypoint.
pub struct MdEncoder;

impl MdEncoder {
    /// Encodes a validated document into markdown with frontmatter.
    ///
    /// This method is mapping-aware and uses the provided template to map
    /// paragraph style IDs back into markdown semantics.
    pub fn encode(document: &Document<Validated>, template: &Template) -> MdResult<String> {
        lossy::encode_with_template(document, template)
    }

    /// Encodes a validated document into readable markdown without template mapping.
    pub fn encode_lossy(document: &Document<Validated>) -> MdResult<String> {
        lossy::encode_without_template(document)
    }

    /// Encodes a validated document into lossless markdown (frontmatter + HTML-like body).
    pub fn encode_lossless(document: &Document<Validated>) -> MdResult<String> {
        lossless::encode_lossless(document)
    }
}
