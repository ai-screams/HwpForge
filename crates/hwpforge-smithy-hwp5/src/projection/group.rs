//! Group (묶음 객체) projection: folds each child's matrix chain into exact
//! Core geometry with the shared Core definition
//! ([`fold_group_child`](hwpforge_core::control::geometry::fold_group_child)).
//!
//! A child's `ShapeComponent` stores, like HWPX `<hp:renderingInfo>`, a
//! `trans` matrix and one `(sca, rot)` pair per group level. A local point
//! `p` lands in the outermost group's space at
//!
//! ```text
//! M · p,   M = T · R1 · S1 · R2 · S2 · … · Rn · Sn
//! ```
//!
//! — the last pair (the shape's own) applies first, the first pair (given by
//! the outermost group) last, and the shapes of a nested group map straight
//! into the outermost space. The outermost space's origin is the group's own
//! page offset: Hancom's "ungroup" puts every shape at the group's position
//! plus its chained coordinates, even when the group's own frame (its chain
//! applied to its initial size) starts elsewhere.
//!
//! Whatever Core cannot carry is reported as [`Hwp5Warning::ContentLost`]:
//! what the fold cannot place exactly, an unreadable chain, pictures and OLE
//! objects (Core groups have no picture child yet), the text of a shape that
//! does not keep it, the controls inside a child's text (the group child
//! parser keeps only text), a directional fill (HWP5 shape styles are not
//! projected at all yet — only the fills that decide geometry are reported
//! here), a displayed size or a nested group box that disagrees with the
//! shapes, and a group left with nothing.

use hwpforge_core::control::geometry::{
    emitted_corners, fold_group_child, group_bounds, Bounds, EmissionPolicy, FoldInput,
    FoldLossKind, FoldOutcome, GeometryFacts, ShapeTransform,
};
use hwpforge_core::control::{Control, ShapePoint};
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::ObjectId;
use hwpforge_foundation::{ArcType, CharShapeIndex, CurveSegmentType, HwpUnit};

use crate::ctrl_ids::{CTRL_ID_EQED, CTRL_ID_GSO, CTRL_ID_TABLE};
use crate::decoder::section::{Hwp5Control, Hwp5GroupChild, Hwp5GroupControl};
use crate::decoder::Hwp5Warning;
use crate::schema::section::{Hwp5Matrix, Hwp5ShapeFill, Hwp5ShapePoint, Hwp5ShapeRender};

use super::shapes::offset_placement;
use super::text::hwp_unit_from_u32;
use super::{
    project_equation_run, project_nested_paragraphs, project_text_art_run,
    shape_vertical_align_from_list_header, ImageProjectionContext, ProjectionImageState,
};

/// Largest distance, in HWPUNIT, between a group's displayed size and the
/// bounds of its shapes still treated as the same box (as the HWPX decoder).
const DISPLAY_SIZE_TOLERANCE: f64 = 2.0;

/// How far a nested group's own box may sit from the bounds of its shapes
/// (as the HWPX decoder).
const NESTED_BOX_TOLERANCE: f64 = 1.0;

/// Where a warning sits: the group's number in document order, then the
/// child positions (`group[3].child[2].child[0]`).
struct Place {
    group: usize,
    path: Vec<usize>,
}

impl Place {
    fn location(&self) -> String {
        let mut s = format!("group[{}]", self.group);
        for i in &self.path {
            s.push_str(&format!(".child[{i}]"));
        }
        s
    }
}

fn lost(state: &mut ProjectionImageState<'_>, place: &Place, kind: &str, reason: String) {
    state.warnings.push(Hwp5Warning::ContentLost {
        kind: kind.to_string(),
        reason,
        location: place.location(),
    });
}

fn matrix(m: &Hwp5Matrix) -> ShapeTransform {
    ShapeTransform::from_matrix(*m)
}

/// `T · R1 · S1 · … · Rn · Sn`: within a pair the scale applies first.
fn chain(render: &Hwp5ShapeRender) -> ShapeTransform {
    render
        .pairs
        .iter()
        .fold(matrix(&render.trans), |m, (sca, rot)| m.then(matrix(rot)).then(matrix(sca)))
}

fn initial_size(render: &Hwp5ShapeRender) -> (f64, f64) {
    (f64::from(render.initial.0), f64::from(render.initial.1))
}

/// The four corners of a render's initial box, in the outermost space.
fn render_corners(render: &Hwp5ShapeRender) -> [(f64, f64); 4] {
    let m = chain(render);
    let (w, h) = initial_size(render);
    [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].map(|p| m.apply(p))
}

fn point(p: Hwp5ShapePoint) -> ShapePoint {
    ShapePoint { x: p.x, y: p.y }
}

/// Name of a child kind Core groups do not hold.
fn unsupported_kind(control: &Hwp5Control) -> Option<&'static str> {
    match control {
        Hwp5Control::Image(_) => Some("pic"),
        Hwp5Control::OleObject(_) => Some("ole"),
        Hwp5Control::Rect(_)
        | Hwp5Control::Ellipse(_)
        | Hwp5Control::Arc(_)
        | Hwp5Control::Polygon(_)
        | Hwp5Control::Curve(_)
        | Hwp5Control::Line(_)
        | Hwp5Control::ConnectLine(_)
        | Hwp5Control::TextArt(_)
        | Hwp5Control::Equation(_)
        | Hwp5Control::Group(_) => None,
        _ => Some("unknown"),
    }
}

/// Bounds, in the outermost space, of every child box under `children`
/// (nested groups included); children Core does not carry count only when
/// `all` is set. Unreadable chains are skipped (they are reported on their
/// own).
fn wire_bounds(children: &[Hwp5GroupChild], all: bool) -> Option<Bounds> {
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for child in children {
        if let Hwp5Control::Group(nested) = &child.control {
            if let Some(b) = wire_bounds(&nested.children, all) {
                pts.extend([(b.min_x(), b.min_y()), (b.max_x(), b.max_y())]);
            }
            continue;
        }
        if !all && unsupported_kind(&child.control).is_some() {
            continue;
        }
        if let Ok(render) = &child.render {
            pts.extend(render_corners(render));
        }
    }
    group_bounds(pts)
}

/// Whether a child under `children` (nested groups included) is a kind Core
/// groups do not hold.
fn has_unsupported_child(children: &[Hwp5GroupChild]) -> bool {
    children.iter().any(|c| match &c.control {
        Hwp5Control::Group(nested) => has_unsupported_child(&nested.children),
        other => unsupported_kind(other).is_some(),
    })
}

/// Projects a top-level group into a `Control::Group` run. The group keeps
/// its displayed size (`CtrlHeader`) and page offset; its children are folded
/// against the origin of the outermost space. `None` when no child can be
/// represented.
pub(super) fn project_group_run(
    group: &Hwp5GroupControl,
    state: &mut ProjectionImageState<'_>,
) -> Option<Run> {
    let mut place = Place { group: state.group_seq, path: Vec::new() };
    state.group_seq += 1;
    if let Err(reason) = &group.render {
        lost(state, &place, "group_transform", format!("group matrix chain: {reason}"));
    }
    check_display_size(group, &place, state);
    let children = project_children(&group.children, (0.0, 0.0), &mut place, state);
    if children.is_empty() {
        lost(state, &place, "group", "no group child can be represented in Core".to_string());
        return None;
    }
    let inst_id = (group.instance_id != 0).then_some(ObjectId::new(u64::from(group.instance_id)));
    Some(Run::control(
        Control::Group {
            children,
            width: hwp_unit_from_u32(group.geometry.width),
            height: hwp_unit_from_u32(group.geometry.height),
            placement: offset_placement(group.geometry.x, group.geometry.y),
            inst_id,
        },
        CharShapeIndex::new(0),
    ))
}

/// The displayed size is the extent of every shape the group holds
/// (pictures included) — or a box from the origin that holds them. Anything
/// else means the shapes do not sit where the group is drawn.
fn check_display_size(
    group: &Hwp5GroupControl,
    place: &Place,
    state: &mut ProjectionImageState<'_>,
) {
    let size = (i32::try_from(group.geometry.width), i32::try_from(group.geometry.height));
    let (Ok(width), Ok(height)) = size else {
        lost(state, place, "group_display_size", "displayed size outside the HWPUNIT range".into());
        return;
    };
    if HwpUnit::new(width).is_err() || HwpUnit::new(height).is_err() {
        lost(state, place, "group_display_size", "displayed size outside the HWPUNIT range".into());
        return;
    }
    let Some(b) = wire_bounds(&group.children, true) else { return };
    let (w, h) = (f64::from(width), f64::from(height));
    let extent_error = ((b.max_x() - b.min_x()) - w).abs().max(((b.max_y() - b.min_y()) - h).abs());
    let overflow = (-b.min_x()).max(-b.min_y()).max(b.max_x() - w).max(b.max_y() - h).max(0.0);
    let size_error = extent_error.min(overflow);
    if size_error > DISPLAY_SIZE_TOLERANCE {
        lost(
            state,
            place,
            "group_display_size",
            format!(
                "displayed size {width}x{height} differs from the bounds of its shapes by \
                 {size_error:.2}"
            ),
        );
    }
}

/// Children folded against `origin`, the corner of their direct parent group
/// in the outermost space.
fn project_children(
    children: &[Hwp5GroupChild],
    origin: (f64, f64),
    place: &mut Place,
    state: &mut ProjectionImageState<'_>,
) -> Vec<Control> {
    let mut out = Vec::with_capacity(children.len());
    for (i, child) in children.iter().enumerate() {
        place.path.push(i);
        if let Some(control) = project_child(child, origin, place, state) {
            out.push(control);
        }
        place.path.pop();
    }
    out
}

/// A nested group: its children are folded against the parent's corner,
/// then the group is sized by their exact Core corners and they are moved to
/// be relative to it — what a decode of the HWPX writer's output finds again.
fn project_nested_group(
    nested: &Hwp5GroupControl,
    parent_origin: (f64, f64),
    place: &mut Place,
    state: &mut ProjectionImageState<'_>,
) -> Option<Control> {
    check_nested_box(nested, place, state);
    let mut children = project_children(&nested.children, parent_origin, place, state);
    let corners: Vec<(f64, f64)> = children
        .iter()
        .filter_map(|c| emitted_corners(c, &EmissionPolicy::exact()))
        .flatten()
        .collect();
    let Some(b) = group_bounds(corners) else {
        lost(state, place, "group", "no group child can be represented in Core".to_string());
        return None;
    };
    let (dx, dy) = (b.min_x().round() as i32, b.min_y().round() as i32);
    for child in &mut children {
        shift_child(child, -dx, -dy);
    }
    let (w, h) =
        ((b.max_x() - f64::from(dx)).round() as i32, (b.max_y() - f64::from(dy)).round() as i32);
    let fits = [w, h, dx, dy].iter().all(|&v| HwpUnit::new(v).is_ok());
    let (Ok(width), Ok(height), true) = (HwpUnit::new(w), HwpUnit::new(h), fits) else {
        lost(
            state,
            place,
            "nested_group_placement",
            "size or offset outside the HWPUNIT range".to_string(),
        );
        return None;
    };
    Some(Control::Group {
        children,
        width,
        height,
        placement: offset_placement(dx, dy),
        inst_id: None,
    })
}

/// A nested group's own matrices should describe the box of its shapes. A
/// child Core cannot carry (a picture) still counts in the stored box but
/// not in the shapes' bounds; it is reported on its own, so the boxes are not
/// compared then.
fn check_nested_box(
    nested: &Hwp5GroupControl,
    place: &Place,
    state: &mut ProjectionImageState<'_>,
) {
    if has_unsupported_child(&nested.children) {
        return;
    }
    let Some(leaves) = wire_bounds(&nested.children, false) else { return };
    match &nested.render {
        Ok(render) => {
            let Some(own) = group_bounds(render_corners(render)) else { return };
            let err = (own.min_x() - leaves.min_x())
                .abs()
                .max((own.min_y() - leaves.min_y()).abs())
                .max((own.max_x() - leaves.max_x()).abs())
                .max((own.max_y() - leaves.max_y()).abs());
            if err > NESTED_BOX_TOLERANCE {
                lost(
                    state,
                    place,
                    "nested_group_placement",
                    format!("nested group box differs from its shapes by {err:.2}"),
                );
            }
        }
        Err(reason) => lost(
            state,
            place,
            "nested_group_placement",
            format!("nested group matrix chain: {reason}"),
        ),
    }
}

fn shift_child(child: &mut Control, dx: i32, dy: i32) {
    let placement = match child {
        Control::TextBox { placement, .. }
        | Control::Rect { placement, .. }
        | Control::Ellipse { placement, .. }
        | Control::Arc { placement, .. }
        | Control::Polygon { placement, .. }
        | Control::Curve { placement, .. }
        | Control::ConnectLine { placement, .. }
        | Control::Line { placement, .. }
        | Control::TextArt { placement, .. }
        | Control::EmbeddedChart { placement, .. }
        | Control::Group { placement, .. } => placement,
        _ => return,
    };
    let (x, y) =
        placement.as_ref().map_or((0, 0), |p| (p.horz_offset.as_i32(), p.vert_offset.as_i32()));
    *placement = offset_placement(x + dx, y + dy);
}

fn project_child(
    child: &Hwp5GroupChild,
    parent_origin: (f64, f64),
    place: &mut Place,
    state: &mut ProjectionImageState<'_>,
) -> Option<Control> {
    if let Hwp5Control::Group(nested) = &child.control {
        return project_nested_group(nested, parent_origin, place, state);
    }
    if let Some(kind) = unsupported_kind(&child.control) {
        lost(state, place, kind, "unsupported group child".to_string());
        return None;
    }
    let render = match &child.render {
        Ok(render) => render,
        Err(reason) => {
            lost(state, place, "group_child_transform", format!("matrix chain: {reason}"));
            return None;
        }
    };
    report_nested_controls(child, place, state);
    let control = wire_local_control(child, render, place, state)?;
    let turns = match render.fill {
        Hwp5ShapeFill::Plain => false,
        Hwp5ShapeFill::Directional(fill) => {
            // It turns with the shape, so the shape keeps its rotation.
            lost(state, place, "group_child_fill", format!("{fill} fill not carried by Core"));
            true
        }
        Hwp5ShapeFill::Unreadable => {
            lost(state, place, "group_child_fill", "fill information unreadable".to_string());
            true
        }
    };
    let input = FoldInput::new(chain(render), initial_size(render), parent_origin)
        .with_geometry_facts(GeometryFacts::new(turns));
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
        lost(state, place, kind, loss.detail().to_string());
    }
    kept
}

/// The group child parser keeps only the text of a child's paragraphs:
/// every control under the child is lost. Objects (tables, drawing objects,
/// equations) are `group_nested_object`, the rest `group_child_control`.
fn report_nested_controls(
    child: &Hwp5GroupChild,
    place: &Place,
    state: &mut ProjectionImageState<'_>,
) {
    for &id in &child.nested_controls {
        let name = String::from_utf8_lossy(&id.to_be_bytes()).into_owned();
        if matches!(id, CTRL_ID_TABLE | CTRL_ID_GSO | CTRL_ID_EQED) {
            lost(
                state,
                place,
                "group_nested_object",
                format!("`{name}` object inside a group child is not decoded"),
            );
        } else {
            lost(
                state,
                place,
                "group_child_control",
                format!("`{name}` control in a group child's text is not decoded"),
            );
        }
    }
}

/// The child as a Core control with its stored local geometry (points not
/// rescaled, centre and axes as stored), ready to fold. `None` = reported
/// and dropped.
fn wire_local_control(
    child: &Hwp5GroupChild,
    render: &Hwp5ShapeRender,
    place: &Place,
    state: &mut ProjectionImageState<'_>,
) -> Option<Control> {
    let (width, height) =
        (hwp_unit_from_u32(render.initial.0), hwp_unit_from_u32(render.initial.1));
    let has_text = !child.paragraphs.is_empty();
    let text = |state: &mut ProjectionImageState<'_>| {
        project_nested_paragraphs(&child.paragraphs, state, ImageProjectionContext::TextBox)
    };
    let valign = shape_vertical_align_from_list_header(child.list_header_properties);
    let degenerate = |state: &mut ProjectionImageState<'_>, what: &str| {
        lost(state, place, "group_child_degenerate", format!("{what} has no shape to draw"));
        None
    };
    let control = match &child.control {
        Hwp5Control::Rect(_) if has_text => {
            let mut c = Control::text_box(text(state), width, height);
            if let Control::TextBox { text_vertical_align, .. } = &mut c {
                *text_vertical_align = valign;
            }
            c
        }
        Hwp5Control::Rect(_) => match Control::rect(width, height) {
            Ok(c) => c,
            Err(_) => return degenerate(state, "rect"),
        },
        Hwp5Control::Ellipse(e) => {
            let mut c = if has_text {
                Control::ellipse_with_text(width, height, text(state))
            } else {
                Control::ellipse(width, height)
            };
            if let Control::Ellipse { center, axis1, axis2, text_vertical_align, .. } = &mut c {
                *center = point(e.shape.center);
                *axis1 = point(e.shape.axis1);
                *axis2 = point(e.shape.axis2);
                if has_text {
                    *text_vertical_align = valign;
                }
            }
            c
        }
        Hwp5Control::Arc(a) => {
            let mut c = Control::arc(ArcType::Normal, width, height);
            if let Control::Arc { center, axis1, axis2, start1, end1, start2, end2, .. } = &mut c {
                *center = point(a.shape.center);
                *axis1 = point(a.shape.axis1);
                *axis2 = point(a.shape.axis2);
                *start1 = point(a.shape.start1);
                *end1 = point(a.shape.end1);
                *start2 = point(a.shape.start2);
                *end2 = point(a.shape.end2);
            }
            c
        }
        Hwp5Control::Polygon(p) => {
            match Control::polygon(p.points.iter().copied().map(point).collect()) {
                Ok(c) => c,
                Err(_) => return degenerate(state, "polygon"),
            }
        }
        Hwp5Control::Curve(curve) => {
            let mut c = match Control::curve(curve.points.iter().copied().map(point).collect()) {
                Ok(c) => c,
                Err(_) => return degenerate(state, "curve"),
            };
            if let Control::Curve { segment_types, .. } = &mut c {
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
            c
        }
        Hwp5Control::Line(l) => match Control::line(point(l.start), point(l.end)) {
            Ok(c) => c,
            Err(_) => return degenerate(state, "line"),
        },
        Hwp5Control::ConnectLine(l) => match Control::connect_line(point(l.start), point(l.end)) {
            Ok(c) => c,
            Err(_) => return degenerate(state, "connect line"),
        },
        Hwp5Control::TextArt(t) => match project_text_art_run(t, &mut state.warnings).content {
            RunContent::Control(c) => *c,
            _ => return None,
        },
        Hwp5Control::Equation(e) => match project_equation_run(e).content {
            RunContent::Control(c) => *c,
            _ => return None,
        },
        // Groups and unsupported kinds never reach here (`project_child`).
        _ => return None,
    };
    let keeps_text = matches!(control, Control::TextBox { .. } | Control::Ellipse { .. });
    if has_text && !keeps_text {
        lost(
            state,
            place,
            "group_child_text",
            format!("text of a {} group child is not carried", control.kind_name()),
        );
    }
    Some(control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::package::{DecompressBudget, MAX_TOTAL_DECOMPRESSED};
    use crate::decoder::section::{
        Hwp5EllipseControl, Hwp5EquationControl, Hwp5ImageControl, Hwp5Paragraph,
        Hwp5PolygonControl, Hwp5RectControl, Hwp5TextArtControl,
    };
    use crate::schema::section::{
        Hwp5ShapeComponentEllipse, Hwp5ShapeComponentGeometry, Hwp5ShapeTextArt,
    };

    const ID: Hwp5Matrix = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

    fn tr(x: f64, y: f64) -> Hwp5Matrix {
        [1.0, 0.0, x, 0.0, 1.0, y]
    }

    fn rot(deg: f64) -> Hwp5Matrix {
        let (s, c) = deg.to_radians().sin_cos();
        [c, -s, 0.0, s, c, 0.0]
    }

    fn render(
        initial: (u32, u32),
        trans: Hwp5Matrix,
        pair: (Hwp5Matrix, Hwp5Matrix),
    ) -> Hwp5ShapeRender {
        Hwp5ShapeRender { initial, trans, pairs: vec![pair], fill: Hwp5ShapeFill::Plain }
    }

    /// A shape `w × h` placed by a translation only.
    fn at(w: u32, h: u32, x: f64, y: f64) -> Result<Hwp5ShapeRender, String> {
        Ok(render((w, h), tr(x, y), (ID, ID)))
    }

    fn geo(x: i32, y: i32, width: u32, height: u32) -> Hwp5ShapeComponentGeometry {
        Hwp5ShapeComponentGeometry { x, y, width, height }
    }

    fn child(control: Hwp5Control, render: Result<Hwp5ShapeRender, String>) -> Hwp5GroupChild {
        Hwp5GroupChild {
            control,
            paragraphs: Vec::new(),
            list_header_properties: None,
            render,
            nested_controls: Vec::new(),
        }
    }

    fn rect() -> Hwp5Control {
        Hwp5Control::Rect(Hwp5RectControl {
            ctrl_id: 0,
            geometry: geo(0, 0, 0, 0),
            ctrl_properties: 0,
        })
    }

    fn polygon(points: &[(i32, i32)]) -> Hwp5Control {
        Hwp5Control::Polygon(Hwp5PolygonControl {
            ctrl_id: 0,
            geometry: geo(0, 0, 0, 0),
            points: points.iter().map(|&(x, y)| Hwp5ShapePoint { x, y }).collect(),
            ctrl_properties: 0,
        })
    }

    fn picture() -> Hwp5Control {
        Hwp5Control::Image(Hwp5ImageControl {
            ctrl_id: CTRL_ID_GSO,
            geometry: geo(0, 0, 0, 0),
            binary_data_id: 1,
            instance_id: 0,
            ctrl_properties: 0,
        })
    }

    fn group(
        geometry: Hwp5ShapeComponentGeometry,
        render: Result<Hwp5ShapeRender, String>,
        children: Vec<Hwp5GroupChild>,
    ) -> Hwp5GroupControl {
        Hwp5GroupControl { ctrl_id: CTRL_ID_GSO, geometry, children, render, instance_id: 0 }
    }

    fn nested(
        render: Result<Hwp5ShapeRender, String>,
        children: Vec<Hwp5GroupChild>,
    ) -> Hwp5GroupChild {
        child(Hwp5Control::Group(group(geo(0, 0, 0, 0), render, children)), Ok(render_stub()))
    }

    fn render_stub() -> Hwp5ShapeRender {
        render((0, 0), ID, (ID, ID))
    }

    fn paragraph(text: &str) -> Hwp5Paragraph {
        Hwp5Paragraph {
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: text.to_string(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            line_segments: Vec::new(),
            controls: vec![],
        }
    }

    /// Projects `top`, returning the group (if any) and the `ContentLost`
    /// kinds in order.
    fn project(top: &Hwp5GroupControl) -> (Option<Control>, Vec<String>) {
        let mut state =
            ProjectionImageState::new(None, None, DecompressBudget::new(MAX_TOTAL_DECOMPRESSED));
        let run = project_group_run(top, &mut state);
        let kinds = state
            .warnings
            .iter()
            .filter_map(|w| match w {
                Hwp5Warning::ContentLost { kind, .. } => Some(kind.clone()),
                _ => None,
            })
            .collect();
        let group = run.map(|r| match r.content {
            RunContent::Control(c) => *c,
            other => panic!("not a control: {other:?}"),
        });
        (group, kinds)
    }

    fn children(c: &Control) -> &[Control] {
        match c {
            Control::Group { children, .. } => children,
            other => panic!("not a group: {other:?}"),
        }
    }

    fn offset(c: &Control) -> (i32, i32) {
        let placement = match c {
            Control::Rect { placement, .. }
            | Control::Polygon { placement, .. }
            | Control::Ellipse { placement, .. }
            | Control::TextBox { placement, .. }
            | Control::TextArt { placement, .. }
            | Control::Group { placement, .. } => placement,
            Control::Equation { .. } => &None,
            other => panic!("unexpected {other:?}"),
        };
        placement.as_ref().map_or((0, 0), |p| (p.horz_offset.as_i32(), p.vert_offset.as_i32()))
    }

    fn rotation(c: &Control) -> Option<f32> {
        match c {
            Control::Polygon { style, .. } | Control::Rect { style, .. } => {
                style.as_ref().and_then(|s| s.rotation)
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    /// HWP5 places every shape of a group, nested groups included, in the
    /// outermost group's space; Core places a child relative to its direct
    /// parent group. Three levels, leaves at outermost A (100, 50),
    /// B (2600, 3400) and C (2500, 5000):
    ///
    /// | Core | offset | sum on the path |
    /// | -- | -- | -- |
    /// | A | (100, 50) | (100, 50) |
    /// | N1 (bounds of B, C) | (2500, 3400) | |
    /// | B | (100, 0) | (2600, 3400) |
    /// | N2 (bounds of C) | (0, 1600) | |
    /// | C | (0, 0) — inline placement | (2500, 5000) |
    ///
    /// 이것을 실패시키는 것: 중첩 묶음이 자식 offset 을 자기 원점만큼 빼지 않는
    /// 것(B 가 (2600, 3400)), 두 번 빼는 것(B 가 (−2400, −3400)).
    #[test]
    fn nested_children_are_placed_relative_to_their_direct_group() {
        let n2 =
            nested(at(200, 100, 2500.0, 5000.0), vec![child(rect(), at(200, 100, 2500.0, 5000.0))]);
        let n1 = nested(
            at(600, 1700, 2500.0, 3400.0),
            vec![child(rect(), at(500, 600, 2600.0, 3400.0)), n2],
        );
        let top = group(
            geo(1000, 2000, 5000, 6000),
            at(5000, 6000, 0.0, 0.0),
            vec![child(rect(), at(100, 100, 100.0, 50.0)), n1],
        );
        let (g, lost) = project(&top);
        assert!(lost.is_empty(), "{lost:?}");
        let g = g.unwrap();
        let [a, n1] = children(&g) else { panic!() };
        assert_eq!(offset(a), (100, 50));
        assert_eq!(offset(n1), (2500, 3400));
        let [b, n2] = children(n1) else { panic!() };
        assert_eq!(offset(b), (100, 0));
        assert_eq!(offset(n2), (0, 1600));
        let [c] = children(n2) else { panic!() };
        let Control::Rect { placement, .. } = c else { panic!() };
        assert!(placement.is_none(), "a child at its group's corner stays inline");
    }

    /// Children are folded against the outermost space's origin, not against
    /// the corner of the group's own frame. Here the group's own chain turns
    /// its 1000 × 1000 frame by 90°, so the frame starts at (−1000, 0); the
    /// rect at chained (300, 40) still gets offset (300, 40).
    ///
    /// 이것을 실패시키는 것: 최상위 원점을 묶음 자기 행렬열로 옮긴 틀의 최솟값으로
    /// 잡는 것(offset 이 (1300, 40)).
    #[test]
    fn top_level_children_are_placed_from_the_outer_origin() {
        let own = Ok(render((1000, 1000), [0.0, -1.0, 0.0, 1.0, 0.0, 0.0], (ID, ID)));
        let top = group(geo(0, 0, 400, 140), own, vec![child(rect(), at(100, 100, 300.0, 40.0))]);
        let (g, lost) = project(&top);
        assert!(lost.is_empty(), "{lost:?}");
        assert_eq!(offset(&children(&g.unwrap())[0]), (300, 40));
    }

    /// An unreadable chain drops the child with that reason — not as a
    /// zero-size shape.
    ///
    /// 이것을 실패시키는 것: 행렬열 오류 갈래에서 경고를 지우는 것.
    #[test]
    fn unreadable_child_chain_is_reported_as_such() {
        let top = group(
            geo(0, 0, 100, 100),
            at(100, 100, 0.0, 0.0),
            vec![child(rect(), Err("cut short".into())), child(rect(), at(100, 100, 0.0, 0.0))],
        );
        let (g, lost) = project(&top);
        assert_eq!(lost, ["group_child_transform"]);
        assert_eq!(children(&g.unwrap()).len(), 1);
    }

    /// 이것을 실패시키는 것: 최상위 묶음 행렬열 오류 경고를 지우는 것.
    #[test]
    fn unreadable_group_chain_is_reported() {
        let top = group(
            geo(0, 0, 100, 100),
            Err("cut short".into()),
            vec![child(rect(), at(100, 100, 0.0, 0.0))],
        );
        assert_eq!(project(&top).1, ["group_transform"]);
    }

    /// The stored centre and axes are kept and scaled with the box, even when
    /// the centre is not the box centre: 1000 × 600 at ×0.5 →
    /// centre (300, 300) → (150, 150), axis1 (800, 300) → (400, 150),
    /// axis2 (300, 600) → (150, 300).
    ///
    /// 이것을 실패시키는 것: 타원 중심·축을 상자에서 다시 만드는 것(중심이
    /// (250, 150)).
    #[test]
    fn ellipse_keeps_its_stored_centre_and_axes() {
        let pt = |x, y| Hwp5ShapePoint { x, y };
        let shape = Hwp5ShapeComponentEllipse {
            property: 0,
            center: pt(300, 300),
            axis1: pt(800, 300),
            axis2: pt(300, 600),
            start1: pt(0, 0),
            end1: pt(0, 0),
            start2: pt(0, 0),
            end2: pt(0, 0),
        };
        let ellipse = Hwp5Control::Ellipse(Hwp5EllipseControl {
            ctrl_id: 0,
            geometry: geo(0, 0, 0, 0),
            ctrl_properties: 0,
            shape,
        });
        let half = [0.5, 0.0, 0.0, 0.0, 0.5, 0.0];
        let top = group(
            geo(0, 0, 500, 300),
            at(500, 300, 0.0, 0.0),
            vec![child(ellipse, Ok(render((1000, 600), ID, (half, ID))))],
        );
        let (g, lost) = project(&top);
        assert!(lost.is_empty(), "{lost:?}");
        let g = g.unwrap();
        let Control::Ellipse { width, height, center, axis1, axis2, .. } = &children(&g)[0] else {
            panic!()
        };
        assert_eq!((width.as_i32(), height.as_i32()), (500, 300));
        assert_eq!((center.x, center.y), (150, 150));
        assert_eq!((axis1.x, axis1.y), (400, 150));
        assert_eq!((axis2.x, axis2.y), (150, 300));
    }

    /// Polygon points are taken as stored — outside the stored box and inside
    /// it — not rescaled onto the box. Points (−1000, 0), (2000, −500),
    /// (3000, 3000), (1000, 1000) of a 4000 × 3000 box moved by (1000, 2000)
    /// land at (0, 2000), (3000, 1500), (4000, 5000), (2000, 3000): offset
    /// (0, 1500), local points (0, 500), (3000, 0), (4000, 3500),
    /// (2000, 1500), size 4000 × 3500.
    ///
    /// 이것을 실패시키는 것: 묶음 경로에서 점을 상자에 맞춰 늘리는 것
    /// (`scale_polygon_points`).
    #[test]
    fn polygon_keeps_its_stored_points() {
        let poly = polygon(&[(-1000, 0), (2000, -500), (3000, 3000), (1000, 1000)]);
        let top = group(
            geo(0, 0, 5000, 5000),
            at(5000, 5000, 0.0, 0.0),
            vec![child(poly, at(4000, 3000, 1000.0, 2000.0))],
        );
        let (g, lost) = project(&top);
        assert!(lost.is_empty(), "{lost:?}");
        let Control::Polygon { width, height, vertices, .. } = &children(g.as_ref().unwrap())[0]
        else {
            panic!()
        };
        let pts: Vec<(i32, i32)> = vertices.iter().map(|p| (p.x, p.y)).collect();
        assert_eq!(pts, [(0, 500), (3000, 0), (4000, 3500), (2000, 1500)]);
        assert_eq!((width.as_i32(), height.as_i32()), (4000, 3500));
        assert_eq!(offset(&children(&g.unwrap())[0]), (0, 1500));
    }

    /// Pictures are not Core group children yet: each is reported, and a
    /// group with nothing else is reported too.
    ///
    /// 이것을 실패시키는 것: 미지원 자식 경고를 지우는 것, 빈 묶음 경고를 지우는 것.
    #[test]
    fn pictures_are_reported_and_a_picture_only_group_too() {
        let mixed = group(
            geo(0, 0, 200, 100),
            at(200, 100, 0.0, 0.0),
            vec![child(picture(), at(100, 100, 0.0, 0.0)), child(rect(), at(100, 100, 100.0, 0.0))],
        );
        let (g, lost) = project(&mixed);
        assert_eq!(lost, ["pic"]);
        assert_eq!(children(&g.unwrap()).len(), 1);

        let only = group(
            geo(0, 0, 200, 100),
            at(200, 100, 0.0, 0.0),
            vec![
                child(picture(), at(100, 100, 0.0, 0.0)),
                child(picture(), at(100, 100, 100.0, 0.0)),
            ],
        );
        let (g, lost) = project(&only);
        assert!(g.is_none());
        assert_eq!(lost, ["pic", "pic", "group"]);
    }

    /// A rect keeps its text as a text box; a polygon's text is not carried.
    ///
    /// 이것을 실패시키는 것: 글자 drop 경고를 지우는 것.
    #[test]
    fn text_of_a_shape_that_cannot_keep_it_is_reported() {
        let mut boxed = child(rect(), at(100, 100, 0.0, 0.0));
        boxed.paragraphs.push(paragraph("kept"));
        let mut poly = child(polygon(&[(0, 0), (100, 0), (0, 100)]), at(100, 100, 100.0, 0.0));
        poly.paragraphs.push(paragraph("dropped"));
        let top = group(geo(0, 0, 200, 100), at(200, 100, 0.0, 0.0), vec![boxed, poly]);
        let (g, lost) = project(&top);
        assert_eq!(lost, ["group_child_text"]);
        let g = g.unwrap();
        assert!(matches!(children(&g)[0], Control::TextBox { .. }));
        assert!(matches!(children(&g)[1], Control::Polygon { .. }));
    }

    /// Controls under a child's text are not decoded: an object is
    /// `group_nested_object`, anything else `group_child_control`.
    ///
    /// 이것을 실패시키는 것: 중첩 컨트롤 보고를 지우는 것, 개체·그 밖 분류를 지우는 것.
    #[test]
    fn controls_inside_a_child_are_reported() {
        let mut boxed = child(rect(), at(100, 100, 0.0, 0.0));
        boxed.paragraphs.push(paragraph("x"));
        boxed.nested_controls = vec![CTRL_ID_TABLE, u32::from_be_bytes(*b"%bmk")];
        let top = group(geo(0, 0, 100, 100), at(100, 100, 0.0, 0.0), vec![boxed]);
        assert_eq!(project(&top).1, ["group_nested_object", "group_child_control"]);
    }

    /// A directional fill turns with the shape, so a polygon with one keeps
    /// the group's 30° turn as a rotation (box rule) and the lost fill is
    /// reported; solid stays on the point rule (no rotation, no warning).
    /// Solid and pattern share the kind bits.
    ///
    /// 이것을 실패시키는 것: 채우기 사실을 무시하는 것(그러데이션이 점 규칙으로),
    /// 채우기 손실 경고를 지우는 것.
    #[test]
    fn directional_fill_keeps_the_turn_and_is_reported() {
        let fold = |fill: Hwp5ShapeFill| {
            let mut r = render((1000, 1000), tr(1000.0, 1000.0), (ID, rot(30.0)));
            r.fill = fill;
            let top = group(
                geo(0, 0, 3000, 3000),
                at(3000, 3000, 0.0, 0.0),
                vec![child(polygon(&[(0, 0), (1000, 0), (0, 1000)]), Ok(r))],
            );
            let (g, lost) = project(&top);
            (rotation(&children(&g.unwrap())[0]), lost)
        };
        let (r, lost) = fold(Hwp5ShapeFill::Plain);
        assert_eq!((r, lost.len()), (None, 0));
        for fill in [
            Hwp5ShapeFill::Directional("gradient"),
            Hwp5ShapeFill::Directional("pattern"),
            Hwp5ShapeFill::Directional("picture"),
            Hwp5ShapeFill::Unreadable,
        ] {
            let (r, lost) = fold(fill);
            assert!(r.is_some_and(|a| (a - 30.0).abs() < 1e-3), "{fill:?}: {r:?}");
            assert_eq!(lost, ["group_child_fill"], "{fill:?}");
        }
    }

    /// TextArt has no rotation field and an equation no offset: both stay as
    /// unrotated boxes and the loss is reported.
    ///
    /// 이것을 실패시키는 것: 접기 손실(`Unstorable`)을 경고로 옮기지 않는 것.
    #[test]
    fn textart_turn_and_equation_offset_are_reported() {
        let pt = Hwp5ShapePoint { x: 0, y: 0 };
        let art = Hwp5Control::TextArt(Hwp5TextArtControl {
            ctrl_id: 0,
            geometry: geo(0, 0, 0, 0),
            text_art: Hwp5ShapeTextArt {
                points: [pt; 4],
                text: "A".into(),
                font_name: String::new(),
                font_style: String::new(),
                font_type: 0,
                text_shape: 0,
                line_spacing: 120,
                char_spacing: 100,
                align: 0,
            },
            instance_id: 0,
        });
        let top = group(
            geo(0, 0, 3000, 3000),
            at(3000, 3000, 0.0, 0.0),
            vec![child(art, Ok(render((1000, 500), tr(1000.0, 1000.0), (ID, rot(30.0)))))],
        );
        assert_eq!(project(&top).1, ["group_child_transform"]);

        let eq = Hwp5Control::Equation(Hwp5EquationControl {
            ctrl_id: CTRL_ID_EQED,
            geometry: geo(0, 0, 0, 0),
            script: "x".into(),
            instance_id: 0,
        });
        let top = group(
            geo(0, 0, 300, 100),
            at(300, 100, 0.0, 0.0),
            vec![child(eq, at(200, 100, 100.0, 0.0))],
        );
        assert_eq!(project(&top).1, ["group_child_transform"]);
    }

    /// A child the group's chain flattens is dropped and reported.
    ///
    /// 이것을 실패시키는 것: 접기 `Degenerate` 를 다른 종류로 옮기는 것.
    #[test]
    fn collapsed_child_is_dropped_and_reported() {
        let flat = [0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let top = group(
            geo(0, 0, 100, 100),
            at(100, 100, 0.0, 0.0),
            vec![
                child(rect(), Ok(render((100, 100), ID, (flat, ID)))),
                child(rect(), at(100, 100, 0.0, 0.0)),
            ],
        );
        let (g, lost) = project(&top);
        assert_eq!(lost, ["group_child_degenerate"]);
        assert_eq!(children(&g.unwrap()).len(), 1);
    }

    /// The displayed size must be the shapes' extent or a box from the origin
    /// holding them: a 2000 × 1000 shape in a 1000 × 1000 group is neither.
    ///
    /// 이것을 실패시키는 것: 표시 크기 비교를 지우는 것, 넘침(overflow) 대안을
    /// 지우는 것(아래 원점 상자 사례가 경고), 그림을 범위에서 빼는 것.
    #[test]
    fn displayed_size_off_the_shapes_is_reported() {
        let off = group(
            geo(0, 0, 1000, 1000),
            at(1000, 1000, 0.0, 0.0),
            vec![child(rect(), at(2000, 1000, 0.0, 0.0))],
        );
        assert_eq!(project(&off).1, ["group_display_size"]);
        // A box from the origin that holds the shape: not a loss.
        let holds = group(
            geo(0, 0, 5000, 5000),
            at(5000, 5000, 0.0, 0.0),
            vec![child(rect(), at(1000, 1000, 2000.0, 0.0))],
        );
        assert!(project(&holds).1.is_empty());
        // Pictures count in the extent: the rect alone (−500..500) neither
        // spans 2000 nor fits a box from the origin.
        let with_pic = group(
            geo(0, 0, 2000, 1000),
            at(2000, 1000, 0.0, 0.0),
            vec![
                child(rect(), at(1000, 1000, -500.0, 0.0)),
                child(picture(), at(1000, 1000, 500.0, 0.0)),
            ],
        );
        assert_eq!(project(&with_pic).1, ["pic"]);
    }

    /// A nested group whose own box is 10 off its shapes is reported; with a
    /// picture under it the boxes are not compared.
    ///
    /// 이것을 실패시키는 것: 중첩 묶음 상자 비교를 지우는 것, 미지원 자식이 있을
    /// 때 비교를 건너뛰지 않는 것.
    #[test]
    fn nested_group_box_off_its_shapes_is_reported() {
        let n =
            nested(at(500, 600, 2610.0, 3400.0), vec![child(rect(), at(500, 600, 2600.0, 3400.0))]);
        let top = group(geo(0, 0, 4000, 4000), at(4000, 4000, 0.0, 0.0), vec![n]);
        assert_eq!(project(&top).1, ["nested_group_placement"]);

        let n = nested(
            at(500, 600, 2610.0, 3400.0),
            vec![
                child(rect(), at(500, 600, 2600.0, 3400.0)),
                child(picture(), at(10, 10, 2600.0, 3400.0)),
            ],
        );
        let top = group(geo(0, 0, 4000, 4000), at(4000, 4000, 0.0, 0.0), vec![n]);
        assert_eq!(project(&top).1, ["pic"]);
    }
}
