//! `ops::export_section` — the section export that `patch` expects back.
#![cfg(feature = "ops-hwpx")]

use std::collections::BTreeSet;

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::ops::exchange::{export_section, to_json, ExportSectionOptions, ToJsonOptions};

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

fn address_count(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Object(map) => {
            map.iter().map(|(key, child)| usize::from(key == "addr") + address_count(child)).sum()
        }
        serde_json::Value::Array(items) => items.iter().map(address_count).sum(),
        _ => 0,
    }
}

#[test]
fn the_default_exports_the_first_section_with_styles() {
    let out =
        export_section(&fixture("SimpleTable.hwpx"), &ExportSectionOptions::default()).expect("ok");

    assert_eq!(out.exported.section_index, 0);
    assert!(out.exported.styles.is_some(), "styles are on by default, as in `to_json`");
    assert!(out.section.get("preservation").is_some(), "this section can be patched back");
    assert!(
        out.warnings.is_empty(),
        "no preservation warning for this section: {:?}",
        out.warnings
    );
}

#[test]
fn asking_for_no_styles_drops_the_key() {
    let out = export_section(
        &fixture("SimpleTable.hwpx"),
        &ExportSectionOptions::default().with_styles(false),
    )
    .expect("ok");

    assert!(out.exported.styles.is_none());
    assert!(out.section.get("styles").is_none(), "{}", out.section);
    assert!(out.section.get("section").is_some(), "the section itself stays");
}

#[test]
fn cell_grid_addresses_are_annotated_onto_the_section() {
    let out =
        export_section(&fixture("SimpleTable.hwpx"), &ExportSectionOptions::default()).expect("ok");

    assert!(address_count(&out.section) > 0, "a table section must carry cell addresses");
    let plain = serde_json::to_value(&out.exported).expect("serialise typed tree");
    assert_eq!(address_count(&plain), 0, "the typed tree carries no addresses");
    assert!(out.warnings.is_empty(), "every table was addressable: {:?}", out.warnings);
}

#[test]
fn meta_keys_are_section_and_warnings() {
    let out =
        export_section(&fixture("SimpleTable.hwpx"), &ExportSectionOptions::default()).expect("ok");

    let value = serde_json::to_value(out.meta()).expect("serialise meta");

    assert_eq!(keys(&value), ["section", "warnings"].map(String::from).into_iter().collect());
    assert_eq!(value["warnings"], serde_json::json!([]));
    assert_eq!(value["section"], out.section, "the payload is the annotated tree");
}

#[test]
fn a_section_outside_the_document_is_refused() {
    let err = export_section(
        &fixture("SimpleTable.hwpx"),
        &ExportSectionOptions::default().with_section(99),
    )
    .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::SectionOutOfRange, "{err}");
    assert_eq!(err.code().as_str(), "SECTION_OUT_OF_RANGE");
}

#[test]
fn non_hwpx_input_is_a_decode_failure() {
    let err = export_section(b"not a zip at all", &ExportSectionOptions::default())
        .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
}

/// The export still succeeds when preservation metadata cannot be built, but
/// it must say so: without that metadata the section can only be rebuilt, not
/// patched back, and a caller who edits it and calls `patch` would be told
/// nothing until the patch failed.
#[test]
fn a_section_without_preservation_metadata_warns_instead_of_failing() {
    let bytes = repo_fixture("mixed/mixed_01_image_and_chart_same_doc.hwpx");

    let out = export_section(&bytes, &ExportSectionOptions::default()).expect("export still works");

    let codes: Vec<String> = out.meta().warnings.into_iter().map(|w| w.code).collect();
    assert_eq!(codes, ["PRESERVATION_METADATA_UNAVAILABLE"], "{codes:?}");
    assert!(out.section.get("section").is_some(), "the section itself is still exported");
}

/// The warning reaches the wire payload, not just the Rust output.
#[test]
fn the_preservation_warning_reaches_the_meta() {
    let bytes = repo_fixture("mixed/mixed_01_image_and_chart_same_doc.hwpx");
    let out = export_section(&bytes, &ExportSectionOptions::default()).expect("export");

    let value = serde_json::to_value(out.meta()).expect("serialise meta");

    assert_eq!(keys(&value), ["section", "warnings"].map(String::from).into_iter().collect());
    assert_eq!(
        value["warnings"][0]["code"],
        serde_json::json!("PRESERVATION_METADATA_UNAVAILABLE")
    );
    assert!(
        value["warnings"][0]["message"].as_str().is_some_and(|m| !m.is_empty()),
        "a frontend prints this: {}",
        value["warnings"][0]
    );
}

/// `export_section` reports decode warnings, exactly as `to_json` does.
///
/// The phase-2 design says both exports unify their warning reporting. The
/// section workflow used to decode internally and drop the list, so this
/// surface silently disagreed with `to_json` on the same document.
#[test]
fn decode_warnings_reach_the_caller_as_they_do_for_to_json() {
    let bytes = repo_fixture("user_samples/sample-text-char-runs-basic.hwpx");

    let out = export_section(&bytes, &ExportSectionOptions::default()).expect("export_section");

    let codes: Vec<String> = out.meta().warnings.into_iter().map(|w| w.code).collect();
    assert!(!codes.is_empty(), "a decoding export must report what the decode found");
    assert!(codes.contains(&"LAYOUT_CACHE_DROPPED".to_string()), "{codes:?}");

    // The two exports must agree about the decode. `export_section` also
    // carries its own workflow warning, which `to_json` has no notion of, so
    // the comparison is over the decode-warning subset.
    let whole = to_json(&bytes, &ToJsonOptions::default()).expect("to_json");
    let whole_codes: Vec<String> = whole.meta().warnings.into_iter().map(|w| w.code).collect();
    let decode_only: Vec<String> =
        codes.iter().filter(|c| *c == "LAYOUT_CACHE_DROPPED").cloned().collect();
    assert_eq!(decode_only, whole_codes, "the two exports must report the same decode");
}

/// The documented merge order is decoder → workflow → grid.
#[test]
fn decoder_warnings_come_before_the_workflow_warning() {
    let bytes = repo_fixture("user_samples/sample-text-char-runs-basic.hwpx");

    let out = export_section(&bytes, &ExportSectionOptions::default()).expect("export_section");

    let codes: Vec<String> = out.meta().warnings.into_iter().map(|w| w.code).collect();
    let decoder_at = codes.iter().position(|c| c == "LAYOUT_CACHE_DROPPED");
    let workflow_at = codes.iter().position(|c| c == "PRESERVATION_METADATA_UNAVAILABLE");
    assert!(decoder_at.is_some(), "{codes:?}");
    if let (Some(decoder), Some(workflow)) = (decoder_at, workflow_at) {
        assert!(decoder < workflow, "decoder warnings come first: {codes:?}");
    }
}
