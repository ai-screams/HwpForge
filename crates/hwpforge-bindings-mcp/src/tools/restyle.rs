//! `hwpforge_restyle` — Apply a different style preset to an existing HWPX document.

use std::path::Path;

use serde::Serialize;

use hwpforge_smithy_hwpx::presets::style_store_for_preset;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

use crate::output::{check_file_size, ToolErrorInfo};

/// Output data from a successful restyle operation.
#[derive(Debug, Serialize)]
pub struct RestyleData {
    /// Path to the generated HWPX file.
    pub output_path: String,
    /// Applied preset name.
    pub applied_preset: String,
    /// Size of the output file in bytes.
    pub size_bytes: u64,
    /// Number of sections.
    pub sections: usize,
}

/// Apply a style preset to an existing HWPX document.
pub fn run_restyle(
    file_path: &str,
    preset: &str,
    output_path: &str,
) -> Result<RestyleData, ToolErrorInfo> {
    // 1. Validate output extension
    if !output_path.ends_with(".hwpx") {
        return Err(ToolErrorInfo::new(
            "INVALID_EXTENSION",
            format!("Output path must end with .hwpx: {output_path}"),
            "Use a .hwpx extension for the output file.",
        ));
    }

    // 2. Look up preset
    let style_store = style_store_for_preset(preset).ok_or_else(|| {
        ToolErrorInfo::new(
            "PRESET_NOT_FOUND",
            format!("Preset '{preset}' not found"),
            "Use hwpforge_templates to see available presets.",
        )
    })?;

    // 3. Read and decode source HWPX
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

    // 4. Validate and encode with new style store
    let validated = hwpx_doc.document.validate().map_err(|e| {
        ToolErrorInfo::new(
            "VALIDATION_ERROR",
            format!("Document validation failed: {e}"),
            "Check document structure.",
        )
    })?;

    let section_count = validated.section_count();

    let output_bytes = HwpxEncoder::encode(&validated, &style_store, &hwpx_doc.image_store)
        .map_err(|e| {
            ToolErrorInfo::new(
                "ENCODE_ERROR",
                format!("HWPX encoding failed: {e}"),
                "This may be a bug. Please report at https://github.com/ai-screams/HwpForge/issues",
            )
        })?;

    // 5. Write output
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
    std::fs::write(out, &output_bytes).map_err(|e| {
        ToolErrorInfo::new(
            "WRITE_ERROR",
            format!("Failed to write HWPX: {e}"),
            "Check disk space and permissions.",
        )
    })?;

    let size_bytes = output_bytes.len() as u64;

    Ok(RestyleData {
        output_path: output_path.to_string(),
        applied_preset: preset.to_string(),
        size_bytes,
        sections: section_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restyle_invalid_extension() {
        let err = run_restyle("/tmp/doc.hwpx", "default", "/tmp/out.txt").unwrap_err();
        assert_eq!(err.code, "INVALID_EXTENSION");
    }

    #[test]
    fn restyle_unknown_preset() {
        let err = run_restyle("/tmp/doc.hwpx", "unknown", "/tmp/out.hwpx").unwrap_err();
        assert_eq!(err.code, "PRESET_NOT_FOUND");
    }

    #[test]
    fn restyle_missing_file() {
        let err = run_restyle("/nonexistent/file.hwpx", "modern", "/tmp/out.hwpx").unwrap_err();
        assert_eq!(err.code, "FILE_NOT_FOUND");
    }

    #[test]
    fn restyle_happy_path() {
        // 1. Create a valid HWPX via convert (default preset = 함초롬돋움)
        let dir = tempfile::tempdir().unwrap();
        let hwpx_path = dir.path().join("source.hwpx");
        crate::tools::convert::run_convert(
            "# Test\n\nSome content.",
            false,
            hwpx_path.to_str().unwrap(),
            "default",
        )
        .unwrap();

        // 2. Restyle with "modern" preset (맑은 고딕)
        let out_path = dir.path().join("restyled.hwpx");
        let data =
            run_restyle(hwpx_path.to_str().unwrap(), "modern", out_path.to_str().unwrap()).unwrap();

        assert!(out_path.exists());
        assert_eq!(data.applied_preset, "modern");
        assert!(data.size_bytes > 0);
        assert!(data.sections >= 1);
    }
}
