#[path = "support/text_margin.rs"]
mod shared;
use shared::*;

use hwpforge_core::control::Control;
use hwpforge_smithy_hwpx::{DecodeWarning, HwpxDecoder, HwpxEncoder};

/// A polygon's text margin other than 283 is reported too: a polygon with
/// text, encoded, then given a margin on the wire.
///
/// 이것을 실패시키는 것: 다각형 쪽 보고를 지우기.
#[test]
fn polygon_text_margin_is_reported() {
    use hwpforge_core::control::ShapePoint;
    use hwpforge_core::{Document, PageSettings, Paragraph, Run, Section};
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    use hwpforge_smithy_hwpx::style_store::HwpxStyleStore;
    let mut polygon = Control::polygon(vec![
        ShapePoint::new(0, 0),
        ShapePoint::new(4000, 0),
        ShapePoint::new(2000, 3000),
    ])
    .expect("polygon");
    if let Control::Polygon { paragraphs, .. } = &mut polygon {
        *paragraphs = vec![Paragraph::with_runs(
            vec![Run::text("다각형", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )];
    }
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::control(polygon, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let bytes = HwpxEncoder::encode(
        &doc.validate().expect("validate"),
        &{
            // 이것을 실패시키는 것: 합성 fixture의 원본 charPr/paraPr 0 정의 누락.
            let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
            store.push_char_shape(hwpforge_smithy_hwpx::HwpxCharShape::default());
            store.push_para_shape(hwpforge_smithy_hwpx::HwpxParaShape::default());
            store
        },
        &Default::default(),
    )
    .expect("encode");
    let xml = section_xml(&bytes);
    let at = xml.find("<hp:polygon ").expect("polygon");
    let end = at + xml[at..].find("</hp:subList>").expect("sublist") + "</hp:subList>".len();
    let with =
        |margin: &str| with_section(&bytes, &format!("{}{}{}", &xml[..end], margin, &xml[end..]));
    let reported = |bytes: &[u8]| {
        HwpxDecoder::decode(bytes).expect("decode").warnings.iter().any(|w| {
            matches!(
                w,
                DecodeWarning::UnknownEnumValue { attribute: "hp:drawText/hp:textMargin", .. }
            )
        })
    };
    assert!(reported(&with(ODD)));
    assert!(!reported(&with(DEFAULT)));
}

/// A text box built without a margin (old JSON, the API's constructor)
/// writes 283 on every side, as before the field existed.
///
/// 이것을 실패시키는 것: `None` 을 0 으로 쓰기.
#[test]
fn text_box_without_a_margin_writes_283() {
    use hwpforge_core::{Document, PageSettings, Paragraph, Run, Section};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
    use hwpforge_smithy_hwpx::style_store::HwpxStyleStore;
    let unit = HwpUnit::new(3000).unwrap();
    let inner =
        Paragraph::with_runs(vec![Run::text("글", CharShapeIndex::new(0))], ParaShapeIndex::new(0));
    let tb = Control::text_box(vec![inner], unit, unit);
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::control(tb, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let bytes = HwpxEncoder::encode(
        &doc.validate().expect("validate"),
        &{
            // 이것을 실패시키는 것: 합성 fixture의 원본 charPr/paraPr 0 정의 누락.
            let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
            store.push_char_shape(hwpforge_smithy_hwpx::HwpxCharShape::default());
            store.push_para_shape(hwpforge_smithy_hwpx::HwpxParaShape::default());
            store
        },
        &Default::default(),
    )
    .expect("encode");
    assert_eq!(section_xml(&bytes).matches(DEFAULT).count(), 1);
}
