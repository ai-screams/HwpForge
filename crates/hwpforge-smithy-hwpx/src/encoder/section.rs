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
use hwpforge_core::control::{Control, ShapeStyle};
use hwpforge_core::image::Image;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;

use crate::encoder::package::XMLNS_DECLS;
use crate::error::{HwpxError, HwpxResult};
use crate::schema::section::{
    HxCaption, HxCellAddr, HxCellSpan, HxCellSz, HxCtrl, HxDrawText, HxEllipse, HxFillBrush,
    HxFlip, HxFootNote, HxImg, HxImgClip, HxImgDim, HxImgRect, HxLine, HxLineSeg, HxLineSegArray,
    HxLineShape, HxMatrix, HxOffset, HxPageMargin, HxPagePr, HxParagraph, HxPic, HxPoint,
    HxPolygon, HxRect, HxRenderingInfo, HxRotationInfo, HxRun, HxSecPr, HxSection, HxShadow,
    HxSizeAttr, HxSubList, HxTable, HxTableCell, HxTableMargin, HxTablePos, HxTableRow, HxTableSz,
    HxText,
};

/// Maximum nesting depth for tables-within-tables.
///
/// Mirrors the decoder's limit. Prevents stack overflow from deeply nested
/// table structures (e.g. a table cell containing another table, ad infinitum).
const MAX_NESTING_DEPTH: usize = 32;

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
pub(crate) fn encode_section(section: &Section, _section_index: usize) -> HwpxResult<String> {
    let hx_section = build_section(section)?;
    let inner_xml = quick_xml::se::to_string(&hx_section)
        .map_err(|e| HwpxError::XmlSerialize { detail: e.to_string() })?;

    // quick_xml produces `<sec>...</sec>` (from the serde rename).
    // We need `<hs:sec xmlns:...>...</hs:sec>`, so strip the outer
    // element and wrap with our template.
    let inner_content = strip_root_element(&inner_xml);

    // Enrich <hp:secPr> with sub-elements required by 한글 (grid,
    // startNum, visibility, footnote/endnote, pageBorderFill).
    let mut enriched = enrich_sec_pr(inner_content, section.column_settings.as_ref());

    // Inject header/footer/page number controls after colPr
    inject_header_footer_pagenum(&mut enriched, section);

    Ok(wrap_section_xml(&enriched))
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
fn build_section(section: &Section) -> HwpxResult<HxSection> {
    let paragraphs = section
        .paragraphs
        .iter()
        .enumerate()
        .map(|(idx, para)| {
            let inject_sec_pr = idx == 0;
            let page_settings = if inject_sec_pr { Some(&section.page_settings) } else { None };
            build_paragraph(para, inject_sec_pr, page_settings, idx, 0)
        })
        .collect::<HwpxResult<Vec<_>>>()?;

    Ok(HxSection { paragraphs })
}

/// Builds an `HxParagraph` from a Core `Paragraph`.
///
/// When `inject_sec_pr` is true (first paragraph of the section), page
/// settings are embedded in the first run's `<hp:secPr>`.
/// `depth` tracks table nesting level for overflow prevention.
fn build_paragraph(
    para: &Paragraph,
    inject_sec_pr: bool,
    page_settings: Option<&PageSettings>,
    para_idx: usize,
    depth: usize,
) -> HwpxResult<HxParagraph> {
    let runs = build_runs(&para.runs, inject_sec_pr, page_settings, depth)?;

    // Build a placeholder linesegarray with approximate values.
    // 한글 will recalculate this on open, but having a placeholder
    // improves initial rendering.
    let horzsize = page_settings
        .map(|ps| ps.width.as_i32() - ps.margin_left.as_i32() - ps.margin_right.as_i32())
        .unwrap_or(DEFAULT_HORZ_SIZE);
    let linesegarray = Some(build_linesegarray(horzsize));

    Ok(HxParagraph {
        id: format!("{para_idx}"),
        para_pr_id_ref: para.para_shape_id.get() as u32,
        style_id_ref: 0,
        page_break: 0,
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
/// Hyperlink and Unknown controls are silently skipped.
///
/// If `inject_sec_pr` is true and `page_settings` is `Some`, the first
/// run gets `<hp:secPr>` attached.
fn build_runs(
    runs: &[Run],
    inject_sec_pr: bool,
    page_settings: Option<&PageSettings>,
    depth: usize,
) -> HwpxResult<Vec<HxRun>> {
    let mut result = Vec::new();
    let mut sec_pr_injected = false;

    for run in runs {
        let sec_pr = if inject_sec_pr && !sec_pr_injected && !run.content.is_control() {
            sec_pr_injected = true;
            page_settings.map(build_sec_pr)
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

        match &run.content {
            RunContent::Text(s) => {
                texts.push(HxText { text: s.clone() });
            }
            RunContent::Table(t) => {
                tables.push(build_table(t, depth)?);
            }
            RunContent::Image(img) => {
                pictures.push(build_picture(img, depth)?);
            }
            RunContent::Control(ctrl) => {
                match ctrl.as_ref() {
                    Control::Footnote { .. } | Control::Endnote { .. } => {
                        if let Some(hx_ctrl) = encode_control_to_ctrl(ctrl, depth)? {
                            ctrls.push(hx_ctrl);
                        }
                    }
                    Control::TextBox { .. } => {
                        rects.push(encode_textbox_to_rect(ctrl, depth)?);
                    }
                    Control::Line { .. } => {
                        lines.push(encode_line_to_hx(ctrl, depth)?);
                    }
                    Control::Ellipse { .. } => {
                        ellipses.push(encode_ellipse_to_hx(ctrl, depth)?);
                    }
                    Control::Polygon { .. } => {
                        polygons.push(encode_polygon_to_hx(ctrl, depth)?);
                    }
                    Control::Hyperlink { .. } | Control::Unknown { .. } => {
                        // Skip for Phase 4.5 (not implemented yet)
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
                    sec_pr: Some(build_sec_pr(ps)),
                    texts: Vec::new(),
                    tables: Vec::new(),
                    pictures: Vec::new(),
                    ctrls: Vec::new(),
                    rects: Vec::new(),
                    lines: Vec::new(),
                    ellipses: Vec::new(),
                    polygons: Vec::new(),
                },
            );
        }
    }

    Ok(result)
}

/// Converts a Core Control (Footnote/Endnote) to `HxCtrl`.
///
/// Returns `None` for non-ctrl controls (TextBox, Hyperlink, Unknown).
fn encode_control_to_ctrl(ctrl: &Control, depth: usize) -> HwpxResult<Option<HxCtrl>> {
    match ctrl {
        Control::Footnote { inst_id, paragraphs } => Ok(Some(HxCtrl {
            foot_note: Some(HxFootNote {
                inst_id: *inst_id,
                sub_list: encode_paragraphs_to_sublist(paragraphs, depth)?,
            }),
            ..Default::default()
        })),
        Control::Endnote { inst_id, paragraphs } => Ok(Some(HxCtrl {
            end_note: Some(HxFootNote {
                inst_id: *inst_id,
                sub_list: encode_paragraphs_to_sublist(paragraphs, depth)?,
            }),
            ..Default::default()
        })),
        _ => Ok(None),
    }
}

/// Encodes a `Vec<Paragraph>` into `HxSubList` with standard defaults.
fn encode_paragraphs_to_sublist(paragraphs: &[Paragraph], depth: usize) -> HwpxResult<HxSubList> {
    let hx_paragraphs = paragraphs
        .iter()
        .enumerate()
        .map(|(idx, para)| build_paragraph(para, false, None, idx, depth + 1))
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

// ── Shape-common helpers ─────────────────────────────────────────

/// Collected common sub-elements required by 한글 for all drawing objects.
///
/// All four shape encoders (rect/textbox, line, ellipse, polygon) produce
/// the same prefix block. This struct avoids repeating the construction logic.
struct ShapeCommon {
    offset: HxOffset,
    org_sz: HxSizeAttr,
    cur_sz: HxSizeAttr,
    flip: HxFlip,
    rotation_info: HxRotationInfo,
    rendering_info: HxRenderingInfo,
    line_shape: HxLineShape,
    fill_brush: HxFillBrush,
    shadow: HxShadow,
}

/// Builds the shape-common block for a drawing object of the given pixel size.
///
/// Defaults match 한글's output for a newly created shape:
/// - zero offset, orgSz = given dimensions, curSz = 0×0
/// - identity rotation/rendering matrices
/// - solid black border, white fill, no shadow
fn build_shape_common(width: i32, height: i32, style: Option<&ShapeStyle>) -> ShapeCommon {
    let mut line_shape = HxLineShape::default_solid();
    let mut fill_brush = HxFillBrush::default_white();

    if let Some(s) = style {
        if let Some(ref c) = s.line_color {
            line_shape.color = c.clone();
        }
        if let Some(w) = s.line_width {
            line_shape.width = w;
        }
        if let Some(ref ls) = s.line_style {
            line_shape.style = ls.clone();
        }
        if let Some(ref c) = s.fill_color {
            fill_brush.win_brush.face_color = c.clone();
        }
    }

    ShapeCommon {
        offset: HxOffset { x: 0, y: 0 },
        org_sz: HxSizeAttr { width, height },
        cur_sz: HxSizeAttr { width: 0, height: 0 },
        flip: HxFlip { horizontal: 0, vertical: 0 },
        rotation_info: HxRotationInfo {
            angle: 0,
            center_x: width / 2,
            center_y: height / 2,
            rotate_image: 1,
        },
        rendering_info: HxRenderingInfo {
            trans_matrix: HxMatrix::identity(),
            sca_matrix: HxMatrix::identity(),
            rot_matrix: HxMatrix::identity(),
        },
        line_shape,
        fill_brush,
        shadow: HxShadow::default_none(),
    }
}

/// Encodes a Core `Control::TextBox` into `HxRect` with `<hp:drawText>`.
///
/// Phase 4.5 MVP: inline positioning (treatAsChar=1) when offsets are (0,0).
fn encode_textbox_to_rect(ctrl: &Control, depth: usize) -> HwpxResult<HxRect> {
    let (paragraphs, width, height, horz_offset, vert_offset, caption, style) = match ctrl {
        Control::TextBox {
            paragraphs,
            width,
            height,
            horz_offset,
            vert_offset,
            caption,
            style,
        } => (paragraphs, *width, *height, *horz_offset, *vert_offset, caption, style),
        _ => unreachable!("encode_textbox_to_rect called with non-TextBox"),
    };

    let width_hwp = width.as_i32();
    let height_hwp = height.as_i32();

    // Default text margin: 283 HWPUNIT (~1mm)
    const MARGIN: i32 = 283;
    let last_width = (width_hwp - 2 * MARGIN).max(0) as u32;

    let sub_list = encode_paragraphs_to_sublist(paragraphs, depth)?;
    let sc = build_shape_common(width_hwp, height_hwp, style.as_ref());

    Ok(HxRect {
        id: generate_instid(),
        z_order: 0,
        numbering_type: "NONE".to_string(),
        text_wrap: "TOP_AND_BOTTOM".to_string(),
        text_flow: "BOTH_SIDES".to_string(),
        lock: 0,
        dropcap_style: "None".to_string(),
        href: String::new(),
        group_level: 0,
        instid: generate_instid(),
        ratio: 0,

        offset: Some(sc.offset),
        org_sz: Some(sc.org_sz),
        cur_sz: Some(sc.cur_sz),
        flip: Some(sc.flip),
        rotation_info: Some(sc.rotation_info),
        rendering_info: Some(sc.rendering_info),
        line_shape: Some(sc.line_shape),
        fill_brush: Some(sc.fill_brush),
        shadow: Some(sc.shadow),

        sz: Some(HxTableSz {
            width: width_hwp,
            width_rel_to: "ABSOLUTE".to_string(),
            height: height_hwp,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: 0,
        }),

        pos: Some(HxTablePos {
            treat_as_char: if horz_offset == 0 && vert_offset == 0 { 1 } else { 0 },
            affect_l_spacing: 0,
            flow_with_text: 0,
            allow_overlap: 0,
            hold_anchor_and_so: 0,
            vert_rel_to: "PARA".to_string(),
            horz_rel_to: "PARA".to_string(),
            vert_align: "TOP".to_string(),
            horz_align: "LEFT".to_string(),
            vert_offset,
            horz_offset,
        }),

        out_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
        caption: caption.as_ref().map(|c| build_hx_caption(c, width_hwp, depth)).transpose()?,

        draw_text: Some(HxDrawText {
            last_width,
            name: String::new(),
            editable: 0,
            sub_list,
            text_margin: Some(HxTableMargin {
                left: MARGIN,
                right: MARGIN,
                top: MARGIN,
                bottom: MARGIN,
            }),
        }),

        pt0: Some(HxPoint { x: 0, y: 0 }),
        pt1: Some(HxPoint { x: width_hwp, y: 0 }),
        pt2: Some(HxPoint { x: width_hwp, y: height_hwp }),
        pt3: Some(HxPoint { x: 0, y: height_hwp }),
    })
}

/// Encodes a Core `Control::Line` into `HxLine`.
fn encode_line_to_hx(ctrl: &Control, depth: usize) -> HwpxResult<HxLine> {
    let (start, end, width, height, caption, style) = match ctrl {
        Control::Line { start, end, width, height, caption, style } => {
            (start, end, *width, *height, caption, style)
        }
        _ => unreachable!("encode_line_to_hx called with non-Line"),
    };

    let w = width.as_i32();
    let h = height.as_i32();
    let sc = build_shape_common(w, h, style.as_ref());

    Ok(HxLine {
        id: generate_instid(),
        z_order: 0,
        numbering_type: "NONE".to_string(),
        text_wrap: "TOP_AND_BOTTOM".to_string(),
        text_flow: "BOTH_SIDES".to_string(),
        lock: 0,
        dropcap_style: "None".to_string(),
        href: String::new(),
        group_level: 0,
        instid: generate_instid(),
        is_reverse_hv: 0,
        offset: Some(sc.offset),
        org_sz: Some(sc.org_sz),
        cur_sz: Some(sc.cur_sz),
        flip: Some(sc.flip),
        rotation_info: Some(sc.rotation_info),
        rendering_info: Some(sc.rendering_info),
        line_shape: Some(sc.line_shape),
        fill_brush: Some(sc.fill_brush),
        shadow: Some(sc.shadow),
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
        shape_comment: None,
        caption: caption.as_ref().map(|c| build_hx_caption(c, w, depth)).transpose()?,
        start_pt: Some(HxPoint { x: start.x, y: start.y }),
        end_pt: Some(HxPoint { x: end.x, y: end.y }),
    })
}

/// Encodes a Core `Control::Ellipse` into `HxEllipse`.
fn encode_ellipse_to_hx(ctrl: &Control, depth: usize) -> HwpxResult<HxEllipse> {
    let (center, axis1, axis2, width, height, horz_offset, vert_offset, paragraphs, caption, style) =
        match ctrl {
            Control::Ellipse {
                center,
                axis1,
                axis2,
                width,
                height,
                horz_offset,
                vert_offset,
                paragraphs,
                caption,
                style,
            } => (
                center,
                axis1,
                axis2,
                *width,
                *height,
                horz_offset,
                vert_offset,
                paragraphs,
                caption,
                style,
            ),
            _ => unreachable!("encode_ellipse_to_hx called with non-Ellipse"),
        };

    let w = width.as_i32();
    let h = height.as_i32();
    let sc = build_shape_common(w, h, style.as_ref());

    let draw_text = if paragraphs.is_empty() {
        None
    } else {
        let sub_list = encode_paragraphs_to_sublist(paragraphs, depth)?;
        Some(HxDrawText {
            last_width: 0,
            name: String::new(),
            editable: 0,
            sub_list,
            text_margin: None,
        })
    };

    Ok(HxEllipse {
        id: generate_instid(),
        z_order: 0,
        numbering_type: "NONE".to_string(),
        text_wrap: "TOP_AND_BOTTOM".to_string(),
        text_flow: "BOTH_SIDES".to_string(),
        lock: 0,
        dropcap_style: "None".to_string(),
        href: String::new(),
        group_level: 0,
        instid: generate_instid(),
        interval_dirty: 0,
        has_arc_pr: 0,
        arc_type: "NORMAL".to_string(),
        offset: Some(sc.offset),
        org_sz: Some(sc.org_sz),
        cur_sz: Some(sc.cur_sz),
        flip: Some(sc.flip),
        rotation_info: Some(sc.rotation_info),
        rendering_info: Some(sc.rendering_info),
        line_shape: Some(sc.line_shape),
        fill_brush: Some(sc.fill_brush),
        shadow: Some(sc.shadow),
        sz: Some(HxTableSz {
            width: w,
            width_rel_to: "ABSOLUTE".to_string(),
            height: h,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: 0,
        }),
        pos: Some(HxTablePos {
            treat_as_char: if *horz_offset == 0 && *vert_offset == 0 { 1 } else { 0 },
            affect_l_spacing: 0,
            flow_with_text: 0,
            allow_overlap: 0,
            hold_anchor_and_so: 0,
            vert_rel_to: "PARA".to_string(),
            horz_rel_to: "PARA".to_string(),
            vert_align: "TOP".to_string(),
            horz_align: "LEFT".to_string(),
            vert_offset: *vert_offset,
            horz_offset: *horz_offset,
        }),
        out_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
        caption: caption.as_ref().map(|c| build_hx_caption(c, w, depth)).transpose()?,
        draw_text,
        center: Some(HxPoint { x: center.x, y: center.y }),
        ax1: Some(HxPoint { x: axis1.x, y: axis1.y }),
        ax2: Some(HxPoint { x: axis2.x, y: axis2.y }),
        start1: Some(HxPoint { x: 0, y: 0 }),
        end1: Some(HxPoint { x: 0, y: 0 }),
        start2: Some(HxPoint { x: 0, y: 0 }),
        end2: Some(HxPoint { x: 0, y: 0 }),
    })
}

/// Encodes a Core `Control::Polygon` into `HxPolygon`.
fn encode_polygon_to_hx(ctrl: &Control, depth: usize) -> HwpxResult<HxPolygon> {
    let (vertices, width, height, paragraphs, caption, style) = match ctrl {
        Control::Polygon { vertices, width, height, paragraphs, caption, style } => {
            (vertices, *width, *height, paragraphs, caption, style)
        }
        _ => unreachable!("encode_polygon_to_hx called with non-Polygon"),
    };

    let w = width.as_i32();
    let h = height.as_i32();
    let sc = build_shape_common(w, h, style.as_ref());

    let draw_text = if paragraphs.is_empty() {
        None
    } else {
        let sub_list = encode_paragraphs_to_sublist(paragraphs, depth)?;
        Some(HxDrawText {
            last_width: 0,
            name: String::new(),
            editable: 0,
            sub_list,
            text_margin: None,
        })
    };

    let points = vertices.iter().map(|v| HxPoint { x: v.x, y: v.y }).collect();

    Ok(HxPolygon {
        id: generate_instid(),
        z_order: 0,
        numbering_type: "NONE".to_string(),
        text_wrap: "TOP_AND_BOTTOM".to_string(),
        text_flow: "BOTH_SIDES".to_string(),
        lock: 0,
        dropcap_style: "None".to_string(),
        href: String::new(),
        group_level: 0,
        instid: generate_instid(),
        offset: Some(sc.offset),
        org_sz: Some(sc.org_sz),
        cur_sz: Some(sc.cur_sz),
        flip: Some(sc.flip),
        rotation_info: Some(sc.rotation_info),
        rendering_info: Some(sc.rendering_info),
        line_shape: Some(sc.line_shape),
        fill_brush: Some(sc.fill_brush),
        shadow: Some(sc.shadow),
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
        caption: caption.as_ref().map(|c| build_hx_caption(c, w, depth)).transpose()?,
        draw_text,
        points,
    })
}

/// Converts a Core `Caption` into an `HxCaption`.
///
/// `parent_width` is used for `lastWidth` (= parent object sz.width in HWPUNIT).
fn build_hx_caption(caption: &Caption, parent_width: i32, depth: usize) -> HwpxResult<HxCaption> {
    let side = match caption.side {
        CaptionSide::Left => "LEFT",
        CaptionSide::Right => "RIGHT",
        CaptionSide::Top => "TOP",
        CaptionSide::Bottom => "BOTTOM",
    }
    .to_string();

    let width = caption.width.map(|w| w.as_i32()).unwrap_or(parent_width);
    let gap = caption.gap.as_i32();
    let sub_list = encode_paragraphs_to_sublist(&caption.paragraphs, depth)?;

    // parent_width comes from HwpUnit::as_i32(), guaranteed non-negative
    Ok(HxCaption { side, full_sz: 0, width, gap, last_width: parent_width as u32, sub_list })
}

/// Generates a unique instance ID string via atomic counter.
///
/// Each call returns a monotonically increasing ID, safe for parallel encoding.
fn generate_instid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static INSTID_COUNTER: AtomicU64 = AtomicU64::new(1);
    INSTID_COUNTER.fetch_add(1, Ordering::Relaxed).to_string()
}

/// Builds `HxSecPr` from Core `PageSettings`.
fn build_sec_pr(ps: &PageSettings) -> HxSecPr {
    HxSecPr {
        text_direction: "HORIZONTAL".to_string(),
        page_pr: Some(HxPagePr {
            landscape: "WIDELY".to_string(),
            width: ps.width.as_i32(),
            height: ps.height.as_i32(),
            gutter_type: "LEFT_ONLY".to_string(),
            margin: Some(HxPageMargin {
                header: ps.header_margin.as_i32(),
                footer: ps.footer_margin.as_i32(),
                gutter: 0,
                left: ps.margin_left.as_i32(),
                right: ps.margin_right.as_i32(),
                top: ps.margin_top.as_i32(),
                bottom: ps.margin_bottom.as_i32(),
            }),
        }),
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
fn build_table(table: &Table, depth: usize) -> HwpxResult<HxTable> {
    if depth >= MAX_NESTING_DEPTH {
        return Err(HwpxError::InvalidStructure {
            detail: format!("table nesting depth {} exceeds limit of {}", depth, MAX_NESTING_DEPTH,),
        });
    }

    let col_cnt = table.rows.iter().map(|r| r.cells.len()).max().unwrap_or(0) as u32;

    let rows = table
        .rows
        .iter()
        .enumerate()
        .map(|(row_idx, row)| build_table_row(row, row_idx as u32, depth))
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
        dropcap_style: "None".to_string(),
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
            .map(|c| build_hx_caption(c, table_width, depth))
            .transpose()?,
        in_margin: Some(DEFAULT_CELL_MARGIN),
        rows,
    })
}

/// Builds `HxTableRow` from a Core `TableRow`.
fn build_table_row(row: &TableRow, row_idx: u32, depth: usize) -> HwpxResult<HxTableRow> {
    let cells = row
        .cells
        .iter()
        .enumerate()
        .map(|(col_idx, cell)| build_table_cell(cell, col_idx as u32, row_idx, depth))
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
) -> HwpxResult<HxTableCell> {
    let paragraphs = cell
        .paragraphs
        .iter()
        .enumerate()
        .map(|(idx, para)| build_paragraph(para, false, None, idx, depth + 1))
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
fn build_picture(img: &Image, depth: usize) -> HwpxResult<HxPic> {
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
        dropcap_style: "None".to_string(),
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
        caption: img.caption.as_ref().map(|c| build_hx_caption(c, w, depth)).transpose()?,
    })
}

// ── Linesegarray placeholder ─────────────────────────────────────

/// Default horizontal size for A4 with 30mm margins (59528 - 8504 - 8504).
const DEFAULT_HORZ_SIZE: i32 = 42520;

/// Default char height in HWPUNIT (10pt = 1000).
const DEFAULT_CHAR_HEIGHT: i32 = 1000;

/// Builds a minimal placeholder `HxLineSegArray` with one line segment.
///
/// Uses fixed defaults for char height (10pt) and calculates baseline
/// and spacing from standard ratios. 한글 recalculates these on open,
/// so exact values are not critical.
fn build_linesegarray(horzsize: i32) -> HxLineSegArray {
    let vertsize = DEFAULT_CHAR_HEIGHT;
    let baseline = vertsize * 85 / 100;
    let spacing = 600; // default for ~160% line spacing
    HxLineSegArray {
        items: vec![HxLineSeg {
            textpos: 0,
            vertpos: 0,
            vertsize,
            textheight: vertsize,
            baseline,
            spacing,
            horzpos: 0,
            horzsize,
            flags: 393216,
        }],
    }
}

// ── 한글 compatibility: secPr enrichment ────────────────────────

/// Enriched `<hp:secPr>` opening tag with all attributes 한글 expects.
const SEC_PR_OPEN_ENRICHED: &str = concat!(
    r#"<hp:secPr id="" textDirection="HORIZONTAL" "#,
    r#"spaceColumns="1134" tabStop="8000" tabStopVal="4000" tabStopUnit="HWPUNIT" "#,
    r#"outlineShapeIDRef="1" memoShapeIDRef="0" textVerticalWidthHead="0" masterPageCnt="0">"#,
);

/// Sub-elements inserted before `<hp:pagePr>` inside secPr.
const SEC_PR_PRE_ELEMENTS: &str = concat!(
    r#"<hp:grid lineGrid="0" charGrid="0" wonggojiFormat="0"/>"#,
    r#"<hp:startNum pageStartsOn="BOTH" page="0" pic="0" tbl="0" equation="0"/>"#,
    r#"<hp:visibility hideFirstHeader="0" hideFirstFooter="0" hideFirstMasterPage="0" "#,
    r#"border="SHOW_ALL" fill="SHOW_ALL" hideFirstPageNum="0" hideFirstEmptyLine="0" showLineNumber="0"/>"#,
    r#"<hp:lineNumberShape restartType="0" countBy="0" distance="0" startNumber="0"/>"#,
);

/// Sub-elements inserted after `</hp:pagePr>` and before `</hp:secPr>`.
const SEC_PR_POST_ELEMENTS: &str = concat!(
    r#"<hp:footNotePr>"#,
    r#"<hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar=")" supscript="0"/>"#,
    r##"<hp:noteLine length="-1" type="SOLID" width="0.12 mm" color="#000000"/>"##,
    r#"<hp:noteSpacing betweenNotes="283" belowLine="567" aboveLine="850"/>"#,
    r#"<hp:numbering type="CONTINUOUS" newNum="1"/>"#,
    r#"<hp:placement place="EACH_COLUMN" beneathText="0"/>"#,
    r#"</hp:footNotePr>"#,
    r#"<hp:endNotePr>"#,
    r#"<hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar=")" supscript="0"/>"#,
    r##"<hp:noteLine length="14692344" type="SOLID" width="0.12 mm" color="#000000"/>"##,
    r#"<hp:noteSpacing betweenNotes="0" belowLine="567" aboveLine="850"/>"#,
    r#"<hp:numbering type="CONTINUOUS" newNum="1"/>"#,
    r#"<hp:placement place="END_OF_DOCUMENT" beneathText="0"/>"#,
    r#"</hp:endNotePr>"#,
    r#"<hp:pageBorderFill type="BOTH" borderFillIDRef="1" textBorder="PAPER" headerInside="0" footerInside="0" fillArea="PAPER">"#,
    r#"<hp:offset left="1417" right="1417" top="1417" bottom="1417"/>"#,
    r#"</hp:pageBorderFill>"#,
    r#"<hp:pageBorderFill type="EVEN" borderFillIDRef="1" textBorder="PAPER" headerInside="0" footerInside="0" fillArea="PAPER">"#,
    r#"<hp:offset left="1417" right="1417" top="1417" bottom="1417"/>"#,
    r#"</hp:pageBorderFill>"#,
    r#"<hp:pageBorderFill type="ODD" borderFillIDRef="1" textBorder="PAPER" headerInside="0" footerInside="0" fillArea="PAPER">"#,
    r#"<hp:offset left="1417" right="1417" top="1417" bottom="1417"/>"#,
    r#"</hp:pageBorderFill>"#,
);

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
fn enrich_sec_pr(xml: &str, column_settings: Option<&ColumnSettings>) -> String {
    let minimal_open = r#"<hp:secPr textDirection="HORIZONTAL">"#;

    // If no secPr to enrich, return as-is
    if !xml.contains(minimal_open) {
        return xml.to_string();
    }

    let mut result =
        xml.replacen(minimal_open, &format!("{SEC_PR_OPEN_ENRICHED}{SEC_PR_PRE_ELEMENTS}"), 1);

    // Insert post-elements before the first </hp:secPr>
    if let Some(pos) = result.find("</hp:secPr>") {
        result.insert_str(pos, SEC_PR_POST_ELEMENTS);
    }

    // Insert colPr after </hp:secPr>
    if let Some(pos) = result.find("</hp:secPr>") {
        let insert_pos = pos + "</hp:secPr>".len();
        let col_pr = build_col_pr_xml(column_settings);
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
fn find_ctrl_injection_point(xml: &str) -> usize {
    // Look for colPr ctrl: find "</hp:colPr>" and then the next "</hp:ctrl>"
    if let Some(col_pr_pos) = xml.find("</hp:colPr>") {
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
            r#"<hp:p id="{idx}" paraPrIDRef="{}" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0">"#,
            para.para_shape_id.get(),
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
        side_char = escape_xml(&pn.side_char),
    )
    .expect("write to String is infallible");
    xml
}

/// Escapes XML special characters in text content.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
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
        let xml = encode_section(&section, 0).unwrap();

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
        let xml = encode_section(&section, 0).unwrap();

        // Parse back with the decoder
        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();
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
        };
        let section = Section::with_paragraphs(vec![text_paragraph("Content", 0, 0)], ps);
        let xml = encode_section(&section, 0).unwrap();

        assert!(xml.contains("<hp:secPr"), "missing secPr");
        assert!(xml.contains(r#"textDirection="HORIZONTAL""#), "missing textDirection");
        assert!(xml.contains(r#"width="59528""#), "missing width");
        assert!(xml.contains(r#"height="84188""#), "missing height");
        assert!(xml.contains(r#"left="8504""#), "missing left margin");

        // Roundtrip the page settings through the decoder
        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();
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
        let xml = encode_section(&section, 0).unwrap();

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
        let xml = encode_section(&section, 0).unwrap();

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
        let xml = encode_section(&section, 0).unwrap();

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
        let xml = encode_section(&section, 0).unwrap();
        assert!(xml.contains("<hp:t>Deep</hp:t>"), "missing nested text");
    }

    // ── Test 8: Control is skipped ───────────────────────────────

    #[test]
    fn control_skipped() {
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
        let xml = encode_section(&section, 0).unwrap();

        assert!(xml.contains("<hp:t>before</hp:t>"), "missing 'before' text");
        assert!(xml.contains("<hp:t>after</hp:t>"), "missing 'after' text");
        // Control content should not appear
        assert!(!xml.contains("example.com"), "control content should not appear in XML");
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
        let xml = encode_section(&section, 0).unwrap();

        // Should parse without error
        assert!(xml.contains("<hs:sec"), "missing root element");
        assert!(xml.contains("</hs:sec>"), "missing close tag");
    }

    // ── Test 10: Korean text preservation ────────────────────────

    #[test]
    fn korean_text_preservation() {
        let korean = "우리는 수학을 공부한다.";
        let section = simple_section(korean);
        let xml = encode_section(&section, 0).unwrap();

        // Roundtrip through decoder
        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();
        assert_eq!(result.paragraphs[0].runs[0].content.as_text(), Some(korean),);
    }

    // ── Additional edge cases ────────────────────────────────────

    #[test]
    fn empty_section_produces_valid_xml() {
        let section = Section::new(PageSettings::a4());
        let xml = encode_section(&section, 0).unwrap();

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
        let err = build_table(&hx_table, MAX_NESTING_DEPTH).unwrap_err();
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
        let hx = build_picture(&img, 0).unwrap();
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
        let xml = encode_section(&section, 0).unwrap();
        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();

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
        let xml = encode_section(&section, 0).unwrap();
        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();

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
        let xml = encode_section(&section, 0).unwrap();
        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();

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

        let xml = encode_section(&section, 0).unwrap();
        assert!(xml.contains("<hp:header"), "XML should contain header element");
        assert!(xml.contains("Header Content"), "XML should contain header text");

        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();
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

        let xml = encode_section(&section, 0).unwrap();
        assert!(xml.contains("<hp:footer"), "XML should contain footer element");

        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();
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
        section.page_number = Some(PageNumber::with_side_char(
            PageNumberPosition::BottomCenter,
            NumberFormatType::Digit,
            "- ".to_string(),
        ));

        let xml = encode_section(&section, 0).unwrap();
        assert!(xml.contains("<hp:pageNum"), "XML should contain pageNum element");
        assert!(xml.contains(r#"pos="BOTTOM_CENTER""#), "XML should contain pos attribute");
        assert!(xml.contains(r#"formatType="DIGIT""#), "XML should contain formatType");
        assert!(xml.contains("sideChar=\"- \""), "XML should contain side char");

        // Full roundtrip: encoder outputs <hp:pageNum> which decoder parses back
        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();
        let pn = result.page_number.expect("decoded section should have page number");
        assert_eq!(pn.position, PageNumberPosition::BottomCenter);
        assert_eq!(pn.number_format, NumberFormatType::Digit);
        assert_eq!(pn.side_char, "- ");
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

        let xml = encode_section(&section, 0).unwrap();

        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();
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
        let xml = encode_section(&section, 0).unwrap();

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
        let xml = encode_section(&section, 0).unwrap();

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
        let xml = encode_section(&section, 0).unwrap();

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

        let xml = encode_section(&section, 0).unwrap();
        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();

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

        let xml = encode_section(&section, 0).unwrap();
        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();

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

        let xml = encode_section(&section, 0).unwrap();
        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();

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

        let xml = encode_section(&section, 0).unwrap();
        assert!(xml.contains("A &amp; B &lt; C &gt; D"), "special chars must be escaped");

        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();
        let header = result.header.expect("should have header");
        assert_eq!(header.paragraphs[0].runs[0].content.as_text(), Some("A & B < C > D"),);
    }
}
