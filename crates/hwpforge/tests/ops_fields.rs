//! `ops::fields` — click-here field discovery.
#![cfg(feature = "ops-hwpx")]

use std::collections::BTreeSet;

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::ops::read::fields;
use hwpforge::ops::OpsWarning;

fn hwpx_fixture(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../hwpforge-smithy-hwpx/tests/fixtures/");
    std::fs::read(format!("{path}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn repo_fixture(relative: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/");
    std::fs::read(format!("{path}{relative}")).unwrap_or_else(|e| panic!("fixture {relative}: {e}"))
}

fn keys(value: &serde_json::Value) -> BTreeSet<String> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("meta must serialise as an object, got {value}"))
        .keys()
        .cloned()
        .collect()
}

#[test]
fn an_unfilled_field_reports_its_hint_as_the_current_value() {
    let out = fields(&repo_fixture("fields/clickhere_named.hwpx")).expect("fields");

    assert_eq!(out.fields.len(), 1);
    let field = &out.fields[0];
    assert_eq!(field.name.as_deref(), Some("user_email"));
    assert_eq!(field.section, 0);
    assert!(field.fillable);
    assert_eq!(
        field.current, "회사 이메일을 입력하세요",
        "a native unfilled field shows its hint as body text"
    );
    assert_eq!(field.current, field.hint.clone().expect("hint"));
}

#[test]
fn a_filled_field_reports_the_value_instead_of_the_hint() {
    let out = fields(&repo_fixture("fields/clickhere_filled.hwpx")).expect("fields");

    let field = &out.fields[0];
    assert_eq!(field.name.as_deref(), Some("user_email"));
    assert_eq!(field.current, "hanyul.ryu@example.com");
    assert_ne!(Some(&field.current), field.hint.as_ref(), "the hint no longer shows through");
}

#[test]
fn a_document_without_click_here_fields_reports_an_empty_list() {
    let out = fields(&hwpx_fixture("SimpleTable.hwpx")).expect("fields");

    assert!(out.fields.is_empty(), "{:?}", out.fields);
}

/// A date field is not a click-here field, so it must not appear here even
/// though it is a field control in the document.
#[test]
fn only_click_here_fields_are_listed() {
    let out = fields(&repo_fixture("fields/date_field.hwpx")).expect("fields");

    assert!(out.fields.is_empty(), "{:?}", out.fields);
}

#[test]
fn meta_carries_the_fields_and_the_warning_list() {
    let out = fields(&repo_fixture("fields/clickhere_named.hwpx")).expect("fields");

    let value = serde_json::to_value(out.meta()).expect("serialise meta");

    assert_eq!(
        keys(&value),
        ["fields", "warnings"].map(String::from).into_iter().collect(),
        "{value}"
    );
    assert_eq!(value["warnings"], serde_json::json!([]), "a clean decode still declares the key");
}

#[test]
fn the_meta_payload_is_the_library_dto_unchanged() {
    let out = fields(&repo_fixture("fields/clickhere_filled.hwpx")).expect("fields");

    let meta = serde_json::to_value(out.meta()).expect("serialise meta");
    let payload = serde_json::to_value(&out.fields).expect("serialise payload");

    assert_eq!(meta["fields"], payload);
}

#[test]
fn a_clean_document_carries_an_empty_warning_list() {
    let out = fields(&repo_fixture("fields/clickhere_named.hwpx")).expect("fields");

    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

/// `fields` decodes, so it collects the decoder's warnings — and reports them.
///
/// A frontend that only ever sees the wire shape (the Python bindings return
/// nothing else) could not otherwise tell a clean read from one whose layout
/// cache was dropped.
#[test]
fn a_dropped_layout_cache_reaches_both_the_output_and_the_wire_shape() {
    let bytes = repo_fixture("layout/stale-line-cache.hwpx");

    let out = fields(&bytes).expect("fields");

    assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
    assert!(
        matches!(out.warnings[0], OpsWarning::Decode(_)),
        "a decode warning, not a re-encode one: {:?}",
        out.warnings[0]
    );
    assert_eq!(out.warnings[0].info().code, "LAYOUT_CACHE_DROPPED");

    let value = serde_json::to_value(out.meta()).expect("serialise meta");
    assert_eq!(
        value["warnings"],
        serde_json::json!([serde_json::to_value(out.warnings[0].info()).expect("warning")]),
        "the wire shape carries the same list the output does: {value}"
    );
}

#[test]
fn non_hwpx_input_is_a_decode_failure() {
    let err = fields(b"not a zip at all").expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
    assert_eq!(err.code().as_str(), "DECODE_FAILED");
}
