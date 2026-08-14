//! Encodes Core shape controls into HWPX schema types.
//!
//! Split from `section.rs` to enable parallel development of shape features.
//! Functions here convert `Control::TextBox`, `Control::Line`, `Control::Ellipse`,
//! and `Control::Polygon` into their corresponding `Hx*` schema types.

use hwpforge_core::control::{Control, Fill, ShapeStyle};
use hwpforge_foundation::{ArrowType, CurveSegmentType, DropCapStyle, Flip, GradientType};

/// Extracts the dropcap style string from an optional `ShapeStyle`.
fn dropcap_str(style: &Option<ShapeStyle>) -> String {
    style.as_ref().map_or(DropCapStyle::None, |s| s.drop_cap_style).to_string()
}

/// Resolve `ArrowType` to KS X 6101 string.
///
/// For geometric shapes (Diamond, Oval, Open), 한글 only recognises the `EMPTY_*`
/// form and uses the separate `headfill`/`tailfill` attribute to control fill.
/// Reference: `SimpleLine.hwpx` uses `EMPTY_BOX` + `headfill="1"` for a filled box.
fn resolve_arrow_type_str(arrow_type: &ArrowType) -> String {
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

use super::escape_xml;
use super::section::{
    build_hx_caption, encode_paragraphs_to_sublist_with_align, generate_instid, EncodeSink,
};
use super::EncodeOptions;
use crate::decoder::PathSeg;

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

fn shape_is_floating(horz_offset: i32, vert_offset: i32) -> bool {
    horz_offset != 0 || vert_offset != 0
}

fn shape_numbering_type(horz_offset: i32, vert_offset: i32) -> String {
    if shape_is_floating(horz_offset, vert_offset) {
        "PICTURE".to_string()
    } else {
        "NONE".to_string()
    }
}

fn shape_text_wrap(horz_offset: i32, vert_offset: i32) -> String {
    if shape_is_floating(horz_offset, vert_offset) {
        "IN_FRONT_OF_TEXT".to_string()
    } else {
        "TOP_AND_BOTTOM".to_string()
    }
}

fn shape_position(horz_offset: i32, vert_offset: i32) -> HxTablePos {
    let floating = shape_is_floating(horz_offset, vert_offset);
    HxTablePos {
        treat_as_char: if floating { 0 } else { 1 },
        affect_l_spacing: 0,
        flow_with_text: 0,
        allow_overlap: if floating { 1 } else { 0 },
        hold_anchor_and_so: 0,
        vert_rel_to: if floating { "PAPER".to_string() } else { "PARA".to_string() },
        horz_rel_to: if floating { "PAPER".to_string() } else { "PARA".to_string() },
        vert_align: "TOP".to_string(),
        horz_align: "LEFT".to_string(),
        vert_offset,
        horz_offset,
    }
}

/// Builds the shape-common block for a drawing object of the given pixel size.
///
/// Defaults match 한글's output for a newly created shape:
/// - zero offset, orgSz = curSz = given dimensions
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

    // Rotation angle in integer degrees for HWPX schema
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
            if let Some(ref mut wb) = fill_brush.win_brush {
                wb.face_color = c.to_hex_rgb();
            }
        }

        // Advanced fill (overrides fill_color when present).
        // Per KS X 6101, fillBrush is xs:choice — only ONE child element.
        if let Some(ref fill) = s.fill {
            match fill {
                Fill::Solid { color } => {
                    if let Some(ref mut wb) = fill_brush.win_brush {
                        wb.face_color = color.to_hex_rgb();
                    }
                }
                Fill::Gradient { gradient_type, angle, colors } => {
                    // xs:choice: fillBrush has exactly ONE child (winBrush|gradation|imgBrush).
                    // When using gradation, winBrush must be absent (hwpxlib reference confirms).
                    fill_brush.win_brush = None;
                    // LINEAR: center=0,0 for one-directional gradient.
                    // RADIAL/SQUARE/CONICAL: center=50,50 for center-outward gradient.
                    let (cx, cy) = match gradient_type {
                        GradientType::Linear => (0, 0),
                        _ => (50, 50),
                    };
                    fill_brush.gradation = Some(super::super::schema::shapes::HxGradation {
                        gradation_type: gradient_type.to_string(),
                        angle: *angle,
                        center_x: cx,
                        center_y: cy,
                        step: 255,
                        color_num: colors.len() as i32,
                        step_center: 50,
                        alpha: 0,
                        colors: colors
                            .iter()
                            .map(|(c, _pos)| super::super::schema::shapes::HxGradColor {
                                value: c.to_hex_rgb(),
                            })
                            .collect(),
                    });
                }
                Fill::Pattern { fg_color, bg_color, pattern_type } => {
                    if let Some(ref mut wb) = fill_brush.win_brush {
                        wb.face_color = bg_color.to_hex_rgb();
                        wb.hatch_color = fg_color.to_hex_rgb();
                        wb.hatch_style = Some(pattern_type.to_string());
                    }
                }
                Fill::Image { .. } => {
                    // Image fill requires imgBrush — not yet supported in shapes
                }
                _ => {} // future Fill variants
            }
        }

        // Rotation: Core uses degrees (f32), HWPX uses integer degrees.
        // rem_euclid normalises negatives to [0,360). NaN/INF pass through
        // as NaN but Rust's saturating cast maps NaN → 0, so angle stays 0.
        if let Some(rot) = s.rotation {
            let clamped = rot.rem_euclid(360.0);
            angle = clamped.round() as i32;
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
            line_shape.head_style = resolve_arrow_type_str(&arrow.arrow_type);
            line_shape.head_sz = arrow.size.to_string();
            line_shape.head_fill = if arrow.filled { 1 } else { 0 };
        }
        if let Some(ref arrow) = s.tail_arrow {
            line_shape.tail_style = resolve_arrow_type_str(&arrow.arrow_type);
            line_shape.tail_sz = arrow.size.to_string();
            line_shape.tail_fill = if arrow.filled { 1 } else { 0 };
        }
    }

    // Build rotation matrix for rotation and/or flip.
    // 한글 reads both flip and rotation from rotMatrix.
    // scaMatrix and transMatrix stay identity (unless external scaling).
    //
    // Rotation convention (한글): [cos θ, -sin θ, tx; sin θ, cos θ, ty]
    //   tx = cx*(1-cos) + cy*sin,  ty = cy*(1-cos) - cx*sin
    //   where cx=width/2, cy=height/2 (rotation around center)
    //
    // Flip: horizontal → e1=-1, e3=width; vertical → e5=-1, e6=height
    let has_flip = hx_flip.horizontal != 0 || hx_flip.vertical != 0;
    let rot_matrix = if angle != 0 && !has_flip {
        // Pure rotation, no flip
        let rad = (angle as f64).to_radians();
        let cos_val = rad.cos();
        let sin_val = rad.sin();
        let cx = width as f64 / 2.0;
        let cy = height as f64 / 2.0;
        let tx = cx * (1.0 - cos_val) + cy * sin_val;
        let ty = cy * (1.0 - cos_val) - cx * sin_val;
        HxMatrix {
            e1: format!("{cos_val:.6}"),
            e2: format!("{:.6}", -sin_val),
            e3: format!("{tx:.6}"),
            e4: format!("{sin_val:.6}"),
            e5: format!("{cos_val:.6}"),
            e6: format!("{ty:.6}"),
        }
    } else if has_flip {
        // Flip (with or without rotation) — encode flip in rotMatrix
        // TODO: compose flip+rotation when both are present
        let h = hx_flip.horizontal != 0;
        let v = hx_flip.vertical != 0;
        HxMatrix {
            e1: if h { "-1" } else { "1" }.to_string(),
            e2: "0".to_string(),
            e3: if h { width.to_string() } else { "0".to_string() },
            e4: "0".to_string(),
            e5: if v { "-1" } else { "1" }.to_string(),
            e6: if v { height.to_string() } else { "0".to_string() },
        }
    } else {
        HxMatrix::identity()
    };
    let sca_matrix = HxMatrix::identity();
    let trans_matrix = HxMatrix::identity();

    ShapeCommon {
        offset: HxOffset { x: 0, y: 0 },
        org_sz: HxSizeAttr { width, height },
        cur_sz: HxSizeAttr { width, height },
        flip: hx_flip,
        rotation_info: HxRotationInfo {
            angle,
            center_x: width / 2,
            center_y: height / 2,
            rotate_image: 1,
        },
        rendering_info: HxRenderingInfo { trans_matrix, sca_matrix, rot_matrix },
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
    options: EncodeOptions,
    sink: &mut EncodeSink,
) -> HwpxResult<HxRect> {
    let (paragraphs, width, height, horz_offset, vert_offset, caption, style, text_vertical_align) =
        match ctrl {
            Control::TextBox {
                paragraphs,
                width,
                height,
                horz_offset,
                vert_offset,
                caption,
                style,
                text_vertical_align,
            } => (
                paragraphs,
                *width,
                *height,
                *horz_offset,
                *vert_offset,
                caption,
                style,
                *text_vertical_align,
            ),
            _ => unreachable!("encode_textbox_to_rect called with non-TextBox"),
        };

    let width_hwp = width.as_i32();
    let height_hwp = height.as_i32();

    // Default text margin: 283 HWPUNIT (~1mm)
    const MARGIN: i32 = 283;
    let last_width = width_hwp.max(0) as u32;

    sink.enter(PathSeg::TextBox);
    let sub_list_result = encode_paragraphs_to_sublist_with_align(
        paragraphs,
        depth,
        &text_vertical_align.to_string(),
        hyperlink_entries,
        options,
        sink,
    );
    sink.leave();
    let sub_list = sub_list_result?;
    let sc = build_shape_common(width_hwp, height_hwp, style.as_ref());

    Ok(HxRect {
        id: generate_instid(),
        z_order: 0,
        numbering_type: shape_numbering_type(horz_offset, vert_offset),
        text_wrap: shape_text_wrap(horz_offset, vert_offset),
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

        pos: Some(shape_position(horz_offset, vert_offset)),

        out_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, width_hwp, depth, hyperlink_entries, options, sink))
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

/// Encodes a Core `Control::Rect` into `HxRect`.
///
/// Pure rectangle (no `<hp:drawText>` child). Distinct from `encode_textbox_to_rect`
/// which encodes `Control::TextBox` as a rect with embedded text.
pub(crate) fn encode_rect_to_hx(
    ctrl: &Control,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut EncodeSink,
) -> HwpxResult<HxRect> {
    let (width, height, horz_offset, vert_offset, caption, style) = match ctrl {
        Control::Rect { width, height, horz_offset, vert_offset, caption, style } => {
            (*width, *height, *horz_offset, *vert_offset, caption, style)
        }
        _ => unreachable!("encode_rect_to_hx called with non-Rect"),
    };

    let width_hwp = width.as_i32();
    let height_hwp = height.as_i32();
    let sc = build_shape_common(width_hwp, height_hwp, style.as_ref());

    Ok(HxRect {
        id: generate_instid(),
        z_order: 0,
        numbering_type: shape_numbering_type(horz_offset, vert_offset),
        text_wrap: shape_text_wrap(horz_offset, vert_offset),
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
        shadow: Some(sc.shadow),

        sz: Some(HxTableSz {
            width: width_hwp,
            width_rel_to: "ABSOLUTE".to_string(),
            height: height_hwp,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: 0,
        }),
        pos: Some(shape_position(horz_offset, vert_offset)),
        out_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, width_hwp, depth, hyperlink_entries, options, sink))
            .transpose()?,
        // Pure rect: no embedded text content.
        draw_text: None,
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
    options: EncodeOptions,
    sink: &mut EncodeSink,
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
        numbering_type: shape_numbering_type(*horz_offset, *vert_offset),
        text_wrap: shape_text_wrap(*horz_offset, *vert_offset),
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
        pos: Some(shape_position(*horz_offset, *vert_offset)),
        out_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
        shape_comment: Some(HxShapeComment { text: "선입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, depth, hyperlink_entries, options, sink))
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
    options: EncodeOptions,
    sink: &mut EncodeSink,
) -> HwpxResult<HxEllipse> {
    let (
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
        text_vertical_align,
    ) = match ctrl {
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
            text_vertical_align,
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
            *text_vertical_align,
        ),
        _ => unreachable!("encode_ellipse_to_hx called with non-Ellipse"),
    };

    let w = width.as_i32();
    let h = height.as_i32();
    let sc = build_shape_common(w, h, style.as_ref());

    let draw_text = if paragraphs.is_empty() {
        None
    } else {
        sink.enter(PathSeg::TextBox);
        let sub_list_result = encode_paragraphs_to_sublist_with_align(
            paragraphs,
            depth,
            &text_vertical_align.to_string(),
            hyperlink_entries,
            options,
            sink,
        );
        sink.leave();
        let sub_list = sub_list_result?;
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
        numbering_type: shape_numbering_type(*horz_offset, *vert_offset),
        text_wrap: shape_text_wrap(*horz_offset, *vert_offset),
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
        pos: Some(shape_position(*horz_offset, *vert_offset)),
        out_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
        shape_comment: Some(HxShapeComment { text: "타원입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, depth, hyperlink_entries, options, sink))
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
    options: EncodeOptions,
    sink: &mut EncodeSink,
) -> HwpxResult<HxPolygon> {
    let (
        vertices,
        width,
        height,
        horz_offset,
        vert_offset,
        paragraphs,
        caption,
        style,
        text_vertical_align,
    ) = match ctrl {
        Control::Polygon {
            vertices,
            width,
            height,
            horz_offset,
            vert_offset,
            paragraphs,
            caption,
            style,
            text_vertical_align,
        } => (
            vertices,
            *width,
            *height,
            horz_offset,
            vert_offset,
            paragraphs,
            caption,
            style,
            *text_vertical_align,
        ),
        _ => unreachable!("encode_polygon_to_hx called with non-Polygon"),
    };

    let w = width.as_i32();
    let h = height.as_i32();
    let sc = build_shape_common(w, h, style.as_ref());

    let draw_text = if paragraphs.is_empty() {
        None
    } else {
        sink.enter(PathSeg::TextBox);
        let sub_list_result = encode_paragraphs_to_sublist_with_align(
            paragraphs,
            depth,
            &text_vertical_align.to_string(),
            hyperlink_entries,
            options,
            sink,
        );
        sink.leave();
        let sub_list = sub_list_result?;
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
        numbering_type: shape_numbering_type(*horz_offset, *vert_offset),
        text_wrap: shape_text_wrap(*horz_offset, *vert_offset),
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
        pos: Some(shape_position(*horz_offset, *vert_offset)),
        out_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
        shape_comment: Some(HxShapeComment { text: "다각형입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, depth, hyperlink_entries, options, sink))
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
    options: EncodeOptions,
    sink: &mut EncodeSink,
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
        numbering_type: shape_numbering_type(*horz_offset, *vert_offset),
        text_wrap: shape_text_wrap(*horz_offset, *vert_offset),
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
        pos: Some(shape_position(*horz_offset, *vert_offset)),
        out_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
        shape_comment: Some(HxShapeComment { text: "호입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, depth, hyperlink_entries, options, sink))
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
    options: EncodeOptions,
    sink: &mut EncodeSink,
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
        numbering_type: shape_numbering_type(*horz_offset, *vert_offset),
        text_wrap: shape_text_wrap(*horz_offset, *vert_offset),
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
        pos: Some(shape_position(*horz_offset, *vert_offset)),
        out_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
        shape_comment: Some(HxShapeComment { text: "곡선입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, depth, hyperlink_entries, options, sink))
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
    options: EncodeOptions,
    sink: &mut EncodeSink,
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
        numbering_type: shape_numbering_type(*horz_offset, *vert_offset),
        text_wrap: shape_text_wrap(*horz_offset, *vert_offset),
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
        pos: Some(shape_position(*horz_offset, *vert_offset)),
        out_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
        shape_comment: Some(HxShapeComment { text: "연결선입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, depth, hyperlink_entries, options, sink))
            .transpose()?,
    })
}

/// Serializes any serde value to an XML fragment with the given root element
/// name (e.g. `"hp:rect"`). Used by the recursive group emitter to turn each
/// `Hx*` shape and the container's shape-common sub-blocks into raw XML that
/// is concatenated in document (z-) order inside `<hp:container>`.
fn serialize_with_root<T: serde::Serialize>(value: &T, root: &str) -> HwpxResult<String> {
    let mut buf = String::new();
    let ser = quick_xml::se::Serializer::with_root(&mut buf, Some(root))
        .map_err(|e| crate::error::HwpxError::XmlSerialize { detail: e.to_string() })?;
    value
        .serialize(ser)
        .map_err(|e| crate::error::HwpxError::XmlSerialize { detail: e.to_string() })?;
    Ok(buf)
}

/// Sets the `groupLevel` attribute on a serialized shape XML fragment.
///
/// Every `Hx*` shape encoder emits `groupLevel="0"`; children of a group must
/// carry the parent's depth + 1. We rewrite the first `groupLevel="0"` (which
/// is always the shape's own attribute, the leftmost occurrence) rather than
/// thread the level through every encoder signature.
fn set_group_level(xml: &str, level: u32) -> String {
    xml.replacen(r#"groupLevel="0""#, &format!(r#"groupLevel="{level}""#), 1)
}

/// Rewrites the shape's `<hp:offset x="0" y="0"/>` to its group-relative
/// position. `build_shape_common` always emits a zero offset (a free-floating
/// shape carries its position in `<hp:pos>`), but a container child's
/// position lives in `<hp:offset>` relative to the group origin. We rewrite
/// the leftmost (own) `<hp:offset>` rather than thread coordinates through
/// every per-shape encoder signature — same approach as [`set_group_level`].
fn set_group_child_offset(xml: &str, x: i32, y: i32) -> String {
    xml.replacen(r#"<hp:offset x="0" y="0"/>"#, &format!(r#"<hp:offset x="{x}" y="{y}"/>"#), 1)
}

/// Removes every self-closing `<hp:{local} .../>` element from `xml`.
///
/// Top-level shape encoders emit `<hp:sz>` + `<hp:pos>` for placement
/// relative to the page/paragraph. A container child is positioned by its
/// `<hp:offset>` within the group instead — native 한컴 omits `sz`/`pos` on
/// group children — so we strip them from the child fragment (the container's
/// own `sz`/`pos`, added later by `encode_group_to_xml`, are unaffected).
fn remove_self_closing_element(xml: &str, local: &str) -> String {
    let open = format!("<{local} ");
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        let Some(end_rel) = rest[start..].find("/>") else { break };
        out.push_str(&rest[..start]);
        rest = &rest[start + end_rel + 2..];
    }
    out.push_str(rest);
    out
}

/// Encodes a Core `Control::TextArt` (글맵시) into its full `<hp:textart>` XML
/// fragment.
///
/// Serde cannot model the fixed `<hc:pt0..pt3>` corner block plus the
/// `<hp:textartPr>` sub-element shape, and the `scaMatrix` entries are
/// derived (`width/14173`, `height/14173`) rather than stored, so this emits
/// the native 한컴 element directly (mirroring the group/chart raw-XML path).
/// `text` and string attributes are XML-escaped; the fill color falls back to
/// the native default `#0000FF` when none is carried.
pub(crate) fn encode_text_art_to_xml(ctrl: &Control) -> HwpxResult<String> {
    let (
        text,
        shape,
        font_name,
        font_style,
        align,
        line_spacing,
        char_spacing,
        w,
        h,
        hx,
        vy,
        fill,
        inst,
    ) = match ctrl {
        Control::TextArt {
            text,
            shape,
            font_name,
            font_style,
            align,
            line_spacing,
            char_spacing,
            width,
            height,
            horz_offset,
            vert_offset,
            fill_color,
            inst_id,
        } => (
            text,
            shape,
            font_name,
            font_style,
            align,
            *line_spacing,
            *char_spacing,
            width.as_i32(),
            height.as_i32(),
            *horz_offset,
            *vert_offset,
            *fill_color,
            *inst_id,
        ),
        _ => unreachable!("encode_text_art_to_xml called with non-TextArt"),
    };

    let id = generate_instid();
    let instid = inst.map_or_else(generate_instid, |v| v.to_string());
    let face_color = fill.map_or_else(|| "#0000FF".to_string(), |c| c.to_hex_rgb());
    // scaMatrix maps the 14173-HWPUNIT design box onto the rendered size.
    let sca_x = format!("{:.6}", f64::from(w) / 14173.0);
    let sca_y = format!("{:.6}", f64::from(h) / 14173.0);
    let center_x = w / 2;
    let center_y = h / 2;
    let text_esc = escape_xml(text);
    let shape_esc = escape_xml(shape);
    let font_name_esc = escape_xml(font_name);
    let font_style_esc = escape_xml(font_style);
    let align_esc = escape_xml(align);

    Ok(format!(
        r##"<hp:textart id="{id}" zOrder="0" numberingType="PICTURE" textWrap="SQUARE" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="0" instid="{instid}" text="{text}"><hp:offset x="{hx}" y="{vy}"/><hp:orgSz width="14173" height="14173"/><hp:curSz width="{w}" height="{h}"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="{center_x}" centerY="{center_y}" rotateimage="1"/><hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:scaMatrix e1="{sca_x}" e2="0" e3="0" e4="0" e5="{sca_y}" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo><hp:lineShape color="#000000" width="0" style="NONE" endCap="ROUND" headStyle="NORMAL" tailStyle="NORMAL" headfill="0" tailfill="0" headSz="SMALL_SMALL" tailSz="SMALL_SMALL" outlineStyle="INNER" alpha="0"/><hc:fillBrush><hc:winBrush faceColor="{face_color}" hatchColor="#000000" alpha="0"/></hc:fillBrush><hp:shadow type="NONE" color="#B2B2B2" offsetX="0" offsetY="0" alpha="0"/><hc:pt0 x="0" y="0"/><hc:pt1 x="14173" y="0"/><hc:pt2 x="14173" y="14173"/><hc:pt3 x="0" y="14173"/><hp:textartPr fontName="{font_name}" fontStyle="{font_style}" fontType="TTF" textShape="{shape}" lineSpacing="{line_spacing}" charSpacing="{char_spacing}" align="{align}"><hp:shadow type="NONE" color="#000000" offsetX="0" offsetY="0" alpha="0"/></hp:textartPr><hp:sz width="{w}" widthRelTo="ABSOLUTE" height="{h}" heightRelTo="ABSOLUTE" protect="0"/><hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="1" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="COLUMN" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/><hp:outMargin left="56" right="56" top="0" bottom="0"/><hp:shapeComment>글맵시</hp:shapeComment></hp:textart>"##,
        id = id,
        instid = instid,
        text = text_esc,
        hx = hx,
        vy = vy,
        w = w,
        h = h,
        center_x = center_x,
        center_y = center_y,
        sca_x = sca_x,
        sca_y = sca_y,
        face_color = face_color,
        font_name = font_name_esc,
        font_style = font_style_esc,
        shape = shape_esc,
        line_spacing = line_spacing,
        char_spacing = char_spacing,
        align = align_esc,
    ))
}

/// Encodes a container child's group-relative position into its
/// `<hc:transMatrix>` translation (`e3` = x, `e6` = y).
///
/// 한컴 positions a `<hp:container>` child by the translation components of
/// its transform matrix, NOT by `<hp:offset>` (verified against native
/// `sample-gso-group.hwpx`: a child at offset (17360, 0) carries
/// `transMatrix e3="17360" e6="0"`; an identity matrix renders every child at
/// the group origin → overlap). `build_shape_common` emits an identity
/// transMatrix for non-rotated shapes, so we rewrite that exact string. The
/// sibling `scaMatrix`/`rotMatrix` share the identity numbers but a distinct
/// tag name, so the first-match replace only touches `transMatrix`.
fn set_group_child_translate(xml: &str, x: i32, y: i32) -> String {
    xml.replacen(
        r#"<hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>"#,
        &format!(r#"<hc:transMatrix e1="1" e2="0" e3="{x}" e4="0" e5="1" e6="{y}"/>"#),
        1,
    )
}

/// Rewrites the first `<hp:curSz .../>` to `width="0" height="0"` — the value
/// native 한컴 emits for container children (the rendered size comes from
/// `<hp:orgSz>`). Keeps byte-parity with native group children.
fn zero_cur_sz(xml: &str) -> String {
    let open = "<hp:curSz ";
    let Some(start) = xml.find(open) else { return xml.to_string() };
    let Some(end_rel) = xml[start..].find("/>") else { return xml.to_string() };
    let mut out = String::with_capacity(xml.len());
    out.push_str(&xml[..start]);
    out.push_str(r#"<hp:curSz width="0" height="0"/>"#);
    out.push_str(&xml[start + end_rel + 2..]);
    out
}

/// Group-relative (x, y) offset of a shape child, read from the variant's
/// `horz_offset`/`vert_offset`. Returns `(0, 0)` for variants without an
/// offset (they sit at the group origin).
fn group_child_offset(child: &Control) -> (i32, i32) {
    match child {
        Control::TextBox { horz_offset, vert_offset, .. }
        | Control::Rect { horz_offset, vert_offset, .. }
        | Control::Ellipse { horz_offset, vert_offset, .. }
        | Control::Arc { horz_offset, vert_offset, .. }
        | Control::Polygon { horz_offset, vert_offset, .. }
        | Control::Curve { horz_offset, vert_offset, .. }
        | Control::ConnectLine { horz_offset, vert_offset, .. }
        | Control::Line { horz_offset, vert_offset, .. }
        | Control::Group { horz_offset, vert_offset, .. } => (*horz_offset, *vert_offset),
        _ => (0, 0),
    }
}

/// Encodes one child shape control into its HWPX XML fragment, reusing the
/// existing per-shape encoders. Text-bearing children (`TextBox`,
/// `Ellipse`-with-paragraphs) carry their `<hp:drawText>` automatically via
/// those encoders. Returns `None` for controls that have no flat-shape
/// representation (these are dropped — the decoder already warns for degraded
/// nested groups).
fn encode_group_child_xml(
    child: &Control,
    depth: usize,
    group_level: u32,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut EncodeSink,
) -> HwpxResult<Option<String>> {
    // Nested group (Wave B): recurse into a full `<hp:container>` fragment.
    // `encode_group_to_xml` bakes the correct `groupLevel` into the opening
    // tag directly, so we must NOT run `set_group_level` afterward (it patches
    // a `groupLevel="0"` placeholder that a container never has). The other
    // post-processors are nesting-safe — the container's own shape-common
    // block precedes its children, so first-match ops hit the container's own
    // `offset`/`transMatrix`/`curSz`, and the children's `sz`/`pos` were
    // already stripped during their own recursion.
    if let Control::Group { .. } = child {
        let (x, y) = group_child_offset(child);
        let raw = encode_group_to_xml(child, depth, group_level, hyperlink_entries, options, sink)?;
        let raw = set_group_child_offset(&raw, x, y);
        let raw = set_group_child_translate(&raw, x, y);
        let raw = zero_cur_sz(&raw);
        let raw = remove_self_closing_element(&raw, "hp:sz");
        let raw = remove_self_closing_element(&raw, "hp:pos");
        return Ok(Some(raw));
    }
    let raw = match child {
        Control::TextBox { .. } => serialize_with_root(
            &encode_textbox_to_rect(child, depth, hyperlink_entries, options, sink)?,
            "hp:rect",
        )?,
        Control::Rect { .. } => serialize_with_root(
            &encode_rect_to_hx(child, depth, hyperlink_entries, options, sink)?,
            "hp:rect",
        )?,
        Control::Line { .. } => serialize_with_root(
            &encode_line_to_hx(child, depth, hyperlink_entries, options, sink)?,
            "hp:line",
        )?,
        Control::Ellipse { .. } => serialize_with_root(
            &encode_ellipse_to_hx(child, depth, hyperlink_entries, options, sink)?,
            "hp:ellipse",
        )?,
        Control::Arc { .. } => serialize_with_root(
            &encode_arc_to_hx(child, depth, hyperlink_entries, options, sink)?,
            "hp:ellipse",
        )?,
        Control::Polygon { .. } => serialize_with_root(
            &encode_polygon_to_hx(child, depth, hyperlink_entries, options, sink)?,
            "hp:polygon",
        )?,
        Control::Curve { .. } => serialize_with_root(
            &encode_curve_to_hx(child, depth, hyperlink_entries, options, sink)?,
            "hp:curve",
        )?,
        Control::ConnectLine { .. } => serialize_with_root(
            &encode_connect_line_to_hx(child, depth, hyperlink_entries, options, sink)?,
            "hp:connectLine",
        )?,
        Control::TextArt { .. } => encode_text_art_to_xml(child)?,
        // Nested groups are Wave B; in Wave A a group child is always flat.
        // Anything else (Equation, EmbeddedChart, Group, …) is not emitted as
        // a container child yet — drop it rather than fabricate.
        _ => return Ok(None),
    };
    let (x, y) = group_child_offset(child);
    // 한컴 positions a container child by its transform-matrix translation;
    // `<hp:offset>` mirrors it (native carries both). Identity matrices render
    // every child at the group origin (overlap) — verified visually.
    let raw = set_group_child_offset(&raw, x, y);
    let raw = set_group_child_translate(&raw, x, y);
    let raw = zero_cur_sz(&raw);
    // Container children are placed within the group, not relative to the
    // page; strip the top-level `<hp:sz>`/`<hp:pos>` placement elements the
    // per-shape encoders emit (native 한컴 omits them on group children).
    let raw = remove_self_closing_element(&raw, "hp:sz");
    let raw = remove_self_closing_element(&raw, "hp:pos");
    Ok(Some(set_group_level(&raw, group_level)))
}

/// Encodes a Core `Control::Group` (묶음 객체) into a complete `<hp:container>`
/// XML fragment (KS X 6101 §10.9.8).
///
/// Layout mirrors the native fixture: container attributes, the shape-common
/// block (`offset`/`orgSz`/`curSz`/`flip`/`rotationInfo`/`renderingInfo`),
/// the child shapes in z-order (each with `groupLevel` = parent + 1), then
/// `sz`/`pos`/`outMargin`/`shapeComment`. Children carry absolute geometry —
/// no rescaling (gotcha #3: geometry stays in the `hc:` namespace, which the
/// per-shape encoders already honor).
pub(crate) fn encode_group_to_xml(
    ctrl: &Control,
    depth: usize,
    group_level: u32,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut EncodeSink,
) -> HwpxResult<String> {
    let (children, width, height, horz_offset, vert_offset, inst_id) = match ctrl {
        Control::Group { children, width, height, horz_offset, vert_offset, inst_id } => {
            (children, width.as_i32(), height.as_i32(), *horz_offset, *vert_offset, *inst_id)
        }
        _ => unreachable!("encode_group_to_xml called with non-Group"),
    };

    let sc = build_shape_common(width, height, None);

    // Shape-common block (serialized via the same Hx* sub-structs the per-shape
    // encoders use, so the element shape matches native exactly).
    let mut common = String::new();
    common.push_str(&serialize_with_root(&sc.offset, "hp:offset")?);
    common.push_str(&serialize_with_root(&sc.org_sz, "hp:orgSz")?);
    common.push_str(&serialize_with_root(&sc.cur_sz, "hp:curSz")?);
    common.push_str(&serialize_with_root(&sc.flip, "hp:flip")?);
    common.push_str(&serialize_with_root(&sc.rotation_info, "hp:rotationInfo")?);
    common.push_str(&serialize_with_root(&sc.rendering_info, "hp:renderingInfo")?);

    // Children in z-order, each at the next group level. `emitted_idx` only
    // advances for children that actually produce XML — dropped controls
    // (Equation/EmbeddedChart/… — see `encode_group_child_xml`) never
    // acquire a `GroupChild` path segment, so nested cache-drop warnings
    // index against the emitted sequence, not the source child list.
    let mut children_xml = String::new();
    let mut emitted_idx = 0usize;
    for child in children {
        sink.enter(PathSeg::GroupChild(emitted_idx));
        let child_result =
            encode_group_child_xml(child, depth, group_level + 1, hyperlink_entries, options, sink);
        sink.leave();
        if let Some(xml) = child_result? {
            children_xml.push_str(&xml);
            emitted_idx += 1;
        }
    }

    // Trailing sz / pos / outMargin / shapeComment.
    let sz = serialize_with_root(
        &HxTableSz {
            width,
            width_rel_to: "ABSOLUTE".to_string(),
            height,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: 0,
        },
        "hp:sz",
    )?;
    let pos = serialize_with_root(&shape_position(horz_offset, vert_offset), "hp:pos")?;
    let out_margin = serialize_with_root(
        &HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 },
        "hp:outMargin",
    )?;
    let shape_comment = serialize_with_root(
        &HxShapeComment { text: "묶음 개체입니다.".to_string() },
        "hp:shapeComment",
    )?;

    let id = generate_instid();
    let instid = inst_id.map_or_else(generate_instid, |v| v.to_string());
    Ok(format!(
        r#"<hp:container id="{id}" zOrder="0" numberingType="{numbering}" textWrap="{wrap}" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="{group_level}" instid="{instid}">{common}{children_xml}{sz}{pos}{out_margin}{shape_comment}</hp:container>"#,
        numbering = shape_numbering_type(horz_offset, vert_offset),
        wrap = shape_text_wrap(horz_offset, vert_offset),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::control::{ArrowStyle, Control, Fill, LineStyle, ShapePoint, ShapeStyle};
    use hwpforge_foundation::{
        ArcType, ArrowSize, ArrowType, Color, CurveSegmentType, DropCapStyle, Flip, HwpUnit,
        PatternType, VerticalAlign,
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
        assert_eq!(resolve_arrow_type_str(&ArrowType::None), "NORMAL");
    }

    #[test]
    fn arrow_type_normal_maps_to_arrow() {
        assert_eq!(resolve_arrow_type_str(&ArrowType::Normal), "ARROW");
    }

    #[test]
    fn arrow_type_arrow_maps_to_spear() {
        assert_eq!(resolve_arrow_type_str(&ArrowType::Arrow), "SPEAR");
    }

    #[test]
    fn arrow_type_concave_maps_to_concave_arrow() {
        assert_eq!(resolve_arrow_type_str(&ArrowType::Concave), "CONCAVE_ARROW");
    }

    #[test]
    fn arrow_type_diamond_maps_to_empty_diamond() {
        // Gotcha #14: 한글 only recognises EMPTY_* form for geometric shapes
        assert_eq!(resolve_arrow_type_str(&ArrowType::Diamond), "EMPTY_DIAMOND");
    }

    #[test]
    fn arrow_type_oval_maps_to_empty_circle() {
        assert_eq!(resolve_arrow_type_str(&ArrowType::Oval), "EMPTY_CIRCLE");
    }

    #[test]
    fn arrow_type_open_maps_to_empty_box() {
        assert_eq!(resolve_arrow_type_str(&ArrowType::Open), "EMPTY_BOX");
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
    fn build_shape_common_cur_sz_matches_dimensions() {
        let sc = build_shape_common(8000, 4000, None);
        assert_eq!(sc.cur_sz.width, 8000);
        assert_eq!(sc.cur_sz.height, 4000);
    }

    #[test]
    fn build_shape_common_offset_is_zero() {
        let sc = build_shape_common(1000, 500, None);
        assert_eq!(sc.offset.x, 0);
        assert_eq!(sc.offset.y, 0);
    }

    #[test]
    fn group_encodes_container_with_positioned_children() {
        use hwpforge_foundation::HwpUnit;
        let hu = |v: i32| HwpUnit::new(v).unwrap();
        // A group with two rects at distinct offsets (0,1365) + (17360,0),
        // mirroring the placement in native `sample-gso-group`.
        let child = |w, h, hx, vy| Control::Rect {
            width: hu(w),
            height: hu(h),
            horz_offset: hx,
            vert_offset: vy,
            caption: None,
            style: None,
        };
        let group = Control::Group {
            children: vec![child(14_922, 7780, 0, 1365), child(6998, 12_426, 17_360, 0)],
            width: hu(24_358),
            height: hu(12_426),
            horz_offset: 0,
            vert_offset: 0,
            inst_id: None,
        };
        let mut entries = Vec::new();
        let xml = encode_group_to_xml(
            &group,
            0,
            0,
            &mut entries,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();

        assert!(xml.contains("<hp:container"), "missing container: {xml}");
        assert_eq!(xml.matches("<hp:rect").count(), 2, "expected 2 rect children");
        // Children positioned by transMatrix translation (e3=x, e6=y), NOT an
        // identity matrix — the bug that made every child render at the origin.
        assert!(
            xml.contains(r#"<hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="1365"/>"#),
            "first child missing y-translation: {xml}"
        );
        assert!(
            xml.contains(r#"<hc:transMatrix e1="1" e2="0" e3="17360" e4="0" e5="1" e6="0"/>"#),
            "second child missing x-translation: {xml}"
        );
        // Children carry groupLevel=1; only the container itself owns <hp:pos>.
        assert!(xml.contains(r#"groupLevel="1""#), "child groupLevel not set");
        assert_eq!(xml.matches("<hp:pos ").count(), 1, "only the container has <hp:pos>");
    }

    #[test]
    fn text_art_encodes_native_textart_element() {
        use hwpforge_foundation::HwpUnit;
        let hu = |v: i32| HwpUnit::new(v).unwrap();
        let ta = Control::TextArt {
            text: "글맵시".to_string(),
            shape: "WAVE2".to_string(),
            font_name: "함초롬바탕".to_string(),
            font_style: "보통".to_string(),
            align: "LEFT".to_string(),
            line_spacing: 120,
            char_spacing: 100,
            width: hu(6500),
            height: hu(5000),
            horz_offset: 0,
            vert_offset: 0,
            fill_color: None,
            inst_id: Some(hwpforge_core::ObjectId::new(40_257_166)),
        };
        let xml = encode_text_art_to_xml(&ta).unwrap();
        assert!(xml.contains("<hp:textart "), "missing textart open: {xml}");
        assert!(xml.contains(r#"text="글맵시""#), "text attr missing");
        assert!(xml.contains(r#"textShape="WAVE2""#), "textShape missing");
        assert!(xml.contains(r#"fontName="함초롬바탕""#), "fontName missing");
        assert!(xml.contains(r#"instid="40257166""#), "carried instid missing");
        assert!(xml.contains(r#"<hp:orgSz width="14173" height="14173"/>"#), "orgSz wrong");
        assert!(xml.contains(r#"<hp:curSz width="6500" height="5000"/>"#), "curSz wrong");
        // scaMatrix derived from curSz/orgSz: 6500/14173, 5000/14173.
        assert!(xml.contains(r#"e1="0.458618""#), "scaMatrix e1 wrong: {xml}");
        assert!(xml.contains(r#"e5="0.352783""#), "scaMatrix e5 wrong");
        // No fill → native default blue.
        assert!(xml.contains(r##"faceColor="#0000FF""##), "default fill missing");
    }

    #[test]
    fn group_encodes_nested_container_recursively() {
        use hwpforge_foundation::HwpUnit;
        let hu = |v: i32| HwpUnit::new(v).unwrap();
        // outer group = { inner group { rect, ellipse }, line }, mirroring the
        // native `sample-gso-group-nested` layout: a $con nested inside a $con.
        let rect = Control::Rect {
            width: hu(12_440),
            height: hu(6000),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let ellipse = Control::Ellipse {
            center: ShapePoint::new(5116, 3000),
            axis1: ShapePoint::new(10_232, 3000),
            axis2: ShapePoint::new(5116, 6000),
            width: hu(10_232),
            height: hu(6000),
            horz_offset: 1164,
            vert_offset: 6512,
            paragraphs: Vec::new(),
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let inner = Control::Group {
            children: vec![rect, ellipse],
            width: hu(13_604),
            height: hu(12_512),
            horz_offset: 0,
            vert_offset: 0,
            inst_id: None,
        };
        let line = Control::Line {
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(20_000, 0),
            width: hu(20_000),
            height: hu(0),
            horz_offset: 525,
            vert_offset: 13_422,
            caption: None,
            style: None,
        };
        let outer = Control::Group {
            children: vec![inner, line],
            width: hu(42_520),
            height: hu(13_422),
            horz_offset: 0,
            vert_offset: 0,
            inst_id: None,
        };
        let mut entries = Vec::new();
        let xml = encode_group_to_xml(
            &outer,
            0,
            0,
            &mut entries,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();

        // Two nested containers: outer groupLevel=0, inner groupLevel=1.
        assert_eq!(xml.matches("<hp:container").count(), 2, "expected 2 containers: {xml}");
        assert!(xml.contains(r#"groupLevel="0""#), "outer container groupLevel=0 missing");
        assert!(xml.contains(r#"groupLevel="1""#), "inner container groupLevel=1 missing");
        // Inner group's leaves carry groupLevel=2.
        assert!(xml.contains(r#"groupLevel="2""#), "leaf groupLevel=2 missing");
        assert!(xml.contains("<hp:rect"), "missing nested rect");
        assert!(xml.contains("<hp:ellipse"), "missing nested ellipse");
        assert!(xml.contains("<hp:line"), "missing sibling line");
        // The inner container is positioned by transMatrix translation just like
        // any other child (here identity since it sits at the outer origin).
        assert!(
            xml.contains(r#"<hc:transMatrix e1="1" e2="0" e3="525" e4="0" e5="1" e6="13422"/>"#),
            "sibling line missing translation: {xml}"
        );
        // Only the outermost container owns <hp:pos>; nested ones omit it.
        assert_eq!(xml.matches("<hp:pos ").count(), 1, "only the outer container has <hp:pos>");
    }

    #[test]
    fn build_shape_common_rotation_applied_correctly() {
        let style = ShapeStyle { rotation: Some(45.0), ..Default::default() };
        let sc = build_shape_common(1000, 500, Some(&style));
        assert_eq!(sc.rotation_info.angle, 45);
        // rotation center = dimension / 2
        assert_eq!(sc.rotation_info.center_x, 500);
        assert_eq!(sc.rotation_info.center_y, 250);
    }

    #[test]
    fn build_shape_common_rotation_90_degrees() {
        let style = ShapeStyle { rotation: Some(90.0), ..Default::default() };
        let sc = build_shape_common(2000, 1000, Some(&style));
        assert_eq!(sc.rotation_info.angle, 90);
        // 한글 rotation matrix convention: [cos, -sin; sin, cos]
        let e1: f64 = sc.rendering_info.rot_matrix.e1.parse().unwrap();
        let e2: f64 = sc.rendering_info.rot_matrix.e2.parse().unwrap();
        let e4: f64 = sc.rendering_info.rot_matrix.e4.parse().unwrap();
        assert!(e1.abs() < 0.001, "cos(90°) must be ~0");
        assert!((e2 + 1.0).abs() < 0.001, "-sin(90°) must be ~-1");
        assert!((e4 - 1.0).abs() < 0.001, "sin(90°) must be ~1");
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
        assert_eq!(sc.fill_brush.win_brush.as_ref().unwrap().face_color, "#00FF00");
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
        assert_eq!(sc.fill_brush.win_brush.as_ref().unwrap().face_color, "#FFFFFF");
    }

    #[test]
    fn build_shape_common_no_rotation_uses_identity_matrix() {
        let sc = build_shape_common(1000, 500, None);
        assert_eq!(sc.rendering_info.rot_matrix.e1, "1");
        assert_eq!(sc.rendering_info.rot_matrix.e2, "0");
        assert_eq!(sc.rendering_info.rot_matrix.e5, "1");
    }

    // ── pattern fill encode tests ──────────────────────────────────────

    #[test]
    fn build_shape_common_pattern_fill_sets_hatch_style() {
        let style = ShapeStyle {
            fill: Some(Fill::Pattern {
                pattern_type: PatternType::Horizontal,
                fg_color: Color::BLACK,
                bg_color: Color::WHITE,
            }),
            ..Default::default()
        };
        let sc = build_shape_common(1000, 500, Some(&style));
        let wb = sc.fill_brush.win_brush.as_ref().unwrap();
        assert_eq!(wb.hatch_style, Some("HORIZONTAL".to_string()));
        assert_eq!(wb.face_color, "#FFFFFF");
        assert_eq!(wb.hatch_color, "#000000");
    }

    #[test]
    fn build_shape_common_pattern_backslash_outputs_slash() {
        // 한글 spec reversal: BackSlash → "SLASH"
        let style = ShapeStyle {
            fill: Some(Fill::Pattern {
                pattern_type: PatternType::BackSlash,
                fg_color: Color::from_rgb(0, 150, 0),
                bg_color: Color::from_rgb(230, 255, 230),
            }),
            ..Default::default()
        };
        let sc = build_shape_common(1000, 500, Some(&style));
        let wb = sc.fill_brush.win_brush.as_ref().unwrap();
        assert_eq!(wb.hatch_style, Some("SLASH".to_string()));
    }

    #[test]
    fn build_shape_common_pattern_slash_outputs_back_slash() {
        // 한글 spec reversal: Slash → "BACK_SLASH"
        let style = ShapeStyle {
            fill: Some(Fill::Pattern {
                pattern_type: PatternType::Slash,
                fg_color: Color::from_rgb(150, 0, 150),
                bg_color: Color::from_rgb(255, 230, 255),
            }),
            ..Default::default()
        };
        let sc = build_shape_common(1000, 500, Some(&style));
        let wb = sc.fill_brush.win_brush.as_ref().unwrap();
        assert_eq!(wb.hatch_style, Some("BACK_SLASH".to_string()));
    }

    #[test]
    fn build_shape_common_solid_fill_no_hatch_style() {
        let style = ShapeStyle {
            fill: Some(Fill::Solid { color: Color::from_rgb(255, 0, 0) }),
            ..Default::default()
        };
        let sc = build_shape_common(1000, 500, Some(&style));
        let wb = sc.fill_brush.win_brush.as_ref().unwrap();
        assert_eq!(wb.hatch_style, None);
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
        let result =
            encode_arc_to_hx(&ctrl, 0, &mut hl, EncodeOptions::default(), &mut EncodeSink::new(0))
                .unwrap();
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
        let result =
            encode_arc_to_hx(&ctrl, 0, &mut hl, EncodeOptions::default(), &mut EncodeSink::new(0))
                .unwrap();
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
        let result =
            encode_arc_to_hx(&ctrl, 0, &mut hl, EncodeOptions::default(), &mut EncodeSink::new(0))
                .unwrap();
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
        let result =
            encode_arc_to_hx(&ctrl, 0, &mut hl, EncodeOptions::default(), &mut EncodeSink::new(0))
                .unwrap();
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
        let result =
            encode_arc_to_hx(&ctrl, 0, &mut hl, EncodeOptions::default(), &mut EncodeSink::new(0))
                .unwrap();
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
        let result =
            encode_arc_to_hx(&ctrl, 0, &mut hl, EncodeOptions::default(), &mut EncodeSink::new(0))
                .unwrap();
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
        let result = encode_curve_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
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
        let result = encode_curve_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
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
        let result = encode_curve_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
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
        let result = encode_curve_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
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
        let result = encode_curve_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
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
        let result = encode_curve_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
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
        let result = encode_connect_line_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
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
        let result = encode_connect_line_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
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
        let result = encode_connect_line_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
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
        let result = encode_connect_line_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
        assert!(result.fill_brush.is_none(), "connect lines must have no fill_brush");
    }

    #[test]
    fn encode_connect_line_floating_offset_uses_paper_relative_positioning() {
        // A floating connector (non-zero offset) must anchor to PAPER as
        // PICTURE/IN_FRONT_OF_TEXT, exactly like a floating line/rect — not the
        // inline PARA/TOP_AND_BOTTOM defaults this encoder used to hardcode,
        // which mis-placed 한컴-sourced connectors.
        let ctrl = Control::ConnectLine {
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(14000, 0),
            control_points: vec![],
            connect_type: "STRAIGHT".to_string(),
            width: HwpUnit::new(14000).unwrap(),
            height: HwpUnit::new(0).unwrap(),
            horz_offset: 17657,
            vert_offset: 14057,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
        assert_eq!(result.numbering_type, "PICTURE");
        assert_eq!(result.text_wrap, "IN_FRONT_OF_TEXT");
        let pos = result.pos.as_ref().expect("connect line should carry a pos block");
        assert_eq!(pos.treat_as_char, 0);
        assert_eq!(pos.vert_rel_to, "PAPER");
        assert_eq!(pos.horz_rel_to, "PAPER");
    }

    #[test]
    fn encode_connect_line_inline_offset_zero_keeps_para_relative_positioning() {
        // Inline connectors (zero offset) keep treat-as-char PARA positioning.
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
        let result = encode_connect_line_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
        assert_eq!(result.numbering_type, "NONE");
        assert_eq!(result.text_wrap, "TOP_AND_BOTTOM");
        let pos = result.pos.as_ref().expect("connect line should carry a pos block");
        assert_eq!(pos.treat_as_char, 1);
        assert_eq!(pos.vert_rel_to, "PARA");
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
        let result = encode_connect_line_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
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
        let result = encode_connect_line_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
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
        let result = encode_connect_line_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
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
        let result =
            encode_line_to_hx(&ctrl, 0, &mut hl, EncodeOptions::default(), &mut EncodeSink::new(0))
                .unwrap();
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
        let result =
            encode_line_to_hx(&ctrl, 0, &mut hl, EncodeOptions::default(), &mut EncodeSink::new(0))
                .unwrap();
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
        let result =
            encode_line_to_hx(&ctrl, 0, &mut hl, EncodeOptions::default(), &mut EncodeSink::new(0))
                .unwrap();
        assert_eq!(result.start_pt.as_ref().unwrap().x, 50);
        assert_eq!(result.start_pt.as_ref().unwrap().y, 100);
        assert_eq!(result.end_pt.as_ref().unwrap().x, 500);
        assert_eq!(result.end_pt.as_ref().unwrap().y, 200);
    }

    #[test]
    fn encode_line_non_zero_offset_uses_floating_shape_defaults() {
        let ctrl = Control::Line {
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 0),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            horz_offset: 50,
            vert_offset: 60,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result =
            encode_line_to_hx(&ctrl, 0, &mut hl, EncodeOptions::default(), &mut EncodeSink::new(0))
                .unwrap();
        assert_eq!(result.numbering_type, "PICTURE");
        assert_eq!(result.text_wrap, "IN_FRONT_OF_TEXT");
        let pos = result.pos.as_ref().unwrap();
        assert_eq!(pos.treat_as_char, 0);
        assert_eq!(pos.allow_overlap, 1);
        assert_eq!(pos.vert_rel_to, "PAPER");
        assert_eq!(pos.horz_rel_to, "PAPER");
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
            text_vertical_align: VerticalAlign::Top,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_ellipse_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
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
            text_vertical_align: VerticalAlign::Top,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_ellipse_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
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
            text_vertical_align: VerticalAlign::Top,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_ellipse_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
        assert!(result.draw_text.is_none());
    }

    fn ellipse_with_valign(valign: VerticalAlign) -> Control {
        use hwpforge_core::paragraph::Paragraph;
        use hwpforge_foundation::ParaShapeIndex;
        Control::Ellipse {
            center: ShapePoint::new(0, 0),
            axis1: ShapePoint::new(100, 0),
            axis2: ShapePoint::new(0, 50),
            width: HwpUnit::new(200).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![Paragraph::new(ParaShapeIndex::new(0))],
            caption: None,
            style: None,
            text_vertical_align: valign,
        }
    }

    #[test]
    fn encode_ellipse_default_top_emits_top_sublist() {
        // Regression guard: default Top must still serialize vertAlign="TOP"
        // so existing top-aligned shapes are byte-unchanged.
        let ctrl = ellipse_with_valign(VerticalAlign::Top);
        let mut hl = empty_hyperlinks();
        let result = encode_ellipse_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
        let dt = result.draw_text.expect("ellipse with text must emit drawText");
        assert_eq!(dt.sub_list.vert_align, "TOP");
    }

    #[test]
    fn encode_ellipse_center_emits_center_sublist() {
        let ctrl = ellipse_with_valign(VerticalAlign::Center);
        let mut hl = empty_hyperlinks();
        let result = encode_ellipse_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
        let dt = result.draw_text.expect("ellipse with text must emit drawText");
        assert_eq!(dt.sub_list.vert_align, "CENTER");
    }

    #[test]
    fn encode_ellipse_bottom_emits_bottom_sublist() {
        let ctrl = ellipse_with_valign(VerticalAlign::Bottom);
        let mut hl = empty_hyperlinks();
        let result = encode_ellipse_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
        let dt = result.draw_text.expect("ellipse with text must emit drawText");
        assert_eq!(dt.sub_list.vert_align, "BOTTOM");
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
            text_vertical_align: VerticalAlign::Top,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_polygon_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
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
            text_vertical_align: VerticalAlign::Top,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_polygon_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
        assert_eq!(result.shape_comment.as_ref().unwrap().text, "다각형입니다.");
    }

    #[test]
    fn encode_polygon_non_zero_offset_uses_floating_shape_defaults() {
        let ctrl = Control::Polygon {
            vertices: vec![
                ShapePoint::new(0, 0),
                ShapePoint::new(100, 0),
                ShapePoint::new(50, 100),
                ShapePoint::new(0, 0),
            ],
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            horz_offset: 50,
            vert_offset: 60,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_polygon_to_hx(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
        assert_eq!(result.numbering_type, "PICTURE");
        assert_eq!(result.text_wrap, "IN_FRONT_OF_TEXT");
        let pos = result.pos.as_ref().unwrap();
        assert_eq!(pos.treat_as_char, 0);
        assert_eq!(pos.allow_overlap, 1);
        assert_eq!(pos.vert_rel_to, "PAPER");
        assert_eq!(pos.horz_rel_to, "PAPER");
    }

    #[test]
    fn encode_rect_pure_emits_no_draw_text() {
        let ctrl = Control::rect(HwpUnit::new(8000).unwrap(), HwpUnit::new(4000).unwrap()).unwrap();
        let mut hl = empty_hyperlinks();
        let result =
            encode_rect_to_hx(&ctrl, 0, &mut hl, EncodeOptions::default(), &mut EncodeSink::new(0))
                .unwrap();
        assert!(result.draw_text.is_none(), "pure rect must not emit <hp:drawText>");
        let sz = result.sz.as_ref().unwrap();
        assert_eq!(sz.width, 8000);
        assert_eq!(sz.height, 4000);
        // Inline positioning (treat_as_char=1) when offsets are zero.
        assert_eq!(result.pos.as_ref().unwrap().treat_as_char, 1);
    }

    #[test]
    fn encode_rect_serializes_to_hp_rect_element() {
        let ctrl = Control::rect(HwpUnit::new(5000).unwrap(), HwpUnit::new(3000).unwrap()).unwrap();
        let mut hl = empty_hyperlinks();
        let rect =
            encode_rect_to_hx(&ctrl, 0, &mut hl, EncodeOptions::default(), &mut EncodeSink::new(0))
                .unwrap();
        let mut buf = String::new();
        let ser = quick_xml::se::Serializer::with_root(&mut buf, Some("hp:rect")).unwrap();
        serde::Serialize::serialize(&rect, ser).unwrap();
        assert!(buf.contains("<hp:rect"), "encoded XML should contain <hp:rect: {buf}");
        assert!(!buf.contains("<hp:drawText"), "pure rect must not contain <hp:drawText>: {buf}");
    }

    #[test]
    fn encode_textbox_non_zero_offset_uses_floating_shape_defaults() {
        let ctrl = Control::TextBox {
            paragraphs: vec![],
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            horz_offset: 50,
            vert_offset: 60,
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_textbox_to_rect(
            &ctrl,
            0,
            &mut hl,
            EncodeOptions::default(),
            &mut EncodeSink::new(0),
        )
        .unwrap();
        assert_eq!(result.numbering_type, "PICTURE");
        assert_eq!(result.text_wrap, "IN_FRONT_OF_TEXT");
        let pos = result.pos.as_ref().unwrap();
        assert_eq!(pos.treat_as_char, 0);
        assert_eq!(pos.allow_overlap, 1);
        assert_eq!(pos.vert_rel_to, "PAPER");
        assert_eq!(pos.horz_rel_to, "PAPER");
    }
}
