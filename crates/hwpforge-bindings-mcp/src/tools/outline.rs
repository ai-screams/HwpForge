//! `hwpforge_outline` — 문서 항법 지도 (E5 읽기 표면).

use serde::Serialize;

use hwpforge_smithy_hwpx::{DocumentOutline, HwpxReader};

use crate::output::{read_file_bytes, ToolErrorInfo};

/// Output data from an outline projection.
#[derive(Debug, Serialize)]
pub struct OutlineData {
    /// The document navigation map (headings, tables, fields, bookmarks).
    #[serde(flatten)]
    pub outline: DocumentOutline,
}

/// Inline response ceiling shared with `hwpforge_to_json` (1 MB).
const MAX_INLINE_RESPONSE: usize = 1024 * 1024;

/// Build the document navigation map for an HWPX file.
pub fn run_outline(file_path: &str) -> Result<OutlineData, ToolErrorInfo> {
    let bytes = read_file_bytes(file_path)?;
    let outline = HwpxReader::outline(&bytes).map_err(|e| {
        ToolErrorInfo::new(
            "DECODE_ERROR",
            format!("HWPX decode failed: {e}"),
            "Check that the file is valid HWPX. For .hwp files, convert with hwpforge_convert first.",
        )
    })?;

    let inline_size = serde_json::to_string(&outline).map(|s| s.len()).unwrap_or(usize::MAX);
    if inline_size > MAX_INLINE_RESPONSE {
        return Err(ToolErrorInfo::new(
            "OUTPUT_TOO_LARGE",
            format!("Navigation map is {inline_size} bytes (limit {MAX_INLINE_RESPONSE})"),
            "Use the CLI instead: hwpforge outline <file> --json",
        ));
    }

    Ok(OutlineData { outline })
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
    }

    #[test]
    fn outline_missing_file_reports_file_not_found() {
        let err = run_outline("/nonexistent/e5-outline-probe.hwpx").unwrap_err();
        assert_eq!(err.code, "FILE_NOT_FOUND");
    }
}
