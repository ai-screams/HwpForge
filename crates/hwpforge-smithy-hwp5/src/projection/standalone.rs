//! Independent GSO transforms join the already projected object exactly once.
use crate::decoder::{section::Hwp5Control, Hwp5Warning};
use hwpforge_core::control::{
    geometry::{fold_standalone, fold_standalone_picture, FoldInput},
    ShapePoint,
};
use hwpforge_core::{Control, RunContent};
use hwpforge_foundation::HwpUnit;

pub(super) fn settle(
    wire: &Hwp5Control,
    run: &mut hwpforge_core::Run,
    warnings: &mut Vec<Hwp5Warning>,
) {
    let (render, geometry) = match wire {
        Hwp5Control::TextBox(s) => (s.standalone_render.as_ref(), &s.geometry),
        Hwp5Control::Rect(s) => (s.standalone_render.as_ref(), &s.geometry),
        Hwp5Control::Ellipse(s) => (s.standalone_render.as_ref(), &s.geometry),
        Hwp5Control::Arc(s) => (s.standalone_render.as_ref(), &s.geometry),
        Hwp5Control::Polygon(s) => (s.standalone_render.as_ref(), &s.geometry),
        Hwp5Control::Curve(s) => (s.standalone_render.as_ref(), &s.geometry),
        Hwp5Control::Line(s) => (s.standalone_render.as_ref(), &s.geometry),
        Hwp5Control::ConnectLine(s) => (s.standalone_render.as_ref(), &s.geometry),
        Hwp5Control::Image(s) => (s.standalone_render.as_ref(), &s.geometry),
        _ => return,
    };
    let Some(render) = render else { return }; // group leaves never carry this slot
    let mut lost = |reason: String| {
        warnings.push(Hwp5Warning::ContentLost {
            kind: "shape_transform".into(),
            reason,
            location: format!("standalone at ({},{})", geometry.x, geometry.y),
        })
    };
    let render = match render {
        Ok(r) => r,
        Err(e) => {
            lost(format!("matrix chain: {e}"));
            return;
        }
    };
    let map = super::group::chain(render);
    let m = map.matrix();
    if m[1].abs() < 1e-9 && m[3].abs() < 1e-9 && m[0] >= 0.0 && m[4] >= 0.0 {
        if render.angle.rem_euclid(360) != 0 || render.flip.0 || render.flip.1 {
            let kind = match &run.content {
                RunContent::Control(c) => c.kind_name(),
                RunContent::Image(_) => "picture",
                _ => return,
            };
            lost(format!(
                "metadata mismatch; {kind} id={}; source angle={}, flip=({},{}); composite matrix has no rotation or reflection; discarded rotation/flip metadata",
                render.source_id, render.angle, u32::from(render.flip.0), u32::from(render.flip.1)
            ));
        }
        return;
    }
    let original = (f64::from(render.initial.0), f64::from(render.initial.1));
    let input = FoldInput::new(map, original, (0.0, 0.0));
    let mut margin = [0; 4];
    let result = match &mut run.content {
        RunContent::Control(c) => {
            // Restore wire points before folding: a legacy fallback may have
            // fitted them to sz. Never transform those fitted points again.
            let saved = hwpforge_core::control::geometry::defining_points(c);
            super::shapes::restore_wire_points(wire, c);
            let result = fold_standalone(c, &input);
            if result.is_err() {
                replace_points(c, saved);
            }
            result
        }
        RunContent::Image(image) => {
            if let Hwp5Control::Image(w) = wire {
                margin = w.picture.in_margin;
                let (ow, oh) = (original.0 as i32, original.1 as i32);
                if w.picture.rectangle != [(0, 0), (ow, 0), (ow, oh), (0, oh)] {
                    lost(format!(
                        "picture id={}: nonrectangular imgRect {:?}",
                        w.instance_id, w.picture.rectangle
                    ));
                    return;
                }
            }
            if margin[0] != margin[1] || margin[2] != margin[3] {
                lost(format!("picture: asymmetric inner margin {margin:?} cannot be represented"));
                return;
            }
            let result = fold_standalone_picture(image, &input);
            if result.is_ok() {
                if let Hwp5Control::Image(w) = wire {
                    if let Some((w, h)) = w.picture.dimension {
                        if let (Ok(w), Ok(h)) = (HwpUnit::new(w as i32), HwpUnit::new(h as i32)) {
                            image.image_extent = Some((w, h));
                        }
                    }
                }
            }
            if result.is_ok() && margin != [0; 4] {
                let u = |v| HwpUnit::new(i32::from(v)).expect("u16 margin fits HWPUNIT");
                image.in_margin = Some(hwpforge_core::table::TableMargin {
                    left: u(margin[0]),
                    right: u(margin[1]),
                    top: u(margin[2]),
                    bottom: u(margin[3]),
                });
            }
            result
        }
        _ => return,
    };
    if let Err(e) = result {
        lost(e.to_string());
        return;
    }
    let bounds = [(0.0, 0.0), (original.0, 0.0), original, (0.0, original.1)]
        .map(|p| map.apply(p))
        .into_iter()
        .fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |b, p| {
            (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1))
        });
    let declared = (
        f64::from(geometry.width) - f64::from(margin[0]) - f64::from(margin[1]),
        f64::from(geometry.height) - f64::from(margin[2]) - f64::from(margin[3]),
    );
    let error = bounds
        .0
        .abs()
        .max(bounds.1.abs())
        .max((bounds.2 - declared.0).abs())
        .max((bounds.3 - declared.1).abs());
    if error > 2.5 {
        use hwpforge_core::control::geometry::standalone_layout;
        let (size, rotation, flip, kind) = match &run.content {
            RunContent::Image(i) => {
                ((i.width.as_i32(), i.height.as_i32()), i.rotation, i.flip, "picture")
            }
            RunContent::Control(c) => {
                let (w, h, style) = match c.as_ref() {
                    Control::TextBox { width, height, style, .. }
                    | Control::Rect { width, height, style, .. }
                    | Control::Ellipse { width, height, style, .. }
                    | Control::Arc { width, height, style, .. }
                    | Control::Polygon { width, height, style, .. }
                    | Control::Curve { width, height, style, .. }
                    | Control::Line { width, height, style, .. }
                    | Control::ConnectLine { width, height, style, .. } => (*width, *height, style),
                    _ => return,
                };
                (
                    (w.as_i32(), h.as_i32()),
                    style.as_ref().and_then(|s| s.rotation),
                    style.as_ref().and_then(|s| s.flip),
                    c.kind_name(),
                )
            }
            _ => return,
        };
        let (_, layout, translation) = standalone_layout(size, rotation, flip);
        let output = (
            i64::from(layout.0) + i64::from(margin[0]) + i64::from(margin[1]),
            i64::from(layout.1) + i64::from(margin[2]) + i64::from(margin[3]),
        );
        lost(format!("G7; {kind} id={}; source sz={}x{}, composite bounds={bounds:?}, max boundary error={error}; recomputed sz={output:?}, transMatrix=({},{}); normalized source matrix placement mismatch",render.source_id,geometry.width,geometry.height,translation.0,translation.1));
    }
}

// Restore only the object's own points; never clone its caption or text tree.
fn replace_points(core: &mut Control, points: Vec<ShapePoint>) {
    let slots: Vec<&mut ShapePoint> = match core {
        Control::Line { start, end, .. } | Control::ConnectLine { start, end, .. } => {
            vec![start, end]
        }
        Control::Polygon { vertices, .. } => {
            *vertices = points;
            return;
        }
        Control::Curve { points: current, .. } => {
            *current = points;
            return;
        }
        Control::Ellipse { center, axis1, axis2, .. } => vec![center, axis1, axis2],
        Control::Arc {
            definition: hwpforge_core::control::ArcDefinition::AxisEndpoints,
            center,
            axis1,
            axis2,
            ..
        } => {
            vec![center, axis1, axis2]
        }
        Control::Arc { center, axis1, axis2, start1, end1, start2, end2, .. } => {
            vec![center, axis1, axis2, start1, end1, start2, end2]
        }
        _ => return,
    };
    for (slot, point) in slots.into_iter().zip(points) {
        *slot = point;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::section::Hwp5LineControl;
    use crate::schema::section::{
        Hwp5ShapeComponentGeometry, Hwp5ShapeFill, Hwp5ShapePoint, Hwp5ShapeRender,
    };

    /// 이것을 실패시키는 것: 원시 점 복원 후 fold 실패에서 fallback 점을 되돌리지 않기.
    #[test]
    fn failed_fold_preserves_projected_fallback_points() {
        let line = Hwp5LineControl {
            standalone_render: Some(Ok(Hwp5ShapeRender {
                source_id: 17,
                angle: 0,
                flip: (false, false),
                initial: (200_000_000, 200_000_000),
                trans: [1., 0., 0., 0., 1., 0.],
                pairs: vec![([1., 0., 0., 0., 1., 0.], [0., -1., 100., 1., 0., 0.])],
                fill: Hwp5ShapeFill::Plain,
            })),
            ctrl_id: 0,
            geometry: Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
                outer_margin: [0; 4],
            },
            start: Hwp5ShapePoint { x: 0, y: 0 },
            end: Hwp5ShapePoint { x: 1000, y: 1000 },
            ctrl_properties: 0,
            look: None,
            original_size: Some((200_000_000, 200_000_000)),
        };
        let mut warnings = vec![];
        let mut run = super::super::shapes::project_line_run(&line, &mut warnings).unwrap();
        let before = run.clone();
        warnings.clear();
        settle(&Hwp5Control::Line(line), &mut run, &mut warnings);
        assert_eq!(run, before);
        assert_eq!(warnings.len(), 1);
        assert!(format!("{:?}", warnings[0]).contains("shape_transform"));
        let RunContent::Control(c) = run.content else { panic!("line") };
        assert_eq!(
            hwpforge_core::control::geometry::defining_points(&c),
            vec![ShapePoint::new(0, 0), ShapePoint::new(100, 100)]
        );
    }

    /// 이것을 실패시키는 것: HWP5 무회전 합성의 angle/flip 불일치를 무음 처리.
    #[test]
    fn unturned_metadata_is_reported_without_changing_projection() {
        let line = Hwp5LineControl {
            standalone_render: Some(Ok(Hwp5ShapeRender {
                source_id: 17,
                angle: 30,
                flip: (true, false),
                initial: (100, 100),
                trans: [1., 0., 0., 0., 1., 0.],
                pairs: vec![([1., 0., 0., 0., 1., 0.], [1., 0., 0., 0., 1., 0.])],
                fill: Hwp5ShapeFill::Plain,
            })),
            ctrl_id: 0,
            geometry: Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
                outer_margin: [0; 4],
            },
            start: Hwp5ShapePoint { x: 0, y: 0 },
            end: Hwp5ShapePoint { x: 100, y: 100 },
            ctrl_properties: 0,
            look: None,
            original_size: Some((100, 100)),
        };
        let mut warnings = vec![];
        let mut run = super::super::shapes::project_line_run(&line, &mut warnings).unwrap();
        let before = run.clone();
        warnings.clear();
        settle(&Hwp5Control::Line(line), &mut run, &mut warnings);
        assert_eq!(run, before);
        assert_eq!(warnings.len(), 1);
        let message = format!("{:?}", warnings[0]);
        assert!(message.contains("shape_transform"));
        assert!(message.contains("metadata mismatch; line id=17; source angle=30, flip=(1,0)"));
        assert!(message.contains("composite matrix has no rotation or reflection"));
        assert!(message.contains("discarded rotation/flip metadata"));
    }

    /// 이것을 실패시키는 것: HWP5 G7 판정/진단에서 안쪽 여백을 빼거나 두 번 가산.
    #[test]
    fn picture_g7_uses_content_bounds_but_reports_declared_and_output_sizes() {
        use crate::decoder::section::Hwp5ImageControl;
        use crate::schema::section::Hwp5ShapePicture;
        for shift in [0., 3000.] {
            let wire = Hwp5Control::Image(Hwp5ImageControl {
                standalone_render: Some(Ok(Hwp5ShapeRender {
                    source_id: 19,
                    angle: 90,
                    flip: (false, false),
                    initial: (100, 50),
                    trans: [1., 0., shift, 0., 1., 0.],
                    pairs: vec![([1., 0., 0., 0., 1., 0.], [0., -1., 50., 1., 0., 0.])],
                    fill: Hwp5ShapeFill::Plain,
                })),
                ctrl_id: 0,
                geometry: Hwp5ShapeComponentGeometry {
                    x: 0,
                    y: 0,
                    width: 70,
                    height: 120,
                    outer_margin: [0; 4],
                },
                binary_data_id: 1,
                instance_id: 19,
                ctrl_properties: 0,
                picture: Hwp5ShapePicture {
                    rectangle: [(0, 0), (100, 0), (100, 50), (0, 50)],
                    in_margin: [10; 4],
                    ..Default::default()
                },
                original_size: Some((100, 50)),
                link: None,
            });
            let mut run = hwpforge_core::Run::image(
                hwpforge_core::image::Image::from_path(
                    "x.png",
                    HwpUnit::new(70).unwrap(),
                    HwpUnit::new(120).unwrap(),
                ),
                hwpforge_foundation::CharShapeIndex::new(0),
            );
            let mut warnings = vec![];
            settle(&wire, &mut run, &mut warnings);
            if shift == 0. {
                assert!(warnings.is_empty(), "{warnings:?}");
            } else {
                assert_eq!(warnings.len(), 1);
                let message = format!("{:?}", warnings[0]);
                assert!(message.contains("G7; picture id=19; source sz=70x120"));
                assert!(message.contains("recomputed sz=(70, 120)"));
                assert!(message.contains("transMatrix=(-25,25)"), "{message}");
            }
        }
    }
}
