#[path = "support/header_footer_anchor.rs"]
mod shared;
use shared::*;

use hwpforge_core::image::ImageStore;
use hwpforge_core::run::Run;
use hwpforge_core::section::{HeaderFooter, Section};
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

fn api_doc(anchor: Option<usize>) -> Vec<u8> {
    let para = |t: &str| {
        Paragraph::with_runs(vec![Run::text(t, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
    };
    let mut section = Section::with_paragraphs(vec![para("one"), para("two")], PageSettings::a4());
    let mut header = HeaderFooter::all_pages(vec![para("head")]);
    header.anchor_paragraph = anchor;
    section.headers.push(header);
    let mut doc = Document::new();
    doc.add_section(section);
    // 이것을 실패시키는 것: 합성 fixture에서 참조하는 원본 글꼴 정의 누락.
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬바탕");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
        .expect("encode")
}

// F-T12: 앵커 없는 API 문서는 첫 문단에 쓰이고, `Some(0)`(JSON 으로 올 수
// 있음)도 같은 뜻이다. 이것을 실패시키는 것: `None` 을 0 이 아닌 문단으로
// 처리 / `Some(0)` 에 자리표시 run 을 만들기.
#[test]
fn unanchored_header_is_written_into_the_first_paragraph() {
    let none = section_xml(&api_doc(None));
    let zero = section_xml(&api_doc(Some(0)));
    let strip_ids = |x: &str| {
        let re_free: String =
            x.split(" id=\"").map(|s| s.split_once('"').map_or(s, |(_, r)| r)).collect();
        re_free
    };
    assert_eq!(paragraphs_with(&none, "<hp:header "), [0]);
    assert!(
        none.find("<hp:colPr").unwrap() < none.find("<hp:header ").unwrap(),
        "paragraph 0 headers follow the column control, as before"
    );
    assert_eq!(strip_ids(&none), strip_ids(&zero));
    // The bytes the encoder wrote before anchors existed (P13, `ccffa8bc`,
    // the same document): an unanchored header changes nothing.
    // 이것을 실패시키는 것: 두 경로를 함께 바꾸는 직렬화 회귀(예: 앵커 없는
    // 머리말에도 자리표시 run).
    // (The file ends with the newline the repo's end-of-file rule adds.)
    let before = include_str!("fixtures/p13_header_section0.xml").trim_end();
    assert_eq!(strip_ids(&none), strip_ids(before));
    let one = section_xml(&api_doc(Some(1)));
    assert_eq!(paragraphs_with(&one, "<hp:header "), [1]);
}
