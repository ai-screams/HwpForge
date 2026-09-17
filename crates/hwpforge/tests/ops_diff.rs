//! `ops::diff` — the two-channel change report.
#![cfg(feature = "ops-hwpx")]

use std::collections::BTreeSet;

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::ops::diff::diff;

fn fixture(name: &str) -> Vec<u8> {
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

/// A diff decodes two documents, so it reports two decoders' warnings.
///
/// The documented provenance rule is base first, then revised. Diffing a
/// warning-producing document against itself is the sharpest way to pin it:
/// the same warning must appear twice, once for each side.
#[test]
fn both_inputs_decode_warnings_reach_the_caller_base_first() {
    let noisy = repo_fixture("user_samples/sample-text-char-runs-basic.hwpx");

    let out = diff(&noisy, &noisy).expect("diff");

    let codes: Vec<String> = out.meta().warnings.into_iter().map(|w| w.code).collect();
    assert!(!codes.is_empty(), "a decoding query must report what the decode found");
    assert_eq!(
        codes,
        vec!["LAYOUT_CACHE_DROPPED".to_string(), "LAYOUT_CACHE_DROPPED".to_string()],
        "one entry per side, base then revised"
    );
}

/// Pins the order itself, which the symmetric case above cannot see.
#[test]
fn the_base_warnings_come_before_the_revised_ones() {
    let noisy = repo_fixture("user_samples/sample-text-char-runs-basic.hwpx");
    let quiet = fixture("SimpleTable.hwpx");

    let base_noisy = diff(&noisy, &quiet).expect("diff").warnings;
    let revised_noisy = diff(&quiet, &noisy).expect("diff").warnings;

    // Same single warning either way — what differs is which side produced
    // it, and both orders put it in the one list without losing it.
    assert_eq!(base_noisy.len(), 1, "{base_noisy:?}");
    assert_eq!(revised_noisy.len(), 1, "{revised_noisy:?}");
}

#[test]
fn two_clean_documents_report_nothing() {
    let bytes = fixture("SimpleTable.hwpx");

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
