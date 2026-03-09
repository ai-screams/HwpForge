//! `hwpforge_patch` — JSON → HWPX section replacement tool.

use std::path::Path;

use serde::Serialize;

use crate::output::{check_file_size, ToolErrorInfo};
use hwpforge_smithy_hwpx::{ExportedSection, HwpxDecoder, HwpxEncoder};

/// Output data from a successful patch operation.
#[derive(Debug, Serialize)]
pub struct PatchData {
    /// Path to the generated HWPX file.
    pub output_path: String,
    /// Section index that was replaced.
    pub patched_section: usize,
    /// Total number of sections in the output.
    pub sections: usize,
    /// Size of the output file in bytes.
    pub size_bytes: u64,
}

/// Patch a section in an existing HWPX file with JSON data.
pub fn run_patch(
    base_path: &str,
    section_idx: usize,
    section_json_path: &str,
    output_path: &str,
) -> Result<PatchData, ToolErrorInfo> {
    // 1. Read base HWPX
    let base = Path::new(base_path);
    if !base.exists() {
        return Err(ToolErrorInfo::new(
            "FILE_NOT_FOUND",
            format!("Base HWPX file not found: {base_path}"),
            "Check the file path and try again.",
        ));
    }
    check_file_size(base)?;
    let base_bytes = std::fs::read(base).map_err(|e| {
        ToolErrorInfo::new(
            "READ_ERROR",
            format!("Failed to read base file: {e}"),
            "Check file permissions.",
        )
    })?;

    let mut hwpx_doc = HwpxDecoder::decode(&base_bytes).map_err(|e| {
        ToolErrorInfo::new(
            "DECODE_ERROR",
            format!("HWPX decode failed: {e}"),
            "Check that the base file is valid HWPX.",
        )
    })?;

    // 2. Read section JSON
    let json_path = Path::new(section_json_path);
    if !json_path.exists() {
        return Err(ToolErrorInfo::new(
            "FILE_NOT_FOUND",
            format!("Section JSON file not found: {section_json_path}"),
            "Check the file path and try again.",
        ));
    }
    check_file_size(json_path)?;
    let json_str = std::fs::read_to_string(json_path).map_err(|e| {
        ToolErrorInfo::new(
            "READ_ERROR",
            format!("Failed to read JSON file: {e}"),
            "Check file permissions.",
        )
    })?;

    let exported: ExportedSection = serde_json::from_str(&json_str).map_err(|e| {
        ToolErrorInfo::new(
            "JSON_PARSE_ERROR",
            format!("Invalid section JSON: {e}"),
            "Ensure the JSON matches the ExportedSection schema from hwpforge_to_json output.",
        )
    })?;

    // 3. Warn if section index in JSON doesn't match request
    if exported.section_index != section_idx {
        return Err(ToolErrorInfo::new(
            "SECTION_INDEX_MISMATCH",
            format!(
                "Requested section {} but JSON contains section {} data",
                section_idx, exported.section_index,
            ),
            format!(
                "Use section: {} to match the JSON, or re-export section {} with hwpforge_to_json.",
                exported.section_index, section_idx,
            ),
        ));
    }

    // 4. Replace section
    let sections = hwpx_doc.document.sections_mut();
    if section_idx >= sections.len() {
        return Err(ToolErrorInfo::new(
            "SECTION_OUT_OF_RANGE",
            format!(
                "Section {section_idx} does not exist (document has {} sections)",
                sections.len()
            ),
            format!("Valid range: 0..{}", sections.len().saturating_sub(1)),
        ));
    }
    sections[section_idx] = exported.section;

    // 5. Use patch styles if provided, otherwise keep base styles
    let style_store = exported.styles.unwrap_or(hwpx_doc.style_store);

    // 6. Validate and encode
    let validated = hwpx_doc.document.validate().map_err(|e| {
        ToolErrorInfo::new(
            "VALIDATION_ERROR",
            format!("Validation error after patch: {e}"),
            "The patched section may have invalid structure.",
        )
    })?;

    let bytes =
        HwpxEncoder::encode(&validated, &style_store, &hwpx_doc.image_store).map_err(|e| {
            ToolErrorInfo::new(
                "ENCODE_ERROR",
                format!("HWPX encoding failed: {e}"),
                "This may be a bug. Please report at https://github.com/ai-screams/HwpForge/issues",
            )
        })?;

    // 7. Write output
    let out = Path::new(output_path);
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ToolErrorInfo::new(
                    "WRITE_ERROR",
                    format!("Cannot create output directory: {e}"),
                    "Check write permissions.",
                )
            })?;
        }
    }
    std::fs::write(out, &bytes).map_err(|e| {
        ToolErrorInfo::new(
            "WRITE_ERROR",
            format!("Failed to write HWPX: {e}"),
            "Check disk space and permissions.",
        )
    })?;

    let size_bytes = bytes.len() as u64;
    let section_count = validated.section_count();

    Ok(PatchData {
        output_path: output_path.to_string(),
        patched_section: section_idx,
        sections: section_count,
        size_bytes,
    })
}
