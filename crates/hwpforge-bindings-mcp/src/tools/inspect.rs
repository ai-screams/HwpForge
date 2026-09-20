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

    // ── package-scope + deep counts (W6c audit follow-up, additive) ────
    //
    // The eight fields above are the legacy top-level-only contract this
    // struct always had. The nine below carry `ops::InspectSection`'s
    // package-scope and deep counts through unchanged — same name, same
    // value, same scope as documented on `ops::InspectSection` itself
    // (`hwpforge::ops::inspect` module) — closing the gap where the Python
    // bindings already exposed these nine (`InspectSection` in
    // `hwpforge-bindings-py`'s `.pyi`) but MCP silently dropped them. Added
    // at the end, after the eight legacy fields, so a client destructuring
    // this struct's serialized fields positionally is unaffected.
    /// Tables, captions included, master pages excluded — a decoded-object
    /// count, not a raw scan. See `ops::InspectSection::tables_all`'s doc for
    /// the exact scope and the caveat against the CLI's raw-scan `tables`
    /// value on the same document.
    pub tables_all: usize,
    /// Images, same scope and caveat as [`Self::tables_all`]. See
    /// `ops::InspectSection::images_all`.
    pub images_all: usize,
    /// Text boxes (HWPX `<hp:rect>` with a nested `<hp:drawText>`), same
    /// scope and caveat as [`Self::tables_all`]. See
    /// `ops::InspectSection::text_boxes`.
    pub text_boxes: usize,
    /// Line drawing objects, same scope and caveat as [`Self::tables_all`].
    /// See `ops::InspectSection::lines`.
    pub lines: usize,
    /// Pure rectangles — a text-bearing one counts under
    /// [`Self::text_boxes`] instead, never both. See
    /// `ops::InspectSection::rectangles`.
    pub rectangles: usize,
    /// Polygon drawing objects, same scope and caveat as
    /// [`Self::tables_all`]. See `ops::InspectSection::polygons`.
    pub polygons: usize,
    /// Body-flow paragraphs with visible text ([`Self::paragraphs`]'s scope,
    /// each paragraph's visibility probed one recursion step deeper). See
    /// `ops::InspectSection::non_empty_paragraphs`.
    pub non_empty_paragraphs: usize,
    /// Body + header + footer paragraphs, recursing into table cells and
    /// text boxes/notes/shapes/memos — not captions, group children or
    /// master pages. See `ops::InspectSection::deep_paragraphs`.
    pub deep_paragraphs: usize,
    /// Same recursion as [`Self::deep_paragraphs`], counting only paragraphs
    /// with visible text. See `ops::InspectSection::deep_non_empty_paragraphs`.
    pub deep_non_empty_paragraphs: usize,
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
                tables_all: s.tables_all,
                images_all: s.images_all,
                text_boxes: s.text_boxes,
                lines: s.lines,
                rectangles: s.rectangles,
                polygons: s.polygons,
                non_empty_paragraphs: s.non_empty_paragraphs,
                deep_paragraphs: s.deep_paragraphs,
                deep_non_empty_paragraphs: s.deep_non_empty_paragraphs,
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

    /// W6c audit follow-up (C6): `SectionDetail` used to stop at the eight
    /// legacy fields and drop `ops::InspectSection`'s nine package-scope/deep
    /// counts on the floor — Python exposed them, MCP did not. This checks
    /// the nine new fields against a *direct* `ops::inspect` call on the same
    /// bytes rather than hardcoded numbers, so the assertion tracks whatever
    /// the decoder actually reports instead of a value copied out of a
    /// one-off run.
    #[test]
    fn inspect_section_detail_carries_ops_package_scope_and_deep_counts() {
        let path = fixture("mixed/mixed_01_image_and_chart_same_doc.hwpx");
        let bytes = std::fs::read(&path).unwrap();
        let ops_out = ops::inspect(&bytes, &InspectOptions::default()).unwrap();
        let ops_section =
            ops_out.report.section_details.first().expect("fixture has at least one section");

        let data = run_inspect(&path, false).unwrap();
        let detail = data.section_details.first().expect("mcp inspect must report the section too");

        // Existing eight fields: unchanged mapping (top-level scope).
        assert_eq!(detail.index, ops_section.index);
        assert_eq!(detail.paragraphs, ops_section.top_level_paragraphs);
        assert_eq!(detail.tables, ops_section.top_level_tables);
        assert_eq!(detail.images, ops_section.top_level_images);
        assert_eq!(detail.charts, ops_section.top_level_charts);
        assert_eq!(detail.has_header, ops_section.has_header);
        assert_eq!(detail.has_footer, ops_section.has_footer);
        assert_eq!(detail.has_page_number, ops_section.has_page_number);

        // New nine fields: same name, same value, straight from ops.
        assert_eq!(detail.tables_all, ops_section.tables_all);
        assert_eq!(detail.images_all, ops_section.images_all);
        assert_eq!(detail.text_boxes, ops_section.text_boxes);
        assert_eq!(detail.lines, ops_section.lines);
        assert_eq!(detail.rectangles, ops_section.rectangles);
        assert_eq!(detail.polygons, ops_section.polygons);
        assert_eq!(detail.non_empty_paragraphs, ops_section.non_empty_paragraphs);
        assert_eq!(detail.deep_paragraphs, ops_section.deep_paragraphs);
        assert_eq!(detail.deep_non_empty_paragraphs, ops_section.deep_non_empty_paragraphs);

        // At least one of the newly-exposed counts must be non-zero on this
        // fixture, or the equality assertions above would pass vacuously
        // (both sides zero) without ever exercising a real decoded count.
        assert!(
            ops_section.images_all > 0 || ops_section.deep_paragraphs > 0,
            "fixture must exercise at least one non-trivial package-scope/deep count: {ops_section:?}"
        );
    }

    /// W6c audit follow-up (C6): the nine new fields must land *after* the
    /// eight legacy ones — additive, not reordered — because a client that
    /// destructures `section_details[i]` positionally (a naive JSON-schema
    /// consumer, a Python `TypedDict` that iterates `.items()`) would
    /// otherwise silently pick up the wrong value for an old field.
    #[test]
    fn inspect_section_detail_key_order_is_existing_eight_then_new_nine() {
        let path = fixture("mixed/mixed_01_image_and_chart_same_doc.hwpx");
        let data = run_inspect(&path, false).unwrap();
        let detail = data.section_details.first().expect("fixture has at least one section");

        let json = serde_json::to_string(detail).unwrap();
        let expected_order = [
            "index",
            "paragraphs",
            "tables",
            "images",
            "charts",
            "has_header",
            "has_footer",
            "has_page_number",
            "tables_all",
            "images_all",
            "text_boxes",
            "lines",
            "rectangles",
            "polygons",
            "non_empty_paragraphs",
            "deep_paragraphs",
            "deep_non_empty_paragraphs",
        ];

        let mut last_pos = 0;
        for key in expected_order {
            let needle = format!("\"{key}\":");
            let pos = json
                .find(&needle)
                .unwrap_or_else(|| panic!("key {key} missing from serialized section: {json}"));
            assert!(pos >= last_pos, "key {key} out of order in serialized section: {json}");
            last_pos = pos;
        }

        // Exactly this key set — no stray extras, nothing missing — checked
        // independently of order (serde_json's `Value::Object` may not
        // preserve insertion order without the `preserve_order` feature, so
        // this only asserts the *set*; the loop above already asserts order
        // straight from the serialized struct).
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let keys: std::collections::BTreeSet<&str> =
            value.as_object().unwrap().keys().map(String::as_str).collect();
        let expected_set: std::collections::BTreeSet<&str> = expected_order.into_iter().collect();
        assert_eq!(keys, expected_set);
    }
}
