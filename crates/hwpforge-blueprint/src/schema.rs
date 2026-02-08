//! JSON Schema generation for Template validation.
//!
//! Uses [`schemars`] to derive a JSON Schema from the [`Template`] type.
//! This schema can be used by editors (VS Code, Zed) for YAML autocompletion
//! and validation, and by LLM agents to understand the template format.

use schemars::schema::RootSchema;

use crate::template::Template;

/// Generates the JSON Schema for [`Template`] as a [`RootSchema`].
///
/// Use this for programmatic access to the schema object.
pub fn template_schema() -> RootSchema {
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
        let schema = template_schema();
        let title = schema.schema.metadata.as_ref().and_then(|m| m.title.as_deref());
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
        let parsed: RootSchema = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string_pretty(&parsed).unwrap();
        assert_eq!(json, json2);
    }
}
