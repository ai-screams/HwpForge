//! `ops::inspect` — the template operation: pure, byte-in, report-out.
#![cfg(feature = "ops-hwpx")]

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::ops::{inspect, InspectOptions};

fn fixture(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../hwpforge-smithy-hwpx/tests/fixtures/");
    std::fs::read(format!("{path}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

#[test]
fn reports_structure_of_a_table_fixture() {
    let bytes = fixture("SimpleTable.hwpx");

    let out = inspect(&bytes, &InspectOptions::default()).expect("inspect");

    assert_eq!(out.report.sections, 1, "fixture has one section");
    assert_eq!(out.report.tables, 1, "fixture has one table");
    assert_eq!(out.report.section_details.len(), out.report.sections);
    assert_eq!(out.report.section_details[0].index, 0);
    assert_eq!(out.report.section_details[0].tables, 1);
    assert!(out.report.paragraphs >= out.report.section_details[0].top_level_paragraphs);
    assert!(out.report.styles.is_none(), "styles are opt-in");
}

#[test]
fn image_fixture_counts_images() {
    let out = inspect(&fixture("SimplePicture.hwpx"), &InspectOptions::default()).expect("inspect");

    assert_eq!(out.report.images, 1);
    assert_eq!(out.report.tables, 0);
}

#[test]
fn chart_fixture_counts_charts() {
    let out = inspect(&fixture("charts.hwpx"), &InspectOptions::default()).expect("inspect");

    assert!(out.report.charts > 0, "the chart fixture must report charts");
    assert_eq!(
        out.report.charts,
        out.report.section_details.iter().map(|s| s.charts).sum::<usize>()
    );
}

#[test]
fn a_fixture_without_fields_reports_none() {
    let out = inspect(&fixture("SimpleTable.hwpx"), &InspectOptions::default()).expect("inspect");

    assert!(out.report.fields.is_empty(), "{:?}", out.report.fields);
}

#[test]
fn with_styles_adds_the_style_summary_and_nothing_else() {
    let bytes = fixture("SimpleTable.hwpx");

    let plain = inspect(&bytes, &InspectOptions::default()).expect("inspect");
    let styled =
        inspect(&bytes, &InspectOptions::default().with_styles(true)).expect("inspect styled");

    assert!(plain.report.styles.is_none());
    let styles = styled.report.styles.as_ref().expect("styles requested");
    assert!(!styles.fonts.is_empty(), "a real document declares fonts");
    assert!(!styles.char_shapes.is_empty());
    assert!(!styles.para_shapes.is_empty());
    // The rest of the report must not depend on the flag.
    assert_eq!(plain.report.sections, styled.report.sections);
    assert_eq!(plain.report.paragraphs, styled.report.paragraphs);
    assert_eq!(plain.report.tables, styled.report.tables);
    assert_eq!(plain.report.fields, styled.report.fields);
}

#[test]
fn styles_serialise_only_when_present() {
    let bytes = fixture("SimpleTable.hwpx");

    let plain = serde_json::to_value(inspect(&bytes, &InspectOptions::default()).unwrap().report)
        .expect("serialise");
    let styled = serde_json::to_value(
        inspect(&bytes, &InspectOptions::default().with_styles(true)).unwrap().report,
    )
    .expect("serialise");

    assert!(plain.get("styles").is_none(), "empty Option is skipped: {plain}");
    assert!(styled.get("styles").is_some());
    assert!(plain.get("section_details").is_some());
}

#[test]
fn non_hwpx_input_is_a_decode_failure() {
    let err = inspect(b"not a zip at all", &InspectOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
    assert_eq!(err.code().as_str(), "DECODE_FAILED");
    assert!(err.hint().is_some(), "decode failures carry the CLI hint");
}

#[test]
fn empty_input_is_a_decode_failure() {
    let err = inspect(&[], &InspectOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::DecodeFailed, "{err}");
}
