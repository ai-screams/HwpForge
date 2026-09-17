//! `ops::stamp::stamp_plan` — candidate discovery, phase one of stamping.
#![cfg(feature = "ops-hwpx")]

use hwpforge::core::image::ImageStore;
use hwpforge::core::run::Run;
use hwpforge::core::table::{Table, TableCell, TableRow};
use hwpforge::core::{Document, Draft, PageSettings, Paragraph, Section};
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge::hwpx::stamp::STAMP_MAP_VERSION;
use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge::hwpx::HwpxEncoder;
use hwpforge::ops::stamp::stamp_plan;

fn text_para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn encode(doc: Document<Draft>) -> Vec<u8> {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
        .expect("encode")
}

/// The two dominant class-A shapes of the government corpus: a paren blank
/// in the body and a checkbox inside a table cell.
fn two_candidates() -> Vec<u8> {
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

/// A document with no placeholder markers at all.
fn no_candidates() -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![text_para("본문")], PageSettings::a4()));
    encode(doc)
}

/// The object's keys, sorted, so the assertion is on the key **set** rather
/// than on `serde_json`'s map ordering.
fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
    keys.sort();
    keys
}

#[test]
fn finds_the_text_candidates_a_document_offers() {
    let out = stamp_plan(&two_candidates()).expect("plan");

    assert_eq!(out.plan.text.len(), 2, "paren blank + checkbox: {:?}", out.plan.text);
    assert_eq!(out.plan.schema_version, STAMP_MAP_VERSION);
    assert!(out.plan.cells.is_empty(), "the only cell already has authored content");
    assert!(out.plan.skipped_tables.is_empty(), "the table tiles a well-formed grid");
    assert!(out.warnings.is_empty(), "this document decodes cleanly: {:?}", out.warnings);
}

#[test]
fn the_source_hash_pins_the_exact_input_bytes() {
    let bytes = two_candidates();
    let plan = stamp_plan(&bytes).expect("plan").plan;

    assert_eq!(plan.source_sha256.len(), 64, "{}", plan.source_sha256);
    assert!(plan.source_sha256.bytes().all(|b| b.is_ascii_hexdigit()), "{}", plan.source_sha256);
    assert_eq!(
        plan.source_sha256,
        stamp_plan(&bytes).expect("re-plan").plan.source_sha256,
        "one buffer always hashes the same"
    );
    assert_ne!(
        plan.source_sha256,
        stamp_plan(&no_candidates()).expect("plan").plan.source_sha256,
        "different documents cannot share a pin"
    );
}

#[test]
fn a_document_without_markers_plans_nothing() {
    let out = stamp_plan(&no_candidates()).expect("plan");

    assert!(out.plan.text.is_empty());
    assert!(out.plan.cells.is_empty());
}

#[test]
fn discovery_never_touches_the_document() {
    let bytes = two_candidates();

    let first = stamp_plan(&bytes).expect("plan");
    let second = stamp_plan(&bytes).expect("re-plan");

    assert_eq!(first.plan.text.len(), second.plan.text.len(), "planning is read-only");
    assert_eq!(first.plan.source_sha256, second.plan.source_sha256);
}

#[test]
fn a_non_document_input_is_a_stamp_codec_failure() {
    let err = stamp_plan(b"not a zip at all").expect_err("must reject");

    assert_eq!(err.code(), OpsCode::StampCodecFailed, "{err}");
    assert_eq!(err.code().as_str(), "STAMP_CODEC_FAILED");
}

#[test]
fn meta_flattens_the_plan_beside_the_warnings() {
    let out = stamp_plan(&two_candidates()).expect("plan");

    let value = serde_json::to_value(out.meta()).expect("serialise");
    assert_eq!(
        keys(&value),
        ["cells", "schema_version", "skipped_tables", "source_sha256", "text", "warnings"]
    );
    assert_eq!(value["schema_version"], STAMP_MAP_VERSION);
    assert_eq!(value["text"].as_array().expect("text array").len(), 2);
    assert_eq!(value["warnings"], serde_json::json!([]));
}
