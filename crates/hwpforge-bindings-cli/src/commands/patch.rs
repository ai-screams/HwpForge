//! Patch a section in an existing HWPX file.

use std::path::PathBuf;

use hwpforge_smithy_hwpx::{HwpxPatcher, PackageReader};

use crate::commands::to_json::ExportedSection;
use crate::error::{check_file_size, CliError};

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
    let section_count = match PackageReader::new(&base_bytes) {
        Ok(pkg) => pkg.section_count(),
        Err(e) => {
            CliError::new("DECODE_FAILED", format!("HWPX decode error: {e}")).exit(json_mode, 2);
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

    if exported_section.section_index != section_idx {
        if json_mode {
            let warn = serde_json::json!({
                "status": "warning",
                "code": "SECTION_INDEX_MISMATCH",
                "message": format!(
                    "--section {} does not match JSON section_index {}; using --section value",
                    section_idx, exported_section.section_index
                ),
            });
            eprintln!("{}", serde_json::to_string(&warn).unwrap());
        } else {
            eprintln!(
                "Warning: --section {} does not match JSON section_index {}; using --section value",
                section_idx, exported_section.section_index
            );
        }
    }

    if section_idx >= section_count {
        CliError::new(
            "SECTION_OUT_OF_RANGE",
            format!(
                "Section {section_idx} does not exist (document has {} sections)",
                section_count
            ),
        )
        .with_hint(format!("Valid range: 0..{}", section_count.saturating_sub(1)))
        .exit(json_mode, 1);
    }

    let bytes = match HwpxPatcher::patch_section_preserving(
        &base_bytes,
        section_idx,
        &exported_section.section,
        exported_section.styles.as_ref(),
        exported_section.preservation.as_ref(),
    ) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("PATCH_FAILED", format!("Preserving patch error: {e}"))
                .with_hint(
                    "Re-export the target section with this version of hwpforge so the JSON contains preservation metadata. Structural or style changes still require a broader rebuild workflow.",
                )
                .exit(json_mode, 2);
        }
    };

    if let Err(e) = std::fs::write(output, &bytes) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 1);
    }

    let result = serde_json::json!({
        "status": "ok",
        "output": output.display().to_string(),
        "patched_section": section_idx,
        "sections": section_count,
        "size_bytes": bytes.len(),
    });

    if json_mode {
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!("Patched section {} -> {} ({} bytes)", section_idx, output.display(), bytes.len());
    }
}
