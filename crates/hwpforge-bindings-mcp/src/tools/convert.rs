//! `hwpforge_convert` — Markdown → HWPX conversion tool.

use serde::Serialize;

use hwpforge::ops::{convert_md as ops_convert_md, ConvertMdOptions, OpsError};
use hwpforge_smithy_hwpx::presets::builtin_presets;

use crate::compat::{self, Tool};
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
    /// 이미지 임베드에서 제외된 참조들 (W6 §12b — typed 경고의 표시 문자열).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
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
    // 1. Preset existence, checked before touching the filesystem: this
    //    preserves the legacy ordering (a preset typo must not cost a file
    //    read or an output-extension check). `ops::convert_md` checks the
    //    same thing again, authoritatively, once it has a document to
    //    build.
    if !builtin_presets().iter().any(|p| p.name == preset) {
        return Err(compat::tool_error(
            Tool::ConvertMd,
            OpsError::PresetNotFound { name: preset.to_string() },
        ));
    }

    // 2. Validate output extension (MCP-local — stays outside `ops`).
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

    // 4. base_dir — image references resolve relative to the markdown
    //    file's directory (W6 §12b); inline input has none, so a relative
    //    reference is excluded (and reported) while an inline `data:` URI
    //    still embeds. Bare filenames' `parent()` is the empty path
    //    `Some("")` — normalize to the current directory (독립 리뷰 B2).
    let base_dir = if is_file {
        match std::path::Path::new(markdown).parent() {
            Some(p) if p.as_os_str().is_empty() => Some(std::path::Path::new(".")),
            other => other,
        }
    } else {
        None
    };

    // 5. Delegate decode → preset font swap → asset resolve → style
    //    rebind → validate → encode to `ops::convert_md`.
    let out =
        ops_convert_md(&md_content, base_dir, &ConvertMdOptions::default().with_preset(preset))
            .map_err(|e| compat::tool_error(Tool::ConvertMd, e))?;

    // 6. 이미지 임베드 제외 + 인코드 경고 (W6 §12b — typed 경고의 표시
    //    문자열). 무음 폐기 금지.
    let warnings: Vec<String> = out.warnings.iter().map(|w| compat::warning(w).message).collect();

    // 7. Write output file
    write_output_file(output_path, &out.bytes)?;

    let size_bytes: u64 = out.bytes.len() as u64;

    Ok(ConvertData {
        output_path: output_path.to_string(),
        size_bytes,
        sections: out.sections,
        paragraphs: out.paragraphs,
        warnings,
    })
}
