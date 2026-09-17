//! `ops::diff` — the two-channel change report.
#![cfg(feature = "ops-hwpx")]

use std::collections::BTreeSet;

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::ops::diff::diff;

fn fixture(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../hwpforge-smithy-hwpx/tests/fixtures/");
    std::fs::read(format!("{path}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
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
fn a_document_against_itself_is_identical() {
    let bytes = fixture("SimpleTable.hwpx");

    let out = diff(&bytes, &bytes).expect("diff");

    assert!(out.diff.identical, "{:?}", out.diff.semantic);
    assert!(out.diff.package.is_empty());
    assert!(out.diff.semantic.is_empty());
}

#[test]
fn two_different_documents_are_not_identical() {
    let out = diff(&fixture("SimpleTable.hwpx"), &fixture("SimplePicture.hwpx")).expect("diff");

    assert!(!out.diff.identical);
    assert!(!out.diff.package.is_empty(), "the packages differ");
}

#[test]
fn meta_flattens_the_report_and_adds_warnings() {
    let bytes = fixture("SimpleTable.hwpx");
    let out = diff(&bytes, &bytes).expect("diff");

    let value = serde_json::to_value(out.meta()).expect("serialise meta");

    assert_eq!(
        keys(&value),
        ["identical", "note", "semantic", "package", "warnings"]
            .map(String::from)
            .into_iter()
            .collect(),
        "{value}"
    );
    assert_eq!(value["identical"], serde_json::json!(true));
    assert_eq!(value["warnings"], serde_json::json!([]));
}

#[test]
fn the_flattened_payload_matches_the_dto_key_for_key() {
    let out = diff(&fixture("SimpleTable.hwpx"), &fixture("SimplePicture.hwpx")).expect("diff");

    let meta = serde_json::to_value(out.meta()).expect("serialise meta");
    let payload = serde_json::to_value(&out.diff).expect("serialise payload");

    for (key, value) in payload.as_object().expect("DocumentDiff is an object") {
        assert_eq!(&meta[key], value, "{key} was reshaped by the wrapper");
    }
    // `warnings` is the only key the wrapper contributes.
    assert_eq!(keys(&meta).len(), keys(&payload).len() + 1);
}

#[test]
fn this_query_cannot_warn_today() {
    let bytes = fixture("sample1.hwpx");

    let out = diff(&bytes, &bytes).expect("diff");

    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn an_undecodable_base_is_a_decode_failure() {
    let err = diff(b"not a zip", &fixture("SimpleTable.hwpx")).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
    assert_eq!(err.code().as_str(), "DECODE_FAILED");
}

#[test]
fn an_undecodable_revision_is_a_decode_failure_too() {
    let err = diff(&fixture("SimpleTable.hwpx"), b"").expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
}
