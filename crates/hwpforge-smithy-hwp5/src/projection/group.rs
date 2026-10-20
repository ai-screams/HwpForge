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
//! A picture child becomes a [`GroupChild::Picture`], its box folded like a
//! shape's and kept unturned.
//!
//! Whatever Core cannot carry is reported as [`Hwp5Warning::ContentLost`]:
//! what the fold cannot place exactly, an unreadable chain, OLE objects, a
//! picture's turn, effect or border, a link that is not in Hancom's
//! form, the group's own link, the text of a shape that does not keep it,
//! the controls inside a child's text (the group child parser keeps only
//! text), a directional fill (HWP5 shape styles are not projected at all
//! yet — only the fills that decide geometry are reported here), a
//! displayed size or a nested group box that disagrees with the shapes, and
//! a group left with nothing.

use hwpforge_core::control::geometry::{
    emitted_corners, fold_aligned_child, fold_box, fold_group_child, group_bounds, Bounds,
    EmissionPolicy, FoldInput, FoldLossKind, FoldOutcome, GeometryFacts, ShapeTransform,
};
use hwpforge_core::control::{Control, GroupChild, ShapePoint};
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::ObjectId;
use hwpforge_foundation::{ArcType, CharShapeIndex, CurveSegmentType, HwpUnit};

use crate::ctrl_ids::{CTRL_ID_EQED, CTRL_ID_GSO, CTRL_ID_TABLE};
use crate::decoder::section::{
    Hwp5Control, Hwp5GroupChild, Hwp5GroupControl, Hwp5ImageControl, Hwp5Paragraph,
};
use crate::decoder::Hwp5Warning;
#[cfg(test)]
use crate::schema::section::Hwp5Matrix;
use crate::schema::section::{Hwp5ShapeFill, Hwp5ShapePoint, Hwp5ShapeRender};

use super::shapes::offset_placement;
use super::text::hwp_unit_from_u32;
use super::{
    object_link_url, project_equation_run, project_nested_paragraphs, project_text_art_run,
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

/// `T · R1 · S1 · … · Rn · Sn`: within a pair the scale applies first.
pub(super) fn chain(render: &Hwp5ShapeRender) -> ShapeTransform {
    render.map()
}

/// S20 (§19.23.13): a chain with no turn, mirror or shear —
/// `x' = a·x + c`, `y' = e·y + f`, `a, e > 0`.
fn aligned(render: &Hwp5ShapeRender) -> bool {
    let m = chain(render).matrix();
    m[1].abs() < 1e-9 && m[3].abs() < 1e-9 && m[0] > 0.0 && m[4] > 0.0
}

/// Whether every leaf under `children` (nested groups included) has an
/// axis-aligned chain — the groups that keep their original box. A child
/// without a readable chain, or a kind Core groups do not hold, is not.
/// 이것을 실패시키는 것: 중첩 묶음을 보지 않기(`hwp5_group_keeps_its_original_box`).
fn all_aligned(children: &[Hwp5GroupChild]) -> bool {
    children.iter().all(|c| match &c.control {
        Hwp5Control::Group(g) => all_aligned(&g.children),
        other if unsupported_kind(other).is_some() => false,
        _ => c.render.as_ref().is_ok_and(aligned),
    })
}

/// The original box of a group: its render's initial size when every leaf
/// under it is axis-aligned and the box can be a frame value.
fn group_frame(
    group: &Hwp5GroupControl,
    displayed: (i32, i32),
    warnings: &mut Vec<crate::decoder::Hwp5Warning>,
) -> hwpforge_core::ObjectFrame {
    // The size lock (bit 20) is kept whether or not the box is.
    // 이것을 실패시키는 것: 잠금을 읽지 않기 / 정렬 판정 뒤에서만 읽기
    // (`hwp5_group_size_lock_is_kept`).
    let frame = hwpforge_core::ObjectFrame::new().with_size_protect(
        group.ctrl_properties & super::shapes::CTRL_PROPERTY_SIZE_PROTECT_BIT != 0,
    );
    let Ok(render) = &group.render else { return frame };
    if !all_aligned(&group.children) {
        return frame;
    }
    match super::shapes::original_box(Some(render.initial), displayed, warnings) {
        Some(o) => frame.with_original_size(o),
        None => frame,
    }
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
        Hwp5Control::OleObject(_) => Some("ole"),
        Hwp5Control::Image(_)
        | Hwp5Control::Rect(_)
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

// ── plan: the geometry of a group, decided once ──────────────────
//
// The plan is pure: it decides which children stay, their folded Core
// geometry, which keep their text, and what is lost, without projecting any
// text. Projection replays it (warnings, text), and the layout-hint
// collector walks the same plan, so hints are taken for exactly the text
// that is written.

/// One thing the plan found lost, in the order it is reported.
enum PlanNote {
    Lost {
        kind: &'static str,
        reason: String,
    },
    /// A warning a projection helper raised (TextArt enums).
    Warning(Hwp5Warning),
}

fn note(kind: &'static str, reason: impl Into<String>) -> PlanNote {
    PlanNote::Lost { kind, reason: reason.into() }
}

enum Outcome<'a> {
    /// A shape stays, with folded geometry; `keeps_text` = its paragraphs
    /// are written (a rect or ellipse holding text).
    Kept {
        control: Box<Control>,
        keeps_text: bool,
    },
    /// A nested group stays: its children's plan and its own box, relative
    /// to its parent.
    Nested {
        plan: GroupPlan<'a>,
        offset: (i32, i32),
        size: (i32, i32),
        group: &'a Hwp5GroupControl,
    },
    /// A picture stays, as an unturned box at `offset` in its group.
    Picture {
        image: &'a Hwp5ImageControl,
        size: (i32, i32),
        offset: (i32, i32),
        hyperlink: Option<String>,
        /// The picture's initial box (S20: its original box).
        original: (u32, u32),
    },
    Dropped,
    /// A nested group that cannot be placed: nothing of it is written, but
    /// its children's own losses are still reported.
    DroppedGroup(GroupPlan<'a>),
}

struct ChildPlan<'a> {
    child: &'a Hwp5GroupChild,
    /// Reported before a nested group's children (its own box check).
    before: Vec<PlanNote>,
    outcome: Outcome<'a>,
    /// Reported after (for a shape: everything; for a nested group: an
    /// empty group or a box out of range).
    after: Vec<PlanNote>,
}

struct GroupPlan<'a> {
    children: Vec<ChildPlan<'a>>,
}

impl GroupPlan<'_> {
    fn kept(&self) -> usize {
        self.children
            .iter()
            .filter(|c| !matches!(c.outcome, Outcome::Dropped | Outcome::DroppedGroup(_)))
            .count()
    }
}

/// What the planner knows of a picture's image: `Err(reason)` = the
/// document has no asset for that BinData reference, and why; `Ok(size)` =
/// it has one, with its pixel size when the header could be read.
type ImageAssets<'f> = &'f dyn Fn(u16) -> Result<Option<(u32, u32)>, String>;

fn plan_children<'a>(
    children: &'a [Hwp5GroupChild],
    origin: (f64, f64),
    assets: ImageAssets<'_>,
) -> GroupPlan<'a> {
    GroupPlan { children: children.iter().map(|c| plan_child(c, origin, assets)).collect() }
}

fn plan_child<'a>(
    child: &'a Hwp5GroupChild,
    origin: (f64, f64),
    assets: ImageAssets<'_>,
) -> ChildPlan<'a> {
    if let Hwp5Control::Group(nested) = &child.control {
        return plan_nested(child, nested, origin, assets);
    }
    if let Hwp5Control::Image(image) = &child.control {
        return plan_picture(child, image, origin, assets);
    }
    let mut after = Vec::new();
    let dropped = |after| ChildPlan { child, before: Vec::new(), outcome: Outcome::Dropped, after };
    if let Some(kind) = unsupported_kind(&child.control) {
        after.push(note(kind, "unsupported group child"));
        return dropped(after);
    }
    let render = match &child.render {
        Ok(render) => render,
        Err(reason) => {
            after.push(note("group_child_transform", format!("matrix chain: {reason}")));
            return dropped(after);
        }
    };
    after.extend(nested_control_notes(child));
    let has_text = !child.paragraphs.is_empty();
    let Some(control) = wire_local_control(child, render, has_text, &mut after) else {
        return dropped(after);
    };
    let keeps_text =
        has_text && matches!(control, Control::TextBox { .. } | Control::Ellipse { .. });
    if has_text && !keeps_text {
        after.push(note(
            "group_child_text",
            format!("text of a {} group child is not carried", control.kind_name()),
        ));
    }
    // What turns with the shape, read from the source: its text (written or
    // not — the source shape's text turned with it) and a directional fill.
    let mut turns = has_text;
    match render.fill {
        Hwp5ShapeFill::Plain => {}
        Hwp5ShapeFill::Directional(fill) => {
            after.push(note("group_child_fill", format!("{fill} fill not carried by Core")));
            turns = true;
        }
        Hwp5ShapeFill::Unreadable => {
            after.push(note("group_child_fill", "fill information unreadable"));
            turns = true;
        }
    }
    // S20 (§19.23.13): an axis-aligned leaf keeps its stored points, in its
    // initial box. A line or connector takes its stored ends as they are.
    // 이것을 실패시키는 것: 이 갈래를 지우기(`hwp5_group_keeps_its_original_box`).
    if aligned(render) {
        let raw = match &child.control {
            Hwp5Control::Line(l) => super::shapes::wire_line(l.start, l.end, false),
            Hwp5Control::ConnectLine(l) => super::shapes::wire_line(l.start, l.end, true),
            _ => control.clone(),
        };
        let input = FoldInput::new(chain(render), initial_size(render), origin);
        if let Some(outcome) = fold_aligned_child(raw, &input) {
            let kept = match outcome {
                FoldOutcome::Kept { control, losses } => {
                    for loss in losses {
                        after.push(note("group_child_transform", loss.detail().to_string()));
                    }
                    Some(control)
                }
                FoldOutcome::Dropped { losses } => {
                    for loss in losses {
                        after.push(note("group_child_transform", loss.detail().to_string()));
                    }
                    None
                }
                _ => None,
            };
            let outcome = match kept {
                Some(control) => Outcome::Kept { control, keeps_text },
                None => Outcome::Dropped,
            };
            return ChildPlan { child, before: Vec::new(), outcome, after };
        }
    }
    let input = FoldInput::new(chain(render), initial_size(render), origin)
        .with_geometry_facts(GeometryFacts::new(turns));
    let (kept, losses) = match fold_group_child(control, &input) {
        FoldOutcome::Kept { control, losses } => (Some(control), losses),
        FoldOutcome::Dropped { losses } => (None, losses),
        _ => (None, Vec::new()),
    };
    for loss in losses {
        let kind = match loss.kind() {
            FoldLossKind::Degenerate => "group_child_degenerate",
            _ => "group_child_transform",
        };
        after.push(note(kind, loss.detail().to_string()));
    }
    let outcome = match kept {
        Some(control) => Outcome::Kept { control, keeps_text },
        None => Outcome::Dropped,
    };
    ChildPlan { child, before: Vec::new(), outcome, after }
}

/// A nested group: its children are folded against a provisional origin
/// near their own corner (so a child far from the outer origin but close to
/// its group still fits the HWPUNIT range), then the group is sized by
/// their exact Core corners and they are moved to be relative to it — what
/// a decode of the HWPX writer's output finds again.
fn plan_nested<'a>(
    child: &'a Hwp5GroupChild,
    nested: &'a Hwp5GroupControl,
    parent_origin: (f64, f64),
    assets: ImageAssets<'_>,
) -> ChildPlan<'a> {
    let mut before = nested_box_notes(nested);
    if nested.has_link {
        before.push(group_link_note());
    }
    let provisional = wire_bounds(&nested.children, false)
        .map_or(parent_origin, |b| (b.min_x().round(), b.min_y().round()));
    let mut plan = plan_children(&nested.children, provisional, assets);
    let mut corners: Vec<(f64, f64)> = Vec::new();
    for c in &plan.children {
        match &c.outcome {
            Outcome::Kept { control, .. } => {
                corners.extend(
                    emitted_corners(control, &EmissionPolicy::exact()).into_iter().flatten(),
                );
            }
            Outcome::Nested { offset, size, .. } | Outcome::Picture { offset, size, .. } => {
                let (x, y) = (f64::from(offset.0), f64::from(offset.1));
                let (w, h) = (f64::from(size.0), f64::from(size.1));
                corners.extend([(x, y), (x + w, y + h)]);
            }
            Outcome::Dropped | Outcome::DroppedGroup(_) => {}
        }
    }
    let mut after = Vec::new();
    let Some(b) = group_bounds(corners) else {
        after.push(note("group", "no group child can be represented in Core"));
        return ChildPlan { child, before, outcome: Outcome::DroppedGroup(plan), after };
    };
    // The correction stays in f64 until the range check: the provisional
    // origin can sit far outside the HWPUNIT range.
    let (dx, dy) = (b.min_x().round(), b.min_y().round());
    let size = ((b.max_x() - dx).round(), (b.max_y() - dy).round());
    let offset = (
        (provisional.0 - parent_origin.0).round() + dx,
        (provisional.1 - parent_origin.1).round() + dy,
    );
    let in_range =
        |v: f64| (f64::from(HwpUnit::MIN_VALUE)..=f64::from(HwpUnit::MAX_VALUE)).contains(&v);
    if ![size.0, size.1, offset.0, offset.1].into_iter().all(in_range) {
        after.push(note("nested_group_placement", "size or offset outside the HWPUNIT range"));
        return ChildPlan { child, before, outcome: Outcome::DroppedGroup(plan), after };
    }
    // The children's own boxes are range-checked, so their bounds are too.
    let (dx, dy) = (dx as i32, dy as i32);
    for c in &mut plan.children {
        match &mut c.outcome {
            Outcome::Kept { control, .. } => shift_child(control, -dx, -dy),
            Outcome::Nested { offset, .. } | Outcome::Picture { offset, .. } => {
                *offset = (offset.0 - dx, offset.1 - dy);
            }
            Outcome::Dropped | Outcome::DroppedGroup(_) => {}
        }
    }
    let outcome = Outcome::Nested {
        plan,
        offset: (offset.0 as i32, offset.1 as i32),
        size: (size.0 as i32, size.1 as i32),
        group: nested,
    };
    ChildPlan { child, before, outcome, after }
}

/// The controls inside a child's text, which the group child parser does
/// not decode: an object (table, drawing, equation) or another control.
fn nested_control_notes(child: &Hwp5GroupChild) -> Vec<PlanNote> {
    child
        .nested_controls
        .iter()
        .map(|&id| {
            let name = String::from_utf8_lossy(&id.to_be_bytes()).into_owned();
            if matches!(id, CTRL_ID_TABLE | CTRL_ID_GSO | CTRL_ID_EQED) {
                note(
                    "group_nested_object",
                    format!("`{name}` object inside a group child is not decoded"),
                )
            } else {
                note(
                    "group_child_control",
                    format!("`{name}` control in a group child's text is not decoded"),
                )
            }
        })
        .collect()
}

/// The note for a group's own hyperlink, which a Core group cannot hold.
fn group_link_note() -> PlanNote {
    note("group_hyperlink", "group hyperlink not carried by Core")
}

/// A picture in a group: its box folded like a shape's (`fold_box`), kept
/// unturned. What a Core picture cannot show is noted as
/// `group_child_picture` — a turn or mirror (kept around the same centre),
/// an image effect, a border — as the HWPX decoder does. Its crop is
/// carried like a body picture's.
fn plan_picture<'a>(
    child: &'a Hwp5GroupChild,
    image: &'a Hwp5ImageControl,
    origin: (f64, f64),
    assets: ImageAssets<'_>,
) -> ChildPlan<'a> {
    // Text and controls recorded under a picture (a list of paragraphs —
    // a caption, if anything) are not carried: a Core group picture gets
    // none of them. Reported whether or not the picture itself is kept.
    let mut after = nested_control_notes(child);
    if !child.paragraphs.is_empty() {
        after.push(note(
            "group_child_text",
            format!(
                "text of a picture group child ({} paragraphs) is not carried",
                child.paragraphs.len()
            ),
        ));
    }
    let dropped = |after| ChildPlan { child, before: Vec::new(), outcome: Outcome::Dropped, after };
    if let Err(reason) = assets(image.binary_data_id) {
        after.push(PlanNote::Warning(Hwp5Warning::DroppedControl { control: "image", reason }));
        return dropped(after);
    }
    let render = match &child.render {
        Ok(render) => render,
        Err(reason) => {
            after.push(note("group_child_transform", format!("matrix chain: {reason}")));
            return dropped(after);
        }
    };
    let (w, h) = initial_size(render);
    let fold = match fold_box(w, h, &FoldInput::new(chain(render), (w, h), origin)) {
        Ok(fold) => fold,
        Err(loss) => {
            let kind = match loss.kind() {
                FoldLossKind::Degenerate => "group_child_degenerate",
                _ => "group_child_transform",
            };
            after.push(note(kind, loss.detail().to_string()));
            return dropped(after);
        }
    };
    for loss in fold.losses() {
        after.push(note("group_child_transform", loss.detail().to_string()));
    }
    if fold.rotation().is_some() || fold.flip().is_some() {
        after.push(note(
            "group_child_picture",
            "rotation/flip: a Core picture cannot turn or mirror; kept unturned around the \
             same centre",
        ));
    }
    after.extend(picture_losses(image).into_iter().map(|r| note("group_child_picture", r)));
    let hyperlink = match object_link_url(image.link.as_ref()) {
        Ok(url) => url,
        Err(reason) => {
            after.push(note("picture_hyperlink", reason));
            None
        }
    };
    let outcome = Outcome::Picture {
        image,
        size: (fold.width(), fold.height()),
        offset: fold.offset(),
        hyperlink,
        original: render.initial,
    };
    ChildPlan { child, before: Vec::new(), outcome, after }
}

/// What a group picture shows that a Core picture cannot (the HWPX
/// decoder's rule on the same fields): an effect (brightness, contrast,
/// effect, or any picture-effect flag); a border (a thickness above zero).
/// A crop is carried (`build_group_picture`).
fn picture_losses(image: &Hwp5ImageControl) -> Vec<String> {
    let p = &image.picture;
    let mut out = Vec::new();
    if p.brightness != 0
        || p.contrast != 0
        || p.effect != 0
        || p.effect_flags.is_some_and(|f| f != 0)
    {
        out.push(format!(
            "effect: {} (brightness {}, contrast {}, effect flags {:?})",
            p.effect, p.brightness, p.contrast, p.effect_flags
        ));
    }
    if p.border_thickness > 0 {
        out.push(format!("border: thickness {}", p.border_thickness));
    }
    out
}

/// A nested group's own matrices should describe the box of its shapes. A
/// child Core cannot carry (a picture) still counts in the stored box but
/// not in the shapes' bounds; it is reported on its own, so the boxes are not
/// compared then.
fn nested_box_notes(nested: &Hwp5GroupControl) -> Vec<PlanNote> {
    if has_unsupported_child(&nested.children) {
        return Vec::new();
    }
    let Some(leaves) = wire_bounds(&nested.children, false) else { return Vec::new() };
    match &nested.render {
        Ok(render) => {
            let Some(own) = group_bounds(render_corners(render)) else { return Vec::new() };
            let err = (own.min_x() - leaves.min_x())
                .abs()
                .max((own.min_y() - leaves.min_y()).abs())
                .max((own.max_x() - leaves.max_x()).abs())
                .max((own.max_y() - leaves.max_y()).abs());
            if err > NESTED_BOX_TOLERANCE {
                vec![note(
                    "nested_group_placement",
                    format!("nested group box differs from its shapes by {err:.2}"),
                )]
            } else {
                Vec::new()
            }
        }
        Err(reason) => {
            vec![note("nested_group_placement", format!("nested group matrix chain: {reason}"))]
        }
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

/// The child as a Core control with its stored local geometry (points not
/// rescaled, centre and axes as stored) and, for a rect or ellipse with
/// text, an empty text list the projection fills. `None` = dropped (noted).
fn wire_local_control(
    child: &Hwp5GroupChild,
    render: &Hwp5ShapeRender,
    has_text: bool,
    notes: &mut Vec<PlanNote>,
) -> Option<Control> {
    let (width, height) =
        (hwp_unit_from_u32(render.initial.0), hwp_unit_from_u32(render.initial.1));
    let mut degenerate = |what: &str| {
        notes.push(note("group_child_degenerate", format!("{what} has no shape to draw")));
        None
    };
    let control = match &child.control {
        Hwp5Control::Rect(_) if has_text => Control::text_box(Vec::new(), width, height),
        Hwp5Control::Rect(_) => match Control::rect(width, height) {
            Ok(c) => c,
            Err(_) => return degenerate("rect"),
        },
        Hwp5Control::Ellipse(e) => {
            let mut c = if has_text {
                Control::ellipse_with_text(width, height, Vec::new())
            } else {
                Control::ellipse(width, height)
            };
            if let Control::Ellipse { center, axis1, axis2, .. } = &mut c {
                *center = point(e.shape.center);
                *axis1 = point(e.shape.axis1);
                *axis2 = point(e.shape.axis2);
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
                Err(_) => return degenerate("polygon"),
            }
        }
        Hwp5Control::Curve(curve) => {
            let mut c = match Control::curve(curve.points.iter().copied().map(point).collect()) {
                Ok(c) => c,
                Err(_) => return degenerate("curve"),
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
            Err(_) => return degenerate("line"),
        },
        Hwp5Control::ConnectLine(l) => match Control::connect_line(point(l.start), point(l.end)) {
            Ok(c) => c,
            Err(_) => return degenerate("connect line"),
        },
        Hwp5Control::TextArt(t) => {
            let mut warnings = Vec::new();
            let run = project_text_art_run(t, &mut warnings);
            notes.extend(warnings.into_iter().map(PlanNote::Warning));
            match run.content {
                RunContent::Control(c) => *c,
                _ => return None,
            }
        }
        Hwp5Control::Equation(e) => match project_equation_run(e).content {
            RunContent::Control(c) => *c,
            _ => return None,
        },
        // Groups and unsupported kinds never reach here (`plan_child`).
        _ => return None,
    };
    Some(control)
}

// ── text the plan keeps (for layout hints) ───────────────────────

/// The `drawText` paragraph lists of `group`'s children that projection
/// writes, in document order: rect and ellipse children the fold keeps,
/// under nested groups that are kept. Layout hints are taken for exactly
/// these.
pub(crate) fn written_group_text(group: &Hwp5GroupControl) -> Vec<&[Hwp5Paragraph]> {
    let mut out = Vec::new();
    // Pictures write no text: whether their images exist does not matter.
    let assets: ImageAssets<'_> = &|_| Ok(None);
    collect_written_text(&plan_children(&group.children, (0.0, 0.0), assets), &mut out);
    out
}

fn collect_written_text<'a>(plan: &GroupPlan<'a>, out: &mut Vec<&'a [Hwp5Paragraph]>) {
    for c in &plan.children {
        match &c.outcome {
            Outcome::Kept { keeps_text: true, .. } => out.push(&c.child.paragraphs),
            Outcome::Nested { plan, .. } => collect_written_text(plan, out),
            Outcome::Kept { .. }
            | Outcome::Picture { .. }
            | Outcome::Dropped
            | Outcome::DroppedGroup(_) => {}
        }
    }
}

// ── projection: replay the plan ──────────────────────────────────

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
    if group.has_link {
        report(vec![group_link_note()], &place, state);
    }
    let pixels = |reference: u16| match state.image_asset(reference) {
        Some(asset) => Ok(state.asset_pixels(asset)),
        None => Err(state.missing_asset_reason(reference)),
    };
    let plan = plan_children(&group.children, (0.0, 0.0), &pixels);
    let kept = plan.kept();
    let children = emit(plan, &mut place, state);
    if kept == 0 {
        lost(state, &place, "group", "no group child can be represented in Core".to_string());
        return None;
    }
    let inst_id = (group.instance_id != 0).then_some(ObjectId::new(u64::from(group.instance_id)));
    let displayed = (
        hwp_unit_from_u32(group.geometry.width).as_i32(),
        hwp_unit_from_u32(group.geometry.height).as_i32(),
    );
    Some(Run::control(
        Control::Group {
            frame: group_frame(group, displayed, &mut state.warnings),
            children,
            width: hwp_unit_from_u32(group.geometry.width),
            height: hwp_unit_from_u32(group.geometry.height),
            placement: super::shapes::with_outer_margin(
                offset_placement(group.geometry.x, group.geometry.y),
                &group.geometry,
                &mut state.warnings,
            ),
            inst_id,
        },
        CharShapeIndex::new(0),
    ))
}

fn report(notes: Vec<PlanNote>, place: &Place, state: &mut ProjectionImageState<'_>) {
    for n in notes {
        match n {
            PlanNote::Lost { kind, reason } => lost(state, place, kind, reason),
            PlanNote::Warning(w) => state.warnings.push(w),
        }
    }
}

/// Replays a plan: reports what it lost and builds the kept children, the
/// text of rect and ellipse children projected in.
fn emit(
    plan: GroupPlan<'_>,
    place: &mut Place,
    state: &mut ProjectionImageState<'_>,
) -> Vec<GroupChild> {
    let mut out = Vec::with_capacity(plan.children.len());
    for (i, c) in plan.children.into_iter().enumerate() {
        place.path.push(i);
        report(c.before, place, state);
        let control = match c.outcome {
            Outcome::Nested { plan, offset, size, group } => {
                let children = emit(plan, place, state);
                Some(Control::Group {
                    frame: group_frame(group, size, &mut state.warnings),
                    children,
                    width: HwpUnit::new(size.0).unwrap_or(HwpUnit::ZERO),
                    height: HwpUnit::new(size.1).unwrap_or(HwpUnit::ZERO),
                    placement: offset_placement(offset.0, offset.1),
                    inst_id: None,
                })
            }
            Outcome::Kept { mut control, keeps_text } => {
                if keeps_text {
                    attach_text(&mut control, c.child, state);
                }
                Some(*control)
            }
            Outcome::DroppedGroup(plan) => {
                report_only(plan, place, state);
                None
            }
            Outcome::Picture { image, size, offset, hyperlink, original } => {
                let mut picture = state.build_group_picture(image, size, offset, hyperlink);
                // S20: the initial box against the folded size, as the HWPX
                // decoder judges a group picture's `orgSz`.
                // 이것을 실패시키는 것: 이 대입을 지우기(`hwp5_group_keeps_its_original_box`).
                if let Some(p) = picture.as_mut() {
                    p.frame.original_size =
                        super::shapes::original_box(Some(original), size, &mut state.warnings);
                }
                report(c.after, place, state);
                out.extend(picture.map(GroupChild::Picture));
                place.path.pop();
                continue;
            }
            Outcome::Dropped => None,
        };
        report(c.after, place, state);
        out.extend(control.map(GroupChild::Shape));
        place.path.pop();
    }
    out
}

/// Replays only the losses of a plan whose group is not written.
fn report_only(plan: GroupPlan<'_>, place: &mut Place, state: &mut ProjectionImageState<'_>) {
    for (i, c) in plan.children.into_iter().enumerate() {
        place.path.push(i);
        report(c.before, place, state);
        if let Outcome::Nested { plan, .. } | Outcome::DroppedGroup(plan) = c.outcome {
            report_only(plan, place, state);
        }
        report(c.after, place, state);
        place.path.pop();
    }
}

fn attach_text(
    control: &mut Control,
    child: &Hwp5GroupChild,
    state: &mut ProjectionImageState<'_>,
) {
    let projected =
        project_nested_paragraphs(&child.paragraphs, state, ImageProjectionContext::TextBox);
    let valign =
        shape_vertical_align_from_list_header(child.list_header_properties, &mut state.warnings);
    match control {
        Control::TextBox { paragraphs, text_vertical_align, text_margin, .. } => {
            *paragraphs = projected;
            *text_vertical_align = valign;
            *text_margin = super::text_box_margin(child.text_margin);
        }
        Control::Ellipse { paragraphs, text_vertical_align, .. } => {
            super::report_shape_text_margin(child.text_margin, &mut state.warnings);
            *paragraphs = projected;
            *text_vertical_align = valign;
        }
        _ => {}
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::package::{DecompressBudget, MAX_TOTAL_DECOMPRESSED};
    use crate::decoder::section::{
        Hwp5EllipseControl, Hwp5EquationControl, Hwp5ImageControl, Hwp5OleObjectControl,
        Hwp5Paragraph, Hwp5PolygonControl, Hwp5RectControl, Hwp5TextArtControl,
    };
    use crate::schema::section::{
        Hwp5ShapeComponentEllipse, Hwp5ShapeComponentGeometry, Hwp5ShapePicture, Hwp5ShapeTextArt,
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
        Hwp5ShapeRender {
            source_id: 0,
            angle: 0,
            flip: (false, false),
            initial,
            trans,
            pairs: vec![pair],
            fill: Hwp5ShapeFill::Plain,
        }
    }

    /// A shape `w × h` placed by a translation only.
    fn at(w: u32, h: u32, x: f64, y: f64) -> Result<Hwp5ShapeRender, String> {
        Ok(render((w, h), tr(x, y), (ID, ID)))
    }

    fn geo(x: i32, y: i32, width: u32, height: u32) -> Hwp5ShapeComponentGeometry {
        Hwp5ShapeComponentGeometry { x, y, width, height, outer_margin: [0; 4] }
    }

    fn child(control: Hwp5Control, render: Result<Hwp5ShapeRender, String>) -> Hwp5GroupChild {
        Hwp5GroupChild {
            control,
            paragraphs: Vec::new(),
            list_header_properties: None,
            text_margin: None,
            render,
            nested_controls: Vec::new(),
        }
    }

    fn rect() -> Hwp5Control {
        Hwp5Control::Rect(Hwp5RectControl {
            standalone_render: None,
            original_size: None,
            look: None,
            ctrl_id: 0,
            geometry: geo(0, 0, 0, 0),
            ctrl_properties: 0,
        })
    }

    fn polygon(points: &[(i32, i32)]) -> Hwp5Control {
        Hwp5Control::Polygon(Hwp5PolygonControl {
            standalone_render: None,
            original_size: None,
            look: None,
            ctrl_id: 0,
            paragraphs: Vec::new(),
            list_header_properties: None,
            text_margin: None,
            geometry: geo(0, 0, 0, 0),
            points: points.iter().map(|&(x, y)| Hwp5ShapePoint { x, y }).collect(),
            ctrl_properties: 0,
        })
    }

    fn picture() -> Hwp5Control {
        Hwp5Control::Image(Hwp5ImageControl {
            standalone_render: None,
            ctrl_id: CTRL_ID_GSO,
            geometry: geo(0, 0, 0, 0),
            binary_data_id: 1,
            original_size: None,
            instance_id: 0,
            ctrl_properties: 0,
            picture: Hwp5ShapePicture {
                in_margin: [0; 4],
                binary_data_id: 1,
                crop: [0, 0, 22_500, 15_000],
                dimension: Some((22_500, 15_000)),
                effect_flags: Some(0),
                ..Default::default()
            },
            link: None,
        })
    }

    fn ole() -> Hwp5Control {
        Hwp5Control::OleObject(Hwp5OleObjectControl {
            ctrl_id: CTRL_ID_GSO,
            geometry: geo(0, 0, 0, 0),
            binary_data_id: 2,
            extent_width: 0,
            extent_height: 0,
        })
    }

    /// An image plan holding BinData 1, a 300 x 200 px PNG.
    fn assets() -> crate::Hwp5JoinedImageAssetPlan {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&300u32.to_be_bytes());
        png.extend_from_slice(&200u32.to_be_bytes());
        assets_of(png)
    }

    /// An image plan holding BinData 1 with these bytes.
    fn assets_of(png: Vec<u8>) -> crate::Hwp5JoinedImageAssetPlan {
        let asset = crate::Hwp5JoinedImageAsset {
            payload: crate::Hwp5SemanticImagePayload {
                binary_data_id: 1,
                storage_name: "image1.png".to_string(),
                package_path: "BinData/image1.png".to_string(),
                format: crate::Hwp5SemanticImageFormat::Png,
                width_hwp: None,
                height_hwp: None,
            },
            bytes: png,
        };
        crate::Hwp5JoinedImageAssetPlan {
            ordered_assets: vec![asset.clone()],
            assets_by_reference: [(1, asset)].into_iter().collect(),
            unavailable_reasons: std::collections::BTreeMap::new(),
        }
    }

    fn group(
        geometry: Hwp5ShapeComponentGeometry,
        render: Result<Hwp5ShapeRender, String>,
        children: Vec<Hwp5GroupChild>,
    ) -> Hwp5GroupControl {
        Hwp5GroupControl {
            ctrl_id: CTRL_ID_GSO,
            geometry,
            children,
            render,
            instance_id: 0,
            has_link: false,
            ctrl_properties: 0,
        }
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
            end_char_shape: None,
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
        project_with(top, assets())
    }

    fn project_with(
        top: &Hwp5GroupControl,
        plan: crate::Hwp5JoinedImageAssetPlan,
    ) -> (Option<Control>, Vec<String>) {
        let mut state = ProjectionImageState::new(
            Some(&plan),
            None,
            DecompressBudget::new(MAX_TOTAL_DECOMPRESSED),
        );
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

    fn children(c: &Control) -> Vec<&Control> {
        match c {
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
    /// 이것을 실패시키는 것: 중첩 묶음 offset 을 임시 원점(자식 wire 경계) 대신
    /// 경계 보정량만으로 두는 것(N1 이 (0, 0)).
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
        let [a, n1] = children(&g)[..] else { panic!() };
        assert_eq!(offset(a), (100, 50));
        assert_eq!(offset(n1), (2500, 3400));
        let [b, n2] = children(n1)[..] else { panic!() };
        assert_eq!(offset(b), (100, 0));
        assert_eq!(offset(n2), (0, 1600));
        let [c] = children(n2)[..] else { panic!() };
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
        assert_eq!(offset(children(&g.unwrap())[0]), (300, 40));
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
            native_type: None,
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
            standalone_render: None,
            original_size: None,
            look: None,
            ctrl_id: 0,
            paragraphs: Vec::new(),
            list_header_properties: None,
            text_margin: None,
            geometry: geo(0, 0, 0, 0),
            ctrl_properties: 0,
            shape,
        });
        let half = [0.5, 0.0, 0.0, 0.0, 0.5, 0.0];
        // Turned 180° about the scaled box (S20: a turned leaf folds by the
        // box rule; an axis-aligned one keeps its points in its initial box).
        let half_turn = [-1.0, 0.0, 500.0, 0.0, -1.0, 300.0];
        let top = group(
            geo(0, 0, 500, 300),
            at(500, 300, 0.0, 0.0),
            vec![child(ellipse, Ok(render((1000, 600), ID, (half, half_turn))))],
        );
        let (g, lost) = project(&top);
        assert!(lost.is_empty(), "{lost:?}");
        let g = g.unwrap();
        let Control::Ellipse { width, height, center, axis1, axis2, style, .. } = &children(&g)[0]
        else {
            panic!()
        };
        assert_eq!(style.as_ref().and_then(|s| s.rotation), Some(180.0));
        assert_eq!((width.as_i32(), height.as_i32()), (500, 300));
        assert_eq!((center.x, center.y), (150, 150));
        assert_eq!((axis1.x, axis1.y), (400, 150));
        assert_eq!((axis2.x, axis2.y), (150, 300));
    }

    /// S20 (§19.23.13): an axis-aligned leaf (a rect stored 100 × 50, scaled
    /// ×4) keeps its stored box as its original box, shown 400 × 200; the
    /// group (initial 500 × 300, shown 1000 × 600) keeps its own. A turned
    /// leaf in a nested group makes the outer group keep none.
    /// 이것을 실패시키는 것: 축 정렬 갈래를 지우기 / 묶음 frame 을 두지 않기 /
    /// 중첩 묶음을 정렬 판정에서 빼기.
    #[test]
    fn hwp5_group_keeps_its_original_box() {
        let four = [4.0, 0.0, 0.0, 0.0, 4.0, 0.0];
        let two = [2.0, 0.0, 0.0, 0.0, 2.0, 0.0];
        let u = |v| HwpUnit::new(v).unwrap();
        let size = |w, h| hwpforge_foundation::Size::new(u(w), u(h));
        let leaf = child(rect(), Ok(render((100, 50), ID, (four, ID))));
        let pic = child(picture(), Ok(render((100, 50), tr(0.0, 50.0), (four, ID))));
        let top =
            group(geo(0, 0, 1000, 600), Ok(render((500, 300), ID, (two, ID))), vec![leaf, pic]);
        let (g, lost) = project(&top);
        assert!(lost.is_empty(), "{lost:?}");
        let g = g.unwrap();
        assert_eq!(g.frame().unwrap().original_size, Some(size(500, 300)));
        let Control::Group { children: kids, .. } = &g else { panic!() };
        let GroupChild::Shape(Control::Rect { width, frame, .. }) = &kids[0] else { panic!() };
        assert_eq!(width.as_i32(), 400);
        assert_eq!(frame.original_size, Some(size(100, 50)));
        let GroupChild::Picture(p) = &kids[1] else { panic!("{kids:?}") };
        assert_eq!(p.frame.original_size, Some(size(100, 50)));

        let leaf = child(rect(), Ok(render((100, 50), ID, (four, ID))));
        let turned = nested(
            at(1000, 600, 0.0, 0.0),
            vec![child(rect(), Ok(render((100, 50), tr(500.0, 0.0), (ID, rot(30.0)))))],
        );
        let top =
            group(geo(0, 0, 1000, 600), Ok(render((500, 300), ID, (two, ID))), vec![leaf, turned]);
        let (g, _) = project(&top);
        assert!(g.unwrap().frame().unwrap().original_size.is_none());
    }

    /// S20 review r1: the group's size lock is kept with or without its
    /// original box — a turned leaf (no box) or an unreadable render.
    #[test]
    fn hwp5_group_size_lock_is_kept() {
        let turned = child(rect(), Ok(render((100, 50), tr(500.0, 0.0), (ID, rot(30.0)))));
        let mut top =
            group(geo(0, 0, 1000, 600), Ok(render((500, 300), ID, (ID, ID))), vec![turned]);
        top.ctrl_properties = 1 << 20;
        let (g, _) = project(&top);
        let frame = *g.unwrap().frame().unwrap();
        assert!(frame.size_protect && frame.original_size.is_none(), "{frame:?}");
        let mut top = group(
            geo(0, 0, 1000, 600),
            Err("unreadable".to_string()),
            vec![child(rect(), at(100, 50, 0.0, 0.0))],
        );
        top.ctrl_properties = 1 << 20;
        let (g, _) = project(&top);
        assert!(g.unwrap().frame().unwrap().size_protect);
    }

    /// Polygon points are taken as stored — outside the stored box and inside
    /// it — not rescaled onto the box. The polygon is axis-aligned (moved by
    /// (1000, 2000) only), so S20 keeps them in its 4000 × 3000 initial box:
    /// the box at (1000, 2000), the points as stored.
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
        assert_eq!(pts, [(-1000, 0), (2000, -500), (3000, 3000), (1000, 1000)]);
        assert_eq!((width.as_i32(), height.as_i32()), (4000, 3000));
        assert_eq!(offset(children(&g.unwrap())[0]), (1000, 2000));
    }

    /// Pictures are group children: a picture beside a rect and a group of
    /// pictures only are kept whole. Without the image's asset the picture
    /// is dropped with the usual missing-asset warning, and a group left
    /// with nothing is reported.
    ///
    /// 이것을 실패시키는 것: 그림을 미지원 종류로 되돌리는 것(`pic` 손실) / 자산 없는
    /// 그림을 그대로 두는 것.
    #[test]
    fn pictures_are_kept_and_one_without_its_image_is_dropped() {
        let mixed = group(
            geo(0, 0, 200, 100),
            at(200, 100, 0.0, 0.0),
            vec![child(picture(), at(100, 100, 0.0, 0.0)), child(rect(), at(100, 100, 100.0, 0.0))],
        );
        let (g, lost) = project(&mixed);
        assert!(lost.is_empty(), "{lost:?}");
        let Some(Control::Group { children, .. }) = g else { panic!() };
        assert!(matches!(children[0], GroupChild::Picture(_)), "{children:?}");
        assert!(matches!(children[1], GroupChild::Shape(Control::Rect { .. })), "{children:?}");

        let only = group(
            geo(0, 0, 200, 100),
            at(200, 100, 0.0, 0.0),
            vec![
                child(picture(), at(100, 100, 0.0, 0.0)),
                child(picture(), at(100, 100, 100.0, 0.0)),
            ],
        );
        let (g, lost) = project(&only);
        assert!(lost.is_empty(), "{lost:?}");
        let Some(Control::Group { children, .. }) = g else { panic!() };
        assert_eq!(children.len(), 2);

        let mut state =
            ProjectionImageState::new(None, None, DecompressBudget::new(MAX_TOTAL_DECOMPRESSED));
        assert!(project_group_run(&only, &mut state).is_none());
        let dropped = state
            .warnings
            .iter()
            .filter(|w| matches!(w, Hwp5Warning::DroppedControl { control: "image", .. }))
            .count();
        assert_eq!(dropped, 2, "{:?}", state.warnings);
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
            (rotation(children(&g.unwrap())[0]), lost)
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
            original_size: None,
            ctrl_properties: 0,
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
        assert!(project(&with_pic).1.is_empty());
    }

    /// A nested group whose own box is 10 off its shapes is reported; with an
    /// OLE object (a child Core groups do not hold) under it the boxes are not
    /// compared.
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
                child(ole(), at(10, 10, 2600.0, 3400.0)),
            ],
        );
        let top = group(geo(0, 0, 4000, 4000), at(4000, 4000, 0.0, 0.0), vec![n]);
        assert_eq!(project(&top).1, ["ole"]);
    }

    /// A `ShapeComponent` child payload (type tag once): the 48-byte header,
    /// `trans`, the pairs, then `tail`.
    fn record_bytes(level: u16, initial: (u32, u32), mats: &[Hwp5Matrix], tail: &[u8]) -> Vec<u8> {
        let mut d = b"lop$".to_vec();
        d.extend_from_slice(&[0u8; 8]);
        d.extend_from_slice(&level.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&initial.0.to_le_bytes());
        d.extend_from_slice(&initial.1.to_le_bytes());
        d.extend_from_slice(&[0u8; 22]);
        let pairs = u16::try_from((mats.len() - 1) / 2).unwrap();
        d.extend_from_slice(&pairs.to_le_bytes());
        for m in mats {
            for v in m {
                d.extend_from_slice(&v.to_le_bytes());
            }
        }
        d.extend_from_slice(tail);
        d
    }

    /// The shared pair-order vector (`tests/fixtures/shapes/group-chain-vectors.json`,
    /// read by the HWPX side for its XML form): written as HWP5 record bytes,
    /// parsed and composed, each vector gives its expected matrix — the
    /// scale inside a pair applies before the turn.
    ///
    /// 이것을 실패시키는 것: 한 쌍 안에서 sca 를 rot 뒤에 곱하는 것(`S·R`).
    /// (바깥 쌍이 항등이라 쌍 순서 변이는 잡지 않는다 — F1 고정 테스트가 잡음.)
    #[test]
    fn shared_chain_vectors_compose_as_expected() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/shapes/group-chain-vectors.json"
        );
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let m6 = |v: &serde_json::Value| -> Hwp5Matrix {
            let a: Vec<f64> = v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
            [a[0], a[1], a[2], a[3], a[4], a[5]]
        };
        let vectors = json["vectors"].as_array().unwrap();
        assert!(!vectors.is_empty());
        for v in vectors {
            let mut mats = vec![m6(&v["trans"])];
            for pair in v["pairs"].as_array().unwrap() {
                mats.push(m6(&pair["sca"]));
                mats.push(m6(&pair["rot"]));
            }
            let level = u16::try_from(v["group_level"].as_u64().unwrap()).unwrap();
            let data = record_bytes(level, (100, 100), &mats, &[]);
            let render = Hwp5ShapeRender::parse(&data, false).unwrap();
            let got = chain(&render).matrix();
            let want = m6(&v["expected"]);
            for (g, w) in got.iter().zip(&want) {
                assert!((g - w).abs() < 1e-9, "{}: {got:?} vs {want:?}", v["name"]);
            }
        }
    }

    /// Solid and pattern fills share the kind bits and differ only in the
    /// pattern kind: read from record bytes and projected, the solid
    /// triangle folds its 30° turn into its points (no warning), the
    /// patterned one keeps the turn and reports the fill. Neither carries a
    /// fill in Core — HWP5 shape fills are not projected — so the loss the
    /// warning names is real.
    ///
    /// 이것을 실패시키는 것: 무늬 종류를 보지 않는 것(두 기록이 같은 결과),
    /// 채우기 경고를 지우는 것.
    #[test]
    fn solid_and_pattern_records_fold_differently() {
        let tail = |pattern: i32| {
            let mut t = vec![0u8; 13];
            t.extend_from_slice(&1u32.to_le_bytes());
            t.extend_from_slice(&[0u8; 8]);
            t.extend_from_slice(&pattern.to_le_bytes());
            t.push(0);
            t
        };
        let fold = |pattern: i32| {
            let data = record_bytes(
                1,
                (1000, 1000),
                &[tr(1000.0, 1000.0), ID, ID, ID, rot(30.0)],
                &tail(pattern),
            );
            let render = Hwp5ShapeRender::parse(&data, false).map_err(|e| e.to_string());
            let top = group(
                geo(0, 0, 3000, 3000),
                at(3000, 3000, 0.0, 0.0),
                vec![child(polygon(&[(0, 0), (1000, 0), (0, 1000)]), render)],
            );
            let (g, lost) = project(&top);
            let g = g.unwrap();
            let Control::Polygon { style, .. } = &children(&g)[0] else { panic!() };
            assert!(style.as_ref().and_then(|s| s.fill.as_ref()).is_none());
            (rotation(children(&g)[0]), lost)
        };
        let (r, lost) = fold(-1);
        assert_eq!((r, lost.len()), (None, 0), "solid");
        let (r, lost) = fold(3);
        assert!(r.is_some_and(|a| (a - 30.0).abs() < 1e-3), "pattern: {r:?}");
        assert_eq!(lost, ["group_child_fill"]);
    }

    /// A nested group far from the outer origin: its children are folded
    /// against a provisional origin near their own corner, so a child at
    /// outer x = 110,000,000 (beyond the HWPUNIT range there) but
    /// 20,000,000 from its group is kept: nested offset 90,000,000, child
    /// offsets (0, 0) and (20,000,000, 0).
    ///
    /// 이것을 실패시키는 것: 중첩 묶음 자식을 바깥 원점 기준으로 접고 나서 옮기는 것
    /// (자식이 범위 밖으로 버려진다).
    #[test]
    fn nested_child_far_out_but_near_its_group_is_kept() {
        let n = nested(
            at(20_001_000, 1000, 90_000_000.0, 0.0),
            vec![
                child(rect(), at(1000, 1000, 90_000_000.0, 0.0)),
                child(rect(), at(1000, 1000, 110_000_000.0, 0.0)),
            ],
        );
        let top = group(geo(0, 0, 0, 0), Err("not used".into()), vec![n]);
        let (g, lost) = project(&top);
        assert_eq!(lost, ["group_transform", "group_display_size"]);
        let g = g.unwrap();
        let [n] = children(&g)[..] else { panic!() };
        assert_eq!(offset(n), (90_000_000, 0));
        let offs: Vec<_> = children(n).into_iter().map(offset).collect();
        assert_eq!(offs, [(0, 0), (20_000_000, 0)]);
    }

    /// A polygon's text is not carried, but it turned with the polygon in
    /// the source, so the polygon folds by the box rule: a 30° turn stays a
    /// rotation (the point rule would bake it into the points), and a shear
    /// is reported (the point rule would carry it silently in the points).
    ///
    /// 이것을 실패시키는 것: 기하 사실을 채우기에서만 읽는 것(버린 글자를 빼먹어 점
    /// 규칙으로 접힘).
    #[test]
    fn polygon_with_dropped_text_still_folds_as_a_box() {
        let fold = |pair: (Hwp5Matrix, Hwp5Matrix)| {
            let mut poly = child(
                polygon(&[(0, 0), (1000, 0), (0, 1000)]),
                Ok(render((1000, 1000), tr(1000.0, 1000.0), pair)),
            );
            poly.paragraphs.push(paragraph("dropped"));
            let top = group(geo(0, 0, 3000, 3000), at(3000, 3000, 0.0, 0.0), vec![poly]);
            let (g, lost) = project(&top);
            (g.map(|g| rotation(children(&g)[0])), lost)
        };
        let (r, lost) = fold((ID, rot(30.0)));
        assert!(r.flatten().is_some_and(|a| (a - 30.0).abs() < 1e-3), "{r:?}");
        assert_eq!(lost, ["group_child_text"]);
        let shear = [1.0, 0.5, 0.0, 0.0, 1.0, 0.0];
        let (_, lost) = fold((ID, shear));
        assert_eq!(lost, ["group_child_text", "group_child_transform"]);
    }

    /// A nested group's Core box is the bounds of its children as written,
    /// not of their stored boxes: a mirrored polygon whose points fill only
    /// the lower left of its 1000 × 1000 box at (2000, 0) puts the group at
    /// (2000, 500) and the polygon at the group's corner.
    ///
    /// 이것을 실패시키는 것: 최종 경계로 자식 도형을 옮기지 않는 것(다각형 offset 이
    /// (500, 500) 으로 남아 두 번 옮겨진다), 안쪽 묶음 offset 을 옮기지 않는 것.
    #[test]
    fn nested_group_box_is_the_bounds_of_its_written_children() {
        let n = nested(
            at(1000, 1000, 2000.0, 0.0),
            vec![child(
                polygon(&[(500, 500), (1000, 500), (1000, 1000)]),
                // Mirrored left-right (S20: an axis-aligned polygon keeps its
                // initial box; a mirrored one takes the map onto its points).
                Ok(render((1000, 1000), tr(2000.0, 0.0), (ID, [-1.0, 0.0, 1000.0, 0.0, 1.0, 0.0]))),
            )],
        );
        let top = group(geo(0, 0, 3000, 1000), at(3000, 1000, 0.0, 0.0), vec![n]);
        let (g, _) = project(&top);
        let g = g.unwrap();
        let [n] = children(&g)[..] else { panic!() };
        assert_eq!(offset(n), (2000, 500));
        assert_eq!(offset(children(n)[0]), (0, 0));

        // The same one level deeper: the inner group moves with its parent.
        let inner = nested(
            at(1000, 1000, 2000.0, 0.0),
            vec![child(
                polygon(&[(500, 500), (1000, 500), (1000, 1000)]),
                // Mirrored left-right (S20: an axis-aligned polygon keeps its
                // initial box; a mirrored one takes the map onto its points).
                Ok(render((1000, 1000), tr(2000.0, 0.0), (ID, [-1.0, 0.0, 1000.0, 0.0, 1.0, 0.0]))),
            )],
        );
        let outer = nested(at(1000, 1000, 2000.0, 0.0), vec![inner]);
        let top = group(geo(0, 0, 3000, 1000), at(3000, 1000, 0.0, 0.0), vec![outer]);
        let (g, _) = project(&top);
        let g = g.unwrap();
        let [outer] = children(&g)[..] else { panic!() };
        assert_eq!(offset(outer), (2000, 500));
        let [inner] = children(outer)[..] else { panic!() };
        assert_eq!(offset(inner), (0, 0));
    }

    /// A nested group far beyond the HWPUNIT range (x = 3,000,000,000) is
    /// dropped and reported — the offset is corrected and range-checked in
    /// f64, not added up in `i32` (which overflowed).
    ///
    /// 이것을 실패시키는 것: 원점 차를 범위 검사 전에 `i32` 로 바꿔 보정량을 더하는
    /// 것(overflow panic), 범위 검사를 지우는 것.
    #[test]
    fn nested_group_beyond_the_range_is_reported_not_overflowed() {
        let n = nested(
            at(1000, 1000, 3_000_000_000.0, 0.0),
            vec![child(
                polygon(&[(500, 500), (1000, 500), (1000, 1000)]),
                at(1000, 1000, 3_000_000_000.0, 0.0),
            )],
        );
        let top = group(
            geo(0, 0, 1000, 1000),
            at(1000, 1000, 0.0, 0.0),
            vec![child(rect(), at(1000, 1000, 0.0, 0.0)), n],
        );
        let (g, lost) = project(&top);
        assert_eq!(lost, ["group_display_size", "nested_group_placement"]);
        assert_eq!(children(&g.unwrap()).len(), 1);
    }

    /// A nested group that is dropped still reports its children's own
    /// losses, each at its place: an OLE-only nested group reports both
    /// objects before the group, a nested group whose only child collapses
    /// reports the collapse.
    ///
    /// 이것을 실패시키는 것: 버려진 중첩 묶음의 자식 계획을 재생하지 않는 것(자식별
    /// 손실이 사라지고 `group` 만 남는다), 재생이 한 단계 아래로 내려가지 않는 것.
    #[test]
    fn dropped_nested_group_still_reports_its_children() {
        let mut state =
            ProjectionImageState::new(None, None, DecompressBudget::new(MAX_TOTAL_DECOMPRESSED));
        let flat = [0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let pics = nested(
            at(200, 100, 0.0, 0.0),
            vec![child(ole(), at(100, 100, 0.0, 0.0)), child(ole(), at(100, 100, 100.0, 0.0))],
        );
        let collapsed = nested(
            at(100, 100, 0.0, 0.0),
            vec![child(rect(), Ok(render((100, 100), ID, (flat, ID))))],
        );
        // One level deeper: a dropped group holding a picture-only group.
        let deeper = nested(
            at(100, 100, 0.0, 0.0),
            vec![nested(at(100, 100, 0.0, 0.0), vec![child(ole(), at(100, 100, 0.0, 0.0))])],
        );
        let top = group(
            geo(0, 0, 200, 100),
            at(200, 100, 0.0, 0.0),
            vec![child(rect(), at(100, 100, 0.0, 0.0)), pics, collapsed, deeper],
        );
        assert!(project_group_run(&top, &mut state).is_some());
        let lost: Vec<(String, String)> = state
            .warnings
            .iter()
            .filter_map(|w| match w {
                Hwp5Warning::ContentLost { kind, location, .. } => {
                    Some((kind.clone(), location.clone()))
                }
                _ => None,
            })
            .collect();
        let want = [
            ("ole", "group[0].child[1].child[0]"),
            ("ole", "group[0].child[1].child[1]"),
            ("group", "group[0].child[1]"),
            // Its stored box is 100 wide; its only shape is flattened to 0.
            ("nested_group_placement", "group[0].child[2]"),
            ("group_child_degenerate", "group[0].child[2].child[0]"),
            ("group", "group[0].child[2]"),
            ("ole", "group[0].child[3].child[0].child[0]"),
            ("group", "group[0].child[3].child[0]"),
            ("group", "group[0].child[3]"),
        ];
        let want: Vec<(String, String)> =
            want.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect();
        assert_eq!(lost, want);
    }

    /// A one-picture group whose picture record is `edit`ed first.
    fn picture_group(edit: impl FnOnce(&mut Hwp5ImageControl)) -> Hwp5GroupControl {
        let mut pic = picture();
        let Hwp5Control::Image(image) = &mut pic else { unreachable!() };
        edit(image);
        group(geo(0, 0, 100, 100), at(100, 100, 0.0, 0.0), vec![child(pic, at(100, 100, 0.0, 0.0))])
    }

    /// Branch review B-H3: a group of pictures of one asset stores it once,
    /// and (r2 H3) reads its image header once — for the group plan and for
    /// every picture's crop together.
    /// 이것을 실패시키는 것: 참조마다 복사하기(그림 수만큼 복사) · 크기 캐시 없이
    /// 읽기(계획과 방출에서 그림마다 두 번).
    #[test]
    fn an_asset_is_copied_once() {
        super::super::image_size::PARSES.with(|n| n.set(0));
        let pictures: Vec<Hwp5GroupChild> =
            (0..20).map(|_| child(picture(), at(100, 100, 0.0, 0.0))).collect();
        let top = group(geo(0, 0, 100, 100), at(100, 100, 0.0, 0.0), pictures);
        super::super::ASSET_COPIES.with(|n| n.set(0));
        let (g, lost) = project(&top);
        assert!(lost.is_empty(), "{lost:?}");
        let Some(Control::Group { children, .. }) = g else { panic!("group") };
        assert_eq!(children.len(), 20);
        assert_eq!(super::super::ASSET_COPIES.with(std::cell::Cell::get), 1);
        assert_eq!(super::super::image_size::PARSES.with(std::cell::Cell::get), 1);
    }

    /// A group picture's crop is carried (not reported): the record's crop
    /// with its image size, nothing for the whole image.
    ///
    /// 이것을 실패시키는 것: `build_group_picture` 가 자르기를 싣지 않는 것 /
    /// 자르기를 다시 손실로 보고하는 것.
    #[test]
    fn group_picture_crop_is_carried() {
        let crop = |c: [i32; 4]| {
            picture_group(|i| {
                i.picture.crop = c;
                i.picture.dimension = Some((22_500, 15_000));
            })
        };
        let picture_crop = |g: &Option<Control>| match g {
            Some(Control::Group { children, .. }) => match &children[0] {
                GroupChild::Picture(img) => {
                    img.crop.map(|c| [c.left, c.top, c.right, c.bottom].map(|v| v.as_i32()))
                }
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        };
        let (g, lost) = project(&crop([0, 0, 7474, 15_000]));
        assert!(lost.is_empty(), "{lost:?}");
        assert_eq!(picture_crop(&g), Some([0, 0, 7474, 15_000]));
        let (g, lost) = project(&crop([0, 0, 22_500, 15_000]));
        assert!(lost.is_empty(), "{lost:?}");
        assert_eq!(picture_crop(&g), None);
    }

    /// Effects and a border are reported; a plain picture is not.
    ///
    /// 이것을 실패시키는 것: 밝기·명암·효과·효과 플래그·테두리 조건을 각각 지우는 것.
    #[test]
    fn group_picture_effect_and_border_rule() {
        type Edit = fn(&mut Hwp5ImageControl);
        let edits: [Edit; 5] = [
            |i| i.picture.brightness = -10,
            |i| i.picture.contrast = 5,
            |i| i.picture.effect = 1,
            |i| i.picture.effect_flags = Some(4),
            |i| i.picture.border_thickness = 28,
        ];
        for (n, edit) in edits.into_iter().enumerate() {
            assert_eq!(project(&picture_group(edit)).1, ["group_child_picture"], "edit {n}");
        }
        assert!(project(&picture_group(|_| {})).1.is_empty());
    }

    /// A group picture keeps its link; a link record in another form is
    /// reported. A group's own link (top-level or nested) is reported.
    ///
    /// 이것을 실패시키는 것: 그림 링크를 싣지 않는 것 / 형식 밖 링크 경고 삭제 /
    /// 최상위·중첩 묶음 링크 경고를 각각 지우는 것.
    #[test]
    fn group_picture_and_group_links() {
        let linked = picture_group(|i| i.link = Some(Ok("http://a.kr;1;0;0;".to_string())));
        let (g, lost) = project(&linked);
        assert!(lost.is_empty(), "{lost:?}");
        let Some(Control::Group { children, .. }) = g else { panic!() };
        let GroupChild::Picture(image) = &children[0] else { panic!() };
        assert_eq!(image.hyperlink.as_deref(), Some("http://a.kr"));

        let odd = picture_group(|i| i.link = Some(Ok("http://a.kr".to_string())));
        assert_eq!(project(&odd).1, ["picture_hyperlink"]);

        let mut top = picture_group(|_| {});
        top.has_link = true;
        assert_eq!(project(&top).1, ["group_hyperlink"]);

        let mut inner = group(
            geo(0, 0, 0, 0),
            at(100, 100, 0.0, 0.0),
            vec![child(rect(), at(100, 100, 0.0, 0.0))],
        );
        inner.has_link = true;
        let outer = group(
            geo(0, 0, 100, 100),
            at(100, 100, 0.0, 0.0),
            vec![child(Hwp5Control::Group(inner), Ok(render_stub()))],
        );
        assert_eq!(project(&outer).1, ["group_hyperlink"]);
    }

    /// Text and controls recorded under a picture child are reported, as
    /// for any other child, and the picture is still kept.
    ///
    /// 이것을 실패시키는 것: 그림 갈래에서 중첩 컨트롤 보고 삭제 / 글자 보고 삭제
    /// (각각 따로).
    #[test]
    fn text_and_controls_under_a_picture_are_reported() {
        let mut pic = child(picture(), at(100, 100, 0.0, 0.0));
        pic.paragraphs.push(paragraph("그림 설명"));
        pic.nested_controls.push(CTRL_ID_TABLE);
        pic.nested_controls.push(u32::from_be_bytes(*b"%hlk"));
        let top = group(geo(0, 0, 100, 100), at(100, 100, 0.0, 0.0), vec![pic]);
        let (g, lost) = project(&top);
        assert_eq!(lost, ["group_nested_object", "group_child_control", "group_child_text"]);
        let Some(Control::Group { children, .. }) = g else { panic!() };
        assert!(matches!(children[0], GroupChild::Picture(_)));
    }
}
