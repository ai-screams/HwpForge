//! Font width qualification is shared by all editing entry points.
#[path = "support/document.rs"]
mod document;
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;

use hwpforge_core::control::Control;
use hwpforge_core::{
    Document, ImageStore, PageSettings, Paragraph, Run, RunContent, Section, Table, TableCell,
    TableRow,
};
use hwpforge_foundation::{CharShapeIndex, FieldType, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape};
use hwpforge_smithy_hwpx::{
    CellSpec, CellTarget, HwpxCellEditor, HwpxDecoder, HwpxEncoder, HwpxFiller, HwpxPatcher,
    HwpxStyleStore,
};

fn input(field: bool) -> Vec<u8> {
    let mut paragraph = document::text_para("가");
    if field {
        paragraph.runs[0].content = RunContent::Control(Box::new(Control::Field {
            field_type: FieldType::ClickHere,
            hint_text: None,
            help_text: None,
            name: Some("값".into()),
            display_text: "가".into(),
        }));
    }
    let table = Table::new(vec![TableRow::new(vec![TableCell::new(
        vec![paragraph],
        HwpUnit::new(8000).unwrap(),
    )])]);
    let host = Paragraph::with_runs(
        vec![Run::table(table, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![host, document::text_para("뒤")],
        PageSettings::a4(),
    ));
    let mut store = HwpxStyleStore::with_default_fonts("맑은 고딕");
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());
    let bytes = HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new()).unwrap();
    package::splice(
        &bytes,
        "</hp:p>",
        r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="100" vertsize="1000" textheight="1000" baseline="850" spacing="600" horzpos="0" horzsize="1050" flags="393216"/></hp:linesegarray></hp:p>"#,
    )
}

fn patch(bytes: &[u8]) -> Vec<u8> {
    let decoded = HwpxDecoder::decode(bytes).unwrap();
    let mut section = decoded.document.sections()[0].clone();
    let preservation = HwpxPatcher::export_section_preservation(bytes, 0, &section).unwrap();
    let RunContent::Table(table) = &mut section.paragraphs[0].runs[0].content else {
        panic!("table")
    };
    table.rows[0].cells[0].paragraphs[0].runs[0].content = RunContent::Text("힣".into());
    HwpxPatcher::patch_section_preserving(bytes, 0, &section, None, Some(&preservation)).unwrap()
}

// 이것을 실패시키는 것: 공통 폭 자격 우회, 한 슬롯만 검사, 같은 허용 항목 요구 삭제,
// HFT/이름 정규화/substFont 검사 삭제, raw 경로 검사 생략, 허용표 항목 삭제.
#[test]
fn measured_font_matrix_preserves_or_drops_in_all_three_editors() {
    for (from, to, count) in [
        ("맑은 고딕", "맑은 고딕", 3),
        ("맑은 고딕", "돋움체", 3),
        ("맑은 고딕", "맑은고딕", 0),
        ("맑은 고딕", " 맑은 고딕 ", 0),
        ("맑은 고딕", "Unmeasured", 0),
        ("type=\"TTF\"", "type=\"HFT\"", 0),
        ("</hh:font>", "<hh:substFont face=\"맑은 고딕\" type=\"TTF\" isEmbedded=\"0\" binaryItemIDRef=\"\"/></hh:font>", 0),
        ("lang=\"LATIN\" fontCnt=\"1\"><hh:font id=\"0\" face=\"맑은 고딕\"", "lang=\"LATIN\" fontCnt=\"1\"><hh:font id=\"0\" face=\"Times New Roman\"", 0),
        ("lang=\"LATIN\" fontCnt=\"1\"><hh:font id=\"0\" face=\"맑은 고딕\"", "lang=\"LATIN\" fontCnt=\"1\"><hh:font id=\"0\" face=\"돋움체\"", 0),
    ] {
        let plain = package::splice_part(&input(false), "Contents/header.xml", from, to);
        let field = package::splice_part(&input(true), "Contents/header.xml", from, to);
        let cell = HwpxCellEditor::set_cells(&plain, &[CellSpec {table: 0, target: CellTarget::At(hwpforge_core::table::grid::GridCoord::new(0,0)), text:"힣".into()}]).unwrap().bytes;
        let filled = HwpxFiller::fill(&field, &std::collections::BTreeMap::from([("값".into(), "힣".into())])).unwrap().bytes;
        for (route, out) in [("set-cell", cell), ("patch", patch(&plain)), ("fill", filled)] {
            let tree = xml::Element::parse(&package::section_xml(&out));
            assert_eq!(tree.descendants("hp:linesegarray").len(), count, "{route}: {from} -> {to}");
            assert_eq!(tree.find("hp:tc").find("hp:t").text, "힣");
            assert_eq!(tree.children.last().unwrap().find("hp:t").text, "뒤");
        }
    }
}

// 이것을 실패시키는 것: raw 경로의 isEmbedded 검사 생략.
#[test]
fn embedded_fonts_drop_preserving_patch_and_fill_caches() {
    let plain = package::splice_part(
        &input(false),
        "Contents/header.xml",
        "isEmbedded=\"0\"",
        "isEmbedded=\"1\"",
    );
    let field = package::splice_part(
        &input(true),
        "Contents/header.xml",
        "isEmbedded=\"0\"",
        "isEmbedded=\"1\"",
    );
    let filled =
        HwpxFiller::fill(&field, &std::collections::BTreeMap::from([("값".into(), "힣".into())]))
            .unwrap()
            .bytes;
    for out in [patch(&plain), filled] {
        let tree = xml::Element::parse(&package::section_xml(&out));
        assert_eq!(tree.descendants("hp:linesegarray").len(), 0);
        assert_eq!(tree.find("hp:tc").find("hp:t").text, "힣");
    }
}
