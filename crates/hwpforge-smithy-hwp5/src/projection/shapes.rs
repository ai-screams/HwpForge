//! GSO shape projection helpers for HWP5 → Core projection.
//!
//! This submodule holds the leaf builders that turn decoded HWP5 GSO shape
//! controls (line, rect, polygon, ellipse, arc, curve, connect line) into
//! Core `Control` runs, together with the point-scaling helpers that map raw
//! shape coordinates into the owning bounding box. They carry no
//! document-traversal state; the parent `projection` module dispatches each
//! GSO control into the matching builder here.

use hwpforge_core::placement::{ObjectPlacement, ObjectRelativeTo, ObjectTextFlow, ObjectTextWrap};
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

pub(super) fn project_line_run(
    line: &Hwp5LineControl,
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<Run> {
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
    if let Control::Rect { placement, .. } = &mut control {
        *placement = shape_placement(&rect.geometry, rect.ctrl_properties, warnings);
    }
    Some(Run::control(control, CharShapeIndex::new(0)))
}

pub(super) fn project_polygon_run(
    polygon: &Hwp5PolygonControl,
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<Run> {
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
    if let Control::Ellipse { placement, .. } = &mut control {
        *placement = shape_placement(&ellipse.geometry, ellipse.ctrl_properties, warnings);
    }
    Some(Run::control(control, CharShapeIndex::new(0)))
}

/// Project an arc. 한컴 stores arcs inside the ellipse (`0x50`) record; we have
/// verified the `Normal` open-arc shape end to end. Pie/chord arc types and
/// exact arc-sweep endpoints are a future refinement that needs dedicated
/// fixtures, so we carry a `Normal` arc sized from the bounding box rather than
/// guess a sweep we cannot yet validate.
pub(super) fn project_arc_run(
    arc: &Hwp5ArcControl,
    warnings: &mut Vec<Hwp5Warning>,
) -> Option<Run> {
    let width = HwpUnit::new(positive_i32_from_u32(arc.geometry.width)?).ok()?;
    let height = HwpUnit::new(positive_i32_from_u32(arc.geometry.height)?).ok()?;
    let mut control = hwpforge_core::control::Control::arc(ArcType::Normal, width, height);
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
    let vertices = scale_polygon_points(&curve.points, &curve.geometry);
    let mut control = hwpforge_core::control::Control::curve(vertices).ok()?;
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

    fn zero_geometry() -> crate::schema::section::Hwp5ShapeComponentGeometry {
        crate::schema::section::Hwp5ShapeComponentGeometry { x: 0, y: 0, width: 100, height: 100 }
    }

    fn shape_point(x: i32, y: i32) -> crate::schema::section::Hwp5ShapePoint {
        crate::schema::section::Hwp5ShapePoint { x, y }
    }

    #[test]
    fn project_line_run_returns_none_for_degenerate_start_equals_end() {
        // A degenerate line (start == end) cannot form a valid Core line; the
        // projection must return None instead of panicking via .expect(). The
        // single point makes the scaled start/end coincide so
        // `Control::line` rejects it.
        let line = crate::decoder::section::Hwp5LineControl {
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
            ctrl_id: 0,
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
            ctrl_id: 0,
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
