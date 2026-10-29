//! JSON style-reference transformations shared by exchange tests.
use serde_json::{Map, Value};

pub fn transform(value: &mut Value, change: &impl Fn(&mut Map<String, Value>)) {
    match value {
        Value::Object(map) => {
            change(map);
            for child in map.values_mut() {
                transform(child, change);
            }
        }
        Value::Array(items) => {
            for child in items {
                transform(child, change);
            }
        }
        _ => {}
    }
}
