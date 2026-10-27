//! Encodes Core shape controls into HWPX schema types.
//!
//! Split from `section.rs` to enable parallel development of shape features.
//! Functions here convert `Control::TextBox`, `Control::Line`, `Control::Ellipse`,
//! and `Control::Polygon` into their corresponding `Hx*` schema types.

use hwpforge_core::control::geometry::{
    drop_original_size, drop_original_sizes, frame_plan, group_child_frame_plan, group_plan,
    local_transform, preserve_group_child, AngleRounding, CoefficientRounding, EmissionPolicy,
    FramePlan, GroupNodePlan,
};
use hwpforge_core::control::{Control, Fill, GroupChild, ShapeStyle};
use hwpforge_core::placement::ObjectPlacement;
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
use crate::wire_xml::textart_text_to_wire;

use super::escape_xml;
use super::section::{
    build_hx_caption, encode_paragraphs_to_sublist_with_align, generate_instid,
    DocumentEncodeContext,
};
use super::EncodeOptions;
use crate::decoder::PathSeg;

// ── Shape-common helpers ─────────────────────────────────────────

/// Collected common sub-elements required by 한글 for all drawing objects.
///
/// All four shape encoders (rect/textbox, line, ellipse, polygon) produce
/// the same prefix block. This struct avoids repeating the construction logic.
pub(crate) struct ShapeCommon {
    pub size: (i32, i32),
    pub offset: HxOffset,
    pub org_sz: HxSizeAttr,
    pub cur_sz: HxSizeAttr,
    pub flip: HxFlip,
    pub rotation_info: HxRotationInfo,
    pub rendering_info: HxRenderingInfo,
    pub line_shape: HxLineShape,
    /// `None` = no `<hc:fillBrush>` (Core [`Fill::None`]: the interior stays
    /// transparent).
    pub fill_brush: Option<HxFillBrush>,
    pub shadow: HxShadow,
}

/// `<hp:*>` `numberingType` for a shape.
///
/// `None` (the historical inline default) and an inline (`treat_as_char`)
/// placement both number as `NONE`; an anchored/floating object numbers as
/// `PICTURE`. Anchoring is keyed on `treat_as_char` (HWP5 bit0), not on a
/// non-zero offset — an object anchored at the origin is still floating.
fn shape_numbering_type(placement: Option<&ObjectPlacement>) -> String {
    match placement {
        Some(p) if !p.treat_as_char => "PICTURE".to_string(),
        _ => "NONE".to_string(),
    }
}

/// `<hp:*>` `textWrap` for a shape. `None` → the inline default
/// `TOP_AND_BOTTOM`; `Some` re-emits the carried wrap mode.
fn shape_text_wrap(placement: Option<&ObjectPlacement>) -> String {
    match placement {
        Some(p) => p.text_wrap.as_hwpx_str().into_owned(),
        None => "TOP_AND_BOTTOM".to_string(),
    }
}

/// `<hp:*>` `textFlow` for a shape. `None` → the inline default `BOTH_SIDES`;
/// `Some` re-emits the carried flow mode.
fn shape_text_flow(placement: Option<&ObjectPlacement>) -> String {
    match placement {
        Some(p) => p.text_flow.as_hwpx_str().into_owned(),
        None => "BOTH_SIDES".to_string(),
    }
}

/// Builds the `<hp:pos>` block for a shape.
///
/// The `None` branch reproduces the historical zero-offset inline output
/// byte-for-byte (treat-as-char, paragraph-anchored, zero offset); the `Some`
/// branch mirrors `build_picture_position`, sourcing every carried field from
/// the [`ObjectPlacement`].
fn shape_position(placement: Option<&ObjectPlacement>) -> HxTablePos {
    match placement {
        Some(p) => HxTablePos {
            treat_as_char: u32::from(p.treat_as_char),
            affect_l_spacing: u32::from(p.affect_line_spacing),
            flow_with_text: u32::from(p.flow_with_text),
            allow_overlap: u32::from(p.allow_overlap),
            hold_anchor_and_so: u32::from(p.hold_anchor_and_so),
            vert_rel_to: p.vert_rel_to.as_hwpx_str().into_owned(),
            horz_rel_to: p.horz_rel_to.as_hwpx_str().into_owned(),
            vert_align: p.vert_align.as_hwpx_str().to_string(),
            horz_align: p.horz_align.as_hwpx_str().to_string(),
            vert_offset: p.vert_offset.as_i32(),
            horz_offset: p.horz_offset.as_i32(),
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

/// What HWPX can store for a shape's own transform: `rotationInfo@angle` is
/// whole degrees, matrix coefficients are written with six decimals, and one
/// `rotMatrix` holds a mirror and a rotation together — Hancom itself writes
/// a shape turned and mirrored as its stored angle and flip with the single
/// matrix `R(angle)·F`, as observed in saved rotated and mirrored shapes.
pub(crate) const HWPX_POLICY: EmissionPolicy =
    EmissionPolicy::new(AngleRounding::WholeDegrees, CoefficientRounding::Decimals(6), true);

/// Width, height and style of a control with a shape-common block (`0`/`None`
/// for anything else).
fn shape_parts(ctrl: &Control) -> (i32, i32, Option<&ShapeStyle>) {
    match ctrl {
        Control::TextBox { width, height, style, .. }
        | Control::Rect { width, height, style, .. }
        | Control::Ellipse { width, height, style, .. }
        | Control::Arc { width, height, style, .. }
        | Control::Polygon { width, height, style, .. }
        | Control::Curve { width, height, style, .. }
        | Control::ConnectLine { width, height, style, .. }
        | Control::Line { width, height, style, .. } => {
            (width.as_i32(), height.as_i32(), style.as_ref())
        }
        Control::Group { width, height, .. } => (width.as_i32(), height.as_i32(), None),
        _ => (0, 0, None),
    }
}

/// Reports a shape whose stored values ([`HWPX_POLICY`]) land more than
/// 2 HWPUNIT from its exact Core position as
/// [`EncodeWarning::ContentLost`](super::EncodeWarning::ContentLost)
/// (`shape_emission`) — a large box turned by a fraction of a degree, say,
/// once its angle is rounded to whole degrees, or an original box so much
/// larger than the shape that even the displayed box misses.
pub(crate) fn report_shape_emission(plan: &FramePlan, sink: &mut DocumentEncodeContext) {
    if let Some(loss) = plan.loss() {
        sink.content_lost("shape_emission", loss.detail().to_string());
    }
}

fn report_shape_output(
    ctrl: &Control,
    as_group_child: bool,
    frame: &FramePlan,
    sink: &mut DocumentEncodeContext,
) {
    report_shape_emission(frame, sink);
    let (w, h, style) = shape_parts(ctrl);
    let (rotation, flip) = style.map_or((None, None), |s| (s.rotation, s.flip));
    if as_group_child || frame.loss().is_some() || !super::standalone::transformed(rotation, flip) {
        return;
    }
    let emitted = super::standalone::plan((w, h), rotation, flip, frame);
    let exact = frame_plan(ctrl, &EmissionPolicy::exact()).expect("drawing frame");
    let residual = super::standalone::residual(frame, &exact, &emitted);
    if residual > 2.0 {
        sink.content_lost(
            "shape_emission",
            format!("actual standalone T*R*S residual {residual} HWPUNIT"),
        );
    }
}

/// The emission plan of a shape control under [`HWPX_POLICY`]
/// ([`frame_plan`]): what its `<hp:orgSz>`, `scaMatrix` and points store.
///
/// A negative original size has no place on the wire (Hancom's readings of
/// one are unknown) and is refused rather than written wrapped, also when the
/// Core value never went through `validate`.
/// 이것을 실패시키는 것: 이 거부를 지우기(`negative_original_size_is_not_written`
/// — `orgSz` 가 감싼 값 `4294967295` 로 써진다).
pub(crate) fn shape_frame_plan(ctrl: &Control, as_group_child: bool) -> HwpxResult<FramePlan> {
    if let Some(o) = ctrl.frame().and_then(|f| f.original_size) {
        if o.width.as_i32() < 0 || o.height.as_i32() < 0 {
            return Err(crate::error::HwpxError::InvalidStructure {
                detail: format!(
                    "{} has a negative original size {}x{}",
                    ctrl.kind_name(),
                    o.width.as_i32(),
                    o.height.as_i32()
                ),
            });
        }
    }
    let plan = if as_group_child {
        group_child_frame_plan(ctrl, &HWPX_POLICY)
    } else {
        frame_plan(ctrl, &HWPX_POLICY)
    };
    plan.ok_or_else(|| crate::error::HwpxError::InvalidStructure {
        detail: format!("{} has no shape box", ctrl.kind_name()),
    })
}

/// `scaMatrix` for a plan's scale: the identity Hancom writes for an
/// unscaled object, else the two ratios with six decimals.
pub(crate) fn scale_matrix(scale: (f64, f64)) -> HxMatrix {
    if scale == (1.0, 1.0) {
        return HxMatrix::identity();
    }
    HxMatrix {
        e1: format!("{:.6}", scale.0),
        e2: "0".to_string(),
        e3: "0".to_string(),
        e4: "0".to_string(),
        e5: format!("{:.6}", scale.1),
        e6: "0".to_string(),
    }
}

/// `<hp:sz protect>` of an object: its frame's size lock.
pub(crate) fn size_protect(ctrl: &Control) -> u32 {
    u32::from(ctrl.frame().is_some_and(|f| f.size_protect))
}

/// Builds the shape-common block for a drawing object.
///
/// Defaults match 한글's output for a newly created shape:
/// - zero offset, orgSz = the plan's original box, curSz = the control's
///   dimensions, `scaMatrix` = the plan's scale
/// - identity rotation/rendering matrices
/// - solid black border, white fill, no shadow
///
/// When the control's `ShapeStyle` carries rotation, flip, or arrow
/// overrides, they are applied to the common block instead of identity
/// values. The rotation matrix is the Core local transform under
/// [`HWPX_POLICY`].
pub(crate) fn build_shape_common(
    ctrl: &Control,
    plan: &FramePlan,
    as_group_child: bool,
) -> ShapeCommon {
    let (width, height, style) = shape_parts(ctrl);
    let mut line_shape = HxLineShape::default_solid();
    let mut fill_brush = HxFillBrush::default_white();
    // 이것을 실패시키는 것: `Fill::None` 갈래를 지우기 — 채우기 없던 도형이
    // 흰 채우기로 다시 써진다 (`no_fill_shape_writes_no_fill_brush`,
    // `tests/shape_no_fill.rs`).
    let mut no_fill = false;

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
                Fill::Gradient {
                    gradient_type,
                    angle,
                    colors,
                    center_x,
                    center_y,
                    step,
                    step_center,
                } => {
                    // xs:choice: fillBrush has exactly ONE child (winBrush|gradation|imgBrush).
                    // When using gradation, winBrush must be absent (hwpxlib reference confirms).
                    fill_brush.win_brush = None;
                    // Absent values (`None`) keep what this encoder always
                    // wrote, so old JSON and API gradients encode unchanged:
                    // LINEAR: center=0,0 for one-directional gradient.
                    // RADIAL/SQUARE/CONICAL: center=50,50 for center-outward gradient.
                    // 이것을 실패시키는 것: `None` 을 0 으로 쓰기(옛 RADIAL 이 0,0 으로,
                    // `gradient_without_center_keeps_the_per_kind_default`).
                    let (cx, cy) = match gradient_type {
                        GradientType::Linear => (0, 0),
                        _ => (50, 50),
                    };
                    fill_brush.gradation = Some(super::super::schema::shapes::HxGradation {
                        gradation_type: gradient_type.to_string(),
                        angle: *angle,
                        center_x: center_x.unwrap_or(cx),
                        center_y: center_y.unwrap_or(cy),
                        step: step.map_or(255, |s| i32::try_from(s).unwrap_or(i32::MAX)),
                        color_num: colors.len() as i32,
                        step_center: step_center
                            .map_or(50, |s| i32::try_from(s).unwrap_or(i32::MAX)),
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
                Fill::Image { image_id, mode } => {
                    // The image is named as a picture names its own
                    // (`BinData/image1.png` → `image1`); a name the package
                    // does not carry is reported by the reference check
                    // (`ImageReferenceUnresolved`), not dropped here.
                    // 이것을 실패시키는 것: 이 갈래를 빈 갈래로 되돌리기(그림
                    // 채우기가 흰 칠로, `picture_fill_round_trips`).
                    let without_prefix = image_id.strip_prefix("BinData/").unwrap_or(image_id);
                    let binary_ref = without_prefix
                        .rfind('.')
                        .map_or(without_prefix, |dot| &without_prefix[..dot]);
                    let wire_mode = crate::decoder::shapes::IMAGE_FILL_MODES
                        .iter()
                        .find(|(_, m)| m == mode)
                        .map_or("TILE", |(wire, _)| wire);
                    fill_brush.win_brush = None;
                    fill_brush.img_brush = Some(crate::schema::header::HxImgBrush {
                        mode: wire_mode.to_string(),
                        img: Some(crate::schema::header::HxImg {
                            binary_item_id_ref: binary_ref.to_string(),
                            bright: 0,
                            contrast: 0,
                            effect: "REAL_PIC".to_string(),
                            alpha: 0,
                        }),
                    });
                }
                Fill::None => no_fill = true,
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

    // Rotation and/or flip in one `rotMatrix` (한글 reads both from it):
    // the Core local transform `C + R(θ)·F·(p − C)` under `HWPX_POLICY`.
    // scaMatrix and transMatrix stay identity (unless external scaling).
    // A mirror alone keeps its integer form: horizontal → e1=-1, e3=width;
    // vertical → e5=-1, e6=height.
    let has_flip = hx_flip.horizontal != 0 || hx_flip.vertical != 0;
    let rot_matrix = if angle == 0 && !has_flip {
        HxMatrix::identity()
    } else if angle == 0 {
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
        let m = local_transform(ctrl, &HWPX_POLICY)
            .map_or([1.0, 0.0, 0.0, 0.0, 1.0, 0.0], |t| t.matrix());
        HxMatrix {
            e1: format!("{:.6}", m[0]),
            e2: format!("{:.6}", m[1]),
            e3: format!("{:.6}", m[2]),
            e4: format!("{:.6}", m[3]),
            e5: format!("{:.6}", m[4]),
            e6: format!("{:.6}", m[5]),
        }
    };
    // the original box and the scale to the displayed one.
    // 이것을 실패시키는 것: `orgSz` 를 현재 크기로 되돌리기(`r6_circle_keeps_its_original_box`).
    let sca_matrix = scale_matrix(plan.scale());
    let trans_matrix = HxMatrix::identity();
    let (org_w, org_h) = plan.original_size();

    let mut common = ShapeCommon {
        size: (width, height),
        offset: HxOffset { x: 0, y: 0 },
        org_sz: HxSizeAttr { width: org_w, height: org_h },
        cur_sz: HxSizeAttr { width, height },
        flip: hx_flip,
        rotation_info: HxRotationInfo {
            angle,
            center_x: width / 2,
            center_y: height / 2,
            rotate_image: 1,
        },
        rendering_info: HxRenderingInfo {
            trans_matrix,
            sca_matrix,
            rot_matrix,
            extra_pairs: Vec::new(),
        },
        line_shape,
        fill_brush: (!no_fill).then_some(fill_brush),
        shadow: HxShadow::default_none(),
    };
    let (rotation, flip) = style.map_or((None, None), |s| (s.rotation, s.flip));
    if !as_group_child && super::standalone::transformed(rotation, flip) {
        let standalone = super::standalone::plan((width, height), rotation, flip, plan);
        common.size = standalone.size;
        common.offset = standalone.offset;
        common.cur_sz = standalone.cur;
        common.rotation_info = standalone.rotation;
        common.rendering_info = standalone.rendering;
        common.flip = standalone.flip;
    }
    common
}

/// Encodes a Core `Control::TextBox` into `HxRect` with `<hp:drawText>`.
///
/// MVP: inline positioning (treatAsChar=1) when offsets are (0,0).
pub(crate) fn encode_textbox_to_rect(
    ctrl: &Control,
    as_group_child: bool,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut DocumentEncodeContext,
) -> HwpxResult<HxRect> {
    let (paragraphs, width, height, placement, caption, style, text_vertical_align, text_margin) =
        match ctrl {
            Control::TextBox {
                frame: _,
                text_margin,
                paragraphs,
                width,
                height,
                placement,
                caption,
                style,
                text_vertical_align,
            } => (
                paragraphs,
                *width,
                *height,
                placement,
                caption,
                style,
                *text_vertical_align,
                *text_margin,
            ),
            _ => unreachable!("encode_textbox_to_rect called with non-TextBox"),
        };

    let _width_hwp = width.as_i32();
    let _height_hwp = height.as_i32();

    // Default text margin: 283 HWPUNIT (~1mm)
    const MARGIN: i32 = 283;

    sink.enter(PathSeg::TextBox);
    let sub_list_result = encode_paragraphs_to_sublist_with_align(
        paragraphs,
        &text_vertical_align.to_string(),
        hyperlink_entries,
        options,
        sink,
    );
    sink.leave();
    let sub_list = sub_list_result?;
    let plan = shape_frame_plan(ctrl, as_group_child)?;
    let sc = build_shape_common(ctrl, &plan, as_group_child);
    report_shape_output(ctrl, as_group_child, &plan, sink);
    let (width_hwp, height_hwp) = sc.size;
    let last_width = width_hwp.max(0) as u32;

    Ok(HxRect {
        id: generate_instid(),
        z_order: 0,
        numbering_type: shape_numbering_type(placement.as_ref()),
        text_wrap: shape_text_wrap(placement.as_ref()),
        text_flow: shape_text_flow(placement.as_ref()),
        lock: 0,
        dropcap_style: dropcap_str(style),
        href: String::new(),
        group_level: 0,
        instid: generate_instid(),
        ratio: style.as_ref().and_then(|s| s.corner_ratio).unwrap_or(0),

        offset: Some(sc.offset),
        org_sz: Some(sc.org_sz),
        cur_sz: Some(sc.cur_sz),
        flip: Some(sc.flip),
        rotation_info: Some(sc.rotation_info),
        rendering_info: Some(sc.rendering_info),
        line_shape: Some(sc.line_shape),
        fill_brush: sc.fill_brush,
        shadow: Some(HxShadow { alpha: 178, ..HxShadow::default_none() }),

        sz: Some(HxTableSz {
            width: width_hwp,
            width_rel_to: "ABSOLUTE".to_string(),
            height: height_hwp,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: size_protect(ctrl),
        }),

        pos: Some(shape_position(placement.as_ref())),

        out_margin: Some(object_out_margin(placement.as_ref())),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, width_hwp, hyperlink_entries, options, sink))
            .transpose()?,

        draw_text: Some(HxDrawText {
            last_width,
            name: String::new(),
            editable: 0,
            sub_list,
            // The box's own margin; `None` = 283 on every side.
            // 이것을 실패시키는 것: 상수 복귀 / 왼·오, 위·아래 맞바꿈
            // (`shape_text_margins_round_trip`).
            text_margin: Some(match text_margin {
                Some(m) => HxTableMargin {
                    left: m.left.as_i32(),
                    right: m.right.as_i32(),
                    top: m.top.as_i32(),
                    bottom: m.bottom.as_i32(),
                },
                None => HxTableMargin { left: MARGIN, right: MARGIN, top: MARGIN, bottom: MARGIN },
            }),
        }),

        pt0: Some(HxPoint { x: 0, y: 0 }),
        pt1: Some(HxPoint { x: plan.original_size().0, y: 0 }),
        pt2: Some(HxPoint { x: plan.original_size().0, y: plan.original_size().1 }),
        pt3: Some(HxPoint { x: 0, y: plan.original_size().1 }),
        shape_comment: Some(HxShapeComment { text: "사각형입니다.".to_string() }),
    })
}

/// Encodes a Core `Control::Rect` into `HxRect`.
///
/// Pure rectangle (no `<hp:drawText>` child). Distinct from `encode_textbox_to_rect`
/// which encodes `Control::TextBox` as a rect with embedded text.
pub(crate) fn encode_rect_to_hx(
    ctrl: &Control,
    as_group_child: bool,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut DocumentEncodeContext,
) -> HwpxResult<HxRect> {
    let (width, height, placement, caption, style) = match ctrl {
        Control::Rect { frame: _, width, height, placement, caption, style } => {
            (*width, *height, placement, caption, style)
        }
        _ => unreachable!("encode_rect_to_hx called with non-Rect"),
    };

    let _width_hwp = width.as_i32();
    let _height_hwp = height.as_i32();
    let plan = shape_frame_plan(ctrl, as_group_child)?;
    let sc = build_shape_common(ctrl, &plan, as_group_child);
    report_shape_output(ctrl, as_group_child, &plan, sink);
    let (width_hwp, height_hwp) = sc.size;

    Ok(HxRect {
        id: generate_instid(),
        z_order: 0,
        numbering_type: shape_numbering_type(placement.as_ref()),
        text_wrap: shape_text_wrap(placement.as_ref()),
        text_flow: shape_text_flow(placement.as_ref()),
        lock: 0,
        dropcap_style: dropcap_str(style),
        href: String::new(),
        group_level: 0,
        instid: generate_instid(),
        ratio: style.as_ref().and_then(|s| s.corner_ratio).unwrap_or(0),

        offset: Some(sc.offset),
        org_sz: Some(sc.org_sz),
        cur_sz: Some(sc.cur_sz),
        flip: Some(sc.flip),
        rotation_info: Some(sc.rotation_info),
        rendering_info: Some(sc.rendering_info),
        line_shape: Some(sc.line_shape),
        fill_brush: sc.fill_brush,
        shadow: Some(sc.shadow),

        sz: Some(HxTableSz {
            width: width_hwp,
            width_rel_to: "ABSOLUTE".to_string(),
            height: height_hwp,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: size_protect(ctrl),
        }),
        pos: Some(shape_position(placement.as_ref())),
        out_margin: Some(object_out_margin(placement.as_ref())),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, width_hwp, hyperlink_entries, options, sink))
            .transpose()?,
        // Pure rect: no embedded text content.
        draw_text: None,
        pt0: Some(HxPoint { x: 0, y: 0 }),
        pt1: Some(HxPoint { x: plan.original_size().0, y: 0 }),
        pt2: Some(HxPoint { x: plan.original_size().0, y: plan.original_size().1 }),
        pt3: Some(HxPoint { x: 0, y: plan.original_size().1 }),
        shape_comment: Some(HxShapeComment { text: "사각형입니다.".to_string() }),
    })
}

/// Encodes a Core `Control::Line` into `HxLine`.
pub(crate) fn encode_line_to_hx(
    ctrl: &Control,
    as_group_child: bool,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut DocumentEncodeContext,
) -> HwpxResult<HxLine> {
    let (_, _, width, height, placement, caption, style) = match ctrl {
        Control::Line { frame: _, start, end, width, height, placement, caption, style } => {
            (start, end, *width, *height, placement, caption, style)
        }
        _ => unreachable!("encode_line_to_hx called with non-Line"),
    };

    let _w = width.as_i32();
    let _h = height.as_i32();
    let plan = shape_frame_plan(ctrl, as_group_child)?;
    let sc = build_shape_common(ctrl, &plan, as_group_child);
    report_shape_output(ctrl, as_group_child, &plan, sink);
    let (w, h) = sc.size;
    let stored = plan.points();

    Ok(HxLine {
        id: generate_instid(),
        z_order: 0,
        numbering_type: shape_numbering_type(placement.as_ref()),
        text_wrap: shape_text_wrap(placement.as_ref()),
        text_flow: shape_text_flow(placement.as_ref()),
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
        fill_brush: None, // lines have no fill brush
        shadow: Some(sc.shadow),
        sz: Some(HxTableSz {
            width: w,
            width_rel_to: "ABSOLUTE".to_string(),
            height: h,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: size_protect(ctrl),
        }),
        pos: Some(shape_position(placement.as_ref())),
        out_margin: Some(object_out_margin(placement.as_ref())),
        shape_comment: Some(HxShapeComment { text: "선입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, hyperlink_entries, options, sink))
            .transpose()?,
        // The points as the plan stores them: in the original box.
        // 이것을 실패시키는 것: Core 점을 그대로 쓰기(fallback 한 극단 비율 선이
        // 원래 상자 좌표 그대로 남는다 — `extreme_ratio_line_falls_back`).
        start_pt: Some(HxPoint { x: stored[0].x, y: stored[0].y }),
        end_pt: Some(HxPoint { x: stored[1].x, y: stored[1].y }),
    })
}

/// Encodes a Core `Control::Ellipse` into `HxEllipse`.
pub(crate) fn encode_ellipse_to_hx(
    ctrl: &Control,
    as_group_child: bool,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut DocumentEncodeContext,
) -> HwpxResult<HxEllipse> {
    let (
        _center,
        _axis1,
        _axis2,
        width,
        height,
        placement,
        paragraphs,
        caption,
        style,
        text_vertical_align,
    ) = match ctrl {
        Control::Ellipse {
            frame: _,
            center,
            axis1,
            axis2,
            width,
            height,
            placement,
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
            placement,
            paragraphs,
            caption,
            style,
            *text_vertical_align,
        ),
        _ => unreachable!("encode_ellipse_to_hx called with non-Ellipse"),
    };

    let _w = width.as_i32();
    let _h = height.as_i32();
    let plan = shape_frame_plan(ctrl, as_group_child)?;
    let sc = build_shape_common(ctrl, &plan, as_group_child);
    report_shape_output(ctrl, as_group_child, &plan, sink);
    let (w, h) = sc.size;

    let draw_text = if paragraphs.is_empty() {
        None
    } else {
        sink.enter(PathSeg::TextBox);
        let sub_list_result = encode_paragraphs_to_sublist_with_align(
            paragraphs,
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
        axis_endpoints: false,
        id: generate_instid(),
        z_order: 0,
        numbering_type: shape_numbering_type(placement.as_ref()),
        text_wrap: shape_text_wrap(placement.as_ref()),
        text_flow: shape_text_flow(placement.as_ref()),
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
        fill_brush: sc.fill_brush,
        shadow: Some(sc.shadow),
        sz: Some(HxTableSz {
            width: w,
            width_rel_to: "ABSOLUTE".to_string(),
            height: h,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: size_protect(ctrl),
        }),
        pos: Some(shape_position(placement.as_ref())),
        out_margin: Some(object_out_margin(placement.as_ref())),
        shape_comment: Some(HxShapeComment { text: "타원입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, hyperlink_entries, options, sink))
            .transpose()?,
        draw_text,
        center: Some(HxPoint { x: plan.points()[0].x, y: plan.points()[0].y }),
        ax1: Some(HxPoint { x: plan.points()[1].x, y: plan.points()[1].y }),
        ax2: Some(HxPoint { x: plan.points()[2].x, y: plan.points()[2].y }),
        start1: Some(HxPoint { x: 0, y: 0 }),
        end1: Some(HxPoint { x: 0, y: 0 }),
        start2: Some(HxPoint { x: 0, y: 0 }),
        end2: Some(HxPoint { x: 0, y: 0 }),
    })
}

/// Encodes a Core `Control::Polygon` into `HxPolygon`.
pub(crate) fn encode_polygon_to_hx(
    ctrl: &Control,
    as_group_child: bool,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut DocumentEncodeContext,
) -> HwpxResult<HxPolygon> {
    let (_vertices, width, height, placement, paragraphs, caption, style, text_vertical_align) =
        match ctrl {
            Control::Polygon {
                frame: _,
                vertices,
                width,
                height,
                placement,
                paragraphs,
                caption,
                style,
                text_vertical_align,
            } => (
                vertices,
                *width,
                *height,
                placement,
                paragraphs,
                caption,
                style,
                *text_vertical_align,
            ),
            _ => unreachable!("encode_polygon_to_hx called with non-Polygon"),
        };

    let _w = width.as_i32();
    let _h = height.as_i32();
    let plan = shape_frame_plan(ctrl, as_group_child)?;
    let sc = build_shape_common(ctrl, &plan, as_group_child);
    report_shape_output(ctrl, as_group_child, &plan, sink);
    let (w, h) = sc.size;

    let draw_text = if paragraphs.is_empty() {
        None
    } else {
        sink.enter(PathSeg::TextBox);
        let sub_list_result = encode_paragraphs_to_sublist_with_align(
            paragraphs,
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

    let points = plan.points().iter().map(|v| HxPoint { x: v.x, y: v.y }).collect();

    Ok(HxPolygon {
        id: generate_instid(),
        z_order: 0,
        numbering_type: shape_numbering_type(placement.as_ref()),
        text_wrap: shape_text_wrap(placement.as_ref()),
        text_flow: shape_text_flow(placement.as_ref()),
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
        fill_brush: sc.fill_brush,
        shadow: Some(sc.shadow),
        sz: Some(HxTableSz {
            width: w,
            width_rel_to: "ABSOLUTE".to_string(),
            height: h,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: size_protect(ctrl),
        }),
        pos: Some(shape_position(placement.as_ref())),
        out_margin: Some(object_out_margin(placement.as_ref())),
        shape_comment: Some(HxShapeComment { text: "다각형입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, hyperlink_entries, options, sink))
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
    as_group_child: bool,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut DocumentEncodeContext,
) -> HwpxResult<HxEllipse> {
    let (
        arc_type,
        _center,
        _axis1,
        _axis2,
        _start1,
        _end1,
        _start2,
        _end2,
        width,
        height,
        placement,
        caption,
        style,
    ) = match ctrl {
        Control::Arc {
            definition: _,
            frame: _,
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
            placement,
            caption,
            style,
        } => (
            arc_type, center, axis1, axis2, start1, end1, start2, end2, *width, *height, placement,
            caption, style,
        ),
        _ => unreachable!("encode_arc_to_hx called with non-Arc"),
    };

    let _w = width.as_i32();
    let _h = height.as_i32();
    let plan = shape_frame_plan(ctrl, as_group_child)?;
    let sc = build_shape_common(ctrl, &plan, as_group_child);
    report_shape_output(ctrl, as_group_child, &plan, sink);
    let (w, h) = sc.size;
    let arc = plan.points();

    Ok(HxEllipse {
        axis_endpoints: matches!(
            ctrl,
            Control::Arc { definition: hwpforge_core::control::ArcDefinition::AxisEndpoints, .. }
        ),
        id: generate_instid(),
        z_order: 0,
        numbering_type: shape_numbering_type(placement.as_ref()),
        text_wrap: shape_text_wrap(placement.as_ref()),
        text_flow: shape_text_flow(placement.as_ref()),
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
        fill_brush: sc.fill_brush,
        shadow: Some(sc.shadow),
        sz: Some(HxTableSz {
            width: w,
            width_rel_to: "ABSOLUTE".to_string(),
            height: h,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: size_protect(ctrl),
        }),
        pos: Some(shape_position(placement.as_ref())),
        out_margin: Some(object_out_margin(placement.as_ref())),
        shape_comment: Some(HxShapeComment { text: "호입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, hyperlink_entries, options, sink))
            .transpose()?,
        draw_text: None,
        center: Some(HxPoint { x: arc[0].x, y: arc[0].y }),
        ax1: Some(HxPoint { x: arc[1].x, y: arc[1].y }),
        ax2: Some(HxPoint { x: arc[2].x, y: arc[2].y }),
        start1: arc.get(3).map(|p| HxPoint { x: p.x, y: p.y }),
        end1: arc.get(4).map(|p| HxPoint { x: p.x, y: p.y }),
        start2: arc.get(5).map(|p| HxPoint { x: p.x, y: p.y }),
        end2: arc.get(6).map(|p| HxPoint { x: p.x, y: p.y }),
    })
}

/// Serialize both arc definitions identically for standalone and group children.
pub(crate) fn serialize_arc(hx: &HxEllipse) -> HwpxResult<String> {
    if !hx.axis_endpoints {
        return serialize_with_root(hx, "hp:ellipse");
    }
    let xml = serialize_with_root(hx, "hp:arc")?;
    let (root, children) = xml.split_once('>').expect("serialized arc root");
    Ok(format!(
        "{}>{children}",
        root.replace(" arcType=", " type=")
            .replace(" intervalDirty=\"0\"", "")
            .replace(" hasArcPr=\"1\"", "")
    ))
}

/// Encodes a Core `Control::Curve` into `HxCurve`.
pub(crate) fn encode_curve_to_hx(
    ctrl: &Control,
    as_group_child: bool,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut DocumentEncodeContext,
) -> HwpxResult<HxCurve> {
    let (_points, segment_types, width, height, placement, caption, style) = match ctrl {
        Control::Curve {
            frame: _,
            points,
            segment_types,
            width,
            height,
            placement,
            caption,
            style,
        } => (points, segment_types, *width, *height, placement, caption, style),
        _ => unreachable!("encode_curve_to_hx called with non-Curve"),
    };

    let _w = width.as_i32();
    let _h = height.as_i32();
    let plan = shape_frame_plan(ctrl, as_group_child)?;
    let sc = build_shape_common(ctrl, &plan, as_group_child);
    report_shape_output(ctrl, as_group_child, &plan, sink);
    let (w, h) = sc.size;

    // Per KS X 6101 표 269: each <hp:seg> has x1/y1 (start) + x2/y2 (end).
    // Points array encodes control vertices; segments connect adjacent pairs.
    let points = plan.points();
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
        numbering_type: shape_numbering_type(placement.as_ref()),
        text_wrap: shape_text_wrap(placement.as_ref()),
        text_flow: shape_text_flow(placement.as_ref()),
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
        fill_brush: sc.fill_brush,
        shadow: Some(sc.shadow),
        sz: Some(HxTableSz {
            width: w,
            width_rel_to: "ABSOLUTE".to_string(),
            height: h,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: size_protect(ctrl),
        }),
        pos: Some(shape_position(placement.as_ref())),
        out_margin: Some(object_out_margin(placement.as_ref())),
        shape_comment: Some(HxShapeComment { text: "곡선입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, hyperlink_entries, options, sink))
            .transpose()?,
        points: vec![], // KS X 6101: coordinates are in <hp:seg> elements, not <hc:pt>
        segments,
    })
}

/// Encodes a Core `Control::ConnectLine` into `HxConnectLine`.
pub(crate) fn encode_connect_line_to_hx(
    ctrl: &Control,
    as_group_child: bool,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut DocumentEncodeContext,
) -> HwpxResult<HxConnectLine> {
    let (_start, _end, _control_points, connect_type, width, height, placement, caption, style) =
        match ctrl {
            Control::ConnectLine {
                frame: _,
                start,
                end,
                control_points,
                connect_type,
                width,
                height,
                placement,
                caption,
                style,
            } => (
                start,
                end,
                control_points,
                connect_type,
                *width,
                *height,
                placement,
                caption,
                style,
            ),
            _ => unreachable!("encode_connect_line_to_hx called with non-ConnectLine"),
        };

    let _w = width.as_i32();
    let _h = height.as_i32();
    let plan = shape_frame_plan(ctrl, as_group_child)?;
    let sc = build_shape_common(ctrl, &plan, as_group_child);
    report_shape_output(ctrl, as_group_child, &plan, sink);
    let (w, h) = sc.size;

    // Per golden fixture: controlPoints wrapper contains ALL points
    // (start type=3, intermediates type=2, end type=26).
    // The plan's points: start, end, then the control points.
    let stored = plan.points();
    let (start, end, control_points) = (&stored[0], &stored[1], &stored[2..]);
    let mut all_points = Vec::with_capacity(control_points.len() + 2);
    all_points.push(HxControlPoint { x: start.x, y: start.y, point_type: "3".to_string() });
    for p in control_points {
        all_points.push(HxControlPoint { x: p.x, y: p.y, point_type: "2".to_string() });
    }
    all_points.push(HxControlPoint { x: end.x, y: end.y, point_type: "26".to_string() });

    Ok(HxConnectLine {
        id: generate_instid(),
        z_order: 0,
        numbering_type: shape_numbering_type(placement.as_ref()),
        text_wrap: shape_text_wrap(placement.as_ref()),
        text_flow: shape_text_flow(placement.as_ref()),
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
            protect: size_protect(ctrl),
        }),
        pos: Some(shape_position(placement.as_ref())),
        out_margin: Some(object_out_margin(placement.as_ref())),
        shape_comment: Some(HxShapeComment { text: "연결선입니다.".to_string() }),
        caption: caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, hyperlink_entries, options, sink))
            .transpose()?,
    })
}

/// Serializes any serde value to an XML fragment with the given root element
/// name (e.g. `"hp:rect"`). Used by the recursive group emitter to turn each
/// `Hx*` shape and the container's shape-common sub-blocks into raw XML that
/// is concatenated in document (z-) order inside `<hp:container>`.
pub(super) fn serialize_with_root<T: serde::Serialize>(
    value: &T,
    root: &str,
) -> HwpxResult<String> {
    let mut buf = String::new();
    #[allow(clippy::disallowed_methods)] // normalized below
    let ser = quick_xml::se::Serializer::with_root(&mut buf, Some(root))
        .map_err(|e| crate::error::HwpxError::XmlSerialize { detail: e.to_string() })?;
    value
        .serialize(ser)
        .map_err(|e| crate::error::HwpxError::XmlSerialize { detail: e.to_string() })?;
    // Most fragments hold no control-character reference; keep `buf` then.
    match crate::wire_xml::normalize_attr_control_whitespace(&buf) {
        std::borrow::Cow::Owned(normalized) => Ok(normalized),
        std::borrow::Cow::Borrowed(_) => Ok(buf),
    }
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
///
/// A negative offset is written as its unsigned wrap, as Hancom writes it
/// (the `transMatrix` translation stays signed, as Hancom writes that one).
fn set_group_child_offset(xml: &str, x: i32, y: i32) -> String {
    let (x, y) = (x as u32, y as u32);
    xml.replacen(r#"<hp:offset x="0" y="0"/>"#, &format!(r#"<hp:offset x="{x}" y="{y}"/>"#), 1)
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
///
/// `as_group_child` = the TextArt sits in a group: it is placed by its
/// matrix there, so no `<hp:sz>`/`<hp:pos>` is written and its offset is left
/// at zero for the group encoder to fill in.
pub(crate) fn encode_text_art_to_xml(ctrl: &Control, as_group_child: bool) -> HwpxResult<String> {
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
            frame: _,
            text,
            shape,
            font_name,
            font_style,
            align,
            line_spacing,
            char_spacing,
            width,
            height,
            placement,
            fill_color,
            inst_id,
        } => {
            // TextArt anchors at `<hp:offset>`; its `<hp:pos>` is a fixed
            // floating template, so only the placement offsets are sourced.
            // A group child's offset is written by the group encoder, from
            // the same placement plus its parent's origin.
            let (hx, vy) = placement
                .as_ref()
                .filter(|_| !as_group_child)
                .map(|p| (p.horz_offset.as_i32(), p.vert_offset.as_i32()))
                .unwrap_or((0, 0));
            (
                text,
                shape,
                font_name,
                font_style,
                align,
                *line_spacing,
                *char_spacing,
                width.as_i32(),
                height.as_i32(),
                hx,
                vy,
                *fill_color,
                *inst_id,
            )
        }
        _ => unreachable!("encode_text_art_to_xml called with non-TextArt"),
    };

    let id = generate_instid();
    let instid = super::object_ids::core_or_drawn("textart", inst);
    let face_color = fill.map_or_else(|| "#0000FF".to_string(), |c| c.to_hex_rgb());
    // scaMatrix maps the original box (the 14173-HWPUNIT design box unless
    // the frame holds another) onto the rendered size.
    // 이것을 실패시키는 것: 14173 상수로 되돌리기(`textart_frame_round_trips`).
    let plan = shape_frame_plan(ctrl, as_group_child)?;
    let (org_w, org_h) = plan.original_size();
    let sca_x = format!("{:.6}", plan.scale().0);
    let sca_y = format!("{:.6}", plan.scale().1);
    let protect = size_protect(ctrl);
    let center_x = w / 2;
    let center_y = h / 2;
    let text_esc = escape_xml(&textart_text_to_wire(text));
    let shape_esc = escape_xml(shape);
    let font_name_esc = escape_xml(font_name);
    let font_style_esc = escape_xml(font_style);
    let align_esc = escape_xml(align);
    // Hancom writes a group child TextArt as `NONE` / `TOP_AND_BOTTOM`
    // ; a free one keeps the writer's `PICTURE` / `SQUARE`.
    let (numbering, wrap) =
        if as_group_child { ("NONE", "TOP_AND_BOTTOM") } else { ("PICTURE", "SQUARE") };
    let placement = if as_group_child {
        String::new()
    } else {
        format!(
            r#"{sz}<hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="1" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="COLUMN" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/>"#,
            sz = format_args!(
                r#"<hp:sz width="{w}" widthRelTo="ABSOLUTE" height="{h}" heightRelTo="ABSOLUTE" protect="{protect}"/>"#
            ),
        )
    };

    Ok(format!(
        r##"<hp:textart id="{id}" zOrder="0" numberingType="{numbering}" textWrap="{wrap}" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="0" instid="{instid}" text="{text}"><hp:offset x="{hx}" y="{vy}"/><hp:orgSz width="{org_w}" height="{org_h}"/><hp:curSz width="{cur_w}" height="{cur_h}"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="{center_x}" centerY="{center_y}" rotateimage="1"/><hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:scaMatrix e1="{sca_x}" e2="0" e3="0" e4="0" e5="{sca_y}" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo><hp:lineShape color="#000000" width="0" style="NONE" endCap="ROUND" headStyle="NORMAL" tailStyle="NORMAL" headfill="0" tailfill="0" headSz="SMALL_SMALL" tailSz="SMALL_SMALL" outlineStyle="INNER" alpha="0"/><hc:fillBrush><hc:winBrush faceColor="{face_color}" hatchColor="#000000" alpha="0"/></hc:fillBrush><hp:shadow type="NONE" color="#B2B2B2" offsetX="0" offsetY="0" alpha="0"/><hc:pt0 x="0" y="0"/><hc:pt1 x="{org_w}" y="0"/><hc:pt2 x="{org_w}" y="{org_h}"/><hc:pt3 x="0" y="{org_h}"/><hp:textartPr fontName="{font_name}" fontStyle="{font_style}" fontType="TTF" textShape="{shape}" lineSpacing="{line_spacing}" charSpacing="{char_spacing}" align="{align}"><hp:shadow type="NONE" color="#000000" offsetX="0" offsetY="0" alpha="0"/></hp:textartPr>{placement}<hp:outMargin left="56" right="56" top="0" bottom="0"/><hp:shapeComment>글맵시</hp:shapeComment></hp:textart>"##,
        id = id,
        instid = instid,
        text = text_esc,
        // Hancom writes a negative offset as its unsigned wrap.
        hx = hx as u32,
        vy = vy as u32,
        // Hancom writes a negative `<hp:curSz>` side as its unsigned wrap.
        cur_w = w as u32,
        cur_h = h as u32,
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
        placement = placement,
        numbering = numbering,
        wrap = wrap,
    ))
}

/// Encodes a container child's group-relative position into its
/// `<hc:transMatrix>` translation (`e3` = x, `e6` = y).
///
/// 한컴 positions a `<hp:container>` child by the translation components of
/// its transform matrix, NOT by `<hp:offset>` (verified against
/// a Hancom-saved group document: a child at offset (17360, 0) carries
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
        Control::TextBox { placement, .. }
        | Control::Rect { placement, .. }
        | Control::Ellipse { placement, .. }
        | Control::Arc { placement, .. }
        | Control::Polygon { placement, .. }
        | Control::Curve { placement, .. }
        | Control::ConnectLine { placement, .. }
        | Control::Line { placement, .. }
        | Control::TextArt { placement, .. }
        | Control::Group { placement, .. } => placement
            .as_ref()
            .map(|p| (p.horz_offset.as_i32(), p.vert_offset.as_i32()))
            .unwrap_or((0, 0)),
        _ => (0, 0),
    }
}

pub(crate) fn group_child_is_encodable(c: &Control) -> bool {
    matches!(
        c,
        Control::TextBox { .. }
            | Control::Rect { .. }
            | Control::Line { .. }
            | Control::Ellipse { .. }
            | Control::Arc { .. }
            | Control::Polygon { .. }
            | Control::Curve { .. }
            | Control::ConnectLine { .. }
            | Control::TextArt { .. }
            | Control::Group { .. }
    )
}

/// The name a group child is reported under.
fn group_child_kind(child: &GroupChild) -> &'static str {
    match child {
        GroupChild::Picture(_) => "picture",
        GroupChild::Shape(control) => control.kind_name(),
        _ => "unknown",
    }
}

/// Encodes one child shape control into its HWPX XML fragment, reusing the
/// existing per-shape encoders. Text-bearing children (`TextBox`,
/// `Ellipse`-with-paragraphs) carry their `<hp:drawText>` automatically via
/// those encoders. Returns `None` for controls that have no group-child
/// form (the caller reports them as
/// [`EncodeWarning::ContentLost`](super::EncodeWarning::ContentLost)).
///
/// `parent_origin` is where the parent group's box starts in the outermost
/// group's space. Hancom maps every shape of a nested group straight into
/// the outermost group's space, so a child's `transMatrix`/`<hp:offset>` is
/// `parent_origin` + the child's own offset (for a top-level group the
/// origin is `(0, 0)`).
fn encode_group_child_xml(
    child: &GroupChild,
    group_level: u32,
    parent_origin: (i32, i32),
    nodes: &mut std::slice::Iter<'_, GroupNodePlan>,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut DocumentEncodeContext,
) -> HwpxResult<Option<String>> {
    let child = match child {
        GroupChild::Shape(control) => control,
        GroupChild::Picture(image) => {
            let node = nodes.next();
            return encode_group_picture_xml(
                image,
                group_level,
                parent_origin,
                hyperlink_entries,
                options,
                sink,
            )
            .map(|xml| Some(with_node_chain(xml, node)));
        }
        _ => return Ok(None),
    };
    if !group_child_is_encodable(child) {
        return Ok(None);
    }
    let (dx, dy) = group_child_offset(child);
    let (x, y) = (parent_origin.0.saturating_add(dx), parent_origin.1.saturating_add(dy));
    // Nested group: recurse into a full `<hp:container>` fragment.
    // `encode_group_to_xml` bakes the correct `groupLevel` into the opening
    // tag directly, so we must NOT run `set_group_level` afterward (it patches
    // a `groupLevel="0"` placeholder that a container never has). The other
    // post-processors are nesting-safe — the container's own shape-common
    // block precedes its children, so first-match ops hit the container's own
    // `offset`/`transMatrix`/`curSz`, and the children their own `sz`/`pos` (a group child is placed by its
    // matrix, not anchored to the page).
    if let Control::Group { .. } = child {
        // The nested group's node precedes its children's (preorder).
        let node = nodes.next();
        let raw = encode_group_at(
            child,
            group_level,
            (x, y),
            true,
            nodes,
            hyperlink_entries,
            options,
            sink,
        )?;
        let raw = set_group_child_offset(&raw, x, y);
        let raw = set_group_child_translate(&raw, x, y);
        let raw = zero_cur_sz(&raw);
        return Ok(Some(with_node_chain(raw, node)));
    }
    let baseline = nodes.next();
    // 이것을 실패시키는 것: fallback 뒤 그림의 30.4° 손실을 무경고로 방출.
    let preserved =
        baseline.map(|node| preserve_group_child(child, node, group_level as usize, &HWPX_POLICY));
    let node = preserved.as_ref().map(|(node, _)| node);
    let folded;
    let child = if let Some((_, Some(loss))) = &preserved {
        sink.content_lost("shape_emission", loss.detail().to_string());
        let mut plain = child.clone();
        drop_original_size(&mut plain);
        super::section::traversal::rebind_group_notes(child, &plain, sink)?;
        folded = plain;
        &folded
    } else {
        child
    };
    let raw = match child {
        Control::TextBox { .. } => {
            let mut hx = encode_textbox_to_rect(child, true, hyperlink_entries, options, sink)?;
            (hx.sz, hx.pos) = (None, None);
            serialize_with_root(&hx, "hp:rect")?
        }
        Control::Rect { .. } => {
            let mut hx = encode_rect_to_hx(child, true, hyperlink_entries, options, sink)?;
            (hx.sz, hx.pos) = (None, None);
            serialize_with_root(&hx, "hp:rect")?
        }
        Control::Line { .. } => {
            let mut hx = encode_line_to_hx(child, true, hyperlink_entries, options, sink)?;
            (hx.sz, hx.pos) = (None, None);
            serialize_with_root(&hx, "hp:line")?
        }
        Control::Ellipse { .. } => {
            let mut hx = encode_ellipse_to_hx(child, true, hyperlink_entries, options, sink)?;
            (hx.sz, hx.pos) = (None, None);
            serialize_with_root(&hx, "hp:ellipse")?
        }
        Control::Arc { .. } => {
            let mut hx = encode_arc_to_hx(child, true, hyperlink_entries, options, sink)?;
            (hx.sz, hx.pos) = (None, None);
            serialize_arc(&hx)?
        }
        Control::Polygon { .. } => {
            let mut hx = encode_polygon_to_hx(child, true, hyperlink_entries, options, sink)?;
            (hx.sz, hx.pos) = (None, None);
            serialize_with_root(&hx, "hp:polygon")?
        }
        Control::Curve { .. } => {
            let mut hx = encode_curve_to_hx(child, true, hyperlink_entries, options, sink)?;
            (hx.sz, hx.pos) = (None, None);
            serialize_with_root(&hx, "hp:curve")?
        }
        Control::ConnectLine { .. } => {
            let mut hx = encode_connect_line_to_hx(child, true, hyperlink_entries, options, sink)?;
            (hx.sz, hx.pos) = (None, None);
            serialize_with_root(&hx, "hp:connectLine")?
        }
        Control::TextArt { .. } => encode_text_art_to_xml(child, true)?,
        // Anything else (Equation, EmbeddedChart, …) has no container-child
        // form — Hancom will not group an equation — so it is left out
        // rather than fabricated; `encode_group_at` reports it as lost. (The
        // group plan has no node for it either.)
        _ => return Ok(None),
    };
    // 한컴 positions a container child by its transform-matrix translation;
    // `<hp:offset>` mirrors it (native carries both). Identity matrices render
    // every child at the group origin (overlap) — verified visually.
    let raw = set_group_child_offset(&raw, x, y);
    let raw = set_group_child_translate(&raw, x, y);
    let raw = zero_cur_sz(&raw);
    let raw = match node {
        Some(node)
            if matches!(
                shape_parts(child).2.and_then(|s| s.fill.as_ref()),
                Some(Fill::Image { .. })
            ) =>
        {
            with_image_center(raw, node)
        }
        _ => raw,
    };
    Ok(Some(with_node_chain(set_group_level(&raw, group_level), node)))
}

/// Image metadata describes the integer displayed box obtained by decoding
/// the quantized chain, including ancestor scales. The stored corner distances
/// are already computed by Core's admission; no second loss calculation lives here.
fn with_image_center(mut xml: String, node: &GroupNodePlan) -> String {
    let corners = node.stored_positions();
    let size =
        |i: usize| (corners[i].0 - corners[0].0).hypot(corners[i].1 - corners[0].1).round() as i32;
    // 이것을 실패시키는 것: 982→984 양자화 뒤에도 centerX=491을 써 재통과 때 492로 바뀌기.
    for (key, value) in [("centerX", size(1) / 2), ("centerY", size(3) / 2)] {
        let prefix = format!("{key}=\"");
        if let Some(start) = xml.find(&prefix) {
            let start = start + prefix.len();
            if let Some(end) = xml[start..].find('"') {
                xml.replace_range(start..start + end, &value.to_string());
            }
        }
    }
    xml
}

/// A coefficient as the plan stores it: `0` and `1` as Hancom writes them,
/// any other value with six decimals.
fn coefficient(v: f64) -> String {
    if v == 0.0 {
        "0".to_string()
    } else if v == 1.0 {
        "1".to_string()
    } else {
        format!("{v:.6}")
    }
}

/// `xml` with its own (first) `<hp:renderingInfo>` replaced by the group
/// plan's chain for this node — the translation, then every enclosing
/// group's pair and its own — when an enclosing group stores a
/// scale or an image-filled child needs its identity ancestor pairs. With
/// one pair the per-shape writer's block already is the plan.
/// 이것을 실패시키는 것: 이 교체를 지우기(묶음 배율이 있는 잎이 자기 쌍 하나만
/// 써서 두 번 축소된다 — `parent_and_child_scales_land_where_hancom_draws`).
fn with_node_chain(xml: String, node: Option<&GroupNodePlan>) -> String {
    let Some(node) = node.filter(|n| n.pairs().len() > 1) else { return xml };
    let (Some(start), Some(end)) =
        (xml.find("<hp:renderingInfo>"), xml.find("</hp:renderingInfo>"))
    else {
        return xml;
    };
    let m = |tag: &str, c: &[f64; 6]| {
        format!(
            r#"<hc:{tag} e1="{}" e2="{}" e3="{}" e4="{}" e5="{}" e6="{}"/>"#,
            coefficient(c[0]),
            coefficient(c[1]),
            coefficient(c[2]),
            coefficient(c[3]),
            coefficient(c[4]),
            coefficient(c[5])
        )
    };
    let (tx, ty) = node.translation();
    let mut chain = format!(
        r#"<hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="{tx}" e4="0" e5="1" e6="{ty}"/>"#
    );
    for (sca, rot) in node.pairs() {
        chain.push_str(&m("scaMatrix", sca));
        chain.push_str(&m("rotMatrix", rot));
    }
    chain.push_str("</hp:renderingInfo>");
    let close = end + "</hp:renderingInfo>".len();
    format!("{}{chain}{}", &xml[..start], &xml[close..])
}

/// Encodes a group picture as Hancom writes one: a `<hp:pic>` placed by its
/// matrix (no `<hp:sz>`/`<hp:pos>`/`<hp:outMargin>`), `numberingType`
/// NONE (every group picture in the corpus) and `textWrap` TOP_AND_BOTTOM
/// (2,091 of the 2,162; the rest SQUARE), its offset written into
/// `<hp:offset>` and the `transMatrix` translation like a shape child's.
fn encode_group_picture_xml(
    image: &hwpforge_core::image::Image,
    group_level: u32,
    parent_origin: (i32, i32),
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut DocumentEncodeContext,
) -> HwpxResult<String> {
    let (dx, dy) = image
        .placement
        .as_ref()
        .map_or((0, 0), |p| (p.horz_offset.as_i32(), p.vert_offset.as_i32()));
    let (x, y) = (parent_origin.0.saturating_add(dx), parent_origin.1.saturating_add(dy));
    let mut hx = super::section::build_picture(image, true, hyperlink_entries, options, sink)?;
    (hx.sz, hx.pos, hx.out_margin) = (None, None, None);
    hx.numbering_type = "NONE".to_string();
    hx.text_wrap = "TOP_AND_BOTTOM".to_string();
    let raw = serialize_with_root(&hx, "hp:pic")?;
    let raw = set_group_child_offset(&raw, x, y);
    let raw = set_group_child_translate(&raw, x, y);
    let raw = zero_cur_sz(&raw);
    Ok(set_group_level(&raw, group_level))
}

/// Encodes a Core `Control::Group` (묶음 객체) into a complete `<hp:container>`
/// XML fragment (KS X 6101 §10.9.8).
///
/// Layout mirrors the native fixture: container attributes, the shape-common
/// block (`offset`/`orgSz`/`curSz`/`flip`/`rotationInfo`/`renderingInfo`),
/// the child shapes in z-order (each with `groupLevel` = parent + 1), then
/// `sz`/`pos`/`outMargin`/`shapeComment`. Children carry absolute geometry —
/// no rescaling (geometry stays in the `hc:` namespace, which the
/// per-shape encoders already honor).
pub(crate) fn encode_group_to_xml(
    ctrl: &Control,
    group_level: u32,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut DocumentEncodeContext,
) -> HwpxResult<String> {
    // group_plan uses Core's folded image decision input, including for JSON Core.
    // Image admission below uses the same Core function as both decoders.
    // 이것을 실패시키는 것: 잎 하나만 fallback 하기 / fallback 뒤 재측정을 지우기
    // (`sibling_fallback_is_predicted`).
    let plan = group_plan(ctrl, &HWPX_POLICY).ok_or_else(|| {
        crate::error::HwpxError::InvalidStructure { detail: "group has no plan".to_string() }
    })?;
    if let Some(loss) = plan.loss() {
        sink.content_lost("shape_emission", loss.detail().to_string());
    }
    let stripped;
    let target = if plan.is_fallback() {
        let mut plain = ctrl.clone();
        drop_original_sizes(&mut plain);
        super::section::traversal::rebind_group_notes(ctrl, &plain, sink)?;
        stripped = plain;
        &stripped
    } else {
        ctrl
    };
    encode_group_at(
        target,
        group_level,
        (0, 0),
        false,
        &mut plan.nodes().iter(),
        hyperlink_entries,
        options,
        sink,
    )
}

/// [`encode_group_to_xml`] for a group whose box starts at `origin` in the
/// outermost group's space (`(0, 0)` for the outermost group itself).
///
/// `as_child` = the group is itself a group child: it is placed by its
/// matrix inside its parent, so it writes no `<hp:sz>`/`<hp:pos>` of its own
/// (native nested containers have neither).
#[allow(clippy::too_many_arguments)]
fn encode_group_at(
    ctrl: &Control,
    group_level: u32,
    origin: (i32, i32),
    as_child: bool,
    nodes: &mut std::slice::Iter<'_, GroupNodePlan>,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut DocumentEncodeContext,
) -> HwpxResult<String> {
    let (children, width, height, placement, inst_id) = match ctrl {
        Control::Group { frame: _, children, width, height, placement, inst_id } => {
            (children, width.as_i32(), height.as_i32(), placement.as_ref(), *inst_id)
        }
        _ => unreachable!("encode_group_to_xml called with non-Group"),
    };
    // Diagnostic slot before the children (document order).
    let id_slot = super::object_ids::reserve();

    let plan = shape_frame_plan(ctrl, as_child)?;
    let sc = build_shape_common(ctrl, &plan, true);

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
    // advances for children that actually produce XML — a dropped control
    // (Equation/EmbeddedChart/… — see `encode_group_child_xml`) is reported
    // at the index the next written child takes, so every warning's path
    // indexes the written sequence, not the source child list.
    let mut children_xml = String::new();
    let mut emitted_idx = 0usize;
    for (source_idx, child) in children.iter().enumerate() {
        sink.enter(PathSeg::GroupChild(emitted_idx));
        let child_result = encode_group_child_xml(
            child,
            group_level + 1,
            origin,
            nodes,
            hyperlink_entries,
            options,
            sink,
        );
        if let Ok(None) = child_result {
            // Reported where it would have stood in the written group; the
            // reason names its place among the Core children.
            sink.content_lost(
                group_child_kind(child),
                format!(
                    "group child {source_idx} ({}) has no HWPX group form and is left out",
                    group_child_kind(child)
                ),
            );
        }
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
            protect: size_protect(ctrl),
        },
        "hp:sz",
    )?;
    let pos = serialize_with_root(&shape_position(placement), "hp:pos")?;
    let (sz, pos) = if as_child { (String::new(), String::new()) } else { (sz, pos) };
    // A group child is placed by its parent; only the outer group carries a
    // margin.
    let out_margin = serialize_with_root(
        &object_out_margin(if as_child { None } else { placement }),
        "hp:outMargin",
    )?;
    let shape_comment = serialize_with_root(
        &HxShapeComment { text: "묶음 개체입니다.".to_string() },
        "hp:shapeComment",
    )?;

    let id = generate_instid();
    let instid = super::object_ids::core_or_drawn_at(id_slot, "group", inst_id);
    Ok(format!(
        r#"<hp:container id="{id}" zOrder="0" numberingType="{numbering}" textWrap="{wrap}" textFlow="{flow}" lock="0" dropcapstyle="None" href="" groupLevel="{group_level}" instid="{instid}">{common}{children_xml}{sz}{pos}{out_margin}{shape_comment}</hp:container>"#,
        numbering = shape_numbering_type(placement),
        wrap = shape_text_wrap(placement),
        flow = shape_text_flow(placement),
    ))
}

/// The `<hp:outMargin>` of a drawn object or picture: its placement's outer
/// margin, 0 on every side without one — what every object wrote
/// before.
/// 이것을 실패시키는 것: 개체 종류 하나라도 0 상수로 되돌리기
/// (`object_out_margins_round_trip`).
pub(crate) fn object_out_margin(placement: Option<&ObjectPlacement>) -> HxTableMargin {
    placement.and_then(|p| p.out_margin).map_or(
        HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 },
        |m| HxTableMargin {
            left: m.left.as_i32(),
            right: m.right.as_i32(),
            top: m.top.as_i32(),
            bottom: m.bottom.as_i32(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_fill_shape_writes_no_fill_brush() {
        use hwpforge_core::control::Fill;
        let style = ShapeStyle { fill: Some(Fill::None), ..Default::default() };
        assert!(common(100, 50, Some(&style)).fill_brush.is_none());
        // 지정 없음은 종전대로 흰 채우기.
        let default = common(100, 50, None).fill_brush.expect("default fill");
        assert_eq!(default.win_brush.expect("winBrush").face_color, "#FFFFFF");
    }
    use hwpforge_core::control::{ArrowStyle, Control, Fill, LineStyle, ShapePoint, ShapeStyle};
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::placement::{ObjectRelativeTo, ObjectTextWrap};
    use hwpforge_core::run::Run;
    use hwpforge_foundation::{
        ArcType, ArrowSize, ArrowType, Color, CurveSegmentType, DropCapStyle, Flip, HwpUnit,
        PatternType, VerticalAlign,
    };
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

    fn empty_hyperlinks() -> Vec<(String, String)> {
        vec![]
    }

    /// Floating placement matching what the encoder historically derived from a
    /// non-zero shape offset: anchored to PAPER, IN_FRONT_OF_TEXT, overlap-allowed.
    fn floating_placement(horz: i32, vert: i32) -> ObjectPlacement {
        ObjectPlacement::new(
            ObjectTextWrap::InFrontOfText,
            ObjectRelativeTo::Paper,
            ObjectRelativeTo::Paper,
        )
        .with_allow_overlap(true)
        .with_vert_offset(HwpUnit::new(vert).unwrap())
        .with_horz_offset(HwpUnit::new(horz).unwrap())
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
        // 한글 only recognises EMPTY_* form for geometric shapes
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

    /// The shape-common block of a `w × h` rect with `style`.
    fn common(w: i32, h: i32, style: Option<&ShapeStyle>) -> ShapeCommon {
        let hu = |v: i32| hwpforge_foundation::HwpUnit::new(v).unwrap();
        let mut ctrl = Control::rect(hu(w), hu(h)).unwrap();
        if let Control::Rect { style: s, .. } = &mut ctrl {
            *s = style.cloned();
        }
        build_shape_common(&ctrl, &shape_frame_plan(&ctrl, false).unwrap(), false)
    }

    #[test]
    fn build_shape_common_default_style_gives_identity_rotation() {
        let sc = common(1000, 500, None);
        assert_eq!(sc.rotation_info.angle, 0);
        assert_eq!(sc.flip.horizontal, 0);
        assert_eq!(sc.flip.vertical, 0);
    }

    #[test]
    fn build_shape_common_org_sz_matches_dimensions() {
        let sc = common(8000, 4000, None);
        assert_eq!(sc.org_sz.width, 8000);
        assert_eq!(sc.org_sz.height, 4000);
    }

    #[test]
    fn build_shape_common_cur_sz_matches_dimensions() {
        let sc = common(8000, 4000, None);
        assert_eq!(sc.cur_sz.width, 8000);
        assert_eq!(sc.cur_sz.height, 4000);
    }

    #[test]
    fn build_shape_common_offset_is_zero() {
        let sc = common(1000, 500, None);
        assert_eq!(sc.offset.x, 0);
        assert_eq!(sc.offset.y, 0);
    }

    #[test]
    fn group_encodes_container_with_positioned_children() {
        use hwpforge_foundation::HwpUnit;
        let hu = |v: i32| HwpUnit::new(v).unwrap();
        // A group with two rects at distinct offsets (0,1365) + (17360,0),
        // mirroring the placement in a Hancom-saved group document.
        let child = |w, h, hx, vy| Control::Rect {
            frame: hwpforge_core::ObjectFrame::new(),
            width: hu(w),
            height: hu(h),
            placement: Some(
                ObjectPlacement::legacy_inline_defaults()
                    .with_horz_offset(hu(hx))
                    .with_vert_offset(hu(vy)),
            ),
            caption: None,
            style: None,
        };
        let group = Control::Group {
            frame: hwpforge_core::ObjectFrame::new(),
            children: vec![
                hwpforge_core::control::GroupChild::Shape(child(14_922, 7780, 0, 1365)),
                hwpforge_core::control::GroupChild::Shape(child(6998, 12_426, 17_360, 0)),
            ],
            width: hu(24_358),
            height: hu(12_426),
            placement: None,
            inst_id: None,
        };
        let mut entries = Vec::new();
        let xml = encode_group_to_xml(
            &group,
            0,
            &mut entries,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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
        assert_eq!(
            normalize_ids(&xml),
            r##"<hp:container id="#" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="0" instid="#"><hp:offset x="0" y="0"/><hp:orgSz width="24358" height="12426"/><hp:curSz width="24358" height="12426"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="12179" centerY="6213" rotateimage="1"/><hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo><hp:rect id="#" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="1" instid="#" ratio="0"><hp:offset x="0" y="1365"/><hp:orgSz width="14922" height="7780"/><hp:curSz width="0" height="0"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="7461" centerY="3890" rotateimage="1"/><hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="1365"/><hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo><hp:lineShape color="#000000" width="33" style="SOLID" endCap="FLAT" headStyle="NORMAL" tailStyle="NORMAL" headfill="1" tailfill="1" headSz="MEDIUM_MEDIUM" tailSz="MEDIUM_MEDIUM" outlineStyle="NORMAL" alpha="0"/><hc:fillBrush><hc:winBrush faceColor="#FFFFFF" hatchColor="#000000" alpha="0"/></hc:fillBrush><hp:shadow type="NONE" color="#B2B2B2" offsetX="0" offsetY="0" alpha="0"/><hc:pt0 x="0" y="0"/><hc:pt1 x="14922" y="0"/><hc:pt2 x="14922" y="7780"/><hc:pt3 x="0" y="7780"/><hp:outMargin left="0" right="0" top="0" bottom="0"/><hp:shapeComment>사각형입니다.</hp:shapeComment></hp:rect><hp:rect id="#" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="1" instid="#" ratio="0"><hp:offset x="17360" y="0"/><hp:orgSz width="6998" height="12426"/><hp:curSz width="0" height="0"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="3499" centerY="6213" rotateimage="1"/><hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="17360" e4="0" e5="1" e6="0"/><hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo><hp:lineShape color="#000000" width="33" style="SOLID" endCap="FLAT" headStyle="NORMAL" tailStyle="NORMAL" headfill="1" tailfill="1" headSz="MEDIUM_MEDIUM" tailSz="MEDIUM_MEDIUM" outlineStyle="NORMAL" alpha="0"/><hc:fillBrush><hc:winBrush faceColor="#FFFFFF" hatchColor="#000000" alpha="0"/></hc:fillBrush><hp:shadow type="NONE" color="#B2B2B2" offsetX="0" offsetY="0" alpha="0"/><hc:pt0 x="0" y="0"/><hc:pt1 x="6998" y="0"/><hc:pt2 x="6998" y="12426"/><hc:pt3 x="0" y="12426"/><hp:outMargin left="0" right="0" top="0" bottom="0"/><hp:shapeComment>사각형입니다.</hp:shapeComment></hp:rect><hp:sz width="24358" widthRelTo="ABSOLUTE" height="12426" heightRelTo="ABSOLUTE" protect="0"/><hp:pos treatAsChar="1" affectLSpacing="0" flowWithText="0" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="PARA" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/><hp:outMargin left="0" right="0" top="0" bottom="0"/><hp:shapeComment>묶음 개체입니다.</hp:shapeComment></hp:container>"##
        );
    }

    #[test]
    fn text_art_encodes_native_textart_element() {
        use hwpforge_foundation::HwpUnit;
        let hu = |v: i32| HwpUnit::new(v).unwrap();
        let ta = Control::TextArt {
            frame: hwpforge_core::ObjectFrame::new(),
            text: "글맵시".to_string(),
            shape: "WAVE2".to_string(),
            font_name: "함초롬바탕".to_string(),
            font_style: "보통".to_string(),
            align: "LEFT".to_string(),
            line_spacing: 120,
            char_spacing: 100,
            width: hu(6500),
            height: hu(5000),
            placement: None,
            fill_color: None,
            inst_id: Some(hwpforge_core::ObjectId::new(40_257_166)),
        };
        let xml = encode_text_art_to_xml(&ta, false).unwrap();
        assert!(xml.contains("<hp:textart "), "missing textart open: {xml}");
        // 이것을 실패시키는 것: 본문 글맵시도 묶음 자식처럼 `NONE`/`TOP_AND_BOTTOM` 으로 쓰는 것.
        assert!(xml.contains(r#"numberingType="PICTURE" textWrap="SQUARE""#), "{xml}");
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

    /// Encodes a TextArt carrying `text` and returns its `text` attribute.
    fn text_art_wire_text(text: &str) -> String {
        use hwpforge_foundation::HwpUnit;
        let ta = Control::TextArt {
            frame: hwpforge_core::ObjectFrame::new(),
            text: text.to_string(),
            shape: "WAVE2".to_string(),
            font_name: "함초롬바탕".to_string(),
            font_style: "보통".to_string(),
            align: "LEFT".to_string(),
            line_spacing: 120,
            char_spacing: 100,
            width: HwpUnit::new(6500).unwrap(),
            height: HwpUnit::new(5000).unwrap(),
            placement: None,
            fill_color: None,
            inst_id: None,
        };
        let xml = encode_text_art_to_xml(&ta, false).unwrap();
        let value = xml.split(" text=\"").nth(1).expect("text attribute");
        value[..value.find('"').expect("text attribute closes")].to_string()
    }

    /// Issue #199: Hancom writes a TextArt line break as `␍␊`; a raw CR/LF in
    /// the attribute is read back as a space.
    /// 이것을 실패시키는 것: `\r\n` 을 쌍으로 묶지 않고 글자마다 바꿈(`␍␊␍␊`), 또는 그대로 씀.
    #[test]
    fn text_art_crlf_encodes_as_one_hancom_pair() {
        assert_eq!(text_art_wire_text("첫 줄\r\n둘째 줄"), "첫 줄␍␊둘째 줄");
    }

    /// 이것을 실패시키는 것: 빈 줄(`\r\n\r\n`)을 한 쌍으로 접음.
    #[test]
    fn text_art_blank_line_keeps_two_pairs() {
        assert_eq!(text_art_wire_text("위\r\n\r\n아래"), "위␍␊␍␊아래");
    }

    /// 이것을 실패시키는 것: 단독 `\n` 을 빠뜨림 — JSON 입력의 흔한 줄바꿈.
    #[test]
    fn text_art_lone_lf_encodes_as_one_pair() {
        assert_eq!(text_art_wire_text("a\nb"), "a␍␊b");
    }

    /// 이것을 실패시키는 것: 단독 `\r` 을 빠뜨림.
    #[test]
    fn text_art_lone_cr_encodes_as_one_pair() {
        assert_eq!(text_art_wire_text("a\rb"), "a␍␊b");
    }

    /// `\r\r\n` = 단독 CR 하나 + CRLF 하나 = 줄바꿈 둘.
    /// 이것을 실패시키는 것: CR 뒤 LF 를 미리 보지 않고 CR 을 바로 쌍으로 바꿈(`␍␊␍␊␍␊`).
    #[test]
    fn text_art_cr_then_crlf_is_two_breaks() {
        assert_eq!(text_art_wire_text("a\r\r\nb"), "a␍␊␍␊b");
    }

    #[test]
    fn group_encodes_nested_container_recursively() {
        use hwpforge_foundation::HwpUnit;
        let hu = |v: i32| HwpUnit::new(v).unwrap();
        // outer group = { inner group { rect, ellipse }, line }, mirroring the
        // Hancom-saved nested group layout: a $con nested inside a $con.
        let rect = Control::Rect {
            frame: hwpforge_core::ObjectFrame::new(),
            width: hu(12_440),
            height: hu(6000),
            placement: None,
            caption: None,
            style: None,
        };
        let ellipse = Control::Ellipse {
            frame: hwpforge_core::ObjectFrame::new(),
            center: ShapePoint::new(5116, 3000),
            axis1: ShapePoint::new(10_232, 3000),
            axis2: ShapePoint::new(5116, 6000),
            width: hu(10_232),
            height: hu(6000),
            placement: Some(
                ObjectPlacement::legacy_inline_defaults()
                    .with_horz_offset(hu(1164))
                    .with_vert_offset(hu(6512)),
            ),
            paragraphs: Vec::new(),
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let inner = Control::Group {
            frame: hwpforge_core::ObjectFrame::new(),
            children: vec![
                hwpforge_core::control::GroupChild::Shape(rect),
                hwpforge_core::control::GroupChild::Shape(ellipse),
            ],
            width: hu(13_604),
            height: hu(12_512),
            placement: None,
            inst_id: None,
        };
        let line = Control::Line {
            frame: hwpforge_core::ObjectFrame::new(),
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(20_000, 0),
            width: hu(20_000),
            height: hu(0),
            placement: Some(
                ObjectPlacement::legacy_inline_defaults()
                    .with_horz_offset(hu(525))
                    .with_vert_offset(hu(13_422)),
            ),
            caption: None,
            style: None,
        };
        let outer = Control::Group {
            frame: hwpforge_core::ObjectFrame::new(),
            children: vec![
                hwpforge_core::control::GroupChild::Shape(inner),
                hwpforge_core::control::GroupChild::Shape(line),
            ],
            width: hu(42_520),
            height: hu(13_422),
            placement: None,
            inst_id: None,
        };
        let mut entries = Vec::new();
        let xml = encode_group_to_xml(
            &outer,
            0,
            &mut entries,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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
        assert_eq!(
            normalize_ids(&xml),
            r##"<hp:container id="#" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="0" instid="#"><hp:offset x="0" y="0"/><hp:orgSz width="42520" height="13422"/><hp:curSz width="42520" height="13422"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="21260" centerY="6711" rotateimage="1"/><hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo><hp:container id="#" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="1" instid="#"><hp:offset x="0" y="0"/><hp:orgSz width="13604" height="12512"/><hp:curSz width="0" height="0"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="6802" centerY="6256" rotateimage="1"/><hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo><hp:rect id="#" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="2" instid="#" ratio="0"><hp:offset x="0" y="0"/><hp:orgSz width="12440" height="6000"/><hp:curSz width="0" height="0"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="6220" centerY="3000" rotateimage="1"/><hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo><hp:lineShape color="#000000" width="33" style="SOLID" endCap="FLAT" headStyle="NORMAL" tailStyle="NORMAL" headfill="1" tailfill="1" headSz="MEDIUM_MEDIUM" tailSz="MEDIUM_MEDIUM" outlineStyle="NORMAL" alpha="0"/><hc:fillBrush><hc:winBrush faceColor="#FFFFFF" hatchColor="#000000" alpha="0"/></hc:fillBrush><hp:shadow type="NONE" color="#B2B2B2" offsetX="0" offsetY="0" alpha="0"/><hc:pt0 x="0" y="0"/><hc:pt1 x="12440" y="0"/><hc:pt2 x="12440" y="6000"/><hc:pt3 x="0" y="6000"/><hp:outMargin left="0" right="0" top="0" bottom="0"/><hp:shapeComment>사각형입니다.</hp:shapeComment></hp:rect><hp:ellipse id="#" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="2" instid="#" intervalDirty="0" hasArcPr="0" arcType="NORMAL"><hp:offset x="1164" y="6512"/><hp:orgSz width="10232" height="6000"/><hp:curSz width="0" height="0"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="5116" centerY="3000" rotateimage="1"/><hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="1164" e4="0" e5="1" e6="6512"/><hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo><hp:lineShape color="#000000" width="33" style="SOLID" endCap="FLAT" headStyle="NORMAL" tailStyle="NORMAL" headfill="1" tailfill="1" headSz="MEDIUM_MEDIUM" tailSz="MEDIUM_MEDIUM" outlineStyle="NORMAL" alpha="0"/><hc:fillBrush><hc:winBrush faceColor="#FFFFFF" hatchColor="#000000" alpha="0"/></hc:fillBrush><hp:shadow type="NONE" color="#B2B2B2" offsetX="0" offsetY="0" alpha="0"/><hc:center x="5116" y="3000"/><hc:ax1 x="10232" y="3000"/><hc:ax2 x="5116" y="6000"/><hc:start1 x="0" y="0"/><hc:end1 x="0" y="0"/><hc:start2 x="0" y="0"/><hc:end2 x="0" y="0"/><hp:outMargin left="0" right="0" top="0" bottom="0"/><hp:shapeComment>타원입니다.</hp:shapeComment></hp:ellipse><hp:outMargin left="0" right="0" top="0" bottom="0"/><hp:shapeComment>묶음 개체입니다.</hp:shapeComment></hp:container><hp:line id="#" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="1" instid="#" isReverseHV="0"><hp:offset x="525" y="13422"/><hp:orgSz width="20000" height="0"/><hp:curSz width="0" height="0"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="10000" centerY="0" rotateimage="1"/><hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="525" e4="0" e5="1" e6="13422"/><hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo><hp:lineShape color="#000000" width="33" style="SOLID" endCap="FLAT" headStyle="NORMAL" tailStyle="NORMAL" headfill="1" tailfill="1" headSz="MEDIUM_MEDIUM" tailSz="MEDIUM_MEDIUM" outlineStyle="NORMAL" alpha="0"/><hp:shadow type="NONE" color="#B2B2B2" offsetX="0" offsetY="0" alpha="0"/><hc:startPt x="0" y="0"/><hc:endPt x="20000" y="0"/><hp:outMargin left="0" right="0" top="0" bottom="0"/><hp:shapeComment>선입니다.</hp:shapeComment></hp:line><hp:sz width="42520" widthRelTo="ABSOLUTE" height="13422" heightRelTo="ABSOLUTE" protect="0"/><hp:pos treatAsChar="1" affectLSpacing="0" flowWithText="0" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="PARA" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/><hp:outMargin left="0" right="0" top="0" bottom="0"/><hp:shapeComment>묶음 개체입니다.</hp:shapeComment></hp:container>"##
        );
    }

    /// Element paths (`hp:container/hp:rect/hp:sz`, …) of every `<hp:sz>` and
    /// `<hp:pos>` in `xml`, read with an XML reader so depth is exact.
    fn placement_paths(xml: &str) -> Vec<String> {
        use quick_xml::events::Event;
        let mut reader = quick_xml::Reader::from_str(xml);
        let mut stack: Vec<String> = Vec::new();
        let mut out = Vec::new();
        loop {
            match reader.read_event().unwrap() {
                Event::Start(e) => {
                    stack.push(e.name().as_ref().to_owned());
                }
                Event::Empty(e) => {
                    let name = e.name().as_ref().to_owned();
                    if name == "hp:sz" || name == "hp:pos" {
                        out.push(format!("{}/{name}", stack.join("/")));
                    }
                }
                Event::End(_) => {
                    stack.pop();
                }
                Event::Eof => break,
                _ => {}
            }
        }
        out
    }

    fn table_of_width(width: i32) -> hwpforge_core::table::Table {
        use hwpforge_core::table::{Table, TableCell, TableRow};
        let cell = |t: &str| {
            TableCell::new(
                vec![Paragraph::with_runs(
                    vec![Run::text(t, CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                HwpUnit::new(width / 2).unwrap(),
            )
        };
        let mut table = Table::new(vec![TableRow::new(vec![cell("A"), cell("B")])]);
        table.width = Some(HwpUnit::new(width).unwrap());
        table
    }

    fn para_of(runs: Vec<Run>) -> Paragraph {
        Paragraph::with_runs(runs, ParaShapeIndex::new(0))
    }

    fn group_of(children: Vec<Control>) -> Control {
        Control::Group {
            frame: hwpforge_core::ObjectFrame::new(),
            children: children.into_iter().map(GroupChild::Shape).collect(),
            width: HwpUnit::new(45_000).unwrap(),
            height: HwpUnit::new(56_000).unwrap(),
            placement: None,
            inst_id: None,
        }
    }

    /// A line with an original box, its two ends, displayed size and turn.
    fn framed_line(
        end: (i32, i32),
        size: (i32, i32),
        original: (i32, i32),
        rotation: Option<f32>,
    ) -> Control {
        let hu = |v| HwpUnit::new(v).unwrap();
        Control::Line {
            start: hwpforge_core::control::ShapePoint { x: 0, y: 0 },
            end: hwpforge_core::control::ShapePoint { x: end.0, y: end.1 },
            width: hu(size.0),
            height: hu(size.1),
            frame: hwpforge_core::ObjectFrame::new()
                .with_original_size(hwpforge_foundation::Size::new(hu(original.0), hu(original.1))),
            placement: None,
            caption: None,
            style: rotation.map(|r| hwpforge_core::control::ShapeStyle {
                rotation: Some(r),
                ..Default::default()
            }),
        }
    }

    /// In a group a sibling sends to its fallback, a
    /// framed line turned 0.15° (stored as 0°) whose end moves from 1499 in
    /// its original box 1500 to 999 in its shown box 1000 lands 2.099 from
    /// where Core draws it — the writer reports it.
    /// 이것을 실패시키는 것: 묶음 fallback 재측정을 반올림한 사본 기준으로 하기.
    #[test]
    fn group_fallback_reports_a_rounding_miss() {
        let line = framed_line((1499, 0), (1000, 1000), (1500, 1500), Some(0.15));
        let trigger = framed_line((100_000_000, 0), (49, 0), (100_000_000, 0), None);
        let (xml, sink) = encode_group(&group_of(vec![line, trigger]));
        assert!(xml.contains(r#"<hc:endPt x="999" y="0"/>"#), "{xml}");
        use crate::encoder::EncodeWarning;
        let kinds: Vec<&str> = sink
            .warnings
            .iter()
            .filter_map(|w| match w {
                EncodeWarning::ContentLost { kind, .. } => Some(kind.as_str()),
                _ => None,
            })
            .collect();
        assert!(kinds.contains(&"shape_emission"), "{:?}", sink.warnings);
    }

    fn encode_group(group: &Control) -> (String, DocumentEncodeContext) {
        let mut sink = DocumentEncodeContext::new(0);
        let xml =
            encode_group_to_xml(group, 0, &mut Vec::new(), EncodeOptions::default(), &mut sink)
                .unwrap();
        (xml, sink)
    }

    /// A table and an equation in a grouped text box keep their own `<hp:sz>`
    /// and `<hp:pos>`; only the text box itself, a direct group child, has
    /// none (Hancom keeps the inner table's `<hp:sz width="19842">`).
    ///
    /// 이것을 실패시키는 것: 직속 자식 구조체의 `sz`/`pos` 를 `None` 으로 두지
    /// 않는 것(rect 에 남음), fragment 전체에서 지우는 것(tbl·equation 에서
    /// 사라짐).
    #[test]
    fn grouped_text_box_keeps_its_objects_placement() {
        let text_box = Control::text_box(
            vec![para_of(vec![
                Run::table(table_of_width(43_779), CharShapeIndex::new(0)),
                Run::control(Control::equation("a+b"), CharShapeIndex::new(0)),
            ])],
            HwpUnit::new(45_000).unwrap(),
            HwpUnit::new(56_000).unwrap(),
        );
        let (xml, _) = encode_group(&group_of(vec![text_box]));
        let paths = placement_paths(&xml);
        assert!(
            !paths
                .iter()
                .any(|p| p.starts_with("hp:container/hp:rect/hp:") && p.matches('/').count() == 2),
            "direct child keeps placement: {paths:?}"
        );
        for tag in ["hp:tbl/hp:sz", "hp:tbl/hp:pos", "hp:equation/hp:sz", "hp:equation/hp:pos"] {
            assert!(
                paths.iter().any(|p| p.starts_with("hp:container/hp:rect/hp:drawText/")
                    && p.ends_with(tag)),
                "{tag} missing: {paths:?}"
            );
        }
        assert!(xml.contains(r#"<hp:sz width="43779""#), "{xml}");
    }

    /// A nested group writes no `<hp:sz>`/`<hp:pos>` of its own; a table in a
    /// text box under it keeps both.
    ///
    /// 이것을 실패시키는 것: 중첩 묶음에서도 자기 배치를 쓰는 것(자식 여부 무시),
    /// fragment 전체에서 지우는 것.
    #[test]
    fn nested_group_omits_only_its_own_placement() {
        let text_box = Control::text_box(
            vec![para_of(vec![Run::table(table_of_width(20_000), CharShapeIndex::new(0))])],
            HwpUnit::new(30_000).unwrap(),
            HwpUnit::new(20_000).unwrap(),
        );
        let (xml, _) = encode_group(&group_of(vec![group_of(vec![text_box])]));
        let paths = placement_paths(&xml);
        assert_eq!(
            paths.iter().filter(|p| p.matches('/').count() == 1).count(),
            2,
            "only the outer container has its own sz/pos: {paths:?}"
        );
        assert!(
            !paths
                .iter()
                .any(|p| p.starts_with("hp:container/hp:container/hp:")
                    && p.matches('/').count() == 2),
            "{paths:?}"
        );
        assert!(paths.iter().any(|p| p.ends_with("hp:tbl/hp:sz")), "{paths:?}");
        assert!(paths.iter().any(|p| p.ends_with("hp:tbl/hp:pos")), "{paths:?}");
    }

    /// Every shape kind written as a group child drops its own `<hp:sz>` and
    /// `<hp:pos>`, while the table, picture and equation in its caption keep
    /// both.
    ///
    /// 이것을 실패시키는 것: 종류마다 `(hx.sz, hx.pos) = (None, None)` 을 지우는
    /// 것(그 종류만 직속 배치가 남음), fragment 전체에서 지우는 것(캡션 개체의
    /// `sz`·`pos` 가 사라짐).
    #[test]
    fn every_grouped_kind_drops_only_its_own_placement() {
        use hwpforge_core::caption::{Caption, CaptionSide};
        use hwpforge_core::image::Image;
        use hwpforge_foundation::ArcType;
        let hu = |v: i32| HwpUnit::new(v).unwrap();
        let p = |x, y| ShapePoint::new(x, y);
        let kinds: Vec<(&str, Control)> = vec![
            ("hp:rect", Control::rect(hu(4000), hu(2000)).unwrap()),
            ("hp:rect", Control::text_box(vec![para_of(Vec::new())], hu(4000), hu(2000))),
            ("hp:line", Control::line(p(0, 0), p(4000, 2000)).unwrap()),
            ("hp:ellipse", Control::ellipse(hu(4000), hu(2000))),
            ("hp:ellipse", Control::arc(ArcType::Normal, hu(4000), hu(2000))),
            ("hp:polygon", Control::polygon(vec![p(0, 0), p(4000, 0), p(2000, 2000)]).unwrap()),
            ("hp:curve", Control::curve(vec![p(0, 0), p(2000, 2000), p(4000, 0)]).unwrap()),
            ("hp:connectLine", Control::connect_line(p(0, 0), p(4000, 2000)).unwrap()),
        ];
        for (tag, mut shape) in kinds {
            let caption = Caption::new(
                vec![para_of(vec![
                    Run::table(table_of_width(4000), CharShapeIndex::new(0)),
                    Run::image(
                        Image::from_path("BinData/image1.png", hu(1000), hu(800)),
                        CharShapeIndex::new(0),
                    ),
                    Run::control(Control::equation("a+b"), CharShapeIndex::new(0)),
                ])],
                CaptionSide::Bottom,
            );
            match &mut shape {
                Control::Rect { caption: c, .. }
                | Control::TextBox { caption: c, .. }
                | Control::Line { caption: c, .. }
                | Control::Ellipse { caption: c, .. }
                | Control::Arc { caption: c, .. }
                | Control::Polygon { caption: c, .. }
                | Control::Curve { caption: c, .. }
                | Control::ConnectLine { caption: c, .. } => *c = Some(caption),
                _ => unreachable!(),
            }
            let name = shape.kind_name();
            let (xml, _) = encode_group(&group_of(vec![shape]));
            let paths = placement_paths(&xml);
            let own = |leaf: &str| format!("hp:container/{tag}/{leaf}");
            assert!(
                !paths.contains(&own("hp:sz")) && !paths.contains(&own("hp:pos")),
                "{name}: {paths:?}"
            );
            for object in ["hp:tbl", "hp:pic", "hp:equation"] {
                for leaf in ["hp:sz", "hp:pos"] {
                    assert!(
                        paths
                            .iter()
                            .any(|q| q.starts_with(&format!("hp:container/{tag}/hp:caption/"))
                                && q.ends_with(&format!("{object}/{leaf}"))),
                        "{name}: {object}/{leaf} missing: {paths:?}"
                    );
                }
            }
        }
    }

    /// A table in the caption of a grouped rectangle keeps its placement.
    ///
    /// 이것을 실패시키는 것: fragment 전체에서 `sz`/`pos` 를 지우는 것.
    #[test]
    fn grouped_rect_caption_table_keeps_its_placement() {
        use hwpforge_core::caption::{Caption, CaptionSide};
        let mut rect =
            Control::rect(HwpUnit::new(8000).unwrap(), HwpUnit::new(4000).unwrap()).unwrap();
        if let Control::Rect { caption, .. } = &mut rect {
            *caption = Some(Caption::new(
                vec![para_of(vec![Run::table(table_of_width(8000), CharShapeIndex::new(0))])],
                CaptionSide::Bottom,
            ));
        }
        let (xml, _) = encode_group(&group_of(vec![rect]));
        let paths = placement_paths(&xml);
        assert!(
            !paths
                .iter()
                .any(|p| p.starts_with("hp:container/hp:rect/hp:s") && p.matches('/').count() == 2),
            "{paths:?}"
        );
        assert!(
            paths.iter().any(|p| p.contains("hp:caption/") && p.ends_with("hp:tbl/hp:sz")),
            "{paths:?}"
        );
    }

    /// An equation or a chart in a group has no HWPX group form (Hancom will
    /// not group an equation), so it is left out and reported as a
    /// semantic loss at the place it would take; the other children stay.
    ///
    /// 이것을 실패시키는 것: 버리는 갈래의 `content_lost` 호출을 지우는 것, 경로를
    /// 쓰인 순번 대신 Core 자식 순번으로 매기는 것.
    #[test]
    fn group_equation_and_chart_are_reported_as_lost() {
        use crate::decoder::PathSeg;
        let hu = |v: i32| HwpUnit::new(v).unwrap();
        let chart = Control::EmbeddedChart {
            chart_xml: String::new(),
            ole_bytes: Vec::new(),
            width: hu(3000),
            height: hu(2000),
            placement: None,
        };
        let group = group_of(vec![
            Control::rect(hu(1000), hu(1000)).unwrap(),
            Control::equation("a+b"),
            chart,
            Control::rect(hu(2000), hu(1000)).unwrap(),
        ]);
        let (xml, sink) = encode_group(&group);
        assert_eq!(xml.matches("<hp:rect ").count(), 2, "{xml}");
        assert!(!xml.contains("<hp:equation") && !xml.contains("<hp:ole"), "{xml}");
        use crate::encoder::EncodeWarning;
        let lost: Vec<(&str, &str, Option<&PathSeg>)> = sink
            .warnings
            .iter()
            .filter_map(|w| match w {
                EncodeWarning::ContentLost { kind, reason, path, .. } => {
                    Some((kind.as_str(), reason.as_str(), path.0.last()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(lost.len(), 2, "{:?}", sink.warnings);
        assert_eq!((lost[0].0, lost[0].2), ("equation", Some(&PathSeg::GroupChild(1))));
        assert!(lost[0].1.contains("group child 1"), "{}", lost[0].1);
        assert_eq!((lost[1].0, lost[1].2), ("embedded_chart", Some(&PathSeg::GroupChild(1))));
        assert!(lost[1].1.contains("group child 2"), "{}", lost[1].1);
        assert!(sink.warnings.iter().all(EncodeWarning::is_semantic_loss));
    }

    fn text_art_at(x: i32, y: i32) -> Control {
        Control::TextArt {
            frame: hwpforge_core::ObjectFrame::new(),
            text: "묶음글맵시".to_string(),
            shape: "WAVE2".to_string(),
            font_name: "함초롬바탕".to_string(),
            font_style: "보통".to_string(),
            align: "LEFT".to_string(),
            line_spacing: 120,
            char_spacing: 100,
            width: HwpUnit::new(7087).unwrap(),
            height: HwpUnit::new(3543).unwrap(),
            placement: Some(
                ObjectPlacement::legacy_inline_defaults()
                    .with_horz_offset(HwpUnit::new(x).unwrap())
                    .with_vert_offset(HwpUnit::new(y).unwrap()),
            ),
            fill_color: Some(Color::from_rgb(255, 0, 0)),
            inst_id: None,
        }
    }

    /// The `<hp:offset>` and transMatrix translation of the first
    /// `<hp:textart>` in `xml`.
    fn text_art_place(xml: &str) -> (String, String) {
        let at = xml.find("<hp:textart ").expect("textart written");
        let tail = &xml[at..];
        let offset = tail.find("<hp:offset ").map(|i| &tail[i..]).unwrap();
        let trans = tail.find("<hc:transMatrix ").map(|i| &tail[i..]).unwrap();
        (
            offset[..offset.find("/>").unwrap() + 2].to_string(),
            trans[..trans.find("/>").unwrap() + 2].to_string(),
        )
    }

    /// A TextArt in a group is placed at its own offset: the offset and
    /// the transMatrix translation both carry it, and no `sz`/`pos` is written.
    ///
    /// 이것을 실패시키는 것: `group_child_offset` 의 TextArt 갈래를 지우는 것
    /// (원점 (0, 0) 에 놓임).
    #[test]
    fn grouped_text_art_keeps_its_offset() {
        let (xml, _) = encode_group(&group_of(vec![text_art_at(1200, 800)]));
        let (offset, trans) = text_art_place(&xml);
        assert_eq!(offset, r#"<hp:offset x="1200" y="800"/>"#);
        assert_eq!(trans, r#"<hc:transMatrix e1="1" e2="0" e3="1200" e4="0" e5="1" e6="800"/>"#);
        assert!(!placement_paths(&xml).iter().any(|p| p.contains("hp:textart/")), "{xml}");
        assert!(xml.contains(r##"faceColor="#FF0000""##), "{xml}");
        // Written as Hancom writes a group child TextArt.
        // 이것을 실패시키는 것: 묶음 자식 글맵시도 `PICTURE`/`SQUARE` 로 쓰는 것.
        let art = &xml[xml.find("<hp:textart ").unwrap()..];
        let head = &art[..art.find('>').unwrap()];
        assert!(head.contains(r#"numberingType="NONE" textWrap="TOP_AND_BOTTOM""#), "{head}");
    }

    /// In a nested group the TextArt's offset adds its parent's origin, the
    /// same as the matrix translation.
    ///
    /// 이것을 실패시키는 것: 묶음 자식 글맵시가 자기 배치 offset 을 그대로 쓰는 것
    /// (부모 원점이 빠진 (1200, 800) 이 남음).
    #[test]
    fn nested_grouped_text_art_adds_its_parent_origin() {
        let inner = Control::Group {
            frame: hwpforge_core::ObjectFrame::new(),
            children: vec![hwpforge_core::control::GroupChild::Shape(text_art_at(1200, 800))],
            width: HwpUnit::new(9000).unwrap(),
            height: HwpUnit::new(5000).unwrap(),
            placement: Some(
                ObjectPlacement::legacy_inline_defaults()
                    .with_horz_offset(HwpUnit::new(500).unwrap())
                    .with_vert_offset(HwpUnit::new(300).unwrap()),
            ),
            inst_id: None,
        };
        let (xml, _) = encode_group(&group_of(vec![inner]));
        let (offset, trans) = text_art_place(&xml);
        assert_eq!(offset, r#"<hp:offset x="1700" y="1100"/>"#);
        assert_eq!(trans, r#"<hc:transMatrix e1="1" e2="0" e3="1700" e4="0" e5="1" e6="1100"/>"#);
    }

    /// Replaces the counter-based `id`/`instid` values with `#` so encoder
    /// output can be compared byte for byte.
    fn normalize_ids(xml: &str) -> String {
        let mut out = String::with_capacity(xml.len());
        let mut rest = xml;
        loop {
            let next = [" id=\"", " instid=\""]
                .iter()
                .filter_map(|key| rest.find(key).map(|at| (at, key.len())))
                .min();
            let Some((at, len)) = next else { break };
            out.push_str(&rest[..at + len]);
            rest = &rest[at + len..];
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 {
                out.push('#');
            }
            rest = &rest[digits..];
        }
        out.push_str(rest);
        out
    }

    #[test]
    fn build_shape_common_rotation_applied_correctly() {
        let style = ShapeStyle { rotation: Some(45.0), ..Default::default() };
        let sc = common(1000, 500, Some(&style));
        assert_eq!(sc.rotation_info.angle, 45);
        // 이것을 실패시키는 것: 독립 출력 중심에서 배치 이동 T 누락.
        assert_eq!(sc.rotation_info.center_x, 530);
        assert_eq!(sc.rotation_info.center_y, 530);
    }

    #[test]
    fn build_shape_common_rotation_90_degrees() {
        let style = ShapeStyle { rotation: Some(90.0), ..Default::default() };
        let sc = common(2000, 1000, Some(&style));
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
        let sc = common(1000, 500, Some(&style));
        assert_eq!(sc.flip.horizontal, 1);
        assert_eq!(sc.flip.vertical, 0);
    }

    #[test]
    fn build_shape_common_flip_vertical() {
        let style = ShapeStyle { flip: Some(Flip::Vertical), ..Default::default() };
        let sc = common(1000, 500, Some(&style));
        assert_eq!(sc.flip.horizontal, 0);
        assert_eq!(sc.flip.vertical, 1);
    }

    #[test]
    fn build_shape_common_flip_both() {
        let style = ShapeStyle { flip: Some(Flip::Both), ..Default::default() };
        let sc = common(1000, 500, Some(&style));
        assert_eq!(sc.flip.horizontal, 1);
        assert_eq!(sc.flip.vertical, 1);
    }

    #[test]
    fn build_shape_common_line_color_overridden() {
        let style = make_style(Some("#FF0000"), None, None);
        let sc = common(1000, 500, Some(&style));
        assert_eq!(sc.line_shape.color, "#FF0000");
    }

    /// Hancom writes a negative `<hp:curSz>` side or `<hp:offset>` as its
    /// unsigned wrap (44 curSz and 2,076 offset attributes in 485
    /// Hancom-saved files, never as a negative literal).
    ///
    /// 이것을 실패시키는 것: `HxSizeAttr` 의 `width`·`height`, `HxOffset` 의
    /// `x`·`y` 를 i32 로 쓰기(각각).
    #[test]
    fn negative_size_and_offset_are_written_wrapped() {
        let sz = HxSizeAttr { width: -14173, height: -2355 };
        assert_eq!(
            serialize_with_root(&sz, "hp:curSz").unwrap(),
            r#"<hp:curSz width="4294953123" height="4294964941"/>"#
        );
        let offset = HxOffset { x: -310, y: -8148 };
        assert_eq!(
            serialize_with_root(&offset, "hp:offset").unwrap(),
            r#"<hp:offset x="4294966986" y="4294959148"/>"#
        );
    }

    #[test]
    fn build_shape_common_line_width_overridden() {
        let style = make_style(None, None, Some(100));
        let sc = common(1000, 500, Some(&style));
        assert_eq!(sc.line_shape.width, 100);
    }

    #[test]
    fn build_shape_common_fill_color_overridden() {
        let style = make_style(None, Some("#00FF00"), None);
        let sc = common(1000, 500, Some(&style));
        assert_eq!(
            sc.fill_brush.as_ref().unwrap().win_brush.as_ref().unwrap().face_color,
            "#00FF00"
        );
    }

    #[test]
    fn build_shape_common_line_style_dash_overridden() {
        let style = ShapeStyle { line_style: Some(LineStyle::Dash), ..Default::default() };
        let sc = common(1000, 500, Some(&style));
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
        let sc = common(1000, 500, Some(&style));
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
        let sc = common(1000, 500, Some(&style));
        // Diamond always maps to EMPTY_DIAMOND; headfill controls fill
        assert_eq!(sc.line_shape.tail_style, "EMPTY_DIAMOND");
        assert_eq!(sc.line_shape.tail_fill, 0);
        assert_eq!(sc.line_shape.tail_sz, "SMALL_SMALL");
    }

    #[test]
    fn build_shape_common_default_solid_line_style() {
        let sc = common(1000, 500, None);
        assert_eq!(sc.line_shape.style, "SOLID");
        assert_eq!(sc.line_shape.color, "#000000");
        assert_eq!(sc.line_shape.width, 33);
    }

    #[test]
    fn build_shape_common_default_white_fill() {
        let sc = common(1000, 500, None);
        assert_eq!(
            sc.fill_brush.as_ref().unwrap().win_brush.as_ref().unwrap().face_color,
            "#FFFFFF"
        );
    }

    #[test]
    fn build_shape_common_no_rotation_uses_identity_matrix() {
        let sc = common(1000, 500, None);
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
        let sc = common(1000, 500, Some(&style));
        let wb = sc.fill_brush.as_ref().unwrap().win_brush.as_ref().unwrap();
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
        let sc = common(1000, 500, Some(&style));
        let wb = sc.fill_brush.as_ref().unwrap().win_brush.as_ref().unwrap();
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
        let sc = common(1000, 500, Some(&style));
        let wb = sc.fill_brush.as_ref().unwrap().win_brush.as_ref().unwrap();
        assert_eq!(wb.hatch_style, Some("BACK_SLASH".to_string()));
    }

    #[test]
    fn build_shape_common_solid_fill_no_hatch_style() {
        let style = ShapeStyle {
            fill: Some(Fill::Solid { color: Color::from_rgb(255, 0, 0) }),
            ..Default::default()
        };
        let sc = common(1000, 500, Some(&style));
        let wb = sc.fill_brush.as_ref().unwrap().win_brush.as_ref().unwrap();
        assert_eq!(wb.hatch_style, None);
    }

    // ── encode_arc_to_hx tests ───────────────────────────────────────

    #[test]
    fn encode_arc_has_arc_pr_flag_set() {
        let ctrl = Control::Arc {
            definition: hwpforge_core::control::ArcDefinition::EllipseSweep,
            frame: hwpforge_core::ObjectFrame::new(),
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
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_arc_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert_eq!(result.has_arc_pr, 1, "Arc must have hasArcPr=1");
    }

    #[test]
    fn encode_arc_type_pie_encoded() {
        let ctrl = Control::Arc {
            definition: hwpforge_core::control::ArcDefinition::EllipseSweep,
            frame: hwpforge_core::ObjectFrame::new(),
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
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_arc_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert_eq!(result.arc_type, "PIE");
    }

    #[test]
    fn encode_arc_geometry_points_preserved() {
        let ctrl = Control::Arc {
            definition: hwpforge_core::control::ArcDefinition::EllipseSweep,
            frame: hwpforge_core::ObjectFrame::new(),
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
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_arc_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
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
            definition: hwpforge_core::control::ArcDefinition::EllipseSweep,
            frame: hwpforge_core::ObjectFrame::new(),
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
            placement: Some(
                ObjectPlacement::legacy_inline_defaults()
                    .with_treat_as_char(false)
                    .with_horz_offset(HwpUnit::new(100).unwrap())
                    .with_vert_offset(HwpUnit::new(200).unwrap()),
            ),
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_arc_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert_eq!(result.sz.as_ref().unwrap().width, 7000);
        assert_eq!(result.sz.as_ref().unwrap().height, 4000);
        // Floating placement (treat_as_char=false) → pos treatAsChar=0
        assert_eq!(result.pos.as_ref().unwrap().treat_as_char, 0);
        assert_eq!(result.pos.as_ref().unwrap().horz_offset, 100);
    }

    #[test]
    fn encode_arc_shape_comment_is_ho() {
        let ctrl = Control::Arc {
            definition: hwpforge_core::control::ArcDefinition::EllipseSweep,
            frame: hwpforge_core::ObjectFrame::new(),
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
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_arc_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert_eq!(result.shape_comment.as_ref().unwrap().text, "호입니다.");
    }

    #[test]
    fn encode_arc_draw_text_is_none() {
        let ctrl = Control::Arc {
            definition: hwpforge_core::control::ArcDefinition::EllipseSweep,
            frame: hwpforge_core::ObjectFrame::new(),
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
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_arc_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert!(result.draw_text.is_none(), "Arc should have no draw_text");
    }

    // ── encode_curve_to_hx tests ─────────────────────────────────────

    #[test]
    fn encode_curve_empty_points_gives_empty_segments() {
        let ctrl = Control::Curve {
            frame: hwpforge_core::ObjectFrame::new(),
            points: vec![],
            segment_types: vec![],
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(500).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_curve_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert!(result.segments.is_empty());
        assert!(result.points.is_empty(), "KS X 6101: coords go in segments, not points");
    }

    #[test]
    fn encode_curve_segments_created_from_points() {
        let ctrl = Control::Curve {
            frame: hwpforge_core::ObjectFrame::new(),
            points: vec![
                ShapePoint::new(0, 0),
                ShapePoint::new(100, 50),
                ShapePoint::new(200, 100),
            ],
            segment_types: vec![CurveSegmentType::Curve, CurveSegmentType::Line],
            width: HwpUnit::new(3000).unwrap(),
            height: HwpUnit::new(1500).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_curve_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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
            frame: hwpforge_core::ObjectFrame::new(),
            points: vec![ShapePoint::new(50, 50)],
            segment_types: vec![],
            width: HwpUnit::new(500).unwrap(),
            height: HwpUnit::new(500).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_curve_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert!(result.segments.is_empty(), "single point → no segments");
    }

    #[test]
    fn encode_curve_segment_type_repeats_when_fewer_types_than_segments() {
        // More points than segment_types → extra segments repeat Curve type
        let ctrl = Control::Curve {
            frame: hwpforge_core::ObjectFrame::new(),
            points: vec![
                ShapePoint::new(0, 0),
                ShapePoint::new(100, 0),
                ShapePoint::new(200, 0),
                ShapePoint::new(300, 0),
            ],
            segment_types: vec![CurveSegmentType::Line], // only 1 type for 3 segments
            width: HwpUnit::new(4000).unwrap(),
            height: HwpUnit::new(500).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_curve_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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
            frame: hwpforge_core::ObjectFrame::new(),
            points: vec![],
            segment_types: vec![],
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(500).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_curve_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert_eq!(result.shape_comment.as_ref().unwrap().text, "곡선입니다.");
    }

    #[test]
    fn encode_curve_inline_offset_zero_gives_treat_as_char_1() {
        let ctrl = Control::Curve {
            frame: hwpforge_core::ObjectFrame::new(),
            points: vec![],
            segment_types: vec![],
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(500).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_curve_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert_eq!(result.pos.as_ref().unwrap().treat_as_char, 1);
    }

    // ── encode_connect_line_to_hx tests ──────────────────────────────

    #[test]
    fn encode_connect_line_control_points_wrapped_with_start_end() {
        let ctrl = Control::ConnectLine {
            frame: hwpforge_core::ObjectFrame::new(),
            start: ShapePoint::new(10, 20),
            end: ShapePoint::new(500, 600),
            control_points: vec![ShapePoint::new(200, 300)],
            connect_type: "BENT".to_string(),
            width: HwpUnit::new(3000).unwrap(),
            height: HwpUnit::new(2000).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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
            frame: hwpforge_core::ObjectFrame::new(),
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 100),
            control_points: vec![],
            connect_type: "STRAIGHT".to_string(),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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
            frame: hwpforge_core::ObjectFrame::new(),
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 100),
            control_points: vec![],
            connect_type: "CURVED".to_string(),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert_eq!(result.connect_type, "CURVED");
    }

    #[test]
    fn encode_connect_line_fill_brush_is_none() {
        // Connect lines have no fill brush (same as regular lines per golden)
        let ctrl = Control::ConnectLine {
            frame: hwpforge_core::ObjectFrame::new(),
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 100),
            control_points: vec![],
            connect_type: "STRAIGHT".to_string(),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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
            frame: hwpforge_core::ObjectFrame::new(),
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(14000, 0),
            control_points: vec![],
            connect_type: "STRAIGHT".to_string(),
            width: HwpUnit::new(14000).unwrap(),
            height: HwpUnit::new(0).unwrap(),
            placement: Some(floating_placement(17657, 14057)),
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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
            frame: hwpforge_core::ObjectFrame::new(),
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 100),
            control_points: vec![],
            connect_type: "STRAIGHT".to_string(),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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
            frame: hwpforge_core::ObjectFrame::new(),
            start: ShapePoint::new(111, 222),
            end: ShapePoint::new(333, 444),
            control_points: vec![],
            connect_type: "STRAIGHT".to_string(),
            width: HwpUnit::new(2000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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
            frame: hwpforge_core::ObjectFrame::new(),
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 0),
            control_points: vec![],
            connect_type: "STRAIGHT".to_string(),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert_eq!(result.shape_comment.as_ref().unwrap().text, "연결선입니다.");
    }

    #[test]
    fn encode_connect_line_non_zero_offset_gives_treat_as_char_0() {
        let ctrl = Control::ConnectLine {
            frame: hwpforge_core::ObjectFrame::new(),
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 0),
            control_points: vec![],
            connect_type: "STRAIGHT".to_string(),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            placement: Some(floating_placement(50, 0)),
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_connect_line_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert_eq!(result.pos.as_ref().unwrap().treat_as_char, 0);
    }

    // ── encode_line_to_hx tests ──────────────────────────────────────

    #[test]
    fn encode_line_fill_brush_is_none() {
        let ctrl = Control::Line {
            frame: hwpforge_core::ObjectFrame::new(),
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 0),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_line_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert!(result.fill_brush.is_none(), "lines have no fill brush per golden");
    }

    #[test]
    fn encode_line_shape_comment_is_seon() {
        let ctrl = Control::Line {
            frame: hwpforge_core::ObjectFrame::new(),
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 0),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_line_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert_eq!(result.shape_comment.as_ref().unwrap().text, "선입니다.");
    }

    #[test]
    fn encode_line_endpoints_preserved() {
        let ctrl = Control::Line {
            frame: hwpforge_core::ObjectFrame::new(),
            start: ShapePoint::new(50, 100),
            end: ShapePoint::new(500, 200),
            width: HwpUnit::new(5000).unwrap(),
            height: HwpUnit::new(2000).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_line_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert_eq!(result.start_pt.as_ref().unwrap().x, 50);
        assert_eq!(result.start_pt.as_ref().unwrap().y, 100);
        assert_eq!(result.end_pt.as_ref().unwrap().x, 500);
        assert_eq!(result.end_pt.as_ref().unwrap().y, 200);
    }

    #[test]
    fn encode_line_non_zero_offset_uses_floating_shape_defaults() {
        let ctrl = Control::Line {
            frame: hwpforge_core::ObjectFrame::new(),
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(100, 0),
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            placement: Some(floating_placement(50, 60)),
            caption: None,
            style: None,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_line_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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

    // ── encode_ellipse_to_hx tests ───────────────────────────────────

    #[test]
    fn encode_ellipse_shape_comment_is_taewon() {
        let ctrl = Control::Ellipse {
            frame: hwpforge_core::ObjectFrame::new(),
            center: ShapePoint::new(0, 0),
            axis1: ShapePoint::new(100, 0),
            axis2: ShapePoint::new(0, 50),
            width: HwpUnit::new(200).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            placement: None,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_ellipse_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert_eq!(result.shape_comment.as_ref().unwrap().text, "타원입니다.");
    }

    #[test]
    fn encode_ellipse_has_arc_pr_zero() {
        let ctrl = Control::Ellipse {
            frame: hwpforge_core::ObjectFrame::new(),
            center: ShapePoint::new(0, 0),
            axis1: ShapePoint::new(100, 0),
            axis2: ShapePoint::new(0, 50),
            width: HwpUnit::new(200).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            placement: None,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_ellipse_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert_eq!(result.has_arc_pr, 0, "Ellipse must have hasArcPr=0");
    }

    #[test]
    fn encode_ellipse_empty_paragraphs_gives_no_draw_text() {
        let ctrl = Control::Ellipse {
            frame: hwpforge_core::ObjectFrame::new(),
            center: ShapePoint::new(0, 0),
            axis1: ShapePoint::new(100, 0),
            axis2: ShapePoint::new(0, 50),
            width: HwpUnit::new(200).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            placement: None,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_ellipse_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert!(result.draw_text.is_none());
    }

    fn ellipse_with_valign(valign: VerticalAlign) -> Control {
        use hwpforge_core::paragraph::Paragraph;
        use hwpforge_foundation::ParaShapeIndex;
        Control::Ellipse {
            frame: hwpforge_core::ObjectFrame::new(),
            center: ShapePoint::new(0, 0),
            axis1: ShapePoint::new(100, 0),
            axis2: ShapePoint::new(0, 50),
            width: HwpUnit::new(200).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            placement: None,
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
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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
            frame: hwpforge_core::ObjectFrame::new(),
            vertices: vertices.clone(),
            width: HwpUnit::new(2000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            placement: None,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_polygon_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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
            frame: hwpforge_core::ObjectFrame::new(),
            vertices: vec![
                ShapePoint::new(0, 0),
                ShapePoint::new(100, 0),
                ShapePoint::new(50, 100),
                ShapePoint::new(0, 0),
            ],
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            placement: None,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_polygon_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert_eq!(result.shape_comment.as_ref().unwrap().text, "다각형입니다.");
    }

    #[test]
    fn encode_polygon_non_zero_offset_uses_floating_shape_defaults() {
        let ctrl = Control::Polygon {
            frame: hwpforge_core::ObjectFrame::new(),
            vertices: vec![
                ShapePoint::new(0, 0),
                ShapePoint::new(100, 0),
                ShapePoint::new(50, 100),
                ShapePoint::new(0, 0),
            ],
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            placement: Some(floating_placement(50, 60)),
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_polygon_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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
        let result = encode_rect_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
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
        let rect = encode_rect_to_hx(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        let mut buf = String::new();
        #[allow(clippy::disallowed_methods)] // test inspects raw serde output
        let ser = quick_xml::se::Serializer::with_root(&mut buf, Some("hp:rect")).unwrap();
        serde::Serialize::serialize(&rect, ser).unwrap();
        assert!(buf.contains("<hp:rect"), "encoded XML should contain <hp:rect: {buf}");
        assert!(!buf.contains("<hp:drawText"), "pure rect must not contain <hp:drawText>: {buf}");
    }

    #[test]
    fn encode_textbox_non_zero_offset_uses_floating_shape_defaults() {
        let ctrl = Control::TextBox {
            frame: hwpforge_core::ObjectFrame::new(),
            text_margin: None,
            paragraphs: vec![],
            width: HwpUnit::new(1000).unwrap(),
            height: HwpUnit::new(1000).unwrap(),
            placement: Some(floating_placement(50, 60)),
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let mut hl = empty_hyperlinks();
        let result = encode_textbox_to_rect(
            &ctrl,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
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
    fn constructor_shapes_emit_legacy_inline_placement() {
        // The generation path (constructor → `placement: None`) must reproduce
        // the historical inline `<hp:pos>` unchanged: treatAsChar=1, PARA/PARA,
        // zero offset, numberingType NONE, textWrap TOP_AND_BOTTOM. This is the
        // hard gate that the placement refactor left the None path byte-stable.
        let mut hl = empty_hyperlinks();
        let rect = Control::rect(HwpUnit::new(8000).unwrap(), HwpUnit::new(4000).unwrap()).unwrap();
        let hx = encode_rect_to_hx(
            &rect,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert_eq!(hx.numbering_type, "NONE");
        assert_eq!(hx.text_wrap, "TOP_AND_BOTTOM");
        assert_eq!(hx.text_flow, "BOTH_SIDES");
        let pos = hx.pos.expect("pos");
        assert_eq!(pos.treat_as_char, 1);
        assert_eq!(pos.allow_overlap, 0);
        assert_eq!(pos.flow_with_text, 0);
        assert_eq!(pos.vert_rel_to, "PARA");
        assert_eq!(pos.horz_rel_to, "PARA");
        assert_eq!(pos.vert_align, "TOP");
        assert_eq!(pos.horz_align, "LEFT");
        assert_eq!(pos.horz_offset, 0);
        assert_eq!(pos.vert_offset, 0);

        // A constructor text box (also `placement: None`) emits the same block.
        let tb =
            Control::text_box(vec![], HwpUnit::new(8000).unwrap(), HwpUnit::new(4000).unwrap());
        let hx = encode_textbox_to_rect(
            &tb,
            false,
            &mut hl,
            EncodeOptions::default(),
            &mut DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert_eq!(hx.numbering_type, "NONE");
        assert_eq!(hx.text_wrap, "TOP_AND_BOTTOM");
        let pos = hx.pos.expect("pos");
        assert_eq!(pos.treat_as_char, 1);
        assert_eq!(pos.vert_rel_to, "PARA");
        assert_eq!(pos.horz_offset, 0);
    }
}
