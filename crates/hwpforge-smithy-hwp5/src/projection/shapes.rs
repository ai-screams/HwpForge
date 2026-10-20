//! GSO shape projection helpers for HWP5 → Core projection.
//!
//! This submodule holds the leaf builders that turn decoded HWP5 GSO shape
//! controls (line, rect, polygon, ellipse, arc, curve, connect line) into
//! Core `Control` runs, together with the point-scaling helpers that map raw
//! shape coordinates into the owning bounding box. They carry no
//! document-traversal state; the parent `projection` module dispatches each
//! GSO control into the matching builder here.

use hwpforge_core::placement::{ObjectPlacement, ObjectRelativeTo, ObjectTextWrap};
use hwpforge_core::run::Run;
use hwpforge_core::Control;
use hwpforge_foundation::{ArcType, CharShapeIndex, CurveSegmentType, HwpUnit};

use crate::decoder::section::{
    Hwp5ArcControl, Hwp5ConnectLineControl, Hwp5CurveControl, Hwp5EllipseControl, Hwp5LineControl,
    Hwp5PolygonControl, Hwp5RectControl,
};
use crate::decoder::Hwp5Warning;
use crate::numeric::positive_i32_from_u32;
use crate::schema::section::{Hwp5ShapeComponentGeometry, Hwp5ShapePoint};

/// Derives a GSO shape's Core [`ObjectPlacement`] from the owning `gso `
/// `CtrlHeader` 속성 word via [`super::image_placement_from_wire`], which now
/// byte-grounds the full placement (W5 w0).
///
/// An inline (`treat_as_char`, bit0=1) shape collapses to `None` so the encoder
/// emits the legacy inline placement. A floating shape (bit0=0) carries the
/// axes decoded from the shared `gso ` CtrlHeader word (표 70):
/// `vertRelTo`/`horzRelTo`/`textWrap`/`textFlow`/`flowWithText`/`allowOverlap`
/// plus the signed `(x, y)` offset — no longer the pre-W5 anchor/wrap
/// convention. The `textbox_anchored` native `.hwp`/`.hwpx` pair (a floating
/// `<hp:rect>` 글상자: `vertRelTo=PARA`, `horzRelTo=PAGE`, `textWrap=SQUARE`)
/// is the byte-comparison gate.
///
/// `warnings` carries the byte-ground fail-closed diagnostics (unknown
/// `relTo`/`wrap` bits) emitted by [`super::object_placement_from_ctrl_properties`].
pub(super) fn shape_placement(
    geometry: &Hwp5ShapeComponentGeometry,
    ctrl_properties: u32,
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<ObjectPlacement> {
    let placement = super::image_placement_from_wire(
        geometry,
        super::ImageProjectionContext::Flow,
        ctrl_properties,
        warnings,
    );
    (placement != ObjectPlacement::legacy_inline_defaults()).then_some(placement)
}

/// The object's outer margin from its `CtrlHeader` bytes 28–36 (S18): `None`
/// when 0 on every side (so an inline default still collapses), and for a
/// negative side — the corpus has `-1` on all four in a few old documents,
/// whose meaning is unknown — which is reported and read as 0.
/// 이것을 실패시키는 것: 여백을 싣지 않기 / 음수 보고를 지우기
/// (`object_outer_margin_is_read`).
pub(super) fn outer_margin(
    geometry: &Hwp5ShapeComponentGeometry,
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<hwpforge_core::table::TableMargin> {
    let [left, right, top, bottom] = geometry.outer_margin;
    if [left, right, top, bottom] == [0; 4] {
        return None;
    }
    if [left, right, top, bottom].iter().any(|v| *v < 0) {
        super::push_projection_fallback(
            warnings,
            "object.out_margin",
            format!("outer margin {left} {right} {top} {bottom} has a negative side; read as 0"),
        );
        return None;
    }
    let unit = |v: i16| {
        hwpforge_foundation::HwpUnit::new(i32::from(v))
            .unwrap_or(hwpforge_foundation::HwpUnit::ZERO)
    };
    Some(hwpforge_core::table::TableMargin {
        left: unit(left),
        right: unit(right),
        top: unit(top),
        bottom: unit(bottom),
    })
}

/// `placement` with the outer margin of `geometry` (S18), on the inline
/// default when there was no placement.
pub(super) fn with_outer_margin(
    placement: Option<ObjectPlacement>,
    geometry: &Hwp5ShapeComponentGeometry,
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<ObjectPlacement> {
    match outer_margin(geometry, warnings) {
        None => placement,
        Some(margin) => {
            let mut placement = placement.unwrap_or_else(ObjectPlacement::legacy_inline_defaults);
            placement.out_margin = Some(margin);
            Some(placement)
        }
    }
}

/// Placement for shape families whose `gso ` 속성 word (bit0) is not threaded
/// into the decoder model — the group container, `EmbeddedChart`, and
/// `TextArt`. Falls back to the pre-placement offset heuristic: a non-zero
/// offset floats (`PAPER`, `IN_FRONT_OF_TEXT` — the established shape floating
/// convention), a zero offset stays inline (`None`). The chart/textart encoders
/// read only the offset (their `<hp:pos>` is a fixed template); the group
/// container encoder derives numbering/wrap/pos from these fields exactly as it
/// did from the old non-zero-offset heuristic.
pub(super) fn offset_placement(x: i32, y: i32) -> Option<ObjectPlacement> {
    if x == 0 && y == 0 {
        return None;
    }
    Some(
        ObjectPlacement::new(
            ObjectTextWrap::InFrontOfText,
            ObjectRelativeTo::Paper,
            ObjectRelativeTo::Paper,
        )
        .with_allow_overlap(true)
        .with_vert_offset(HwpUnit::new(y).unwrap_or(HwpUnit::ZERO))
        .with_horz_offset(HwpUnit::new(x).unwrap_or(HwpUnit::ZERO)),
    )
}

/// Object common property bit 20 (표 70): the size is locked (HWPX
/// `<hp:sz protect>`; HWP5 ↔ Hancom twins 1,806/1,806, §19.23.1).
pub(super) const CTRL_PROPERTY_SIZE_PROTECT_BIT: u32 = 1 << 20;

/// S20: an HWP 5.0 object's original box from its `ShapeComponent` initial
/// size (bytes 20–28, HWPX `orgSz` — twins 1,806/1,806), `None` when it is
/// the displayed size or cannot be one: out of the signed range (reported,
/// §19.23.15), or an axis of 0 under a displayed axis that is not
/// (reported).
/// 이것을 실패시키는 것: 이 변환을 지우기(`top_level_shapes_carry_their_frame`) /
/// 범위 밖·0 축 보고를 지우기(`unusable_initial_size_is_reported`).
pub(super) fn original_box(
    initial: Option<(u32, u32)>,
    displayed: (i32, i32),
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<hwpforge_foundation::Size> {
    framed_initial(initial, displayed, displayed, warnings)
}

/// [`original_box`] against a kind's own default box (`default`): a
/// TextArt's default is its 14173 design box, not its displayed size.
pub(super) fn framed_initial(
    initial: Option<(u32, u32)>,
    displayed: (i32, i32),
    default: (i32, i32),
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<hwpforge_foundation::Size> {
    let (w, h) = initial?;
    let (Ok(ow), Ok(oh)) = (i32::try_from(w), i32::try_from(h)) else {
        super::push_projection_fallback(
            warnings,
            "object.original_size",
            format!("initial size {w}x{h} is out of the signed range; not carried"),
        );
        return None;
    };
    if (ow == 0 && displayed.0 != 0) || (oh == 0 && displayed.1 != 0) {
        super::push_projection_fallback(
            warnings,
            "object.original_size",
            format!(
                "initial size {ow}x{oh} has an axis of 0 under the displayed {}x{}",
                displayed.0, displayed.1
            ),
        );
        return None;
    }
    let (Ok(uw), Ok(uh)) = (HwpUnit::new(ow), HwpUnit::new(oh)) else { return None };
    ((ow, oh) != default).then(|| hwpforge_foundation::Size::new(uw, uh))
}

/// The frame of a top-level shape: [`original_box`] and the size lock.
pub(super) fn object_frame(
    initial: Option<(u32, u32)>,
    displayed: (i32, i32),
    ctrl_properties: u32,
    warnings: &mut Vec<Hwp5Warning>,
) -> hwpforge_core::ObjectFrame {
    let frame = hwpforge_core::ObjectFrame::new()
        .with_size_protect(ctrl_properties & CTRL_PROPERTY_SIZE_PROTECT_BIT != 0);
    match original_box(initial, displayed, warnings) {
        Some(o) => frame.with_original_size(o),
        None => frame,
    }
}

/// The displayed size of a shape from its `CtrlHeader` (the layout size,
/// HWPX `sz`), when both axes fit.
fn displayed(geometry: &Hwp5ShapeComponentGeometry) -> Option<(i32, i32)> {
    let w = i32::try_from(geometry.width).ok()?;
    let h = i32::try_from(geometry.height).ok()?;
    (HwpUnit::new(w).is_ok() && HwpUnit::new(h).is_ok()).then_some((w, h))
}

/// S20 (§19.23.11): a shape whose initial box is known keeps its stored
/// points (they are coordinates in that box) and is shown at its layout size
/// with that box as its original box. `control` holds the wire points; this
/// sets its size and frame. `false` when the initial box is unknown or the
/// layout size does not fit — the caller keeps the pre-S20 projection.
fn frame_wire_shape(
    control: &mut Control,
    geometry: &Hwp5ShapeComponentGeometry,
    initial: Option<(u32, u32)>,
    ctrl_properties: u32,
    warnings: &mut Vec<Hwp5Warning>,
) -> bool {
    let (Some(shown), Some((iw, ih))) = (displayed(geometry), initial) else { return false };
    let frame = object_frame(initial, shown, ctrl_properties, warnings);
    // An initial box that cannot be the frame (reported by `original_box`)
    // leaves the points in a box Core cannot hold: the pre-S20 projection.
    let same = i64::from(iw) == i64::from(shown.0) && i64::from(ih) == i64::from(shown.1);
    if frame.original_size.is_none() && !same {
        return false;
    }
    match control {
        Control::Line { width, height, frame: f, .. }
        | Control::ConnectLine { width, height, frame: f, .. }
        | Control::Polygon { width, height, frame: f, .. }
        | Control::Curve { width, height, frame: f, .. }
        | Control::Ellipse { width, height, frame: f, .. }
        | Control::Arc { width, height, frame: f, .. }
        | Control::Rect { width, height, frame: f, .. } => {
            *width = HwpUnit::new(shown.0).unwrap_or(HwpUnit::ZERO);
            *height = HwpUnit::new(shown.1).unwrap_or(HwpUnit::ZERO);
            *f = frame;
            true
        }
        _ => false,
    }
}

fn wire_point(p: Hwp5ShapePoint) -> hwpforge_core::control::ShapePoint {
    hwpforge_core::control::ShapePoint { x: p.x, y: p.y }
}

/// A line or connector with its stored ends, before [`frame_wire_shape`].
pub(super) fn wire_line(start: Hwp5ShapePoint, end: Hwp5ShapePoint, connect: bool) -> Control {
    let (start, end) = (wire_point(start), wire_point(end));
    if connect {
        Control::ConnectLine {
            start,
            end,
            control_points: Vec::new(),
            connect_type: "STRAIGHT".to_string(),
            width: HwpUnit::ZERO,
            height: HwpUnit::ZERO,
            frame: hwpforge_core::ObjectFrame::new(),
            placement: None,
            caption: None,
            style: None,
        }
    } else {
        Control::Line {
            start,
            end,
            width: HwpUnit::ZERO,
            height: HwpUnit::ZERO,
            frame: hwpforge_core::ObjectFrame::new(),
            placement: None,
            caption: None,
            style: None,
        }
    }
}

pub(super) fn project_line_run(
    line: &Hwp5LineControl,
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<Run> {
    // S20: the stored ends in the initial box (§19.23.11).
    // 이것을 실패시키는 것: 이 갈래를 지우기(끝점이 표시 상자로 맞춰지고 원래 상자를 잃는다).
    let mut control = wire_line(line.start, line.end, false);
    if line.start != line.end
        && frame_wire_shape(
            &mut control,
            &line.geometry,
            line.original_size,
            line.ctrl_properties,
            warnings,
        )
    {
        if let Control::Line { placement, .. } = &mut control {
            *placement = shape_placement(&line.geometry, line.ctrl_properties, warnings);
        }
        return Some(Run::control(control, CharShapeIndex::new(0)));
    }
    let projected_start = scale_point_into_geometry(
        line.start,
        line.start.x.min(line.end.x),
        line.start.x.max(line.end.x),
        line.geometry.width,
        100,
        Axis::Horizontal,
    );
    let projected_end = scale_point_into_geometry(
        line.end,
        line.start.x.min(line.end.x),
        line.start.x.max(line.end.x),
        line.geometry.width,
        100,
        Axis::Horizontal,
    );
    let projected_start_y = scale_point_into_geometry(
        line.start,
        line.start.y.min(line.end.y),
        line.start.y.max(line.end.y),
        line.geometry.height,
        100,
        Axis::Vertical,
    );
    let projected_end_y = scale_point_into_geometry(
        line.end,
        line.start.y.min(line.end.y),
        line.start.y.max(line.end.y),
        line.geometry.height,
        100,
        Axis::Vertical,
    );

    let scaled_start =
        hwpforge_core::control::ShapePoint { x: projected_start, y: projected_start_y };
    let scaled_end = hwpforge_core::control::ShapePoint { x: projected_end, y: projected_end_y };
    let mut control = hwpforge_core::control::Control::line(scaled_start, scaled_end).ok()?;
    if let Control::Line { placement, .. } = &mut control {
        *placement = shape_placement(&line.geometry, line.ctrl_properties, warnings);
    }
    Some(Run::control(control, CharShapeIndex::new(0)))
}

pub(super) fn project_rect_run(
    rect: &Hwp5RectControl,
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<Run> {
    let width = HwpUnit::new(positive_i32_from_u32(rect.geometry.width)?).ok()?;
    let height = HwpUnit::new(positive_i32_from_u32(rect.geometry.height)?).ok()?;
    let mut control = hwpforge_core::control::Control::rect(width, height).ok()?;
    if let Control::Rect { placement, frame, .. } = &mut control {
        *placement = shape_placement(&rect.geometry, rect.ctrl_properties, warnings);
        // 이것을 실패시키는 것: frame 을 두지 않기(`top_level_shapes_carry_their_frame`).
        *frame = object_frame(
            rect.original_size,
            (width.as_i32(), height.as_i32()),
            rect.ctrl_properties,
            warnings,
        );
    }
    Some(Run::control(control, CharShapeIndex::new(0)))
}

pub(super) fn project_polygon_run(
    polygon: &Hwp5PolygonControl,
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<Run> {
    // S20: the stored points in the initial box; without one, fitted into
    // the displayed box as before.
    let wire = hwpforge_core::control::Control::polygon(
        polygon.points.iter().copied().map(wire_point).collect(),
    );
    if let Ok(mut control) = wire {
        if displayed(&polygon.geometry).is_some_and(|(w, h)| w > 0 && h > 0)
            && frame_wire_shape(
                &mut control,
                &polygon.geometry,
                polygon.original_size,
                polygon.ctrl_properties,
                warnings,
            )
        {
            if let Control::Polygon { placement, .. } = &mut control {
                *placement = shape_placement(&polygon.geometry, polygon.ctrl_properties, warnings);
            }
            return Some(Run::control(control, CharShapeIndex::new(0)));
        }
    }
    let vertices = scale_polygon_points(&polygon.points, &polygon.geometry);
    let mut control = hwpforge_core::control::Control::polygon(vertices).ok()?;
    if let Control::Polygon { placement, .. } = &mut control {
        *placement = shape_placement(&polygon.geometry, polygon.ctrl_properties, warnings);
    }
    Some(Run::control(control, CharShapeIndex::new(0)))
}

/// Project a plain ellipse. Center/axes are derived from the bounding box
/// (`Control::ellipse`), which matches how a HWP5 plain ellipse is defined.
pub(super) fn project_ellipse_run(
    ellipse: &Hwp5EllipseControl,
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<Run> {
    let width = HwpUnit::new(positive_i32_from_u32(ellipse.geometry.width)?).ok()?;
    let height = HwpUnit::new(positive_i32_from_u32(ellipse.geometry.height)?).ok()?;
    let mut control = hwpforge_core::control::Control::ellipse(width, height);
    // S20: with a known initial box the stored centre and axes are kept
    // (they are in that box — the ministry's bullet: centre 280 in 560).
    let mut framed = control.clone();
    if let Control::Ellipse { center, axis1, axis2, .. } = &mut framed {
        *center = wire_point(ellipse.shape.center);
        *axis1 = wire_point(ellipse.shape.axis1);
        *axis2 = wire_point(ellipse.shape.axis2);
    }
    if frame_wire_shape(
        &mut framed,
        &ellipse.geometry,
        ellipse.original_size,
        ellipse.ctrl_properties,
        warnings,
    ) {
        control = framed;
    }
    if let Control::Ellipse { placement, .. } = &mut control {
        *placement = shape_placement(&ellipse.geometry, ellipse.ctrl_properties, warnings);
    }
    Some(Run::control(control, CharShapeIndex::new(0)))
}

pub(super) fn restore_wire_points(wire: &crate::decoder::section::Hwp5Control, core: &mut Control) {
    use crate::decoder::section::Hwp5Control;
    let p = |v: crate::schema::section::Hwp5ShapePoint| hwpforge_core::control::ShapePoint {
        x: v.x,
        y: v.y,
    };
    match (wire, core) {
        (Hwp5Control::Line(w), Control::Line { start, end, .. }) => {
            *start = p(w.start);
            *end = p(w.end);
        }
        (Hwp5Control::ConnectLine(w), Control::ConnectLine { start, end, .. }) => {
            *start = p(w.start);
            *end = p(w.end);
        }
        (Hwp5Control::Polygon(w), Control::Polygon { vertices, .. }) => {
            *vertices = w.points.iter().copied().map(p).collect();
        }
        (Hwp5Control::Curve(w), Control::Curve { points, .. }) => {
            *points = w.points.iter().copied().map(p).collect();
        }
        (Hwp5Control::Ellipse(w), Control::Ellipse { center, axis1, axis2, .. }) => {
            *center = p(w.shape.center);
            *axis1 = p(w.shape.axis1);
            *axis2 = p(w.shape.axis2);
        }
        (Hwp5Control::Arc(w), core @ Control::Arc { .. }) => restore_arc_points(&w.shape, core),
        _ => {}
    }
}

/// Build only arc semantics; placement remains the caller's responsibility.
pub(super) fn wire_arc(
    shape: &crate::schema::section::Hwp5ShapeComponentEllipse,
    width: HwpUnit,
    height: HwpUnit,
) -> Result<Control, String> {
    use hwpforge_core::control::ArcDefinition;
    let (definition, arc_type) = match shape.native_type {
        Some(raw) => (
            ArcDefinition::AxisEndpoints,
            ArcType::try_from(raw).map_err(|_| {
                format!("native arc type {raw} unsupported (expected 0–2); arc omitted")
            })?,
        ),
        // Ellipse property closure semantics are not inferred from native values.
        None => (ArcDefinition::EllipseSweep, ArcType::Normal),
    };
    let mut control = Control::arc(arc_type, width, height);
    if let Control::Arc { definition: d, .. } = &mut control {
        *d = definition;
    }
    restore_arc_points(shape, &mut control);
    Ok(control)
}

fn restore_arc_points(
    shape: &crate::schema::section::Hwp5ShapeComponentEllipse,
    core: &mut Control,
) {
    if let Control::Arc { center, axis1, axis2, start1, end1, start2, end2, .. } = core {
        *center = wire_point(shape.center);
        *axis1 = wire_point(shape.axis1);
        *axis2 = wire_point(shape.axis2);
        *start1 = wire_point(shape.start1);
        *end1 = wire_point(shape.end1);
        *start2 = wire_point(shape.start2);
        *end2 = wire_point(shape.end2);
    }
}

/// Project native and ellipse-derived arcs using their stored definition points.
pub(super) fn project_arc_run(
    arc: &Hwp5ArcControl,
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<Run> {
    let width = HwpUnit::new(positive_i32_from_u32(arc.geometry.width)?).ok()?;
    let height = HwpUnit::new(positive_i32_from_u32(arc.geometry.height)?).ok()?;
    let mut control = wire_arc(&arc.shape, width, height)
        .map_err(|reason| {
            super::push_projection_fallback(warnings, "shape.arc_type", reason);
        })
        .ok()?;
    let mut framed = control.clone();
    if frame_wire_shape(
        &mut framed,
        &arc.geometry,
        arc.original_size,
        arc.ctrl_properties,
        warnings,
    ) {
        control = framed;
    } else if arc.shape.native_type.is_none() {
        // Preserve the existing ellipse-sweep fallback when its source box
        // cannot be represented. Native axis points are never invented.
        control = Control::arc(ArcType::Normal, width, height);
    }
    if let Control::Arc { placement, .. } = &mut control {
        *placement = shape_placement(&arc.geometry, arc.ctrl_properties, warnings);
    }
    Some(Run::control(control, CharShapeIndex::new(0)))
}

/// Project a curve, scaling its control points into the bounding box like a
/// polygon and mapping the decoded per-segment type bytes onto the Core enum.
pub(super) fn project_curve_run(
    curve: &Hwp5CurveControl,
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<Run> {
    let mut wired = hwpforge_core::control::Control::curve(
        curve.points.iter().copied().map(wire_point).collect(),
    )
    .ok();
    let framed = wired.as_mut().is_some_and(|c| {
        frame_wire_shape(c, &curve.geometry, curve.original_size, curve.ctrl_properties, warnings)
    });
    let mut control = match wired {
        Some(c) if framed => c,
        _ => {
            let vertices = scale_polygon_points(&curve.points, &curve.geometry);
            hwpforge_core::control::Control::curve(vertices).ok()?
        }
    };
    if let Control::Curve { placement, segment_types, .. } = &mut control {
        *placement = shape_placement(&curve.geometry, curve.ctrl_properties, warnings);
        let decoded: Vec<CurveSegmentType> = curve
            .segment_types
            .iter()
            .map(|byte| match byte {
                0 => CurveSegmentType::Line,
                _ => CurveSegmentType::Curve,
            })
            .collect();
        if !decoded.is_empty() {
            *segment_types = decoded;
        }
    }
    Some(Run::control(control, CharShapeIndex::new(0)))
}

/// Project a connect line. 한컴 stores it in the same `ShapeComponentLine`
/// record as a plain line, so endpoints are scaled into the bounding box the
/// same way; only a straight connector is carried (the source object-link
/// references have no `<hp:connectLine>` representation).
pub(super) fn project_connectline_run(
    connect_line: &Hwp5ConnectLineControl,
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<Run> {
    let mut control = wire_line(connect_line.start, connect_line.end, true);
    if connect_line.start != connect_line.end
        && frame_wire_shape(
            &mut control,
            &connect_line.geometry,
            connect_line.original_size,
            connect_line.ctrl_properties,
            warnings,
        )
    {
        if let Control::ConnectLine { placement, .. } = &mut control {
            *placement =
                shape_placement(&connect_line.geometry, connect_line.ctrl_properties, warnings);
        }
        return Some(Run::control(control, CharShapeIndex::new(0)));
    }
    let min_x = connect_line.start.x.min(connect_line.end.x);
    let max_x = connect_line.start.x.max(connect_line.end.x);
    let min_y = connect_line.start.y.min(connect_line.end.y);
    let max_y = connect_line.start.y.max(connect_line.end.y);
    let scaled_start = hwpforge_core::control::ShapePoint {
        x: scale_point_into_geometry(
            connect_line.start,
            min_x,
            max_x,
            connect_line.geometry.width,
            100,
            Axis::Horizontal,
        ),
        y: scale_point_into_geometry(
            connect_line.start,
            min_y,
            max_y,
            connect_line.geometry.height,
            100,
            Axis::Vertical,
        ),
    };
    let scaled_end = hwpforge_core::control::ShapePoint {
        x: scale_point_into_geometry(
            connect_line.end,
            min_x,
            max_x,
            connect_line.geometry.width,
            100,
            Axis::Horizontal,
        ),
        y: scale_point_into_geometry(
            connect_line.end,
            min_y,
            max_y,
            connect_line.geometry.height,
            100,
            Axis::Vertical,
        ),
    };
    let mut control =
        hwpforge_core::control::Control::connect_line(scaled_start, scaled_end).ok()?;
    if let Control::ConnectLine { placement, .. } = &mut control {
        *placement =
            shape_placement(&connect_line.geometry, connect_line.ctrl_properties, warnings);
    }
    Some(Run::control(control, CharShapeIndex::new(0)))
}

#[derive(Debug, Clone, Copy)]
enum Axis {
    Horizontal,
    Vertical,
}

fn scale_polygon_points(
    points: &[Hwp5ShapePoint],
    geometry: &Hwp5ShapeComponentGeometry,
) -> Vec<hwpforge_core::control::ShapePoint> {
    let min_x = points.iter().map(|point| point.x).min().unwrap_or(0);
    let max_x = points.iter().map(|point| point.x).max().unwrap_or(0);
    let min_y = points.iter().map(|point| point.y).min().unwrap_or(0);
    let max_y = points.iter().map(|point| point.y).max().unwrap_or(0);

    points
        .iter()
        .map(|point| hwpforge_core::control::ShapePoint {
            x: scale_point_into_geometry(*point, min_x, max_x, geometry.width, 1, Axis::Horizontal),
            y: scale_point_into_geometry(*point, min_y, max_y, geometry.height, 1, Axis::Vertical),
        })
        .collect()
}

fn scale_point_into_geometry(
    point: Hwp5ShapePoint,
    raw_min: i32,
    raw_max: i32,
    geometry_span: u32,
    minimum_target_span: i32,
    axis: Axis,
) -> i32 {
    let raw_span = i64::from(raw_max) - i64::from(raw_min);
    let target_span =
        i64::from(i32::try_from(geometry_span).unwrap_or(i32::MAX).max(minimum_target_span));
    if raw_span <= 0 {
        return 0;
    }

    let raw_value = match axis {
        Axis::Horizontal => point.x,
        Axis::Vertical => point.y,
    };
    let relative = i64::from(raw_value) - i64::from(raw_min);
    let scaled = (relative * target_span + (raw_span / 2)) / raw_span;
    i32::try_from(scaled).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::section::Hwp5EllipseControl;

    fn zero_geometry() -> crate::schema::section::Hwp5ShapeComponentGeometry {
        crate::schema::section::Hwp5ShapeComponentGeometry {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            outer_margin: [0; 4],
        }
    }

    fn shape_point(x: i32, y: i32) -> crate::schema::section::Hwp5ShapePoint {
        crate::schema::section::Hwp5ShapePoint { x, y }
    }

    fn r6_ellipse(original: Option<(u32, u32)>, props: u32) -> Hwp5EllipseControl {
        let pt = shape_point;
        Hwp5EllipseControl {
            standalone_render: None,
            original_size: original,
            look: None,
            ctrl_id: 0,
            paragraphs: Vec::new(),
            list_header_properties: None,
            text_margin: None,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 850,
                height: 850,
                outer_margin: [0; 4],
            },
            ctrl_properties: props | 1,
            shape: crate::schema::section::Hwp5ShapeComponentEllipse {
                native_type: None,
                property: 0,
                center: pt(280, 280),
                axis1: pt(280, 0),
                axis2: pt(560, 280),
                start1: pt(0, 0),
                end1: pt(0, 0),
                start2: pt(0, 0),
                end2: pt(0, 0),
            },
        }
    }

    fn control_of(run: Option<Run>) -> Control {
        match run.expect("run").content {
            hwpforge_core::run::RunContent::Control(c) => *c,
            other => panic!("{other:?}"),
        }
    }

    /// S20 (§19.23.11, FR-T1): the ministry's bullet stored in HWP 5.0 —
    /// initial box 560, shown 850, size locked — keeps its stored centre
    /// 280 in its original box 560 instead of a centre rebuilt in the shown
    /// box. A line keeps its stored ends with its initial box; a rect its box.
    /// 이것을 실패시키는 것: 초기 크기를 frame 으로 옮기지 않기 / 저장 정의점 대신
    /// 상자에서 다시 만들기 / bit 20 대신 19 를 읽기.
    #[test]
    fn top_level_shapes_carry_their_frame() {
        let mut w = Vec::new();
        let c = control_of(project_ellipse_run(&r6_ellipse(Some((560, 560)), 1 << 20), &mut w));
        assert!(w.is_empty(), "{w:?}");
        let Control::Ellipse { center, axis1, width, frame, .. } = &c else { panic!("{c:?}") };
        assert_eq!((center.x, center.y, axis1.x, axis1.y), (280, 280, 280, 0));
        assert_eq!(width.as_i32(), 850);
        let u = |v| HwpUnit::new(v).unwrap();
        assert_eq!(frame.original_size, Some(hwpforge_foundation::Size::new(u(560), u(560))));
        assert!(frame.size_protect);
        let c = control_of(project_ellipse_run(&r6_ellipse(Some((560, 560)), 1 << 19), &mut w));
        assert!(!c.frame().unwrap().size_protect);

        let line = crate::decoder::section::Hwp5LineControl {
            standalone_render: None,
            original_size: Some((1000, 0)),
            look: None,
            ctrl_id: 0,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 2000,
                height: 0,
                outer_margin: [0; 4],
            },
            ctrl_properties: 1,
            start: shape_point(0, 0),
            end: shape_point(1000, 0),
        };
        let c = control_of(project_line_run(&line, &mut w));
        let Control::Line { end, width, height, frame, .. } = &c else { panic!() };
        assert_eq!((end.x, width.as_i32(), height.as_i32()), (1000, 2000, 0));
        assert_eq!(frame.original_size, Some(hwpforge_foundation::Size::new(u(1000), u(0))));

        let rect = crate::decoder::section::Hwp5RectControl {
            standalone_render: None,
            original_size: Some((100, 50)),
            look: None,
            ctrl_id: 0,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
                outer_margin: [0; 4],
            },
            ctrl_properties: 1,
        };
        let c = control_of(project_rect_run(&rect, &mut w));
        assert_eq!(
            c.frame().unwrap().original_size,
            Some(hwpforge_foundation::Size::new(u(100), u(50)))
        );
    }

    /// An initial size that cannot be a frame value is reported and the shape
    /// projected as before (centre rebuilt in the shown box).
    /// 이것을 실패시키는 것: 범위 밖·0 축 보고를 지우기 / 쓸 수 없는 초기 상자로도
    /// 저장 점을 쓰기.
    #[test]
    fn unusable_initial_size_is_reported() {
        for original in [(u32::MAX, 560), (0, 560)] {
            let mut w = Vec::new();
            let c = control_of(project_ellipse_run(&r6_ellipse(Some(original), 0), &mut w));
            assert_eq!(w.len(), 1, "{w:?}");
            let Control::Ellipse { center, frame, .. } = &c else { panic!() };
            assert_eq!(center.x, 425);
            assert!(frame.original_size.is_none());
        }
    }

    #[test]
    fn project_line_run_returns_none_for_degenerate_start_equals_end() {
        // A degenerate line (start == end) cannot form a valid Core line; the
        // projection must return None instead of panicking via .expect(). The
        // single point makes the scaled start/end coincide so
        // `Control::line` rejects it.
        let line = crate::decoder::section::Hwp5LineControl {
            standalone_render: None,
            original_size: None,
            look: None,
            ctrl_id: 0,
            geometry: zero_geometry(),
            ctrl_properties: 0,
            start: shape_point(50, 50),
            end: shape_point(50, 50),
        };
        assert!(
            project_line_run(&line, &mut Vec::new()).is_none(),
            "degenerate line (start == end) must project to None, not panic",
        );
    }

    #[test]
    fn project_line_run_returns_some_for_valid_line() {
        // Regression: a non-degenerate line still projects successfully.
        let line = crate::decoder::section::Hwp5LineControl {
            standalone_render: None,
            original_size: None,
            look: None,
            ctrl_id: 0,
            geometry: zero_geometry(),
            ctrl_properties: 0,
            start: shape_point(0, 0),
            end: shape_point(100, 100),
        };
        assert!(
            project_line_run(&line, &mut Vec::new()).is_some(),
            "valid line must still project to Some"
        );
    }

    #[test]
    fn project_polygon_run_returns_none_for_too_few_vertices() {
        // A polygon needs >= 3 vertices. With 2 points `Control::polygon`
        // rejects the shape, so the projection must return None rather than
        // panicking via .expect().
        let polygon = crate::decoder::section::Hwp5PolygonControl {
            standalone_render: None,
            original_size: None,
            look: None,
            ctrl_id: 0,
            paragraphs: Vec::new(),
            list_header_properties: None,
            text_margin: None,
            geometry: zero_geometry(),
            ctrl_properties: 0,
            points: vec![shape_point(0, 0), shape_point(100, 0)],
        };
        assert!(
            project_polygon_run(&polygon, &mut Vec::new()).is_none(),
            "polygon with < 3 vertices must project to None, not panic",
        );
    }

    #[test]
    fn project_polygon_run_returns_some_for_valid_polygon() {
        // Regression: a valid 3-vertex polygon still projects successfully.
        let polygon = crate::decoder::section::Hwp5PolygonControl {
            standalone_render: None,
            original_size: None,
            look: None,
            ctrl_id: 0,
            paragraphs: Vec::new(),
            list_header_properties: None,
            text_margin: None,
            geometry: zero_geometry(),
            ctrl_properties: 0,
            points: vec![shape_point(0, 0), shape_point(100, 0), shape_point(50, 100)],
        };
        assert!(
            project_polygon_run(&polygon, &mut Vec::new()).is_some(),
            "valid polygon must still project to Some"
        );
    }

    // ── placement derivation (W4 w1) ─────────────────────────────────

    #[test]
    fn shape_placement_collapses_inline_and_byte_grounds_floating_axes() {
        let geometry = crate::schema::section::Hwp5ShapeComponentGeometry {
            outer_margin: [0; 4],
            x: 1_234,
            y: 5_678,
            width: 8_000,
            height: 6_000,
        };
        // bit0=1 (글자처럼 취급) → inline default → None (offset intentionally
        // dropped for an inline shape).
        assert!(shape_placement(&geometry, 0x1, &mut Vec::new()).is_none());
        // bit0=0, 나머지 축 비트 전부 0 → 부유 배치가 Paper/Paper/Square 로
        // 디코드되고 CtrlHeader 오프셋을 싣는다 (W5 w0 바이트-그라운드).
        let zeroed = shape_placement(&geometry, 0x0, &mut Vec::new()).expect("floating placement");
        assert!(!zeroed.treat_as_char);
        assert_eq!(zeroed.horz_offset.as_i32(), 1_234);
        assert_eq!(zeroed.vert_offset.as_i32(), 5_678);
        assert_eq!(zeroed.vert_rel_to, ObjectRelativeTo::Paper);
        assert_eq!(zeroed.horz_rel_to, ObjectRelativeTo::Paper);
        assert_eq!(zeroed.text_wrap, ObjectTextWrap::Square);
        assert!(!zeroed.flow_with_text);
        assert!(!zeroed.allow_overlap);
        // 실측 속성 word (textbox_anchored native fixture, attr=0x040a4110) →
        // 축이 이제 byte-ground: PARA/PAGE/SQUARE, overlap=1, flow=0.
        let real =
            shape_placement(&geometry, 0x040a_4110, &mut Vec::new()).expect("floating placement");
        assert_eq!(real.vert_rel_to, ObjectRelativeTo::Para);
        assert_eq!(real.horz_rel_to, ObjectRelativeTo::Page);
        assert_eq!(real.text_wrap, ObjectTextWrap::Square);
        assert!(!real.flow_with_text);
        assert!(real.allow_overlap);
        // bit0 이 여전히 inline/None collapse 를 지배한다 (다른 비트 무관).
        assert!(shape_placement(&geometry, 0xFFFF_FFFE, &mut Vec::new()).is_some());
        assert!(shape_placement(&geometry, 0xFFFF_FFFF, &mut Vec::new()).is_none());
    }

    #[test]
    fn offset_placement_floats_only_on_non_zero_offset() {
        // The bit0-less fallback (group container / chart / textart): a zero
        // offset stays inline (None), any non-zero offset floats.
        assert!(offset_placement(0, 0).is_none());
        let placement = offset_placement(1_000, 0).expect("non-zero floats");
        assert!(!placement.treat_as_char);
        assert_eq!(placement.horz_offset.as_i32(), 1_000);
        assert_eq!(placement.text_wrap, ObjectTextWrap::InFrontOfText);
    }
}
