//! `hwpforge_convert` — Markdown → HWPX conversion tool.

use serde::Serialize;

use hwpforge_core::image::ImageStore;
use hwpforge_foundation::FontId;
use hwpforge_smithy_hwpx::presets::builtin_presets;
use hwpforge_smithy_hwpx::{HwpxEncoder, HwpxStyleStore};
use hwpforge_smithy_md::MdDecoder;

use crate::output::{read_file_string, write_output_file, ToolErrorInfo, MAX_INLINE_SIZE};

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
    let presets = builtin_presets();
    let preset_info = presets.iter().find(|p| p.name == preset).ok_or_else(|| {
        ToolErrorInfo::new(
            "PRESET_NOT_FOUND",
            format!("Preset '{preset}' not found"),
            "Use hwpforge_templates to see available presets.",
        )
    })?;
    let preset_font = preset_info.font.clone();

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
        read_file_string(markdown)?
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
    let mut md_doc = MdDecoder::decode_with_default(&md_content).map_err(|e| {
        ToolErrorInfo::new(
            "MD_DECODE_ERROR",
            format!("Markdown decode failed: {e}"),
            "Check Markdown syntax. Use GFM (GitHub Flavored Markdown).",
        )
    })?;

    // 5. Count sections and paragraphs
    let sections: usize = md_doc.document.sections().len();
    let paragraphs: usize = md_doc.document.sections().iter().map(|s| s.paragraphs.len()).sum();

    // 6. Apply preset font to style registry, then build full style store.
    //    from_registry() creates the complete store (char shapes, para shapes,
    //    styles, border fills) unlike with_default_fonts() which only sets fonts.
    //    Only replace base font entries — preserve specialty fonts (e.g., D2Coding
    //    for code blocks) by checking against the original base font name.
    let preset_font_id = FontId::new(&preset_font).map_err(|e| {
        ToolErrorInfo::new("PRESET_ERROR", format!("Invalid preset font name: {e}"), "")
    })?;
    let original_base =
        md_doc.style_registry.fonts.first().map(|f| f.as_str().to_string()).unwrap_or_default();
    md_doc.style_registry.fonts = md_doc
        .style_registry
        .fonts
        .iter()
        .map(|f| if f.as_str() == original_base { preset_font_id.clone() } else { f.clone() })
        .collect();
    for cs in &mut md_doc.style_registry.char_shapes {
        if cs.font == original_base {
            cs.font.clone_from(&preset_font);
        }
    }
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
    write_output_file(output_path, &hwpx_bytes)?;

    let size_bytes: u64 = hwpx_bytes.len() as u64;

    Ok(ConvertData { output_path: output_path.to_string(), size_bytes, sections, paragraphs })
}
