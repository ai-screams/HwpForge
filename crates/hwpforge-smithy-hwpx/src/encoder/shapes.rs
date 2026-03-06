//! Encodes Core shape controls into HWPX schema types.
//!
//! Split from `section.rs` to enable parallel development of shape features.
//! Functions here convert `Control::TextBox`, `Control::Line`, `Control::Ellipse`,
//! and `Control::Polygon` into their corresponding `Hx*` schema types.

use hwpforge_core::control::{Control, ShapeStyle};
use hwpforge_foundation::{ArrowType, CurveSegmentType, DropCapStyle, Flip};

/// Extracts the dropcap style string from an optional `ShapeStyle`.
fn dropcap_str(style: &Option<ShapeStyle>) -> String {
    style.as_ref().map_or(DropCapStyle::None, |s| s.drop_cap_style).to_string()
}

/// Resolve `ArrowType` to KS X 6101 string.
///
/// For geometric shapes (Diamond, Oval, Open), 한글 only recognises the `EMPTY_*`
/// form and uses the separate `headfill`/`tailfill` attribute to control fill.
/// Reference: `SimpleLine.hwpx` uses `EMPTY_BOX` + `headfill="1"` for a filled box.
fn resolve_arrow_type_str(arrow_type: &ArrowType, _filled: bool) -> String {
    match arrow_type {
        ArrowType::None => "NORMAL",
        ArrowType::Normal => "ARROW",
        ArrowType::Arrow => "SPEAR",
        ArrowType::Concave => "CONCAVE_ARROW",
        ArrowType::Diamond => "EMPTY_DIAMOND",
        ArrowType::Oval => "EMPTY_CIRCLE",
        ArrowType::Open => "EMPTY_BOX",
        _ => "NORMAL",
    }
    .to_string()
}

use crate::error::HwpxResult;
use crate::schema::section::{
    HxConnectLine, HxConnectPoint, HxControlPoint, HxControlPoints, HxCurve, HxCurveSegment,
    HxDrawText, HxEllipse, HxFillBrush, HxFlip, HxLine, HxLineShape, HxMatrix, HxOffset, HxPoint,
    HxPolygon, HxRect, HxRenderingInfo, HxRotationInfo, HxShadow, HxShapeComment, HxSizeAttr,
    HxTableMargin, HxTablePos, HxTableSz,
};

use super::section::{build_hx_caption, encode_paragraphs_to_sublist, generate_instid};

// ── Shape-common helpers ─────────────────────────────────────────

/// Collected common sub-elements required by 한글 for all drawing objects.
///
/// All four shape encoders (rect/textbox, line, ellipse, polygon) produce
/// the same prefix block. This struct avoids repeating the construction logic.
pub(crate) struct ShapeCommon {
    pub offset: HxOffset,
    pub org_sz: HxSizeAttr,
    pub cur_sz: HxSizeAttr,
    pub flip: HxFlip,
    pub rotation_info: HxRotationInfo,
    pub rendering_info: HxRenderingInfo,
    pub line_shape: HxLineShape,
    pub fill_brush: HxFillBrush,
    pub shadow: HxShadow,
}

/// Builds the shape-common block for a drawing object of the given pixel size.
///
/// Defaults match 한글's output for a newly created shape:
/// - zero offset, orgSz = given dimensions, curSz = 0×0
/// - identity rotation/rendering matrices
/// - solid black border, white fill, no shadow
///
/// When `ShapeStyle` contains rotation, flip, or arrow overrides, they are
/// applied to the common block instead of identity values.
pub(crate) fn build_shape_common(
    width: i32,
    height: i32,
    style: Option<&ShapeStyle>,
) -> ShapeCommon {
    let mut line_shape = HxLineShape::default_solid();
    let mut fill_brush = HxFillBrush::default_white();

    // Rotation angle (in 1/100 degree units for HWPX schema)
    let mut angle: i32 = 0;
    let mut hx_flip = HxFlip { horizontal: 0, vertical: 0 };

    if let Some(s) = style {
        if let Some(ref c) = s.line_color {
            line_shape.color = c.to_hex_rgb();
        }
        if let Some(w) = s.line_width {
            line_shape.width = w as i32;
        }
        if let Some(ref ls) = s.line_style {
            line_shape.style = ls.to_string();
        }
        if let Some(ref c) = s.fill_color {
            fill_brush.win_brush.face_color = c.to_hex_rgb();
        }

        // Rotation: Core uses degrees (f32), HWPX uses degrees * 100 (i32)
        if let Some(rot) = s.rotation {
            angle = (rot * 100.0) as i32;
        }

        // Flip
        if let Some(f) = s.flip {
            match f {
                Flip::None => {}
                Flip::Horizontal => hx_flip.horizontal = 1,
                Flip::Vertical => hx_flip.vertical = 1,
                Flip::Both => {
                    hx_flip.horizontal = 1;
                    hx_flip.vertical = 1;
                }
                _ => {} // future Flip variants
            }
        }

        // Arrow heads — resolve FILLED_ vs EMPTY_ for geometric types per KS X 6101.
        if let Some(ref arrow) = s.head_arrow {
            line_shape.head_style = resolve_arrow_type_str(&arrow.arrow_type, arrow.filled);
            line_shape.head_sz = arrow.size.to_string();
            line_shape.head_fill = if arrow.filled { 1 } else { 0 };
        }
        if let Some(ref arrow) = s.tail_arrow {
            line_shape.tail_style = resolve_arrow_type_str(&arrow.arrow_type, arrow.filled);
            line_shape.tail_sz = arrow.size.to_string();
            line_shape.tail_fill = if arrow.filled { 1 } else { 0 };
        }
    }

    // Build rotation matrix if angle != 0
    let rot_matrix = if angle != 0 {
        let rad = (angle as f64) / 100.0 * std::f64::consts::PI / 180.0;
        let cos_val = rad.cos();
        let sin_val = rad.sin();
        HxMatrix {
            e1: format!("{cos_val:.6}"),
            e2: format!("{sin_val:.6}"),
            e3: "0".to_string(),
            e4: format!("{:.6}", -sin_val),
            e5: format!("{cos_val:.6}"),
            e6: "0".to_string(),
        }
    } else {
        HxMatrix::identity()
    };

    ShapeCommon {
        offset: HxOffset { x: 0, y: 0 },
        org_sz: HxSizeAttr { width, height },
        cur_sz: HxSizeAttr { width: 0, height: 0 },
        flip: hx_flip,
        rotation_info: HxRotationInfo {
            angle,
            center_x: width / 2,
            center_y: height / 2,
            rotate_image: 1,
        },
        rendering_info: HxRenderingInfo {
            trans_matrix: HxMatrix::identity(),
            sca_matrix: HxMatrix::identity(),
            rot_matrix,
        },
        line_shape,
        fill_brush,
        shadow: HxShadow::default_none(),
    }
}

/// Encodes a Core `Control::TextBox` into `HxRect` with `<hp:drawText>`.
///
/// Phase 4.5 MVP: inline positioning (treatAsChar=1) when offsets are (0,0).
pub(crate) fn encode_textbox_to_rect(
    ctrl: &Control,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<HxRect> {
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
    let last_width = width_hwp as u32;

    let sub_list = encode_paragraphs_to_sublist(paragraphs, depth, hyperlink_entries)?;
    let sc = build_shape_common(width_hwp, height_hwp, style.as_ref());

    Ok(HxRect {
        id: generate_instid(),
        z_order: 0,
        numbering_type: "NONE".to_string(),
        text_wrap: "TOP_AND_BOTTOM".to_string(),
        text_flow: "BOTH_SIDES".to_string(),
        lock: 0,
        dropcap_style: dropcap_str(style),
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
        shadow: Some(HxShadow { alpha: 178, ..HxShadow::default_none() }),

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
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, width_hwp, depth, hyperlink_entries))
            .transpose()?,

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
        shape_comment: Some(HxShapeComment { text: "사각형입니다.".to_string() }),
    })
}

/// Encodes a Core `Control::Line` into `HxLine`.
pub(crate) fn encode_line_to_hx(
    ctrl: &Control,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<HxLine> {
    let (start, end, width, height, horz_offset, vert_offset, caption, style) = match ctrl {
        Control::Line { start, end, width, height, horz_offset, vert_offset, caption, style } => {
            (start, end, *width, *height, horz_offset, vert_offset, caption, style)
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
        dropcap_style: dropcap_str(style),
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
        fill_brush: None, // lines have no fill brush per golden (line.hwpx)
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
        shape_comment: Some(HxShapeComment { text: "선입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, depth, hyperlink_entries))
            .transpose()?,
        start_pt: Some(HxPoint { x: start.x, y: start.y }),
        end_pt: Some(HxPoint { x: end.x, y: end.y }),
    })
}

/// Encodes a Core `Control::Ellipse` into `HxEllipse`.
pub(crate) fn encode_ellipse_to_hx(
    ctrl: &Control,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<HxEllipse> {
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
        let sub_list = encode_paragraphs_to_sublist(paragraphs, depth, hyperlink_entries)?;
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
        dropcap_style: dropcap_str(style),
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
        shape_comment: Some(HxShapeComment { text: "타원입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, depth, hyperlink_entries))
            .transpose()?,
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
pub(crate) fn encode_polygon_to_hx(
    ctrl: &Control,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<HxPolygon> {
    let (vertices, width, height, horz_offset, vert_offset, paragraphs, caption, style) = match ctrl
    {
        Control::Polygon {
            vertices,
            width,
            height,
            horz_offset,
            vert_offset,
            paragraphs,
            caption,
            style,
        } => (vertices, *width, *height, horz_offset, vert_offset, paragraphs, caption, style),
        _ => unreachable!("encode_polygon_to_hx called with non-Polygon"),
    };

    let w = width.as_i32();
    let h = height.as_i32();
    let sc = build_shape_common(w, h, style.as_ref());

    let draw_text = if paragraphs.is_empty() {
        None
    } else {
        let sub_list = encode_paragraphs_to_sublist(paragraphs, depth, hyperlink_entries)?;
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
        dropcap_style: dropcap_str(style),
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
        shape_comment: Some(HxShapeComment { text: "다각형입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, depth, hyperlink_entries))
            .transpose()?,
        draw_text,
        points,
    })
}

/// Encodes a Core `Control::Arc` into `HxEllipse` with `hasArcPr=1`.
///
/// Arc reuses the ellipse schema with arc-specific fields enabled.
pub(crate) fn encode_arc_to_hx(
    ctrl: &Control,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<HxEllipse> {
    let (
        arc_type,
        center,
        axis1,
        axis2,
        start1,
        end1,
        start2,
        end2,
        width,
        height,
        horz_offset,
        vert_offset,
        caption,
        style,
    ) = match ctrl {
        Control::Arc {
            arc_type,
            center,
            axis1,
            axis2,
            start1,
            end1,
            start2,
            end2,
            width,
            height,
            horz_offset,
            vert_offset,
            caption,
            style,
        } => (
            arc_type,
            center,
            axis1,
            axis2,
            start1,
            end1,
            start2,
            end2,
            *width,
            *height,
            horz_offset,
            vert_offset,
            caption,
            style,
        ),
        _ => unreachable!("encode_arc_to_hx called with non-Arc"),
    };

    let w = width.as_i32();
    let h = height.as_i32();
    let sc = build_shape_common(w, h, style.as_ref());

    Ok(HxEllipse {
        id: generate_instid(),
        z_order: 0,
        numbering_type: "NONE".to_string(),
        text_wrap: "TOP_AND_BOTTOM".to_string(),
        text_flow: "BOTH_SIDES".to_string(),
        lock: 0,
        dropcap_style: dropcap_str(style),
        href: String::new(),
        group_level: 0,
        instid: generate_instid(),
        interval_dirty: 0,
        has_arc_pr: 1,
        arc_type: arc_type.to_string(),
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
        shape_comment: Some(HxShapeComment { text: "호입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, depth, hyperlink_entries))
            .transpose()?,
        draw_text: None,
        center: Some(HxPoint { x: center.x, y: center.y }),
        ax1: Some(HxPoint { x: axis1.x, y: axis1.y }),
        ax2: Some(HxPoint { x: axis2.x, y: axis2.y }),
        start1: Some(HxPoint { x: start1.x, y: start1.y }),
        end1: Some(HxPoint { x: end1.x, y: end1.y }),
        start2: Some(HxPoint { x: start2.x, y: start2.y }),
        end2: Some(HxPoint { x: end2.x, y: end2.y }),
    })
}

/// Encodes a Core `Control::Curve` into `HxCurve`.
pub(crate) fn encode_curve_to_hx(
    ctrl: &Control,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<HxCurve> {
    let (points, segment_types, width, height, horz_offset, vert_offset, caption, style) =
        match ctrl {
            Control::Curve {
                points,
                segment_types,
                width,
                height,
                horz_offset,
                vert_offset,
                caption,
                style,
            } => (points, segment_types, *width, *height, horz_offset, vert_offset, caption, style),
            _ => unreachable!("encode_curve_to_hx called with non-Curve"),
        };

    let w = width.as_i32();
    let h = height.as_i32();
    let sc = build_shape_common(w, h, style.as_ref());

    // Per KS X 6101 표 269: each <hp:seg> has x1/y1 (start) + x2/y2 (end).
    // Points array encodes control vertices; segments connect adjacent pairs.
    let segments: Vec<HxCurveSegment> = if points.len() >= 2 {
        points
            .windows(2)
            .zip(segment_types.iter().chain(std::iter::repeat(&CurveSegmentType::Curve)))
            .map(|(pair, st)| HxCurveSegment {
                seg_type: st.to_string(),
                x1: pair[0].x,
                y1: pair[0].y,
                x2: pair[1].x,
                y2: pair[1].y,
            })
            .collect()
    } else {
        vec![]
    };

    Ok(HxCurve {
        id: generate_instid(),
        z_order: 0,
        numbering_type: "NONE".to_string(),
        text_wrap: "TOP_AND_BOTTOM".to_string(),
        text_flow: "BOTH_SIDES".to_string(),
        lock: 0,
        dropcap_style: dropcap_str(style),
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
        shape_comment: Some(HxShapeComment { text: "곡선입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, depth, hyperlink_entries))
            .transpose()?,
        points: vec![], // KS X 6101: coordinates are in <hp:seg> elements, not <hc:pt>
        segments,
    })
}

/// Encodes a Core `Control::ConnectLine` into `HxConnectLine`.
pub(crate) fn encode_connect_line_to_hx(
    ctrl: &Control,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
) -> HwpxResult<HxConnectLine> {
    let (
        start,
        end,
        control_points,
        connect_type,
        width,
        height,
        horz_offset,
        vert_offset,
        caption,
        style,
    ) = match ctrl {
        Control::ConnectLine {
            start,
            end,
            control_points,
            connect_type,
            width,
            height,
            horz_offset,
            vert_offset,
            caption,
            style,
        } => (
            start,
            end,
            control_points,
            connect_type,
            *width,
            *height,
            horz_offset,
            vert_offset,
            caption,
            style,
        ),
        _ => unreachable!("encode_connect_line_to_hx called with non-ConnectLine"),
    };

    let w = width.as_i32();
    let h = height.as_i32();
    let sc = build_shape_common(w, h, style.as_ref());

    // Per golden fixture: controlPoints wrapper contains ALL points
    // (start type=3, intermediates type=2, end type=26).
    let mut all_points = Vec::with_capacity(control_points.len() + 2);
    all_points.push(HxControlPoint { x: start.x, y: start.y, point_type: "3".to_string() });
    for p in control_points {
        all_points.push(HxControlPoint { x: p.x, y: p.y, point_type: "2".to_string() });
    }
    all_points.push(HxControlPoint { x: end.x, y: end.y, point_type: "26".to_string() });

    Ok(HxConnectLine {
        id: generate_instid(),
        z_order: 0,
        numbering_type: "NONE".to_string(),
        text_wrap: "TOP_AND_BOTTOM".to_string(),
        text_flow: "BOTH_SIDES".to_string(),
        lock: 0,
        dropcap_style: dropcap_str(style),
        href: String::new(),
        group_level: 0,
        instid: generate_instid(),
        connect_type: connect_type.clone(),
        offset: Some(sc.offset),
        org_sz: Some(sc.org_sz),
        cur_sz: Some(sc.cur_sz),
        flip: Some(sc.flip),
        rotation_info: Some(sc.rotation_info),
        rendering_info: Some(sc.rendering_info),
        line_shape: Some(sc.line_shape),
        fill_brush: None, // connect lines have no fill like regular lines
        shadow: Some(sc.shadow),
        start_pt: Some(HxConnectPoint {
            x: start.x,
            y: start.y,
            subject_id_ref: "0".to_string(),
            subject_idx: "0".to_string(),
        }),
        end_pt: Some(HxConnectPoint {
            x: end.x,
            y: end.y,
            subject_id_ref: "0".to_string(),
            subject_idx: "0".to_string(),
        }),
        control_points: Some(HxControlPoints { points: all_points }),
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
        shape_comment: Some(HxShapeComment { text: "연결선입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, depth, hyperlink_entries))
            .transpose()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::control::{ArrowStyle, Control, LineStyle, ShapePoint, ShapeStyle};
    use hwpforge_foundation::{
        ArcType, ArrowSize, ArrowType, CurveSegmentType, DropCapStyle, Flip, HwpUnit,
    };

    fn empty_hyperlinks() -> Vec<(String, String)> {
        vec![]
    }

    fn make_style(
        line_color_hex: Option<&str>,
        fill_color_hex: Option<&str>,
        line_width: Option<u32>,
    ) -> ShapeStyle {
        use hwpforge_foundation::Color;
        ShapeStyle {
            line_color: line_color_hex.map(|h| {
                let r = u8::from_str_radix(&h[1..3], 16).unwrap();
                let g = u8::from_str_radix(&h[3..5], 16).unwrap();
                let b = u8::from_str_radix(&h[5..7], 16).unwrap();
                Color::from_rgb(r, g, b)
            }),
            fill_color: fill_color_hex.map(|h| {
                let r = u8::from_str_radix(&h[1..3], 16).unwrap();
                let g = u8::from_str_radix(&h[3..5], 16).unwrap();
                let b = u8::from_str_radix(&h[5..7], 16).unwrap();
                Color::from_rgb(r, g, b)
            }),
            line_width,
            ..Default::default()
        }
    }

    // ── dropcap_str tests ────────────────────────────────────────────

    #[test]
    fn dropcap_str_none_style_returns_none_string() {
        let s = dropcap_str(&None);
        assert_eq!(s, "None");
    }

    #[test]
    fn dropcap_str_default_shapestyle_returns_none() {
        let style = Some(ShapeStyle::default());
        assert_eq!(dropcap_str(&style), "None");
    }

    #[test]
    fn dropcap_str_double_line() {
        let style = ShapeStyle { drop_cap_style: DropCapStyle::DoubleLine, ..Default::default() };
        assert_eq!(dropcap_str(&Some(style)), "DoubleLine");
    }

    #[test]
    fn dropcap_str_triple_line() {
        let style = ShapeStyle { drop_cap_style: DropCapStyle::TripleLine, ..Default::default() };
        assert_eq!(dropcap_str(&Some(style)), "TripleLine");
    }

    #[test]
    fn dropcap_str_margin() {
        let style = ShapeStyle { drop_cap_style: DropCapStyle::Margin, ..Default::default() };
        assert_eq!(dropcap_str(&Some(style)), "Margin");
    }

    // ── resolve_arrow_type_str tests ─────────────────────────────────

    #[test]
    fn arrow_type_none_maps_to_normal() {
        assert_eq!(resolve_arrow_type_str(&ArrowType::None, false), "NORMAL");
    }

    #[test]
    fn arrow_type_normal_maps_to_arrow() {
        assert_eq!(resolve_arrow_type_str(&ArrowType::Normal, false), "ARROW");
    }

    #[test]
    fn arrow_type_arrow_maps_to_spear() {
        assert_eq!(resolve_arrow_type_str(&ArrowType::Arrow, false), "SPEAR");
    }

    #[test]
    fn arrow_type_concave_maps_to_concave_arrow() {
        assert_eq!(resolve_arrow_type_str(&ArrowType::Concave, false), "CONCAVE_ARROW");
    }

    #[test]
    fn arrow_type_diamond_maps_to_empty_diamond_regardless_of_fill() {
        // Gotcha #25: 한글 only recognises EMPTY_* form for geometric shapes
        assert_eq!(resolve_arrow_type_str(&ArrowType::Diamond, true), "EMPTY_DIAMOND");
        assert_eq!(resolve_arrow_type_str(&ArrowType::Diamond, false), "EMPTY_DIAMOND");
    }

    #[test]
    fn arrow_type_oval_maps_to_empty_circle() {
        assert_eq!(resolve_arrow_type_str(&ArrowType::Oval, true), "EMPTY_CIRCLE");
        assert_eq!(resolve_arrow_type_str(&ArrowType::Oval, false), "EMPTY_CIRCLE");
    }

    #[test]
    fn arrow_type_open_maps_to_empty_box() {
        assert_eq!(resolve_arrow_type_str(&ArrowType::Open, false), "EMPTY_BOX");
    }

    // ── build_shape_common tests ─────────────────────────────────────

    #[test]
    fn build_shape_common_default_style_gives_identity_rotation() {
        let sc = build_shape_common(1000, 500, None);
        assert_eq!(sc.rotation_info.angle, 0);
        assert_eq!(sc.flip.horizontal, 0);
        assert_eq!(sc.flip.vertical, 0);
    }

    #[test]
    fn build_shape_common_org_sz_matches_dimensions() {
        let sc = build_shape_common(8000, 4000, None);
        assert_eq!(sc.org_sz.width, 8000);
        assert_eq!(sc.org_sz.height, 4000);
    }

    #[test]
    fn build_shape_common_cur_sz_is_zero() {
        let sc = build_shape_common(8000, 4000, None);
        assert_eq!(sc.cur_sz.width, 0);
        assert_eq!(sc.cur_sz.height, 0);
    }

    #[test]
    fn build_shape_common_offset_is_zero() {
        let sc = build_shape_common(1000, 500, None);
        assert_eq!(sc.offset.x, 0);
        assert_eq!(sc.offset.y, 0);
    }

    #[test]
    fn build_shape_common_rotation_applied_correctly() {
        let style = ShapeStyle { rotation: Some(45.0), ..Default::default() };
        let sc = build_shape_common(1000, 500, Some(&style));
        assert_eq!(sc.rotation_info.angle, 4500);
        // rotation center = dimension / 2
        assert_eq!(sc.rotation_info.center_x, 500);
        assert_eq!(sc.rotation_info.center_y, 250);
    }

    #[test]
    fn build_shape_common_rotation_90_degrees() {
        let style = ShapeStyle { rotation: Some(90.0), ..Default::default() };
        let sc = build_shape_common(2000, 1000, Some(&style));
        assert_eq!(sc.rotation_info.angle, 9000);
        // rotation matrix: cos(90°)≈0, sin(90°)≈1
        let e1: f64 = sc.rendering_info.rot_matrix.e1.parse().unwrap();
        let e2: f64 = sc.rendering_info.rot_matrix.e2.parse().unwrap();
        assert!(e1.abs() < 0.001, "cos(90°) must be ~0");
        assert!((e2 - 1.0).abs() < 0.001, "sin(90°) must be ~1");
    }

    #[test]
    fn build_shape_common_flip_horizontal() {
        let style = ShapeStyle { flip: Some(Flip::Horizontal), ..Default::default() };
        let sc = build_shape_common(1000, 500, Some(&style));
        assert_eq!(sc.flip.horizontal, 1);
        assert_eq!(sc.flip.vertical, 0);
    }

    #[test]
    fn build_shape_common_flip_vertical() {
        let style = ShapeStyle { flip: Some(Flip::Vertical), ..Default::default() };
        let sc = build_shape_common(1000, 500, Some(&style));
        assert_eq!(sc.flip.horizontal, 0);
        assert_eq!(sc.flip.vertical, 1);
    }

    #[test]
    fn build_shape_common_flip_both() {
        let style = ShapeStyle { flip: Some(Flip::Both), ..Default::default() };
        let sc = build_shape_common(1000, 500, Some(&style));
        assert_eq!(sc.flip.horizontal, 1);
        assert_eq!(sc.flip.vertical, 1);
    }

    #[test]
    fn build_shape_common_line_color_overridden() {
        let style = make_style(Some("#FF0000"), None, None);
        let sc = build_shape_common(1000, 500, Some(&style));
        assert_eq!(sc.line_shape.color, "#FF0000");
    }

    #[test]
    fn build_shape_common_line_width_overridden() {
        let style = make_style(None, None, Some(100));
        let sc = build_shape_common(1000, 500, Some(&style));
        assert_eq!(sc.line_shape.width, 100);
    }

    #[test]
    fn build_shape_common_fill_color_overridden() {
        let style = make_style(None, Some("#00FF00"), None);
        let sc = build_shape_common(1000, 500, Some(&style));
        assert_eq!(sc.fill_brush.win_brush.face_color, "#00FF00");
    }

    #[test]
    fn build_shape_common_line_style_dash_overridden() {
        let style = ShapeStyle { line_style: Some(LineStyle::Dash), ..Default::default() };
        let sc = build_shape_common(1000, 500, Some(&style));
        assert_eq!(sc.line_shape.style, "DASH");
    }

    #[test]
    fn build_shape_common_head_arrow_spear_filled() {
        let style = ShapeStyle {
            head_arrow: Some(ArrowStyle {
                arrow_type: ArrowType::Arrow,
                size: ArrowSize::Large,
                filled: true,
            }),
            ..Default::default()
        };
        let sc = build_shape_common(1000, 500, Some(&style));
        assert_eq!(sc.line_shape.head_style, "SPEAR");
        assert_eq!(sc.line_shape.head_fill, 1);
        assert_eq!(sc.line_shape.head_sz, "LARGE_LARGE");
    }

    #[test]
    fn build_shape_common_tail_arrow_diamond_unfilled() {
        let style = ShapeStyle {
            tail_arrow: Some(ArrowStyle {
                arrow_type: ArrowType::Diamond,
                size: ArrowSize::Small,
                filled: false,
            }),
            ..Default::default()
        };
        let sc = build_shape_common(1000, 500, Some(&style));
        // Per gotcha #25: Diamond always maps to EMPTY_DIAMOND; headfill controls fill
        assert_eq!(sc.line_shape.tail_style, "EMPTY_DIAMOND");
        assert_eq!(sc.line_shape.tail_fill, 0);
        assert_eq!(sc.line_shape.tail_sz, "SMALL_SMALL");
    }

    #[test]
    fn build_shape_common_default_solid_line_style() {
        let sc = build_shape_common(1000, 500, None);
        assert_eq!(sc.line_shape.style, "SOLID");
        assert_eq!(sc.line_shape.color, "#000000");
        assert_eq!(sc.line_shape.width, 33);
    }

    #[test]
    fn build_shape_common_default_white_fill() {
        let sc = build_shape_common(1000, 500, None);
        assert_eq!(sc.fill_brush.win_brush.face_color, "#FFFFFF");
    }

    #[test]
    fn build_shape_common_no_rotation_uses_identity_matrix() {
        let sc = build_shape_common(1000, 500, None);
        assert_eq!(sc.rendering_info.rot_matrix.e1, "1");
        assert_eq!(sc.rendering_info.rot_matrix.e2, "0");
        assert_eq!(sc.rendering_info.rot_matrix.e5, "1");
    }

    // ── encode_arc_to_hx tests ───────────────────────────────────────

    #[test]
    fn encode_arc_has_arc_pr_flag_set() {
        let ctrl = Control::Arc {
            arc_type: ArcType::Normal,
            center: ShapePoint::new(0, 0),
            axis1: ShapePoint::new(500, 0),
            axis2: ShapePoint::new(0, 300),
            start1: ShapePoint::new(0, 0),
            end1: ShapePoint::new(0, 0),
            start2: ShapePoint::new(0, 0),
            end2: ShapePoint::new(0, 0),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(600).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_arc_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.has_arc_pr, 1, "Arc must have hasArcPr=1");
    }

    #[test]
    fn encode_arc_type_pie_encoded() {
        let ctrl = Control::Arc {
            arc_type: ArcType::Pie,
            center: ShapePoint::new(0, 0),
            axis1: ShapePoint::new(100, 0),
            axis2: ShapePoint::new(0, 100),
            start1: ShapePoint::new(0, 0),
            end1: ShapePoint::new(0, 0),
            start2: ShapePoint::new(0, 0),
            end2: ShapePoint::new(0, 0),
            width: HwpUnit::new(200).unwrap(),
            height: HwpUnit::new(200).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_arc_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.arc_type, "PIE");
    }

    #[test]
    fn encode_arc_geometry_points_preserved() {
        let ctrl = Control::Arc {
            arc_type: ArcType::Normal,
            center: ShapePoint::new(100, 200),
            axis1: ShapePoint::new(300, 200),
            axis2: ShapePoint::new(100, 400),
            start1: ShapePoint::new(50, 100),
            end1: ShapePoint::new(150, 100),
            start2: ShapePoint::new(200, 300),
            end2: ShapePoint::new(400, 300),
            width: HwpUnit::new(5000).unwrap(),
            height: HwpUnit::new(3000).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_arc_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.center.as_ref().unwrap().x, 100);
        assert_eq!(result.center.as_ref().unwrap().y, 200);
        assert_eq!(result.ax1.as_ref().unwrap().x, 300);
        assert_eq!(result.ax2.as_ref().unwrap().y, 400);
        assert_eq!(result.start1.as_ref().unwrap().x, 50);
        assert_eq!(result.end1.as_ref().unwrap().x, 150);
        assert_eq!(result.start2.as_ref().unwrap().x, 200);
        assert_eq!(result.end2.as_ref().unwrap().x, 400);
    }

    #[test]
    fn encode_arc_size_preserved() {
        let ctrl = Control::Arc {
            arc_type: ArcType::Normal,
            center: ShapePoint::new(0, 0),
            axis1: ShapePoint::new(0, 0),
            axis2: ShapePoint::new(0, 0),
            start1: ShapePoint::new(0, 0),
            end1: ShapePoint::new(0, 0),
            start2: ShapePoint::new(0, 0),
            end2: ShapePoint::new(0, 0),
            width: HwpUnit::new(7000).unwrap(),
            height: HwpUnit::new(4000).unwrap(),
            horz_offset: 100,
            vert_offset: 200,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_arc_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.sz.as_ref().unwrap().width, 7000);
        assert_eq!(result.sz.as_ref().unwrap().height, 4000);
        // Non-zero offset → treat_as_char=0
        assert_eq!(result.pos.as_ref().unwrap().treat_as_char, 0);
        assert_eq!(result.pos.as_ref().unwrap().horz_offset, 100);
    }

    #[test]
    fn encode_arc_shape_comment_is_ho() {
        let ctrl = Control::Arc {
            arc_type: ArcType::Normal,
            center: ShapePoint::new(0, 0),
            axis1: ShapePoint::new(0, 0),
            axis2: ShapePoint::new(0, 0),
            start1: ShapePoint::new(0, 0),
            end1: ShapePoint::new(0, 0),
            start2: ShapePoint::new(0, 0),
            end2: ShapePoint::new(0, 0),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(500).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_arc_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.shape_comment.as_ref().unwrap().text, "호입니다.");
    }

    #[test]
    fn encode_arc_draw_text_is_none() {
        let ctrl = Control::Arc {
            arc_type: ArcType::Normal,
            center: ShapePoint::new(0, 0),
            axis1: ShapePoint::new(0, 0),
            axis2: ShapePoint::new(0, 0),
            start1: ShapePoint::new(0, 0),
            end1: ShapePoint::new(0, 0),
            start2: ShapePoint::new(0, 0),
            end2: ShapePoint::new(0, 0),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(500).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_arc_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert!(result.draw_text.is_none(), "Arc should have no draw_text");
    }

    // ── encode_curve_to_hx tests ─────────────────────────────────────

    #[test]
    fn encode_curve_empty_points_gives_empty_segments() {
        let ctrl = Control::Curve {
            points: vec![],
            segment_types: vec![],
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(500).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_curve_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert!(result.segments.is_empty());
        assert!(result.points.is_empty(), "KS X 6101: coords go in segments, not points");
    }

    #[test]
    fn encode_curve_segments_created_from_points() {
        let ctrl = Control::Curve {
            points: vec![
                ShapePoint::new(0, 0),
                ShapePoint::new(100, 50),
                ShapePoint::new(200, 100),
            ],
            segment_types: vec![CurveSegmentType::Curve, CurveSegmentType::Line],
            width: HwpUnit::new(3000).unwrap(),
            height: HwpUnit::new(1500).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_curve_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.segments.len(), 2);
        let seg0 = &result.segments[0];
        assert_eq!(seg0.seg_type, "CURVE");
        assert_eq!(seg0.x1, 0);
        assert_eq!(seg0.y1, 0);
        assert_eq!(seg0.x2, 100);
        assert_eq!(seg0.y2, 50);
        let seg1 = &result.segments[1];
        assert_eq!(seg1.seg_type, "LINE");
        assert_eq!(seg1.x1, 100);
        assert_eq!(seg1.y1, 50);
        assert_eq!(seg1.x2, 200);
        assert_eq!(seg1.y2, 100);
    }

    #[test]
    fn encode_curve_single_point_gives_no_segments() {
        let ctrl = Control::Curve {
            points: vec![ShapePoint::new(50, 50)],
            segment_types: vec![],
            width: HwpUnit::new(500).unwrap(),
            height: HwpUnit::new(500).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_curve_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert!(result.segments.is_empty(), "single point → no segments");
    }

    #[test]
    fn encode_curve_segment_type_repeats_when_fewer_types_than_segments() {
        // More points than segment_types → extra segments repeat Curve type
        let ctrl = Control::Curve {
            points: vec![
                ShapePoint::new(0, 0),
                ShapePoint::new(100, 0),
                ShapePoint::new(200, 0),
                ShapePoint::new(300, 0),
            ],
            segment_types: vec![CurveSegmentType::Line], // only 1 type for 3 segments
            width: HwpUnit::new(4000).unwrap(),
            height: HwpUnit::new(500).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_curve_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.segments.len(), 3);
        assert_eq!(result.segments[0].seg_type, "LINE");
        // Remaining use default CurveSegmentType::Curve
        assert_eq!(result.segments[1].seg_type, "CURVE");
        assert_eq!(result.segments[2].seg_type, "CURVE");
    }

    #[test]
    fn encode_curve_shape_comment_is_curve() {
        let ctrl = Control::Curve {
            points: vec![],
            segment_types: vec![],
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(500).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_curve_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.shape_comment.as_ref().unwrap().text, "곡선입니다.");
    }

    #[test]
    fn encode_curve_inline_offset_zero_gives_treat_as_char_1() {
        let ctrl = Control::Curve {
            points: vec![],
            segment_types: vec![],
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(500).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_curve_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.pos.as_ref().unwrap().treat_as_char, 1);
    }

    // ── encode_connect_line_to_hx tests ──────────────────────────────

    #[test]
    fn encode_connect_line_control_points_wrapped_with_start_end() {
        let ctrl = Control::ConnectLine {
            start: ShapePoint::new(10, 20),
            end: ShapePoint::new(500, 600),
            control_points: vec![ShapePoint::new(200, 300)],
            connect_type: "BENT".to_string(),
            width: HwpUnit::new(3000).unwrap(),
            height: HwpUnit::new(2000).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(&ctrl, 0, &mut hl).unwrap();
        let cp = result.control_points.as_ref().unwrap();
        // 1 intermediate + start + end = 3 total
        assert_eq!(cp.points.len(), 3);
        assert_eq!(cp.points[0].x, 10);
        assert_eq!(cp.points[0].point_type, "3"); // start
        assert_eq!(cp.points[1].x, 200);
        assert_eq!(cp.points[1].point_type, "2"); // intermediate
        assert_eq!(cp.points[2].x, 500);
        assert_eq!(cp.points[2].point_type, "26"); // end
    }

    #[test]
    fn encode_connect_line_no_intermediate_points() {
        let ctrl = Control::ConnectLine {
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 100),
            control_points: vec![],
            connect_type: "STRAIGHT".to_string(),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(&ctrl, 0, &mut hl).unwrap();
        let cp = result.control_points.as_ref().unwrap();
        // Only start + end = 2
        assert_eq!(cp.points.len(), 2);
        assert_eq!(cp.points[0].point_type, "3");
        assert_eq!(cp.points[1].point_type, "26");
    }

    #[test]
    fn encode_connect_line_connect_type_preserved() {
        let ctrl = Control::ConnectLine {
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 100),
            control_points: vec![],
            connect_type: "CURVED".to_string(),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.connect_type, "CURVED");
    }

    #[test]
    fn encode_connect_line_fill_brush_is_none() {
        // Connect lines have no fill brush (same as regular lines per golden)
        let ctrl = Control::ConnectLine {
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 100),
            control_points: vec![],
            connect_type: "STRAIGHT".to_string(),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert!(result.fill_brush.is_none(), "connect lines must have no fill_brush");
    }

    #[test]
    fn encode_connect_line_start_end_points_set() {
        let ctrl = Control::ConnectLine {
            start: ShapePoint::new(111, 222),
            end: ShapePoint::new(333, 444),
            control_points: vec![],
            connect_type: "STRAIGHT".to_string(),
            width: HwpUnit::new(2000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.start_pt.as_ref().unwrap().x, 111);
        assert_eq!(result.start_pt.as_ref().unwrap().y, 222);
        assert_eq!(result.end_pt.as_ref().unwrap().x, 333);
        assert_eq!(result.end_pt.as_ref().unwrap().y, 444);
    }

    #[test]
    fn encode_connect_line_shape_comment_is_yeongyeolseon() {
        let ctrl = Control::ConnectLine {
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 0),
            control_points: vec![],
            connect_type: "STRAIGHT".to_string(),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.shape_comment.as_ref().unwrap().text, "연결선입니다.");
    }

    #[test]
    fn encode_connect_line_non_zero_offset_gives_treat_as_char_0() {
        let ctrl = Control::ConnectLine {
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 0),
            control_points: vec![],
            connect_type: "STRAIGHT".to_string(),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            horz_offset: 50,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.pos.as_ref().unwrap().treat_as_char, 0);
    }

    // ── encode_line_to_hx tests ──────────────────────────────────────

    #[test]
    fn encode_line_fill_brush_is_none() {
        let ctrl = Control::Line {
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 0),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_line_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert!(result.fill_brush.is_none(), "lines have no fill brush per golden");
    }

    #[test]
    fn encode_line_shape_comment_is_seon() {
        let ctrl = Control::Line {
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 0),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_line_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.shape_comment.as_ref().unwrap().text, "선입니다.");
    }

    #[test]
    fn encode_line_endpoints_preserved() {
        let ctrl = Control::Line {
            start: ShapePoint::new(50, 100),
            end: ShapePoint::new(500, 200),
            width: HwpUnit::new(5000).unwrap(),
            height: HwpUnit::new(2000).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_line_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.start_pt.as_ref().unwrap().x, 50);
        assert_eq!(result.start_pt.as_ref().unwrap().y, 100);
        assert_eq!(result.end_pt.as_ref().unwrap().x, 500);
        assert_eq!(result.end_pt.as_ref().unwrap().y, 200);
    }

    // ── encode_ellipse_to_hx tests ───────────────────────────────────

    #[test]
    fn encode_ellipse_shape_comment_is_taewon() {
        let ctrl = Control::Ellipse {
            center: ShapePoint::new(0, 0),
            axis1: ShapePoint::new(100, 0),
            axis2: ShapePoint::new(0, 50),
            width: HwpUnit::new(200).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_ellipse_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.shape_comment.as_ref().unwrap().text, "타원입니다.");
    }

    #[test]
    fn encode_ellipse_has_arc_pr_zero() {
        let ctrl = Control::Ellipse {
            center: ShapePoint::new(0, 0),
            axis1: ShapePoint::new(100, 0),
            axis2: ShapePoint::new(0, 50),
            width: HwpUnit::new(200).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_ellipse_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.has_arc_pr, 0, "Ellipse must have hasArcPr=0");
    }

    #[test]
    fn encode_ellipse_empty_paragraphs_gives_no_draw_text() {
        let ctrl = Control::Ellipse {
            center: ShapePoint::new(0, 0),
            axis1: ShapePoint::new(100, 0),
            axis2: ShapePoint::new(0, 50),
            width: HwpUnit::new(200).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_ellipse_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert!(result.draw_text.is_none());
    }

    // ── encode_polygon_to_hx tests ───────────────────────────────────

    #[test]
    fn encode_polygon_vertices_preserved() {
        let vertices = vec![
            ShapePoint::new(0, 100),
            ShapePoint::new(50, 0),
            ShapePoint::new(100, 100),
            ShapePoint::new(0, 100), // closed
        ];
        let ctrl = Control::Polygon {
            vertices: vertices.clone(),
            width: HwpUnit::new(2000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_polygon_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.points.len(), 4);
        assert_eq!(result.points[0].x, 0);
        assert_eq!(result.points[0].y, 100);
        assert_eq!(result.points[1].x, 50);
        assert_eq!(result.points[1].y, 0);
    }

    #[test]
    fn encode_polygon_shape_comment_is_dagakbyeong() {
        let ctrl = Control::Polygon {
            vertices: vec![
                ShapePoint::new(0, 0),
                ShapePoint::new(100, 0),
                ShapePoint::new(50, 100),
                ShapePoint::new(0, 0),
            ],
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_polygon_to_hx(&ctrl, 0, &mut hl).unwrap();
        assert_eq!(result.shape_comment.as_ref().unwrap().text, "다각형입니다.");
    }
}
