#[path = "support/begin_num.rs"]
mod shared;
use shared::*;

use hwpforge_core::image::ImageStore;
use hwpforge_core::run::Run;
use hwpforge_core::section::{BeginNum, Section};
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

/// A document built through the API or `from-json`, whose first
/// section continues (`page 0`), gets Hancom's header of ones — the
/// generated files used to start at page 0.
///
/// 이것을 실패시키는 것: header 를 다시 첫 구역 `begin_num` 에서 만드는 것.
#[test]
fn a_generated_document_gets_a_header_of_ones() {
    let mut section = Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("본문", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    );
    section.begin_num =
        Some(BeginNum { page: 0, pic: 0, tbl: 0, equation: 0, ..BeginNum::default() });
    let mut doc = Document::new();
    doc.add_section(section);
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    let out = HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
        .expect("encode");
    assert_eq!(header_numbers(&out), ones());
    assert_eq!(start_numbers(&out), ["0", "0", "0", "0"]);
}
