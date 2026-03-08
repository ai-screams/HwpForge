//! Convert JSON back to HWPX.

use std::path::PathBuf;

use hwpforge_core::image::ImageStore;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, HwpxStyleStore};

use crate::commands::to_json::ExportedDocument;
use crate::error::{check_file_size, CliError};

/// Run the from-json command.
pub fn run(input: &PathBuf, output: &PathBuf, base: &Option<PathBuf>, json_mode: bool) {
    check_file_size(input, json_mode);

    let json_str = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", input.display()))
                .exit(json_mode, 1);
        }
    };

    let exported: ExportedDocument = match serde_json::from_str(&json_str) {
        Ok(d) => d,
        Err(e) => {
            CliError::new("JSON_PARSE_FAILED", format!("Invalid JSON: {e}"))
                .with_hint(
                    "Ensure the JSON matches the HwpForge document schema (run 'hwpforge schema document')",
                )
                .exit(json_mode, 2);
        }
    };

    let style_store =
        exported.styles.unwrap_or_else(|| HwpxStyleStore::with_default_fonts("함초롬돋움"));

    let validated = match exported.document.validate() {
        Ok(v) => v,
        Err(e) => {
            CliError::new("VALIDATION_FAILED", format!("Document validation error: {e}"))
                .exit(json_mode, 2);
        }
    };

    // Image store: inherit from base HWPX if provided
    let image_store = if let Some(base_path) = base {
        check_file_size(base_path, json_mode);
        let base_bytes = match std::fs::read(base_path) {
            Ok(b) => b,
            Err(e) => {
                CliError::new(
                    "FILE_READ_FAILED",
                    format!("Cannot read base '{}': {e}", base_path.display()),
                )
                .exit(json_mode, 1);
            }
        };
        match HwpxDecoder::decode(&base_bytes) {
            Ok(d) => d.image_store,
            Err(e) => {
                CliError::new("DECODE_FAILED", format!("Base HWPX decode error: {e}"))
                    .exit(json_mode, 2);
            }
        }
    } else {
        ImageStore::new()
    };

    let bytes = match HwpxEncoder::encode(&validated, &style_store, &image_store) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("ENCODE_FAILED", format!("HWPX encode error: {e}")).exit(json_mode, 2);
        }
    };

    if let Err(e) = std::fs::write(output, &bytes) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 1);
    }

    let result = serde_json::json!({
        "status": "ok",
        "output": output.display().to_string(),
        "sections": validated.section_count(),
        "size_bytes": bytes.len(),
    });

    if json_mode {
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!(
            "Generated {} ({} sections, {} bytes)",
            output.display(),
            validated.section_count(),
            bytes.len()
        );
    }
}
