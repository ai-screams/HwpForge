//! `hwpforge_convert` — Markdown → HWPX conversion tool.

use std::path::Path;

use serde::Serialize;

use hwpforge_core::image::ImageStore;
use hwpforge_smithy_hwpx::{HwpxEncoder, HwpxStyleStore};
use hwpforge_smithy_md::MdDecoder;

use crate::output::{check_file_size, ToolErrorInfo, MAX_INLINE_SIZE};

/// Output data from a successful conversion.
#[derive(Debug, Serialize)]
pub struct ConvertData {
    /// Path to the generated HWPX file.
    pub output_path: String,
    /// Size of the generated file in bytes.
    pub size_bytes: u64,
    /// Number of sections in the document.
    pub sections: usize,
    /// Total number of paragraphs across all sections.
    pub paragraphs: usize,
}

/// Execute Markdown → HWPX conversion.
///
/// This is the pure business logic, shared between the MCP handler and tests.
pub fn run_convert(
    markdown: &str,
    is_file: bool,
    output_path: &str,
    preset: &str,
) -> Result<ConvertData, ToolErrorInfo> {
    // 1. Validate preset
    if preset != "default" {
        return Err(ToolErrorInfo::new(
            "PRESET_NOT_FOUND",
            format!("Preset '{preset}' not found"),
            "Available presets: default. Use hwpforge_templates to see all.",
        ));
    }

    // 2. Validate output extension
    if !output_path.ends_with(".hwpx") {
        return Err(ToolErrorInfo::new(
            "INVALID_EXTENSION",
            format!("Output path must end with .hwpx: {output_path}"),
            "Use a .hwpx extension for the output file.",
        ));
    }

    // 3. Read markdown content
    let md_content: String = if is_file {
        let path = Path::new(markdown);
        if !path.exists() {
            return Err(ToolErrorInfo::new(
                "FILE_NOT_FOUND",
                format!("Markdown file not found: {markdown}"),
                "Check the file path and try again.",
            ));
        }
        check_file_size(path)?;
        std::fs::read_to_string(path).map_err(|e| {
            ToolErrorInfo::new(
                "READ_ERROR",
                format!("Failed to read file: {e}"),
                "Check file permissions.",
            )
        })?
    } else {
        if markdown.len() > MAX_INLINE_SIZE {
            return Err(ToolErrorInfo::new(
                "INPUT_TOO_LARGE",
                format!(
                    "Inline content is {} MB, exceeds {} MB limit",
                    markdown.len() / 1024 / 1024,
                    MAX_INLINE_SIZE / 1024 / 1024,
                ),
                "Write the content to a file and use is_file: true.",
            ));
        }
        markdown.to_string()
    };

    // 4. Decode Markdown → Core Document
    let md_doc = MdDecoder::decode_with_default(&md_content).map_err(|e| {
        ToolErrorInfo::new(
            "MD_DECODE_ERROR",
            format!("Markdown decode failed: {e}"),
            "Check Markdown syntax. Use GFM (GitHub Flavored Markdown).",
        )
    })?;

    // 5. Count sections and paragraphs
    let sections: usize = md_doc.document.sections().len();
    let paragraphs: usize = md_doc.document.sections().iter().map(|s| s.paragraphs.len()).sum();

    // 6. Build style store and validate
    let style_store = HwpxStyleStore::from_registry(&md_doc.style_registry);
    let image_store = ImageStore::new();

    let validated = md_doc.document.validate().map_err(|e| {
        ToolErrorInfo::new(
            "VALIDATION_ERROR",
            format!("Document validation failed: {e}"),
            "Check document structure.",
        )
    })?;

    // 7. Encode to HWPX bytes
    let hwpx_bytes = HwpxEncoder::encode(&validated, &style_store, &image_store).map_err(|e| {
        ToolErrorInfo::new(
            "ENCODE_ERROR",
            format!("HWPX encoding failed: {e}"),
            "This may be a bug. Please report at https://github.com/ai-screams/HwpForge/issues",
        )
    })?;

    // 8. Write output file
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
    std::fs::write(out, &hwpx_bytes).map_err(|e| {
        ToolErrorInfo::new(
            "WRITE_ERROR",
            format!("Failed to write HWPX: {e}"),
            "Check disk space and permissions.",
        )
    })?;

    let size_bytes: u64 = hwpx_bytes.len() as u64;

    Ok(ConvertData { output_path: output_path.to_string(), size_bytes, sections, paragraphs })
}
