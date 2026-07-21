//! Core -> Markdown encoders.

mod list_format;
mod lossless;
mod lossy;
mod styled;

use hwpforge_blueprint::template::Template;
use hwpforge_core::{Document, StyleLookup, Validated};

use crate::error::MdResult;

pub use styled::MdOutput;

/// Markdown encoder entrypoint.
pub struct MdEncoder;

/// Warning emitted by the lossy markdown encoders (warning-first: what the
/// output can no longer represent is reported, not silently dropped).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MdWarning {
    /// A table with merged cells was flattened into a plain GFM pipe grid
    /// (spans are not representable there). The style-aware encoder
    /// (`encode_styled`) renders such tables as HTML with colspan/rowspan
    /// instead and does not emit this warning.
    MergedCellsFlattened {
        /// Number of cells in the table with `col_span > 1` or `row_span > 1`.
        merged_cells: usize,
    },
}

impl core::fmt::Display for MdWarning {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MergedCellsFlattened { merged_cells } => write!(
                f,
                "table with {merged_cells} merged cell(s) flattened to a plain GFM grid \
                 (use styled mode to keep spans as HTML)"
            ),
        }
    }
}

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

    /// Like [`MdEncoder::encode_lossy`], additionally reporting what the
    /// lossy rendering dropped (e.g. merged table cells flattened to GFM).
    pub fn encode_lossy_with_report(
        document: &Document<Validated>,
    ) -> MdResult<(String, Vec<MdWarning>)> {
        lossy::encode_without_template_report(document)
    }

    /// Like [`MdEncoder::encode`], additionally reporting what the lossy
    /// rendering dropped.
    pub fn encode_with_report(
        document: &Document<Validated>,
        template: &Template,
    ) -> MdResult<(String, Vec<MdWarning>)> {
        lossy::encode_with_template_report(document, template)
    }

    /// Encodes a validated document into lossless markdown (frontmatter + HTML-like body).
    pub fn encode_lossless(document: &Document<Validated>) -> MdResult<String> {
        lossless::encode_lossless(document)
    }

    /// Encodes a validated document into style-aware markdown.
    ///
    /// Queries the provided [`StyleLookup`] for character/paragraph/style
    /// properties to emit inline formatting (bold, italic, strikeout),
    /// heading markers, and extracted images.
    pub fn encode_styled(document: &Document<Validated>, styles: &dyn StyleLookup) -> MdOutput {
        styled::encode_styled(document, styles)
    }
}
