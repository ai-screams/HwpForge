//! `hwpforge_restyle` — Apply a different style preset to an existing HWPX document.

use serde::Serialize;

use hwpforge_smithy_hwpx::presets::builtin_presets;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

use crate::output::{read_file_bytes, write_output_file, ToolErrorInfo};

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

    // 2. Look up preset font
    let presets = builtin_presets();
    let preset_info = presets.iter().find(|p| p.name == preset).ok_or_else(|| {
        ToolErrorInfo::new(
            "PRESET_NOT_FOUND",
            format!("Preset '{preset}' not found"),
            "Use hwpforge_templates to see available presets.",
        )
    })?;
    let preset_font = preset_info.font.clone();

    // 3. Read and decode source HWPX
    let bytes = read_file_bytes(file_path)?;

    let hwpx_doc = HwpxDecoder::decode(&bytes).map_err(|e| {
        ToolErrorInfo::new(
            "DECODE_ERROR",
            format!("HWPX decode failed: {e}"),
            "Check that the file is a valid HWPX document.",
        )
    })?;

    // 4. Replace base font in the decoded style store.
    //    Instead of creating a new preset store (which would lose char/para shape
    //    definitions the document references), we keep the original style store
    //    intact and only swap font face names. This preserves all shape indices
    //    while applying the new font.
    let mut style_store = hwpx_doc.style_store;
    // The first font in the store is the base/body font by encoder contract
    // (HwpxStyleStore::push_font writes base font first). Third-party HWPX
    // files may have a different ordering — a future improvement could resolve
    // the base font from the default paragraph style instead.
    let original_base: Option<String> =
        style_store.iter_fonts().next().map(|f| f.face_name.clone());
    match original_base {
        Some(ref base) => style_store.replace_font(base, &preset_font),
        None => {
            return Err(ToolErrorInfo::new(
                "NO_FONTS",
                "Document has no fonts to restyle",
                "The HWPX file may be malformed. Use hwpforge_validate to check.",
            ));
        }
    }

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
    write_output_file(output_path, &output_bytes)?;

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

    #[test]
    fn restyle_preserves_all_shape_indices_with_complex_doc() {
        // Regression test: documents with code blocks reference higher char/para
        // shape indices (7+/20+). The old implementation created a preset store
        // with only 7+20 default shapes, causing index mismatch on encode.
        let dir = tempfile::tempdir().unwrap();
        let hwpx_path = dir.path().join("complex.hwpx");
        let md = "# Heading\n\nBody text.\n\n```rust\nfn main() {}\n```\n\n> Blockquote\n";
        crate::tools::convert::run_convert(md, false, hwpx_path.to_str().unwrap(), "default")
            .unwrap();

        // Restyle must not panic or produce corrupt output
        let out_path = dir.path().join("restyled.hwpx");
        let data = run_restyle(hwpx_path.to_str().unwrap(), "classic", out_path.to_str().unwrap())
            .unwrap();

        assert!(out_path.exists());
        assert_eq!(data.applied_preset, "classic");
        assert!(data.size_bytes > 0);

        // Verify the restyled file can be decoded back (not corrupted)
        let restyled_bytes = std::fs::read(&out_path).unwrap();
        let restyled_doc = HwpxDecoder::decode(&restyled_bytes).unwrap();
        assert!(!restyled_doc.document.sections().is_empty());
    }
}
