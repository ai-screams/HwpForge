//! `ops::edit::delete_para` — the preserving structural delete (E4).
#![cfg(feature = "ops-hwpx")]

use hwpforge::core::control::Control;
use hwpforge::core::image::ImageStore;
use hwpforge::core::run::Run;
use hwpforge::core::{Document, Draft, PageSettings, Paragraph, Section};
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge::hwpx::{HwpxDecoder, HwpxEncoder};
use hwpforge::ops::edit::{delete_para, DeleteParaOptions};

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

/// One section holding four named paragraphs.
fn four_paragraphs() -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![text_para("첫째"), text_para("둘째"), text_para("셋째"), text_para("넷째")],
        PageSettings::a4(),
    ));
    encode(doc)
}

/// Four paragraphs where the second one also carries an index mark.
fn with_index_mark() -> Vec<u8> {
    let mut marked = text_para("색인 문단");
    marked.add_run(Run::control(
        Control::IndexMark { primary: "색인어".to_owned(), secondary: None },
        CharShapeIndex::new(0),
    ));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![text_para("첫째"), marked, text_para("셋째"), text_para("넷째")],
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

/// The object's keys, sorted, so the assertion is on the key **set** rather
/// than on `serde_json`'s map ordering.
fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
    keys.sort();
    keys
}

#[test]
fn deletes_one_paragraph_and_leaves_the_rest_in_order() {
    let bytes = four_paragraphs();

    let out =
        delete_para(&bytes, &DeleteParaOptions::default().with_indexes(vec![2])).expect("delete");

    assert_eq!((out.inserted, out.deleted), (0, 1));
    assert_eq!(texts(&out.bytes), ["첫째", "둘째", "넷째"]);
    assert!(out.warnings.is_empty(), "a plain paragraph has nothing to advise about");
}

/// The batch resolves every index against one pristine snapshot, so the
/// second target does not shift when the first one is removed.
#[test]
fn a_batch_resolves_its_indexes_against_the_original_document() {
    let out =
        delete_para(&four_paragraphs(), &DeleteParaOptions::default().with_indexes(vec![1, 3]))
            .expect("delete");

    assert_eq!(out.deleted, 2);
    assert_eq!(texts(&out.bytes), ["첫째", "셋째"]);
}

/// Deleting a paragraph takes its index-mark entries out of the document
/// index. That is intended, so the library advises rather than refusing, and
/// the advisory has to reach the caller.
#[test]
fn deleting_an_index_mark_paragraph_says_what_left_the_index() {
    let out = delete_para(&with_index_mark(), &DeleteParaOptions::default().with_indexes(vec![1]))
        .expect("delete");

    assert_eq!(out.deleted, 1);
    assert_eq!(texts(&out.bytes), ["첫째", "셋째", "넷째"]);
    assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
    let info = out.warnings[0].info();
    assert_eq!(info.code, "INDEX_MARK_REMOVED");
    assert!(info.message.contains("index-mark entry"), "{}", info.message);

    let value = serde_json::to_value(out.meta()).expect("serialise");
    assert_eq!(value["warnings"][0]["code"], "INDEX_MARK_REMOVED");
}

/// A paragraph with nothing to advise about produces no warnings at all, so
/// the advisory channel cannot be mistaken for noise on every delete.
#[test]
fn deleting_a_plain_paragraph_advises_nothing() {
    let out = delete_para(&with_index_mark(), &DeleteParaOptions::default().with_indexes(vec![2]))
        .expect("delete");

    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    assert_eq!(texts(&out.bytes), ["첫째", "색인 문단", "넷째"]);
}

/// The advisory runs before the edit, so a batch that both warns and fails
/// must still come back as the failure with no bytes. An advisory can never
/// stand in for a refusal.
#[test]
fn a_batch_that_would_warn_and_fail_reports_only_the_failure() {
    let err =
        delete_para(&with_index_mark(), &DeleteParaOptions::default().with_indexes(vec![1, 99]))
            .expect_err("the out-of-range target refuses the whole batch");

    assert_eq!(err.code(), OpsCode::ParagraphOutOfRange, "{err}");
}

#[test]
fn no_target_is_rejected_rather_than_silently_doing_nothing() {
    let err = delete_para(&four_paragraphs(), &DeleteParaOptions::default())
        .expect_err("the library would return the input verbatim");

    assert_eq!(err.code(), OpsCode::DeleteNoTarget, "{err}");
    assert_eq!(err.code().as_str(), "DELETE_NO_TARGET");
    assert!(err.to_string().contains("at least one paragraph index"), "{err}");
}

#[test]
fn the_same_index_twice_is_a_duplicate_target() {
    let err =
        delete_para(&four_paragraphs(), &DeleteParaOptions::default().with_indexes(vec![1, 1]))
            .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DuplicateTarget, "{err}");
}

#[test]
fn a_target_outside_the_document_is_reported_precisely() {
    let bytes = four_paragraphs();

    let bad_section =
        delete_para(&bytes, &DeleteParaOptions::default().with_section(9).with_indexes(vec![0]))
            .expect_err("no such section");
    assert_eq!(bad_section.code(), OpsCode::SectionOutOfRange, "{bad_section}");

    let bad_index = delete_para(&bytes, &DeleteParaOptions::default().with_indexes(vec![99]))
        .expect_err("no such paragraph");
    assert_eq!(bad_index.code(), OpsCode::ParagraphOutOfRange, "{bad_index}");
}

#[test]
fn a_non_document_input_is_a_structural_codec_failure() {
    let err = delete_para(b"not a zip at all", &DeleteParaOptions::default().with_indexes(vec![0]))
        .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::StructuralCodec, "{err}");
}

#[test]
fn meta_carries_exactly_the_documented_keys() {
    let out = delete_para(&four_paragraphs(), &DeleteParaOptions::default().with_indexes(vec![1]))
        .expect("delete");

    let value = serde_json::to_value(out.meta()).expect("serialise");
    assert_eq!(keys(&value), ["deleted", "inserted", "warnings"]);
    assert_eq!(value["deleted"], 1);
    assert_eq!(value["inserted"], 0);
    assert_eq!(value["warnings"], serde_json::json!([]));
}
