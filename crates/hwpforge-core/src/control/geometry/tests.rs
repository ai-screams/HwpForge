//! Expected values are computed independently from the definition
//! `offset + C + R(θ)·F·(p − C)` (worked outside the crate), not from this
//! module.

use super::*;

const HWPX_LIKE: EmissionPolicy =
    EmissionPolicy::new(AngleRounding::WholeDegrees, CoefficientRounding::Decimals(6), true);

fn unit(v: i32) -> HwpUnit {
    HwpUnit::new(v).unwrap()
}

fn rect(w: i32, h: i32, rotation: Option<f32>, flip: Option<Flip>) -> Control {
    let mut c = Control::rect(unit(w), unit(h)).unwrap();
    set_rotation_and_flip(&mut c, rotation, flip);
    c
}

fn with_offset(mut c: Control, x: i32, y: i32) -> Control {
    if let Some(b) = shape_box(&c) {
        let (w, h) = (b.width, b.height);
        set_box(&mut c, w, h, (x, y));
    }
    c
}

fn close(a: (f64, f64), b: (f64, f64), tol: f64) -> bool {
    (a.0 - b.0).abs() <= tol && (a.1 - b.1).abs() <= tol
}

fn kept(outcome: FoldOutcome) -> (Control, Vec<FoldLoss>) {
    match outcome {
        FoldOutcome::Kept { control, losses } => (*control, losses),
        FoldOutcome::Dropped { losses } => panic!("dropped: {losses:?}"),
    }
}

fn kinds(losses: &[FoldLoss]) -> Vec<FoldLossKind> {
    losses.iter().map(FoldLoss::kind).collect()
}

fn rotation_and_flip(c: &Control) -> (Option<f32>, Option<Flip>) {
    shape_box(c).and_then(|b| b.style).map_or((None, None), |s| (s.rotation, s.flip))
}

/// A 1000×500 box at (100, 200), turned 30° and mirrored left-right: the
/// mirror applies first and both turn about the centre.
///
/// 이것을 실패시키는 것: 반사와 회전 순서를 뒤집는 것(`F·R` — 첫 모서리가
/// (908.01, −16.51) 로 감), 또는 원점 기준 회전.
#[test]
fn mirror_applies_before_rotation_about_the_centre() {
    let c = with_offset(rect(1000, 500, Some(30.0), Some(Flip::Horizontal)), 100, 200);
    let got = emitted_corners(&c, &EmissionPolicy::exact()).unwrap();
    let want = [
        (1158.012702, 483.493649),
        (291.987298, -16.506351),
        (41.987298, 416.506351),
        (908.012702, 916.506351),
    ];
    for (g, w) in got.iter().zip(want) {
        assert!(close(*g, w, 1e-6), "{got:?}");
    }
}

/// Folding does not depend on any output format: a child turned by 30.4°
/// keeps 30.4° (as `f32`) and its offset is solved from the exact angle.
/// Solving it from a whole-degree matrix would give (4080, 5433).
///
/// 이것을 실패시키는 것: 접기에서 각도를 정수로 반올림하는 것, 또는 offset 을
/// 방출 정책 행렬로 역산하는 것.
#[test]
fn fold_keeps_the_exact_angle_and_offset() {
    let (s, c) = 30.4f64.to_radians().sin_cos();
    let map = ShapeTransform::from_matrix([c, -s, 5000.0, s, c, 3000.0]);
    let input = FoldInput::new(map, (10_000.0, 1_000.0), (0.0, 0.0));
    let (child, losses) = kept(fold_group_child(rect(1, 1, None, None), &input));
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(rotation_and_flip(&child), (Some(30.4_f32), None));
    let b = shape_box(&child).unwrap();
    assert_eq!((b.width, b.height, b.offset), (10_000, 1_000, (4060, 5461)));
}

/// A 10,000,000 × 1,000 child turned by 30.0004° keeps that angle; storing
/// it as `f32` leaves it 0.05 off, inside the tolerance. Rounding to 30°
/// would leave it 34.91 off.
///
/// 이것을 실패시키는 것: 접기 각도를 0.001° 등 고정 간격으로 정규화하는 것
/// (`Residual` 이 생김).
#[test]
fn fold_keeps_a_tiny_fraction_of_a_degree() {
    let (s, c) = 30.0004f64.to_radians().sin_cos();
    let map = ShapeTransform::from_matrix([c, -s, 0.0, s, c, 0.0]);
    let input = FoldInput::new(map, (10_000_000.0, 1_000.0), (0.0, 0.0));
    let (child, losses) = kept(fold_group_child(rect(1, 1, None, None), &input));
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(rotation_and_flip(&child).0, Some(30.0004_f32));
}

/// Storing 30.4° as a whole degree: the 10,000 × 1,000 box moves its local
/// origin by (−20.58, +28.41) and a corner by 35.08; the 100 × 100 box only
/// by 0.494, which is no loss.
///
/// 이것을 실패시키는 것: 잔차를 정책 대신 원시 각도로 계산하는 것, 또는
/// 정수 아닌 각이면 무조건 손실로 보는 것.
#[test]
fn whole_degree_storage_is_judged_by_the_actual_residual() {
    let big = rect(10_000, 1_000, Some(30.4), None);
    let check = emission_residual(&big, &HWPX_LIKE);
    assert!((check.residual() - 35.08).abs() < 0.01, "{}", check.residual());
    assert_eq!(check.loss().map(|l| l.kind()), Some(FoldLossKind::Residual));
    let stored = local_transform(&big, &HWPX_LIKE).unwrap().apply((0.0, 0.0));
    let exact = local_transform(&big, &EmissionPolicy::exact()).unwrap().apply((0.0, 0.0));
    assert!(close((stored.0 - exact.0, stored.1 - exact.1), (-20.58, 28.41), 0.01));

    let small = emission_residual(&rect(100, 100, Some(30.4), None), &HWPX_LIKE);
    assert!((small.residual() - 0.494).abs() < 0.001, "{}", small.residual());
    assert!(small.loss().is_none());
}

/// Six-digit coefficients alone move a corner of a 10,000,000-wide box by
/// 4.04 at a whole 30°.
///
/// 이것을 실패시키는 것: 잔차 계산에서 계수 양자화를 빼는 것(원시 f64).
#[test]
fn six_digit_coefficients_count_in_the_residual() {
    let check = emission_residual(&rect(10_000_000, 1_000, Some(30.0), None), &HWPX_LIKE);
    assert!((check.residual() - 4.038).abs() < 0.001, "{}", check.residual());
    assert!(check.loss().is_some());
    let exact_digits =
        EmissionPolicy::new(AngleRounding::WholeDegrees, CoefficientRounding::Exact, true);
    let raw = emission_residual(&rect(10_000_000, 1_000, Some(30.0), None), &exact_digits);
    assert!(raw.residual() < 1e-6);
}

/// Whole degrees round half away from zero: 29.5→30, 30.5→31. The wrap
/// into `[0, 360)` (−30→330, −0.4→360) does not change the matrix — a turn
/// by −30° is a turn by 330° — so it is pinned on the writer's stored angle
/// instead (smithy-hwpx `rotation_angle_boundaries_match_the_writer`).
///
/// 이것을 실패시키는 것: 반올림을 내림(29.5 → 29)이나 짝수 반올림(30.5 → 30)으로
/// 바꾸는 것.
#[test]
fn whole_degree_boundaries_match_the_writer() {
    for (given, stored) in
        [(29.5, 30.0), (30.5, 31.0), (-30.0, 330.0), (359.6, 360.0), (-0.4, 360.0)]
    {
        let got = local_transform(&rect(1000, 500, Some(given), None), &HWPX_LIKE).unwrap();
        let want = local_transform(&rect(1000, 500, Some(stored), None), &EmissionPolicy::exact())
            .unwrap();
        let q = |v: f64| quantize(v, CoefficientRounding::Decimals(6));
        assert_eq!(got.matrix(), want.matrix().map(q), "{given}");
    }
}

/// Mirrored both ways and turned 37° is the same matrix as an unmirrored
/// 217° turn.
///
/// 이것을 실패시키는 것: 양축 반사에 각도 부호 변환(`R(−θ)`)을 적용하는 것,
/// 또는 회전과 함께 있는 반사를 반사만 남기는 것.
#[test]
fn both_mirrors_with_a_rotation_equal_a_half_turn_more() {
    let both =
        local_transform(&rect(1000, 500, Some(37.0), Some(Flip::Both)), &EmissionPolicy::exact())
            .unwrap();
    let turn =
        local_transform(&rect(1000, 500, Some(217.0), None), &EmissionPolicy::exact()).unwrap();
    for (a, b) in both.matrix().iter().zip(turn.matrix()) {
        assert!((a - b).abs() < 1e-9, "{both:?} vs {turn:?}");
    }
}

/// A policy that cannot store a mirror with a rotation writes the mirror
/// only and says so.
///
/// 이것을 실패시키는 것: `mirror_with_rotation` 검사를 지우는 것.
#[test]
fn mirror_and_rotation_the_policy_cannot_store_is_unemittable() {
    let c = rect(1000, 500, Some(30.0), Some(Flip::Horizontal));
    let no_mix =
        EmissionPolicy::new(AngleRounding::WholeDegrees, CoefficientRounding::Decimals(6), false);
    let check = emission_residual(&c, &no_mix);
    assert!(check.is_unstorable());
    assert_eq!(check.loss().map(|l| l.kind()), Some(FoldLossKind::Unemittable));
    assert_eq!(local_transform(&c, &no_mix).unwrap().matrix(), [-1.0, 0.0, 1000.0, 0.0, 1.0, 0.0]);
    assert!(!emission_residual(&c, &HWPX_LIKE).is_unstorable());
}

/// 이것을 실패시키는 것: `Err` 대신 0 크기 `Ok` 를 돌려주는 것, 또는 shear·범위
/// 검사를 지우는 것.
#[test]
fn fold_box_refuses_what_it_cannot_place() {
    let at = |m: [f64; 6]| {
        fold_box(
            100.0,
            50.0,
            &FoldInput::new(ShapeTransform::from_matrix(m), (100.0, 50.0), (0.0, 0.0)),
        )
    };
    assert_eq!(at([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]).unwrap_err().kind(), FoldLossKind::Degenerate);
    assert_eq!(at([0.004, 0.0, 0.0, 0.0, 1.0, 0.0]).unwrap_err().kind(), FoldLossKind::Degenerate);
    assert_eq!(at([1.0, 1.0, 0.0, 0.0, 1.0, 0.0]).unwrap_err().kind(), FoldLossKind::Shear);
    assert_eq!(at([1.0, 0.0, 2.0e8, 0.0, 1.0, 0.0]).unwrap_err().kind(), FoldLossKind::OutOfRange);
    assert_eq!(
        at([f64::NAN, 0.0, 0.0, 0.0, 1.0, 0.0]).unwrap_err().kind(),
        FoldLossKind::Degenerate
    );
    assert!(at([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]).is_ok());
}

/// A mirror in the map becomes a horizontal flip; mirror and turn together
/// keep the flip and adjust the angle.
///
/// 이것을 실패시키는 것: 행렬식 음수 판정을 지우는 것.
#[test]
fn fold_reports_a_mirror_as_a_horizontal_flip() {
    let map = ShapeTransform::from_matrix([-1.0, 0.0, 100.0, 0.0, 1.0, 0.0]);
    let b = fold_box(100.0, 50.0, &FoldInput::new(map, (100.0, 50.0), (0.0, 0.0))).unwrap();
    assert_eq!((b.rotation(), b.flip(), b.offset()), (None, Some(Flip::Horizontal), (0, 0)));
    assert!(b.losses().is_empty());
}

/// A line takes a shearing map onto its points and keeps no rotation.
///
/// 이것을 실패시키는 것: 점 기반 도형을 상자 규칙으로 보내는 것(shear 손실).
#[test]
fn point_shape_takes_the_whole_map() {
    let line = Control::line(ShapePoint::new(0, 0), ShapePoint::new(100, 100)).unwrap();
    let map = ShapeTransform::from_matrix([1.0, 0.5, 10.0, 0.0, 1.0, 20.0]);
    let (child, losses) =
        kept(fold_group_child(line, &FoldInput::new(map, (100.0, 100.0), (0.0, 0.0))));
    assert!(losses.is_empty(), "{losses:?}");
    let Control::Line { start, end, width, height, .. } = &child else { panic!() };
    assert_eq!((start.x, start.y, end.x, end.y), (0, 0, 150, 100));
    assert_eq!((width.as_i32(), height.as_i32()), (150, 100));
    assert_eq!(shape_box(&child).unwrap().offset, (10, 20));
    assert_eq!(rotation_and_flip(&child), (None, None));
}

/// The same line, told that something turns with it, folds like a box.
///
/// 이것을 실패시키는 것: `GeometryFacts` 를 무시하는 것.
#[test]
fn geometry_facts_send_a_point_shape_to_the_box_rule() {
    let line = Control::line(ShapePoint::new(0, 0), ShapePoint::new(100, 100)).unwrap();
    let map = ShapeTransform::from_matrix([0.0, -1.0, 100.0, 1.0, 0.0, 0.0]);
    let input = FoldInput::new(map, (100.0, 100.0), (0.0, 0.0))
        .with_geometry_facts(GeometryFacts::new(true));
    let (child, _) = kept(fold_group_child(line, &input));
    assert_eq!(rotation_and_flip(&child), (Some(90.0), None));
}

/// TextArt has no rotation field: it keeps an unrotated box with the same
/// centre and reports it. An equation has no offset field.
///
/// 이것을 실패시키는 것: `Unstorable` 판정을 지우는 것.
#[test]
fn variants_without_fields_report_unstorable() {
    let art = Control::TextArt {
        frame: crate::frame::ObjectFrame::new(),
        text: "x".into(),
        shape: "WAVE2".into(),
        font_name: String::new(),
        font_style: String::new(),
        align: "LEFT".into(),
        line_spacing: 100,
        char_spacing: 100,
        width: unit(100),
        height: unit(50),
        placement: None,
        fill_color: None,
        inst_id: None,
    };
    let turn = ShapeTransform::from_matrix([0.0, -1.0, 50.0, 1.0, 0.0, 0.0]);
    let (child, losses) =
        kept(fold_group_child(art, &FoldInput::new(turn, (100.0, 50.0), (0.0, 0.0))));
    assert_eq!(kinds(&losses), [FoldLossKind::Unstorable]);
    // Centre (25, 50) kept: box 100 × 50 from (−25, 25).
    assert_eq!(shape_box(&child).unwrap().offset, (-25, 25));

    let shift = ShapeTransform::from_matrix([1.0, 0.0, 30.0, 0.0, 1.0, 0.0]);
    let (_, losses) = kept(fold_group_child(
        Control::equation("x"),
        &FoldInput::new(shift, (100.0, 50.0), (0.0, 0.0)),
    ));
    assert_eq!(kinds(&losses), [FoldLossKind::Unstorable]);
    let still = ShapeTransform::from_matrix([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    let (_, losses) = kept(fold_group_child(
        Control::equation("x"),
        &FoldInput::new(still, (100.0, 50.0), (0.0, 0.0)),
    ));
    assert!(losses.is_empty());
}

/// The child's offset is measured from its direct parent's corner.
///
/// 이것을 실패시키는 것: `parent_origin` 차감을 지우는 것 또는 두 번 빼는 것.
#[test]
fn offset_is_relative_to_the_parent_origin() {
    let map = ShapeTransform::from_matrix([1.0, 0.0, 700.0, 0.0, 1.0, 900.0]);
    let (child, _) = kept(fold_group_child(
        rect(1, 1, None, None),
        &FoldInput::new(map, (10.0, 10.0), (200.0, 300.0)),
    ));
    assert_eq!(shape_box(&child).unwrap().offset, (500, 600));
}

/// 2 HWPUNIT is still exact, 3 is not.
///
/// 이것을 실패시키는 것: `>` 를 `>=` 로 바꾸는 것, 또는 허용 오차를 무한으로.
#[test]
fn residual_tolerance_is_two_hwpunit() {
    assert!(residual_loss(2.0).is_none());
    assert_eq!(residual_loss(3.0).map(|l| l.kind()), Some(FoldLossKind::Residual));
}

/// A polygon collapsing to a line and a collapsed box are dropped.
///
/// 이것을 실패시키는 것: 붕괴 판정을 지우는 것.
#[test]
fn collapsed_children_are_dropped() {
    let tri = Control::polygon(vec![
        ShapePoint::new(0, 0),
        ShapePoint::new(10, 0),
        ShapePoint::new(0, 10),
    ])
    .unwrap();
    let flat = ShapeTransform::from_matrix([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let input = FoldInput::new(flat, (10.0, 10.0), (0.0, 0.0));
    assert!(matches!(fold_group_child(tri, &input), FoldOutcome::Dropped { .. }));
    assert!(matches!(
        fold_group_child(rect(1, 1, None, None), &input),
        FoldOutcome::Dropped { .. }
    ));
}

#[test]
fn groups_and_non_shapes_are_not_folded_here() {
    let id = ShapeTransform::from_matrix([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    let input = FoldInput::new(id, (10.0, 10.0), (0.0, 0.0));
    assert!(matches!(
        fold_group_child(Control::bookmark("b"), &input),
        FoldOutcome::Dropped { .. }
    ));
    assert_eq!(emission_residual(&Control::bookmark("b"), &HWPX_LIKE).residual(), 0.0);
    assert!(emitted_corners(&Control::bookmark("b"), &HWPX_LIKE).is_none());
}

/// An equation has no rotation or offset field, but a shear must still be
/// reported: `[1, 0.1, −5, 0, 1, 0]` slants a 100 × 100 box by 5 HWPUNIT.
///
/// 이것을 실패시키는 것: 저장 필드가 없는 갈래에서 shear·잔차 검사를 건너뛰고
/// 손실 없이 돌아가는 것.
#[test]
fn a_shear_on_a_variant_without_a_rotation_field_is_still_reported() {
    let slant = ShapeTransform::from_matrix([1.0, 0.1, -5.0, 0.0, 1.0, 0.0]);
    let (_, losses) = kept(fold_group_child(
        Control::equation("x"),
        &FoldInput::new(slant, (100.0, 100.0), (0.0, 0.0)),
    ));
    assert_eq!(kinds(&losses), [FoldLossKind::Shear]);
}

/// A line, curve or connector squeezed to one point is dropped; a line
/// that was a single point to begin with is not something the map did.
///
/// 이것을 실패시키는 것: 점 기반 붕괴 판정을 다각형에만 두는 것, 붕괴를 반올림한
/// 로컬 크기로만 판정하는 것(반 단위 이동이 1 × 1 로 살아남음), 저장되는 정수
/// 점의 폭을 보지 않는 것(작은 scale 이 두 점을 같은 점으로 저장), 또는 원래 한
/// 점이던 선까지 버리는 것.
#[test]
fn point_shapes_squeezed_to_a_point_are_dropped() {
    let (a, b) = (ShapePoint::new(0, 0), ShapePoint::new(100, 100));
    // Onto a grid point, and onto (−0.5, −0.5), which rounds to a 1 × 1 box
    // from the offset.
    // Also a tiny scale whose points round to one stored point:
    // (0,0)→(−0.5,−0.5) and (100,100)→(0.28,0.28) both store as (1,1).
    for (k, t) in [(0.0, (10.0, 20.0)), (0.0, (-0.5, -0.5)), (0.0078125, (-0.5, -0.5))] {
        let squeeze = FoldInput::new(
            ShapeTransform::from_matrix([k, 0.0, t.0, 0.0, k, t.1]),
            (100.0, 100.0),
            (0.0, 0.0),
        );
        for shape in [
            Control::line(a, b).unwrap(),
            Control::curve(vec![a, b, ShapePoint::new(50, 0)]).unwrap(),
            Control::connect_line(a, b).unwrap(),
        ] {
            let kind = shape.kind_name();
            match fold_group_child(shape, &squeeze) {
                FoldOutcome::Dropped { losses } => {
                    assert_eq!(kinds(&losses), [FoldLossKind::Degenerate], "{kind} {t:?}");
                }
                other => panic!("{kind} {t:?}: {other:?}"),
            }
        }
    }
    let shift = FoldInput::new(
        ShapeTransform::from_matrix([1.0, 0.0, 10.0, 0.0, 1.0, 20.0]),
        (0.0, 0.0),
        (0.0, 0.0),
    );
    let mut dot = Control::line(a, b).unwrap();
    if let Control::Line { end, .. } = &mut dot {
        *end = a;
    }
    assert!(matches!(fold_group_child(dot, &shift), FoldOutcome::Kept { .. }));
}

/// Mirrored both ways and turned is a plain turn 180° further, so even a
/// policy that cannot store a mirror with a rotation stores it exactly.
///
/// 이것을 실패시키는 것: 양축 반사를 단축 반사처럼 정책 제약에 걸리게 두는 것
/// (반사만 남아 모서리 오차 약 354.76, `Unemittable`).
#[test]
fn both_mirrors_need_no_mirror_with_rotation() {
    let no_mix =
        EmissionPolicy::new(AngleRounding::WholeDegrees, CoefficientRounding::Decimals(6), false);
    let check = emission_residual(&rect(1000, 500, Some(37.0), Some(Flip::Both)), &no_mix);
    assert!(!check.is_unstorable() && check.loss().is_none(), "{check:?}");
    assert!(check.residual() < 1e-3, "{check:?}");
}

/// A fold reads only finite numbers: a parent origin that is not a number
/// drops the child instead of turning into a zero offset.
///
/// 이것을 실패시키는 것: 유한성 검사를 행렬에만 두는 것.
#[test]
fn non_finite_inputs_are_refused() {
    let id = ShapeTransform::from_matrix([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    let nan_origin = FoldInput::new(id, (100.0, 50.0), (f64::NAN, 0.0));
    assert!(matches!(
        fold_group_child(rect(1, 1, None, None), &nan_origin),
        FoldOutcome::Dropped { .. }
    ));
    assert_eq!(fold_box(100.0, 50.0, &nan_origin).unwrap_err().kind(), FoldLossKind::Degenerate);
    let nan_size = FoldInput::new(id, (f64::NAN, 50.0), (0.0, 0.0));
    assert!(matches!(
        fold_group_child(rect(1, 1, None, None), &nan_size),
        FoldOutcome::Dropped { .. }
    ));
    assert!(max_residual(&[(0.0, 0.0)], &[(f64::NAN, 0.0)]).is_infinite());
}

/// Core keeps the angle as `f32`. On a box 100,000,000 wide, 300.00001°
/// stores as 300° and the far corner moves about 17.45: kept, reported.
///
/// 이것을 실패시키는 것: 최종 Core 값의 잔차 검사를 지우는 것
/// (`fold_boxed`·`fold_box` 각각).
#[test]
fn an_angle_f32_cannot_hold_on_a_huge_box_is_a_residual() {
    let (s, c) = 300.00001f64.to_radians().sin_cos();
    let map = ShapeTransform::from_matrix([c, -s, 0.0, s, c, 0.0]);
    let input = FoldInput::new(map, (100_000_000.0, 1_000.0), (0.0, 0.0));
    let (child, losses) = kept(fold_group_child(rect(1, 1, None, None), &input));
    assert_eq!(rotation_and_flip(&child).0, Some(300.0));
    assert_eq!(kinds(&losses), [FoldLossKind::Residual]);
    let b = fold_box(100_000_000.0, 1_000.0, &input).unwrap();
    assert_eq!(b.losses().iter().map(FoldLoss::kind).collect::<Vec<_>>(), [FoldLossKind::Residual]);
}

/// A line stretched past the HWPUNIT range is dropped as out of range, not
/// a panic from measuring its stored points.
///
/// 이것을 실패시키는 것: 저장 점 폭을 뺄셈(`max − min`)으로 재는 것(overflow 검사
/// 빌드에서 panic).
#[test]
fn a_point_shape_stretched_past_the_range_is_dropped() {
    let line = Control::line(ShapePoint::new(0, 0), ShapePoint::new(100_000_000, 100)).unwrap();
    let stretch = ShapeTransform::from_matrix([100.0, 0.0, -5_000_000_000.0, 0.0, 1.0, 0.0]);
    match fold_group_child(line, &FoldInput::new(stretch, (100_000_000.0, 100.0), (0.0, 0.0))) {
        FoldOutcome::Dropped { losses } => {
            assert_eq!(kinds(&losses), [FoldLossKind::OutOfRange]);
        }
        other => panic!("{other:?}"),
    }
}

// ── S20 frame emission plan ─────────────────────────────────────

fn framed_line(end: (i32, i32), size: (i32, i32), original: (i32, i32)) -> Control {
    let mut c = Control::Line {
        start: ShapePoint { x: 0, y: 0 },
        end: ShapePoint { x: end.0, y: end.1 },
        width: unit(size.0),
        height: unit(size.1),
        frame: crate::frame::ObjectFrame::new(),
        placement: None,
        caption: None,
        style: None,
    };
    c.frame_mut().unwrap().original_size =
        Some(hwpforge_foundation::Size::new(unit(original.0), unit(original.1)));
    c
}

/// The extreme ratio of §19.23.11 H3: shown 49 wide from an original box of
/// 100,000,000 the six-decimal scale is `0.000000` — the end would land 49
/// off. The plan falls back to the displayed box (end at 49, scale 1) and
/// is exact again.
/// 이것을 실패시키는 것: fallback 을 지우기(배율 0 으로 끝점 0) / 원래 상자 =
/// 표시 상자일 때도 fallback 으로 보기(아래 두 번째 단언).
#[test]
fn extreme_ratio_falls_back() {
    let line = framed_line((100_000_000, 0), (49, 0), (100_000_000, 0));
    let plan = frame_plan(&line, &HWPX_LIKE).unwrap();
    assert!(plan.is_fallback());
    assert_eq!((plan.original_size(), plan.scale()), ((49, 0), (1.0, 1.0)));
    assert_eq!(plan.points()[1], ShapePoint { x: 49, y: 0 });
    assert!(plan.loss().is_none());
    assert!(close(plan.stored_positions()[5], (49.0, 0.0), 1e-9));
    // A frame that is the displayed box never falls back, even when the
    // policy misses (a turn of 30.4° on a large box).
    let turned = rect(10_000, 1_000, Some(30.4), None);
    let plan = frame_plan(&turned, &HWPX_LIKE).unwrap();
    assert!(!plan.is_fallback());
    assert_eq!(plan.loss().map(FoldLoss::kind), Some(FoldLossKind::Residual));
}

/// A group the sibling sends to its fallback measures every framed node
/// against its exact Core place, not against the copy rounded into the
/// displayed box: a line with original box 1500, shown 1000, turned 0.15°
/// (stored as 0°) and ending at 1499 lands at (999, 0) instead of
/// (1000.6406, 1.3090) — off by 2.099, a loss the writer reports.
/// 이것을 실패시키는 것: fallback 을 반올림한 사본 기준으로 다시 재기.
#[test]
fn fallback_measures_against_the_original_points() {
    let mut line = framed_line((1499, 0), (1000, 1000), (1500, 1500));
    set_rotation_and_flip(&mut line, Some(0.15), None);
    let trigger = with_offset(framed_line((100_000_000, 0), (49, 0), (100_000_000, 0)), 1200, 0);
    let group = Control::Group {
        children: vec![
            super::super::GroupChild::Shape(line),
            super::super::GroupChild::Shape(trigger),
        ],
        width: unit(2000),
        height: unit(1000),
        frame: crate::frame::ObjectFrame::new(),
        placement: None,
        inst_id: None,
    };
    let plan = group_plan(&group, &HWPX_LIKE).unwrap();
    assert!(plan.is_fallback());
    assert_eq!(plan.nodes()[0].points()[1], ShapePoint { x: 999, y: 0 });
    assert_eq!(plan.loss().map(FoldLoss::kind), Some(FoldLossKind::Residual), "{plan:?}");
    // Unturned, the same rounding moves the end by a third: no loss.
    let mut line = framed_line((1499, 0), (1000, 1000), (1500, 1500));
    set_rotation_and_flip(&mut line, None, None);
    let trigger = with_offset(framed_line((100_000_000, 0), (49, 0), (100_000_000, 0)), 1200, 0);
    let group = Control::Group {
        children: vec![
            super::super::GroupChild::Shape(line),
            super::super::GroupChild::Shape(trigger),
        ],
        width: unit(2000),
        height: unit(1000),
        frame: crate::frame::ObjectFrame::new(),
        placement: None,
        inst_id: None,
    };
    let plan = group_plan(&group, &HWPX_LIKE).unwrap();
    assert!(plan.is_fallback());
    assert!(plan.loss().is_none(), "{plan:?}");
}

/// The plan keeps a frame the policy stores within 2: the r6 circle
/// (original 560, shown 850) stores its centre at 280 and the scale
/// 850 / 560, and lands exactly where Core draws it (centre at 425).
/// 이것을 실패시키는 것: 점을 현재 상자로 옮겨 쓰기 / 배율을 1 로.
#[test]
fn frame_plan_keeps_a_storable_frame() {
    let mut circle = Control::ellipse(unit(560), unit(560));
    assert!(circle.resize(unit(850), unit(850)));
    let plan = frame_plan(&circle, &HWPX_LIKE).unwrap();
    assert!(!plan.is_fallback());
    assert_eq!(plan.original_size(), (560, 560));
    assert_eq!(plan.points()[0], ShapePoint { x: 280, y: 280 });
    assert!((plan.scale().0 - 850.0 / 560.0).abs() < 1e-12);
    // corners 0..3, then centre, axis 1, axis 2
    assert!(close(plan.stored_positions()[4], (425.0, 425.0), 1e-3));
    assert!(close(plan.stored_positions()[2], (850.0, 850.0), 1e-3));
}

/// Dropping the original box keeps the drawing: the points move into the
/// displayed box. A TextArt keeps its displayed size as the original box,
/// since its default is the 14173 design box.
/// 이것을 실패시키는 것: 점을 옮기지 않기 / 글맵시도 `None` 으로 두기.
#[test]
fn drop_original_size_keeps_the_drawing() {
    let mut circle = Control::ellipse(unit(560), unit(560));
    circle.resize(unit(850), unit(1700));
    assert!(drop_original_size(&mut circle));
    let Control::Ellipse { center, axis2, frame, .. } = &circle else { unreachable!() };
    // axis 2 sits at (280, 560) in the original box: 280 × 850/560, 560 × 1700/560.
    assert_eq!((*center, *axis2), (ShapePoint { x: 425, y: 850 }, ShapePoint { x: 425, y: 1700 }));
    assert!(frame.original_size.is_none());
    assert!(!drop_original_size(&mut circle));

    let mut art = Control::TextArt {
        text: "가".into(),
        shape: "WAVE2".into(),
        font_name: String::new(),
        font_style: String::new(),
        align: "LEFT".into(),
        line_spacing: 120,
        char_spacing: 100,
        width: unit(5000),
        height: unit(3000),
        frame: crate::frame::ObjectFrame::new(),
        placement: None,
        fill_color: None,
        inst_id: None,
    };
    art.frame_mut().unwrap().original_size =
        Some(hwpforge_foundation::Size::new(unit(20_000), unit(10_000)));
    assert!(drop_original_size(&mut art));
    assert_eq!(
        art.frame().unwrap().original_size,
        Some(hwpforge_foundation::Size::new(unit(5000), unit(3000)))
    );
}

/// An original box that cannot be a frame value — a negative axis (a
/// wrapped `orgSz`) or one beyond the HWPUNIT range — is not folded here:
/// the caller reports it and folds the child without one.
#[test]
fn unusable_original_box_is_not_folded() {
    let circle = Control::ellipse(unit(200), unit(100));
    let two = ShapeTransform::from_matrix([2.0, 0.0, 0.0, 0.0, 2.0, 0.0]);
    for original in [(-1.0, 50.0), (100.0, -50.0), (1e12, 50.0)] {
        let input = FoldInput::new(two, original, (0.0, 0.0));
        assert!(fold_aligned_child(circle.clone(), &input).is_none(), "{original:?}");
    }
    let input = FoldInput::new(two, (100.0, 50.0), (0.0, 0.0));
    let (control, _) = kept(fold_aligned_child(circle, &input).unwrap());
    assert_eq!(
        control.frame().unwrap().original_size,
        Some(hwpforge_foundation::Size::new(unit(100), unit(50)))
    );
}

/// An axis-aligned map keeps the points and records the original box; a
/// turned one is not folded here; an original box equal to the displayed
/// one is no frame value.
/// 이것을 실패시키는 것: 비스듬함·돌림 판정을 지우기 / 기본값 접기를 지우기 /
/// 상자형 0 축을 받아들이기.
#[test]
fn aligned_fold_refuses_a_turn() {
    let circle = Control::ellipse(unit(560), unit(560));
    let input =
        |m: [f64; 6]| FoldInput::new(ShapeTransform::from_matrix(m), (560.0, 560.0), (0.0, 0.0));
    let turned = [0.0, -1.0, 560.0, 1.0, 0.0, 0.0];
    assert!(fold_aligned_child(circle.clone(), &input(turned)).is_none());
    let sheared = [1.0, 0.5, 0.0, 0.0, 1.0, 0.0];
    assert!(fold_aligned_child(circle.clone(), &input(sheared)).is_none());
    let Some(FoldOutcome::Kept { control, losses }) =
        fold_aligned_child(circle.clone(), &input([1.0, 0.0, 30.0, 0.0, 1.0, 20.0]))
    else {
        panic!()
    };
    assert!(losses.is_empty());
    assert!(control.frame().unwrap().is_unset(), "unscaled: no frame value");
    // A box whose original box is flat along an axis would be shown flat.
    let flat = FoldInput::new(
        ShapeTransform::from_matrix([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]),
        (0.0, 560.0),
        (0.0, 0.0),
    );
    assert!(fold_aligned_child(circle, &flat).is_none(), "a box may not be flat");
}

/// 이것을 실패시키는 것: 독립 개체에 set_box/group placement/점 접기를 적용.
#[test]
fn standalone_keeps_points_placement_and_non_geometry_style() {
    let mut c = Control::polygon(vec![
        ShapePoint::new(10, 10),
        ShapePoint::new(50, 10),
        ShapePoint::new(10, 25),
    ])
    .unwrap();
    if let Control::Polygon { placement, style, .. } = &mut c {
        *placement = Some(ObjectPlacement::legacy_inline_defaults());
        *style = Some(ShapeStyle { line_width: Some(33), ..Default::default() });
    }
    let points = defining_points(&c);
    let map = ShapeTransform::from_matrix([0., -3., 3000., 2., 0., -3000.]);
    fold_standalone(&mut c, &FoldInput::new(map, (100., 50.), (0., 0.))).unwrap();
    assert_eq!(defining_points(&c), points);
    let Control::Polygon { width, height, placement, style, frame, .. } = c else { panic!() };
    assert_eq!((width.as_i32(), height.as_i32()), (200, 150));
    assert_eq!(placement, Some(ObjectPlacement::legacy_inline_defaults()));
    assert_eq!(style.as_ref().unwrap().line_width, Some(33));
    assert_eq!(style.unwrap().rotation, Some(90.));
    assert_eq!(frame.original_size.unwrap().width.as_i32(), 100);
}

/// 이것을 실패시키는 것: 작은 비영 배율을 epsilon으로 0 처리 / curSz에서 0축 복원.
#[test]
fn standalone_line_zero_columns_and_tiny_nonzero_scale() {
    let mut line = Control::line(ShapePoint::new(0, 0), ShapePoint::new(100, 0)).unwrap();
    fold_standalone(
        &mut line,
        &FoldInput::new(
            ShapeTransform::from_matrix([66.483778, 0., 0., 38.385, 0., 0.]),
            (100., 100.),
            (0., 0.),
        ),
    )
    .unwrap();
    let b = shape_box(&line).unwrap();
    assert_eq!((b.width, b.height), (7677, 0));
    assert!((rotation_and_flip(&line).0.unwrap() - 30.).abs() < 0.001);
    let mut c = rect(1, 1, None, None);
    let loss = fold_standalone(
        &mut c,
        &FoldInput::new(
            ShapeTransform::from_matrix([0., -1e-8, 0., 1e-8, 0., 0.]),
            (100_000_000., 100_000_000.),
            (0., 0.),
        ),
    );
    assert!(loss.is_ok(), "{loss:?}");
    assert_eq!(shape_box(&c).unwrap().width, 1);
}

/// 이것을 실패시키는 것: 점 도형 전단 우회 / 포화 캐스트 / 오류 전 부분 변경.
#[test]
fn standalone_errors_are_atomic_and_specific() {
    for (m, o, kind) in [
        ([1., 0.5, 0., 0., 1., 0.], (100., 100.), FoldLossKind::Shear),
        ([0., 0., 0., 0., 0., 0.], (100., 100.), FoldLossKind::Degenerate),
        ([0., -2., 0., 2., 0., 0.], (100_000_000., 100.), FoldLossKind::OutOfRange),
        ([0., -1., 0., 1., 0., 0.], (-1., 100.), FoldLossKind::OutOfRange),
        ([f64::NAN, 0., 0., 0., 1., 0.], (100., 100.), FoldLossKind::Degenerate),
    ] {
        let mut c = rect(100, 100, None, None);
        let before = c.clone();
        let loss =
            fold_standalone(&mut c, &FoldInput::new(ShapeTransform::from_matrix(m), o, (0., 0.)))
                .unwrap_err();
        assert_eq!(loss.kind(), kind);
        // 이것을 실패시키는 것: 실패 축/O/계산 D를 범용 out_of_range 문구로 숨기기.
        if kind == FoldLossKind::OutOfRange {
            assert!(loss.detail().contains("x:") && loss.detail().contains("O="), "{loss}");
            if o.0 > 0.0 {
                assert!(loss.detail().contains("D=200000000"), "{loss}");
            }
        }
        assert_eq!(c, before);
    }
}

/// 이것을 실패시키는 것: 음수 배율을 절댓값으로만 저장 / f32 360 정규화 누락.
#[test]
fn standalone_reflection_uses_matrix_and_preserves_direction() {
    let mut c = rect(1, 1, None, None);
    fold_standalone(
        &mut c,
        &FoldInput::new(
            ShapeTransform::from_matrix([0.866025, 0.5, 0., 0.5, -0.866025, 0.]),
            (100., 50.),
            (0., 0.),
        ),
    )
    .unwrap();
    let (r, f) = rotation_and_flip(&c);
    assert!((r.unwrap() - 210.).abs() < 0.001);
    assert_eq!(f, Some(Flip::Horizontal));
    fold_standalone(
        &mut c,
        &FoldInput::new(
            ShapeTransform::from_matrix([1., 1e-10, 0., -1e-10, 1., 0.]),
            (100., 50.),
            (0., 0.),
        ),
    )
    .unwrap();
    assert_eq!(rotation_and_flip(&c), (None, None));
}

/// 이것을 실패시키는 것: 정수 중심 대신 정확 중심 / 경계 대신 폭 반올림 / Q 양자화 생략.
#[test]
fn standalone_layout_has_one_fixed_hwpx_rounding_rule() {
    for (size, angle, expected_size, translation) in [
        ((6217, 9325), 90., (9325, 6217), (1555, -1554)),
        ((10329, 8619), 90., (8619, 10329), (-854, 855)),
        ((8881, 32), 90., (32, 8881), (-4424, 4424)),
        ((5790, 12766), 30., (11398, 13950), (2804, 592)),
        ((9168, 9338), 30., (12608, 12670), (1720, 1666)),
        ((7677, 0), 30., (6649, 3839), (-514, 1919)),
    ] {
        let (_, b, t) = standalone_layout(size, Some(angle), None);
        assert_eq!(b, expected_size);
        assert_eq!(t, translation);
    }
}

/// 이것을 실패시키는 것: 그림 fold에서 실패 전 width/height/frame 변경.
#[test]
fn standalone_picture_errors_leave_every_field_intact() {
    let u = |v| HwpUnit::new(v).unwrap();
    let mut image = crate::image::Image::from_path("x.png", u(100), u(200));
    image.rotation = Some(45.);
    image.flip = Some(Flip::Vertical);
    for (map, o) in [
        ([1., 0.5, 0., 0., 1., 0.], (100., 200.)),
        ([0., -1., 0., 1., 0., 0.], (200_000_000., 200.)),
        ([f64::INFINITY, 0., 0., 0., 1., 0.], (100., 200.)),
    ] {
        let before = image.clone();
        assert!(fold_standalone_picture(
            &mut image,
            &FoldInput::new(ShapeTransform::from_matrix(map), o, (0., 0.))
        )
        .is_err());
        assert_eq!(image, before);
    }
}

/// 이것을 실패시키는 것: AxisEndpoints의 미사용 sweep 네 점을 정의점으로 변환하기.
#[test]
fn arc_definition_selects_active_points_and_defaults_for_old_json() {
    use crate::control::ArcDefinition;
    let mut arc = Control::arc(hwpforge_foundation::ArcType::Pie, unit(100), unit(80));
    assert_eq!(defining_points(&arc).len(), 7);
    let mut old = serde_json::to_value(&arc).unwrap();
    old["Arc"].as_object_mut().unwrap().remove("definition");
    assert_eq!(serde_json::from_value::<Control>(old).unwrap(), arc);
    let unused = ShapePoint::new(123456, -654321);
    if let Control::Arc { definition, start1, end1, start2, end2, .. } = &mut arc {
        *definition = ArcDefinition::AxisEndpoints;
        *start1 = unused;
        *end1 = unused;
        *start2 = unused;
        *end2 = unused;
    }
    assert_eq!(defining_points(&arc).len(), 3);
    for p in defining_points_mut(&mut arc) {
        p.x += 3;
    }
    let Control::Arc { start1, end1, start2, end2, .. } = arc else { panic!() };
    assert_eq!([start1, end1, start2, end2], [unused; 4]);
}

fn image_filled(mut control: Control, mode: hwpforge_foundation::ImageFillMode) -> Control {
    style_mut(&mut control).unwrap().get_or_insert_with(ShapeStyle::default).fill =
        Some(Fill::Image { image_id: "BinData/brush".into(), mode });
    control
}

fn image_group(children: Vec<Control>) -> Control {
    Control::Group {
        children: children.into_iter().map(super::super::GroupChild::Shape).collect(),
        width: unit(5000),
        height: unit(5000),
        frame: Default::default(),
        placement: None,
        inst_id: None,
    }
}

/// 이것을 실패시키는 것: 원래 정의점 접기, 점형 도형의 box 경로 우회,
/// TOTAL만 보존, 회전/대칭에서 O 배율을 두 번 적용.
#[test]
fn image_fill_fold_keeps_original_points_for_all_shapes_and_modes() {
    use hwpforge_foundation::{ArcType, ImageFillMode};
    let points = vec![ShapePoint::new(10, 10), ShapePoint::new(100, 10), ShapePoint::new(20, 50)];
    let shapes = vec![
        rect(100, 50, None, None),
        Control::ellipse(unit(100), unit(50)),
        Control::arc(ArcType::Pie, unit(100), unit(50)),
        Control::polygon(points.clone()).unwrap(),
        Control::curve(points).unwrap(),
        Control::text_box(Vec::new(), unit(100), unit(50)),
    ];
    for shape in shapes {
        for mode in [
            ImageFillMode::Stretch,
            ImageFillMode::Tile,
            ImageFillMode::Center,
            ImageFillMode::FitAll,
        ] {
            for angle in [0.0, 30.0, 180.0] {
                for flip in [None, Some(Flip::Horizontal), Some(Flip::Vertical), Some(Flip::Both)] {
                    let shape = image_filled(shape.clone(), mode);
                    let points = defining_points(&shape);
                    let target = with_offset(rect(200, 150, Some(angle), flip), 700, 900);
                    let map = local_transform(&target, &EmissionPolicy::exact())
                        .unwrap()
                        .then(ShapeTransform::from_matrix([2., 0., 0., 0., 3., 0.]));
                    let input = FoldInput::new(map, (100., 50.), (0., 0.));
                    let (got, losses) = kept(fold_group_child_with_original_box(shape, &input));
                    assert!(losses.is_empty(), "{} {angle} {flip:?}: {losses:?}", got.kind_name());
                    assert_eq!(defining_points(&got), points);
                    let plan = group_child_frame_plan(&got, &HWPX_LIKE).unwrap();
                    assert_eq!(plan.original_size(), (100, 50));
                    assert_eq!(plan.scale(), (2., 3.));
                    let b = shape_box(&got).unwrap();
                    for (p, actual) in box_corners(100., 50.)
                        .into_iter()
                        .chain(points.iter().map(|p| (f64::from(p.x), f64::from(p.y))))
                        .zip(plan.stored_positions())
                    {
                        assert!(close(
                            map.apply(p),
                            (actual.0 + f64::from(b.offset.0), actual.1 + f64::from(b.offset.1)),
                            0.01
                        ));
                    }
                }
            }
        }
    }
}

/// 이것을 실패시키는 것: 깊이 1만 보정, 비그림 형제까지 단위 쌍 추가,
/// 조상 배율 역보정 누락 또는 중첩 origin 누락.
#[test]
fn image_fill_nested_pairs_keep_geometry_and_scope() {
    use hwpforge_foundation::{ImageFillMode, Size};
    for scaled in [false, true] {
        let mut leaf =
            image_filled(rect(100, 50, Some(30.), Some(Flip::Horizontal)), ImageFillMode::Tile);
        leaf.resize(unit(200), unit(150));
        leaf = with_offset(leaf, 70, 90);
        let plain = rect(100, 50, None, None);
        let mut nested = image_group(vec![leaf.clone(), plain]);
        if let Control::Group { placement, .. } = &mut nested {
            *placement = group_child_placement(200, 300);
        }
        if scaled {
            nested.frame_mut().unwrap().original_size = Some(Size::new(unit(2500), unit(1250)));
        }
        let mut outer = image_group(vec![nested]);
        if scaled {
            outer.frame_mut().unwrap().original_size = Some(Size::new(unit(1250), unit(2500)));
        }
        let plan = group_plan(&outer, &HWPX_LIKE).unwrap();
        assert!(!plan.is_fallback() && plan.loss().is_none(), "{plan:?}");
        let (preserved, loss) = preserve_group_child(&leaf, &plan.nodes()[1], 2, &HWPX_LIKE);
        assert!(loss.is_none(), "{loss:?}");
        assert_eq!(preserved.pairs().len(), 3);
        assert_eq!(plan.nodes()[2].pairs().len(), if scaled { 3 } else { 1 });
        let expected = emitted_corners(&leaf, &EmissionPolicy::exact()).unwrap();
        for (a, e) in preserved.stored_positions().iter().zip(expected) {
            assert!(
                close(*a, (e.0 + 200., e.1 + 300.), 0.01),
                "scaled={scaled}, actual={a:?}, expected={e:?}, node={:?}",
                plan.nodes()[1]
            );
        }
    }
}

/// 이것을 실패시키는 것: 보존 후보 손실을 그룹에 넣거나 거부된 후보를 채택,
/// 실패 잔차를 무경고로 통과, 독립 개체 fallback까지 변경.
#[test]
fn image_fill_failed_preservation_folds_only_the_child() {
    use hwpforge_foundation::{ImageFillMode, Size};
    let mut brush = image_filled(rect(49, 100, None, None), ImageFillMode::Stretch);
    brush.frame_mut().unwrap().original_size = Some(Size::new(unit(100_000_000), unit(100)));
    assert!(frame_plan(&brush, &HWPX_LIKE).unwrap().is_fallback(), "standalone unchanged");
    let child = group_child_frame_plan(&brush, &HWPX_LIKE).unwrap();
    assert!(!child.is_fallback());
    assert_eq!(child.original_size(), (100_000_000, 100));
    assert!(child.loss().is_none(), "only full-chain admission measures image loss");
    let mut group = image_group(vec![brush.clone()]);
    let plan = group_plan(&group, &HWPX_LIKE).unwrap();
    assert!(!plan.is_fallback());
    assert!(plan.loss().is_none());
    let (node, loss) = preserve_group_child(&brush, &plan.nodes()[0], 1, &HWPX_LIKE);
    let mut expected_node = plan.nodes()[0].clone();
    if expected_node.pairs.len() == 1 {
        expected_node.pairs.insert(0, (IDENTITY_M, IDENTITY_M));
    }
    assert_eq!(node, expected_node);
    assert_eq!(node.original_size(), (49, 100));
    assert!(loss.unwrap().detail().contains("49.000000"));
    drop_original_sizes(&mut group);
    let Control::Group { children, .. } = group else { panic!() };
    drop_original_size(&mut brush);
    assert_eq!(children, vec![super::super::GroupChild::Shape(brush)]);
}

/// 이것을 실패시키는 것: 보존 경로에서 원래 크기 범위/0축/전단 진단 생략.
#[test]
fn image_fill_fold_keeps_range_degeneracy_and_shear_diagnostics() {
    let id = ShapeTransform::IDENTITY;
    for (org, kind) in [
        ((-100., 50.), FoldLossKind::OutOfRange),
        ((0., 50.), FoldLossKind::Degenerate),
        ((200_000_000., 50.), FoldLossKind::OutOfRange),
    ] {
        let outcome = fold_group_child_with_original_box(
            rect(100, 50, None, None),
            &FoldInput::new(id, org, (0., 0.)),
        );
        let FoldOutcome::Dropped { losses } = outcome else { panic!("{outcome:?}") };
        assert_eq!(losses[0].kind(), kind);
    }
    let shear = ShapeTransform::from_matrix([1., 0.5, 0., 0., 1., 0.]);
    let (got, losses) = kept(fold_group_child_with_original_box(
        rect(100, 50, None, None),
        &FoldInput::new(shear, (100., 50.), (0., 0.)),
    ));
    assert_eq!(kinds(&losses), [FoldLossKind::Shear]);
    // 이것을 실패시키는 것: 전단 여부/정규화된 각도로 보존을 별도 판정하기.
    // The source shear is already reported by the common fold. Group decisions
    // still use the folded child, and preservation judges its exact Core geometry.
    assert_eq!(
        got.effective_original_size().unwrap(),
        hwpforge_foundation::Size::new(unit(100), unit(50))
    );
    let got = image_filled(got, hwpforge_foundation::ImageFillMode::Tile);
    let group = image_group(vec![got.clone()]);
    let plan = group_plan(&group, &HWPX_LIKE).unwrap();
    assert!(!plan.is_fallback());
    let (node, loss) = preserve_group_child(&got, &plan.nodes()[0], 1, &HWPX_LIKE);
    assert!(loss.is_none(), "{loss:?}");
    assert_eq!(node.original_size(), (100, 50));
}

/// 이것을 실패시키는 것: 배율 1인 그림 채우기 자식의 기본 frame을 명시적 O로 변경.
#[test]
fn image_fill_unscaled_original_uses_the_existing_default() {
    let original = rect(100, 50, Some(30.), Some(Flip::Horizontal));
    let map = local_transform(&original, &EmissionPolicy::exact()).unwrap();
    let (got, losses) = kept(fold_group_child_with_original_box(
        rect(100, 50, None, None),
        &FoldInput::new(map, (100., 50.), (0., 0.)),
    ));
    assert!(losses.is_empty());
    assert!(got.frame().unwrap().original_size.is_none());
    assert_eq!(got.effective_original_size(), original.effective_original_size());
}

/// 이것을 실패시키는 것: 그림 자식의 각도 손실로 비그림 형제의 O/점/배율 변경,
/// 또는 그림을 무조건 제외하여 조상 배율 보정의 추가 오차까지 무시.
#[test]
fn image_fill_local_loss_does_not_fold_siblings_but_ancestors_still_do() {
    use hwpforge_foundation::{ImageFillMode, Size};
    let mut brush = image_filled(rect(5000, 500, Some(30.4), None), ImageFillMode::Stretch);
    brush.resize(unit(10000), unit(1000));
    let mut sibling = rect(100, 50, None, None);
    sibling.resize(unit(200), unit(100));
    let mut legacy_brush = brush.clone();
    drop_original_size(&mut legacy_brush);
    let group = image_group(vec![legacy_brush, sibling.clone()]);
    let plan = group_plan(&group, &HWPX_LIKE).unwrap();
    assert!(!plan.is_fallback());
    assert!(plan.loss().is_none());
    assert!(group_child_frame_plan(&brush, &HWPX_LIKE).unwrap().loss().is_none());
    let plain = group_plan(&image_group(vec![sibling]), &HWPX_LIKE).unwrap();
    assert_eq!(plan.nodes()[1], plain.nodes()[0]);

    // Even an exactly representable ancestor scale keeps HEAD's counted
    // contract: the old folded brush's angle loss was already judged here.
    let mut ordinary_scale = group.clone();
    if let Control::Group { width, height, frame, .. } = &mut ordinary_scale {
        frame.original_size = Some(Size::new(unit(width.as_i32() / 2), unit(height.as_i32() / 2)));
    }
    assert!(group_plan(&ordinary_scale, &HWPX_LIKE).unwrap().is_fallback());

    // A large ancestor scale makes inverse compensation round to zero.
    // This is additional error, so the existing group fallback still runs.
    let mut scaled = group;
    if let Control::Group { width, height, frame, .. } = &mut scaled {
        *width = unit(100_000_000);
        *height = unit(100_000_000);
        frame.original_size = Some(Size::new(unit(1), unit(1)));
    }
    let plan = group_plan(&scaled, &HWPX_LIKE).unwrap();
    assert!(plan.is_fallback());
    assert_eq!(plan.nodes()[1].original_size(), (200, 100));
    assert_eq!(plan.nodes()[0].original_size(), (10000, 1000));
}

/// 이것을 실패시키는 것: 새 그림 O에서만 생긴 조상 보정 손실로 형제를 접기.
#[test]
fn image_fill_new_ancestor_error_does_not_request_group_fallback() {
    use hwpforge_foundation::{ImageFillMode, Size};
    let mut brush = image_filled(rect(100_000_000, 100, Some(30.), None), ImageFillMode::Stretch);
    brush.resize(unit(100), unit(100));
    assert!(group_child_frame_plan(&brush, &HWPX_LIKE).unwrap().loss().is_none());
    let mut old_brush = brush.clone();
    drop_original_size(&mut old_brush);
    let mut sibling = rect(100, 50, None, None);
    sibling.resize(unit(200), unit(100));
    let scale = |c| {
        let mut group = image_group(vec![c, sibling.clone()]);
        if let Control::Group { width, height, frame, .. } = &mut group {
            *width = unit(1000);
            *height = unit(1000);
            frame.original_size = Some(Size::new(unit(1), unit(1)));
        }
        group
    };
    let previous = group_plan(&scale(old_brush.clone()), &HWPX_LIKE).unwrap();
    assert!(!previous.is_fallback());
    let plan = group_plan(&scale(old_brush), &HWPX_LIKE).unwrap();
    assert!(!plan.is_fallback(), "preservation must not introduce a group fallback");
    assert!(plan.loss().is_none(), "the writer reports the actual chain at the child");
    assert_eq!(plan.nodes()[1], previous.nodes()[1]);
    let (node, loss) = preserve_group_child(&brush, &plan.nodes()[0], 1, &HWPX_LIKE);
    let mut expected_node = plan.nodes()[0].clone();
    if expected_node.pairs.len() == 1 {
        expected_node.pairs.insert(0, (IDENTITY_M, IDENTITY_M));
    }
    assert_eq!(node, expected_node);
    assert!(loss.is_some());
}

/// 이것을 실패시키는 것: HEAD 접기 결과 뒤의 보존 비교에서 0.9+1.9를 따로 검사.
#[test]
fn image_fill_total_error_and_normalized_turn_are_child_only() {
    use hwpforge_foundation::{ImageFillMode, Size};
    for (ow, oh, w, h, angle, scale, expected) in [
        (100_000_000, 100, 49, 100, None, 1, "49.000000"),
        (1_900_000, 32, 94, 96, Some(90.), 3, "2.800001"),
    ] {
        let mut brush = image_filled(rect(ow, oh, angle, None), ImageFillMode::Stretch);
        brush.resize(unit(w), unit(h));
        let mut sibling = rect(100, 50, None, None);
        sibling.resize(unit(200), unit(100));
        let mut legacy_brush = brush.clone();
        drop_original_size(&mut legacy_brush);
        let mut group = image_group(vec![legacy_brush, sibling]);
        if let Control::Group { width, height, frame, .. } = &mut group {
            *width = unit(3000);
            *height = unit(3000);
            frame.original_size = Some(Size::new(unit(3000 / scale), unit(3000 / scale)));
        }
        let plan = group_plan(&group, &HWPX_LIKE).unwrap();
        assert!(!plan.is_fallback());
        let (node, loss) = preserve_group_child(&brush, &plan.nodes()[0], 1, &HWPX_LIKE);
        let mut expected_node = plan.nodes()[0].clone();
        if expected_node.pairs.len() == 1 {
            expected_node.pairs.insert(0, (IDENTITY_M, IDENTITY_M));
        }
        assert_eq!(node, expected_node);
        let loss = loss.unwrap();
        assert!(loss.detail().contains(expected), "{loss:?}");
    }
}

/// 이것을 실패시키는 것: 축 정렬 그림 O를 판정에 포함해 형제까지 접기.
#[test]
fn aligned_image_uses_folded_decision_and_child_only_rejection() {
    use hwpforge_foundation::ImageFillMode;
    let mut brush = image_filled(rect(100_000_000, 100, None, None), ImageFillMode::Stretch);
    brush.resize(unit(49), unit(100));
    let mut sibling = rect(100, 50, None, None);
    sibling.resize(unit(200), unit(100));
    let mut group = image_group(vec![brush, sibling]);
    let plan = group_plan(&group, &HWPX_LIKE).unwrap();
    assert!(!plan.is_fallback());
    assert_eq!(plan.nodes()[0].original_size(), (49, 100));
    assert_eq!(plan.nodes()[1].original_size(), (100, 50));
    assert_eq!(plan.nodes()[1].pairs(), &[([2., 0., 0., 0., 2., 0.], IDENTITY_M)]);
    let losses = preserve_group_original_boxes(&mut group, &[], &HWPX_LIKE);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].0, [0]);
    assert!(losses[0].1.detail().contains("49.000000"));
}

/// 이것을 실패시키는 것: 이동을 뺀 잔차 재검사 또는 epsilon으로 경계 양쪽을 합치기.
#[test]
fn image_fill_admission_uses_one_absolute_boundary() {
    use hwpforge_foundation::ImageFillMode;
    for (shift, accepted) in [(0, false), (1000, true)] {
        let mut brush = image_filled(rect(4_000_000, 100, None, None), ImageFillMode::Stretch);
        brush.resize(unit(982), unit(100));
        if let Control::Rect { placement, .. } = &mut brush {
            let p = placement.get_or_insert_with(crate::ObjectPlacement::legacy_inline_defaults);
            p.horz_offset = unit(shift);
            p.vert_offset = unit(shift);
        }
        let group = image_group(vec![brush.clone()]);
        let plan = group_plan(&group, &HWPX_LIKE).unwrap();
        assert!(!plan.is_fallback());
        let (node, loss) = preserve_group_child(&brush, &plan.nodes()[0], 1, &HWPX_LIKE);
        assert_eq!(loss.is_none(), accepted);
        assert_eq!(node.original_size(), if accepted { (4_000_000, 100) } else { (982, 100) });
        if accepted {
            // Frozen IEEE-754 boundary: actual absolute corner 1984 minus exact 1982 = 2.
            assert_eq!(node.stored_positions()[1], (1984., 1000.));
        }
    }
}
