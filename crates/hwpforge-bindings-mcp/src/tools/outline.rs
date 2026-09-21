//! `hwpforge_outline` — 문서 항법 지도 (E5 읽기 표면).

use serde::Serialize;

use hwpforge::ops;
use hwpforge_smithy_hwpx::DocumentOutline;

use crate::compat::{self, Tool};
use crate::output::{read_file_bytes, ToolErrorInfo, ToolWarningInfo};

/// Output data from an outline projection.
#[derive(Debug, Serialize)]
pub struct OutlineData {
    /// The document navigation map (headings, tables, fields, bookmarks).
    #[serde(flatten)]
    pub outline: DocumentOutline,
    /// Decoder warnings for this document (`ops::OutlineOutput::warnings`).
    /// Omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ToolWarningInfo>,
}

/// Inline response ceiling shared with `hwpforge_to_json` (1 MB).
const MAX_INLINE_RESPONSE: usize = 1024 * 1024;

/// Build the document navigation map for an HWPX file.
pub fn run_outline(file_path: &str) -> Result<OutlineData, ToolErrorInfo> {
    let bytes = read_file_bytes(file_path)?;
    let out = ops::outline(&bytes).map_err(|e| compat::tool_error(Tool::Outline, e))?;
    let warnings: Vec<ToolWarningInfo> = out.warnings.iter().map(compat::warning).collect();

    build_outline_data(out.outline, warnings)
}

/// Builds the final [`OutlineData`], gating on the complete serialized
/// response — including `warnings` — rather than on the navigation map
/// alone: a document with many decode warnings but a small outline could
/// otherwise slip past a narrower check while still exceeding the real
/// inline ceiling. Unlike `diff`'s `report_path`, outline has no
/// externalization path, so an oversized response — whether driven by the
/// map or by `warnings` alone — always errors.
///
/// Split out from [`run_outline`] so a test can exercise the gate with a
/// synthetic oversized `warnings` list, without needing a fixture large
/// enough to trigger it for real.
fn build_outline_data(
    outline: DocumentOutline,
    warnings: Vec<ToolWarningInfo>,
) -> Result<OutlineData, ToolErrorInfo> {
    let data = OutlineData { outline, warnings };

    let inline_size = serde_json::to_string(&data).map(|s| s.len()).unwrap_or(usize::MAX);
    if inline_size > MAX_INLINE_RESPONSE {
        return Err(ToolErrorInfo::new(
            "OUTPUT_TOO_LARGE",
            format!("Navigation response is {inline_size} bytes (limit {MAX_INLINE_RESPONSE})"),
            "Use the CLI instead: hwpforge outline <file> --json",
        ));
    }

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_via_mcp_surface_reports_headings_and_tables() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe.hwpx");
        crate::tools::convert::run_convert(
            "# 사업 개요\n\n본문 문단입니다.\n\n## 세부 목표\n\n| 항목 | 값 |\n| --- | --- |\n| 성명 |  |",
            false,
            path.to_str().unwrap(),
            "default",
        )
        .unwrap();

        let data = run_outline(path.to_str().unwrap()).unwrap();
        assert_eq!(data.outline.headings.len(), 2);
        assert_eq!(data.outline.headings[0].text, "사업 개요");
        assert_eq!(data.outline.headings[0].level, 1);
        assert_eq!(data.outline.headings[1].text, "세부 목표");
        assert_eq!(data.outline.headings[1].level, 2);
        assert_eq!(data.outline.tables.len(), 1);
        assert_eq!(data.outline.tables[0].ordinal, 0);
        assert_eq!((data.outline.tables[0].rows, data.outline.tables[0].cols), (Some(2), Some(2)));
        assert!(data.outline.tables[0].addressable);
        assert!(data.warnings.is_empty(), "a clean document must not warn: {:?}", data.warnings);
    }

    fn fixture(rel: &str) -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(rel)
            .to_str()
            .unwrap()
            .to_string()
    }

    /// 줄 조판 캐시가 낡은 fixture 를 outline 하면, 디코드 경고
    /// (`LAYOUT_CACHE_DROPPED`)가 `warnings` 에 실려야 한다.
    #[test]
    fn outline_surfaces_decode_warnings() {
        let path = fixture("layout/stale-line-cache.hwpx");
        let data = run_outline(&path).unwrap();
        assert!(
            data.warnings.iter().any(|w| w.code == "LAYOUT_CACHE_DROPPED"),
            "outline must surface the decode warning: {:?}",
            data.warnings
        );

        let value = serde_json::to_value(&data).unwrap();
        assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED");
        assert!(!value["warnings"][0]["message"].as_str().unwrap_or_default().is_empty());
    }

    #[test]
    fn outline_missing_file_reports_file_not_found() {
        let err = run_outline("/nonexistent/e5-outline-probe.hwpx").unwrap_err();
        assert_eq!(err.code, "FILE_NOT_FOUND");
    }

    #[test]
    fn outline_non_hwpx_bytes_reports_decode_error() {
        let dir = tempfile::tempdir().unwrap();
        let garbage = dir.path().join("garbage.hwpx");
        std::fs::write(&garbage, b"not a zip").unwrap();

        let err = run_outline(garbage.to_str().unwrap()).unwrap_err();
        assert_eq!(err.code, "DECODE_ERROR");
    }

    /// The outline itself is empty here — the oversized total comes
    /// entirely from `warnings` — so this exercises the branch the
    /// whole-payload gate exists for (finding #4). Outline has no
    /// `report_path`-style externalization, so this must still error
    /// rather than silently return an oversized response.
    #[test]
    fn outline_oversized_warnings_alone_reports_output_too_large() {
        let outline = DocumentOutline {
            title: None,
            sections: Vec::new(),
            headings: Vec::new(),
            tables: Vec::new(),
            fields: Vec::new(),
            bookmarks: Vec::new(),
        };
        let huge =
            vec![ToolWarningInfo::new("STUB_OVERSIZED", "x".repeat(MAX_INLINE_RESPONSE + 1))];

        let err = build_outline_data(outline, huge).unwrap_err();
        assert_eq!(err.code, "OUTPUT_TOO_LARGE");
    }
}
