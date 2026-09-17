//! `ops::to_json` — the whole-document export.
#![cfg(feature = "ops-hwpx")]

use std::collections::BTreeSet;

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::ops::exchange::{to_json, ToJsonOptions};

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

/// Collects every `addr` object in the tree, so the annotation step can be
/// asserted without hard-coding a path into the document schema.
fn addresses(value: &serde_json::Value) -> Vec<serde_json::Value> {
    let mut found = Vec::new();
    let mut stack = vec![value];
    while let Some(node) = stack.pop() {
        match node {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    if key == "addr" {
                        found.push(child.clone());
                    }
                    stack.push(child);
                }
            }
            serde_json::Value::Array(items) => stack.extend(items),
            _ => {}
        }
    }
    found
}

#[test]
fn styles_are_included_by_default() {
    let out = to_json(&fixture("SimpleTable.hwpx"), &ToJsonOptions::default()).expect("to_json");

    assert!(out.exported.styles.is_some(), "the default must not invert the CLI's --no-styles");
    assert!(out.document.get("styles").is_some(), "{}", out.document);
}

#[test]
fn asking_for_no_styles_drops_the_key_entirely() {
    let out = to_json(&fixture("SimpleTable.hwpx"), &ToJsonOptions::default().with_styles(false))
        .expect("to_json");

    assert!(out.exported.styles.is_none());
    assert!(out.document.get("styles").is_none(), "{}", out.document);
    assert!(out.document.get("document").is_some(), "the document itself stays");
}

/// The annotation step is the reason the export is a `Value` and not just
/// `serde_json::to_value` of the typed tree: cell grid addresses are derived
/// from the span layout and injected afterwards.
#[test]
fn cell_grid_addresses_are_annotated_onto_the_tree() {
    let out = to_json(&fixture("SimpleTable.hwpx"), &ToJsonOptions::default()).expect("to_json");

    let annotated = addresses(&out.document);
    assert!(!annotated.is_empty(), "a table document must carry cell addresses");
    for addr in &annotated {
        assert!(addr.get("row").is_some() && addr.get("col").is_some(), "{addr}");
    }

    // The typed tree is the same content without the addresses, which is why
    // both shapes are returned.
    let plain = serde_json::to_value(&out.exported).expect("serialise typed tree");
    assert!(addresses(&plain).is_empty(), "the typed tree carries no addresses");
    assert!(out.warnings.is_empty(), "every table was addressable: {:?}", out.warnings);
}

#[test]
fn a_document_without_tables_has_nothing_to_annotate() {
    let out = to_json(&fixture("SimplePicture.hwpx"), &ToJsonOptions::default()).expect("to_json");

    assert!(addresses(&out.document).is_empty());
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn meta_keys_are_document_and_warnings() {
    let out = to_json(&fixture("SimpleTable.hwpx"), &ToJsonOptions::default()).expect("to_json");

    let value = serde_json::to_value(out.meta()).expect("serialise meta");

    assert_eq!(keys(&value), ["document", "warnings"].map(String::from).into_iter().collect());
    assert_eq!(value["warnings"], serde_json::json!([]), "{}", value["warnings"]);
}

/// The wire payload must be the **annotated** tree, not the typed one:
/// `from_json` verifies those addresses, so an unannotated payload would
/// silently turn the staleness check off.
#[test]
fn the_meta_payload_is_the_annotated_tree() {
    let out = to_json(&fixture("SimpleTable.hwpx"), &ToJsonOptions::default()).expect("to_json");

    let value = serde_json::to_value(out.meta()).expect("serialise meta");

    assert_eq!(value["document"], out.document);
    assert!(!addresses(&value["document"]).is_empty());
}

#[test]
fn non_hwpx_input_is_a_decode_failure() {
    let err = to_json(b"not a zip at all", &ToJsonOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
    assert_eq!(err.code().as_str(), "DECODE_FAILED");
}

#[test]
fn empty_input_is_a_decode_failure() {
    let err = to_json(&[], &ToJsonOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
}

/// The CLI drops the decoder's warnings on this path and prints only
/// grid-address ones. This operation reports both, so a frontend built on it
/// stops losing them.
#[test]
fn decode_warnings_reach_the_caller() {
    let bytes = repo_fixture("user_samples/sample-text-char-runs-basic.hwpx");

    let out = to_json(&bytes, &ToJsonOptions::default()).expect("to_json");

    let codes: Vec<String> = out.meta().warnings.into_iter().map(|w| w.code).collect();
    assert!(codes.contains(&"LAYOUT_CACHE_DROPPED".to_string()), "{codes:?}");
    assert!(out.document.get("document").is_some(), "a warning does not stop the export");
}
