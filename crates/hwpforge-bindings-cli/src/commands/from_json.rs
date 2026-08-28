//! Convert JSON back to HWPX.

use std::path::PathBuf;

use crate::error::{check_file_size, CliError};
use hwpforge_core::image::ImageStore;
use hwpforge_smithy_hwpx::{ExportedDocument, HwpxDecoder, HwpxEncoder, HwpxStyleStore};

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

    // Parse the tree once; the typed document deserializes from it by
    // reference (no reparse, no clone).
    let value: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            CliError::new("JSON_PARSE_FAILED", format!("Invalid JSON: {e}")).exit(json_mode, 2);
        }
    };
    let exported: ExportedDocument = match serde::Deserialize::deserialize(&value) {
        Ok(d) => d,
        Err(e) => {
            CliError::new("JSON_PARSE_FAILED", format!("Invalid JSON: {e}"))
                .with_hint(
                    "Ensure the JSON matches the HwpForge document schema (run 'hwpforge schema document')",
                )
                .exit(json_mode, 2);
        }
    };

    // Supplied cell grid addresses are validated, then discarded: absence
    // means no check, a mismatch means the caller acted on stale addresses.
    if let Err(e) =
        hwpforge_smithy_hwpx::grid_addr::verify_document_addresses(&value, &exported.document)
    {
        CliError::new("GRID_ADDR_INVALID", format!("Cell grid address check failed: {e}"))
            .with_hint(
                "Grid addresses come from to-json output; after structural edits, drop the stale addr fields (or re-export) and retry",
            )
            .exit(json_mode, 2);
    }

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

    let outcome = match HwpxEncoder::encode_with_diagnostics(
        &validated,
        &style_store,
        &image_store,
        hwpforge_smithy_hwpx::EncodeOptions::default(),
    ) {
        Ok(o) => o,
        Err(e) => {
            CliError::new("ENCODE_FAILED", format!("HWPX encode error: {e}")).exit(json_mode, 2);
        }
    };
    let bytes = outcome.bytes;
    // 인코드 경고(각주 번호 머리 생략 등)를 무음 폐기하지 않는다.
    let encode_warnings: Vec<String> =
        outcome.warnings.iter().map(std::string::ToString::to_string).collect();
    for w in &encode_warnings {
        eprintln!("[from-json] {w}");
    }

    if let Err(e) = std::fs::write(output, &bytes) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 1);
    }

    let result = serde_json::json!({
        "status": "ok",
        "output": output.display().to_string(),
        "sections": validated.section_count(),
        "size_bytes": bytes.len(),
        "warnings": encode_warnings,
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
