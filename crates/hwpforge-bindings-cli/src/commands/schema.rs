//! Output JSON Schema for document/style types.

use schemars::schema_for;

use hwpforge_core::document::Document;

use crate::commands::to_json::{ExportedDocument, ExportedSection};
use crate::error::CliError;

/// Run the schema command.
pub fn run(type_name: &str, json_mode: bool) {
    let schema_value = match type_name {
        "document" => {
            let schema = schema_for!(Document<hwpforge_core::Draft>);
            serde_json::to_value(&schema).unwrap()
        }
        "exported-document" => {
            let schema = schema_for!(ExportedDocument);
            serde_json::to_value(&schema).unwrap()
        }
        "exported-section" => {
            let schema = schema_for!(ExportedSection);
            serde_json::to_value(&schema).unwrap()
        }
        _ => {
            CliError::new("UNKNOWN_SCHEMA_TYPE", format!("Unknown type '{type_name}'"))
                .with_hint("Available types: document, exported-document, exported-section")
                .exit(json_mode, 1);
        }
    };

    if json_mode {
        let result = serde_json::json!({
            "status": "ok",
            "type": type_name,
            "schema": schema_value,
        });
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!("{}", serde_json::to_string_pretty(&schema_value).unwrap());
    }
}
