//! `ops::edit::insert_para` — the preserving structural insert (E4).
#![cfg(feature = "ops-hwpx")]

#[path = "support/document.rs"]
mod document;
use document::{encode, text_para, texts};
#[path = "support/fixtures.rs"]
mod fixtures;
use fixtures::warning_fixture;
#[path = "support/json.rs"]
mod json;
use hwpforge::core::{Document, PageSettings, Section};
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::ops::edit::{insert_para, InsertParaOptions};
use json::sorted_keys as keys;

/// One section holding three named paragraphs.
fn three_paragraphs() -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![text_para("첫째"), text_para("둘째"), text_para("셋째")],
        PageSettings::a4(),
    ));
    encode(doc)
}

fn owned(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|t| (*t).to_owned()).collect()
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
    // 이것을 실패시키는 것: T2 해당 입력 decode stage를 Encode로 변경.
    let err =
        insert_para(b"not a zip at all", &InsertParaOptions::default().with_text(owned(&["x"])))
            .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
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

/// A text holding a character XML cannot carry is inserted without it —
/// it used to fail the self-verify — and the removal is reported.
#[test]
fn a_forbidden_character_in_a_text_is_removed_and_reported() {
    // 이것을 실패시키는 것: `insert_para` 가 `encode_warnings` 를 `warnings` 에 합치지 않는 것.
    let bytes = three_paragraphs();

    let out = insert_para(
        &bytes,
        &InsertParaOptions::default()
            .with_section(0)
            .with_anchor(0)
            .with_text(owned(&["새\u{1}글"])),
    )
    .expect("insert_para");

    assert_eq!(texts(&out.bytes), owned(&["첫째", "새글", "둘째", "셋째"]));
    let codes: Vec<(String, String)> =
        out.meta().warnings.into_iter().map(|w| (w.code, w.message)).collect();
    assert_eq!(
        codes,
        [(
            "XML_FORBIDDEN_CHARS_REMOVED".to_string(),
            "removed 1 XML-forbidden character(s) from new paragraph 1 in Contents/section0.xml"
                .to_string()
        )]
    );
}
