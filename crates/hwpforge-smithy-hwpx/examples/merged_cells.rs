//! A numbered 3×3 table with rectangular and horizontal merges.
//! Run from the workspace root: cargo run -p hwpforge-smithy-hwpx --example merged_cells
use hwpforge_core::{
    document::Document,
    image::ImageStore,
    layout::{LayoutCache, LineSeg},
    paragraph::Paragraph,
    run::Run,
    section::Section,
    table::{Table, TableCell, TableRow},
    PageSettings,
};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{
    style_store::{HwpxCharShape, HwpxParaShape, HwpxStyle, HwpxStyleStore},
    EncodeOptions, HwpxEncoder,
};

fn paragraph(text: &str) -> Paragraph {
    let mut p =
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0));
    p.layout_cache = Some(LayoutCache::new(vec![line()]));
    p
}

fn line() -> LineSeg {
    let mut line = LineSeg::default();
    line.vertsize = 1000;
    line.textheight = 1000;
    line.baseline = 850;
    line.spacing = 600;
    line.horzsize = 48188;
    line.flags = 0x0060_0000;
    line
}

fn cell(text: &str, cols: u16, rows: u16) -> TableCell {
    TableCell::with_span(
        vec![paragraph(text)],
        HwpUnit::new(12000 * i32::from(cols)).unwrap(),
        cols,
        rows,
    )
}

pub fn build_merged_cells() -> Vec<u8> {
    // An invented, deliberately unavailable face makes font resolution failures reproducible.
    let mut styles = HwpxStyleStore::with_default_fonts("HwpForge Missing Test Face");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    styles.push_style(HwpxStyle::new(0, "PARA", "본문", "Body", 0, 0, 0, 1042, 0));
    let table = Table::new(vec![
        TableRow::new(vec![cell("1", 2, 2), cell("2", 1, 1)]),
        TableRow::new(vec![cell("3", 1, 1)]),
        TableRow::new(vec![cell("5", 1, 1), cell("4", 2, 1)]),
    ]);
    let mut host = Paragraph::with_runs(
        vec![Run::table(table, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    host.layout_cache = Some(LayoutCache::new(vec![line()]));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![host, paragraph("병합 셀의 숫자를 읽습니다.")],
        PageSettings::a4(),
    ));
    HwpxEncoder::encode_with_options(
        &doc.validate().unwrap(),
        &styles,
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap()
}

fn main() {
    let bytes = build_merged_cells();
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "crates/hwpforge-smithy-hwpx/tests/fixtures/SimpleTable.hwpx".into());
    std::fs::write(path, bytes).unwrap();
}
