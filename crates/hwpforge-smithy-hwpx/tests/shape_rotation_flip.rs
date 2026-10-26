#[path = "support/shape_rotation_flip.rs"]
mod shared;
use shared::*;

use hwpforge_core::control::{Control, ShapeStyle};
use hwpforge_core::image::ImageStore;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, Flip, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{EncodeOptions, EncodeWarning, HwpxEncoder, PackageReader};

fn shape_emission(warnings: &[EncodeWarning]) -> Vec<String> {
    warnings
        .iter()
        .filter_map(|w| match w {
            EncodeWarning::ContentLost { kind, reason, part, .. } if kind == "shape_emission" => {
                assert_eq!(part, "Contents/section0.xml");
                Some(reason.clone())
            }
            _ => None,
        })
        .collect()
}

/// Whole degrees cost a 10,000 × 1,000 box turned 30.4° up to 35.08 HWPUNIT:
/// reported. A 100 × 100 box at 30.4° moves 0.49: not reported. Six-digit
/// coefficients alone move a 10,000,000-wide box at a whole 30° by 4.04:
/// reported.
///
/// 이것을 실패시키는 것: 인코더의 방출 검사(`report_shape_emission`)를 지우는
/// 것, 정수 아닌 각이면 무조건 경고하는 것, 정책(각도·계수 양자화)을 무시하고
/// 원시 값으로 잔차를 재는 것.
#[test]
fn shapes_hwpx_cannot_store_exactly_are_reported() {
    let (_, w) = encode_one(rect(10_000, 1_000, 30.4, None));
    assert_eq!(shape_emission(&w).len(), 1, "{w:?}");
    assert!(shape_emission(&w)[0].contains("35.08"), "{w:?}");

    let (_, w) = encode_one(rect(100, 100, 30.4, None));
    assert!(shape_emission(&w).is_empty(), "{w:?}");

    let (_, w) = encode_one(rect(10_000_000, 1_000, 30.0, None));
    assert_eq!(shape_emission(&w).len(), 1, "{w:?}");
    assert!(shape_emission(&w)[0].contains("4.04"), "{w:?}");
}

/// Mirrored both ways and turned 37°: Hancom's form — angle 37, both flips,
/// and the single matrix `R(37°)·(−I)`, which is a 217° turn. No warning.
///
/// 이것을 실패시키는 것: 양축 반사에 각도 부호 변환을 적용하는 것
/// (`R(−37°)·(−I)`), 또는 회전과 함께 있는 반사를 반사만 쓰는 것.
#[test]
fn both_mirrors_with_a_rotation_write_one_half_turn_matrix() {
    let (xml, w) = encode_one(rect(1000, 500, 37.0, Some(Flip::Both)));
    assert!(w.is_empty(), "{w:?}");
    let t = &wire_transforms(&xml)[0];
    assert_eq!((t.0.as_str(), t.1.as_str(), t.2.as_str()), ("37", "1", "1"));
    let (s, c) = 217f64.to_radians().sin_cos();
    let quantize = |x: f64| format!("{x:.6}").parse::<f64>().unwrap();
    let (s, c) = (quantize(s), quantize(c));
    let want = [c, -s, 500.0 - (c * 500.0 - s * 250.0), s, c, 250.0 - (s * 500.0 + c * 250.0)];
    for (got, want) in t.3.iter().zip(want) {
        let got: f64 = got.parse().unwrap();
        assert!((got - want).abs() < 1e-5, "{t:?}");
    }
}

/// Whole degrees wrap first, then round half away from zero.
///
/// 이것을 실패시키는 것: `rem_euclid` 를 빼는 것, 반올림 방식을 바꾸는 것.
#[test]
fn rotation_angle_boundaries_match_the_writer() {
    for (given, stored) in
        [(29.5, "30"), (30.5, "31"), (-30.0, "330"), (359.6, "360"), (-0.4, "360")]
    {
        let (xml, _) = encode_one(rect(1000, 500, given, None));
        assert_eq!(wire_transforms(&xml)[0].0, stored, "{given}");
    }
}
