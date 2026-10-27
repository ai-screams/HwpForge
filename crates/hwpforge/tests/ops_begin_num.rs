#![cfg(feature = "ops-hwpx")]
#[path = "support/document.rs"]
mod document;
use document::text_para;
#[path = "support/stamping.rs"]
mod stamping;
use stamping::stamp_all;
#[path = "support/ops_begin_num.rs"]
mod shared;
use shared::*;

use std::io::{Cursor, Read, Write};

use hwpforge::core::image::ImageStore;
use hwpforge::core::run::Run;
use hwpforge::core::section::BeginNum;
use hwpforge::core::table::{Table, TableCell, TableRow};
use hwpforge::core::{Document, PageSettings, Paragraph, Section};
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge::hwpx::HwpxEncoder;
use hwpforge::ops::edit::{set_cell, SetCellOptions};
use hwpforge::ops::style::{restyle, RestyleOptions};

/// A package this codec wrote — set-cell and stamp accept it — whose only
/// section restarts at page 5, with a stamp candidate
/// (`(   )`) and a 1×2 label table. Its header comes out as ones.
fn editable() -> Vec<u8> {
    let width = HwpUnit::new(8000).expect("width");
    let table = Table::new(vec![TableRow::new(vec![
        TableCell::new(vec![text_para("성명")], width),
        TableCell::new(vec![text_para("")], width),
    ])]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(table, CharShapeIndex::new(0)));
    let mut section =
        Section::with_paragraphs(vec![text_para("소속: (   )"), host], PageSettings::a4());
    section.begin_num =
        Some(BeginNum { page: 5, pic: 0, tbl: 0, equation: 0, ..BeginNum::default() });
    let mut doc = Document::new();
    doc.add_section(section);
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    let bytes =
        HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
            .expect("encode");
    assert_hancom_numbers(&bytes, "the codec's own output");
    bytes
}

fn edit_cell(bytes: &[u8]) -> Result<hwpforge::ops::edit::SetCellOutput, hwpforge::ops::OpsError> {
    set_cell(bytes, &SetCellOptions::default().with_table(0).with_at("0,1").with_text("류한율"))
}

/// for the preserve-first editors: set-cell and stamp keep the header at
/// ones and the section's own restart at 5; a header of 0 is repaired and
/// reported; a header start number Core cannot carry is refused.
///
/// 이것을 실패시키는 것: header 를 첫 구역 `begin_num` 에서 만드는 것(header
/// `page="5"`), 고침 경고를 지우는 것, 0 고침을 손실로 분류하는 것(편집 거부),
/// 손실 경고를 지우는 것(편집이 5 를 1 로 다시 씀).
#[test]
fn set_cell_and_stamp_keep_hancoms_header_start_numbers() {
    let source = editable();
    let edited = edit_cell(&source).expect("set-cell");
    assert_hancom_numbers(&edited.bytes, "set-cell");
    let stamped = stamp_all(&source).expect("stamp");
    assert_hancom_numbers(&stamped.bytes, "stamp");

    let zero = with_header_numbers(
        &source,
        r#"<hh:beginNum page="0" footnote="1" endnote="1" pic="0" tbl="0" equation="0"/>"#,
    );
    let edited = edit_cell(&zero).expect("set-cell repairs a header of 0");
    assert_hancom_numbers(&edited.bytes, "set-cell of a header of 0");
    assert!(
        edited.warnings.iter().any(|w| w.info().code == "BEGIN_NUM_REPAIRED"),
        "{:?}",
        edited.warnings
    );
    let stamped = stamp_all(&zero).expect("stamp repairs a header of 0");
    assert_hancom_numbers(&stamped.bytes, "stamp of a header of 0");
    assert!(
        stamped.warnings.iter().any(|w| w.info().code == "BEGIN_NUM_REPAIRED"),
        "{:?}",
        stamped.warnings
    );

    let lost = with_header_numbers(
        &source,
        r#"<hh:beginNum page="5" footnote="1" endnote="1" pic="1" tbl="1" equation="1"/>"#,
    );
    for (surface, err) in [
        ("set-cell", edit_cell(&lost).map(|_| ()).unwrap_err()),
        ("stamp", stamp_all(&lost).map(|_| ()).unwrap_err()),
    ] {
        assert_eq!(err.code(), OpsCode::InputNotRoundtripSafe, "{surface}: {err}");
        assert!(err.to_string().contains("begin_num"), "{surface}: {err}");
    }
}

/// A header with a 0 to repair and values to lose at once is refused: the
/// loss is not hidden behind the repair.
///
/// 이것을 실패시키는 것: 고침 경고 뒤에 조기 반환하는 것, 손실 판정에서
/// tbl·equation 을 빼는 것.
#[test]
fn a_mixed_header_is_refused_by_the_editors() {
    let mixed = with_header_numbers(
        &editable(),
        r#"<hh:beginNum page="0" footnote="1" endnote="1" pic="1" tbl="2" equation="3"/>"#,
    );
    let opts = RestyleOptions::default().with_preset("modern");
    for (surface, err) in [
        ("restyle", restyle(&mixed, &opts).map(|_| ()).unwrap_err()),
        ("set-cell", edit_cell(&mixed).map(|_| ()).unwrap_err()),
        ("stamp", stamp_all(&mixed).map(|_| ()).unwrap_err()),
    ] {
        assert_eq!(err.code(), OpsCode::InputNotRoundtripSafe, "{surface}: {err}");
        assert!(err.to_string().contains("begin_num"), "{surface}: {err}");
    }
}
