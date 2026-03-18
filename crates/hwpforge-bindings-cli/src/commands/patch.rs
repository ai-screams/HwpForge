//! Patch a section in an existing HWPX file.

use std::path::PathBuf;

use crate::error::{check_file_size, CliError};
use hwpforge_smithy_hwpx::{
    ExportedSection, HwpxPatcher, SectionPatchOutcome, SectionWorkflowError,
};

/// Run the patch command.
pub fn run(
    base: &PathBuf,
    section_idx: usize,
    section_json: &PathBuf,
    output: &PathBuf,
    json_mode: bool,
) {
    // Read base HWPX
    check_file_size(base, json_mode);
    let base_bytes = match std::fs::read(base) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", base.display()))
                .exit(json_mode, 1);
        }
    };
    // Read section JSON
    check_file_size(section_json, json_mode);
    let json_str = match std::fs::read_to_string(section_json) {
        Ok(s) => s,
        Err(e) => {
            CliError::new(
                "FILE_READ_FAILED",
                format!("Cannot read '{}': {e}", section_json.display()),
            )
            .exit(json_mode, 1);
        }
    };

    let exported_section: ExportedSection = match serde_json::from_str(&json_str) {
        Ok(s) => s,
        Err(e) => {
            CliError::new("JSON_PARSE_FAILED", format!("Invalid section JSON: {e}"))
                .exit(json_mode, 2);
        }
    };

    let outcome =
        match HwpxPatcher::patch_exported_section(&base_bytes, section_idx, &exported_section) {
            Ok(outcome) => outcome,
            Err(error) => exit_section_patch_error(error, json_mode),
        };
    let SectionPatchOutcome { bytes, patched_section, sections } = outcome;

    if let Err(e) = std::fs::write(output, &bytes) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 1);
    }

    let result = serde_json::json!({
        "status": "ok",
        "output": output.display().to_string(),
        "patched_section": patched_section,
        "sections": sections,
        "size_bytes": bytes.len(),
    });

    if json_mode {
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!(
            "Patched section {} -> {} ({} bytes)",
            patched_section,
            output.display(),
            bytes.len()
        );
    }
}

fn exit_section_patch_error(error: SectionWorkflowError, json_mode: bool) -> ! {
    match error {
        SectionWorkflowError::Decode { detail } => {
            CliError::new("DECODE_FAILED", format!("HWPX decode error: {detail}"))
                .exit(json_mode, 2);
        }
        SectionWorkflowError::SectionOutOfRange { requested, sections } => {
            CliError::new(
                "SECTION_OUT_OF_RANGE",
                format!("Section {requested} does not exist (document has {sections} sections)"),
            )
            .with_hint(format!("Valid range: 0..{}", sections.saturating_sub(1)))
            .exit(json_mode, 1);
        }
        SectionWorkflowError::SectionIndexMismatch { requested, actual } => {
            CliError::new(
                "SECTION_INDEX_MISMATCH",
                format!("Requested section {requested} but JSON contains section {actual} data"),
            )
            .with_hint(format!(
                "Use --section {actual} to match the JSON, or re-export section {requested} with this version of hwpforge."
            ))
            .exit(json_mode, 2);
        }
        SectionWorkflowError::PreservingPatch(error) => {
            CliError::new("PATCH_FAILED", format!("Preserving patch error: {error}"))
                .with_hint(
                    "Re-export the target section with this version of hwpforge so the JSON contains preservation metadata. Structural or style changes still require a broader rebuild workflow.",
                )
                .exit(json_mode, 2);
        }
        _ => {
            CliError::new("SECTION_WORKFLOW_FAILED", error.to_string())
                .with_hint(
                    "Update hwpforge so the CLI understands the newer section workflow error.",
                )
                .exit(json_mode, 2);
        }
    }
}
