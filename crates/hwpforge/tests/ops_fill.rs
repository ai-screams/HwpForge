//! `ops::edit::fill` — the preserving, name-addressed delta edit.
#![cfg(feature = "ops-hwpx")]

use hwpforge::core::control::Control;
use hwpforge::core::image::ImageStore;
use hwpforge::core::run::{Run, RunContent};
use hwpforge::core::{Document, Draft, PageSettings, Paragraph, Section};
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::foundation::{CharShapeIndex, FieldType, ParaShapeIndex};
use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge::hwpx::{HwpxDecoder, HwpxEncoder};
use hwpforge::ops::edit::{fill, FillOptions};
use hwpforge::ops::{inspect, InspectOptions};

/// A native Hancom document with one named, unfilled click-here field.
fn named_fixture() -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/fields/");
    std::fs::read(format!("{path}clickhere_named.hwpx")).expect("clickhere_named.hwpx")
}

fn named_field(name: &str, hint: &str) -> Control {
    Control::Field {
        field_type: FieldType::ClickHere,
        hint_text: Some(hint.to_owned()),
        help_text: None,
        name: Some(name.to_owned()),
        display_text: String::new(),
    }
}

/// Encodes one section per entry, each holding the given fields.
fn build(sections: &[Vec<Control>]) -> Vec<u8> {
    let mut doc = Document::new();
    for controls in sections {
        let paragraphs = controls
            .iter()
            .map(|control| {
                Paragraph::with_runs(
                    vec![Run::control(control.clone(), CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )
            })
            .collect();
        doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
    }
    encode(doc)
}

fn encode(doc: Document<Draft>) -> Vec<u8> {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
        .expect("encode")
}

/// The body text of the named field in `section`.
fn body(bytes: &[u8], section: usize, name: &str) -> String {
    HwpxDecoder::decode(bytes).expect("decode").document.sections()[section]
        .paragraphs
        .iter()
        .flat_map(|p| p.runs.iter())
        .find_map(|run| match &run.content {
            RunContent::Control(control) => match control.as_ref() {
                Control::Field { name: n, display_text, .. } if n.as_deref() == Some(name) => {
                    Some(display_text.clone())
                }
                _ => None,
            },
            _ => None,
        })
        .unwrap_or_else(|| panic!("field '{name}' not in section {section}"))
}

fn pairs(values: &[(&str, &str)]) -> Vec<(String, String)> {
    values.iter().map(|(n, v)| ((*n).to_owned(), (*v).to_owned())).collect()
}

/// The object's keys, sorted, so the assertion is on the key **set** rather
/// than on `serde_json`'s map ordering.
fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
    keys.sort();
    keys
}

#[test]
fn fills_a_named_field_and_reports_what_it_replaced() {
    let bytes = named_fixture();

    let out =
        fill(&bytes, &pairs(&[("user_email", "hanyul@example.com")]), &FillOptions::default())
            .expect("fill");

    assert_eq!(out.filled.len(), 1);
    assert_eq!(out.filled[0].name, "user_email");
    assert_eq!(out.filled[0].section, 0);
    assert_eq!(out.filled[0].previous, "회사 이메일을 입력하세요", "the hint was the body");
    assert_eq!(body(&out.bytes, 0, "user_email"), "hanyul@example.com");
    assert!(out.warnings.is_empty(), "the filler has no warning channel yet");
}

#[test]
fn fills_every_section_in_one_batch() {
    let bytes = build(&[
        vec![named_field("과제명", "과제명을 입력하세요")],
        vec![named_field("총연구비", "금액을 입력하세요")],
    ]);

    let out = fill(
        &bytes,
        &pairs(&[("과제명", "AI 문서 자동화"), ("총연구비", "300,000천원")]),
        &FillOptions::default(),
    )
    .expect("fill");

    assert_eq!(out.filled.len(), 2);
    assert_eq!(body(&out.bytes, 0, "과제명"), "AI 문서 자동화");
    assert_eq!(body(&out.bytes, 1, "총연구비"), "300,000천원");
}

/// Preservation invariant, at the Core level: filling back the reported
/// `previous` restores the original body, and the document's structure never
/// moves in either direction. A regenerating edit would fail this by
/// reflowing the package.
///
/// The round trip is **not** byte-identical — the patcher rewrites the ZIP
/// container even when the section XML comes back to the same text — so this
/// asserts decoded equality, the same level `fill_api.rs` asserts. Wire-level
/// damage that survives a Core comparison is out of reach here; the
/// fail-closed encode gate is what guards that, and only regenerating edits
/// run one.
#[test]
fn filling_back_the_previous_value_restores_the_document() {
    let bytes = named_fixture();
    let before = inspect(&bytes, &InspectOptions::default()).expect("inspect").report;

    let forward =
        fill(&bytes, &pairs(&[("user_email", "x@y.z")]), &FillOptions::default()).expect("forward");
    let previous = forward.filled[0].previous.clone();
    let back = fill(&forward.bytes, &pairs(&[("user_email", &previous)]), &FillOptions::default())
        .expect("reverse");

    assert_eq!(body(&back.bytes, 0, "user_email"), previous, "the body is back where it started");
    assert_eq!(back.filled[0].previous, "x@y.z", "the reverse edit reports the forward value");
    for (stage, bytes) in [("forward", &forward.bytes), ("back", &back.bytes)] {
        let after = inspect(bytes, &InspectOptions::default()).expect("inspect").report;
        assert_eq!(after.sections, before.sections, "{stage}");
        assert_eq!(after.paragraphs, before.paragraphs, "{stage}");
        assert_eq!(after.tables, before.tables, "{stage}");
        assert_eq!(after.fields, before.fields, "{stage}");
    }
}

#[test]
fn no_values_is_rejected_before_the_document_is_touched() {
    let err =
        fill(b"not a document at all", &[], &FillOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::NoValues, "{err}");
    assert_eq!(err.code().as_str(), "NO_VALUES");
    assert!(err.to_string().contains("no field values"), "{err}");
}

#[test]
fn a_name_given_twice_is_rejected_rather_than_silently_collapsed() {
    let bytes = named_fixture();

    let err = fill(
        &bytes,
        &pairs(&[("user_email", "a@b.c"), ("user_email", "d@e.f")]),
        &FillOptions::default(),
    )
    .expect_err("a map cannot hold both");

    assert_eq!(err.code(), OpsCode::InvalidInput, "no dedicated code: {err}");
    assert!(err.to_string().contains("more than once"), "{err}");
}

#[test]
fn an_unknown_field_name_is_reported_with_the_library_code() {
    let err = fill(&named_fixture(), &pairs(&[("없는이름", "x")]), &FillOptions::default())
        .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::FieldNotFound, "{err}");
    assert_eq!(err.code().as_str(), "FIELD_NOT_FOUND");
    assert!(err.to_string().contains("user_email"), "the message lists what is available: {err}");
}

#[test]
fn an_empty_value_is_refused_because_it_is_the_hint_sentinel() {
    let err = fill(&named_fixture(), &pairs(&[("user_email", "")]), &FillOptions::default())
        .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::EmptyFieldValue, "{err}");
    assert!(err.hint().is_some(), "the CLI hint explains the sentinel");
}

#[test]
fn a_name_that_appears_twice_in_the_document_is_ambiguous() {
    let bytes = build(&[vec![named_field("이름", "성명 입력"), named_field("이름", "성명 입력")]]);

    let err = fill(&bytes, &pairs(&[("이름", "류한율")]), &FillOptions::default())
        .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::FieldNameAmbiguous, "{err}");
}

#[test]
fn a_non_document_input_is_a_fill_workflow_failure() {
    let err = fill(b"not a zip at all", &pairs(&[("a", "b")]), &FillOptions::default())
        .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::FillFailed, "{err}");
}

#[test]
fn meta_carries_exactly_the_documented_keys() {
    let out = fill(&named_fixture(), &pairs(&[("user_email", "x@y.z")]), &FillOptions::default())
        .expect("fill");

    let value = serde_json::to_value(out.meta()).expect("serialise");
    assert_eq!(keys(&value), ["filled", "warnings"]);
    assert_eq!(value["filled"][0]["name"], "user_email");
    assert_eq!(value["warnings"], serde_json::json!([]));
}
