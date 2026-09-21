//! `hwpforge_patch` — JSON → HWPX section replacement tool.

use serde::Serialize;

use hwpforge::ops::{self, PatchOptions};

use crate::compat::{self, Tool};
use crate::output::{
    read_file_bytes, read_file_string, write_output_file, ToolErrorInfo, ToolWarningInfo,
};

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
    /// Decode warnings for the base package (`ops::PatchOutput::warnings`)
    /// — a preserving patch never re-encodes, so there is no encode half.
    /// Omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ToolWarningInfo>,
}

/// Patch a section in an existing HWPX file with JSON data.
pub fn run_patch(
    base_path: &str,
    section_idx: usize,
    section_json_path: &str,
    output_path: &str,
) -> Result<PatchData, ToolErrorInfo> {
    if !output_path.ends_with(".hwpx") {
        return Err(ToolErrorInfo::new(
            "INVALID_EXTENSION",
            format!("Output path must end with .hwpx: {output_path}"),
            "Use a .hwpx extension for the output file.",
        ));
    }

    let base_bytes = read_file_bytes(base_path)?;
    let json_str = read_file_string(section_json_path)?;

    // `ops::patch` owns the JSON parse, the grid-address verification and
    // the preserving patch itself — the JSON-parse/grid-addr checks used to
    // live here (before `HwpxPatcher::patch_exported_section` ran) and are
    // now `ops`'s.
    let opts = PatchOptions::default().with_section(section_idx).with_patch(json_str);
    let outcome = ops::patch(&base_bytes, &opts).map_err(|e| compat::tool_error(Tool::Patch, e))?;

    write_output_file(output_path, &outcome.bytes)?;

    let warnings: Vec<ToolWarningInfo> = outcome.warnings.iter().map(compat::warning).collect();
    Ok(PatchData {
        output_path: output_path.to_string(),
        patched_section: outcome.section,
        sections: outcome.sections,
        size_bytes: outcome.bytes.len() as u64,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::run::RunContent;
    use hwpforge_smithy_hwpx::ExportedSection;

    fn replace_first_text(exported: &mut ExportedSection, replacement: &str) {
        for paragraph in &mut exported.section.paragraphs {
            for run in &mut paragraph.runs {
                match &mut run.content {
                    RunContent::Text(text) => {
                        *text = replacement.to_string();
                        return;
                    }
                    // Downgrade-on-modify: replacing the visible
                    // string of an `InlineText` run collapses the
                    // structure to plain `Text(String)`. Documented
                    // policy in debug doc §3a-C18 — substitution is
                    // a plain-text operation.
                    RunContent::InlineText(_) => {
                        run.content = RunContent::Text(replacement.to_string());
                        return;
                    }
                    _ => {}
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
        assert!(data.warnings.is_empty(), "a clean base must not warn: {:?}", data.warnings);

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

    fn fixture(rel: &str) -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(rel)
            .to_str()
            .unwrap()
            .to_string()
    }

    /// 줄 조판 캐시가 낡은 fixture 를 patch 하면, 대상 base 를 검증하려고 돌린
    /// 디코드의 경고(`LAYOUT_CACHE_DROPPED`)가 `warnings` 에 실려야 한다.
    #[test]
    fn patch_surfaces_decode_warnings() {
        let path = fixture("layout/stale-line-cache.hwpx");
        let json_data = crate::tools::to_json::run_to_json(&path, Some(0), None).unwrap();
        let json = json_data.json_content.expect("inline section json expected");

        let dir = tempfile::tempdir().unwrap();
        let section_json_path = dir.path().join("section.json");
        std::fs::write(&section_json_path, json).unwrap();
        let out = dir.path().join("out.hwpx");

        let data = run_patch(&path, 0, section_json_path.to_str().unwrap(), out.to_str().unwrap())
            .unwrap();
        assert!(
            data.warnings.iter().any(|w| w.code == "LAYOUT_CACHE_DROPPED"),
            "patch must surface the base decode warning: {:?}",
            data.warnings
        );

        let value = serde_json::to_value(&data).unwrap();
        assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED");
        assert!(!value["warnings"][0]["message"].as_str().unwrap_or_default().is_empty());
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
