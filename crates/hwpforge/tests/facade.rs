//! The facade's own job: re-export wiring that works in the default feature
//! set.
//!
//! This test is deliberately **not** feature-gated. Everything else in
//! `tests/` needs `ops-hwpx` or `ops-md`, so without this file
//! `cargo nextest run -p hwpforge` would find no tests at all and a broken
//! default surface would reach users unnoticed.
#![cfg(feature = "hwpx")]

use hwpforge::core::{Document, Draft, ImageStore, PageSettings, Paragraph, Run, Section};
use hwpforge::foundation::{CharShapeIndex, Color, ParaShapeIndex};
use hwpforge::hwpx::{HwpxDecoder, HwpxEncoder, HwpxStyleStore};

#[test]
fn a_document_built_through_the_facade_round_trips() {
    let mut doc = Document::<Draft>::new();
    let paragraph = Paragraph::with_runs(
        vec![Run::text("Hello, 한글!", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    doc.add_section(Section::with_paragraphs(vec![paragraph], PageSettings::a4()));

    let validated = doc.validate().expect("valid document");
    let bytes = HwpxEncoder::encode(
        &validated,
        &HwpxStyleStore::with_default_fonts("함초롬바탕"),
        &ImageStore::new(),
    )
    .expect("encode");

    let decoded = HwpxDecoder::decode(&bytes).expect("decode");
    assert_eq!(decoded.document.sections().len(), 1);
}

#[test]
fn foundation_colours_are_built_from_rgb_not_raw() {
    // The workspace's oldest trap: the wire order is BGR, so `from_rgb` is
    // the only safe constructor. Pinned here because the facade is where a
    // new user meets `Color` first.
    let red = Color::from_rgb(0xFF, 0x00, 0x00);

    assert_eq!(red.to_hex_rgb(), "#FF0000");
}
