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
