//! JSON Schema generation for Template validation.
//!
//! Uses [`schemars`] to derive a JSON Schema from the [`Template`] type.
//! This schema can be used by editors (VS Code, Zed) for YAML autocompletion
//! and validation, and by LLM agents to understand the template format.

use crate::template::Template;

/// Generates the JSON Schema for [`Template`] as a [`Schema`](schemars::Schema).
///
/// Use this for programmatic access to the schema object.
pub fn template_schema() -> schemars::Schema {
    schemars::schema_for!(Template)
}

/// Generates the JSON Schema for [`Template`] as a pretty-printed JSON string.
///
/// Useful for writing to a file or embedding in documentation.
///
/// # Panics
///
/// Panics if JSON serialization fails (should never happen for a valid schema).
pub fn template_schema_json() -> String {
    serde_json::to_string_pretty(&template_schema()).expect("schema serialization should not fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn schema_is_valid_json() {
        let json = template_schema_json();
        let _: serde_json::Value = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn schema_has_template_title() {
        let json = template_schema_json();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let title = value.get("title").and_then(|t| t.as_str());
        assert_eq!(title, Some("Template"));
    }

    #[test]
    fn schema_contains_meta_property() {
        let json = template_schema_json();
        assert!(json.contains("\"meta\""), "Schema should reference 'meta' property");
    }

    #[test]
    fn schema_contains_styles_property() {
        let json = template_schema_json();
        assert!(json.contains("\"styles\""), "Schema should reference 'styles' property");
    }

    #[test]
    fn schema_contains_tabs_property() {
        let json = template_schema_json();
        assert!(json.contains("\"tabs\""), "Schema should reference 'tabs' property");
    }

    #[test]
    fn schema_contains_page_property() {
        let json = template_schema_json();
        assert!(json.contains("\"page\""), "Schema should reference 'page' property");
    }

    #[test]
    fn schema_references_partial_char_shape() {
        let json = template_schema_json();
        assert!(json.contains("PartialCharShape"), "Schema should reference PartialCharShape type");
    }

    #[test]
    fn schema_references_partial_para_shape() {
        let json = template_schema_json();
        assert!(json.contains("PartialParaShape"), "Schema should reference PartialParaShape type");
    }

    #[test]
    fn schema_roundtrip_through_json() {
        let json = template_schema_json();
        let parsed: schemars::Schema = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string_pretty(&parsed).unwrap();
        assert_eq!(json, json2);
    }

    /// A small JSON Schema checker for the keywords these schemas use
    /// (`$ref`, `anyOf`, `oneOf`, `type`, `enum`, `const`, `properties`,
    /// `additionalProperties`, `minimum`, `maximum` and one hex-color
    /// `pattern`). Unknown patterns panic rather than pass.
    fn valid(schema: &serde_json::Value, v: &serde_json::Value, root: &serde_json::Value) -> bool {
        use serde_json::Value;
        if let Value::Bool(b) = schema {
            return *b;
        }
        if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
            let name = r.trim_start_matches("#/$defs/");
            return valid(&root["$defs"][name], v, root);
        }
        for key in ["anyOf", "oneOf"] {
            if let Some(alts) = schema.get(key).and_then(Value::as_array) {
                if !alts.iter().any(|a| valid(a, v, root)) {
                    return false;
                }
            }
        }
        if let Some(t) = schema.get("type") {
            let types: Vec<&str> = match t {
                Value::String(s) => vec![s.as_str()],
                Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
                _ => vec![],
            };
            let ok = types.iter().any(|t| match *t {
                "string" => v.is_string(),
                "integer" => v.is_i64() || v.is_u64(),
                "number" => v.is_number(),
                "boolean" => v.is_boolean(),
                "object" => v.is_object(),
                "array" => v.is_array(),
                "null" => v.is_null(),
                _ => false,
            });
            if !ok {
                return false;
            }
        }
        if let Some(e) = schema.get("enum").and_then(Value::as_array) {
            if !e.contains(v) {
                return false;
            }
        }
        if let Some(c) = schema.get("const") {
            if c != v {
                return false;
            }
        }
        if let Some(p) = schema.get("pattern").and_then(Value::as_str) {
            assert_eq!(p, "^#[0-9A-Fa-f]{6}$", "unsupported pattern");
            let s = v.as_str().unwrap_or_default();
            if !(s.len() == 7
                && s.starts_with('#')
                && s[1..].bytes().all(|b| b.is_ascii_hexdigit()))
            {
                return false;
            }
        }
        if let Some(n) = v.as_i64() {
            if schema.get("minimum").and_then(Value::as_i64).is_some_and(|m| n < m)
                || schema.get("maximum").and_then(Value::as_i64).is_some_and(|m| n > m)
            {
                return false;
            }
        }
        if let Some(obj) = v.as_object() {
            let props = schema.get("properties").and_then(Value::as_object);
            for (k, val) in obj {
                match props.and_then(|p| p.get(k)) {
                    Some(ps) => {
                        if !valid(ps, val, root) {
                            return false;
                        }
                    }
                    None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => {
                        return false
                    }
                    None => {}
                }
            }
        }
        true
    }

    /// The template schema accepts every documented `shadow` shape and rejects
    /// a null color and an out-of-range offset (S21 7.4).
    /// 이것을 실패시키는 것: `PartialCharShadow` 스키마를 문자열만으로 두는 것(맵 예시 실패),
    /// null 색·범위를 스키마가 막지 않는 것.
    #[test]
    fn schema_validates_char_shadow_examples() {
        let root = serde_json::to_value(template_schema()).unwrap();
        let char_shape = &root["$defs"]["PartialCharShape"];
        let check = |yaml: &str| {
            let v: serde_json::Value = serde_yaml::from_str(yaml).unwrap();
            valid(char_shape, &v, &root)
        };
        for ok in [
            "shadow: drop",
            "shadow: {kind: continuous}",
            "shadow: {color: none}",
            "shadow: {color: '#C0C0C0'}",
            "shadow: {kind: Drop, offset_x: -100, offset_y: 100}",
        ] {
            assert!(check(ok), "{ok}");
        }
        for bad in ["shadow: {color: null}", "shadow: {offset_x: 101}", "shadow: {alpha: 1}"] {
            assert!(!check(bad), "{bad}");
        }
    }
}
