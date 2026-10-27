#[path = "support/encoding.rs"]
mod encoding;
use encoding::batang_styles as styles;
#[path = "support/wrapped_offsets.rs"]
mod shared;
use shared::*;

use hwpforge_core::image::ImageStore;
use hwpforge_core::placement::{ObjectPlacement, ObjectRelativeTo, ObjectTextWrap};
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::{Control, Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

fn encode_one(run: Run) -> Vec<u8> {
    let para = Paragraph::with_runs(vec![run], ParaShapeIndex::new(0));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles(), &ImageStore::new())
        .expect("encode")
}

fn negative_placement() -> ObjectPlacement {
    ObjectPlacement::new(ObjectTextWrap::Square, ObjectRelativeTo::Para, ObjectRelativeTo::Column)
        .with_vert_offset(HwpUnit::new(-2429).unwrap())
        .with_horz_offset(HwpUnit::new(-106).unwrap())
}

/// HWP5 stores offsets as signed `i32`; a Core table placed at negative
/// offsets (as the HWP5 decoder produces it) is written wrapped and decodes
/// back to the same placement.
///
/// 이것을 실패시키는 것: `HxTablePos` 를 i32 로 쓰기.
#[test]
fn signed_core_table_offsets_are_written_wrapped() {
    let cell = TableCell::new(
        vec![Paragraph::with_runs(
            vec![Run::text("c", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        HwpUnit::new(8000).unwrap(),
    );
    let table = Table::new(vec![TableRow::new(vec![cell])]).with_placement(negative_placement());
    let bytes = encode_one(Run::table(table, CharShapeIndex::new(0)));
    let out = section_xml(&bytes);
    assert!(out.contains(&format!(r#"vertOffset="{V}" horzOffset="{H}""#)), "{out}");
    assert_no_signed_offset(&out);

    let doc = HwpxDecoder::decode(&bytes).expect("decode");
    let placement = doc.document.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .find_map(|r| match &r.content {
            RunContent::Table(t) => t.placement.clone(),
            _ => None,
        })
        .expect("table placement");
    assert_eq!(placement, negative_placement());
}

/// An HWP5 OLE chart is written as a hand-built `<hp:switch>` whose chart
/// and OLE fallback both carry `<hp:pos>`; negative Core offsets are written
/// wrapped in both, and the chart decodes with the signed offsets.
///
/// 이것을 실패시키는 것: `build_embedded_chart_run_xml` 의 `{vo}`·`{ho}` 를
/// i32 로 쓰기.
#[test]
fn hwp5_chart_offsets_are_written_wrapped() {
    let chart = Control::EmbeddedChart {
        chart_xml: r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"/>"#.to_string(),
        ole_bytes: vec![0; 16],
        width: HwpUnit::new(20000).unwrap(),
        height: HwpUnit::new(10000).unwrap(),
        placement: Some(negative_placement()),
    };
    let bytes = encode_one(Run::control(chart, CharShapeIndex::new(0)));
    let out = section_xml(&bytes);
    assert_eq!(out.matches(&format!(r#"vertOffset="{V}" horzOffset="{H}""#)).count(), 2, "{out}");
    assert_no_signed_offset(&out);
}

/// A TextArt whose Core size is negative (validation rejects only 0) writes
/// `<hp:curSz>` as the unsigned wrap Hancom uses; the derived `scaMatrix`
/// stays signed.
///
/// 이것을 실패시키는 것: 글맵시 `<hp:curSz>` 의 `width`·`height` 를 i32 로
/// 쓰기(각각) / 크기 계산(`scaMatrix`)까지 부호 없는 값으로 하기.
#[test]
fn text_art_negative_size_writes_wrapped_cur_sz() {
    let art = Control::TextArt {
        frame: hwpforge_core::ObjectFrame::new(),
        text: "글".to_string(),
        shape: "WAVE2".to_string(),
        font_name: "함초롬바탕".to_string(),
        font_style: "보통".to_string(),
        align: "LEFT".to_string(),
        line_spacing: 120,
        char_spacing: 100,
        width: HwpUnit::new(-14173).unwrap(),
        height: HwpUnit::new(-2355).unwrap(),
        placement: None,
        fill_color: None,
        inst_id: None,
    };
    let out = section_xml(&encode_one(Run::control(art, CharShapeIndex::new(0))));
    assert!(out.contains(r#"<hp:curSz width="4294953123" height="4294964941"/>"#), "{out}");
    assert!(
        out.contains(r#"<hc:scaMatrix e1="-1.000000" e2="0" e3="0" e4="0" e5="-0.166161""#),
        "{out}"
    );
}
