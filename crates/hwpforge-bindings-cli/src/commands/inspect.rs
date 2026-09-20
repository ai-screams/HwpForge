//! Inspect HWPX document structure.
//!
//! This command is intentionally HWPX-only. HWP5 sources should flow through
//! `convert-hwp5` or `audit-hwp5` first.

use std::path::PathBuf;

use serde::Serialize;

use hwpforge::ops::inspect::{InspectSection, InspectStyles};
use hwpforge::ops::{self, InspectOptions, OpsWarning};
use hwpforge_foundation::diagnostics::WarningInfo;
use hwpforge_smithy_hwpx::PackageReader;

use crate::analysis::hwpx_paths::collect_section_path_inventory;
use crate::compat::{self, Command};
use crate::error::{check_file_size, read_input, CliError};

#[derive(Serialize)]
struct InspectResult {
    status: &'static str,
    metadata: MetadataInfo,
    sections: Vec<SectionInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    styles: Option<InspectStyles>,
    /// Decode warnings raised while decoding (`ops::InspectOutput::warnings`,
    /// for example `LAYOUT_CACHE_DROPPED`). Omitted when empty; new key, so a
    /// clean document's `--json` shape is unchanged (W5 follow-up — see the
    /// `run` module comment).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<WarningInfo>,
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
///
/// # Single-decode contract (W6b audit follow-up)
///
/// Most fields below now come from the one `ops::inspect` decode: the
/// legacy-scope fields `ops::InspectSection` gained (`tables_all`/
/// `images_all`/`text_boxes`/`lines`/`rectangles`/`polygons`/
/// `non_empty_paragraphs`/`deep_paragraphs`/`deep_non_empty_paragraphs` —
/// see its rustdoc for the exact scope: legacy raw-XML scope, captions
/// included, master pages excluded) replace what a second, local
/// `HwpxDecoder::decode` plus a full raw-XML element scan used to supply.
///
/// `ole_objects` and `charts` are the two fields that still need bytes read
/// a second time, both for the same reason but at different layers:
///
/// - `ole_objects`: Core's HWPX decoder has no representation for
///   `<hp:ole>` at all — no `Control` variant is ever constructed for it
///   (verified by grepping the decoder) — so there is no Core-tree count to
///   read it from.
/// - `charts`: Core *can* represent a chart (`Control::Chart`), but
///   `hwpforge-smithy-hwpx`'s decoder only reconstructs one when it is a
///   section's own top-level paragraph content — chart is not one of the
///   `HxRunChildKind` variants the recursive per-container dispatch
///   handles, so a chart nested in a caption, a table cell, a text box or
///   anywhere else `ops::walk::object_counts` also reaches is silently
///   invisible on decode (see that module's doc and its
///   `chart_nested_in_a_caption_is_a_documented_decoder_gap` test for the
///   reproduction — no fixture under `tests/fixtures/**` exercises this
///   today, but a decode-based count would silently misreport the first
///   real one).
///
/// Both stay a raw scan, but a much lighter one than before:
/// `collect_section_path_inventory` reads `section-N.xml` text and matches
/// element names — no `HwpxDecoder::decode`, no document tree.
pub fn run(file: &PathBuf, show_styles: bool, json_mode: bool) {
    check_file_size(file, json_mode);
    let bytes = read_input(file, json_mode);

    let out = match ops::inspect(&bytes, &InspectOptions::default().with_styles(show_styles)) {
        Ok(o) => o,
        Err(e) => {
            let err = compat::cli_error(Command::Inspect, e);
            let exit = compat::exit_code(Command::Inspect, &err);
            err.exit(json_mode, exit);
        }
    };
    // W5 follow-up: the pre-migration CLI never captured decode warnings
    // here (there was no decode step to capture them from); now that
    // `ops::inspect` reports them (`LAYOUT_CACHE_DROPPED` etc., mirroring
    // `hwpforge_inspect`/`doc.inspect()` on the MCP/Python surfaces), they
    // are additive: a new, omit-if-empty `warnings` key in `--json`, and one
    // `[inspect]`-prefixed stderr line each in text mode.
    let warnings: Vec<WarningInfo> = out.warnings.iter().map(OpsWarning::info).collect();
    let report = out.report;

    let raw_scan = match raw_scan_counts_per_section(&bytes, report.section_details.len()) {
        Ok(counts) => counts,
        Err(err) => {
            CliError::new("ANALYSIS_FAILED", format!("Cannot analyze '{}': {err}", file.display()))
                .exit(json_mode, 2);
        }
    };

    let sections: Vec<SectionInfo> = report
        .section_details
        .into_iter()
        .zip(raw_scan)
        .map(|(section, raw)| SectionInfo::from_ops(section, raw))
        .collect();

    let result = InspectResult {
        status: "ok",
        metadata: MetadataInfo { title: report.metadata.title, author: report.metadata.author },
        sections,
        styles: report.styles,
        warnings,
    };

    if json_mode {
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        for w in &result.warnings {
            eprintln!("[inspect] {}: {}", w.code, w.message);
        }
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

/// The two fields [`run`]'s doc explains still need a raw scan.
#[derive(Debug, Clone, Copy, Default)]
struct RawScanCounts {
    charts: usize,
    ole_objects: usize,
}

/// Counts `<hp:chart>` and `<hp:ole>` elements per section — see [`run`]'s
/// doc for why these two, and only these two, stay a raw scan rather than a
/// Core-tree count. `collect_section_path_inventory` reads section XML text
/// and matches element names by local name; it does not build a document
/// tree, so this is not the "second decode" the W6b finding measured
/// (135.7 ms), just a comparatively cheap text scan.
fn raw_scan_counts_per_section(
    bytes: &[u8],
    section_count: usize,
) -> hwpforge_smithy_hwpx::HwpxResult<Vec<RawScanCounts>> {
    let mut package_reader = PackageReader::new(bytes)?;
    let occurrences = collect_section_path_inventory(&mut package_reader)?;
    let mut counts = vec![RawScanCounts::default(); section_count];
    for occurrence in &occurrences {
        let Some(slot) = counts.get_mut(occurrence.section_index) else { continue };
        match occurrence.kind.as_str() {
            "chart" => slot.charts += 1,
            "ole" => slot.ole_objects += 1,
            _ => {}
        }
    }
    Ok(counts)
}

impl SectionInfo {
    /// Builds one section's CLI-facing counts from `ops::inspect`'s single
    /// decode (`section`) plus the two raw-scanned fields this command
    /// still needs (`charts`/`ole_objects` — see [`run`]'s doc).
    ///
    /// `tables`/`images`/`text_boxes`/`lines`/`rectangles`/`polygons`/
    /// `non_empty_paragraphs`/`deep_paragraphs`/`deep_non_empty_paragraphs`
    /// in this CLI's `--json` output read from `ops::InspectSection`'s
    /// `*_all`/legacy-named fields (added by the W6b audit follow-up), not
    /// from `ops::InspectSection`'s same-named-but-differently-scoped
    /// `tables`/`images`/`paragraphs` fields — those stay caption-blind and
    /// master-page-inclusive for MCP/Python, a third, incompatible scope
    /// (see `hwpforge::ops::inspect`'s rustdoc).
    fn from_ops(section: InspectSection, raw: RawScanCounts) -> Self {
        Self {
            index: section.index,
            paragraphs: section.top_level_paragraphs,
            deep_paragraphs: section.deep_paragraphs,
            non_empty_paragraphs: section.non_empty_paragraphs,
            deep_non_empty_paragraphs: section.deep_non_empty_paragraphs,
            tables: section.tables_all,
            images: section.images_all,
            charts: raw.charts,
            ole_objects: raw.ole_objects,
            text_boxes: section.text_boxes,
            lines: section.lines,
            rectangles: section.rectangles,
            polygons: section.polygons,
            has_header: section.has_header,
            has_footer: section.has_footer,
            has_page_number: section.has_page_number,
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
