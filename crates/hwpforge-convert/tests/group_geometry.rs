//! HWP5 group (묶음) geometry against Hancom's own answers.
//!
//! Each `.hwp` fixture was grouped and transformed in Hancom; its
//! `-ungrouped.hwpx` twin is Hancom's "ungroup" of the same group, so every
//! shape there sits at its page position with no group in between. The HWP5
//! decoder folds each group child's matrix chain into Core geometry; placed
//! on the page by the shared Core definition (`emitted_corners`,
//! `local_transform` at the exact policy) each leaf must land within
//! 2 HWPUNIT of Hancom's, defining point by defining point — and, for a box
//! (rect, ellipse), corner by corner in order, so its turn and mirror have
//! to match too.
//!
//! The Hancom side is read straight from the wire (page = `<hp:pos>` −
//! the box minimum + matrix chain · point), not through a decoder.

use std::io::Read;
use std::path::PathBuf;

use hwpforge_core::control::geometry::{emitted_corners, local_transform, EmissionPolicy};
use hwpforge_core::control::{Control, GroupChild, ShapePoint};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::RunContent;
use hwpforge_smithy_hwp5::{decode_hwp5_with_images, Hwp5Warning};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

fn fixture(name: &str) -> Vec<u8> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/shapes").join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

/// One shape on the page.
#[derive(Debug)]
struct PageShape {
    kind: &'static str,
    corners: Vec<(f64, f64)>,
    points: Vec<(f64, f64)>,
}

// ── Hancom's ungrouped shapes, from the wire ─────────────────────

type Matrix = [f64; 6];

fn apply(m: &Matrix, (x, y): (f64, f64)) -> (f64, f64) {
    (m[0] * x + m[1] * y + m[2], m[3] * x + m[4] * y + m[5])
}

/// `a · b` (apply `b` first).
fn mul(a: &Matrix, b: &Matrix) -> Matrix {
    [
        a[0] * b[0] + a[1] * b[3],
        a[0] * b[1] + a[1] * b[4],
        a[0] * b[2] + a[1] * b[5] + a[2],
        a[3] * b[0] + a[4] * b[3],
        a[3] * b[1] + a[4] * b[4],
        a[3] * b[2] + a[4] * b[5] + a[5],
    ]
}

fn attr(e: &BytesStart<'_>, name: &str) -> Option<String> {
    e.attributes().with_checks(false).filter_map(Result::ok).find_map(|a| {
        let key: &str = a.key.as_ref();
        (key == name).then(|| a.value.to_string())
    })
}

fn num(e: &BytesStart<'_>, name: &str) -> f64 {
    let v = attr(e, name).unwrap_or_else(|| panic!("missing {name}"));
    // Offsets may be written as wrapped u32.
    let n: f64 = v.parse().unwrap_or_else(|_| panic!("{name}={v}"));
    if n > f64::from(i32::MAX) {
        n - 4_294_967_296.0
    } else {
        n
    }
}

fn matrix(e: &BytesStart<'_>) -> Matrix {
    ["e1", "e2", "e3", "e4", "e5", "e6"].map(|k| num(e, k))
}

#[derive(Default)]
struct WireShape {
    kind: &'static str,
    org: (f64, f64),
    mats: Vec<Matrix>,
    pos: (f64, f64),
    points: Vec<(f64, f64)>,
}

/// Top-level shapes of `section0.xml`: page = `<hp:pos>` − the minimum of
/// the matrix-chained box + matrix chain · point.
fn ungrouped_shapes(hwpx: &[u8]) -> Vec<PageShape> {
    wire_shapes(hwpx, false).into_iter().map(page_shape).collect()
}

/// The direct child shapes of the first `<hp:container>` of `section0.xml`,
/// wherever it sits (body or table cell), in the group's outer space: each
/// child's own matrix chain already carries the group's transforms and
/// `<hp:pos>` places the outer origin (0, 0), so a child lands at chain ·
/// point (see the HWPX decoder's `decode_container`).
fn grouped_shapes(hwpx: &[u8]) -> Vec<PageShape> {
    wire_shapes(hwpx, true).into_iter().map(group_shape).collect()
}

/// Wire shapes of `section0.xml`: the top-level ones, or (`grouped`) the
/// direct children of the first `<hp:container>`.
fn wire_shapes(hwpx: &[u8], grouped: bool) -> Vec<WireShape> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(hwpx)).unwrap();
    let mut xml = String::new();
    archive.by_name("Contents/section0.xml").unwrap().read_to_string(&mut xml).unwrap();
    let mut reader = Reader::from_str(&xml);
    let mut out = Vec::new();
    // (shape, depth of its start tag)
    let mut current: Option<(WireShape, usize)> = None;
    // Depth of the first `<hp:container>` start tag (grouped mode).
    let mut container: Option<usize> = None;
    let mut depth = 0usize;
    loop {
        let event = reader.read_event().unwrap();
        let (e, empty) = match &event {
            Event::Start(e) => (e.clone(), false),
            Event::Empty(e) => (e.clone(), true),
            Event::End(_) => {
                depth -= 1;
                if current.as_ref().is_some_and(|(_, d)| *d == depth) {
                    let (s, _) = current.take().unwrap();
                    out.push(s);
                }
                if container == Some(depth) {
                    break;
                }
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let name = e.name().as_ref().to_string();
        let kind = match name.as_str() {
            "hp:rect" => Some("rect"),
            "hp:ellipse" => Some("ellipse"),
            "hp:polygon" => Some("polygon"),
            "hp:line" => Some("line"),
            "hp:curve" => Some("curve"),
            "hp:connectLine" => Some("connectLine"),
            _ => None,
        };
        if grouped && container.is_none() && name == "hp:container" {
            container = Some(depth);
        }
        let in_scope = !grouped || container.is_some_and(|c| depth == c + 1);
        match (&mut current, kind) {
            (None, Some(kind)) if in_scope => {
                current = Some((WireShape { kind, ..WireShape::default() }, depth));
            }
            (Some((s, d)), _) if depth == *d + 1 || name.starts_with("hc:") => {
                match name.as_str() {
                    "hp:orgSz" if depth == *d + 1 => s.org = (num(&e, "width"), num(&e, "height")),
                    "hp:pos" if depth == *d + 1 => {
                        s.pos = (num(&e, "horzOffset"), num(&e, "vertOffset"))
                    }
                    "hc:transMatrix" | "hc:scaMatrix" | "hc:rotMatrix" => s.mats.push(matrix(&e)),
                    "hc:center" | "hc:ax1" | "hc:ax2" | "hc:pt" | "hc:startPt" | "hc:endPt"
                        if depth == *d + 1 =>
                    {
                        s.points.push((num(&e, "x"), num(&e, "y")));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        if !empty {
            depth += 1;
        }
    }
    out
}

fn page_shape(s: WireShape) -> PageShape {
    // trans, sca, rot: M = T · R · S.
    let [t, sca, rot] = s.mats[..] else { panic!("{}: {} matrices", s.kind, s.mats.len()) };
    let m = mul(&mul(&t, &rot), &sca);
    let (w, h) = s.org;
    let cs: Vec<(f64, f64)> =
        [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].iter().map(|&p| apply(&m, p)).collect();
    let min_x = cs.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let min_y = cs.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let base = (s.pos.0 - min_x, s.pos.1 - min_y);
    let page = |(x, y): (f64, f64)| (x + base.0, y + base.1);
    // Ellipses list centre, ax1, ax2, then four arc points we do not compare.
    let points = if s.kind == "ellipse" { &s.points[..3] } else { &s.points[..] };
    PageShape {
        kind: s.kind,
        corners: cs.into_iter().map(page).collect(),
        points: points.iter().map(|&p| page(apply(&m, p))).collect(),
    }
}

fn group_shape(s: WireShape) -> PageShape {
    // trans, then each (sca, rot) pair: M = T · R1 · S1 · … · Rn · Sn.
    let (t, pairs) = s.mats.split_first().expect("a transMatrix");
    assert!(pairs.len() % 2 == 0, "{}: {} matrices", s.kind, s.mats.len());
    let m = pairs.chunks(2).fold(*t, |m, p| mul(&mul(&m, &p[1]), &p[0]));
    let (w, h) = s.org;
    let points = if s.kind == "ellipse" { &s.points[..3] } else { &s.points[..] };
    PageShape {
        kind: s.kind,
        corners: [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].iter().map(|&p| apply(&m, p)).collect(),
        points: points.iter().map(|&p| apply(&m, p)).collect(),
    }
}

// ── our HWP5 group, placed by the Core definition ────────────────

fn offset_of(c: &Control) -> (f64, f64) {
    let placement = match c {
        Control::TextBox { placement, .. }
        | Control::Rect { placement, .. }
        | Control::Ellipse { placement, .. }
        | Control::Arc { placement, .. }
        | Control::Polygon { placement, .. }
        | Control::Curve { placement, .. }
        | Control::ConnectLine { placement, .. }
        | Control::Line { placement, .. }
        | Control::Group { placement, .. } => placement,
        _ => &None,
    };
    placement.as_ref().map_or((0.0, 0.0), |p| {
        (f64::from(p.horz_offset.as_i32()), f64::from(p.vert_offset.as_i32()))
    })
}

/// Displayed width and height of a leaf.
fn local_size(c: &Control) -> (i32, i32) {
    match c {
        Control::TextBox { width, height, .. }
        | Control::Rect { width, height, .. }
        | Control::Ellipse { width, height, .. }
        | Control::Arc { width, height, .. }
        | Control::Polygon { width, height, .. }
        | Control::Curve { width, height, .. }
        | Control::ConnectLine { width, height, .. }
        | Control::Line { width, height, .. } => (width.as_i32(), height.as_i32()),
        _ => (0, 0),
    }
}

fn leaf_points(c: &Control) -> (&'static str, Vec<ShapePoint>) {
    match c {
        Control::TextBox { .. } | Control::Rect { .. } => ("rect", vec![]),
        Control::Ellipse { center, axis1, axis2, .. } => ("ellipse", vec![*center, *axis1, *axis2]),
        Control::Polygon { vertices, .. } => ("polygon", vertices.clone()),
        Control::Line { start, end, .. } => ("line", vec![*start, *end]),
        Control::Curve { points, .. } => ("curve", points.clone()),
        Control::ConnectLine { start, end, .. } => ("connectLine", vec![*start, *end]),
        other => panic!("unexpected leaf {other:?}"),
    }
}

/// [`collect_leaves`] for a group child; a picture is its unturned box.
fn collect_child(c: &GroupChild, base: (f64, f64), out: &mut Vec<PageShape>) {
    match c {
        GroupChild::Shape(c) => collect_leaves(c, base, out),
        GroupChild::Picture(image) => {
            let (x, y) = image.placement.as_ref().map_or((0.0, 0.0), |p| {
                (f64::from(p.horz_offset.as_i32()), f64::from(p.vert_offset.as_i32()))
            });
            let (x, y) = (base.0 + x, base.1 + y);
            let (w, h) = (f64::from(image.width.as_i32()), f64::from(image.height.as_i32()));
            out.push(PageShape {
                kind: "picture",
                corners: vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)],
                points: Vec::new(),
            });
        }
        _ => {}
    }
}

/// Leaves of `c` on the page; `base` = the page position of `c`'s parent
/// group's corner.
fn collect_leaves(c: &Control, base: (f64, f64), out: &mut Vec<PageShape>) {
    let o = offset_of(c);
    if let Control::Group { children, .. } = c {
        for child in children {
            collect_child(child, (base.0 + o.0, base.1 + o.1), out);
        }
        return;
    }
    let exact = EmissionPolicy::exact();
    let corners = emitted_corners(c, &exact).expect("a shape box");
    let l = local_transform(c, &exact).expect("a shape transform");
    let (kind, pts) = leaf_points(c);
    // S20: the points are in the original box, drawn scaled to the shown one.
    let (w, h) = match emitted_corners(c, &EmissionPolicy::exact()) {
        Some(_) => {
            let b = c.effective_original_size().expect("a frame");
            let corners_box = local_size(c);
            let s = |d: i32, o: i32| if o == 0 { 1.0 } else { f64::from(d) / f64::from(o) };
            (s(corners_box.0, b.width.as_i32()), s(corners_box.1, b.height.as_i32()))
        }
        None => (1.0, 1.0),
    };
    out.push(PageShape {
        kind,
        corners: corners.iter().map(|&(x, y)| (x + base.0, y + base.1)).collect(),
        points: pts
            .iter()
            .map(|p| {
                let (x, y) = l.apply((f64::from(p.x) * w, f64::from(p.y) * h));
                (x + base.0 + o.0, y + base.1 + o.1)
            })
            .collect(),
    });
}

/// The groups among the runs of `paragraphs` (not inside their tables).
fn groups_in<'a>(paragraphs: impl IntoIterator<Item = &'a Paragraph>) -> Vec<Control> {
    paragraphs
        .into_iter()
        .flat_map(|p| p.runs.iter())
        .filter_map(|r| match &r.content {
            RunContent::Control(c) if matches!(**c, Control::Group { .. }) => Some((**c).clone()),
            _ => None,
        })
        .collect()
}

/// The paragraphs of every table cell among the runs of `paragraphs`.
fn cell_paragraphs(paragraphs: &[Paragraph]) -> Vec<&Paragraph> {
    paragraphs
        .iter()
        .flat_map(|p| p.runs.iter())
        .filter_map(|r| match &r.content {
            RunContent::Table(t) => Some(t),
            _ => None,
        })
        .flat_map(|t| t.rows.iter().flat_map(|row| row.cells.iter()))
        .flat_map(|cell| cell.paragraphs.iter())
        .collect()
}

/// The first group of section 0's body, or else of its table cells (F12).
fn first_group(doc: &hwpforge_core::document::Document) -> Control {
    let paragraphs = &doc.sections()[0].paragraphs;
    groups_in(paragraphs)
        .into_iter()
        .chain(groups_in(cell_paragraphs(paragraphs)))
        .next()
        .expect("a group")
}

/// The first group of section 0 (body first, then table cells) and the
/// decode warnings.
fn hwp5_group(name: &str) -> (Control, Vec<Hwp5Warning>) {
    let doc = decode_hwp5_with_images(&fixture(name)).unwrap();
    (first_group(&doc.document), doc.warnings)
}

fn lost_kinds(warnings: &[Hwp5Warning]) -> Vec<String> {
    warnings
        .iter()
        .filter_map(|w| match w {
            Hwp5Warning::ContentLost { kind, .. } => Some(kind.clone()),
            _ => None,
        })
        .collect()
}

fn dist(a: &[(f64, f64)], b: &[(f64, f64)]) -> f64 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(p, q)| (p.0 - q.0).hypot(p.1 - q.1)).fold(0.0, f64::max)
}

/// Every leaf of the HWP5 group (except `skip` kinds) lands within 2 HWPUNIT
/// of its Hancom-ungrouped counterpart; `extra` = shapes outside the group,
/// present in the ungrouped file only as top-level shapes.
fn assert_leaves_match(hwp: &str, ungrouped: &str, extra: usize, skip: &[&str]) {
    let (group, _) = hwp5_group(hwp);
    let pos = offset_of(&group);
    let mut ours = Vec::new();
    // The group's own offset is its page position; its children hang off it.
    if let Control::Group { children, .. } = &group {
        for child in children {
            collect_child(child, pos, &mut ours);
        }
    }
    let theirs = ungrouped_shapes(&fixture(ungrouped));
    assert_eq!(ours.len() + extra, theirs.len(), "{hwp}: leaf count");
    for leaf in ours.iter().filter(|l| !skip.contains(&l.kind)) {
        let best = theirs
            .iter()
            .filter(|s| s.kind == leaf.kind && s.points.len() == leaf.points.len())
            .map(|s| {
                // A point shape folds its turn and mirror into its points, so
                // only a box's corners keep their order.
                let corners = if matches!(leaf.kind, "rect" | "ellipse") {
                    dist(&leaf.corners, &s.corners)
                } else {
                    0.0
                };
                corners.max(dist(&leaf.points, &s.points))
            })
            .fold(f64::INFINITY, f64::min);
        assert!(best <= 2.0, "{hwp}: {} off Hancom's ungroup by {best:.2}\n{leaf:?}", leaf.kind);
    }
}

/// F1: a group scaled ×1.5/×0.5 (g1), mirrored left-right (g3), and a group
/// inside a group scaled ×1.2 with a pre-shrunk shape (g4).
///
/// F11: a rect and an ellipse grouped, that group grouped with a rect lying
/// above and left of it, the outer group stretched ×1.2 along x — the inner
/// group sits away from the outer corner.
///
/// 이것을 실패시키는 것: HWP5 행렬열을 무시하는 것(옛 투영: offset·초기 크기만),
/// 중첩 묶음 자식을 그 묶음 기준으로 옮기지 않는 것, 중첩 묶음 offset 을 빠뜨리거나
/// 두 번 더하는 것(F11),
/// 쌍 적용 순서를 첫 쌍부터로 바꾸는 것, 한 쌍 안에서 sca 를 rot 뒤에 곱하는 것,
/// 최상위 레코드의 반복 type tag 를 건너뛰지 않는 것(또는 자식에서 건너뛰는 것).
#[test]
fn f1_groups_match_hancom_ungroup() {
    for (hwp, ungrouped) in [
        ("group-f1-g1-scaled.hwp", "group-f1-g1-ungrouped.hwpx"),
        ("group-f1-g3-flipped.hwp", "group-f1-g3-ungrouped.hwpx"),
        ("group-f1-g4-nested.hwp", "group-f1-g4-ungrouped.hwpx"),
        ("group-f11-nested-offset.hwp", "group-f11-nested-offset-ungrouped.hwpx"),
    ] {
        let (_, warnings) = hwp5_group(hwp);
        assert!(lost_kinds(&warnings).is_empty(), "{hwp}: {warnings:?}");
        assert_leaves_match(hwp, ungrouped, 0, &[]);
    }
}

/// F8: a point-edited pentagon (a vertex pulled outside its box, one pushed
/// inside) grouped with a rect, the group scaled ×1.5 by ×0.5. Hancom keeps
/// the points in `[0, orgSz]` and scales through the matrices.
///
/// Hancom's points already fill `[0, orgSz]`, so this does not tell stored
/// points from points stretched onto the box — the projection unit test
/// `polygon_keeps_its_stored_points` does.
///
/// 이것을 실패시키는 것: 행렬열을 무시하는 것(점에 묶음 크기 변경이 안 걸림).
#[test]
fn f8_point_edited_polygon_matches_hancom_ungroup() {
    let (_, warnings) = hwp5_group("group-f8-poly-margin.hwp");
    assert!(lost_kinds(&warnings).is_empty(), "{warnings:?}");
    assert_leaves_match("group-f8-poly-margin.hwp", "group-f8-poly-margin-ungrouped.hwpx", 0, &[]);
}

/// F10: a rect and a triangle turned 30° and a line turned 45° on their own,
/// then the group stretched along x only. The stretch shears the turned
/// rect, which a Core box cannot hold: it is reported once and left out of
/// the comparison; the triangle and the line (points only) land on
/// Hancom's.
///
/// 이것을 실패시키는 것: 내용 없는 점 기반 도형을 상자 규칙으로 보내는 것, shear
/// 손실 보고를 지우는 것.
#[test]
fn f10_sheared_group_keeps_points_and_reports_the_box() {
    let (_, warnings) = hwp5_group("group-f10-rot-then-scale.hwp");
    assert_eq!(lost_kinds(&warnings), ["group_child_transform"]);
    assert_leaves_match(
        "group-f10-rot-then-scale.hwp",
        "group-f10-rot-then-scale-ungrouped.hwpx",
        0,
        &["rect"],
    );
}

/// F9: rect A (turned 30° then mirrored), triangle B (turned 30° then
/// mirrored top-bottom) and ellipse C (mirrored) grouped, the group turned
/// 20° and mirrored; D and E stay outside the group. The group's own frame
/// starts at (−1680, −3359) while its shapes start at (−1679, 0): children
/// are placed from the outer origin, not from the frame.
///
/// Every leaf here is mirrored twice (its own mirror and the group's), so
/// the net map has no mirror; F1 g3 is the mirrored case.
///
/// 이것을 실패시키는 것: 최상위 원점을 묶음 자기 틀의 최솟값으로 잡는 것, 쌍 적용
/// 순서를 첫 쌍부터로 바꾸는 것.
#[test]
fn f9_turned_and_mirrored_group_matches_hancom_ungroup() {
    let (_, warnings) = hwp5_group("rotflip-group.hwp");
    assert!(lost_kinds(&warnings).is_empty(), "{warnings:?}");
    assert_leaves_match("rotflip-group.hwp", "rotflip-group-ungrouped.hwpx", 2, &[]);
}

/// F12: a polygon and a rect (each turned 150°) grouped inside a 1×1 table
/// cell's paragraph, the group turned 330° and mirrored left-right; a rect
/// anchored to the paper stays outside the table and the group.
///
/// The HWP5 group stays a group in the cell (nothing lifted to the body) and
/// loses nothing; its leaves, placed from the group's outer origin, land
/// within 2 HWPUNIT of the children of Hancom's HWPX save of the same file,
/// read from the wire (chain · point, no cell or page position involved).
///
/// Hancom's ungroup re-anchors the children in the body paragraph above the
/// table, so their positions there differ from the grouped ones by the
/// cell's place on the page. Only where the leaves sit relative to each
/// other is compared: one translation, fitted on the polygon's first point,
/// must carry every leaf of ours within 2 HWPUNIT of the ungrouped one.
///
/// The HWPX writer then writes the group back into the cell.
///
/// 이것을 실패시키는 것: 표 칸 문단의 `$con` 을 묶음 빌더로 열지 않는 것(칸의 묶음이
/// `unknown_control` 로 버려진다), 칸 안 개체를 본문 문단에 붙이는 것(묶음이 본문으로
/// 올라간다), HWP5 묶음 행렬열의 쌍 적용 순서를 첫 쌍부터로 바꾸는 것(두 잎이 한컴의
/// 묶음·풀기 답 모두에서 벗어나고 사각형이 기울어 `group_child_transform` 이 보고된다),
/// HWPX 인코더가 표 칸 안의 묶음을 쓰지 않는 것.
#[test]
fn f12_group_in_a_table_cell_matches_hancom() {
    let hwp = "group-f12-cell-group.hwp";
    let doc = decode_hwp5_with_images(&fixture(hwp)).unwrap();
    let loss: Vec<_> = doc
        .warnings
        .iter()
        .filter(|w| {
            matches!(w, Hwp5Warning::ContentLost { .. } | Hwp5Warning::DroppedControl { .. })
        })
        .collect();
    assert!(loss.is_empty(), "{loss:?}");
    let paragraphs = &doc.document.sections()[0].paragraphs;
    assert!(groups_in(paragraphs).is_empty(), "the cell's group was lifted to the body");
    let in_cell = groups_in(cell_paragraphs(paragraphs));
    let [group] = &in_cell[..] else { panic!("{} groups in the cell", in_cell.len()) };
    let Control::Group { children, .. } = group else { unreachable!() };

    let mut ours = Vec::new();
    for child in children {
        collect_child(child, (0.0, 0.0), &mut ours);
    }
    let close = |x: &PageShape, y: &PageShape, shift: (f64, f64)| {
        let moved = |ps: &[(f64, f64)]| -> Vec<(f64, f64)> {
            ps.iter().map(|p| (p.0 + shift.0, p.1 + shift.1)).collect()
        };
        assert_eq!(x.kind, y.kind);
        // A point shape folds its turn and mirror into its points, so only a
        // box's corners keep their order (as in `assert_leaves_match`).
        let corners = if x.kind == "rect" { dist(&moved(&x.corners), &y.corners) } else { 0.0 };
        corners.max(dist(&moved(&x.points), &y.points))
    };

    let theirs = grouped_shapes(&fixture("group-f12-cell-group.hwpx"));
    assert_eq!(ours.len(), theirs.len());
    for (x, y) in ours.iter().zip(&theirs) {
        let off = close(x, y, (0.0, 0.0));
        assert!(off <= 2.0, "{hwp}: {} off Hancom's group by {off:.2}\n{x:?}\n{y:?}", x.kind);
    }

    // The paper-anchored 6303 × 2743 rect outside the group.
    let ungrouped: Vec<PageShape> =
        ungrouped_shapes(&fixture("group-f12-cell-group-ungrouped.hwpx"))
            .into_iter()
            .filter(|s| {
                let (w, h) = (s.corners[2].0 - s.corners[0].0, s.corners[2].1 - s.corners[0].1);
                !(s.kind == "rect" && (w - 6303.0).abs() < 1.0 && (h - 2743.0).abs() < 1.0)
            })
            .collect();
    assert_eq!(ungrouped.len(), ours.len());
    let polygon = |v: &[PageShape]| v.iter().find(|s| s.kind == "polygon").unwrap().points[0];
    let (a, b) = (polygon(&ours), polygon(&ungrouped));
    let shift = (b.0 - a.0, b.1 - a.1);
    for x in &ours {
        let y = ungrouped.iter().find(|s| s.kind == x.kind).unwrap();
        let off = close(x, y, shift);
        assert!(off <= 2.0, "{hwp}: {} off Hancom's ungroup by {off:.2}\n{x:?}\n{y:?}", x.kind);
    }

    // The HWPX writer writes the group back into the cell.
    let (bytes, _) = hwpforge_convert::hwp5_to_hwpx_bytes(&fixture(hwp)).unwrap();
    let back = hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).unwrap();
    let paragraphs = &back.document.sections()[0].paragraphs;
    assert_eq!(groups_in(cell_paragraphs(paragraphs)).len(), 1);
}

/// F2: a group holding a text box with a table and a picture in it. The
/// group child parser keeps only the text box's own text, so the text box
/// stays and the two objects in it are reported, one each. Hancom's HWPX twin
/// carries both objects, so its decode reports nothing. Before, the
/// picture's records leaked into the text box's own and the whole text box
/// was dropped.
///
/// 이것을 실패시키는 것: 묶음 자식 아래 `CtrlHeader` 수집을 지우는 것, 자식의
/// 도형 기록을 자기 수준으로 제한하지 않는 것(글상자가 `unknown` 으로 사라진다),
/// 문단 기록을 자기 수준으로 제한하지 않는 것(표 칸 글자가 글상자 문단을 덮는다).
#[test]
fn f2_objects_in_a_grouped_text_box_are_reported() {
    let (group, warnings) = hwp5_group("group-f2-textbox-table.hwp");
    let ours = lost_kinds(&warnings);
    assert_eq!(ours, ["group_nested_object", "group_nested_object"], "{warnings:?}");
    let Control::Group { children, .. } = &group else { unreachable!() };
    assert!(
        children
            .iter()
            .any(|c| matches!(c, GroupChild::Shape(Control::TextBox { paragraphs, .. }) if !paragraphs.is_empty())),
        "{children:?}"
    );
    let twin =
        hwpforge_smithy_hwpx::HwpxDecoder::decode(&fixture("group-f2-textbox-table.hwpx")).unwrap();
    let theirs: Vec<String> = twin
        .warnings
        .iter()
        .filter_map(|w| match w {
            hwpforge_smithy_hwpx::DecodeWarning::ContentLost { kind, .. } => Some(kind.clone()),
            _ => None,
        })
        .collect();
    assert!(theirs.is_empty(), "{theirs:?}");
    // The text box keeps its own paragraphs' text, as the twin has it — not
    // the text of the table cells inside it.
    let text = |c: &GroupChild| -> Vec<String> {
        let GroupChild::Shape(Control::TextBox { paragraphs, .. }) = c else { return Vec::new() };
        paragraphs.iter().map(paragraph_text).collect()
    };
    let twin_group = twin.document.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| p.runs.iter())
        .find_map(|r| match &r.content {
            RunContent::Control(c) if matches!(**c, Control::Group { .. }) => Some((**c).clone()),
            _ => None,
        })
        .unwrap();
    let Control::Group { children: twin_children, .. } = &twin_group else { unreachable!() };
    let ours_text: Vec<String> = children.iter().flat_map(text).collect();
    assert_eq!(ours_text, twin_children.iter().flat_map(text).collect::<Vec<_>>());
}

fn paragraph_text(p: &hwpforge_core::paragraph::Paragraph) -> String {
    p.runs
        .iter()
        .filter_map(|r| match &r.content {
            RunContent::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

/// F3: picture groups (a logo of three pictures, a rect with two pictures, a
/// cropped, a turned and a grey-scale picture group). Every picture is a
/// Core group child; what a Core picture cannot show is reported, once per
/// picture: the turn of F3d, the grey scale of F3e. F3c's crop is carried,
/// the same as the HWPX decoder reads it from Hancom's HWPX save.
///
/// 이것을 실패시키는 것: HWP5 판정 표의 회전·효과 행을 각각 지우는 것 /
/// 그림을 미지원 자식으로 되돌리는 것(`pic` 손실) / 묶음 그림 자르기를 싣지 않는 것.
#[test]
fn f3_picture_groups_keep_their_pictures() {
    for (name, pictures, expected) in [
        ("group-f3a-logo.hwp", 3, None),
        ("group-f3b-mixed.hwp", 2, None),
        ("group-f3c-cropped.hwp", 3, None),
        ("group-f3d-rotated-pic.hwp", 3, Some("rotation/flip:")),
        ("group-f3e-gray.hwp", 3, Some("effect: 1")),
    ] {
        let doc = decode_hwp5_with_images(&fixture(name)).unwrap();
        let lost: Vec<(String, String)> = doc
            .warnings
            .iter()
            .filter_map(|w| match w {
                Hwp5Warning::ContentLost { kind, reason, .. } => {
                    Some((kind.clone(), reason.clone()))
                }
                _ => None,
            })
            .collect();
        match expected {
            None => assert!(lost.is_empty(), "{name}: {lost:?}"),
            Some(reason) => {
                assert_eq!(lost.len(), 1, "{name}: {lost:?}");
                assert_eq!(lost[0].0, "group_child_picture", "{name}");
                assert!(lost[0].1.starts_with(reason), "{name}: {lost:?}");
            }
        }
        let Control::Group { children, .. } = first_group(&doc.document) else { unreachable!() };
        let count = children.iter().filter(|c| matches!(c, GroupChild::Picture(_))).count();
        assert_eq!(count, pictures, "{name}");
    }
    let crops = |group: Control| match group {
        Control::Group { children, .. } => children
            .into_iter()
            .filter_map(|c| match c {
                GroupChild::Picture(img) => Some((img.crop, img.image_extent)),
                _ => None,
            })
            .collect::<Vec<_>>(),
        _ => unreachable!(),
    };
    let ours = crops(first_group(
        &decode_hwp5_with_images(&fixture("group-f3c-cropped.hwp")).unwrap().document,
    ));
    let twin =
        hwpforge_smithy_hwpx::HwpxDecoder::decode(&fixture("group-f3c-cropped.hwpx")).unwrap();
    let theirs = crops(first_group(&twin.document));
    assert_eq!(ours, theirs);
    assert_eq!(ours.iter().filter(|(c, _)| c.is_some()).count(), 1, "{ours:?}");
}

// ── twin: the HWPX decoder on Hancom's HWPX save of the same group ──

/// One leaf's Core values with its offset summed down the group path.
#[derive(Debug)]
struct CoreLeaf {
    kind: &'static str,
    ints: Vec<i64>,
    rotation: f32,
    flip: Option<hwpforge_foundation::Flip>,
}

fn core_leaves(c: &Control, base: (i64, i64), out: &mut Vec<CoreLeaf>) {
    let o = offset_of(c);
    let at = (base.0 + o.0 as i64, base.1 + o.1 as i64);
    if let Control::Group { children, .. } = c {
        for child in children {
            match child {
                GroupChild::Shape(child) => core_leaves(child, at, out),
                GroupChild::Picture(image) => {
                    let (x, y) = image.placement.as_ref().map_or((0, 0), |p| {
                        (i64::from(p.horz_offset.as_i32()), i64::from(p.vert_offset.as_i32()))
                    });
                    out.push(CoreLeaf {
                        kind: "picture",
                        ints: vec![
                            i64::from(image.width.as_i32()),
                            i64::from(image.height.as_i32()),
                            at.0 + x,
                            at.1 + y,
                        ],
                        rotation: 0.0,
                        flip: None,
                    });
                }
                _ => {}
            }
        }
        return;
    }
    let (w, h, style) = match c {
        Control::TextBox { width, height, style, .. }
        | Control::Rect { width, height, style, .. }
        | Control::Ellipse { width, height, style, .. }
        | Control::Polygon { width, height, style, .. }
        | Control::Line { width, height, style, .. }
        | Control::Curve { width, height, style, .. }
        | Control::ConnectLine { width, height, style, .. } => {
            (width.as_i32(), height.as_i32(), style)
        }
        other => panic!("unexpected leaf {other:?}"),
    };
    let (kind, pts) = leaf_points(c);
    let mut ints = vec![i64::from(w), i64::from(h), at.0, at.1];
    // Points as drawn: S20 keeps an axis-aligned leaf's points in its
    // original box, scaled to the displayed one.
    let o = c.effective_original_size().unwrap();
    let scale = |d: i32, o: hwpforge_foundation::HwpUnit| {
        if o.as_i32() == 0 {
            1.0
        } else {
            f64::from(d) / f64::from(o.as_i32())
        }
    };
    let (sx, sy) = (scale(w, o.width), scale(h, o.height));
    ints.extend(pts.iter().flat_map(|p| {
        [(f64::from(p.x) * sx).round() as i64, (f64::from(p.y) * sy).round() as i64]
    }));
    out.push(CoreLeaf {
        kind,
        ints,
        rotation: style.as_ref().and_then(|s| s.rotation).unwrap_or(0.0),
        flip: style.as_ref().and_then(|s| s.flip),
    });
}

/// The same group decoded from Hancom's HWP5 and HWPX saves gives the same
/// Core, leaf by leaf: sizes, offsets (summed down the group path) and
/// points within 1 HWPUNIT (HWP5 stores f64 matrices, HWPX six-digit
/// decimals), rotation within 0.001°, the same mirror.
/// `rotflip-group` is a turned and mirrored group whose own frame starts
/// at (−1680, −3359): both decoders place its children from the outer
/// origin. F12's group sits in a table cell.
///
/// 이것을 실패시키는 것: 쌍 적용 순서를 첫 쌍부터로 바꾸는 것(한쪽 decoder 만),
/// 최상위 원점을 묶음 자기 틀의 최솟값으로 잡는 것(rotflip), 표 칸 문단의 `$con` 을
/// 묶음 빌더로 열지 않는 것(F12).
#[test]
fn hwp5_and_hwpx_saves_decode_to_the_same_group() {
    for (hwp, hwpx) in [
        ("group-f1-g1-scaled.hwp", "group-f1-g1-scaled.hwpx"),
        ("group-f1-g3-flipped.hwp", "group-f1-g3-flipped.hwpx"),
        ("group-f1-g4-nested.hwp", "group-f1-g4-nested.hwpx"),
        ("group-f8-poly-margin.hwp", "group-f8-poly-margin.hwpx"),
        ("group-f10-rot-then-scale.hwp", "group-f10-rot-then-scale.hwpx"),
        ("rotflip-group.hwp", "rotflip-group.hwpx"),
        ("group-f11-nested-offset.hwp", "group-f11-nested-offset.hwpx"),
        ("group-f12-cell-group.hwp", "group-f12-cell-group.hwpx"),
        ("group-f3a-logo.hwp", "group-f3a-logo.hwpx"),
        ("group-f3b-mixed.hwp", "group-f3b-mixed.hwpx"),
        ("group-f3c-cropped.hwp", "group-f3c-cropped.hwpx"),
        ("group-f3d-rotated-pic.hwp", "group-f3d-rotated-pic.hwpx"),
        ("group-f3e-gray.hwp", "group-f3e-gray.hwpx"),
    ] {
        let (ours, _) = hwp5_group(hwp);
        let twin = hwpforge_smithy_hwpx::HwpxDecoder::decode(&fixture(hwpx)).unwrap();
        let theirs = first_group(&twin.document);
        let (mut a, mut b) = (Vec::new(), Vec::new());
        core_leaves(&ours, (0, 0), &mut a);
        core_leaves(&theirs, (0, 0), &mut b);
        assert_eq!(a.len(), b.len(), "{hwp}");
        for (x, y) in a.iter().zip(&b) {
            let close = x.kind == y.kind
                && x.ints.len() == y.ints.len()
                && x.ints.iter().zip(&y.ints).all(|(p, q)| (p - q).abs() <= 1)
                && (x.rotation - y.rotation).abs() < 1e-3
                && x.flip == y.flip;
            assert!(close, "{hwp}:\n  hwp5 {x:?}\n  hwpx {y:?}");
        }
    }
}

/// The HWP5 group written by the HWPX writer and decoded again gives
/// the same shapes, leaf by leaf (corners in order and defining points
/// within 2 HWPUNIT), with no lost content. F11's inner group (a rect and
/// an ellipse) sits away from its outer group's corner, so the writer's
/// nested-offset convention is pinned too.
///
/// Both steps must lose nothing: the conversion's own warnings carry no
/// `CONTENT_LOST` and no other semantic loss, and the re-decode none either.
///
/// 이것을 실패시키는 것: HWPX 방출 정책의 행렬 계수를 소수 1자리로 거칠게 쓰는
/// 것(F1 크기 변경 묶음의 잎이 2 넘게 어긋나고 `shape_emission` 경고가 난다),
/// 인코더가 중첩 묶음 자식에 부모 원점을 더하지 않는 것(F11), 표 칸 문단의 `$con` 을
/// 묶음 빌더로 열지 않는 것·인코더가 표 칸 안의 묶음을 쓰지 않는 것(F12).
#[test]
fn hwp5_groups_survive_the_hwpx_writer() {
    for hwp in [
        "group-f1-g1-scaled.hwp",
        "group-f1-g3-flipped.hwp",
        "group-f1-g4-nested.hwp",
        "group-f8-poly-margin.hwp",
        "rotflip-group.hwp",
        "group-f11-nested-offset.hwp",
        "group-f12-cell-group.hwp",
        "group-f3a-logo.hwp",
        "group-f3b-mixed.hwp",
    ] {
        let (ours, _) = hwp5_group(hwp);
        let (bytes, conversion) = hwpforge_convert::hwp5_to_hwpx_bytes(&fixture(hwp)).unwrap();
        let conversion_loss: Vec<_> = conversion
            .iter()
            .filter(|w| match w {
                hwpforge_convert::ConvertWarning::Hwp5(w) => {
                    matches!(w, hwpforge_smithy_hwp5::Hwp5Warning::ContentLost { .. })
                }
                hwpforge_convert::ConvertWarning::HwpxEncode(w) => w.is_semantic_loss(),
                // Any future kind counts as a loss until it is classified.
                _ => true,
            })
            .collect();
        assert!(conversion_loss.is_empty(), "{hwp}: {conversion_loss:?}");
        let back = hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).unwrap();
        let lost: Vec<_> = back
            .warnings
            .iter()
            .filter(|w| matches!(w, hwpforge_smithy_hwpx::DecodeWarning::ContentLost { .. }))
            .collect();
        assert!(lost.is_empty(), "{hwp}: {lost:?}");
        let again = first_group(&back.document);
        // The writer stores whole degrees (F9 rect 9.9977° → 10°), so the
        // turn is compared by where the corners land, not by the angle.
        let page = |g: &Control| {
            let mut out = Vec::new();
            if let Control::Group { children, .. } = g {
                for child in children {
                    collect_child(child, offset_of(g), &mut out);
                }
            }
            out
        };
        let (a, b) = (page(&ours), page(&again));
        assert_eq!(a.len(), b.len(), "{hwp}");
        for (x, y) in a.iter().zip(&b) {
            let off = dist(&x.corners, &y.corners).max(dist(&x.points, &y.points));
            assert!(
                x.kind == y.kind && off <= 2.0,
                "{hwp}: off by {off:.2}\n  hwp5 {x:?}\n  back {y:?}"
            );
        }
    }
}
