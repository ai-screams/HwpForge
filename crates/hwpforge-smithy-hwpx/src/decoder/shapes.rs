//! Decodes HWPX shape elements into Core control types.
//!
//! Split from `section.rs` to enable parallel development of shape features.
//! Functions here convert `HxRect`, `HxLine`, `HxEllipse`, and `HxPolygon`
//! into Core `Run` values with the appropriate `Control` variant.

use hwpforge_core::control::{Control, ShapeStyle};
use hwpforge_core::run::{Run, RunContent};
use hwpforge_foundation::{ArcType, CharShapeIndex, Color, CurveSegmentType, Flip, HwpUnit};

use crate::error::HwpxResult;
use crate::schema::section::{
    HxConnectLine, HxCurve, HxEllipse, HxFillBrush, HxLine, HxLineShape, HxPolygon, HxRect,
};

use super::section::{convert_hx_caption, decode_sublist_paragraphs, parse_hex_color};

/// Decodes an `HxRect`'s draw text into a Core `Run` with `Control::TextBox`, if present.
///
/// Only rects with `<hp:drawText>` are treated as textboxes; rects without
/// text content (pure shapes) are silently skipped.
pub(crate) fn decode_textbox(
    rect: &HxRect,
    char_shape_id: CharShapeIndex,
    depth: usize,
) -> HwpxResult<Option<Run>> {
    let draw_text = match &rect.draw_text {
        Some(dt) => dt,
        None => return Ok(None),
    };

    let paragraphs = decode_sublist_paragraphs(&draw_text.sub_list, depth)?;

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

    // Extract offsets from pos (treatAsChar=1 means inline, offsets=0)
    let (horz_offset, vert_offset) =
        rect.pos.as_ref().map(|p| (p.horz_offset, p.vert_offset)).unwrap_or((0, 0));

    let caption = rect.caption.as_ref().map(|c| convert_hx_caption(c, depth)).transpose()?;

    Ok(Some(Run {
        content: RunContent::Control(Box::new(Control::TextBox {
            paragraphs,
            width,
            height,
            horz_offset,
            vert_offset,
            caption,
            style: None,
        })),
        char_shape_id,
    }))
}

/// Decodes an `HxLine` into a Core `Run` with `Control::Line`.
pub(crate) fn decode_line(
    line: &HxLine,
    char_shape_id: CharShapeIndex,
    depth: usize,
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

    let caption = line.caption.as_ref().map(|c| convert_hx_caption(c, depth)).transpose()?;

    let (horz_offset, vert_offset) =
        line.pos.as_ref().map(|p| (p.horz_offset, p.vert_offset)).unwrap_or((0, 0));

    Ok(Run {
        content: RunContent::Control(Box::new(Control::Line {
            start,
            end,
            width,
            height,
            horz_offset,
            vert_offset,
            caption,
            style: decode_shape_style(&line.line_shape, &line.fill_brush),
        })),
        char_shape_id,
    })
}

/// Decodes an `HxEllipse` into a Core `Run` with `Control::Ellipse`.
pub(crate) fn decode_ellipse(
    ellipse: &HxEllipse,
    char_shape_id: CharShapeIndex,
    depth: usize,
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

    let paragraphs = match &ellipse.draw_text {
        Some(dt) => decode_sublist_paragraphs(&dt.sub_list, depth)?,
        None => Vec::new(),
    };

    let caption = ellipse.caption.as_ref().map(|c| convert_hx_caption(c, depth)).transpose()?;

    let (horz_offset, vert_offset) =
        ellipse.pos.as_ref().map(|p| (p.horz_offset, p.vert_offset)).unwrap_or((0, 0));

    Ok(Run {
        content: RunContent::Control(Box::new(Control::Ellipse {
            center,
            axis1,
            axis2,
            width,
            height,
            horz_offset,
            vert_offset,
            paragraphs,
            caption,
            style: decode_shape_style(&ellipse.line_shape, &ellipse.fill_brush),
        })),
        char_shape_id,
    })
}

/// Decodes an `HxPolygon` into a Core `Run` with `Control::Polygon`.
pub(crate) fn decode_polygon(
    polygon: &HxPolygon,
    char_shape_id: CharShapeIndex,
    depth: usize,
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

    let paragraphs = match &polygon.draw_text {
        Some(dt) => decode_sublist_paragraphs(&dt.sub_list, depth)?,
        None => Vec::new(),
    };

    let caption = polygon.caption.as_ref().map(|c| convert_hx_caption(c, depth)).transpose()?;

    let (horz_offset, vert_offset) =
        polygon.pos.as_ref().map(|p| (p.horz_offset, p.vert_offset)).unwrap_or((0, 0));

    Ok(Run {
        content: RunContent::Control(Box::new(Control::Polygon {
            vertices,
            width,
            height,
            horz_offset,
            vert_offset,
            paragraphs,
            caption,
            style: decode_shape_style(&polygon.line_shape, &polygon.fill_brush),
        })),
        char_shape_id,
    })
}

/// Extracts a [`ShapeStyle`] from HWPX shape common elements.
///
/// Maps `HxLineShape` and `HxFillBrush` to Core's `ShapeStyle`.
/// Returns `None` if no style information is present.
pub(crate) fn decode_shape_style(
    line_shape: &Option<HxLineShape>,
    fill_brush: &Option<HxFillBrush>,
) -> Option<ShapeStyle> {
    decode_shape_style_full(line_shape, fill_brush, None, None)
}

/// Extended shape style decoder that also extracts rotation, flip, and arrow info.
pub(crate) fn decode_shape_style_full(
    line_shape: &Option<HxLineShape>,
    fill_brush: &Option<HxFillBrush>,
    rotation_info: Option<&crate::schema::section::HxRotationInfo>,
    flip_info: Option<&crate::schema::section::HxFlip>,
) -> Option<ShapeStyle> {
    use hwpforge_core::control::ArrowStyle;
    use hwpforge_foundation::{ArrowSize, ArrowType};

    let fill_color: Option<Color> = fill_brush
        .as_ref()
        .map(|fb| &fb.win_brush.face_color)
        .filter(|c| !c.is_empty())
        .and_then(|c| parse_hex_color(c));

    let (line_color, line_width, line_style) = match line_shape.as_ref() {
        None => (None, None, None),
        Some(ls) => (
            if ls.color.is_empty() { None } else { parse_hex_color(&ls.color) },
            if ls.width == 0 { None } else { u32::try_from(ls.width).ok() },
            if ls.style.is_empty() {
                None
            } else {
                ls.style.parse::<hwpforge_core::control::LineStyle>().ok()
            },
        ),
    };

    // Decode rotation (HWPX stores angle * 100)
    let rotation: Option<f32> =
        rotation_info.filter(|ri| ri.angle != 0).map(|ri| ri.angle as f32 / 100.0);

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

    let has_anything = line_color.is_some()
        || line_width.is_some()
        || line_style.is_some()
        || fill_color.is_some()
        || rotation.is_some()
        || flip.is_some()
        || head_arrow.is_some()
        || tail_arrow.is_some();

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
        fill: None,
    })
}

/// Decodes an `HxEllipse` with `hasArcPr=1` into a Core `Run` with `Control::Arc`.
pub(crate) fn decode_arc(
    ellipse: &HxEllipse,
    char_shape_id: CharShapeIndex,
    _depth: usize,
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

    let (horz_offset, vert_offset) =
        ellipse.pos.as_ref().map(|p| (p.horz_offset, p.vert_offset)).unwrap_or((0, 0));
    let caption = ellipse.caption.as_ref().map(|c| convert_hx_caption(c, _depth)).transpose()?;

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
            horz_offset,
            vert_offset,
            caption,
            style: decode_shape_style_full(
                &ellipse.line_shape,
                &ellipse.fill_brush,
                ellipse.rotation_info.as_ref(),
                ellipse.flip.as_ref(),
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
) -> HwpxResult<Run> {
    use hwpforge_core::control::ShapePoint;

    let points: Vec<ShapePoint> = curve.points.iter().map(|p| ShapePoint::new(p.x, p.y)).collect();

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

    let (horz_offset, vert_offset) =
        curve.pos.as_ref().map(|p| (p.horz_offset, p.vert_offset)).unwrap_or((0, 0));
    let caption = curve.caption.as_ref().map(|c| convert_hx_caption(c, depth)).transpose()?;

    Ok(Run {
        content: RunContent::Control(Box::new(Control::Curve {
            points,
            segment_types,
            width,
            height,
            horz_offset,
            vert_offset,
            caption,
            style: decode_shape_style_full(
                &curve.line_shape,
                &curve.fill_brush,
                curve.rotation_info.as_ref(),
                curve.flip.as_ref(),
            ),
        })),
        char_shape_id,
    })
}

/// Decodes an `HxConnectLine` into a Core `Run` with `Control::ConnectLine`.
pub(crate) fn decode_connect_line(
    cl: &HxConnectLine,
    char_shape_id: CharShapeIndex,
    depth: usize,
) -> HwpxResult<Run> {
    use hwpforge_core::control::ShapePoint;

    let start =
        cl.start_pt.as_ref().map(|p| ShapePoint::new(p.x, p.y)).unwrap_or(ShapePoint::new(0, 0));
    let end =
        cl.end_pt.as_ref().map(|p| ShapePoint::new(p.x, p.y)).unwrap_or(ShapePoint::new(0, 0));
    let control_points: Vec<ShapePoint> =
        cl.control_points.iter().map(|p| ShapePoint::new(p.x, p.y)).collect();

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

    let (horz_offset, vert_offset) =
        cl.pos.as_ref().map(|p| (p.horz_offset, p.vert_offset)).unwrap_or((0, 0));
    let caption = cl.caption.as_ref().map(|c| convert_hx_caption(c, depth)).transpose()?;

    Ok(Run {
        content: RunContent::Control(Box::new(Control::ConnectLine {
            start,
            end,
            control_points,
            connect_type: cl.connect_type.clone(),
            width,
            height,
            horz_offset,
            vert_offset,
            caption,
            style: decode_shape_style_full(
                &cl.line_shape,
                &cl.fill_brush,
                cl.rotation_info.as_ref(),
                cl.flip.as_ref(),
            ),
        })),
        char_shape_id,
    })
}
