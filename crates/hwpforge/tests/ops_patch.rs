//! `ops::patch` — the preserving section replacement.
//!
//! Preservation is the whole point, so it is asserted directly: patching a
//! section back unchanged must leave the document semantically identical,
//! which is what makes the operation safe on a document whose layout matters.
#![cfg(feature = "ops-hwpx")]

use std::collections::BTreeSet;

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::ops::diff::diff;
use hwpforge::ops::exchange::{export_section, patch, ExportSectionOptions, PatchOptions};

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

/// The annotated section export of `name`, as JSON text.
fn exported_section_text(name: &str, section: usize) -> String {
    let out =
        export_section(&fixture(name), &ExportSectionOptions::default().with_section(section))
            .expect("export_section");
    serde_json::to_string(&out.section).expect("serialise")
}

#[test]
fn an_unedited_patch_round_trips() {
    let bytes = fixture("SimpleEdit.hwpx");
    let opts = PatchOptions::default().with_patch(exported_section_text("SimpleEdit.hwpx", 0));

    let out = patch(&bytes, &opts).expect("patch");

    assert_eq!(out.section, 0);
    assert_eq!(out.sections, 1);
    assert!(!out.bytes.is_empty());
}

/// A preserving patch that changes nothing must leave the document
/// semantically identical — this is what makes it safe to use for a narrow
/// edit on a document whose layout matters.
#[test]
fn an_unedited_patch_preserves_the_document() {
    let bytes = fixture("SimpleEdit.hwpx");
    let opts = PatchOptions::default().with_patch(exported_section_text("SimpleEdit.hwpx", 0));

    let patched = patch(&bytes, &opts).expect("patch").bytes;

    let report = diff(&bytes, &patched).expect("diff").diff;
    assert!(report.semantic.is_empty(), "nothing changed semantically: {:?}", report.semantic);
}

#[test]
fn meta_keys_are_section_and_warnings() {
    let bytes = fixture("SimpleEdit.hwpx");
    let opts = PatchOptions::default().with_patch(exported_section_text("SimpleEdit.hwpx", 0));

    let value =
        serde_json::to_value(patch(&bytes, &opts).expect("patch").meta()).expect("serialise meta");

    assert_eq!(keys(&value), ["section", "warnings"].map(String::from).into_iter().collect());
    assert_eq!(value["section"], serde_json::json!(0));
}

/// A preserving patch has no encoder, but it does decode the base.
///
/// The `warnings` channel must carry whatever the library produced on the
/// path; for `patch` that is the base decode, which used to be dropped and
/// the wrapper hard-coded to empty. `SimpleEdit.hwpx` is a committed fixture
/// whose decode drops a layout cache, so the loss is observable rather than
/// theoretical.
#[test]
fn the_base_decode_warnings_reach_the_caller() {
    let bytes = fixture("SimpleEdit.hwpx");
    let opts = PatchOptions::default().with_patch(exported_section_text("SimpleEdit.hwpx", 0));

    let out = patch(&bytes, &opts).expect("patch");

    let codes: Vec<String> = out.meta().warnings.into_iter().map(|w| w.code).collect();
    assert!(!codes.is_empty(), "a patch that decodes must report what the decode found");
    assert!(codes.contains(&"LAYOUT_CACHE_DROPPED".to_string()), "{codes:?}");
}

/// The other half: a clean base means a genuinely empty list.
#[test]
fn a_clean_base_reports_nothing() {
    let bytes = fixture("SimpleTable.hwpx");
    let opts = PatchOptions::default().with_patch(exported_section_text("SimpleTable.hwpx", 0));

    let out = patch(&bytes, &opts).expect("patch");

    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn a_patch_that_is_not_json_is_a_parse_failure() {
    let opts = PatchOptions::default().with_patch("{ not json");

    let err = patch(&fixture("SimpleEdit.hwpx"), &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::JsonParseFailed, "{err}");
    assert_eq!(err.code().as_str(), "JSON_PARSE_FAILED");
}

#[test]
fn a_patch_that_is_not_an_exported_section_is_a_parse_failure() {
    let opts = PatchOptions::default().with_patch(r#"{"unexpected": true}"#);

    let err = patch(&fixture("SimpleEdit.hwpx"), &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::JsonParseFailed, "{err}");
}

/// Writing section 0's content over section 1 would silently destroy a
/// section, so the index carried by the JSON has to agree with the request.
#[test]
fn a_section_index_that_disagrees_with_the_json_is_refused() {
    let opts = PatchOptions::default()
        .with_section(1)
        .with_patch(exported_section_text("SimpleEdit.hwpx", 0));

    let err = patch(&fixture("SimpleEdit.hwpx"), &opts).expect_err("must reject");

    assert!(
        matches!(err.code(), OpsCode::SectionIndexMismatch | OpsCode::SectionOutOfRange),
        "{err} reported {}",
        err.code().as_str()
    );
}

#[test]
fn a_stale_grid_address_in_the_patch_is_refused() {
    let mut value: serde_json::Value =
        serde_json::from_str(&exported_section_text("SimpleTable.hwpx", 0)).expect("parse");
    let mut patched = 0usize;
    corrupt_first_address(&mut value, &mut patched);
    assert_eq!(patched, 1, "the fixture must have an address to corrupt");

    let opts = PatchOptions::default().with_patch(value.to_string());
    let err = patch(&fixture("SimpleTable.hwpx"), &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::GridAddrInvalid, "{err}");
}

fn corrupt_first_address(value: &mut serde_json::Value, patched: &mut usize) {
    if *patched > 0 {
        return;
    }
    match value {
        serde_json::Value::Object(map) => {
            if let Some(addr) = map.get_mut("addr") {
                *addr = serde_json::json!({ "row": 99, "col": 99 });
                *patched += 1;
                return;
            }
            for child in map.values_mut() {
                corrupt_first_address(child, patched);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                corrupt_first_address(child, patched);
            }
        }
        _ => {}
    }
}

#[test]
fn an_undecodable_base_is_a_decode_failure() {
    let opts = PatchOptions::default().with_patch(exported_section_text("SimpleEdit.hwpx", 0));

    let err = patch(b"not a zip at all", &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
}
