//! `hwpforge_diff` — 두 문서 비교 (E5 검증 표면).

use serde::Serialize;

use hwpforge_smithy_hwpx::{DocumentDiff, HwpxDiffer};

use crate::output::{read_file_bytes, ToolErrorInfo};

/// Inline response ceiling shared with `hwpforge_to_json` (1 MB).
const MAX_INLINE_RESPONSE: usize = 1024 * 1024;

/// Output data from a diff.
#[derive(Debug, Serialize)]
pub struct DiffData {
    /// The diff report; omitted when it exceeded the inline ceiling and was
    /// written to `report_path` instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<DocumentDiff>,
    /// Where the full pretty-printed report was written, if requested or
    /// required.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_path: Option<String>,
    /// One-line change summary.
    pub summary: String,
}

/// Diff two HWPX files; optionally write the full report to `output_path`.
pub fn run_diff(
    base_path: &str,
    revised_path: &str,
    output_path: Option<&str>,
) -> Result<DiffData, ToolErrorInfo> {
    let base = read_file_bytes(base_path)?;
    let revised = read_file_bytes(revised_path)?;

    let diff = HwpxDiffer::diff(&base, &revised).map_err(|e| {
        ToolErrorInfo::new(
            "DECODE_ERROR",
            format!("HWPX decode failed: {e}"),
            "Both inputs must be valid HWPX. For .hwp files, convert with hwpforge_convert first.",
        )
    })?;

    let summary = summarize(&diff);

    let report_path = match output_path {
        Some(path) => {
            let report = serde_json::to_string_pretty(&diff).map_err(|e| {
                ToolErrorInfo::new("SERIALIZE_ERROR", format!("Report serialize failed: {e}"), "")
            })?;
            std::fs::write(path, report).map_err(|e| {
                ToolErrorInfo::new(
                    "FILE_WRITE_FAILED",
                    format!("Cannot write '{path}': {e}"),
                    "Check the output path and permissions.",
                )
            })?;
            Some(path.to_string())
        }
        None => None,
    };

    let inline_size = serde_json::to_string(&diff).map(|s| s.len()).unwrap_or(usize::MAX);
    if inline_size > MAX_INLINE_RESPONSE {
        if report_path.is_none() {
            return Err(ToolErrorInfo::new(
                "OUTPUT_TOO_LARGE",
                format!("Diff report is {inline_size} bytes (limit {MAX_INLINE_RESPONSE})"),
                "Pass output_path to write the full report to a file.",
            ));
        }
        return Ok(DiffData { diff: None, report_path, summary });
    }

    Ok(DiffData { diff: Some(diff), report_path, summary })
}

fn summarize(diff: &DocumentDiff) -> String {
    if diff.identical {
        return "identical".to_string();
    }
    let s = &diff.semantic;
    format!(
        "{} field(s), {} cell(s), {} paragraph(s), {} structure, {} unclassified (+{} dropped); package +{}/-{}/~{}",
        s.field_values.len(),
        s.cells.len(),
        s.paragraphs.len(),
        s.structure.len(),
        s.raw.len(),
        s.raw_dropped,
        diff.package.added.len(),
        diff.package.removed.len(),
        diff.package.changed.len(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_self_is_identical_via_mcp_surface() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe.hwpx");
        crate::tools::convert::run_convert(
            "# 제목\n\n본문",
            false,
            path.to_str().unwrap(),
            "default",
        )
        .unwrap();

        let data = run_diff(path.to_str().unwrap(), path.to_str().unwrap(), None).unwrap();
        let diff = data.diff.expect("inline diff");
        assert!(diff.identical);
        assert_eq!(data.summary, "identical");
    }

    #[test]
    fn diff_summarizes_non_identical_documents() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.hwpx");
        let b = dir.path().join("b.hwpx");
        crate::tools::convert::run_convert(
            "# 제목\n\n본문 A",
            false,
            a.to_str().unwrap(),
            "default",
        )
        .unwrap();
        crate::tools::convert::run_convert(
            "# 제목\n\n본문 B",
            false,
            b.to_str().unwrap(),
            "default",
        )
        .unwrap();

        let data = run_diff(a.to_str().unwrap(), b.to_str().unwrap(), None).unwrap();
        let diff = data.diff.expect("inline diff");
        assert!(!diff.identical);
        assert!(data.summary.contains("paragraph"), "summary: {}", data.summary);
    }

    #[test]
    fn diff_missing_input_reports_file_not_found() {
        let err = run_diff("/nonexistent/a.hwpx", "/nonexistent/b.hwpx", None).unwrap_err();
        assert_eq!(err.code, "FILE_NOT_FOUND");
    }

    #[test]
    fn diff_non_hwpx_bytes_reports_decode_error() {
        let dir = tempfile::tempdir().unwrap();
        let garbage = dir.path().join("garbage.hwpx");
        std::fs::write(&garbage, b"not a zip").unwrap();

        let err = run_diff(garbage.to_str().unwrap(), garbage.to_str().unwrap(), None).unwrap_err();
        assert_eq!(err.code, "DECODE_ERROR");
    }

    #[test]
    fn diff_unwritable_report_path_reports_write_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe.hwpx");
        crate::tools::convert::run_convert("본문", false, path.to_str().unwrap(), "default")
            .unwrap();

        let err = run_diff(
            path.to_str().unwrap(),
            path.to_str().unwrap(),
            Some("/nonexistent-dir/report.json"),
        )
        .unwrap_err();
        assert_eq!(err.code, "FILE_WRITE_FAILED");
    }

    #[test]
    fn diff_writes_report_file_when_requested() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe.hwpx");
        crate::tools::convert::run_convert("본문", false, path.to_str().unwrap(), "default")
            .unwrap();
        let report = dir.path().join("report.json");

        let data = run_diff(
            path.to_str().unwrap(),
            path.to_str().unwrap(),
            Some(report.to_str().unwrap()),
        )
        .unwrap();
        assert_eq!(data.report_path.as_deref(), report.to_str());
        assert!(report.exists());
    }
}
