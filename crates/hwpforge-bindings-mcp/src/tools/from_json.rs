//! `hwpforge_from_json` — JSON → HWPX direct creation tool.

use serde::Serialize;

use hwpforge::ops;

use crate::compat::{self, Tool};
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
    /// 인코드 경고 (각주 번호 머리 생략 등 — 무음 폐기 금지).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Create an HWPX document from a JSON structure (ExportedDocument schema).
pub fn run_from_json(structure: &str, output_path: &str) -> Result<FromJsonData, ToolErrorInfo> {
    // Path/size checks stay MCP-local (`INVALID_EXTENSION`, `INPUT_TOO_LARGE`
    // have no `ops` equivalent — `ops::from_json` works on an already-sized
    // JSON string).
    if !output_path.ends_with(".hwpx") {
        return Err(ToolErrorInfo::new(
            "INVALID_EXTENSION",
            format!("Output path must end with .hwpx: {output_path}"),
            "Use a .hwpx extension for the output file.",
        ));
    }
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

    let outcome = ops::from_json(structure, &ops::FromJsonOptions::default())
        .map_err(|e| compat::tool_error(Tool::FromJson, e))?;

    write_output_file(output_path, &outcome.bytes)?;

    let size_bytes = outcome.bytes.len() as u64;
    let warnings: Vec<String> =
        outcome.warnings.iter().map(|w| compat::warning(w).message).collect();

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
    fn fixture(rel: &str) -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(rel)
            .to_str()
            .unwrap()
            .to_string()
    }

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

    /// `rect.hwpx` carries a `linesegarray` layout cache that `to_json`
    /// promotes into the export; `from_json` always re-encodes with the
    /// cache emission off, which must surface as a warning, not a silent
    /// drop, through this tool's own warnings channel too.
    #[test]
    fn from_json_warns_when_the_input_carries_a_layout_cache_it_does_not_re_emit() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture("shapes/rect.hwpx");
        let json_data = crate::tools::to_json::run_to_json(&path, None, None).unwrap();
        let json_str = json_data.json_content.expect("inline JSON expected");

        let out_path = dir.path().join("from_json.hwpx");
        let data = run_from_json(&json_str, out_path.to_str().unwrap()).unwrap();

        assert!(
            data.warnings.iter().any(|w| w.contains("layout cache dropped at section[0]")),
            "{:?}",
            data.warnings
        );
    }
}
