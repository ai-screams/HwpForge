//! Inspect HWPX document structure.
//!
//! This command is intentionally HWPX-only. HWP5 sources should flow through
//! `convert-hwp5` or `audit-hwp5` first.

use std::path::PathBuf;

use serde::Serialize;

use hwpforge::ops::inspect::{InspectSection, InspectStyles};
use hwpforge::ops::{self, InspectOptions, OpsError};
use hwpforge_smithy_hwpx::HwpxDecoder;

use crate::analysis::deep_counts::{summarize_hwpx_document, DeepSectionSummary};
use crate::compat::{self, Command};
use crate::error::{check_file_size, CliError};

#[derive(Serialize)]
struct InspectResult {
    status: &'static str,
    metadata: MetadataInfo,
    sections: Vec<SectionInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    styles: Option<InspectStyles>,
}

#[derive(Serialize)]
struct MetadataInfo {
    title: String,
    author: String,
}

#[derive(Serialize)]
struct SectionInfo {
    index: usize,
    paragraphs: usize,
    deep_paragraphs: usize,
    non_empty_paragraphs: usize,
    deep_non_empty_paragraphs: usize,
    tables: usize,
    images: usize,
    charts: usize,
    ole_objects: usize,
    text_boxes: usize,
    lines: usize,
    rectangles: usize,
    polygons: usize,
    has_header: bool,
    has_footer: bool,
    has_page_number: bool,
}

/// Run the inspect command.
pub fn run(file: &PathBuf, show_styles: bool, json_mode: bool) {
    check_file_size(file, json_mode);
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", file.display()))
                .exit(json_mode, 1);
        }
    };

    // `ops::inspect` is the canonical report: metadata (title/author) and
    // (with `--styles`) the style summary come from here, plus the
    // top-level paragraph count and the header/footer/page-number flags,
    // which the two decoders compute identically (see `SectionInfo::merge`
    // doc comment).
    //
    // The `tables`/`images`/`charts`/`deep_paragraphs` fields do NOT come
    // from `ops::inspect`, even though it has same-named deep counters.
    // `ops::inspect`'s shared paragraph traversal (`Section::for_each_paragraph`)
    // deliberately does not descend into **image** captions (Core's
    // `image_caption_paragraphs_are_skipped_documents_known_gap` test) — a
    // table, image or chart nested inside an image's caption is invisible
    // to it. The pre-migration CLI's raw-XML `count_occurrences` scan (still
    // run below via `summarize_hwpx_document`, kept for the fields
    // `ops::InspectReport` has no equivalent for) has no such blind spot —
    // it counts every `<hp:tbl>`/`<hp:pic>`/`<hp:chart>` element in the
    // section regardless of nesting. So these four fields keep reading from
    // the local scanner (`deep`), matching the pre-migration byte-for-byte;
    // see `inspect_deep_counts_table_image_chart_nested_in_image_caption_and_master_page`
    // for
    // the regression lock. `top_level_paragraphs`/`tables`/`images`/`charts`
    // (undercounts — see `img_05_image_in_table_cell.hwpx`'s
    // `inspect_deep_counts_image_in_table_cell` test) stay unused for the
    // same reason the deep ones aren't sourced from `ops` either.
    //
    // `ops::InspectReport` has no field for `text_boxes`/`ole_objects`/
    // `lines`/`rectangles`/`polygons`/`non_empty_paragraphs` (ops gap, W3
    // remediation report) — the local `summarize_hwpx_document` scanner
    // still supplies those, decoding the document a second time.
    let out = match ops::inspect(&bytes, &InspectOptions::default().with_styles(show_styles)) {
        Ok(o) => o,
        Err(e) => {
            let err = compat::cli_error(Command::Inspect, e);
            let exit = compat::exit_code(Command::Inspect, &err);
            err.exit(json_mode, exit);
        }
    };
    let report = out.report;

    // Second decode, only for the deep-count scanner (see module comment
    // above). `ops::inspect` already proved these bytes decode; this branch
    // is therefore not expected to trigger in practice, but stays
    // symmetrical with the shared `compat::cli_error` path rather than
    // `.expect`-ing determinism.
    let hwpx_doc = match HwpxDecoder::decode(&bytes) {
        Ok(d) => d,
        Err(e) => {
            let err = compat::cli_error(Command::Inspect, OpsError::decode(e));
            let exit = compat::exit_code(Command::Inspect, &err);
            err.exit(json_mode, exit);
        }
    };
    let deep_summary = match summarize_hwpx_document(&bytes, &hwpx_doc) {
        Ok(summary) => summary,
        Err(err) => {
            CliError::new("ANALYSIS_FAILED", format!("Cannot analyze '{}': {err}", file.display()))
                .exit(json_mode, 2);
        }
    };

    let sections: Vec<SectionInfo> = report
        .section_details
        .iter()
        .zip(deep_summary.sections.iter())
        .map(|(ops_section, deep)| SectionInfo::merge(ops_section, deep))
        .collect();

    let result = InspectResult {
        status: "ok",
        metadata: MetadataInfo { title: report.metadata.title, author: report.metadata.author },
        sections,
        styles: report.styles,
    };

    if json_mode {
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!("Document: {}", file.display());
        println!("  Title:  {}", result.metadata.title);
        println!("  Author: {}", result.metadata.author);
        println!("  Sections: {}", result.sections.len());
        for sec in &result.sections {
            let extras: String = render_extra_control_counts(sec);
            println!(
                "    [{}] {} paras (deep {}), {} tables, {} images, {} charts{} | header={} footer={} pagenum={}",
                sec.index,
                sec.paragraphs,
                sec.deep_paragraphs,
                sec.tables,
                sec.images,
                sec.charts,
                extras,
                sec.has_header,
                sec.has_footer,
                sec.has_page_number
            );
        }
        if let Some(styles) = &result.styles {
            println!("  Fonts: {} unique", styles.fonts.len());
            println!("  CharShapes: {}", styles.char_shapes.len());
            println!("  ParaShapes: {}", styles.para_shapes.len());
        }
    }
}

impl SectionInfo {
    /// Combines the canonical `ops::inspect` per-section counts with the
    /// CLI-only deep counts (`ops` gap — module comment on [`run`]) the
    /// local scanner still computes.
    ///
    /// `paragraphs` and the three `has_*` flags read from `ops_section`
    /// because both decoders compute them identically: `top_level_paragraphs`
    /// is `section.paragraphs.len()`, exactly what `deep.paragraphs` is too
    /// (`summarize_hwpx_section` in `analysis/deep_counts.rs`), and
    /// `has_header`/`has_footer`/`has_page_number` are the same
    /// `!section.headers.is_empty()`/etc. check in both places.
    ///
    /// `deep_paragraphs` reads from `deep`, NOT from `ops_section.paragraphs`
    /// (also a deep count, confusingly under the same field name) — the two
    /// are not interchangeable. `ops_section.paragraphs` comes from
    /// `Section::for_each_paragraph`, whose recursion visits master-page
    /// paragraphs (`crates/hwpforge-core/src/section.rs`'s
    /// `walk_paragraphs`/`walk_paragraphs_mut`, and the
    /// `document_with_all_containers` test fixture that locks it). The
    /// legacy local scanner's `deep.deep_paragraphs`
    /// (`count_paragraphs_recursive` over `section.paragraphs` plus headers
    /// and footers only, `analysis/deep_counts.rs`) never visits master
    /// pages — no `master_page`/`masterPage` reference exists in that file.
    /// Any section with paragraphs in a master page would see the two
    /// diverge, so `deep_paragraphs` keeps its pre-migration source.
    fn merge(ops_section: &InspectSection, deep: &DeepSectionSummary) -> Self {
        Self {
            index: ops_section.index,
            paragraphs: ops_section.top_level_paragraphs,
            deep_paragraphs: deep.deep_paragraphs,
            non_empty_paragraphs: deep.non_empty_paragraphs,
            deep_non_empty_paragraphs: deep.deep_non_empty_paragraphs,
            tables: deep.tables,
            images: deep.images,
            charts: deep.charts,
            ole_objects: deep.ole_objects,
            text_boxes: deep.text_boxes,
            lines: deep.lines,
            rectangles: deep.rectangles,
            polygons: deep.polygons,
            has_header: ops_section.has_header,
            has_footer: ops_section.has_footer,
            has_page_number: ops_section.has_page_number,
        }
    }
}

fn render_extra_control_counts(section: &SectionInfo) -> String {
    let mut parts: Vec<String> = Vec::new();
    if section.text_boxes > 0 {
        parts.push(format!(" textboxes={}", section.text_boxes));
    }
    if section.ole_objects > 0 {
        parts.push(format!(" ole={}", section.ole_objects));
    }
    if section.lines > 0 {
        parts.push(format!(" lines={}", section.lines));
    }
    if section.rectangles > 0 {
        parts.push(format!(" rects={}", section.rectangles));
    }
    if section.polygons > 0 {
        parts.push(format!(" polygons={}", section.polygons));
    }
    parts.concat()
}
