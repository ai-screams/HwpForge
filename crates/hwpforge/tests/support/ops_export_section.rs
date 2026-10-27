#[path = "json.rs"]
mod json;

use super::*;

pub(super) fn keys(value: &serde_json::Value) -> BTreeSet<String> {
    json::key_set(value)
}
