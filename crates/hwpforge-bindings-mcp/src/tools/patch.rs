//! `hwpforge_patch` — JSON → HWPX section replacement tool.

use serde::Serialize;

use hwpforge_smithy_hwpx::{ExportedSection, HwpxPatcher, PackageReader};

use crate::output::{read_file_bytes, read_file_string, write_output_file, ToolErrorInfo};

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
    // 0. Validate output extension
    if !output_path.ends_with(".hwpx") {
        return Err(ToolErrorInfo::new(
            "INVALID_EXTENSION",
            format!("Output path must end with .hwpx: {output_path}"),
            "Use a .hwpx extension for the output file.",
        ));
    }

    // 1. Read base HWPX
    let base_bytes = read_file_bytes(base_path)?;

    let section_count = PackageReader::new(&base_bytes)
        .map_err(|e| {
            ToolErrorInfo::new(
                "DECODE_ERROR",
                format!("HWPX decode failed: {e}"),
                "Check that the base file is valid HWPX.",
            )
        })?
        .section_count();

    // 2. Read section JSON
    let json_str = read_file_string(section_json_path)?;

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

    // 4. Validate section index before patching
    if section_idx >= section_count {
        return Err(ToolErrorInfo::new(
            "SECTION_OUT_OF_RANGE",
            format!(
                "Section {section_idx} does not exist (document has {} sections)",
                section_count
            ),
            format!("Valid range: 0..={}", section_count.saturating_sub(1)),
        ));
    }

    // 5. Preserve-first patch
    let bytes = HwpxPatcher::patch_section_preserving(
        &base_bytes,
        section_idx,
        &exported.section,
        exported.styles.as_ref(),
        exported.preservation.as_ref(),
    )
    .map_err(|e| {
        ToolErrorInfo::new(
            "PATCH_ERROR",
            format!("Preserving patch failed: {e}"),
            "Re-export the target section with the current hwpforge_to_json tool so preservation metadata is embedded. Structural/style changes still require a broader rebuild workflow.",
        )
    })?;

    // 7. Write output
    write_output_file(output_path, &bytes)?;

    let size_bytes = bytes.len() as u64;
    Ok(PatchData {
        output_path: output_path.to_string(),
        patched_section: section_idx,
        sections: section_count,
        size_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::run::RunContent;

    fn replace_first_text(exported: &mut ExportedSection, replacement: &str) {
        for paragraph in &mut exported.section.paragraphs {
            for run in &mut paragraph.runs {
                if let RunContent::Text(text) = &mut run.content {
                    *text = replacement.to_string();
                    return;
                }
            }
        }
        panic!("expected at least one text run in exported section");
    }

    #[test]
    fn patch_roundtrip_section_happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let hwpx_path = dir.path().join("source.hwpx");
        crate::tools::convert::run_convert(
            "# 제목\n\n본문 문단입니다.",
            false,
            hwpx_path.to_str().unwrap(),
            "default",
        )
        .unwrap();

        let json_data =
            crate::tools::to_json::run_to_json(hwpx_path.to_str().unwrap(), Some(0), None).unwrap();
        let json = json_data.json_content.expect("inline section json expected");
        let mut exported: ExportedSection = serde_json::from_str(&json).unwrap();
        replace_first_text(&mut exported, "[TEST] preserving patch");

        let section_json_path = dir.path().join("section.json");
        std::fs::write(
            &section_json_path,
            serde_json::to_vec_pretty(&exported).expect("serialize patched section"),
        )
        .unwrap();

        let patched_path = dir.path().join("patched.hwpx");
        let data = run_patch(
            hwpx_path.to_str().unwrap(),
            0,
            section_json_path.to_str().unwrap(),
            patched_path.to_str().unwrap(),
        )
        .unwrap();

        assert!(patched_path.exists());
        assert_eq!(data.patched_section, 0);
        assert!(data.size_bytes > 0);

        let patched_json =
            crate::tools::to_json::run_to_json(patched_path.to_str().unwrap(), Some(0), None)
                .unwrap();
        let patched_exported: ExportedSection =
            serde_json::from_str(&patched_json.json_content.unwrap()).unwrap();
        let first_text = patched_exported.section.paragraphs[0].runs[0]
            .content
            .as_text()
            .expect("first run text");
        assert_eq!(first_text, "[TEST] preserving patch");
    }
}
