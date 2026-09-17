//! `ops::edit::insert_para` — the preserving structural insert (E4).
#![cfg(feature = "ops-hwpx")]

use hwpforge::core::image::ImageStore;
use hwpforge::core::run::Run;
use hwpforge::core::{Document, Draft, PageSettings, Paragraph, Section};
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge::hwpx::{HwpxDecoder, HwpxEncoder};
use hwpforge::ops::edit::{insert_para, InsertParaOptions};

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

/// One section holding three named paragraphs.
fn three_paragraphs() -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![text_para("첫째"), text_para("둘째"), text_para("셋째")],
        PageSettings::a4(),
    ));
    encode(doc)
}

/// Top-level paragraph texts of the first section.
fn texts(bytes: &[u8]) -> Vec<String> {
    HwpxDecoder::decode(bytes).expect("decode").document.sections()[0]
        .paragraphs
        .iter()
        .map(Paragraph::text_content)
        .collect()
}

fn owned(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|t| (*t).to_owned()).collect()
}

/// The object's keys, sorted, so the assertion is on the key **set** rather
/// than on `serde_json`'s map ordering.
fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
    keys.sort();
    keys
}

#[test]
fn inserts_a_block_after_its_anchor() {
    let bytes = three_paragraphs();

    let out = insert_para(
        &bytes,
        &InsertParaOptions::default().with_anchor(1).with_text(owned(&["새 A", "새 B"])),
    )
    .expect("insert");

    assert_eq!((out.inserted, out.deleted), (2, 0));
    assert_eq!(texts(&out.bytes), ["첫째", "둘째", "새 A", "새 B", "셋째"]);
    assert!(
        out.warnings.is_empty(),
        "a cleanly decoding document reports nothing: {:?}",
        out.warnings
    );
}

#[test]
fn inserts_before_the_anchor_when_asked() {
    let out = insert_para(
        &three_paragraphs(),
        &InsertParaOptions::default().with_anchor(1).with_text(owned(&["끼움"])).with_before(true),
    )
    .expect("insert");

    assert_eq!((out.inserted, out.deleted), (1, 0));
    assert_eq!(texts(&out.bytes), ["첫째", "끼움", "둘째", "셋째"]);
}

/// Preservation invariant: the paragraphs that were already there keep their
/// text and their order, and only the requested block appears.
#[test]
fn every_untouched_paragraph_survives_unchanged() {
    let bytes = three_paragraphs();
    let before = texts(&bytes);

    let out =
        insert_para(&bytes, &InsertParaOptions::default().with_anchor(2).with_text(owned(&["끝"])))
            .expect("insert");

    let after = texts(&out.bytes);
    let survivors: Vec<String> = after.iter().filter(|t| *t != "끝").cloned().collect();
    assert_eq!(survivors, before, "the original paragraphs are untouched and in order");
    assert_eq!(after.len(), before.len() + 1);
}

#[test]
fn no_text_is_rejected_rather_than_silently_doing_nothing() {
    let bytes = three_paragraphs();

    let err = insert_para(&bytes, &InsertParaOptions::default().with_anchor(0))
        .expect_err("the library would return the input verbatim");

    assert_eq!(err.code(), OpsCode::InsertTextRequired, "{err}");
    assert_eq!(err.code().as_str(), "INSERT_TEXT_REQUIRED");
    assert!(err.to_string().contains("at least one paragraph text"), "{err}");
}

#[test]
fn a_text_with_a_line_break_is_more_than_one_paragraph() {
    let err = insert_para(
        &three_paragraphs(),
        &InsertParaOptions::default().with_anchor(0).with_text(owned(&["한 줄\n두 줄"])),
    )
    .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::MultiParagraphText, "{err}");
}

#[test]
fn an_anchor_outside_the_document_is_reported_precisely() {
    let bytes = three_paragraphs();
    let text = owned(&["x"]);

    let bad_section =
        insert_para(&bytes, &InsertParaOptions::default().with_section(9).with_text(text.clone()))
            .expect_err("no such section");
    assert_eq!(bad_section.code(), OpsCode::SectionOutOfRange, "{bad_section}");

    let bad_anchor =
        insert_para(&bytes, &InsertParaOptions::default().with_anchor(99).with_text(text))
            .expect_err("no such paragraph");
    assert_eq!(bad_anchor.code(), OpsCode::ParagraphOutOfRange, "{bad_anchor}");
}

#[test]
fn a_non_document_input_is_a_structural_codec_failure() {
    let err =
        insert_para(b"not a zip at all", &InsertParaOptions::default().with_text(owned(&["x"])))
            .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::StructuralCodec, "{err}");
}

#[test]
fn meta_carries_exactly_the_documented_keys() {
    let out = insert_para(
        &three_paragraphs(),
        &InsertParaOptions::default().with_anchor(0).with_text(owned(&["새 문단"])),
    )
    .expect("insert");

    let value = serde_json::to_value(out.meta()).expect("serialise");
    assert_eq!(keys(&value), ["deleted", "inserted", "warnings"]);
    assert_eq!(value["inserted"], 1);
    assert_eq!(value["deleted"], 0);
    assert_eq!(value["warnings"], serde_json::json!([]));
}

/// A package whose decode raises `LAYOUT_CACHE_DROPPED` and which is still
/// admissible.
///
/// It is this codec's own output with the last paragraph's
/// `<hp:linesegarray>` pointing past the end of that paragraph's text — the
/// shape a third-party edit leaves behind — so the decoder refuses to
/// promote a guessed coordinate and reports the drop instead. Being our own
/// output is what keeps it admissible: the two native documents in the tree
/// that warn both fail the gate on uncarried ZIP entries.
///
/// The same fixture backs the decode-warning tests of `fill`, `insert_para`,
/// `delete_para` and `stamp`, so the four surfaces cannot disagree about
/// what "a document that warns" means.
fn warning_fixture() -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/layout/");
    std::fs::read(format!("{path}stale-line-cache.hwpx")).expect("stale-line-cache.hwpx")
}

/// The insert admits its input by decoding it, so the caller hears what that
/// decode reported. Inserting has no advisory scan of its own, so the
/// decoder's list is the whole list.
///
/// The count is part of the contract: the admission gate decodes the
/// re-encoded package as well, and the self-verify decodes the output, but
/// those two are verification artefacts of packages the caller never
/// receives, so only the input decode is reported.
#[test]
fn decode_warnings_reach_the_insert_output() {
    let out = insert_para(
        &warning_fixture(),
        &InsertParaOptions::default().with_anchor(2).with_text(owned(&["끼움"])),
    )
    .expect("insert");

    let codes: Vec<String> = out.meta().warnings.into_iter().map(|w| w.code).collect();
    assert_eq!(codes, ["LAYOUT_CACHE_DROPPED"], "one input decode, reported once: {codes:?}");
    assert_eq!((out.inserted, out.deleted), (1, 0), "the edit itself still happened");
}
