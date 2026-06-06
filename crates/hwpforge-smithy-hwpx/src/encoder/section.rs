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

mod table;

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
    HxIndexMark, HxMatrix, HxOffset, HxPageMargin, HxPagePr, HxParagraph, HxPic, HxPoint,
    HxRenderingInfo, HxRotationInfo, HxRun, HxRunCase, HxRunSwitch, HxScript, HxSecPr, HxSection,
    HxShapeComment, HxSizeAttr, HxSubList, HxTable, HxTableCell, HxTableMargin, HxTablePos,
    HxTableRow, HxTableSz, HxText, HxTitleMark,
};

use self::table::build_table;
use super::chart::generate_chart_xml;
use super::escape_xml;

/// Shared nonce counter for all marker-based placeholder runs.
///
/// Using a single module-level counter prevents duplicate marker strings
/// even when multiple Control variants are encoded in the same document.
static MARKER_NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const HWP5_CROSSREF_UNKNOWN_TAG: &str = "hwp5.crossref";

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
    inject_header_footer_pagenum(&mut enriched, section, &mut run_xml_replacements)?;

    // Replace hyperlink placeholder runs with real interleaved XML.
    // Serde cannot express the ctrl-text-ctrl interleaving required by
    // HWPX fieldBegin/fieldEnd, so we serialize a marker and swap it here.
    for (marker_xml, real_xml) in &run_xml_replacements {
        enriched = enriched.replacen(marker_xml, real_xml, 1);
    }

    // Generate masterpage XML files
    let master_pages = build_masterpage_entries(section, masterpage_offset);

    Ok(SectionEncodeResult {
        xml: wrap_section_xml(&enriched),
        charts: chart_entries,
        master_pages,
        embedded_oles,
    })
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
fn build_section(
    section: &Section,
    chart_entries: &mut Vec<(String, String)>,
    embedded_oles: &mut Vec<(String, Vec<u8>)>,
    hyperlink_entries: &mut Vec<(String, String)>,
    chart_offset: usize,
    embedded_ole_offset: usize,
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
    let linesegarray = None;

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
                tables.push(build_table(t, depth, hyperlink_entries)?);
            }
            RunContent::Image(img) => {
                pictures.push(build_picture(img, depth, hyperlink_entries)?);
            }
            RunContent::Control(ctrl) => {
                match ctrl.as_ref() {
                    Control::Footnote { .. } | Control::Endnote { .. } => {
                        if let Some(hx_ctrl) =
                            encode_control_to_ctrl(ctrl, depth, hyperlink_entries)?
                        {
                            ctrls.push(hx_ctrl);
                        }
                    }
                    Control::TextBox { .. } => {
                        rects.push(encode_textbox_to_rect(ctrl, depth, hyperlink_entries)?);
                    }
                    Control::Rect { .. } => {
                        rects.push(encode_rect_to_hx(ctrl, depth, hyperlink_entries)?);
                    }
                    Control::Line { .. } => {
                        lines.push(encode_line_to_hx(ctrl, depth, hyperlink_entries)?);
                    }
                    Control::Ellipse { .. } => {
                        ellipses.push(encode_ellipse_to_hx(ctrl, depth, hyperlink_entries)?);
                    }
                    Control::Polygon { .. } => {
                        polygons.push(encode_polygon_to_hx(ctrl, depth, hyperlink_entries)?);
                    }
                    Control::Arc { .. } => {
                        ellipses.push(encode_arc_to_hx(ctrl, depth, hyperlink_entries)?);
                    }
                    Control::Curve { .. } => {
                        curves.push(encode_curve_to_hx(ctrl, depth, hyperlink_entries)?);
                    }
                    Control::ConnectLine { .. } => {
                        connect_lines.push(encode_connect_line_to_hx(
                            ctrl,
                            depth,
                            hyperlink_entries,
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
                            encode_control_to_ctrl(ctrl, depth, hyperlink_entries)?
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
                    Control::Field { field_type, hint_text, help_text, name } => {
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPFD", field_id);
                        let hint = hint_text.as_deref().unwrap_or("");
                        let real_xml = build_field_run_xml(
                            field_type,
                            hint,
                            help_text.as_deref().unwrap_or(""),
                            name.as_deref().unwrap_or(""),
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
                    // UnknownSummery arms below emit SUMMERY-shaped XML as a
                    // *best-effort* HWPX surrogate. HWPX has no native counterpart
                    // for `%smr` unknown tokens or `%dte` format patterns.
                    // Round-tripping through HWPX → Core decoder normalises these
                    // back as `Field(ModifiedTime/CreatedTime)` (for
                    // DateCodeField) or `UnknownSummery` (for UnknownSummery), so
                    // the original Core variant is NOT preserved.
                    //
                    // PathField is NO LONGER LOSSY (Wave 12n Step 6) — see the
                    // arm further below which emits Hancom-native
                    // `type="PATH"` with `Format=` param and a distinct fieldid.
                    Control::UnknownSummery { token } => {
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPFD", field_id);
                        let real_xml = build_summery_run_xml_raw(
                            token,
                            "",
                            "",
                            char_pr_id_ref,
                            1_000_000_000_u64 + field_id as u64,
                        );
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText::new(marker));
                    }
                    Control::DateCodeField { raw_command, is_time_mode, .. } => {
                        // LOSSY: %dte → SUMMERY mapping; raw_trailer is discarded.
                        // Round-trip through HWPX comes back as `Field(ModifiedTime)`
                        // or `Field(CreatedTime)` — proven by
                        // `lossy_roundtrip_datecodefield_{date,time}_becomes_*` tests.
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPFD", field_id);
                        let token: &str =
                            if *is_time_mode { "$createtime" } else { "$modifiedtime" };
                        let display = raw_command.as_str();
                        let real_xml = build_summery_run_xml_raw(
                            token,
                            display,
                            "",
                            char_pr_id_ref,
                            1_000_000_000_u64 + field_id as u64,
                        );
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText::new(marker));
                    }
                    Control::PathField { command } => {
                        // Wave 12n Step 6 — LOSSLESS emit. The prior SUMMERY
                        // surrogate (mapped %pat → type="SUMMERY") triggered the
                        // Hancom "low security level — content recovered"
                        // warning (#120) because native files emit
                        // type="PATH" with `Format=` param, `fieldid=628121972`,
                        // and `editable="0"`. We now emit the wire shape
                        // directly. Body is left empty so Hancom recomputes
                        // `$P$F` against the file's actual on-disk path the
                        // same way `<opf:meta name="date"/>` is recomputed.
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPFD", field_id);
                        let real_xml = build_path_field_run_xml_raw(
                            command.wire_command(),
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
                    Control::CrossRef { target_name, ref_type, content_type, as_hyperlink } => {
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPXR", field_id);
                        let real_xml = build_crossref_run_xml(
                            target_name,
                            target_name,
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
                    Control::Unknown { tag, data }
                        if tag == HWP5_CROSSREF_UNKNOWN_TAG
                            && data
                                .as_deref()
                                .and_then(parse_hwp5_crossref_unknown_data)
                                .is_some() =>
                    {
                        let payload = parse_hwp5_crossref_unknown_data(data.as_deref().unwrap())
                            .expect("guard already parsed hwp5 crossref payload");
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPXH5XRF", field_id);
                        let real_xml = build_hwp5_crossref_run_xml(
                            payload.target_name,
                            payload.display_text,
                            payload.ref_type,
                            payload.content_type,
                            payload.as_hyperlink,
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
                        let sublist_xml = encode_memo_sublist(content, depth, hyperlink_entries)?;
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
) -> HwpxResult<Option<HxCtrl>> {
    match ctrl {
        Control::Footnote { inst_id, paragraphs } => Ok(Some(HxCtrl {
            foot_note: Some(HxFootNote {
                inst_id: *inst_id,
                sub_list: encode_paragraphs_to_sublist(paragraphs, depth, hyperlink_entries)?,
            }),
            ..Default::default()
        })),
        Control::Endnote { inst_id, paragraphs } => Ok(Some(HxCtrl {
            end_note: Some(HxFootNote {
                inst_id: *inst_id,
                sub_list: encode_paragraphs_to_sublist(paragraphs, depth, hyperlink_entries)?,
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

/// Encodes a `Vec<Paragraph>` into `HxSubList` with standard defaults.
pub(crate) fn encode_paragraphs_to_sublist(
    paragraphs: &[Paragraph],
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<HxSubList> {
    build_sublist(paragraphs, depth, "TOP", hyperlink_entries)
}

fn build_sublist(
    paragraphs: &[Paragraph],
    depth: usize,
    vert_align: &str,
    hyperlink_entries: &mut Vec<(String, String)>,
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

/// Encodes a Core `Control::Equation` into `HxEquation`.
///
/// Equations have NO shape common block (no offset, orgSz, curSz, flip,
/// rotation, lineShape, fillBrush, shadow). Only sz + pos + outMargin + script.
/// Does not take `depth` because equations have no recursive sub-content.
fn encode_equation_to_hx(ctrl: &Control) -> HwpxResult<HxEquation> {
    let (script, width, height, base_line, text_color, font) = match ctrl {
        Control::Equation { script, width, height, base_line, text_color, font } => {
            (script, *width, *height, *base_line, text_color, font)
        }
        _ => unreachable!("encode_equation_to_hx called with non-Equation"),
    };

    let w = width.as_i32();
    let h = height.as_i32();

    Ok(HxEquation {
        id: generate_instid(),
        z_order: 0,
        numbering_type: "EQUATION".to_string(),
        text_wrap: "TOP_AND_BOTTOM".to_string(),
        text_flow: "BOTH_SIDES".to_string(),
        lock: 0,
        dropcap_style: DropCapStyle::None.to_string(),

        // Equation-specific attrs (hardcoded constants per ground truth)
        version: "Equation Version 60".to_string(),
        base_line,
        text_color: text_color.to_hex_rgb(),
        base_unit: 1000,
        line_mode: "CHAR".to_string(),
        font: font.clone(),

        sz: Some(HxTableSz {
            width: w,
            width_rel_to: "ABSOLUTE".to_string(),
            height: h,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: 0,
        }),
        pos: Some(HxTablePos {
            treat_as_char: 1,
            affect_l_spacing: 0,
            flow_with_text: 1, // equations always flowWithText=1
            allow_overlap: 0,
            hold_anchor_and_so: 0,
            vert_rel_to: "PARA".to_string(),
            horz_rel_to: "PARA".to_string(),
            vert_align: "TOP".to_string(),
            horz_align: "LEFT".to_string(),
            vert_offset: 0,
            horz_offset: 0,
        }),
        out_margin: Some(HxTableMargin { left: 56, right: 56, top: 0, bottom: 0 }),
        shape_comment: Some(HxShapeComment { text: "수식입니다.".to_string() }),
        script: Some(HxScript { text: script.clone() }),
    })
}

/// Encodes a Core `Control::Dutmal` into `HxDutmal`.
fn encode_dutmal_to_hx(
    main_text: &str,
    sub_text: &str,
    position: DutmalPosition,
    sz_ratio: u32,
    align: DutmalAlign,
    option: u32,
) -> HxDutmal {
    let pos_type = match position {
        DutmalPosition::Top => "TOP",
        DutmalPosition::Bottom => "BOTTOM",
        DutmalPosition::Right => "RIGHT",
        DutmalPosition::Left => "LEFT",
        _ => "TOP",
    };
    let align_str = match align {
        DutmalAlign::Center => "CENTER",
        DutmalAlign::Left => "LEFT",
        DutmalAlign::Right => "RIGHT",
        _ => "CENTER",
    };
    HxDutmal {
        pos_type: pos_type.to_string(),
        sz_ratio,
        option,
        style_id_ref: 0,
        align: align_str.to_string(),
        main_text: main_text.to_string(),
        sub_text: sub_text.to_string(),
    }
}

/// Encodes a Core `Control::Compose` into `HxCompose`.
///
/// Always emits 10 `<hp:charPr>` entries — KS X 6101 fixes
/// `charPrCnt` at 10. `char_pr_ids` from the Core variant is
/// padded with `u32::MAX` ("no override" sentinel) if shorter than
/// 10 and truncated if longer; the resulting slice maps 1:1 onto
/// the `<hp:charPr prIDRef="…"/>` children.
fn encode_compose_to_hx(
    compose_text: &str,
    circle_type: &str,
    char_sz: i32,
    compose_type: &str,
    char_pr_ids: &[u32],
) -> HxCompose {
    let char_prs = (0..10)
        .map(|i| HxComposeCharPr { pr_id_ref: char_pr_ids.get(i).copied().unwrap_or(u32::MAX) })
        .collect();
    HxCompose {
        circle_type: circle_type.to_string(),
        char_sz,
        compose_type: compose_type.to_string(),
        char_pr_cnt: 10,
        compose_text: compose_text.to_string(),
        char_prs,
    }
}

/// Builds a complete `<hp:run>` XML string for a hyperlink.
///
/// HWPX hyperlinks use a `fieldBegin`/`fieldEnd` pair inside `<hp:ctrl>`
/// elements, interleaved with text content within a single `<hp:run>`:
///
/// ```xml
/// <hp:run charPrIDRef="N">
///   <hp:ctrl>
///     <hp:fieldBegin type="HYPERLINK" ... fieldid="F" ...>
///       <hp:parameters cnt="4" name="">
///         <hp:stringParam name="Path">URL</hp:stringParam>
///         ...
///       </hp:parameters>
///     </hp:fieldBegin>
///   </hp:ctrl>
///   <hp:t>display text</hp:t>
///   <hp:ctrl>
///     <hp:fieldEnd beginIDRef="F" fieldid="F"/>
///   </hp:ctrl>
/// </hp:run>
/// ```
///
/// This interleaved ordering (ctrl → text → ctrl) cannot be expressed by
/// serde's field-order-based serialization, hence the manual XML generation.
fn build_hyperlink_run_xml(text: &str, url: &str, char_pr_id_ref: u32, field_id: usize) -> String {
    let escaped_url = escape_xml(url);
    let text_xml = build_text_element_xml(text);
    // Unique begin_id per field instance (matches build_field_run_xml pattern).
    // beginIDRef must reference this id, NOT the fieldid.
    // Hancom reads `fieldBegin id` as a signed 32-bit int; this base + field_id
    // must stay well below i32::MAX (2_147_483_647). Distinct per builder.
    let begin_id = 1_100_000_000_u64 + field_id as u64;
    // `fieldid` is a Hancom field instance id and must be a non-zero 32-bit
    // value; `fieldid="0"` is treated as an invalid instance. Distinct base
    // keeps it unique vs other field types and stays under 2^31.
    let field_uid = 1_628_000_000_u64 + field_id as u64;
    // KS X 6101: mailto: → HWPHYPERLINK_TYPE_EMAIL, others → HWPHYPERLINK_TYPE_URL
    let category = if url.starts_with("mailto:") {
        "HWPHYPERLINK_TYPE_EMAIL"
    } else {
        "HWPHYPERLINK_TYPE_URL"
    };
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="HYPERLINK" name="" editable="0" dirty="0" "#,
            r#"zorder="-1" fieldid="{fid}" metaTag="">"#,
            r#"<hp:parameters cnt="4" name="">"#,
            r#"<hp:stringParam name="Path">{url}</hp:stringParam>"#,
            r#"<hp:stringParam name="Category">{cat}</hp:stringParam>"#,
            r#"<hp:stringParam name="TargetType">HWPHYPERLINK_TARGET_DOCUMENT_DONTCARE</hp:stringParam>"#,
            r#"<hp:stringParam name="DocOpenType">HWPHYPERLINK_JUMP_NEWTAB</hp:stringParam>"#,
            r#"</hp:parameters>"#,
            r#"</hp:fieldBegin>"#,
            r#"</hp:ctrl>"#,
            r#"{txt}"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="{fid}"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        fid = field_uid,
        url = escaped_url,
        cat = category,
        txt = text_xml,
    )
}

/// Builds a `<hp:run>` XML string for a span bookmark (fieldBegin/fieldEnd).
/// Builds a `<hp:run>` containing only `<hp:fieldBegin>` for bookmark span start.
///
/// The matching `<hp:fieldEnd>` is emitted by [`build_bookmark_span_end_run_xml`].
/// Text between them (in separate runs) is covered by the bookmark span.
fn build_bookmark_span_start_run_xml(name: &str, char_pr_id_ref: u32, field_id: usize) -> String {
    let escaped_name = escape_xml(name);
    // Signed-32-bit-safe base; MUST match `build_bookmark_span_end_run_xml`
    // so the paired fieldEnd `beginIDRef` references this `id`.
    let begin_id = 1_200_000_000_u64 + field_id as u64;
    // Non-zero 32-bit field instance id; must match the paired fieldEnd.
    let field_uid = 1_728_000_000_u64 + field_id as u64;
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="BOOKMARK" name="{name}" editable="0" dirty="0" "#,
            r#"zorder="-1" fieldid="{fid}" metaTag=""/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        fid = field_uid,
        name = escaped_name,
    )
}

/// Builds a `<hp:run>` containing only `<hp:fieldEnd>` for bookmark span end.
fn build_bookmark_span_end_run_xml(char_pr_id_ref: u32, field_id: usize) -> String {
    // Signed-32-bit-safe base; MUST match `build_bookmark_span_start_run_xml`.
    let begin_id = 1_200_000_000_u64 + field_id as u64;
    // Non-zero 32-bit field instance id; must match the paired fieldBegin.
    let field_uid = 1_728_000_000_u64 + field_id as u64;
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="{fid}"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        fid = field_uid,
    )
}

/// Dispatches a `Control::Field` to the right HWPX `<hp:run>` builder
/// based on the field family.
///
/// # Field families
///
/// - **CLICK_HERE** (`build_clickhere_field_xml`): editable press-field
///   (누름틀). `type="CLICK_HERE"`, `fieldid=627272811`,
///   `Command=Clickhere:set:N:...`.
/// - **SUMMERY** (`build_summery_field_xml`): `$author`, `$lastsaveby`,
///   `$createtime`, `$modifiedtime`, `$title`. `type="SUMMERY"` (한글 typo),
///   `fieldid=628321650`.
fn build_field_run_xml(
    field_type: &hwpforge_foundation::FieldType,
    hint: &str,
    help: &str,
    name: &str,
    char_pr_id_ref: u32,
    field_id: usize,
) -> String {
    use hwpforge_foundation::FieldType;
    let begin_id = 1_000_000_000_u64 + field_id as u64;
    match field_type {
        FieldType::ClickHere => {
            build_clickhere_field_xml(hint, help, name, char_pr_id_ref, begin_id)
        }
        FieldType::Author
        | FieldType::LastSavedBy
        | FieldType::CreatedTime
        | FieldType::ModifiedTime
        | FieldType::Title => {
            build_summery_field_xml(field_type, hint, name, char_pr_id_ref, begin_id)
        }
        // `FieldType` is `#[non_exhaustive]`. We intentionally do NOT collapse
        // future variants into ClickHere (Wave 12n architect review): silently
        // mis-encoding a future SUMMERY/auto-field token as CLICK_HERE would
        // create a stealth corruption path. New variants must explicitly extend
        // this match.
        _ => unreachable!(
            "FieldType variant added without an HWPX encoder branch — extend build_field_run_xml first"
        ),
    }
}

/// Builds the CLICK_HERE (누름틀) `<hp:run>` XML.
///
/// Wire convention: `hint_len`/`help_len` are UTF-16 code unit counts of the
/// *decoded* strings. `Command N` is computed by `clickhere_command_string`
/// from the empirically-derived formula (see that function's doc comment).
fn build_clickhere_field_xml(
    hint: &str,
    help: &str,
    name: &str,
    char_pr_id_ref: u32,
    begin_id: u64,
) -> String {
    let escaped_hint = escape_xml(hint);
    let escaped_name = escape_xml(name);
    let hint_len = hint.encode_utf16().count();
    let help_len = help.encode_utf16().count();
    let command = clickhere_command_string(hint, help, hint_len, help_len);
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="CLICK_HERE" name="{name}" editable="1" dirty="0" "#,
            r#"zorder="-1" fieldid="627272811" metaTag="">"#,
            r#"<hp:parameters cnt="3" name="">"#,
            r#"<hp:integerParam name="Prop">9</hp:integerParam>"#,
            r#"<hp:stringParam name="Command" xml:space="preserve">{cmd}</hp:stringParam>"#,
            r#"<hp:stringParam name="Direction">{hint}</hp:stringParam>"#,
            r#"</hp:parameters>"#,
            r#"</hp:fieldBegin>"#,
            r#"</hp:ctrl>"#,
            r#"{display}"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="627272811"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        name = escaped_name,
        cmd = escape_xml(&command),
        hint = escaped_hint,
        display = build_text_element_xml(hint),
    )
}

/// Builds a SUMMERY (Author/LastSavedBy/CreatedTime/ModifiedTime/Title) `<hp:run>` XML.
///
/// Reference: `tests/fixtures/fields/date_field.hwpx`. The HWP5 ctrl_id `%smr`
/// is shared by all SUMMERY auto-fields; discrimination is via the `Command`
/// `$token`. Token mapping verified against 한컴 native fixtures in Wave 12n
/// (see `.docs/research/2026-06-02_auto_field_wire_dump.md`).
fn build_summery_field_xml(
    field_type: &hwpforge_foundation::FieldType,
    hint: &str,
    name: &str,
    char_pr_id_ref: u32,
    begin_id: u64,
) -> String {
    use hwpforge_foundation::FieldType;
    let command = field_type.summery_token().expect("caller guards SUMMERY variants");
    let display_text = match field_type {
        FieldType::ModifiedTime => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let days = now / 86400;
            let (y, m, d) = days_to_ymd(days);
            format!("{y}-{m:02}-{d:02}")
        }
        FieldType::CreatedTime => " ".to_string(),
        FieldType::Author | FieldType::LastSavedBy | FieldType::Title => {
            if !hint.is_empty() {
                hint.to_string()
            } else {
                " ".to_string()
            }
        }
        FieldType::ClickHere => unreachable!("caller already routed ClickHere elsewhere"),
        _ => " ".to_string(),
    };
    build_summery_run_xml_raw(command, &display_text, name, char_pr_id_ref, begin_id)
}

/// Lowest-level SUMMERY `<hp:run>` builder — emits a `type="SUMMERY"`
/// `fieldBegin`/`fieldEnd` pair with the caller-supplied `command` token
/// and `display` text. Used by [`build_summery_field_xml`] for typed
/// [`hwpforge_foundation::FieldType`] variants and by Wave 12n
/// `UnknownSummery` / `DateCodeField` fallback paths.
///
/// Wave 12n Step 6: `Control::PathField` no longer uses this builder.
/// See [`build_path_field_run_xml_raw`] for the native PATH wire shape.
fn build_summery_run_xml_raw(
    command: &str,
    display: &str,
    name: &str,
    char_pr_id_ref: u32,
    begin_id: u64,
) -> String {
    let escaped_name = escape_xml(name);
    let escaped_cmd = escape_xml(command);
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="SUMMERY" name="{name}" editable="1" dirty="0" "#,
            r#"zorder="-1" fieldid="628321650" metaTag="">"#,
            r#"<hp:parameters cnt="3" name="">"#,
            r#"<hp:integerParam name="Prop">8</hp:integerParam>"#,
            r#"<hp:stringParam name="Command">{cmd}</hp:stringParam>"#,
            r#"<hp:stringParam name="Property">{cmd}</hp:stringParam>"#,
            r#"</hp:parameters>"#,
            r#"</hp:fieldBegin>"#,
            r#"</hp:ctrl>"#,
            r#"{display}"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="628321650"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        name = escaped_name,
        cmd = escaped_cmd,
        display = build_text_element_xml(display),
    )
}

/// Lowest-level PATH `<hp:run>` builder — emits a `type="PATH"`
/// `fieldBegin`/`fieldEnd` pair carrying a `$P`/`$F`/`$P$F` format code
/// in the `Format` parameter. Wave 12n Step 6 — replaces the prior
/// SUMMERY surrogate for `Control::PathField`.
///
/// Wire shape (empirically derived from Hancom Office native
/// `sample-field-docsummary.hwp` → `.hwpx` conversion):
///
/// - `type="PATH"` (not SUMMERY — different field semantics)
/// - `fieldid="628121972"` (distinct from the SUMMERY `628321650`)
/// - `editable="0"` (PATH fields are read-only — Hancom recomputes)
/// - `<hp:parameters cnt="3">` with `Prop` / `Command` / **`Format`**
///   (NOT the SUMMERY `Property`)
/// - empty body (Hancom evaluates `$P$F` to the absolute path on save,
///   the same way `date` is recomputed)
fn build_path_field_run_xml_raw(command: &str, char_pr_id_ref: u32, begin_id: u64) -> String {
    let escaped_cmd = escape_xml(command);
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="PATH" name="" editable="0" dirty="0" "#,
            r#"zorder="-1" fieldid="628121972" metaTag="">"#,
            r#"<hp:parameters cnt="3" name="">"#,
            r#"<hp:integerParam name="Prop">8</hp:integerParam>"#,
            r#"<hp:stringParam name="Command">{cmd}</hp:stringParam>"#,
            r#"<hp:stringParam name="Format">{cmd}</hp:stringParam>"#,
            r#"</hp:parameters>"#,
            r#"</hp:fieldBegin>"#,
            r#"</hp:ctrl>"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="628121972"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        cmd = escaped_cmd,
    )
}

/// Builds the `Clickhere:set:N:...` command string.
///
/// `N` is **not** the total UTF-16 length of the command — empirically (verified
/// against five 한컴-authored fixtures including `basic`, `with-help`,
/// `empty-hint`, `multi`, and `named`) it equals the UTF-16 length of the
/// substring after `"Clickhere:set:N:"` minus one (one of the two trailing
/// spaces is excluded from `N`). The encoder can compute this directly
/// without iteration because the formula does not depend on `digits(N)`.
///
/// See `.docs/research/2026-06-02_clickhere_wire_dump.md` for the empirical
/// derivation.
fn clickhere_command_string(hint: &str, help: &str, hint_len: usize, help_len: usize) -> String {
    let rest =
        format!("Direction:wstring:{hint_len}:{hint} HelpState:wstring:{help_len}:{help}  ",);
    let n = rest.encode_utf16().count().saturating_sub(1);
    format!("Clickhere:set:{n}:{rest}")
}

/// Builds a `<hp:run>` XML string for an inline page number (`<hp:autoNum>`).
///
/// Page numbers within body text use `<hp:autoNum numType="PAGE">` (current
/// page) or `numType="TOTAL_PAGE"` (total pages) — NOT fieldBegin/fieldEnd.
/// HWPX 스펙: `paralist.xsd` (`numType` enumeration includes
/// `PAGE`/`TOTAL_PAGE`/`FOOTNOTE`/...).
///
/// Returns `None` for [`hwpforge_core::control::InlinePageKind::Unknown`] —
/// the caller is expected to skip and emit a warning rather than fabricate
/// a `numType`. Wave 12n architect review CRITICAL: do not collapse
/// `TotalPages`/`Unknown` to `CurrentPage`.
fn build_autonum_run_xml(
    char_pr_id_ref: u32,
    kind: hwpforge_core::control::InlinePageKind,
) -> Option<String> {
    let num_type = match kind {
        hwpforge_core::control::InlinePageKind::CurrentPage => "PAGE",
        hwpforge_core::control::InlinePageKind::TotalPages => "TOTAL_PAGE",
        hwpforge_core::control::InlinePageKind::Unknown => return None,
        // `InlinePageKind` is `#[non_exhaustive]`. Skip future kinds instead of
        // fabricating a numType — match the Unknown policy.
        _ => return None,
    };
    Some(format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:autoNum num="1" numType="{nt}">"#,
            r#"<hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar="" supscript="0"/>"#,
            r#"</hp:autoNum>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        nt = num_type,
    ))
}

/// Simple days-since-epoch to (year, month, day) conversion.
fn days_to_ymd(days_since_epoch: u64) -> (u64, u64, u64) {
    // Simplified civil calendar calculation.
    let z = days_since_epoch + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Builds a `<hp:run>` XML string for a cross-reference (상호참조).
fn build_crossref_run_xml(
    target_name: &str,
    display_text: &str,
    ref_type: &hwpforge_foundation::RefType,
    content_type: &hwpforge_foundation::RefContentType,
    as_hyperlink: bool,
    char_pr_id_ref: u32,
    field_id: usize,
) -> String {
    let escaped_name = escape_xml(target_name);
    let ref_path = format!("?#{escaped_name}");
    let ref_type_str = ref_type.to_string();
    let content_type_str = content_type.to_string();
    let hyperlink_val = if as_hyperlink { "true" } else { "false" };
    // Signed-32-bit-safe begin_id base; distinct from other field builders.
    let begin_id = 1_300_000_000_u64 + field_id as u64;
    // Non-zero 32-bit field instance id (see `build_hwp5_crossref_run_xml`).
    // Distinct base from the HWP5 crossref builder to avoid fieldid collisions.
    let field_uid = 1_828_000_000_u64 + field_id as u64;
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="CROSSREF" name="" editable="0" dirty="0" "#,
            r#"zorder="-1" fieldid="{fid}" metaTag="">"#,
            r#"<hp:parameters cnt="5" name="">"#,
            r#"<hp:stringParam name="RefPath">{ref_path}</hp:stringParam>"#,
            r#"<hp:stringParam name="RefType">{ref_type}</hp:stringParam>"#,
            r#"<hp:stringParam name="RefContentType">{content_type}</hp:stringParam>"#,
            r#"<hp:booleanParam name="RefHyperLink">{hyperlink}</hp:booleanParam>"#,
            r#"<hp:stringParam name="RefOpenType">HYPERLINK_JUMP_DONTCARE</hp:stringParam>"#,
            r#"</hp:parameters>"#,
            r#"</hp:fieldBegin>"#,
            r#"</hp:ctrl>"#,
            r#"{name}"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="{fid}"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        fid = field_uid,
        ref_path = ref_path,
        ref_type = ref_type_str,
        content_type = content_type_str,
        hyperlink = hyperlink_val,
        name = build_text_element_xml(display_text),
    )
}

#[derive(Debug, Clone, Copy)]
struct Hwp5CrossRefUnknownPayload<'a> {
    target_name: &'a str,
    display_text: &'a str,
    ref_type: hwpforge_foundation::RefType,
    content_type: hwpforge_foundation::RefContentType,
    as_hyperlink: bool,
}

fn parse_hwp5_crossref_unknown_data(data: &str) -> Option<Hwp5CrossRefUnknownPayload<'_>> {
    let mut lines = data.splitn(5, '\n');
    let target_name = lines.next()?;
    let display_text = lines.next()?;
    let ref_type = lines.next()?.parse().ok()?;
    let content_type = lines.next()?.parse().ok()?;
    let as_hyperlink = matches!(lines.next()?, "true" | "1");
    Some(Hwp5CrossRefUnknownPayload {
        target_name,
        display_text,
        ref_type,
        content_type,
        as_hyperlink,
    })
}

fn build_hwp5_crossref_run_xml(
    target_name: &str,
    display_text: &str,
    ref_type: hwpforge_foundation::RefType,
    content_type: hwpforge_foundation::RefContentType,
    as_hyperlink: bool,
    char_pr_id_ref: u32,
    field_id: usize,
) -> String {
    let escaped_target_name = escape_xml(target_name);
    let escaped_display_text = build_text_element_xml(display_text);
    let ref_type_str = ref_type.to_string();
    let content_type_str = content_type.to_string();
    let hyperlink_val = if as_hyperlink { "true" } else { "false" };
    // Signed-32-bit-safe begin_id base; distinct from other field builders.
    // Hancom reads `fieldBegin id` as i32; a base >= 2^31 wraps negative and
    // the field is no longer recognized (click / F9 / Ctrl+click jump fail).
    let begin_id = 1_400_000_000_u64 + field_id as u64;
    // `fieldid` is a Hancom field instance id and must be a non-zero 32-bit
    // value. A raw 0-based `field_id` would emit `fieldid="0"`, which Hancom
    // treats as an invalid instance (F9 refresh / Ctrl+click jump break).
    // Distinct base keeps it unique vs other field types' fieldid id-space
    // and stays under 2^31 (truth fixtures use values < 2^31).
    let field_uid = 1_928_000_000_u64 + field_id as u64;
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="CROSSREF" name="" editable="0" dirty="0" "#,
            r#"zorder="-1" fieldid="{fid}" metaTag="">"#,
            r#"<hp:parameters cnt="8" name="">"#,
            r#"<hp:booleanParam name="Fiexde">1</hp:booleanParam>"#,
            r#"<hp:integerParam name="Prop">0</hp:integerParam>"#,
            r#"<hp:stringParam name="Command">?{target};6;0;0;0;</hp:stringParam>"#,
            r#"<hp:stringParam name="RefPath">?{target};</hp:stringParam>"#,
            r#"<hp:stringParam name="RefType">{ref_type}</hp:stringParam>"#,
            r#"<hp:stringParam name="RefContentType">{content_type}</hp:stringParam>"#,
            r#"<hp:booleanParam name="RefHyperLink">{hyperlink}</hp:booleanParam>"#,
            r#"<hp:stringParam name="RefOpenType">HWPHYPERLINK_JUMP_CURRENTTAB</hp:stringParam>"#,
            r#"</hp:parameters>"#,
            r#"</hp:fieldBegin>"#,
            r#"</hp:ctrl>"#,
            r#"{display_text}"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="{fid}"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        fid = field_uid,
        target = escaped_target_name,
        ref_type = ref_type_str,
        content_type = content_type_str,
        hyperlink = hyperlink_val,
        display_text = escaped_display_text,
    )
}

/// Builds a `<hp:run>` XML string for a memo annotation.
///
/// `anchor_xml` is the inline `<hp:t>…</hp:t>` sequence representing the
/// visible body span the memo is attached to; it is placed *between*
/// `<hp:fieldBegin>` and `<hp:fieldEnd>` in the same `<hp:run>`. An empty
/// `anchor_xml` reproduces the pre-Wave-12f point-anchored layout, which
/// 한컴 renders as `[메모 시작][필드 끝]` (the memo end marker is
/// unpaired); see `.docs/algorithms/2026-06-01_memo_anchor_serialization.md`
/// for why we collapse anchor_runs to a single `<hp:t>` element here.
fn build_memo_run_xml(
    sublist_xml: &str,
    anchor_xml: &str,
    metadata: &hwpforge_core::MemoMetadata,
    char_pr_id_ref: u32,
    field_id: usize,
) -> String {
    // Signed-32-bit-safe begin_id base; distinct from other field builders.
    let begin_id = 1_500_000_000_u64 + field_id as u64;
    // Non-zero 32-bit field instance id; `fieldid="0"` is invalid in Hancom.
    let field_uid = 2_028_000_000_u64 + field_id as u64;

    let id = metadata.hwpx_id();
    let command = if metadata.command.is_empty() {
        // HwpForge-authored memos synthesise a minimal Command string so
        // 한컴 still pairs the field markers correctly. Format mirrors what
        // 한컴 writes for a wire-less memo.
        format!("MEMO/{}/{}/0/0/{}/\\;;", metadata.shape_id_ref, metadata.number, metadata.author)
    } else {
        metadata.command.clone()
    };
    let create_datetime = if metadata.create_datetime.is_empty() {
        iso8601_utc_now()
    } else {
        metadata.create_datetime.clone()
    };

    let parameters = build_memo_parameters_xml(
        metadata.shape_id_ref,
        &command,
        &id,
        metadata.number,
        &metadata.author,
        &create_datetime,
    );

    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="MEMO" name="" editable="1" dirty="1" "#,
            r#"zorder="1" fieldid="{fid}" metaTag="">"#,
            r#"{params}"#,
            r#"{sublist}"#,
            r#"</hp:fieldBegin>"#,
            r#"</hp:ctrl>"#,
            r#"{anchor}"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="{fid}"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        fid = field_uid,
        params = parameters,
        sublist = sublist_xml,
        anchor = anchor_xml,
    )
}

/// Builds the 7-parameter `<hp:parameters>` block 한컴 writes for a memo
/// fieldBegin. Extracted from `build_memo_run_xml` so the same structure
/// is easy to find when other field types need similar parameter blocks
/// (hyperlink/crossref already use a 4-parameter analogue inside
/// `build_hyperlink_run_xml`; that one can converge on this helper when
/// it gains parity with 한컴 truth).
fn build_memo_parameters_xml(
    shape_id_ref: u32,
    command: &str,
    id: &str,
    number: u32,
    author: &str,
    create_datetime: &str,
) -> String {
    let command_esc = escape_xml(command);
    let id_esc = escape_xml(id);
    let author_esc = escape_xml(author);
    let dt_esc = escape_xml(create_datetime);
    format!(
        concat!(
            r#"<hp:parameters cnt="7" name="">"#,
            r#"<hp:integerParam name="Prop">0</hp:integerParam>"#,
            r#"<hp:stringParam name="Command">{cmd}</hp:stringParam>"#,
            r#"<hp:stringParam name="ID">{id}</hp:stringParam>"#,
            r#"<hp:integerParam name="Number">{num}</hp:integerParam>"#,
            r#"<hp:stringParam name="Author">{author}</hp:stringParam>"#,
            r#"<hp:stringParam name="MemoShapeIDRef">{shape}</hp:stringParam>"#,
            r#"<hp:stringParam name="CreateDateTime">{dt}</hp:stringParam>"#,
            r#"</hp:parameters>"#,
        ),
        cmd = command_esc,
        id = id_esc,
        num = number,
        author = author_esc,
        shape = shape_id_ref,
        dt = dt_esc,
    )
}

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

fn unix_to_ymdhms(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let total_days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let hour = (tod / 3_600) as u32;
    let minute = ((tod % 3_600) / 60) as u32;
    let second = (tod % 60) as u32;
    // 1970-01-01 → days since 0000-03-01 (era anchor) = 719468.
    let z = total_days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146097)
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 400)
    let y0 = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 366)
    let mp = (5 * doy + 2) / 153; // [0, 12)
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month_civil = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year_civil = if month_civil <= 2 { y0 + 1 } else { y0 };
    (year_civil, month_civil, day, hour, minute, second)
}

/// Serializes a memo's `anchor_runs` into an inline `<hp:t>…</hp:t>` sequence
/// that lives between `<hp:fieldBegin type="MEMO">` and `<hp:fieldEnd>`.
///
/// Lossy by design: every `RunContent::Text` is concatenated into a single
/// `<hp:t>` element; non-text runs are skipped; the per-run `char_shape_id`
/// is *not* preserved (the surrounding `<hp:run>` already carries a single
/// `charPrIDRef`). 한컴's own HWPX output is the same shape — a memo's
/// anchor is a single `<hp:t>` per `<hp:run>` even when the source HWP5
/// stream split it across char_shape changes.
///
/// Returns `<hp:t/>` for an empty anchor; that path reproduces the
/// pre-Wave-12f point-anchored layout, which 한컴 mis-renders, so the
/// projection layer should always populate `anchor_runs` when a memo's
/// HWP5 `FieldBegin..FieldEnd` span contains text.
///
/// See `.docs/algorithms/2026-06-01_memo_anchor_serialization.md` for the
/// fidelity tradeoff and why we accept it.
fn build_memo_anchor_xml(anchor_runs: &[hwpforge_core::run::Run]) -> String {
    use hwpforge_core::run::RunContent;
    let mut text = String::new();
    for run in anchor_runs {
        if let RunContent::Text(s) = &run.content {
            text.push_str(s);
        }
        // Non-text variants are dropped; a memo anchor cannot wrap
        // tables/images/nested controls in HWPX, and 한컴 does not produce
        // such anchors. If they ever appear, the lossy collapse here is
        // strictly preferable to emitting an empty anchor (the old buggy
        // path) — both bug 1 (wrong anchor position) and bug 2 (`[필드 끝]`
        // mis-label) regress if the anchor is empty.
    }
    if text.is_empty() {
        return "<hp:t/>".to_string();
    }
    build_text_element_xml(&text)
}

/// Encodes memo body paragraphs as an XML string for embedding inside fieldBegin.
///
/// `quick_xml::se::to_string` uses the Rust struct name `HxSubList` as the root
/// element because `HxSubList` has no struct-level serde rename (the `hp:subList`
/// rename lives on parent struct fields). We must fix the root tag manually.
fn encode_memo_sublist(
    paragraphs: &[Paragraph],
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<String> {
    let sublist = encode_paragraphs_to_sublist(paragraphs, depth, hyperlink_entries)?;
    let xml = quick_xml::se::to_string(&sublist)
        .map_err(|e| HwpxError::InvalidStructure { detail: e.to_string() })?;
    // Fix root element: <HxSubList ...>...</HxSubList> → <hp:subList ...>...</hp:subList>
    let xml = xml.replacen("<HxSubList", "<hp:subList", 1);
    let xml = xml.replacen("</HxSubList>", "</hp:subList>", 1);
    Ok(xml)
}

/// Builds the full `<hp:run>` XML for a [`Control::EmbeddedChart`] (Wave 4c).
///
/// Emits the marker-replaced run that 한컴 expects for a HWP5-sourced
/// chart carry:
///
/// ```xml
/// <hp:run charPrIDRef="N">
///   <hp:switch>
///     <hp:case hp:required-namespace="...">
///       <hp:chart id="..." chartIDRef="Chart/chartN.xml">…</hp:chart>
///     </hp:case>
///     <hp:default>
///       <hp:ole id="..." binaryItemIDRef="oleN" …>…</hp:ole>
///     </hp:default>
///   </hp:switch>
///   <hp:t/>
/// </hp:run>
/// ```
///
/// `HxRunSwitch` only models the `<hp:case>` arm, so we cannot reuse the
/// serde path used by the structured [`Control::Chart`] arm. The OPF
/// manifest entry for `BinData/{ole_item_id}.ole` is registered by the
/// `PackageWriter` callsite via [`SectionEncodeResult::embedded_oles`].
#[allow(clippy::too_many_arguments)]
fn build_embedded_chart_run_xml(
    char_pr_id_ref: u32,
    chart_ref: &str,
    ole_item_id: &str,
    width: i32,
    height: i32,
    horz_offset: i32,
    vert_offset: i32,
) -> String {
    // The `id` attributes are render-time instance identifiers; they only
    // need to be unique within the document and stay below i32::MAX (the
    // attribute is read as a signed 32-bit int by Hancom). `generate_instid`
    // already enforces both for us.
    let shared_id = generate_instid();
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:switch>"#,
            // ── <hp:case> with the OOXML chart reference ────────────────
            r#"<hp:case hp:required-namespace="http://www.hancom.co.kr/hwpml/2016/ooxmlchart">"#,
            r#"<hp:chart id="{id}" zOrder="0" numberingType="PICTURE" "#,
            r#"textWrap="SQUARE" textFlow="BOTH_SIDES" lock="0" "#,
            r#"dropcapstyle="None" chartIDRef="{chart_ref}">"#,
            r#"<hp:sz width="{w}" widthRelTo="ABSOLUTE" height="{h}" heightRelTo="ABSOLUTE" protect="0"/>"#,
            r#"<hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="1" "#,
            r#"allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="COLUMN" "#,
            r#"vertAlign="TOP" horzAlign="LEFT" vertOffset="{vo}" horzOffset="{ho}"/>"#,
            r#"<hp:outMargin left="0" right="0" top="0" bottom="0"/>"#,
            r#"</hp:chart>"#,
            r#"</hp:case>"#,
            // ── <hp:default> with the OLE fallback ──────────────────────
            r#"<hp:default>"#,
            r#"<hp:ole id="{id}" zOrder="0" numberingType="PICTURE" "#,
            r#"textWrap="SQUARE" textFlow="BOTH_SIDES" lock="0" "#,
            r#"dropcapstyle="None" href="" groupLevel="0" instid="0" "#,
            r#"objectType="UNKNOWN" binaryItemIDRef="{ole}" hasMoniker="0" "#,
            r#"drawAspect="CONTENT" eqBaseLine="0">"#,
            r#"<hp:offset x="0" y="0"/>"#,
            r#"<hp:orgSz width="7200" height="7200"/>"#,
            r#"<hp:curSz width="0" height="0"/>"#,
            r#"<hp:flip horizontal="0" vertical="0"/>"#,
            r#"<hp:rotationInfo angle="0" centerX="0" centerY="0" rotateimage="1"/>"#,
            r#"<hp:renderingInfo>"#,
            r#"<hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>"#,
            r#"<hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>"#,
            r#"<hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>"#,
            r#"</hp:renderingInfo>"#,
            r#"<hc:extent x="7200" y="7200"/>"#,
            r##"<hp:lineShape color="#000000" width="0" style="NONE" endCap="ROUND" "##,
            r#"headStyle="NORMAL" tailStyle="NORMAL" headfill="0" tailfill="0" "#,
            r#"headSz="SMALL_SMALL" tailSz="SMALL_SMALL" outlineStyle="NORMAL" alpha="0"/>"#,
            r#"<hp:sz width="{w}" widthRelTo="ABSOLUTE" height="{h}" heightRelTo="ABSOLUTE" protect="0"/>"#,
            r#"<hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="1" "#,
            r#"allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="COLUMN" "#,
            r#"vertAlign="TOP" horzAlign="LEFT" vertOffset="{vo}" horzOffset="{ho}"/>"#,
            r#"<hp:outMargin left="0" right="0" top="0" bottom="0"/>"#,
            r#"</hp:ole>"#,
            r#"</hp:default>"#,
            r#"</hp:switch>"#,
            r#"<hp:t/>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        id = shared_id,
        chart_ref = chart_ref,
        ole = ole_item_id,
        w = width,
        h = height,
        vo = vert_offset,
        ho = horz_offset,
    )
}

/// Encodes a Core `Control::Chart` into an `HxRunSwitch` wrapping `HxChart`.
///
/// Charts use `<hp:switch><hp:case><hp:chart>` structure in section XML,
/// referencing a separate OOXML chart XML file in the ZIP archive.
fn encode_chart_switch(ctrl: &Control, chart_ref: &str) -> HxRunSwitch {
    let (width, height) = match ctrl {
        Control::Chart { width, height, .. } => (*width, *height),
        _ => unreachable!("encode_chart_switch called with non-Chart"),
    };

    HxRunSwitch {
        case: Some(HxRunCase {
            required_namespace: "http://www.hancom.co.kr/hwpml/2016/ooxmlchart".to_string(),
            chart: Some(HxChart {
                id: generate_instid(),
                z_order: 0,
                numbering_type: "PICTURE".to_string(),
                text_wrap: "TOP_AND_BOTTOM".to_string(),
                text_flow: "BOTH_SIDES".to_string(),
                lock: 0,
                dropcap_style: DropCapStyle::None.to_string(),
                chart_id_ref: chart_ref.to_string(),
                sz: Some(HxTableSz {
                    width: width.as_i32(),
                    width_rel_to: "ABSOLUTE".to_string(),
                    height: height.as_i32(),
                    height_rel_to: "ABSOLUTE".to_string(),
                    protect: 0,
                }),
                pos: Some(HxTablePos {
                    treat_as_char: 0,
                    affect_l_spacing: 0,
                    flow_with_text: 1,
                    allow_overlap: 0,
                    hold_anchor_and_so: 0,
                    vert_rel_to: "PARA".to_string(),
                    horz_rel_to: "COLUMN".to_string(),
                    vert_align: "TOP".to_string(),
                    horz_align: "LEFT".to_string(),
                    vert_offset: 0,
                    horz_offset: 0,
                }),
                out_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
            }),
        }),
    }
}

/// Converts a Core `Caption` into an `HxCaption`.
///
/// `parent_width` is used for `lastWidth` (= parent object sz.width in HWPUNIT).
pub(crate) fn build_hx_caption(
    caption: &Caption,
    parent_width: i32,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
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
    let sub_list = encode_paragraphs_to_sublist(&caption.paragraphs, depth, hyperlink_entries)?;

    // parent_width comes from HwpUnit::as_i32(), guaranteed non-negative
    Ok(HxCaption { side, full_sz: 0, width, gap, last_width: parent_width as u32, sub_list })
}

/// Generates a unique instance ID string via atomic counter.
///
/// Each call returns a monotonically increasing ID, safe for parallel encoding.
pub(crate) fn generate_instid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static INSTID_COUNTER: AtomicU64 = AtomicU64::new(1);
    INSTID_COUNTER.fetch_add(1, Ordering::Relaxed).to_string()
}

/// Builds `HxSecPr` from Core `PageSettings` and text direction.
fn build_sec_pr(ps: &PageSettings, text_direction: TextDirection) -> HxSecPr {
    let gutter_type_str = match ps.gutter_type {
        hwpforge_foundation::GutterType::LeftOnly => "LEFT_ONLY",
        hwpforge_foundation::GutterType::LeftRight => "LEFT_RIGHT",
        hwpforge_foundation::GutterType::TopOnly => "TOP_ONLY",
        hwpforge_foundation::GutterType::TopBottom => "TOP_BOTTOM",
        _ => "LEFT_ONLY",
    };
    HxSecPr {
        text_direction: text_direction.to_string(),
        master_page_cnt: 0,
        visibility: None,
        line_number_shape: None,
        page_pr: Some(HxPagePr {
            // 한글 실제 동작: WIDELY=portrait (세로), NARROWLY=landscape (가로)
            // KS X 6101 스펙과 반대! (gotcha #3: landscape 값 반전)
            landscape: if ps.landscape { "NARROWLY".to_string() } else { "WIDELY".to_string() },
            width: ps.width.as_i32(),
            height: ps.height.as_i32(),
            gutter_type: gutter_type_str.to_string(),
            margin: Some(HxPageMargin {
                header: ps.header_margin.as_i32(),
                footer: ps.footer_margin.as_i32(),
                gutter: ps.gutter.as_i32(),
                left: ps.margin_left.as_i32(),
                right: ps.margin_right.as_i32(),
                top: ps.margin_top.as_i32(),
                bottom: ps.margin_bottom.as_i32(),
            }),
        }),
        page_border_fills: Vec::new(),
        start_num: None,
    }
}

/// Default inner cell margin (left/right: 510 ≈ 1.8mm, top/bottom: 141 ≈ 0.5mm).
const DEFAULT_CELL_MARGIN: HxTableMargin =
    HxTableMargin { left: 510, right: 510, top: 141, bottom: 141 };

/// Default outer table margin (283 ≈ 1mm on all sides).
const DEFAULT_OUT_MARGIN: HxTableMargin =
    HxTableMargin { left: 283, right: 283, top: 283, bottom: 283 };

/// `borderFillIDRef` for table cells (matches header.xml borderFill id=3).
const TABLE_BORDER_FILL_ID: u32 = 3;

/// Builds `HxTable` from a Core `Table`.
///
/// Populates all attributes and sub-elements required by 한글:
/// `hp:sz`, `hp:pos`, `hp:outMargin`, `hp:inMargin`, plus full
/// attribute set on `<hp:tbl>`.
///
/// Builds `HxPic` from a Core `Image` with complete shape structure.
///
/// The `BinData/` prefix and file extension are stripped from the path
/// to produce the `binaryItemIDRef` attribute value. For example,
/// `"BinData/image1.png"` becomes `"image1"`. This matches 한글's
/// convention where `binaryItemIDRef` is a logical name without extension.
///
/// Generates all required sub-elements (offset, orgSz, curSz, flip,
/// rotationInfo, renderingInfo, imgRect, imgClip, inMargin, imgDim,
/// img, sz, pos, outMargin) to match 한글's expected structure.
fn build_picture(
    img: &Image,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<HxPic> {
    let without_prefix = img.path.strip_prefix("BinData/").unwrap_or(&img.path);
    // Strip extension: "image1.png" → "image1"
    let binary_ref = match without_prefix.rfind('.') {
        Some(dot) => &without_prefix[..dot],
        None => without_prefix,
    };

    let w = img.width.as_i32();
    let h = img.height.as_i32();
    let half_w = w / 2;
    let half_h = h / 2;
    let placement = img.placement.as_ref();

    Ok(HxPic {
        id: generate_instid(),
        z_order: 0,
        numbering_type: "PICTURE".to_string(),
        text_wrap: placement
            .map(|value| value.text_wrap.as_hwpx_str().into_owned())
            .unwrap_or_else(|| "TOP_AND_BOTTOM".to_string()),
        text_flow: placement
            .map(|value| value.text_flow.as_hwpx_str().into_owned())
            .unwrap_or_else(|| "BOTH_SIDES".to_string()),
        lock: 0,
        dropcap_style: DropCapStyle::None.to_string(),
        href: String::new(),
        group_level: 0,
        instid: generate_instid(),
        reverse: 0,

        offset: Some(HxOffset { x: 0, y: 0 }),
        org_sz: Some(HxSizeAttr { width: w, height: h }),
        cur_sz: Some(HxSizeAttr { width: w, height: h }),
        flip: Some(HxFlip { horizontal: 0, vertical: 0 }),
        rotation_info: Some(HxRotationInfo {
            angle: 0,
            center_x: half_w,
            center_y: half_h,
            rotate_image: 1,
        }),
        rendering_info: Some(HxRenderingInfo {
            trans_matrix: HxMatrix::identity(),
            sca_matrix: HxMatrix::identity(),
            rot_matrix: HxMatrix::identity(),
        }),
        img_rect: Some(HxImgRect {
            pt0: HxPoint { x: 0, y: 0 },
            pt1: HxPoint { x: w, y: 0 },
            pt2: HxPoint { x: w, y: h },
            pt3: HxPoint { x: 0, y: h },
        }),
        img_clip: Some(HxImgClip { left: 0, right: w, top: 0, bottom: h }),
        in_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
        img_dim: Some(HxImgDim { dim_width: w, dim_height: h }),
        img: Some(HxImg {
            binary_item_id_ref: binary_ref.to_string(),
            bright: 0,
            contrast: 0,
            effect: "REAL_PIC".to_string(),
            alpha: "0".to_string(),
        }),
        sz: Some(HxTableSz {
            width: w,
            width_rel_to: "ABSOLUTE".to_string(),
            height: h,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: 0,
        }),
        pos: Some(build_picture_position(placement)),
        out_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
        caption: img
            .caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, depth, hyperlink_entries))
            .transpose()?,
    })
}

fn build_picture_position(placement: Option<&ImagePlacement>) -> HxTablePos {
    match placement {
        Some(value) => HxTablePos {
            treat_as_char: u32::from(value.treat_as_char),
            affect_l_spacing: 0,
            flow_with_text: u32::from(value.flow_with_text),
            allow_overlap: u32::from(value.allow_overlap),
            hold_anchor_and_so: 0,
            vert_rel_to: value.vert_rel_to.as_hwpx_str().into_owned(),
            horz_rel_to: value.horz_rel_to.as_hwpx_str().into_owned(),
            vert_align: "TOP".to_string(),
            horz_align: "LEFT".to_string(),
            vert_offset: value.vert_offset.as_i32(),
            horz_offset: value.horz_offset.as_i32(),
        },
        None => HxTablePos {
            treat_as_char: 1,
            affect_l_spacing: 0,
            flow_with_text: 0,
            allow_overlap: 0,
            hold_anchor_and_so: 0,
            vert_rel_to: "PARA".to_string(),
            horz_rel_to: "PARA".to_string(),
            vert_align: "TOP".to_string(),
            horz_align: "LEFT".to_string(),
            vert_offset: 0,
            horz_offset: 0,
        },
    }
}

// ── Linesegarray placeholder ─────────────────────────────────────

/// Default horizontal size for A4 with 30mm margins (59528 - 8504 - 8504).
const DEFAULT_HORZ_SIZE: i32 = 42520;

// NOTE: linesegarray is intentionally omitted from paragraph output.
// Previously we emitted a 1-seg placeholder, but 한글 uses lineseg data
// for justify alignment layout. Inaccurate values (1 seg for multi-line
// paragraphs) caused character overlap. Omitting it lets 한글 compute
// accurate linesegs from scratch on open.

// ── 한글 compatibility: secPr enrichment ────────────────────────

/// Builds the enriched `<hp:secPr>` opening tag with all attributes 한글 expects.
///
/// `master_page_cnt` is set dynamically from the section's master pages.
/// `textVerticalWidthHead` is `"1"` when text direction is not horizontal, `"0"` otherwise.
fn build_sec_pr_open_enriched(section: &Section) -> String {
    let master_page_cnt = section.master_pages.as_ref().map_or(0, |v| v.len());
    let text_direction = section.text_direction.to_string();
    let vert_width_head =
        if section.text_direction == TextDirection::Horizontal { "0" } else { "1" };
    format!(
        r#"<hp:secPr id="" textDirection="{text_direction}" spaceColumns="1134" tabStop="8000" tabStopVal="4000" tabStopUnit="HWPUNIT" outlineShapeIDRef="1" memoShapeIDRef="0" textVerticalWidthHead="{vert_width_head}" masterPageCnt="{master_page_cnt}">"#,
    )
}

/// Builds sub-elements inserted before `<hp:pagePr>` inside secPr.
///
/// Reads visibility and line number settings from the Section, falling back
/// to 한글 defaults when not specified.
fn build_sec_pr_pre_elements(section: &Section) -> String {
    use std::fmt::Write as _;

    let vis = section.visibility.as_ref().cloned().unwrap_or_default();
    let lns = section.line_number_shape.as_ref().copied().unwrap_or_default();

    let border_str = show_mode_to_hwpx(vis.border);
    let fill_str = show_mode_to_hwpx(vis.fill);

    let mut xml = String::with_capacity(512);
    let _ = write!(xml, r#"<hp:grid lineGrid="0" charGrid="0" wonggojiFormat="0"/>"#);

    // Use section's begin_num if set, otherwise default to all zeros
    let bn = section.begin_num.as_ref();
    let page = bn.map_or(0, |b| b.page);
    let pic = bn.map_or(0, |b| b.pic);
    let tbl = bn.map_or(0, |b| b.tbl);
    let equation = bn.map_or(0, |b| b.equation);
    let _ = write!(
        xml,
        r#"<hp:startNum pageStartsOn="BOTH" page="{page}" pic="{pic}" tbl="{tbl}" equation="{equation}"/>"#,
    );
    let _ = write!(
        xml,
        r#"<hp:visibility hideFirstHeader="{}" hideFirstFooter="{}" hideFirstMasterPage="{}" border="{border_str}" fill="{fill_str}" hideFirstPageNum="{}" hideFirstEmptyLine="{}" showLineNumber="{}"/>"#,
        u8::from(vis.hide_first_header),
        u8::from(vis.hide_first_footer),
        u8::from(vis.hide_first_master_page),
        u8::from(vis.hide_first_page_num),
        u8::from(vis.hide_first_empty_line),
        u8::from(vis.show_line_number),
    );
    let _ = write!(
        xml,
        r#"<hp:lineNumberShape restartType="{}" countBy="{}" distance="{}" startNumber="{}"/>"#,
        lns.restart_type,
        lns.count_by,
        lns.distance.as_i32(),
        lns.start_number,
    );
    xml
}

/// Converts a [`ShowMode`] enum to the HWPX SCREAMING_SNAKE string.
fn show_mode_to_hwpx(mode: hwpforge_foundation::ShowMode) -> &'static str {
    use hwpforge_foundation::ShowMode;
    match mode {
        ShowMode::ShowAll => "SHOW_ALL",
        ShowMode::HideAll => "HIDE_ALL",
        ShowMode::ShowOdd => "SHOW_ODD",
        ShowMode::ShowEven => "SHOW_EVEN",
        _ => "SHOW_ALL",
    }
}

/// Builds sub-elements inserted after `</hp:pagePr>` and before `</hp:secPr>`.
///
/// Reads page border fill entries from the Section, falling back to 한글
/// defaults (3 entries: BOTH/EVEN/ODD with borderFillIDRef=1).
fn build_sec_pr_post_elements(section: &Section) -> String {
    use hwpforge_core::section::PageBorderFillEntry;
    use std::fmt::Write as _;

    let mut xml = String::with_capacity(1024);

    // Footnote/endnote properties — newNum uses begin_num if set
    let footnote_new_num = section.begin_num.as_ref().map_or(1, |b| b.footnote);
    let endnote_new_num = section.begin_num.as_ref().map_or(1, |b| b.endnote);
    let _ = write!(
        xml,
        r#"<hp:footNotePr><hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar=")" supscript="0"/>"#,
    );
    let _ = write!(
        xml,
        r##"<hp:noteLine length="-1" type="SOLID" width="0.12 mm" color="#000000"/>"##,
    );
    let _ = write!(xml, r#"<hp:noteSpacing betweenNotes="283" belowLine="567" aboveLine="850"/>"#,);
    let _ = write!(xml, r#"<hp:numbering type="CONTINUOUS" newNum="{footnote_new_num}"/>"#,);
    let _ = write!(xml, r#"<hp:placement place="EACH_COLUMN" beneathText="0"/></hp:footNotePr>"#,);
    let _ = write!(
        xml,
        r#"<hp:endNotePr><hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar=")" supscript="0"/>"#,
    );
    let _ = write!(
        xml,
        r##"<hp:noteLine length="14692344" type="SOLID" width="0.12 mm" color="#000000"/>"##,
    );
    let _ = write!(xml, r#"<hp:noteSpacing betweenNotes="0" belowLine="567" aboveLine="850"/>"#,);
    let _ = write!(xml, r#"<hp:numbering type="CONTINUOUS" newNum="{endnote_new_num}"/>"#,);
    let _ =
        write!(xml, r#"<hp:placement place="END_OF_DOCUMENT" beneathText="0"/></hp:endNotePr>"#,);

    // Page border fills
    let default_entries = vec![
        PageBorderFillEntry { apply_type: "BOTH".to_string(), ..Default::default() },
        PageBorderFillEntry { apply_type: "EVEN".to_string(), ..Default::default() },
        PageBorderFillEntry { apply_type: "ODD".to_string(), ..Default::default() },
    ];
    let entries = section.page_border_fills.as_deref().unwrap_or(&default_entries);
    for entry in entries {
        let hi = u8::from(entry.header_inside);
        let fi = u8::from(entry.footer_inside);
        let [l, r, t, b] = entry.offset;
        let _ = write!(
            xml,
            r#"<hp:pageBorderFill type="{}" borderFillIDRef="{}" textBorder="{}" headerInside="{hi}" footerInside="{fi}" fillArea="{}">"#,
            entry.apply_type, entry.border_fill_id, entry.text_border, entry.fill_area,
        );
        let _ = write!(
            xml,
            r#"<hp:offset left="{}" right="{}" top="{}" bottom="{}"/>"#,
            l.as_i32(),
            r.as_i32(),
            t.as_i32(),
            b.as_i32(),
        );
        let _ = write!(xml, "</hp:pageBorderFill>");
    }

    xml
}

/// Builds `<hp:masterPage idRef="masterpageN"/>` references for secPr.
fn build_masterpage_refs(section: &Section, masterpage_offset: usize) -> String {
    use std::fmt::Write as _;
    let Some(ref masters) = section.master_pages else {
        return String::new();
    };
    let mut xml = String::new();
    for (i, _mp) in masters.iter().enumerate() {
        let idx = masterpage_offset + i;
        let _ = write!(xml, r#"<hp:masterPage idRef="masterpage{idx}"/>"#);
    }
    xml
}

/// Generates masterpage XML files for a section's master pages.
///
/// Returns `(ZIP path, XML content)` pairs for each master page.
fn build_masterpage_entries(section: &Section, masterpage_offset: usize) -> Vec<(String, String)> {
    use std::fmt::Write as _;
    let Some(ref masters) = section.master_pages else {
        return Vec::new();
    };
    masters
        .iter()
        .enumerate()
        .map(|(i, mp)| {
            let idx = masterpage_offset + i;
            let mp_id = format!("masterpage{idx}");
            let apply_type = match mp.apply_page_type {
                hwpforge_foundation::ApplyPageType::Both => "BOTH",
                hwpforge_foundation::ApplyPageType::Even => "EVEN",
                hwpforge_foundation::ApplyPageType::Odd => "ODD",
                _ => "BOTH",
            };

            let mut xml = String::with_capacity(1024);
            let _ = write!(xml, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>"#);
            // Root element uses NO namespace prefix (like real 한글 output).
            // All 15 xmlns declarations are required.
            let _ = write!(
                xml,
                r#"<masterPage{} id="{mp_id}" type="{apply_type}" pageNumber="0" pageDuplicate="0" pageFront="0">"#,
                super::package::XMLNS_DECLS,
            );
            // subList uses hp: prefix (NOT hm:)
            let _ = write!(
                xml,
                r#"<hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="42520" textHeight="65762" hasTextRef="0" hasNumRef="0">"#,
            );

            for (pidx, para) in mp.paragraphs.iter().enumerate() {
                let _ = write!(
                    xml,
                    r#"<hp:p id="{pidx}" paraPrIDRef="{}" styleIDRef="{}" pageBreak="0" columnBreak="0" merged="0">"#,
                    para.para_shape_id.get(),
                    para.style_id.map_or(0, |s| s.get()),
                );
                for run in &para.runs {
                    // Master page paragraphs emit a stripped-down
                    // run-with-text element. `InlineText` falls
                    // through to its tab-attribute-preserving builder
                    // so a master page that contains rich tabs (rare
                    // but possible after the HWPX decoder Phase 3
                    // carry) survives encode. See debug doc §3a-B11.
                    match &run.content {
                        RunContent::Text(text) => {
                            let _ = write!(
                                xml,
                                r#"<hp:run charPrIDRef="{}"><hp:t>{}</hp:t></hp:run>"#,
                                run.char_shape_id.get(),
                                super::escape_xml(text),
                            );
                        }
                        RunContent::InlineText(it) => {
                            let _ = write!(
                                xml,
                                r#"<hp:run charPrIDRef="{}">{}</hp:run>"#,
                                run.char_shape_id.get(),
                                build_inline_text_element_xml(it),
                            );
                        }
                        _ => {}
                    }
                }
                xml.push_str("</hp:p>");
            }

            xml.push_str("</hp:subList></masterPage>");
            (format!("Contents/masterpage{idx}.xml"), xml)
        })
        .collect()
}

/// Builds `<hp:ctrl><hp:colPr>...</hp:colPr></hp:ctrl>` XML string.
///
/// When `column_settings` is `None`, produces the single-column default
/// matching 한글's standard output. Otherwise generates multi-column
/// XML with the appropriate attributes and optional `<hp:col>` children.
fn build_col_pr_xml(column_settings: Option<&ColumnSettings>) -> String {
    match column_settings {
        None => {
            // Single-column default
            concat!(
                r#"<hp:ctrl>"#,
                r#"<hp:colPr id="" type="NEWSPAPER" layout="LEFT" colCount="1" sameSz="1" sameGap="0"/>"#,
                r#"</hp:ctrl>"#,
            )
            .to_string()
        }
        Some(cs) => {
            let col_type = match cs.column_type {
                ColumnType::Newspaper => "NEWSPAPER",
                ColumnType::Parallel => "PARALLEL",
                _ => "NEWSPAPER",
            };
            let layout = match cs.layout_mode {
                ColumnLayoutMode::Left => "LEFT",
                ColumnLayoutMode::Right => "RIGHT",
                ColumnLayoutMode::Mirror => "MIRROR",
                _ => "LEFT",
            };
            let col_count = cs.columns.len();
            let all_same = cs.is_equal_width();

            if all_same {
                // sameSz=1: 한글 calculates equal widths, we just specify gap
                let same_gap = if col_count >= 2 { cs.columns[0].gap.as_i32() } else { 0 };
                format!(
                    r#"<hp:ctrl><hp:colPr id="" type="{col_type}" layout="{layout}" colCount="{col_count}" sameSz="1" sameGap="{same_gap}"/></hp:ctrl>"#
                )
            } else {
                // sameSz=0: explicit <hp:col> children
                let mut xml = format!(
                    r#"<hp:ctrl><hp:colPr id="" type="{col_type}" layout="{layout}" colCount="{col_count}" sameSz="0" sameGap="0">"#
                );
                for col in &cs.columns {
                    xml.push_str(&format!(
                        r#"<hp:col width="{}" gap="{}"/>"#,
                        col.width.as_i32(),
                        col.gap.as_i32()
                    ));
                }
                xml.push_str("</hp:colPr></hp:ctrl>");
                xml
            }
        }
    }
}

/// Enriches the minimal `<hp:secPr>` output with sub-elements required
/// by 한글 for proper rendering.
///
/// Replaces the opening tag with an enriched version carrying all expected
/// attributes, inserts grid/visibility elements before `<hp:pagePr>`,
/// appends footnote/endnote/pageBorderFill after `</hp:pagePr>`,
/// and injects `<hp:ctrl><hp:colPr>` after the closing `</hp:secPr>`.
fn enrich_sec_pr(xml: &str, section: &Section, masterpage_offset: usize) -> String {
    let sec_pr_prefix = r#"<hp:secPr "#;

    // If no secPr to enrich, return as-is
    let Some(start) = xml.find(sec_pr_prefix) else {
        return xml.to_string();
    };

    // Find the closing `>` of the opening tag to replace the entire opening element
    let Some(end) = xml[start..].find('>') else {
        return xml.to_string();
    };
    let minimal_open = &xml[start..start + end + 1];

    let open_enriched = build_sec_pr_open_enriched(section);
    let pre_elements = build_sec_pr_pre_elements(section);
    let post_elements = build_sec_pr_post_elements(section);
    let masterpage_refs = build_masterpage_refs(section, masterpage_offset);

    let mut result = xml.replacen(minimal_open, &format!("{open_enriched}{pre_elements}"), 1);

    // Insert post-elements + masterPage refs before the first </hp:secPr>
    if let Some(pos) = result.find("</hp:secPr>") {
        result.insert_str(pos, &format!("{post_elements}{masterpage_refs}"));
    }

    // Insert colPr after </hp:secPr>
    if let Some(pos) = result.find("</hp:secPr>") {
        let insert_pos = pos + "</hp:secPr>".len();
        let col_pr = build_col_pr_xml(section.column_settings.as_ref());
        result.insert_str(insert_pos, &col_pr);
    }

    result
}

// ── Header/Footer/PageNumber injection ──────────────────────────

/// Injects header, footer, and page number `<hp:ctrl>` blocks into
/// the section XML after the colPr ctrl (in the first run).
///
/// In real HWPX from 한글, these appear as:
/// - `<hp:ctrl><hp:header><hp:p>...</hp:p></hp:header></hp:ctrl>`
/// - `<hp:ctrl><hp:footer><hp:p>...</hp:p></hp:footer></hp:ctrl>`
/// - `<hp:ctrl><hp:autoNum numType="PAGE" ...></hp:ctrl>`
fn inject_header_footer_pagenum(
    xml: &mut String,
    section: &Section,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<()> {
    // Find insertion point: after the last </hp:ctrl> that contains colPr
    // (or after </hp:secPr> if no colPr).
    // We inject after the colPr ctrl block.
    let insert_pos = find_ctrl_injection_point(xml);
    if insert_pos == 0 {
        return Ok(()); // no suitable injection point found
    }

    let mut injection = String::new();

    // Header — emit each `<hp:header>` element preserving the HWPX
    // multi-cardinality wire shape (ADR-002).
    for header in &section.headers {
        injection.push_str(&build_header_xml(header, "header", hyperlink_entries)?);
    }

    // Footer — same cardinality model.
    for footer in &section.footers {
        injection.push_str(&build_header_xml(footer, "footer", hyperlink_entries)?);
    }

    // Page number
    if let Some(ref page_number) = section.page_number {
        injection.push_str(&build_page_number_xml(page_number));
    }

    if !injection.is_empty() {
        xml.insert_str(insert_pos, &injection);
    }

    Ok(())
}

/// Finds the insertion point for header/footer/pagenum ctrl blocks.
///
/// Returns the byte offset after the colPr `</hp:ctrl>` block.
/// Falls back to after `</hp:secPr>` if no colPr is found.
///
/// NOTE: `<hp:colPr>` is typically emitted as a self-closing element
/// (`<hp:colPr .../>`). Looking for `</hp:colPr>` fails in that case and
/// causes controls (pageNum/header/footer) to be injected before colPr.
/// We anchor on the `<hp:colPr` start tag and then find the enclosing
/// `</hp:ctrl>`.
fn find_ctrl_injection_point(xml: &str) -> usize {
    // Look for colPr ctrl: find "<hp:colPr" and then the next "</hp:ctrl>"
    // so both self-closing and expanded colPr forms are supported.
    if let Some(col_pr_pos) = xml.find("<hp:colPr") {
        if let Some(ctrl_close) = xml[col_pr_pos..].find("</hp:ctrl>") {
            return col_pr_pos + ctrl_close + "</hp:ctrl>".len();
        }
    }
    // Fallback: after </hp:secPr>
    if let Some(sec_pr_end) = xml.find("</hp:secPr>") {
        return sec_pr_end + "</hp:secPr>".len();
    }
    0
}

/// Builds `<hp:ctrl><hp:header>` or `<hp:ctrl><hp:footer>` XML.
///
/// `tag_name` should be `"header"` or `"footer"`.
fn build_header_xml(
    hf: &hwpforge_core::section::HeaderFooter,
    tag_name: &str,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<String> {
    use std::fmt::Write as _;

    let apply_page = match hf.apply_page_type {
        hwpforge_foundation::ApplyPageType::Both => "BOTH",
        hwpforge_foundation::ApplyPageType::Even => "EVEN",
        hwpforge_foundation::ApplyPageType::Odd => "ODD",
        _ => "BOTH",
    };

    let hf_id = generate_instid();
    let mut xml = String::new();
    write!(xml, r#"<hp:ctrl><hp:{tag_name} applyPageType="{apply_page}" id="{hf_id}">"#,)
        .expect("write to String is infallible");
    xml.push_str(&encode_memo_sublist(&hf.paragraphs, 0, hyperlink_entries)?);
    write!(xml, "</hp:{tag_name}></hp:ctrl>").expect("write to String is infallible");
    Ok(xml)
}

/// Builds `<hp:ctrl><hp:pageNum>` XML for page numbers.
///
/// Uses the HWPX `<hp:pageNum>` element (not `<hp:autoNum>`) which is
/// the correct representation for page number controls. The `pos` attribute
/// specifies where the page number appears, `formatType` controls the
/// numbering style, and `sideChar` adds surrounding characters.
fn build_page_number_xml(pn: &hwpforge_core::section::PageNumber) -> String {
    use std::fmt::Write as _;

    let pos = match pn.position {
        hwpforge_foundation::PageNumberPosition::None => "NONE",
        hwpforge_foundation::PageNumberPosition::TopLeft => "TOP_LEFT",
        hwpforge_foundation::PageNumberPosition::TopCenter => "TOP_CENTER",
        hwpforge_foundation::PageNumberPosition::TopRight => "TOP_RIGHT",
        hwpforge_foundation::PageNumberPosition::BottomLeft => "BOTTOM_LEFT",
        hwpforge_foundation::PageNumberPosition::BottomCenter => "BOTTOM_CENTER",
        hwpforge_foundation::PageNumberPosition::BottomRight => "BOTTOM_RIGHT",
        hwpforge_foundation::PageNumberPosition::OutsideTop => "OUTSIDE_TOP",
        hwpforge_foundation::PageNumberPosition::OutsideBottom => "OUTSIDE_BOTTOM",
        hwpforge_foundation::PageNumberPosition::InsideTop => "INSIDE_TOP",
        hwpforge_foundation::PageNumberPosition::InsideBottom => "INSIDE_BOTTOM",
        _ => "BOTTOM_CENTER",
    };

    let format_type = match pn.number_format {
        hwpforge_foundation::NumberFormatType::Digit => "DIGIT",
        hwpforge_foundation::NumberFormatType::CircledDigit => "CIRCLED_DIGIT",
        hwpforge_foundation::NumberFormatType::RomanCapital => "ROMAN_CAPITAL",
        hwpforge_foundation::NumberFormatType::RomanSmall => "ROMAN_SMALL",
        hwpforge_foundation::NumberFormatType::LatinCapital => "LATIN_CAPITAL",
        hwpforge_foundation::NumberFormatType::LatinSmall => "LATIN_SMALL",
        hwpforge_foundation::NumberFormatType::CircledLatinSmall => "CIRCLED_LATIN_SMALL",
        hwpforge_foundation::NumberFormatType::HangulSyllable => "HANGUL_SYLLABLE",
        hwpforge_foundation::NumberFormatType::HangulJamo => "HANGUL_JAMO",
        hwpforge_foundation::NumberFormatType::HanjaDigit => "HANJA_DIGIT",
        _ => "DIGIT",
    };

    let mut xml = String::new();
    write!(
        xml,
        r#"<hp:ctrl><hp:pageNum pos="{pos}" formatType="{format_type}" sideChar="{side_char}"/></hp:ctrl>"#,
        side_char = escape_xml(&pn.decoration),
    )
    .expect("write to String is infallible");
    xml
}

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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

        // Should parse without error
        assert!(xml.contains("<hs:sec"), "missing root element");
        assert!(xml.contains("</hs:sec>"), "missing close tag");
    }

    // ── Test 10: Korean text preservation ────────────────────────

    #[test]
    fn korean_text_preservation() {
        let korean = "우리는 수학을 공부한다.";
        let section = simple_section(korean);
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
        let err = build_table(&hx_table, MAX_NESTING_DEPTH, &mut Vec::new()).unwrap_err();
        match &err {
            HwpxError::InvalidStructure { detail } => {
                assert!(detail.contains("nesting depth"));
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
        let hx = build_picture(&img, 0, &mut Vec::new()).unwrap();
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

        let hx = build_picture(&img, 0, &mut Vec::new()).unwrap();
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
        let hx = build_picture(&img, 0, &mut Vec::new()).unwrap();
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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

        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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

        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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

        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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

        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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

        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
                        Control::Footnote { inst_id: Some(42), paragraphs: vec![footnote_para] },
                        CharShapeIndex::new(0),
                    ),
                ],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
                    },
                    CharShapeIndex::new(0),
                )],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
                        Control::Footnote { inst_id: Some(7), paragraphs: vec![footnote_para] },
                        CharShapeIndex::new(1),
                    ),
                ],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );

        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
                    assert_eq!(*inst_id, Some(7));
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

        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
                    },
                    CharShapeIndex::new(0),
                )],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );

        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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

        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        assert_eq!(result.paragraphs[0].style_id, Some(StyleIndex::new(3)));
    }

    #[test]
    fn decoder_zero_style_id_ref_gives_none() {
        let section = simple_section("normal");
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;

        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        assert_eq!(result.paragraphs[0].style_id, None);
    }

    // ── TextDirection tests ──────────────────────────────────────

    #[test]
    fn text_direction_horizontal_is_default() {
        let section = simple_section("가로쓰기");
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"landscape="NARROWLY""#), "landscape=true must encode as NARROWLY");
    }

    #[test]
    fn portrait_encodes_as_widely() {
        let ps = PageSettings { landscape: false, ..PageSettings::a4() };
        let section = Section::with_paragraphs(vec![text_paragraph("portrait", 0, 0)], ps);
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"landscape="WIDELY""#), "landscape=false must encode as WIDELY");
    }

    #[test]
    fn landscape_roundtrips() {
        let ps = PageSettings { landscape: true, ..PageSettings::a4() };
        let section = Section::with_paragraphs(vec![text_paragraph("land", 0, 0)], ps);
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"gutterType="LEFT_RIGHT""#));
    }

    #[test]
    fn gutter_type_top_only_encodes() {
        use hwpforge_foundation::GutterType;
        let ps = PageSettings { gutter_type: GutterType::TopOnly, ..PageSettings::a4() };
        let section = Section::with_paragraphs(vec![text_paragraph("gutter", 0, 0)], ps);
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"gutterType="TOP_ONLY""#));
    }

    // ── Visibility encoding ──────────────────────────────────────

    #[test]
    fn visibility_defaults_encode() {
        let section = simple_section("text");
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"restartType="1""#));
        assert!(xml.contains(r#"countBy="5""#));
        assert!(xml.contains(r#"distance="1000""#));
        assert!(xml.contains(r#"startNumber="3""#));
    }

    #[test]
    fn line_number_shape_defaults_encode() {
        // Section with no line_number_shape uses all-zero defaults
        let section = simple_section("text");
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"restartType="0""#));
        assert!(xml.contains(r#"countBy="0""#));
        assert!(xml.contains(r#"startNumber="0""#));
    }

    // ── PageBorderFillEntry encoding ─────────────────────────────

    #[test]
    fn page_border_fill_defaults_encode_three_entries() {
        let section = simple_section("text");
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let result = encode_section(&section, 0, 0, 0, 0).unwrap();
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
        let result = encode_section(&section, 0, 0, 5, 0).unwrap();
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
        let result = encode_section(&section, 0, 0, 0, 0).unwrap();
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"pageBreak="1""#), "page_break=true must encode as pageBreak=1");
    }

    #[test]
    fn column_break_encodes_as_one() {
        let mut para = text_paragraph("col break", 0, 0);
        para.column_break = true;
        let section =
            Section::with_paragraphs(vec![text_paragraph("first", 0, 0), para], PageSettings::a4());
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"columnBreak="1""#));
    }

    #[test]
    fn page_break_roundtrips() {
        let mut para = text_paragraph("break", 0, 0);
        para.page_break = true;
        let section =
            Section::with_paragraphs(vec![text_paragraph("first", 0, 0), para], PageSettings::a4());
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(xml.contains("<hp:indexmark"), "indexmark element required");
        assert!(xml.contains("색인항목"), "primary key must be present");
        assert!(xml.contains("부항목"), "secondary key must be present");
    }

    // ── Field encoding ────────────────────────────────────────────

    #[test]
    fn field_pagenum_produces_autonum() {
        // Wave 12n: PageNum moved from FieldType::PageNum to Control::InlinePageNumber.
        use hwpforge_core::control::{Control, InlinePageKind};
        let ctrl = Control::InlinePageNumber { kind: InlinePageKind::CurrentPage, raw_flag: 0 };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(
            xml.contains(r#"<hp:autoNum num="1" numType="PAGE">"#),
            "autoNum for InlinePageNumber"
        );
        assert!(xml.contains("<hp:autoNumFormat"), "autoNumFormat required");
    }

    #[test]
    fn field_date_produces_summery_type() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::ModifiedTime,
            hint_text: None,
            help_text: None,
            name: None,
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        // Date uses SUMMERY type (한글 typo)
        assert!(xml.contains(r#"type="SUMMERY""#), "Date field must use SUMMERY type");
        assert!(xml.contains(r#"fieldid="628321650""#), "Date field must use fieldid 628321650");
        assert!(xml.contains("$modifiedtime"), "Date field Command must be $modifiedtime");
    }

    #[test]
    fn field_time_produces_summery_createtime() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::CreatedTime,
            hint_text: None,
            help_text: None,
            name: None,
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"type="SUMMERY""#));
        assert!(xml.contains("$createtime"));
    }

    #[test]
    fn field_docsummary_produces_summery_author() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::Author,
            hint_text: None,
            help_text: None,
            name: None,
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"type="SUMMERY""#));
        assert!(xml.contains("$author"));
    }

    #[test]
    fn field_userinfo_produces_summery_lastsaveby() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::LastSavedBy,
            hint_text: None,
            help_text: None,
            name: None,
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"type="CLICK_HERE""#), "ClickHere field type");
        assert!(xml.contains(r#"fieldid="627272811""#), "ClickHere fieldid");
        assert!(xml.contains("클릭하세요"), "hint text must appear");
    }

    // ── Wave 12n LOSSY-policy round-trip tests ──────────────────────
    //
    // These tests pin the *intentional* lossy mapping documented in the
    // encoder arms for DateCodeField / PathField / UnknownSummery. They
    // exist so a future encoder change that silently fixes round-trip
    // (e.g. switching to a different HWPX representation) is caught and
    // the lossy-policy comments can be updated rather than left stale.

    #[test]
    fn lossy_datecodefield_emits_summery_token() {
        // %dte time-mode → $createtime SUMMERY (lossy; raw_command kept as display).
        use hwpforge_core::control::Control;
        let ctrl = Control::DateCodeField {
            raw_command: "T\\:H:mm;0;".to_string(),
            is_time_mode: true,
            raw_trailer: [0; 8],
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"type="SUMMERY""#), "DateCodeField surrogates SUMMERY");
        assert!(xml.contains("$createtime"), "time-mode → $createtime token");
    }

    #[test]
    fn pathfield_emits_native_path_wire() {
        // Wave 12n Step 6 — PathField now emits Hancom-native
        // `type="PATH"` with `Format=` param, distinct `fieldid`, and
        // `editable="0"`. Replaces the prior LOSSY SUMMERY surrogate
        // (`lossy_pathfield_emits_summery_with_raw_command`).
        use hwpforge_core::control::{Control, PathFieldCommand};
        let ctrl = Control::PathField { command: PathFieldCommand::PathAndFileName };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
    fn lossy_unknown_summery_carries_raw_token() {
        use hwpforge_core::control::Control;
        let ctrl = Control::UnknownSummery { token: "$company".to_string() };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let ctrl = Control::DateCodeField {
            raw_command: "T\\:H:mm;0;".to_string(),
            is_time_mode: true,
            raw_trailer: [0; 8],
        };
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
        let ctrl = Control::DateCodeField {
            raw_command: "\\:1년 2월 3일;0;".to_string(),
            is_time_mode: false,
            raw_trailer: [0; 8],
        };
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
        // (`lossy_roundtrip_pathfield_becomes_unknown_summery`). With
        // the new `type="PATH"` builder + decoder arm, all three typed
        // PathFieldCommand variants round-trip without value loss.
        use hwpforge_core::control::{Control, PathFieldCommand};
        for cmd in
            [PathFieldCommand::PathAndFileName, PathFieldCommand::Path, PathFieldCommand::FileName]
        {
            let decoded =
                lossy_roundtrip_decode_first_control(Control::PathField { command: cmd.clone() });
            match decoded {
                Control::PathField { command } => {
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
        // `UnknownSummery` like the prior LOSSY policy).
        use hwpforge_core::control::{Control, PathFieldCommand};
        let ctrl = Control::PathField { command: PathFieldCommand::Unknown("$X".to_string()) };
        let decoded = lossy_roundtrip_decode_first_control(ctrl);
        match decoded {
            Control::PathField { command } => match command {
                PathFieldCommand::Unknown(s) => assert_eq!(s, "$X"),
                other => panic!("expected PathFieldCommand::Unknown(\"$X\"), got {other:?}"),
            },
            other => panic!("expected Control::PathField, got {other:?}"),
        }
    }

    #[test]
    fn lossy_roundtrip_unknown_summery_preserves_token() {
        use hwpforge_core::control::Control;
        let ctrl = Control::UnknownSummery { token: "$company".to_string() };
        let decoded = lossy_roundtrip_decode_first_control(ctrl);
        match decoded {
            Control::UnknownSummery { token } => {
                assert_eq!(token, "$company", "unknown $token must round-trip verbatim");
            }
            other => panic!("expected UnknownSummery($company), got {other:?}"),
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
    // `raw_flag` values mirror the decoder's hardcoded mapping
    // (PAGE → 0, TOTAL_PAGE → 0x06 — see decoder/section.rs around
    // line 423). The encoder discards `raw_flag` on emission, so the
    // round-trip is lossless only when the input matches what the
    // decoder fabricates on parse.

    #[test]
    fn roundtrip_summery_author_lossless() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::Author,
            hint_text: None,
            help_text: None,
            name: None,
        };
        let decoded = lossy_roundtrip_decode_first_control(ctrl.clone());
        assert_eq!(decoded, ctrl, "SUMMERY $author must round-trip lossless");
    }

    #[test]
    fn roundtrip_summery_lastsavedby_lossless() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::LastSavedBy,
            hint_text: None,
            help_text: None,
            name: None,
        };
        let decoded = lossy_roundtrip_decode_first_control(ctrl.clone());
        assert_eq!(decoded, ctrl, "SUMMERY $lastsaveby must round-trip lossless");
    }

    #[test]
    fn roundtrip_summery_createdtime_lossless() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::CreatedTime,
            hint_text: None,
            help_text: None,
            name: None,
        };
        let decoded = lossy_roundtrip_decode_first_control(ctrl.clone());
        assert_eq!(decoded, ctrl, "SUMMERY $createtime must round-trip lossless");
    }

    #[test]
    fn roundtrip_summery_modifiedtime_lossless() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field {
            field_type: FieldType::ModifiedTime,
            hint_text: None,
            help_text: None,
            name: None,
        };
        let decoded = lossy_roundtrip_decode_first_control(ctrl.clone());
        assert_eq!(decoded, ctrl, "SUMMERY $modifiedtime must round-trip lossless");
    }

    #[test]
    fn roundtrip_summery_title_lossless() {
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
        };
        let decoded = lossy_roundtrip_decode_first_control(ctrl.clone());
        assert_eq!(decoded, ctrl, "SUMMERY $title must round-trip lossless");
    }

    #[test]
    fn roundtrip_inline_pagenumber_currentpage_lossless() {
        use hwpforge_core::control::{Control, InlinePageKind};
        let ctrl = Control::InlinePageNumber { kind: InlinePageKind::CurrentPage, raw_flag: 0 };
        let decoded = lossy_roundtrip_decode_first_control(ctrl.clone());
        assert_eq!(decoded, ctrl, "autoNum PAGE must round-trip lossless");
    }

    #[test]
    fn roundtrip_inline_pagenumber_totalpages_lossless() {
        // Wave 12n architect review CRITICAL gate: TotalPages must not
        // collapse to CurrentPage in either direction. Encoder emits
        // numType="TOTAL_PAGE"; decoder maps it back to TotalPages
        // with raw_flag 0x06.
        use hwpforge_core::control::{Control, InlinePageKind};
        let ctrl = Control::InlinePageNumber { kind: InlinePageKind::TotalPages, raw_flag: 0x06 };
        let decoded = lossy_roundtrip_decode_first_control(ctrl.clone());
        assert_eq!(decoded, ctrl, "autoNum TOTAL_PAGE must round-trip lossless");
    }

    #[test]
    fn roundtrip_inline_pagenumber_unknown_emits_no_autonum() {
        // Encoder skip path: InlinePageNumber{Unknown} must not
        // fabricate an autoNum. Decoder therefore sees no control.
        // If anyone collapses Unknown → CurrentPage, this flips.
        use hwpforge_core::control::{Control, InlinePageKind};
        let ctrl = Control::InlinePageNumber { kind: InlinePageKind::Unknown, raw_flag: 0 };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        use hwpforge_core::control::Control;
        use hwpforge_foundation::{RefContentType, RefType};
        let ctrl = Control::CrossRef {
            target_name: "bookmark1".to_string(),
            ref_type: RefType::default(),
            content_type: RefContentType::default(),
            as_hyperlink: true,
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
            "bookmark1",
            "see bookmark1",
            &RefType::default(),
            &RefContentType::default(),
            true,
            0,
            0,
        );
        assert!(!core_xml.contains(r#"fieldid="0""#), "Core CROSSREF must never emit fieldid=0");
        assert!(
            core_xml.contains(r#"fieldid="1828000000""#),
            "Core CROSSREF fieldid must be the non-zero derived value"
        );
        assert_eq!(
            core_xml.matches(r#"fieldid="1828000000""#).count(),
            2,
            "fieldBegin and fieldEnd must share the same non-zero fieldid"
        );
        assert!(
            core_xml.contains(r#"<hp:fieldEnd beginIDRef="1300000000" fieldid="1828000000"/>"#),
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"type="MEMO""#), "MEMO fieldBegin type");
        assert!(xml.contains("MemoShapeID"), "MemoShapeID param required");
        assert!(!xml.contains("__HWPME_"), "no leftover Memo marker");
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        });
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
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
        });
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"colCount="3""#));
        assert!(xml.contains(r#"sameSz="0""#), "variable width must use sameSz=0");
        // Explicit hp:col children required
        assert!(xml.contains(r#"<hp:col"#));
    }

    // ── days_to_ymd helper ────────────────────────────────────────

    #[test]
    fn days_to_ymd_unix_epoch() {
        // Days 0 = 1970-01-01
        let (y, m, d) = days_to_ymd(0);
        assert_eq!(y, 1970);
        assert_eq!(m, 1);
        assert_eq!(d, 1);
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2026-03-06: days since epoch
        // 2026-01-01 = 365*56 + 14 (leap years 1972..2024) = 20454 days
        // Then + 31 (Jan) + 28 (Feb non-leap) + 5 = 64 → total 20518
        // Use a direct calculation: 2026-03-06 = 20518 days
        let days: u64 = (365 * 56 + 14 + 31 + 28 + 5) as u64; // rough calculation
        let (y, _m, _d) = days_to_ymd(days);
        // Just verify it's in a reasonable range for 2026
        assert!((2025..=2026).contains(&y), "year should be around 2026, got {y}");
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
        let result = encode_section(&section, 0, 0, 0, 0);
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
        let result = encode_section(&section, 0, 0, 0, 0);
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
        let result = encode_section(&section, 0, 0, 0, 0).expect("schemeless URL must be accepted");
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
        let result = encode_section(&section, 0, 0, 0, 0).unwrap();
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
        let result = encode_section(&section, 0, 5, 0, 0).unwrap();
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
            &build_field_run_xml(&FieldType::ClickHere, "", "", "", 0, big),
            "field_run/CLICK_HERE",
        );
        assert_ids_under_limit(
            &build_field_run_xml(&FieldType::ModifiedTime, "", "", "", 0, big),
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
                "bookmark1",
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
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(xml.contains("<hp:titleMark"), "titleMark required for headings");
        assert!(xml.contains(r#"ignore="false""#));
    }

    #[test]
    fn no_heading_level_no_title_mark() {
        let section = simple_section("Normal paragraph");
        let xml = encode_section(&section, 0, 0, 0, 0).unwrap().xml;
        assert!(!xml.contains("<hp:titleMark"), "non-heading must NOT have titleMark");
    }
}
