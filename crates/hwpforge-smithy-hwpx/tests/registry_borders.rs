use hwpforge_blueprint::{
    border_fill::{Border, BorderFill, Fill},
    registry::StyleRegistry,
};
use hwpforge_core::{
    document::Document,
    image::ImageStore,
    page::PageSettings,
    paragraph::Paragraph,
    run::Run,
    section::{HeaderFooter, Section},
    table::{Table, TableCell, TableRow},
};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{
    HancomStyleSet, HwpxEncoder, HwpxRegistryBridge, HwpxStyleStore, ListHeadPolicy,
};
#[path = "support/xml.rs"]
mod xml;
fn registry() -> StyleRegistry {
    let mut r = StyleRegistry::with_fonts(vec![]);
    let mut shape = hwpforge_blueprint::style::PartialCharShape::default();
    shape.font = Some("Batang".into());
    shape.size = Some(HwpUnit::new(1000).unwrap());
    r.char_shapes.push(shape.resolve("body").unwrap());
    r.para_shapes.push(hwpforge_blueprint::style::ParaShape::default());
    r.register_border_fill(BorderFill { border: Border::default(), fill: Fill::default() })
        .unwrap();
    r
}
fn paragraph() -> Paragraph {
    Paragraph::with_runs(vec![Run::text("A", CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}
fn table(id: u32, content: Vec<Paragraph>) -> Table {
    Table::new(vec![TableRow::new(vec![
        TableCell::new(content, HwpUnit::new(10000).unwrap()).with_border_fill_id(id)
    ])])
    .with_border_fill_id(id)
}
fn containing(t: Table) -> Paragraph {
    Paragraph::with_runs(vec![Run::table(t, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}
// 이것을 실패시키는 것: 기존 bridge/store 경로의 테두리 목록 거부 삭제.
#[test]
fn legacy_border_registry_rejected() {
    let r = registry();
    assert!(HwpxRegistryBridge::from_registry(&r).is_err());
    assert!(HwpxRegistryBridge::from_registry_with(&r, HancomStyleSet::Classic).is_err());
    for policy in [ListHeadPolicy::Template, ListHeadPolicy::PreserveEffective] {
        assert!(HwpxRegistryBridge::from_registry_with_diagnostics(
            &r,
            HancomStyleSet::Modern,
            policy
        )
        .is_err());
    }
    assert!(HwpxStyleStore::from_registry(&r).is_err());
}
// 이것을 실패시키는 것: 중첩/머리말/꼬리말 표 누락, 두 번 재연결, 미등록 참조 통과.
#[test]
fn nested_import_border_references() {
    let r = registry();
    let bridge =
        HwpxRegistryBridge::from_registry_for_import(&r, HancomStyleSet::Modern).unwrap().value;
    let mut doc = Document::new();
    let mut section = Section::with_paragraphs(
        vec![containing(table(1, vec![containing(table(1, vec![paragraph()]))]))],
        PageSettings::a4(),
    );
    section.headers.push(HeaderFooter::all_pages(vec![containing(table(1, vec![paragraph()]))]));
    section.footers.push(HeaderFooter::all_pages(vec![containing(table(1, vec![paragraph()]))]));
    doc.add_section(section);
    let rebound = bridge.rebind_draft_document(doc).unwrap();
    let mut count = 0;
    rebound.for_each_table(|t| {
        count += 1;
        assert_eq!(t.border_fill_id, Some(4));
        assert_eq!(t.rows[0].cells[0].border_fill_id, Some(4));
    });
    assert_eq!(count, 4);
    let fill = bridge.style_store().border_fill(4).unwrap();
    assert_eq!(
        [
            fill.left.line_type.as_str(),
            fill.right.line_type.as_str(),
            fill.top.line_type.as_str(),
            fill.bottom.line_type.as_str()
        ],
        ["NONE"; 4]
    );
    assert!(fill.fill.is_none());
    assert!(fill.diagonal.is_none());
    let bytes =
        HwpxEncoder::encode(&rebound.validate().unwrap(), bridge.style_store(), &ImageStore::new())
            .unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut header = String::new();
    use std::io::Read;
    archive.by_name("Contents/header.xml").unwrap().read_to_string(&mut header).unwrap();
    let tree = xml::Element::parse(&header);
    let fills = tree.descendants("hh:borderFill");
    let fill = fills.iter().find(|e| e.attr("id") == Some("4")).unwrap();
    for name in ["hh:leftBorder", "hh:rightBorder", "hh:topBorder", "hh:bottomBorder"] {
        assert_eq!(fill.child(name).attr("type"), Some("NONE"));
        assert_eq!(fill.child(name).attr("width"), Some("0.12 mm"));
        assert_eq!(fill.child(name).attr("color"), Some("#000000"));
    }
    assert!(fill.descendants("hc:fillBrush").is_empty());
    let mut section = String::new();
    archive.by_name("Contents/section0.xml").unwrap().read_to_string(&mut section).unwrap();
    let tree = xml::Element::parse(&section);
    for tag in ["hp:tbl", "hp:tc"] {
        for element in tree.descendants(tag) {
            assert_eq!(element.attr("borderFillIDRef"), Some("4"));
        }
    }
}
// 이것을 실패시키는 것: store에 있는 참조를 registry 참조로 허용.
#[test]
fn missing_import_border_reference() {
    let bridge = HwpxRegistryBridge::from_registry_for_import(&registry(), HancomStyleSet::Modern)
        .unwrap()
        .value;
    for id in [0, 2, 4, u32::MAX] {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![containing(table(id, vec![paragraph()]))],
            PageSettings::a4(),
        ));
        assert!(bridge.rebind_draft_document(doc).is_err());
    }
}
// 이것을 실패시키는 것: raw 참조를 import 외 경로에서도 재연결.
#[test]
fn legacy_raw_border_references_are_unchanged() {
    let mut r = registry();
    let mut json = serde_json::to_value(&r).unwrap();
    json.as_object_mut().unwrap().remove("border_fills");
    r = serde_json::from_value(json).unwrap();
    let bridge = HwpxRegistryBridge::from_registry(&r).unwrap();
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![containing(table(2, vec![paragraph()]))],
        PageSettings::a4(),
    ));
    let doc = bridge.rebind_draft_document(doc).unwrap();
    doc.for_each_table(|t| {
        assert_eq!(t.border_fill_id, Some(2));
        assert_eq!(t.rows[0].cells[0].border_fill_id, Some(2));
    });
}
// 이것을 실패시키는 것: 지원하지 않는 import 참조 축의 무음 raw 통과.
#[test]
fn unsupported_import_border_axes() {
    let mut r = registry();
    r.char_shapes[0].char_border_fill_id = Some(1);
    assert!(HwpxRegistryBridge::from_registry_for_import(&r, HancomStyleSet::Modern).is_err());
    let mut r = registry();
    r.para_shapes[0].border_fill_id = Some(hwpforge_foundation::BorderFillIndex::new(1));
    assert!(HwpxRegistryBridge::from_registry_for_import(&r, HancomStyleSet::Modern).is_err());
    let bridge = HwpxRegistryBridge::from_registry_for_import(&registry(), HancomStyleSet::Modern)
        .unwrap()
        .value;
    for page in [true, false] {
        let mut t = table(1, vec![paragraph()]);
        if !page {
            t.cell_zones.push(hwpforge_core::table::CellZone::new(0, 0, 0, 0, 1));
        }
        let mut s = Section::with_paragraphs(vec![containing(t)], PageSettings::a4());
        if page {
            s.page_border_fills = Some(Vec::new());
        }
        let mut doc = Document::new();
        doc.add_section(s);
        assert!(bridge.rebind_draft_document(doc).is_err());
    }
}
// 이것을 실패시키는 것: 바탕쪽/표 캡션 재귀를 누락하거나 None을 명시 없음 참조로 바꿈.
#[test]
fn ancillary_import_border_references() {
    use hwpforge_core::{
        caption::{Caption, CaptionSide},
        section::MasterPage,
    };
    let mut outer = table(1, vec![paragraph()]);
    outer.caption =
        Some(Caption::new(vec![containing(table(1, vec![paragraph()]))], CaptionSide::Bottom));
    outer.border_fill_id = None;
    let mut s = Section::with_paragraphs(vec![containing(outer)], PageSettings::a4());
    s.master_pages = Some(vec![MasterPage::new(
        hwpforge_foundation::ApplyPageType::Both,
        vec![containing(table(1, vec![paragraph()]))],
    )]);
    let mut doc = Document::new();
    doc.add_section(s);
    let bridge = HwpxRegistryBridge::from_registry_for_import(&registry(), HancomStyleSet::Modern)
        .unwrap()
        .value;
    let doc = bridge.rebind_draft_document(doc).unwrap();
    let mut refs = Vec::new();
    doc.for_each_table(|t| refs.push(t.border_fill_id));
    assert_eq!(refs, [None, Some(4), Some(4)]);
}
