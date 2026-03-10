//! `hwpforge_from_json` — JSON → HWPX direct creation tool.

use serde::Serialize;

use hwpforge_core::image::ImageStore;
use hwpforge_smithy_hwpx::presets::style_store_for_preset;
use hwpforge_smithy_hwpx::{ExportedDocument, HwpxEncoder};

use crate::output::{write_output_file, ToolErrorInfo, MAX_INLINE_SIZE};

/// Output data from a successful JSON → HWPX creation.
#[derive(Debug, Serialize)]
pub struct FromJsonData {
    /// Path to the generated HWPX file.
    pub output_path: String,
    /// Size of the generated file in bytes.
    pub size_bytes: u64,
    /// Number of sections.
    pub sections: usize,
    /// Total paragraphs.
    pub paragraphs: usize,
}

/// Create an HWPX document from a JSON structure (ExportedDocument schema).
pub fn run_from_json(structure: &str, output_path: &str) -> Result<FromJsonData, ToolErrorInfo> {
    // 1. Validate output extension
    if !output_path.ends_with(".hwpx") {
        return Err(ToolErrorInfo::new(
            "INVALID_EXTENSION",
            format!("Output path must end with .hwpx: {output_path}"),
            "Use a .hwpx extension for the output file.",
        ));
    }

    // 2. Check inline size
    if structure.len() > MAX_INLINE_SIZE {
        return Err(ToolErrorInfo::new(
            "INPUT_TOO_LARGE",
            format!(
                "JSON input is {} MB, exceeds {} MB limit",
                structure.len() / 1024 / 1024,
                MAX_INLINE_SIZE / 1024 / 1024,
            ),
            "Split into sections or write to a file.",
        ));
    }

    // 3. Parse JSON
    let exported: ExportedDocument = serde_json::from_str(structure).map_err(|e| {
        ToolErrorInfo::new(
            "JSON_PARSE_ERROR",
            format!("Invalid JSON: {e}"),
            "Ensure JSON matches the ExportedDocument schema from hwpforge_to_json output.",
        )
    })?;

    // 4. Resolve styles (use embedded styles or default fallback)
    let style_store = match exported.styles {
        Some(s) => s,
        None => style_store_for_preset("default").ok_or_else(|| {
            ToolErrorInfo::new(
                "INTERNAL_ERROR",
                "Default preset not found",
                "This is a bug. Please report at https://github.com/ai-screams/HwpForge/issues",
            )
        })?,
    };

    // 5. Count before validation
    let sections = exported.document.sections().len();
    let paragraphs: usize = exported.document.sections().iter().map(|s| s.paragraphs.len()).sum();

    // 6. Validate
    let validated = exported.document.validate().map_err(|e| {
        ToolErrorInfo::new(
            "VALIDATION_ERROR",
            format!("Document validation failed: {e}"),
            "Check document structure.",
        )
    })?;

    // 7. Encode
    let image_store = ImageStore::new();
    let hwpx_bytes = HwpxEncoder::encode(&validated, &style_store, &image_store).map_err(|e| {
        ToolErrorInfo::new(
            "ENCODE_ERROR",
            format!("HWPX encoding failed: {e}"),
            "This may be a bug. Please report at https://github.com/ai-screams/HwpForge/issues",
        )
    })?;

    // 8. Write output file
    write_output_file(output_path, &hwpx_bytes)?;

    let size_bytes = hwpx_bytes.len() as u64;

    Ok(FromJsonData { output_path: output_path.to_string(), size_bytes, sections, paragraphs })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_json_invalid_extension() {
        let err = run_from_json("{}", "/tmp/out.txt").unwrap_err();
        assert_eq!(err.code, "INVALID_EXTENSION");
    }

    #[test]
    fn from_json_invalid_json() {
        let err = run_from_json("not json", "/tmp/out.hwpx").unwrap_err();
        assert_eq!(err.code, "JSON_PARSE_ERROR");
    }

    #[test]
    fn from_json_empty_document() {
        // Empty document structure fails deserialization (Document<Draft> requires valid structure)
        let json = r#"{"document":{"sections":[]}}"#;
        let err = run_from_json(json, "/tmp/out.hwpx").unwrap_err();
        assert_eq!(err.code, "JSON_PARSE_ERROR");
    }

    #[test]
    fn from_json_roundtrip_happy_path() {
        // 1. Create a valid HWPX via convert
        let dir = tempfile::tempdir().unwrap();
        let hwpx_path = dir.path().join("test.hwpx");
        crate::tools::convert::run_convert(
            "# Hello\n\nTest paragraph.",
            false,
            hwpx_path.to_str().unwrap(),
            "default",
        )
        .unwrap();

        // 2. Export to JSON via to_json
        let json_data =
            crate::tools::to_json::run_to_json(hwpx_path.to_str().unwrap(), None, None).unwrap();
        let json_str = json_data.json_content.expect("inline JSON expected");

        // 3. Recreate from JSON
        let out_path = dir.path().join("from_json.hwpx");
        let data = run_from_json(&json_str, out_path.to_str().unwrap()).unwrap();

        assert!(out_path.exists());
        assert!(data.size_bytes > 0);
        assert!(data.sections >= 1);
        assert!(data.paragraphs >= 1);
    }
}
