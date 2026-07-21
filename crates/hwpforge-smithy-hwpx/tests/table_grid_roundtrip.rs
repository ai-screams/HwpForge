//! Wave 0 gates for covered empty table rows (E3 table grid addressing).
//!
//! Native wire keeps a row element with no cells when row spans from earlier
//! rows cover the row completely. These gates lock the truthful Core shape
//! through the HWPX codec: encode emits an empty row element, decode restores
//! the empty row, and the result is a decode/encode fixed point.

use std::io::{Cursor, Read};

use hwpforge_core::image::ImageStore;
use hwpforge_core::page::PageSettings;
use hwpforge_core::run::Run;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::{Document, Draft, Paragraph, Section};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

fn text_para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn encode(doc: Document<Draft>) -> Vec<u8> {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    let validated = doc.validate().expect("validate");
    HwpxEncoder::encode(&validated, &styles, &ImageStore::new()).expect("encode")
}

/// 2×1 grid: one rs-2 anchor, second row fully covered (no cells).
fn covered_row_doc() -> Document<Draft> {
    let width = HwpUnit::new(8000).unwrap();
    let anchor = TableCell::with_span(vec![text_para("병합")], width, 1, 2);
    let table = Table::new(vec![TableRow::new(vec![anchor]), TableRow::new(vec![])]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(table, CharShapeIndex::new(0)));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![host], PageSettings::default()));
    doc
}

fn section0_xml(bytes: &[u8]) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).expect("open zip");
    let mut file = archive.by_name("Contents/section0.xml").expect("section0");
    let mut xml = String::new();
    file.read_to_string(&mut xml).expect("read section0");
    xml
}

#[test]
fn covered_empty_row_encodes_as_empty_row_element() {
    let bytes = encode(covered_row_doc());
    let xml = section0_xml(&bytes);
    assert_eq!(xml.matches("<hp:tr").count(), 2, "wire must keep both rows: {xml}");
    assert_eq!(xml.matches("<hp:tc").count(), 1, "only the anchor cell exists as tc");
    assert!(xml.contains(r#"rowSpan="2""#), "anchor must carry its row span");
}

#[test]
fn covered_empty_row_round_trips_to_fixed_point() {
    let bytes = encode(covered_row_doc());

    let d1 = HwpxDecoder::decode(&bytes).expect("d1");
    {
        let table = d1.document.sections()[0].paragraphs[0]
            .runs
            .iter()
            .find_map(|run| run.content.as_table())
            .expect("expected table run");
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0].cells.len(), 1);
        assert_eq!(table.rows[0].cells[0].row_span, 2);
        assert!(table.rows[1].cells.is_empty(), "decoded covered row must stay empty");
    }

    let e1 = HwpxEncoder::encode(
        &d1.document.clone().validate().expect("validate d1"),
        &d1.style_store,
        &d1.image_store,
    )
    .expect("e1");
    let d2 = HwpxDecoder::decode(&e1).expect("d2");
    assert_eq!(d1.document, d2.document, "covered empty row must be a decode/encode fixed point");
}
