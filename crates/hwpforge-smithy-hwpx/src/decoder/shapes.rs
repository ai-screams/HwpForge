//! Decodes HWPX shape elements into Core control types.
//!
//! Split from `section.rs` to enable parallel development of shape features.
//! Functions here convert `HxRect`, `HxLine`, `HxEllipse`, and `HxPolygon`
//! into Core `Run` values with the appropriate `Control` variant.

use hwpforge_core::control::{Control, GroupChild, ShapeStyle};
use hwpforge_core::placement::{ObjectPlacement, ObjectRelativeTo, ObjectTextFlow, ObjectTextWrap};
use hwpforge_core::run::{Run, RunContent};
use hwpforge_foundation::{
    ArcType, CharShapeIndex, Color, CurveSegmentType, DropCapStyle, Flip, HwpUnit, PatternType,
    VerticalAlign,
};

use crate::error::HwpxResult;
use crate::schema::section::{
    HxConnectLine, HxCurve, HxEllipse, HxFillBrush, HxLine, HxLineShape, HxPolygon, HxRect,
    HxTablePos, HxTextArt,
};
use crate::wire_xml::textart_text_from_wire;

use super::section::{convert_hx_caption, decode_sublist_paragraphs, parse_hex_color, DecodeCtx};

/// Builds an [`ObjectPlacement`] from a shape's `<hp:pos>` block plus the shape
/// element's `textWrap`/`textFlow` attributes, collapsing a plain-inline
/// placement back to `None`.
///
/// A shape whose placement equals [`ObjectPlacement::legacy_inline_defaults`]
/// carries no positioning information; returning `None` lets the encoder
/// re-emit it through the untouched legacy inline path (the same `Option`
/// collapse `decode_shape_style_full` applies to `ShapeStyle`). Empty
/// `textWrap`/`textFlow`/`relTo` attributes — which 한컴 omits for the default —
/// map to the inline defaults so the collapse fires for native inline shapes.
fn decode_object_placement(
    pos: Option<&HxTablePos>,
    text_wrap: &str,
    text_flow: &str,
) -> Option<ObjectPlacement> {
    let pos = pos?;
    let placement = ObjectPlacement {
        text_wrap: if text_wrap.is_empty() {
            ObjectTextWrap::TopAndBottom
        } else {
            ObjectTextWrap::from_hwpx(text_wrap)
        },
        text_flow: if text_flow.is_empty() {
            ObjectTextFlow::BothSides
        } else {
            ObjectTextFlow::from_hwpx(text_flow)
        },
        treat_as_char: pos.treat_as_char != 0,
        flow_with_text: pos.flow_with_text != 0,
        allow_overlap: pos.allow_overlap != 0,
        vert_rel_to: if pos.vert_rel_to.is_empty() {
            ObjectRelativeTo::Para
        } else {
            ObjectRelativeTo::from_hwpx(&pos.vert_rel_to)
        },
        horz_rel_to: if pos.horz_rel_to.is_empty() {
            ObjectRelativeTo::Para
        } else {
            ObjectRelativeTo::from_hwpx(&pos.horz_rel_to)
        },
        vert_offset: HwpUnit::new(pos.vert_offset).unwrap_or(HwpUnit::ZERO),
        horz_offset: HwpUnit::new(pos.horz_offset).unwrap_or(HwpUnit::ZERO),
    };
    if placement == ObjectPlacement::legacy_inline_defaults() {
        None
    } else {
        Some(placement)
    }
}

/// Parses an `<hp:subList vertAlign="...">` token into a Core [`VerticalAlign`].
///
/// An empty attribute (한컴 omits it for the default) or any unrecognized value
/// falls back to [`VerticalAlign::Top`], matching the encoder default so that
/// default shapes round-trip byte-for-byte.
fn decode_shape_vert_align(token: &str) -> VerticalAlign {
    token.parse().unwrap_or(VerticalAlign::Top)
}

/// Decodes an `HxRect` into a Core `Run`.
///
/// Rects with `<hp:drawText>` become [`Control::TextBox`] (textbox with embedded
/// paragraphs). Rects without `<hp:drawText>` become [`Control::Rect`] (pure
/// rectangle shape).
pub(crate) fn decode_textbox(
    rect: &HxRect,
    char_shape_id: CharShapeIndex,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> HwpxResult<Option<Run>> {
    // Extract width/height from sz, falling back to zero
    let (width, height) = rect
        .sz
        .as_ref()
        .map(|sz| {
            (
                HwpUnit::new(sz.width).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(sz.height).unwrap_or(HwpUnit::ZERO),
            )
        })
        .unwrap_or((HwpUnit::ZERO, HwpUnit::ZERO));

    let placement = decode_object_placement(rect.pos.as_ref(), &rect.text_wrap, &rect.text_flow);

    let caption = rect.caption.as_ref().map(|c| convert_hx_caption(c, depth, ctx)).transpose()?;

    let style = decode_shape_style(
        &rect.line_shape,
        &rect.fill_brush,
        true,
        rect.rotation_info.as_ref(),
        rect.flip.as_ref(),
        &rect.dropcap_style,
        ctx,
    );

    let control = match &rect.draw_text {
        Some(draw_text) => {
            ctx.enter(super::PathSeg::TextBox);
            let result = decode_sublist_paragraphs(&draw_text.sub_list, depth, ctx);
            ctx.leave();
            let paragraphs = result?;
            let text_vertical_align = decode_shape_vert_align(&draw_text.sub_list.vert_align);
            Control::TextBox {
                paragraphs,
                width,
                height,
                placement,
                caption,
                style,
                text_vertical_align,
            }
        }
        None => Control::Rect { width, height, placement, caption, style },
    };

    Ok(Some(Run { content: RunContent::Control(Box::new(control)), char_shape_id }))
}

/// Decodes an `HxLine` into a Core `Run` with `Control::Line`.
pub(crate) fn decode_line(
    line: &HxLine,
    char_shape_id: CharShapeIndex,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> HwpxResult<Run> {
    use hwpforge_core::control::ShapePoint;

    let start = line
        .start_pt
        .as_ref()
        .map(|p| ShapePoint { x: p.x, y: p.y })
        .unwrap_or(ShapePoint { x: 0, y: 0 });
    let end = line
        .end_pt
        .as_ref()
        .map(|p| ShapePoint { x: p.x, y: p.y })
        .unwrap_or(ShapePoint { x: 0, y: 0 });

    let (width, height) = line
        .sz
        .as_ref()
        .map(|sz| {
            (
                HwpUnit::new(sz.width).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(sz.height).unwrap_or(HwpUnit::ZERO),
            )
        })
        .unwrap_or((HwpUnit::ZERO, HwpUnit::ZERO));

    let caption = line.caption.as_ref().map(|c| convert_hx_caption(c, depth, ctx)).transpose()?;

    let placement = decode_object_placement(line.pos.as_ref(), &line.text_wrap, &line.text_flow);

    Ok(Run {
        content: RunContent::Control(Box::new(Control::Line {
            start,
            end,
            width,
            height,
            placement,
            caption,
            style: decode_shape_style(
                &line.line_shape,
                &line.fill_brush,
                false,
                line.rotation_info.as_ref(),
                line.flip.as_ref(),
                &line.dropcap_style,
                ctx,
            ),
        })),
        char_shape_id,
    })
}

/// Decodes an `HxEllipse` into a Core `Run` with `Control::Ellipse`.
pub(crate) fn decode_ellipse(
    ellipse: &HxEllipse,
    char_shape_id: CharShapeIndex,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> HwpxResult<Run> {
    use hwpforge_core::control::ShapePoint;

    let center = ellipse
        .center
        .as_ref()
        .map(|p| ShapePoint { x: p.x, y: p.y })
        .unwrap_or(ShapePoint { x: 0, y: 0 });
    let axis1 = ellipse
        .ax1
        .as_ref()
        .map(|p| ShapePoint { x: p.x, y: p.y })
        .unwrap_or(ShapePoint { x: 0, y: 0 });
    let axis2 = ellipse
        .ax2
        .as_ref()
        .map(|p| ShapePoint { x: p.x, y: p.y })
        .unwrap_or(ShapePoint { x: 0, y: 0 });

    let (width, height) = ellipse
        .sz
        .as_ref()
        .map(|sz| {
            (
                HwpUnit::new(sz.width).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(sz.height).unwrap_or(HwpUnit::ZERO),
            )
        })
        .unwrap_or((HwpUnit::ZERO, HwpUnit::ZERO));

    let (paragraphs, text_vertical_align) = match &ellipse.draw_text {
        Some(dt) => {
            ctx.enter(super::PathSeg::TextBox);
            let result = decode_sublist_paragraphs(&dt.sub_list, depth, ctx);
            ctx.leave();
            (result?, decode_shape_vert_align(&dt.sub_list.vert_align))
        }
        None => (Vec::new(), VerticalAlign::Top),
    };

    let caption =
        ellipse.caption.as_ref().map(|c| convert_hx_caption(c, depth, ctx)).transpose()?;

    let placement =
        decode_object_placement(ellipse.pos.as_ref(), &ellipse.text_wrap, &ellipse.text_flow);

    Ok(Run {
        content: RunContent::Control(Box::new(Control::Ellipse {
            center,
            axis1,
            axis2,
            width,
            height,
            placement,
            paragraphs,
            caption,
            style: decode_shape_style(
                &ellipse.line_shape,
                &ellipse.fill_brush,
                true,
                ellipse.rotation_info.as_ref(),
                ellipse.flip.as_ref(),
                &ellipse.dropcap_style,
                ctx,
            ),
            text_vertical_align,
        })),
        char_shape_id,
    })
}

/// Decodes an `HxPolygon` into a Core `Run` with `Control::Polygon`.
pub(crate) fn decode_polygon(
    polygon: &HxPolygon,
    char_shape_id: CharShapeIndex,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> HwpxResult<Run> {
    use hwpforge_core::control::ShapePoint;

    let vertices: Vec<ShapePoint> =
        polygon.points.iter().map(|p| ShapePoint { x: p.x, y: p.y }).collect();

    let (width, height) = polygon
        .sz
        .as_ref()
        .map(|sz| {
            (
                HwpUnit::new(sz.width).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(sz.height).unwrap_or(HwpUnit::ZERO),
            )
        })
        .unwrap_or((HwpUnit::ZERO, HwpUnit::ZERO));

    let (paragraphs, text_vertical_align) = match &polygon.draw_text {
        Some(dt) => {
            ctx.enter(super::PathSeg::TextBox);
            let result = decode_sublist_paragraphs(&dt.sub_list, depth, ctx);
            ctx.leave();
            (result?, decode_shape_vert_align(&dt.sub_list.vert_align))
        }
        None => (Vec::new(), VerticalAlign::Top),
    };

    let caption =
        polygon.caption.as_ref().map(|c| convert_hx_caption(c, depth, ctx)).transpose()?;

    let placement =
        decode_object_placement(polygon.pos.as_ref(), &polygon.text_wrap, &polygon.text_flow);

    Ok(Run {
        content: RunContent::Control(Box::new(Control::Polygon {
            vertices,
            width,
            height,
            placement,
            paragraphs,
            caption,
            style: decode_shape_style(
                &polygon.line_shape,
                &polygon.fill_brush,
                true,
                polygon.rotation_info.as_ref(),
                polygon.flip.as_ref(),
                &polygon.dropcap_style,
                ctx,
            ),
            text_vertical_align,
        })),
        char_shape_id,
    })
}

/// [`decode_shape_style_full`] for a shape the decoder places in Core, plus
/// the picture-fill loss report: a picture fill Core cannot carry exactly is
/// [`ContentLost`](super::DecodeWarning::ContentLost) (`shape_image_fill`).
/// Group children are reported by the group path (`group_child_fill`).
fn decode_shape_style(
    line_shape: &Option<HxLineShape>,
    fill_brush: &Option<HxFillBrush>,
    fillable: bool,
    rotation_info: Option<&crate::schema::section::HxRotationInfo>,
    flip_info: Option<&crate::schema::section::HxFlip>,
    dropcap_style: &str,
    ctx: &mut DecodeCtx,
) -> Option<ShapeStyle> {
    if !ctx.in_group_child() {
        if let Some(reason) = fill_brush.as_ref().and_then(image_fill_loss) {
            ctx.content_lost("shape_image_fill", reason);
        }
    }
    decode_shape_style_full(
        line_shape,
        fill_brush,
        fillable,
        rotation_info,
        flip_info,
        dropcap_style,
    )
}

/// The HWPX `imgBrush@mode` values Core's [`ImageFillMode`] carries, and
/// back. The other twelve modes (`TILE_HORZ_TOP` …) have no Core value.
pub(crate) const IMAGE_FILL_MODES: [(&str, hwpforge_foundation::ImageFillMode); 4] = [
    ("TILE", hwpforge_foundation::ImageFillMode::Tile),
    ("CENTER", hwpforge_foundation::ImageFillMode::Center),
    ("TOTAL", hwpforge_foundation::ImageFillMode::Stretch),
    ("ZOOM", hwpforge_foundation::ImageFillMode::FitAll),
];

/// Why a shape's picture fill cannot be carried exactly, if it cannot: a
/// missing image reference, a mode Core has no value for, or brightness,
/// contrast, transparency or an effect other than `REAL_PIC`, which Core
/// `Fill::Image` has no fields for.
///
/// 이것을 실패시키는 것: 조건 하나를 지우기(`picture_fill_core_cannot_carry_is_reported`).
fn image_fill_loss(fill: &HxFillBrush) -> Option<String> {
    let brush = fill.img_brush.as_ref()?;
    if fill.gradation.is_some() {
        return None;
    }
    let Some(img) = brush.img.as_ref().filter(|i| !i.binary_item_id_ref.is_empty()) else {
        return Some("picture fill without an image reference".into());
    };
    if !IMAGE_FILL_MODES.iter().any(|(wire, _)| *wire == brush.mode) {
        return Some(format!("picture fill mode {} not carried by Core", brush.mode));
    }
    if img.bright != 0 || img.contrast != 0 || img.alpha != 0 || img.effect != "REAL_PIC" {
        return Some(format!(
            "picture fill bright={} contrast={} alpha={} effect={} not carried by Core",
            img.bright, img.contrast, img.alpha, img.effect
        ));
    }
    None
}

/// Extracts a [`ShapeStyle`] from HWPX shape common elements including rotation, flip, arrow,
/// and drop cap info.
///
/// `fillable` = the shape has an interior (everything but lines and connect
/// lines). For such a shape a missing `<hc:fillBrush>` means "no fill" and
/// becomes [`Fill::None`](hwpforge_core::control::Fill::None); leaving it
/// `None` would let the encoder write its default white fill.
pub(crate) fn decode_shape_style_full(
    line_shape: &Option<HxLineShape>,
    fill_brush: &Option<HxFillBrush>,
    fillable: bool,
    rotation_info: Option<&crate::schema::section::HxRotationInfo>,
    flip_info: Option<&crate::schema::section::HxFlip>,
    dropcap_style: &str,
) -> Option<ShapeStyle> {
    use hwpforge_core::control::{ArrowStyle, Fill};
    use hwpforge_foundation::{ArrowSize, ArrowType, GradientType};

    let fill_color: Option<Color> = fill_brush
        .as_ref()
        .and_then(|fb| fb.win_brush.as_ref())
        .map(|wb| &wb.face_color)
        .filter(|c| !c.is_empty() && *c != "none")
        .and_then(|c| parse_hex_color(c));

    // Decode advanced fill (gradient or pattern)
    let fill: Option<Fill> = fill_brush.as_ref().and_then(|fb| {
        // Gradient takes priority (xs:choice — only one child)
        if let Some(g) = fb.gradation.as_ref() {
            let gradient_type = match g.gradation_type.as_str() {
                "RADIAL" => GradientType::Radial,
                "SQUARE" => GradientType::Square,
                "CONICAL" => GradientType::Conical,
                _ => GradientType::Linear,
            };
            return Some(Fill::Gradient {
                gradient_type,
                angle: g.angle,
                colors: g
                    .colors
                    .iter()
                    .map(|c| {
                        let color = parse_hex_color(&c.value).unwrap_or(Color::BLACK);
                        (color, 0)
                    })
                    .collect(),
                // S13: wire 값을 그대로 `Some` — 인코더 기본값과 같아도 접지
                // 않는다(다시 읽은 Core 가 같아야 왕복이 정확하다).
                // 이것을 실패시키는 것: 기본값과 같으면 `None` 으로 접기
                // (`gradient_center_and_steps_round_trip`).
                center_x: Some(g.center_x),
                center_y: Some(g.center_y),
                step: u32::try_from(g.step).ok(),
                step_center: u32::try_from(g.step_center).ok(),
            });
        }
        // Picture fill: the image a Core `Fill::Image` names the way a
        // picture names its image (`BinData/{binaryItemIDRef}`). What Core
        // cannot hold is reported by `decode_shape_style`.
        if let Some(img) = fb
            .img_brush
            .as_ref()
            .and_then(|b| b.img.as_ref().map(|img| (b, img)))
            .filter(|(_, img)| !img.binary_item_id_ref.is_empty())
        {
            let (brush, img) = img;
            let mode = IMAGE_FILL_MODES
                .iter()
                .find(|(wire, _)| *wire == brush.mode)
                .map_or(hwpforge_foundation::ImageFillMode::Tile, |(_, mode)| *mode);
            return Some(Fill::Image {
                image_id: format!("BinData/{}", img.binary_item_id_ref),
                mode,
            });
        }
        // Pattern fill: winBrush with hatchStyle present
        if let Some(wb) = fb.win_brush.as_ref() {
            if let Some(ref hs) = wb.hatch_style {
                let pattern_type = hs.parse::<PatternType>().unwrap_or(PatternType::Horizontal);
                let fg = parse_hex_color(&wb.hatch_color).unwrap_or(Color::BLACK);
                let bg = parse_hex_color(&wb.face_color).unwrap_or(Color::WHITE);
                return Some(Fill::Pattern { pattern_type, fg_color: fg, bg_color: bg });
            }
        }
        None
    });
    // 이것을 실패시키는 것: 이 대입을 지우기 — 채우기 없는 도형이 Core 에서
    // "인코더 기본값"과 구별되지 않는다 (`no_fill_shape_decodes_as_fill_none`).
    let fill = if fillable && fill_brush.is_none() { Some(Fill::None) } else { fill };

    let (line_color, line_width, line_style) = match line_shape.as_ref() {
        None => (None, None, None),
        Some(ls) => (
            if ls.color.is_empty() { None } else { parse_hex_color(&ls.color) },
            // 굵기 0 도 실제 값이다 — `None` 으로 받으면 인코더가 기본 굵기 33 을
            // 쓴다. 속성이 없을 때(`LINE_WIDTH_ABSENT`, 음수)만 `None`.
            // 이것을 실패시키는 것: 0 을 `None` 으로 되돌리기
            // (`zero_line_width_decodes_as_zero`, `tests/shape_no_fill.rs`).
            u32::try_from(ls.width).ok(),
            if ls.style.is_empty() {
                None
            } else {
                ls.style.parse::<hwpforge_core::control::LineStyle>().ok()
            },
        ),
    };

    // Decode rotation (HWPX stores angle in integer degrees)
    let rotation: Option<f32> = rotation_info.filter(|ri| ri.angle != 0).map(|ri| ri.angle as f32);

    // Decode flip
    let flip: Option<Flip> = flip_info.and_then(|fi| match (fi.horizontal, fi.vertical) {
        (0, 0) => None,
        (1, 0) => Some(Flip::Horizontal),
        (0, 1) => Some(Flip::Vertical),
        (1, 1) => Some(Flip::Both),
        _ => None,
    });

    // Decode arrows from line_shape
    let (head_arrow, tail_arrow) = match line_shape.as_ref() {
        None => (None, None),
        Some(ls) => {
            let head = if ls.head_style != "NORMAL" && !ls.head_style.is_empty() {
                Some(ArrowStyle {
                    arrow_type: ls.head_style.parse::<ArrowType>().unwrap_or(ArrowType::None),
                    size: ls.head_sz.parse::<ArrowSize>().unwrap_or(ArrowSize::Medium),
                    filled: ls.head_fill != 0,
                })
            } else {
                None
            };
            let tail = if ls.tail_style != "NORMAL" && !ls.tail_style.is_empty() {
                Some(ArrowStyle {
                    arrow_type: ls.tail_style.parse::<ArrowType>().unwrap_or(ArrowType::None),
                    size: ls.tail_sz.parse::<ArrowSize>().unwrap_or(ArrowSize::Medium),
                    filled: ls.tail_fill != 0,
                })
            } else {
                None
            };
            (head, tail)
        }
    };

    let drop_cap = DropCapStyle::from_hwpx_str(dropcap_style);

    let has_anything = line_color.is_some()
        || line_width.is_some()
        || line_style.is_some()
        || fill_color.is_some()
        || fill.is_some()
        || rotation.is_some()
        || flip.is_some()
        || head_arrow.is_some()
        || tail_arrow.is_some()
        || drop_cap != DropCapStyle::None;

    if !has_anything {
        return None;
    }

    Some(ShapeStyle {
        line_color,
        fill_color,
        line_width,
        line_style,
        rotation,
        flip,
        head_arrow,
        tail_arrow,
        fill,
        drop_cap_style: drop_cap,
    })
}

/// Decodes an `HxEllipse` with `hasArcPr=1` into a Core `Run` with `Control::Arc`.
pub(crate) fn decode_arc(
    ellipse: &HxEllipse,
    char_shape_id: CharShapeIndex,
    _depth: usize,
    ctx: &mut DecodeCtx,
) -> HwpxResult<Run> {
    use hwpforge_core::control::ShapePoint;

    let arc_type = ellipse.arc_type.parse::<ArcType>().unwrap_or(ArcType::Normal);

    let center =
        ellipse.center.as_ref().map(|p| ShapePoint::new(p.x, p.y)).unwrap_or(ShapePoint::new(0, 0));
    let axis1 =
        ellipse.ax1.as_ref().map(|p| ShapePoint::new(p.x, p.y)).unwrap_or(ShapePoint::new(0, 0));
    let axis2 =
        ellipse.ax2.as_ref().map(|p| ShapePoint::new(p.x, p.y)).unwrap_or(ShapePoint::new(0, 0));
    let start1 =
        ellipse.start1.as_ref().map(|p| ShapePoint::new(p.x, p.y)).unwrap_or(ShapePoint::new(0, 0));
    let end1 =
        ellipse.end1.as_ref().map(|p| ShapePoint::new(p.x, p.y)).unwrap_or(ShapePoint::new(0, 0));
    let start2 =
        ellipse.start2.as_ref().map(|p| ShapePoint::new(p.x, p.y)).unwrap_or(ShapePoint::new(0, 0));
    let end2 =
        ellipse.end2.as_ref().map(|p| ShapePoint::new(p.x, p.y)).unwrap_or(ShapePoint::new(0, 0));

    let (width, height) = ellipse
        .sz
        .as_ref()
        .map(|sz| {
            (
                HwpUnit::new(sz.width).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(sz.height).unwrap_or(HwpUnit::ZERO),
            )
        })
        .unwrap_or((HwpUnit::ZERO, HwpUnit::ZERO));

    let placement =
        decode_object_placement(ellipse.pos.as_ref(), &ellipse.text_wrap, &ellipse.text_flow);
    let caption =
        ellipse.caption.as_ref().map(|c| convert_hx_caption(c, _depth, ctx)).transpose()?;

    Ok(Run {
        content: RunContent::Control(Box::new(Control::Arc {
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
            style: decode_shape_style(
                &ellipse.line_shape,
                &ellipse.fill_brush,
                true,
                ellipse.rotation_info.as_ref(),
                ellipse.flip.as_ref(),
                &ellipse.dropcap_style,
                ctx,
            ),
        })),
        char_shape_id,
    })
}

/// Decodes an `HxCurve` into a Core `Run` with `Control::Curve`.
pub(crate) fn decode_curve(
    curve: &HxCurve,
    char_shape_id: CharShapeIndex,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> HwpxResult<Run> {
    use hwpforge_core::control::ShapePoint;

    // KS X 6101: coordinates are in <hp:seg> x1/y1/x2/y2, not separate <hc:pt>.
    // Reconstruct points from segments if the points array is empty.
    let points: Vec<ShapePoint> = if !curve.points.is_empty() {
        curve.points.iter().map(|p| ShapePoint::new(p.x, p.y)).collect()
    } else if !curve.segments.is_empty() {
        let mut pts = Vec::with_capacity(curve.segments.len() + 1);
        pts.push(ShapePoint::new(curve.segments[0].x1, curve.segments[0].y1));
        for seg in &curve.segments {
            pts.push(ShapePoint::new(seg.x2, seg.y2));
        }
        pts
    } else {
        vec![]
    };

    let segment_types: Vec<CurveSegmentType> = curve
        .segments
        .iter()
        .map(|s| s.seg_type.parse::<CurveSegmentType>().unwrap_or(CurveSegmentType::Curve))
        .collect();

    let (width, height) = curve
        .sz
        .as_ref()
        .map(|sz| {
            (
                HwpUnit::new(sz.width).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(sz.height).unwrap_or(HwpUnit::ZERO),
            )
        })
        .unwrap_or((HwpUnit::ZERO, HwpUnit::ZERO));

    let placement = decode_object_placement(curve.pos.as_ref(), &curve.text_wrap, &curve.text_flow);
    let caption = curve.caption.as_ref().map(|c| convert_hx_caption(c, depth, ctx)).transpose()?;

    Ok(Run {
        content: RunContent::Control(Box::new(Control::Curve {
            points,
            segment_types,
            width,
            height,
            placement,
            caption,
            style: decode_shape_style(
                &curve.line_shape,
                &curve.fill_brush,
                true,
                curve.rotation_info.as_ref(),
                curve.flip.as_ref(),
                &curve.dropcap_style,
                ctx,
            ),
        })),
        char_shape_id,
    })
}

/// Decodes an `HxTextArt` (`<hp:textart>`) into a Core `Run` with
/// `Control::TextArt`. Round-trip mirror of the encoder's raw-XML emission:
/// geometry from `<hp:offset>`/`<hp:sz>` (a group child, which has no `sz`,
/// is sized later from its matrix chain), typography from `<hp:textartPr>`,
/// the colour from the fill brush ([`textart_fill_color`]).
pub(crate) fn decode_textart(
    text_art: &HxTextArt,
    char_shape_id: CharShapeIndex,
    _depth: usize,
    ctx: &mut DecodeCtx,
) -> HwpxResult<Run> {
    let (width, height) = text_art
        .sz
        .as_ref()
        .map(|sz| {
            (
                HwpUnit::new(sz.width).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(sz.height).unwrap_or(HwpUnit::ZERO),
            )
        })
        .unwrap_or((HwpUnit::ZERO, HwpUnit::ZERO));
    // TextArt anchors via `<hp:offset>` (not `<hp:pos>`); the encoder reads
    // only the placement offsets back. A zero offset collapses to `None`.
    let (ox, oy) = text_art.offset.as_ref().map(|o| (o.x, o.y)).unwrap_or((0, 0));
    let placement = {
        let candidate = ObjectPlacement {
            horz_offset: HwpUnit::new(ox).unwrap_or(HwpUnit::ZERO),
            vert_offset: HwpUnit::new(oy).unwrap_or(HwpUnit::ZERO),
            ..ObjectPlacement::legacy_inline_defaults()
        };
        (candidate != ObjectPlacement::legacy_inline_defaults()).then_some(candidate)
    };
    let inst_id =
        text_art.instid.parse::<u64>().ok().filter(|v| *v != 0).map(hwpforge_core::ObjectId::new);
    let pr = text_art.textart_pr.clone().unwrap_or_default();
    let line_spacing = u32::try_from(pr.line_spacing).unwrap_or(0);
    let char_spacing = u32::try_from(pr.char_spacing).unwrap_or(0);
    let shape = if pr.text_shape.is_empty() { "RECTANGLE".to_string() } else { pr.text_shape };
    let align = if pr.align.is_empty() { "LEFT".to_string() } else { pr.align };

    Ok(Run {
        content: RunContent::Control(Box::new(Control::TextArt {
            text: textart_text_from_wire(&text_art.text),
            shape,
            font_name: pr.font_name,
            font_style: pr.font_style,
            align,
            line_spacing,
            char_spacing,
            width,
            height,
            placement,
            fill_color: {
                report_textart_style(text_art, ctx);
                textart_fill_color(text_art.fill_brush.as_ref(), ctx)
            },
            inst_id,
        })),
        char_shape_id,
    })
}

/// Reports the TextArt styling Core `TextArt` has no field for: a visible
/// outline (`<hp:lineShape>` whose style is not `NONE`) as `textart_outline`,
/// a font type other than `TTF` (`HTF`) as `textart_font_type`, and a shape
/// or text shadow (`<hp:shadow>` whose type is not `NONE`) as
/// `textart_shadow`. The encoder writes neither, so without these warnings
/// they would be lost silently. An absent element (or attribute) is taken
/// as the default — there is nothing on the wire to lose.
fn report_textart_style(text_art: &HxTextArt, ctx: &mut DecodeCtx) {
    let shown = |value: &str| !value.is_empty() && !value.eq_ignore_ascii_case("NONE");
    if let Some(line) = text_art.line_shape.as_ref().filter(|l| shown(&l.style)) {
        ctx.content_lost(
            "textart_outline",
            format!(
                "outline {} {} width {} not carried by Core TextArt",
                line.style, line.color, line.width
            ),
        );
    }
    if let Some(pr) =
        text_art.textart_pr.as_ref().filter(|p| !p.font_type.eq_ignore_ascii_case("TTF"))
    {
        ctx.content_lost(
            "textart_font_type",
            format!("font type {} not carried by Core TextArt", pr.font_type),
        );
    }
    let shadows = [
        ("shadow", text_art.shadow.as_ref()),
        ("text shadow", text_art.textart_pr.as_ref().and_then(|p| p.shadow.as_ref())),
    ];
    for (what, shadow) in shadows {
        if let Some(shadow) = shadow.filter(|s| shown(&s.shadow_type)) {
            ctx.content_lost(
                "textart_shadow",
                format!(
                    "{what} {} {} not carried by Core TextArt",
                    shadow.shadow_type, shadow.color
                ),
            );
        }
    }
}

/// The colour Core `TextArt.fill_color` can carry: a `winBrush` with a valid
/// `faceColor`, no hatch and no transparency, and no gradation or picture
/// beside it. Any other fill is reported as
/// [`ContentLost`](super::DecodeWarning::ContentLost) (`textart_fill`) and
/// left to the encoder's default, since Core has nowhere to keep it.
fn textart_fill_color(
    fill: Option<&HxFillBrush>,
    ctx: &mut DecodeCtx,
) -> Option<hwpforge_foundation::Color> {
    let lost = |ctx: &mut DecodeCtx, what: &str| {
        ctx.content_lost("textart_fill", format!("fill: {what} not carried by Core TextArt"));
        None
    };
    let Some(fill) = fill else { return lost(ctx, "missing fill brush") };
    if fill.gradation.is_some() {
        return lost(ctx, "gradation");
    }
    if fill.img_brush.is_some() {
        return lost(ctx, "picture fill");
    }
    let Some(brush) = fill.win_brush.as_ref() else { return lost(ctx, "empty fill brush") };
    if brush.hatch_style.is_some() {
        return lost(ctx, "pattern");
    }
    if brush.alpha != 0 {
        return lost(ctx, "transparency");
    }
    match crate::color::parse_hex_color_raw(&brush.face_color) {
        Some(color) => Some(color),
        None => lost(ctx, &format!("face colour {:?}", brush.face_color)),
    }
}

/// Decodes an `HxConnectLine` into a Core `Run` with `Control::ConnectLine`.
pub(crate) fn decode_connect_line(
    cl: &HxConnectLine,
    char_shape_id: CharShapeIndex,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> HwpxResult<Run> {
    use hwpforge_core::control::ShapePoint;

    let start =
        cl.start_pt.as_ref().map(|p| ShapePoint::new(p.x, p.y)).unwrap_or(ShapePoint::new(0, 0));
    let end =
        cl.end_pt.as_ref().map(|p| ShapePoint::new(p.x, p.y)).unwrap_or(ShapePoint::new(0, 0));
    // controlPoints wrapper contains all points (start type=3, intermediates type=2, end type=26).
    // Extract only intermediate points (skip first=start and last=end).
    let control_points: Vec<ShapePoint> = cl
        .control_points
        .as_ref()
        .map(|cp| {
            let pts = &cp.points;
            if pts.len() > 2 {
                pts[1..pts.len() - 1].iter().map(|p| ShapePoint::new(p.x, p.y)).collect()
            } else {
                vec![]
            }
        })
        .unwrap_or_default();

    let (width, height) = cl
        .sz
        .as_ref()
        .map(|sz| {
            (
                HwpUnit::new(sz.width).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(sz.height).unwrap_or(HwpUnit::ZERO),
            )
        })
        .unwrap_or((HwpUnit::ZERO, HwpUnit::ZERO));

    let placement = decode_object_placement(cl.pos.as_ref(), &cl.text_wrap, &cl.text_flow);
    let caption = cl.caption.as_ref().map(|c| convert_hx_caption(c, depth, ctx)).transpose()?;

    Ok(Run {
        content: RunContent::Control(Box::new(Control::ConnectLine {
            start,
            end,
            control_points,
            connect_type: cl.connect_type.clone(),
            width,
            height,
            placement,
            caption,
            style: decode_shape_style(
                &cl.line_shape,
                &cl.fill_brush,
                false,
                cl.rotation_info.as_ref(),
                cl.flip.as_ref(),
                &cl.dropcap_style,
                ctx,
            ),
        })),
        char_shape_id,
    })
}

/// Decodes a top-level `<hp:container>` (group / 묶음 객체) into a Core `Run`
/// carrying `Control::Group`.
///
/// # Geometry
///
/// The Core group is an unrotated box of the container's displayed size
/// (`<hp:sz>`), anchored by `<hp:pos>` with the container's
/// `textWrap`/`textFlow`. Its origin is the corner of the box the container's
/// own matrix chain makes of `<hp:orgSz>`. Children are placed relative to
/// that origin; each child's matrix chain (see
/// [`group_geometry`](super::group_geometry)) is folded into the child's own
/// Core geometry, the way Hancom's "ungroup" does:
///
/// - lines, polygons, curves and connectors without text, caption or
///   directional fill take the whole map onto their points;
/// - boxes (rect/text box, ellipse, arc) and point shapes that do carry text,
///   a caption or a directional fill are scaled along their own axes and get
///   the rotation and horizontal mirror as `ShapeStyle.rotation`/`flip`;
/// - a TextArt is scaled the same way from its `<hp:orgSz>`; it has no
///   rotation or mirror field, so a turned or mirrored one is reported.
///
/// Each child is then checked: the matrix the encoder would write from the
/// decoded Core value must put every defining point within
/// [`EXACT_TOLERANCE`](super::group_geometry::EXACT_TOLERANCE) of where the
/// wire put it. Whatever cannot be carried exactly — a shear, an axis of
/// size zero, a residual, a displayed size that does not match the
/// container's own matrices, a nested group whose box disagrees with its
/// shapes, and every child element Core has no group child for (`pic`,
/// `ole`, `equation`, `switch`, …) — is reported as
/// [`DecodeWarning::ContentLost`](super::DecodeWarning::ContentLost). Group
/// child paths use the child's position among the container's child
/// elements in wire order.
///
/// Returns `None` when no child can be represented (the group is then
/// reported as lost too).
///
/// # Errors
///
/// Returns [`HwpxError::InvalidStructure`](crate::HwpxError) when group nesting reaches
/// `MAX_NESTING_DEPTH`, like nested tables. Returning `None` there would
/// cascade up and silently drop the whole outermost group.
pub(crate) fn decode_container(
    container: &crate::schema::section::HxContainer,
    char_shape_id: CharShapeIndex,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> HwpxResult<Option<Run>> {
    use super::group_geometry::{bounds, corners, wire_chain, Affine, EXACT_TOLERANCE};

    let (org_w, org_h) = container_org_size(container);
    let own = match container.rendering_info.as_ref() {
        None => Affine::IDENTITY,
        Some(ri) => wire_chain(ri).unwrap_or_else(|| {
            // The group's own frame is unknown; the stored file is not
            // what this decoder reads it as.
            ctx.content_lost("group_transform", "unparseable group transform matrix");
            Affine::IDENTITY
        }),
    };
    // `<hp:pos>` places the origin of the outermost space: Hancom's ungroup
    // puts every shape at `pos` + its chained coordinates. In rotflip-group
    // (turned 20° and mirrored) the group's own frame starts at
    // (−1680, −3359) and its shapes at (−1679, 0), yet each ungrouped shape
    // sits exactly at `pos` + its chain. The displayed size `<hp:sz>` is the
    // extent of the shapes (there 39231 × 15541), not of the turned frame
    // (40910 × 23514).
    let origin = (0.0, 0.0);
    let own_box: Vec<(f64, f64)> = corners(org_w, org_h).iter().map(|&p| own.apply(p)).collect();
    let (min_x, min_y, max_x, max_y) =
        leaf_bounds(container).or_else(|| bounds(&own_box)).unwrap_or((0.0, 0.0, org_w, org_h));
    // `sz` is either the shapes' own extent (what Hancom writes) or a box
    // from the origin that holds them (what this crate's writer gives a
    // group whose point shapes sit inside their old boxes).
    let extent = (max_x - min_x, max_y - min_y);
    let (width, height) = match container.sz.as_ref() {
        Some(sz) => (sz.width, sz.height),
        None => (round_i32(max_x - min_x.min(0.0)), round_i32(max_y - min_y.min(0.0))),
    };
    let (w, h) = (f64::from(width), f64::from(height));
    let extent_error = (extent.0 - w).abs().max((extent.1 - h).abs());
    let overflow = (-min_x).max(-min_y).max(max_x - w).max(max_y - h).max(0.0);
    let size_error = extent_error.min(overflow);
    if !fits_hwpunit(&[width, height]) {
        ctx.content_lost("group_display_size", "displayed size outside the HWPUNIT range");
    } else if size_error > EXACT_TOLERANCE {
        ctx.content_lost(
            "group_display_size",
            format!(
                "displayed size {width}x{height} differs from the bounds of its shapes by \
                 {size_error:.2}"
            ),
        );
    }

    let Some(children) = decode_group_children(container, origin, char_shape_id, depth, ctx)?
    else {
        return Ok(None);
    };

    let placement =
        decode_object_placement(container.pos.as_ref(), &container.text_wrap, &container.text_flow);
    Ok(Some(Run {
        content: RunContent::Control(Box::new(Control::Group {
            children,
            width: HwpUnit::new(width).unwrap_or(HwpUnit::ZERO),
            height: HwpUnit::new(height).unwrap_or(HwpUnit::ZERO),
            placement,
            inst_id: container_inst_id(container),
        })),
        char_shape_id,
    }))
}

fn container_org_size(container: &crate::schema::section::HxContainer) -> (f64, f64) {
    container
        .org_sz
        .as_ref()
        .or(container.cur_sz.as_ref())
        .map(|s| (f64::from(s.width), f64::from(s.height)))
        .unwrap_or_else(|| {
            container.sz.as_ref().map_or((0.0, 0.0), |s| (f64::from(s.width), f64::from(s.height)))
        })
}

fn container_inst_id(
    container: &crate::schema::section::HxContainer,
) -> Option<hwpforge_core::ObjectId> {
    container.instid.parse::<u64>().ok().filter(|&v| v != 0).map(hwpforge_core::ObjectId::new)
}

fn round_i32(v: f64) -> i32 {
    // Saturating float→int cast; inputs are HWPUNIT-sized.
    v.round() as i32
}

/// Placement of a group child at `(x, y)` inside its parent group.
///
/// Same convention as the HWP5 projection of group children
/// (`offset_placement`): the origin stays `None` (inline defaults), any other
/// offset floats, so HWPX- and HWP5-sourced groups carry identical Core
/// values and the encoder reads the offset back from the placement.
fn group_child_placement(x: i32, y: i32) -> Option<ObjectPlacement> {
    if x == 0 && y == 0 {
        return None;
    }
    Some(ObjectPlacement {
        text_wrap: ObjectTextWrap::InFrontOfText,
        text_flow: ObjectTextFlow::BothSides,
        treat_as_char: false,
        flow_with_text: false,
        allow_overlap: true,
        vert_rel_to: ObjectRelativeTo::Paper,
        horz_rel_to: ObjectRelativeTo::Paper,
        vert_offset: HwpUnit::new(y).unwrap_or(HwpUnit::ZERO),
        horz_offset: HwpUnit::new(x).unwrap_or(HwpUnit::ZERO),
    })
}

/// A child shape's wire map: its matrix chain, or `<hp:offset>` when the
/// shape has no `<hp:renderingInfo>`. `None` when a matrix does not parse.
fn child_chain(
    rendering_info: Option<&crate::schema::section::HxRenderingInfo>,
    offset: Option<&crate::schema::section::HxOffset>,
) -> Option<super::group_geometry::Affine> {
    use super::group_geometry::{wire_chain, Affine};
    match rendering_info {
        Some(ri) => wire_chain(ri),
        None => {
            let (x, y) = offset.map_or((0, 0), |o| (o.x, o.y));
            Some(Affine::translate(f64::from(x), f64::from(y)))
        }
    }
}

fn org_size(
    org: Option<&crate::schema::section::HxSizeAttr>,
    cur: Option<&crate::schema::section::HxSizeAttr>,
) -> (f64, f64) {
    org.or(cur).map_or((0.0, 0.0), |s| (f64::from(s.width), f64::from(s.height)))
}

/// The wire map and box of one supported child, plus its decoded Core
/// control (geometry still wire-local).
struct WireChild {
    control: Control,
    map: Option<super::group_geometry::Affine>,
    org: (f64, f64),
    /// The wire shape has a picture fill (`<hc:imgBrush>`), which Core
    /// cannot carry.
    picture_fill: bool,
}

fn has_picture_fill(fill: Option<&HxFillBrush>) -> bool {
    fill.is_some_and(|f| f.img_brush.is_some())
}

/// Decodes the children of `container` in wire order, folding each child's
/// map into Core geometry relative to `origin` (in the outermost group's
/// space). `None` when nothing could be represented.
fn decode_group_children(
    container: &crate::schema::section::HxContainer,
    origin: (f64, f64),
    char_shape_id: CharShapeIndex,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> HwpxResult<Option<Vec<GroupChild>>> {
    use crate::schema::section::HxContainerChildKind as K;

    // Bound nested-container recursion (group-in-group) against pathological
    // depth — same cap and error as table nesting (`convert_table`).
    if depth >= crate::decoder::section::MAX_NESTING_DEPTH {
        return Err(crate::HwpxError::InvalidStructure {
            detail: format!(
                "group nesting depth {} exceeds limit of {}",
                depth,
                crate::decoder::section::MAX_NESTING_DEPTH,
            ),
        });
    }

    let mut children: Vec<GroupChild> = Vec::new();
    for (wire_idx, &(kind, idx)) in container.child_order.iter().enumerate() {
        ctx.enter(super::PathSeg::GroupChild(wire_idx));
        ctx.enter_group_child();
        let result = decode_group_child(container, kind, idx, origin, char_shape_id, depth, ctx);
        ctx.leave_group_child();
        let decoded = match result {
            Ok(decoded) => decoded,
            Err(e) => {
                ctx.leave();
                return Err(e);
            }
        };
        if let Some(child) = decoded {
            children.push(child);
        } else if let K::Unsupported(name) = kind {
            let reason = match name {
                "caption" | "parameterset" => "group-level element not carried by Core",
                _ => "unsupported group child",
            };
            ctx.content_lost(name, reason);
        }
        ctx.leave();
    }

    if !container.href.is_empty() {
        // Core groups have no link; Hancom writes the group's own link here.
        ctx.content_lost("group_hyperlink", format!("group hyperlink {:?}", container.href));
    }
    if children.is_empty() {
        ctx.content_lost("group", "no group child can be represented in Core");
        return Ok(None);
    }
    Ok(Some(children))
}

/// Decodes one child of a group. `Ok(None)` = not represented (unsupported
/// kind, or dropped with its own `ContentLost` already reported).
fn decode_group_child(
    container: &crate::schema::section::HxContainer,
    kind: crate::schema::section::HxContainerChildKind,
    idx: usize,
    origin: (f64, f64),
    char_shape_id: CharShapeIndex,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> HwpxResult<Option<GroupChild>> {
    use crate::schema::section::HxContainerChildKind as K;

    let take = |run: Option<Run>| match run.map(|r| r.content) {
        Some(RunContent::Control(boxed)) => Some(*boxed),
        _ => None,
    };
    let wire = match kind {
        K::Rect => {
            let r = &container.rects[idx];
            take(decode_textbox(r, char_shape_id, depth, ctx)?).map(|control| WireChild {
                control,
                map: child_chain(r.rendering_info.as_ref(), r.offset.as_ref()),
                org: org_size(r.org_sz.as_ref(), r.cur_sz.as_ref()),
                picture_fill: has_picture_fill(r.fill_brush.as_ref()),
            })
        }
        K::Line => {
            let l = &container.lines[idx];
            take(Some(decode_line(l, char_shape_id, depth, ctx)?)).map(|control| WireChild {
                control,
                map: child_chain(l.rendering_info.as_ref(), l.offset.as_ref()),
                org: org_size(l.org_sz.as_ref(), l.cur_sz.as_ref()),
                picture_fill: has_picture_fill(l.fill_brush.as_ref()),
            })
        }
        K::Ellipse => {
            let e = &container.ellipses[idx];
            let run = if e.has_arc_pr == 1 {
                decode_arc(e, char_shape_id, depth, ctx)?
            } else {
                decode_ellipse(e, char_shape_id, depth, ctx)?
            };
            take(Some(run)).map(|control| WireChild {
                control,
                map: child_chain(e.rendering_info.as_ref(), e.offset.as_ref()),
                org: org_size(e.org_sz.as_ref(), e.cur_sz.as_ref()),
                picture_fill: has_picture_fill(e.fill_brush.as_ref()),
            })
        }
        K::Polygon => {
            let p = &container.polygons[idx];
            take(Some(decode_polygon(p, char_shape_id, depth, ctx)?)).map(|control| WireChild {
                control,
                map: child_chain(p.rendering_info.as_ref(), p.offset.as_ref()),
                org: org_size(p.org_sz.as_ref(), p.cur_sz.as_ref()),
                picture_fill: has_picture_fill(p.fill_brush.as_ref()),
            })
        }
        K::Curve => {
            let c = &container.curves[idx];
            take(Some(decode_curve(c, char_shape_id, depth, ctx)?)).map(|control| WireChild {
                control,
                map: child_chain(c.rendering_info.as_ref(), c.offset.as_ref()),
                org: org_size(c.org_sz.as_ref(), c.cur_sz.as_ref()),
                picture_fill: has_picture_fill(c.fill_brush.as_ref()),
            })
        }
        K::ConnectLine => {
            let c = &container.connect_lines[idx];
            take(Some(decode_connect_line(c, char_shape_id, depth, ctx)?)).map(|control| {
                WireChild {
                    control,
                    map: child_chain(c.rendering_info.as_ref(), c.offset.as_ref()),
                    org: org_size(c.org_sz.as_ref(), c.cur_sz.as_ref()),
                    picture_fill: has_picture_fill(c.fill_brush.as_ref()),
                }
            })
        }
        K::TextArt => {
            let t = &container.textarts[idx];
            take(Some(decode_textart(t, char_shape_id, depth, ctx)?)).map(|control| WireChild {
                control,
                map: child_chain(t.rendering_info.as_ref(), t.offset.as_ref()),
                org: org_size(t.org_sz.as_ref(), t.cur_sz.as_ref()),
                // Its fill is judged (and reported) by `decode_textart`.
                picture_fill: false,
            })
        }
        K::Picture => {
            return decode_group_picture(&container.pics[idx], origin, depth, ctx);
        }
        K::Container => {
            return Ok(decode_nested_container(
                &container.containers[idx],
                origin,
                char_shape_id,
                depth + 1,
                ctx,
            )?
            .map(GroupChild::Shape));
        }
        K::Unsupported(_) => return Ok(None),
    };
    let Some(wire) = wire else { return Ok(None) };
    Ok(normalize_group_child(wire, origin, ctx).map(GroupChild::Shape))
}

/// A picture in a group: its box folded like a shape's
/// ([`fold_box`](hwpforge_core::control::geometry::fold_box)), kept as an
/// unturned [`GroupChild::Picture`]. What a Core picture cannot show is
/// reported as `group_child_picture`: a turn or mirror (the picture is kept
/// unturned around the same centre), a crop, an image effect, a border.
/// `None` = dropped (its loss already reported).
fn decode_group_picture(
    pic: &crate::schema::section::HxPic,
    origin: (f64, f64),
    depth: usize,
    ctx: &mut DecodeCtx,
) -> HwpxResult<Option<GroupChild>> {
    use hwpforge_core::control::geometry::{fold_box, FoldInput, FoldLossKind};

    let Some(mut image) = super::section::convert_picture(pic, depth, ctx)? else {
        ctx.content_lost("pic", "picture without an image reference");
        return Ok(None);
    };
    let Some(map) = child_chain(pic.rendering_info.as_ref(), pic.offset.as_ref()) else {
        ctx.content_lost("group_child_transform", "unparseable transform matrix");
        return Ok(None);
    };
    let org = org_size(pic.org_sz.as_ref(), pic.cur_sz.as_ref());
    let kind_of = |kind: FoldLossKind| match kind {
        FoldLossKind::Degenerate => "group_child_degenerate",
        _ => "group_child_transform",
    };
    let fold = match fold_box(org.0, org.1, &FoldInput::new(map.to_transform(), org, origin)) {
        Ok(fold) => fold,
        Err(loss) => {
            ctx.content_lost(kind_of(loss.kind()), loss.detail().to_string());
            return Ok(None);
        }
    };
    for loss in fold.losses() {
        ctx.content_lost(kind_of(loss.kind()), loss.detail().to_string());
    }
    if fold.rotation().is_some() || fold.flip().is_some() {
        ctx.content_lost(
            "group_child_picture",
            "rotation/flip: a Core picture cannot turn or mirror; kept unturned around the \
             same centre",
        );
    }
    for reason in group_picture_losses(pic, &ctx.image_pixels) {
        ctx.content_lost("group_child_picture", reason);
    }
    image.width = HwpUnit::new(fold.width()).unwrap_or(HwpUnit::ZERO);
    image.height = HwpUnit::new(fold.height()).unwrap_or(HwpUnit::ZERO);
    image.placement = group_child_placement(fold.offset().0, fold.offset().1);
    Ok(Some(GroupChild::Picture(image)))
}

/// HWPUNIT per image pixel (96 dpi): the unit of `<hp:imgDim>`.
const HWPUNIT_PER_PIXEL: i64 = 75;

/// What a group picture shows that a Core picture cannot: a crop, an image
/// effect (`<hc:img>` effect, brightness, contrast, alpha, or anything under
/// `<hp:effects>`), a visible border (`<hp:lineShape>`).
///
/// A picture is cropped when its `<hp:imgClip>` is more than one pixel off
/// the whole image: `<hp:imgDim>`, or, when that is missing or zero, the
/// image's own pixel size × 75 (`image_pixels`). Without either the crop
/// cannot be ruled out and is reported.
fn group_picture_losses(
    pic: &crate::schema::section::HxPic,
    image_pixels: &std::collections::HashMap<String, (u32, u32)>,
) -> Vec<String> {
    let mut out = Vec::new();
    let reference = pic.img.as_ref().map_or("", |i| i.binary_item_id_ref.as_str());
    if let Some(clip) = pic.img_clip.as_ref() {
        // In i64: the wire values are any i32, and `right − width` of two
        // i32 overflows.
        let whole = match pic.img_dim.as_ref() {
            Some(d) if d.dim_width > 0 && d.dim_height > 0 => {
                Some((i64::from(d.dim_width), i64::from(d.dim_height)))
            }
            _ => image_pixels.get(reference).map(|&(w, h)| {
                (i64::from(w) * HWPUNIT_PER_PIXEL, i64::from(h) * HWPUNIT_PER_PIXEL)
            }),
        };
        match whole {
            Some((w, h)) => {
                let [left, top, right, bottom] =
                    [clip.left, clip.top, clip.right, clip.bottom].map(i64::from);
                let off =
                    [left, top, right - w, bottom - h].iter().map(|v| v.abs()).max().unwrap_or(0);
                if off > HWPUNIT_PER_PIXEL {
                    out.push(format!(
                        "crop: imgClip ({}, {}, {}, {}) of a {w}x{h} image",
                        clip.left, clip.top, clip.right, clip.bottom
                    ));
                }
            }
            None => out.push(
                "crop: the image's size is unknown, so a crop cannot be ruled out".to_string(),
            ),
        }
    }
    if let Some(img) = pic.img.as_ref() {
        let plain_effect = img.effect.is_empty() || img.effect == "REAL_PIC";
        let opaque = img.alpha.is_empty() || img.alpha == "0";
        if !plain_effect || img.bright != 0 || img.contrast != 0 || !opaque {
            out.push(format!(
                "effect: {} (brightness {}, contrast {}, alpha {})",
                img.effect, img.bright, img.contrast, img.alpha
            ));
        }
    }
    if pic.effects.as_ref().is_some_and(|e| !e.items.is_empty()) {
        out.push("effect: shadow, glow, soft edge or reflection".to_string());
    }
    if let Some(line) = pic.line_shape.as_ref().filter(|l| l.style != "NONE") {
        out.push(format!("border: {} line {}", line.style, line.width));
    }
    out
}

/// A nested `<hp:container>`: its Core box is the bounds of its shapes in the
/// outermost group's space, placed relative to the parent's `origin`.
fn decode_nested_container(
    nested: &crate::schema::section::HxContainer,
    parent_origin: (f64, f64),
    char_shape_id: CharShapeIndex,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> HwpxResult<Option<Control>> {
    use super::group_geometry::{bounds, corners, wire_chain};

    let leaf_box = leaf_bounds(nested);
    let Some((min_x, min_y, max_x, max_y)) = leaf_box else {
        // No geometry to place: decode the children anyway so unsupported
        // ones are reported, then drop the group.
        let _ = decode_group_children(nested, parent_origin, char_shape_id, depth, ctx)?;
        return Ok(None);
    };
    // The nested group's own matrices should describe the same box. A
    // child Core cannot carry (a picture) still counts in the wire box but
    // not in the shapes' bounds; it is reported on its own, so the boxes
    // are not compared then.
    let (org_w, org_h) = container_org_size(nested);
    match nested.rendering_info.as_ref().and_then(wire_chain) {
        _ if has_unsupported_child(nested) => {}
        Some(own) => {
            let own_box: Vec<(f64, f64)> =
                corners(org_w, org_h).iter().map(|&p| own.apply(p)).collect();
            if let Some((ox0, oy0, ox1, oy1)) = bounds(&own_box) {
                let err = (ox0 - min_x)
                    .abs()
                    .max((oy0 - min_y).abs())
                    .max((ox1 - max_x).abs())
                    .max((oy1 - max_y).abs());
                if err > NESTED_BOX_TOLERANCE {
                    ctx.content_lost(
                        "nested_group_placement",
                        format!("nested group box differs from its shapes by {err:.2}"),
                    );
                }
            }
        }
        None => ctx.content_lost("nested_group_placement", "nested group has no transform"),
    }

    let provisional = (min_x.round(), min_y.round());
    let Some(mut children) = decode_group_children(nested, provisional, char_shape_id, depth, ctx)?
    else {
        return Ok(None);
    };
    // The Core box is the bounds of the decoded children as the encoder will
    // write them, not of the wire boxes: a polygon whose points sit inside
    // its wire box shrinks to its points, and rounding is settled once. A
    // decode of our own output then finds the same box again.
    let Some((bx0, by0, bx1, by1)) =
        bounds(&children.iter().flat_map(core_child_corners).collect::<Vec<_>>())
    else {
        return Ok(None);
    };
    let (dx, dy) = (round_i32(bx0), round_i32(by0));
    if (dx, dy) != (0, 0) {
        for child in &mut children {
            shift_child(child, -dx, -dy);
        }
    }
    let origin = (provisional.0 + f64::from(dx), provisional.1 + f64::from(dy));
    let x = round_i32(origin.0 - parent_origin.0);
    let y = round_i32(origin.1 - parent_origin.1);
    let (w, h) = (round_i32(bx1 - f64::from(dx)), round_i32(by1 - f64::from(dy)));
    if !fits_hwpunit(&[w, h, x, y]) {
        ctx.content_lost("nested_group_placement", "size or offset outside the HWPUNIT range");
        return Ok(None);
    }
    Ok(Some(Control::Group {
        children,
        width: HwpUnit::new(w).unwrap_or(HwpUnit::ZERO),
        height: HwpUnit::new(h).unwrap_or(HwpUnit::ZERO),
        placement: group_child_placement(x, y),
        inst_id: container_inst_id(nested),
    }))
}

/// Corners of a decoded group child's box, relative to its parent group,
/// at its exact Core position (a nested group and a picture are unrotated
/// boxes).
fn core_child_corners(child: &GroupChild) -> Vec<(f64, f64)> {
    use hwpforge_core::control::geometry::{emitted_corners, EmissionPolicy};
    match child {
        GroupChild::Picture(image) => picture_corners(image).to_vec(),
        GroupChild::Shape(control) => {
            emitted_corners(control, &EmissionPolicy::exact()).map_or_else(Vec::new, |c| c.to_vec())
        }
        _ => Vec::new(),
    }
}

/// The four corners of a group picture's (unrotated) box in its parent
/// group.
fn picture_corners(image: &hwpforge_core::image::Image) -> [(f64, f64); 4] {
    let (x, y) = image.placement.as_ref().map_or((0.0, 0.0), |p| {
        (f64::from(p.horz_offset.as_i32()), f64::from(p.vert_offset.as_i32()))
    });
    let (w, h) = (f64::from(image.width.as_i32()), f64::from(image.height.as_i32()));
    [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
}

/// Moves a group child by `(dx, dy)` inside its parent group.
fn shift_child(child: &mut GroupChild, dx: i32, dy: i32) {
    let placement = match child {
        GroupChild::Picture(image) => &mut image.placement,
        GroupChild::Shape(
            Control::TextBox { placement, .. }
            | Control::Rect { placement, .. }
            | Control::Ellipse { placement, .. }
            | Control::Arc { placement, .. }
            | Control::Polygon { placement, .. }
            | Control::Curve { placement, .. }
            | Control::ConnectLine { placement, .. }
            | Control::Line { placement, .. }
            | Control::TextArt { placement, .. }
            | Control::Group { placement, .. },
        ) => placement,
        _ => return,
    };
    let (x, y) =
        placement.as_ref().map_or((0, 0), |p| (p.horz_offset.as_i32(), p.vert_offset.as_i32()));
    *placement = group_child_placement(x + dx, y + dy);
}

/// Whether `container` or a group under it has a child Core cannot carry
/// (an unsupported element, or a picture with no image to show).
fn has_unsupported_child(container: &crate::schema::section::HxContainer) -> bool {
    use crate::schema::section::HxContainerChildKind as K;
    container.child_order.iter().any(|&(kind, idx)| match kind {
        K::Unsupported(_) => true,
        K::Picture => !has_image(&container.pics[idx]),
        _ => false,
    }) || container.containers.iter().any(has_unsupported_child)
}

/// Whether a picture names an image (one that does not is not carried).
fn has_image(pic: &crate::schema::section::HxPic) -> bool {
    pic.img.as_ref().is_some_and(|i| !i.binary_item_id_ref.is_empty())
}

/// How far a nested group's own box may sit from the bounds of its shapes.
const NESTED_BOX_TOLERANCE: f64 = 1.0;

/// Bounds, in the outermost group's space, of every shape box under
/// `container` (nested groups included). `None` when there is no shape or a
/// matrix does not parse.
fn leaf_bounds(container: &crate::schema::section::HxContainer) -> Option<(f64, f64, f64, f64)> {
    use super::group_geometry::{bounds, corners};
    use crate::schema::section::HxContainerChildKind as K;

    let mut pts: Vec<(f64, f64)> = Vec::new();
    for &(kind, idx) in &container.child_order {
        let (map, org) = match kind {
            K::Rect => {
                let s = &container.rects[idx];
                (
                    child_chain(s.rendering_info.as_ref(), s.offset.as_ref()),
                    org_size(s.org_sz.as_ref(), s.cur_sz.as_ref()),
                )
            }
            K::Line => {
                let s = &container.lines[idx];
                (
                    child_chain(s.rendering_info.as_ref(), s.offset.as_ref()),
                    org_size(s.org_sz.as_ref(), s.cur_sz.as_ref()),
                )
            }
            K::Ellipse => {
                let s = &container.ellipses[idx];
                (
                    child_chain(s.rendering_info.as_ref(), s.offset.as_ref()),
                    org_size(s.org_sz.as_ref(), s.cur_sz.as_ref()),
                )
            }
            K::Polygon => {
                let s = &container.polygons[idx];
                (
                    child_chain(s.rendering_info.as_ref(), s.offset.as_ref()),
                    org_size(s.org_sz.as_ref(), s.cur_sz.as_ref()),
                )
            }
            K::Curve => {
                let s = &container.curves[idx];
                (
                    child_chain(s.rendering_info.as_ref(), s.offset.as_ref()),
                    org_size(s.org_sz.as_ref(), s.cur_sz.as_ref()),
                )
            }
            K::ConnectLine => {
                let s = &container.connect_lines[idx];
                (
                    child_chain(s.rendering_info.as_ref(), s.offset.as_ref()),
                    org_size(s.org_sz.as_ref(), s.cur_sz.as_ref()),
                )
            }
            K::TextArt => {
                let s = &container.textarts[idx];
                (
                    child_chain(s.rendering_info.as_ref(), s.offset.as_ref()),
                    org_size(s.org_sz.as_ref(), s.cur_sz.as_ref()),
                )
            }
            K::Picture if !has_image(&container.pics[idx]) => continue,
            K::Picture => {
                let s = &container.pics[idx];
                (
                    child_chain(s.rendering_info.as_ref(), s.offset.as_ref()),
                    org_size(s.org_sz.as_ref(), s.cur_sz.as_ref()),
                )
            }
            K::Container => {
                if let Some((a, b, c, d)) = leaf_bounds(&container.containers[idx]) {
                    pts.extend([(a, b), (c, d)]);
                }
                continue;
            }
            K::Unsupported(_) => continue,
        };
        let map = map?;
        pts.extend(corners(org.0, org.1).iter().map(|&p| map.apply(p)));
    }
    bounds(&pts)
}

/// Folds one child's wire map into exact Core geometry relative to `origin`
/// with the shared Core definition
/// ([`fold_group_child`](hwpforge_core::control::geometry::fold_group_child)),
/// reporting what it cannot carry. `None` = dropped (its `ContentLost` is
/// already reported).
fn normalize_group_child(
    wire: WireChild,
    origin: (f64, f64),
    ctx: &mut DecodeCtx,
) -> Option<Control> {
    use hwpforge_core::control::geometry::{
        fold_group_child, FoldInput, FoldLossKind, FoldOutcome, GeometryFacts,
    };

    let WireChild { control, map, org, picture_fill } = wire;
    let Some(m) = map else {
        ctx.content_lost("group_child_transform", "unparseable transform matrix");
        return None;
    };
    let mut control = control;
    if picture_fill {
        // The picture is not carried; the shape still is, and a picture
        // fill turns with it, so it takes the box rule.
        ctx.content_lost("group_child_fill", "picture fill not carried by Core");
        // Group children keep their old output (the encoder's default fill)
        // until group picture fills are carried; see `decode_shape_style`.
        if let Some(style) = control_style_mut(&mut control) {
            if matches!(style.fill, Some(hwpforge_core::control::Fill::Image { .. })) {
                style.fill = None;
            }
        }
    }
    let input = FoldInput::new(m.to_transform(), org, origin)
        .with_geometry_facts(GeometryFacts::new(picture_fill));
    let (kept, losses) = match fold_group_child(control, &input) {
        FoldOutcome::Kept { control, losses } => (Some(*control), losses),
        FoldOutcome::Dropped { losses } => (None, losses),
        _ => (None, Vec::new()),
    };
    for loss in losses {
        let kind = match loss.kind() {
            FoldLossKind::Degenerate => "group_child_degenerate",
            _ => "group_child_transform",
        };
        ctx.content_lost(kind, loss.detail().to_string());
    }
    kept
}

/// The style of a shape control, when it has one.
fn control_style_mut(control: &mut Control) -> Option<&mut ShapeStyle> {
    match control {
        Control::TextBox { style, .. }
        | Control::Rect { style, .. }
        | Control::Ellipse { style, .. }
        | Control::Polygon { style, .. }
        | Control::Arc { style, .. }
        | Control::Curve { style, .. }
        | Control::Line { style, .. }
        | Control::ConnectLine { style, .. } => style.as_mut(),
        _ => None,
    }
}

/// Whether every value is a valid [`HwpUnit`] (which a Core size or offset
/// must be — an invalid one would be stored as 0).
fn fits_hwpunit(values: &[i32]) -> bool {
    values.iter().all(|&v| HwpUnit::new(v).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 이것을 실패시키는 것: 결측 `width` 의 기본값을 0 으로 되돌리기(결측이
    // 굵기 0 으로 읽힘), 또는 결측 표식을 직렬화에서 빼지 않기(`width="-1"`).
    #[test]
    fn missing_line_width_stays_unspecified() {
        let ls: HxLineShape =
            crate::decoder::xml_from_str(r##"<lineShape color="#000000" style="SOLID"/>"##)
                .unwrap();
        let style =
            decode_shape_style_full(&Some(ls.clone()), &None, false, None, None, "None").unwrap();
        assert_eq!(style.line_width, None);
        #[allow(clippy::disallowed_methods)]
        let back = quick_xml::se::to_string(&ls).unwrap();
        assert!(!back.contains("width"), "{back}");
    }

    #[test]
    fn zero_line_width_decodes_as_zero() {
        let line =
            HxLineShape { width: 0, style: "NONE".to_string(), ..HxLineShape::default_solid() };
        let style = decode_shape_style_full(&Some(line), &None, false, None, None, "None").unwrap();
        assert_eq!(style.line_width, Some(0));
        assert_eq!(style.line_style, Some(hwpforge_core::control::LineStyle::None));
    }

    #[test]
    fn no_fill_shape_decodes_as_fill_none() {
        use hwpforge_core::control::Fill;
        let style = decode_shape_style_full(&None, &None, true, None, None, "None").unwrap();
        assert_eq!(style.fill, Some(Fill::None));
        // 선(채울 내부가 없음)은 종전대로 스타일 없음.
        assert!(decode_shape_style_full(&None, &None, false, None, None, "None").is_none());
    }
    use crate::schema::section::{
        HxConnectLine, HxControlPoint, HxControlPoints, HxCurve, HxCurveSegment, HxEllipse,
        HxFillBrush, HxFlip, HxLine, HxLineShape, HxPoint, HxRotationInfo, HxTablePos, HxTableSz,
    };
    use hwpforge_core::control::{Control, Fill, ShapePoint};

    /// Test-only convenience wrapper that calls `decode_shape_style_full` without rotation/flip.
    fn decode_shape_style(
        line_shape: &Option<HxLineShape>,
        fill_brush: &Option<HxFillBrush>,
        dropcap_style: &str,
    ) -> Option<ShapeStyle> {
        decode_shape_style_full(line_shape, fill_brush, false, None, None, dropcap_style)
    }

    use hwpforge_foundation::{
        ArcType, ArrowSize, ArrowType, CharShapeIndex, Color, DropCapStyle, Flip, PatternType,
    };

    // ── Helper builders ──────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn make_line_shape(
        color: &str,
        width: i32,
        style: &str,
        head_style: &str,
        tail_style: &str,
        head_fill: u32,
        tail_fill: u32,
        head_sz: &str,
        tail_sz: &str,
    ) -> HxLineShape {
        HxLineShape {
            color: color.to_string(),
            width,
            style: style.to_string(),
            end_cap: "FLAT".to_string(),
            head_style: head_style.to_string(),
            tail_style: tail_style.to_string(),
            head_fill,
            tail_fill,
            head_sz: head_sz.to_string(),
            tail_sz: tail_sz.to_string(),
            outline_style: "NORMAL".to_string(),
            alpha: 0,
        }
    }

    fn make_fill_brush(face_color: &str) -> HxFillBrush {
        use crate::schema::shapes::HxWinBrush;
        HxFillBrush {
            win_brush: Some(HxWinBrush {
                face_color: face_color.to_string(),
                hatch_color: "#000000".to_string(),
                hatch_style: None,
                alpha: 0,
            }),
            gradation: None,
            img_brush: None,
        }
    }

    fn make_sz(w: i32, h: i32) -> HxTableSz {
        HxTableSz {
            width: w,
            width_rel_to: "ABSOLUTE".to_string(),
            height: h,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: 0,
        }
    }

    fn make_pos(horz: i32, vert: i32) -> HxTablePos {
        HxTablePos {
            treat_as_char: 0,
            affect_l_spacing: 0,
            flow_with_text: 0,
            allow_overlap: 0,
            hold_anchor_and_so: 0,
            vert_rel_to: "PARA".to_string(),
            horz_rel_to: "PARA".to_string(),
            vert_align: "TOP".to_string(),
            horz_align: "LEFT".to_string(),
            vert_offset: vert,
            horz_offset: horz,
        }
    }

    fn default_ellipse() -> HxEllipse {
        HxEllipse {
            id: String::new(),
            z_order: 0,
            numbering_type: "NONE".to_string(),
            text_wrap: "TOP_AND_BOTTOM".to_string(),
            text_flow: "BOTH_SIDES".to_string(),
            lock: 0,
            dropcap_style: "None".to_string(),
            href: String::new(),
            group_level: 0,
            instid: String::new(),
            interval_dirty: 0,
            has_arc_pr: 0,
            arc_type: "NORMAL".to_string(),
            offset: None,
            org_sz: None,
            cur_sz: None,
            flip: None,
            rotation_info: None,
            rendering_info: None,
            line_shape: None,
            fill_brush: None,
            shadow: None,
            sz: None,
            pos: None,
            out_margin: None,
            shape_comment: None,
            caption: None,
            draw_text: None,
            center: None,
            ax1: None,
            ax2: None,
            start1: None,
            end1: None,
            start2: None,
            end2: None,
        }
    }

    fn default_curve() -> HxCurve {
        HxCurve {
            id: String::new(),
            z_order: 0,
            numbering_type: "NONE".to_string(),
            text_wrap: "TOP_AND_BOTTOM".to_string(),
            text_flow: "BOTH_SIDES".to_string(),
            lock: 0,
            dropcap_style: "None".to_string(),
            href: String::new(),
            group_level: 0,
            instid: String::new(),
            offset: None,
            org_sz: None,
            cur_sz: None,
            flip: None,
            rotation_info: None,
            rendering_info: None,
            line_shape: None,
            fill_brush: None,
            shadow: None,
            sz: None,
            pos: None,
            out_margin: None,
            shape_comment: None,
            caption: None,
            points: vec![],
            segments: vec![],
        }
    }

    fn default_connect_line() -> HxConnectLine {
        HxConnectLine {
            id: String::new(),
            z_order: 0,
            numbering_type: "NONE".to_string(),
            text_wrap: "TOP_AND_BOTTOM".to_string(),
            text_flow: "BOTH_SIDES".to_string(),
            lock: 0,
            dropcap_style: "None".to_string(),
            href: String::new(),
            group_level: 0,
            instid: String::new(),
            connect_type: "STRAIGHT".to_string(),
            offset: None,
            org_sz: None,
            cur_sz: None,
            flip: None,
            rotation_info: None,
            rendering_info: None,
            line_shape: None,
            fill_brush: None,
            shadow: None,
            start_pt: None,
            end_pt: None,
            control_points: None,
            sz: None,
            pos: None,
            out_margin: None,
            shape_comment: None,
            caption: None,
        }
    }

    // ── decode_shape_style tests ─────────────────────────────────────

    #[test]
    fn decode_shape_style_none_inputs_returns_none() {
        let result = decode_shape_style(&None, &None, "None");
        assert!(result.is_none(), "all-None inputs should yield None style");
    }

    #[test]
    fn decode_shape_style_empty_color_fields_returns_none() {
        // Empty color, missing width and empty style should not produce a style.
        // (Width 0 is a real value — `zero_line_width_decodes_as_zero`.)
        let ls = make_line_shape(
            "",
            crate::schema::shapes::LINE_WIDTH_ABSENT,
            "",
            "NORMAL",
            "NORMAL",
            1,
            1,
            "MEDIUM_MEDIUM",
            "MEDIUM_MEDIUM",
        );
        let result = decode_shape_style(&Some(ls), &None, "None");
        assert!(result.is_none(), "empty color + missing width + empty style should be None");
    }

    #[test]
    fn decode_shape_style_line_color_extracted() {
        let ls = make_line_shape(
            "#FF0000",
            33,
            "SOLID",
            "NORMAL",
            "NORMAL",
            1,
            1,
            "MEDIUM_MEDIUM",
            "MEDIUM_MEDIUM",
        );
        let style = decode_shape_style(&Some(ls), &None, "None").unwrap();
        let c = style.line_color.unwrap();
        // Color is RGB: r=255, g=0, b=0
        assert_eq!(c.to_hex_rgb(), "#FF0000");
    }

    #[test]
    fn decode_shape_style_line_width_extracted() {
        let ls = make_line_shape(
            "#000000",
            100,
            "SOLID",
            "NORMAL",
            "NORMAL",
            1,
            1,
            "MEDIUM_MEDIUM",
            "MEDIUM_MEDIUM",
        );
        let style = decode_shape_style(&Some(ls), &None, "None").unwrap();
        assert_eq!(style.line_width, Some(100));
    }

    #[test]
    fn decode_shape_style_line_style_dash_extracted() {
        use hwpforge_core::control::LineStyle;
        let ls = make_line_shape(
            "#000000",
            33,
            "DASH",
            "NORMAL",
            "NORMAL",
            1,
            1,
            "MEDIUM_MEDIUM",
            "MEDIUM_MEDIUM",
        );
        let style = decode_shape_style(&Some(ls), &None, "None").unwrap();
        assert_eq!(style.line_style, Some(LineStyle::Dash));
    }

    #[test]
    fn decode_shape_style_fill_color_extracted() {
        let fb = make_fill_brush("#00FF00");
        let style = decode_shape_style(&None, &Some(fb), "None").unwrap();
        let c = style.fill_color.unwrap();
        assert_eq!(c.to_hex_rgb(), "#00FF00");
    }

    #[test]
    fn decode_shape_style_fill_color_empty_is_ignored() {
        let fb = make_fill_brush("");
        let result = decode_shape_style(&None, &Some(fb), "None");
        assert!(result.is_none(), "empty fill color should be ignored");
    }

    #[test]
    fn decode_shape_style_dropcap_double_line() {
        let style = decode_shape_style(&None, &None, "DoubleLine").unwrap();
        assert_eq!(style.drop_cap_style, DropCapStyle::DoubleLine);
    }

    #[test]
    fn decode_shape_style_dropcap_triple_line() {
        let style = decode_shape_style(&None, &None, "TripleLine").unwrap();
        assert_eq!(style.drop_cap_style, DropCapStyle::TripleLine);
    }

    #[test]
    fn decode_shape_style_dropcap_margin() {
        let style = decode_shape_style(&None, &None, "Margin").unwrap();
        assert_eq!(style.drop_cap_style, DropCapStyle::Margin);
    }

    #[test]
    fn decode_shape_style_head_arrow_normal() {
        let ls = make_line_shape(
            "#000000",
            33,
            "SOLID",
            "ARROW",
            "NORMAL",
            1,
            0,
            "MEDIUM_MEDIUM",
            "MEDIUM_MEDIUM",
        );
        let style = decode_shape_style(&Some(ls), &None, "None").unwrap();
        let head = style.head_arrow.unwrap();
        assert_eq!(head.arrow_type, ArrowType::Normal);
        assert_eq!(head.size, ArrowSize::Medium);
        assert!(head.filled);
    }

    #[test]
    fn decode_shape_style_tail_arrow_diamond() {
        let ls = make_line_shape(
            "#000000",
            33,
            "SOLID",
            "NORMAL",
            "EMPTY_DIAMOND",
            0,
            1,
            "MEDIUM_MEDIUM",
            "LARGE_LARGE",
        );
        let style = decode_shape_style(&Some(ls), &None, "None").unwrap();
        // head is NORMAL so no head_arrow
        assert!(style.head_arrow.is_none());
        let tail = style.tail_arrow.unwrap();
        assert_eq!(tail.arrow_type, ArrowType::Diamond);
        assert_eq!(tail.size, ArrowSize::Large);
        assert!(tail.filled);
    }

    #[test]
    fn decode_shape_style_tail_arrow_unfilled_oval() {
        let ls = make_line_shape(
            "#000000",
            33,
            "SOLID",
            "NORMAL",
            "EMPTY_CIRCLE",
            0,
            0,
            "MEDIUM_MEDIUM",
            "SMALL_SMALL",
        );
        let style = decode_shape_style(&Some(ls), &None, "None").unwrap();
        let tail = style.tail_arrow.unwrap();
        assert_eq!(tail.arrow_type, ArrowType::Oval);
        assert!(!tail.filled);
        assert_eq!(tail.size, ArrowSize::Small);
    }

    // ── decode_shape_style_full tests ────────────────────────────────

    #[test]
    fn decode_shape_style_full_rotation_extracted() {
        let ri = HxRotationInfo { angle: 45, center_x: 50, center_y: 50, rotate_image: 1 };
        let style = decode_shape_style_full(&None, &None, false, Some(&ri), None, "None").unwrap();
        let rot = style.rotation.unwrap();
        assert!((rot - 45.0f32).abs() < 0.01, "45 degrees expected, got {rot}");
    }

    #[test]
    fn decode_shape_style_full_rotation_zero_ignored() {
        let ri = HxRotationInfo { angle: 0, center_x: 0, center_y: 0, rotate_image: 1 };
        let result = decode_shape_style_full(&None, &None, false, Some(&ri), None, "None");
        assert!(result.is_none(), "zero rotation + no other fields should be None");
    }

    #[test]
    fn decode_shape_style_full_flip_horizontal() {
        let fi = HxFlip { horizontal: 1, vertical: 0 };
        let style = decode_shape_style_full(&None, &None, false, None, Some(&fi), "None").unwrap();
        assert_eq!(style.flip, Some(Flip::Horizontal));
    }

    #[test]
    fn decode_shape_style_full_flip_vertical() {
        let fi = HxFlip { horizontal: 0, vertical: 1 };
        let style = decode_shape_style_full(&None, &None, false, None, Some(&fi), "None").unwrap();
        assert_eq!(style.flip, Some(Flip::Vertical));
    }

    #[test]
    fn decode_shape_style_full_flip_both() {
        let fi = HxFlip { horizontal: 1, vertical: 1 };
        let style = decode_shape_style_full(&None, &None, false, None, Some(&fi), "None").unwrap();
        assert_eq!(style.flip, Some(Flip::Both));
    }

    #[test]
    fn decode_shape_style_full_flip_none_ignored() {
        let fi = HxFlip { horizontal: 0, vertical: 0 };
        let result = decode_shape_style_full(&None, &None, false, None, Some(&fi), "None");
        assert!(result.is_none(), "zero flip should yield None");
    }

    #[test]
    fn decode_shape_style_full_combined_rotation_and_flip() {
        let ri = HxRotationInfo { angle: 90, center_x: 0, center_y: 0, rotate_image: 1 };
        let fi = HxFlip { horizontal: 1, vertical: 0 };
        let style =
            decode_shape_style_full(&None, &None, false, Some(&ri), Some(&fi), "None").unwrap();
        let rot = style.rotation.unwrap();
        assert!((rot - 90.0f32).abs() < 0.01);
        assert_eq!(style.flip, Some(Flip::Horizontal));
    }

    // ── pattern fill decode tests ─────────────────────────────────────

    #[test]
    fn decode_shape_style_pattern_fill_horizontal() {
        let fb = Some(HxFillBrush {
            win_brush: Some(crate::schema::shapes::HxWinBrush {
                face_color: "#FFD700".to_string(),
                hatch_color: "#000000".to_string(),
                hatch_style: Some("HORIZONTAL".to_string()),
                alpha: 0,
            }),
            gradation: None,
            img_brush: None,
        });
        let style = decode_shape_style_full(&None, &fb, false, None, None, "None").unwrap();
        match style.fill.unwrap() {
            Fill::Pattern { pattern_type, fg_color, bg_color } => {
                assert_eq!(pattern_type, PatternType::Horizontal);
                assert_eq!(fg_color, Color::BLACK);
                assert_eq!(bg_color, Color::from_rgb(0xFF, 0xD7, 0x00));
            }
            other => panic!("expected Fill::Pattern, got {other:?}"),
        }
    }

    #[test]
    fn decode_shape_style_pattern_fill_backslash_swapped() {
        // HWPX "BACK_SLASH" → PatternType::Slash (한글 spec reversal)
        let fb = Some(HxFillBrush {
            win_brush: Some(crate::schema::shapes::HxWinBrush {
                face_color: "#FFFFFF".to_string(),
                hatch_color: "#FF0000".to_string(),
                hatch_style: Some("BACK_SLASH".to_string()),
                alpha: 0,
            }),
            gradation: None,
            img_brush: None,
        });
        let style = decode_shape_style_full(&None, &fb, false, None, None, "None").unwrap();
        match style.fill.unwrap() {
            Fill::Pattern { pattern_type, .. } => {
                assert_eq!(pattern_type, PatternType::Slash);
            }
            other => panic!("expected Fill::Pattern, got {other:?}"),
        }
    }

    #[test]
    fn decode_shape_style_pattern_fill_slash_swapped() {
        // HWPX "SLASH" → PatternType::BackSlash (한글 spec reversal)
        let fb = Some(HxFillBrush {
            win_brush: Some(crate::schema::shapes::HxWinBrush {
                face_color: "#FFFFFF".to_string(),
                hatch_color: "#0000FF".to_string(),
                hatch_style: Some("SLASH".to_string()),
                alpha: 0,
            }),
            gradation: None,
            img_brush: None,
        });
        let style = decode_shape_style_full(&None, &fb, false, None, None, "None").unwrap();
        match style.fill.unwrap() {
            Fill::Pattern { pattern_type, .. } => {
                assert_eq!(pattern_type, PatternType::BackSlash);
            }
            other => panic!("expected Fill::Pattern, got {other:?}"),
        }
    }

    #[test]
    fn decode_shape_style_no_hatch_style_is_solid_not_pattern() {
        // winBrush without hatchStyle → solid fill (fill_color), NOT pattern
        let fb = Some(HxFillBrush {
            win_brush: Some(crate::schema::shapes::HxWinBrush {
                face_color: "#FF0000".to_string(),
                hatch_color: "#000000".to_string(),
                hatch_style: None,
                alpha: 0,
            }),
            gradation: None,
            img_brush: None,
        });
        let style = decode_shape_style_full(&None, &fb, false, None, None, "None").unwrap();
        assert!(style.fill.is_none(), "no hatchStyle → no Fill::Pattern");
        assert!(style.fill_color.is_some(), "fill_color should be set");
    }

    #[test]
    fn decode_shape_style_gradient_takes_priority_over_pattern() {
        // If both gradation and winBrush with hatchStyle exist, gradient wins
        let fb = Some(HxFillBrush {
            win_brush: Some(crate::schema::shapes::HxWinBrush {
                face_color: "#FFFFFF".to_string(),
                hatch_color: "#000000".to_string(),
                hatch_style: Some("HORIZONTAL".to_string()),
                alpha: 0,
            }),
            gradation: Some(crate::schema::shapes::HxGradation {
                gradation_type: "LINEAR".to_string(),
                angle: 0,
                center_x: 0,
                center_y: 0,
                step: 255,
                color_num: 2,
                step_center: 50,
                alpha: 0,
                colors: vec![
                    crate::schema::shapes::HxGradColor { value: "#FF0000".to_string() },
                    crate::schema::shapes::HxGradColor { value: "#0000FF".to_string() },
                ],
            }),
            img_brush: None,
        });
        let style = decode_shape_style_full(&None, &fb, false, None, None, "None").unwrap();
        assert!(matches!(style.fill, Some(Fill::Gradient { .. })));
    }

    // ── decode_arc tests ─────────────────────────────────────────────

    #[test]
    fn decode_arc_default_fields_normal_type() {
        let ellipse = default_ellipse();
        let cs = CharShapeIndex::new(0);
        let run = decode_arc(&ellipse, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::Arc { arc_type, center, axis1, axis2, width, height, .. } =
            run.content.as_control().unwrap().clone()
        {
            assert_eq!(arc_type, ArcType::Normal);
            assert_eq!(center, ShapePoint::new(0, 0));
            assert_eq!(axis1, ShapePoint::new(0, 0));
            assert_eq!(axis2, ShapePoint::new(0, 0));
            assert_eq!(width, hwpforge_foundation::HwpUnit::ZERO);
            assert_eq!(height, hwpforge_foundation::HwpUnit::ZERO);
        } else {
            panic!("expected Control::Arc");
        }
    }

    #[test]
    fn decode_arc_pie_type() {
        let mut ellipse = default_ellipse();
        ellipse.arc_type = "PIE".to_string();
        let cs = CharShapeIndex::new(0);
        let run = decode_arc(&ellipse, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::Arc { arc_type, .. } = run.content.as_control().unwrap().clone() {
            assert_eq!(arc_type, ArcType::Pie);
        } else {
            panic!("expected Control::Arc");
        }
    }

    #[test]
    fn decode_arc_chord_type() {
        let mut ellipse = default_ellipse();
        ellipse.arc_type = "CHORD".to_string();
        let cs = CharShapeIndex::new(0);
        let run = decode_arc(&ellipse, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::Arc { arc_type, .. } = run.content.as_control().unwrap().clone() {
            assert_eq!(arc_type, ArcType::Chord);
        } else {
            panic!("expected Control::Arc");
        }
    }

    #[test]
    fn decode_arc_with_geometry_points() {
        let mut ellipse = default_ellipse();
        ellipse.center = Some(HxPoint { x: 100, y: 200 });
        ellipse.ax1 = Some(HxPoint { x: 300, y: 200 });
        ellipse.ax2 = Some(HxPoint { x: 100, y: 400 });
        ellipse.start1 = Some(HxPoint { x: 50, y: 100 });
        ellipse.end1 = Some(HxPoint { x: 150, y: 100 });
        let cs = CharShapeIndex::new(2);
        let run = decode_arc(&ellipse, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        assert_eq!(run.char_shape_id, cs);
        if let Control::Arc { center, axis1, axis2, start1, end1, .. } =
            run.content.as_control().unwrap().clone()
        {
            assert_eq!(center, ShapePoint::new(100, 200));
            assert_eq!(axis1, ShapePoint::new(300, 200));
            assert_eq!(axis2, ShapePoint::new(100, 400));
            assert_eq!(start1, ShapePoint::new(50, 100));
            assert_eq!(end1, ShapePoint::new(150, 100));
        } else {
            panic!("expected Control::Arc");
        }
    }

    #[test]
    fn decode_arc_with_size_and_offset() {
        let mut ellipse = default_ellipse();
        ellipse.sz = Some(make_sz(5000, 3000));
        ellipse.pos = Some(make_pos(100, 200));
        let cs = CharShapeIndex::new(0);
        let run = decode_arc(&ellipse, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::Arc { width, height, placement, .. } =
            run.content.as_control().unwrap().clone()
        {
            assert_eq!(width.as_i32(), 5000);
            assert_eq!(height.as_i32(), 3000);
            let placement = placement.expect("anchored pos carries placement");
            assert_eq!(placement.horz_offset.as_i32(), 100);
            assert_eq!(placement.vert_offset.as_i32(), 200);
        } else {
            panic!("expected Control::Arc");
        }
    }

    #[test]
    fn decode_arc_with_rotation_style() {
        let mut ellipse = default_ellipse();
        ellipse.rotation_info =
            Some(HxRotationInfo { angle: 45, center_x: 50, center_y: 50, rotate_image: 1 });
        let cs = CharShapeIndex::new(0);
        let run = decode_arc(&ellipse, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::Arc { style, .. } = run.content.as_control().unwrap().clone() {
            let s = style.unwrap();
            let rot = s.rotation.unwrap();
            assert!((rot - 45.0f32).abs() < 0.01);
        } else {
            panic!("expected Control::Arc");
        }
    }

    #[test]
    fn decode_arc_unknown_type_falls_back_to_normal() {
        let mut ellipse = default_ellipse();
        ellipse.arc_type = "BOGUS".to_string();
        let cs = CharShapeIndex::new(0);
        let run = decode_arc(&ellipse, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::Arc { arc_type, .. } = run.content.as_control().unwrap().clone() {
            assert_eq!(arc_type, ArcType::Normal);
        } else {
            panic!("expected Control::Arc");
        }
    }

    // ── decode_curve tests ───────────────────────────────────────────

    #[test]
    fn decode_curve_empty_returns_no_points() {
        let curve = default_curve();
        let cs = CharShapeIndex::new(0);
        let run = decode_curve(&curve, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::Curve { points, segment_types, .. } =
            run.content.as_control().unwrap().clone()
        {
            assert!(points.is_empty());
            assert!(segment_types.is_empty());
        } else {
            panic!("expected Control::Curve");
        }
    }

    #[test]
    fn decode_curve_from_segments_reconstructs_points() {
        let mut curve = default_curve();
        curve.segments = vec![
            HxCurveSegment { seg_type: "CURVE".to_string(), x1: 0, y1: 0, x2: 100, y2: 50 },
            HxCurveSegment { seg_type: "LINE".to_string(), x1: 100, y1: 50, x2: 200, y2: 100 },
        ];
        let cs = CharShapeIndex::new(0);
        let run = decode_curve(&curve, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::Curve { points, segment_types, .. } =
            run.content.as_control().unwrap().clone()
        {
            // 2 segments → 3 points: start of first, then end of each
            assert_eq!(points.len(), 3);
            assert_eq!(points[0], ShapePoint::new(0, 0));
            assert_eq!(points[1], ShapePoint::new(100, 50));
            assert_eq!(points[2], ShapePoint::new(200, 100));
            assert_eq!(segment_types.len(), 2);
            assert_eq!(segment_types[0], CurveSegmentType::Curve);
            assert_eq!(segment_types[1], CurveSegmentType::Line);
        } else {
            panic!("expected Control::Curve");
        }
    }

    #[test]
    fn decode_curve_explicit_points_preferred_over_segments() {
        let mut curve = default_curve();
        curve.points = vec![HxPoint { x: 10, y: 20 }, HxPoint { x: 30, y: 40 }];
        curve.segments =
            vec![HxCurveSegment { seg_type: "LINE".to_string(), x1: 0, y1: 0, x2: 999, y2: 999 }];
        let cs = CharShapeIndex::new(0);
        let run = decode_curve(&curve, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::Curve { points, .. } = run.content.as_control().unwrap().clone() {
            assert_eq!(points.len(), 2);
            assert_eq!(points[0], ShapePoint::new(10, 20));
            assert_eq!(points[1], ShapePoint::new(30, 40));
        } else {
            panic!("expected Control::Curve");
        }
    }

    #[test]
    fn decode_curve_unknown_segment_type_defaults_to_curve() {
        let mut curve = default_curve();
        curve.segments =
            vec![HxCurveSegment { seg_type: "BOGUS".to_string(), x1: 0, y1: 0, x2: 100, y2: 100 }];
        let cs = CharShapeIndex::new(0);
        let run = decode_curve(&curve, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::Curve { segment_types, .. } = run.content.as_control().unwrap().clone() {
            assert_eq!(segment_types[0], CurveSegmentType::Curve);
        } else {
            panic!("expected Control::Curve");
        }
    }

    #[test]
    fn decode_curve_with_size_and_offset() {
        let mut curve = default_curve();
        curve.sz = Some(make_sz(8000, 4000));
        curve.pos = Some(make_pos(50, 75));
        let cs = CharShapeIndex::new(1);
        let run = decode_curve(&curve, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        assert_eq!(run.char_shape_id, cs);
        if let Control::Curve { width, height, placement, .. } =
            run.content.as_control().unwrap().clone()
        {
            assert_eq!(width.as_i32(), 8000);
            assert_eq!(height.as_i32(), 4000);
            let placement = placement.expect("anchored pos carries placement");
            assert_eq!(placement.horz_offset.as_i32(), 50);
            assert_eq!(placement.vert_offset.as_i32(), 75);
        } else {
            panic!("expected Control::Curve");
        }
    }

    // ── decode_connect_line tests ────────────────────────────────────

    #[test]
    fn decode_connect_line_defaults() {
        let cl = default_connect_line();
        let cs = CharShapeIndex::new(0);
        let run = decode_connect_line(&cl, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::ConnectLine { start, end, control_points, connect_type, .. } =
            run.content.as_control().unwrap().clone()
        {
            assert_eq!(start, ShapePoint::new(0, 0));
            assert_eq!(end, ShapePoint::new(0, 0));
            assert!(control_points.is_empty());
            assert_eq!(connect_type, "STRAIGHT");
        } else {
            panic!("expected Control::ConnectLine");
        }
    }

    #[test]
    fn decode_connect_line_with_endpoints() {
        let mut cl = default_connect_line();
        cl.start_pt = Some(crate::schema::shapes::HxConnectPoint {
            x: 100,
            y: 200,
            subject_id_ref: "0".to_string(),
            subject_idx: "0".to_string(),
        });
        cl.end_pt = Some(crate::schema::shapes::HxConnectPoint {
            x: 500,
            y: 600,
            subject_id_ref: "0".to_string(),
            subject_idx: "0".to_string(),
        });
        let cs = CharShapeIndex::new(0);
        let run = decode_connect_line(&cl, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::ConnectLine { start, end, .. } = run.content.as_control().unwrap().clone() {
            assert_eq!(start, ShapePoint::new(100, 200));
            assert_eq!(end, ShapePoint::new(500, 600));
        } else {
            panic!("expected Control::ConnectLine");
        }
    }

    #[test]
    fn decode_connect_line_control_points_extracted() {
        let mut cl = default_connect_line();
        cl.control_points = Some(HxControlPoints {
            points: vec![
                HxControlPoint { x: 0, y: 0, point_type: "3".to_string() }, // start — skipped
                HxControlPoint { x: 200, y: 300, point_type: "2".to_string() }, // intermediate
                HxControlPoint { x: 400, y: 500, point_type: "2".to_string() }, // intermediate
                HxControlPoint { x: 600, y: 700, point_type: "26".to_string() }, // end — skipped
            ],
        });
        let cs = CharShapeIndex::new(0);
        let run = decode_connect_line(&cl, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::ConnectLine { control_points, .. } =
            run.content.as_control().unwrap().clone()
        {
            assert_eq!(control_points.len(), 2);
            assert_eq!(control_points[0], ShapePoint::new(200, 300));
            assert_eq!(control_points[1], ShapePoint::new(400, 500));
        } else {
            panic!("expected Control::ConnectLine");
        }
    }

    #[test]
    fn decode_connect_line_only_two_control_points_returns_empty_intermediates() {
        let mut cl = default_connect_line();
        cl.control_points = Some(HxControlPoints {
            points: vec![
                HxControlPoint { x: 0, y: 0, point_type: "3".to_string() },
                HxControlPoint { x: 600, y: 700, point_type: "26".to_string() },
            ],
        });
        let cs = CharShapeIndex::new(0);
        let run = decode_connect_line(&cl, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::ConnectLine { control_points, .. } =
            run.content.as_control().unwrap().clone()
        {
            assert!(control_points.is_empty());
        } else {
            panic!("expected Control::ConnectLine");
        }
    }

    #[test]
    fn decode_connect_line_bent_type() {
        let mut cl = default_connect_line();
        cl.connect_type = "BENT".to_string();
        let cs = CharShapeIndex::new(0);
        let run = decode_connect_line(&cl, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::ConnectLine { connect_type, .. } = run.content.as_control().unwrap().clone()
        {
            assert_eq!(connect_type, "BENT");
        } else {
            panic!("expected Control::ConnectLine");
        }
    }

    #[test]
    fn decode_connect_line_with_size() {
        let mut cl = default_connect_line();
        cl.sz = Some(make_sz(10000, 5000));
        cl.pos = Some(make_pos(150, 250));
        let cs = CharShapeIndex::new(3);
        let run = decode_connect_line(&cl, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        assert_eq!(run.char_shape_id, cs);
        if let Control::ConnectLine { width, height, placement, .. } =
            run.content.as_control().unwrap().clone()
        {
            assert_eq!(width.as_i32(), 10000);
            assert_eq!(height.as_i32(), 5000);
            let placement = placement.expect("anchored pos carries placement");
            assert_eq!(placement.horz_offset.as_i32(), 150);
            assert_eq!(placement.vert_offset.as_i32(), 250);
        } else {
            panic!("expected Control::ConnectLine");
        }
    }

    // ── decode_line tests ────────────────────────────────────────────

    #[test]
    fn decode_line_defaults_all_zero() {
        let line = HxLine {
            id: String::new(),
            z_order: 0,
            numbering_type: "NONE".to_string(),
            text_wrap: "TOP_AND_BOTTOM".to_string(),
            text_flow: "BOTH_SIDES".to_string(),
            lock: 0,
            dropcap_style: "None".to_string(),
            href: String::new(),
            group_level: 0,
            instid: String::new(),
            is_reverse_hv: 0,
            offset: None,
            org_sz: None,
            cur_sz: None,
            flip: None,
            rotation_info: None,
            rendering_info: None,
            line_shape: None,
            fill_brush: None,
            shadow: None,
            sz: None,
            pos: None,
            out_margin: None,
            shape_comment: None,
            caption: None,
            start_pt: None,
            end_pt: None,
        };
        let cs = CharShapeIndex::new(0);
        let run = decode_line(&line, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::Line { start, end, width, height, .. } =
            run.content.as_control().unwrap().clone()
        {
            assert_eq!(start, ShapePoint::new(0, 0));
            assert_eq!(end, ShapePoint::new(0, 0));
            assert_eq!(width, hwpforge_foundation::HwpUnit::ZERO);
            assert_eq!(height, hwpforge_foundation::HwpUnit::ZERO);
        } else {
            panic!("expected Control::Line");
        }
    }

    fn default_rect(width: i32, height: i32) -> HxRect {
        HxRect {
            id: String::new(),
            z_order: 0,
            numbering_type: "NONE".to_string(),
            text_wrap: "TOP_AND_BOTTOM".to_string(),
            text_flow: "BOTH_SIDES".to_string(),
            lock: 0,
            dropcap_style: "None".to_string(),
            href: String::new(),
            group_level: 0,
            instid: String::new(),
            ratio: 0,
            offset: None,
            org_sz: None,
            cur_sz: None,
            flip: None,
            rotation_info: None,
            rendering_info: None,
            line_shape: None,
            fill_brush: None,
            shadow: None,
            sz: Some(crate::schema::section::HxTableSz {
                width,
                width_rel_to: "ABSOLUTE".to_string(),
                height,
                height_rel_to: "ABSOLUTE".to_string(),
                protect: 0,
            }),
            pos: None,
            out_margin: None,
            shape_comment: None,
            caption: None,
            draw_text: None,
            pt0: None,
            pt1: None,
            pt2: None,
            pt3: None,
        }
    }

    #[test]
    fn decode_rect_without_draw_text_produces_control_rect() {
        let rect = default_rect(8000, 4000);
        let cs = CharShapeIndex::new(0);
        let run = decode_textbox(&rect, cs, 0, &mut DecodeCtx::new(0))
            .unwrap()
            .expect("pure rect must decode to a Run");
        match run.content.as_control().unwrap().clone() {
            Control::Rect { width, height, placement, .. } => {
                assert_eq!(width, hwpforge_foundation::HwpUnit::new(8000).unwrap());
                assert_eq!(height, hwpforge_foundation::HwpUnit::new(4000).unwrap());
                assert!(placement.is_none());
            }
            other => panic!("expected Control::Rect, got {other:?}"),
        }
    }

    #[test]
    fn decode_ellipse_defaults_all_zero() {
        let ellipse = default_ellipse();
        let cs = CharShapeIndex::new(0);
        let run = decode_ellipse(&ellipse, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::Ellipse { center, axis1, axis2, width, height, paragraphs, .. } =
            run.content.as_control().unwrap().clone()
        {
            assert_eq!(center, ShapePoint::new(0, 0));
            assert_eq!(axis1, ShapePoint::new(0, 0));
            assert_eq!(axis2, ShapePoint::new(0, 0));
            assert_eq!(width, hwpforge_foundation::HwpUnit::ZERO);
            assert_eq!(height, hwpforge_foundation::HwpUnit::ZERO);
            assert!(paragraphs.is_empty());
        } else {
            panic!("expected Control::Ellipse");
        }
    }

    fn ellipse_with_draw_text_align(vert_align: &str) -> HxEllipse {
        use crate::schema::section::HxSubList;
        use crate::schema::shapes::HxDrawText;
        let mut ellipse = default_ellipse();
        ellipse.draw_text = Some(HxDrawText {
            last_width: 0,
            name: String::new(),
            editable: 0,
            sub_list: HxSubList {
                vert_align: vert_align.to_string(),
                paragraphs: vec![],
                ..Default::default()
            },
            text_margin: None,
        });
        ellipse
    }

    #[test]
    fn decode_ellipse_reads_center_vert_align_from_sublist() {
        let ellipse = ellipse_with_draw_text_align("CENTER");
        let run =
            decode_ellipse(&ellipse, CharShapeIndex::new(0), 0, &mut DecodeCtx::new(0)).unwrap();
        match run.content.as_control().unwrap().clone() {
            Control::Ellipse { text_vertical_align, .. } => {
                assert_eq!(text_vertical_align, hwpforge_foundation::VerticalAlign::Center);
            }
            other => panic!("expected Control::Ellipse, got {other:?}"),
        }
    }

    #[test]
    fn decode_ellipse_empty_vert_align_defaults_top() {
        // 한컴 omits vertAlign for the default; empty must decode as Top so
        // default shapes round-trip unchanged.
        let ellipse = ellipse_with_draw_text_align("");
        let run =
            decode_ellipse(&ellipse, CharShapeIndex::new(0), 0, &mut DecodeCtx::new(0)).unwrap();
        match run.content.as_control().unwrap().clone() {
            Control::Ellipse { text_vertical_align, .. } => {
                assert_eq!(text_vertical_align, hwpforge_foundation::VerticalAlign::Top);
            }
            other => panic!("expected Control::Ellipse, got {other:?}"),
        }
    }

    #[test]
    fn ellipse_center_align_survives_encode_decode_round_trip() {
        use hwpforge_core::paragraph::Paragraph;
        use hwpforge_foundation::{HwpUnit, ParaShapeIndex, VerticalAlign};

        // Build a Core ellipse with CENTER-aligned text, encode it to the HWPX
        // shape model, then decode it back. The new field must survive.
        let ctrl = Control::Ellipse {
            center: ShapePoint::new(100, 50),
            axis1: ShapePoint::new(200, 50),
            axis2: ShapePoint::new(100, 100),
            width: HwpUnit::new(200).unwrap(),
            height: HwpUnit::new(100).unwrap(),
            placement: None,
            paragraphs: vec![Paragraph::new(ParaShapeIndex::new(0))],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Center,
        };
        let mut hl = Vec::new();
        let hx = crate::encoder::shapes::encode_ellipse_to_hx(
            &ctrl,
            0,
            &mut hl,
            crate::encoder::EncodeOptions::default(),
            &mut crate::encoder::section::EncodeSink::new(0),
        )
        .unwrap();
        assert_eq!(
            hx.draw_text.as_ref().unwrap().sub_list.vert_align,
            "CENTER",
            "encoder must emit CENTER"
        );
        let run = decode_ellipse(&hx, CharShapeIndex::new(0), 0, &mut DecodeCtx::new(0)).unwrap();
        match run.content.as_control().unwrap().clone() {
            Control::Ellipse { text_vertical_align, .. } => {
                assert_eq!(
                    text_vertical_align,
                    VerticalAlign::Center,
                    "round-trip must preserve CENTER"
                );
            }
            other => panic!("expected Control::Ellipse, got {other:?}"),
        }
    }

    // ── ObjectPlacement decode (W4 w1) ───────────────────────────────

    #[test]
    fn decode_object_placement_collapses_inline_default_to_none() {
        // The plain inline default (treatAsChar=1, offsets 0, PARA) carries no
        // placement info, so it collapses to None and the encoder re-emits the
        // legacy inline `<hp:pos>`. An absent pos and empty (omitted) wrap/flow
        // attributes collapse the same way. (`make_pos` defaults treatAsChar=0,
        // so it is set explicitly here.)
        let mut inline = make_pos(0, 0);
        inline.treat_as_char = 1;
        assert!(decode_object_placement(Some(&inline), "TOP_AND_BOTTOM", "BOTH_SIDES").is_none());
        assert!(decode_object_placement(Some(&inline), "", "").is_none());
        assert!(decode_object_placement(None, "SQUARE", "LEFT_ONLY").is_none());
        // treatAsChar=0 at the origin is anchored, not inline → carries placement.
        assert!(decode_object_placement(Some(&make_pos(0, 0)), "", "").is_some());
    }

    #[test]
    fn decode_object_placement_maps_floating_pos_fields() {
        let mut pos = make_pos(1_200, 3_400);
        pos.treat_as_char = 0;
        pos.flow_with_text = 1;
        pos.allow_overlap = 1;
        pos.vert_rel_to = "PAPER".to_string();
        pos.horz_rel_to = "PAGE".to_string();
        let placement =
            decode_object_placement(Some(&pos), "SQUARE", "RIGHT_ONLY").expect("floating");
        assert_eq!(placement.text_wrap, ObjectTextWrap::Square);
        assert_eq!(placement.text_flow, ObjectTextFlow::RightOnly);
        assert!(!placement.treat_as_char);
        assert!(placement.flow_with_text);
        assert!(placement.allow_overlap);
        assert_eq!(placement.vert_rel_to, ObjectRelativeTo::Paper);
        assert_eq!(placement.horz_rel_to, ObjectRelativeTo::Page);
        assert_eq!(placement.horz_offset.as_i32(), 1_200);
        assert_eq!(placement.vert_offset.as_i32(), 3_400);
    }

    #[test]
    fn floating_rect_placement_round_trips_decode_then_encode() {
        // decode → Core → encode: a floating rect keeps its full pos across the
        // round-trip (the `<hp:pos>` silent-loss this slice removes).
        let mut rect = default_rect(10_000, 6_000);
        rect.text_wrap = "SQUARE".to_string();
        rect.text_flow = "RIGHT_ONLY".to_string();
        let mut pos = make_pos(1_000, 2_000);
        pos.treat_as_char = 0;
        pos.allow_overlap = 1;
        pos.vert_rel_to = "PAPER".to_string();
        pos.horz_rel_to = "PAPER".to_string();
        rect.pos = Some(pos);

        let control = decode_textbox(&rect, CharShapeIndex::new(0), 0, &mut DecodeCtx::new(0))
            .unwrap()
            .expect("rect decodes to a run")
            .content
            .as_control()
            .unwrap()
            .clone();
        let placement = match &control {
            Control::Rect { placement, .. } => placement.clone().expect("floating placement"),
            other => panic!("expected Control::Rect, got {other:?}"),
        };
        assert!(!placement.treat_as_char);
        assert_eq!(placement.horz_offset.as_i32(), 1_000);

        let mut hyperlinks = Vec::new();
        let encoded = crate::encoder::shapes::encode_rect_to_hx(
            &control,
            0,
            &mut hyperlinks,
            crate::encoder::EncodeOptions::default(),
            &mut crate::encoder::section::EncodeSink::new(0),
        )
        .unwrap();
        assert_eq!(encoded.numbering_type, "PICTURE");
        assert_eq!(encoded.text_wrap, "SQUARE");
        assert_eq!(encoded.text_flow, "RIGHT_ONLY");
        let out_pos = encoded.pos.expect("encoded pos");
        assert_eq!(out_pos.treat_as_char, 0);
        assert_eq!(out_pos.allow_overlap, 1);
        assert_eq!(out_pos.vert_rel_to, "PAPER");
        assert_eq!(out_pos.horz_offset, 1_000);
        assert_eq!(out_pos.vert_offset, 2_000);
    }

    #[test]
    fn floating_textbox_placement_round_trips_decode_then_encode() {
        use crate::schema::section::HxSubList;
        use crate::schema::shapes::HxDrawText;
        // Same as the rect case, but a `<hp:drawText>` child makes it a TextBox.
        let mut rect = default_rect(12_000, 8_000);
        rect.text_wrap = "SQUARE".to_string();
        let mut pos = make_pos(500, 700);
        pos.treat_as_char = 0;
        pos.vert_rel_to = "PAPER".to_string();
        pos.horz_rel_to = "PAPER".to_string();
        rect.pos = Some(pos);
        rect.draw_text = Some(HxDrawText {
            last_width: 0,
            name: String::new(),
            editable: 0,
            sub_list: HxSubList { paragraphs: vec![], ..Default::default() },
            text_margin: None,
        });

        let control = decode_textbox(&rect, CharShapeIndex::new(0), 0, &mut DecodeCtx::new(0))
            .unwrap()
            .expect("textbox decodes to a run")
            .content
            .as_control()
            .unwrap()
            .clone();
        match &control {
            Control::TextBox { placement, .. } => {
                let placement = placement.clone().expect("floating placement");
                assert!(!placement.treat_as_char);
                assert_eq!(placement.horz_offset.as_i32(), 500);
            }
            other => panic!("expected Control::TextBox, got {other:?}"),
        }

        let mut hyperlinks = Vec::new();
        let encoded = crate::encoder::shapes::encode_textbox_to_rect(
            &control,
            0,
            &mut hyperlinks,
            crate::encoder::EncodeOptions::default(),
            &mut crate::encoder::section::EncodeSink::new(0),
        )
        .unwrap();
        assert_eq!(encoded.numbering_type, "PICTURE");
        let out_pos = encoded.pos.expect("encoded pos");
        assert_eq!(out_pos.horz_offset, 500);
        assert_eq!(out_pos.vert_offset, 700);
    }

    /// Decodes a TextArt whose wire `text` attribute is `wire`.
    fn text_art_core_text(wire: &str) -> String {
        let hx = HxTextArt { text: wire.to_string(), ..Default::default() };
        let mut ctx = DecodeCtx::new(0);
        let run = decode_textart(&hx, CharShapeIndex::new(0), 0, &mut ctx).expect("decode");
        match run.content {
            RunContent::Control(c) => match *c {
                Control::TextArt { text, .. } => text,
                other => panic!("expected TextArt, got {other:?}"),
            },
            other => panic!("expected a control run, got {other:?}"),
        }
    }

    /// Issue #199: Hancom's `␍␊` is one line break; Core carries it as `\r\n`
    /// like the HWP5 record does.
    /// 이것을 실패시키는 것: 디코더가 `␍␊` 를 풀지 않음.
    #[test]
    fn textart_hancom_pair_decodes_to_crlf() {
        assert_eq!(text_art_core_text("위␍␊␍␊아래"), "위\r\n\r\n아래");
    }

    /// A lone `␍` or `␊` is a symbol the author typed, not a break.
    /// 이것을 실패시키는 것: 쌍이 아닌 기호도 제어 문자로 풂.
    #[test]
    fn textart_lone_symbols_stay_symbols() {
        assert_eq!(text_art_core_text("a␍b␊c␊␍d"), "a␍b␊c␊␍d");
    }

    /// Another writer's `&#10;` / `&#13;` reaches the decoder as a raw
    /// `\n` / `\r`; Core must still hold the single `\r\n` form.
    /// 이것을 실패시키는 것: 디코더가 `␍␊` 쌍만 풀고 단독 CR·LF 를 그대로 둠.
    #[test]
    fn textart_raw_lone_breaks_fold_to_crlf() {
        assert_eq!(text_art_core_text("a\nb\rc\r\nd"), "a\r\nb\r\nc\r\nd");
    }

    /// The documented collision: the wire cannot tell a typed `␍␊` from a
    /// break, so Core text holding the literal pair comes back as `\r\n`.
    /// 이것을 실패시키는 것: 디코더가 `␍␊` 를 풀지 않음.
    #[test]
    fn textart_literal_pair_round_trips_as_a_break() {
        use crate::wire_xml::textart_text_to_wire;
        let wire = textart_text_to_wire("a␍␊b");
        assert_eq!(wire, "a␍␊b");
        assert_eq!(text_art_core_text(&wire), "a\r\nb");
    }

    /// 이것을 실패시키는 것: 끝 줄바꿈을 잘라 냄(로컬 corpus 56개는 `글맵시 1␍␊` 처럼 끝에 붙음).
    #[test]
    fn textart_trailing_break_survives_round_trip() {
        use crate::wire_xml::textart_text_to_wire;
        assert_eq!(textart_text_to_wire("글맵시 1\r\n"), "글맵시 1␍␊");
        assert_eq!(text_art_core_text("글맵시 1␍␊"), "글맵시 1\r\n");
    }
}

/// Group (묶음) decoding against Hancom's own "ungroup" answers.
#[cfg(test)]
mod group_tests {
    use hwpforge_core::control::{Control, GroupChild};
    use hwpforge_core::run::RunContent;
    use hwpforge_foundation::Flip;

    use crate::decoder::group_geometry::{bounds, corners, wire_chain, Affine};
    use crate::schema::section::{HxRun, HxSection};
    use crate::HwpxDecoder;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/shapes/");

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("{FIXTURES}{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    fn section0_xml(bytes: &[u8]) -> String {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut s = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("Contents/section0.xml").unwrap(), &mut s)
            .unwrap();
        s
    }

    /// One shape on the page: kind, page coordinates of its box corners and
    /// defining points, rotation and horizontal mirror.
    #[derive(Debug)]
    struct PageShape {
        kind: &'static str,
        corners: Vec<(f64, f64)>,
        points: Vec<(f64, f64)>,
        angle: i32,
        flip: bool,
    }

    /// Top-level shapes of an ungrouped fixture, read straight from the wire:
    /// page = `<hp:pos>` − box minimum + matrix chain · point.
    fn ungrouped_shapes(bytes: &[u8]) -> Vec<PageShape> {
        let xml = section0_xml(bytes);
        let sec: HxSection = crate::decoder::xml_from_str(&xml).unwrap();
        let runs: Vec<&HxRun> = sec.paragraphs.iter().flat_map(|p| p.runs.iter()).collect();
        let mut out = Vec::new();
        macro_rules! push {
            ($s:expr, $kind:expr, $pts:expr) => {{
                let s = $s;
                let m = wire_chain(s.rendering_info.as_ref().unwrap()).unwrap();
                let org = s.org_sz.as_ref().unwrap();
                let cs: Vec<(f64, f64)> = corners(f64::from(org.width), f64::from(org.height))
                    .iter()
                    .map(|&p| m.apply(p))
                    .collect();
                let (mx, my, _, _) = bounds(&cs).unwrap();
                let pos = s.pos.as_ref().unwrap();
                let base = (f64::from(pos.horz_offset) - mx, f64::from(pos.vert_offset) - my);
                let page = |p: (f64, f64)| {
                    let (x, y) = m.apply(p);
                    (x + base.0, y + base.1)
                };
                let pts: Vec<(f64, f64)> = $pts;
                out.push(PageShape {
                    kind: $kind,
                    corners: cs.iter().map(|&(x, y)| (x + base.0, y + base.1)).collect(),
                    points: pts.into_iter().map(page).collect(),
                    angle: s.rotation_info.as_ref().map_or(0, |r| r.angle),
                    flip: s.flip.as_ref().is_some_and(|f| f.horizontal == 1),
                });
            }};
        }
        let pt = |p: &Option<crate::schema::section::HxPoint>| {
            let p = p.as_ref().unwrap();
            (f64::from(p.x), f64::from(p.y))
        };
        for r in runs {
            for s in &r.rects {
                push!(s, "rect", vec![]);
            }
            for s in &r.ellipses {
                push!(s, "ellipse", vec![pt(&s.center), pt(&s.ax1), pt(&s.ax2)]);
            }
            for s in &r.polygons {
                push!(
                    s,
                    "polygon",
                    s.points.iter().map(|p| (f64::from(p.x), f64::from(p.y))).collect()
                );
            }
            for s in &r.lines {
                push!(s, "line", vec![pt(&s.start_pt), pt(&s.end_pt)]);
            }
        }
        out
    }

    /// The decoded group's leaves on the page, placed the way the encoder
    /// writes them: group `<hp:pos>` + accumulated offsets + the rotation
    /// matrix `build_shape_common` makes from the Core value.
    fn grouped_shapes(bytes: &[u8]) -> (Control, Vec<PageShape>) {
        let doc = HwpxDecoder::decode(bytes).unwrap();
        let group = doc.document.sections()[0]
            .paragraphs
            .iter()
            .flat_map(|p| p.runs.iter())
            .find_map(|r| match &r.content {
                RunContent::Control(c) if matches!(**c, Control::Group { .. }) => {
                    Some((**c).clone())
                }
                _ => None,
            })
            .expect("a group");
        let Control::Group { placement, .. } = &group else { unreachable!() };
        let p = placement.as_ref().expect("floating group");
        let origin = (f64::from(p.horz_offset.as_i32()), f64::from(p.vert_offset.as_i32()));
        let mut out = Vec::new();
        collect_leaves(&group, origin, &mut out, true);
        (group, out)
    }

    fn offset_of(placement: &Option<hwpforge_core::placement::ObjectPlacement>) -> (f64, f64) {
        placement.as_ref().map_or((0.0, 0.0), |p| {
            (f64::from(p.horz_offset.as_i32()), f64::from(p.vert_offset.as_i32()))
        })
    }

    fn collect_leaves(c: &Control, base: (f64, f64), out: &mut Vec<PageShape>, top: bool) {
        use hwpforge_core::control::ShapePoint;
        let style_of = |s: &Option<hwpforge_core::control::ShapeStyle>| s.clone();
        let (kind, w, h, placement, style, pts): (&str, i32, i32, _, _, Vec<ShapePoint>) = match c {
            Control::Group { children, placement, .. } => {
                let o = if top { (0.0, 0.0) } else { offset_of(placement) };
                for child in children {
                    if let GroupChild::Shape(child) = child {
                        collect_leaves(child, (base.0 + o.0, base.1 + o.1), out, false);
                    }
                }
                return;
            }
            Control::Rect { width, height, placement, style, .. } => {
                ("rect", width.as_i32(), height.as_i32(), placement, style_of(style), vec![])
            }
            Control::Ellipse { width, height, placement, style, center, axis1, axis2, .. } => (
                "ellipse",
                width.as_i32(),
                height.as_i32(),
                placement,
                style_of(style),
                vec![*center, *axis1, *axis2],
            ),
            Control::Polygon { width, height, placement, style, vertices, .. } => (
                "polygon",
                width.as_i32(),
                height.as_i32(),
                placement,
                style_of(style),
                vertices.clone(),
            ),
            Control::Line { width, height, placement, style, start, end, .. } => (
                "line",
                width.as_i32(),
                height.as_i32(),
                placement,
                style_of(style),
                vec![*start, *end],
            ),
            other => panic!("unexpected leaf {other:?}"),
        };
        let o = offset_of(placement);
        let rot = Affine::from_hx(
            &crate::encoder::shapes::build_shape_common(c).rendering_info.rot_matrix,
        )
        .unwrap();
        let map = Affine::translate(base.0 + o.0, base.1 + o.1).then_after(rot);
        out.push(PageShape {
            kind,
            corners: corners(f64::from(w), f64::from(h)).iter().map(|&p| map.apply(p)).collect(),
            points: pts.iter().map(|p| map.apply((f64::from(p.x), f64::from(p.y)))).collect(),
            angle: style.as_ref().and_then(|s| s.rotation).map_or(0, |r| r.round() as i32),
            flip: style.as_ref().and_then(|s| s.flip) == Some(Flip::Horizontal),
        });
    }

    fn dist(a: &[(f64, f64)], b: &[(f64, f64)]) -> f64 {
        assert_eq!(a.len(), b.len());
        a.iter().zip(b).map(|(p, q)| (p.0 - q.0).hypot(p.1 - q.1)).fold(0.0, f64::max)
    }

    /// Every leaf of the grouped fixture against the ungrouped one: defining
    /// points within 2 HWPUNIT; boxes (rect, ellipse) also their corners,
    /// rotation and mirror.
    fn assert_matches_ungrouped(grouped: &str, ungrouped: &str) {
        let doc = HwpxDecoder::decode(&fixture(grouped)).unwrap();
        assert!(doc.warnings.is_empty(), "{grouped}: {:?}", doc.warnings);
        assert_leaves_match(grouped, ungrouped, 0, &[]);
    }

    /// Every leaf of the first group of `grouped` against Hancom's ungrouped
    /// shapes, except the leaves of `skip` kinds; `extra` = shapes outside the
    /// group, present in both files.
    fn assert_leaves_match(grouped: &str, ungrouped: &str, extra: usize, skip: &[&str]) {
        assert_bytes_match(grouped, &fixture(grouped), ungrouped, extra, skip);
    }

    fn assert_bytes_match(
        grouped: &str,
        bytes: &[u8],
        ungrouped: &str,
        extra: usize,
        skip: &[&str],
    ) {
        let (_, g) = grouped_shapes(bytes);
        let mut u = ungrouped_shapes(&fixture(ungrouped));
        assert_eq!(g.len() + extra, u.len(), "{grouped}: leaf count");
        for leaf in g.iter().filter(|l| !skip.contains(&l.kind)) {
            let box_kind = matches!(leaf.kind, "rect" | "ellipse");
            let score = |s: &PageShape| {
                if s.kind != leaf.kind {
                    return f64::INFINITY;
                }
                if box_kind {
                    dist(&s.corners, &leaf.corners).max(dist(&s.points, &leaf.points))
                } else {
                    dist(&s.points, &leaf.points)
                }
            };
            let (i, best) = u
                .iter()
                .enumerate()
                .map(|(i, s)| (i, score(s)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap();
            assert!(
                best <= 2.0,
                "{grouped}: {} off by {best:.2}: {leaf:?} vs {:?}",
                leaf.kind,
                u[i]
            );
            if box_kind {
                assert_eq!(
                    (leaf.angle, leaf.flip),
                    (u[i].angle, u[i].flip),
                    "{grouped}: {}",
                    leaf.kind
                );
            }
            u.remove(i);
        }
    }

    /// F1: four groups made in Hancom and the same groups after "ungroup".
    ///
    /// 이것을 실패시키는 것: 쌍을 첫 쌍부터 적용하는 것, 또는 회전·반사를
    /// 자식의 `rotationInfo`·`flip` 에서 읽는 것 (g2·g3 가 어긋난다).
    #[test]
    fn f1_groups_match_hancom_ungroup_per_child() {
        // (see the F9, F10 and F8 tests below for turned, sheared and
        // point-edited groups)
        for (g, u) in [
            ("group-f1-g1-scaled.hwpx", "group-f1-g1-ungrouped.hwpx"),
            ("group-f1-g2-rotated.hwpx", "group-f1-g2-ungrouped.hwpx"),
            ("group-f1-g3-flipped.hwpx", "group-f1-g3-ungrouped.hwpx"),
            ("group-f1-g4-nested.hwpx", "group-f1-g4-ungrouped.hwpx"),
        ] {
            assert_matches_ungrouped(g, u);
        }
    }

    /// F9: rect A (turned 30° then mirrored), triangle B (turned 30° then
    /// mirrored top-bottom) and ellipse C (mirrored) grouped, the group turned
    /// 20° and mirrored. Hancom's ungroup gives A 10°, B 10° mirrored both
    /// ways, C 340°; every leaf lands within 2 HWPUNIT of it, A and C with the
    /// same angle and mirror. D and E stay outside the group in both files.
    ///
    /// 이것을 실패시키는 것: 쌍 적용 순서를 첫 쌍부터로 바꾸는 것(묶음의 반사·회전이
    /// 자식 자신의 것보다 먼저 적용됨), 최상위 묶음 원점을 묶음 자기 frame 의
    /// 최솟값으로 되돌리는 것. (여기 자식들은 반사가 둘씩 겹쳐 상쇄되므로 반사
    /// 판정은 `box_child_mirrored_and_turned_is_carried` 가 잡는다.)
    #[test]
    fn f9_turned_and_mirrored_group_matches_hancom_ungroup() {
        let doc = HwpxDecoder::decode(&fixture("rotflip-group.hwpx")).unwrap();
        assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
        assert_leaves_match("rotflip-group.hwpx", "rotflip-group-ungrouped.hwpx", 2, &[]);
    }

    /// The same F9 group decoded, written by this crate and decoded again:
    /// still within 2 HWPUNIT of Hancom's ungroup, leaf by leaf, with no
    /// warning. (The leaves here are placed with the writer's own matrix, so
    /// a writer bug that the decoder folds back is caught by
    /// `writer_matrix_matches_hancom_at_the_native_size` instead.)
    ///
    /// 이것을 실패시키는 것: 최상위 묶음 원점을 묶음 자기 frame 의 최솟값으로
    /// 되돌리는 것.
    #[test]
    fn f9_group_survives_our_writer() {
        let doc = HwpxDecoder::decode(&fixture("rotflip-group.hwpx")).unwrap();
        let bytes = encode(&doc);
        let back = HwpxDecoder::decode(&bytes).unwrap();
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        assert_bytes_match("ours", &bytes, "rotflip-group-ungrouped.hwpx", 2, &[]);
    }

    /// F10: a rect and a triangle turned 30° and a line turned 45° on their
    /// own, then the group stretched along x only. Hancom's ungroup keeps the
    /// shear in each shape's own `scaMatrix`; Core boxes cannot, so the rect
    /// is reported once, while the triangle and line (points only) land
    /// within 2 HWPUNIT of Hancom's.
    ///
    /// 이것을 실패시키는 것: 내용 없는 점 기반 도형을 상자 규칙으로 보내는 것.
    /// (shear 판정만 지우면 같은 사각형이 잔차로 보고되어 살아남는다 — shear
    /// 자체는 core `fold_box_refuses_what_it_cannot_place` 가 잡는다.)
    #[test]
    fn f10_sheared_group_keeps_points_and_reports_the_box() {
        let doc = HwpxDecoder::decode(&fixture("group-f10-rot-then-scale.hwpx")).unwrap();
        assert_eq!(lost_kinds(&doc.warnings), ["group_child_transform"]);
        assert_leaves_match(
            "group-f10-rot-then-scale.hwpx",
            "group-f10-rot-then-scale-ungrouped.hwpx",
            0,
            &["rect"],
        );
    }

    /// F8: a point-edited pentagon (Hancom re-bases its points onto
    /// `[0, orgSz]`) grouped with a rect, the group scaled ×1.5 by ×0.5.
    ///
    /// 이것을 실패시키는 것: 다각형 점에 묶음 scale 을 적용하지 않는 것.
    #[test]
    fn f8_point_edited_polygon_matches_hancom_ungroup() {
        assert_matches_ungrouped(
            "group-f8-poly-margin.hwpx",
            "group-f8-poly-margin-ungrouped.hwpx",
        );
    }

    // ── synthetic wire ─────────────────────────────────────────────

    const ID: [f64; 6] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

    fn mat(tag: &str, e: [f64; 6]) -> String {
        format!(
            r#"<hc:{tag} e1="{}" e2="{}" e3="{}" e4="{}" e5="{}" e6="{}"/>"#,
            e[0], e[1], e[2], e[3], e[4], e[5]
        )
    }

    /// `<hp:renderingInfo>` with `trans` and `(sca, rot)` pairs in wire order.
    fn ri(trans: [f64; 6], pairs: &[([f64; 6], [f64; 6])]) -> String {
        let mut s = format!("<hp:renderingInfo>{}", mat("transMatrix", trans));
        for (sca, rot) in pairs {
            s.push_str(&mat("scaMatrix", *sca));
            s.push_str(&mat("rotMatrix", *rot));
        }
        s + "</hp:renderingInfo>"
    }

    fn rot(deg: f64) -> [f64; 6] {
        let (s, c) = deg.to_radians().sin_cos();
        [c, -s, 0.0, s, c, 0.0]
    }

    fn shape_head(tag: &str, org: (i32, i32), ri: &str) -> String {
        format!(
            r#"<hp:{tag} id="1" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="1" instid="7" hasArcPr="0" arcType="NORMAL"><hp:offset x="0" y="0"/><hp:orgSz width="{}" height="{}"/><hp:curSz width="0" height="0"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="0" centerY="0" rotateimage="1"/>{ri}"#,
            org.0, org.1
        )
    }

    fn rect(org: (i32, i32), ri: &str) -> String {
        format!("{}</hp:rect>", shape_head("rect", org, ri))
    }

    fn polygon(org: (i32, i32), ri: &str, pts: &[(i32, i32)], inner: &str) -> String {
        let pts: String = pts.iter().map(|(x, y)| format!(r#"<hc:pt x="{x}" y="{y}"/>"#)).collect();
        format!("{}{inner}{pts}</hp:polygon>", shape_head("polygon", org, ri))
    }

    fn ellipse(org: (i32, i32), ri: &str) -> String {
        let (w, h) = org;
        format!(
            r#"{}<hc:center x="{}" y="{}"/><hc:ax1 x="{w}" y="{}"/><hc:ax2 x="{}" y="{h}"/></hp:ellipse>"#,
            shape_head("ellipse", org, ri),
            w / 2,
            h / 2,
            h / 2,
            w / 2
        )
    }

    fn draw_text(text: &str) -> String {
        format!(
            r#"<hp:drawText lastWidth="0" name="" editable="0"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>{text}</hp:t></hp:run></hp:p></hp:subList></hp:drawText>"#
        )
    }

    /// A top-level container with identity matrices whose box is `org`.
    /// A top-level group without `<hp:sz>`: its box is then the bounds of
    /// its shapes, as Hancom's always is (the synthetic children here do not
    /// fill the group's frame).
    fn container(org: (i32, i32), children: &str) -> String {
        container_xml(org, None, &ri(ID, &[(ID, ID)]), children)
    }

    fn container_with(org: (i32, i32), sz: (i32, i32), own: &str, children: &str) -> String {
        container_xml(org, Some(sz), own, children)
    }

    fn container_xml(org: (i32, i32), sz: Option<(i32, i32)>, own: &str, children: &str) -> String {
        let sz = sz.map_or_else(String::new, |(w, h)| {
            format!(
                r#"<hp:sz width="{w}" widthRelTo="ABSOLUTE" height="{h}" heightRelTo="ABSOLUTE" protect="0"/>"#
            )
        });
        format!(
            r#"<hp:container id="9" zOrder="0" numberingType="PICTURE" textWrap="SQUARE" textFlow="LARGEST_ONLY" lock="0" dropcapstyle="None" href="" groupLevel="0" instid="900"><hp:offset x="0" y="0"/><hp:orgSz width="{}" height="{}"/><hp:curSz width="0" height="0"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="0" centerY="0" rotateimage="1"/>{own}{children}{sz}<hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="0" allowOverlap="1" holdAnchorAndSO="0" vertRelTo="PAPER" horzRelTo="PAPER" vertAlign="TOP" horzAlign="LEFT" vertOffset="5000" horzOffset="6000"/></hp:container>"#,
            org.0, org.1
        )
    }

    fn nested(org: (i32, i32), own: &str, children: &str) -> String {
        format!(
            r#"<hp:container id="8" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="1" instid="800"><hp:offset x="0" y="0"/><hp:orgSz width="{}" height="{}"/><hp:curSz width="0" height="0"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="0" centerY="0" rotateimage="1"/>{own}{children}</hp:container>"#,
            org.0, org.1
        )
    }

    fn section(run_inner: &str) -> String {
        format!(
            r#"<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0">{run_inner}</hp:run></hp:p></hs:sec>"#
        )
    }

    /// Decodes one section; returns its first group (if any) and the warnings.
    fn decode(run_inner: &str) -> (Option<Control>, Vec<crate::DecodeWarning>) {
        let r = crate::decoder::section::parse_section(
            &section(run_inner),
            0,
            &std::collections::HashMap::new(),
        )
        .unwrap();
        let group = r.paragraphs[0].runs.iter().find_map(|run| match &run.content {
            RunContent::Control(c) if matches!(**c, Control::Group { .. }) => Some((**c).clone()),
            _ => None,
        });
        (group, r.warnings)
    }

    fn children(group: &Control) -> Vec<&Control> {
        match group {
            Control::Group { children, .. } => children
                .iter()
                .map(|c| match c {
                    GroupChild::Shape(c) => c,
                    other => panic!("not a shape: {other:?}"),
                })
                .collect(),
            other => panic!("not a group: {other:?}"),
        }
    }

    fn lost_kinds(warnings: &[crate::DecodeWarning]) -> Vec<String> {
        warnings
            .iter()
            .filter_map(|w| match w {
                crate::DecodeWarning::ContentLost { kind, .. } => Some(kind.clone()),
                _ => None,
            })
            .collect()
    }

    fn size_and_offset(c: &Control) -> (i32, i32, (f64, f64)) {
        match c {
            Control::Rect { width, height, placement, .. }
            | Control::Ellipse { width, height, placement, .. }
            | Control::Polygon { width, height, placement, .. }
            | Control::Line { width, height, placement, .. }
            | Control::Group { width, height, placement, .. } => {
                (width.as_i32(), height.as_i32(), offset_of(placement))
            }
            other => panic!("unexpected child {other:?}"),
        }
    }

    fn rotation(c: &Control) -> (Option<f32>, Option<Flip>) {
        let style = match c {
            Control::Rect { style, .. }
            | Control::Ellipse { style, .. }
            | Control::Polygon { style, .. }
            | Control::Line { style, .. } => style,
            other => panic!("unexpected child {other:?}"),
        };
        style.as_ref().map_or((None, None), |s| (s.rotation, s.flip))
    }

    /// First pair `S1 = [0.5, 0, 310; 0, 0.5, 0]`: a 1000×400 rect becomes
    /// 500×200 at x = 310 (the scale's own translation).
    ///
    /// 이것을 실패시키는 것: sca 행렬의 이동 성분(e3·e6)을 버리는 것 (x = 0).
    #[test]
    fn first_pair_translation_moves_the_child() {
        let s1 = [0.5, 0.0, 310.0, 0.0, 0.5, 0.0];
        let (g, w) =
            decode(&container((810, 200), &rect((1000, 400), &ri(ID, &[(s1, ID), (ID, ID)]))));
        assert!(w.is_empty(), "{w:?}");
        let g = g.unwrap();
        assert_eq!(size_and_offset(children(&g)[0]), (500, 200, (310.0, 0.0)));
    }

    /// A polygon turned 30° on its own, then stretched 2× along x by the
    /// group: the map has shear, which only points can hold. Expected
    /// vertices are `S · R30 · p` minus their minimum, computed outside the
    /// decoder: (0,270) (1466,0) (1732,770), offset (−600, 250).
    ///
    /// 이것을 실패시키는 것: 내용 없는 점 기반 도형을 상자 규칙으로 보내는 것
    /// (shear 로 `ContentLost` 가 난다).
    #[test]
    fn point_shape_takes_a_sheared_map_on_its_points() {
        let s = [2.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let child = polygon(
            (1000, 600),
            &ri(ID, &[(s, ID), (ID, rot(30.0))]),
            &[(0, 600), (500, 0), (1000, 600)],
            "",
        );
        let (g, w) = decode(&container((3000, 3000), &child));
        assert!(w.is_empty(), "{w:?}");
        let g = g.unwrap();
        let Control::Polygon { vertices, .. } = &children(&g)[0] else { panic!() };
        let v: Vec<(i32, i32)> = vertices.iter().map(|p| (p.x, p.y)).collect();
        assert_eq!(v, [(0, 270), (1466, 0), (1732, 770)]);
        assert_eq!(size_and_offset(children(&g)[0]), (1732, 770, (-600.0, 250.0)));
        assert_eq!(rotation(children(&g)[0]), (None, None));
    }

    /// A rect in a group the user turned 30°: the rotation is the rect's own.
    ///
    /// 이것을 실패시키는 것: 상자 규칙에서 회전을 조건 밖으로 처리하거나 회전을
    /// 스타일로 옮기지 않는 것 (재인코드 잔차로 `ContentLost`).
    #[test]
    fn box_child_carries_the_group_rotation() {
        let (g, w) = decode(&container(
            (3000, 3000),
            &rect((1000, 400), &ri(ID, &[(ID, rot(30.0)), (ID, ID)])),
        ));
        assert!(w.is_empty(), "{w:?}");
        let g = g.unwrap();
        assert_eq!(rotation(children(&g)[0]), (Some(30.0), None));
        assert_eq!(size_and_offset(children(&g)[0]).0, 1000);
    }

    /// A rect mirrored and turned 30° by its group folds to a 30° turn with a
    /// horizontal mirror, carried without loss (the writer side is pinned by
    /// `f9_group_survives_our_writer` and `shape_rotation_flip`).
    ///
    /// 이것을 실패시키는 것: 접기에서 반사(행렬식 음수) 판정을 지우는 것.
    #[test]
    fn box_child_mirrored_and_turned_is_carried() {
        let mirror = [-1.0, 0.0, 1000.0, 0.0, 1.0, 0.0];
        let (g, w) = decode(&container(
            (3000, 3000),
            &rect((1000, 400), &ri(ID, &[(mirror, rot(30.0)), (ID, ID)])),
        ));
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(rotation(children(&g.unwrap())[0]), (Some(30.0), Some(Flip::Horizontal)));
    }

    /// Shapes the group squashes flat have no Core shape: a rect with a zero
    /// axis, a rect whose height rounds to 0, a polygon whose points end up
    /// on one line. Each is dropped and reported, so the document still
    /// validates.
    ///
    /// 이것을 실패시키는 것: 세 갈래(축 0 상자, 반올림 0 상자, 일직선 다각형) 중
    /// 하나의 방출을 지우는 것 (각각 따로 — 경고 수가 3 → 2).
    #[test]
    fn collapsed_children_are_dropped_and_reported() {
        let flat = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let thin = [1.0, 0.0, 0.0, 0.0, 0.4, 0.0];
        let keep = rect((500, 500), &ri(ID, &[(ID, ID), (ID, ID)]));
        let zero_axis = rect((1000, 400), &ri(ID, &[(flat, ID), (ID, ID)]));
        let rounds_to_zero = rect((1000, 1), &ri(ID, &[(thin, ID), (ID, ID)]));
        let on_a_line = polygon(
            (1000, 600),
            &ri(ID, &[(flat, ID), (ID, ID)]),
            &[(0, 600), (500, 0), (1000, 600)],
            "",
        );
        let (g, w) = decode(&container(
            (1000, 500),
            &format!("{keep}{zero_axis}{rounds_to_zero}{on_a_line}"),
        ));
        assert_eq!(children(&g.unwrap()).len(), 1);
        assert_eq!(lost_kinds(&w), ["group_child_degenerate"; 3]);
    }

    /// A rect turned on its own, then stretched unevenly by the group: a box
    /// cannot hold the shear, so the child is reported once.
    ///
    /// 이것을 실패시키는 것: shear 판정(`SHEAR_TOLERANCE`) 갈래의 방출을 지우고
    /// 잔차 보고도 끄는 것.
    #[test]
    fn sheared_box_child_is_content_lost_once() {
        let s = [2.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let (g, w) = decode(&container(
            (3000, 3000),
            &rect((1000, 400), &ri(ID, &[(s, ID), (ID, rot(30.0))])),
        ));
        assert!(g.is_some());
        assert_eq!(lost_kinds(&w), ["group_child_transform"]);
    }

    /// An ellipse in a group scaled by 0.5: centre and axis points scale too.
    ///
    /// 이것을 실패시키는 것: 상자 규칙에서 정의 점에 대각 scale 을 적용하지 않는
    /// 것 (중심·축이 원래 좌표로 남아 잔차 `ContentLost`, 아래 단언도 실패).
    #[test]
    fn ellipse_points_scale_with_the_box() {
        let half = [0.5, 0.0, 0.0, 0.0, 0.5, 0.0];
        let (g, w) = decode(&container(
            (2000, 2000),
            &ellipse((2000, 1000), &ri(ID, &[(half, ID), (ID, ID)])),
        ));
        assert!(w.is_empty(), "{w:?}");
        let g = g.unwrap();
        let Control::Ellipse { center, axis1, axis2, width, height, .. } = &children(&g)[0] else {
            panic!()
        };
        assert_eq!((width.as_i32(), height.as_i32()), (1000, 500));
        assert_eq!(
            (center.x, center.y, axis1.x, axis1.y, axis2.x, axis2.y),
            (500, 250, 1000, 250, 500, 500)
        );
    }

    /// A polygon carrying text in a group turned 30°: the text must turn with
    /// it, so the polygon keeps its points (scaled only) and gets rotation 30.
    /// With a shear instead, it is reported.
    ///
    /// 이것을 실패시키는 것: 내용 있는 점 기반 도형을 내용 없는 경로로 보내는 것
    /// (회전이 점에 접혀 rotation 단언이 실패한다).
    #[test]
    fn text_polygon_keeps_its_own_rotation() {
        let pts = [(0, 600), (500, 0), (1000, 600)];
        let child =
            polygon((1000, 600), &ri(ID, &[(ID, rot(30.0)), (ID, ID)]), &pts, &draw_text("글"));
        let (g, w) = decode(&container((3000, 3000), &child));
        assert!(w.is_empty(), "{w:?}");
        let g = g.unwrap();
        let c = &children(&g)[0];
        assert_eq!(rotation(c), (Some(30.0), None));
        let Control::Polygon { vertices, paragraphs, .. } = c else { panic!() };
        assert_eq!(vertices.iter().map(|p| (p.x, p.y)).collect::<Vec<_>>(), pts);
        assert_eq!(paragraphs[0].text_content(), "글");

        let s = [2.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let sheared =
            polygon((1000, 600), &ri(ID, &[(s, ID), (ID, rot(30.0))]), &pts, &draw_text("글"));
        let (_, w) = decode(&container((3000, 3000), &sheared));
        assert_eq!(lost_kinds(&w), ["group_child_transform"]);
    }

    /// A nested group is sized by its children at their exact Core position:
    /// a 10,000 × 1,000 rect turned 30.4° spans 9131.2 × 5922.9 (±1 for the
    /// integer offset and bounds), not the 9160 × 5866 of the whole-degree
    /// matrix HWPX would store.
    ///
    /// 이것을 실패시키는 것: 중첩 묶음 상자를 정확한 정의 대신 HWPX 방출 정책
    /// (정수 도·6자리)으로 계산하는 것.
    #[test]
    fn nested_group_box_uses_the_exact_child_geometry() {
        let leaf = rect((10_000, 1_000), &ri(ID, &[(ID, ID), (ID, ID), (ID, rot(30.4))]));
        let own = ri(ID, &[(ID, ID), (ID, ID)]);
        let (g, _) = decode(&container((30_000, 30_000), &nested((10_000, 1_000), &own, &leaf)));
        let g = g.unwrap();
        let (w, h, _) = size_and_offset(children(&g)[0]);
        assert!(
            (f64::from(w) - 9131.2).abs() <= 1.0 && (f64::from(h) - 5922.9).abs() <= 1.0,
            "{w}x{h}"
        );
    }

    /// A nested group whose own box does not match its shapes.
    ///
    /// 이것을 실패시키는 것: 중첩 묶음의 자기 상자 비교를 지우는 것.
    #[test]
    fn nested_group_box_mismatch_is_content_lost() {
        let leaf = rect((1000, 1000), &ri(ID, &[(ID, ID), (ID, ID), (ID, ID)]));
        let own = ri([1.0, 0.0, 500.0, 0.0, 1.0, 0.0], &[(ID, ID), (ID, ID)]);
        let (g, w) = decode(&container((3000, 3000), &nested((1000, 1000), &own, &leaf)));
        assert!(g.is_some());
        assert_eq!(lost_kinds(&w), ["nested_group_placement"]);

        // Same group with a matching box: no warning.
        let own = ri(ID, &[(ID, ID), (ID, ID)]);
        let (_, w) = decode(&container((3000, 3000), &nested((1000, 1000), &own, &leaf)));
        assert!(w.is_empty(), "{w:?}");
    }

    /// A child without matrices sits at its `<hp:offset>`, read as signed:
    /// Hancom writes −310 as `4294966986`.
    ///
    /// 이것을 실패시키는 것: `HxOffset` 에서 부호 헬퍼(`deser_i32_or_u32`)를 빼는
    /// 것 (값이 i32 범위 밖이라 디코드가 실패한다).
    #[test]
    fn negative_offset_written_as_u32_is_signed() {
        let child = r#"<hp:rect id="1" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="1" instid="7" ratio="0"><hp:offset x="4294966986" y="0"/><hp:orgSz width="1000" height="400"/></hp:rect>"#;
        let (g, w) = decode(&container((1000, 400), child));
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(size_and_offset(children(&g.unwrap())[0]).2, (-310.0, 0.0));
    }

    /// A `scaMatrix` without its `rotMatrix` is malformed, not silently cut.
    ///
    /// 이것을 실패시키는 것: 짝 검사를 지우고 남은 행렬을 버리는 것.
    #[test]
    fn unpaired_matrix_is_a_parse_error() {
        let bad = format!(
            "<hp:renderingInfo>{}{}{}{}</hp:renderingInfo>",
            mat("transMatrix", ID),
            mat("scaMatrix", ID),
            mat("rotMatrix", ID),
            mat("scaMatrix", ID)
        );
        let err = crate::decoder::section::parse_section(
            &section(&container((1000, 400), &rect((1000, 400), &bad))),
            0,
            &std::collections::HashMap::new(),
        )
        .unwrap_err();
        assert!(matches!(err, crate::HwpxError::XmlParse { .. }), "{err:?}");
    }

    /// `renderingInfo` must start with `transMatrix` and carry at least one
    /// pair.
    ///
    /// 이것을 실패시키는 것: 첫 행렬 검사 또는 "쌍 0개" 검사를 지우는 것 (각각 따로).
    #[test]
    fn rendering_info_without_trans_or_pairs_is_a_parse_error() {
        for bad in [
            // A pair after a stray first matrix — only the `transMatrix`-first
            // check can reject this one.
            format!(
                "<hp:renderingInfo>{}{}{}</hp:renderingInfo>",
                mat("rotMatrix", ID),
                mat("scaMatrix", ID),
                mat("rotMatrix", ID)
            ),
            format!("<hp:renderingInfo>{}</hp:renderingInfo>", mat("transMatrix", ID)),
        ] {
            let err = crate::decoder::section::parse_section(
                &section(&container((1000, 400), &rect((1000, 400), &bad))),
                0,
                &std::collections::HashMap::new(),
            )
            .unwrap_err();
            assert!(matches!(err, crate::HwpxError::XmlParse { .. }), "{err:?}");
        }
    }

    /// A matrix element that is not a number leaves the child without a
    /// place: it is dropped and reported.
    ///
    /// 이것을 실패시키는 것: 해석 불가 행렬 갈래의 방출을 지우는 것.
    #[test]
    fn unparseable_child_matrix_is_reported() {
        let bad = ri(ID, &[(ID, ID)]).replacen(r#"e3="0""#, r#"e3="x""#, 1);
        let keep = rect((500, 500), &ri(ID, &[(ID, ID), (ID, ID)]));
        let (g, w) = decode(&container((1000, 500), &format!("{keep}{}", rect((1000, 400), &bad))));
        assert_eq!(children(&g.unwrap()).len(), 1);
        assert_eq!(lost_kinds(&w), ["group_child_transform"]);
    }

    /// A nested group holding a picture with no image: the picture is
    /// reported, and its absence from Core does not also count as a
    /// misplaced group box.
    ///
    /// 이것을 실패시키는 것: 미지원 자식이 있는 중첩 묶음에서도 상자를 비교하는 것
    /// (`nested_group_placement` 가 더해진다).
    #[test]
    fn nested_group_with_a_picture_is_reported_once() {
        let leaf = rect((1000, 1000), &ri(ID, &[(ID, ID), (ID, ID), (ID, ID)]));
        let pic = r#"<hp:pic id="2" groupLevel="2"><hp:offset x="0" y="0"/></hp:pic>"#;
        // The group box spans the picture too (2000 wide), the rect only 1000.
        let own = ri(ID, &[(ID, ID), (ID, ID)]);
        let (g, w) =
            decode(&container((3000, 3000), &nested((2000, 1000), &own, &format!("{leaf}{pic}"))));
        assert!(g.is_some());
        assert_eq!(lost_kinds(&w), ["pic"]);
    }

    /// A nested group without its own matrices cannot be checked against its
    /// shapes; one holding only a picture with no image has nothing to place.
    ///
    /// 이것을 실패시키는 것: 중첩 묶음의 "행렬 없음" 갈래 방출을 지우는 것, 또는
    /// 잎이 없는 중첩 묶음에서 자식 디코드(그림 보고)를 건너뛰는 것.
    #[test]
    fn nested_group_without_matrices_or_shapes_is_reported() {
        let leaf = rect((1000, 1000), &ri(ID, &[(ID, ID), (ID, ID), (ID, ID)]));
        let no_matrix = nested((1000, 1000), "", &leaf);
        let (g, w) = decode(&container((3000, 3000), &no_matrix));
        assert!(g.is_some());
        assert_eq!(lost_kinds(&w), ["nested_group_placement"]);

        let pic = r#"<hp:pic id="2" groupLevel="2"><hp:offset x="0" y="0"/></hp:pic>"#;
        let pics_only = nested((1000, 1000), &ri(ID, &[(ID, ID), (ID, ID)]), pic);
        let keep = rect((500, 500), &ri(ID, &[(ID, ID), (ID, ID)]));
        let (g, w) = decode(&container((3000, 3000), &format!("{keep}{pics_only}")));
        assert_eq!(children(&g.unwrap()).len(), 1);
        assert_eq!(lost_kinds(&w), ["pic", "group"]);
    }

    // ── TextArt in a group (§7 contract; no Hancom sample yet) ───────

    fn solid_brush(face: &str) -> String {
        format!(
            r##"<hc:fillBrush><hc:winBrush faceColor="{face}" hatchColor="#000000" alpha="0"/></hc:fillBrush>"##
        )
    }

    fn text_art(level: u32, ri: &str, brush: &str, tail: &str) -> String {
        format!(
            r##"<hp:textart id="5" zOrder="0" numberingType="PICTURE" textWrap="SQUARE" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="{level}" instid="55" text="묶음글맵시"><hp:offset x="0" y="0"/><hp:orgSz width="14173" height="14173"/><hp:curSz width="0" height="0"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="0" centerY="0" rotateimage="1"/>{ri}<hp:lineShape color="#000000" width="0" style="NONE" endCap="ROUND" headStyle="NORMAL" tailStyle="NORMAL" headfill="0" tailfill="0" headSz="SMALL_SMALL" tailSz="SMALL_SMALL" outlineStyle="INNER" alpha="0"/>{brush}<hc:pt0 x="0" y="0"/><hc:pt1 x="14173" y="0"/><hc:pt2 x="14173" y="14173"/><hc:pt3 x="0" y="14173"/><hp:textartPr fontName="함초롬바탕" fontStyle="보통" fontType="TTF" textShape="WAVE2" lineSpacing="120" charSpacing="100" align="LEFT"/>{tail}</hp:textart>"##
        )
    }

    /// A one-child group holding a TextArt whose first pair scales the
    /// 14173 × 14173 design box by 0.5 × 0.25 (after `first_rot`), placed at
    /// (1200, 800) — or at (5000, 5000) when turned, which keeps the turned
    /// box clear of the group's origin.
    fn grouped_text_art(first_rot: [f64; 6], brush: &str) -> String {
        const HALF_QUARTER: [f64; 6] = [0.5, 0.0, 0.0, 0.0, 0.25, 0.0];
        let (x, y) = if first_rot == ID { (1200.0, 800.0) } else { (5000.0, 5000.0) };
        let ri = ri([1.0, 0.0, x, 0.0, 1.0, y], &[(HALF_QUARTER, first_rot), (ID, ID)]);
        container((14_173, 14_173), &text_art(1, &ri, brush, ""))
    }

    /// (size, offset, colour) of a group's only child, a TextArt.
    fn text_art_geometry(
        group: &Control,
    ) -> ((i32, i32), (i32, i32), Option<hwpforge_foundation::Color>) {
        let Control::TextArt { width, height, placement, fill_color, .. } = &children(group)[0]
        else {
            panic!("not a TextArt: {group:?}")
        };
        let at =
            placement.as_ref().map_or((0, 0), |p| (p.horz_offset.as_i32(), p.vert_offset.as_i32()));
        ((width.as_i32(), height.as_i32()), at, *fill_color)
    }

    /// A TextArt in a group: its size is `orgSz` × the chain's scale
    /// (14173 × 0.5 = 7086.5 → 7087, × 0.25 = 3543.25 → 3543), its place the
    /// chain's translation, its colour the solid `faceColor`. Encoded again
    /// and decoded, it comes back the same, with the colour on the wire.
    ///
    /// 이것을 실패시키는 것: 묶음 글맵시 디코드 갈래를 지우는 것(미지원 손실로
    /// 돌아감), 크기를 `orgSz` 대신 `sz` 에서 읽는 것(0 × 0 으로 버려짐),
    /// `fill_color` 를 `None` 으로 두는 것(재인코드가 기본 파랑).
    #[test]
    fn grouped_text_art_round_trips_size_place_and_colour() {
        let (g, w) = decode(&grouped_text_art(ID, &solid_brush("#FF0000")));
        assert!(w.is_empty(), "{w:?}");
        let g = g.unwrap();
        let red = Some(hwpforge_foundation::Color::from_rgb(255, 0, 0));
        assert_eq!(text_art_geometry(&g), ((7087, 3543), (1200, 800), red));

        let xml = crate::encoder::shapes::encode_group_to_xml(
            &g,
            0,
            0,
            &mut Vec::new(),
            crate::encoder::EncodeOptions::default(),
            &mut crate::encoder::section::EncodeSink::new(0),
        )
        .unwrap();
        assert!(xml.contains(r##"faceColor="#FF0000""##), "{xml}");
        let (again, w) = decode(&xml);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(text_art_geometry(&again.unwrap()), text_art_geometry(&g));
    }

    /// A TextArt in a nested group next to a polygon whose points sit inside
    /// its box: the nested group's box is the bounds of its decoded shapes
    /// (from the polygon's points at (200, 200)), so every child, the TextArt
    /// too, moves by that corner and the TextArt stays at (1200, 800) on the
    /// outer group. The nested group's own box (8287 × 4343) holds the
    /// TextArt, so the wire box check passes only when the TextArt counts.
    ///
    /// 이것을 실패시키는 것: `shift_child` 에서 TextArt 를 빼는 것(글맵시가
    /// (200, 200) 밀림), 잎 경계(`leaf_bounds`)에서 TextArt 를 빼는 것
    /// (`nested_group_placement` 경고).
    #[test]
    fn text_art_in_a_nested_group_keeps_its_place() {
        const HALF_QUARTER: [f64; 6] = [0.5, 0.0, 0.0, 0.0, 0.25, 0.0];
        let three = |sca: [f64; 6]| [(sca, ID), (ID, ID), (ID, ID)];
        let poly =
            polygon((1000, 1000), &ri(ID, &three(ID)), &[(200, 200), (800, 200), (500, 800)], "");
        let art = text_art(
            2,
            &ri([1.0, 0.0, 1200.0, 0.0, 1.0, 800.0], &three(HALF_QUARTER)),
            &solid_brush("#FF0000"),
            "",
        );
        let inner = nested((8287, 4343), &ri(ID, &[(ID, ID), (ID, ID)]), &format!("{poly}{art}"));
        let (g, w) =
            decode(&container_with((8287, 4343), (8287, 4343), &ri(ID, &[(ID, ID)]), &inner));
        assert!(w.is_empty(), "{w:?}");
        let g = g.unwrap();
        let Control::Group { placement, .. } = &children(&g)[0] else { panic!("{g:?}") };
        let at = placement.as_ref().map(|p| (p.horz_offset.as_i32(), p.vert_offset.as_i32()));
        assert_eq!(at, Some((200, 200)));
        let (size, art_at, _) = text_art_geometry(&Control::Group {
            children: vec![hwpforge_core::control::GroupChild::Shape(
                children(children(&g)[0])[1].clone(),
            )],
            width: hwpforge_foundation::HwpUnit::ZERO,
            height: hwpforge_foundation::HwpUnit::ZERO,
            placement: None,
            inst_id: None,
        });
        assert_eq!((size, (art_at.0 + 200, art_at.1 + 200)), ((7087, 3543), (1200, 800)));
    }

    /// F14 (Hancom): a 40 × 20 mm TextArt grouped with a rectangle, the group
    /// at (8559, 11338) on the paper. Returns the decode warnings, the
    /// TextArt's (width, height), its corner on the paper and its colour.
    fn f14_text_art(
        name: &str,
    ) -> (Vec<crate::DecodeWarning>, (i32, i32), (i32, i32), Option<hwpforge_foundation::Color>)
    {
        let doc = HwpxDecoder::decode(&fixture(name)).unwrap();
        let group = first_group(&doc);
        let Control::Group { placement, .. } = &group else { unreachable!() };
        let p = placement.as_ref().expect("floating group");
        let art = children(&group)
            .iter()
            .find(|c| matches!(c, Control::TextArt { .. }))
            .copied()
            .expect("TextArt child")
            .clone();
        let Control::TextArt { width, height, placement: at, fill_color, .. } = art else {
            unreachable!()
        };
        let (x, y) = at.map_or((0, 0), |q| (q.horz_offset.as_i32(), q.vert_offset.as_i32()));
        (
            doc.warnings,
            (width.as_i32(), height.as_i32()),
            (p.horz_offset.as_i32() + x, p.vert_offset.as_i32() + y),
            fill_color,
        )
    }

    /// The one decode warning F14 raises: its header start numbers are 0.
    fn f14_header_repair() -> Vec<crate::DecodeWarning> {
        vec![crate::DecodeWarning::BeginNumRepaired {
            axes: vec!["page", "pic", "tbl", "equation"],
        }]
    }

    fn near(a: (i32, i32), b: (i32, i32)) -> bool {
        (a.0 - b.0).abs() <= 2 && (a.1 - b.1).abs() <= 2
    }

    /// The colour the F14 author picked (`faceColor="#C75252"`).
    fn f14_colour() -> Option<hwpforge_foundation::Color> {
        Some(hwpforge_foundation::Color::from_rgb(0xC7, 0x52, 0x52))
    }

    /// F14 as grouped: the TextArt's own pair scales the 14173 design box to
    /// 40 × 20 mm (14173 × 0.799972 = 11338, × 0.399986 = 5669) and it sits
    /// at the group's corner; its colour is carried; nothing is lost.
    ///
    /// 이것을 실패시키는 것: 묶음 글맵시 디코드 갈래를 지우는 것, 크기를 `orgSz`
    /// 대신 `sz`(없음)에서 읽는 것, `fill_color` 를 `None` 으로 두는 것.
    #[test]
    fn f14_grouped_text_art_matches_hancom() {
        let (w, size, at, colour) = f14_text_art("group-f14-textart.hwpx");
        // F14's base came from an older HwpForge with header start numbers
        // of 0 (Hancom keeps them): that repair, on all four axes, is the
        // only warning.
        assert_eq!(w, f14_header_repair());
        assert!(near(size, (11_338, 5669)), "{size:?}");
        assert!(near(at, (8559, 11_338)), "{at:?}");
        assert_eq!(colour, f14_colour());
    }

    /// F14 after Hancom scaled the group ×1.5024 / ×0.499824: the TextArt's
    /// size is `orgSz` × both pairs, and it lands where Hancom's own ungroup
    /// of the same file puts it (`<hp:sz>` 17034 × 2834 at (8559, 11338)).
    ///
    /// 이것을 실패시키는 것: 쌍 하나만 적용하는 것(크기가 자기 쌍만의 11338 ×
    /// 5669), `curSz` 를 `orgSz` 보다 먼저 읽는 것.
    #[test]
    fn f14_scaled_text_art_matches_hancom_ungroup() {
        let (w, size, at, colour) = f14_text_art("group-f14-textart-scaled.hwpx");
        assert_eq!(w, f14_header_repair());
        let ungrouped = section0_xml(&fixture("group-f14-textart-scaled-ungrouped.hwpx"));
        let art = &ungrouped[ungrouped.find("<hp:textart ").unwrap()..];
        let attr = |tag: &str, name: &str| -> i32 {
            let el = &art[art.find(&format!("<hp:{tag} ")).unwrap()..];
            let v = &el[el.find(&format!(" {name}=\"")).unwrap() + name.len() + 3..];
            v[..v.find('"').unwrap()].parse().unwrap()
        };
        let hancom_size = (attr("sz", "width"), attr("sz", "height"));
        let hancom_at = (attr("pos", "horzOffset"), attr("pos", "vertOffset"));
        assert_eq!((hancom_size, hancom_at), ((17_034, 2834), (8559, 11_338)));
        assert!(near(size, hancom_size), "{size:?} vs {hancom_size:?}");
        assert!(near(at, hancom_at), "{at:?} vs {hancom_at:?}");
        assert_eq!(colour, f14_colour());
    }

    /// F14 written again: the TextArt goes out the way Hancom writes a group
    /// child — no `<hp:sz>`/`<hp:pos>`, the 14173 design box scaled by the
    /// matrix, its colour — and decodes to the same size, place and colour.
    ///
    /// 이것을 실패시키는 것: 묶음 글맵시가 `sz`/`pos` 를 쓰는 것, 채우기 색을
    /// 버리는 것(기본 파랑).
    #[test]
    fn f14_text_art_survives_our_encoder() {
        let doc = HwpxDecoder::decode(&fixture("group-f14-textart-scaled.hwpx")).unwrap();
        let bytes = encode(&doc);
        let xml = section0_xml(&bytes);
        let at = xml.find("<hp:textart ").expect("TextArt written");
        let art = &xml[at..at + xml[at..].find("</hp:textart>").unwrap()];
        assert!(art.contains(r#"groupLevel="1""#), "{art}");
        assert!(art.contains(r#"<hp:orgSz width="14173" height="14173"/>"#), "{art}");
        assert!(art.contains(r##"faceColor="#C75252""##), "{art}");
        assert!(!art.contains("<hp:sz ") && !art.contains("<hp:pos "), "{art}");
        let before = f14_text_art("group-f14-textart-scaled.hwpx");
        let again = HwpxDecoder::decode(&bytes).unwrap();
        assert!(again.warnings.is_empty(), "{:?}", again.warnings);
        let group = first_group(&again);
        let Control::TextArt { width, height, fill_color, .. } =
            children(&group).iter().find(|c| matches!(c, Control::TextArt { .. })).unwrap()
        else {
            unreachable!()
        };
        assert!(near((width.as_i32(), height.as_i32()), before.1));
        assert_eq!(*fill_color, f14_colour());
    }

    /// Core TextArt has no rotation: a group turned 30° reports it.
    ///
    /// 이것을 실패시키는 것: 접기의 회전·반사 저장 불가 판정을 지우는 것.
    #[test]
    fn turned_grouped_text_art_is_reported() {
        let (g, w) = decode(&grouped_text_art(rot(30.0), &solid_brush("#FF0000")));
        assert!(g.is_some());
        assert_eq!(lost_kinds(&w), ["group_child_transform"], "{w:?}");
    }

    /// Only a plain opaque colour is carried; a hatch, transparency, a
    /// gradation, a picture beside the colour, no fill or a face colour that
    /// is not one is reported (the TextArt itself stays).
    ///
    /// 이것을 실패시키는 것: 허용 조건을 `winBrush` 가 있는지만으로 줄이는 것
    /// (무늬·투명도·그러데이션·그림·채우기 없음·색 아님 검사를 지우는 것, 각각).
    #[test]
    fn text_art_fill_other_than_a_plain_colour_is_reported() {
        for (brush, what) in [
            (
                r##"<hc:fillBrush><hc:winBrush faceColor="#FF0000" hatchColor="#000000" hatchStyle="CROSS" alpha="0"/></hc:fillBrush>"##,
                "pattern",
            ),
            (
                r##"<hc:fillBrush><hc:winBrush faceColor="#FF0000" hatchColor="#000000" alpha="128"/></hc:fillBrush>"##,
                "transparency",
            ),
            (
                r##"<hc:fillBrush><hc:gradation type="LINEAR" angle="90" centerX="0" centerY="0" step="255" colorNum="2" stepCenter="50" alpha="0"><hc:color value="#FF0000"/><hc:color value="#0000FF"/></hc:gradation></hc:fillBrush>"##,
                "gradation",
            ),
            (
                r##"<hc:fillBrush><hc:winBrush faceColor="#FF0000" hatchColor="#000000" alpha="0"/><hc:imgBrush mode="TOTAL"><hc:img binaryItemIDRef="image1" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/></hc:imgBrush></hc:fillBrush>"##,
                "picture",
            ),
            ("<hc:fillBrush/>", "empty fill brush"),
            ("", "missing fill brush"),
            (
                r##"<hc:fillBrush><hc:winBrush faceColor="none" hatchColor="#000000" alpha="0"/></hc:fillBrush>"##,
                "face colour",
            ),
        ] {
            let (g, w) = decode(&grouped_text_art(ID, brush));
            assert_eq!(lost_kinds(&w), ["textart_fill"], "{brush}");
            let crate::DecodeWarning::ContentLost { reason, .. } = &w[0] else { unreachable!() };
            assert!(reason.contains(what), "{what}: {reason}");
            assert_eq!(text_art_geometry(&g.unwrap()).2, None, "{brush}");
        }
    }

    /// A TextArt outline or shadow (shape or text) has no Core field and the
    /// encoder writes none, so each is reported, in a group or not; the
    /// defaults Hancom writes (`style="NONE"`, `type="NONE"`) are not.
    ///
    /// 이것을 실패시키는 것: 외곽선 검사·도형 그림자 검사·글자 그림자 검사를
    /// 지우는 것(각각), `NONE` 판정을 뒤집는 것(기본값에도 경고 — F14 테스트가
    /// 잡는다).
    #[test]
    fn text_art_outline_and_shadow_are_reported() {
        let base = grouped_text_art(ID, &solid_brush("#FF0000"));
        let outline = base.replace(
            r##"<hp:lineShape color="#000000" width="0" style="NONE""##,
            r##"<hp:lineShape color="#000000" width="33" style="SOLID""##,
        );
        let shadow = base.replace(
            r##"<hc:pt0 "##,
            r##"<hp:shadow type="PARELLEL_RIGHTBOTTOM" color="#B2B2B2" offsetX="283" offsetY="283" alpha="0"/><hc:pt0 "##,
        );
        let text_shadow = base.replace(
            r#"align="LEFT"/>"#,
            r##"align="LEFT"><hp:shadow type="PARELLEL_RIGHTBOTTOM" color="#000000" offsetX="10" offsetY="10" alpha="0"/></hp:textartPr>"##,
        );
        for (xml, kind, what) in [
            (&outline, "textart_outline", "outline SOLID"),
            (&shadow, "textart_shadow", "shadow PARELLEL"),
            (&text_shadow, "textart_shadow", "text shadow PARELLEL"),
        ] {
            assert_ne!(xml, &base, "{what}: fixture not changed");
            let (g, w) = decode(xml);
            assert!(g.is_some());
            assert_eq!(lost_kinds(&w), [kind], "{what}");
            let crate::DecodeWarning::ContentLost { reason, .. } = &w[0] else { unreachable!() };
            assert!(reason.starts_with(what), "{what}: {reason}");
        }
        // The same outline on a TextArt outside groups.
        let sz = r#"<hp:sz width="7000" widthRelTo="ABSOLUTE" height="3000" heightRelTo="ABSOLUTE" protect="0"/>"#;
        let body = text_art(0, &ri(ID, &[(ID, ID)]), &solid_brush("#00FF00"), sz)
            .replace(r#"width="0" style="NONE""#, r#"width="33" style="SOLID""#);
        let r = crate::decoder::section::parse_section(
            &section(&body),
            0,
            &std::collections::HashMap::new(),
        )
        .unwrap();
        assert_eq!(lost_kinds(&r.warnings), ["textart_outline"]);
    }

    /// A font type other than TTF (`HTF`) has no Core field and the writer
    /// always writes TTF, so it is reported.
    ///
    /// 이것을 실패시키는 것: 글꼴 유형 검사를 지우는 것.
    #[test]
    fn text_art_hangul_font_type_is_reported() {
        let base = grouped_text_art(ID, &solid_brush("#FF0000"));
        let htf = base.replace(r#"fontType="TTF""#, r#"fontType="HTF""#);
        assert_ne!(htf, base);
        let (g, w) = decode(&htf);
        assert!(g.is_some());
        assert_eq!(lost_kinds(&w), ["textart_font_type"]);
        let (_, w) = decode(&base);
        assert!(w.is_empty(), "{w:?}");
    }

    /// Typography attributes left out of `<hp:textartPr>` — or the whole
    /// element — take the OWPML defaults (fontStyle REGULAR, line spacing
    /// 120, character spacing 100, font type TTF), not empty or zero, and
    /// the writer puts those values back.
    ///
    /// 이것을 실패시키는 것: 생략 기본값을 빈 문자열·0 으로 되돌리는 것(속성별,
    /// 그리고 요소가 없을 때의 `Default`), fontType 기본값을 비우는 것(TTF 가
    /// 아니라 손실 경고).
    #[test]
    fn text_art_omitted_typography_takes_the_schema_defaults() {
        let full = r#"<hp:textartPr fontName="함초롬바탕" fontStyle="보통" fontType="TTF" textShape="WAVE2" lineSpacing="120" charSpacing="100" align="LEFT"/>"#;
        let base = grouped_text_art(ID, &solid_brush("#FF0000"));
        assert!(base.contains(full));
        for (case, pr) in [
            ("attributes", r#"<hp:textartPr fontName="함초롬바탕" textShape="WAVE2"/>"#),
            ("element", ""),
        ] {
            let (g, w) = decode(&base.replace(full, pr));
            assert!(w.is_empty(), "{case}: {w:?}");
            let g = g.unwrap();
            let Control::TextArt { font_style, line_spacing, char_spacing, align, .. } =
                &children(&g)[0]
            else {
                panic!("{case}: {g:?}")
            };
            assert_eq!(
                (font_style.as_str(), *line_spacing, *char_spacing, align.as_str()),
                ("REGULAR", 120, 100, "LEFT"),
                "{case}"
            );
            let xml =
                crate::encoder::shapes::encode_text_art_to_xml(children(&g)[0], true).unwrap();
            assert!(
                xml.contains(r#"fontStyle="REGULAR" fontType="TTF""#)
                    && xml.contains(r#"lineSpacing="120" charSpacing="100""#),
                "{case}: {xml}"
            );
        }
    }

    /// A TextArt outside groups keeps its colour too (it used to come back
    /// blue, the encoder's default).
    ///
    /// 이것을 실패시키는 것: `fill_color` 를 `None` 으로 두는 것.
    #[test]
    fn body_text_art_keeps_its_colour() {
        let sz = r#"<hp:sz width="7000" widthRelTo="ABSOLUTE" height="3000" heightRelTo="ABSOLUTE" protect="0"/>"#;
        let ri = ri(ID, &[(ID, ID)]);
        let r = crate::decoder::section::parse_section(
            &section(&text_art(0, &ri, &solid_brush("#00FF00"), sz)),
            0,
            &std::collections::HashMap::new(),
        )
        .unwrap();
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        let art =
            r.paragraphs[0].runs.iter().find_map(|run| run.content.as_control().cloned()).unwrap();
        let Control::TextArt { fill_color, .. } = &art else { panic!("{art:?}") };
        assert_eq!(*fill_color, Some(hwpforge_foundation::Color::from_rgb(0, 255, 0)));
        let xml = crate::encoder::shapes::encode_text_art_to_xml(&art, false).unwrap();
        assert!(xml.contains(r##"faceColor="#00FF00""##), "{xml}");
    }

    const SWITCH: &str = r#"<hp:switch><hp:case hp:required-namespace="http://www.hancom.co.kr/hwpml/2016/ooxmlchart"/></hp:switch>"#;

    fn sublist(run_inner: &str) -> String {
        format!(
            r#"<hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0">{run_inner}</hp:run></hp:p></hp:subList>"#
        )
    }

    fn text_box(run_inner: &str, level: u32) -> String {
        format!(
            r#"{}<hp:drawText lastWidth="0" name="" editable="0">{}</hp:drawText></hp:rect>"#,
            shape_head("rect", (1000, 400), &ri(ID, &vec![(ID, ID); level as usize + 1]))
                .replace(r#"groupLevel="1""#, &format!(r#"groupLevel="{level}""#)),
            sublist(run_inner)
        )
    }

    /// A chart switch (or an element we do not know) inside a group child's
    /// text box, caption, or a nested group's text box is lost; nothing
    /// recovers it later inside a group. Outside groups it stays as before.
    ///
    /// 이것을 실패시키는 것: `convert_run` 의 `Switch`·`Other` 갈래에서
    /// `note_unsupported_in_group_child` 호출을 지우는 것, 또는 그 안의 깊이
    /// 조건을 `true` 로 바꾸는 것 (묶음 밖 글상자에도 경고).
    #[test]
    fn unsupported_run_inside_group_content_is_reported() {
        let in_box = text_box(SWITCH, 1);
        let (_, w) = decode(&container((1000, 400), &in_box));
        assert_eq!(lost_kinds(&w), ["group_nested_switch"]);

        let caption = format!(
            r#"{}<hp:caption side="BOTTOM" fullSz="0" width="1000" gap="850" lastWidth="1000">{}</hp:caption></hp:rect>"#,
            shape_head("rect", (1000, 400), &ri(ID, &[(ID, ID), (ID, ID)])),
            sublist("<hp:futureThing/>")
        );
        let (_, w) = decode(&container((1000, 400), &caption));
        assert_eq!(lost_kinds(&w), ["group_nested_unknown"]);

        let nested_box = text_box(SWITCH, 2);
        let (_, w) = decode(&container(
            (1000, 400),
            &nested((1000, 400), &ri(ID, &[(ID, ID), (ID, ID)]), &nested_box),
        ));
        assert_eq!(lost_kinds(&w), ["group_nested_switch"]);

        // A text box that is not in a group: no warning.
        let free = text_box(SWITCH, 0);
        let (_, w) = decode(&free);
        assert!(lost_kinds(&w).is_empty(), "{w:?}");
    }

    /// A polygon filled with a picture: Core has no picture fill for shapes,
    /// so the fill is reported, and the polygon keeps its own rotation
    /// (a picture turns with its shape) instead of folding it into points.
    ///
    /// 이것을 실패시키는 것: 그림 채우기 방출을 지우는 것, 또는 점 기반 분기
    /// 조건에서 `!picture_fill` 을 빼는 것 (회전이 점에 접힌다).
    #[test]
    fn picture_filled_child_is_reported_and_keeps_its_rotation() {
        let fill = r#"<hc:fillBrush><hc:imgBrush mode="TOTAL"><hc:img binaryItemIDRef="image1" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/></hc:imgBrush></hc:fillBrush>"#;
        let child = polygon(
            (1000, 600),
            &ri(ID, &[(ID, rot(30.0)), (ID, ID)]),
            &[(0, 600), (500, 0), (1000, 600)],
            fill,
        );
        let (g, w) = decode(&container((3000, 3000), &child));
        assert_eq!(lost_kinds(&w), ["group_child_fill"]);
        let g = g.unwrap();
        assert_eq!(rotation(children(&g)[0]), (Some(30.0), None));
        // 묶음 자식은 그림 채우기를 아직 나르지 않는다 — 옛 출력(인코더 기본
        // 채우기)대로. 이것을 실패시키는 것: 묶음 경로의 `Fill::Image` 제거를
        // 지우기(손실 경고와 함께 그림 채우기가 방출됨).
        let Control::Polygon { style, .. } = children(&g)[0] else { panic!("polygon") };
        assert!(
            !matches!(
                style.as_ref().and_then(|s| s.fill.as_ref()),
                Some(hwpforge_core::control::Fill::Image { .. })
            ),
            "{style:?}"
        );
    }

    /// Sizes and offsets outside the HWPUNIT range cannot be stored (they
    /// would become 0): the child is dropped and reported, on both the box
    /// and the point path, and so is a group size outside it.
    ///
    /// 이것을 실패시키는 것: 상자 경로·점 경로의 범위 검사(`fits_hwpunit`) 중
    /// 하나를 지우는 것 (각각 따로 — 사유 문구가 잔차로 바뀌거나 경고가 없다),
    /// 또는 최상위 크기 범위 검사를 지우는 것.
    #[test]
    fn out_of_range_geometry_is_reported() {
        let far = [1.0, 0.0, 100_000_001.0, 0.0, 1.0, 0.0];
        let keep = rect((500, 500), &ri(ID, &[(ID, ID), (ID, ID)]));
        let reasons = |w: &[crate::DecodeWarning]| -> Vec<String> {
            w.iter()
                .filter_map(|w| match w {
                    crate::DecodeWarning::ContentLost { reason, .. } => Some(reason.clone()),
                    _ => None,
                })
                .collect()
        };
        let boxy = rect((1000, 400), &ri(far, &[(ID, ID), (ID, ID)]));
        let own = ri(ID, &[(ID, ID)]);
        // `sz` given, so the far child's extent shows only as its own loss and
        // as a displayed size that does not hold it.
        let (g, w) =
            decode(&container_with((1000, 500), (1000, 500), &own, &format!("{keep}{boxy}")));
        assert_eq!(children(&g.unwrap()).len(), 1);
        let child_and_box = |w: &[crate::DecodeWarning]| {
            let r = reasons(w);
            assert_eq!(r.len(), 2, "{r:?}");
            assert!(r[0].starts_with("displayed size 1000x500 differs"), "{r:?}");
            assert_eq!(r[1], "size or offset outside the HWPUNIT range");
        };
        child_and_box(&w);

        let pointy = polygon(
            (1000, 600),
            &ri(far, &[(ID, ID), (ID, ID)]),
            &[(0, 600), (500, 0), (1000, 600)],
            "",
        );
        let (_, w) =
            decode(&container_with((1000, 500), (1000, 500), &own, &format!("{keep}{pointy}")));
        child_and_box(&w);

        let huge = container_with((1000, 500), (200_000_000, 500), &ri(ID, &[(ID, ID)]), &keep);
        let (_, w) = decode(&huge);
        assert_eq!(reasons(&w), ["displayed size outside the HWPUNIT range"]);
    }

    /// A top-level group whose own matrix does not parse: its children are
    /// placed against a guessed origin, so that is reported.
    ///
    /// 이것을 실패시키는 것: 최상위 묶음 행렬 해석 실패를 조용히 항등으로 두는 것.
    #[test]
    fn unparseable_group_matrix_is_reported() {
        let own = ri(ID, &[(ID, ID)]).replacen(r#"e1="1""#, r#"e1="NaN""#, 1);
        let keep = rect((500, 500), &ri(ID, &[(ID, ID), (ID, ID)]));
        let (g, w) = decode(&container_with((500, 500), (500, 500), &own, &keep));
        assert!(g.is_some());
        assert_eq!(lost_kinds(&w), ["group_transform"]);
    }

    /// A group of pictures only: Core has no group child for them yet, so
    /// every picture and the group itself are reported.
    ///
    /// 이것을 실패시키는 것: 자식이 하나도 남지 않을 때 `group` 방출을 지우는 것.
    #[test]
    fn picture_only_group_reports_each_picture_and_the_group() {
        let pic = r#"<hp:pic id="2" groupLevel="1"><hp:offset x="0" y="0"/></hp:pic>"#;
        let (g, w) = decode(&container((1000, 1000), &format!("{pic}{pic}")));
        assert!(g.is_none());
        assert_eq!(lost_kinds(&w), ["pic", "pic", "group"]);
    }

    /// The container's `sz` must describe its shapes: their extent (Hancom)
    /// or a box from the origin that holds them (this crate's writer). A
    /// size that cuts a shape off is reported.
    ///
    /// 이것을 실패시키는 것: 표시 크기 비교를 지우는 것(800 이 통과), 또는
    /// 원점에서 시작해 도형을 담는 상자를 받아들이지 않는 것(1500 이 손실).
    #[test]
    fn display_size_that_cuts_off_a_shape_is_content_lost() {
        let leaf = rect((1000, 1000), &ri(ID, &[(ID, ID), (ID, ID)]));
        let own = ri(ID, &[(ID, ID)]);
        let (g, w) = decode(&container_with((1000, 1000), (800, 1000), &own, &leaf));
        let (width, height, _) = size_and_offset(&g.unwrap());
        assert_eq!((width, height), (800, 1000), "displayed size is `sz`");
        assert_eq!(lost_kinds(&w), ["group_display_size"]);

        let (_, w) = decode(&container_with((1000, 1000), (1500, 1000), &own, &leaf));
        assert!(w.is_empty(), "{w:?}");
    }

    // ── fixtures and round trips ───────────────────────────────────

    fn first_group(doc: &crate::HwpxDocument) -> Control {
        doc.document.sections()[0]
            .paragraphs
            .iter()
            .flat_map(|p| p.runs.iter())
            .find_map(|r| match &r.content {
                RunContent::Control(c) if matches!(**c, Control::Group { .. }) => {
                    Some((**c).clone())
                }
                _ => None,
            })
            .expect("a group")
    }

    fn replace_first_group(doc: &mut crate::HwpxDocument, group: Control) {
        let mut slot = Some(group);
        for p in &mut doc.document.sections_mut()[0].paragraphs {
            for r in &mut p.runs {
                if let RunContent::Control(c) = &mut r.content {
                    if matches!(**c, Control::Group { .. }) {
                        if let Some(g) = slot.take() {
                            **c = g;
                        }
                    }
                }
            }
        }
        assert!(slot.is_none(), "no group to replace");
    }

    fn encode(doc: &crate::HwpxDocument) -> Vec<u8> {
        let validated = doc.document.clone().validate().expect("validate");
        crate::HwpxEncoder::encode(&validated, &doc.style_store, &doc.image_store).expect("encode")
    }

    /// The two native groups made in Hancom (flat and nested) decode, and
    /// the group keeps one Core child per wire child.
    ///
    /// 이것을 실패시키는 것: `HxRenderingInfo` 를 한 쌍만 받는 derive 로 되돌리는 것
    /// (`duplicate field scaMatrix` 로 문서 전체 디코드 실패).
    #[test]
    fn native_groups_decode_with_every_child() {
        for (name, top_children) in [("group-native.hwpx", 2), ("group-native-nested.hwpx", 2)] {
            let doc = HwpxDecoder::decode(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(doc.warnings.is_empty(), "{name}: {:?}", doc.warnings);
            assert_eq!(children(&first_group(&doc)).len(), top_children, "{name}");
            doc.document.clone().validate().unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        let doc = HwpxDecoder::decode(&fixture("group-native-nested.hwpx")).unwrap();
        let g = first_group(&doc);
        assert!(matches!(&children(&g)[0], Control::Group { children, .. } if children.len() == 2));
    }

    /// F1 g1 (group scaled to 150 % × 50 %): the Core group is the displayed
    /// box `sz` (34015 × 7086, not `orgSz` 22677 × 14173), anchored the way
    /// the wire says, in front of text; re-encoding writes the same box.
    ///
    /// 이것을 실패시키는 것: 크기를 `orgSz` 에서 읽는 것, 또는 container 의
    /// `textWrap` 을 읽지 않는 것 (각각 따로).
    #[test]
    fn f1_g1_top_level_group_is_the_displayed_box() {
        use hwpforge_core::placement::ObjectTextWrap;
        let doc = HwpxDecoder::decode(&fixture("group-f1-g1-scaled.hwpx")).unwrap();
        let g = first_group(&doc);
        let Control::Group { width, height, placement, .. } = &g else { unreachable!() };
        assert_eq!((width.as_i32(), height.as_i32()), (34015, 7086));
        let p = placement.as_ref().unwrap();
        assert_eq!(p.text_wrap, ObjectTextWrap::InFrontOfText);
        assert_eq!((p.horz_offset.as_i32(), p.vert_offset.as_i32()), (8504, 11339));

        let xml = section0_xml(&encode(&doc));
        let open = &xml[xml.find("<hp:container").unwrap()..];
        let open = &open[..open.find('>').unwrap()];
        assert!(open.contains(r#"textWrap="IN_FRONT_OF_TEXT""#), "{open}");
        assert!(xml.contains(r#"<hp:sz width="34015" widthRelTo="ABSOLUTE" height="7086""#));
        assert!(xml.contains(r#"vertOffset="11339" horzOffset="8504""#));
    }

    /// The container's `textFlow` survives decode → encode → decode.
    ///
    /// 이것을 실패시키는 것: 인코더의 `textFlow="BOTH_SIDES"` 하드코딩 복귀, 또는
    /// 디코더가 container 의 `textFlow` 를 읽지 않는 것.
    #[test]
    fn group_text_flow_round_trips() {
        use hwpforge_core::placement::ObjectTextFlow;
        let mut doc = HwpxDecoder::decode(&fixture("group-f1-g1-scaled.hwpx")).unwrap();
        let mut g = first_group(&doc);
        if let Control::Group { placement: Some(p), .. } = &mut g {
            p.text_flow = ObjectTextFlow::LargestOnly;
        }
        replace_first_group(&mut doc, g);
        let bytes = encode(&doc);
        assert!(section0_xml(&bytes).contains(r#"textFlow="LARGEST_ONLY""#));
        let back = first_group(&HwpxDecoder::decode(&bytes).unwrap());
        let Control::Group { placement: Some(p), .. } = &back else { panic!("{back:?}") };
        assert_eq!(p.text_flow, ObjectTextFlow::LargestOnly);
    }

    /// F1 g2 (group turned 30°): the Core group is an unrotated
    /// 33001 × 23144 box; the rect and ellipse carry the 30°, the polygon and
    /// line hold it in their points. Expected points come from applying the
    /// wire matrices outside the decoder (see the F1 per-child test for the
    /// page positions).
    ///
    /// 이것을 실패시키는 것: 묶음 회전을 자식으로 접지 않고 남기는 것, 또는 점
    /// 기반 자식에 회전을 남기고 점을 접지 않는 것.
    #[test]
    fn f1_g2_rotation_moves_onto_the_children() {
        let doc = HwpxDecoder::decode(&fixture("group-f1-g2-rotated.hwpx")).unwrap();
        assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
        let g = first_group(&doc);
        let (w, h, _) = size_and_offset(&g);
        assert_eq!((w, h), (33001, 23144));
        for c in children(&g) {
            match c {
                Control::Rect { .. } | Control::Ellipse { .. } => {
                    // The exact angle of Hancom's six-digit matrices, not a
                    // whole degree (30.000011).
                    let (angle, flip) = rotation(c);
                    assert!((angle.unwrap() - 30.0).abs() < 1e-3 && flip.is_none(), "{c:?}");
                }
                Control::Polygon { vertices, placement, .. } => {
                    assert_eq!(rotation(c), (None, None));
                    let v: Vec<(i32, i32)> = vertices.iter().map(|p| (p.x, p.y)).collect();
                    assert_eq!(v, [(0, 0), (6941, 734), (11047, 6378)]);
                    assert_eq!(offset_of(placement), (0.0, 6137.0));
                }
                Control::Line { start, end, placement, .. } => {
                    assert_eq!(rotation(c), (None, None));
                    assert_eq!((start.x, start.y, end.x, end.y), (0, 0, 9630, 8832));
                    assert_eq!(offset_of(placement), (19828.0, 14312.0));
                }
                other => panic!("unexpected child {other:?}"),
            }
        }
    }

    /// A nested group written by us: every shape's `transMatrix` is in the
    /// outermost group's space (parent offsets added up), and decoding it
    /// back gives the same places with no warning.
    ///
    /// 이것을 실패시키는 것: 인코더가 누적 원점 인수를 무시하는 것 (안쪽 사각형이
    /// 원점에 놓여 `nested_group_placement`, transMatrix 단언 실패).
    #[test]
    fn nested_group_round_trips_in_outer_coordinates() {
        use hwpforge_foundation::HwpUnit;
        let at = |x, y| super::group_child_placement(x, y);
        let rect = |w, h, placement| Control::Rect {
            width: HwpUnit::new(w).unwrap(),
            height: HwpUnit::new(h).unwrap(),
            placement,
            caption: None,
            style: None,
        };
        let mut doc = HwpxDecoder::decode(&fixture("group-f1-g1-scaled.hwpx")).unwrap();
        let Control::Group { placement, inst_id, .. } = first_group(&doc) else { unreachable!() };
        let group = Control::Group {
            children: vec![
                hwpforge_core::control::GroupChild::Shape(rect(1000, 500, at(100, 50))),
                hwpforge_core::control::GroupChild::Shape(Control::Group {
                    children: vec![hwpforge_core::control::GroupChild::Shape(rect(800, 600, None))],
                    width: HwpUnit::new(800).unwrap(),
                    height: HwpUnit::new(600).unwrap(),
                    placement: at(2000, 3000),
                    inst_id: None,
                }),
            ],
            width: HwpUnit::new(2800).unwrap(),
            height: HwpUnit::new(3600).unwrap(),
            placement,
            inst_id,
        };
        replace_first_group(&mut doc, group);
        let bytes = encode(&doc);
        let xml = section0_xml(&bytes);
        let inner = &xml[xml.find(r#"groupLevel="1""#).unwrap()..];
        let inner_rect = &inner[inner.find(r#"<hp:rect"#).unwrap()..];
        assert!(inner_rect
            .contains(r#"<hc:transMatrix e1="1" e2="0" e3="2000" e4="0" e5="1" e6="3000"/>"#));

        let back = HwpxDecoder::decode(&bytes).unwrap();
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        let g = first_group(&back);
        assert_eq!(size_and_offset(children(&g)[0]), (1000, 500, (100.0, 50.0)));
        let n = &children(&g)[1];
        assert_eq!(size_and_offset(n), (800, 600, (2000.0, 3000.0)));
        assert_eq!(size_and_offset(children(n)[0]), (800, 600, (0.0, 0.0)));
    }

    /// `group-converted-from-hwp5.hwpx` with its group replaced by `container`.
    fn package_with_group(container: &str) -> Vec<u8> {
        use std::io::{Read, Write};
        let base = fixture("group-converted-from-hwp5.hwpx");
        let mut zin = zip::ZipArchive::new(std::io::Cursor::new(&base)).unwrap();
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for i in 0..zin.len() {
            let mut f = zin.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            if name == "Contents/section0.xml" {
                let xml = String::from_utf8(data).unwrap();
                let a = xml.find("<hp:container").unwrap();
                let b = xml.rfind("</hp:container>").unwrap() + "</hp:container>".len();
                data = format!("{}{container}{}", &xml[..a], &xml[b..]).into_bytes();
            }
            let opts = zip::write::SimpleFileOptions::default().compression_method(f.compression());
            out.start_file(name, opts).unwrap();
            out.write_all(&data).unwrap();
        }
        out.finish().unwrap().into_inner()
    }

    /// A nested group whose polygon's points sit inside the polygon's own
    /// box: the Core group box is the points' extent, so decoding our own
    /// encode of it gives the same group again.
    ///
    /// 이것을 실패시키는 것: 중첩 묶음 상자를 디코드한 자식 대신 wire 상자 경계로
    /// 정하는 것 (재디코드 때마다 상자가 8씩 줄어든다).
    #[test]
    fn nested_group_box_is_stable_across_round_trips() {
        let inset = polygon(
            (1000, 600),
            &ri(ID, &[(ID, ID), (ID, ID), (ID, ID)]),
            &[(8, 8), (500, 300), (992, 592)],
            "",
        )
        .replace(r#"groupLevel="1""#, r#"groupLevel="2""#);
        let group =
            container((1000, 600), &nested((1000, 600), &ri(ID, &[(ID, ID), (ID, ID)]), &inset));
        let first = HwpxDecoder::decode(&package_with_group(&group)).unwrap();
        let g1 = first_group(&first);
        assert_eq!(size_and_offset(children(&g1)[0]), (984, 584, (8.0, 8.0)));
        let second = HwpxDecoder::decode(&encode(&first)).unwrap();
        assert!(second.warnings.is_empty(), "{:?}", second.warnings);
        let g2 = first_group(&second);
        let geometry = |g: &Control| {
            let n = &children(g)[0];
            let Control::Polygon { vertices, .. } = &children(n)[0] else { panic!() };
            (size_and_offset(n), size_and_offset(children(n)[0]), vertices.clone())
        };
        assert_eq!(geometry(&g2), geometry(&g1));
    }

    /// A group converted from HWP5 by us decodes to a document that
    /// validates (shapes no longer come back 0 × 0), and its children keep
    /// their size and place through another encode and decode.
    ///
    /// 이것을 실패시키는 것: 자식 기하 정규화를 빼는 것 (`zero dimension`).
    #[test]
    fn converted_group_validates_and_round_trips() {
        let doc = HwpxDecoder::decode(&fixture("group-converted-from-hwp5.hwpx")).unwrap();
        assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
        let first = first_group(&doc);
        let back = first_group(&HwpxDecoder::decode(&encode(&doc)).unwrap());
        let geom = |g: &Control| {
            children(g)
                .iter()
                .map(|c| match c {
                    Control::TextBox { width, height, placement, .. } => {
                        (width.as_i32(), height.as_i32(), offset_of(placement))
                    }
                    other => size_and_offset(other),
                })
                .collect::<Vec<_>>()
        };
        assert!(geom(&first).iter().all(|(w, h, _)| *w > 0 && *h > 0), "{:?}", geom(&first));
        assert_eq!(geom(&first), geom(&back));
    }

    /// Supported children (a picture among them) keep the wire order; every
    /// element Core cannot carry is reported by its own name, in order.
    ///
    /// 이것을 실패시키는 것: 미지원 갈래의 방출을 지우는 것, 또는 사이드카 대신
    /// 종류별 고정 순서로 디코드하는 것, 또는 사이드카에서 pic 갈래를 빼는 것
    /// (그림이 다시 `pic` 손실로 보고된다).
    #[test]
    fn unsupported_children_are_reported_in_wire_order() {
        let doc = HwpxDecoder::decode(&fixture("group-unsupported-children.hwpx")).unwrap();
        assert_eq!(lost_kinds(&doc.warnings), ["ole", "equation", "unknown"]);
        let paths: Vec<String> = doc.warnings.iter().map(|w| w.location().unwrap()).collect();
        assert!(paths[0].ends_with(".group[3]"), "{paths:?}");
        assert!(paths[2].ends_with(".group[5]"), "{paths:?}");
        let g = first_group(&doc);
        let Control::Group { children, .. } = &g else { unreachable!() };
        let kinds: Vec<&str> = children
            .iter()
            .map(|c| match c {
                GroupChild::Picture(_) => "picture",
                GroupChild::Shape(Control::Rect { .. }) => "rect",
                GroupChild::Shape(Control::Polygon { .. }) => "polygon",
                GroupChild::Shape(Control::Line { .. }) => "line",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["rect", "picture", "polygon", "line"]);
    }

    /// A table inside a grouped text box (the shape of a public notice whose
    /// 43779 × 54767 table sits in a group) decodes without a warning and
    /// keeps its size through an encode and a second decode.
    ///
    /// 이것을 실패시키는 것: 묶음 자식 안 개체 임시 경고를 남기는 것(경고가
    /// 남음), 그 경고만 지우고 인코더는 fragment 전체의 `sz`/`pos` 를 지우는 것
    /// (재인코드한 표 `<hp:sz>` 가 사라짐).
    #[test]
    fn object_inside_a_group_child_round_trips() {
        let doc = HwpxDecoder::decode(&fixture("group-nested-table.hwpx")).unwrap();
        assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
        let bytes = encode(&doc);
        let xml = section0_xml(&bytes);
        // The table height on the wire is a decode-only cache; the table
        // encoder always writes 0 there, in a group or not.
        assert!(xml.contains(r#"<hp:sz width="43779" widthRelTo="ABSOLUTE" height="0""#), "{xml}");
        let again = HwpxDecoder::decode(&bytes).unwrap();
        assert!(again.warnings.is_empty(), "{:?}", again.warnings);
        let table_width = |doc: &crate::HwpxDocument| {
            let group = first_group(doc);
            let Control::TextBox { paragraphs, .. } = &children(&group)[0] else { panic!() };
            paragraphs[0].runs.iter().find_map(|r| r.content.as_table()).and_then(|t| t.width)
        };
        assert_eq!(table_width(&again), Some(hwpforge_foundation::HwpUnit::new(43_779).unwrap()));
        assert_eq!(table_width(&again), table_width(&doc));
    }

    /// Hancom's F2 (a group → text box → 2×2 table and picture): nothing is
    /// lost, and the re-encoded table and picture keep the `<hp:sz>` Hancom
    /// gave them (table width 19842, picture 8504 × 5669).
    ///
    /// 이것을 실패시키는 것: 인코더가 묶음 자식 fragment 전체의 `sz`/`pos` 를
    /// 지우는 것, 묶음 자식 안 개체 임시 경고를 남기는 것.
    #[test]
    fn hancom_grouped_text_box_keeps_its_table_and_picture() {
        let doc = HwpxDecoder::decode(&fixture("group-f2-textbox-table.hwpx")).unwrap();
        assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
        let xml = section0_xml(&encode(&doc));
        let tbl = xml.find("<hp:tbl ").expect("table written");
        assert!(xml[tbl..].contains(r#"><hp:sz width="19842" widthRelTo="ABSOLUTE""#), "{xml}");
        let pic = xml.find("<hp:pic ").expect("picture written");
        assert!(
            xml[pic..].contains(r#"<hp:sz width="8504" widthRelTo="ABSOLUTE" height="5669""#),
            "{xml}"
        );
        let again = HwpxDecoder::decode(&encode(&doc)).unwrap();
        assert!(again.warnings.is_empty(), "{:?}", again.warnings);
    }

    /// A table in an ordinary text box is not a group child: no warning.
    #[test]
    fn object_outside_groups_is_not_reported() {
        let tbl_section = {
            let path = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../tests/fixtures/tables/table_01_basic_2x2.hwpx"
            );
            section0_xml(&std::fs::read(path).unwrap())
        };
        let r = crate::decoder::section::parse_section(
            &tbl_section,
            0,
            &std::collections::HashMap::new(),
        )
        .unwrap();
        assert!(lost_kinds(&r.warnings).is_empty(), "{:?}", r.warnings);
    }

    // ── input boundaries of the regenerating editors ───────────────

    fn assert_refused_by_stamp_and_set_cell(name: &str, kind: &str) {
        use crate::stamp::{HwpxStamper, StampRequestV2, StamperError, STAMP_MAP_VERSION};
        use crate::{CellEditError, HwpxCellEditor};
        let bytes = fixture(name);
        let expected = format!("input decode: CONTENT_LOST {kind}");
        let check = |component: &str, diff_path: &str, surface: &str| {
            assert_eq!(component, expected, "{name} {surface}");
            assert!(diff_path.starts_with("Contents/section0.xml:"), "{surface}: {diff_path}");
        };

        match HwpxStamper::stamp(&bytes, &[]) {
            Err(StamperError::NotRoundTripSafe { component, diff_path }) => {
                check(&component, &diff_path, "stamp v1");
            }
            other => panic!("{name} stamp v1: {other:?}"),
        }
        let request = StampRequestV2 {
            schema_version: STAMP_MAP_VERSION,
            source_sha256: crate::patch::sha256_hex(&bytes),
            text: Vec::new(),
            cells: Vec::new(),
        };
        match HwpxStamper::stamp_v2(&bytes, &request) {
            Err(StamperError::NotRoundTripSafe { component, diff_path }) => {
                check(&component, &diff_path, "stamp v2");
            }
            other => panic!("{name} stamp v2: {other:?}"),
        }
        match HwpxCellEditor::set_cells(&bytes, &[]) {
            Err(CellEditError::NotRoundTripSafe { component, diff_path }) => {
                check(&component, &diff_path, "set-cell");
            }
            other => panic!("{name} set-cell: {other:?}"),
        }
    }

    /// A child the group slants (a shear with no rotation) is kept as the
    /// nearest box and reported. Its no-op round trip is stable, so stamping
    /// (both request versions) and cell editing reach the decode-loss check
    /// and refuse it there.
    ///
    /// 이것을 실패시키는 것: `stamp_with_diagnostics`·`stamp_v2_with_diagnostics`·
    /// `set_cells_with_diagnostics` 의 `reject_input_content_loss` 호출을 하나씩
    /// 지우는 것 (그 표면만 다른 결과가 된다).
    #[test]
    fn editors_refuse_an_input_with_a_slanted_group_child() {
        let doc = HwpxDecoder::decode(&fixture("group-slanted-child.hwpx")).unwrap();
        assert_eq!(lost_kinds(&doc.warnings), ["group_child_transform"]);
        assert_refused_by_stamp_and_set_cell("group-slanted-child.hwpx", "group_child_transform");
    }

    /// A sheared child is kept as the nearest box and reported. Its exact
    /// angle is not a whole degree, so the no-op round trip through HWPX
    /// already differs and the older admission check refuses it first.
    #[test]
    fn editors_refuse_an_input_with_a_sheared_group_child() {
        use crate::stamp::{HwpxStamper, StamperError};
        use crate::{CellEditError, HwpxCellEditor};
        let bytes = fixture("group-sheared-child.hwpx");
        let doc = HwpxDecoder::decode(&bytes).unwrap();
        assert_eq!(lost_kinds(&doc.warnings), ["group_child_transform"]);
        assert!(matches!(
            HwpxStamper::stamp(&bytes, &[]),
            Err(StamperError::NotRoundTripSafe { .. })
        ));
        assert!(matches!(
            HwpxCellEditor::set_cells(&bytes, &[]),
            Err(CellEditError::NotRoundTripSafe { .. })
        ));
    }

    /// A table inside a grouped text box no longer stops the editors: the
    /// decode loses nothing and the no-op round trip keeps the table.
    ///
    /// 이것을 실패시키는 것: 묶음 자식 안 개체 임시 경고를 남기는 것, 인코더가
    /// fragment 전체의 `sz`/`pos` 를 지우는 것 (둘 다 `NotRoundTripSafe` 로 거부).
    #[test]
    fn editors_accept_an_input_with_an_object_inside_a_group_child() {
        use crate::stamp::{HwpxStamper, StampRequestV2, STAMP_MAP_VERSION};
        use crate::HwpxCellEditor;
        let bytes = fixture("group-nested-table.hwpx");
        HwpxStamper::stamp(&bytes, &[]).expect("stamp v1");
        let request = StampRequestV2 {
            schema_version: STAMP_MAP_VERSION,
            source_sha256: crate::patch::sha256_hex(&bytes),
            text: Vec::new(),
            cells: Vec::new(),
        };
        HwpxStamper::stamp_v2(&bytes, &request).expect("stamp v2");
        HwpxCellEditor::set_cells(&bytes, &[]).expect("set-cell");
    }

    /// `bytes` with one more ZIP entry the encoder does not write.
    fn with_uncarried_entry(bytes: &[u8]) -> Vec<u8> {
        use std::io::{Read, Write};
        let mut zin = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for i in 0..zin.len() {
            let mut f = zin.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            let opts = zip::write::SimpleFileOptions::default().compression_method(f.compression());
            out.start_file(name, opts).unwrap();
            out.write_all(&data).unwrap();
        }
        out.start_file("Preview/PrvText.txt", zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all("미리보기".as_bytes()).unwrap();
        out.finish().unwrap().into_inner()
    }

    /// An input both carrying a ZIP entry the encoder drops and losing group
    /// content keeps the older code, `UncarriedZipEntries`
    /// (`INPUT_ENTRIES_NOT_CARRIED`): the content-loss check runs last.
    ///
    /// 이것을 실패시키는 것: `reject_input_content_loss` 를 `check_zip_carry` 앞으로
    /// 옮기는 것 (stamp v1·v2·set-cell 각각 — 그 표면이 `NotRoundTripSafe` 로 바뀐다).
    #[test]
    fn uncarried_entries_keep_their_code_over_lost_content() {
        use crate::stamp::{HwpxStamper, StampRequestV2, StamperError, STAMP_MAP_VERSION};
        use crate::{CellEditError, HwpxCellEditor};
        let bytes = with_uncarried_entry(&fixture("group-slanted-child.hwpx"));
        let doc = HwpxDecoder::decode(&bytes).unwrap();
        assert_eq!(lost_kinds(&doc.warnings), ["group_child_transform"]);

        let uncarried = |entries: &[String]| entries == ["Preview/PrvText.txt"];
        match HwpxStamper::stamp(&bytes, &[]) {
            Err(StamperError::UncarriedZipEntries { entries }) if uncarried(&entries) => {}
            other => panic!("stamp v1: {other:?}"),
        }
        let request = StampRequestV2 {
            schema_version: STAMP_MAP_VERSION,
            source_sha256: crate::patch::sha256_hex(&bytes),
            text: Vec::new(),
            cells: Vec::new(),
        };
        match HwpxStamper::stamp_v2(&bytes, &request) {
            Err(StamperError::UncarriedZipEntries { entries }) if uncarried(&entries) => {}
            other => panic!("stamp v2: {other:?}"),
        }
        match HwpxCellEditor::set_cells(&bytes, &[]) {
            Err(CellEditError::UncarriedZipEntries { entries }) if uncarried(&entries) => {}
            other => panic!("set-cell: {other:?}"),
        }
    }

    // ── pictures in a group (P5) ─────────────────────────────────────

    /// A group picture as Hancom writes one: no `<hp:sz>`/`<hp:pos>`, its
    /// whole image clipped (`imgClip` = `imgDim`).
    fn pic(org: (i32, i32), ri: &str, image: &str, tail: &str) -> String {
        format!(
            r#"<hp:pic id="0" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="1" instid="4" reverse="0"><hp:offset x="0" y="0"/><hp:orgSz width="{w}" height="{h}"/><hp:curSz width="0" height="0"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="0" centerY="0" rotateimage="1"/>{ri}<hc:img binaryItemIDRef="{image}" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/><hp:imgRect><hc:pt0 x="0" y="0"/><hc:pt1 x="{w}" y="0"/><hc:pt2 x="{w}" y="{h}"/><hc:pt3 x="0" y="{h}"/></hp:imgRect><hp:imgClip left="0" right="22500" top="0" bottom="15000"/><hp:inMargin left="0" right="0" top="0" bottom="0"/><hp:imgDim dimwidth="22500" dimheight="15000"/>{tail}</hp:pic>"#,
            w = org.0,
            h = org.1
        )
    }

    /// (path, width, height, offset) of each picture child of `group`, in
    /// child order; shapes are skipped.
    fn pictures(group: &Control) -> Vec<(String, i32, i32, (f64, f64))> {
        let Control::Group { children, .. } = group else { panic!("not a group") };
        children
            .iter()
            .filter_map(|c| match c {
                GroupChild::Picture(i) => Some((
                    i.path.clone(),
                    i.width.as_i32(),
                    i.height.as_i32(),
                    offset_of(&i.placement),
                )),
                _ => None,
            })
            .collect()
    }

    fn lost(warnings: &[crate::DecodeWarning]) -> Vec<(String, String)> {
        warnings
            .iter()
            .filter_map(|w| match w {
                crate::DecodeWarning::ContentLost { kind, reason, .. } => {
                    Some((kind.clone(), reason.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// A group of pictures only (written by hand like Hancom's: no `sz`/`pos`
    /// on the pictures) decodes to one group of three pictures whose size and
    /// place are the chain's, not `orgSz`.
    ///
    /// 이것을 실패시키는 것: 사이드카에서 pic 갈래 삭제(그림이 `pic` 손실, 묶음 소멸) /
    /// 그림 크기를 `orgSz`·`curSz` 에서 읽기(첫 그림이 1000x600) / sca 이동 성분 무시
    /// (셋째 그림 offset 0).
    #[test]
    fn group_of_pictures_decodes_to_core_pictures() {
        let half_310 = [0.5, 0.0, 310.0, 0.0, 0.5, 0.0];
        let children = [
            pic(
                (1000, 600),
                &ri(
                    [1.0, 0.0, 100.0, 0.0, 1.0, 50.0],
                    &[([2.0, 0.0, 0.0, 0.0, 0.5, 0.0], ID), (ID, ID)],
                ),
                "image1",
                "",
            ),
            pic(
                (400, 400),
                &ri([1.0, 0.0, 1000.0, 0.0, 1.0, 0.0], &[(ID, ID), (ID, ID)]),
                "image2",
                "",
            ),
            pic((400, 400), &ri(ID, &[(half_310, ID), (ID, ID)]), "image3", ""),
        ]
        .concat();
        let (g, w) = decode(&container((3000, 3000), &children));
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(
            pictures(&g.unwrap()),
            [
                ("BinData/image1".to_string(), 2000, 300, (100.0, 50.0)),
                ("BinData/image2".to_string(), 400, 400, (1000.0, 0.0)),
                ("BinData/image3".to_string(), 200, 200, (310.0, 0.0)),
            ]
        );
    }

    /// Hancom's F3a (three logos grouped) decodes to its three pictures in
    /// wire order at their own offsets, and loses nothing.
    ///
    /// 이것을 실패시키는 것: 그림 갈래를 사이드카 순서 대신 종류별로 모으는 것 /
    /// 그림 offset 을 원점으로.
    #[test]
    fn hancom_logo_group_decodes_to_three_pictures() {
        let doc = HwpxDecoder::decode(&fixture("group-f3a-logo.hwpx")).unwrap();
        assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
        let g = first_group(&doc);
        let Control::Group { width, height, .. } = &g else { unreachable!() };
        assert_eq!((width.as_i32(), height.as_i32()), (28_346, 5669));
        assert_eq!(
            pictures(&g),
            [
                ("BinData/image1".to_string(), 8504, 5669, (9921.0, 0.0)),
                ("BinData/image2".to_string(), 8504, 5669, (0.0, 0.0)),
                ("BinData/image3".to_string(), 8504, 5669, (19_842.0, 0.0)),
            ]
        );
    }

    /// Hancom's F3c (a picture cropped before grouping), F3d (one turned
    /// 30°) and F3e (one in greyscale): each reports what a Core picture
    /// cannot show, once, and keeps the picture.
    ///
    /// 이것을 실패시키는 것: 판정 표의 자르기·회전·효과 행을 각각 지우는 것(그 파일만
    /// 경고가 사라진다).
    #[test]
    fn hancom_picture_crop_turn_and_effect_are_reported() {
        for (name, reason) in [
            ("group-f3c-cropped.hwpx", "crop:"),
            ("group-f3d-rotated-pic.hwpx", "rotation/flip:"),
            ("group-f3e-gray.hwpx", "effect: GRAY_SCALE"),
        ] {
            let doc = HwpxDecoder::decode(&fixture(name)).unwrap();
            let lost = lost(&doc.warnings);
            assert_eq!(lost.len(), 1, "{name}: {lost:?}");
            assert_eq!(lost[0].0, "group_child_picture", "{name}");
            assert!(lost[0].1.starts_with(reason), "{name}: {lost:?}");
            assert_eq!(pictures(&first_group(&doc)).len(), 3, "{name}");
        }
    }

    /// The turned picture of F3d stays as an unturned box around the same
    /// centre: Hancom's centre of the 30° picture is T·R·(4252, 2834.5) =
    /// (15021.0, 4580.5) in the group (its `rotMatrix` turns about the
    /// box's centre); the Core box is 8504 x 5669 around it.
    ///
    /// 이것을 실패시키는 것: 회전한 상자의 모서리(offset)를 축 정렬 상자의 모서리로
    /// 그대로 쓰는 것(중심이 어긋난다).
    #[test]
    fn turned_group_picture_keeps_its_centre() {
        let doc = HwpxDecoder::decode(&fixture("group-f3d-rotated-pic.hwpx")).unwrap();
        let (_, w, h, (x, y)) = pictures(&first_group(&doc))[1].clone();
        assert_eq!((w, h), (8504, 5669));
        let centre = (x + f64::from(w) / 2.0, y + f64::from(h) / 2.0);
        assert!(
            (centre.0 - 15_021.0).abs() <= 1.0 && (centre.1 - 4580.5).abs() <= 1.0,
            "{centre:?}"
        );
    }

    fn bare_pic(
        clip: (i32, i32, i32, i32),
        dim: Option<(i32, i32)>,
    ) -> crate::schema::section::HxPic {
        use crate::schema::section::{HxImg, HxImgClip, HxImgDim, HxPic};
        HxPic {
            img: Some(HxImg {
                binary_item_id_ref: "image2".to_string(),
                effect: "REAL_PIC".to_string(),
                alpha: "0".to_string(),
                ..Default::default()
            }),
            img_clip: Some(HxImgClip { left: clip.0, right: clip.1, top: clip.2, bottom: clip.3 }),
            img_dim: dim.map(|(w, h)| HxImgDim { dim_width: w, dim_height: h }),
            ..Default::default()
        }
    }

    fn reasons(pic: &crate::schema::section::HxPic, pixels: &[(&str, (u32, u32))]) -> Vec<String> {
        let map: std::collections::HashMap<String, (u32, u32)> =
            pixels.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        super::group_picture_losses(pic, &map)
            .into_iter()
            .map(|r| r.split(':').next().unwrap().to_string())
            .collect()
    }

    /// The crop rule (W0, §8.1): more than one pixel (75 HWPUNIT) off the
    /// whole image is a crop. The whole image is `imgDim`; without it the
    /// image's pixel size × 75; without both the crop cannot be ruled out.
    /// The cases are corpus values: 7050 of 7080 (`imgDim` one pixel off the
    /// image), a 94 x 33 logo with no `imgDim` (clip 7080 x 2460 = uncropped,
    /// clip with right 0 and the size in top/bottom = reported), and no size
    /// at all.
    ///
    /// 이것을 실패시키는 것: 허용 폭 `>` 를 `>=` 로(75 가 손실이 된다) / 픽셀 크기
    /// 대체를 지우는 것 / 크기 없음 갈래 방출 삭제 / `left`·`top` 을 비교에서 빼는 것.
    #[test]
    fn group_picture_crop_rule() {
        assert!(reasons(&bare_pic((0, 22_500, 0, 15_000), Some((22_500, 15_000))), &[]).is_empty());
        assert!(reasons(&bare_pic((0, 7050, 0, 2460), Some((7080, 2460))), &[]).is_empty());
        assert!(reasons(&bare_pic((0, 7005, 0, 2460), Some((7080, 2460))), &[]).is_empty());
        assert_eq!(reasons(&bare_pic((0, 7004, 0, 2460), Some((7080, 2460))), &[]), ["crop"]);
        assert_eq!(reasons(&bare_pic((76, 7080, 0, 2460), Some((7080, 2460))), &[]), ["crop"]);
        let logo = [("image2", (94, 33))];
        assert!(reasons(&bare_pic((0, 7080, 0, 2460), None), &logo).is_empty());
        assert!(reasons(&bare_pic((0, 7080, 0, 2460), Some((0, 0))), &logo).is_empty());
        assert_eq!(reasons(&bare_pic((0, 0, 7080, 2460), None), &logo), ["crop"]);
        assert_eq!(reasons(&bare_pic((0, 7080, 0, 2460), None), &[]), ["crop"]);
        assert_eq!(reasons(&bare_pic((0, 7080, 0, 2460), Some((0, 0))), &[]), ["crop"]);
    }

    /// Image effects and a visible border are reported; `lineShape` NONE and
    /// an empty `<hp:effects/>` are not.
    ///
    /// 이것을 실패시키는 것: effect·bright·contrast·alpha·effects 자식·테두리 조건을
    /// 각각 지우는 것 / NONE 테두리도 손실로 보는 것.
    #[test]
    fn group_picture_effect_and_border_rule() {
        use crate::schema::section::{HxAnyElement, HxLineShape, HxPicEffects};
        let base = || bare_pic((0, 22_500, 0, 15_000), Some((22_500, 15_000)));
        let mut p = base();
        p.img.as_mut().unwrap().effect = "GRAY_SCALE".to_string();
        assert_eq!(reasons(&p, &[]), ["effect"]);
        let mut p = base();
        p.img.as_mut().unwrap().bright = -10;
        assert_eq!(reasons(&p, &[]), ["effect"]);
        let mut p = base();
        p.img.as_mut().unwrap().contrast = 5;
        assert_eq!(reasons(&p, &[]), ["effect"]);
        let mut p = base();
        p.img.as_mut().unwrap().alpha = "50".to_string();
        assert_eq!(reasons(&p, &[]), ["effect"]);
        let mut p = base();
        p.effects = Some(HxPicEffects { items: vec![HxAnyElement::Element] });
        assert_eq!(reasons(&p, &[]), ["effect"]);
        p.effects = Some(HxPicEffects::default());
        assert!(reasons(&p, &[]).is_empty());
        let mut p = base();
        p.line_shape = Some(HxLineShape {
            style: "DOT".to_string(),
            width: 75,
            ..HxLineShape::default_solid()
        });
        assert_eq!(reasons(&p, &[]), ["border"]);
        p.line_shape =
            Some(HxLineShape { style: "NONE".to_string(), ..HxLineShape::default_solid() });
        assert!(reasons(&p, &[]).is_empty());
    }

    /// A Hancom `<hp:effects>` with a shadow inside is read as having a
    /// child (the element is parsed, not skipped).
    ///
    /// 이것을 실패시키는 것: `HxPic::effects` 의 역직렬화 이름을 지우는 것.
    #[test]
    fn group_picture_effects_element_is_read() {
        let shadow = r#"<hp:effects><hp:shadow style="OUTSIDE" alpha="0.5"/></hp:effects>"#;
        let one = pic((1000, 600), &ri(ID, &[(ID, ID), (ID, ID)]), "image1", shadow);
        let (_, w) = decode(&container((3000, 3000), &one));
        assert_eq!(lost(&w).iter().map(|(_, r)| &r[..7]).collect::<Vec<_>>(), ["effect:"]);
    }

    /// A picture in a nested group counts in the nested group's box: the
    /// group (2000 wide) spans a 1000 rect and a 1000 picture beside it, and
    /// nothing is reported.
    ///
    /// 이것을 실패시키는 것: `leaf_bounds` 의 Picture 갈래 삭제
    /// (`nested_group_placement`) / `core_child_corners` 의 Picture 모서리 삭제
    /// (Core 중첩 묶음 폭 1000).
    #[test]
    fn picture_counts_in_a_nested_group_box() {
        let leaf = rect((1000, 1000), &ri(ID, &[(ID, ID), (ID, ID), (ID, ID)]));
        let one = pic(
            (1000, 1000),
            &ri([1.0, 0.0, 1000.0, 0.0, 1.0, 0.0], &[(ID, ID), (ID, ID), (ID, ID)]),
            "image1",
            "",
        )
        .replace(r#"groupLevel="1""#, r#"groupLevel="2""#);
        let own = ri(ID, &[(ID, ID), (ID, ID)]);
        let (g, w) =
            decode(&container((3000, 3000), &nested((2000, 1000), &own, &format!("{leaf}{one}"))));
        assert!(w.is_empty(), "{w:?}");
        let inner = children(&g.unwrap())[0].clone();
        let Control::Group { width, .. } = &inner else { panic!("{inner:?}") };
        assert_eq!(width.as_i32(), 2000);
        assert_eq!(pictures(&inner), [("BinData/image1".to_string(), 1000, 1000, (1000.0, 0.0))]);
    }

    /// A group picture whose `<hp:imgDim>` is missing is judged against its
    /// image's pixel size read from BinData: F3a with `imgDim` removed (its
    /// logos are 300 x 200 px = 22500 x 15000, the clip) loses nothing.
    ///
    /// 이것을 실패시키는 것: 패키지 디코더가 픽셀 크기 표를 넘기지 않는 것(세 그림이
    /// 모두 "크기 모름" 자르기로 보고된다).
    #[test]
    fn missing_image_size_is_read_from_the_image() {
        use std::io::{Read, Write};
        let src = fixture("group-f3a-logo.hwpx");
        let mut zin = zip::ZipArchive::new(std::io::Cursor::new(&src)).unwrap();
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for i in 0..zin.len() {
            let mut f = zin.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            if name == "Contents/section0.xml" {
                let xml = String::from_utf8(data).unwrap();
                let stripped =
                    xml.replace(r#"<hp:imgDim dimwidth="22500" dimheight="15000"/>"#, "");
                assert_ne!(stripped, xml);
                data = stripped.into_bytes();
            }
            let opts = zip::write::SimpleFileOptions::default().compression_method(f.compression());
            out.start_file(name, opts).unwrap();
            out.write_all(&data).unwrap();
        }
        let bytes = out.finish().unwrap().into_inner();
        let doc = HwpxDecoder::decode(&bytes).unwrap();
        assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
    }

    /// A picture turned 30° alone in a nested group: it stays an unturned
    /// 1000 x 600 box around its centre (5500, 5300), the nested group is
    /// sized by that final box (not by the turned one), and the picture sits
    /// at the group's corner.
    ///
    /// 이것을 실패시키는 것: 중첩 묶음 자식 이동(`shift_child`)의 Picture 갈래 삭제
    /// (그림 offset 이 회전 상자 모서리 기준으로 남는다) / bounds 를 회전 모서리로.
    #[test]
    fn turned_picture_in_a_nested_group_is_an_unturned_box() {
        let turn = [
            0.866_025_403_784_438_7,
            -0.5,
            216.987_298_107_780_6,
            0.5,
            0.866_025_403_784_438_7,
            -209.807_621_135_331_6,
        ];
        let one = pic(
            (1000, 600),
            &ri([1.0, 0.0, 5000.0, 0.0, 1.0, 5000.0], &[(ID, ID), (ID, ID), (ID, turn)]),
            "image1",
            "",
        )
        .replace(r#"groupLevel="1""#, r#"groupLevel="2""#);
        let own = ri([1.0, 0.0, 4916.987, 0.0, 1.0, 4790.192], &[(ID, ID), (ID, ID)]);
        let (g, w) = decode(&container((7000, 7000), &nested((1166, 1020), &own, &one)));
        assert_eq!(
            lost(&w).iter().map(|(k, r)| (k.as_str(), &r[..14])).collect::<Vec<_>>(),
            [("group_child_picture", "rotation/flip:")]
        );
        let inner = children(&g.unwrap())[0].clone();
        let Control::Group { width, height, placement, .. } = &inner else { panic!("{inner:?}") };
        assert_eq!((width.as_i32(), height.as_i32()), (1000, 600));
        assert_eq!(offset_of(placement), (5000.0, 5000.0));
        assert_eq!(pictures(&inner), [("BinData/image1".to_string(), 1000, 600, (0.0, 0.0))]);
    }

    /// The `<hp:pic>` elements of a written section, as raw XML.
    fn written_pics(xml: &str) -> Vec<&str> {
        xml.match_indices("<hp:pic ")
            .map(|(i, _)| &xml[i..i + xml[i..].find("</hp:pic>").unwrap()])
            .collect()
    }

    /// Hancom's F3a and F3b (pictures between shapes) written back keep every
    /// picture, in order, as Hancom writes a group picture — no
    /// `<hp:sz>`/`<hp:pos>`/`<hp:outMargin>`, `numberingType` NONE,
    /// `textWrap` TOP_AND_BOTTOM, `groupLevel` 1, its offset in `<hp:offset>`
    /// and the `transMatrix` translation, `curSz` 0 — and decode again to the
    /// same pictures with no warning.
    ///
    /// 이것을 실패시키는 것: 인코더 Picture 갈래 삭제(그림이 CONTENT_LOST 로 빠짐) /
    /// `sz`·`pos`·`outMargin` None 설정 삭제 / NONE·TOP_AND_BOTTOM 줄 삭제 / offset 을
    /// 원점으로 / `set_group_level` 생략(각각 따로).
    #[test]
    fn hancom_picture_groups_round_trip() {
        for (name, pic_offsets) in [
            ("group-f3a-logo.hwpx", vec![(9921, 0), (0, 0), (19_842, 0)]),
            ("group-f3b-mixed.hwpx", vec![]),
        ] {
            let doc = HwpxDecoder::decode(&fixture(name)).unwrap();
            let validated = doc.document.clone().validate().expect("validate");
            let out = crate::HwpxEncoder::encode_with_diagnostics(
                &validated,
                &doc.style_store,
                &doc.image_store,
                crate::EncodeOptions::default(),
            )
            .unwrap();
            assert!(out.warnings.is_empty(), "{name}: {:?}", out.warnings);
            let bytes = out.bytes;
            let xml = section0_xml(&bytes);
            let pics = written_pics(&xml);
            assert_eq!(pics.len(), pictures(&first_group(&doc)).len(), "{name}");
            for p in &pics {
                for absent in ["<hp:sz ", "<hp:pos ", "<hp:outMargin "] {
                    assert!(!p.contains(absent), "{name}: {absent} in {p}");
                }
                for present in [
                    r#"numberingType="NONE""#,
                    r#"textWrap="TOP_AND_BOTTOM""#,
                    r#"groupLevel="1""#,
                    r#"<hp:curSz width="0" height="0"/>"#,
                ] {
                    assert!(p.contains(present), "{name}: no {present} in {p}");
                }
            }
            for (p, (x, y)) in pics.iter().zip(&pic_offsets) {
                assert!(p.contains(&format!(r#"<hp:offset x="{x}" y="{y}"/>"#)), "{p}");
                assert!(p.contains(&format!(r#"e3="{x}" e4="0" e5="1" e6="{y}""#)), "{p}");
            }
            let again = HwpxDecoder::decode(&bytes).unwrap();
            assert!(again.warnings.is_empty(), "{name}: {:?}", again.warnings);
            assert_eq!(first_group(&again), first_group(&doc), "{name}");
        }
    }

    /// The turned picture of a nested group (kept as an unturned box) is
    /// written as that box and decodes again to the same group: decoding is
    /// stable on its own output.
    ///
    /// 이것을 실패시키는 것: 인코더가 그림 offset 에 부모 원점을 더하지 않는 것(중첩
    /// 그림이 바깥 묶음 모서리로 간다).
    #[test]
    fn turned_picture_in_a_nested_group_round_trips() {
        let turn = [
            0.866_025_403_784_438_7,
            -0.5,
            216.987_298_107_780_6,
            0.5,
            0.866_025_403_784_438_7,
            -209.807_621_135_331_6,
        ];
        let one = pic(
            (1000, 600),
            &ri([1.0, 0.0, 5000.0, 0.0, 1.0, 5000.0], &[(ID, ID), (ID, ID), (ID, turn)]),
            "image1",
            "",
        )
        .replace(r#"groupLevel="1""#, r#"groupLevel="2""#);
        let own = ri([1.0, 0.0, 4916.987, 0.0, 1.0, 4790.192], &[(ID, ID), (ID, ID)]);
        let (g, _) = decode(&container((7000, 7000), &nested((1166, 1020), &own, &one)));
        let g = g.unwrap();
        let mut doc = HwpxDecoder::decode(&fixture("group-f3a-logo.hwpx")).unwrap();
        replace_first_group(&mut doc, g.clone());
        let again = HwpxDecoder::decode(&encode(&doc)).unwrap();
        assert!(again.warnings.is_empty(), "{:?}", again.warnings);
        let back = first_group(&again);
        assert_eq!(children(&back)[0], children(&g)[0]);
    }

    /// A group picture's link is written into its `href` and read back.
    ///
    /// 이것을 실패시키는 것: 묶음 그림 인코드에서 `build_picture` 대신 `href` 를 비우는 것.
    #[test]
    fn group_picture_hyperlink_round_trips() {
        let mut doc = HwpxDecoder::decode(&fixture("group-f3a-logo.hwpx")).unwrap();
        let mut g = first_group(&doc);
        let Control::Group { children, .. } = &mut g else { unreachable!() };
        let GroupChild::Picture(first) = &mut children[0] else { panic!() };
        first.hyperlink = Some("http://www.korea.kr".to_string());
        replace_first_group(&mut doc, g.clone());
        let bytes = encode(&doc);
        assert!(section0_xml(&bytes).contains(r#"href="http://www.korea.kr;1;0;0;""#));
        assert_eq!(first_group(&HwpxDecoder::decode(&bytes).unwrap()), g);
    }

    /// A group's own hyperlink (`<hp:container href>`, 59 groups in the
    /// corpus) has no place in a Core group and is reported, for a top-level
    /// and a nested group alike.
    ///
    /// 이것을 실패시키는 것: `href` 검사 삭제 / 스키마에서 `@href` 를 읽지 않는 것.
    #[test]
    fn group_hyperlink_is_reported() {
        let leaf = rect((1000, 1000), &ri(ID, &[(ID, ID), (ID, ID), (ID, ID)]));
        let own = ri(ID, &[(ID, ID), (ID, ID)]);
        let linked =
            |xml: String| xml.replacen(r#"href="""#, r#"href="http://www.korea.kr;1;0;0;""#, 1);
        let (g, w) = decode(&linked(container(
            (3000, 3000),
            &rect((1000, 1000), &ri(ID, &[(ID, ID), (ID, ID)])),
        )));
        assert!(g.is_some());
        assert_eq!(lost_kinds(&w), ["group_hyperlink"]);
        let (_, w) = decode(&container((3000, 3000), &linked(nested((1000, 1000), &own, &leaf))));
        assert_eq!(lost_kinds(&w), ["group_hyperlink"]);
    }

    /// `imgClip` and `imgDim` are any i32 on the wire: the crop test does not
    /// overflow at the extremes of either axis, and still reports the crop.
    ///
    /// 이것을 실패시키는 것: 뺄셈을 i32 로 되돌리는 것(debug 패닉 / release 에서 차이가
    /// 1 로 감겨 crop 이 사라짐).
    #[test]
    fn crop_rule_survives_extreme_values() {
        let (min, max) = (i32::MIN, i32::MAX);
        assert_eq!(reasons(&bare_pic((0, min, 0, 15_000), Some((max, 15_000))), &[]), ["crop"]);
        assert_eq!(reasons(&bare_pic((0, 22_500, 0, min), Some((22_500, max))), &[]), ["crop"]);
        assert_eq!(
            reasons(&bare_pic((min, 22_500, min, 15_000), Some((22_500, 15_000))), &[]),
            ["crop"]
        );
        assert!(reasons(&bare_pic((0, max, 0, max), Some((max, max))), &[]).is_empty());
    }

    /// A picture whose `binaryItemIDRef` keeps the extension
    /// (`image1.png`, as some writers save it) is judged against its image's
    /// pixel size like Hancom's extensionless reference: F3a with `imgDim`
    /// removed and every reference written with its extension loses nothing.
    ///
    /// 이것을 실패시키는 것: 픽셀 크기 표에 저장된 전체 이름을 넣지 않는 것(세 그림이
    /// "크기 모름" 자르기로 보고된다).
    #[test]
    fn image_reference_with_its_extension_finds_the_image_size() {
        use std::io::{Read, Write};
        let src = fixture("group-f3a-logo.hwpx");
        let mut zin = zip::ZipArchive::new(std::io::Cursor::new(&src)).unwrap();
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for i in 0..zin.len() {
            let mut f = zin.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            if name == "Contents/section0.xml" {
                let mut xml = String::from_utf8(data).unwrap();
                xml = xml.replace(r#"<hp:imgDim dimwidth="22500" dimheight="15000"/>"#, "");
                for n in 1..=3 {
                    xml = xml.replace(
                        &format!(r#"binaryItemIDRef="image{n}""#),
                        &format!(r#"binaryItemIDRef="image{n}.png""#),
                    );
                }
                assert_eq!(xml.matches(r#".png" bright"#).count(), 3);
                data = xml.into_bytes();
            }
            let opts = zip::write::SimpleFileOptions::default().compression_method(f.compression());
            out.start_file(name, opts).unwrap();
            out.write_all(&data).unwrap();
        }
        let doc = HwpxDecoder::decode(&out.finish().unwrap().into_inner()).unwrap();
        assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
    }
}
