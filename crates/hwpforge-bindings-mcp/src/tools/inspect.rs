//! `hwpforge_inspect` — HWPX document structure inspection tool.

use serde::Serialize;

use hwpforge_smithy_hwpx::HwpxDecoder;

use crate::output::{read_file_bytes, ToolErrorInfo};

/// Summary of a single section.
#[derive(Debug, Serialize)]
pub struct SectionDetail {
    /// Section index (0-based).
    pub index: usize,
    /// Number of paragraphs.
    pub paragraphs: usize,
    /// Number of tables.
    pub tables: usize,
    /// Number of images.
    pub images: usize,
    /// Number of charts.
    pub charts: usize,
    /// Whether header is present.
    pub has_header: bool,
    /// Whether footer is present.
    pub has_footer: bool,
    /// Whether page number is present.
    pub has_page_number: bool,
}

/// Document metadata summary.
#[derive(Debug, Serialize)]
pub struct MetadataInfo {
    /// Document title (empty string if not set).
    pub title: String,
    /// Document author (empty string if not set).
    pub author: String,
    /// Document subject (empty string if not set).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub subject: String,
    /// Creation date in ISO 8601 format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    /// Last modification date in ISO 8601 format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    /// Searchable keywords.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
}

/// Output data from a successful inspection.
#[derive(Debug, Serialize)]
pub struct InspectData {
    /// Document metadata (title, author, dates, etc.).
    pub metadata: MetadataInfo,
    /// Total number of sections.
    pub sections: usize,
    /// Total number of paragraphs across all sections.
    pub total_paragraphs: usize,
    /// Total number of tables.
    pub total_tables: usize,
    /// Total number of images.
    pub total_images: usize,
    /// Total number of charts.
    pub total_charts: usize,
    /// Per-section detail.
    pub section_details: Vec<SectionDetail>,
}

/// Inspect an HWPX file and return structural summary.
pub fn run_inspect(file_path: &str, _show_styles: bool) -> Result<InspectData, ToolErrorInfo> {
    let bytes = read_file_bytes(file_path)?;

    let hwpx_doc = HwpxDecoder::decode(&bytes).map_err(|e| {
        ToolErrorInfo::new(
            "DECODE_ERROR",
            format!("HWPX decode failed: {e}"),
            "Check that the file is a valid HWPX document.",
        )
    })?;

    let doc = &hwpx_doc.document;
    let meta = doc.metadata();
    let metadata = MetadataInfo {
        title: meta.title.clone().unwrap_or_default(),
        author: meta.author.clone().unwrap_or_default(),
        subject: meta.subject.clone().unwrap_or_default(),
        created: meta.created.clone(),
        modified: meta.modified.clone(),
        keywords: meta.keywords.clone(),
    };

    let mut total_tables: usize = 0;
    let mut total_images: usize = 0;
    let mut total_charts: usize = 0;
    let mut total_paragraphs: usize = 0;

    let section_details: Vec<SectionDetail> = doc
        .sections()
        .iter()
        .enumerate()
        .map(|(i, sec)| {
            let counts = sec.content_counts();

            total_tables += counts.tables;
            total_images += counts.images;
            total_charts += counts.charts;
            total_paragraphs += sec.paragraphs.len();

            SectionDetail {
                index: i,
                paragraphs: sec.paragraphs.len(),
                tables: counts.tables,
                images: counts.images,
                charts: counts.charts,
                has_header: sec.header.is_some(),
                has_footer: sec.footer.is_some(),
                has_page_number: sec.page_number.is_some(),
            }
        })
        .collect();

    Ok(InspectData {
        metadata,
        sections: section_details.len(),
        total_paragraphs,
        total_tables,
        total_images,
        total_charts,
        section_details,
    })
}
