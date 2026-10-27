#![allow(dead_code)]

use hwpforge::core::image::ImageStore;
use hwpforge::core::run::Run;
use hwpforge::core::table::{Table, TableCell, TableRow};
use hwpforge::core::{Document, Draft, PageSettings, Paragraph, Section};
use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge::hwpx::{HwpxDecoder, HwpxEncoder};

pub(crate) fn text_para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

/// A stamp candidate (`(   )`) and a 1×2 label table.
pub(crate) fn editable() -> Vec<u8> {
    let width = HwpUnit::new(8000).expect("width");
    let table = Table::new(vec![TableRow::new(vec![
        TableCell::new(vec![text_para("성명")], width),
        TableCell::new(vec![text_para("")], width),
    ])]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(table, CharShapeIndex::new(0)));
    let section =
        Section::with_paragraphs(vec![text_para("소속: (   )"), host], PageSettings::a4());
    let mut doc = Document::new();
    doc.add_section(section);
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
        .expect("encode")
}

pub(crate) fn encode(doc: Document<Draft>) -> Vec<u8> {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
        .expect("encode")
}

/// Top-level paragraph texts of the first section.
pub(crate) fn texts(bytes: &[u8]) -> Vec<String> {
    HwpxDecoder::decode(bytes).expect("decode").document.sections()[0]
        .paragraphs
        .iter()
        .map(Paragraph::text_content)
        .collect()
}

/// A body paren blank plus a checkbox in a table cell.
pub(crate) fn two_candidates() -> Vec<u8> {
    let width = HwpUnit::new(8000).expect("width");
    let row = TableRow::new(vec![TableCell::new(vec![text_para("□ 동의")], width)]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(Table::new(vec![row]), CharShapeIndex::new(0)));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![text_para("성명: (   )"), host],
        PageSettings::a4(),
    ));
    encode(doc)
}

pub(crate) fn run_para(run: Run) -> Paragraph {
    Paragraph::with_runs(vec![run], ParaShapeIndex::new(0))
}
