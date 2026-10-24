//! Decodes HWPX shape elements into Core control types.
//!
//! Split from `section.rs` to enable parallel development of shape features.
//! Functions here convert `HxRect`, `HxLine`, `HxEllipse`, and `HxPolygon`
//! into Core `Run` values with the appropriate `Control` variant.

use hwpforge_core::control::geometry::FramePlan;
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

/// Largest translation-free distance between two lists of positions taken
/// in the same order, anchored at their first entry (the original box's
/// top-left corner).
fn anchored_residual(a: &[(f64, f64)], b: &[(f64, f64)]) -> f64 {
    let (Some(a0), Some(b0)) = (a.first(), b.first()) else { return 0.0 };
    if a.len() != b.len() {
        return f64::INFINITY;
    }
    a.iter()
        .zip(b)
        .map(|(p, q)| ((p.0 - a0.0) - (q.0 - b0.0)).hypot((p.1 - a0.1) - (q.1 - b0.1)))
        .fold(0.0, f64::max)
}

/// The source's map of an object's stored box: its `(sca, rot)` pairs
/// without the translation (the object is placed by `<hp:pos>`). An object
/// whose Core kind has no rotation or mirror (`with_rotation` false) is
/// judged on the scale's size only: a mirror kept in a negative scale is a
/// separate loss, not a frame residual (§19.23.20). No `<hp:renderingInfo>`
/// is the identity.
fn source_map(
    ri: Option<&crate::schema::section::HxRenderingInfo>,
    with_rotation: bool,
) -> Option<super::group_geometry::Affine> {
    use super::group_geometry::{pair_chain, Affine};
    let Some(ri) = ri else { return Some(Affine::IDENTITY) };
    let m = pair_chain(ri, with_rotation)?;
    Some(if with_rotation { m } else { Affine { a: m.a.abs(), e: m.e.abs(), c: 0.0, f: 0.0, ..m } })
}

/// Why an `<hp:orgSz>` cannot become a frame value, if it cannot: a negative
/// side (no reading of one by Hancom is known — §19.23.15), an axis of 0 under
/// a displayed axis that is not (`validate` would refuse it), or a side out
/// of the HWPUNIT range.
fn unusable_original(org: (i32, i32), displayed: (i32, i32)) -> Option<String> {
    let (ow, oh) = org;
    if ow < 0 || oh < 0 {
        return Some(format!("negative orgSz {ow}x{oh} is not carried"));
    }
    if (ow == 0 && displayed.0 != 0) || (oh == 0 && displayed.1 != 0) {
        return Some(format!(
            "orgSz {ow}x{oh} has an axis of 0 under the displayed {}x{}",
            displayed.0, displayed.1
        ));
    }
    if HwpUnit::new(ow).is_err() || HwpUnit::new(oh).is_err() {
        return Some(format!("orgSz {ow}x{oh} is out of range"));
    }
    None
}

/// What the re-encode of an object with this frame lands against the source:
/// the decoder keeps the frame when the writer's plan reproduces the source
/// within 2 HWPUNIT; otherwise it tries the object without its original box.
///
/// - The plan without it is within 2: the original size is dropped and
///   reported (`object_original_size`).
/// - Neither is within 2 (the source draws a size other than its layout size,
///   §19.23.13 H1): reported as `shape_drawn_extent`. **The frame is kept**
///   (§19.23.20, decision pending): the drawn result is the same either way
///   (both write the layout size), and dropping it would lose `orgSz` values
///   the decoder carried before — design §19.23.17 drops it.
enum FrameCheck {
    Kept,
    Dropped(String),
    DrawnExtent(String),
}

fn check_frame(
    kept: &[(f64, f64)],
    dropped: &[(f64, f64)],
    source: &[(f64, f64)],
    extent: (f64, f64),
    displayed: (i32, i32),
    org: (i32, i32),
) -> FrameCheck {
    use super::group_geometry::EXACT_TOLERANCE;
    let with = anchored_residual(kept, source);
    // 이것을 실패시키는 것: 계획 대신 Core 정확값과 비교하기(2.49 반례가 frame 을
    // 둔 채 1000 으로 써진다 — `accumulated_rounding_drops_the_frame`).
    if with <= EXACT_TOLERANCE {
        return FrameCheck::Kept;
    }
    let without = anchored_residual(dropped, source);
    if without <= EXACT_TOLERANCE {
        return FrameCheck::Dropped(format!(
            "orgSz {}x{} is dropped: with it the re-encode lands {with:.2} from the source, \
             without it {without:.2}",
            org.0, org.1
        ));
    }
    FrameCheck::DrawnExtent(format!(
        "matrix extent {:.0}x{:.0} differs from layout size {}x{} by {without:.2}",
        extent.0, extent.1, displayed.0, displayed.1
    ))
}

/// The displayed width and height of a control with a frame.
fn displayed_size(control: &Control) -> Option<(i32, i32)> {
    match control {
        Control::TextBox { width, height, .. }
        | Control::Line { width, height, .. }
        | Control::Ellipse { width, height, .. }
        | Control::Rect { width, height, .. }
        | Control::Polygon { width, height, .. }
        | Control::Arc { width, height, .. }
        | Control::Curve { width, height, .. }
        | Control::ConnectLine { width, height, .. }
        | Control::Group { width, height, .. }
        | Control::TextArt { width, height, .. } => Some((width.as_i32(), height.as_i32())),
        _ => None,
    }
}

/// Classifies the complete chain, deliberately ignoring angle/flip/offset caches.
fn standalone_map(
    ri: Option<&crate::schema::section::HxRenderingInfo>,
) -> Result<Option<super::group_geometry::Affine>, &'static str> {
    let Some(ri) = ri else { return Ok(None) };
    let m =
        super::group_geometry::wire_chain(ri).ok_or("unparseable or nonfinite renderingInfo")?;
    if ![m.a, m.b, m.c, m.d, m.e, m.f].iter().all(|v| v.is_finite()) {
        return Err("composed renderingInfo is nonfinite");
    }
    Ok((!(m.b.abs() < 1e-9 && m.d.abs() < 1e-9 && m.a >= 0.0 && m.e >= 0.0)).then_some(m))
}

fn clear_standalone_transform(control: &mut Control) {
    let style = match control {
        Control::TextBox { style, .. }
        | Control::Rect { style, .. }
        | Control::Ellipse { style, .. }
        | Control::Arc { style, .. }
        | Control::Polygon { style, .. }
        | Control::Curve { style, .. }
        | Control::Line { style, .. }
        | Control::ConnectLine { style, .. } => style,
        _ => return,
    };
    if let Some(s) = style {
        s.rotation = None;
        s.flip = None;
    }
    if style.as_ref().is_some_and(|s| *s == ShapeStyle::default()) {
        *style = None;
    }
}

fn check_standalone_source(
    map: super::group_geometry::Affine,
    original: (i32, i32),
    declared_and_margin: ((i32, i32), [i32; 4]),
    plan: &crate::encoder::standalone::StandalonePlan,
    points: &[hwpforge_core::control::ShapePoint],
    kind: &str,
    ctx: &mut DecodeCtx,
) {
    let (declared, margin) = declared_and_margin;
    // Validate margins before reaching here; widen before sums and differences.
    let content = (
        i64::from(declared.0) - i64::from(margin[0]) - i64::from(margin[1]),
        i64::from(declared.1) - i64::from(margin[2]) - i64::from(margin[3]),
    );
    let output = (
        i64::from(plan.size.0) + i64::from(margin[0]) + i64::from(margin[1]),
        i64::from(plan.size.1) + i64::from(margin[2]) + i64::from(margin[3]),
    );
    let corners = super::group_geometry::corners(f64::from(original.0), f64::from(original.1));
    let source: Vec<_> = corners.iter().map(|&p| map.apply(p)).collect();
    let b = super::group_geometry::bounds(&source).expect("four corners");
    let error =
        b.0.abs()
            .max(b.1.abs())
            .max((b.2 - content.0 as f64).abs())
            .max((b.3 - content.1 as f64).abs());
    // 이것을 실패시키는 것: offset을 검사 / transMatrix 이동을 버리기 / 2.0 임계값.
    if error > 2.5 {
        ctx.content_lost("shape_transform",format!(
            "G7; {kind}; source sz={}x{}, composite bounds={b:?}, max boundary error={error}; recomputed sz={:?}, transMatrix=({},{}); normalized source matrix placement mismatch",
            declared.0,declared.1,output,plan.rendering.trans_matrix.e3,plan.rendering.trans_matrix.e6));
    }
    let ps = corners.into_iter().chain(points.iter().map(|p| (f64::from(p.x), f64::from(p.y))));
    let source: Vec<_> = ps.clone().map(|p| map.apply(p)).collect();
    let emitted: Vec<_> = ps.map(|p| plan.map.apply(p)).collect();
    let residual = anchored_residual(&source, &emitted);
    if residual > 2.0 {
        ctx.content_lost(
            "shape_emission",
            format!("{kind}: actual T*R*S residual {residual} HWPUNIT"),
        );
    }
}

fn report_unturned_metadata(kind: &str, metadata: (i32, u32, u32), ctx: &mut DecodeCtx) {
    let (angle, horizontal, vertical) = metadata;
    if angle.rem_euclid(360) != 0 || horizontal != 0 || vertical != 0 {
        ctx.content_lost("shape_transform", format!(
            "metadata mismatch; {kind}; source angle={angle}, flip=({horizontal},{vertical}); composite matrix has no rotation or reflection; discarded rotation/flip metadata"
        ));
    }
}

fn settle_standalone_shape(
    control: &mut Control,
    source_id: &str,
    metadata: (i32, u32, u32),
    org: Option<&crate::schema::section::HxSizeAttr>,
    ri: Option<&crate::schema::section::HxRenderingInfo>,
    ctx: &mut DecodeCtx,
) -> bool {
    use hwpforge_core::control::geometry::{
        defining_points, fold_standalone, frame_plan, FoldInput,
    };
    if !matches!(
        control,
        Control::TextBox { .. }
            | Control::Rect { .. }
            | Control::Ellipse { .. }
            | Control::Arc { .. }
            | Control::Polygon { .. }
            | Control::Curve { .. }
            | Control::Line { .. }
            | Control::ConnectLine { .. }
    ) {
        return false;
    }
    let map = match standalone_map(ri) {
        Ok(None) => {
            // Appendix B: only the complete matrix defines geometry. Missing
            // renderingInfo keeps the legacy metadata path (no matrix to compare).
            if ri.is_some() {
                report_unturned_metadata(
                    &format!("{} id={source_id}", control.kind_name()),
                    metadata,
                    ctx,
                );
                clear_standalone_transform(control);
            }
            return false;
        }
        Ok(Some(m)) => m,
        Err(e) => {
            clear_standalone_transform(control);
            ctx.content_lost("shape_transform", e);
            return true;
        }
    };
    let declared = displayed_size(control).expect("shape size");
    let Some(org) = org else {
        clear_standalone_transform(control);
        ctx.content_lost("shape_transform", "missing orgSz");
        return true;
    };
    let original = (org.width, org.height);
    let input = FoldInput::new(
        map.to_transform(),
        (f64::from(org.width), f64::from(org.height)),
        (0.0, 0.0),
    );
    match fold_standalone(control, &input) {
        Ok(_) => {
            let frame =
                frame_plan(control, &crate::encoder::shapes::HWPX_POLICY).expect("shape frame");
            let style = match control {
                Control::TextBox { style, .. }
                | Control::Rect { style, .. }
                | Control::Ellipse { style, .. }
                | Control::Arc { style, .. }
                | Control::Polygon { style, .. }
                | Control::Curve { style, .. }
                | Control::Line { style, .. }
                | Control::ConnectLine { style, .. } => style.as_ref(),
                _ => None,
            };
            let (rotation, flip) = style.map_or((None, None), |s| (s.rotation, s.flip));
            let plan = crate::encoder::standalone::plan(
                displayed_size(control).unwrap(),
                rotation,
                flip,
                &frame,
            );
            check_standalone_source(
                map,
                original,
                (declared, [0; 4]),
                &plan,
                &defining_points(control),
                &format!("{} id={source_id}", control.kind_name()),
                ctx,
            );
        }
        Err(e) => {
            clear_standalone_transform(control);
            if unusable_original(original, declared).is_none() {
                if let Some(frame) = control.frame_mut() {
                    frame.original_size = Some(hwpforge_foundation::Size::new(
                        HwpUnit::new(original.0).expect("checked original"),
                        HwpUnit::new(original.1).expect("checked original"),
                    ));
                }
            }
            ctx.content_lost("shape_transform", format!("{}: {e}", control.kind_name()));
        }
    }
    true
}

/// S20: a top-level shape's original box and size lock from the wire
/// (`<hp:orgSz>`, `<hp:sz protect>`), checked against what the writer will
/// store ([`FrameCheck`]). Its defining points stay as read: they are
/// coordinates in the original box. A group's direct children take their
/// frame with the group, not here.
///
/// 이것을 실패시키는 것: 이 호출을 지우기(`r6_circle_keeps_its_original_box`) /
/// 잠금을 읽지 않기(같은 테스트) / frame 을 버릴 때 잠금까지 지우기
/// (`dropping_the_original_box_keeps_the_lock`).
pub(crate) fn settle_shape_frame(
    control: &mut Control,
    source_id: &str,
    metadata: (i32, u32, u32),
    org: Option<&crate::schema::section::HxSizeAttr>,
    sz: Option<&crate::schema::section::HxTableSz>,
    ri: Option<&crate::schema::section::HxRenderingInfo>,
    ctx: &mut DecodeCtx,
) {
    use crate::encoder::shapes::HWPX_POLICY;
    use hwpforge_core::control::geometry::{defining_points, drop_original_size, frame_plan};
    if ctx.is_direct_group_child() {
        return;
    }
    if let Some(frame) = control.frame_mut() {
        frame.size_protect = sz.is_some_and(|s| s.protect != 0);
    }
    if settle_standalone_shape(control, source_id, metadata, org, ri, ctx) {
        return;
    }
    let Some(displayed) = displayed_size(control) else { return };
    let Some(frame) = control.frame_mut() else { return };
    frame.size_protect = sz.is_some_and(|s| s.protect != 0);
    let Some(org) = org.map(|o| (o.width, o.height)) else { return };
    if let Some(reason) = unusable_original(org, displayed) {
        ctx.content_lost("object_original_size", reason);
        return;
    }
    let unit = |v: i32| HwpUnit::new(v).unwrap_or(HwpUnit::ZERO);
    let original = hwpforge_foundation::Size::new(unit(org.0), unit(org.1));
    frame.original_size = Some(original);
    let default = match control {
        Control::TextArt { .. } => {
            (hwpforge_core::frame::TEXT_ART_DESIGN_SIDE, hwpforge_core::frame::TEXT_ART_DESIGN_SIDE)
        }
        _ => displayed,
    };
    // The kind's default is no frame value; the object is still judged
    // against the source below (a default box drawn at another scale is a
    // drawn-extent loss, not a frame question).
    // 이것을 실패시키는 것: 기본값일 때 검사 전에 돌아가기
    // (`default_frame_still_reports_the_drawn_extent`).
    if org == default {
        if let Some(frame) = control.frame_mut() {
            frame.original_size = None;
        }
    }
    let turns = !matches!(control, Control::TextArt { .. } | Control::Group { .. });
    let Some(map) = source_map(ri, turns) else { return };
    let source: Vec<(f64, f64)> = [(0.0, 0.0), (f64::from(org.0), 0.0)]
        .into_iter()
        .chain([(f64::from(org.0), f64::from(org.1)), (0.0, f64::from(org.1))])
        .chain(defining_points(control).iter().map(|p| (f64::from(p.x), f64::from(p.y))))
        .map(|p| map.apply(p))
        .collect();
    // r4 M1 (same shape): the copy that tries the dropped box holds only the
    // control's own fields — its text, caption and group children are set
    // aside, the frame plan reads none of them — so a text box holding a
    // chain of groups is not copied whole at every level.
    // 이것을 실패시키는 것: 통째 복제로 되돌리기(`frame_checks_copy_no_text`).
    let contents = take_contents(control);
    let mut dropped = control.clone();
    put_contents(control, contents);
    #[cfg(test)]
    crate::work::tick_n(text_bytes_of(&dropped));
    drop_original_size(&mut dropped);
    let (Some(kept_plan), Some(dropped_plan)) =
        (frame_plan(control, &HWPX_POLICY), frame_plan(&dropped, &HWPX_POLICY))
    else {
        return;
    };
    let extent = (
        (map.a * f64::from(org.0)).hypot(map.d * f64::from(org.0)),
        (map.b * f64::from(org.1)).hypot(map.e * f64::from(org.1)),
    );
    match check_frame(
        kept_plan.stored_positions(),
        dropped_plan.stored_positions(),
        &source,
        extent,
        displayed,
        org,
    ) {
        FrameCheck::Kept => {}
        FrameCheck::Dropped(reason) => {
            let contents = take_contents(control);
            *control = dropped;
            put_contents(control, contents);
            ctx.content_lost("object_original_size", reason);
        }
        FrameCheck::DrawnExtent(reason) => ctx.content_lost("shape_drawn_extent", reason),
    }
}

/// What a control holds besides its own fields: text, caption, group
/// children.
#[derive(Default)]
struct Contents {
    paragraphs: Vec<hwpforge_core::paragraph::Paragraph>,
    caption: Option<hwpforge_core::caption::Caption>,
    children: Vec<GroupChild>,
}

/// Moves `control`'s [`Contents`] out, leaving it empty of them.
fn take_contents(control: &mut Control) -> Contents {
    let mut out = Contents::default();
    match control {
        Control::TextBox { paragraphs, caption, .. }
        | Control::Ellipse { paragraphs, caption, .. }
        | Control::Polygon { paragraphs, caption, .. } => {
            out.paragraphs = std::mem::take(paragraphs);
            out.caption = caption.take();
        }
        Control::Rect { caption, .. }
        | Control::Line { caption, .. }
        | Control::Arc { caption, .. }
        | Control::Curve { caption, .. }
        | Control::ConnectLine { caption, .. } => out.caption = caption.take(),
        Control::Group { children, .. } => out.children = std::mem::take(children),
        _ => {}
    }
    out
}

/// Puts [`take_contents`]'s result back.
fn put_contents(control: &mut Control, contents: Contents) {
    match control {
        Control::TextBox { paragraphs, caption, .. }
        | Control::Ellipse { paragraphs, caption, .. }
        | Control::Polygon { paragraphs, caption, .. } => {
            *paragraphs = contents.paragraphs;
            *caption = contents.caption;
        }
        Control::Rect { caption, .. }
        | Control::Line { caption, .. }
        | Control::Arc { caption, .. }
        | Control::Curve { caption, .. }
        | Control::ConnectLine { caption, .. } => *caption = contents.caption,
        Control::Group { children, .. } => *children = contents.children,
        _ => {}
    }
}

/// Text bytes held in `control`'s paragraphs (tests count what a copy
/// carries).
#[cfg(test)]
fn text_bytes_of(control: &Control) -> usize {
    let mut holder =
        hwpforge_core::paragraph::Paragraph::new(hwpforge_foundation::ParaShapeIndex::new(0));
    holder.runs.push(Run::control(control.clone(), hwpforge_foundation::CharShapeIndex::new(0)));
    let mut bytes = 0;
    holder.for_each_paragraph(|p| {
        bytes += p.runs.iter().filter_map(|r| r.content.as_text()).map(str::len).sum::<usize>();
    });
    bytes
}

/// S20: a top-level picture's size lock, and its original box checked
/// against what the writer will store, as [`settle_shape_frame`] does for a
/// shape (a picture has no points, no rotation or mirror Core can hold: the
/// source is judged on its scale's size). `convert_picture` has set the
/// original box from `<hp:orgSz>` already. A group's pictures are decoded by
/// the group.
///
/// 이것을 실패시키는 것: 이 호출을 지우기(`picture_and_table_protect_round_trip`) /
/// 음수 원래 크기 보고를 지우기(`negative_original_size_is_reported`).
fn settle_standalone_picture(
    image: &mut hwpforge_core::image::Image,
    pic: &crate::schema::section::HxPic,
    ctx: &mut DecodeCtx,
) -> bool {
    use hwpforge_core::control::geometry::{
        fold_standalone_picture, picture_frame_plan, FoldInput,
    };
    let map = match standalone_map(pic.rendering_info.as_ref()) {
        Ok(None) => {
            if pic.rendering_info.is_some() {
                report_unturned_metadata(
                    &format!("picture id={}", pic.id),
                    (
                        pic.rotation_info.as_ref().map_or(0, |r| r.angle),
                        pic.flip.as_ref().map_or(0, |f| f.horizontal),
                        pic.flip.as_ref().map_or(0, |f| f.vertical),
                    ),
                    ctx,
                );
            }
            return false;
        }
        Ok(Some(m)) => m,
        Err(e) => {
            ctx.content_lost("shape_transform", format!("picture id={}: {e}", pic.id));
            return true;
        }
    };
    let Some(org) = pic.org_sz.as_ref() else {
        ctx.content_lost("shape_transform", "picture missing orgSz");
        return true;
    };
    let original = (org.width, org.height);
    let displayed = (image.width.as_i32(), image.height.as_i32());
    if original != displayed && unusable_original(original, displayed).is_none() {
        if let (Ok(w), Ok(h)) = (HwpUnit::new(original.0), HwpUnit::new(original.1)) {
            image.frame.original_size = Some(hwpforge_foundation::Size::new(w, h));
        }
    }
    let margin = pic.in_margin.as_ref().map_or([0; 4], |m| [m.left, m.right, m.top, m.bottom]);
    let rectangular = pic.img_rect.as_ref().is_none_or(|r| {
        [(r.pt0.x, r.pt0.y), (r.pt1.x, r.pt1.y), (r.pt2.x, r.pt2.y), (r.pt3.x, r.pt3.y)]
            == [(0, 0), (org.width, 0), original, (0, org.height)]
    });
    // 이것을 실패시키는 것: 범위 검증 전에 1200000000 여백의 합/차 계산.
    let mut units = [HwpUnit::ZERO; 4];
    for (i, name) in ["left", "right", "top", "bottom"].iter().enumerate() {
        match HwpUnit::new(margin[i]) {
            Ok(value) if margin[i] >= 0 => units[i] = value,
            _ => {
                ctx.content_lost("shape_transform", format!(
                    "picture id={}: invalid inner margin {name}={}; expected 0..={}; using declared sz={}x{} without rotation/flip/inner margin",
                    pic.id, margin[i], HwpUnit::MAX_VALUE, displayed.0, displayed.1
                ));
                return true;
            }
        }
    }
    if margin[0] != margin[1] || margin[2] != margin[3] || !rectangular {
        ctx.content_lost(
            "shape_transform",
            format!(
                "picture id={}: unsupported inner margin {margin:?} or nonrectangular imgRect",
                pic.id
            ),
        );
        return true;
    }
    let input = FoldInput::new(
        map.to_transform(),
        (f64::from(org.width), f64::from(org.height)),
        (0.0, 0.0),
    );
    match fold_standalone_picture(image, &input) {
        Ok(_) => {
            if let Some(dim) = &pic.img_dim {
                if let (Ok(w), Ok(h)) = (HwpUnit::new(dim.dim_width), HwpUnit::new(dim.dim_height))
                {
                    image.image_extent = Some((w, h));
                }
            }
            if margin != [0; 4] {
                image.in_margin = Some(hwpforge_core::table::TableMargin {
                    left: units[0],
                    right: units[1],
                    top: units[2],
                    bottom: units[3],
                });
            }
            let frame = picture_frame_plan(image, &crate::encoder::shapes::HWPX_POLICY);
            let plan = crate::encoder::standalone::plan(
                (image.width.as_i32(), image.height.as_i32()),
                image.rotation,
                image.flip,
                &frame,
            );
            check_standalone_source(
                map,
                original,
                (displayed, margin),
                &plan,
                &[],
                &format!("picture id={}", pic.id),
                ctx,
            );
        }
        Err(e) => ctx.content_lost("shape_transform", format!("picture id={}: {e}", pic.id)),
    }
    true
}

pub(crate) fn settle_picture_frame(
    image: &mut hwpforge_core::image::Image,
    pic: &crate::schema::section::HxPic,
    ctx: &mut DecodeCtx,
) {
    use crate::encoder::shapes::HWPX_POLICY;
    use hwpforge_core::control::geometry::picture_frame_plan;
    if ctx.is_direct_group_child() {
        return;
    }
    image.frame.size_protect = pic.sz.as_ref().is_some_and(|s| s.protect != 0);
    if settle_standalone_picture(image, pic, ctx) {
        return;
    }
    let displayed = (image.width.as_i32(), image.height.as_i32());
    let Some(org) = pic.org_sz.as_ref().map(|o| (o.width, o.height)) else { return };
    if let Some(reason) = unusable_original(org, displayed) {
        ctx.content_lost("object_original_size", reason);
        return;
    }
    // A picture whose original box is its displayed one (no frame value) is
    // still judged against the source: kept and dropped are then the same.
    // 이것을 실패시키는 것: 원래 상자가 없을 때 검사 전에 돌아가기
    // (`mirrored_picture_keeps_its_original_box` 의 둘째 단언).
    let Some(map) = source_map(pic.rendering_info.as_ref(), false) else { return };
    let source: Vec<(f64, f64)> = [
        (0.0, 0.0),
        (f64::from(org.0), 0.0),
        (f64::from(org.0), f64::from(org.1)),
        (0.0, f64::from(org.1)),
    ]
    .into_iter()
    .map(|p| map.apply(p))
    .collect();
    // The dropped box differs only in the frame field, so the plan is taken
    // with it unset and the field put back — no copy of the picture and its
    // caption (see `settle_shape_frame`).
    let kept_plan = picture_frame_plan(image, &HWPX_POLICY);
    let original = image.frame.original_size.take();
    let dropped_plan = picture_frame_plan(image, &HWPX_POLICY);
    image.frame.original_size = original;
    let extent = (map.a * f64::from(org.0), map.e * f64::from(org.1));
    match check_frame(
        kept_plan.stored_positions(),
        dropped_plan.stored_positions(),
        &source,
        extent,
        displayed,
        org,
    ) {
        FrameCheck::Kept => {}
        FrameCheck::Dropped(reason) => {
            image.frame.original_size = None;
            ctx.content_lost("object_original_size", reason);
        }
        FrameCheck::DrawnExtent(reason) => ctx.content_lost("shape_drawn_extent", reason),
    }
}

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
    ctx: &mut DecodeCtx,
) -> Option<ObjectPlacement> {
    let pos = pos?;
    let placement = ObjectPlacement::new(
        if text_wrap.is_empty() {
            ObjectTextWrap::TopAndBottom
        } else {
            ObjectTextWrap::from_hwpx(text_wrap)
        },
        if pos.vert_rel_to.is_empty() {
            ObjectRelativeTo::Para
        } else {
            ObjectRelativeTo::from_hwpx(&pos.vert_rel_to)
        },
        if pos.horz_rel_to.is_empty() {
            ObjectRelativeTo::Para
        } else {
            ObjectRelativeTo::from_hwpx(&pos.horz_rel_to)
        },
    )
    .with_text_flow(if text_flow.is_empty() {
        ObjectTextFlow::BothSides
    } else {
        ObjectTextFlow::from_hwpx(text_flow)
    })
    .with_treat_as_char(pos.treat_as_char != 0)
    .with_flow_with_text(pos.flow_with_text != 0)
    .with_allow_overlap(pos.allow_overlap != 0)
    .with_vert_offset(HwpUnit::new(pos.vert_offset).unwrap_or(HwpUnit::ZERO))
    .with_horz_offset(HwpUnit::new(pos.horz_offset).unwrap_or(HwpUnit::ZERO));
    let placement = with_pos_alignment(placement, pos, ctx);
    if placement == ObjectPlacement::legacy_inline_defaults() {
        None
    } else {
        Some(placement)
    }
}

/// `placement` with the `<hp:pos>` alignment and the two flags Core carries
/// since S1 (`vertAlign`, `horzAlign`, `affectLSpacing`, `holdAnchorAndSO`).
/// An alignment this codec does not know is reported
/// ([`UnknownEnumValue`](super::DecodeWarning::UnknownEnumValue)) and read as
/// top/left; an empty attribute is the default without a report.
///
/// 이것을 실패시키는 것: 이 호출을 지우기(정렬이 재인코드에서 TOP/LEFT 로,
/// `object_alignment_and_flags_round_trip`) / 미지값 경고 삭제.
pub(crate) fn with_pos_alignment(
    placement: ObjectPlacement,
    pos: &HxTablePos,
    ctx: &mut DecodeCtx,
) -> ObjectPlacement {
    use hwpforge_core::placement::{HorzAlign, VertAlign};
    let vert_align = match pos.vert_align.as_str() {
        "" => VertAlign::Top,
        raw => VertAlign::from_hwpx(raw).unwrap_or_else(|| {
            ctx.warnings.push(super::DecodeWarning::UnknownEnumValue {
                attribute: "hp:pos@vertAlign",
                raw: raw.to_string(),
                fallback: "TOP",
            });
            VertAlign::Top
        }),
    };
    let horz_align = match pos.horz_align.as_str() {
        "" => HorzAlign::Left,
        raw => HorzAlign::from_hwpx(raw).unwrap_or_else(|| {
            ctx.warnings.push(super::DecodeWarning::UnknownEnumValue {
                attribute: "hp:pos@horzAlign",
                raw: raw.to_string(),
                fallback: "LEFT",
            });
            HorzAlign::Left
        }),
    };
    placement
        .with_vert_align(vert_align)
        .with_horz_align(horz_align)
        .with_affect_line_spacing(pos.affect_l_spacing != 0)
        .with_hold_anchor_and_so(pos.hold_anchor_and_so != 0)
}

/// Parses an `<hp:subList vertAlign="...">` token into a Core [`VerticalAlign`].
///
/// An empty attribute (한컴 omits it for the default) or any unrecognized value
/// falls back to [`VerticalAlign::Top`], matching the encoder default so that
/// default shapes round-trip byte-for-byte.
fn decode_shape_vert_align(token: &str) -> VerticalAlign {
    token.parse().unwrap_or(VerticalAlign::Top)
}

/// Largest corner ratio Core holds (Hancom's semicircular end).
const MAX_CORNER_RATIO: u8 = 50;

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

    let placement = with_object_out_margin(
        decode_object_placement(rect.pos.as_ref(), &rect.text_wrap, &rect.text_flow, ctx),
        rect.out_margin.as_ref(),
    )?;

    let caption = rect
        .caption
        .as_ref()
        .map(|c| convert_hx_caption(c, Some(width.as_i32()), depth, ctx))
        .transpose()?;

    let style = decode_shape_style(
        &rect.line_shape,
        &rect.fill_brush,
        true,
        rect.rotation_info.as_ref(),
        rect.flip.as_ref(),
        &rect.dropcap_style,
        ctx,
    );
    // S4 (§19.5 1c): 모서리 곡률. 0 = 각진 모서리(`None`), 51 이상은 Core
    // 범위 밖이라 보고하고 50 으로 자른다.
    // 이것을 실패시키는 것: 이 읽기를 지우기(`corner_ratio_round_trips`) /
    // 범위 밖 경고 삭제.
    let style = match rect.ratio {
        0 => style,
        ratio => {
            let mut style = style.unwrap_or_default();
            if ratio > MAX_CORNER_RATIO {
                ctx.warnings.push(super::DecodeWarning::UnknownEnumValue {
                    attribute: "hp:rect@ratio",
                    raw: ratio.to_string(),
                    fallback: "50",
                });
            }
            style.corner_ratio = Some(ratio.min(MAX_CORNER_RATIO));
            Some(style)
        }
    };

    let control = match &rect.draw_text {
        Some(draw_text) => {
            ctx.enter(super::PathSeg::TextBox);
            let result = decode_sublist_paragraphs(&draw_text.sub_list, depth, ctx);
            ctx.leave();
            let paragraphs = result?;
            let text_vertical_align = decode_shape_vert_align(&draw_text.sub_list.vert_align);
            Control::TextBox {
                frame: hwpforge_core::ObjectFrame::new(),
                // §19.22 D4: the wire value, also when it is 283 everywhere.
                // 이것을 실패시키는 것: 읽지 않기(`shape_text_margins_round_trip`).
                text_margin: draw_text_margin(draw_text.text_margin.as_ref())?,
                paragraphs,
                width,
                height,
                placement,
                caption,
                style,
                text_vertical_align,
            }
        }
        None => Control::Rect {
            frame: hwpforge_core::ObjectFrame::new(),
            width,
            height,
            placement,
            caption,
            style,
        },
    };

    let mut control = control;
    settle_shape_frame(
        &mut control,
        &rect.id,
        (
            rect.rotation_info.as_ref().map_or(0, |r| r.angle),
            rect.flip.as_ref().map_or(0, |f| f.horizontal),
            rect.flip.as_ref().map_or(0, |f| f.vertical),
        ),
        rect.org_sz.as_ref(),
        rect.sz.as_ref(),
        rect.rendering_info.as_ref(),
        ctx,
    );
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

    let caption = line
        .caption
        .as_ref()
        .map(|c| convert_hx_caption(c, Some(width.as_i32()), depth, ctx))
        .transpose()?;

    let placement = with_object_out_margin(
        decode_object_placement(line.pos.as_ref(), &line.text_wrap, &line.text_flow, ctx),
        line.out_margin.as_ref(),
    )?;

    let mut control = Control::Line {
        frame: hwpforge_core::ObjectFrame::new(),
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
    };
    settle_shape_frame(
        &mut control,
        &line.id,
        (
            line.rotation_info.as_ref().map_or(0, |r| r.angle),
            line.flip.as_ref().map_or(0, |f| f.horizontal),
            line.flip.as_ref().map_or(0, |f| f.vertical),
        ),
        line.org_sz.as_ref(),
        line.sz.as_ref(),
        line.rendering_info.as_ref(),
        ctx,
    );
    Ok(Run { content: RunContent::Control(Box::new(control)), char_shape_id })
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
            report_shape_text_margin(dt.text_margin.as_ref(), ctx);
            ctx.enter(super::PathSeg::TextBox);
            let result = decode_sublist_paragraphs(&dt.sub_list, depth, ctx);
            ctx.leave();
            (result?, decode_shape_vert_align(&dt.sub_list.vert_align))
        }
        None => (Vec::new(), VerticalAlign::Top),
    };

    let caption = ellipse
        .caption
        .as_ref()
        .map(|c| convert_hx_caption(c, Some(width.as_i32()), depth, ctx))
        .transpose()?;

    let placement = with_object_out_margin(
        decode_object_placement(ellipse.pos.as_ref(), &ellipse.text_wrap, &ellipse.text_flow, ctx),
        ellipse.out_margin.as_ref(),
    )?;

    let mut control = Control::Ellipse {
        frame: hwpforge_core::ObjectFrame::new(),
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
    };
    settle_shape_frame(
        &mut control,
        &ellipse.id,
        (
            ellipse.rotation_info.as_ref().map_or(0, |r| r.angle),
            ellipse.flip.as_ref().map_or(0, |f| f.horizontal),
            ellipse.flip.as_ref().map_or(0, |f| f.vertical),
        ),
        ellipse.org_sz.as_ref(),
        ellipse.sz.as_ref(),
        ellipse.rendering_info.as_ref(),
        ctx,
    );
    Ok(Run { content: RunContent::Control(Box::new(control)), char_shape_id })
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
            report_shape_text_margin(dt.text_margin.as_ref(), ctx);
            ctx.enter(super::PathSeg::TextBox);
            let result = decode_sublist_paragraphs(&dt.sub_list, depth, ctx);
            ctx.leave();
            (result?, decode_shape_vert_align(&dt.sub_list.vert_align))
        }
        None => (Vec::new(), VerticalAlign::Top),
    };

    let caption = polygon
        .caption
        .as_ref()
        .map(|c| convert_hx_caption(c, Some(width.as_i32()), depth, ctx))
        .transpose()?;

    let placement = with_object_out_margin(
        decode_object_placement(polygon.pos.as_ref(), &polygon.text_wrap, &polygon.text_flow, ctx),
        polygon.out_margin.as_ref(),
    )?;

    let mut control = Control::Polygon {
        frame: hwpforge_core::ObjectFrame::new(),
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
    };
    settle_shape_frame(
        &mut control,
        &polygon.id,
        (
            polygon.rotation_info.as_ref().map_or(0, |r| r.angle),
            polygon.flip.as_ref().map_or(0, |f| f.horizontal),
            polygon.flip.as_ref().map_or(0, |f| f.vertical),
        ),
        polygon.org_sz.as_ref(),
        polygon.sz.as_ref(),
        polygon.rendering_info.as_ref(),
        ctx,
    );
    Ok(Run { content: RunContent::Control(Box::new(control)), char_shape_id })
}

/// [`decode_shape_style_full`] for a shape the decoder places in Core, plus
/// the picture-fill loss report: a picture fill Core cannot carry exactly is
/// [`ContentLost`](super::DecodeWarning::ContentLost) (`shape_image_fill`).
/// Direct group children use `group_child_fill`; a shape in a group child's
/// text uses `shape_image_fill`, like any other standalone shape.
fn decode_shape_style(
    line_shape: &Option<HxLineShape>,
    fill_brush: &Option<HxFillBrush>,
    fillable: bool,
    rotation_info: Option<&crate::schema::section::HxRotationInfo>,
    flip_info: Option<&crate::schema::section::HxFlip>,
    dropcap_style: &str,
    ctx: &mut DecodeCtx,
) -> Option<ShapeStyle> {
    // Both hosts carry supported picture fills. Unsupported details retain
    // their diagnostic, attributed to the direct child or nested shape.
    if let Some(reason) = fill_brush.as_ref().and_then(image_fill_loss) {
        ctx.content_lost(
            if ctx.is_direct_group_child() { "group_child_fill" } else { "shape_image_fill" },
            reason,
        );
    }
    decode_shape_style_full(
        line_shape,
        fill_brush,
        fillable,
        rotation_info,
        flip_info,
        dropcap_style,
        ctx,
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

/// Unknown endpoint values discard only that endpoint and report both the raw enum and the located loss.
fn decode_arrow(
    shape: &str,
    size: &str,
    filled: u32,
    shape_attribute: &'static str,
    size_attribute: &'static str,
    ctx: &mut DecodeCtx,
) -> Option<hwpforge_core::control::ArrowStyle> {
    use hwpforge_foundation::{ArrowSize, ArrowType};
    if shape.is_empty() {
        return None;
    }
    let arrow_type = shape.parse::<ArrowType>().ok();
    let size_value = size.parse::<ArrowSize>().ok();
    for (valid, attribute, raw) in [
        (arrow_type.is_some(), shape_attribute, shape),
        (size_value.is_some(), size_attribute, size),
    ] {
        if !valid {
            ctx.warnings.push(super::DecodeWarning::UnknownEnumValue {
                attribute,
                raw: raw.to_string(),
                fallback: "arrow omitted",
            });
        }
    }
    // 이것을 실패시키는 것: 끝점 소실을 enum 경고만 남겨 편집 차단 우회.
    if arrow_type.is_none() || size_value.is_none() {
        ctx.content_lost(
            "shape_arrow",
            format!(
                "{shape_attribute}={shape}, {size_attribute}={size}: unsupported endpoint omitted"
            ),
        );
    }
    hwpforge_core::control::ArrowStyle {
        arrow_type: arrow_type?,
        size: size_value?,
        filled: filled != 0,
    }
    .into_nondefault()
}

/// Extracts a [`ShapeStyle`] from HWPX shape common elements including rotation, flip, arrow,
/// and drop cap info.
///
/// `fillable` = the shape has an interior (everything but lines and connect
/// lines). For such a shape a missing `<hc:fillBrush>` or a patternless
/// `winBrush faceColor="none"` means "no fill" and becomes
/// [`Fill::None`](hwpforge_core::control::Fill::None); leaving it
/// `None` would let the encoder write its default white fill.
pub(crate) fn decode_shape_style_full(
    line_shape: &Option<HxLineShape>,
    fill_brush: &Option<HxFillBrush>,
    fillable: bool,
    rotation_info: Option<&crate::schema::section::HxRotationInfo>,
    flip_info: Option<&crate::schema::section::HxFlip>,
    dropcap_style: &str,
    ctx: &mut DecodeCtx,
) -> Option<ShapeStyle> {
    use hwpforge_core::control::Fill;
    use hwpforge_foundation::GradientType;

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
                // 이것을 실패시키는 것: 면 없음+무늬의 흰 배경 대체를 무음 처리.
                if wb.face_color == "none" {
                    ctx.content_lost(
                        "shape_pattern_background",
                        format!("winBrush faceColor=none with hatchStyle={hs}: transparent pattern background replaced by white"),
                    );
                }
                let bg = parse_hex_color(&wb.face_color).unwrap_or(Color::WHITE);
                return Some(Fill::Pattern { pattern_type, fg_color: fg, bg_color: bg });
            }
            // Fixture 32: Hancom's explicit no-face brush has the same Core
            // meaning as an absent fillBrush. Keep the writer's spelling.
            // 이것을 실패시키는 것: faceColor="none"을 기본 흰 채우기로 읽기.
            if fillable && wb.face_color == "none" {
                return Some(Fill::None);
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
            let head = decode_arrow(
                &ls.head_style,
                &ls.head_sz,
                ls.head_fill,
                "lineShape@headStyle",
                "lineShape@headSz",
                ctx,
            );
            let tail = decode_arrow(
                &ls.tail_style,
                &ls.tail_sz,
                ls.tail_fill,
                "lineShape@tailStyle",
                "lineShape@tailSz",
                ctx,
            );
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
        corner_ratio: None,
    })
}

/// Decodes an `HxEllipse` with `hasArcPr=1` into a Core `Run` with `Control::Arc`.
pub(crate) fn decode_arc(
    ellipse: &HxEllipse,
    char_shape_id: CharShapeIndex,
    _depth: usize,
    ctx: &mut DecodeCtx,
) -> HwpxResult<Option<Run>> {
    use hwpforge_core::control::{ArcDefinition, ShapePoint};
    let definition = if ellipse.axis_endpoints {
        ArcDefinition::AxisEndpoints
    } else {
        ArcDefinition::EllipseSweep
    };
    let arc_type = match ellipse.arc_type.parse::<ArcType>() {
        Ok(kind) => kind,
        Err(_) => {
            ctx.warnings.push(super::DecodeWarning::UnknownEnumValue {
                attribute: if ellipse.axis_endpoints { "arc@type" } else { "ellipse@arcType" },
                raw: ellipse.arc_type.clone(),
                fallback: "arc omitted",
            });
            ctx.content_lost(
                "arc",
                format!("unsupported arc type {:?}; arc omitted", ellipse.arc_type),
            );
            return Ok(None);
        }
    };
    let fields = [
        ("center", &ellipse.center),
        ("ax1", &ellipse.ax1),
        ("ax2", &ellipse.ax2),
        ("start1", &ellipse.start1),
        ("end1", &ellipse.end1),
        ("start2", &ellipse.start2),
        ("end2", &ellipse.end2),
    ];
    let required = if ellipse.axis_endpoints { 3 } else { 7 };
    let missing: Vec<_> =
        fields[..required].iter().filter_map(|(name, p)| p.is_none().then_some(*name)).collect();
    if !missing.is_empty() {
        ctx.content_lost(
            "arc",
            format!(
                "{definition:?} arc missing required points: {}; arc omitted",
                missing.join(", ")
            ),
        );
        return Ok(None);
    }
    let points: Vec<_> = fields
        .iter()
        .map(|(_, p)| p.as_ref().map_or(ShapePoint::new(0, 0), |p| ShapePoint::new(p.x, p.y)))
        .collect();
    let [center, axis1, axis2, start1, end1, start2, end2] = points[..] else { unreachable!() };

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

    let placement = with_object_out_margin(
        decode_object_placement(ellipse.pos.as_ref(), &ellipse.text_wrap, &ellipse.text_flow, ctx),
        ellipse.out_margin.as_ref(),
    )?;
    let caption = ellipse
        .caption
        .as_ref()
        .map(|c| convert_hx_caption(c, Some(width.as_i32()), _depth, ctx))
        .transpose()?;

    let mut control = Control::Arc {
        definition,
        frame: hwpforge_core::ObjectFrame::new(),
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
    };
    settle_shape_frame(
        &mut control,
        &ellipse.id,
        (
            ellipse.rotation_info.as_ref().map_or(0, |r| r.angle),
            ellipse.flip.as_ref().map_or(0, |f| f.horizontal),
            ellipse.flip.as_ref().map_or(0, |f| f.vertical),
        ),
        ellipse.org_sz.as_ref(),
        ellipse.sz.as_ref(),
        ellipse.rendering_info.as_ref(),
        ctx,
    );
    Ok(Some(Run { content: RunContent::Control(Box::new(control)), char_shape_id }))
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

    let placement = with_object_out_margin(
        decode_object_placement(curve.pos.as_ref(), &curve.text_wrap, &curve.text_flow, ctx),
        curve.out_margin.as_ref(),
    )?;
    let caption = curve
        .caption
        .as_ref()
        .map(|c| convert_hx_caption(c, Some(width.as_i32()), depth, ctx))
        .transpose()?;

    let mut control = Control::Curve {
        frame: hwpforge_core::ObjectFrame::new(),
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
    };
    settle_shape_frame(
        &mut control,
        &curve.id,
        (
            curve.rotation_info.as_ref().map_or(0, |r| r.angle),
            curve.flip.as_ref().map_or(0, |f| f.horizontal),
            curve.flip.as_ref().map_or(0, |f| f.vertical),
        ),
        curve.org_sz.as_ref(),
        curve.sz.as_ref(),
        curve.rendering_info.as_ref(),
        ctx,
    );
    Ok(Run { content: RunContent::Control(Box::new(control)), char_shape_id })
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
        let candidate = ObjectPlacement::legacy_inline_defaults()
            .with_horz_offset(HwpUnit::new(ox).unwrap_or(HwpUnit::ZERO))
            .with_vert_offset(HwpUnit::new(oy).unwrap_or(HwpUnit::ZERO));
        (candidate != ObjectPlacement::legacy_inline_defaults()).then_some(candidate)
    };
    let inst_id =
        text_art.instid.parse::<u64>().ok().filter(|v| *v != 0).map(hwpforge_core::ObjectId::new);
    let pr = text_art.textart_pr.clone().unwrap_or_default();
    let line_spacing = u32::try_from(pr.line_spacing).unwrap_or(0);
    let char_spacing = u32::try_from(pr.char_spacing).unwrap_or(0);
    let shape = if pr.text_shape.is_empty() { "RECTANGLE".to_string() } else { pr.text_shape };
    let align = if pr.align.is_empty() { "LEFT".to_string() } else { pr.align };

    let mut control = Control::TextArt {
        frame: hwpforge_core::ObjectFrame::new(),
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
    };
    settle_shape_frame(
        &mut control,
        "",
        (0, 0, 0),
        text_art.org_sz.as_ref(),
        text_art.sz.as_ref(),
        text_art.rendering_info.as_ref(),
        ctx,
    );
    Ok(Run { content: RunContent::Control(Box::new(control)), char_shape_id })
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

    let placement = with_object_out_margin(
        decode_object_placement(cl.pos.as_ref(), &cl.text_wrap, &cl.text_flow, ctx),
        cl.out_margin.as_ref(),
    )?;
    let caption = cl
        .caption
        .as_ref()
        .map(|c| convert_hx_caption(c, Some(width.as_i32()), depth, ctx))
        .transpose()?;

    let mut control = Control::ConnectLine {
        frame: hwpforge_core::ObjectFrame::new(),
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
    };
    settle_shape_frame(
        &mut control,
        &cl.id,
        (
            cl.rotation_info.as_ref().map_or(0, |r| r.angle),
            cl.flip.as_ref().map_or(0, |f| f.horizontal),
            cl.flip.as_ref().map_or(0, |f| f.vertical),
        ),
        cl.org_sz.as_ref(),
        cl.sz.as_ref(),
        cl.rendering_info.as_ref(),
        ctx,
    );
    Ok(Run { content: RunContent::Control(Box::new(control)), char_shape_id })
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
    // Branch review B: an outer group's retry decodes its whole subtree
    // again, and a group in a child's text would be retried again for each
    // outer attempt — 2^depth decodes. Its result and warnings do not depend
    // on the outer attempt (its own state is saved below), so the first one
    // is reused.
    // 이것을 실패시키는 것: 이 재사용 삭제(`nested_group_retries_stay_linear`).
    //
    // r2 M3 / r3 H1·M2: a group decoded inside another group's decode is
    // kept once in a slot (with the warnings of its own level), numbered in
    // the order its parent made the call, and a placeholder stands for it in
    // the parent's tree. The parent's retry makes the same calls in the same
    // order, so it takes the same slots back by that order — no address
    // identifies a group (a group in a note's first paragraph lives in a
    // temporary copy). The outermost group moves every slot into the place
    // of its placeholder and gathers every slot's warnings once, placed or
    // lost, in call order; a
    // placeholder without its slot is an error.
    // 이것을 실패시키는 것: 재시도에서 슬롯 대신 다시 디코드하기 · 경고를 슬롯마다
    // 후손까지 들기 · 자리표 해소 실패를 무시하기(`nested_group_retries_stay_linear`,
    // `groups_in_note_heads_keep_their_own_content`).
    if let Some(frame) = ctx.group_calls.last_mut() {
        if let Some(cursor) = frame.replay.as_mut() {
            let id =
                frame.record.get(*cursor).copied().ok_or_else(|| group_replay_error(*cursor))?;
            *cursor += 1;
            crate::work::tick();
            let slot = ctx.group_slots.get(id).and_then(Option::as_ref);
            let slot = slot.ok_or_else(|| group_replay_error(id))?;
            // A lost group's warnings stay in its slot either way.
            return Ok(slot.run.as_ref().map(|run| group_placeholder(id, run.char_shape_id)));
        }
    }
    // A group in a group child's text is decoded on its own: keep the outer
    // group's state.
    let saved_sources = std::mem::take(&mut ctx.group_sources);
    let saved_frames = ctx.group_frames;
    ctx.group_frames = true;
    let mark = ctx.warnings.len();
    ctx.group_calls.push(super::section::GroupCalls::default());
    let mut candidates = Vec::new();
    let result = decode_container_once(container, char_shape_id, depth, ctx, &mut candidates)
        .and_then(|run| match run {
            Some(run) => settle_group_frames(
                container,
                run,
                char_shape_id,
                depth,
                mark,
                ctx,
                &mut candidates,
            ),
            None => Ok(None),
        });
    let frame = ctx.group_calls.pop().unwrap_or_default();
    ctx.group_sources = saved_sources;
    ctx.group_frames = saved_frames;
    let mut run = result?;
    if let Some(Run { content: RunContent::Control(group), .. }) = &mut run {
        // Source retry uses only folded image decision geometry; admission is child-local.
        for (path, loss) in hwpforge_core::control::geometry::preserve_group_original_boxes(
            group,
            &candidates,
            &crate::encoder::shapes::HWPX_POLICY,
        ) {
            for &index in &path {
                ctx.enter(super::PathSeg::GroupChild(index));
            }
            ctx.content_lost("shape_emission", loss.detail().to_string());
            for _ in &path {
                ctx.leave();
            }
        }
    }
    let own = ctx.warnings.split_off(mark);
    crate::work::tick_n(own.len());
    if !ctx.group_calls.is_empty() {
        // Nested: keep it in a slot, the parent records the slot. A lost
        // group (no run) keeps its warnings and children there too — they
        // are gathered once, at the outermost group (r4 H1: copying them to
        // the parent as well doubled them at every lost level).
        // 이것을 실패시키는 것: 잃은 묶음 경고를 부모에도 넣기(`lost_nested_groups_report_each_warning_once`).
        let slot = GroupSlot { run, placed: false, warnings: own, children: frame.record };
        #[cfg(test)]
        if let Some(r) = &slot.run {
            // Tests count the paragraphs the slots hold.
            let mut holder = hwpforge_core::paragraph::Paragraph::new(
                hwpforge_foundation::ParaShapeIndex::new(0),
            );
            holder.runs.push(r.clone());
            holder.for_each_paragraph(|_| crate::work::tick());
        }
        let placeholder =
            slot.run.as_ref().map(|r| group_placeholder(ctx.group_slots.len(), r.char_shape_id));
        ctx.group_slots.push(Some(slot));
        let id = ctx.group_slots.len() - 1;
        if let Some(parent) = ctx.group_calls.last_mut() {
            parent.record.push(id);
        }
        return Ok(placeholder);
    }
    // The outermost group: its own warnings, then every kept group's — placed
    // or lost — once each, in call order.
    ctx.warnings.extend(own);
    let mut slots = std::mem::take(&mut ctx.group_slots);
    let placed = run.is_some();
    let out = match run {
        Some(mut run) => {
            resolve_group_placeholders(&mut run, &mut slots)?;
            Some(run)
        }
        None => None,
    };
    let mut gathered = Vec::new();
    gather_slot_warnings(&frame.record, &mut slots, &mut gathered, placed)?;
    ctx.warnings.extend(gathered);
    if let Some(left) = slots.iter().position(Option::is_some) {
        return Err(crate::HwpxError::InvalidStructure {
            detail: format!("group {left} decoded inside another group has no parent"),
        });
    }
    Ok(out)
}

/// A group decoded inside another group's decode, kept until the outermost
/// group places it.
#[derive(Debug)]
pub(crate) struct GroupSlot {
    /// The group (`None` once placed, or when it was lost).
    run: Option<Run>,
    /// Whether the outermost group put it in its placeholder's place.
    placed: bool,
    /// The warnings of its own level (not of the groups decoded inside it).
    warnings: Vec<crate::DecodeWarning>,
    /// The slots of the groups decoded directly inside it.
    children: Vec<usize>,
}

fn group_replay_error(at: usize) -> crate::HwpxError {
    crate::HwpxError::InvalidStructure {
        detail: format!("a group's retry made a nested group call ({at}) its first decode did not"),
    }
}

/// Moves the warnings of the slots `ids` and of their descendants into
/// `out` (each slot's own, then its children's), emptying those slots.
/// `parent_placed`: whether every group above these is in the result —
/// then a group of theirs still unplaced is an error; below a lost group,
/// nothing is placed.
fn gather_slot_warnings(
    ids: &[usize],
    slots: &mut [Option<GroupSlot>],
    out: &mut Vec<crate::DecodeWarning>,
    parent_placed: bool,
) -> HwpxResult<()> {
    for &id in ids {
        let slot = slots.get_mut(id).and_then(Option::take).ok_or_else(|| {
            crate::HwpxError::InvalidStructure {
                detail: format!("group {id} decoded inside another group was gathered twice"),
            }
        })?;
        if parent_placed && slot.run.is_some() {
            return Err(crate::HwpxError::InvalidStructure {
                detail: format!("group {id} decoded inside another group was not placed"),
            });
        }
        crate::work::tick_n(slot.warnings.len());
        out.extend(slot.warnings);
        gather_slot_warnings(&slot.children, slots, out, parent_placed && slot.placed)?;
    }
    Ok(())
}

/// The tag of a [`group_placeholder`].
const GROUP_PLACEHOLDER: &str = "\u{0}hwpforge-group";

/// A run standing for the group in slot `id` until the outermost group's
/// decode puts that group in its place.
fn group_placeholder(id: usize, char_shape_id: CharShapeIndex) -> Run {
    Run::control(
        Control::Unknown { tag: GROUP_PLACEHOLDER.to_string(), data: Some(id.to_string()) },
        char_shape_id,
    )
}

/// Replaces every group placeholder in `run` (and in what replaces it) with
/// its group, moved out of `slots` (the slot keeps its warnings and is
/// marked placed). Each paragraph is visited once.
///
/// # Errors
///
/// A placeholder whose slot is missing or already used.
fn resolve_group_placeholders(run: &mut Run, slots: &mut [Option<GroupSlot>]) -> HwpxResult<()> {
    fn swap(run: &mut Run, slots: &mut [Option<GroupSlot>]) -> HwpxResult<()> {
        crate::work::tick();
        let RunContent::Control(c) = &run.content else { return Ok(()) };
        let Control::Unknown { tag, data } = &**c else { return Ok(()) };
        if tag != GROUP_PLACEHOLDER {
            return Ok(());
        }
        let id: Option<usize> = data.as_deref().and_then(|d| d.parse().ok());
        let slot = id.and_then(|id| slots.get_mut(id)).and_then(Option::as_mut);
        let Some(group) = slot.and_then(|slot| {
            slot.placed = slot.run.is_some();
            slot.run.take()
        }) else {
            return Err(crate::HwpxError::InvalidStructure {
                detail: format!("group placeholder {data:?} has no group to place"),
            });
        };
        *run = group;
        Ok(())
    }
    fn paragraphs(
        list: &mut [hwpforge_core::paragraph::Paragraph],
        slots: &mut [Option<GroupSlot>],
    ) -> HwpxResult<()> {
        for paragraph in list {
            let mut result = Ok(());
            paragraph.for_each_paragraph_mut(|p| {
                for r in &mut p.runs {
                    if result.is_err() {
                        return;
                    }
                    result = swap(r, slots);
                    // Run pictures' captions are not walked: enter them here.
                    if let (Ok(()), RunContent::Image(image)) = (&result, &mut r.content) {
                        if let Some(caption) = image.caption.as_mut() {
                            result = paragraphs(&mut caption.paragraphs, slots);
                        }
                    }
                }
            });
            result?;
        }
        Ok(())
    }
    swap(run, slots)?;
    let mut holder =
        hwpforge_core::paragraph::Paragraph::new(hwpforge_foundation::ParaShapeIndex::new(0));
    holder.runs.push(std::mem::replace(run, Run::text("", run.char_shape_id)));
    let result = paragraphs(std::slice::from_mut(&mut holder), slots);
    *run = holder.runs.pop().expect("the run put in");
    result
}

#[cfg(test)]
thread_local! {
    /// How many times [`decode_container_once`] ran on this thread.
    pub(crate) static CONTAINER_DECODES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// S20 (§19.23.17 M2, §19.23.20): a group decoded with its original boxes is
/// checked as one — the writer's [`group_plan`] against the source, for every
/// node that keeps an original box and the group's own box. Preservation
/// has not touched this tree. Over 2 HWPUNIT anywhere: decoded again without
/// original boxes (as before S20); if that
/// plan lands within 2 of the source, the boxes are dropped and reported
/// (`object_original_size`). Otherwise the source draws a size its layout
/// does not hold: `shape_drawn_extent`, and the boxes are kept (the drawn
/// result is the same).
///
/// 이것을 실패시키는 것: 이 판정을 지우기(`sibling_fallback_is_predicted` 가
/// 계획과 원본의 차이를 놓친다).
///
/// [`group_plan`]: hwpforge_core::control::geometry::group_plan
fn settle_group_frames(
    container: &crate::schema::section::HxContainer,
    run: Run,
    char_shape_id: CharShapeIndex,
    depth: usize,
    mark: usize,
    ctx: &mut DecodeCtx,
    candidates: &mut Vec<(usize, FramePlan)>,
) -> HwpxResult<Option<Run>> {
    use super::group_geometry::EXACT_TOLERANCE;
    let sources = std::mem::take(&mut ctx.group_sources);
    let RunContent::Control(group) = &run.content else { return Ok(Some(run)) };
    // The group's own box is judged when it keeps an original box — then
    // also without it, for the same source.
    let judge_own = group.frame().is_some_and(|f| f.original_size.is_some());
    let with = group_residual(group, &sources, container, judge_own);
    if with <= EXACT_TOLERANCE {
        return Ok(Some(run));
    }
    let first_warnings = ctx.warnings.split_off(mark);
    ctx.group_frames = false;
    // The retry takes the groups the first decode made inside this one back
    // from their slots, by call order (see `decode_container`).
    if let Some(frame) = ctx.group_calls.last_mut() {
        frame.replay = Some(0);
    }
    let first_candidates = std::mem::take(candidates);
    let plain = decode_container_once(container, char_shape_id, depth, ctx, candidates)?;
    // A retry that made fewer calls leaves slots no tree places, which the
    // outermost group reports (`decode_container`).
    if let Some(frame) = ctx.group_calls.last_mut() {
        frame.replay = None;
    }
    ctx.group_sources.clear();
    let without = match plain.as_ref().map(|r| &r.content) {
        Some(RunContent::Control(g)) => group_residual(g, &sources, container, judge_own),
        _ => f64::INFINITY,
    };
    if without <= EXACT_TOLERANCE {
        candidates.clear();
        ctx.content_lost(
            "object_original_size",
            format!(
                "the group's original boxes are dropped: with them the re-encode lands \
                 {with:.2} from the source, without them {without:.2}"
            ),
        );
        return Ok(plain);
    }
    ctx.warnings.truncate(mark);
    ctx.warnings.extend(first_warnings);
    *candidates = first_candidates;
    ctx.content_lost(
        "shape_drawn_extent",
        format!("a group's shapes land {with:.2} from where its matrices draw them"),
    );
    Ok(Some(run))
}

/// How far the writer's plan of `group` puts the judged nodes from the
/// source (`sources`, preorder; `None` = not judged), and the group's own
/// box when it keeps an original box (its scale's size only, from its
/// origin corner).
fn group_residual(
    group: &Control,
    sources: &[Option<Vec<(f64, f64)>>],
    container: &crate::schema::section::HxContainer,
    judge_own: bool,
) -> f64 {
    use crate::encoder::shapes::HWPX_POLICY;
    use hwpforge_core::control::geometry::group_plan;
    let Some(plan) = group_plan(group, &HWPX_POLICY) else { return 0.0 };
    if plan.nodes().len() != sources.len() {
        // The plan and the decode disagree on the nodes: judged as off (a
        // report, not a silent pass).
        // 이것을 실패시키는 것: 버린 중첩 묶음의 자리 되돌리기를 지우기
        // (`nested_group_without_matrices_or_shapes_is_reported` 에 경고가 더해진다).
        return f64::INFINITY;
    }
    let mut worst = 0.0_f64;
    for (node, source) in plan.nodes().iter().zip(sources) {
        let Some(source) = source else { continue };
        let stored = node.stored_positions();
        if stored.len() != source.len() {
            return f64::INFINITY;
        }
        for (a, b) in stored.iter().zip(source) {
            worst = worst.max((a.0 - b.0).hypot(a.1 - b.1));
        }
    }
    let org = container.org_sz.as_ref().or(container.cur_sz.as_ref());
    if let (true, Some(org)) = (judge_own, org) {
        if let Some(map) = source_map(container.rendering_info.as_ref(), false) {
            let (ow, oh) = (f64::from(org.width), f64::from(org.height));
            let source: Vec<(f64, f64)> = [(0.0, 0.0), (ow, 0.0), (ow, oh), (0.0, oh)]
                .into_iter()
                .map(|p| map.apply(p))
                .collect();
            worst = worst.max(anchored_residual(plan.own().stored_positions(), &source));
        }
    }
    worst
}

/// One decode of a top-level group ([`decode_container`]).
fn decode_container_once(
    container: &crate::schema::section::HxContainer,
    char_shape_id: CharShapeIndex,
    depth: usize,
    ctx: &mut DecodeCtx,
    candidates: &mut Vec<(usize, FramePlan)>,
) -> HwpxResult<Option<Run>> {
    #[cfg(test)]
    CONTAINER_DECODES.with(|n| n.set(n.get() + 1));
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

    let Some(children) =
        decode_group_children(container, origin, char_shape_id, depth, ctx, candidates)?
    else {
        return Ok(None);
    };

    let placement = with_object_out_margin(
        decode_object_placement(
            container.pos.as_ref(),
            &container.text_wrap,
            &container.text_flow,
            ctx,
        ),
        container.out_margin.as_ref(),
    )?;
    Ok(Some(Run {
        content: RunContent::Control(Box::new(Control::Group {
            frame: group_frame(container, (width, height), true, ctx),
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
    /// The wire shape has a picture fill (`<hc:imgBrush>`), so even an
    /// unsupported brush participates in the existing turning-content rule.
    picture_fill: bool,
    /// `<hp:flip>` sets a mirror (S20: such a leaf is not axis-aligned).
    flipped: bool,
}

/// Whether `<hp:flip>` mirrors the object.
fn flipped(flip: Option<&crate::schema::section::HxFlip>) -> bool {
    flip.is_some_and(|f| f.horizontal != 0 || f.vertical != 0)
}

/// S20 (§19.23.13): a group leaf is axis-aligned when its whole chain is
/// `x' = a·x + c`, `y' = e·y + f` with `a, e > 0` and `<hp:flip>` mirrors
/// nothing — the leaves that keep their original box.
fn wire_aligned(map: Option<super::group_geometry::Affine>, flipped: bool) -> bool {
    map.is_some_and(|m| m.b.abs() < 1e-9 && m.d.abs() < 1e-9 && m.a > 0.0 && m.e > 0.0) && !flipped
}

/// Whether every non-image-filled leaf under `container`, nested groups
/// included, is axis-aligned ([`wire_aligned`]) — the groups that keep their original box
/// (§19.23.13). An OLE object counts as a leaf that is not.
/// 이것을 실패시키는 것: 중첩 묶음을 보지 않기 / 잎 하나만 보기
/// (`mixed_group_keeps_aligned_leaves_only`).
// 이것을 실패시키는 것: 회전 그림 하나 때문에 부모 frame이 재디코드에서 사라지기.
fn all_aligned(container: &crate::schema::section::HxContainer) -> bool {
    use crate::schema::section::HxContainerChildKind as K;
    let leaf = |ri: Option<&crate::schema::section::HxRenderingInfo>,
                offset: Option<&crate::schema::section::HxOffset>,
                flip: Option<&crate::schema::section::HxFlip>| {
        wire_aligned(child_chain(ri, offset), flipped(flip))
    };
    container.child_order.iter().all(|&(kind, idx)| match kind {
        K::Rect => {
            let s = &container.rects[idx];
            has_picture_fill(s.fill_brush.as_ref())
                || leaf(s.rendering_info.as_ref(), s.offset.as_ref(), s.flip.as_ref())
        }
        K::Line => {
            let s = &container.lines[idx];
            has_picture_fill(s.fill_brush.as_ref())
                || leaf(s.rendering_info.as_ref(), s.offset.as_ref(), s.flip.as_ref())
        }
        K::Ellipse => {
            let s = &container.ellipses[idx];
            has_picture_fill(s.fill_brush.as_ref())
                || leaf(s.rendering_info.as_ref(), s.offset.as_ref(), s.flip.as_ref())
        }
        K::Polygon => {
            let s = &container.polygons[idx];
            has_picture_fill(s.fill_brush.as_ref())
                || leaf(s.rendering_info.as_ref(), s.offset.as_ref(), s.flip.as_ref())
        }
        K::Curve => {
            let s = &container.curves[idx];
            has_picture_fill(s.fill_brush.as_ref())
                || leaf(s.rendering_info.as_ref(), s.offset.as_ref(), s.flip.as_ref())
        }
        K::ConnectLine => {
            let s = &container.connect_lines[idx];
            has_picture_fill(s.fill_brush.as_ref())
                || leaf(s.rendering_info.as_ref(), s.offset.as_ref(), s.flip.as_ref())
        }
        K::TextArt => {
            // HxTextArt keeps no `<hp:flip>`: the chain alone decides.
            let s = &container.textarts[idx];
            leaf(s.rendering_info.as_ref(), s.offset.as_ref(), None)
        }
        K::Picture => {
            let s = &container.pics[idx];
            leaf(s.rendering_info.as_ref(), s.offset.as_ref(), s.flip.as_ref())
        }
        K::Container => all_aligned(&container.containers[idx]),
        K::Unsupported(name) => name != "ole",
    })
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
    candidates: &mut Vec<(usize, FramePlan)>,
) -> HwpxResult<Option<Vec<GroupChild>>> {
    use crate::schema::section::HxContainerChildKind as K;

    // Bound nested-container recursion (group-in-group) against pathological
    // depth — same cap and error as table nesting (`convert_table`).
    if depth >= crate::decoder::section::MAX_NESTING_DEPTH {
        return Err(crate::HwpxError::InvalidStructure {
            detail: format!(
                "group nesting depth {} exceeds limit of {} in {}",
                depth + 1,
                crate::decoder::section::MAX_NESTING_DEPTH,
                ctx.part,
            ),
        });
    }

    let mut children: Vec<GroupChild> = Vec::new();
    for (wire_idx, &(kind, idx)) in container.child_order.iter().enumerate() {
        ctx.enter(super::PathSeg::GroupChild(wire_idx));
        ctx.enter_group_child();
        let result =
            decode_group_child(container, kind, idx, origin, char_shape_id, depth, ctx, candidates);
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
#[allow(clippy::too_many_arguments)]
fn decode_group_child(
    container: &crate::schema::section::HxContainer,
    kind: crate::schema::section::HxContainerChildKind,
    idx: usize,
    origin: (f64, f64),
    char_shape_id: CharShapeIndex,
    depth: usize,
    ctx: &mut DecodeCtx,
    candidates: &mut Vec<(usize, FramePlan)>,
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
                flipped: flipped(r.flip.as_ref()),
            })
        }
        K::Line => {
            let l = &container.lines[idx];
            take(Some(decode_line(l, char_shape_id, depth, ctx)?)).map(|control| WireChild {
                control,
                map: child_chain(l.rendering_info.as_ref(), l.offset.as_ref()),
                org: org_size(l.org_sz.as_ref(), l.cur_sz.as_ref()),
                picture_fill: has_picture_fill(l.fill_brush.as_ref()),
                flipped: flipped(l.flip.as_ref()),
            })
        }
        K::Ellipse => {
            let e = &container.ellipses[idx];
            let run = if e.has_arc_pr == 1 {
                decode_arc(e, char_shape_id, depth, ctx)?
            } else {
                Some(decode_ellipse(e, char_shape_id, depth, ctx)?)
            };
            take(run).map(|control| WireChild {
                control,
                map: child_chain(e.rendering_info.as_ref(), e.offset.as_ref()),
                org: org_size(e.org_sz.as_ref(), e.cur_sz.as_ref()),
                picture_fill: has_picture_fill(e.fill_brush.as_ref()),
                flipped: flipped(e.flip.as_ref()),
            })
        }
        K::Polygon => {
            let p = &container.polygons[idx];
            take(Some(decode_polygon(p, char_shape_id, depth, ctx)?)).map(|control| WireChild {
                control,
                map: child_chain(p.rendering_info.as_ref(), p.offset.as_ref()),
                org: org_size(p.org_sz.as_ref(), p.cur_sz.as_ref()),
                picture_fill: has_picture_fill(p.fill_brush.as_ref()),
                flipped: flipped(p.flip.as_ref()),
            })
        }
        K::Curve => {
            let c = &container.curves[idx];
            take(Some(decode_curve(c, char_shape_id, depth, ctx)?)).map(|control| WireChild {
                control,
                map: child_chain(c.rendering_info.as_ref(), c.offset.as_ref()),
                org: org_size(c.org_sz.as_ref(), c.cur_sz.as_ref()),
                picture_fill: has_picture_fill(c.fill_brush.as_ref()),
                flipped: flipped(c.flip.as_ref()),
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
                    flipped: flipped(c.flip.as_ref()),
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
                flipped: false,
            })
        }
        K::Picture => {
            return decode_group_picture(&container.pics[idx], origin, depth, ctx);
        }
        K::Container => {
            // The nested group's node comes before its children's (preorder).
            let mark = ctx.group_sources.len();
            ctx.group_sources.push(None);
            let nested = decode_nested_container(
                &container.containers[idx],
                origin,
                char_shape_id,
                depth + 1,
                ctx,
                candidates,
            )?;
            if nested.is_none() {
                ctx.group_sources.truncate(mark);
                candidates.retain(|(index, _)| *index < mark);
            }
            return Ok(nested.map(GroupChild::Shape));
        }
        K::Unsupported(_) => return Ok(None),
    };
    let Some(wire) = wire else { return Ok(None) };
    Ok(normalize_group_child(wire, origin, ctx, candidates).map(GroupChild::Shape))
}

/// A picture in a group: its box folded like a shape's
/// ([`fold_box`](hwpforge_core::control::geometry::fold_box)), kept as an
/// unturned [`GroupChild::Picture`]. What a Core picture cannot show is
/// reported as `group_child_picture`: a turn or mirror (the picture is kept
/// unturned around the same centre), an image effect, a border. Its crop is
/// carried like a body picture's. `None` = dropped (its loss already
/// reported).
fn decode_group_picture(
    pic: &crate::schema::section::HxPic,
    origin: (f64, f64),
    depth: usize,
    ctx: &mut DecodeCtx,
) -> HwpxResult<Option<GroupChild>> {
    use hwpforge_core::control::geometry::{fold_box, FoldInput, FoldLossKind};

    let Some(mut image) = super::section::convert_picture(pic, depth, ctx)? else {
        ctx.content_lost("pic", "picture without an hc:img element");
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
    for reason in group_picture_losses(pic) {
        ctx.content_lost("group_child_picture", reason);
    }
    image.width = HwpUnit::new(fold.width()).unwrap_or(HwpUnit::ZERO);
    image.height = HwpUnit::new(fold.height()).unwrap_or(HwpUnit::ZERO);
    image.placement = group_child_placement(fold.offset().0, fold.offset().1);
    // S20 (§19.23.13 M5): the original box is judged against the FINAL
    // displayed size, once the chain has scaled the picture. `convert_picture`
    // judged it against a provisional size (`orgSz` itself when there is no
    // `sz` and `curSz` is 0) and folded it away. (A picture has no points,
    // so its original box changes nothing drawn, turned or not.)
    // 이것을 실패시키는 것: 이 재판정을 지우기(`group_picture_keeps_its_original_box`
    // — 원래 크기 `None`, 재인코드 `orgSz` 가 표시 크기로) / 표시 크기와 같은
    // 원래 크기를 접지 않기(같은 테스트의 둘째 그림).
    let display = (fold.width(), fold.height());
    image.frame.original_size = pic
        .org_sz
        .as_ref()
        .map(|s| (s.width, s.height))
        .filter(|&o| ctx.group_frames && o != display)
        .and_then(|(w, h)| {
            Some(hwpforge_foundation::Size::new(HwpUnit::new(w).ok()?, HwpUnit::new(h).ok()?))
        });
    // The frame check judges a picture with an original box (S20).
    ctx.group_sources.push(image.frame.original_size.map(|_| {
        [(0.0, 0.0), (org.0, 0.0), (org.0, org.1), (0.0, org.1)]
            .into_iter()
            .map(|p| map.apply(p))
            .collect()
    }));
    Ok(Some(GroupChild::Picture(image)))
}

/// What a group picture shows that a Core picture cannot: an image effect
/// (`<hc:img>` effect, brightness, contrast, alpha, or anything under
/// `<hp:effects>`), a visible border (`<hp:lineShape>`). A crop is carried
/// ([`Image::crop`](hwpforge_core::image::Image::crop), decoded with the
/// picture by `convert_picture`).
fn group_picture_losses(pic: &crate::schema::section::HxPic) -> Vec<String> {
    let mut out = Vec::new();
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
    candidates: &mut Vec<(usize, FramePlan)>,
) -> HwpxResult<Option<Control>> {
    use super::group_geometry::{bounds, corners, wire_chain};

    let leaf_box = leaf_bounds(nested);
    let Some((min_x, min_y, max_x, max_y)) = leaf_box else {
        // No geometry to place: decode the children anyway so unsupported
        // ones are reported, then drop the group.
        let _ =
            decode_group_children(nested, parent_origin, char_shape_id, depth, ctx, candidates)?;
        return Ok(None);
    };
    // The nested group's own matrices should describe the same box. A
    // child Core cannot carry (a picture without hc:img) counts in the wire box but
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
    let Some(mut children) =
        decode_group_children(nested, provisional, char_shape_id, depth, ctx, candidates)?
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
        frame: group_frame(nested, (w, h), false, ctx),
        children,
        width: HwpUnit::new(w).unwrap_or(HwpUnit::ZERO),
        height: HwpUnit::new(h).unwrap_or(HwpUnit::ZERO),
        placement: group_child_placement(x, y),
        inst_id: container_inst_id(nested),
    }))
}

/// S20 (§19.23.13): a group's original box — its `orgSz` (or `curSz`) when
/// every non-image-filled leaf is axis-aligned ([`all_aligned`]) and the box can be a
/// frame value — and, for a top-level group, its size lock. A group that is
/// not all aligned keeps no original box (its turned leaves are folded into
/// the displayed space).
/// 이것을 실패시키는 것: 정렬 판정을 지우기(혼합 묶음에 원래 크기가 생긴다 —
/// `mixed_group_keeps_aligned_leaves_only`) / 잠금을 읽지 않기
/// (`group_frame_round_trips`).
fn group_frame(
    container: &crate::schema::section::HxContainer,
    displayed: (i32, i32),
    top: bool,
    ctx: &mut DecodeCtx,
) -> hwpforge_core::ObjectFrame {
    let mut frame = hwpforge_core::ObjectFrame::new()
        .with_size_protect(container.sz.as_ref().is_some_and(|s| s.protect != 0));
    if !ctx.group_frames || !all_aligned(container) {
        return frame;
    }
    // A top-level group written without `<hp:sz>` (not Hancom's form) has a
    // displayed size taken from its shapes, unrelated to its `orgSz`.
    // 이것을 실패시키는 것: 이 조건을 지우기(`sz` 없는 합성 묶음마다 원래 크기
    // 버림 보고가 생긴다 — `ellipse_points_scale_with_the_box`).
    if top && container.sz.is_none() {
        return frame;
    }
    let Some(org) = container.org_sz.as_ref().or(container.cur_sz.as_ref()) else { return frame };
    let org = (org.width, org.height);
    // A displayed size out of range is reported and kept as 0: nothing to
    // scale to. 이것을 실패시키는 것: 이 조건을 지우기
    // (`out_of_range_geometry_is_reported` 에 `shape_drawn_extent` 가 더해진다).
    if org == displayed || !fits_hwpunit(&[displayed.0, displayed.1]) {
        return frame;
    }
    if let Some(reason) = unusable_original(org, displayed) {
        ctx.content_lost("object_original_size", reason);
        return frame;
    }
    let unit = |v: i32| HwpUnit::new(v).unwrap_or(HwpUnit::ZERO);
    frame.original_size = Some(hwpforge_foundation::Size::new(unit(org.0), unit(org.1)));
    frame
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
/// (an unsupported element, or a picture without `<hc:img>`).
fn has_unsupported_child(container: &crate::schema::section::HxContainer) -> bool {
    use crate::schema::section::HxContainerChildKind as K;
    container.child_order.iter().any(|&(kind, idx)| match kind {
        K::Unsupported(_) => true,
        K::Picture => !has_image(&container.pics[idx]),
        _ => false,
    }) || container.containers.iter().any(has_unsupported_child)
}

/// Whether a picture has `<hc:img>`; an empty reference still carries its frame.
/// 이것을 실패시키는 것: 빈 참조를 false 로 판정(중첩 묶음의 그림 경계 소실).
fn has_image(pic: &crate::schema::section::HxPic) -> bool {
    pic.img.is_some()
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
    candidates: &mut Vec<(usize, FramePlan)>,
) -> Option<Control> {
    use hwpforge_core::control::geometry::{
        fold_aligned_child, fold_group_child, fold_group_child_with_original_box, FoldInput,
        FoldLossKind, FoldOutcome, GeometryFacts,
    };

    let WireChild { control, map, org, picture_fill, flipped } = wire;
    let Some(m) = map else {
        ctx.content_lost("group_child_transform", "unparseable transform matrix");
        return None;
    };
    let mut control = control;
    // Only the decoded image fill selects preservation. A raw imgBrush can
    // lose to a gradient or lack a reference; its existing diagnostics remain.
    let preserve_picture_fill = match &control {
        Control::TextBox { style, .. }
        | Control::Rect { style, .. }
        | Control::Ellipse { style, .. }
        | Control::Arc { style, .. }
        | Control::Polygon { style, .. }
        | Control::Curve { style, .. }
        | Control::Line { style, .. }
        | Control::ConnectLine { style, .. } => matches!(
            style.as_ref().and_then(|s| s.fill.as_ref()),
            Some(hwpforge_core::control::Fill::Image { .. })
        ),
        _ => false,
    };
    // S20 (§19.23.13): an axis-aligned leaf keeps its points in its original
    // box. Non-aligned picture geometry is saved separately for the later trial.
    // 이것을 실패시키는 것: 이 갈래를 지우기(`aligned_leaves_keep_their_original_box`).
    // An aligned leaf whose `orgSz` cannot be a frame value is reported as
    // at the top level (§19.23.15) and folds without an original box.
    // 이것을 실패시키는 것: 이 보고를 지우기(`negative_original_size_in_a_group_is_reported`).
    let usable = org.0 >= 0.0
        && org.1 >= 0.0
        && HwpUnit::new(round_i32(org.0)).is_ok()
        && HwpUnit::new(round_i32(org.1)).is_ok();
    if ctx.group_frames && wire_aligned(Some(m), flipped) && !usable {
        ctx.content_lost(
            "object_original_size",
            format!("orgSz {}x{} cannot be an original size; not carried", org.0, org.1),
        );
    }
    if ctx.group_frames && wire_aligned(Some(m), flipped) && usable {
        let input = FoldInput::new(m.to_transform(), org, origin);
        // Keep decoded allocations (and their decoder source identities) intact.
        // Geometry trials need only the shape shell, never its paragraph tree.
        let contents = take_contents(&mut control);
        let trial = control.clone();
        put_contents(&mut control, contents);
        if let Some(outcome) = fold_aligned_child(trial, &input) {
            let source: Vec<(f64, f64)> = [(0.0, 0.0), (org.0, 0.0), (org.0, org.1), (0.0, org.1)]
                .into_iter()
                .chain(
                    hwpforge_core::control::geometry::defining_points(&control)
                        .iter()
                        .map(|p| (f64::from(p.x), f64::from(p.y))),
                )
                .map(|p| m.apply(p))
                .collect();
            let (kept, losses) = match outcome {
                FoldOutcome::Kept { control: mut kept, losses } => {
                    put_contents(&mut kept, take_contents(&mut control));
                    (Some(*kept), losses)
                }
                FoldOutcome::Dropped { losses } => (None, losses),
                _ => (None, Vec::new()),
            };
            for loss in losses {
                ctx.content_lost("group_child_transform", loss.detail().to_string());
            }
            if kept.is_some() {
                ctx.group_sources.push((!preserve_picture_fill).then_some(source));
            }
            return kept;
        }
    }
    let input = FoldInput::new(m.to_transform(), org, origin)
        .with_geometry_facts(GeometryFacts::new(picture_fill));
    // 이것을 실패시키는 것: 보존 후보를 HEAD의 정렬/접기 입력 대신 전달하기.
    // Copy the shell only. Paragraphs, captions and their source identities stay put.
    let candidate = if preserve_picture_fill {
        let contents = take_contents(&mut control);
        let trial = control.clone();
        put_contents(&mut control, contents);
        match fold_group_child_with_original_box(trial, &input) {
            FoldOutcome::Kept { control, .. } => {
                hwpforge_core::control::geometry::group_child_frame_plan(
                    &control,
                    &hwpforge_core::control::geometry::EmissionPolicy::exact(),
                )
            }
            _ => None,
        }
    } else {
        None
    };
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
    if kept.is_some() {
        if let Some(candidate) = candidate {
            candidates.push((ctx.group_sources.len(), candidate));
        }
        ctx.group_sources.push(None);
    }
    kept
}

/// Whether every value is a valid [`HwpUnit`] (which a Core size or offset
/// must be — an invalid one would be stored as 0).
fn fits_hwpunit(values: &[i32]) -> bool {
    values.iter().all(|&v| HwpUnit::new(v).is_ok())
}

/// A drawn shape's text margin (`<hp:drawText><hp:textMargin>`) as a Core
/// margin; `None` when absent (§19.22 D4). A side outside the HWPUNIT range
/// is an error, as for table margins — not a silent fall back to 283
/// (review r1). 이것을 실패시키는 것: 범위 밖 한 변을 `None` 으로 접기
/// (`out_of_range_text_margin_is_refused`).
fn draw_text_margin(
    margin: Option<&crate::schema::section::HxTableMargin>,
) -> crate::error::HwpxResult<Option<hwpforge_core::table::TableMargin>> {
    let Some(m) = margin else { return Ok(None) };
    let side = |v: i32, name: &str| {
        HwpUnit::new(v).map_err(|_| crate::error::HwpxError::InvalidStructure {
            detail: format!("invalid text box margin {name}: {v}"),
        })
    };
    Ok(Some(hwpforge_core::table::TableMargin {
        left: side(m.left, "left")?,
        right: side(m.right, "right")?,
        top: side(m.top, "top")?,
        bottom: side(m.bottom, "bottom")?,
    }))
}

/// An ellipse's or polygon's text margin other than 283 on every side: Core
/// carries a text margin for text boxes only, so the re-encode writes 283 —
/// reported, not refused (the text stays; only its inset changes).
/// 이것을 실패시키는 것: 이 보고를 지우기(`shape_text_margins_round_trip`).
fn report_shape_text_margin(
    margin: Option<&crate::schema::section::HxTableMargin>,
    ctx: &mut super::section::DecodeCtx,
) {
    if let Some(m) = margin {
        if [m.left, m.right, m.top, m.bottom] != [283; 4] {
            ctx.warnings.push(super::DecodeWarning::UnknownEnumValue {
                attribute: "hp:drawText/hp:textMargin",
                raw: format!("{} {} {} {}", m.left, m.right, m.top, m.bottom),
                fallback: "283",
            });
        }
    }
}

/// `placement` with the object's outer margin (`<hp:outMargin>`, S18). A
/// margin that is 0 everywhere (or absent) leaves the placement as it is, so
/// an inline default still collapses to `None`; any other margin is carried,
/// on the inline default when there was no placement. A side outside the
/// HWPUNIT range is an error, as for table and text box margins.
/// 이것을 실패시키는 것: 여백을 싣지 않기 / 0 여백도 `Some` 으로
/// (`object_out_margins_round_trip`).
pub(super) fn with_object_out_margin(
    placement: Option<ObjectPlacement>,
    margin: Option<&crate::schema::section::HxTableMargin>,
) -> crate::error::HwpxResult<Option<ObjectPlacement>> {
    let Some(m) = margin.filter(|m| [m.left, m.right, m.top, m.bottom] != [0; 4]) else {
        return Ok(placement);
    };
    let side = |v: i32, name: &str| {
        HwpUnit::new(v).map_err(|_| crate::error::HwpxError::InvalidStructure {
            detail: format!("invalid object outer margin {name}: {v}"),
        })
    };
    let mut placement = placement.unwrap_or_else(ObjectPlacement::legacy_inline_defaults);
    placement.out_margin = Some(hwpforge_core::table::TableMargin {
        left: side(m.left, "left")?,
        right: side(m.right, "right")?,
        top: side(m.top, "top")?,
        bottom: side(m.bottom, "bottom")?,
    });
    Ok(Some(placement))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 이것을 실패시키는 것: 축 정렬 그림 source를 그룹 잔차/retry에 포함하기.
    #[test]
    fn image_fill_retry_excludes_brush_sources_including_aligned_children() {
        use crate::decoder::group_geometry::Affine;
        use hwpforge_core::control::Fill;
        use hwpforge_foundation::ImageFillMode;
        let u = |v| HwpUnit::new(v).unwrap();
        for frames in [false, true] {
            for turned in [false, true] {
                for image in [false, true] {
                    let mut c = Control::rect(u(100), u(50)).unwrap();
                    if let Control::Rect { style, .. } = &mut c {
                        *style = Some(ShapeStyle {
                            fill: image.then(|| Fill::Image {
                                image_id: "BinData/brush".into(),
                                mode: ImageFillMode::Tile,
                            }),
                            ..Default::default()
                        });
                    }
                    let map = if turned {
                        Affine { a: 0., b: -3., c: 200., d: 2., e: 0., f: 300. }
                    } else {
                        Affine { a: 2., b: 0., c: 200., d: 0., e: 3., f: 300. }
                    };
                    let mut ctx = DecodeCtx::new(0);
                    ctx.group_frames = frames;
                    let mut candidates = Vec::new();
                    let got = normalize_group_child(
                        WireChild {
                            control: c,
                            map: Some(map),
                            org: (100., 50.),
                            picture_fill: true,
                            flipped: false,
                        },
                        (0., 0.),
                        &mut ctx,
                        &mut candidates,
                    )
                    .unwrap();
                    assert!(ctx.warnings.is_empty(), "{:?}", ctx.warnings);
                    assert_eq!(got.frame().unwrap().original_size.is_some(), frames && !turned);
                    assert_eq!(ctx.group_sources.len(), 1);
                    assert_eq!(ctx.group_sources[0].is_some(), frames && !turned && !image);
                    assert_eq!(candidates.len(), usize::from(image && (!frames || turned)));
                }
            }
        }
    }

    fn decode_shape_style_full(
        line_shape: &Option<HxLineShape>,
        fill_brush: &Option<HxFillBrush>,
        fillable: bool,
        rotation_info: Option<&crate::schema::section::HxRotationInfo>,
        flip_info: Option<&crate::schema::section::HxFlip>,
        dropcap_style: &str,
        warnings: &mut Vec<crate::DecodeWarning>,
    ) -> Option<ShapeStyle> {
        let mut ctx = DecodeCtx::new(0);
        let style = super::decode_shape_style_full(
            line_shape,
            fill_brush,
            fillable,
            rotation_info,
            flip_info,
            dropcap_style,
            &mut ctx,
        );
        warnings.extend(ctx.warnings);
        style
    }

    // 이것을 실패시키는 것: 결측 `width` 의 기본값을 0 으로 되돌리기(결측이
    // 굵기 0 으로 읽힘), 또는 결측 표식을 직렬화에서 빼지 않기(`width="-1"`).
    #[test]
    fn missing_line_width_stays_unspecified() {
        let ls: HxLineShape =
            crate::decoder::xml_from_str(r##"<lineShape color="#000000" style="SOLID"/>"##)
                .unwrap();
        let style = decode_shape_style_full(
            &Some(ls.clone()),
            &None,
            false,
            None,
            None,
            "None",
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(style.line_width, None);
        #[allow(clippy::disallowed_methods)]
        let back = quick_xml::se::to_string(&ls).unwrap();
        assert!(!back.contains("width"), "{back}");
    }

    #[test]
    fn zero_line_width_decodes_as_zero() {
        let line =
            HxLineShape { width: 0, style: "NONE".to_string(), ..HxLineShape::default_solid() };
        let style =
            decode_shape_style_full(&Some(line), &None, false, None, None, "None", &mut Vec::new())
                .unwrap();
        assert_eq!(style.line_width, Some(0));
        assert_eq!(style.line_style, Some(hwpforge_core::control::LineStyle::None));
    }

    #[test]
    fn no_fill_shape_decodes_as_fill_none() {
        use hwpforge_core::control::Fill;
        let style =
            decode_shape_style_full(&None, &None, true, None, None, "None", &mut Vec::new())
                .unwrap();
        assert_eq!(style.fill, Some(Fill::None));
        // 선(채울 내부가 없음)은 종전대로 스타일 없음.
        assert!(decode_shape_style_full(&None, &None, false, None, None, "None", &mut Vec::new())
            .is_none());
    }

    /// 이것을 실패시키는 것: 명시적 none 누락 / 면 없음 무늬까지 지우기 /
    /// 선 도형에 Fill::None 추가 / 빈 면 색을 none으로 추측하기.
    #[test]
    fn hancom_no_face_brush_is_no_fill_only_without_a_pattern() {
        for fillable in [false, true] {
            for face in ["none", "", "#FFFFFF"] {
                for hatch in [None, Some("SLASH".to_string())] {
                    let mut brush = HxFillBrush::default_white();
                    let wb = brush.win_brush.as_mut().unwrap();
                    wb.face_color = face.into();
                    wb.hatch_style = hatch.clone();
                    let mut warnings = Vec::new();
                    let style = decode_shape_style_full(
                        &None,
                        &Some(brush),
                        fillable,
                        None,
                        None,
                        "None",
                        &mut warnings,
                    );
                    let fill = style.as_ref().and_then(|s| s.fill.as_ref());
                    if hatch.is_some() {
                        assert!(matches!(fill, Some(Fill::Pattern { .. })));
                    } else if fillable && face == "none" {
                        assert_eq!(fill, Some(&Fill::None));
                        assert_eq!(style.unwrap().fill_color, None);
                    } else {
                        assert_eq!(fill, None);
                    }
                    assert_eq!(
                        warnings.len(),
                        usize::from(face == "none" && hatch.is_some()),
                        "{warnings:?}"
                    );
                    assert!(warnings.iter().all(crate::DecodeWarning::is_content_loss));
                }
            }
        }
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
        decode_shape_style_full(
            line_shape,
            fill_brush,
            false,
            None,
            None,
            dropcap_style,
            &mut Vec::new(),
        )
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
            axis_endpoints: false,
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
        // 이것을 실패시키는 것: NORMAL 쪽 비기본 채움 0을 버리기.
        let head = style.head_arrow.unwrap();
        assert_eq!(head.arrow_type, ArrowType::None);
        assert_eq!(head.size, ArrowSize::Medium);
        assert!(!head.filled);
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
        let style =
            decode_shape_style_full(&None, &None, false, Some(&ri), None, "None", &mut Vec::new())
                .unwrap();
        let rot = style.rotation.unwrap();
        assert!((rot - 45.0f32).abs() < 0.01, "45 degrees expected, got {rot}");
    }

    #[test]
    fn decode_shape_style_full_rotation_zero_ignored() {
        let ri = HxRotationInfo { angle: 0, center_x: 0, center_y: 0, rotate_image: 1 };
        let result =
            decode_shape_style_full(&None, &None, false, Some(&ri), None, "None", &mut Vec::new());
        assert!(result.is_none(), "zero rotation + no other fields should be None");
    }

    #[test]
    fn decode_shape_style_full_flip_horizontal() {
        let fi = HxFlip { horizontal: 1, vertical: 0 };
        let style =
            decode_shape_style_full(&None, &None, false, None, Some(&fi), "None", &mut Vec::new())
                .unwrap();
        assert_eq!(style.flip, Some(Flip::Horizontal));
    }

    #[test]
    fn decode_shape_style_full_flip_vertical() {
        let fi = HxFlip { horizontal: 0, vertical: 1 };
        let style =
            decode_shape_style_full(&None, &None, false, None, Some(&fi), "None", &mut Vec::new())
                .unwrap();
        assert_eq!(style.flip, Some(Flip::Vertical));
    }

    #[test]
    fn decode_shape_style_full_flip_both() {
        let fi = HxFlip { horizontal: 1, vertical: 1 };
        let style =
            decode_shape_style_full(&None, &None, false, None, Some(&fi), "None", &mut Vec::new())
                .unwrap();
        assert_eq!(style.flip, Some(Flip::Both));
    }

    #[test]
    fn decode_shape_style_full_flip_none_ignored() {
        let fi = HxFlip { horizontal: 0, vertical: 0 };
        let result =
            decode_shape_style_full(&None, &None, false, None, Some(&fi), "None", &mut Vec::new());
        assert!(result.is_none(), "zero flip should yield None");
    }

    #[test]
    fn decode_shape_style_full_combined_rotation_and_flip() {
        let ri = HxRotationInfo { angle: 90, center_x: 0, center_y: 0, rotate_image: 1 };
        let fi = HxFlip { horizontal: 1, vertical: 0 };
        let style = decode_shape_style_full(
            &None,
            &None,
            false,
            Some(&ri),
            Some(&fi),
            "None",
            &mut Vec::new(),
        )
        .unwrap();
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
        let style = decode_shape_style_full(&None, &fb, false, None, None, "None", &mut Vec::new())
            .unwrap();
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
        let style = decode_shape_style_full(&None, &fb, false, None, None, "None", &mut Vec::new())
            .unwrap();
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
        let style = decode_shape_style_full(&None, &fb, false, None, None, "None", &mut Vec::new())
            .unwrap();
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
        let style = decode_shape_style_full(&None, &fb, false, None, None, "None", &mut Vec::new())
            .unwrap();
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
        let style = decode_shape_style_full(&None, &fb, false, None, None, "None", &mut Vec::new())
            .unwrap();
        assert!(matches!(style.fill, Some(Fill::Gradient { .. })));
    }

    fn complete_arc() -> HxEllipse {
        let mut e = default_ellipse();
        e.has_arc_pr = 1;
        for slot in [
            &mut e.center,
            &mut e.ax1,
            &mut e.ax2,
            &mut e.start1,
            &mut e.end1,
            &mut e.start2,
            &mut e.end2,
        ] {
            *slot = Some(HxPoint { x: 0, y: 0 });
        }
        e
    }

    // ── decode_arc tests ─────────────────────────────────────────────

    #[test]
    fn decode_arc_default_fields_normal_type() {
        let ellipse = complete_arc();
        let cs = CharShapeIndex::new(0);
        let run = decode_arc(&ellipse, cs, 0, &mut DecodeCtx::new(0)).unwrap().unwrap();
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
        let mut ellipse = complete_arc();
        ellipse.arc_type = "PIE".to_string();
        let cs = CharShapeIndex::new(0);
        let run = decode_arc(&ellipse, cs, 0, &mut DecodeCtx::new(0)).unwrap().unwrap();
        if let Control::Arc { arc_type, .. } = run.content.as_control().unwrap().clone() {
            assert_eq!(arc_type, ArcType::Pie);
        } else {
            panic!("expected Control::Arc");
        }
    }

    #[test]
    fn decode_arc_chord_type() {
        let mut ellipse = complete_arc();
        ellipse.arc_type = "CHORD".to_string();
        let cs = CharShapeIndex::new(0);
        let run = decode_arc(&ellipse, cs, 0, &mut DecodeCtx::new(0)).unwrap().unwrap();
        if let Control::Arc { arc_type, .. } = run.content.as_control().unwrap().clone() {
            assert_eq!(arc_type, ArcType::Chord);
        } else {
            panic!("expected Control::Arc");
        }
    }

    #[test]
    fn decode_arc_with_geometry_points() {
        let mut ellipse = complete_arc();
        ellipse.center = Some(HxPoint { x: 100, y: 200 });
        ellipse.ax1 = Some(HxPoint { x: 300, y: 200 });
        ellipse.ax2 = Some(HxPoint { x: 100, y: 400 });
        ellipse.start1 = Some(HxPoint { x: 50, y: 100 });
        ellipse.end1 = Some(HxPoint { x: 150, y: 100 });
        let cs = CharShapeIndex::new(2);
        let run = decode_arc(&ellipse, cs, 0, &mut DecodeCtx::new(0)).unwrap().unwrap();
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
        let mut ellipse = complete_arc();
        ellipse.sz = Some(make_sz(5000, 3000));
        ellipse.pos = Some(make_pos(100, 200));
        let cs = CharShapeIndex::new(0);
        let run = decode_arc(&ellipse, cs, 0, &mut DecodeCtx::new(0)).unwrap().unwrap();
        if let Control::Arc { width, height, ref placement, .. } =
            *run.content.as_control().unwrap()
        {
            assert_eq!(width.as_i32(), 5000);
            assert_eq!(height.as_i32(), 3000);
            let placement = placement.as_ref().expect("anchored pos carries placement");
            assert_eq!(placement.horz_offset.as_i32(), 100);
            assert_eq!(placement.vert_offset.as_i32(), 200);
        } else {
            panic!("expected Control::Arc");
        }
    }

    #[test]
    fn decode_arc_with_rotation_style() {
        let mut ellipse = complete_arc();
        ellipse.rotation_info =
            Some(HxRotationInfo { angle: 45, center_x: 50, center_y: 50, rotate_image: 1 });
        let cs = CharShapeIndex::new(0);
        let run = decode_arc(&ellipse, cs, 0, &mut DecodeCtx::new(0)).unwrap().unwrap();
        if let Control::Arc { ref style, .. } = *run.content.as_control().unwrap() {
            let s = style.as_ref().unwrap();
            let rot = s.rotation.unwrap();
            assert!((rot - 45.0f32).abs() < 0.01);
        } else {
            panic!("expected Control::Arc");
        }
    }

    /// 이것을 실패시키는 것: 모르는 호 종류를 Normal로 대체하기.
    #[test]
    fn decode_arc_unknown_type_is_reported_and_omitted() {
        for native in [false, true] {
            let mut ellipse = complete_arc();
            ellipse.axis_endpoints = native;
            ellipse.arc_type = "BOGUS".to_string();
            let mut ctx = DecodeCtx::new(0);
            assert!(decode_arc(&ellipse, CharShapeIndex::new(0), 0, &mut ctx).unwrap().is_none());
            assert!(ctx.warnings.iter().any(|w| matches!(w, super::super::DecodeWarning::UnknownEnumValue { attribute, raw, fallback } if *attribute == if native { "arc@type" } else { "ellipse@arcType" } && raw == "BOGUS" && *fallback == "arc omitted")));
        }
    }

    /// 이것을 실패시키는 것: 필수 점 누락을 (0,0)으로 채우기, 축 끝점 호에 sweep을 요구하기.
    #[test]
    fn decode_arc_checks_only_active_required_points() {
        for native in [false, true] {
            for missing in 0..7 {
                let mut ellipse = complete_arc();
                ellipse.axis_endpoints = native;
                let slots = [
                    &mut ellipse.center,
                    &mut ellipse.ax1,
                    &mut ellipse.ax2,
                    &mut ellipse.start1,
                    &mut ellipse.end1,
                    &mut ellipse.start2,
                    &mut ellipse.end2,
                ];
                *slots.into_iter().nth(missing).unwrap() = None;
                let mut ctx = DecodeCtx::new(0);
                let run = decode_arc(&ellipse, CharShapeIndex::new(0), 0, &mut ctx).unwrap();
                let required = !native || missing < 3;
                assert_eq!(run.is_none(), required);
                if required {
                    assert!(ctx
                        .warnings
                        .iter()
                        .any(|w| format!("{w:?}").contains("missing required points")));
                }
            }
        }
    }

    // ── decode_curve tests ───────────────────────────────────────────

    #[test]
    fn decode_curve_empty_returns_no_points() {
        let curve = default_curve();
        let cs = CharShapeIndex::new(0);
        let run = decode_curve(&curve, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::Curve { ref points, ref segment_types, .. } =
            *run.content.as_control().unwrap()
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
        if let Control::Curve { ref points, ref segment_types, .. } =
            *run.content.as_control().unwrap()
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
        if let Control::Curve { ref points, .. } = *run.content.as_control().unwrap() {
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
        if let Control::Curve { ref segment_types, .. } = *run.content.as_control().unwrap() {
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
        if let Control::Curve { width, height, ref placement, .. } =
            *run.content.as_control().unwrap()
        {
            assert_eq!(width.as_i32(), 8000);
            assert_eq!(height.as_i32(), 4000);
            let placement = placement.as_ref().expect("anchored pos carries placement");
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
        if let Control::ConnectLine { start, end, ref control_points, ref connect_type, .. } =
            *run.content.as_control().unwrap()
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
        if let Control::ConnectLine { ref control_points, .. } = *run.content.as_control().unwrap()
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
        if let Control::ConnectLine { ref control_points, .. } = *run.content.as_control().unwrap()
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
        if let Control::ConnectLine { ref connect_type, .. } = *run.content.as_control().unwrap() {
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
        if let Control::ConnectLine { width, height, ref placement, .. } =
            *run.content.as_control().unwrap()
        {
            assert_eq!(width.as_i32(), 10000);
            assert_eq!(height.as_i32(), 5000);
            let placement = placement.as_ref().expect("anchored pos carries placement");
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
        match *run.content.as_control().unwrap() {
            Control::Rect { width, height, ref placement, .. } => {
                assert_eq!(width, hwpforge_foundation::HwpUnit::new(8000).unwrap());
                assert_eq!(height, hwpforge_foundation::HwpUnit::new(4000).unwrap());
                assert!(placement.is_none());
            }
            ref other => panic!("expected Control::Rect, got {other:?}"),
        }
    }

    #[test]
    fn decode_ellipse_defaults_all_zero() {
        let ellipse = default_ellipse();
        let cs = CharShapeIndex::new(0);
        let run = decode_ellipse(&ellipse, cs, 0, &mut DecodeCtx::new(0)).unwrap();
        if let Control::Ellipse { center, axis1, axis2, width, height, ref paragraphs, .. } =
            *run.content.as_control().unwrap()
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
            frame: hwpforge_core::ObjectFrame::new(),
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
            false,
            &mut hl,
            crate::encoder::EncodeOptions::default(),
            &mut crate::encoder::section::DocumentEncodeContext::new(0),
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
        assert!(decode_object_placement(
            Some(&inline),
            "TOP_AND_BOTTOM",
            "BOTH_SIDES",
            &mut crate::decoder::section::DecodeCtx::new(0)
        )
        .is_none());
        assert!(decode_object_placement(
            Some(&inline),
            "",
            "",
            &mut crate::decoder::section::DecodeCtx::new(0)
        )
        .is_none());
        assert!(decode_object_placement(
            None,
            "SQUARE",
            "LEFT_ONLY",
            &mut crate::decoder::section::DecodeCtx::new(0)
        )
        .is_none());
        // treatAsChar=0 at the origin is anchored, not inline → carries placement.
        assert!(decode_object_placement(
            Some(&make_pos(0, 0)),
            "",
            "",
            &mut crate::decoder::section::DecodeCtx::new(0)
        )
        .is_some());
    }

    #[test]
    fn decode_object_placement_maps_floating_pos_fields() {
        let mut pos = make_pos(1_200, 3_400);
        pos.treat_as_char = 0;
        pos.flow_with_text = 1;
        pos.allow_overlap = 1;
        pos.vert_rel_to = "PAPER".to_string();
        pos.horz_rel_to = "PAGE".to_string();
        let placement = decode_object_placement(
            Some(&pos),
            "SQUARE",
            "RIGHT_ONLY",
            &mut crate::decoder::section::DecodeCtx::new(0),
        )
        .expect("floating");
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
            ref other => panic!("expected Control::Rect, got {other:?}"),
        };
        assert!(!placement.treat_as_char);
        assert_eq!(placement.horz_offset.as_i32(), 1_000);

        let mut hyperlinks = Vec::new();
        let encoded = crate::encoder::shapes::encode_rect_to_hx(
            &control,
            false,
            &mut hyperlinks,
            crate::encoder::EncodeOptions::default(),
            &mut crate::encoder::section::DocumentEncodeContext::new(0),
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
            false,
            &mut hyperlinks,
            crate::encoder::EncodeOptions::default(),
            &mut crate::encoder::section::DocumentEncodeContext::new(0),
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
            RunContent::Control(mut c) => match c.as_mut() {
                Control::TextArt { text, .. } => std::mem::take(text),
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
            &crate::encoder::shapes::build_shape_common(
                c,
                &crate::encoder::shapes::shape_frame_plan(c, false).unwrap(),
                false,
            )
            .rendering_info
            .rot_matrix,
        )
        .unwrap();
        let map = Affine::translate(base.0 + o.0, base.1 + o.1).then_after(rot);
        // S20: points are in the original box, drawn scaled to the displayed one.
        let o_box = c.effective_original_size().unwrap();
        let scale = |d: i32, o: hwpforge_foundation::HwpUnit| {
            if o.as_i32() == 0 {
                1.0
            } else {
                f64::from(d) / f64::from(o.as_i32())
            }
        };
        let (sx, sy) = (scale(w, o_box.width), scale(h, o_box.height));
        out.push(PageShape {
            kind,
            corners: corners(f64::from(w), f64::from(h)).iter().map(|&p| map.apply(p)).collect(),
            points: pts
                .iter()
                .map(|p| map.apply((f64::from(p.x) * sx, f64::from(p.y) * sy)))
                .collect(),
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
        // D/E are standalone siblings: their rotation is now representable.
        // Group leaf geometry assertions below remain unchanged.
        assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
        assert!(
            doc.warnings.iter().all(|w| matches!(
                w,
                crate::DecodeWarning::ContentLost { path, .. }
                    if path.0.contains(&crate::decoder::PathSeg::BodyParagraph(1))
            )),
            "{:?}",
            doc.warnings
        );
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

    /// A shape with a picture fill Core cannot hold (brightness 10) in the
    /// text of a group's text box is reported like one anywhere else; only
    /// the group's direct children are left to the group path.
    ///
    /// 이것을 실패시키는 것: 직접 자식 판정을 "묶음 안 어디든" 으로 되돌리기
    /// (글상자 안 도형의 손실이 무음).
    #[test]
    fn image_fill_inside_a_group_text_box_is_reported() {
        let brush = r#"<hc:fillBrush><hc:imgBrush mode="TOTAL"><hc:img binaryItemIDRef="image1" bright="10" contrast="0" effect="REAL_PIC" alpha="0"/></hc:imgBrush></hc:fillBrush>"#;
        let inner = format!(
            r#"<hp:rect id="5" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="0" instid="55"><hp:offset x="0" y="0"/><hp:orgSz width="500" height="500"/><hp:curSz width="500" height="500"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="0" centerY="0" rotateimage="1"/>{}{brush}<hp:sz width="500" widthRelTo="ABSOLUTE" height="500" heightRelTo="ABSOLUTE" protect="0"/><hp:pos treatAsChar="1" affectLSpacing="0" flowWithText="1" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="PARA" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/></hp:rect>"#,
            ri(ID, &[(ID, ID)])
        );
        let text_box = format!(
            r#"{}<hp:drawText lastWidth="0" name="" editable="0"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0">{inner}<hp:t>x</hp:t></hp:run></hp:p></hp:subList></hp:drawText></hp:rect>"#,
            shape_head("rect", (4000, 3000), &ri(ID, &[(ID, ID)]))
        );
        let (group, warnings) = decode(&container((4000, 3000), &text_box));
        assert!(group.is_some(), "{warnings:?}");
        assert!(lost_kinds(&warnings).contains(&"shape_image_fill".to_string()), "{warnings:?}");
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
        // Turned 180° about its own box (S20: a turned leaf folds by the box
        // rule; an axis-aligned one keeps its points — next test).
        let half_turn = [-1.0, 0.0, 2000.0, 0.0, -1.0, 1000.0];
        let (g, w) = decode(&container(
            (2000, 2000),
            &ellipse((2000, 1000), &ri(ID, &[(half, ID), (ID, half_turn)])),
        ));
        assert!(w.is_empty(), "{w:?}");
        let g = g.unwrap();
        let Control::Ellipse { center, axis1, axis2, width, height, style, .. } = &children(&g)[0]
        else {
            panic!()
        };
        assert_eq!(style.as_ref().and_then(|s| s.rotation), Some(180.0));
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

    /// The scale that draws the 14173 design box at a body TextArt's layout
    /// size of 7000 x 3000 (Hancom's form; an identity scale would draw it
    /// 14173 wide — a drawn-extent loss).
    const BODY_ART_SCALE: [f64; 6] = [7000.0 / 14173.0, 0.0, 0.0, 0.0, 3000.0 / 14173.0, 0.0];

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
            &mut Vec::new(),
            crate::encoder::EncodeOptions::default(),
            &mut crate::encoder::section::DocumentEncodeContext::new(0),
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
        // Turned 90° in its own box (x' = 1000 − y, y' = x): a turned point
        // shape takes the map onto its points (S20 keeps the points of an
        // axis-aligned one), so the box is the points' extent (200..800).
        let turned = [(ID, ID), (ID, ID), (ID, [0.0, -1.0, 1000.0, 1.0, 0.0, 0.0])];
        let poly =
            polygon((1000, 1000), &ri(ID, &turned), &[(200, 200), (800, 200), (500, 800)], "");
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
            frame: hwpforge_core::ObjectFrame::new(),
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
            .expect("TextArt child");
        let Control::TextArt { width, height, placement: ref at, fill_color, .. } = *art else {
            unreachable!()
        };
        let (x, y) =
            at.as_ref().map_or((0, 0), |q| (q.horz_offset.as_i32(), q.vert_offset.as_i32()));
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
        let body = text_art(0, &ri(ID, &[(BODY_ART_SCALE, ID)]), &solid_brush("#00FF00"), sz)
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
        // Hancom scales the 14173 design box to the layout size.
        let ri = ri(ID, &[(BODY_ART_SCALE, ID)]);
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

        // A text box that is not in a group: no group warning. (This one is
        // written without `<hp:sz>`, so its `orgSz` 1000 × 400 cannot be
        // drawn at the 0 × 0 it decodes to — reported as `shape_drawn_extent`,
        // S20.)
        let free = text_box(SWITCH, 0);
        let (_, w) = decode(&free);
        assert_eq!(lost_kinds(&w), ["shape_drawn_extent"], "{w:?}");
    }

    /// 이것을 실패시키는 것: 묶음 그림 채우기를 지우거나 회전을 점에 굽기.
    #[test]
    fn picture_filled_child_is_carried_and_keeps_its_rotation() {
        let fill = r#"<hc:fillBrush><hc:imgBrush mode="TOTAL"><hc:img binaryItemIDRef="image1" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/></hc:imgBrush></hc:fillBrush>"#;
        let child = polygon(
            (1000, 600),
            &ri(ID, &[(ID, rot(30.0)), (ID, ID)]),
            &[(0, 600), (500, 0), (1000, 600)],
            fill,
        );
        let (g, w) = decode(&container((3000, 3000), &child));
        assert!(lost_kinds(&w).is_empty(), "{w:?}");
        let g = g.unwrap();
        assert_eq!(rotation(children(&g)[0]), (Some(30.0), None));
        let Control::Polygon { style, .. } = children(&g)[0] else { panic!("polygon") };
        assert!(matches!(style.as_ref().and_then(|s| s.fill.as_ref()),
            Some(hwpforge_core::control::Fill::Image { image_id, .. }) if image_id == "BinData/image1"));
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
        // (`orgSz` = `sz`, as Hancom writes a group with an identity own
        // matrix — otherwise the group's own box is judged too, S20.)
        let (g, w) = decode(&container_with((800, 1000), (800, 1000), &own, &leaf));
        let (width, height, _) = size_and_offset(&g.unwrap());
        assert_eq!((width, height), (800, 1000), "displayed size is `sz`");
        assert_eq!(lost_kinds(&w), ["group_display_size"]);

        let (_, w) = decode(&container_with((1500, 1000), (1500, 1000), &own, &leaf));
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
            frame: hwpforge_core::ObjectFrame::new(),
            width: HwpUnit::new(w).unwrap(),
            height: HwpUnit::new(h).unwrap(),
            placement,
            caption: None,
            style: None,
        };
        let mut doc = HwpxDecoder::decode(&fixture("group-f1-g1-scaled.hwpx")).unwrap();
        let mut source = first_group(&doc);
        let Control::Group { placement, inst_id, .. } = &mut source else { unreachable!() };
        let group = Control::Group {
            frame: hwpforge_core::ObjectFrame::new(),
            children: vec![
                hwpforge_core::control::GroupChild::Shape(rect(1000, 500, at(100, 50))),
                hwpforge_core::control::GroupChild::Shape(Control::Group {
                    frame: hwpforge_core::ObjectFrame::new(),
                    children: vec![hwpforge_core::control::GroupChild::Shape(rect(800, 600, None))],
                    width: HwpUnit::new(800).unwrap(),
                    height: HwpUnit::new(600).unwrap(),
                    placement: at(2000, 3000),
                    inst_id: None,
                }),
            ],
            width: HwpUnit::new(2800).unwrap(),
            height: HwpUnit::new(3600).unwrap(),
            placement: placement.take(),
            inst_id: *inst_id,
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
            let name = f.name().expect("ZIP entry name").to_string();
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
        // Turned 180° in its own box (S20: a turned point shape takes the
        // map onto its points; an axis-aligned one keeps its original box).
        let inset = polygon(
            (1000, 600),
            &ri(ID, &[(ID, ID), (ID, ID), (ID, [-1.0, 0.0, 1000.0, 0.0, -1.0, 600.0])]),
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

    fn loss_test_rect() -> String {
        rect((1000, 1000), &ri(ID, &[(ID, ID)]))
            .replace("</hp:rect>", r#"<hp:sz width="1000" widthRelTo="ABSOLUTE" height="1000" heightRelTo="ABSOLUTE" protect="0"/></hp:rect>"#)
    }

    fn assert_loss_rejects_editors(bytes: &[u8], kind: &str) {
        use crate::stamp::{HwpxStamper, StampRequestV2, StamperError, STAMP_MAP_VERSION};
        let check = |component: String, diff_path: String| {
            if kind == "shape_pattern_background" && component == "document" {
                // The existing semantic round-trip check can run first: the
                // white replacement also changes fill_color on re-decode.
                assert!(diff_path.contains("fill_color"), "{diff_path}");
                return;
            }
            assert_eq!(component, format!("input decode: CONTENT_LOST {kind}"), "{diff_path}");
            assert!(diff_path.starts_with("Contents/section0.xml:"), "{diff_path}");
        };
        match HwpxStamper::stamp(bytes, &[]) {
            Err(StamperError::NotRoundTripSafe { component, diff_path }) => {
                check(component, diff_path)
            }
            other => panic!("stamp v1: {other:?}"),
        }
        let request = StampRequestV2 {
            schema_version: STAMP_MAP_VERSION,
            source_sha256: crate::patch::sha256_hex(bytes),
            text: Vec::new(),
            cells: Vec::new(),
        };
        match HwpxStamper::stamp_v2(bytes, &request) {
            Err(StamperError::NotRoundTripSafe { component, diff_path }) => {
                check(component, diff_path)
            }
            other => panic!("stamp v2: {other:?}"),
        }
        match crate::HwpxCellEditor::set_cells(bytes, &[]) {
            Err(crate::CellEditError::NotRoundTripSafe { component, diff_path }) => {
                check(component, diff_path)
            }
            other => panic!("cell edit: {other:?}"),
        }
    }

    /// 이것을 실패시키는 것: head/tail 모양/크기 손실 경고 또는 편집 차단 삭제.
    #[test]
    fn discarded_arrow_endpoints_reject_editors_in_both_hosts() {
        for grouped in [false, true] {
            for side in ["head", "tail"] {
                for unknown in ["Style", "Sz"] {
                    let outline = r##"<hp:lineShape color="#000000" width="33" style="SOLID" headStyle="ARROW" tailStyle="ARROW" headSz="MEDIUM_MEDIUM" tailSz="MEDIUM_MEDIUM" headfill="1" tailfill="1"/>"##;
                    let shape =
                        loss_test_rect().replace("</hp:rect>", &format!("{outline}</hp:rect>"));
                    let shape = if grouped { container((1000, 1000), &shape) } else { shape };
                    let good = encode(&HwpxDecoder::decode(&package_with_group(&shape)).unwrap());
                    // Start from a stable package, so refusal must be this loss,
                    // not unrelated ZIP carry or pre-existing geometry drift.
                    crate::stamp::HwpxStamper::stamp(&good, &[]).unwrap();
                    crate::HwpxCellEditor::set_cells(&good, &[]).unwrap();
                    let xml = section0_xml(&good);
                    let old = format!(
                        "{side}{unknown}=\"{}\"",
                        if unknown == "Style" { "ARROW" } else { "MEDIUM_MEDIUM" }
                    );
                    let bad_shape = shape.replace(&old, &format!("{side}{unknown}=\"FUTURE\""));
                    assert!(xml.contains(&old));
                    let bad = package_with_group(&bad_shape);
                    let decoded = HwpxDecoder::decode(&bad).unwrap();
                    let lost: Vec<_> =
                        decoded.warnings.iter().filter(|w| w.is_content_loss()).collect();
                    assert_eq!(lost.len(), 1, "{:?}", decoded.warnings);
                    assert!(
                        matches!(lost[0], crate::DecodeWarning::ContentLost { kind, path, reason, .. }
                        if kind == "shape_arrow" && !path.0.is_empty() && reason.contains(side))
                    );
                    assert_loss_rejects_editors(&bad, "shape_arrow");
                }
            }
        }
    }

    /// 이것을 실패시키는 것: UnknownEnumValue 전체를 손실로 분류/B' 값 제거.
    #[test]
    fn preserved_absent_arrow_values_and_enum_warning_allow_editing() {
        let outline = r##"<hp:lineShape color="#000000" width="33" style="SOLID" headStyle="NORMAL" tailStyle="NORMAL" headSz="LARGE_LARGE" tailSz="SMALL_SMALL" headfill="0" tailfill="0"/>"##;
        let shape = loss_test_rect().replace("</hp:rect>", &format!("{outline}</hp:rect>"));
        let bytes = encode(&HwpxDecoder::decode(&package_with_group(&shape)).unwrap());
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        assert!(!decoded.warnings.iter().any(crate::DecodeWarning::is_content_loss));
        let warning = crate::DecodeWarning::UnknownEnumValue {
            attribute: "test@preserved",
            raw: "FUTURE".into(),
            fallback: "preserved",
        };
        assert!(!warning.is_content_loss());
        crate::stamp::reject_input_content_loss(&[warning]).unwrap();
        let stamped = crate::stamp::HwpxStamper::stamp(&bytes, &[]).unwrap();
        assert!(section0_xml(&stamped.bytes).contains("headSz=\"LARGE_LARGE\""));
        crate::HwpxCellEditor::set_cells(&bytes, &[]).unwrap();
    }

    /// 이것을 실패시키는 것: 면 없음+무늬 손실 진단/독립·묶음 공용 경로 삭제.
    #[test]
    fn transparent_pattern_background_is_located_and_rejects_editors() {
        for grouped in [false, true] {
            let brush = r##"<hc:fillBrush><hc:winBrush faceColor="none" hatchColor="#000000" hatchStyle="SLASH" alpha="0"/></hc:fillBrush>"##;
            let shape = loss_test_rect().replace("</hp:rect>", &format!("{brush}</hp:rect>"));
            let shape = if grouped { container((1000, 1000), &shape) } else { shape };
            let bytes = package_with_group(&shape);
            let decoded = HwpxDecoder::decode(&bytes).unwrap();
            assert!(
                matches!(decoded.warnings.as_slice(), [crate::DecodeWarning::ContentLost { kind, reason, part, path }]
                if kind == "shape_pattern_background" && reason.contains("white") && part == "Contents/section0.xml" && !path.0.is_empty()),
                "{:?}",
                decoded.warnings
            );
            assert_loss_rejects_editors(&bytes, "shape_pattern_background");
            // Explicit opaque white is representable and must stay editable.
            let opaque =
                package_with_group(&shape.replace("faceColor=\"none\"", "faceColor=\"#FFFFFF\""));
            let opaque = encode(&HwpxDecoder::decode(&opaque).unwrap());
            assert!(!HwpxDecoder::decode(&opaque)
                .unwrap()
                .warnings
                .iter()
                .any(crate::DecodeWarning::is_content_loss));
            crate::stamp::HwpxStamper::stamp(&opaque, &[]).unwrap();
            crate::HwpxCellEditor::set_cells(&opaque, &[]).unwrap();
        }
    }

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
            let name = f.name().expect("ZIP entry name").to_string();
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

    /// 빈 참조는 중첩 묶음에서도 실제 경계를 가진 그림 틀이다.
    /// 이것을 실패시키는 것: convert_picture 빈 참조 버리기 / has_image 의
    /// 빈 참조 거부 복원 / Core 묶음 빈 경로 거부 복원.
    #[test]
    fn assetless_picture_survives_nested_group_roundtrip() {
        let picture = pic((1000, 600), &ri(ID, &[(ID, ID), (ID, ID), (ID, ID)]), "", "");
        let inner = nested((1000, 600), &ri(ID, &[(ID, ID), (ID, ID)]), &picture);
        let bytes = package_with_group(&container((1000, 600), &inner));
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
        let group = first_group(&decoded);
        let inner = children(&group)[0];
        assert_eq!(pictures(inner), [(String::new(), 1000, 600, (0.0, 0.0))]);
        let encoded = encode(&decoded);
        let xml = section0_xml(&encoded);
        assert_eq!(xml.matches("<hp:pic ").count(), 1);
        assert!(xml.contains(r#"binaryItemIDRef="""#));
        let back = HwpxDecoder::decode(&encoded).unwrap();
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        let group = first_group(&back);
        let inner = children(&group)[0];
        assert_eq!(pictures(inner), [(String::new(), 1000, 600, (0.0, 0.0))]);
    }

    /// 기존 계약 고정: 크기 0 묶음 그림은 fold_box에서 CONTENT_LOST 후 제외된다.
    #[test]
    fn zero_size_assetless_group_picture_is_dropped_without_rejecting_document() {
        for size in [(0, 600), (1000, 0), (0, 0)] {
            let picture = pic(size, &ri(ID, &[(ID, ID), (ID, ID)]), "", "");
            let bytes = package_with_group(&container((1000, 600), &picture));
            let decoded = HwpxDecoder::decode(&bytes).unwrap();
            assert!(lost(&decoded.warnings)
                .iter()
                .any(|(kind, _)| kind == "group_child_degenerate"));
            assert!(!section0_xml(&encode(&decoded)).contains("<hp:pic "));
            assert!(decoded.document.validate().is_ok());
        }
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

    /// A group picture with no `<hp:sz>` and `curSz` 0, scaled by its chain
    /// (×2 × ×0.5): its original box `orgSz` 1000 × 600 survives the fold,
    /// and the writer puts it back with the scale between.
    ///
    /// 이것을 실패시키는 것: 접기 뒤 재판정을 지우기(`convert_picture` 의 임시
    /// 표시 크기 판정으로 `None` 이 되어 `orgSz` 가 2000 × 300 으로 써진다).
    #[test]
    fn group_picture_keeps_its_original_box() {
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
        ]
        .concat();
        let (g, w) = decode(&container((3000, 3000), &children));
        assert!(w.is_empty(), "{w:?}");
        let g = g.unwrap();
        let Control::Group { children, .. } = &g else { unreachable!() };
        // Not scaled: the original box is the displayed one, no frame.
        let GroupChild::Picture(plain) = &children[1] else { panic!("{children:?}") };
        assert!(plain.frame.is_unset(), "{:?}", plain.frame);
        let GroupChild::Picture(image) = &children[0] else { panic!("{children:?}") };
        assert_eq!((image.width.as_i32(), image.height.as_i32()), (2000, 300));
        let u = |v| hwpforge_foundation::HwpUnit::new(v).unwrap();
        assert_eq!(
            image.frame.original_size,
            Some(hwpforge_foundation::Size::new(u(1000), u(600)))
        );
        let xml = crate::encoder::shapes::encode_group_to_xml(
            &g,
            0,
            &mut Vec::new(),
            crate::encoder::EncodeOptions::default(),
            &mut crate::encoder::section::DocumentEncodeContext::new(0),
        )
        .unwrap();
        assert!(xml.contains(r#"<hp:orgSz width="1000" height="600"/>"#), "{xml}");
        assert!(xml.contains(r#"e1="2.000000""#) && xml.contains(r#"e5="0.500000""#), "{xml}");
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

    /// Hancom's F3d (a picture turned 30°) and F3e (one in greyscale):
    /// each reports what a Core picture cannot show, once, and keeps the
    /// picture.
    ///
    /// 이것을 실패시키는 것: 판정 표의 회전·효과 행을 각각 지우는 것(그 파일만
    /// 경고가 사라진다).
    #[test]
    fn hancom_picture_turn_and_effect_are_reported() {
        for (name, reason) in [
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

    fn reasons(pic: &crate::schema::section::HxPic) -> Vec<String> {
        super::group_picture_losses(pic)
            .into_iter()
            .map(|r| r.split(':').next().unwrap().to_string())
            .collect()
    }

    /// The crop a picture decodes to (`[left, top, right, bottom]`), its
    /// extent, and the kinds of the warnings it raised (`UnknownEnumValue`
    /// as its attribute).
    fn crop_of(
        pic: &crate::schema::section::HxPic,
        pixels: &[(&str, (u32, u32))],
    ) -> (Option<[i32; 4]>, Option<(i32, i32)>, Vec<String>) {
        let mut ctx = crate::decoder::section::DecodeCtx::new(0);
        ctx.image_pixels = pixels.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        let image = crate::decoder::section::convert_picture(pic, 0, &mut ctx).unwrap().unwrap();
        let crop = image.crop.map(|c| [c.left, c.top, c.right, c.bottom].map(|v| v.as_i32()));
        let extent = image.image_extent.map(|(w, h)| (w.as_i32(), h.as_i32()));
        let kinds = ctx
            .warnings
            .iter()
            .map(|w| match w {
                crate::DecodeWarning::ContentLost { kind, .. } => kind.clone(),
                crate::DecodeWarning::UnknownEnumValue { attribute, .. } => attribute.to_string(),
                other => format!("{other:?}"),
            })
            .collect();
        (crop, extent, kinds)
    }

    /// The crop of a picture with `<hp:imgDim>`: the clip read by name;
    /// the whole extent is no crop, anything else — one pixel off included —
    /// is carried exactly with its extent. Hancom's F3c (cropped before
    /// grouping) keeps its crop (0, 0, 7474, 15000) of 22500 x 15000 with
    /// no loss.
    ///
    /// 이것을 실패시키는 것: `imgDim` 이 있을 때 전체 비교를 지우는 것(전체 그림이
    /// 자르기로) / 자르기가 있을 때 범위를 버리는 것 / 묶음 그림 자르기를 다시
    /// 손실로 보고하는 것.
    #[test]
    fn picture_crop_with_image_dim() {
        let whole = bare_pic((0, 22_500, 0, 15_000), Some((22_500, 15_000)));
        assert_eq!(crop_of(&whole, &[]), (None, Some((22_500, 15_000)), vec![]));
        let off = bare_pic((0, 7050, 0, 2460), Some((7080, 2460)));
        assert_eq!(crop_of(&off, &[]), (Some([0, 0, 7050, 2460]), Some((7080, 2460)), vec![]));
        let left = bare_pic((76, 7080, 0, 2460), Some((7080, 2460)));
        assert_eq!(crop_of(&left, &[]).0, Some([76, 0, 7080, 2460]));

        let doc = HwpxDecoder::decode(&fixture("group-f3c-cropped.hwpx")).unwrap();
        assert!(lost(&doc.warnings).is_empty(), "{:?}", lost(&doc.warnings));
        let group = first_group(&doc);
        let crops: Vec<_> = match &group {
            Control::Group { children, .. } => children
                .iter()
                .filter_map(|c| match c {
                    GroupChild::Picture(img) => Some(
                        img.crop.map(|c| [c.left, c.top, c.right, c.bottom].map(|v| v.as_i32())),
                    ),
                    _ => None,
                })
                .collect(),
            _ => unreachable!(),
        };
        assert_eq!(crops.iter().flatten().collect::<Vec<_>>(), [&[0, 0, 7474, 15_000]]);
    }

    /// The crop of a picture without `<hp:imgDim>` (§19.21 H-v4): Hancom
    /// wrote two orders, named and `right`/`top` exchanged. The one valid
    /// reading is taken (exchanged → a warning, the value is exact); both
    /// valid and different → `picture_crop_order` lost; both empty →
    /// `picture_crop` lost. A reading within one pixel of the image's pixel
    /// size × 75 is the whole image; without the pixel size the reading is
    /// kept as a crop with no extent. Corpus values: the 94 x 33 logo
    /// (clip 7080 x 2460 = whole), the crop (0, 1113, 7260, 3851) written
    /// `right=1113 top=7260`, and the two-way (0, 7260, 5618, 9036).
    ///
    /// 이것을 실패시키는 것: 판정 규칙을 "항상 이름대로" 로(맞바뀐 자르기가 깨짐) /
    /// 둘 다 유효할 때 이름대로 고르기(`picture_crop_order` 사라짐) / 둘 다 무효
    /// 갈래 삭제 / 픽셀 전체 판정의 허용 폭을 0 으로 / 픽셀 크기를 범위로 쓰기.
    #[test]
    fn picture_crop_without_image_dim() {
        let logo = [("image2", (94, 33))];
        assert_eq!(crop_of(&bare_pic((0, 7080, 0, 2460), None), &logo), (None, None, vec![]));
        assert_eq!(crop_of(&bare_pic((0, 0, 7080, 2460), None), &logo), (None, None, vec![]));
        assert_eq!(crop_of(&bare_pic((0, 7080, 0, 2460), Some((0, 0))), &logo).0, None);
        assert_eq!(
            crop_of(&bare_pic((0, 7080, 0, 2460), None), &[]),
            (Some([0, 0, 7080, 2460]), None, vec![])
        );
        let photo = [("image2", (2000, 2000))];
        assert_eq!(
            crop_of(&bare_pic((0, 1113, 7260, 3851), None), &photo),
            (Some([0, 1113, 7260, 3851]), None, vec!["hp:imgClip@right/top".to_string()])
        );
        assert_eq!(
            crop_of(&bare_pic((0, 7260, 1113, 3851), None), &photo),
            (Some([0, 1113, 7260, 3851]), None, vec![])
        );
        assert_eq!(
            crop_of(&bare_pic((0, 5618, 7260, 9036), None), &photo),
            (None, None, vec!["picture_crop_order".to_string()])
        );
        assert_eq!(
            crop_of(&bare_pic((0, 0, 0, 0), None), &photo),
            (None, None, vec!["picture_crop".to_string()])
        );
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
        assert_eq!(reasons(&p), ["effect"]);
        let mut p = base();
        p.img.as_mut().unwrap().bright = -10;
        assert_eq!(reasons(&p), ["effect"]);
        let mut p = base();
        p.img.as_mut().unwrap().contrast = 5;
        assert_eq!(reasons(&p), ["effect"]);
        let mut p = base();
        p.img.as_mut().unwrap().alpha = "50".to_string();
        assert_eq!(reasons(&p), ["effect"]);
        let mut p = base();
        p.effects = Some(HxPicEffects { items: vec![HxAnyElement::Element] });
        assert_eq!(reasons(&p), ["effect"]);
        p.effects = Some(HxPicEffects::default());
        assert!(reasons(&p).is_empty());
        let mut p = base();
        p.line_shape = Some(HxLineShape {
            style: "DOT".to_string(),
            width: 75,
            ..HxLineShape::default_solid()
        });
        assert_eq!(reasons(&p), ["border"]);
        p.line_shape =
            Some(HxLineShape { style: "NONE".to_string(), ..HxLineShape::default_solid() });
        assert!(reasons(&p).is_empty());
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
            let name = f.name().expect("ZIP entry name").to_string();
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

    /// `imgClip` and `imgDim` are any i32 on the wire: the whole-image test
    /// does not overflow at the extremes, and a value outside HWPUNIT's range
    /// is reported lost rather than clamped.
    ///
    /// 이것을 실패시키는 것: 픽셀 비교 뺄셈을 i32 로 되돌리는 것(debug 패닉) / 범위
    /// 밖 값을 0 으로 접는 것(손실 경고 없이 자르기가 바뀜).
    #[test]
    fn crop_rule_survives_extreme_values() {
        let (min, max) = (i32::MIN, i32::MAX);
        let lost = |pic| crop_of(&pic, &[("image2", (u32::MAX, u32::MAX))]).2;
        assert_eq!(lost(bare_pic((0, min, 0, 15_000), Some((max, 15_000)))), ["picture_crop"]);
        assert_eq!(
            lost(bare_pic((min, 22_500, min, 15_000), Some((22_500, 15_000)))),
            ["picture_crop"]
        );
        assert_eq!(lost(bare_pic((0, max, 0, max), None)), ["picture_crop"]);
        assert_eq!(lost(bare_pic((min, max, min, max), None)), ["picture_crop"]);
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
            let name = f.name().expect("ZIP entry name").to_string();
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

    // ── S20: original boxes in groups (§19.23.13–§19.23.17) ─────────

    fn s20_size(w: i32, h: i32) -> hwpforge_foundation::Size {
        let u = |v| hwpforge_foundation::HwpUnit::new(v).unwrap();
        hwpforge_foundation::Size::new(u(w), u(h))
    }

    /// Our `<hp:container>` for a decoded group.
    fn group_out(g: &Control) -> String {
        crate::encoder::shapes::encode_group_to_xml(
            g,
            0,
            &mut Vec::new(),
            crate::encoder::EncodeOptions::default(),
            &mut crate::encoder::section::DocumentEncodeContext::new(0),
        )
        .unwrap()
    }

    /// (width, height, offset, original size) of a group node.
    fn framed(c: &Control) -> (i32, i32, (f64, f64), Option<hwpforge_foundation::Size>) {
        let (w, h, o) = size_and_offset(c);
        (w, h, o, c.frame().and_then(|f| f.original_size))
    }

    /// §19.23.13 H2: a group scaled ×2 holding a rect scaled ×3 at
    /// (100, 40). By hand the rect's box 100 × 50 lands at (100, 40)–(700,
    /// 340): Core shows it 600 × 300 there and keeps 100 × 50 as its original
    /// box (the group keeps 350 × 170). The writer stores the group's pair and
    /// the rect's own after it, as Hancom does, and decoding that gives the
    /// same Core.
    ///
    /// 이것을 실패시키는 것: 축 정렬 잎 갈래를 지우기(원래 상자 `None`) / 묶음의
    /// 원래 상자 판정을 지우기 / 노드 사슬 쓰기를 지우기(쌍 하나만 써진다).
    #[test]
    fn parent_and_child_scales_land_where_hancom_draws() {
        let two = [2.0, 0.0, 0.0, 0.0, 2.0, 0.0];
        let three = [3.0, 0.0, 0.0, 0.0, 3.0, 0.0];
        let leaf =
            rect((100, 50), &ri([1.0, 0.0, 100.0, 0.0, 1.0, 40.0], &[(two, ID), (three, ID)]));
        let (g, w) = decode(&container_with((350, 170), (700, 340), &ri(ID, &[(two, ID)]), &leaf));
        assert!(w.is_empty(), "{w:?}");
        let g = g.unwrap();
        assert_eq!(framed(children(&g)[0]), (600, 300, (100.0, 40.0), Some(s20_size(100, 50))));
        assert_eq!(g.frame().unwrap().original_size, Some(s20_size(350, 170)));
        let out = group_out(&g);
        assert!(out.contains(r#"<hp:orgSz width="350" height="170"/>"#), "{out}");
        assert!(
            out.contains(r#"<hc:transMatrix e1="1" e2="0" e3="100" e4="0" e5="1" e6="40"/><hc:scaMatrix e1="2.000000" e2="0" e3="0" e4="0" e5="2.000000" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:scaMatrix e1="3.000000" e2="0" e3="0" e4="0" e5="3.000000" e6="0"/>"#),
            "{out}"
        );
        let (back, w) = decode(&out);
        assert!(w.is_empty(), "{w:?}");
        let back = back.unwrap();
        assert_eq!(framed(children(&back)[0]), framed(children(&g)[0]));
        assert_eq!(back.frame(), g.frame());
    }

    /// Three levels, every chain axis-aligned: top ×2, middle ×1.5, inner
    /// ×1, the rect 100 × 100 at (30, 20) — by hand at (30, 20)–(330, 320).
    /// Every group and the rect keep their original boxes, and the rect's
    /// four pairs come back.
    /// 이것을 실패시키는 것: 중첩 묶음을 정렬 판정에서 빼기 / 조상 쌍 누적을
    /// 지우기(재디코드 위치가 어긋난다).
    #[test]
    fn three_level_aligned_group_keeps_every_box() {
        let two = [2.0, 0.0, 0.0, 0.0, 2.0, 0.0];
        let one_half = [1.5, 0.0, 0.0, 0.0, 1.5, 0.0];
        let t = [1.0, 0.0, 30.0, 0.0, 1.0, 20.0];
        let leaf = rect((100, 100), &ri(t, &[(two, ID), (one_half, ID), (ID, ID), (ID, ID)]))
            .replace(r#"groupLevel="1""#, r#"groupLevel="3""#);
        let inner = nested((100, 100), &ri(t, &[(two, ID), (one_half, ID), (ID, ID)]), &leaf)
            .replace(r#"groupLevel="1""#, r#"groupLevel="2""#);
        let mid = nested((100, 100), &ri(t, &[(two, ID), (one_half, ID)]), &inner);
        let (g, w) = decode(&container_with((165, 160), (330, 320), &ri(ID, &[(two, ID)]), &mid));
        assert!(w.is_empty(), "{w:?}");
        let g = g.unwrap();
        let mid = children(&g)[0];
        let inner = children(mid)[0];
        let leaf = children(inner)[0];
        assert_eq!(framed(mid), (300, 300, (30.0, 20.0), Some(s20_size(100, 100))));
        assert_eq!(framed(inner), (300, 300, (0.0, 0.0), Some(s20_size(100, 100))));
        assert_eq!(framed(leaf), (300, 300, (0.0, 0.0), Some(s20_size(100, 100))));
        let out = group_out(&g);
        let (back, w) = decode(&out);
        assert!(w.is_empty(), "{w:?}");
        let back = back.unwrap();
        let leaf2 = children(children(children(&back)[0])[0])[0];
        assert_eq!(framed(leaf2), framed(leaf));
        assert_eq!(framed(children(&back)[0]), framed(mid));
    }

    /// A group with one axis-aligned rect (scaled ×2) and one turned 30°: the
    /// aligned rect keeps its original box, the turned one folds as before
    /// (no original box, rotation 30), and the group keeps none — one of its
    /// leaves is not axis-aligned (§19.23.13 표).
    /// 이것을 실패시키는 것: 묶음 정렬 판정에서 잎 하나만 보기 / 축 정렬 판정에서
    /// 돌림을 무시하기.
    #[test]
    fn mixed_group_keeps_aligned_leaves_only() {
        let two = [2.0, 0.0, 0.0, 0.0, 2.0, 0.0];
        let aligned = rect((100, 50), &ri(ID, &[(ID, ID), (two, ID)]));
        let turned =
            rect((100, 50), &ri([1.0, 0.0, 500.0, 0.0, 1.0, 0.0], &[(ID, ID), (ID, rot(30.0))]));
        let (g, w) = decode(&container_with(
            (1000, 400),
            (1000, 400),
            &ri(ID, &[(ID, ID)]),
            &format!("{aligned}{turned}"),
        ));
        assert!(w.is_empty(), "{w:?}");
        let g = g.unwrap();
        assert_eq!(framed(children(&g)[0]).3, Some(s20_size(100, 50)));
        let Control::Rect { frame, style, .. } = children(&g)[1] else { panic!() };
        assert_eq!(frame.original_size, None);
        assert_eq!(style.as_ref().and_then(|s| s.rotation), Some(30.0));
        assert!(g.frame().unwrap().original_size.is_none());
        let flipped = aligned.replace(
            r#"<hp:flip horizontal="0" vertical="0"/>"#,
            r#"<hp:flip horizontal="1" vertical="0"/>"#,
        );
        let (g, _) =
            decode(&container_with((1000, 400), (1000, 400), &ri(ID, &[(ID, ID)]), &flipped));
        assert!(
            framed(children(&g.unwrap())[0]).3.is_none(),
            "a mirrored leaf is not axis-aligned"
        );
    }

    /// A group of our own (Core) scaled ×2 holding a 30° text box, a 30°
    /// polygon with text and a 30° polygon with a gradient: none is
    /// axis-aligned, so each stores `inverse(group pair) · R(30°)` after the
    /// group's pair — the turn is read from each Core value's `ShapeStyle`,
    /// whatever the kind (§19.23.17 H1). Decoded again, every shape lands
    /// where Core draws it.
    /// 이것을 실패시키는 것: 다각형의 θ 를 종류로 0 으로 정하기 / 자기 쌍에서
    /// 조상 배율을 빼지 않기.
    #[test]
    fn turned_siblings_under_a_scaled_group_keep_their_turn() {
        use hwpforge_core::control::geometry::{emitted_corners, EmissionPolicy};
        let (g, _) = decode(&turned_family(""));
        let mut g = g.unwrap();
        // Scale the whole group ×2 through its own frame: the box doubles
        // around its original one.
        let (gw, gh, _) = size_and_offset(&g);
        if let Control::Group { frame, .. } = &mut g {
            frame.original_size = Some(s20_size(gw / 2, gh / 2));
        }
        // The pairs store the turns exactly enough: no fallback, two pairs.
        let plan =
            hwpforge_core::control::geometry::group_plan(&g, &crate::encoder::shapes::HWPX_POLICY)
                .unwrap();
        assert!(!plan.is_fallback());
        assert!(plan.nodes().iter().all(|n| n.pairs().len() == 2));
        let out = group_out(&g);
        let (back, w) = decode(&out);
        let back = back.unwrap();
        assert!(w.is_empty(), "{w:?}");
        for (a, b) in children(&g).iter().zip(children(&back)) {
            let ca = emitted_corners(a, &EmissionPolicy::exact()).unwrap();
            let cb = emitted_corners(b, &EmissionPolicy::exact()).unwrap();
            assert!(dist(&ca, &cb) <= 2.0, "{:?} vs {:?}", ca, cb);
        }
    }

    /// The three turned siblings (text box, text polygon, gradient polygon,
    /// each 30°) plus `extra`, in an unscaled group.
    fn turned_family(extra: &str) -> String {
        let at = |x: f64| [1.0, 0.0, x, 0.0, 1.0, 100.0];
        let text_box = format!(
            r#"{}<hp:drawText lastWidth="0" name="" editable="0">{}</hp:drawText></hp:rect>"#,
            shape_head("rect", (1000, 600), &ri(at(500.0), &[(ID, ID), (ID, rot(30.0))])),
            sublist("<hp:t>글</hp:t>")
        );
        let tri = [(0, 600), (500, 0), (1000, 600)];
        let text_poly = polygon(
            (1000, 600),
            &ri(at(2000.0), &[(ID, ID), (ID, rot(30.0))]),
            &tri,
            &draw_text("글"),
        );
        let gradient = r##"<hc:fillBrush><hc:gradation type="LINEAR" angle="0" centerX="0" centerY="0" step="255" colorNum="2" stepCenter="50" alpha="0"><hc:color value="#FF0000"/><hc:color value="#0000FF"/></hc:gradation></hc:fillBrush>"##;
        let grad_poly =
            polygon((1000, 600), &ri(at(3500.0), &[(ID, ID), (ID, rot(30.0))]), &tri, "").replacen(
                "<hc:pt ",
                &format!("{gradient}<hc:pt "),
                1,
            );
        // No `<hp:sz>`: the displayed size is the shapes' extent.
        container((5000, 1500), &format!("{text_box}{text_poly}{grad_poly}{extra}"))
    }

    /// §19.23.15 H1: an axis-aligned leaf whose original box is far too
    /// large to store its scale (100,000,000 shown 49) makes the whole group
    /// fall back — but only the original boxes go: the three 30° siblings
    /// keep their `rotMatrix` 30° and land within 2 of where Core draws them.
    /// 이것을 실패시키는 것: fallback 이 형제의 회전까지 펴기 / 잎 하나만
    /// fallback 하기.
    #[test]
    fn fallback_keeps_the_siblings_turned() {
        use hwpforge_core::control::geometry::{emitted_corners, group_plan, EmissionPolicy};
        let tiny = [0.00000049, 0.0, 0.0, 0.0, 1.0, 0.0];
        let extreme = rect(
            (100_000_000, 100),
            &ri([1.0, 0.0, 5000.0, 0.0, 1.0, 0.0], &[(ID, ID), (tiny, ID)]),
        );
        let (g, w) = decode(&turned_family(&extreme));
        assert!(w.is_empty(), "{w:?}");
        let g = g.unwrap();
        let plan = group_plan(&g, &crate::encoder::shapes::HWPX_POLICY).unwrap();
        assert!(plan.is_fallback());
        let out = group_out(&g);
        assert_eq!(
            out.matches(r#"<hc:rotMatrix e1="0.866025" e2="-0.500000""#).count(),
            3,
            "{out}"
        );
        assert!(out.contains(r#"<hp:orgSz width="49" height="100"/>"#), "{out}");
        let (back, _) = decode(&out);
        for (a, b) in children(&g).iter().zip(children(&back.unwrap())).take(3) {
            let ca = emitted_corners(a, &EmissionPolicy::exact()).unwrap();
            let cb = emitted_corners(b, &EmissionPolicy::exact()).unwrap();
            assert!(dist(&ca, &cb) <= 2.0, "{:?} vs {:?}", ca, cb);
        }
    }

    /// S20 review r1: an aligned group leaf whose `orgSz` is wrapped
    /// (4294967295) is reported and kept without an original box — no
    /// negative frame value reaches Core.
    #[test]
    fn negative_original_size_in_a_group_is_reported() {
        let two = [2.0, 0.0, 0.0, 0.0, 2.0, 0.0];
        let leaf = rect((100, 50), &ri(ID, &[(ID, ID), (two, ID)])).replace(
            r#"<hp:orgSz width="100" height="50"/>"#,
            r#"<hp:orgSz width="4294967295" height="50"/>"#,
        );
        let (g, w) = decode(&container_with((1000, 400), (1000, 400), &ri(ID, &[(ID, ID)]), &leaf));
        assert!(lost_kinds(&w).iter().any(|k| k == "object_original_size"), "{w:?}");
        let g = g.unwrap();
        for c in children(&g) {
            assert!(
                c.frame().and_then(|f| f.original_size).is_none_or(|o| o.width.as_i32() >= 0),
                "{c:?}"
            );
        }
    }

    /// §19.23.17 M2 (b): an axis-aligned leaf B next to the extreme leaf A.
    /// The plan falls back for the whole group, so B too is written with its
    /// displayed box; the decoder measures that same plan against the source
    /// (no report: it lands within 2), and what is written is the plan.
    /// 이것을 실패시키는 것: 인코더가 묶음 계획을 무시하기(B 의 `orgSz` 가 원래
    /// 크기로) / 디코더가 fallback 전 단계까지만 예측하기(원래 상자 버림 보고).
    #[test]
    fn sibling_fallback_is_predicted() {
        use hwpforge_core::control::geometry::group_plan;
        let two = [2.0, 0.0, 0.0, 0.0, 2.0, 0.0];
        let b = rect((100, 50), &ri(ID, &[(ID, ID), (two, ID)]));
        let tiny = [0.00000049, 0.0, 0.0, 0.0, 1.0, 0.0];
        let a = rect(
            (100_000_000, 100),
            &ri([1.0, 0.0, 400.0, 0.0, 1.0, 0.0], &[(ID, ID), (tiny, ID)]),
        );
        let (g, w) = decode(&container_with(
            (1000, 400),
            (1000, 400),
            &ri(ID, &[(ID, ID)]),
            &format!("{b}{a}"),
        ));
        assert!(w.is_empty(), "{w:?}");
        let g = g.unwrap();
        assert_eq!(framed(children(&g)[0]).3, Some(s20_size(100, 50)), "Core keeps B's box");
        let plan = group_plan(&g, &crate::encoder::shapes::HWPX_POLICY).unwrap();
        assert!(plan.is_fallback());
        assert_eq!(plan.nodes()[0].original_size(), (200, 100));
        let out = group_out(&g);
        assert!(out.contains(r#"<hp:orgSz width="200" height="100"/>"#), "{out}");
        assert!(!out.contains(r#"<hp:orgSz width="100" height="50"/>"#), "{out}");
    }

    /// A frameless sibling the writer cannot store exactly (a large box
    /// turned 30.4°, stored as 30°) does not make the group fall back: the
    /// frames do not change it, so the aligned rect keeps its original box
    /// (corpus case: a picture next to a sheared ellipse, §19.23.20).
    /// 이것을 실패시키는 것: fallback 판정에 frame 없는 노드도 세기.
    #[test]
    fn frameless_sibling_does_not_drop_the_boxes() {
        use hwpforge_core::control::geometry::group_plan;
        let two = [2.0, 0.0, 0.0, 0.0, 2.0, 0.0];
        let b = rect((100, 50), &ri(ID, &[(ID, ID), (two, ID)]));
        let c = rect(
            (10_000, 1000),
            &ri([1.0, 0.0, 400.0, 0.0, 1.0, 0.0], &[(ID, ID), (ID, rot(30.4))]),
        );
        let (g, _) = decode(&container_with(
            (12_000, 6000),
            (12_000, 6000),
            &ri(ID, &[(ID, ID)]),
            &format!("{b}{c}"),
        ));
        let g = g.unwrap();
        let plan = group_plan(&g, &crate::encoder::shapes::HWPX_POLICY).unwrap();
        assert!(!plan.is_fallback());
        assert!(group_out(&g).contains(r#"<hp:orgSz width="100" height="50"/>"#));
    }

    /// A turned leaf in a nested group makes the outer group not
    /// axis-aligned too: neither group keeps an original box.
    /// 이것을 실패시키는 것: 묶음 정렬 판정이 중첩 묶음을 보지 않기.
    #[test]
    fn nested_turn_unframes_the_outer_group() {
        let two = [2.0, 0.0, 0.0, 0.0, 2.0, 0.0];
        let turned = rect(
            (100, 50),
            &ri([1.0, 0.0, 500.0, 0.0, 1.0, 0.0], &[(two, ID), (ID, ID), (ID, rot(30.0))]),
        )
        .replace(r#"groupLevel="1""#, r#"groupLevel="2""#);
        let inner = nested(
            (100, 50),
            &ri([1.0, 0.0, 500.0, 0.0, 1.0, 0.0], &[(two, ID), (ID, rot(30.0))]),
            &turned,
        );
        let aligned = rect((100, 50), &ri(ID, &[(two, ID), (ID, ID)]));
        let (g, _) = decode(&container_with(
            (600, 400),
            (1200, 800),
            &ri(ID, &[(two, ID)]),
            &format!("{aligned}{inner}"),
        ));
        let g = g.unwrap();
        assert!(g.frame().unwrap().original_size.is_none());
        assert_eq!(framed(children(&g)[0]).3, Some(s20_size(100, 50)));
    }

    /// §19.23.15 M3 in a group: the leaf's source end is 1002.49 (original
    /// 5,000,000, scale 0.000200498); with its original box the writer would
    /// store 1000 — 2.49 away. The decoder drops the group's original boxes
    /// (reported) and the leaf lands at 1002.
    /// 이것을 실패시키는 것: 묶음 판정을 지우기(원래 상자가 남아 1000 으로 써진다).
    #[test]
    fn group_rounding_drops_the_boxes() {
        let s = [0.000200498, 0.0, 0.0, 0.0, 1.0, 0.0];
        let leaf = rect((5_000_000, 100), &ri(ID, &[(ID, ID), (s, ID)]));
        let (g, w) = decode(&container((2000, 200), &leaf));
        assert_eq!(lost_kinds(&w), ["object_original_size"]);
        let g = g.unwrap();
        assert!(framed(children(&g)[0]).3.is_none());
        assert!(group_out(&g).contains(r#"<hp:orgSz width="1002" height="100"/>"#));
    }

    /// A group whose own matrices draw a box other than its layout size
    /// (`orgSz` 1000 under an identity matrix, `sz` 800): neither its
    /// original box nor its displayed one is drawn where Hancom draws it —
    /// `shape_drawn_extent`, the original box kept.
    /// 이것을 실패시키는 것: 묶음 판정을 지우기(보고가 없다).
    #[test]
    fn group_box_its_matrices_do_not_draw_is_reported() {
        let leaf = rect((500, 500), &ri(ID, &[(ID, ID), (ID, ID)]));
        let (g, w) = decode(&container_with((1000, 1000), (800, 800), &ri(ID, &[(ID, ID)]), &leaf));
        assert_eq!(lost_kinds(&w), ["shape_drawn_extent"]);
        assert_eq!(g.unwrap().frame().unwrap().original_size, Some(s20_size(1000, 1000)));
    }

    /// Branch review B: a group whose box its matrices do not draw is
    /// decoded twice (with and without original boxes). Nested through a
    /// child's text — group → polygon text → group … — the inner groups must
    /// not be decoded again for every outer retry: the work stays linear in
    /// the depth (it doubled per level: depth 16 took about 0.6 s).
    /// 이것을 실패시키는 것: 안쪽 묶음 결과를 다시 쓰지 않기(디코드 횟수 2^깊이) ·
    /// 단계마다 자리표 해소(작업량 깊이²) · 자리표를 남기기.
    #[test]
    fn nested_group_retries_stay_linear() {
        fn nest(depth: usize) -> String {
            let leaf = rect((500, 500), &ri(ID, &[(ID, ID), (ID, ID)]));
            let mut xml = container_with((1000, 1000), (800, 800), &ri(ID, &[(ID, ID)]), &leaf);
            for _ in 1..depth {
                let text = format!(
                    r#"<hp:drawText lastWidth="0" name="" editable="0"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0">{xml}</hp:run></hp:p></hp:subList></hp:drawText>"#
                );
                let pts = [(0, 0), (500, 0), (500, 500)];
                let shape = polygon((500, 500), &ri(ID, &[(ID, ID), (ID, ID)]), &pts, &text);
                xml = container_with(
                    (1000, 1000),
                    (800, 800),
                    &ri(ID, &[(ID, ID)]),
                    &format!("{leaf}{shape}"),
                );
            }
            xml
        }
        // Every group of the chain is in the tree once and no placeholder is
        // left: groups counted through paragraphs (text bodies included).
        fn groups(control: &Control) -> (usize, usize) {
            let mut found = (0, 0);
            let mut holder = hwpforge_core::paragraph::Paragraph::new(
                hwpforge_foundation::ParaShapeIndex::new(0),
            );
            holder.runs.push(hwpforge_core::run::Run::control(
                control.clone(),
                hwpforge_foundation::CharShapeIndex::new(0),
            ));
            holder.for_each_paragraph(|p| {
                for r in &p.runs {
                    match r.content.as_control() {
                        Some(Control::Group { .. }) => found.0 += 1,
                        Some(Control::Unknown { .. }) => found.1 += 1,
                        _ => {}
                    }
                }
            });
            found
        }
        let mut counts = Vec::new();
        let mut work = Vec::new();
        for depth in [4, 8] {
            super::CONTAINER_DECODES.with(|n| n.set(0));
            crate::work::WORK.with(|w| w.set(0));
            let (g, w) = decode(&nest(depth));
            work.push(crate::work::WORK.with(std::cell::Cell::get));
            assert_eq!(groups(g.as_ref().unwrap()), (depth, 0));
            assert_eq!(
                lost_kinds(&w).iter().filter(|k| *k == "shape_drawn_extent").count(),
                depth,
                "{w:?}"
            );
            counts.push(super::CONTAINER_DECODES.with(std::cell::Cell::get));
        }
        // Two decodes per group: 8 and 16.
        assert_eq!(counts, [8, 16]);
        // r2 M3: holding and placing the groups is linear too — no copy per
        // level (the work doubles with the depth, it does not quadruple).
        assert!(work[1] <= 2 * work[0] + 4, "{work:?}");
    }

    /// Branch review r4 H1: a chain of groups nested through their text
    /// boxes' text, where every text box has an unreadable matrix (so every
    /// group loses its only child and is lost), reports each warning once —
    /// the work and the warnings grow with the depth, they do not double per
    /// level. A chain where only the outermost group survives does the same.
    /// 이것을 실패시키는 것: 잃은 묶음의 경고를 부모에도 넣기(층마다 두 배).
    #[test]
    fn lost_nested_groups_report_each_warning_once() {
        fn chain(depth: usize, bad_outer: bool) -> String {
            let bad = ri([f64::NAN, 0.0, 0.0, 0.0, 1.0, 0.0], &[(ID, ID)]);
            let good = ri(ID, &[(ID, ID), (ID, ID)]);
            let pts = [(0, 0), (500, 0), (500, 500)];
            let mut xml = String::new();
            for level in 0..depth {
                let text = format!(
                    r#"<hp:drawText lastWidth="0" name="" editable="0"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0">{xml}</hp:run></hp:p></hp:subList></hp:drawText>"#
                );
                let outermost = level + 1 == depth;
                let matrix = if outermost && !bad_outer { &good } else { &bad };
                let shape = polygon((500, 500), matrix, &pts, &text);
                xml = container_with((1000, 1000), (1000, 1000), &ri(ID, &[(ID, ID)]), &shape);
            }
            xml
        }
        for bad_outer in [true, false] {
            let mut counts = Vec::new();
            for depth in [4, 8] {
                crate::work::WORK.with(|w| w.set(0));
                let (_, warnings) = decode(&chain(depth, bad_outer));
                let work = crate::work::WORK.with(std::cell::Cell::get);
                let texts: Vec<String> = warnings.iter().map(|w| format!("{w:?}")).collect();
                let mut unique = texts.clone();
                unique.sort();
                unique.dedup();
                assert_eq!(unique.len(), texts.len(), "a warning reported twice: {texts:?}");
                counts.push((texts.len(), work));
            }
            let ((w4, k4), (w8, k8)) = (counts[0], counts[1]);
            assert!(w8 <= 2 * w4 + 4 && k8 <= 2 * k4 + 8, "{bad_outer}: {counts:?}");
        }
    }

    /// Branch review r4 H1: the same through retries, both ways — each
    /// group keeps its first decode (`shape_drawn_extent`) or takes its
    /// retry (`object_original_size`) — with a lost group at the bottom:
    /// every warning once, work linear in the depth.
    /// 이것을 실패시키는 것: 재시도에서 경고를 슬롯과 부모 양쪽에 두기.
    #[test]
    fn retried_chains_report_each_warning_once() {
        fn chain(depth: usize, take_retry: bool) -> String {
            let bad = ri([f64::NAN, 0.0, 0.0, 0.0, 1.0, 0.0], &[(ID, ID)]);
            let pts = [(0, 0), (500, 0), (500, 500)];
            let lost = polygon((500, 500), &bad, &pts, "");
            let mut xml = container_with((1000, 1000), (1000, 1000), &ri(ID, &[(ID, ID)]), &lost);
            for _ in 0..depth {
                let text = format!(
                    r#"<hp:drawText lastWidth="0" name="" editable="0"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0">{xml}</hp:run></hp:p></hp:subList></hp:drawText>"#
                );
                let carrier = polygon((500, 500), &ri(ID, &[(ID, ID), (ID, ID)]), &pts, &text);
                xml = if take_retry {
                    let s = [0.000200498, 0.0, 0.0, 0.0, 1.0, 0.0];
                    let leaf = rect((5_000_000, 100), &ri(ID, &[(ID, ID), (s, ID)]));
                    container_xml(
                        (2000, 200),
                        None,
                        &ri(ID, &[(ID, ID)]),
                        &format!("{leaf}{carrier}"),
                    )
                } else {
                    let leaf = rect((500, 500), &ri(ID, &[(ID, ID), (ID, ID)]));
                    container_with(
                        (1000, 1000),
                        (800, 800),
                        &ri(ID, &[(ID, ID)]),
                        &format!("{leaf}{carrier}"),
                    )
                };
            }
            xml
        }
        for take_retry in [false, true] {
            let mut counts = Vec::new();
            for depth in [4, 8] {
                crate::work::WORK.with(|w| w.set(0));
                let (g, warnings) = decode(&chain(depth, take_retry));
                assert!(g.is_some());
                let work = crate::work::WORK.with(std::cell::Cell::get);
                let texts: Vec<String> = warnings.iter().map(|w| format!("{w:?}")).collect();
                let mut unique = texts.clone();
                unique.sort();
                unique.dedup();
                assert_eq!(unique.len(), texts.len(), "a warning reported twice: {texts:?}");
                let kind = if take_retry { "object_original_size" } else { "shape_drawn_extent" };
                assert_eq!(lost_kinds(&warnings).iter().filter(|k| *k == kind).count(), depth);
                counts.push((texts.len(), work));
            }
            let ((w4, k4), (w8, k8)) = (counts[0], counts[1]);
            assert!(w8 <= 2 * w4 + 4 && k8 <= 2 * k4 + 8, "{take_retry}: {counts:?}");
        }
    }

    /// Branch review r3 H1: groups inside notes whose first paragraph
    /// starts with the encoder's number head are decoded from a trimmed copy
    /// of that paragraph. Two such notes in one text box of an outer group,
    /// each holding its own group, keep their own content: nothing keys a
    /// kept group by the copy's address.
    /// 이것을 실패시키는 것: 임시 복제본 주소로 재사용 키 삼기(주소가 겹치면 B 가 A 가 된다).
    #[test]
    fn groups_in_note_heads_keep_their_own_content() {
        use hwpforge_core::paragraph::Paragraph;
        use hwpforge_core::run::Run;
        use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
        let size = |v| HwpUnit::new(v).unwrap();
        let para = |runs: Vec<Run>| Paragraph::with_runs(runs, ParaShapeIndex::new(0));
        let text = |t: &str| Run::text(t, CharShapeIndex::new(0));
        let group = |label: &str| Control::Group {
            frame: hwpforge_core::ObjectFrame::new(),
            children: vec![GroupChild::Shape(Control::text_box(
                vec![para(vec![text(label)])],
                size(2000),
                size(1000),
            ))],
            width: size(2000),
            height: size(1000),
            placement: None,
            inst_id: None,
        };
        let note = |label: &str| {
            Control::footnote(vec![para(vec![Run::control(group(label), CharShapeIndex::new(0))])])
        };
        let body = para(vec![
            Run::control(note("A"), CharShapeIndex::new(0)),
            Run::control(note("B"), CharShapeIndex::new(0)),
        ]);
        let outer = group_of_text_box(vec![body]);
        let mut doc = hwpforge_core::Document::new();
        doc.add_section(hwpforge_core::Section::with_paragraphs(
            vec![para(vec![Run::control(outer, CharShapeIndex::new(0))])],
            hwpforge_core::PageSettings::a4(),
        ));
        let styles = crate::style_store_for_preset("default").unwrap();
        let bytes = crate::HwpxEncoder::encode(
            &doc.validate().unwrap(),
            &styles,
            &hwpforge_core::image::ImageStore::new(),
        )
        .unwrap();
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let mut labels = Vec::new();
        let mut placeholders = 0;
        for p in &decoded.document.sections()[0].paragraphs {
            p.for_each_paragraph(|p| {
                for r in &p.runs {
                    match r.content.as_control() {
                        Some(Control::Unknown { .. }) => placeholders += 1,
                        Some(Control::Group { children, .. }) => {
                            if let [GroupChild::Shape(Control::TextBox { paragraphs, .. })] =
                                &children[..]
                            {
                                labels.push(
                                    paragraphs[0]
                                        .runs
                                        .iter()
                                        .filter_map(|r| r.content.as_text())
                                        .collect::<String>(),
                                );
                            }
                        }
                        _ => {}
                    }
                }
            });
        }
        assert_eq!(placeholders, 0);
        assert!(
            labels.contains(&"A".to_string()) && labels.contains(&"B".to_string()),
            "{labels:?}"
        );
    }

    /// A placeholder whose group is missing is an error, not a run left in
    /// Core.
    /// 이것을 실패시키는 것: 해소 실패를 성공으로 넘기기.
    #[test]
    fn an_unplaced_group_placeholder_is_an_error() {
        let mut run = super::group_placeholder(3, hwpforge_foundation::CharShapeIndex::new(0));
        let mut slots: Vec<Option<super::GroupSlot>> = Vec::new();
        assert!(super::resolve_group_placeholders(&mut run, &mut slots).is_err());
    }

    /// The group decode's own consistency checks: a retry that makes a
    /// nested call its first decode did not, and a kept group the outermost
    /// group never placed, are errors.
    /// 이것을 실패시키는 것: 두 검사 중 하나를 지우기.
    #[test]
    fn group_slot_bookkeeping_errors_are_errors() {
        let leaf = rect((500, 500), &ri(ID, &[(ID, ID), (ID, ID)]));
        let xml = section(&container_with((1000, 1000), (1000, 1000), &ri(ID, &[(ID, ID)]), &leaf));
        let sec: HxSection = crate::decoder::xml_from_str(&xml).unwrap();
        let container = &sec.paragraphs[0].runs[0].containers[0];
        let char_shape = hwpforge_foundation::CharShapeIndex::new(0);

        // Replaying with nothing recorded.
        let mut ctx = crate::decoder::section::DecodeCtx::new(0);
        ctx.group_calls
            .push(crate::decoder::section::GroupCalls { record: Vec::new(), replay: Some(0) });
        assert!(super::decode_container(container, char_shape, 0, &mut ctx).is_err());

        // A kept group nobody placed.
        let mut ctx = crate::decoder::section::DecodeCtx::new(0);
        ctx.group_slots.push(Some(super::GroupSlot {
            run: Some(hwpforge_core::run::Run::text("x", char_shape)),
            placed: false,
            warnings: Vec::new(),
            children: Vec::new(),
        }));
        assert!(super::decode_container(container, char_shape, 0, &mut ctx).is_err());
        // Gathering: a group under placed ones still unplaced, and one slot
        // named twice, are errors; below a lost group nothing is placed.
        let slot = || {
            Some(super::GroupSlot {
                run: Some(hwpforge_core::run::Run::text("x", char_shape)),
                placed: false,
                warnings: Vec::new(),
                children: Vec::new(),
            })
        };
        let mut out = Vec::new();
        assert!(super::gather_slot_warnings(&[0], &mut [slot()], &mut out, true).is_err());
        assert!(super::gather_slot_warnings(&[0], &mut [slot()], &mut out, false).is_ok());
        assert!(super::gather_slot_warnings(&[0, 0], &mut [slot()], &mut out, false).is_err());
        // Without it the same decode succeeds.
        let mut ctx = crate::decoder::section::DecodeCtx::new(0);
        assert!(super::decode_container(container, char_shape, 0, &mut ctx).unwrap().is_some());
    }

    /// An outer group holding one text box with `paragraphs`.
    fn group_of_text_box(paragraphs: Vec<hwpforge_core::paragraph::Paragraph>) -> Control {
        let size = |v| hwpforge_foundation::HwpUnit::new(v).unwrap();
        Control::Group {
            frame: hwpforge_core::ObjectFrame::new(),
            children: vec![GroupChild::Shape(Control::text_box(
                paragraphs,
                size(8000),
                size(4000),
            ))],
            width: size(8000),
            height: size(4000),
            placement: None,
            inst_id: None,
        }
    }

    /// A group's size lock and original box round-trip.
    /// 이것을 실패시키는 것: 묶음 잠금을 읽지 않거나 0 으로 쓰기.
    #[test]
    fn group_frame_round_trips() {
        let two = [2.0, 0.0, 0.0, 0.0, 2.0, 0.0];
        let leaf = rect((100, 50), &ri(ID, &[(two, ID), (ID, ID)]));
        let xml = container_with((100, 50), (200, 100), &ri(ID, &[(two, ID)]), &leaf).replace(
            r#"heightRelTo="ABSOLUTE" protect="0""#,
            r#"heightRelTo="ABSOLUTE" protect="1""#,
        );
        let (g, w) = decode(&xml);
        assert!(w.is_empty(), "{w:?}");
        let g = g.unwrap();
        assert!(g.frame().unwrap().size_protect);
        let out = group_out(&g);
        assert!(out.contains(r#"heightRelTo="ABSOLUTE" protect="1""#), "{out}");
        assert!(out.contains(r#"<hp:orgSz width="100" height="50"/>"#), "{out}");
    }
}

/// S20: an object's original box and size lock through HWPX decode and
/// encode (design §19.23, implementation log §19.23.20).
#[cfg(test)]
mod frame_tests {
    use hwpforge_core::control::geometry::frame_plan;
    use hwpforge_core::control::{Control, ShapePoint};
    use hwpforge_core::image::{Image, ImageFormat};
    use hwpforge_core::run::RunContent;
    use hwpforge_core::{PageSettings, Paragraph, Run, Section};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, Size};

    use crate::encoder::shapes::HWPX_POLICY;

    const POS: &str = r#"<hp:pos treatAsChar="1" affectLSpacing="0" flowWithText="1" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="PARA" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/>"#;

    fn u(v: i32) -> HwpUnit {
        HwpUnit::new(v).unwrap()
    }

    fn size(w: i32, h: i32) -> Size {
        Size::new(u(w), u(h))
    }

    /// A top-level shape element: `orgSz`, `curSz`, one `(sca, rot)` pair,
    /// `sz` with `protect`, then `tail` (its own points).
    fn shape(
        tag: &str,
        org: (i64, i64),
        sz: (i32, i32),
        sca: (&str, &str),
        protect: u32,
        tail: &str,
    ) -> String {
        format!(
            r##"<hp:{tag} id="1" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="0" instid="7" hasArcPr="0" arcType="NORMAL"><hp:offset x="0" y="0"/><hp:orgSz width="{ow}" height="{oh}"/><hp:curSz width="{w}" height="{h}"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="{cx}" centerY="{cy}" rotateimage="1"/><hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:scaMatrix e1="{sx}" e2="0" e3="0" e4="0" e5="{sy}" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo><hp:lineShape color="#000000" width="33" style="SOLID" endCap="FLAT" headStyle="NORMAL" tailStyle="NORMAL" headfill="1" tailfill="1" headSz="MEDIUM_MEDIUM" tailSz="MEDIUM_MEDIUM" outlineStyle="NORMAL" alpha="0"/><hp:sz width="{w}" widthRelTo="ABSOLUTE" height="{h}" heightRelTo="ABSOLUTE" protect="{protect}"/>{POS}{tail}</hp:{tag}>"##,
            ow = org.0,
            oh = org.1,
            w = sz.0,
            h = sz.1,
            cx = sz.0 / 2,
            cy = sz.1 / 2,
            sx = sca.0,
            sy = sca.1,
        )
    }

    fn section(run_inner: &str) -> String {
        format!(
            r#"<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0">{run_inner}</hp:run></hp:p></hs:sec>"#
        )
    }

    /// The first object of one decoded section, and the warnings' kinds.
    fn decode(run_inner: &str) -> (RunContent, Vec<String>) {
        let r = crate::decoder::section::parse_section(
            &section(run_inner),
            0,
            &std::collections::HashMap::new(),
        )
        .unwrap();
        let content = r.paragraphs[0].runs[0].content.clone();
        let kinds = r
            .warnings
            .iter()
            .filter_map(|w| match w {
                crate::DecodeWarning::ContentLost { kind, .. } => Some(kind.clone()),
                _ => None,
            })
            .collect();
        (content, kinds)
    }

    fn control(content: RunContent) -> Control {
        match content {
            RunContent::Control(c) => *c,
            other => panic!("not a control: {other:?}"),
        }
    }

    /// The section XML this crate writes for `content`.
    fn encode(content: RunContent) -> String {
        let run = Run { content, char_shape_id: CharShapeIndex::new(0) };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        );
        crate::encoder::section::encode_section(
            &section,
            0,
            0,
            0,
            0,
            crate::encoder::EncodeOptions::default(),
        )
        .unwrap()
        .xml
    }

    const R6_POINTS: &str = r#"<hc:center x="280" y="280"/><hc:ax1 x="280" y="0"/><hc:ax2 x="560" y="280"/><hc:start1 x="0" y="0"/><hc:end1 x="0" y="0"/><hc:start2 x="0" y="0"/><hc:end2 x="0" y="0"/>"#;

    /// Branch review r4 M1 (same shape): the copy the frame check tries the
    /// dropped box on holds none of the shape's text — an ellipse with
    /// 100,000 bytes of text copies none of them.
    /// 이것을 실패시키는 것: 도형을 통째 복제하기(글 바이트만큼 센다).
    #[test]
    fn frame_checks_copy_no_text() {
        let big = "x".repeat(100_000);
        let text = format!(
            r#"<hp:drawText lastWidth="0" name="" editable="0"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>{big}</hp:t></hp:run></hp:p></hp:subList></hp:drawText>"#
        );
        let xml = shape(
            "ellipse",
            (560, 560),
            (850, 850),
            ("1.517943", "1.517943"),
            1,
            &format!("{text}{R6_POINTS}"),
        );
        crate::work::WORK.with(|w| w.set(0));
        let (content, _) = decode(&xml);
        let copied = crate::work::WORK.with(std::cell::Cell::get);
        let Control::Ellipse { ref paragraphs, frame, .. } = control(content) else { panic!() };
        assert_eq!(frame.original_size, Some(size(560, 560)), "the frame check ran");
        assert_eq!(paragraphs[0].runs[0].content.as_text().map(str::len), Some(100_000));
        assert_eq!(copied, 0);
    }

    /// FR-T1/T2: the ministry's r6 bullet — original 560, shown 850, scale
    /// 1.517943, defining points in the original box, size locked — decodes
    /// to its frame with the points as stored, and writes them back.
    ///
    /// 이것을 실패시키는 것: 디코더의 frame 호출을 지우기 / 잠금을 읽지 않기 /
    /// 인코더가 `orgSz` 를 현재 크기로 쓰기 / 점을 현재 상자로 옮겨 쓰기.
    #[test]
    fn r6_circle_keeps_its_original_box() {
        let xml = shape("ellipse", (560, 560), (850, 850), ("1.517943", "1.517943"), 1, R6_POINTS);
        let (content, lost) = decode(&xml);
        assert!(lost.is_empty(), "{lost:?}");
        let circle = control(content.clone());
        let Control::Ellipse { center, axis2, frame, .. } = &circle else { panic!("{circle:?}") };
        assert_eq!(frame.original_size, Some(size(560, 560)));
        assert!(frame.size_protect);
        assert_eq!(
            (*center, *axis2),
            (ShapePoint { x: 280, y: 280 }, ShapePoint { x: 560, y: 280 })
        );
        let out = encode(content);
        assert!(out.contains(r#"<hp:orgSz width="560" height="560"/>"#), "{out}");
        assert!(
            out.contains(
                r#"<hc:scaMatrix e1="1.517857" e2="0" e3="0" e4="0" e5="1.517857" e6="0"/>"#
            ),
            "{out}"
        );
        assert!(out.contains(r#"<hc:center x="280" y="280"/>"#), "{out}");
        assert!(out.contains(r#"protect="1""#), "{out}");
    }

    /// Hancom often stores a translation in the scale matrix itself
    /// (`e3`/`e6`, 11,973 objects in the corpus) — the object is placed by
    /// `<hp:pos>`, so it is no residual.
    /// 이것을 실패시키는 것: 원점 모서리 기준 비교를 지우기(이동 성분이
    /// `shape_drawn_extent` 로 보고된다).
    #[test]
    fn scale_translation_is_not_a_residual() {
        let xml = shape("ellipse", (560, 560), (850, 850), ("1.517943", "1.517943"), 0, R6_POINTS)
            .replace(r#"e5="1.517943" e6="0"/>"#, r#"e5="1.517943" e6="-613"/>"#)
            .replace(
                r#"<hc:scaMatrix e1="1.517943" e2="0" e3="0""#,
                r#"<hc:scaMatrix e1="1.517943" e2="0" e3="907""#,
            );
        assert!(xml.contains(r#"e3="907""#) && xml.contains(r#"e6="-613""#), "{xml}");
        let (content, lost) = decode(&xml);
        assert!(lost.is_empty(), "{lost:?}");
        assert_eq!(control(content).frame().unwrap().original_size, Some(size(560, 560)));
    }

    /// A picture mirrored by a negative scale (`scaMatrix e5 < 0`, 115 in the
    /// corpus) keeps its original box: Core holds no mirror for a picture, so
    /// the frame is judged on the scale's size, and the mirror is no frame
    /// residual.
    /// 이것을 실패시키는 것: 그림 원본 배율을 절댓값으로 보지 않기(`orgSz` 를
    /// 잃고 `shape_drawn_extent`).
    #[test]
    fn mirrored_picture_keeps_its_original_box() {
        let pic = r#"<hp:pic id="1" zOrder="0" numberingType="PICTURE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="0" instid="2" reverse="0"><hp:offset x="0" y="0"/><hp:orgSz width="57600" height="43500"/><hp:curSz width="4369" height="4294963997"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="2184" centerY="1649" rotateimage="1"/><hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="-38151" e4="0" e5="1" e6="3299"/><hc:scaMatrix e1="0.075851" e2="0" e3="38151" e4="0" e5="-0.075839" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo><hc:img binaryItemIDRef="image1" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/><hp:imgRect><hc:pt0 x="0" y="0"/><hc:pt1 x="57600" y="0"/><hc:pt2 x="57600" y="43500"/><hc:pt3 x="0" y="43500"/></hp:imgRect><hp:imgClip left="0" right="57600" top="0" bottom="43500"/><hp:inMargin left="0" right="0" top="0" bottom="0"/><hp:imgDim dimwidth="57600" dimheight="43500"/><hp:sz width="4369" widthRelTo="ABSOLUTE" height="3299" heightRelTo="ABSOLUTE" protect="0"/>"#.to_string() + POS + "</hp:pic>";
        let (content, lost) = decode(&pic);
        assert!(lost.is_empty(), "{lost:?}");
        let RunContent::Image(image) = &content else { panic!("{content:?}") };
        assert_eq!(image.frame.original_size, Some(size(57_600, 43_500)));

        // A picture whose `orgSz` is its displayed size has no frame value,
        // but is still judged (S20 review r1): its matrices draw it at
        // 331 x 250, not its layout size — a drawn-extent loss
        // (`default_frame_still_reports_the_drawn_extent`).
        let plain = pic.replace(
            r#"<hp:orgSz width="57600" height="43500"/>"#,
            r#"<hp:orgSz width="4369" height="3299"/>"#,
        );
        let (content, lost) = decode(&plain);
        // 이것을 실패시키는 것: O와 다른 imgRect를 회전 성공으로 인정.
        assert_eq!(lost, ["shape_transform"]);
        let RunContent::Image(image) = &content else { panic!("{content:?}") };
        assert!(image.frame.original_size.is_none());
    }

    /// S20 review r1: an object whose `orgSz` is its default (no frame value)
    /// is still judged against the source — a rect `orgSz = sz = 1000²`
    /// drawn at scale 2 is written 1000 wide where Hancom draws 2000, and
    /// that is reported; at scale 1 nothing is.
    #[test]
    fn default_frame_still_reports_the_drawn_extent() {
        let rect = |sca: &str| {
            shape(
                "rect",
                (1000, 1000),
                (1000, 1000),
                (sca, sca),
                0,
                r#"<hc:pt0 x="0" y="0"/><hc:pt1 x="1000" y="0"/><hc:pt2 x="1000" y="1000"/><hc:pt3 x="0" y="1000"/>"#,
            )
        };
        let (content, lost) = decode(&rect("2"));
        assert_eq!(lost, ["shape_drawn_extent"]);
        assert!(control(content).frame().unwrap().original_size.is_none());
        let (_, lost) = decode(&rect("1"));
        assert!(lost.is_empty(), "{lost:?}");
    }

    /// §19.23.15 M3: original 5,000,000, scale 0.000200498 — the source end
    /// lands at 1002.49, Core shows 1002, and the plan with the frame would
    /// write `0.000200` (1000): 2.49 from the source although each step is
    /// within 2. The decoder drops the original box (reported), keeping the
    /// lock, and the writer stores 1002 unscaled.
    ///
    /// 이것을 실패시키는 것: 디코드 판정을 Core 정확값과 하기(또는 판정을 지우기 —
    /// frame 이 남아 1000 으로 써진다) / frame 을 버릴 때 잠금까지 지우기.
    #[test]
    fn accumulated_rounding_drops_the_frame() {
        let xml = shape("rect", (5_000_000, 1000), (1002, 1000), ("0.000200498", "1"), 1, "");
        let (content, lost) = decode(&xml);
        assert_eq!(lost, ["object_original_size"]);
        let rect = control(content.clone());
        let frame = rect.frame().unwrap();
        assert_eq!(frame.original_size, None);
        assert!(frame.size_protect, "dropping the original box keeps the lock");
        let out = encode(content);
        assert!(out.contains(r#"<hp:orgSz width="1002" height="1000"/>"#), "{out}");
        assert!(out.contains(r#"protect="1""#), "{out}");
    }

    /// The same with text in the box: the dropped box keeps the text.
    /// 이것을 실패시키는 것: 상자를 바꿀 때 글을 되돌려 넣지 않기.
    #[test]
    fn a_dropped_frame_keeps_the_shape_text() {
        let text = r#"<hp:drawText lastWidth="0" name="" editable="0"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>안</hp:t></hp:run></hp:p></hp:subList></hp:drawText>"#;
        let xml = shape("rect", (5_000_000, 1000), (1002, 1000), ("0.000200498", "1"), 1, text);
        let (content, lost) = decode(&xml);
        assert_eq!(lost, ["object_original_size"]);
        let Control::TextBox { ref paragraphs, frame, .. } = control(content) else { panic!() };
        assert_eq!(frame.original_size, None);
        assert_eq!(paragraphs[0].runs[0].content.as_text(), Some("안"));
    }

    /// §19.23.17 M2: original 10,000,000, shown 1004, scale 0.000100 — the
    /// source end is 1000. The writer's plan cannot store 1004 / 10,000,000
    /// within 2 (it would write 1000) and falls back to the displayed box;
    /// the decoder, measuring that same plan, finds the source 4 away with or
    /// without the original box: `shape_drawn_extent`. What the plan says is
    /// what the writer writes.
    ///
    /// 이것을 실패시키는 것: 계획의 fallback 을 지우기 / 인코더가 계획 대신
    /// 원래 크기를 직접 쓰기 (둘 다 `orgSz` 10000000 이 써진다).
    #[test]
    fn drawn_extent_writes_what_the_plan_says() {
        let xml = shape("rect", (10_000_000, 1000), (1004, 1000), ("0.000100", "1"), 0, "");
        let (content, lost) = decode(&xml);
        assert_eq!(lost, ["shape_drawn_extent"]);
        let rect = control(content.clone());
        let plan = frame_plan(&rect, &HWPX_POLICY).unwrap();
        assert!(plan.is_fallback());
        assert_eq!(plan.original_size(), (1004, 1000));
        let out = encode(content);
        assert!(out.contains(r#"<hp:orgSz width="1004" height="1000"/>"#), "{out}");
        assert!(
            out.contains(r#"<hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/>"#),
            "{out}"
        );
    }

    /// §19.23.15: a negative `orgSz` (an unsigned wrap) is reported and not
    /// carried; Core holding one is refused by the writer rather than written
    /// wrapped.
    ///
    /// 이것을 실패시키는 것: 디코드 보고를 지우기 / 인코더 거부를 지우기
    /// (`orgSz="4294967295"` 가 써진다).
    #[test]
    fn negative_original_size_is_not_written() {
        let xml = shape("ellipse", (4_294_967_295, 560), (850, 850), ("1", "1"), 0, R6_POINTS);
        let (content, lost) = decode(&xml);
        assert_eq!(lost, ["object_original_size"]);
        assert_eq!(control(content.clone()).frame().unwrap().original_size, None);
        assert!(!encode(content).contains("4294967295"));

        let mut circle = Control::ellipse(u(850), u(850));
        circle.frame_mut().unwrap().original_size = Some(Size::new(u(-1), u(560)));
        assert!(crate::encoder::shapes::shape_frame_plan(&circle, false).is_err());
        let mut picture = Image::new("BinData/a.png", u(850), u(850), ImageFormat::Png);
        picture.frame.original_size = Some(Size::new(u(560), u(-1)));
        let err = crate::encoder::section::build_picture(
            &picture,
            false,
            &mut Vec::new(),
            crate::encoder::EncodeOptions::default(),
            &mut crate::encoder::section::DocumentEncodeContext::new(0),
        );
        assert!(err.is_err());
    }

    /// An `orgSz` that cannot be a frame value is reported, not carried: an
    /// axis of 0 under a displayed axis that is not, a side outside the
    /// HWPUNIT range. One equal to the kind's default (the displayed size, a
    /// TextArt's 14173 design box) is no frame value at all (FR-T5).
    /// 이것을 실패시키는 것: 0 축 보고를 지우기 / 범위 밖 보고를 지우기 /
    /// 기본값과 같은 원래 크기를 접지 않기.
    #[test]
    fn unusable_and_default_original_sizes() {
        let (content, lost) =
            decode(&shape("ellipse", (0, 560), (850, 850), ("1", "1"), 0, R6_POINTS));
        assert_eq!(lost, ["object_original_size"]);
        assert!(control(content).frame().unwrap().original_size.is_none());
        let (content, lost) =
            decode(&shape("ellipse", (200_000_000, 560), (850, 850), ("1", "1"), 0, R6_POINTS));
        assert_eq!(lost, ["object_original_size"]);
        assert!(control(content).frame().unwrap().original_size.is_none());
        let (content, lost) =
            decode(&shape("ellipse", (850, 850), (850, 850), ("1", "1"), 0, R6_POINTS));
        assert!(lost.is_empty(), "{lost:?}");
        assert!(control(content).frame().unwrap().is_unset());
    }

    /// FR-T6: a shape resized through Core keeps its original box on the
    /// wire with the scale between (850 → 1700: scale 2, centre at 425).
    /// 이것을 실패시키는 것: 인코더가 `orgSz` 를 현재 크기로 쓰기.
    #[test]
    fn resized_shape_writes_its_original_box() {
        let mut circle = Control::ellipse(u(850), u(850));
        assert!(circle.resize(u(1700), u(1700)));
        let out = encode(RunContent::Control(Box::new(circle)));
        assert!(out.contains(r#"<hp:orgSz width="850" height="850"/>"#), "{out}");
        assert!(
            out.contains(
                r#"<hc:scaMatrix e1="2.000000" e2="0" e3="0" e4="0" e5="2.000000" e6="0"/>"#
            ),
            "{out}"
        );
        assert!(out.contains(r#"<hc:center x="425" y="425"/>"#), "{out}");
    }

    /// The extreme ratio written: a line 49 wide from an original box of
    /// 100,000,000 falls back to the displayed box, its end written at 49.
    /// 이것을 실패시키는 것: 인코더가 계획의 점 대신 Core 점을 쓰기(끝점
    /// 100000000).
    #[test]
    fn extreme_ratio_line_falls_back() {
        let mut line = Control::Line {
            start: ShapePoint { x: 0, y: 0 },
            end: ShapePoint { x: 100_000_000, y: 0 },
            width: u(49),
            height: u(0),
            frame: hwpforge_core::ObjectFrame::new(),
            placement: None,
            caption: None,
            style: None,
        };
        line.frame_mut().unwrap().original_size = Some(size(100_000_000, 0));
        let out = encode(RunContent::Control(Box::new(line)));
        assert!(out.contains(r#"<hc:endPt x="49" y="0"/>"#), "{out}");
        assert!(out.contains(r#"<hp:orgSz width="49" height="0"/>"#), "{out}");
    }

    /// A TextArt's original box other than its 14173 design box is carried
    /// and written back with the scale between.
    /// 이것을 실패시키는 것: 인코더를 14173 상수로 되돌리기.
    #[test]
    fn textart_frame_round_trips() {
        let mut art = Control::TextArt {
            text: "가".into(),
            shape: "WAVE2".into(),
            font_name: "함초롬바탕".into(),
            font_style: "보통".into(),
            align: "LEFT".into(),
            line_spacing: 120,
            char_spacing: 100,
            width: u(10_000),
            height: u(5000),
            frame: hwpforge_core::ObjectFrame::new(),
            placement: None,
            fill_color: None,
            inst_id: None,
        };
        art.frame_mut().unwrap().original_size = Some(size(20_000, 10_000));
        let out = encode(RunContent::Control(Box::new(art)));
        assert!(out.contains(r#"<hp:orgSz width="20000" height="10000"/>"#), "{out}");
        assert!(out.contains(r#"e1="0.500000""#) && out.contains(r#"e5="0.500000""#), "{out}");
        assert!(out.contains(r#"<hc:pt2 x="20000" y="10000"/>"#), "{out}");
    }

    /// A picture's and a table's size lock round-trip.
    /// 이것을 실패시키는 것: 그림·표의 `protect` 를 읽지 않거나 상수 0 으로 쓰기.
    #[test]
    fn picture_and_table_protect_round_trip() {
        let pic = r#"<hp:pic id="1" zOrder="0" numberingType="PICTURE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="0" instid="2" reverse="0"><hp:offset x="0" y="0"/><hp:orgSz width="1000" height="500"/><hp:curSz width="2000" height="1000"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="1000" centerY="500" rotateimage="1"/><hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:scaMatrix e1="2" e2="0" e3="0" e4="0" e5="2" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo><hc:img binaryItemIDRef="image1" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/><hp:imgRect><hc:pt0 x="0" y="0"/><hc:pt1 x="1000" y="0"/><hc:pt2 x="1000" y="500"/><hc:pt3 x="0" y="500"/></hp:imgRect><hp:imgClip left="0" right="1000" top="0" bottom="500"/><hp:inMargin left="0" right="0" top="0" bottom="0"/><hp:imgDim dimwidth="1000" dimheight="500"/><hp:sz width="2000" widthRelTo="ABSOLUTE" height="1000" heightRelTo="ABSOLUTE" protect="1"/>"#.to_string() + POS + "</hp:pic>";
        let (content, lost) = decode(&pic);
        assert!(lost.is_empty(), "{lost:?}");
        let RunContent::Image(image) = &content else { panic!("{content:?}") };
        assert!(image.frame.size_protect);
        assert_eq!(image.frame.original_size, Some(size(1000, 500)));
        assert!(encode(content).contains(r#"protect="1""#));

        let tbl = r#"<hp:tbl id="3" zOrder="0" numberingType="TABLE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" pageBreak="CELL" repeatHeader="1" rowCnt="1" colCnt="1" cellSpacing="0" borderFillIDRef="1" noAdjust="0"><hp:sz width="1000" widthRelTo="ABSOLUTE" height="500" heightRelTo="ABSOLUTE" protect="1"/><hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="1" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="COLUMN" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/><hp:outMargin left="0" right="0" top="0" bottom="0"/><hp:inMargin left="0" right="0" top="0" bottom="0"/><hp:tr><hp:tc name="" header="0" hasMargin="0" protect="0" editable="0" dirty="0" borderFillIDRef="1"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="CENTER" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>x</hp:t></hp:run></hp:p></hp:subList><hp:cellAddr colAddr="0" rowAddr="0"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:cellSz width="1000" height="500"/><hp:cellMargin left="0" right="0" top="0" bottom="0"/></hp:tc></hp:tr></hp:tbl>"#;
        let (content, _) = decode(tbl);
        let RunContent::Table(table) = &content else { panic!("{content:?}") };
        assert!(table.frame.size_protect);
        let out = encode(content);
        assert!(out.contains(r#"heightRelTo="ABSOLUTE" protect="1""#), "{out}");
    }

    /// A `rows`×`cols` table whose `inMargin` is followed by `zones`.
    fn zone_tbl(rows: u32, cols: u32, zones: &str) -> String {
        let cell = |r: u32, c: u32| {
            format!(
                r#"<hp:tc name="" header="0" hasMargin="0" protect="0" editable="0" dirty="0" borderFillIDRef="1"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="CENTER" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>x</hp:t></hp:run></hp:p></hp:subList><hp:cellAddr colAddr="{c}" rowAddr="{r}"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:cellSz width="1000" height="500"/><hp:cellMargin left="0" right="0" top="0" bottom="0"/></hp:tc>"#
            )
        };
        let body: String = (0..rows)
            .map(|r| {
                format!("<hp:tr>{}</hp:tr>", (0..cols).map(|c| cell(r, c)).collect::<String>())
            })
            .collect();
        format!(
            r#"<hp:tbl id="3" zOrder="0" numberingType="TABLE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" pageBreak="CELL" repeatHeader="1" rowCnt="{rows}" colCnt="{cols}" cellSpacing="0" borderFillIDRef="1" noAdjust="0"><hp:sz width="{}" widthRelTo="ABSOLUTE" height="500" heightRelTo="ABSOLUTE" protect="0"/><hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="1" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="COLUMN" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/><hp:outMargin left="0" right="0" top="0" bottom="0"/><hp:inMargin left="0" right="0" top="0" bottom="0"/>{zones}{body}</hp:tbl>"#,
            1000 * cols
        )
    }

    /// D: a table caption's `fullSz` and `lastWidth` round-trip; a
    /// `lastWidth` equal to the table width decodes to `None` and writes
    /// back as that width.
    /// 이것을 실패시키는 것: `fullSz` 무시 · 정규화 생략/반전 · 인코더 상수 0·부모
    /// 폭 복귀.
    #[test]
    fn caption_full_size_and_last_width_round_trip() {
        let tbl = |full: u32, last: u32| {
            let caption = format!(
                r#"<hp:caption side="BOTTOM" fullSz="{full}" width="8504" gap="850" lastWidth="{last}"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="8504" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>c</hp:t></hp:run></hp:p></hp:subList></hp:caption>"#
            );
            zone_tbl(1, 2, "").replace("<hp:inMargin", &format!("{caption}<hp:inMargin"))
        };
        for (full, last, expect) in
            [(1u32, 2566u32, Some(2566)), (0, 2000, None), (0, 1999, Some(1999))]
        {
            let (content, lost) = decode(&tbl(full, last));
            assert!(lost.is_empty(), "{lost:?}");
            let RunContent::Table(table) = &content else { panic!("{content:?}") };
            let caption = table.caption.as_ref().expect("caption");
            assert_eq!(caption.full_size, full == 1);
            assert_eq!(caption.last_width.map(|w| w.as_i32()), expect);
            let out = encode(content);
            assert!(
                out.contains(&format!(
                    r#"fullSz="{full}" width="8504" gap="850" lastWidth="{last}""#
                )),
                "{out}"
            );
        }
    }

    /// D review r1 MEDIUM-4: a table caption `lastWidth` past the unit range
    /// (table width 2,000) is reported as lost, not read as "equal to the
    /// table width"; the range end decodes as a value.
    /// 이것을 실패시키는 것: 손실 진단 삭제 · 범위 끝(100,000,000) 거부.
    #[test]
    fn caption_last_width_out_of_range_is_lost() {
        let tbl = |last: u64| {
            let caption = format!(
                r#"<hp:caption side="BOTTOM" fullSz="0" width="8504" gap="850" lastWidth="{last}"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="8504" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>c</hp:t></hp:run></hp:p></hp:subList></hp:caption>"#
            );
            zone_tbl(1, 2, "").replace("<hp:inMargin", &format!("{caption}<hp:inMargin"))
        };
        let (_, lost) = decode(&tbl(100_000_001));
        assert_eq!(lost, ["caption"]);
        let (content, lost) = decode(&tbl(100_000_000));
        assert!(lost.is_empty(), "{lost:?}");
        let RunContent::Table(table) = &content else { panic!("{content:?}") };
        assert_eq!(
            table.caption.as_ref().unwrap().last_width.map(|w| w.as_i32()),
            Some(100_000_000)
        );
    }

    /// D: a picture caption's `lastWidth` equal to the picture width decodes
    /// to `None`; another width stays (the object-width path, not the
    /// table's).
    /// 이것을 실패시키는 것: 개체 폭 정규화 생략(같은 폭이 `Some`) · 반전.
    #[test]
    fn picture_caption_last_width_normalizes_to_the_picture_width() {
        use hwpforge_core::caption::{Caption, CaptionSide};
        for (last, expect) in [(3000, None), (3283, Some(3283))] {
            let mut image = Image::new(
                "BinData/a.png",
                HwpUnit::new(3000).unwrap(),
                HwpUnit::new(2000).unwrap(),
                ImageFormat::Png,
            );
            image.caption = Some(
                Caption::new(
                    vec![Paragraph::with_runs(
                        vec![Run::text("c", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    CaptionSide::Bottom,
                )
                .with_last_width(Some(HwpUnit::new(last).unwrap())),
            );
            let xml = encode(RunContent::Image(image));
            assert!(xml.contains(&format!(r#"lastWidth="{last}""#)), "{xml}");
            let parsed =
                crate::decoder::section::parse_section(&xml, 0, &std::collections::HashMap::new())
                    .unwrap();
            let RunContent::Image(back) = &parsed.paragraphs[0].runs[0].content else { panic!() };
            assert_eq!(back.caption.as_ref().unwrap().last_width.map(|w| w.as_i32()), expect);
        }
    }

    /// F: cell zones round-trip in order and field by field, written between
    /// `inMargin` and the first row; a table without zones writes no list.
    /// 이것을 실패시키는 것: 읽지 않기 · 행·열 뒤바꿈(비대칭 (12,0,31,1)) ·
    /// 미방출 · 빈 목록 방출.
    #[test]
    fn cell_zones_round_trip() {
        let zones = r#"<hp:cellzoneList><hp:cellzone startRowAddr="12" startColAddr="0" endRowAddr="31" endColAddr="1" borderFillIDRef="7"/><hp:cellzone startRowAddr="0" startColAddr="1" endRowAddr="0" endColAddr="1" borderFillIDRef="3"/></hp:cellzoneList>"#;
        let (content, lost) = decode(&zone_tbl(32, 2, zones));
        assert!(lost.is_empty(), "{lost:?}");
        let RunContent::Table(table) = &content else { panic!("{content:?}") };
        assert_eq!(
            table.cell_zones,
            [
                hwpforge_core::table::CellZone::new(12, 0, 31, 1, 7),
                hwpforge_core::table::CellZone::new(0, 1, 0, 1, 3)
            ]
        );
        let out = encode(content);
        assert!(out.contains(&format!("{zones}<hp:tr>")), "{out}");
        assert!(out.find("<hp:inMargin").unwrap() < out.find("<hp:cellzoneList").unwrap(), "{out}");
        let (plain, _) = decode(&zone_tbl(1, 1, ""));
        assert!(!encode(plain).contains("cellzone"));
    }

    /// F: a zone starting past its end or reaching outside the grid (an
    /// address past 65535 included) is dropped and reported as lost content.
    /// 이것을 실패시키는 것: 두 검사 중 하나를 지우기 · 끝을 배타로 비교.
    #[test]
    fn invalid_cell_zone_is_reported_as_lost() {
        for (zone, kept) in [
            (r#"startRowAddr="1" startColAddr="0" endRowAddr="0" endColAddr="1""#, 0),
            (r#"startRowAddr="0" startColAddr="0" endRowAddr="2" endColAddr="1""#, 0),
            (r#"startRowAddr="0" startColAddr="0" endRowAddr="1" endColAddr="2""#, 0),
            (r#"startRowAddr="0" startColAddr="0" endRowAddr="65536" endColAddr="1""#, 0),
            (r#"startRowAddr="1" startColAddr="1" endRowAddr="1" endColAddr="1""#, 1),
        ] {
            let list = format!(
                r#"<hp:cellzoneList><hp:cellzone {zone} borderFillIDRef="3"/></hp:cellzoneList>"#
            );
            let (content, lost) = decode(&zone_tbl(2, 2, &list));
            let RunContent::Table(table) = &content else { panic!("{content:?}") };
            assert_eq!(table.cell_zones.len(), kept, "{zone}");
            assert_eq!(
                lost,
                if kept == 1 { vec![] } else { vec!["cellzone".to_string()] },
                "{zone}"
            );
        }
    }

    /// E: a table's `noAdjust` round-trips on its own and together with the
    /// size lock — all four combinations.
    /// 이것을 실패시키는 것: `noAdjust` 를 읽지 않기 · 상수 0 으로 쓰기 ·
    /// `protect` 와 섞기.
    #[test]
    fn table_no_adjust_and_protect_round_trip() {
        let tbl = |protect: u32, no_adjust: u32| {
            format!(
                r#"<hp:tbl id="3" zOrder="0" numberingType="TABLE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" pageBreak="CELL" repeatHeader="1" rowCnt="1" colCnt="1" cellSpacing="0" borderFillIDRef="1" noAdjust="{no_adjust}"><hp:sz width="1000" widthRelTo="ABSOLUTE" height="500" heightRelTo="ABSOLUTE" protect="{protect}"/><hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="1" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="COLUMN" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/><hp:outMargin left="0" right="0" top="0" bottom="0"/><hp:inMargin left="0" right="0" top="0" bottom="0"/><hp:tr><hp:tc name="" header="0" hasMargin="0" protect="0" editable="0" dirty="0" borderFillIDRef="1"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="CENTER" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0"><hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>x</hp:t></hp:run></hp:p></hp:subList><hp:cellAddr colAddr="0" rowAddr="0"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:cellSz width="1000" height="500"/><hp:cellMargin left="0" right="0" top="0" bottom="0"/></hp:tc></hp:tr></hp:tbl>"#
            )
        };
        for (protect, no_adjust) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            let (content, _) = decode(&tbl(protect, no_adjust));
            let RunContent::Table(table) = &content else { panic!("{content:?}") };
            assert_eq!(table.frame.size_protect, protect == 1);
            assert_eq!(table.no_adjust, no_adjust == 1);
            let out = encode(content);
            assert!(out.contains(&format!(r#"noAdjust="{no_adjust}""#)), "{out}");
            assert!(out.contains(&format!(r#"protect="{protect}""#)), "{out}");
        }
    }
}
