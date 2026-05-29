//! Minimal single-object GSO shape fixtures for Phase 12a (HWP5 shape carry).
//!
//! Each generated HWPX holds exactly ONE drawing object (ellipse, arc, or
//! curve) on a single page, with one short label paragraph above it. The
//! point is to keep the file tiny so it can be opened in 한컴 Office, re-saved
//! as `.hwp` (HWP5), and its `gso ` shape sub-record hex-inspected without any
//! surrounding noise.
//!
//! These are *staging* inputs only. The authoritative fixtures committed under
//! `tests/fixtures/user_samples/` are the 한컴-produced `.hwp`/`.hwpx` pair, not
//! the files this generator emits.
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example gso_shape_fixtures
//!
//! Output:
//!   examples/hwp5_review/sample-gso-ellipse.hwpx
//!   examples/hwp5_review/sample-gso-arc.hwpx
//!   examples/hwp5_review/sample-gso-curve.hwpx

use std::path::Path;

use hwpforge_core::control::{Control, ShapePoint};
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    Alignment, ArcType, CharShapeIndex, CurveSegmentType, HwpUnit, ParaShapeIndex,
};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

const OUT_DIR: &str = "examples/hwp5_review";

/// Style store with a single char shape (index 0) and a single centered
/// paragraph shape (index 0) — the minimum the encoder needs.
fn store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
    store.push_char_shape(HwpxCharShape::default());
    let mut centered = HwpxParaShape::default();
    centered.alignment = Alignment::Center;
    store.push_para_shape(centered);
    store
}

fn label(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn shape_para(control: Control) -> Paragraph {
    Paragraph::with_runs(
        vec![Run::control(control, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )
}

fn save(name: &str, label_text: &str, control: Control) {
    let paras = vec![label(label_text), shape_para(control)];
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paras, PageSettings::a4()));
    let validated = doc.validate().expect("fixture document should validate");
    let path = Path::new(OUT_DIR).join(name);
    HwpxEncoder::encode_file(&path, &validated, &store(), &ImageStore::new())
        .expect("fixture HWPX should encode");
    println!("wrote {}", path.display());
}

fn main() {
    std::fs::create_dir_all(OUT_DIR).expect("create staging output dir");

    let w = HwpUnit::from_mm(50.0).unwrap();
    let h = HwpUnit::from_mm(30.0).unwrap();
    save("sample-gso-ellipse.hwpx", "GSO 타원(Ellipse) 단일 객체", Control::ellipse(w, h));

    let arc_w = HwpUnit::from_mm(40.0).unwrap();
    let arc_h = HwpUnit::from_mm(30.0).unwrap();
    save(
        "sample-gso-arc.hwpx",
        "GSO 호(Arc, Normal) 단일 객체",
        Control::arc(ArcType::Normal, arc_w, arc_h),
    );

    let mut curve = Control::curve(vec![
        ShapePoint::new(0, 5000),
        ShapePoint::new(3000, 0),
        ShapePoint::new(6000, 10000),
        ShapePoint::new(9000, 5000),
    ])
    .expect("4-point bezier curve is valid");
    if let Control::Curve { ref mut segment_types, .. } = curve {
        *segment_types =
            vec![CurveSegmentType::Curve, CurveSegmentType::Curve, CurveSegmentType::Curve];
    }
    save("sample-gso-curve.hwpx", "GSO 곡선(Curve, 베지어) 단일 객체", curve);
}
