//! `ops::read` — one addressable part of a document at a time.
//!
//! The target rules are the CLI's, so they are tested as rules rather than as
//! happy paths: how many targets, in which order the checks fire, and which
//! code each violation reports.
#![cfg(feature = "ops-hwpx")]

use std::collections::BTreeSet;

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::ops::read::{read, ReadOptions};

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

// ── target rules ────────────────────────────────────────────────

#[test]
fn no_target_is_rejected() {
    let err =
        read(&hwpx_fixture("SimpleTable.hwpx"), &ReadOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::ReadTargetRequired, "{err}");
    assert_eq!(err.code().as_str(), "READ_TARGET_REQUIRED");
    assert!(err.to_string().contains("Pass exactly one of --section, --table, --field"), "{err}");
}

#[test]
fn two_targets_are_rejected_by_the_same_rule() {
    let opts = ReadOptions::default().with_section(0).with_table(0);

    let err = read(&hwpx_fixture("SimpleTable.hwpx"), &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::ReadTargetRequired, "{err}");
    assert!(err.to_string().contains("Pass exactly one of"), "{err}");
}

#[test]
fn three_targets_are_rejected_too() {
    let opts = ReadOptions::default().with_section(0).with_table(0).with_field("user_email");

    let err = read(&hwpx_fixture("SimpleTable.hwpx"), &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::ReadTargetRequired, "{err}");
}

#[test]
fn a_range_without_a_section_is_rejected() {
    let opts = ReadOptions::default().with_table(0).with_paras("0..1");

    let err = read(&hwpx_fixture("SimpleTable.hwpx"), &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::ReadParasWithoutSection, "{err}");
    assert_eq!(err.code().as_str(), "READ_PARAS_WITHOUT_SECTION");
    assert!(err.to_string().contains("--paras requires --section"), "{err}");
}

/// Check order matters: a caller who gets both rules wrong must see the
/// target error, because the target count is checked first.
#[test]
fn the_target_rule_fires_before_the_range_rules() {
    let opts = ReadOptions::default().with_table(0).with_field("user_email").with_paras("nonsense");

    let err = read(&hwpx_fixture("SimpleTable.hwpx"), &opts).expect_err("must reject");

    assert!(
        err.to_string().contains("Pass exactly one of"),
        "the target rule must win over both range rules: {err}"
    );
}

#[test]
fn a_malformed_range_is_rejected_with_the_cli_message() {
    let opts = ReadOptions::default().with_section(0).with_paras("1..2..3");

    let err = read(&hwpx_fixture("SimpleTable.hwpx"), &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::ReadParasInvalid, "{err}");
    assert_eq!(err.code().as_str(), "READ_PARAS_INVALID");
    assert!(err.to_string().contains("Cannot parse --paras"), "{err}");
}

/// Argument rules are checked before the document is read, so bytes that are
/// not HWPX at all still produce the argument error.
#[test]
fn argument_rules_are_checked_before_the_document_is_decoded() {
    let err = read(b"not a zip at all", &ReadOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::ReadTargetRequired, "{err}");
    assert_ne!(err.code(), OpsCode::DecodeFailed);
}

// ── section reads ───────────────────────────────────────────────

#[test]
fn a_section_read_without_a_range_returns_the_whole_section() {
    let opts = ReadOptions::default().with_section(0);

    let out = read(&hwpx_fixture("SimpleTable.hwpx"), &opts).expect("read");

    let view = out.paragraphs.expect("a section read returns paragraphs");
    assert_eq!(view.section, 0);
    assert_eq!(view.from, 0);
    assert_eq!(view.paragraphs.len(), view.to - view.from + 1);
    assert!(out.table.is_none());
    assert!(out.fields.is_none());
}

#[test]
fn a_point_range_returns_exactly_one_paragraph() {
    let opts = ReadOptions::default().with_section(0).with_paras("0");

    let out = read(&hwpx_fixture("SimpleTable.hwpx"), &opts).expect("read");

    let view = out.paragraphs.expect("paragraphs");
    assert_eq!((view.from, view.to), (0, 0));
    assert_eq!(view.paragraphs.len(), 1);
}

#[test]
fn a_section_outside_the_document_is_the_library_s_error() {
    let opts = ReadOptions::default().with_section(99);

    let err = read(&hwpx_fixture("SimpleTable.hwpx"), &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::ReadSectionOutOfRange, "{err}");
    assert_eq!(err.code().as_str(), "READ_SECTION_OUT_OF_RANGE");
}

#[test]
fn a_range_outside_the_section_is_the_library_s_error() {
    let opts = ReadOptions::default().with_section(0).with_paras("0..9999");

    let err = read(&hwpx_fixture("SimpleTable.hwpx"), &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::ReadParaRangeInvalid, "{err}");
}

/// `parse_paras` accepts a reversed pair; the library is the one that
/// rejects it, so the rule has a single owner.
#[test]
fn a_reversed_range_is_rejected_by_the_library_not_the_parser() {
    let opts = ReadOptions::default().with_section(0).with_paras("4..0");

    let err = read(&hwpx_fixture("SimpleTable.hwpx"), &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::ReadParaRangeInvalid, "{err}");
    assert!(!err.to_string().contains("Cannot parse --paras"), "{err}");
}

// ── table reads ─────────────────────────────────────────────────

#[test]
fn a_table_read_returns_the_grid() {
    let opts = ReadOptions::default().with_table(0);

    let out = read(&hwpx_fixture("SimpleTable.hwpx"), &opts).expect("read");

    let view = out.table.expect("a table read returns a table");
    assert_eq!(view.ordinal, 0);
    assert!(view.rows > 0 && view.cols > 0);
    // Cells are grid anchors: a merged region appears once, with its spans,
    // so the anchor count never exceeds the coordinate count. This fixture
    // has merges, which is what makes the inequality strict.
    let coordinates = (view.rows * view.cols) as usize;
    assert!(view.cells.len() < coordinates, "{} anchors vs {coordinates}", view.cells.len());
    assert_eq!(
        view.cells.iter().map(|c| (c.row_span * c.col_span) as usize).sum::<usize>(),
        coordinates,
        "every coordinate is covered by exactly one anchor"
    );
    assert!(out.paragraphs.is_none());
    assert!(out.fields.is_none());
}

#[test]
fn a_table_ordinal_outside_the_document_is_the_library_s_error() {
    let opts = ReadOptions::default().with_table(99);

    let err = read(&hwpx_fixture("SimpleTable.hwpx"), &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::ReadTableOutOfRange, "{err}");
}

// ── field reads ─────────────────────────────────────────────────

#[test]
fn a_field_read_returns_every_field_of_that_name() {
    let opts = ReadOptions::default().with_field("user_email");

    let out = read(&repo_fixture("fields/clickhere_filled.hwpx"), &opts).expect("read");

    let found = out.fields.expect("a field read returns fields");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].current, "hanyul.ryu@example.com");
    assert!(out.paragraphs.is_none());
    assert!(out.table.is_none());
}

#[test]
fn an_unknown_field_name_is_the_library_s_error() {
    let opts = ReadOptions::default().with_field("존재하지 않는 필드");

    let err = read(&repo_fixture("fields/clickhere_named.hwpx"), &opts).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::ReadFieldNotFound, "{err}");
}

// ── wire shape ──────────────────────────────────────────────────

#[test]
fn meta_always_carries_all_four_keys() {
    let bytes = hwpx_fixture("SimpleTable.hwpx");

    for opts in [ReadOptions::default().with_section(0), ReadOptions::default().with_table(0)] {
        let value = serde_json::to_value(read(&bytes, &opts).expect("read").meta())
            .expect("serialise meta");

        assert_eq!(
            keys(&value),
            ["paragraphs", "table", "fields", "warnings"].map(String::from).into_iter().collect(),
            "{value}"
        );
    }
}

#[test]
fn the_keys_that_were_not_the_target_are_null_rather_than_absent() {
    let opts = ReadOptions::default().with_table(0);

    let value =
        serde_json::to_value(read(&hwpx_fixture("SimpleTable.hwpx"), &opts).unwrap().meta())
            .expect("serialise meta");

    assert!(value["table"].is_object(), "{value}");
    assert!(value["paragraphs"].is_null(), "skip_serializing_if is forbidden here: {value}");
    assert!(value["fields"].is_null(), "{value}");
    assert_eq!(value["warnings"], serde_json::json!([]));
}

#[test]
fn the_meta_payload_is_the_library_dto_unchanged() {
    let opts = ReadOptions::default().with_section(0);
    let out = read(&hwpx_fixture("SimpleTable.hwpx"), &opts).expect("read");

    let meta = serde_json::to_value(out.meta()).expect("serialise meta");
    let payload = serde_json::to_value(&out.paragraphs).expect("serialise payload");

    assert_eq!(meta["paragraphs"], payload);
}

#[test]
fn a_field_read_serialises_its_list_under_the_fields_key() {
    let opts = ReadOptions::default().with_field("user_email");
    let out = read(&repo_fixture("fields/clickhere_named.hwpx"), &opts).expect("read");

    let value = serde_json::to_value(out.meta()).expect("serialise meta");

    assert!(value["fields"].is_array(), "{value}");
    assert_eq!(value["fields"][0]["name"], serde_json::json!("user_email"));
    assert!(value["paragraphs"].is_null());
    assert!(value["table"].is_null());
}
