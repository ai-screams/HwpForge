//! `InspectMeta` — the serde wire wrapper around the `inspect` report.
//!
//! The key set is the contract the Python stub declares, so it is asserted as
//! an equality against a literal list rather than a "contains" check.
#![cfg(feature = "ops-hwpx")]

use std::collections::BTreeSet;

use hwpforge::ops::{inspect, InspectOptions};

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

fn expected(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn meta_keys_are_the_report_fields_flattened_plus_warnings() {
    let out = inspect(&fixture("SimpleTable.hwpx"), &InspectOptions::default()).expect("inspect");

    let value = serde_json::to_value(out.meta()).expect("serialise meta");

    assert_eq!(
        keys(&value),
        expected(&[
            "metadata",
            "sections",
            "paragraphs",
            "tables",
            "images",
            "charts",
            "fields",
            "section_details",
            "warnings",
        ]),
        "{value}"
    );
}

#[test]
fn styles_is_the_one_conditional_key() {
    let bytes = fixture("SimpleTable.hwpx");

    let plain =
        serde_json::to_value(inspect(&bytes, &InspectOptions::default()).expect("inspect").meta())
            .expect("serialise");
    let styled = serde_json::to_value(
        inspect(&bytes, &InspectOptions::default().with_styles(true)).expect("inspect").meta(),
    )
    .expect("serialise");

    assert!(plain.get("styles").is_none(), "skip_serializing_if survives the flatten: {plain}");
    assert!(styled.get("styles").is_some(), "{styled}");
    // Nothing else moves with the flag.
    assert_eq!(
        keys(&styled).difference(&keys(&plain)).cloned().collect::<Vec<_>>(),
        vec!["styles".to_string()]
    );
}

#[test]
fn the_flattened_report_keeps_its_own_values() {
    let out = inspect(&fixture("SimpleTable.hwpx"), &InspectOptions::default()).expect("inspect");

    let value = serde_json::to_value(out.meta()).expect("serialise meta");

    assert_eq!(value["sections"], serde_json::json!(out.report.sections));
    assert_eq!(value["tables"], serde_json::json!(out.report.tables));
    assert_eq!(value["metadata"]["title"], serde_json::json!(out.report.metadata.title));
}

#[test]
fn a_clean_decode_reports_an_empty_warning_list() {
    let out = inspect(&fixture("SimpleTable.hwpx"), &InspectOptions::default()).expect("inspect");

    let value = serde_json::to_value(out.meta()).expect("serialise meta");

    // Always a list, never absent: the stub declares `list[WarningInfo]`.
    assert_eq!(value["warnings"], serde_json::json!([]), "{value}");
}

#[test]
fn meta_borrows_nothing_and_can_outlive_the_output() {
    let meta = {
        let out =
            inspect(&fixture("SimpleTable.hwpx"), &InspectOptions::default()).expect("inspect");
        out.meta()
    };

    assert_eq!(meta.report.sections, 1);
}
