//! Output JSON Schema for document/style types.

use schemars::schema_for;

use hwpforge_core::document::Document;

use crate::commands::to_json::{ExportedDocument, ExportedSection};
use crate::error::CliError;

/// Documents the `addr` field the export projector adds to table cells.
///
/// Grid addresses are injected after serde serialization (Core structs stay
/// unchanged), so the derived schema does not know about them; this keeps the
/// published schema in sync with actual `to-json` output.
fn document_cell_addr(schema_value: &mut serde_json::Value) {
    let Some(cell) = schema_value
        .get_mut("$defs")
        .and_then(|defs| defs.get_mut("TableCell"))
        .and_then(|cell| cell.get_mut("properties"))
        .and_then(|props| props.as_object_mut())
    else {
        return;
    };
    cell.insert(
        "addr".to_string(),
        serde_json::json!({
            "description": "Pre-merge logical grid anchor of this cell (0-based), \
                            added by to-json when the table tiles a well-formed grid. \
                            Optional on import: absent = unchecked, present = must \
                            match the derived grid.",
            "type": "object",
            "properties": {
                "row": { "type": "integer", "format": "uint32", "minimum": 0 },
                "col": { "type": "integer", "format": "uint32", "minimum": 0 }
            },
            "required": ["row", "col"]
        }),
    );
}

/// Run the schema command.
pub fn run(type_name: &str, json_mode: bool) {
    let mut schema_value = match type_name {
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
    document_cell_addr(&mut schema_value);

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
