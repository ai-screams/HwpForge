#![allow(dead_code)]

use std::collections::BTreeSet;

/// The object's keys, sorted, so the assertion is on the key **set** rather
/// than on `serde_json`'s map ordering.
pub(crate) fn sorted_keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
    keys.sort();
    keys
}

pub(crate) fn key_set(value: &serde_json::Value) -> BTreeSet<String> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("meta must serialise as an object, got {value}"))
        .keys()
        .cloned()
        .collect()
}

pub(crate) fn corrupt_first_address(value: &mut serde_json::Value, patched: &mut usize) {
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
