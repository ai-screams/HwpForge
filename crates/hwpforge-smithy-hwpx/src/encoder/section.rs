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

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::column::{ColumnLayoutMode, ColumnSettings, ColumnType};
use hwpforge_core::control::{Control, DutmalAlign, DutmalPosition};
use hwpforge_core::image::Image;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;

use crate::encoder::package::XMLNS_DECLS;
use crate::error::{HwpxError, HwpxResult};
use hwpforge_foundation::{BookmarkType, DropCapStyle, TextDirection};

use crate::schema::section::{
    HxBookmark, HxCaption, HxCellAddr, HxCellSpan, HxCellSz, HxChart, HxCompose, HxComposeCharPr,
    HxCtrl, HxDutmal, HxEquation, HxFlip, HxFootNote, HxImg, HxImgClip, HxImgDim, HxImgRect,
    HxIndexMark, HxMatrix, HxOffset, HxPageMargin, HxPagePr, HxParagraph, HxPic, HxPoint,
    HxRenderingInfo, HxRotationInfo, HxRun, HxRunCase, HxRunSwitch, HxScript, HxSecPr, HxSection,
    HxShapeComment, HxSizeAttr, HxSubList, HxTable, HxTableCell, HxTableMargin, HxTablePos,
    HxTableRow, HxTableSz, HxText, HxTitleMark,
};

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

/// Result of encoding a section, including chart and masterpage entries for ZIP packaging.
#[derive(Debug)]
pub(crate) struct SectionEncodeResult {
    /// The section XML string.
    pub xml: String,
    /// Chart entries: (ZIP path, OOXML chart XML content).
    pub charts: Vec<(String, String)>,
    /// Master page entries: (ZIP path, masterpage XML content).
    pub master_pages: Vec<(String, String)>,
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
) -> HwpxResult<SectionEncodeResult> {
    let mut chart_entries: Vec<(String, String)> = Vec::new();
    let mut hyperlink_entries: Vec<(String, String)> = Vec::new();
    let hx_section =
        build_section(section, &mut chart_entries, &mut hyperlink_entries, chart_offset)?;
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
    inject_header_footer_pagenum(&mut enriched, section);

    // Replace hyperlink placeholder runs with real interleaved XML.
    // Serde cannot express the ctrl-text-ctrl interleaving required by
    // HWPX fieldBegin/fieldEnd, so we serialize a marker and swap it here.
    for (marker_xml, real_xml) in &hyperlink_entries {
        enriched = enriched.replacen(marker_xml, real_xml, 1);
    }

    // Generate masterpage XML files
    let master_pages = build_masterpage_entries(section, masterpage_offset);

    Ok(SectionEncodeResult {
        xml: wrap_section_xml(&enriched),
        charts: chart_entries,
        master_pages,
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
    hyperlink_entries: &mut Vec<(String, String)>,
    chart_offset: usize,
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
                hyperlink_entries,
                chart_offset,
            )
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
    hyperlink_entries: &mut Vec<(String, String)>,
    chart_offset: usize,
) -> HwpxResult<HxParagraph> {
    let mut runs = build_runs(
        &para.runs,
        inject_sec_pr,
        page_settings,
        text_direction,
        depth,
        chart_entries,
        hyperlink_entries,
        chart_offset,
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
    hyperlink_entries: &mut Vec<(String, String)>,
    chart_offset: usize,
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
                texts.push(HxText { text: s.clone() });
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
                    Control::Hyperlink { text, url } => {
                        // Hyperlinks require interleaved ctrl-text-ctrl inside a
                        // single <hp:run> (fieldBegin → text → fieldEnd). Serde
                        // cannot express this ordering, so we emit a placeholder
                        // run with a unique marker and replace it after
                        // serialization in `encode_section`.
                        // Validate URL scheme before encoding
                        if !super::is_safe_url(url) {
                            return Err(crate::error::HwpxError::InvalidStructure {
                                detail: format!(
                                    "Unsafe URL scheme in hyperlink: '{url}'. Only http://, https://, and mailto: are allowed."
                                ),
                            });
                        }
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPHL", field_id);
                        let real_xml = build_hyperlink_run_xml(text, url, char_pr_id_ref, field_id);
                        // The marker run will serialize to something like
                        // <hp:run charPrIDRef="N"><hp:t>__HWPFORGE_HYPERLINK_0__</hp:t></hp:run>
                        // We record the full serialized marker run pattern so the
                        // replacement in encode_section is exact.
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText { text: marker });
                    }
                    Control::Dutmal { main_text, sub_text, position, sz_ratio, align } => {
                        dutmals.push(encode_dutmal_to_hx(
                            main_text, sub_text, *position, *sz_ratio, *align,
                        ));
                    }
                    Control::Compose { compose_text, circle_type, char_sz, compose_type } => {
                        composes.push(encode_compose_to_hx(
                            compose_text,
                            circle_type,
                            *char_sz,
                            compose_type,
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
                        texts.push(HxText { text: marker });
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
                            texts.push(HxText { text: marker });
                        }
                        // Silently skip if no matching SpanStart found
                    }
                    Control::Field { field_type, hint_text, help_text } => {
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPFD", field_id);
                        let hint = hint_text.as_deref().unwrap_or("");
                        // PageNum uses <hp:autoNum> (NOT fieldBegin/fieldEnd).
                        let real_xml = if *field_type == hwpforge_foundation::FieldType::PageNum {
                            build_autonum_run_xml(char_pr_id_ref)
                        } else {
                            build_field_run_xml(
                                field_type,
                                hint,
                                help_text.as_deref().unwrap_or(""),
                                char_pr_id_ref,
                                field_id,
                            )
                        };
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText { text: marker });
                    }
                    Control::CrossRef { target_name, ref_type, content_type, as_hyperlink } => {
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPXR", field_id);
                        let real_xml = build_crossref_run_xml(
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
                        texts.push(HxText { text: marker });
                    }
                    Control::Memo { content, author, date } => {
                        let field_id = hyperlink_entries.len();
                        let marker = next_marker("HWPME", field_id);
                        let sublist_xml = encode_memo_sublist(content, depth, hyperlink_entries)?;
                        let real_xml = build_memo_run_xml(
                            &sublist_xml,
                            author,
                            date,
                            char_pr_id_ref,
                            field_id,
                        );
                        let marker_run_xml = format!(
                            r#"<hp:run charPrIDRef="{char_pr_id_ref}"><hp:t>{marker}</hp:t></hp:run>"#,
                        );
                        hyperlink_entries.push((marker_run_xml, real_xml));
                        texts.push(HxText { text: marker });
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
    let mut sub_chart_entries = Vec::new();
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
                hyperlink_entries,
                0,
            )
        })
        .collect::<HwpxResult<Vec<_>>>()?;

    Ok(HxSubList {
        id: String::new(),
        text_direction: "HORIZONTAL".to_string(),
        line_wrap: "BREAK".to_string(),
        vert_align: "TOP".to_string(),
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
    encode_line_to_hx, encode_polygon_to_hx, encode_textbox_to_rect,
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
        option: 0,
        style_id_ref: 0,
        align: align_str.to_string(),
        main_text: main_text.to_string(),
        sub_text: sub_text.to_string(),
    }
}

/// Encodes a Core `Control::Compose` into `HxCompose`.
///
/// Always emits 10 `<hp:charPr>` entries with `prIDRef = u32::MAX`
/// (the HWPX sentinel meaning "no override"), as required by KS X 6101.
fn encode_compose_to_hx(
    compose_text: &str,
    circle_type: &str,
    char_sz: i32,
    compose_type: &str,
) -> HxCompose {
    let char_prs = (0..10).map(|_| HxComposeCharPr { pr_id_ref: u32::MAX }).collect();
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
    let escaped_text = escape_xml(text);
    // Unique begin_id per field instance (matches build_field_run_xml pattern).
    // beginIDRef must reference this id, NOT the fieldid.
    let begin_id = 2_000_000_000_u64 + field_id as u64;
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
            r#"<hp:t>{txt}</hp:t>"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="{fid}"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        fid = field_id,
        url = escaped_url,
        cat = category,
        txt = escaped_text,
    )
}

/// Builds a `<hp:run>` XML string for a span bookmark (fieldBegin/fieldEnd).
/// Builds a `<hp:run>` containing only `<hp:fieldBegin>` for bookmark span start.
///
/// The matching `<hp:fieldEnd>` is emitted by [`build_bookmark_span_end_run_xml`].
/// Text between them (in separate runs) is covered by the bookmark span.
fn build_bookmark_span_start_run_xml(name: &str, char_pr_id_ref: u32, field_id: usize) -> String {
    let escaped_name = escape_xml(name);
    let begin_id = 3_000_000_000_u64 + field_id as u64;
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
        fid = field_id,
        name = escaped_name,
    )
}

/// Builds a `<hp:run>` containing only `<hp:fieldEnd>` for bookmark span end.
fn build_bookmark_span_end_run_xml(char_pr_id_ref: u32, field_id: usize) -> String {
    let begin_id = 3_000_000_000_u64 + field_id as u64;
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
        fid = field_id,
    )
}

/// Builds a `<hp:run>` XML string for a field control.
///
/// # Encoding rules (from reference files):
///
/// - **CLICK_HERE**: `type="CLICK_HERE"`, `fieldid=627272811`, `Command=Clickhere:set:43:...`
/// - **Date/Time/DocSummary/UserInfo**: `type="SUMMERY"` (한글 typo for "Summary"),
///   `fieldid=628321650`, `Command=$modifiedtime`/`$createtime`/`$author`/`$lastsaveby`
/// - **PageNum**: NOT handled here — uses `build_autonum_run_xml()` instead.
fn build_field_run_xml(
    field_type: &hwpforge_foundation::FieldType,
    hint: &str,
    help: &str,
    char_pr_id_ref: u32,
    field_id: usize,
) -> String {
    use hwpforge_foundation::FieldType;

    let escaped_hint = escape_xml(hint);
    let begin_id = 1_000_000_000 + field_id as u64;

    match field_type {
        FieldType::ClickHere => {
            // CLICK_HERE: editable press-field (누름틀)
            let escaped_help = escape_xml(help);
            // Lengths must match the escaped strings embedded in the Command attribute.
            let hint_len = escaped_hint.chars().count();
            let help_len = escaped_help.chars().count();
            let command = format!(
                "Clickhere:set:43:Direction:wstring:{hint_len}:{escaped_hint} HelpState:wstring:{help_len}:{escaped_help}  ",
            );
            format!(
                concat!(
                    r#"<hp:run charPrIDRef="{cpr}">"#,
                    r#"<hp:ctrl>"#,
                    r#"<hp:fieldBegin id="{bid}" type="CLICK_HERE" name="" editable="1" dirty="0" "#,
                    r#"zorder="-1" fieldid="627272811" metaTag="">"#,
                    r#"<hp:parameters cnt="3" name="">"#,
                    r#"<hp:integerParam name="Prop">9</hp:integerParam>"#,
                    r#"<hp:stringParam name="Command" xml:space="preserve">{cmd}</hp:stringParam>"#,
                    r#"<hp:stringParam name="Direction">{hint}</hp:stringParam>"#,
                    r#"</hp:parameters>"#,
                    r#"</hp:fieldBegin>"#,
                    r#"</hp:ctrl>"#,
                    r#"<hp:t>{display}</hp:t>"#,
                    r#"<hp:ctrl>"#,
                    r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="627272811"/>"#,
                    r#"</hp:ctrl>"#,
                    r#"</hp:run>"#,
                ),
                cpr = char_pr_id_ref,
                bid = begin_id,
                cmd = command,
                hint = escaped_hint,
                display = escaped_hint,
            )
        }
        FieldType::Date | FieldType::Time | FieldType::DocSummary | FieldType::UserInfo => {
            // SUMMERY fields (한글 internal type for document summary/date/time).
            // Reference: tests/fixtures/date_field.hwpx
            let (command, display_text) = match field_type {
                FieldType::Date => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let days = now / 86400;
                    let (y, m, d) = days_to_ymd(days);
                    ("$modifiedtime".to_string(), format!("{y}-{m:02}-{d:02}"))
                }
                FieldType::Time => ("$createtime".to_string(), " ".to_string()),
                FieldType::DocSummary => {
                    let text =
                        if !hint.is_empty() { escaped_hint.clone() } else { " ".to_string() };
                    ("$author".to_string(), text)
                }
                FieldType::UserInfo => {
                    let text =
                        if !hint.is_empty() { escaped_hint.clone() } else { " ".to_string() };
                    ("$lastsaveby".to_string(), text)
                }
                _ => unreachable!("outer match arm already guards Date|Time|DocSummary|UserInfo"),
            };
            format!(
                concat!(
                    r#"<hp:run charPrIDRef="{cpr}">"#,
                    r#"<hp:ctrl>"#,
                    r#"<hp:fieldBegin id="{bid}" type="SUMMERY" name="" editable="1" dirty="0" "#,
                    r#"zorder="-1" fieldid="628321650" metaTag="">"#,
                    r#"<hp:parameters cnt="3" name="">"#,
                    r#"<hp:integerParam name="Prop">8</hp:integerParam>"#,
                    r#"<hp:stringParam name="Command">{cmd}</hp:stringParam>"#,
                    r#"<hp:stringParam name="Property">{cmd}</hp:stringParam>"#,
                    r#"</hp:parameters>"#,
                    r#"</hp:fieldBegin>"#,
                    r#"</hp:ctrl>"#,
                    r#"<hp:t>{display}</hp:t>"#,
                    r#"<hp:ctrl>"#,
                    r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="628321650"/>"#,
                    r#"</hp:ctrl>"#,
                    r#"</hp:run>"#,
                ),
                cpr = char_pr_id_ref,
                bid = begin_id,
                cmd = command,
                display = display_text,
            )
        }
        _ => {
            // Fallback: encode as CLICK_HERE for any unknown/future field types.
            build_field_run_xml(&FieldType::ClickHere, hint, help, char_pr_id_ref, field_id)
        }
    }
}

/// Builds a `<hp:run>` XML string for an inline page number (`<hp:autoNum>`).
///
/// Page numbers within body text use `<hp:autoNum numType="PAGE">` — NOT
/// fieldBegin/fieldEnd. Reference: tests/fixtures/date_field.hwpx
fn build_autonum_run_xml(char_pr_id_ref: u32) -> String {
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:autoNum num="1" numType="PAGE">"#,
            r#"<hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar="" supscript="0"/>"#,
            r#"</hp:autoNum>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
    )
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
    let begin_id = 4_000_000_000_u64 + field_id as u64;
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
            r#"<hp:t>{name}</hp:t>"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="{fid}"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        fid = field_id,
        ref_path = ref_path,
        ref_type = ref_type_str,
        content_type = content_type_str,
        hyperlink = hyperlink_val,
        name = escaped_name,
    )
}

/// Builds a `<hp:run>` XML string for a memo annotation.
fn build_memo_run_xml(
    sublist_xml: &str,
    _author: &str,
    _date: &str,
    char_pr_id_ref: u32,
    field_id: usize,
) -> String {
    let begin_id = 5_000_000_000_u64 + field_id as u64;
    format!(
        concat!(
            r#"<hp:run charPrIDRef="{cpr}">"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldBegin id="{bid}" type="MEMO" name="" editable="0" dirty="0" "#,
            r#"zorder="-1" fieldid="{fid}" metaTag="">"#,
            r#"<hp:parameters cnt="2" name="">"#,
            r#"<hp:integerParam name="MemoShapeID">0</hp:integerParam>"#,
            r#"<hp:stringParam name="MemoType">DEFAULT</hp:stringParam>"#,
            r#"</hp:parameters>"#,
            r#"{sublist}"#,
            r#"</hp:fieldBegin>"#,
            r#"</hp:ctrl>"#,
            r#"<hp:t/>"#,
            r#"<hp:ctrl>"#,
            r#"<hp:fieldEnd beginIDRef="{bid}" fieldid="{fid}"/>"#,
            r#"</hp:ctrl>"#,
            r#"</hp:run>"#,
        ),
        cpr = char_pr_id_ref,
        bid = begin_id,
        fid = field_id,
        sublist = sublist_xml,
    )
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
/// # Errors
///
/// Returns [`HwpxError::InvalidStructure`] if nesting depth exceeds
/// [`MAX_NESTING_DEPTH`].
fn build_table(
    table: &Table,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<HxTable> {
    if depth >= MAX_NESTING_DEPTH {
        return Err(HwpxError::InvalidStructure {
            detail: format!("table nesting depth {} exceeds limit of {}", depth, MAX_NESTING_DEPTH,),
        });
    }

    // Build grid occupancy map to compute correct cellAddr for merged cells.
    // Tracks which (row, col) positions are occupied by col_span/row_span.
    let mut occupied = std::collections::HashSet::<(u32, u32)>::new();
    let mut cell_addrs: Vec<Vec<u32>> = Vec::new();
    let mut max_col: u32 = 0;

    for (row_idx, row) in table.rows.iter().enumerate() {
        let mut col_addr: u32 = 0;
        let mut addrs = Vec::new();
        for cell in &row.cells {
            // Skip columns occupied by row_span from previous rows
            while occupied.contains(&(row_idx as u32, col_addr)) {
                col_addr += 1;
            }
            addrs.push(col_addr);
            // Mark all grid positions covered by this cell's span
            for dr in 0..cell.row_span as u32 {
                for dc in 0..cell.col_span as u32 {
                    occupied.insert((row_idx as u32 + dr, col_addr + dc));
                }
            }
            col_addr += cell.col_span as u32;
        }
        if col_addr > max_col {
            max_col = col_addr;
        }
        cell_addrs.push(addrs);
    }
    let col_cnt = max_col;

    let rows = table
        .rows
        .iter()
        .enumerate()
        .map(|(row_idx, row)| {
            build_table_row(row, row_idx as u32, &cell_addrs[row_idx], depth, hyperlink_entries)
        })
        .collect::<HwpxResult<Vec<_>>>()?;

    // Table width: use explicit width or sum of first row's cell widths
    let table_width = table.width.map(|w| w.as_i32()).unwrap_or_else(|| {
        table
            .rows
            .first()
            .map_or(DEFAULT_HORZ_SIZE, |r| r.cells.iter().map(|c| c.width.as_i32()).sum())
    });

    Ok(HxTable {
        id: generate_instid(),
        z_order: 0,
        numbering_type: "TABLE".to_string(),
        text_wrap: "TOP_AND_BOTTOM".to_string(),
        text_flow: "BOTH_SIDES".to_string(),
        lock: 0,
        dropcap_style: DropCapStyle::None.to_string(),
        page_break: "CELL".to_string(),
        repeat_header: 1,
        row_cnt: table.rows.len() as u32,
        col_cnt,
        cell_spacing: 0,
        border_fill_id_ref: TABLE_BORDER_FILL_ID,
        no_adjust: 0,
        sz: Some(HxTableSz {
            width: table_width,
            width_rel_to: "ABSOLUTE".to_string(),
            height: 0,
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
        out_margin: Some(DEFAULT_OUT_MARGIN),
        caption: table
            .caption
            .as_ref()
            .map(|c| build_hx_caption(c, table_width, depth, hyperlink_entries))
            .transpose()?,
        in_margin: Some(DEFAULT_CELL_MARGIN),
        rows,
    })
}

/// Builds `HxTableRow` from a Core `TableRow`.
///
/// `col_addrs` contains the precomputed grid column address for each cell,
/// accounting for col_span/row_span from this and previous rows.
fn build_table_row(
    row: &TableRow,
    row_idx: u32,
    col_addrs: &[u32],
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<HxTableRow> {
    let cells = row
        .cells
        .iter()
        .enumerate()
        .map(|(i, cell)| {
            let col_addr = col_addrs.get(i).copied().unwrap_or(i as u32);
            build_table_cell(cell, col_addr, row_idx, depth, hyperlink_entries)
        })
        .collect::<HwpxResult<Vec<_>>>()?;

    Ok(HxTableRow { cells })
}

/// Builds `HxTableCell` from a Core `TableCell`.
///
/// Cell paragraphs are built recursively at `depth + 1` to track nesting.
/// `col_idx` and `row_idx` are used to populate `<hp:cellAddr>`.
fn build_table_cell(
    cell: &TableCell,
    col_idx: u32,
    row_idx: u32,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<HxTableCell> {
    let paragraphs = cell
        .paragraphs
        .iter()
        .enumerate()
        .map(|(idx, para)| {
            let mut sub_chart_entries = Vec::new();
            build_paragraph(
                para,
                false,
                None,
                TextDirection::Horizontal,
                idx,
                depth + 1,
                &mut sub_chart_entries,
                hyperlink_entries,
                0,
            )
        })
        .collect::<HwpxResult<Vec<_>>>()?;

    Ok(HxTableCell {
        name: String::new(),
        header: 0,
        has_margin: 0,
        protect: 0,
        editable: 0,
        dirty: 0,
        border_fill_id_ref: TABLE_BORDER_FILL_ID,
        sub_list: Some(HxSubList {
            id: String::new(),
            text_direction: "HORIZONTAL".to_string(),
            line_wrap: "BREAK".to_string(),
            vert_align: "CENTER".to_string(),
            link_list_id_ref: 0,
            link_list_next_id_ref: 0,
            text_width: 0,
            text_height: 0,
            has_text_ref: 0,
            has_num_ref: 0,
            paragraphs,
        }),
        cell_addr: Some(HxCellAddr { col_addr: col_idx, row_addr: row_idx }),
        cell_span: Some(HxCellSpan {
            col_span: cell.col_span as u32,
            row_span: cell.row_span as u32,
        }),
        cell_sz: Some(HxCellSz { width: cell.width.as_i32(), height: 0 }),
        cell_margin: Some(DEFAULT_CELL_MARGIN),
    })
}

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

    Ok(HxPic {
        id: generate_instid(),
        z_order: 0,
        numbering_type: "PICTURE".to_string(),
        text_wrap: "TOP_AND_BOTTOM".to_string(),
        text_flow: "BOTH_SIDES".to_string(),
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
        pos: Some(HxTablePos {
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
        }),
        out_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
        caption: img
            .caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, depth, hyperlink_entries))
            .transpose()?,
    })
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
                    if let RunContent::Text(text) = &run.content {
                        let _ = write!(
                            xml,
                            r#"<hp:run charPrIDRef="{}"><hp:t>{}</hp:t></hp:run>"#,
                            run.char_shape_id.get(),
                            super::escape_xml(text),
                        );
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
fn inject_header_footer_pagenum(xml: &mut String, section: &Section) {
    // Find insertion point: after the last </hp:ctrl> that contains colPr
    // (or after </hp:secPr> if no colPr).
    // We inject after the colPr ctrl block.
    let insert_pos = find_ctrl_injection_point(xml);
    if insert_pos == 0 {
        return; // no suitable injection point found
    }

    let mut injection = String::new();

    // Header
    if let Some(ref header) = section.header {
        injection.push_str(&build_header_xml(header, "header"));
    }

    // Footer
    if let Some(ref footer) = section.footer {
        injection.push_str(&build_header_xml(footer, "footer"));
    }

    // Page number
    if let Some(ref page_number) = section.page_number {
        injection.push_str(&build_page_number_xml(page_number));
    }

    if !injection.is_empty() {
        xml.insert_str(insert_pos, &injection);
    }
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
fn build_header_xml(hf: &hwpforge_core::section::HeaderFooter, tag_name: &str) -> String {
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

    // Wrap paragraphs in <hp:subList> (required by HWPX schema)
    // Note: subList id is empty string per reference files (NOT "0")
    xml.push_str(
        r#"<hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0">"#,
    );

    // Encode each paragraph in the header/footer
    for (idx, para) in hf.paragraphs.iter().enumerate() {
        write!(
            xml,
            r#"<hp:p id="{idx}" paraPrIDRef="{}" styleIDRef="{}" pageBreak="0" columnBreak="0" merged="0">"#,
            para.para_shape_id.get(),
            para.style_id.map_or(0, |s| s.get()),
        )
        .expect("write to String is infallible");

        for run in &para.runs {
            if let hwpforge_core::run::RunContent::Text(text) = &run.content {
                write!(
                    xml,
                    r#"<hp:run charPrIDRef="{}"><hp:t>{}</hp:t></hp:run>"#,
                    run.char_shape_id.get(),
                    escape_xml(text),
                )
                .expect("write to String is infallible");
            }
        }

        xml.push_str("</hp:p>");
    }

    xml.push_str("</hp:subList>");
    write!(xml, "</hp:{tag_name}></hp:ctrl>").expect("write to String is infallible");
    xml
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
    use hwpforge_core::image::ImageFormat;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

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

    // ── Test 2: Section roundtrip via decoder ────────────────────

    #[test]
    fn encode_section_roundtrip() {
        let section = simple_section("안녕하세요 round-trip test");
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

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
        let row = TableRow { cells: vec![cell1, cell2], height: None };
        let table = Table::new(vec![row]);

        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

        assert!(xml.contains(r#"rowCnt="1""#), "missing rowCnt");
        assert!(xml.contains(r#"colCnt="2""#), "missing colCnt");
        assert!(xml.contains("<hp:t>Cell1</hp:t>"), "missing Cell1 text");
        assert!(xml.contains("<hp:t>Cell2</hp:t>"), "missing Cell2 text");
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

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
        let inner_table = Table::new(vec![TableRow { cells: vec![inner_cell], height: None }]);

        // Outer table: cell contains a paragraph with the inner table
        let outer_cell = TableCell {
            paragraphs: vec![Paragraph::with_runs(
                vec![Run::table(inner_table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            col_span: 1,
            row_span: 1,
            width: HwpUnit::new(8000).unwrap(),
            background: None,
        };
        let outer_table = Table::new(vec![TableRow { cells: vec![outer_cell], height: None }]);

        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(outer_table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );

        // Should succeed within nesting limit
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

        assert!(xml.contains("<hp:t>before</hp:t>"), "missing 'before' text");
        assert!(xml.contains("<hp:t>after</hp:t>"), "missing 'after' text");

        // Hyperlink must produce fieldBegin/fieldEnd pair
        assert!(xml.contains(r#"type="HYPERLINK"#), "missing HYPERLINK fieldBegin type");
        assert!(xml.contains("https://example.com"), "missing hyperlink URL in parameters");
        assert!(xml.contains("<hp:t>link</hp:t>"), "missing hyperlink display text");
        assert!(xml.contains("<hp:fieldEnd"), "missing fieldEnd closing element");

        // fieldBegin must have unique id and fieldid
        assert!(xml.contains(r#"fieldid="0""#), "fieldBegin must have fieldid=0");
        assert!(xml.contains(r#"id="2000000000""#), "fieldBegin must have unique id");
        // fieldEnd.beginIDRef must reference fieldBegin.id (NOT fieldid)
        assert!(
            xml.contains(r#"beginIDRef="2000000000""#),
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

        // Should parse without error
        assert!(xml.contains("<hs:sec"), "missing root element");
        assert!(xml.contains("</hs:sec>"), "missing close tag");
    }

    // ── Test 10: Korean text preservation ────────────────────────

    #[test]
    fn korean_text_preservation() {
        let korean = "우리는 수학을 공부한다.";
        let section = simple_section(korean);
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

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
    fn paragraph_shape_id_preserved_in_roundtrip() {
        let section = Section::with_paragraphs(
            vec![text_paragraph("p0", 3, 5), text_paragraph("p1", 7, 2)],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let table = Table::new(vec![TableRow { cells: vec![cell], height: None }]);

        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();

        let decoded_img = result.paragraphs[0].runs[0].content.as_image().unwrap();
        // binaryItemIDRef is "photo" (extension stripped by encoder),
        // so decoder reconstructs path as "BinData/photo"
        assert_eq!(decoded_img.path, "BinData/photo");
        assert_eq!(decoded_img.width.as_i32(), 10000);
        assert_eq!(decoded_img.height.as_i32(), 5000);
    }

    // ── Header / Footer / PageNum encoder roundtrip ─────────────

    #[test]
    fn header_roundtrip_via_decoder() {
        use hwpforge_core::section::HeaderFooter;
        use hwpforge_foundation::ApplyPageType;

        let mut section = simple_section("Body text");
        section.header = Some(HeaderFooter::new(
            vec![text_paragraph("Header Content", 0, 0)],
            ApplyPageType::Both,
        ));

        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        section.footer = Some(HeaderFooter::new(
            vec![text_paragraph("Footer Content", 0, 0)],
            ApplyPageType::Even,
        ));

        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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

        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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

        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

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
        section.header =
            Some(HeaderFooter::new(vec![text_paragraph("My Header", 0, 0)], ApplyPageType::Both));
        section.footer =
            Some(HeaderFooter::new(vec![text_paragraph("My Footer", 0, 0)], ApplyPageType::Odd));

        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

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

        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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

        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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

        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        section.header = Some(HeaderFooter::new(
            vec![text_paragraph("A & B < C > D", 0, 0)],
            ApplyPageType::Both,
        ));

        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        assert!(xml.contains(r#"id="2000000000""#), "must have unique id");
        assert!(xml.contains(r#"fieldid="0""#));
        assert!(xml.contains(r#"editable="0""#), "editable must be numeric");
        assert!(xml.contains(r#"dirty="0""#), "dirty must be numeric");
        assert!(xml.contains(r#"metaTag="""#));
        assert!(xml.contains(r#"<hp:stringParam name="Path">https://example.com</hp:stringParam>"#));
        assert!(xml.contains("<hp:t>Click here</hp:t>"));
        assert!(xml.contains(r#"<hp:fieldEnd beginIDRef="2000000000" fieldid="0"/>"#));
        assert!(xml.ends_with("</hp:run>"));
    }

    #[test]
    fn build_hyperlink_run_xml_escapes_special_chars() {
        let xml = build_hyperlink_run_xml("A & B < C", "https://example.com?a=1&b=2", 2, 5);
        assert!(xml.contains(r#"charPrIDRef="2""#));
        assert!(xml.contains(r#"id="2000000005""#), "unique id for field_id=5");
        assert!(xml.contains(r#"fieldid="5""#));
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

        // First hyperlink: id=2000000000, fieldid=0
        assert!(xml.contains(r#"<hp:fieldEnd beginIDRef="2000000000" fieldid="0"/>"#));
        // Second hyperlink: id=2000000001, fieldid=1
        assert!(xml.contains(r#"<hp:fieldEnd beginIDRef="2000000001" fieldid="1"/>"#));
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        assert_eq!(result.paragraphs[0].style_id, Some(StyleIndex::new(3)));
    }

    #[test]
    fn decoder_zero_style_id_ref_gives_none() {
        let section = simple_section("normal");
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;

        let result =
            crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                .unwrap();
        assert_eq!(result.paragraphs[0].style_id, None);
    }

    // ── TextDirection tests ──────────────────────────────────────

    #[test]
    fn text_direction_horizontal_is_default() {
        let section = simple_section("가로쓰기");
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"landscape="NARROWLY""#), "landscape=true must encode as NARROWLY");
    }

    #[test]
    fn portrait_encodes_as_widely() {
        let ps = PageSettings { landscape: false, ..PageSettings::a4() };
        let section = Section::with_paragraphs(vec![text_paragraph("portrait", 0, 0)], ps);
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"landscape="WIDELY""#), "landscape=false must encode as WIDELY");
    }

    #[test]
    fn landscape_roundtrips() {
        let ps = PageSettings { landscape: true, ..PageSettings::a4() };
        let section = Section::with_paragraphs(vec![text_paragraph("land", 0, 0)], ps);
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"gutterType="LEFT_RIGHT""#));
    }

    #[test]
    fn gutter_type_top_only_encodes() {
        use hwpforge_foundation::GutterType;
        let ps = PageSettings { gutter_type: GutterType::TopOnly, ..PageSettings::a4() };
        let section = Section::with_paragraphs(vec![text_paragraph("gutter", 0, 0)], ps);
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"gutterType="TOP_ONLY""#));
    }

    // ── Visibility encoding ──────────────────────────────────────

    #[test]
    fn visibility_defaults_encode() {
        let section = simple_section("text");
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"restartType="1""#));
        assert!(xml.contains(r#"countBy="5""#));
        assert!(xml.contains(r#"distance="1000""#));
        assert!(xml.contains(r#"startNumber="3""#));
    }

    #[test]
    fn line_number_shape_defaults_encode() {
        // Section with no line_number_shape uses all-zero defaults
        let section = simple_section("text");
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"restartType="0""#));
        assert!(xml.contains(r#"countBy="0""#));
        assert!(xml.contains(r#"startNumber="0""#));
    }

    // ── PageBorderFillEntry encoding ─────────────────────────────

    #[test]
    fn page_border_fill_defaults_encode_three_entries() {
        let section = simple_section("text");
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let result = encode_section(&section, 0, 0, 0).unwrap();
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
        let result = encode_section(&section, 0, 0, 5).unwrap();
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
        let result = encode_section(&section, 0, 0, 0).unwrap();
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"pageBreak="1""#), "page_break=true must encode as pageBreak=1");
    }

    #[test]
    fn column_break_encodes_as_one() {
        let mut para = text_paragraph("col break", 0, 0);
        para.column_break = true;
        let section =
            Section::with_paragraphs(vec![text_paragraph("first", 0, 0), para], PageSettings::a4());
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"columnBreak="1""#));
    }

    #[test]
    fn page_break_roundtrips() {
        let mut para = text_paragraph("break", 0, 0);
        para.page_break = true;
        let section =
            Section::with_paragraphs(vec![text_paragraph("first", 0, 0), para], PageSettings::a4());
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(xml.contains("<hp:indexmark"), "indexmark element required");
        assert!(xml.contains("색인항목"), "primary key must be present");
        assert!(xml.contains("부항목"), "secondary key must be present");
    }

    // ── Field encoding ────────────────────────────────────────────

    #[test]
    fn field_pagenum_produces_autonum() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl =
            Control::Field { field_type: FieldType::PageNum, hint_text: None, help_text: None };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"<hp:autoNum num="1" numType="PAGE">"#), "autoNum for PageNum");
        assert!(xml.contains("<hp:autoNumFormat"), "autoNumFormat required");
    }

    #[test]
    fn field_date_produces_summery_type() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field { field_type: FieldType::Date, hint_text: None, help_text: None };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        // Date uses SUMMERY type (한글 typo)
        assert!(xml.contains(r#"type="SUMMERY""#), "Date field must use SUMMERY type");
        assert!(xml.contains(r#"fieldid="628321650""#), "Date field must use fieldid 628321650");
        assert!(xml.contains("$modifiedtime"), "Date field Command must be $modifiedtime");
    }

    #[test]
    fn field_time_produces_summery_createtime() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl = Control::Field { field_type: FieldType::Time, hint_text: None, help_text: None };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"type="SUMMERY""#));
        assert!(xml.contains("$createtime"));
    }

    #[test]
    fn field_docsummary_produces_summery_author() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl =
            Control::Field { field_type: FieldType::DocSummary, hint_text: None, help_text: None };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"type="SUMMERY""#));
        assert!(xml.contains("$author"));
    }

    #[test]
    fn field_userinfo_produces_summery_lastsaveby() {
        use hwpforge_core::control::Control;
        use hwpforge_foundation::FieldType;
        let ctrl =
            Control::Field { field_type: FieldType::UserInfo, hint_text: None, help_text: None };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"type="CLICK_HERE""#), "ClickHere field type");
        assert!(xml.contains(r#"fieldid="627272811""#), "ClickHere fieldid");
        assert!(xml.contains("클릭하세요"), "hint text must appear");
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(xml.contains(r#"type="CROSSREF""#), "CROSSREF fieldBegin type");
        assert!(xml.contains("bookmark1"), "target name must appear");
        assert!(xml.contains("RefHyperLink"), "RefHyperLink param required");
        assert!(!xml.contains("__HWPXR_"), "no leftover CrossRef marker");
    }

    // ── Memo encoding ─────────────────────────────────────────────

    #[test]
    fn memo_encoding() {
        use hwpforge_core::control::Control;
        let ctrl = Control::Memo {
            content: vec![text_paragraph("Memo note", 0, 0)],
            author: "Author".to_string(),
            date: "2026-01-01".to_string(),
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![Run::control(ctrl, CSI::new(0))], PSI::new(0))],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        };
        let xml_result =
            encode_dutmal_to_hx("A", "a", DutmalPosition::Bottom, 75, DutmalAlign::Right);
        assert_eq!(xml_result.pos_type, "BOTTOM");
        assert_eq!(xml_result.align, "RIGHT");
        assert_eq!(xml_result.sz_ratio, 75);
        let _ = ctrl; // suppress unused warning
    }

    #[test]
    fn dutmal_position_left_encodes() {
        use hwpforge_core::control::{DutmalAlign, DutmalPosition};
        let hx = encode_dutmal_to_hx("X", "x", DutmalPosition::Left, 60, DutmalAlign::Left);
        assert_eq!(hx.pos_type, "LEFT");
        assert_eq!(hx.align, "LEFT");
    }

    #[test]
    fn dutmal_position_right_encodes() {
        use hwpforge_core::control::{DutmalAlign, DutmalPosition};
        let hx = encode_dutmal_to_hx("X", "x", DutmalPosition::Right, 60, DutmalAlign::Center);
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
        };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(xml.contains("<hp:compose"), "compose element required");
        assert!(xml.contains(r#"charPrCnt="10""#), "10 charPr entries required");
        assert!(xml.contains("AB"), "compose text required");
    }

    #[test]
    fn encode_compose_has_ten_charpr_entries() {
        let hx = encode_compose_to_hx("AB", "CIRCLE", 100, "COMPOSE");
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
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
    fn build_autonum_run_xml_structure() {
        let xml = build_autonum_run_xml(3);
        assert!(xml.contains(r#"charPrIDRef="3""#));
        assert!(xml.contains(r#"<hp:autoNum num="1" numType="PAGE">"#));
        assert!(xml.contains("<hp:autoNumFormat"));
        assert!(xml.contains(r#"type="DIGIT""#));
        assert!(xml.ends_with("</hp:run>"));
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
        let result = encode_section(&section, 0, 0, 0);
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
        let result = encode_section(&section, 0, 0, 0);
        assert!(result.is_ok(), "mailto: URL must be accepted");
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
        let result = encode_section(&section, 0, 0, 0).unwrap();
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
        let result = encode_section(&section, 0, 5, 0).unwrap();
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
        assert!(xml.contains(r#"fieldid="7""#));
        assert!(xml.ends_with("</hp:run>"));
    }

    #[test]
    fn bookmark_span_end_run_xml_structure() {
        let xml = build_bookmark_span_end_run_xml(1, 3);
        assert!(xml.contains(r#"charPrIDRef="1""#));
        assert!(xml.contains("<hp:fieldEnd"));
        // beginIDRef references the unique id (3_000_000_000 + field_id)
        assert!(xml.contains(r#"beginIDRef="3000000003""#));
        assert!(xml.contains(r#"fieldid="3""#));
        assert!(xml.ends_with("</hp:run>"));
    }

    // ── Heading level (titleMark) encoding ───────────────────────

    #[test]
    fn heading_level_injects_title_mark() {
        let mut para = text_paragraph("Heading", 0, 0);
        para.heading_level = Some(1);
        let section = Section::with_paragraphs(vec![para], PageSettings::a4());
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(xml.contains("<hp:titleMark"), "titleMark required for headings");
        assert!(xml.contains(r#"ignore="false""#));
    }

    #[test]
    fn no_heading_level_no_title_mark() {
        let section = simple_section("Normal paragraph");
        let xml = encode_section(&section, 0, 0, 0).unwrap().xml;
        assert!(!xml.contains("<hp:titleMark"), "non-heading must NOT have titleMark");
    }
}
