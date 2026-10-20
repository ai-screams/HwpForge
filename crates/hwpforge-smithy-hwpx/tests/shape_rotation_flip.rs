//! A shape's rotation and mirror are written the way Hancom writes them, and a
//! shape HWPX cannot store exactly is reported.
//!
//! `rotflip-single.hwpx` was made in Hancom (macOS): a rect A turned 30° then
//! mirrored left-right, a triangle B turned 30° then mirrored top-bottom, an
//! ellipse C mirrored left-right, a rect D mirrored left-right then turned
//! 30°, and a rect E turned 30° then mirrored both ways. Whatever order the
//! user clicked, Hancom stores the angle and flip of `R(angle)·F` — mirror
//! first, then rotate about the centre — in one `rotMatrix` (A is saved as
//! 330° + horizontal flip, D as 30° + horizontal flip).

use hwpforge_core::control::{Control, ShapeStyle};
use hwpforge_core::image::ImageStore;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, Flip, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{EncodeOptions, EncodeWarning, HwpxDecoder, HwpxEncoder, PackageReader};

fn fixture(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/shapes/");
    std::fs::read(format!("{path}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn section0(bytes: &[u8]) -> String {
    PackageReader::new(bytes).expect("package").read_section_xml(0).expect("section0.xml")
}

/// `(rotationInfo angle, flip h, flip v, rotMatrix e1..e6)` of every shape's
/// own common block, in document order.
fn wire_transforms(xml: &str) -> Vec<(String, String, String, [String; 6])> {
    let attr = |tag: &str, name: &str| -> String {
        let v = tag.split(&format!(" {name}=\"")).nth(1).expect(name);
        v[..v.find('"').unwrap()].to_string()
    };
    let mut out = Vec::new();
    for (start, _) in xml.match_indices("<hp:rotationInfo ") {
        let rest = &xml[start..];
        let ri = &rest[..rest.find("/>").unwrap()];
        let before = &xml[..start];
        let flip_at = before.rfind("<hp:flip ").unwrap();
        let flip = &xml[flip_at..flip_at + xml[flip_at..].find("/>").unwrap()];
        let rot_at = rest.find("<hc:rotMatrix ").unwrap();
        let rot = &rest[rot_at..rot_at + rest[rot_at..].find("/>").unwrap()];
        out.push((
            attr(ri, "angle"),
            attr(flip, "horizontal"),
            attr(flip, "vertical"),
            ["e1", "e2", "e3", "e4", "e5", "e6"].map(|e| attr(rot, e)),
        ));
    }
    out
}

fn shapes<S>(doc: &Document<S>) -> Vec<Control> {
    doc.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| p.runs.iter())
        .filter_map(|r| match &r.content {
            RunContent::Control(c) => Some((**c).clone()),
            _ => None,
        })
        .collect()
}

fn rotation_and_flip(c: &Control) -> (Option<f32>, Option<Flip>) {
    let style = match c {
        Control::Rect { style, .. }
        | Control::Ellipse { style, .. }
        | Control::Polygon { style, .. } => style.clone(),
        other => panic!("unexpected {other:?}"),
    };
    style.map_or((None, None), |s| (s.rotation, s.flip))
}

/// Hancom's mirrored-and-turned shapes decode to the stored angle and flip
/// and re-encode to Hancom's own matrix: the same angle and flip attributes
/// and the same linear part, digit for digit. The translation is not
/// compared: outside a group the decoder takes a shape's size from `<hp:sz>`
/// (its turned bounds, not `<hp:orgSz>`), an older limit of the standalone
/// path that moves the turning centre.
///
/// 이것을 실패시키는 것: 회전과 반사가 함께 있으면 반사 행렬만 쓰는 것(옛
/// 인코더), 반사와 회전 순서를 뒤집는 것(`F·R`), 단축 반사에 각도 부호를
/// 바꿔 쓰는 것(`R(−θ)·F`).
#[test]
fn hancom_mirror_and_rotation_reencode_to_the_hancom_matrix() {
    let bytes = fixture("rotflip-single.hwpx");
    let original = wire_transforms(&section0(&bytes));
    let decoded = HwpxDecoder::decode(&bytes).expect("decode");
    let got: Vec<_> = shapes(&decoded.document).iter().map(rotation_and_flip).collect();
    let expected = [
        (None, Some(Flip::Horizontal)),        // C ellipse
        (Some(330.0), Some(Flip::Horizontal)), // A
        (Some(150.0), Some(Flip::Horizontal)), // B
        (Some(30.0), Some(Flip::Horizontal)),  // D
        (Some(210.0), None),                   // E
    ];
    for ((angle, flip), (want_angle, want_flip)) in got.iter().zip(expected) {
        assert_eq!(*flip, want_flip);
        assert!((angle.unwrap_or(0.0) - want_angle.unwrap_or(0.0)).abs() < 0.001);
    }

    let validated = decoded.document.validate().expect("validate");
    let outcome = HwpxEncoder::encode_with_diagnostics(
        &validated,
        &decoded.style_store,
        &decoded.image_store,
        EncodeOptions::default(),
    )
    .expect("encode");
    assert!(outcome.warnings.is_empty(), "{:?}", outcome.warnings);
    let ours = wire_transforms(&section0(&outcome.bytes));
    assert_eq!(ours.len(), original.len());
    for (o, h) in ours.iter().zip(&original) {
        // 이것을 실패시키는 것: 메타데이터를 그대로 복사하거나 대칭 이중 적용.
        for i in [0, 1, 3, 4] {
            let (a, b): (f64, f64) = (o.3[i].parse().unwrap(), h.3[i].parse().unwrap());
            assert!((a - b).abs() < 1e-6, "e{}: ours {o:?} hancom {h:?}", i + 1);
        }
    }
}

/// `(orgSz width, height)` of every shape's common block, in document order.
fn wire_org_sizes(xml: &str) -> Vec<(i32, i32)> {
    xml.match_indices("<hp:orgSz ")
        .map(|(start, _)| {
            let tag = &xml[start..start + xml[start..].find("/>").unwrap()];
            let num = |name: &str| -> i32 {
                let v = tag.split(&format!(" {name}=\"")).nth(1).unwrap();
                v[..v.find('"').unwrap()].parse().unwrap()
            };
            (num("width"), num("height"))
        })
        .collect()
}

/// Each of Hancom's mirrored-and-turned shapes, given to the writer at its
/// own local size (`orgSz`), angle and flip: all six `rotMatrix`
/// coefficients match Hancom's — the linear part digit for digit, the
/// translation within 1.2 (Hancom turns about the centre rounded down to a
/// whole HWPUNIT, Core about the exact centre) — and so does every corner.
///
/// 이것을 실패시키는 것: 이동 성분을 원점 기준 회전으로 계산하는 것, 반사·회전
/// 순서를 뒤집는 것(`F·R`), 단축 반사에 각도 부호를 바꾸는 것.
#[test]
fn writer_matrix_matches_hancom_at_the_native_size() {
    let xml = section0(&fixture("rotflip-single.hwpx"));
    let hancom = wire_transforms(&xml);
    let sizes = wire_org_sizes(&xml);
    assert_eq!(hancom.len(), sizes.len());
    for (h, &(w, ht)) in hancom.iter().zip(&sizes) {
        let flip = match (h.1.as_str(), h.2.as_str()) {
            ("1", "1") => Some(Flip::Both),
            ("1", _) => Some(Flip::Horizontal),
            (_, "1") => Some(Flip::Vertical),
            _ => None,
        };
        let angle: f32 = h.0.parse().unwrap();
        let (ours_xml, warnings) = encode_one(rect(w, ht, angle, flip));
        assert!(warnings.is_empty(), "{warnings:?}");
        let ours = &wire_transforms(&ours_xml)[0];
        let num = |m: &[String; 6]| m.clone().map(|v| v.parse::<f64>().unwrap());
        let (o, n) = (num(&ours.3), num(&h.3));
        for i in [0, 1, 3, 4] {
            assert!((o[i] - n[i]).abs() < 1e-6, "e{}: ours {o:?} hancom {n:?}", i + 1);
        }
        for i in [2, 5] {
            assert!((o[i] - n[i]).abs() <= 1.2, "e{}: ours {o:?} hancom {n:?}", i + 1);
        }
        let map = |m: &[f64; 6], (x, y): (f64, f64)| {
            (m[0] * x + m[1] * y + m[2], m[3] * x + m[4] * y + m[5])
        };
        let (fw, fh) = (f64::from(w), f64::from(ht));
        for p in [(0.0, 0.0), (fw, 0.0), (fw, fh), (0.0, fh)] {
            let (a, b) = (map(&o, p), map(&n, p));
            assert!((a.0 - b.0).hypot(a.1 - b.1) <= 2.0, "corner {p:?}: {a:?} vs {b:?}");
        }
    }
}

fn styles() -> HwpxStyleStore {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    styles
}

fn rect(w: i32, h: i32, rotation: f32, flip: Option<Flip>) -> Control {
    let mut c = Control::rect(HwpUnit::new(w).unwrap(), HwpUnit::new(h).unwrap()).unwrap();
    if let Control::Rect { style, .. } = &mut c {
        *style = Some(ShapeStyle { rotation: Some(rotation), flip, ..ShapeStyle::default() });
    }
    c
}

fn encode_one(shape: Control) -> (String, Vec<EncodeWarning>) {
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::control(shape, CharShapeIndex::new(0)));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![host], PageSettings::a4()));
    let outcome = HwpxEncoder::encode_with_diagnostics(
        &doc.validate().expect("validate"),
        &styles(),
        &ImageStore::new(),
        EncodeOptions::default(),
    )
    .expect("encode");
    (section0(&outcome.bytes), outcome.warnings)
}

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
