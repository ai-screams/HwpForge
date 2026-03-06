//! Decodes HWPX shape elements into Core control types.
//!
//! Split from `section.rs` to enable parallel development of shape features.
//! Functions here convert `HxRect`, `HxLine`, `HxEllipse`, and `HxPolygon`
//! into Core `Run` values with the appropriate `Control` variant.

use hwpforge_core::control::{Control, ShapeStyle};
use hwpforge_core::run::{Run, RunContent};
use hwpforge_foundation::{
    ArcType, CharShapeIndex, Color, CurveSegmentType, DropCapStyle, Flip, HwpUnit,
};

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
            style: decode_shape_style(&line.line_shape, &line.fill_brush, &line.dropcap_style),
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
            style: decode_shape_style(
                &ellipse.line_shape,
                &ellipse.fill_brush,
                &ellipse.dropcap_style,
            ),
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
            style: decode_shape_style(
                &polygon.line_shape,
                &polygon.fill_brush,
                &polygon.dropcap_style,
            ),
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
    dropcap_style: &str,
) -> Option<ShapeStyle> {
    decode_shape_style_full(line_shape, fill_brush, None, None, dropcap_style)
}

/// Extended shape style decoder that also extracts rotation, flip, arrow, and drop cap info.
pub(crate) fn decode_shape_style_full(
    line_shape: &Option<HxLineShape>,
    fill_brush: &Option<HxFillBrush>,
    rotation_info: Option<&crate::schema::section::HxRotationInfo>,
    flip_info: Option<&crate::schema::section::HxFlip>,
    dropcap_style: &str,
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

    let drop_cap = DropCapStyle::from_hwpx_str(dropcap_style);

    let has_anything = line_color.is_some()
        || line_width.is_some()
        || line_style.is_some()
        || fill_color.is_some()
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
        fill: None,
        drop_cap_style: drop_cap,
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
                &ellipse.dropcap_style,
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
                &curve.dropcap_style,
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
                &cl.dropcap_style,
            ),
        })),
        char_shape_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::section::{
        HxConnectLine, HxControlPoint, HxControlPoints, HxCurve, HxCurveSegment, HxEllipse,
        HxFillBrush, HxFlip, HxLine, HxLineShape, HxPoint, HxRotationInfo, HxTablePos, HxTableSz,
    };
    use hwpforge_core::control::{Control, ShapePoint};
    use hwpforge_foundation::{ArcType, ArrowSize, ArrowType, CharShapeIndex, DropCapStyle, Flip};

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
            win_brush: HxWinBrush {
                face_color: face_color.to_string(),
                hatch_color: "#000000".to_string(),
                alpha: 0,
            },
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
        // Empty color string and zero width should not produce a style
        let ls =
            make_line_shape("", 0, "", "NORMAL", "NORMAL", 1, 1, "MEDIUM_MEDIUM", "MEDIUM_MEDIUM");
        let result = decode_shape_style(&Some(ls), &None, "None");
        assert!(result.is_none(), "empty color + zero width + empty style should be None");
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
        let ri = HxRotationInfo { angle: 4500, center_x: 50, center_y: 50, rotate_image: 1 };
        let style = decode_shape_style_full(&None, &None, Some(&ri), None, "None").unwrap();
        let rot = style.rotation.unwrap();
        assert!((rot - 45.0f32).abs() < 0.01, "45 degrees expected, got {rot}");
    }

    #[test]
    fn decode_shape_style_full_rotation_zero_ignored() {
        let ri = HxRotationInfo { angle: 0, center_x: 0, center_y: 0, rotate_image: 1 };
        let result = decode_shape_style_full(&None, &None, Some(&ri), None, "None");
        assert!(result.is_none(), "zero rotation + no other fields should be None");
    }

    #[test]
    fn decode_shape_style_full_flip_horizontal() {
        let fi = HxFlip { horizontal: 1, vertical: 0 };
        let style = decode_shape_style_full(&None, &None, None, Some(&fi), "None").unwrap();
        assert_eq!(style.flip, Some(Flip::Horizontal));
    }

    #[test]
    fn decode_shape_style_full_flip_vertical() {
        let fi = HxFlip { horizontal: 0, vertical: 1 };
        let style = decode_shape_style_full(&None, &None, None, Some(&fi), "None").unwrap();
        assert_eq!(style.flip, Some(Flip::Vertical));
    }

    #[test]
    fn decode_shape_style_full_flip_both() {
        let fi = HxFlip { horizontal: 1, vertical: 1 };
        let style = decode_shape_style_full(&None, &None, None, Some(&fi), "None").unwrap();
        assert_eq!(style.flip, Some(Flip::Both));
    }

    #[test]
    fn decode_shape_style_full_flip_none_ignored() {
        let fi = HxFlip { horizontal: 0, vertical: 0 };
        let result = decode_shape_style_full(&None, &None, None, Some(&fi), "None");
        assert!(result.is_none(), "zero flip should yield None");
    }

    #[test]
    fn decode_shape_style_full_combined_rotation_and_flip() {
        let ri = HxRotationInfo { angle: 9000, center_x: 0, center_y: 0, rotate_image: 1 };
        let fi = HxFlip { horizontal: 1, vertical: 0 };
        let style = decode_shape_style_full(&None, &None, Some(&ri), Some(&fi), "None").unwrap();
        let rot = style.rotation.unwrap();
        assert!((rot - 90.0f32).abs() < 0.01);
        assert_eq!(style.flip, Some(Flip::Horizontal));
    }

    // ── decode_arc tests ─────────────────────────────────────────────

    #[test]
    fn decode_arc_default_fields_normal_type() {
        let ellipse = default_ellipse();
        let cs = CharShapeIndex::new(0);
        let run = decode_arc(&ellipse, cs, 0).unwrap();
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
        let run = decode_arc(&ellipse, cs, 0).unwrap();
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
        let run = decode_arc(&ellipse, cs, 0).unwrap();
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
        let run = decode_arc(&ellipse, cs, 0).unwrap();
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
        let run = decode_arc(&ellipse, cs, 0).unwrap();
        if let Control::Arc { width, height, horz_offset, vert_offset, .. } =
            run.content.as_control().unwrap().clone()
        {
            assert_eq!(width.as_i32(), 5000);
            assert_eq!(height.as_i32(), 3000);
            assert_eq!(horz_offset, 100);
            assert_eq!(vert_offset, 200);
        } else {
            panic!("expected Control::Arc");
        }
    }

    #[test]
    fn decode_arc_with_rotation_style() {
        let mut ellipse = default_ellipse();
        ellipse.rotation_info =
            Some(HxRotationInfo { angle: 4500, center_x: 50, center_y: 50, rotate_image: 1 });
        let cs = CharShapeIndex::new(0);
        let run = decode_arc(&ellipse, cs, 0).unwrap();
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
        let run = decode_arc(&ellipse, cs, 0).unwrap();
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
        let run = decode_curve(&curve, cs, 0).unwrap();
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
        let run = decode_curve(&curve, cs, 0).unwrap();
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
        let run = decode_curve(&curve, cs, 0).unwrap();
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
        let run = decode_curve(&curve, cs, 0).unwrap();
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
        let run = decode_curve(&curve, cs, 0).unwrap();
        assert_eq!(run.char_shape_id, cs);
        if let Control::Curve { width, height, horz_offset, vert_offset, .. } =
            run.content.as_control().unwrap().clone()
        {
            assert_eq!(width.as_i32(), 8000);
            assert_eq!(height.as_i32(), 4000);
            assert_eq!(horz_offset, 50);
            assert_eq!(vert_offset, 75);
        } else {
            panic!("expected Control::Curve");
        }
    }

    // ── decode_connect_line tests ────────────────────────────────────

    #[test]
    fn decode_connect_line_defaults() {
        let cl = default_connect_line();
        let cs = CharShapeIndex::new(0);
        let run = decode_connect_line(&cl, cs, 0).unwrap();
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
        let run = decode_connect_line(&cl, cs, 0).unwrap();
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
        let run = decode_connect_line(&cl, cs, 0).unwrap();
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
        let run = decode_connect_line(&cl, cs, 0).unwrap();
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
        let run = decode_connect_line(&cl, cs, 0).unwrap();
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
        let run = decode_connect_line(&cl, cs, 0).unwrap();
        assert_eq!(run.char_shape_id, cs);
        if let Control::ConnectLine { width, height, horz_offset, vert_offset, .. } =
            run.content.as_control().unwrap().clone()
        {
            assert_eq!(width.as_i32(), 10000);
            assert_eq!(height.as_i32(), 5000);
            assert_eq!(horz_offset, 150);
            assert_eq!(vert_offset, 250);
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
        let run = decode_line(&line, cs, 0).unwrap();
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

    #[test]
    fn decode_ellipse_defaults_all_zero() {
        let ellipse = default_ellipse();
        let cs = CharShapeIndex::new(0);
        let run = decode_ellipse(&ellipse, cs, 0).unwrap();
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
}
