//! Encodes Core shape controls into HWPX schema types.
//!
//! Split from `section.rs` to enable parallel development of shape features.
//! Functions here convert `Control::TextBox`, `Control::Line`, `Control::Ellipse`,
//! and `Control::Polygon` into their corresponding `Hx*` schema types.

use hwpforge_core::control::{Control, ShapeStyle};
use hwpforge_foundation::{ArrowType, CurveSegmentType, Flip};

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
        dropcap_style: "None".to_string(),
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
        dropcap_style: "None".to_string(),
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
