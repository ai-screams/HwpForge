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

/// An unaddressable table warns, through the operation, not by hand.
///
/// `TABLE_GRID_UNADDRESSABLE` used to be covered only by constructing the
/// warning directly, which proved the wording but not that any operation can
/// produce it. A ragged table — one row with two cells, the next with one —
/// has no derivable strict grid, so the annotation pass skips it and says so.
///
/// The message is asserted here too, so the direct-construction test it
/// replaces loses no coverage.
#[test]
fn a_ragged_table_warns_through_the_operation() {
    use hwpforge::core::image::ImageStore;
    use hwpforge::core::run::Run;
    use hwpforge::core::table::{Table, TableCell, TableRow};
    use hwpforge::core::{Document, PageSettings, Paragraph, Section};
    use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
    use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
    use hwpforge::hwpx::HwpxEncoder;

    let width = HwpUnit::new(8000).expect("width");
    let cell = |text: &str| {
        TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text(text, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            width,
        )
    };
    // Row 0 has two cells, row 1 has one: the rows do not tile a rectangle.
    let ragged =
        Table::new(vec![TableRow::new(vec![cell("A"), cell("B")]), TableRow::new(vec![cell("C")])]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(ragged, CharShapeIndex::new(0)));

    let mut document = Document::new();
    document.add_section(Section::with_paragraphs(vec![host], PageSettings::a4()));

    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    let bytes =
        HwpxEncoder::encode(&document.validate().expect("validate"), &styles, &ImageStore::new())
            .expect("encode");

    let out = to_json(&bytes, &ToJsonOptions::default()).expect("to_json");

    let warnings = out.meta().warnings;
    assert!(!warnings.is_empty(), "an unaddressable table must warn");
    let unaddressable = warnings
        .iter()
        .find(|w| w.code == "TABLE_GRID_UNADDRESSABLE")
        .unwrap_or_else(|| panic!("no grid warning in {warnings:?}"));
    assert!(
        unaddressable.message.starts_with("table #0 in section 0 exported without grid addresses:"),
        "the CLI wording must not drift: {}",
        unaddressable.message
    );
    // The export still succeeds — an unaddressable table is a warning, not a
    // failure.
    assert!(out.document.get("document").is_some());
    assert!(addresses(&out.document).is_empty(), "the skipped table got no addresses");
}
