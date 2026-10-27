//! `hwpforge_from_json` — JSON → HWPX direct creation tool.

use serde::Serialize;

use hwpforge::ops;

use crate::compat::{self, Tool};
use crate::output::{write_output_file, ToolErrorInfo, ToolWarningInfo, MAX_INLINE_SIZE};

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
    /// 인코드 경고 (각주 번호 머리 생략 등 — 무음 폐기 금지). Omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ToolWarningInfo>,
}

/// Create an HWPX document from a JSON structure (ExportedDocument schema).
pub fn run_from_json(structure: &str, output_path: &str) -> Result<FromJsonData, ToolErrorInfo> {
    // Path/size checks stay MCP-local (`INVALID_EXTENSION`, `INPUT_TOO_LARGE`
    // have no `ops` equivalent — `ops::from_json` works on an already-sized
    // JSON string).
    if !output_path.ends_with(".hwpx") {
        return Err(ToolErrorInfo::new(
            hwpforge_foundation::diagnostics::OpsCode::InvalidExtension,
            format!("Output path must end with .hwpx: {output_path}"),
            "Use a .hwpx extension for the output file.",
        ));
    }
    if structure.len() > MAX_INLINE_SIZE {
        return Err(ToolErrorInfo::new(
            hwpforge_foundation::diagnostics::OpsCode::InputTooLarge,
            format!(
                "JSON input is {} MB, exceeds {} MB limit",
                structure.len() / 1024 / 1024,
                MAX_INLINE_SIZE / 1024 / 1024,
            ),
            "Split into sections or write to a file.",
        ));
    }

    let outcome = ops::from_json(structure, &ops::FromJsonOptions::default())
        .map_err(|e| compat::tool_error(Tool::FromJson, e))?;

    write_output_file(output_path, &outcome.bytes)?;

    let size_bytes = outcome.bytes.len() as u64;
    let warnings: Vec<ToolWarningInfo> = outcome.warnings.iter().map(compat::warning).collect();

    Ok(FromJsonData {
        output_path: output_path.to_string(),
        size_bytes,
        sections: outcome.sections,
        paragraphs: outcome.paragraphs,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Repo-level fixture, shared with other crates' tests.

    #[test]
    fn from_json_invalid_extension() {
        let err = run_from_json("{}", "/tmp/out.txt").unwrap_err();
        assert_eq!(err.code, "INVALID_EXTENSION");
    }

    #[test]
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
    fn from_json_invalid_json() {
        let err = run_from_json("not json", "/tmp/out.hwpx").unwrap_err();
        assert_eq!(err.code, "JSON_PARSE_FAILED");
    }

    #[test]
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
    fn from_json_empty_document() {
        // Empty document structure fails deserialization (Document<Draft> requires valid structure)
        let json = r#"{"document":{"sections":[]}}"#;
        let err = run_from_json(json, "/tmp/out.hwpx").unwrap_err();
        assert_eq!(err.code, "JSON_PARSE_FAILED");
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

    /// The generated rectangle carries a `linesegarray` layout cache that `to_json`
    /// promotes into the export; `from_json` always re-encodes with the
    /// cache emission off, which must surface as a warning, not a silent
    /// drop, through this tool's own warnings channel too.
    #[test]
    // 이것을 실패시키는 것: 입력 layout cache 제거 (실제 음성 대조 실행).
    fn from_json_warns_when_the_input_carries_a_layout_cache_it_does_not_re_emit() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/hwp5_review/rect_simple.hwpx")
            .to_string_lossy()
            .into_owned();
        let json_data = crate::tools::to_json::run_to_json(&path, None, None).unwrap();
        let json_str = json_data.json_content.expect("inline JSON expected");

        let out_path = dir.path().join("from_json.hwpx");
        let data = run_from_json(&json_str, out_path.to_str().unwrap()).unwrap();

        assert!(
            data.warnings.iter().any(|w| w.code == "LAYOUT_CACHE_DROPPED"
                && w.message.contains("layout cache dropped at section[0]")),
            "{:?}",
            data.warnings
        );
    }
}

#[cfg(test)]
mod paragraph_measure_contract {
    #[test]
    fn legacy_json_tabs_share_code_reason_and_remedy() {
        let input = r#"{"styles":{"tabs":[{"stops":[{"position":2001}]}]}}"#;
        let error = super::run_from_json(input, "unused.hwpx").unwrap_err();
        assert_eq!(error.code, "LEGACY_JSON_TABS");
        assert!(error.message.contains("원래 단위와 case/default 척도를 복구할 수 없어"));
        assert!(error.message.contains("$.styles.tabs[0].stops[0].position"));
        assert!(error.message.contains("1건"));
        assert_eq!(
            error.hint,
            "원본 HWPX/HWP5에서 새 버전의 to-json을 다시 실행한 뒤 편집하십시오."
        );
    }
}
