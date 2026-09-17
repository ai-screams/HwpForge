//! `ops::outline` — the document navigation map.
#![cfg(feature = "ops-hwpx")]

use std::collections::BTreeSet;

use hwpforge::foundation::diagnostics::OpsCode;
// Module path, not a `mod.rs` re-export: the re-exports are the lead's to
// add, and a module path keeps working once they land.
use hwpforge::ops::read::outline;

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
fn maps_a_single_section_table_document() {
    let out = outline(&hwpx_fixture("SimpleTable.hwpx")).expect("outline");

    assert_eq!(out.outline.sections.len(), 1);
    assert_eq!(out.outline.sections[0].section, 0);
    assert_eq!(out.outline.tables.len(), 1, "one table, one entry");
    assert_eq!(out.outline.tables[0].ordinal, 0, "ordinals start at zero");
    assert_eq!(out.outline.tables[0].at.section, 0);
}

#[test]
fn meta_keys_are_outline_and_warnings() {
    let out = outline(&hwpx_fixture("SimpleTable.hwpx")).expect("outline");

    let value = serde_json::to_value(out.meta()).expect("serialise meta");

    assert_eq!(keys(&value), ["outline", "warnings"].map(String::from).into_iter().collect());
    assert_eq!(value["warnings"], serde_json::json!([]), "{value}");
    assert!(value["outline"]["sections"].is_array(), "{value}");
}

#[test]
fn the_meta_payload_is_the_library_dto_unchanged() {
    let out = outline(&hwpx_fixture("SimpleTable.hwpx")).expect("outline");

    let meta = serde_json::to_value(out.meta()).expect("serialise meta");
    let payload = serde_json::to_value(&out.outline).expect("serialise payload");

    assert_eq!(meta["outline"], payload, "the wrapper must not reshape the DTO");
}

#[test]
fn a_document_without_tables_reports_an_empty_list() {
    let out = outline(&hwpx_fixture("SimplePicture.hwpx")).expect("outline");

    assert!(out.outline.tables.is_empty(), "{:?}", out.outline.tables);
    assert_eq!(out.outline.sections[0].tables, 0);
}

/// Two sibling tables in the body flow: the summary and the list agree.
#[test]
fn sibling_tables_make_the_summary_and_the_list_agree() {
    let out = outline(&repo_fixture("tables/table_08_nested_table.hwpx")).expect("outline");

    let listed = out.outline.tables.iter().filter(|t| t.at.section == 0).count();

    assert_eq!(out.outline.sections[0].tables, listed);
    assert_eq!(listed, 2, "this fixture carries two body-flow tables");
}

/// The case that decides the `SectionOutline::tables` contract: a table
/// inside a cell is reachable only by the recursive inventory, and the
/// per-section summary must still match the ordinal list. `content_counts()`
/// alone would say 1 here; `HwpxReader::outline` recounts from the inventory
/// afterwards (`read.rs:202-207`), which is what keeps the two in step.
#[test]
fn a_table_inside_a_cell_is_counted_by_both_the_summary_and_the_list() {
    let out = outline(&nested_table_document()).expect("outline");

    let listed = out.outline.tables.iter().filter(|t| t.at.section == 0).count();

    assert_eq!(listed, 2, "the ordinal list reaches the table inside the cell");
    assert_eq!(out.outline.sections[0].tables, listed, "the summary is recounted to match");
    assert_eq!(
        out.outline.tables.iter().map(|t| t.ordinal).collect::<Vec<_>>(),
        vec![0, 1],
        "ordinals stay dense across the nesting"
    );
}

/// Builds an HWPX package whose only body table holds a second table in its
/// single cell. Synthesised rather than loaded, because no committed fixture
/// nests a table (`tables/table_08_nested_table.hwpx` is two siblings).
fn nested_table_document() -> Vec<u8> {
    use hwpforge::core::image::ImageStore;
    use hwpforge::core::table::{Table, TableCell, TableRow};
    use hwpforge::core::{Document, PageSettings, Paragraph, Run, Section};
    use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    let shape = CharShapeIndex::new(0);
    let width = HwpUnit::from_pt(200.0).expect("width");

    let one_cell_table = |paragraphs: Vec<Paragraph>| {
        Table::new(vec![TableRow::new(vec![TableCell::new(paragraphs, width)])])
    };

    let mut inner_paragraph = Paragraph::new(ParaShapeIndex::new(0));
    inner_paragraph.runs.push(Run::text("안쪽", shape));
    let inner = one_cell_table(vec![inner_paragraph]);

    let mut cell_paragraph = Paragraph::new(ParaShapeIndex::new(0));
    cell_paragraph.runs.push(Run::table(inner, shape));
    let outer = one_cell_table(vec![cell_paragraph]);

    let mut body = Paragraph::new(ParaShapeIndex::new(0));
    body.runs.push(Run::table(outer, shape));

    let mut document = Document::new();
    document.add_section(Section::with_paragraphs(vec![body], PageSettings::a4()));

    let validated = document.validate().expect("validate");
    let styles = hwpforge::hwpx::style_store_for_preset("default").expect("preset");
    hwpforge::hwpx::HwpxEncoder::encode(&validated, &styles, &ImageStore::default())
        .expect("encode")
}

#[test]
fn decode_warnings_are_not_observable_through_this_surface() {
    // `HwpxReader::outline` decodes internally and drops `decoded.warnings`,
    // so the list is empty by construction today. The field exists so the
    // wire shape does not change when the reader starts reporting them.
    let out = outline(&hwpx_fixture("sample1.hwpx")).expect("outline");

    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn non_hwpx_input_is_a_decode_failure() {
    let err = outline(b"not a zip at all").expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
    assert_eq!(err.code().as_str(), "DECODE_FAILED");
}

#[test]
fn empty_input_is_a_decode_failure() {
    let err = outline(&[]).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
}
