//! Underline / strikeout line-family showcase.
//!
//! Generates one HWPX with several runs, each carrying a different underline
//! or strikeout line shape (SOLID, DASH, DOT, DASH_DOT, DOUBLE_SLIM, WAVE, …).
//! Used as the visual gate for the "richer strike/underline line families"
//! carry: open in 한컴 and confirm each line family renders distinctly.
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example underline_shapes
//!
//! Output:
//!   examples/hwp5_review/converted-underline-shapes.hwpx

use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    CharShapeIndex, HwpUnit, ParaShapeIndex, StrikeoutShape, UnderlineShape, UnderlineType,
};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

fn underline_shape(store: &mut HwpxStyleStore, shape: UnderlineShape) -> CharShapeIndex {
    let mut cs = HwpxCharShape::default();
    cs.height = HwpUnit::from_pt(14.0).unwrap();
    cs.underline_type = UnderlineType::Bottom;
    cs.underline_shape = shape;
    store.push_char_shape(cs)
}

fn strikeout_shape(store: &mut HwpxStyleStore, shape: StrikeoutShape) -> CharShapeIndex {
    let mut cs = HwpxCharShape::default();
    cs.height = HwpUnit::from_pt(14.0).unwrap();
    cs.strikeout_shape = shape;
    store.push_char_shape(cs)
}

fn para(text: &str, cs: CharShapeIndex) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, cs)], ParaShapeIndex::new(0))
}

fn main() {
    let mut store = HwpxStyleStore::new();
    store.push_char_shape(HwpxCharShape::default()); // index 0: plain (headings)
    store.push_para_shape(HwpxParaShape::default());

    let mut paragraphs = vec![para(
        "밑줄/취소선 선 종류 (underline & strikeout line families)",
        CharShapeIndex::new(0),
    )];

    // Underline line families.
    for (label, shape) in [
        ("밑줄 SOLID (실선)", UnderlineShape::Solid),
        ("밑줄 DASH (파선)", UnderlineShape::Dash),
        ("밑줄 DOT (점선)", UnderlineShape::Dot),
        ("밑줄 DASH_DOT (일점쇄선)", UnderlineShape::DashDot),
        ("밑줄 DASH_DOT_DOT (이점쇄선)", UnderlineShape::DashDotDot),
        ("밑줄 LONG_DASH (긴 파선)", UnderlineShape::LongDash),
        ("밑줄 DOUBLE_SLIM (이중 실선)", UnderlineShape::DoubleSlim),
        ("밑줄 WAVE (물결)", UnderlineShape::Wave),
    ] {
        let cs = underline_shape(&mut store, shape);
        paragraphs.push(para(label, cs));
    }

    // Strikeout line families.
    for (label, shape) in [
        ("취소선 SOLID (실선)", StrikeoutShape::Solid),
        ("취소선 DASH (파선)", StrikeoutShape::Dash),
        ("취소선 DOUBLE_SLIM (이중)", StrikeoutShape::DoubleSlim),
        ("취소선 WAVE (물결)", StrikeoutShape::Wave),
    ] {
        let cs = strikeout_shape(&mut store, shape);
        paragraphs.push(para(label, cs));
    }

    let images = ImageStore::new();
    let mut doc: Document = Document::new();
    doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));

    let validated = doc.validate().expect("validation failed");
    let bytes: Vec<u8> = HwpxEncoder::encode(&validated, &store, &images).expect("encode failed");

    let output_path = "examples/hwp5_review/converted-underline-shapes.hwpx";
    std::fs::write(output_path, &bytes).expect("write failed");
    println!("Generated: {output_path} ({} bytes)", bytes.len());
}
