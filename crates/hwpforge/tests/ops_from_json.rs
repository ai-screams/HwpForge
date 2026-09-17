//! `ops::from_json` — building a package from an exported JSON document.
#![cfg(feature = "ops-hwpx")]

use std::collections::BTreeSet;

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::ops::exchange::{from_json, to_json, FromJsonOptions, ToJsonOptions};

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

/// Exports `name` and returns the annotated JSON text, which is what a
/// caller of `to_json` would have written to a file.
fn exported_text(name: &str) -> String {
    let out = to_json(&fixture(name), &ToJsonOptions::default()).expect("to_json");
    serde_json::to_string(&out.document).expect("serialise")
}

#[test]
fn an_export_round_trips_back_into_a_decodable_package() {
    let json = exported_text("SimpleTable.hwpx");

    let out = from_json(&json, &FromJsonOptions::default()).expect("from_json");

    assert!(!out.bytes.is_empty());
    assert_eq!(out.sections, 1, "the fixture has one section");
    let reread = to_json(&out.bytes, &ToJsonOptions::default()).expect("re-export");
    assert_eq!(
        reread.exported.document.sections().len(),
        1,
        "the generated package decodes to the same section count"
    );
}

/// The annotated export carries `addr` fields. They must survive the trip in
/// as recognised, verified input rather than as an unknown field that breaks
/// deserialisation.
#[test]
fn the_grid_addresses_in_an_export_are_accepted_and_verified() {
    let json = exported_text("SimpleTable.hwpx");
    assert!(json.contains("\"addr\""), "the fixture's export must carry addresses");

    from_json(&json, &FromJsonOptions::default()).expect("annotated input is valid input");
}

/// A stale address means the caller edited against an older export. The
/// document is refused rather than written with the caller's assumption.
#[test]
fn a_mismatched_grid_address_is_refused() {
    let mut value: serde_json::Value =
        serde_json::from_str(&exported_text("SimpleTable.hwpx")).expect("parse");
    let mut patched = 0usize;
    corrupt_first_address(&mut value, &mut patched);
    assert_eq!(patched, 1, "the fixture must have an address to corrupt");

    let err = from_json(&value.to_string(), &FromJsonOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::GridAddrInvalid, "{err}");
    assert_eq!(err.code().as_str(), "GRID_ADDR_INVALID");
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
fn a_document_exported_without_styles_still_builds() {
    let out = to_json(&fixture("SimpleTable.hwpx"), &ToJsonOptions::default().with_styles(false))
        .expect("to_json");

    let built = from_json(&out.document.to_string(), &FromJsonOptions::default())
        .expect("a default style store fills in");

    assert!(!built.bytes.is_empty());
}

/// JSON carries image references, never image bytes, so a picture document
/// needs its original package to keep the pictures.
#[test]
fn a_base_package_supplies_the_images_the_json_only_references() {
    let bytes = fixture("SimplePicture.hwpx");
    let json = exported_text("SimplePicture.hwpx");

    let without = from_json(&json, &FromJsonOptions::default()).expect("no base");
    let with = from_json(&json, &FromJsonOptions::default().with_base(bytes)).expect("with base");

    assert!(
        with.bytes.len() > without.bytes.len(),
        "the inherited image store makes the package larger: {} vs {}",
        with.bytes.len(),
        without.bytes.len()
    );
}

#[test]
fn an_undecodable_base_is_a_decode_failure() {
    let json = exported_text("SimpleTable.hwpx");

    let err = from_json(&json, &FromJsonOptions::default().with_base(b"not a zip".to_vec()))
        .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
}

#[test]
fn text_that_is_not_json_is_a_parse_failure() {
    let err = from_json("{ not json", &FromJsonOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::JsonParseFailed, "{err}");
    assert_eq!(err.code().as_str(), "JSON_PARSE_FAILED");
}

#[test]
fn json_that_is_not_an_exported_document_is_a_parse_failure() {
    let err =
        from_json(r#"{"unexpected": true}"#, &FromJsonOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::JsonParseFailed, "{err}");
}

#[test]
fn meta_has_only_the_warnings_key() {
    let out =
        from_json(&exported_text("SimpleTable.hwpx"), &FromJsonOptions::default()).expect("ok");

    let value = serde_json::to_value(out.meta()).expect("serialise meta");

    assert_eq!(keys(&value), ["warnings"].map(String::from).into_iter().collect(), "{value}");
    assert!(value["warnings"].is_array());
}

/// Generation is **not** fail-closed. The same encode warning that makes a
/// regenerating edit refuse its bytes (`restyle`, `stamp`, `set_cell` all end
/// in `ENCODE_SEMANTIC_LOSS`) comes back here beside the bytes, because a
/// generated document has no original meaning to lose.
///
/// A footnote whose body starts with a heading is the cheapest trigger: the
/// encoder cannot emit the visible number head for it and says so with
/// `NoteHeadSkipped`, which `EncodeWarning::is_semantic_loss` classifies as a
/// semantic loss.
#[test]
fn a_semantic_loss_warning_comes_back_with_the_bytes_instead_of_replacing_them() {
    let out = from_json(&document_with_a_title_mark_footnote(), &FromJsonOptions::default())
        .expect("generation must not fail closed");

    let codes: Vec<String> = out.meta().warnings.into_iter().map(|w| w.code).collect();
    assert!(codes.iter().any(|c| c == "NOTE_HEAD_SKIPPED"), "expected a semantic loss: {codes:?}");
    assert!(!out.bytes.is_empty(), "the bytes are produced anyway");
    assert_eq!(out.sections, 1);
}

/// An exported document whose only paragraph carries a footnote whose body
/// is a heading. Built rather than loaded: no committed fixture triggers a
/// semantic-loss encode warning.
fn document_with_a_title_mark_footnote() -> String {
    use hwpforge::core::control::Control;
    use hwpforge::core::{Document, PageSettings, Paragraph, Run, Section};
    use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};
    use hwpforge::hwpx::ExportedDocument;

    let shape = CharShapeIndex::new(0);
    let mut note_body =
        Paragraph::with_runs(vec![Run::text("제목 각주", shape)], ParaShapeIndex::new(0));
    note_body.heading_level = Some(1);

    let body = Paragraph::with_runs(
        vec![Run::text("본문", shape), Run::control(Control::footnote(vec![note_body]), shape)],
        ParaShapeIndex::new(0),
    );

    let mut document = Document::new();
    document.add_section(Section::with_paragraphs(vec![body], PageSettings::a4()));

    let styles = hwpforge::hwpx::style_store_for_preset("default").expect("preset");
    let exported = ExportedDocument { document, styles: Some(styles) };
    serde_json::to_string(&exported).expect("serialise")
}
