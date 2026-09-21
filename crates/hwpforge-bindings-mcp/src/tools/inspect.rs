//! `hwpforge_inspect` — HWPX document structure inspection tool.

use serde::Serialize;

use hwpforge::ops::{self, InspectOptions};

use crate::compat::{self, Tool};
use crate::output::{read_file_bytes, ToolErrorInfo, ToolWarningInfo};

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
    /// Decoder warnings raised while decoding (`ops::InspectOutput::warnings`).
    /// Omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ToolWarningInfo>,
}

/// Inspect an HWPX file and return structural summary.
///
/// `_show_styles` stays unused: `InspectData` has no `styles` field (schema
/// freeze — adding one is a W4 decision), matching the pre-migration tool,
/// which also decoded without ever building a style summary.
///
/// The legacy contract counts `tables`/`images`/`charts` (and `paragraphs`)
/// **top-level only** — `Section::content_counts()`/`paragraphs.len()`, not
/// descending into table cells, headers/footers, notes, memos or master
/// pages. `ops::inspect`'s per-section deep counts
/// (`InspectSection::{tables,images,charts,paragraphs}`) do that deeper
/// traversal, so this maps from the shallow counterparts
/// (`InspectSection::top_level_*`) instead — values are byte-identical to
/// what this file computed by hand before. Decoder warnings
/// (`ops::InspectOutput::warnings`) are surfaced through
/// `InspectData::warnings`.
pub fn run_inspect(file_path: &str, _show_styles: bool) -> Result<InspectData, ToolErrorInfo> {
    let bytes = read_file_bytes(file_path)?;

    let out = ops::inspect(&bytes, &InspectOptions::default())
        .map_err(|e| compat::tool_error(Tool::Inspect, e))?;
    let warnings: Vec<ToolWarningInfo> = out.warnings.iter().map(compat::warning).collect();
    let report = out.report;

    let metadata = MetadataInfo {
        title: report.metadata.title,
        author: report.metadata.author,
        subject: report.metadata.subject,
        created: report.metadata.created,
        modified: report.metadata.modified,
        keywords: report.metadata.keywords,
    };

    let mut total_tables: usize = 0;
    let mut total_images: usize = 0;
    let mut total_charts: usize = 0;
    let mut total_paragraphs: usize = 0;

    let section_details: Vec<SectionDetail> = report
        .section_details
        .into_iter()
        .map(|s| {
            total_tables += s.top_level_tables;
            total_images += s.top_level_images;
            total_charts += s.top_level_charts;
            total_paragraphs += s.top_level_paragraphs;

            SectionDetail {
                index: s.index,
                paragraphs: s.top_level_paragraphs,
                tables: s.top_level_tables,
                images: s.top_level_images,
                charts: s.top_level_charts,
                has_header: s.has_header,
                has_footer: s.has_footer,
                has_page_number: s.has_page_number,
            }
        })
        .collect();

    Ok(InspectData {
        metadata,
        sections: report.sections,
        total_paragraphs,
        total_tables,
        total_images,
        total_charts,
        section_details,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(rel: &str) -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(rel)
            .to_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn inspect_via_mcp_surface_has_no_warnings_on_a_clean_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe.hwpx");
        crate::tools::convert::run_convert(
            "# 제목\n\n본문 문단입니다.",
            false,
            path.to_str().unwrap(),
            "default",
        )
        .unwrap();

        let data = run_inspect(path.to_str().unwrap(), false).unwrap();
        assert_eq!(data.sections, 1);
        assert!(data.warnings.is_empty(), "a clean document must not warn: {:?}", data.warnings);
    }

    /// 줄 조판 캐시가 낡은 fixture 를 inspect 하면, 디코드 경고
    /// (`LAYOUT_CACHE_DROPPED`)가 `warnings` 에 실려야 한다.
    #[test]
    fn inspect_surfaces_decode_warnings() {
        let path = fixture("layout/stale-line-cache.hwpx");
        let data = run_inspect(&path, false).unwrap();
        assert!(
            data.warnings.iter().any(|w| w.code == "LAYOUT_CACHE_DROPPED"),
            "inspect must surface the decode warning: {:?}",
            data.warnings
        );

        let value = serde_json::to_value(&data).unwrap();
        assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED");
        assert!(!value["warnings"][0]["message"].as_str().unwrap_or_default().is_empty());
    }

    #[test]
    fn inspect_missing_file_reports_file_not_found() {
        let err = run_inspect("/nonexistent/inspect-warnings-probe.hwpx", false).unwrap_err();
        assert_eq!(err.code, "FILE_NOT_FOUND");
    }
}
