#[path = "support/object_placement.rs"]
mod shared;
use shared::*;

use std::io::{Cursor, Read};

use hwpforge_core::image::ImageStore;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

/// A text box (a rectangle with text) writes its corner ratio too.
/// 이것을 실패시키는 것: 글상자 인코더의 `ratio: 0` 복귀.
#[test]
fn text_box_corner_ratio_is_written() {
    use hwpforge_core::control::{Control, ShapeStyle};
    let para = |t: &str| {
        Paragraph::with_runs(vec![Run::text(t, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
    };
    let mut tb = Control::text_box(
        vec![para("box")],
        HwpUnit::new(8000).unwrap(),
        HwpUnit::new(3000).unwrap(),
    );
    if let Control::TextBox { style, .. } = &mut tb {
        *style = Some(ShapeStyle { corner_ratio: Some(20), ..Default::default() });
    }
    let host = Paragraph::with_runs(
        vec![Run::control(tb, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![host], PageSettings::a4()));
    // 이것을 실패시키는 것: 합성 fixture에서 참조하는 원본 글꼴 정의 누락.
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬바탕");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    let bytes =
        HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
            .expect("encode");
    let (start, _) = element_and_pos(&section_xml(&bytes), "hp:rect");
    assert_eq!(attr(&start, "ratio"), "20");
}
