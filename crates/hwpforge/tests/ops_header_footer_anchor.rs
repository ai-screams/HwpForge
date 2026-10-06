//! The re-encoding surfaces keep a header in the paragraph it is anchored to
//! (§19.17 F2). Before, every surface moved it into the first paragraph, so
//! Hancom applied it from page 1 (x4b: header C belongs to page 2 onwards).
#![cfg(feature = "ops-hwpx")]

use std::io::{Cursor, Read};

use hwpforge::core::image::ImageStore;
use hwpforge::core::run::Run;
use hwpforge::core::section::HeaderFooter;
use hwpforge::core::table::{Table, TableCell, TableRow};
use hwpforge::core::{Document, PageSettings, Paragraph, Section};
use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge::hwpx::stamp::{StampAction, StampMap, StampSpec};
use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge::hwpx::HwpxEncoder;
use hwpforge::ops::edit::{set_cell, SetCellOptions};
use hwpforge::ops::exchange::{from_json, to_json, FromJsonOptions, ToJsonOptions};
use hwpforge::ops::stamp::{stamp, stamp_plan, StampOptions};
use hwpforge::ops::style::{restyle, RestyleOptions};

fn section_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name("Contents/section0.xml").expect("section").read_to_string(&mut out).unwrap();
    out
}

/// Top-level paragraph indices whose XML holds `needle`.
fn paragraphs_with(xml: &str, needle: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let (mut depth, mut index, mut start, mut i) = (0usize, 0usize, 0usize, 0usize);
    while i < xml.len() {
        let rest = &xml[i..];
        if rest.starts_with("<hp:p ") || rest.starts_with("<hp:p>") {
            if depth == 0 {
                start = i;
            }
            depth += 1;
            i += 5;
        } else if rest.starts_with("</hp:p>") {
            depth -= 1;
            i += 7;
            if depth == 0 {
                if xml[start..i].contains(needle) {
                    found.push(index);
                }
                index += 1;
            }
        } else {
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    found
}

/// 이것을 실패시키는 것: 인코더가 앵커를 무시하고 첫 문단에 주입(세 표면
/// 모두 `[0]`).
#[test]
fn restyle_and_from_json_keep_x4b_header_in_paragraph_14() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/header_footer/x4b-header-only-later.hwpx"
    );
    let source = std::fs::read(path).expect("x4b");
    let restyled =
        restyle(&source, &RestyleOptions::default().with_preset("modern")).expect("restyle").bytes;
    assert_eq!(paragraphs_with(&section_xml(&restyled), "<hp:header "), [14], "restyle");

    let json = to_json(&source, &ToJsonOptions::default()).expect("to_json").document.to_string();
    assert!(json.contains(r#""anchor_paragraph":14"#), "to_json carries the anchor");
    let rebuilt = from_json(&json, &FromJsonOptions::default().with_base(source.clone()))
        .expect("from_json --base")
        .bytes;
    assert_eq!(paragraphs_with(&section_xml(&rebuilt), "<hp:header "), [14], "from_json --base");
}

fn text_para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

/// A package this codec wrote — set-cell and stamp accept it — with a
/// footer anchored to paragraph 1, a stamp candidate in paragraph 1 and a
/// label table in paragraph 2.
fn editable() -> Vec<u8> {
    let width = HwpUnit::new(8000).expect("width");
    let table = Table::new(vec![TableRow::new(vec![
        TableCell::new(vec![text_para("성명")], width),
        TableCell::new(vec![text_para("")], width),
    ])]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(table, CharShapeIndex::new(0)));
    let mut section = Section::with_paragraphs(
        vec![text_para("first"), text_para("소속: (   )"), host],
        PageSettings::a4(),
    );
    section.footers.push(HeaderFooter::all_pages(vec![text_para("foot")]).with_anchor_paragraph(1));
    let mut doc = Document::new();
    doc.add_section(section);
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
        .expect("encode")
}

/// The preserve-first editors re-encode too; the footer stays in paragraph 1.
/// 이것을 실패시키는 것: 같은 변이(`[0]`), 또는 편집기 admission 이 앵커를
/// Core 비교에서 잃는 것(편집 거부).
#[test]
fn set_cell_and_stamp_keep_an_anchored_footer_in_place() {
    let source = editable();
    assert_eq!(paragraphs_with(&section_xml(&source), "<hp:footer "), [1]);

    let edited = set_cell(
        &source,
        &SetCellOptions::default().with_table(0).with_at("0,1").with_text("이름"),
    )
    .expect("set-cell");
    assert_eq!(paragraphs_with(&section_xml(&edited.bytes), "<hp:footer "), [1], "set-cell");

    let specs: Vec<StampSpec> = stamp_plan(&source)
        .expect("plan")
        .plan
        .text
        .into_iter()
        .map(|candidate| StampSpec {
            section: candidate.section,
            path: candidate.path,
            span: candidate.span,
            marker: candidate.marker.clone(),
            action: StampAction::Field { name: "소속".into(), hint: None },
        })
        .collect();
    assert!(!specs.is_empty(), "the paren blank is a stamp candidate");
    let stamped =
        stamp(&source, &StampMap::Legacy(specs), &StampOptions::default()).expect("stamp");
    assert_eq!(paragraphs_with(&section_xml(&stamped.bytes), "<hp:footer "), [1], "stamp");
}

/// Paragraph insert/delete splice raw bytes, so the footer control moves with
/// its paragraph; the Core document the editor checks against must move the
/// anchor too (before, an insert at the anchor encoded a second footer and
/// the self-verify refused the edit).
/// 이것을 실패시키는 것: 삽입 뒤 앵커 이동을 지우기 / 삭제 뒤 앵커 갱신을
/// 지우기(둘 다 self-verify 거부).
#[test]
fn structural_edits_move_an_anchored_footer_with_its_paragraph() {
    use hwpforge::ops::edit::{delete_para, insert_para, DeleteParaOptions, InsertParaOptions};
    let source = editable();
    let inserted = insert_para(
        &source,
        &InsertParaOptions::default().with_anchor(0).with_text(vec!["new".into()]),
    )
    .expect("insert after paragraph 0");
    assert_eq!(paragraphs_with(&section_xml(&inserted.bytes), "<hp:footer "), [2]);

    let deleted = delete_para(&inserted.bytes, &DeleteParaOptions::default().with_indexes(vec![1]))
        .expect("delete the paragraph before the anchor");
    assert_eq!(paragraphs_with(&section_xml(&deleted.bytes), "<hp:footer "), [1]);

    let gone = delete_para(&source, &DeleteParaOptions::default().with_indexes(vec![1]))
        .expect("delete the anchor paragraph");
    assert!(!section_xml(&gone.bytes).contains("<hp:footer "), "deleted with its paragraph");
}
