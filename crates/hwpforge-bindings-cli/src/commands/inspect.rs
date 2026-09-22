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
/// # Why the object counts are a raw XML scan, not the decode
///
/// This command opens the package twice: one `ops::inspect` decode
/// (metadata, top-level counts, paragraph counts, styles) plus one
/// `raw_scan_counts_per_section` text scan that supplies *every* object
/// count — `tables`/`images`/`charts`/`ole_objects`/`text_boxes`/`lines`/
/// `rectangles`/`polygons`. `ops::InspectSection`'s `all_*` fields have the
/// same scope but are a decoded-object count, and matching scope is not
/// matching values: an element the decoder accepts but cannot represent —
/// an `<hp:pic>` with no usable `binaryItemIDRef`, which `convert_picture`
/// (`hwpforge-smithy-hwpx/src/decoder/section.rs`) turns into `Ok(None)`
/// rather than an error — is in the section XML but never becomes a
/// `Control`/`Image`. Reading the decode here would make this command
/// report fewer objects than its `--json` contract has always reported, on
/// exactly those documents. The raw scan keeps that contract by
/// construction rather than by fixture luck; see
/// `crafted_pic_with_no_binary_ref_is_a_raw_scan_vs_decode_divergence` in
/// `cli_integration.rs` for the reproduction. Two of the eight have their
/// own reasons on top: Core has no HWPX representation for `<hp:ole>` at
/// all, and it cannot reconstruct a chart nested below a section's own
/// top-level paragraphs (`ops::InspectSection`'s scope table).
///
/// The paragraph counts do come from the decode, because paragraph presence
/// is never silently dropped that way — a decoded count and a raw count of
/// the same bytes are deterministically identical there.
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

/// Every object count [`run`]'s doc explains stays a raw scan, per section.
#[derive(Debug, Clone, Copy, Default)]
struct RawScanCounts {
    tables: usize,
    images: usize,
    charts: usize,
    ole_objects: usize,
    text_boxes: usize,
    lines: usize,
    rectangles: usize,
    polygons: usize,
}

/// Counts every legacy object-count element per section — see [`run`]'s doc
/// for why all eight of these stay a raw scan rather than a Core-tree count.
/// `collect_section_path_inventory` reads section XML text and matches
/// element names by local name; it does not build a document tree, so this
/// is not the "second decode" the W6b finding measured (135.7 ms), just a
/// comparatively cheap text scan.
///
/// Ported unchanged from the pre-migration scanner
/// (`analysis/deep_counts.rs::summarize_hwpx_section`, still there for
/// `audit-hwp5`/`census-hwp5`): `tables`/`charts`/`ole_objects`/`lines`/
/// `polygons` are plain element-name tallies (`tbl`/`chart`/`ole`/`line`/
/// `polygon`); `images` counts `pic` only, never the inner `img` reference
/// element `is_interesting_element` also tracks (matching two names there
/// would double the count); `rectangles` is every `rect` element **minus**
/// the ones that are actually text boxes (`rect.saturating_sub(drawText)`,
/// per section, same as the pre-migration rule — a text-bearing `<hp:rect>`
/// nests a `<hp:drawText>` and must count once, under `text_boxes`, never
/// under both).
fn raw_scan_counts_per_section(
    bytes: &[u8],
    section_count: usize,
) -> hwpforge_smithy_hwpx::HwpxResult<Vec<RawScanCounts>> {
    let mut package_reader = PackageReader::new(bytes)?;
    let occurrences = collect_section_path_inventory(&mut package_reader)?;
    let mut counts = vec![RawScanCounts::default(); section_count];
    let mut raw_rects = vec![0usize; section_count];
    for occurrence in &occurrences {
        let Some(slot) = counts.get_mut(occurrence.section_index) else { continue };
        match occurrence.kind.as_str() {
            "tbl" => slot.tables += 1,
            "pic" => slot.images += 1,
            "chart" => slot.charts += 1,
            "ole" => slot.ole_objects += 1,
            "drawText" => slot.text_boxes += 1,
            "line" => slot.lines += 1,
            "polygon" => slot.polygons += 1,
            "rect" => {
                if let Some(raw_rect) = raw_rects.get_mut(occurrence.section_index) {
                    *raw_rect += 1;
                }
            }
            _ => {}
        }
    }
    for (slot, raw_rect) in counts.iter_mut().zip(raw_rects) {
        slot.rectangles = raw_rect.saturating_sub(slot.text_boxes);
    }
    Ok(counts)
}

impl SectionInfo {
    /// Builds one section's CLI-facing counts from `ops::inspect`'s single
    /// decode (`section`, paragraph counts only) plus the raw scan every
    /// object count now reads from (`raw` — see [`run`]'s doc for why: a
    /// decoded-object count and a raw-XML count of the same bytes can
    /// disagree on a document with an accepted-but-dropped element, and this
    /// CLI's legacy fields must stay byte-identical to the pre-migration
    /// scanner regardless).
    ///
    /// `tables`/`images`/`charts`/`ole_objects`/`text_boxes`/`lines`/
    /// `rectangles`/`polygons` all come from `raw`, never from
    /// `ops::InspectSection`'s same-scoped-but-decoded `all_*` fields (see
    /// that struct's scope table). The three paragraph counts still come
    /// from `section` — paragraph presence has no such gap.
    ///
    /// The CLI's own key spellings are older than the ops ones and stay
    /// as they are (`--json` is byte-frozen), so two of the three paragraph
    /// keys are deliberately spelled differently from the ops field they
    /// read: `non_empty_paragraphs` here is `top_level_non_empty_paragraphs`
    /// there, and `paragraphs` here is `top_level_paragraphs` there. Neither
    /// is `ops::InspectSection::paragraphs`, which is a third scope again
    /// (caption-blind, master-page-inclusive) that this command never emits.
    fn from_ops(section: InspectSection, raw: RawScanCounts) -> Self {
        Self {
            index: section.index,
            paragraphs: section.top_level_paragraphs,
            deep_paragraphs: section.deep_paragraphs,
            non_empty_paragraphs: section.top_level_non_empty_paragraphs,
            deep_non_empty_paragraphs: section.deep_non_empty_paragraphs,
            tables: raw.tables,
            images: raw.images,
            charts: raw.charts,
            ole_objects: raw.ole_objects,
            text_boxes: raw.text_boxes,
            lines: raw.lines,
            rectangles: raw.rectangles,
            polygons: raw.polygons,
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
