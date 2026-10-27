#![cfg(feature = "ops-hwpx")]
#[path = "support/document.rs"]
mod document;
use document::text_para;
#[path = "support/ops_header_footer_anchor.rs"]
mod shared;
use shared::*;

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
use hwpforge::ops::stamp::{stamp, stamp_plan, StampOptions};

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
    let mut footer = HeaderFooter::all_pages(vec![text_para("foot")]);
    footer.anchor_paragraph = Some(1);
    section.footers.push(footer);
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
