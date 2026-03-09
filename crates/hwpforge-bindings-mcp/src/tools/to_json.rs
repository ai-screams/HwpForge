//! `hwpforge_to_json` — HWPX → JSON export tool.

use std::path::Path;

use serde::Serialize;

use hwpforge_smithy_hwpx::{ExportedDocument, ExportedSection, HwpxDecoder};

use crate::output::{check_file_size, ToolErrorInfo};

/// Output data from a successful JSON export.
#[derive(Debug, Serialize)]
pub struct ToJsonData {
    /// Path to the generated JSON file (if written to file).
    pub output_path: Option<String>,
    /// Size of the JSON in bytes.
    pub size_bytes: u64,
    /// Whether this is a section-only export.
    pub section_only: bool,
    /// The JSON string (returned inline when no output_path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_content: Option<String>,
}

/// Export HWPX to JSON (full document or single section).
pub fn run_to_json(
    file_path: &str,
    section_idx: Option<usize>,
    output_path: Option<&str>,
) -> Result<ToJsonData, ToolErrorInfo> {
    let path = Path::new(file_path);
    if !path.exists() {
        return Err(ToolErrorInfo::new(
            "FILE_NOT_FOUND",
            format!("HWPX file not found: {file_path}"),
            "Check the file path and try again.",
        ));
    }

    check_file_size(path)?;
    let bytes = std::fs::read(path).map_err(|e| {
        ToolErrorInfo::new(
            "READ_ERROR",
            format!("Failed to read file: {e}"),
            "Check file permissions.",
        )
    })?;

    let hwpx_doc = HwpxDecoder::decode(&bytes).map_err(|e| {
        ToolErrorInfo::new(
            "DECODE_ERROR",
            format!("HWPX decode failed: {e}"),
            "Check that the file is a valid HWPX document.",
        )
    })?;

    let styles = Some(hwpx_doc.style_store);

    let json_string = if let Some(idx) = section_idx {
        let sections = hwpx_doc.document.sections();
        if idx >= sections.len() {
            return Err(ToolErrorInfo::new(
                "SECTION_OUT_OF_RANGE",
                format!("Section {idx} does not exist (document has {} sections)", sections.len()),
                format!("Valid range: 0..{}", sections.len().saturating_sub(1)),
            ));
        }
        let exported =
            ExportedSection { section_index: idx, section: sections[idx].clone(), styles };
        serde_json::to_string_pretty(&exported).map_err(|e| {
            ToolErrorInfo::new(
                "SERIALIZE_ERROR",
                format!("Failed to serialize section: {e}"),
                "This may be a bug.",
            )
        })?
    } else {
        let exported = ExportedDocument { document: hwpx_doc.document, styles };
        serde_json::to_string_pretty(&exported).map_err(|e| {
            ToolErrorInfo::new(
                "SERIALIZE_ERROR",
                format!("Failed to serialize document: {e}"),
                "This may be a bug.",
            )
        })?
    };

    let size_bytes = json_string.len() as u64;

    // Write to file if output_path is given
    if let Some(out_path) = output_path {
        let out = Path::new(out_path);
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
        std::fs::write(out, &json_string).map_err(|e| {
            ToolErrorInfo::new(
                "WRITE_ERROR",
                format!("Failed to write JSON: {e}"),
                "Check disk space and permissions.",
            )
        })?;
        Ok(ToJsonData {
            output_path: Some(out_path.to_string()),
            size_bytes,
            section_only: section_idx.is_some(),
            json_content: None,
        })
    } else {
        // Warn if inline response is very large (> 1 MB)
        const MAX_INLINE_RESPONSE: u64 = 1024 * 1024;
        if size_bytes > MAX_INLINE_RESPONSE {
            return Err(ToolErrorInfo::new(
                "OUTPUT_TOO_LARGE",
                format!("JSON output is {} KB, too large for inline response", size_bytes / 1024,),
                "Use output_path to write to a file, or use section parameter to export a single section.",
            ));
        }
        Ok(ToJsonData {
            output_path: None,
            size_bytes,
            section_only: section_idx.is_some(),
            json_content: Some(json_string),
        })
    }
}
