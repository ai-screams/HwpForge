//! `hwpforge_patch` — JSON → HWPX section replacement tool.

use serde::Serialize;

use hwpforge_smithy_hwpx::{
    ExportedSection, HwpxPatcher, SectionPatchOutcome, SectionWorkflowError,
};

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

    // 2. Read section JSON
    let json_str = read_file_string(section_json_path)?;

    let exported: ExportedSection = serde_json::from_str(&json_str).map_err(|e| {
        ToolErrorInfo::new(
            "JSON_PARSE_ERROR",
            format!("Invalid section JSON: {e}"),
            "Ensure the JSON matches the ExportedSection schema from hwpforge_to_json output.",
        )
    })?;

    // 3. Preserve-first patch
    let outcome = HwpxPatcher::patch_exported_section(&base_bytes, section_idx, &exported)
        .map_err(map_section_workflow_error_for_patch)?;
    let SectionPatchOutcome { bytes, patched_section, sections } = outcome;

    // 7. Write output
    write_output_file(output_path, &bytes)?;

    let size_bytes = bytes.len() as u64;
    Ok(PatchData { output_path: output_path.to_string(), patched_section, sections, size_bytes })
}

fn map_section_workflow_error_for_patch(error: SectionWorkflowError) -> ToolErrorInfo {
    match error {
        SectionWorkflowError::Decode { detail } => ToolErrorInfo::new(
            "DECODE_ERROR",
            format!("HWPX decode failed: {detail}"),
            "Check that the base file is valid HWPX.",
        ),
        SectionWorkflowError::SectionOutOfRange { requested, sections } => ToolErrorInfo::new(
            "SECTION_OUT_OF_RANGE",
            format!("Section {requested} does not exist (document has {sections} sections)"),
            format!("Valid range: 0..={}", sections.saturating_sub(1)),
        ),
        SectionWorkflowError::SectionIndexMismatch { requested, actual } => ToolErrorInfo::new(
            "SECTION_INDEX_MISMATCH",
            format!("Requested section {requested} but JSON contains section {actual} data"),
            format!(
                "Use section: {actual} to match the JSON, or re-export section {requested} with hwpforge_to_json."
            ),
        ),
        SectionWorkflowError::PreservingPatch(error) => ToolErrorInfo::new(
            "PATCH_ERROR",
            format!("Preserving patch failed: {error}"),
            "Re-export the target section with the current hwpforge_to_json tool so preservation metadata is embedded. Structural/style changes still require a broader rebuild workflow.",
        ),
        _ => ToolErrorInfo::new(
            "SECTION_WORKFLOW_ERROR",
            error.to_string(),
            "Update hwpforge so this MCP binding understands the newer section workflow error.",
        ),
    }
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

    #[test]
    fn patch_rejects_legacy_preservation_metadata() {
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
        let mut exported: serde_json::Value = serde_json::from_str(&json).unwrap();
        exported["preservation"]
            .as_object_mut()
            .expect("preservation object")
            .remove("preservation_version");

        let section_json_path = dir.path().join("section.json");
        std::fs::write(
            &section_json_path,
            serde_json::to_vec_pretty(&exported).expect("serialize legacy section"),
        )
        .unwrap();

        let patched_path = dir.path().join("patched.hwpx");
        let error = run_patch(
            hwpx_path.to_str().unwrap(),
            0,
            section_json_path.to_str().unwrap(),
            patched_path.to_str().unwrap(),
        )
        .expect_err("legacy preservation metadata must be rejected");

        assert_eq!(error.code, "PATCH_ERROR");
        assert!(error.message.contains("preservation metadata version"));
        assert!(error.hint.contains("Re-export the target section"));
    }

    #[test]
    fn patch_rejects_section_index_mismatch() {
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
        exported.section_index = 1;

        let section_json_path = dir.path().join("section.json");
        std::fs::write(
            &section_json_path,
            serde_json::to_vec_pretty(&exported).expect("serialize mismatched section"),
        )
        .unwrap();

        let patched_path = dir.path().join("patched.hwpx");
        let error = run_patch(
            hwpx_path.to_str().unwrap(),
            0,
            section_json_path.to_str().unwrap(),
            patched_path.to_str().unwrap(),
        )
        .expect_err("section mismatch must be rejected");

        assert_eq!(error.code, "SECTION_INDEX_MISMATCH");
        assert!(error.message.contains("Requested section 0 but JSON contains section 1 data"));
        assert!(error.hint.contains("Use section: 1"));
    }
}
