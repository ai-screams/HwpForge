#[path = "support/shape_fill_x6.rs"]
mod shared;
use shared::*;

use hwpforge_core::control::{Control, Fill, ShapeStyle};
use hwpforge_core::image::ImageStore;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, Color, GradientType, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

fn gradient_doc(gradient_type: GradientType) -> Vec<u8> {
    let style = ShapeStyle {
        fill: Some(Fill::Gradient {
            gradient_type,
            angle: 0,
            colors: vec![(Color::from_rgb(255, 0, 0), 0), (Color::from_rgb(0, 0, 255), 100)],
            center_x: None,
            center_y: None,
            step: None,
            step_center: None,
        }),
        ..ShapeStyle::default()
    };
    let rect = Control::Rect {
        frame: hwpforge_core::ObjectFrame::new(),
        width: HwpUnit::new(1000).unwrap(),
        height: HwpUnit::new(1000).unwrap(),
        placement: None,
        caption: None,
        style: Some(style),
    };
    let para = Paragraph::with_runs(
        vec![Run::control(rect, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    // 이것을 실패시키는 것: 합성 fixture에서 참조하는 원본 글꼴 정의 누락.
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬바탕");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
        .expect("encode")
}

/// `None` = what the encoder always wrote: LINEAR (0, 0), the other
/// kinds (50, 50), 255 steps, step centre 50 — old JSON and API gradients
/// keep their bytes. 이것을 실패시키는 것: `None` 을 0 으로 쓰기(RADIAL 이 0,0).
#[test]
fn gradient_without_center_keeps_the_per_kind_default() {
    let attrs = |bytes: Vec<u8>| {
        let xml = section_xml(&bytes);
        let at = xml.find("<hc:gradation ").expect("gradation");
        xml[at..at + xml[at..].find('>').unwrap()].to_string()
    };
    let radial = attrs(gradient_doc(GradientType::Radial));
    assert!(
        radial.contains(r#"centerX="50" centerY="50" step="255""#)
            && radial.contains(r#"stepCenter="50""#),
        "{radial}"
    );
    let linear = attrs(gradient_doc(GradientType::Linear));
    assert!(linear.contains(r#"centerX="0" centerY="0""#), "{linear}");
}
