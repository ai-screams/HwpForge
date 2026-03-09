//! `hwpforge_inspect` — HWPX document structure inspection tool.

use std::path::Path;

use serde::Serialize;

use hwpforge_core::control::Control;
use hwpforge_core::RunContent;
use hwpforge_smithy_hwpx::HwpxDecoder;

use crate::output::{check_file_size, ToolErrorInfo};

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

/// Output data from a successful inspection.
#[derive(Debug, Serialize)]
pub struct InspectData {
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
    let path = Path::new(file_path);
    if !path.exists() {
        return Err(ToolErrorInfo::new(
            "FILE_NOT_FOUND",
            format!("HWPX file not found: {file_path}"),
            "Check the file path and try again.",
        ));
    }

    check_file_size(path)?;
    let bytes = std::fs::read(path).map_err(|e| {
        ToolErrorInfo::new(
            "READ_ERROR",
            format!("Failed to read file: {e}"),
            "Check file permissions.",
        )
    })?;

    let hwpx_doc = HwpxDecoder::decode(&bytes).map_err(|e| {
        ToolErrorInfo::new(
            "DECODE_ERROR",
            format!("HWPX decode failed: {e}"),
            "Check that the file is a valid HWPX document.",
        )
    })?;

    let doc = &hwpx_doc.document;
    let mut total_tables: usize = 0;
    let mut total_images: usize = 0;
    let mut total_charts: usize = 0;
    let mut total_paragraphs: usize = 0;

    let section_details: Vec<SectionDetail> = doc
        .sections()
        .iter()
        .enumerate()
        .map(|(i, sec)| {
            let mut tables = 0usize;
            let mut images = 0usize;
            let mut charts = 0usize;

            for para in &sec.paragraphs {
                for run in &para.runs {
                    match &run.content {
                        RunContent::Table(_) => tables += 1,
                        RunContent::Image(_) => images += 1,
                        RunContent::Control(c) => {
                            if matches!(**c, Control::Chart { .. }) {
                                charts += 1;
                            }
                        }
                        _ => {}
                    }
                }
            }

            total_tables += tables;
            total_images += images;
            total_charts += charts;
            total_paragraphs += sec.paragraphs.len();

            SectionDetail {
                index: i,
                paragraphs: sec.paragraphs.len(),
                tables,
                images,
                charts,
                has_header: sec.header.is_some(),
                has_footer: sec.footer.is_some(),
                has_page_number: sec.page_number.is_some(),
            }
        })
        .collect();

    Ok(InspectData {
        sections: section_details.len(),
        total_paragraphs,
        total_tables,
        total_images,
        total_charts,
        section_details,
    })
}
