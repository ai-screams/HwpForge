//! Encodes a Core [`Section`] into HWPX section XML.
//!
//! This is the reverse of [`crate::decoder::section`]: it converts Core types
//! (`Section`, `Paragraph`, `Run`, `Table`, `Image`) into schema types
//! (`HxSection`, `HxParagraph`, `HxRun`, etc.), serializes them with
//! `quick_xml::se::to_string`, and wraps the result in an xmlns-qualified
//! `<hs:sec>` root element.
//!
//! # SecPr Injection
//!
//! HWPX encodes page settings (`<hp:secPr>`) inside the **first run** of
//! the **first paragraph**, not at the section level. This module reproduces
//! that quirk so the output is compatible with the Hancom HWP editor.
//!
//! # Module Layout (task #92)
//!
//! This file holds the encode pipeline (`encode_section` → `build_section`
//! → `build_paragraph` → `build_runs`), shared utilities
//! (`next_marker`, `generate_instid`, `build_hx_caption`,
//! `encode_paragraphs_to_sublist`), and the root tests module. Per-family
//! run/control builders live in submodules (`mod X;` + `use super::*;` +
//! `pub(super) fn` — re-imported below so call sites and tests stay
//! unchanged):
//!
//! | module          | family                                            |
//! |-----------------|---------------------------------------------------|
//! | `table`         | `<hp:tbl>` grid/cell builders                     |
//! | `field`         | hyperlink / bookmark / ClickHere / SUMMERY / path / autonum / cross-ref |
//! | `section_pr`    | `<hp:secPr>` / `<hp:colPr>` build + enrichment    |
//! | `header_footer` | header/footer, masterpage refs, page-number inject|
//! | `memo`          | memo run/sublist builders                         |
//! | `picture`       | paragraph-inline `<hp:pic>`                       |
//! | `chart`         | run-level embedded-chart `<hp:switch>`            |
//! | `typography`    | dutmal (덧말) / compose (글자겹침)                |
//! | `equation`      | `<hp:equation>`                                   |
//!
//! Adding a new control family = new file + `mod` line + re-import. Keep
//! cross-family code paths routed through this root (no family→family
//! imports) so the dependency direction stays one-way.

mod chart;
mod equation;
mod field;
mod header_footer;
mod memo;
mod picture;
mod section_pr;
mod table;
mod typography;

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::column::{ColumnLayoutMode, ColumnSettings, ColumnType};
use hwpforge_core::control::{Control, DutmalAlign, DutmalPosition};
use hwpforge_core::image::{Image, ImagePlacement};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::table::{
    Table, TableCell, TableMargin, TablePageBreak, TableRow, TableVerticalAlign,
};
use hwpforge_core::PageSettings;

use crate::encoder::package::XMLNS_DECLS;
use crate::error::{HwpxError, HwpxResult};
use crate::inline_text::{
    build_inline_text_element_xml, build_text_element_xml, requires_inline_text_markup,
};
use hwpforge_foundation::{BookmarkType, DropCapStyle, HwpUnit, TextDirection};

use crate::schema::section::{
    HxBookmark, HxCaption, HxCellAddr, HxCellSpan, HxCellSz, HxChart, HxCompose, HxComposeCharPr,
    HxCtrl, HxDutmal, HxEquation, HxFlip, HxFootNote, HxImg, HxImgClip, HxImgDim, HxImgRect,
    HxIndexMark, HxLineSeg, HxLineSegArray, HxMatrix, HxOffset, HxPageMargin, HxPagePr,
    HxParagraph, HxPic, HxPoint, HxRenderingInfo, HxRotationInfo, HxRun, HxRunCase, HxRunSwitch,
    HxScript, HxSecPr, HxSection, HxShapeComment, HxSizeAttr, HxSubList, HxTable, HxTableCell,
    HxTableMargin, HxTablePos, HxTableRow, HxTableSz, HxText, HxTitleMark,
};

use super::EncodeOptions;

use self::chart::{build_embedded_chart_run_xml, encode_chart_switch};
use self::equation::encode_equation_to_hx;
use self::field::{
    build_autonum_run_xml, build_bookmark_span_end_run_xml, build_bookmark_span_start_run_xml,
    build_crossref_run_xml, build_field_run_xml, build_hyperlink_run_xml,
    build_path_field_run_xml_raw, build_summary_run_xml_raw, unix_to_ymdhms,
};
#[cfg(test)]
use self::field::{build_hwp5_crossref_run_xml, clickhere_command_string};
#[cfg(test)]
use self::header_footer::find_ctrl_injection_point;
use self::header_footer::{
    build_masterpage_entries, build_masterpage_refs, inject_header_footer_pagenum,
};
use self::memo::{build_memo_anchor_xml, build_memo_run_xml, encode_memo_sublist};
use self::picture::build_picture;
#[cfg(test)]
use self::section_pr::show_mode_to_hwpx;
use self::section_pr::{build_sec_pr, enrich_sec_pr};
use self::table::build_table;
use self::typography::{encode_compose_to_hx, encode_dutmal_to_hx};
use super::chart::generate_chart_xml;
use super::escape_xml;

/// Shared nonce counter for all marker-based placeholder runs.
///
/// Using a single module-level counter prevents duplicate marker strings
/// even when multiple Control variants are encoded in the same document.
static MARKER_NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Returns a unique marker string for placeholder run injection.
fn next_marker(prefix: &str, field_id: usize) -> String {
    let nonce = MARKER_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("__{prefix}_{nonce}_{field_id}__")
}

/// Maximum nesting depth for tables-within-tables.
///
/// Mirrors the decoder's limit. Prevents stack overflow from deeply nested
/// table structures (e.g. a table cell containing another table, ad infinitum).
const MAX_NESTING_DEPTH: usize = 32;

fn encode_table_page_break(value: TablePageBreak) -> &'static str {
    match value {
        TablePageBreak::Cell => "CELL",
        TablePageBreak::Table => "TABLE",
        TablePageBreak::None => "NONE",
    }
}

fn encode_table_vertical_align(value: TableVerticalAlign) -> &'static str {
    match value {
        TableVerticalAlign::Top => "TOP",
        TableVerticalAlign::Center => "CENTER",
        TableVerticalAlign::Bottom => "BOTTOM",
    }
}

fn encode_table_margin(value: TableMargin) -> HxTableMargin {
    HxTableMargin {
        left: value.left.as_i32(),
        right: value.right.as_i32(),
        top: value.top.as_i32(),
        bottom: value.bottom.as_i32(),
    }
}

/// Result of encoding a section, including chart and masterpage entries for ZIP packaging.
#[derive(Debug)]
pub(crate) struct SectionEncodeResult {
    /// The section XML string.
    pub xml: String,
    /// Chart entries: (ZIP path, OOXML chart XML content).
    pub charts: Vec<(String, String)>,
    /// Master page entries: (ZIP path, masterpage XML content).
    pub master_pages: Vec<(String, String)>,
    /// Embedded-chart OLE blob entries: (item id, raw OLE2 bytes).
    ///
    /// Each entry produces a `BinData/{item_id}.ole` file in the ZIP and a
    /// matching `<opf:item id="{item_id}" href="BinData/{item_id}.ole"
    /// media-type="application/ole" isEmbeded="0"/>` line in `content.hpf`.
    /// Populated by [`Control::EmbeddedChart`] (Wave 4c HWP5 chart carry).
    pub embedded_oles: Vec<(String, Vec<u8>)>,
}

/// Encodes a Core [`Section`] into a complete HWPX section XML string.
///
/// The returned string is a well-formed XML document with `<?xml ...?>`
/// declaration and an `<hs:sec>` root element carrying all required
/// namespace declarations.
///
/// `_section_index` is reserved for future use (e.g. error messages) but
/// currently unused.
///
/// # Errors
///
/// Returns [`HwpxError::XmlSerialize`] if quick-xml serialization fails,
/// or [`HwpxError::InvalidStructure`] if table nesting exceeds the limit.
pub(crate) fn encode_section(
    section: &Section,
    _section_index: usize,
    chart_offset: usize,
    masterpage_offset: usize,
    embedded_ole_offset: usize,
    options: EncodeOptions,
) -> HwpxResult<SectionEncodeResult> {
    let mut chart_entries: Vec<(String, String)> = Vec::new();
    let mut embedded_oles: Vec<(String, Vec<u8>)> = Vec::new();
    // Replacement list for run-level XML fragments that serde cannot express
    // directly, such as interleaved hyperlink controls and mixed-content `<hp:t>`.
    let mut run_xml_replacements: Vec<(String, String)> = Vec::new();
    let hx_section = build_section(
        section,
        &mut chart_entries,
        &mut embedded_oles,
        &mut run_xml_replacements,
        chart_offset,
        embedded_ole_offset,
        options,
    )?;
    let inner_xml = quick_xml::se::to_string(&hx_section)
        .map_err(|e| HwpxError::XmlSerialize { detail: e.to_string() })?;

    // quick_xml produces `<sec>...</sec>` (from the serde rename).
    // We need `<hs:sec xmlns:...>...</hs:sec>`, so strip the outer
    // element and wrap with our template.
    let inner_content = strip_root_element(&inner_xml);

    // Enrich <hp:secPr> with sub-elements required by 한글 (grid,
    // startNum, visibility, footnote/endnote, pageBorderFill, masterPage refs).
    let mut enriched = enrich_sec_pr(inner_content, section, masterpage_offset);

    // Inject header/footer/page number controls after colPr
    inject_header_footer_pagenum(&mut enriched, section, &mut run_xml_replacements, options)?;

    // Replace hyperlink placeholder runs with real interleaved XML.
    // Serde cannot express the ctrl-text-ctrl interleaving required by
    // HWPX fieldBegin/fieldEnd, so we serialize a marker and swap it here.
    enriched = apply_run_xml_replacements(enriched, &run_xml_replacements);

    // Generate masterpage XML files
    let master_pages = build_masterpage_entries(section, masterpage_offset);

    Ok(SectionEncodeResult {
        xml: wrap_section_xml(&enriched),
        charts: chart_entries,
        master_pages,
        embedded_oles,
    })
}

/// Applies marker → real-XML substitutions in a single allocation.
///
/// Each marker produced by [`next_marker`] is a globally-unique nonce token
/// that occurs exactly once in `xml` and never inside any replacement value,
/// so locating every marker up front and splicing in offset order yields the
/// same bytes as sequentially calling `String::replacen(marker, real, 1)` —
/// while allocating the output string only once instead of once per marker
/// (the previous loop was O(N·L): one full-string copy per replacement).
///
/// # Invariant this relies on: child-before-parent push ordering
///
/// A `real_xml` payload *may* embed another control's marker token — e.g. a
/// memo/group whose sublist contains a nested field. Equivalence to the old
/// sequential `replacen` loop is preserved only because nested controls push
/// their `(marker, real)` pair to `run_xml_replacements` **before** their
/// enclosing parent. With that ordering both strategies behave identically:
/// the child's pass runs while its marker is still hidden inside the parent's
/// not-yet-applied `real_xml` (a no-op for both), then the parent's pass
/// splices the child marker into the output where it stays unreplaced in both
/// the old loop and this single pass.
///
/// If ordering ever inverted to parent-before-child, the two would DIVERGE:
/// the old `replacen` rescans the mutated string and would resolve the nested
/// marker, whereas this pass only locates markers in the *original* string and
/// would leave it. `apply_run_xml_replacements_child_before_parent_matches_replacen`
/// locks the safe ordering against that regression.
///
/// (The fact that a deeply-nested field marker can survive into the output at
/// all is a separate, pre-existing memo/group limitation — see
/// `BACKLOG_SMITHY_HWPX.md` — and is byte-identical across both strategies, so
/// it does not affect this optimization.)
fn apply_run_xml_replacements(xml: String, replacements: &[(String, String)]) -> String {
    if replacements.is_empty() {
        return xml;
    }
    // Locate each marker's byte offset. Each marker is unique and occurs at
    // most once; a missing marker is a silent no-op, matching `replacen`.
    let mut hits: Vec<(usize, usize, &str)> = Vec::with_capacity(replacements.len());
    for (marker, real) in replacements {
        if let Some(pos) = xml.find(marker.as_str()) {
            hits.push((pos, marker.len(), real.as_str()));
        }
    }
    if hits.is_empty() {
        return xml;
    }
    hits.sort_unstable_by_key(|&(pos, _, _)| pos);

    let extra: usize = hits.iter().map(|&(_, len, real)| real.len().saturating_sub(len)).sum();
    let mut out = String::with_capacity(xml.len() + extra);
    let mut cursor = 0usize;
    for (pos, len, real) in hits {
        // Skip any hit overlapping an already-applied region (defensive: a
        // duplicate marker would otherwise splice twice). Mirrors `replacen`'s
        // "first occurrence wins, later passes find nothing" behavior.
        if pos < cursor {
            continue;
        }
        out.push_str(&xml[cursor..pos]);
        out.push_str(real);
        cursor = pos + len;
    }
    out.push_str(&xml[cursor..]);
    out
}

/// Wraps inner XML content in an `<hs:sec>` element with all xmlns declarations.
fn wrap_section_xml(inner_xml: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><hs:sec{xmlns}>{inner_xml}</hs:sec>"#,
        xmlns = XMLNS_DECLS,
    )
}

/// Strips the outermost element from a serialized XML string, keeping inner content.
///
/// Input: `<sec><hp:p ...>...</hp:p></sec>` produces `<hp:p ...>...</hp:p>`.
/// Input: `<sec/>` (self-closing, empty) produces `""`.
fn strip_root_element(xml: &str) -> &str {
    // Self-closing element: <sec/> or <sec />
    if xml.ends_with("/>") {
        return "";
    }
    // Find first '>' after opening tag
    let start = match xml.find('>') {
        Some(i) => i + 1,
        None => return xml,
    };
    // Find last '</'
    let end = xml.rfind("</").unwrap_or(xml.len());
    &xml[start..end]
}

/// Builds an `HxSection` from a Core `Section`.
#[allow(clippy::too_many_arguments)]
fn build_section(
    section: &Section,
    chart_entries: &mut Vec<(String, String)>,
    embedded_oles: &mut Vec<(String, Vec<u8>)>,
    hyperlink_entries: &mut Vec<(String, String)>,
    chart_offset: usize,
    embedded_ole_offset: usize,
    options: EncodeOptions,
) -> HwpxResult<HxSection> {
    let paragraphs = section
        .paragraphs
        .iter()
        .enumerate()
        .map(|(idx, para)| {
            let inject_sec_pr = idx == 0;
            let page_settings = if inject_sec_pr { Some(&section.page_settings) } else { None };
            build_paragraph(
                para,
                inject_sec_pr,
                page_settings,
                section.text_direction,
                idx,
                0,
                chart_entries,
                embedded_oles,
                hyperlink_entries,
                chart_offset,
                embedded_ole_offset,
                options,
            )
            // (signature already plumbs embedded chart OLE state through)
        })
        .collect::<HwpxResult<Vec<_>>>()?;

    Ok(HxSection { paragraphs })
}

/// Builds an `HxParagraph` from a Core `Paragraph`.
///
/// When `inject_sec_pr` is true (first paragraph of the section), page
/// settings are embedded in the first run's `<hp:secPr>`.
/// `depth` tracks table nesting level for overflow prevention.
#[allow(clippy::too_many_arguments)]
fn build_paragraph(
    para: &Paragraph,
    inject_sec_pr: bool,
    page_settings: Option<&PageSettings>,
    text_direction: TextDirection,
    para_idx: usize,
    depth: usize,
    chart_entries: &mut Vec<(String, String)>,
    embedded_oles: &mut Vec<(String, Vec<u8>)>,
    hyperlink_entries: &mut Vec<(String, String)>,
    chart_offset: usize,
    embedded_ole_offset: usize,
    options: EncodeOptions,
) -> HwpxResult<HxParagraph> {
    let mut runs = build_runs(
        &para.runs,
        inject_sec_pr,
        page_settings,
        text_direction,
        depth,
        chart_entries,
        embedded_oles,
        hyperlink_entries,
        chart_offset,
        embedded_ole_offset,
        options,
    )?;

    // Inject <hp:titleMark ignore="false"/> into the first run when the
    // paragraph has a heading level, enabling 한글 auto-TOC generation.
    if para.heading_level.is_some() {
        if let Some(first_run) = runs.first_mut() {
            first_run.title_mark = Some(HxTitleMark { ignore: false });
        }
    }

    // Omit linesegarray so 한글 recalculates from scratch on open.
    // Previously we emitted a 1-seg placeholder, but justify alignment
    // relied on accurate per-line data — causing character overlap for
    // multi-line paragraphs. Omitting it forces 한글 to compute properly.
    //
    // Exception (opt-in, PDF 재생/비교 파이프라인 전용): emit_layout_cache
    // 가 켜지면 Core 로 승격된 캐시를 wire 그대로 되돌려 방출한다.
    // 편집 표면은 절대 켜지 않는다 — EncodeOptions::emit_layout_cache 문서 참조.
    let linesegarray = if options.emit_layout_cache {
        para.layout_cache.as_ref().map(|cache| HxLineSegArray {
            items: cache
                .lines
                .iter()
                .map(|l| HxLineSeg {
                    textpos: l.textpos,
                    vertpos: l.vertpos,
                    vertsize: l.vertsize,
                    textheight: l.textheight,
                    baseline: l.baseline,
                    spacing: l.spacing,
                    horzpos: l.horzpos,
                    horzsize: l.horzsize,
                    flags: l.flags,
                })
                .collect(),
        })
    } else {
        None
    };

    Ok(HxParagraph {
        id: format!("{para_idx}"),
        para_pr_id_ref: para.para_shape_id.get() as u32,
        style_id_ref: para.style_id.map_or(0, |s| s.get() as u32),
        page_break: u32::from(para.page_break),
        column_break: u32::from(para.column_break),
        merged: 0,
        runs,
        linesegarray,
    })
}

/// Builds `Vec<HxRun>` from Core runs.
///
/// Each Core `Run` maps to exactly one `HxRun`. Control runs produce
/// `HxCtrl` (footnote/endnote) or `HxRect` (textbox) elements.
/// Hyperlinks emit a placeholder text marker that is replaced with real
/// interleaved XML after serialization (see [`build_hyperlink_run_xml`]).
/// Unknown controls are silently skipped.
///
/// If `inject_sec_pr` is true and `page_settings` is `Some`, the first
/// run gets `<hp:secPr>` attached.
#[allow(clippy::too_many_arguments)]
fn build_runs(
    runs: &[Run],
    inject_sec_pr: bool,
    page_settings: Option<&PageSettings>,
    text_direction: TextDirection,
    depth: usize,
    chart_entries: &mut Vec<(String, String)>,
    embedded_oles: &mut Vec<(String, Vec<u8>)>,
    hyperlink_entries: &mut Vec<(String, String)>,
    chart_offset: usize,
    embedded_ole_offset: usize,
    options: EncodeOptions,
) -> HwpxResult<Vec<HxRun>> {
    let mut result = Vec::new();
    let mut sec_pr_injected = false;
    // Track bookmark span field_ids for matching SpanStart → SpanEnd
    let mut bookmark_span_ids: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for run in runs {
        let sec_pr = if inject_sec_pr && !sec_pr_injected && !run.content.is_control() {
            sec_pr_injected = true;
            page_settings.map(|ps| build_sec_pr(ps, text_direction))
        } else {
            None
        };

        let char_pr_id_ref = run.char_shape_id.get() as u32;

        let mut texts = Vec::new();
        let mut tables = Vec::new();
        let mut pictures = Vec::new();
        let mut ctrls = Vec::new();
        let mut rects = Vec::new();
        let mut lines = Vec::new();
        let mut ellipses = Vec::new();
        let mut polygons = Vec::new();
        let mut curves = Vec::new();
        let mut connect_lines = Vec::new();
        let mut equations = Vec::new();
        let mut switches: Vec<HxRunSwitch> = Vec::new();
        let mut dutmals: Vec<HxDutmal> = Vec::new();
        let mut composes: Vec<HxCompose> = Vec::new();

        match &run.content {
            RunContent::Text(s) => {
                if requires_inline_text_markup(s) {
                    let marker = next_marker("HWPTXT", char_pr_id_ref as usize);
                    let marker_xml = format!(r#"<hp:t>{marker}</hp:t>"#);
                    let real_xml = build_text_element_xml(s);
                    hyperlink_entries.push((marker_xml, real_xml));
                    texts.push(HxText::new(marker));
                } else {
                    texts.push(HxText::new(s.clone()));
                }
            }
            RunContent::InlineText(it) => {
                // Always route through the marker-substitution path
                // because InlineText carries `<hp:tab>` mixed content
                // that the plain-string serializer cannot represent.
                let marker = next_marker("HWPTXT", char_pr_id_ref as usize);
                let marker_xml = format!(r#"<hp:t>{marker}</hp:t>"#);
                let real_xml = build_inline_text_element_xml(it);
                hyperlink_entries.push((marker_xml, real_xml));
                texts.push(HxText::new(marker));
            }
            RunContent::Table(t) => {
                tables.push(build_table(t, depth, hyperlink_entries, options)?);
            }
            RunContent::Image(img) => {
                pictures.push(build_picture(img, depth, hyperlink_entries, options)?);
            }
            RunContent::Control(ctrl) => {
                match ctrl.as_ref() {
                    Control::Footnote { .. } | Control::Endnote { .. } => {
                        if let Some(hx_ctrl) =
                            encode_control_to_ctrl(ctrl, depth, hyperlink_entries, options)?
                        {
                            ctrls.push(hx_ctrl);
                        }
                    }
                    Control::TextBox { .. } => {
                        rects.push(encode_textbox_to_rect(
                            ctrl,
                            depth,
                            hyperlink_entries,
                            options,
                        )?);
                    }
                    Control::Rect { .. } => {
                        rects.push(encode_rect_to_hx(ctrl, depth, hyperlink_entries, options)?);
                    }
                    Control::Line { .. } => {
                        lines.push(encode_line_to_hx(ctrl, depth, hyperlink_entries, options)?);
                    }
                    Control::Ellipse { .. } => {
                        ellipses.push(encode_ellipse_to_hx(
                            ctrl,
                            depth,
                            hyperlink_entries,
                            options,
                        )?);
                    }
                    Control::Polygon { .. } => {
                        polygons.push(encode_polygon_to_hx(
                            ctrl,
                            depth,
                            hyperlink_entries,
                            options,
                        )?);
                    }
                    Control::Arc { .. } => {
                        ellipses.push(encode_arc_to_hx(ctrl, depth, hyperlink_entries, options)?);
                    }
                    Control::Curve { .. } => {
                        curves.push(encode_curve_to_hx(ctrl, depth, hyperlink_entries, options)?);
                    }
                    Control::ConnectLine { .. } => {
                        connect_lines.push(encode_connect_line_to_hx(
                            ctrl,
                            depth,
                            hyperlink_entries,
                            options,
                        )?);
                    }
                    Control::Equation { .. } => {
                        equations.push(encode_equation_to_hx(ctrl)?);
                    }
                    Control::Chart { .. } => {
                        let chart_idx = chart_offset + chart_entries.len() + 1;
                        let chart_ref = format!("Chart/chart{chart_idx}.xml");
                        let chart_xml = generate_chart_xml(ctrl)?;
                        chart_entries.push((chart_ref.clone(), chart_xml));
                        switches.push(encode_chart_switch(ctrl, &chart_ref));
                    }
                    Control::EmbeddedChart {
                        chart_xml,
                        ole_bytes,
                        width,
                        height,
                        horz_offset,
                        vert_offset,
                    } => {
                        // Allocate stable per-document ids for the new chart
                        // XML file and the OLE blob. Both lists are
                        // section-global; we add `*_offset` to keep ids
                        // unique across sections (parallel to the Chart arm).
                        let chart_idx = chart_offset + chart_entries.len() + 1;
                        let chart_ref = format!("Chart/chart{chart_idx}.xml");
                        chart_entries.push((chart_ref.clone(), chart_xml.clone()));

                        let ole_idx = embedded_ole_offset + embedded_oles.len() + 1;
                        let ole_item_id = format!("ole{ole_idx}");
                        embedded_oles.push((ole_item_id.clone(), ole_bytes.clone()));

                        // Serde cannot express the `<hp:switch><hp:case>…
                        // </hp:case><hp:default>…</hp:default></hp:switch>`
                        // shape (HxRunSwitch only models the `<hp:case>`
                        // arm). Emit the full switch as a marker-replaced
                        // run XML, mirroring the Hyperlink pattern.
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPECH", field_id);
                        let real_xml = build_embedded_chart_run_xml(
                            char_pr_id_ref,
                            &chart_ref,
                            &ole_item_id,
                            width.as_i32(),
                            height.as_i32(),
                            *horz_offset,
                            *vert_offset,
                        );
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText::new(marker));
                    }
                    Control::Hyperlink { text, url } => {
                        // Hyperlinks require interleaved ctrl-text-ctrl inside a
                        // single <hp:run> (fieldBegin → text → fieldEnd). Serde
                        // cannot express this ordering, so we emit a placeholder
                        // run with a unique marker and replace it after
                        // serialization in `encode_section`.
                        // Normalize the URL scheme before encoding. Schemeless
                        // bare domains (e.g. `www.go.kr`) are promoted to
                        // `http://`; only explicitly unsafe schemes
                        // (`javascript:`, `data:`, `file:`, …) are rejected.
                        let Some(safe_url) = super::normalize_hyperlink_url(url) else {
                            return Err(crate::error::HwpxError::InvalidStructure {
                                detail: format!(
                                    "Unsafe URL scheme in hyperlink: '{url}'. Only http://, https://, and mailto: are allowed."
                                ),
                            });
                        };
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPHL", field_id);
                        let real_xml =
                            build_hyperlink_run_xml(text, &safe_url, char_pr_id_ref, field_id);
                        // The marker run will serialize to something like
                        // <hp:run charPrIDRef="N"><hp:t>__HWPFORGE_HYPERLINK_0__</hp:t></hp:run>
                        // We record the full serialized marker run pattern so the
                        // replacement in encode_section is exact.
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText::new(marker));
                    }
                    Control::Dutmal {
                        main_text,
                        sub_text,
                        position,
                        sz_ratio,
                        align,
                        metadata,
                    } => {
                        dutmals.push(encode_dutmal_to_hx(
                            main_text,
                            sub_text,
                            *position,
                            *sz_ratio,
                            *align,
                            metadata.option,
                        ));
                    }
                    Control::Compose {
                        compose_text,
                        circle_type,
                        char_sz,
                        compose_type,
                        char_pr_ids,
                    } => {
                        composes.push(encode_compose_to_hx(
                            compose_text,
                            circle_type,
                            *char_sz,
                            compose_type,
                            char_pr_ids,
                        ));
                    }
                    Control::IndexMark { .. }
                    | Control::Bookmark { bookmark_type: BookmarkType::Point, .. } => {
                        if let Some(hx_ctrl) =
                            encode_control_to_ctrl(ctrl, depth, hyperlink_entries, options)?
                        {
                            ctrls.push(hx_ctrl);
                        }
                    }
                    Control::Bookmark { name, bookmark_type }
                        if *bookmark_type == BookmarkType::SpanStart =>
                    {
                        let field_id = hyperlink_entries.len();
                        bookmark_span_ids.insert(name.clone(), field_id);
                        let marker = next_marker("HWPBM", field_id);
                        let real_xml =
                            build_bookmark_span_start_run_xml(name, char_pr_id_ref, field_id);
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText::new(marker));
                    }
                    Control::Bookmark { name, bookmark_type }
                        if *bookmark_type == BookmarkType::SpanEnd =>
                    {
                        if let Some(&field_id) = bookmark_span_ids.get(name) {
                            let marker = next_marker("HWPBE", field_id);
                            let real_xml =
                                build_bookmark_span_end_run_xml(char_pr_id_ref, field_id);
                            let marker_run_xml = format!(
                                r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                            );
                            hyperlink_entries.push((marker_run_xml, real_xml));
                            texts.push(HxText::new(marker));
                        }
                        // Silently skip if no matching SpanStart found
                    }
                    Control::Field { field_type, hint_text, help_text, name, display_text } => {
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPFD", field_id);
                        let hint = hint_text.as_deref().unwrap_or("");
                        let real_xml = build_field_run_xml(
                            field_type,
                            hint,
                            help_text.as_deref().unwrap_or(""),
                            name.as_deref().unwrap_or(""),
                            display_text,
                            char_pr_id_ref,
                            field_id,
                        );
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText::new(marker));
                    }
                    // LOSSY (Wave 12n architect review): The DateCodeField and
                    // UnknownSummary arms below emit SUMMERY-shaped XML as a
                    // *best-effort* HWPX surrogate. HWPX has no native counterpart
                    // for `%smr` unknown tokens or `%dte` format patterns.
                    // Round-tripping through HWPX → Core decoder normalises these
                    // back as `Field(ModifiedTime/CreatedTime)` (for
                    // DateCodeField) or `UnknownSummary` (for UnknownSummary), so
                    // the original Core variant is NOT preserved.
                    //
                    // PathField is NO LONGER LOSSY (Wave 12n Step 6) — see the
                    // arm further below which emits Hancom-native
                    // `type="PATH"` with `Format=` param and a distinct fieldid.
                    Control::UnknownSummary { token, display_text } => {
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPFD", field_id);
                        // Wave 12p task #124: unknown token — assume Hancom
                        // recomputes (editable="1"). Matches pre-fix behavior.
                        // #120/#136: carry the cached resolved value in the
                        // body (empty body → 한컴 recovery warning).
                        let real_xml = build_summary_run_xml_raw(
                            token,
                            display_text,
                            "",
                            char_pr_id_ref,
                            1_000_000_000_u64 + field_id as u64,
                            true,
                        );
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText::new(marker));
                    }
                    Control::DateCodeField { is_time_mode, display_text, .. } => {
                        // LOSSY: %dte → SUMMERY mapping; raw_trailer is discarded.
                        // Round-trip through HWPX comes back as `Field(ModifiedTime)`
                        // or `Field(CreatedTime)` — proven by
                        // `lossy_roundtrip_datecodefield_{date,time}_becomes_*` tests.
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPFD", field_id);
                        let token: &str =
                            if *is_time_mode { "$createtime" } else { "$modifiedtime" };
                        // #120/#136: emit the cached resolved date/time as the
                        // body (the raw format pattern was never a valid display
                        // value — an empty/garbage body triggers the 한컴
                        // recovery warning).
                        // Wave 12p task #124: both $createtime / $modifiedtime
                        // are recomputed by Hancom (editable="1").
                        let real_xml = build_summary_run_xml_raw(
                            token,
                            display_text,
                            "",
                            char_pr_id_ref,
                            1_000_000_000_u64 + field_id as u64,
                            true,
                        );
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText::new(marker));
                    }
                    Control::PathField { command, display_text } => {
                        // Wave 12n Step 6 — LOSSLESS emit. The prior SUMMERY
                        // surrogate (mapped %pat → type="SUMMERY") triggered the
                        // Hancom "low security level — content recovered"
                        // warning (#120) because native files emit
                        // type="PATH" with `Format=` param, `fieldid=628121972`,
                        // and `editable="0"`. We now emit the wire shape
                        // directly. #120/#136: the cached resolved path is
                        // carried in the body (an empty body still triggers the
                        // recovery warning); 한컴 recomputes `$P$F` on save.
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPFD", field_id);
                        let real_xml = build_path_field_run_xml_raw(
                            command.wire_command(),
                            display_text,
                            char_pr_id_ref,
                            1_000_000_000_u64 + field_id as u64,
                        );
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText::new(marker));
                    }
                    Control::InlinePageNumber { kind, .. } => {
                        // atno inline page number renders as <hp:autoNum
                        // numType="PAGE"|"TOTAL_PAGE">. Unknown flag values are
                        // skipped to avoid fabricating semantics (Wave 12n
                        // architect review CRITICAL: TotalPages/Unknown must not
                        // collapse to CurrentPage).
                        let Some(real_xml) = build_autonum_run_xml(char_pr_id_ref, *kind) else {
                            continue;
                        };
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPFD", field_id);
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText::new(marker));
                    }
                    Control::CrossRef {
                        target,
                        ref_type,
                        content_type,
                        as_hyperlink,
                        display_text,
                    } => {
                        // Wave 12m Phase 2 Step 4: display_text 는 caller 가
                        // 채워야 하는 visible body. 비어 있으면 target 의
                        // as_display() 로 fallback 하여 사용자가 직접 build 한
                        // CrossRef (no body) 도 한컴이 인식할 최소 text 를 갖
                        // 도록 한다.
                        let fallback_text = target.as_display();
                        let visible_text = if display_text.is_empty() {
                            fallback_text.as_str()
                        } else {
                            display_text.as_str()
                        };
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPXR", field_id);
                        let real_xml = build_crossref_run_xml(
                            target,
                            visible_text,
                            ref_type,
                            content_type,
                            *as_hyperlink,
                            char_pr_id_ref,
                            field_id,
                        );
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText::new(marker));
                    }
                    Control::Memo { content, anchor_runs, metadata } => {
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPME", field_id);
                        let sublist_xml =
                            encode_memo_sublist(content, depth, hyperlink_entries, options)?;
                        let anchor_xml = build_memo_anchor_xml(anchor_runs);
                        let real_xml = build_memo_run_xml(
                            &sublist_xml,
                            &anchor_xml,
                            metadata,
                            char_pr_id_ref,
                            field_id,
                        );
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText::new(marker));
                    }
                    Control::Group { .. } => {
                        // Group (묶음 객체) → <hp:container>. Serde cannot
                        // express the heterogeneous, z-ordered child shapes
                        // inside the container, so we build the full fragment
                        // and inject it via the marker-substitution path
                        // (mirroring hyperlink/memo/chart).
                        let container_xml = super::shapes::encode_group_to_xml(
                            ctrl,
                            depth,
                            0,
                            hyperlink_entries,
                            options,
                        )?;
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPGRP", field_id);
                        let real_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}">{container_xml}</hp:run>"#,
                        );
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText::new(marker));
                    }
                    Control::TextArt { .. } => {
                        // TextArt (글맵시) → <hp:textart>. Serde cannot express
                        // the fixed corner-point block + <hp:textartPr> shape
                        // and the scaMatrix entries are derived, so we build the
                        // full fragment and inject it via the marker-substitution
                        // path (mirroring group/chart/hyperlink).
                        let textart_xml = super::shapes::encode_text_art_to_xml(ctrl)?;
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPTAT", field_id);
                        let real_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}">{textart_xml}</hp:run>"#,
                        );
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText::new(marker));
                    }
                    Control::Unknown { .. } => {
                        // Unknown controls are silently skipped
                        continue;
                    }
                    _ => {
                        // Future Control variants silently skipped
                        continue;
                    }
                }
            }
            _ => {
                // Future RunContent variants are silently skipped
                // (non_exhaustive enum)
                continue;
            }
        }

        result.push(HxRun {
            char_pr_id_ref,
            sec_pr,
            texts,
            tables,
            pictures,
            ctrls,
            rects,
            lines,
            ellipses,
            polygons,
            curves,
            connect_lines,
            equations,
            switches,
            title_mark: None,
            dutmals,
            composes,
            containers: Vec::new(),
            textarts: Vec::new(),
        });
    }

    // If we need to inject secPr but there were no non-control runs,
    // create a synthetic empty run to carry it.
    if inject_sec_pr && !sec_pr_injected {
        if let Some(ps) = page_settings {
            result.insert(
                0,
                HxRun {
                    char_pr_id_ref: 0,
                    sec_pr: Some(build_sec_pr(ps, text_direction)),
                    texts: Vec::new(),
                    tables: Vec::new(),
                    pictures: Vec::new(),
                    ctrls: Vec::new(),
                    rects: Vec::new(),
                    lines: Vec::new(),
                    ellipses: Vec::new(),
                    polygons: Vec::new(),
                    equations: Vec::new(),
                    switches: Vec::new(),
                    title_mark: None,
                    dutmals: Vec::new(),
                    composes: Vec::new(),
                    curves: Vec::new(),
                    connect_lines: Vec::new(),
                    containers: Vec::new(),
                    textarts: Vec::new(),
                },
            );
        }
    }

    Ok(result)
}

/// Converts a Core Control (Footnote/Endnote) to `HxCtrl`.
///
/// Returns `None` for non-ctrl controls (TextBox, Unknown).
fn encode_control_to_ctrl(
    ctrl: &Control,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
) -> HwpxResult<Option<HxCtrl>> {
    match ctrl {
        Control::Footnote { inst_id, paragraphs } => Ok(Some(HxCtrl {
            foot_note: Some(HxFootNote {
                inst_id: inst_id.map(hwpforge_core::ObjectId::value),
                sub_list: encode_paragraphs_to_sublist(
                    paragraphs,
                    depth,
                    hyperlink_entries,
                    options,
                )?,
            }),
            ..Default::default()
        })),
        Control::Endnote { inst_id, paragraphs } => Ok(Some(HxCtrl {
            end_note: Some(HxFootNote {
                inst_id: inst_id.map(hwpforge_core::ObjectId::value),
                sub_list: encode_paragraphs_to_sublist(
                    paragraphs,
                    depth,
                    hyperlink_entries,
                    options,
                )?,
            }),
            ..Default::default()
        })),
        Control::Bookmark { name, bookmark_type: BookmarkType::Point } => Ok(Some(HxCtrl {
            bookmark: Some(HxBookmark { name: name.clone() }),
            ..Default::default()
        })),
        Control::IndexMark { primary, secondary } => Ok(Some(HxCtrl {
            indexmark: Some(HxIndexMark {
                first_key: primary.clone(),
                second_key: secondary.clone(),
            }),
            ..Default::default()
        })),
        _ => Ok(None),
    }
}

/// Encodes a `Vec<Paragraph>` into `HxSubList` with standard defaults
/// (vertical alignment `TOP`).
pub(crate) fn encode_paragraphs_to_sublist(
    paragraphs: &[Paragraph],
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
) -> HwpxResult<HxSubList> {
    build_sublist(paragraphs, depth, "TOP", hyperlink_entries, options)
}

/// Encodes a `Vec<Paragraph>` into `HxSubList` with an explicit vertical
/// alignment token (`TOP`/`CENTER`/`BOTTOM`). Used by shape `drawText`
/// encoders that carry a Core `VerticalAlign` field.
pub(crate) fn encode_paragraphs_to_sublist_with_align(
    paragraphs: &[Paragraph],
    depth: usize,
    vert_align: &str,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
) -> HwpxResult<HxSubList> {
    build_sublist(paragraphs, depth, vert_align, hyperlink_entries, options)
}

fn build_sublist(
    paragraphs: &[Paragraph],
    depth: usize,
    vert_align: &str,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
) -> HwpxResult<HxSubList> {
    let mut sub_chart_entries = Vec::new();
    let mut sub_embedded_oles: Vec<(String, Vec<u8>)> = Vec::new();
    let hx_paragraphs = paragraphs
        .iter()
        .enumerate()
        .map(|(idx, para)| {
            build_paragraph(
                para,
                false,
                None,
                TextDirection::Horizontal,
                idx,
                depth + 1,
                &mut sub_chart_entries,
                &mut sub_embedded_oles,
                hyperlink_entries,
                0,
                0,
                options,
            )
        })
        .collect::<HwpxResult<Vec<_>>>()?;

    Ok(HxSubList {
        id: String::new(),
        text_direction: "HORIZONTAL".to_string(),
        line_wrap: "BREAK".to_string(),
        vert_align: vert_align.to_string(),
        link_list_id_ref: 0,
        link_list_next_id_ref: 0,
        text_width: 0,
        text_height: 0,
        has_text_ref: 0,
        has_num_ref: 0,
        paragraphs: hx_paragraphs,
    })
}

// Shape encoding functions are defined in `super::shapes`.
use super::shapes::{
    encode_arc_to_hx, encode_connect_line_to_hx, encode_curve_to_hx, encode_ellipse_to_hx,
    encode_line_to_hx, encode_polygon_to_hx, encode_rect_to_hx, encode_textbox_to_rect,
};

/// Returns the current UTC time as an ISO 8601 timestamp
/// (`"YYYY-MM-DDTHH:MM:SSZ"`). Used as a sensible default for
/// `<hp:parameters name="CreateDateTime">` when callers don't supply one.
///
/// Implemented against `std::time` so the encoder stays dependency-free.
/// Algorithm is Howard Hinnant's civil-from-days conversion; see
/// <https://howardhinnant.github.io/date_algorithms.html>.
fn iso8601_utc_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, mo, d, h, mi, s) = unix_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Converts a Core `Caption` into an `HxCaption`.
///
/// `parent_width` is used for `lastWidth` (= parent object sz.width in HWPUNIT).
pub(crate) fn build_hx_caption(
    caption: &Caption,
    parent_width: i32,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
) -> HwpxResult<HxCaption> {
    let side = match caption.side {
        CaptionSide::Left => "LEFT",
        CaptionSide::Right => "RIGHT",
        CaptionSide::Top => "TOP",
        CaptionSide::Bottom => "BOTTOM",
    }
    .to_string();

    let width = caption.width.map(|w| w.as_i32()).unwrap_or(parent_width);
    let gap = caption.gap.as_i32();
    let sub_list =
        encode_paragraphs_to_sublist(&caption.paragraphs, depth, hyperlink_entries, options)?;

    // parent_width comes from HwpUnit::as_i32(), guaranteed non-negative
    Ok(HxCaption { side, full_sz: 0, width, gap, last_width: parent_width as u32, sub_list })
}

/// Generates a unique object-instance ID string for authored shapes that
/// carry no imported id.
///
/// # Id-space layout (E6/M2, ADR-010)
///
/// HWPX `id`/`instid`/`fieldid`/`beginIDRef` integers are all read by Hancom
/// as **signed 32-bit**, so every emitted value must stay below `i32::MAX`
/// (locked by the `all_field_builders_emit_signed_i32_safe_begin_id` test).
/// The encoder partitions the positive `i32` range into disjoint bands:
///
/// | band base       | purpose                                   |
/// | --------------- | ----------------------------------------- |
/// | `1` (this fn)   | authored object `id`/`instid` (sequential)|
/// | `1_000_000_000` | summary / auto-num field `beginID`        |
/// | `1_100_000_000` | click-here field `beginID`                |
/// | `1_200_000_000` | bookmark span `beginID`                   |
/// | `1_300_000_000` | cross-reference field `beginID`           |
/// | `1_400_000_000` | path field `beginID`                      |
/// | `1_500_000_000` | memo field `beginID`                      |
/// | `1_6xx`–`2_028…` | per-builder `fieldid` UIDs               |
///
/// Cross-reference **targets** preserve their imported
/// [`ObjectId`](hwpforge_core::ObjectId) verbatim (see `picture`/`table`/
/// `equation`/group/text-art encoders) so the referencer's
/// `RefTarget::Object` resolves to the same `id`. Only authored targets with
/// no imported id fall back to this counter, whose small sequential values
/// are disjoint from every field band above.
///
/// > Follow-up (ADR-010): replace this process-global counter with a
/// > per-encode allocator and move authored object ids into a dedicated
/// > reserved band. Deferred here because it would change authored-object
/// > byte output without a fidelity gate requiring it (the convert path
/// > always preserves imported ids, so it is unaffected).
///
/// Each call returns a monotonically increasing ID, safe for parallel encoding.
pub(crate) fn generate_instid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static INSTID_COUNTER: AtomicU64 = AtomicU64::new(1);
    INSTID_COUNTER.fetch_add(1, Ordering::Relaxed).to_string()
}

/// Default inner cell margin (left/right: 510 ≈ 1.8mm, top/bottom: 141 ≈ 0.5mm).
const DEFAULT_CELL_MARGIN: HxTableMargin =
    HxTableMargin { left: 510, right: 510, top: 141, bottom: 141 };

/// Default outer table margin (283 ≈ 1mm on all sides).
const DEFAULT_OUT_MARGIN: HxTableMargin =
    HxTableMargin { left: 283, right: 283, top: 283, bottom: 283 };

/// `borderFillIDRef` for table cells (matches header.xml borderFill id=3).
const TABLE_BORDER_FILL_ID: u32 = 3;

// ── Linesegarray placeholder ─────────────────────────────────────

/// Default horizontal size for A4 with 30mm margins (59528 - 8504 - 8504).
const DEFAULT_HORZ_SIZE: i32 = 42520;

// NOTE: linesegarray is intentionally omitted from paragraph output.
// Previously we emitted a 1-seg placeholder, but 한글 uses lineseg data
// for justify alignment layout. Inaccurate values (1 seg for multi-line
// paragraphs) caused character overlap. Omitting it lets 한글 compute
// accurate linesegs from scratch on open.

// ── 한글 compatibility: secPr enrichment ────────────────────────

// ── Header/Footer/PageNumber injection ──────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::image::{
        ImageFormat, ImagePlacement, ImageRelativeTo, ImageTextFlow, ImageTextWrap,
    };
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    /// Helper: build a simple text paragraph.
    fn text_paragraph(text: &str, para_shape: usize, char_shape: usize) -> Paragraph {
        Paragraph::with_runs(
            vec![Run::text(text, CharShapeIndex::new(char_shape))],
            ParaShapeIndex::new(para_shape),
        )
    }

    /// Helper: build a section with one text paragraph.
    fn simple_section(text: &str) -> Section {
        Section::with_paragraphs(vec![text_paragraph(text, 0, 0)], PageSettings::a4())
    }

    // ── Test 1: Single text paragraph ────────────────────────────

    #[test]
    fn encode_single_text_paragraph() {
        let section = simple_section("텍스트");
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        assert!(xml.contains("<?xml version="), "missing XML declaration");
        assert!(xml.contains("<hs:sec"), "missing <hs:sec> root");
        assert!(xml.contains("</hs:sec>"), "missing </hs:sec> close");
        assert!(xml.contains("<hp:p "), "missing <hp:p>");

        // Verify Gap 6: colPr is injected after </hp:secPr>
        assert!(xml.contains("<hp:ctrl>"), "missing <hp:ctrl>");
        assert!(
            xml.contains("<hp:colPr id=\"\" type=\"NEWSPAPER\" layout=\"LEFT\" colCount=\"1\""),
            "missing colPr with correct attributes"
        );
        assert!(xml.contains("sameSz=\"1\" sameGap=\"0\""), "colPr missing sameSz/sameGap");

        // Verify colPr appears AFTER </hp:secPr> and BEFORE <hp:t>
        let sec_pr_end = xml.find("</hp:secPr>").expect("secPr must be present");
        let col_pr_pos = xml.find("<hp:colPr").expect("colPr must be present");
        assert!(col_pr_pos > sec_pr_end, "colPr must come after </hp:secPr>");
        assert!(xml.contains("<hp:run "), "missing <hp:run>");
        assert!(xml.contains("<hp:t>텍스트</hp:t>"), "missing text content");
        assert!(xml.contains(r#"xmlns:hp="#), "missing xmlns:hp namespace");
    }

    #[test]
    fn encode_text_paragraph_with_tab_emits_hp_tab_and_roundtrips() {
        let section = simple_section("LEFT\tRIGHT");
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        assert!(
            xml.contains("<hp:t>LEFT<hp:tab/>RIGHT</hp:t>"),
            "tab text must be emitted as mixed-content hp:tab"
        );

        let decoded =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        assert_eq!(decoded.paragraphs[0].runs[0].content.as_text(), Some("LEFT\tRIGHT"));
    }

    // ── Test 2: Section roundtrip via decoder ────────────────────

    #[test]
    fn encode_section_roundtrip() {
        let section = simple_section("안녕하세요 round-trip test");
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        // Parse back with the decoder
        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        assert_eq!(result.paragraphs.len(), 1);
        assert_eq!(
            result.paragraphs[0].runs[0].content.as_text(),
            Some("안녕하세요 round-trip test"),
        );
        assert_eq!(result.paragraphs[0].para_shape_id.get(), 0);
    }

    // ── Test 3: SecPr injection ──────────────────────────────────

    #[test]
    fn sec_pr_injection() {
        let ps = PageSettings {
            width: HwpUnit::new(59528).unwrap(),
            height: HwpUnit::new(84188).unwrap(),
            margin_left: HwpUnit::new(8504).unwrap(),
            margin_right: HwpUnit::new(8504).unwrap(),
            margin_top: HwpUnit::new(5668).unwrap(),
            margin_bottom: HwpUnit::new(4252).unwrap(),
            header_margin: HwpUnit::new(4252).unwrap(),
            footer_margin: HwpUnit::new(4252).unwrap(),
            ..PageSettings::a4()
        };
        let section = Section::with_paragraphs(vec![text_paragraph("Content", 0, 0)], ps);
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        assert!(xml.contains("<hp:secPr"), "missing secPr");
        assert!(xml.contains(r#"textDirection="HORIZONTAL""#), "missing textDirection");
        assert!(xml.contains(r#"width="59528""#), "missing width");
        assert!(xml.contains(r#"height="84188""#), "missing height");
        assert!(xml.contains(r#"left="8504""#), "missing left margin");

        // Roundtrip the page settings through the decoder
        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        let decoded_ps = result.page_settings.unwrap();
        assert_eq!(decoded_ps.width.as_i32(), 59528);
        assert_eq!(decoded_ps.height.as_i32(), 84188);
        assert_eq!(decoded_ps.margin_left.as_i32(), 8504);
        assert_eq!(decoded_ps.margin_right.as_i32(), 8504);
        assert_eq!(decoded_ps.margin_top.as_i32(), 5668);
        assert_eq!(decoded_ps.margin_bottom.as_i32(), 4252);
        assert_eq!(decoded_ps.header_margin.as_i32(), 4252);
        assert_eq!(decoded_ps.footer_margin.as_i32(), 4252);
    }

    // ── Test 4: Table encoding ───────────────────────────────────

    #[test]
    fn table_encoding() {
        let cell1 =
            TableCell::new(vec![text_paragraph("Cell1", 0, 0)], HwpUnit::new(5000).unwrap());
        let cell2 =
            TableCell::new(vec![text_paragraph("Cell2", 0, 0)], HwpUnit::new(5000).unwrap());
        let row = TableRow::new(vec![cell1, cell2]);
        let table = Table::new(vec![row]);

        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        assert!(xml.contains(r#"rowCnt="1""#), "missing rowCnt");
        assert!(xml.contains(r#"colCnt="2""#), "missing colCnt");
        assert!(xml.contains("<hp:t>Cell1</hp:t>"), "missing Cell1 text");
        assert!(xml.contains("<hp:t>Cell2</hp:t>"), "missing Cell2 text");
    }

    #[test]
    fn table_encoding_preserves_presentation_fields() {
        let first = TableCell::new(vec![text_paragraph("A", 0, 0)], HwpUnit::new(10000).unwrap())
            .with_height(HwpUnit::new(282).unwrap())
            .with_border_fill_id(4)
            .with_margin(TableMargin {
                left: HwpUnit::new(4251).unwrap(),
                right: HwpUnit::new(5669).unwrap(),
                top: HwpUnit::new(2834).unwrap(),
                bottom: HwpUnit::new(1417).unwrap(),
            })
            .with_vertical_align(TableVerticalAlign::Top);

        let second = TableCell::new(vec![text_paragraph("B", 0, 0)], HwpUnit::new(10000).unwrap())
            .with_height(HwpUnit::new(1281).unwrap())
            .with_border_fill_id(7)
            .with_vertical_align(TableVerticalAlign::Bottom);

        let table = Table::new(vec![TableRow::new(vec![first, second]).with_header(true)])
            .with_width(HwpUnit::new(20000).unwrap())
            .with_page_break(TablePageBreak::Cell)
            .with_cell_spacing(HwpUnit::new(120).unwrap())
            .with_border_fill_id(9);

        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        assert!(xml.contains(r#"cellSpacing="120""#), "missing cellSpacing");
        assert!(xml.contains(r#"borderFillIDRef="9""#), "missing table borderFillIDRef");
        assert!(xml.contains(r#"borderFillIDRef="4""#), "missing first cell borderFillIDRef");
        assert!(xml.contains(r#"borderFillIDRef="7""#), "missing second cell borderFillIDRef");
        assert!(xml.contains(r#"vertAlign="TOP""#), "missing TOP vertical align");
        assert!(xml.contains(r#"vertAlign="BOTTOM""#), "missing BOTTOM vertical align");
        assert!(xml.contains(r#"cellMargin left="4251" right="5669" top="2834" bottom="1417""#));
        assert!(xml.contains(r#"cellSz width="10000" height="282""#), "missing first cell height");
        assert!(
            xml.contains(r#"cellSz width="10000" height="1281""#),
            "missing second cell height"
        );
    }

    #[test]
    fn table_encoding_does_not_spread_row_height_into_zero_height_cells() {
        let first = TableCell::new(vec![text_paragraph("A", 0, 0)], HwpUnit::new(7777).unwrap())
            .with_height(HwpUnit::new(1226).unwrap());

        let second = TableCell::new(vec![text_paragraph("B", 0, 0)], HwpUnit::new(8888).unwrap());

        let table = Table::new(vec![TableRow::with_height(
            vec![first, second],
            HwpUnit::new(1226).unwrap(),
        )])
        .with_width(HwpUnit::new(16665).unwrap())
        .with_page_break(TablePageBreak::Cell)
        .with_repeat_header(false);

        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        assert!(
            xml.contains(r#"cellSz width="7777" height="1226""#),
            "explicit-height cell should preserve its own height"
        );
        assert!(
            xml.contains(r#"cellSz width="8888" height="0""#),
            "mixed row should keep zero-height cell at 0 instead of inheriting row height"
        );
    }

    // ── Test 5: Image encoding ───────────────────────────────────

    #[test]
    fn image_encoding() {
        let img = Image::new(
            "BinData/logo.png",
            HwpUnit::new(10000).unwrap(),
            HwpUnit::new(5000).unwrap(),
            ImageFormat::Png,
        );
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::image(img, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        assert!(
            xml.contains(r#"binaryItemIDRef="logo""#),
            "missing binaryItemIDRef (should strip BinData/ prefix and extension)"
        );
        assert!(xml.contains(r#"width="10000""#), "missing image width");
        assert!(xml.contains(r#"height="5000""#), "missing image height");
    }

    // ── Test 6: Multiple paragraphs ──────────────────────────────

    #[test]
    fn multi_paragraph() {
        let section = Section::with_paragraphs(
            vec![
                text_paragraph("First", 0, 0),
                text_paragraph("Second", 1, 0),
                text_paragraph("Third", 2, 0),
            ],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        assert!(xml.contains("<hp:t>First</hp:t>"), "missing First");
        assert!(xml.contains("<hp:t>Second</hp:t>"), "missing Second");
        assert!(xml.contains("<hp:t>Third</hp:t>"), "missing Third");

        // secPr should only appear once (in the first paragraph)
        let sec_pr_count = xml.matches("<hp:secPr").count();
        assert_eq!(sec_pr_count, 1, "secPr should appear exactly once, in first paragraph");
    }

    // ── Test 7: Nested table ─────────────────────────────────────

    #[test]
    fn nested_table() {
        // Inner table
        let inner_cell =
            TableCell::new(vec![text_paragraph("Deep", 0, 0)], HwpUnit::new(3000).unwrap());
        let inner_table = Table::new(vec![TableRow::new(vec![inner_cell])]);

        // Outer table: cell contains a paragraph with the inner table
        let outer_cell = TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::table(inner_table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::new(8000).unwrap(),
        );
        let outer_table = Table::new(vec![TableRow::new(vec![outer_cell])]);

        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(outer_table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );

        // Should succeed within nesting limit
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains("<hp:t>Deep</hp:t>"), "missing nested text");
    }

    // ── Test 8: Hyperlink encoding ─────────────────────────────

    #[test]
    fn hyperlink_encoding() {
        use hwpforge_core::control::Control;

        let ctrl =
            Control::Hyperlink { text: "link".to_string(), url: "https://example.com".to_string() };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![
                    Run::text("before", CharShapeIndex::new(0)),
                    Run::control(ctrl, CharShapeIndex::new(0)),
                    Run::text("after", CharShapeIndex::new(0)),
                ],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        assert!(xml.contains("<hp:t>before</hp:t>"), "missing 'before' text");
        assert!(xml.contains("<hp:t>after</hp:t>"), "missing 'after' text");

        // Hyperlink must produce fieldBegin/fieldEnd pair
        assert!(xml.contains(r#"type="HYPERLINK"#), "missing HYPERLINK fieldBegin type");
        assert!(xml.contains("https://example.com"), "missing hyperlink URL in parameters");
        assert!(xml.contains("<hp:t>link</hp:t>"), "missing hyperlink display text");
        assert!(xml.contains("<hp:fieldEnd"), "missing fieldEnd closing element");

        // fieldBegin must have unique id and a non-zero fieldid
        assert!(xml.contains(r#"fieldid="1628000000""#), "fieldBegin must have non-zero fieldid");
        assert!(!xml.contains(r#"fieldid="0""#), "fieldid must never be 0 (invalid in Hancom)");
        assert!(xml.contains(r#"id="1100000000""#), "fieldBegin must have unique id");
        // fieldEnd.beginIDRef must reference fieldBegin.id (NOT fieldid)
        assert!(
            xml.contains(r#"beginIDRef="1100000000""#),
            "fieldEnd must reference fieldBegin id via beginIDRef"
        );

        // No leftover placeholder marker
        assert!(
            !xml.contains("__HWPFORGE_HYPERLINK_"),
            "hyperlink placeholder marker was not replaced"
        );
    }

    // ── Test 8b: Unknown control is skipped ──────────────────────

    #[test]
    fn unknown_control_skipped() {
        use hwpforge_core::control::Control;

        let ctrl = Control::Unknown { tag: "test".to_string(), data: None };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![
                    Run::text("before", CharShapeIndex::new(0)),
                    Run::control(ctrl, CharShapeIndex::new(0)),
                    Run::text("after", CharShapeIndex::new(0)),
                ],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        assert!(xml.contains("<hp:t>before</hp:t>"), "missing 'before' text");
        assert!(xml.contains("<hp:t>after</hp:t>"), "missing 'after' text");
        assert!(!xml.contains("test"), "unknown control content should not appear in XML");
    }

    // ── Test 9: Empty text produces valid XML ────────────────────

    #[test]
    fn empty_text_produces_valid_xml() {
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        // Should parse without error
        assert!(xml.contains("<hs:sec"), "missing root element");
        assert!(xml.contains("</hs:sec>"), "missing close tag");
    }

    // ── Test 10: Korean text preservation ────────────────────────

    #[test]
    fn korean_text_preservation() {
        let korean = "우리는 수학을 공부한다.";
        let section = simple_section(korean);
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        // Roundtrip through decoder
        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        assert_eq!(result.paragraphs[0].runs[0].content.as_text(), Some(korean),);
    }

    // ── Additional edge cases ────────────────────────────────────

    #[test]
    fn empty_section_produces_valid_xml() {
        let section = Section::new(PageSettings::a4());
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        assert!(xml.contains("<hs:sec"), "missing root element");
        assert!(xml.contains("</hs:sec>"), "missing close tag");
    }

    #[test]
    fn strip_root_element_basic() {
        let xml = "<sec><hp:p>inner</hp:p></sec>";
        assert_eq!(strip_root_element(xml), "<hp:p>inner</hp:p>");
    }

    #[test]
    fn strip_root_element_self_closing() {
        assert_eq!(strip_root_element("<sec/>"), "");
    }

    #[test]
    fn strip_root_element_with_attributes() {
        let xml = r#"<sec attr="val"><hp:p>x</hp:p></sec>"#;
        assert_eq!(strip_root_element(xml), "<hp:p>x</hp:p>");
    }

    #[test]
    fn nesting_depth_exceeded() {
        let hx_table = Table::new(vec![]);
        let err =
            build_table(&hx_table, MAX_NESTING_DEPTH, &mut Vec::new(), EncodeOptions::default())
                .unwrap_err();
        match &err {
            HwpxError::InvalidStructure { detail } => {
                assert!(detail.contains("nesting depth"));
            }
            _ => panic!("expected InvalidStructure, got: {err:?}"),
        }
    }

    #[test]
    fn pathological_span_table_rejected_before_placement() {
        // Covered area 1025×1024 = 1_049_600 > MAX_GRID_POSITIONS
        // (1_048_576): the encoder must fail fast instead of feeding the
        // lenient placement scan per-position state.
        let cell = TableCell::with_span(
            vec![text_paragraph("x", 0, 0)],
            HwpUnit::new(3000).unwrap(),
            1025,
            1024,
        );
        let table = Table::new(vec![TableRow::new(vec![cell])]);
        let err = build_table(&table, 0, &mut Vec::new(), EncodeOptions::default()).unwrap_err();
        match &err {
            HwpxError::InvalidStructure { detail } => {
                assert!(detail.contains("covered area"), "unexpected detail: {detail}");
            }
            _ => panic!("expected InvalidStructure, got: {err:?}"),
        }
    }

    #[test]
    fn image_without_bindata_prefix() {
        let img = Image::new(
            "image.jpg",
            HwpUnit::new(1000).unwrap(),
            HwpUnit::new(500).unwrap(),
            ImageFormat::Jpeg,
        );
        let hx = build_picture(&img, 0, &mut Vec::new(), EncodeOptions::default()).unwrap();
        assert_eq!(
            hx.img.unwrap().binary_item_id_ref,
            "image",
            "path without BinData/ prefix should strip extension only"
        );
    }

    #[test]
    fn image_with_explicit_placement_uses_override_values() {
        let img = Image::new(
            "BinData/image1.png",
            HwpUnit::new(10000).unwrap(),
            HwpUnit::new(5000).unwrap(),
            ImageFormat::Png,
        )
        .with_placement(ImagePlacement {
            text_wrap: ImageTextWrap::Square,
            text_flow: ImageTextFlow::RightOnly,
            treat_as_char: false,
            flow_with_text: true,
            allow_overlap: true,
            vert_rel_to: ImageRelativeTo::Paper,
            horz_rel_to: ImageRelativeTo::Page,
            vert_offset: HwpUnit::new(1200).unwrap(),
            horz_offset: HwpUnit::new(3400).unwrap(),
        });

        let hx = build_picture(&img, 0, &mut Vec::new(), EncodeOptions::default()).unwrap();
        assert_eq!(hx.text_wrap, "SQUARE");
        assert_eq!(hx.text_flow, "RIGHT_ONLY");
        let pos = hx.pos.expect("position should be present");
        assert_eq!(pos.treat_as_char, 0);
        assert_eq!(pos.flow_with_text, 1);
        assert_eq!(pos.allow_overlap, 1);
        assert_eq!(pos.vert_rel_to, "PAPER");
        assert_eq!(pos.horz_rel_to, "PAGE");
        assert_eq!(pos.vert_offset, 1200);
        assert_eq!(pos.horz_offset, 3400);
    }

    #[test]
    fn image_without_placement_keeps_legacy_defaults() {
        let img = Image::new(
            "BinData/photo.png",
            HwpUnit::new(10000).unwrap(),
            HwpUnit::new(5000).unwrap(),
            ImageFormat::Png,
        );
        let hx = build_picture(&img, 0, &mut Vec::new(), EncodeOptions::default()).unwrap();
        let pos = hx.pos.expect("picture position should exist");

        assert_eq!(hx.text_wrap, "TOP_AND_BOTTOM");
        assert_eq!(hx.text_flow, "BOTH_SIDES");
        assert_eq!(pos.treat_as_char, 1);
        assert_eq!(pos.flow_with_text, 0);
        assert_eq!(pos.allow_overlap, 0);
        assert_eq!(pos.vert_rel_to, "PARA");
        assert_eq!(pos.horz_rel_to, "PARA");
        assert_eq!(pos.vert_offset, 0);
        assert_eq!(pos.horz_offset, 0);
    }

    #[test]
    fn paragraph_shape_id_preserved_in_roundtrip() {
        let section = Section::with_paragraphs(
            vec![text_paragraph("p0", 3, 5), text_paragraph("p1", 7, 2)],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();

        assert_eq!(result.paragraphs[0].para_shape_id.get(), 3);
        assert_eq!(result.paragraphs[0].runs[0].char_shape_id.get(), 5);
        assert_eq!(result.paragraphs[1].para_shape_id.get(), 7);
        assert_eq!(result.paragraphs[1].runs[0].char_shape_id.get(), 2);
    }

    #[test]
    fn table_roundtrip_via_decoder() {
        let cell = TableCell::new(vec![text_paragraph("Hello", 0, 0)], HwpUnit::new(5000).unwrap());
        let table = Table::new(vec![TableRow::new(vec![cell])]);

        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();

        let decoded_table = result.paragraphs[0].runs[0].content.as_table().unwrap();
        assert_eq!(decoded_table.rows.len(), 1);
        assert_eq!(decoded_table.rows[0].cells.len(), 1);
        assert_eq!(
            decoded_table.rows[0].cells[0].paragraphs[0].runs[0].content.as_text(),
            Some("Hello"),
        );
        assert_eq!(decoded_table.rows[0].cells[0].width.as_i32(), 5000);
    }

    #[test]
    fn table_page_break_and_repeat_header_roundtrip() {
        let cell = TableCell::new(vec![text_paragraph("Hello", 0, 0)], HwpUnit::new(5000).unwrap());
        let table = Table::new(vec![TableRow::new(vec![cell]).with_header(true)])
            .with_page_break(hwpforge_core::table::TablePageBreak::Table)
            .with_repeat_header(false);

        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"pageBreak="TABLE""#), "missing table pageBreak override");
        assert!(xml.contains(r#"repeatHeader="0""#), "missing repeatHeader override");
        assert!(xml.contains(r#"header="1""#), "missing header row marker");

        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        let decoded_table = result.paragraphs[0].runs[0].content.as_table().unwrap();
        assert_eq!(decoded_table.page_break, hwpforge_core::table::TablePageBreak::Table);
        assert!(!decoded_table.repeat_header);
        assert!(decoded_table.rows[0].is_header);
    }

    #[test]
    fn image_roundtrip_via_decoder() {
        let img = Image::new(
            "BinData/photo.png",
            HwpUnit::new(10000).unwrap(),
            HwpUnit::new(5000).unwrap(),
            ImageFormat::Png,
        );
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::image(img, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();

        let decoded_img = result.paragraphs[0].runs[0].content.as_image().unwrap();
        // binaryItemIDRef is "photo" (extension stripped by encoder),
        // so decoder reconstructs path as "BinData/photo"
        assert_eq!(decoded_img.path, "BinData/photo");
        assert_eq!(decoded_img.width.as_i32(), 10000);
        assert_eq!(decoded_img.height.as_i32(), 5000);
        let placement = decoded_img.placement.as_ref().expect("placement should roundtrip");
        assert!(placement.treat_as_char);
        assert_eq!(placement.text_wrap.as_hwpx_str().as_ref(), "TOP_AND_BOTTOM");
    }

    #[test]
    fn image_roundtrip_preserves_explicit_placement() {
        let img = Image::new(
            "BinData/photo.png",
            HwpUnit::new(10000).unwrap(),
            HwpUnit::new(5000).unwrap(),
            ImageFormat::Png,
        )
        .with_placement(ImagePlacement {
            text_wrap: ImageTextWrap::Square,
            text_flow: ImageTextFlow::RightOnly,
            treat_as_char: false,
            flow_with_text: true,
            allow_overlap: true,
            vert_rel_to: ImageRelativeTo::Paper,
            horz_rel_to: ImageRelativeTo::Page,
            vert_offset: HwpUnit::new(1200).unwrap(),
            horz_offset: HwpUnit::new(3400).unwrap(),
        });
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::image(img, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();

        let decoded_img = result.paragraphs[0].runs[0].content.as_image().unwrap();
        let placement = decoded_img.placement.as_ref().expect("placement should survive roundtrip");
        assert_eq!(placement.text_wrap, ImageTextWrap::Square);
        assert_eq!(placement.text_flow, ImageTextFlow::RightOnly);
        assert!(!placement.treat_as_char);
        assert!(placement.flow_with_text);
        assert!(placement.allow_overlap);
        assert_eq!(placement.vert_rel_to, ImageRelativeTo::Paper);
        assert_eq!(placement.horz_rel_to, ImageRelativeTo::Page);
        assert_eq!(placement.vert_offset.as_i32(), 1200);
        assert_eq!(placement.horz_offset.as_i32(), 3400);
    }

    // ── Header / Footer / PageNum encoder roundtrip ─────────────

    #[test]
    fn header_roundtrip_via_decoder() {
        use hwpforge_core::section::HeaderFooter;
        use hwpforge_foundation::ApplyPageType;

        let mut section = simple_section("Body text");
        section.headers.push(HeaderFooter::new(
            vec![text_paragraph("Header Content", 0, 0)],
            ApplyPageType::Both,
        ));

        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains("<hp:header"), "XML should contain header element");
        assert!(xml.contains("Header Content"), "XML should contain header text");

        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        let header = result.header.expect("decoded section should have header");
        assert_eq!(header.apply_page_type, ApplyPageType::Both);
        assert_eq!(header.paragraphs.len(), 1);
        assert_eq!(header.paragraphs[0].runs[0].content.as_text(), Some("Header Content"));
    }

    #[test]
    fn footer_roundtrip_via_decoder() {
        use hwpforge_core::section::HeaderFooter;
        use hwpforge_foundation::ApplyPageType;

        let mut section = simple_section("Body text");
        section.footers.push(HeaderFooter::new(
            vec![text_paragraph("Footer Content", 0, 0)],
            ApplyPageType::Even,
        ));

        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains("<hp:footer"), "XML should contain footer element");

        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        let footer = result.footer.expect("decoded section should have footer");
        assert_eq!(footer.apply_page_type, ApplyPageType::Even);
        assert_eq!(footer.paragraphs.len(), 1);
        assert_eq!(footer.paragraphs[0].runs[0].content.as_text(), Some("Footer Content"));
    }

    #[test]
    fn page_number_roundtrip_via_decoder() {
        use hwpforge_core::section::PageNumber;
        use hwpforge_foundation::{NumberFormatType, PageNumberPosition};

        let mut section = simple_section("Body text");
        section.page_number = Some(PageNumber::with_decoration(
            PageNumberPosition::BottomCenter,
            NumberFormatType::Digit,
            "- ".to_string(),
        ));

        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains("<hp:pageNum"), "XML should contain pageNum element");
        assert!(xml.contains(r#"pos="BOTTOM_CENTER""#), "XML should contain pos attribute");
        assert!(xml.contains(r#"formatType="DIGIT""#), "XML should contain formatType");
        assert!(xml.contains("sideChar=\"- \""), "XML should contain side char");

        // Full roundtrip: encoder outputs <hp:pageNum> which decoder parses back
        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        let pn = result.page_number.expect("decoded section should have page number");
        assert_eq!(pn.position, PageNumberPosition::BottomCenter);
        assert_eq!(pn.number_format, NumberFormatType::Digit);
        assert_eq!(pn.decoration, "- ");
    }

    #[test]
    fn find_ctrl_injection_point_handles_self_closing_colpr() {
        let xml = concat!(
            r#"<hs:sec><hp:p><hp:run><hp:secPr></hp:secPr>"#,
            r#"<hp:ctrl><hp:colPr id="" type="NEWSPAPER" layout="LEFT" colCount="1" sameSz="1" sameGap="0"/></hp:ctrl>"#,
            r#"<hp:t>body</hp:t></hp:run></hp:p></hs:sec>"#,
        );

        let pos = find_ctrl_injection_point(xml);
        let expected =
            xml.find("</hp:ctrl>").expect("colPr ctrl close must be present") + "</hp:ctrl>".len();
        assert_eq!(pos, expected, "insertion point must be after colPr ctrl");
    }

    #[test]
    fn page_number_ctrl_is_injected_after_colpr_ctrl() {
        use hwpforge_core::section::PageNumber;
        use hwpforge_foundation::{NumberFormatType, PageNumberPosition};

        let mut section = simple_section("Body text");
        section.page_number = Some(PageNumber::with_decoration(
            PageNumberPosition::BottomCenter,
            NumberFormatType::Digit,
            "".to_string(),
        ));

        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        let sec_pr_end = xml.find("</hp:secPr>").expect("secPr must be present");
        let col_pr_pos = xml.find("<hp:colPr").expect("colPr must be present");
        let page_num_pos = xml.find("<hp:pageNum").expect("pageNum must be present");

        assert!(col_pr_pos > sec_pr_end, "colPr must come after </hp:secPr>");
        assert!(page_num_pos > col_pr_pos, "pageNum must come after colPr");

        let after_col_pr = &xml[col_pr_pos..];
        assert!(
            after_col_pr.contains("</hp:ctrl><hp:ctrl><hp:pageNum"),
            "pageNum ctrl must be injected after colPr ctrl",
        );
    }

    #[test]
    fn header_and_footer_together_roundtrip() {
        use hwpforge_core::section::HeaderFooter;
        use hwpforge_foundation::ApplyPageType;

        let mut section = simple_section("Main body");
        section
            .headers
            .push(HeaderFooter::new(vec![text_paragraph("My Header", 0, 0)], ApplyPageType::Both));
        section
            .footers
            .push(HeaderFooter::new(vec![text_paragraph("My Footer", 0, 0)], ApplyPageType::Odd));

        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        let header = result.header.expect("should have header");
        let footer = result.footer.expect("should have footer");

        assert_eq!(header.paragraphs[0].runs[0].content.as_text(), Some("My Header"));
        assert_eq!(header.apply_page_type, ApplyPageType::Both);
        assert_eq!(footer.paragraphs[0].runs[0].content.as_text(), Some("My Footer"));
        assert_eq!(footer.apply_page_type, ApplyPageType::Odd);
    }

    // ── Footnote / Endnote / TextBox encoder tests ────────────

    #[test]
    fn footnote_encoding() {
        use hwpforge_core::control::Control;

        let footnote_para = text_paragraph("Note body", 0, 0);
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![
                    Run::text("Main text", CharShapeIndex::new(0)),
                    Run::control(
                        Control::Footnote {
                            inst_id: Some(hwpforge_core::ObjectId::new(42)),
                            paragraphs: vec![footnote_para],
                        },
                        CharShapeIndex::new(0),
                    ),
                ],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        assert!(xml.contains("<hp:ctrl>"), "missing ctrl wrapper");
        assert!(xml.contains("<hp:footNote"), "missing footNote element");
        assert!(xml.contains("<hp:t>Note body</hp:t>"), "missing footnote text");
        assert!(xml.contains(r#"instId="42""#), "missing instId attribute");
    }

    #[test]
    fn endnote_encoding() {
        use hwpforge_core::control::Control;

        let endnote_para = text_paragraph("End note", 0, 0);
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(
                    Control::Endnote { inst_id: None, paragraphs: vec![endnote_para] },
                    CharShapeIndex::new(0),
                )],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        assert!(xml.contains("<hp:endNote"), "missing endNote element");
        assert!(xml.contains("<hp:t>End note</hp:t>"), "missing endnote text");
    }

    #[test]
    fn textbox_encoding() {
        use hwpforge_core::control::Control;

        let tb_para = text_paragraph("Box text", 0, 0);
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(
                    Control::TextBox {
                        paragraphs: vec![tb_para],
                        width: HwpUnit::new(14000).unwrap(),
                        height: HwpUnit::new(8000).unwrap(),
                        horz_offset: 0,
                        vert_offset: 0,
                        caption: None,
                        style: None,
                        text_vertical_align: hwpforge_foundation::VerticalAlign::Top,
                    },
                    CharShapeIndex::new(0),
                )],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        assert!(xml.contains("<hp:rect"), "missing rect element");
        assert!(xml.contains("<hp:drawText"), "missing drawText element");
        assert!(xml.contains("<hp:t>Box text</hp:t>"), "missing textbox text");
        assert!(xml.contains(r#"width="14000""#), "missing width");
        assert!(xml.contains(r#"height="8000""#), "missing height");
        assert!(xml.contains(r#"treatAsChar="1""#), "inline textbox should have treatAsChar=1");
    }

    #[test]
    fn footnote_roundtrip_via_decoder() {
        use hwpforge_core::control::Control;

        let footnote_para = text_paragraph("Roundtrip note", 0, 0);
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![
                    Run::text("Before", CharShapeIndex::new(0)),
                    Run::control(
                        Control::Footnote {
                            inst_id: Some(hwpforge_core::ObjectId::new(7)),
                            paragraphs: vec![footnote_para],
                        },
                        CharShapeIndex::new(1),
                    ),
                ],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );

        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();

        // Find the footnote run in decoded output
        let all_runs = &result.paragraphs[0].runs;
        let footnote_run = all_runs
            .iter()
            .find(|r| r.content.is_control())
            .expect("no control run in decoded output");

        match &footnote_run.content {
            RunContent::Control(ctrl) => match ctrl.as_ref() {
                Control::Footnote { inst_id, paragraphs } => {
                    assert_eq!(*inst_id, Some(hwpforge_core::ObjectId::new(7)));
                    assert_eq!(paragraphs.len(), 1);
                    assert_eq!(paragraphs[0].runs[0].content.as_text(), Some("Roundtrip note"));
                }
                other => panic!("expected Footnote, got {other:?}"),
            },
            _ => panic!("expected Control"),
        }
    }

    #[test]
    fn endnote_roundtrip_via_decoder() {
        use hwpforge_core::control::Control;

        let endnote_para = text_paragraph("Endnote roundtrip", 0, 0);
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(
                    Control::Endnote { inst_id: None, paragraphs: vec![endnote_para] },
                    CharShapeIndex::new(0),
                )],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );

        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();

        let ctrl_run = result.paragraphs[0]
            .runs
            .iter()
            .find(|r| r.content.is_control())
            .expect("no control run");

        match &ctrl_run.content {
            RunContent::Control(ctrl) => match ctrl.as_ref() {
                Control::Endnote { paragraphs, .. } => {
                    assert_eq!(paragraphs[0].runs[0].content.as_text(), Some("Endnote roundtrip"));
                }
                other => panic!("expected Endnote, got {other:?}"),
            },
            _ => panic!("expected Control"),
        }
    }

    #[test]
    fn textbox_roundtrip_via_decoder() {
        use hwpforge_core::control::Control;

        let tb_para = text_paragraph("Textbox roundtrip", 0, 0);
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(
                    Control::TextBox {
                        paragraphs: vec![tb_para],
                        width: HwpUnit::new(14000).unwrap(),
                        height: HwpUnit::new(8000).unwrap(),
                        horz_offset: 0,
                        vert_offset: 0,
                        caption: None,
                        style: None,
                        text_vertical_align: hwpforge_foundation::VerticalAlign::Top,
                    },
                    CharShapeIndex::new(0),
                )],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );

        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();

        let ctrl_run = result.paragraphs[0]
            .runs
            .iter()
            .find(|r| r.content.is_control())
            .expect("no control run");

        match &ctrl_run.content {
            RunContent::Control(ctrl) => match ctrl.as_ref() {
                Control::TextBox {
                    paragraphs, width, height, horz_offset, vert_offset, ..
                } => {
                    assert_eq!(paragraphs[0].runs[0].content.as_text(), Some("Textbox roundtrip"));
                    assert_eq!(width.as_i32(), 14000);
                    assert_eq!(height.as_i32(), 8000);
                    assert_eq!(*horz_offset, 0);
                    assert_eq!(*vert_offset, 0);
                }
                other => panic!("expected TextBox, got {other:?}"),
            },
            _ => panic!("expected Control"),
        }
    }

    #[test]
    fn xml_special_chars_escaped_in_header() {
        use hwpforge_core::section::HeaderFooter;
        use hwpforge_foundation::ApplyPageType;

        let mut section = simple_section("Body");
        section.headers.push(HeaderFooter::new(
            vec![text_paragraph("A & B < C > D", 0, 0)],
            ApplyPageType::Both,
        ));

        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains("A &amp; B &lt; C &gt; D"), "special chars must be escaped");

        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        let header = result.header.expect("should have header");
        assert_eq!(header.paragraphs[0].runs[0].content.as_text(), Some("A & B < C > D"),);
    }

    // ── Hyperlink helper unit tests ──────────────────────────────

    #[test]
    fn build_hyperlink_run_xml_basic() {
        let xml = build_hyperlink_run_xml("Click here", "https://example.com", 0, 0);
        assert!(xml.starts_with(r#"<hp:run charPrIDRef="0">"#));
        assert!(xml.contains(r#"type="HYPERLINK""#));
        assert!(xml.contains(r#"id="1100000000""#), "must have unique id");
        assert!(xml.contains(r#"fieldid="1628000000""#), "fieldid must be non-zero");
        assert!(!xml.contains(r#"fieldid="0""#), "fieldid must never be 0");
        assert!(xml.contains(r#"editable="0""#), "editable must be numeric");
        assert!(xml.contains(r#"dirty="0""#), "dirty must be numeric");
        assert!(xml.contains(r#"metaTag="""#));
        assert!(xml.contains(r#"<hp:stringParam name="Path">https://example.com</hp:stringParam>"#));
        assert!(xml.contains("<hp:t>Click here</hp:t>"));
        assert!(xml.contains(r#"<hp:fieldEnd beginIDRef="1100000000" fieldid="1628000000"/>"#));
        assert!(xml.ends_with("</hp:run>"));
    }

    #[test]
    fn build_hyperlink_run_xml_escapes_special_chars() {
        let xml = build_hyperlink_run_xml("A & B < C", "https://example.com?a=1&b=2", 2, 5);
        assert!(xml.contains(r#"charPrIDRef="2""#));
        assert!(xml.contains(r#"id="1100000005""#), "unique id for field_id=5");
        assert!(xml.contains(r#"fieldid="1628000005""#), "non-zero fieldid for field_id=5");
        assert!(xml.contains("https://example.com?a=1&amp;b=2"), "URL ampersand must be escaped");
        assert!(
            xml.contains("<hp:t>A &amp; B &lt; C</hp:t>"),
            "display text special chars must be escaped"
        );
    }

    #[test]
    fn multiple_hyperlinks_get_unique_field_ids() {
        use hwpforge_core::control::Control;

        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![
                    Run::control(
                        Control::hyperlink("Link 1", "https://one.com"),
                        CharShapeIndex::new(0),
                    ),
                    Run::control(
                        Control::hyperlink("Link 2", "https://two.com"),
                        CharShapeIndex::new(0),
                    ),
                ],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        // First hyperlink: id=1100000000, fieldid=1628000000
        assert!(xml.contains(r#"<hp:fieldEnd beginIDRef="1100000000" fieldid="1628000000"/>"#));
        // Second hyperlink: id=1100000001, fieldid=1628000001
        assert!(xml.contains(r#"<hp:fieldEnd beginIDRef="1100000001" fieldid="1628000001"/>"#));
        // Both URLs present
        assert!(xml.contains("https://one.com"));
        assert!(xml.contains("https://two.com"));
        // Both display texts present
        assert!(xml.contains("<hp:t>Link 1</hp:t>"));
        assert!(xml.contains("<hp:t>Link 2</hp:t>"));
        // No leftover markers
        assert!(!xml.contains("__HWPFORGE_HYPERLINK_"));
    }

    // ── style_id encoding tests ──────────────────────────────────

    #[test]
    fn style_id_none_encodes_as_zero() {
        let section = simple_section("body text");
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"styleIDRef="0""#), "None style_id should encode as styleIDRef=0");
    }

    #[test]
    fn style_id_some_encodes_correctly() {
        use hwpforge_foundation::StyleIndex;
        let para = Paragraph::with_runs(
            vec![Run::text("heading", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )
        .with_style(StyleIndex::new(2));
        let section = Section::with_paragraphs(vec![para], PageSettings::a4());
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(
            xml.contains(r#"styleIDRef="2""#),
            "style_id=Some(2) should encode as styleIDRef=2"
        );
    }

    #[test]
    fn decoder_nonzero_style_id_ref_roundtrips() {
        use hwpforge_foundation::StyleIndex;
        let para = Paragraph::with_runs(
            vec![Run::text("outline", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )
        .with_style(StyleIndex::new(3));
        let section = Section::with_paragraphs(vec![para], PageSettings::a4());
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        assert_eq!(result.paragraphs[0].style_id, Some(StyleIndex::new(3)));
    }

    #[test]
    fn decoder_zero_style_id_ref_gives_none() {
        let section = simple_section("normal");
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        assert_eq!(result.paragraphs[0].style_id, None);
    }

    // ── TextDirection tests ──────────────────────────────────────

    #[test]
    fn text_direction_horizontal_is_default() {
        let section = simple_section("가로쓰기");
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(
            xml.contains(r#"textDirection="HORIZONTAL""#),
            "default section should use HORIZONTAL"
        );
        assert!(
            xml.contains(r#"textVerticalWidthHead="0""#),
            "horizontal should have textVerticalWidthHead=0"
        );
    }

    #[test]
    fn text_direction_vertical_encodes_correctly() {
        let section =
            Section::with_paragraphs(vec![text_paragraph("세로쓰기", 0, 0)], PageSettings::a4())
                .with_text_direction(TextDirection::Vertical);
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(
            xml.contains(r#"textDirection="VERTICAL""#),
            "vertical section should use VERTICAL"
        );
        assert!(
            xml.contains(r#"textVerticalWidthHead="1""#),
            "vertical should have textVerticalWidthHead=1"
        );
    }

    #[test]
    fn text_direction_vertical_all_encodes_correctly() {
        let section = Section::with_paragraphs(
            vec![text_paragraph("세로쓰기 영문 세움", 0, 0)],
            PageSettings::a4(),
        )
        .with_text_direction(TextDirection::VerticalAll);
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(
            xml.contains(r#"textDirection="VERTICALALL""#),
            "verticalall section should use VERTICALALL"
        );
        assert!(
            xml.contains(r#"textVerticalWidthHead="1""#),
            "verticalall should have textVerticalWidthHead=1"
        );
    }

    #[test]
    fn text_direction_vertical_roundtrips() {
        let section = Section::with_paragraphs(
            vec![text_paragraph("세로쓰기 roundtrip", 0, 0)],
            PageSettings::a4(),
        )
        .with_text_direction(TextDirection::Vertical);
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        assert_eq!(result.text_direction, TextDirection::Vertical);
    }

    // ── Landscape / Gutter encoding ──────────────────────────────

    #[test]
    fn landscape_encodes_as_narrowly() {
        let ps = PageSettings { landscape: true, ..PageSettings::a4() };
        let section = Section::with_paragraphs(vec![text_paragraph("landscape", 0, 0)], ps);
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"landscape="NARROWLY""#), "landscape=true must encode as NARROWLY");
    }

    #[test]
    fn portrait_encodes_as_widely() {
        let ps = PageSettings { landscape: false, ..PageSettings::a4() };
        let section = Section::with_paragraphs(vec![text_paragraph("portrait", 0, 0)], ps);
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"landscape="WIDELY""#), "landscape=false must encode as WIDELY");
    }

    #[test]
    fn landscape_roundtrips() {
        let ps = PageSettings { landscape: true, ..PageSettings::a4() };
        let section = Section::with_paragraphs(vec![text_paragraph("land", 0, 0)], ps);
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        assert!(result.page_settings.unwrap().landscape, "landscape must roundtrip");
    }

    #[test]
    fn gutter_type_left_right_encodes() {
        use hwpforge_foundation::GutterType;
        let ps = PageSettings { gutter_type: GutterType::LeftRight, ..PageSettings::a4() };
        let section = Section::with_paragraphs(vec![text_paragraph("gutter", 0, 0)], ps);
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"gutterType="LEFT_RIGHT""#));
    }

    #[test]
    fn gutter_type_top_only_encodes() {
        use hwpforge_foundation::GutterType;
        let ps = PageSettings { gutter_type: GutterType::TopOnly, ..PageSettings::a4() };
        let section = Section::with_paragraphs(vec![text_paragraph("gutter", 0, 0)], ps);
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"gutterType="TOP_ONLY""#));
    }

    // ── Visibility encoding ──────────────────────────────────────

    #[test]
    fn visibility_defaults_encode() {
        let section = simple_section("text");
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        // Default visibility: all zeros, SHOW_ALL
        assert!(xml.contains(r#"hideFirstHeader="0""#));
        assert!(xml.contains(r#"hideFirstFooter="0""#));
        assert!(xml.contains(r#"showLineNumber="0""#));
        assert!(xml.contains(r#"border="SHOW_ALL""#));
        assert!(xml.contains(r#"fill="SHOW_ALL""#));
    }

    #[test]
    fn visibility_custom_encodes() {
        use hwpforge_core::section::Visibility;
        use hwpforge_foundation::ShowMode;
        let mut section = simple_section("text");
        section.visibility = Some(Visibility {
            hide_first_header: true,
            hide_first_footer: true,
            hide_first_master_page: false,
            hide_first_page_num: true,
            hide_first_empty_line: false,
            show_line_number: true,
            border: ShowMode::HideAll,
            fill: ShowMode::ShowOdd,
        });
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"hideFirstHeader="1""#));
        assert!(xml.contains(r#"hideFirstFooter="1""#));
        assert!(xml.contains(r#"hideFirstMasterPage="0""#));
        assert!(xml.contains(r#"hideFirstPageNum="1""#));
        assert!(xml.contains(r#"showLineNumber="1""#));
        assert!(xml.contains(r#"border="HIDE_ALL""#));
        assert!(xml.contains(r#"fill="SHOW_ODD""#));
    }

    #[test]
    fn show_mode_to_hwpx_covers_all_variants() {
        use hwpforge_foundation::ShowMode;
        assert_eq!(show_mode_to_hwpx(ShowMode::ShowAll), "SHOW_ALL");
        assert_eq!(show_mode_to_hwpx(ShowMode::HideAll), "HIDE_ALL");
        assert_eq!(show_mode_to_hwpx(ShowMode::ShowOdd), "SHOW_ODD");
        assert_eq!(show_mode_to_hwpx(ShowMode::ShowEven), "SHOW_EVEN");
    }

    // ── LineNumberShape encoding ─────────────────────────────────

    #[test]
    fn line_number_shape_encodes() {
        use hwpforge_core::section::LineNumberShape;
        let mut section = simple_section("text");
        section.line_number_shape = Some(LineNumberShape {
            restart_type: 1,
            count_by: 5,
            distance: HwpUnit::new(1000).unwrap(),
            start_number: 3,
        });
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"restartType="1""#));
        assert!(xml.contains(r#"countBy="5""#));
        assert!(xml.contains(r#"distance="1000""#));
        assert!(xml.contains(r#"startNumber="3""#));
    }

    #[test]
    fn line_number_shape_defaults_encode() {
        // Section with no line_number_shape uses all-zero defaults
        let section = simple_section("text");
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"restartType="0""#));
        assert!(xml.contains(r#"countBy="0""#));
        assert!(xml.contains(r#"startNumber="0""#));
    }

    // ── PageBorderFillEntry encoding ─────────────────────────────

    #[test]
    fn page_border_fill_defaults_encode_three_entries() {
        let section = simple_section("text");
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        // Default: BOTH, EVEN, ODD entries
        assert!(xml.contains(r#"type="BOTH""#));
        assert!(xml.contains(r#"type="EVEN""#));
        assert!(xml.contains(r#"type="ODD""#));
        assert!(xml.contains("<hp:pageBorderFill"));
    }

    #[test]
    fn page_border_fill_custom_encodes() {
        use hwpforge_core::section::PageBorderFillEntry;
        let mut section = simple_section("text");
        section.page_border_fills = Some(vec![PageBorderFillEntry {
            apply_type: "BOTH".to_string(),
            border_fill_id: 5,
            text_border: "PAGE".to_string(),
            header_inside: true,
            footer_inside: false,
            fill_area: "PAGE".to_string(),
            offset: [
                HwpUnit::new(500).unwrap(),
                HwpUnit::new(600).unwrap(),
                HwpUnit::new(700).unwrap(),
                HwpUnit::new(800).unwrap(),
            ],
        }]);
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"borderFillIDRef="5""#));
        assert!(xml.contains(r#"textBorder="PAGE""#));
        assert!(xml.contains(r#"headerInside="1""#));
        assert!(xml.contains(r#"footerInside="0""#));
        assert!(xml.contains(r#"fillArea="PAGE""#));
        assert!(xml.contains(r#"left="500""#));
        assert!(xml.contains(r#"right="600""#));
    }

    // ── BeginNum encoding ────────────────────────────────────────

    #[test]
    fn begin_num_encodes_in_startnum() {
        use hwpforge_core::section::BeginNum;
        let mut section = simple_section("text");
        section.begin_num =
            Some(BeginNum { page: 3, footnote: 2, endnote: 1, pic: 4, tbl: 5, equation: 6 });
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"page="3""#));
        assert!(xml.contains(r#"pic="4""#));
        assert!(xml.contains(r#"tbl="5""#));
        assert!(xml.contains(r#"equation="6""#));
        // footnote/endnote appear in footNotePr/endNotePr
        assert!(xml.contains(r#"newNum="2""#)); // footnote
        assert!(xml.contains(r#"newNum="1""#)); // endnote
    }

    #[test]
    fn begin_num_none_defaults_to_zero_in_startnum() {
        let section = simple_section("text");
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        // When begin_num is None, startNum defaults page/pic/tbl/equation to 0
        assert!(xml.contains(r#"<hp:startNum pageStartsOn="BOTH" page="0""#));
    }

    // ── MasterPage encoding ──────────────────────────────────────

    #[test]
    fn master_page_encoding_produces_xml_file() {
        use hwpforge_core::section::MasterPage;
        use hwpforge_foundation::ApplyPageType;
        let mut section = simple_section("body");
        section.master_pages =
            Some(vec![MasterPage::new(ApplyPageType::Both, vec![text_paragraph("bg text", 0, 0)])]);
        let result = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap();
        assert_eq!(result.master_pages.len(), 1);
        let (path, xml) = &result.master_pages[0];
        assert_eq!(path, "Contents/masterpage0.xml");
        assert!(xml.contains("<masterPage"), "masterPage root element required");
        assert!(xml.contains(r#"type="BOTH""#));
        assert!(xml.contains("<hp:subList"), "subList required");
        assert!(xml.contains("<hp:t>bg text</hp:t>"), "master page text content");
    }

    #[test]
    fn master_page_offset_applies_to_index() {
        use hwpforge_core::section::MasterPage;
        use hwpforge_foundation::ApplyPageType;
        let mut section = simple_section("body");
        section.master_pages =
            Some(vec![MasterPage::new(ApplyPageType::Even, vec![text_paragraph("mp", 0, 0)])]);
        // offset=5 → masterpage5
        let result = encode_section(&section, 0, 0, 5, 0, EncodeOptions::default()).unwrap();
        let (path, xml) = &result.master_pages[0];
        assert_eq!(path, "Contents/masterpage5.xml");
        assert!(xml.contains(r#"id="masterpage5""#));
        assert!(xml.contains(r#"type="EVEN""#));
    }

    #[test]
    fn masterpage_refs_in_secpr() {
        use hwpforge_core::section::MasterPage;
        use hwpforge_foundation::ApplyPageType;
        let mut section = simple_section("body");
        section.master_pages =
            Some(vec![MasterPage::new(ApplyPageType::Both, vec![text_paragraph("mp", 0, 0)])]);
        let result = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap();
        assert!(
            result.xml.contains(r#"<hp:masterPage idRef="masterpage0"/>"#),
            "secPr must reference the master page"
        );
    }

    // ── page_break / column_break encoding ───────────────────────

    #[test]
    fn page_break_encodes_as_one() {
        let mut para = text_paragraph("break here", 0, 0);
        para.page_break = true;
        let section =
            Section::with_paragraphs(vec![text_paragraph("first", 0, 0), para], PageSettings::a4());
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"pageBreak="1""#), "page_break=true must encode as pageBreak=1");
    }

    #[test]
    fn column_break_encodes_as_one() {
        let mut para = text_paragraph("col break", 0, 0);
        para.column_break = true;
        let section =
            Section::with_paragraphs(vec![text_paragraph("first", 0, 0), para], PageSettings::a4());
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"columnBreak="1""#));
    }

    #[test]
    fn page_break_roundtrips() {
        let mut para = text_paragraph("break", 0, 0);
        para.page_break = true;
        let section =
            Section::with_paragraphs(vec![text_paragraph("first", 0, 0), para], PageSettings::a4());
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        assert!(!result.paragraphs[0].page_break, "first para must NOT have page_break");
        assert!(result.paragraphs[1].page_break, "second para must have page_break");
    }

    // ── Bookmark (Point) encoding ────────────────────────────────

    #[test]
    fn bookmark_point_encoding() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::BookmarkType;
        let ctrl =
            Control::Bookmark { name: "mymark".to_string(), bookmark_type: BookmarkType::Point };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains("<hp:bookmark"), "must emit bookmark element");
        assert!(xml.contains(r#"name="mymark""#));
    }

    // ── Bookmark SpanStart/SpanEnd encoding ──────────────────────

    #[test]
    fn bookmark_span_encoding() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::BookmarkType;
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![
                    Run::control(
                        Control::Bookmark {
                            name: "span1".to_string(),
                            bookmark_type: BookmarkType::SpanStart,
                        },
                        CharShapeIndex::new(0),
                    ),
                    Run::text("covered text", CharShapeIndex::new(0)),
                    Run::control(
                        Control::Bookmark {
                            name: "span1".to_string(),
                            bookmark_type: BookmarkType::SpanEnd,
                        },
                        CharShapeIndex::new(0),
                    ),
                ],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        // SpanStart produces fieldBegin type="BOOKMARK"
        assert!(xml.contains(r#"type="BOOKMARK""#), "BOOKMARK fieldBegin required");
        assert!(xml.contains(r#"name="span1""#));
        // SpanEnd produces fieldEnd
        assert!(xml.contains("<hp:fieldEnd"), "fieldEnd required for SpanEnd");
        assert!(!xml.contains("__HWPBM_"), "no leftover SpanStart marker");
        assert!(!xml.contains("__HWPBE_"), "no leftover SpanEnd marker");
    }

    #[test]
    fn bookmark_span_end_without_start_is_skipped() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::BookmarkType;
        // SpanEnd without matching SpanStart should be silently skipped
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![
                    Run::text("text", CharShapeIndex::new(0)),
                    Run::control(
                        Control::Bookmark {
                            name: "orphan".to_string(),
                            bookmark_type: BookmarkType::SpanEnd,
                        },
                        CharShapeIndex::new(0),
                    ),
                ],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        // Should not panic or error
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains("<hp:t>text</hp:t>"), "text must still be present");
    }

    // ── IndexMark encoding ────────────────────────────────────────

    #[test]
    fn indexmark_encoding() {
        use hwpforge_core::control::Control;
        let ctrl = Control::IndexMark {
            primary: "색인항목".to_string(),
            secondary: Some("부항목".to_string()),
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains("<hp:indexmark"), "indexmark element required");
        assert!(xml.contains("색인항목"), "primary key must be present");
        assert!(xml.contains("부항목"), "secondary key must be present");
    }

    // ── Field encoding ────────────────────────────────────────────

    #[test]
    fn field_pagenum_produces_autonum() {
        // Wave 12n: PageNum moved from FieldType::PageNum to Control::InlinePageNumber.
        use hwpforge_core::control::{Control, InlinePageKind};
        let ctrl = Control::InlinePageNumber { kind: InlinePageKind::CurrentPage };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(
            xml.contains(r#"<hp:autoNum num="1" numType="PAGE">"#),
            "autoNum for InlinePageNumber"
        );
        assert!(xml.contains("<hp:autoNumFormat"), "autoNumFormat required");
    }

    #[test]
    fn field_date_produces_summary_type() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::ModifiedTime,
            hint_text: None,
            help_text: None,
            name: None,
            display_text: String::new(),
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        // Date uses SUMMERY type (한글 typo)
        assert!(xml.contains(r#"type="SUMMERY""#), "Date field must use SUMMERY type");
        assert!(xml.contains(r#"fieldid="628321650""#), "Date field must use fieldid 628321650");
        assert!(xml.contains("$modifiedtime"), "Date field Command must be $modifiedtime");
    }

    #[test]
    fn field_time_produces_summary_createtime() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::CreatedTime,
            hint_text: None,
            help_text: None,
            name: None,
            display_text: String::new(),
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"type="SUMMERY""#));
        assert!(xml.contains("$createtime"));
    }

    #[test]
    fn field_docsummary_produces_summary_author() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::Author,
            hint_text: None,
            help_text: None,
            name: None,
            display_text: String::new(),
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"type="SUMMERY""#));
        assert!(xml.contains("$author"));
    }

    /// #120/#136: a SUMMERY field with a cached value must emit it in the
    /// body between fieldBegin/fieldEnd (an empty `<hp:t/>` body triggers
    /// 한컴's "낮은 보안 수준 복구" warning), and the value must survive a
    /// Core → HWPX → Core round-trip.
    #[test]
    fn summary_cached_value_emitted_in_body_and_roundtrips() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::Author,
            hint_text: None,
            help_text: None,
            name: None,
            display_text: "hanyul".to_string(),
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        // Body carries the cached value, NOT an empty `<hp:t/>`.
        assert!(xml.contains("<hp:t>hanyul</hp:t>"), "cached value missing from body: {xml}");
        // Round-trip: decode the field back and confirm display_text survives.
        let decoded = lossy_roundtrip_decode_first_control(Control::Field {
            field_type: FieldType::Author,
            hint_text: None,
            help_text: None,
            name: None,
            display_text: "hanyul".to_string(),
        });
        match decoded {
            Control::Field { display_text, .. } => {
                assert_eq!(display_text, "hanyul", "display_text lost on round-trip");
            }
            other => panic!("expected Control::Field, got {other:?}"),
        }
    }

    /// #120/#136: a PATH field also carries its cached resolved path in the
    /// body (was hardcoded empty before the fix).
    #[test]
    fn path_field_cached_value_emitted_in_body() {
        use hwpforge_core::control::{Control, PathFieldCommand};
        let ctrl = Control::PathField {
            command: PathFieldCommand::PathAndFileName,
            display_text: "/tmp/doc.hwpx".to_string(),
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"type="PATH""#));
        assert!(xml.contains("<hp:t>/tmp/doc.hwpx</hp:t>"), "PATH cached value missing: {xml}");
    }

    #[test]
    fn field_userinfo_produces_summary_lastsaveby() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::LastSavedBy,
            hint_text: None,
            help_text: None,
            name: None,
            display_text: String::new(),
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"type="SUMMERY""#));
        assert!(xml.contains("$lastsaveby"));
    }

    #[test]
    fn field_clickhere_produces_correct_format() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::ClickHere,
            hint_text: Some("클릭하세요".to_string()),
            help_text: Some("도움말".to_string()),
            name: None,
            display_text: String::new(),
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"type="CLICK_HERE""#), "ClickHere field type");
        assert!(xml.contains(r#"fieldid="627272811""#), "ClickHere fieldid");
        assert!(xml.contains("클릭하세요"), "hint text must appear");
    }

    // ── Wave 12n LOSSY-policy round-trip tests ──────────────────────
    //
    // These tests pin the *intentional* lossy mapping documented in the
    // encoder arms for DateCodeField / PathField / UnknownSummary. They
    // exist so a future encoder change that silently fixes round-trip
    // (e.g. switching to a different HWPX representation) is caught and
    // the lossy-policy comments can be updated rather than left stale.

    #[test]
    fn lossy_datecodefield_emits_summary_token() {
        // %dte time-mode → $createtime SUMMERY (lossy; raw_command kept as display).
        use hwpforge_core::control::Control;
        let ctrl = Control::DateCodeField { is_time_mode: true, display_text: String::new() };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"type="SUMMERY""#), "DateCodeField surrogates SUMMERY");
        assert!(xml.contains("$createtime"), "time-mode → $createtime token");
    }

    #[test]
    fn pathfield_emits_native_path_wire() {
        // Wave 12n Step 6 — PathField now emits Hancom-native
        // `type="PATH"` with `Format=` param, distinct `fieldid`, and
        // `editable="0"`. Replaces the prior LOSSY SUMMERY surrogate
        // (`lossy_pathfield_emits_summary_with_raw_command`).
        use hwpforge_core::control::{Control, PathFieldCommand};
        let ctrl = Control::PathField {
            command: PathFieldCommand::PathAndFileName,
            display_text: String::new(),
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"type="PATH""#), "PathField must emit type=\"PATH\"");
        assert!(
            xml.contains(r#"fieldid="628121972""#),
            "PathField must use PATH fieldid 628121972 (not SUMMERY's 628321650)",
        );
        assert!(xml.contains(r#"editable="0""#), "PathField must be editable=\"0\"");
        assert!(
            xml.contains(r#"<hp:stringParam name="Format">$P$F</hp:stringParam>"#),
            "PathField must carry the command in the Format param (not Property)",
        );
        assert!(
            xml.contains(r#"<hp:stringParam name="Command">$P$F</hp:stringParam>"#),
            "PathField must also surface Command",
        );
        assert!(
            !xml.contains(r#"<hp:stringParam name="Property">$P$F</hp:stringParam>"#),
            "PathField must NOT emit the SUMMERY-style Property param",
        );
    }

    #[test]
    fn lossy_unknown_summary_carries_raw_token() {
        use hwpforge_core::control::Control;
        let ctrl =
            Control::UnknownSummary { token: "$company".to_string(), display_text: String::new() };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"type="SUMMERY""#));
        assert!(xml.contains("$company"), "unknown raw token must surface in SUMMERY");
    }

    // ── Wave 12n Phase 2 Medium 2 — actual encoder→decoder round-trip
    // assertions for the lossy policy. The three `lossy_*` tests above
    // only inspect the emission XML; these tests close the loop by
    // decoding the emitted XML and asserting what the encoder comments
    // claim the round-trip yields. If any of these flip, the lossy
    // comments need to be updated.

    /// Helper: encode a single-control section, decode it back, return
    /// the first decoded `Control`. Panics if shape changed.
    fn lossy_roundtrip_decode_first_control(
        ctrl: hwpforge_core::control::Control,
    ) -> hwpforge_core::control::Control {
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        let parsed =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .expect("decoder must accept its own encoder output");
        for para in &parsed.paragraphs {
            for run in &para.runs {
                if let RunContent::Control(c) = &run.content {
                    return (**c).clone();
                }
            }
        }
        panic!("no decoded Control in round-trip output");
    }

    #[test]
    fn lossy_roundtrip_datecodefield_time_becomes_createdtime() {
        use hwpforge_core::control::Control;
        let ctrl = Control::DateCodeField { is_time_mode: true, display_text: String::new() };
        let decoded = lossy_roundtrip_decode_first_control(ctrl);
        match decoded {
            Control::Field { field_type, .. } => {
                assert_eq!(
                    field_type,
                    hwpforge_foundation::FieldType::CreatedTime,
                    "%dte time-mode lossy round-trip must decode as CreatedTime",
                );
            }
            other => panic!("expected Field(CreatedTime), got {other:?}"),
        }
    }

    #[test]
    fn lossy_roundtrip_datecodefield_date_becomes_modifiedtime() {
        use hwpforge_core::control::Control;
        let ctrl = Control::DateCodeField { is_time_mode: false, display_text: String::new() };
        let decoded = lossy_roundtrip_decode_first_control(ctrl);
        match decoded {
            Control::Field { field_type, .. } => {
                assert_eq!(
                    field_type,
                    hwpforge_foundation::FieldType::ModifiedTime,
                    "%dte date-mode lossy round-trip must decode as ModifiedTime",
                );
            }
            other => panic!("expected Field(ModifiedTime), got {other:?}"),
        }
    }

    #[test]
    fn pathfield_roundtrip_preserves_command_lossless() {
        // Wave 12n Step 6 — replaces the prior LOSSY round-trip
        // (`lossy_roundtrip_pathfield_becomes_unknown_summary`). With
        // the new `type="PATH"` builder + decoder arm, all three typed
        // PathFieldCommand variants round-trip without value loss.
        use hwpforge_core::control::{Control, PathFieldCommand};
        for cmd in
            [PathFieldCommand::PathAndFileName, PathFieldCommand::Path, PathFieldCommand::FileName]
        {
            let decoded = lossy_roundtrip_decode_first_control(Control::PathField {
                command: cmd.clone(),
                display_text: String::new(),
            });
            match decoded {
                Control::PathField { command, .. } => {
                    assert_eq!(command, cmd, "PathField command must round-trip lossless");
                }
                other => {
                    panic!("expected Control::PathField({cmd:?}), got {other:?}");
                }
            }
        }
    }

    #[test]
    fn pathfield_unknown_command_roundtrips_as_unknown() {
        // A non-canonical `$X` Command should round-trip as
        // `PathFieldCommand::Unknown("$X")` (no silent collapse to
        // `UnknownSummary` like the prior LOSSY policy).
        use hwpforge_core::control::{Control, PathFieldCommand};
        let ctrl = Control::PathField {
            command: PathFieldCommand::Unknown("$X".to_string()),
            display_text: String::new(),
        };
        let decoded = lossy_roundtrip_decode_first_control(ctrl);
        match decoded {
            Control::PathField { command, .. } => match command {
                PathFieldCommand::Unknown(s) => assert_eq!(s, "$X"),
                other => panic!("expected PathFieldCommand::Unknown(\"$X\"), got {other:?}"),
            },
            other => panic!("expected Control::PathField, got {other:?}"),
        }
    }

    #[test]
    fn lossy_roundtrip_unknown_summary_preserves_token() {
        use hwpforge_core::control::Control;
        let ctrl =
            Control::UnknownSummary { token: "$company".to_string(), display_text: String::new() };
        let decoded = lossy_roundtrip_decode_first_control(ctrl);
        match decoded {
            Control::UnknownSummary { token, .. } => {
                assert_eq!(token, "$company", "unknown $token must round-trip verbatim");
            }
            other => panic!("expected UnknownSummary($company), got {other:?}"),
        }
    }

    // ── Wave 12n Phase 2 Step 7 — LOSSLESS round-trip gates ────────
    //
    // These pin the lossless-by-design path for the 5 SUMMERY tokens
    // and the 2 inline page-number kinds. They force the encoder
    // emission and the decoder parse to agree on the wire format —
    // if either side desyncs (e.g. encoder emits a new SUMMERY token
    // the decoder does not recognize, or decoder maps autoNum
    // numType="TOTAL_PAGE" to the wrong kind), one of these flips
    // before the encoder/decoder ship as a pair.
    //
    // The kind ↔ numType mapping (CurrentPage → PAGE, TotalPages →
    // TOTAL_PAGE) is symmetric across encoder/decoder, so a known kind
    // round-trips losslessly (the wire flag is no longer part of the
    // core IR — E6 slice C).

    #[test]
    fn roundtrip_summary_author_lossless() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::Author,
            hint_text: None,
            help_text: None,
            name: None,
            display_text: String::new(),
        };
        let decoded = lossy_roundtrip_decode_first_control(ctrl.clone());
        assert_eq!(decoded, ctrl, "SUMMERY $author must round-trip lossless");
    }

    #[test]
    fn roundtrip_summary_lastsavedby_lossless() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::LastSavedBy,
            hint_text: None,
            help_text: None,
            name: None,
            display_text: String::new(),
        };
        let decoded = lossy_roundtrip_decode_first_control(ctrl.clone());
        assert_eq!(decoded, ctrl, "SUMMERY $lastsaveby must round-trip lossless");
    }

    #[test]
    fn roundtrip_summary_createdtime_lossless() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::CreatedTime,
            hint_text: None,
            help_text: None,
            name: None,
            display_text: String::new(),
        };
        let decoded = lossy_roundtrip_decode_first_control(ctrl.clone());
        assert_eq!(decoded, ctrl, "SUMMERY $createtime must round-trip lossless");
    }

    #[test]
    fn roundtrip_summary_modifiedtime_lossless() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::ModifiedTime,
            hint_text: None,
            help_text: None,
            name: None,
            display_text: String::new(),
        };
        let decoded = lossy_roundtrip_decode_first_control(ctrl.clone());
        assert_eq!(decoded, ctrl, "SUMMERY $modifiedtime must round-trip lossless");
    }

    #[test]
    fn roundtrip_summary_title_lossless() {
        // Wave 12n new: Title was added to FieldType in Step 1.
        // No emission-only or parse-only test covers it; this is the
        // sole gate ensuring encoder ↔ decoder agree on `$title`.
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::Title,
            hint_text: None,
            help_text: None,
            name: None,
            display_text: String::new(),
        };
        let decoded = lossy_roundtrip_decode_first_control(ctrl.clone());
        assert_eq!(decoded, ctrl, "SUMMERY $title must round-trip lossless");
    }

    #[test]
    fn roundtrip_inline_pagenumber_currentpage_lossless() {
        use hwpforge_core::control::{Control, InlinePageKind};
        let ctrl = Control::InlinePageNumber { kind: InlinePageKind::CurrentPage };
        let decoded = lossy_roundtrip_decode_first_control(ctrl.clone());
        assert_eq!(decoded, ctrl, "autoNum PAGE must round-trip lossless");
    }

    #[test]
    fn roundtrip_inline_pagenumber_totalpages_lossless() {
        // Wave 12n architect review CRITICAL gate: TotalPages must not
        // collapse to CurrentPage in either direction. Encoder emits
        // numType="TOTAL_PAGE"; decoder maps it back to TotalPages
        // (kind preserved through the autoNum numType attribute).
        use hwpforge_core::control::{Control, InlinePageKind};
        let ctrl = Control::InlinePageNumber { kind: InlinePageKind::TotalPages };
        let decoded = lossy_roundtrip_decode_first_control(ctrl.clone());
        assert_eq!(decoded, ctrl, "autoNum TOTAL_PAGE must round-trip lossless");
    }

    #[test]
    fn roundtrip_inline_pagenumber_unknown_emits_no_autonum() {
        // Encoder skip path: InlinePageNumber{Unknown} must not
        // fabricate an autoNum. Decoder therefore sees no control.
        // If anyone collapses Unknown → CurrentPage, this flips.
        use hwpforge_core::control::{Control, InlinePageKind};
        let ctrl = Control::InlinePageNumber { kind: InlinePageKind::Unknown };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        // Note: `<hp:autoNumFormat>` is emitted unconditionally inside
        // `<hp:footNotePr>` / `<hp:endNotePr>` and is unrelated to
        // inline page numbers. Match the exact inline element form
        // `<hp:autoNum num=…>` instead.
        assert!(
            !xml.contains("<hp:autoNum num="),
            "InlinePageNumber{{Unknown}} must NOT emit <hp:autoNum num=...> (no fabrication)",
        );
    }

    /// Pins the `Clickhere:set:N:` self-referential N formula against
    /// the seven press-field instances observed across Wave 12l native
    /// fixtures. If the formula or the `rest` template ever drifts,
    /// this test catches it before HWPX→한컴 round-trip breaks
    /// silently.
    ///
    /// Expected N comes from `.docs/research/2026-06-02_clickhere_wire_dump.md`.
    #[test]
    fn clickhere_command_string_matches_hancom_fixture_n() {
        // (hint, help, hint_len, help_len, expected_N)
        let cases: &[(&str, &str, usize, usize, usize)] = &[
            // basic: hint-only, 23 한글 chars
            ("이곳을 마우스로 누르고 내용을 입력하세요.", "", 23, 0, 66),
            // with-help
            ("이메일 주소를 입력하세요", "예: user@example.com - 회사 이메일로 입력", 13, 32, 89),
            // empty-hint
            ("", "", 0, 0, 42),
            // multi #1
            ("이름 입력", "", 5, 0, 47),
            // multi #3 (with help)
            ("email@company.com", "회사 이메일을 입력하세요", 17, 13, 74),
            // named
            ("회사 이메일을 입력하세요", "user@company.com", 13, 16, 73),
        ];
        for (hint, help, hint_len, help_len, expected_n) in cases {
            let cmd = clickhere_command_string(hint, help, *hint_len, *help_len);
            assert!(
                cmd.starts_with(&format!("Clickhere:set:{expected_n}:")),
                "expected N={expected_n} for hint={hint:?} help={help:?}, got: {cmd:?}",
            );
            // Sanity: re-parse the embedded N to catch leading-zero or
            // off-by-one regressions in `saturating_sub`.
            let n_str = cmd.strip_prefix("Clickhere:set:").unwrap().split(':').next().unwrap();
            let n: usize = n_str.parse().expect("N must be a decimal integer");
            assert_eq!(n, *expected_n, "embedded N digits must match expected");
        }
    }

    // ── CrossRef encoding ─────────────────────────────────────────

    #[test]
    fn crossref_encoding() {
        use hwpforge_core::control::{Control, RefTarget};
        use hwpforge_foundation::{RefContentType, RefType};
        let ctrl = Control::CrossRef {
            target: RefTarget::Name("bookmark1".to_string()),
            ref_type: RefType::default(),
            content_type: RefContentType::default(),
            as_hyperlink: true,
            display_text: String::new(),
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"type="CROSSREF""#), "CROSSREF fieldBegin type");
        assert!(xml.contains("bookmark1"), "target name must appear");
        assert!(xml.contains("RefHyperLink"), "RefHyperLink param required");
        assert!(!xml.contains("__HWPXR_"), "no leftover CrossRef marker");
    }

    #[test]
    fn crossref_builders_emit_nonzero_matching_fieldid() {
        use hwpforge_foundation::{RefContentType, RefType};

        // HWP5 path: field_id=0 must NOT produce fieldid="0".
        let hwp5_xml = build_hwp5_crossref_run_xml(
            "bookmark1",
            "see bookmark1",
            RefType::default(),
            RefContentType::default(),
            true,
            0,
            0,
        );
        assert!(
            !hwp5_xml.contains(r#"fieldid="0""#),
            "HWP5 CROSSREF must never emit fieldid=0 (invalid in Hancom)"
        );
        assert!(
            hwp5_xml.contains(r#"fieldid="1928000000""#),
            "HWP5 CROSSREF fieldid must be the non-zero derived value"
        );
        // fieldBegin and fieldEnd fieldid must match each other.
        assert_eq!(
            hwp5_xml.matches(r#"fieldid="1928000000""#).count(),
            2,
            "fieldBegin and fieldEnd must share the same non-zero fieldid"
        );
        // beginIDRef must reference the `id` (begin_id), NOT the fieldid.
        assert!(
            hwp5_xml.contains(r#"<hp:fieldBegin id="1400000000""#),
            "HWP5 CROSSREF fieldBegin id must be the derived begin_id"
        );
        assert!(
            hwp5_xml.contains(r#"<hp:fieldEnd beginIDRef="1400000000" fieldid="1928000000"/>"#),
            "fieldEnd beginIDRef must reference id, not fieldid"
        );

        // Non-HWP5 path: same guarantees with its own distinct base.
        let core_xml = build_crossref_run_xml(
            &hwpforge_core::control::RefTarget::Name("bookmark1".to_string()),
            "see bookmark1",
            &RefType::default(),
            &RefContentType::default(),
            true,
            0,
            0,
        );
        // Wave 12m Phase 2 Step 4 fixup: fieldid is now the Hancom
        // `%xrf` ASCII magic constant (0x25787266 = 628650598), shared
        // across every CROSSREF instance in the document. Per-instance
        // identity is carried by `id` (begin_id) instead. Verified
        // against 11 native Hancom-authored .hwpx samples.
        assert!(!core_xml.contains(r#"fieldid="0""#), "Core CROSSREF must never emit fieldid=0");
        assert!(
            core_xml.contains(r#"fieldid="628650598""#),
            "Core CROSSREF fieldid must be the `%xrf` magic constant"
        );
        assert_eq!(
            core_xml.matches(r#"fieldid="628650598""#).count(),
            2,
            "fieldBegin and fieldEnd must share the `%xrf` magic fieldid"
        );
        assert!(
            core_xml.contains(r#"<hp:fieldEnd beginIDRef="1300000000" fieldid="628650598"/>"#),
            "fieldEnd beginIDRef must reference id, not fieldid"
        );
    }

    // ── Memo encoding ─────────────────────────────────────────────

    #[test]
    fn memo_encoding() {
        use hwpforge_core::control::Control;
        let ctrl = Control::Memo {
            content: vec![text_paragraph("Memo note", 0, 0)],
            anchor_runs: vec![Run::text("anchor", CharShapeIndex::new(0))],
            metadata: hwpforge_core::MemoMetadata::default(),
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"type="MEMO""#), "MEMO fieldBegin type");
        assert!(xml.contains("MemoShapeID"), "MemoShapeID param required");
        assert!(!xml.contains("__HWPME_"), "no leftover Memo marker");
    }

    /// Locks the child-before-parent ordering invariant that
    /// [`apply_run_xml_replacements`] relies on for `replacen`-equivalence.
    ///
    /// Models a nested control: a child (hyperlink) marker that is embedded
    /// inside a parent's (memo's) `real_xml`, with the child pair listed
    /// BEFORE the parent pair (the order the encoder actually produces). Under
    /// this ordering both the old sequential `replacen` loop and the new
    /// single-pass splice leave the nested child marker unreplaced — i.e. they
    /// are byte-identical. (Were the order inverted, `replacen` would resolve
    /// the nested marker and the splice would not — the divergence this guards.)
    #[test]
    fn apply_run_xml_replacements_child_before_parent_matches_replacen() {
        let child_marker = "__HWPHL_0_0__".to_string();
        let parent_marker = "__HWPME_1_0__".to_string();
        // Parent payload embeds the child marker (memo sublist containing a field).
        let parent_real = format!("<hp:fieldBegin/><hp:subList>{child_marker}</hp:subList>");
        // Only the parent marker is present in the base string; the child marker
        // appears only AFTER the parent is spliced in.
        let xml = format!("<p>before</p>{parent_marker}<p>after</p>");
        // Child-before-parent ordering, as the encoder emits it.
        let repls = vec![
            (child_marker.clone(), "<hp:run>RESOLVED-CHILD</hp:run>".to_string()),
            (parent_marker, parent_real),
        ];
        let single = apply_run_xml_replacements(xml.clone(), &repls);
        let reference = replacen_reference(xml, &repls);
        assert_eq!(single, reference, "splice must equal replacen under child-before-parent order");
        // Both leave the nested child marker unreplaced (documents the shared behavior).
        assert!(single.contains(&child_marker), "nested child marker stays unreplaced in both");
    }

    // ── Dutmal encoding ────────────────────────────────────────────

    #[test]
    fn dutmal_encoding() {
        use hwpforge_core::control::Control;
        use hwpforge_core::control::{DutmalAlign, DutmalPosition};
        use hwpforge_foundation::{CharShapeIndex as CSI, ParaShapeIndex as PSI};
        let ctrl = Control::Dutmal {
            main_text: "漢".to_string(),
            sub_text: "한".to_string(),
            position: DutmalPosition::Top,
            sz_ratio: 50,
            align: DutmalAlign::Center,
            metadata: hwpforge_core::DutmalMetadata::default(),
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![Run::control(ctrl, CSI::new(0))], PSI::new(0))],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains("<hp:dutmal"), "dutmal element required");
        assert!(xml.contains("漢"), "main text required");
        assert!(xml.contains("한"), "sub text required");
        assert!(xml.contains(r#"szRatio="50""#), "szRatio attribute required");
        assert!(xml.contains(r#"posType="TOP""#), "posType attribute required");
        assert!(xml.contains(r#"align="CENTER""#), "align attribute required");
    }

    #[test]
    fn dutmal_position_bottom_and_align_right() {
        use hwpforge_core::control::Control;
        use hwpforge_core::control::{DutmalAlign, DutmalPosition};
        let ctrl = Control::Dutmal {
            main_text: "A".to_string(),
            sub_text: "a".to_string(),
            position: DutmalPosition::Bottom,
            sz_ratio: 75,
            align: DutmalAlign::Right,
            metadata: hwpforge_core::DutmalMetadata::default(),
        };
        let xml_result =
            encode_dutmal_to_hx("A", "a", DutmalPosition::Bottom, 75, DutmalAlign::Right, 0);
        assert_eq!(xml_result.pos_type, "BOTTOM");
        assert_eq!(xml_result.align, "RIGHT");
        assert_eq!(xml_result.sz_ratio, 75);
        let _ = ctrl; // suppress unused warning
    }

    #[test]
    fn dutmal_position_left_encodes() {
        use hwpforge_core::control::{DutmalAlign, DutmalPosition};
        let hx = encode_dutmal_to_hx("X", "x", DutmalPosition::Left, 60, DutmalAlign::Left, 0);
        assert_eq!(hx.pos_type, "LEFT");
        assert_eq!(hx.align, "LEFT");
    }

    #[test]
    fn dutmal_position_right_encodes() {
        use hwpforge_core::control::{DutmalAlign, DutmalPosition};
        let hx = encode_dutmal_to_hx("X", "x", DutmalPosition::Right, 60, DutmalAlign::Center, 0);
        assert_eq!(hx.pos_type, "RIGHT");
    }

    // ── Compose encoding ───────────────────────────────────────────

    #[test]
    fn compose_encoding() {
        use hwpforge_core::control::Control;
        let ctrl = Control::Compose {
            compose_text: "AB".to_string(),
            circle_type: "CIRCLE".to_string(),
            char_sz: 100,
            compose_type: "COMPOSE".to_string(),
            char_pr_ids: vec![u32::MAX; 10],
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains("<hp:compose"), "compose element required");
        assert!(xml.contains(r#"charPrCnt="10""#), "10 charPr entries required");
        assert!(xml.contains("AB"), "compose text required");
    }

    #[test]
    fn encode_compose_has_ten_charpr_entries() {
        let hx = encode_compose_to_hx("AB", "CIRCLE", 100, "COMPOSE", &[u32::MAX; 10]);
        assert_eq!(hx.char_prs.len(), 10, "always 10 charPr entries");
        // All must have pr_id_ref = u32::MAX (HWPX sentinel)
        for cp in &hx.char_prs {
            assert_eq!(cp.pr_id_ref, u32::MAX);
        }
    }

    // ── Equation encoding ──────────────────────────────────────────

    #[test]
    fn equation_encoding() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::Color;
        let ctrl = Control::Equation {
            script: "{a} over {b}".to_string(),
            width: HwpUnit::new(10000).unwrap(),
            height: HwpUnit::new(5000).unwrap(),
            base_line: 80,
            text_color: Color::BLACK,
            font: "HCR Batang".to_string(),
            inst_id: None,
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains("<hp:equation"), "equation element required");
        assert!(xml.contains("{a} over {b}"), "equation script required");
        assert!(xml.contains(r#"width="10000""#));
        assert!(xml.contains(r#"height="5000""#));
        assert!(xml.contains(r#"baseLine="80""#));
        assert!(xml.contains(r#"textWrap="TOP_AND_BOTTOM""#));
        assert!(xml.contains(r#"flowWithText="1""#));
        assert!(xml.contains(r#"outMargin"#));
        assert!(xml.contains("수식입니다."), "equation shapeComment required");
    }

    // ── Multi-column encoding roundtrip ──────────────────────────

    #[test]
    fn two_column_equal_roundtrip() {
        use hwpforge_core::column::{ColumnDef, ColumnLayoutMode, ColumnSettings, ColumnType};
        let mut section = simple_section("two columns");
        section.column_settings = Some(ColumnSettings {
            column_type: ColumnType::Newspaper,
            layout_mode: ColumnLayoutMode::Left,
            columns: vec![
                ColumnDef { width: HwpUnit::ZERO, gap: HwpUnit::new(1134).unwrap() },
                ColumnDef { width: HwpUnit::ZERO, gap: HwpUnit::ZERO },
            ],
            col_line: None,
        });
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"colCount="2""#));
        assert!(xml.contains(r#"sameSz="1""#));
        assert!(xml.contains(r#"sameGap="1134""#));

        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        let cs = result.column_settings.expect("should have column_settings");
        assert_eq!(cs.columns.len(), 2);
    }

    #[test]
    fn three_column_variable_encodes() {
        use hwpforge_core::column::{ColumnDef, ColumnLayoutMode, ColumnSettings, ColumnType};
        let mut section = simple_section("three columns");
        section.column_settings = Some(ColumnSettings {
            column_type: ColumnType::Newspaper,
            layout_mode: ColumnLayoutMode::Right,
            columns: vec![
                ColumnDef { width: HwpUnit::new(10000).unwrap(), gap: HwpUnit::new(500).unwrap() },
                ColumnDef { width: HwpUnit::new(15000).unwrap(), gap: HwpUnit::new(500).unwrap() },
                ColumnDef { width: HwpUnit::new(10000).unwrap(), gap: HwpUnit::ZERO },
            ],
            col_line: None,
        });
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains(r#"colCount="3""#));
        assert!(xml.contains(r#"sameSz="0""#), "variable width must use sameSz=0");
        // Explicit hp:col children required
        assert!(xml.contains(r#"<hp:col"#));
    }

    #[test]
    fn two_column_with_separator_byte_matches_hancom_native_and_roundtrips() {
        use hwpforge_core::column::{ColumnLine, ColumnSettings};
        use hwpforge_foundation::{BorderLineType, Color, HwpUnit};

        // Reproduces the Hancom-native wire captured in
        // examples/hwp5_review/_verify/nativ-colline.hwpx:
        //   <hp:colPr ... sameGap="2268">
        //     <hp:colLine type="DOUBLE_SLIM" width="0.7 mm" color="#CA56A7"/>
        //   </hp:colPr>
        let mut section = simple_section("with separator");
        section.column_settings = Some(
            ColumnSettings::equal_columns(2, HwpUnit::new(2268).unwrap()).unwrap().with_separator(
                ColumnLine {
                    line_type: BorderLineType::DoubleSlim,
                    width: HwpUnit::from_mm(0.7).unwrap(),
                    color: Color::from_rgb(0xCA, 0x56, 0xA7),
                },
            ),
        );
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;

        // Byte-exact match against the Hancom-native colPr+colLine wire.
        assert!(
            xml.contains(
                r##"<hp:colPr id="" type="NEWSPAPER" layout="LEFT" colCount="2" sameSz="1" sameGap="2268"><hp:colLine type="DOUBLE_SLIM" width="0.7 mm" color="#CA56A7"/></hp:colPr>"##
            ),
            "must byte-match Hancom native colPr+colLine: {xml}"
        );

        // Round-trips back to a ColumnLine.
        let cs = crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
            .unwrap()
            .column_settings
            .expect("column settings");
        let cl = cs.col_line.expect("separator must survive round-trip");
        assert_eq!(cl.line_type, BorderLineType::DoubleSlim);
        assert_eq!(cl.color, Color::from_rgb(0xCA, 0x56, 0xA7));
        // 0.7 mm round-trips through HwpUnit within format precision.
        assert!((cl.width.to_mm() - 0.7).abs() < 0.01, "width drift: {}", cl.width.to_mm());
    }

    // ── build_autonum_run_xml ─────────────────────────────────────

    #[test]
    fn build_autonum_run_xml_current_page() {
        let xml = build_autonum_run_xml(3, hwpforge_core::control::InlinePageKind::CurrentPage)
            .expect("CurrentPage must encode");
        assert!(xml.contains(r#"charPrIDRef="3""#));
        assert!(xml.contains(r#"<hp:autoNum num="1" numType="PAGE">"#));
        assert!(xml.contains("<hp:autoNumFormat"));
        assert!(xml.contains(r#"type="DIGIT""#));
        assert!(xml.ends_with("</hp:run>"));
    }

    #[test]
    fn build_autonum_run_xml_total_pages() {
        // Wave 12n architect review CRITICAL: TotalPages must NOT collapse to PAGE.
        let xml = build_autonum_run_xml(3, hwpforge_core::control::InlinePageKind::TotalPages)
            .expect("TotalPages must encode");
        assert!(
            xml.contains(r#"numType="TOTAL_PAGE""#),
            "TotalPages must emit numType=TOTAL_PAGE, not PAGE"
        );
    }

    #[test]
    fn build_autonum_run_xml_unknown_skipped() {
        // Unknown flag values must not fabricate a numType.
        assert!(
            build_autonum_run_xml(3, hwpforge_core::control::InlinePageKind::Unknown).is_none(),
            "Unknown InlinePageKind must return None (caller skips)"
        );
    }

    // ── Hyperlink unsafe URL rejection ───────────────────────────

    #[test]
    fn unsafe_url_rejected() {
        use hwpforge_core::control::Control;
        let ctrl =
            Control::Hyperlink { text: "evil".to_string(), url: "javascript:alert(1)".to_string() };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let result = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default());
        assert!(result.is_err(), "javascript: URL must be rejected");
        match result.unwrap_err() {
            crate::error::HwpxError::InvalidStructure { detail } => {
                assert!(detail.contains("Unsafe URL"), "error must mention Unsafe URL");
            }
            other => panic!("expected InvalidStructure, got {other:?}"),
        }
    }

    #[test]
    fn mailto_url_is_safe() {
        use hwpforge_core::control::Control;
        let ctrl = Control::Hyperlink {
            text: "email".to_string(),
            url: "mailto:test@example.com".to_string(),
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let result = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default());
        assert!(result.is_ok(), "mailto: URL must be accepted");
    }

    #[test]
    fn schemeless_url_is_normalized_to_http() {
        use hwpforge_core::control::Control;
        // Real-world corpus case: 한글 stores schemeless government domains.
        // The encoder must not abort the whole document; it promotes the URL
        // to an http:// link instead.
        let ctrl = Control::Hyperlink {
            text: "산업부".to_string(),
            url: "www.motie.go.kr".to_string(),
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let result = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default())
            .expect("schemeless URL must be accepted");
        assert!(
            result.xml.contains("http://www.motie.go.kr"),
            "schemeless URL must be promoted to http://, got: {}",
            result.xml
        );
    }

    // ── Chart encoding ────────────────────────────────────────────

    #[test]
    fn chart_encoding_produces_chart_entry() {
        use hwpforge_core::chart::{ChartData, ChartGrouping, ChartType, LegendPosition};
        use hwpforge_core::control::Control;
        let ctrl = Control::Chart {
            chart_type: ChartType::Bar,
            data: ChartData::category(&["A", "B"], &[("Series1", [1.0, 2.0].as_slice())]),
            width: HwpUnit::new(10000).unwrap(),
            height: HwpUnit::new(8000).unwrap(),
            title: None,
            legend: LegendPosition::default(),
            grouping: ChartGrouping::Clustered,
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let result = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap();
        assert_eq!(result.charts.len(), 1, "one chart entry expected");
        let (path, xml) = &result.charts[0];
        assert!(path.starts_with("Chart/chart"), "chart path format");
        assert!(path.ends_with(".xml"), "chart path extension");
        assert!(!xml.is_empty(), "chart XML must not be empty");
    }

    #[test]
    fn chart_offset_applied_to_chart_path() {
        use hwpforge_core::chart::{ChartData, ChartGrouping, ChartType, LegendPosition};
        use hwpforge_core::control::Control;
        let ctrl = Control::Chart {
            chart_type: ChartType::Line,
            data: ChartData::category(&["X"], &[("S", [1.0].as_slice())]),
            width: HwpUnit::new(5000).unwrap(),
            height: HwpUnit::new(4000).unwrap(),
            title: None,
            legend: LegendPosition::default(),
            grouping: ChartGrouping::Clustered,
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        // chart_offset=5 → chart1 index becomes 5+1=6 → chart6.xml
        let result = encode_section(&section, 0, 5, 0, 0, EncodeOptions::default()).unwrap();
        assert_eq!(result.charts.len(), 1);
        assert_eq!(result.charts[0].0, "Chart/chart6.xml");
    }

    // ── build_bookmark_span_start/end run xml helpers ─────────────

    #[test]
    fn bookmark_span_start_run_xml_structure() {
        let xml = build_bookmark_span_start_run_xml("mymark", 2, 7);
        assert!(xml.contains(r#"charPrIDRef="2""#));
        assert!(xml.contains(r#"type="BOOKMARK""#));
        assert!(xml.contains(r#"name="mymark""#));
        assert!(xml.contains(r#"fieldid="1728000007""#), "non-zero fieldid for field_id=7");
        assert!(xml.ends_with("</hp:run>"));
    }

    #[test]
    fn bookmark_span_end_run_xml_structure() {
        let xml = build_bookmark_span_end_run_xml(1, 3);
        assert!(xml.contains(r#"charPrIDRef="1""#));
        assert!(xml.contains("<hp:fieldEnd"));
        // beginIDRef references the unique id (1_200_000_000 + field_id)
        assert!(xml.contains(r#"beginIDRef="1200000003""#));
        assert!(xml.contains(r#"fieldid="1728000003""#), "non-zero fieldid for field_id=3");
        assert!(xml.ends_with("</hp:run>"));
    }

    /// Hancom reads `<hp:fieldBegin id="...">` as a signed 32-bit integer.
    /// Every field builder's `id` (and the paired `beginIDRef`) must stay below
    /// `i32::MAX + 1` (2_147_483_648); a larger value wraps negative and the
    /// field is no longer recognized as a valid instance.
    #[test]
    fn all_field_builders_emit_signed_i32_safe_begin_id() {
        use hwpforge_foundation::{FieldType, RefContentType, RefType};

        const I32_LIMIT: u64 = 2_147_483_648; // i32::MAX + 1

        // Parse every `id="..."` / `beginIDRef="..."` integer in `xml` and
        // assert each fits in a positive signed 32-bit range.
        fn assert_ids_under_limit(xml: &str, builder: &str) {
            for attr in ["id=\"", "beginIDRef=\""] {
                let mut rest = xml;
                while let Some(start) = rest.find(attr) {
                    let after = &rest[start + attr.len()..];
                    let end = after.find('"').expect("unterminated id attribute");
                    let value: u64 = after[..end].parse().unwrap_or_else(|_| {
                        panic!("{builder}: non-numeric id `{}`", &after[..end])
                    });
                    assert!(value < I32_LIMIT, "{builder}: {attr}{value} exceeds signed i32 range");
                    rest = &after[end..];
                }
            }
        }

        // Exercise a large field_id so base + counter is checked, not just base.
        let big = 1_000_000_usize;

        assert_ids_under_limit(
            &build_field_run_xml(&FieldType::ClickHere, "", "", "", "", 0, big),
            "field_run/CLICK_HERE",
        );
        assert_ids_under_limit(
            &build_field_run_xml(&FieldType::ModifiedTime, "", "", "", "", 0, big),
            "field_run/SUMMERY",
        );
        assert_ids_under_limit(
            &build_hyperlink_run_xml("text", "https://example.com", 0, big),
            "hyperlink",
        );
        assert_ids_under_limit(
            &build_bookmark_span_start_run_xml("mark", 0, big),
            "bookmark_span_start",
        );
        assert_ids_under_limit(&build_bookmark_span_end_run_xml(0, big), "bookmark_span_end");
        assert_ids_under_limit(
            &build_crossref_run_xml(
                &hwpforge_core::control::RefTarget::Name("bookmark1".to_string()),
                "see bookmark1",
                &RefType::default(),
                &RefContentType::default(),
                true,
                0,
                big,
            ),
            "crossref",
        );
        assert_ids_under_limit(
            &build_hwp5_crossref_run_xml(
                "bookmark1",
                "see bookmark1",
                RefType::default(),
                RefContentType::default(),
                true,
                0,
                big,
            ),
            "hwp5_crossref",
        );
        assert_ids_under_limit(
            &build_memo_run_xml(
                "",
                "<hp:t>x</hp:t>",
                &hwpforge_core::MemoMetadata::default(),
                0,
                big,
            ),
            "memo",
        );
    }

    // ── Heading level (titleMark) encoding ───────────────────────

    #[test]
    fn heading_level_injects_title_mark() {
        let mut para = text_paragraph("Heading", 0, 0);
        para.heading_level = Some(1);
        let section = Section::with_paragraphs(vec![para], PageSettings::a4());
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(xml.contains("<hp:titleMark"), "titleMark required for headings");
        assert!(xml.contains(r#"ignore="false""#));
    }

    #[test]
    fn no_heading_level_no_title_mark() {
        let section = simple_section("Normal paragraph");
        let xml = encode_section(&section, 0, 0, 0, 0, EncodeOptions::default()).unwrap().xml;
        assert!(!xml.contains("<hp:titleMark"), "non-heading must NOT have titleMark");
    }

    /// Reference implementation: the original sequential `replacen` loop.
    fn replacen_reference(mut xml: String, replacements: &[(String, String)]) -> String {
        for (marker, real) in replacements {
            xml = xml.replacen(marker, real, 1);
        }
        xml
    }

    #[test]
    fn apply_run_xml_replacements_matches_replacen() {
        // Markers are unique nonce tokens; payloads never contain markers.
        let xml = "<p>A</p>__clk_0_1__mid__clk_1_2__tail__clk_2_3__end".to_string();
        let repls = vec![
            ("__clk_0_1__".to_string(), "<hp:fieldBegin/>X<hp:fieldEnd/>".to_string()),
            ("__clk_1_2__".to_string(), "<hp:ctrl>Y</hp:ctrl>".to_string()),
            ("__clk_2_3__".to_string(), "Z".to_string()),
        ];
        let single = apply_run_xml_replacements(xml.clone(), &repls);
        let reference = replacen_reference(xml, &repls);
        assert_eq!(single, reference);
    }

    #[test]
    fn apply_run_xml_replacements_order_independent() {
        // Out-of-order replacement list must still produce position-ordered output.
        let xml = "head__m_2_0____m_0_1____m_1_2__".to_string();
        let repls = vec![
            ("__m_0_1__".to_string(), "B".to_string()),
            ("__m_2_0__".to_string(), "A".to_string()),
            ("__m_1_2__".to_string(), "C".to_string()),
        ];
        let single = apply_run_xml_replacements(xml.clone(), &repls);
        let reference = replacen_reference(xml, &repls);
        assert_eq!(single, reference);
        assert_eq!(single, "headABC");
    }

    #[test]
    fn apply_run_xml_replacements_empty_and_missing() {
        // Empty list: identity.
        let xml = "<p>no markers</p>".to_string();
        assert_eq!(apply_run_xml_replacements(xml.clone(), &[]), xml);
        // Missing marker: silent no-op (matches replacen).
        let repls = vec![("__absent_9_9__".to_string(), "X".to_string())];
        let single = apply_run_xml_replacements(xml.clone(), &repls);
        assert_eq!(single, replacen_reference(xml, &repls));
    }
}
