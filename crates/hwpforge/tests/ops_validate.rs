//! `ops::style::validate` — a verdict, not a failure.
//!
//! The distinction this file pins: "the document is invalid" is the answer
//! the caller asked for and comes back as `ok: false`; "these bytes are not
//! a document" is an error, because there was nothing to ask about.
#![cfg(feature = "ops-hwpx")]

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::ops::inspect::{inspect, InspectOptions};
use hwpforge::ops::style::validate;

fn fixture(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../hwpforge-smithy-hwpx/tests/fixtures/");
    std::fs::read(format!("{path}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

#[test]
fn a_sound_document_passes_with_no_errors() {
    let out = validate(&fixture("SimpleTable.hwpx")).expect("validate");

    assert!(out.ok, "{:?}", out.errors);
    assert!(out.errors.is_empty());
}

#[test]
fn every_shipped_fixture_validates() {
    // A fixture that stopped validating would silently weaken every other
    // test that decodes it.
    for name in ["SimpleTable.hwpx", "SimplePicture.hwpx", "charts.hwpx", "sample1.hwpx"] {
        let out = validate(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(out.ok, "{name}: {:?}", out.errors);
    }
}

#[test]
fn meta_carries_exactly_the_five_documented_keys() {
    let out = validate(&fixture("SimpleTable.hwpx")).expect("validate");

    let value = serde_json::to_value(out.meta()).expect("serialise");
    let mut fields: Vec<&str> =
        value.as_object().expect("object").keys().map(String::as_str).collect();
    fields.sort_unstable();
    assert_eq!(fields, ["errors", "ok", "paragraphs", "sections", "warnings"]);
    assert_eq!(value["ok"], true);
    assert!(value["errors"].as_array().expect("array").is_empty());
    assert!(value["sections"].is_u64());
    assert!(value["paragraphs"].is_u64());
}

#[test]
fn the_counts_spare_a_frontend_a_second_decode() {
    // These are top-level counts, the same numbers the MCP validate tool
    // reports today — deliberately not `inspect`'s deep traversal, which
    // also descends into table cells, notes and headers.
    let out = validate(&fixture("SimpleTable.hwpx")).expect("validate");
    let deep = inspect(&fixture("SimpleTable.hwpx"), &InspectOptions::default()).expect("inspect");

    assert_eq!(out.sections, deep.report.sections, "section count is not a traversal question");
    assert_eq!(
        out.paragraphs,
        deep.report.section_details.iter().map(|s| s.top_level_paragraphs).sum::<usize>(),
        "validate reports body-flow paragraphs"
    );
    assert!(
        out.paragraphs <= deep.report.paragraphs,
        "the deep count can only be larger: {} vs {}",
        out.paragraphs,
        deep.report.paragraphs
    );
}

#[test]
fn undecodable_bytes_are_an_error_not_a_verdict() {
    // There is no document to judge, so `ok: false` would be a lie about
    // *a* document rather than a report about these bytes.
    let err = validate(b"not a zip at all").expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
}

#[test]
fn empty_input_is_a_decode_failure() {
    let err = validate(&[]).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
}
