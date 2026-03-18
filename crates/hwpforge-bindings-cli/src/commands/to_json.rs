//! Export HWPX to editable JSON.

use std::path::PathBuf;

use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxPatcher};

use crate::error::{check_file_size, CliError};

// Re-export shared exchange types so existing imports (`crate::commands::to_json::Exported*`) keep working.
pub use hwpforge_smithy_hwpx::{ExportedDocument, ExportedSection};

/// Run the to-json command.
pub fn run(
    file: &PathBuf,
    output: &PathBuf,
    section_idx: Option<usize>,
    no_styles: bool,
    json_mode: bool,
) {
    check_file_size(file, json_mode);
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", file.display()))
                .exit(json_mode, 1);
        }
    };

    let hwpx_doc = match HwpxDecoder::decode(&bytes) {
        Ok(d) => d,
        Err(e) => {
            CliError::new("DECODE_FAILED", format!("HWPX decode error: {e}")).exit(json_mode, 2);
        }
    };

    let styles = if no_styles { None } else { Some(hwpx_doc.style_store) };

    let json_string = if let Some(idx) = section_idx {
        let sections = hwpx_doc.document.sections();
        if idx >= sections.len() {
            CliError::new(
                "SECTION_OUT_OF_RANGE",
                format!("Section {idx} does not exist (document has {} sections)", sections.len()),
            )
            .with_hint(format!("Valid range: 0..{}", sections.len().saturating_sub(1)))
            .exit(json_mode, 1);
        }
        let preservation =
            match HwpxPatcher::export_section_preservation(&bytes, idx, &sections[idx]) {
                Ok(metadata) => Some(metadata),
                Err(error) => {
                    if json_mode {
                        let warn = serde_json::json!({
                            "status": "warning",
                            "code": "PRESERVATION_METADATA_UNAVAILABLE",
                            "message": format!("Preserving patch metadata unavailable: {error}"),
                        });
                        eprintln!("{}", serde_json::to_string(&warn).unwrap());
                    } else {
                        eprintln!("Warning: preserving patch metadata unavailable: {error}");
                    }
                    None
                }
            };
        let exported = ExportedSection {
            section_index: idx,
            section: sections[idx].clone(),
            styles,
            preservation,
        };
        match serde_json::to_string_pretty(&exported) {
            Ok(s) => s,
            Err(e) => {
                CliError::new("JSON_SERIALIZE_FAILED", format!("Failed to serialize section: {e}"))
                    .with_hint("Check for NaN/Infinity values in chart data")
                    .exit(json_mode, 2);
            }
        }
    } else {
        let exported = ExportedDocument { document: hwpx_doc.document, styles };
        match serde_json::to_string_pretty(&exported) {
            Ok(s) => s,
            Err(e) => {
                CliError::new(
                    "JSON_SERIALIZE_FAILED",
                    format!("Failed to serialize document: {e}"),
                )
                .with_hint("Check for NaN/Infinity values in chart data")
                .exit(json_mode, 2);
            }
        }
    };

    if let Err(e) = std::fs::write(output, &json_string) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 1);
    }

    let result = serde_json::json!({
        "status": "ok",
        "output": output.display().to_string(),
        "size_bytes": json_string.len(),
        "section_only": section_idx.is_some(),
    });

    if json_mode {
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!(
            "Exported {} ({} bytes{})",
            output.display(),
            json_string.len(),
            if let Some(i) = section_idx { format!(", section {i} only") } else { String::new() }
        );
    }
}
