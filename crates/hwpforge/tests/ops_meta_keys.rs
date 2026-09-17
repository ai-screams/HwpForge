//! Flattened `*Meta` wrappers reserve the `warnings` key (review R2 #6).
//!
//! `InspectMeta`, `DiffMeta` and `StampPlanMeta` splice their payload's fields
//! into the wire object and add `warnings` beside them. If a payload ever
//! grew a `warnings` field the two would collide silently, so this test pins
//! the reservation: the payload alone must never serialise that key, and the
//! meta's keys must be exactly the payload's keys plus `warnings`.
#![cfg(feature = "ops-hwpx")]

use std::collections::BTreeSet;

use hwpforge::ops::{diff, inspect, stamp_plan, InspectOptions};

fn fixture(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../hwpforge-smithy-hwpx/tests/fixtures/");
    std::fs::read(format!("{path}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn keys(value: &serde_json::Value) -> BTreeSet<String> {
    value.as_object().expect("object").keys().cloned().collect()
}

fn assert_flatten_contract(payload: &serde_json::Value, meta: &serde_json::Value, what: &str) {
    let payload_keys = keys(payload);
    assert!(!payload_keys.contains("warnings"), "{what}: payload must not carry a `warnings` key");
    let mut expected = payload_keys;
    expected.insert("warnings".into());
    assert_eq!(keys(meta), expected, "{what}: meta keys are payload keys plus `warnings`");
}

#[test]
fn inspect_meta_reserves_the_warnings_key() {
    let out = inspect(&fixture("SimpleTable.hwpx"), &InspectOptions::default()).expect("inspect");
    let payload = serde_json::to_value(&out.report).expect("payload");
    let meta = serde_json::to_value(out.meta()).expect("meta");
    assert_flatten_contract(&payload, &meta, "InspectMeta");
}

#[test]
fn diff_meta_reserves_the_warnings_key() {
    let out = diff(&fixture("SimpleTable.hwpx"), &fixture("SimplePicture.hwpx")).expect("diff");
    let payload = serde_json::to_value(&out.diff).expect("payload");
    let meta = serde_json::to_value(out.meta()).expect("meta");
    assert_flatten_contract(&payload, &meta, "DiffMeta");
}

#[test]
fn stamp_plan_meta_reserves_the_warnings_key() {
    let out = stamp_plan(&fixture("SimpleTable.hwpx")).expect("stamp_plan");
    let payload = serde_json::to_value(&out.plan).expect("payload");
    let meta = serde_json::to_value(out.meta()).expect("meta");
    assert_flatten_contract(&payload, &meta, "StampPlanMeta");
}
