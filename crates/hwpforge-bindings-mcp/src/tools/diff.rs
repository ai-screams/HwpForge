//! `hwpforge_diff` — 두 문서 비교 (E5 검증 표면).

use serde::Serialize;

use hwpforge::ops;
use hwpforge_smithy_hwpx::DocumentDiff;

use crate::compat::{self, Tool};
use crate::output::{read_file_bytes, ToolErrorInfo, ToolWarningInfo};

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
    /// Decoder warnings from **both** inputs (`ops::DiffOutput::warnings`):
    /// every warning from `base_path` first, then every warning from
    /// `revised_path`, each group in decoder order — the same provenance
    /// rule `ops::diff` documents. Omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ToolWarningInfo>,
}

/// Diff two HWPX files; optionally write the full report to `output_path`.
pub fn run_diff(
    base_path: &str,
    revised_path: &str,
    output_path: Option<&str>,
) -> Result<DiffData, ToolErrorInfo> {
    let base = read_file_bytes(base_path)?;
    let revised = read_file_bytes(revised_path)?;

    let out = ops::diff(&base, &revised).map_err(|e| compat::tool_error(Tool::Diff, e))?;
    let diff = out.diff;
    let warnings: Vec<ToolWarningInfo> = out.warnings.iter().map(compat::warning).collect();

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

    build_diff_data(diff, summary, report_path, warnings)
}

/// Builds the final [`DiffData`], gating on the complete serialized
/// response — including `warnings` — rather than on `diff` alone: a
/// document pair with many decode warnings but a small diff could
/// otherwise slip past a narrower check while still exceeding the real
/// inline ceiling.
///
/// Split out from [`run_diff`] so a test can exercise the gate with a
/// synthetic oversized `warnings` list, without needing a fixture large
/// enough to trigger it for real.
fn build_diff_data(
    diff: DocumentDiff,
    summary: String,
    report_path: Option<String>,
    warnings: Vec<ToolWarningInfo>,
) -> Result<DiffData, ToolErrorInfo> {
    let mut data = DiffData { diff: Some(diff), report_path, summary, warnings };

    let inline_size = serde_json::to_string(&data).map(|s| s.len()).unwrap_or(usize::MAX);
    if inline_size > MAX_INLINE_RESPONSE {
        if data.report_path.is_none() {
            return Err(ToolErrorInfo::new(
                "OUTPUT_TOO_LARGE",
                format!("Diff response is {inline_size} bytes (limit {MAX_INLINE_RESPONSE})"),
                "Pass output_path to write the full report to a file.",
            ));
        }
        // The full diff already lives at `report_path`; externalize it by
        // dropping the inline copy and return `summary` + `warnings`
        // instead. If `warnings` alone still exceeds the ceiling there is
        // nowhere left to put them — the same blind spot `to_json`'s own
        // gate has for its `warnings` field — so this does not re-check;
        // the response goes out anyway rather than erroring on a part of
        // the payload that has no externalization path of its own.
        data.diff = None;
    }
    Ok(data)
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
        assert!(data.warnings.is_empty(), "a clean pair must not warn: {:?}", data.warnings);
    }

    fn fixture(rel: &str) -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(rel)
            .to_str()
            .unwrap()
            .to_string()
    }

    /// 줄 조판 캐시가 낡은 fixture 를 base 와 revised 양쪽에 쓰면, 두 디코드
    /// 모두의 경고(`LAYOUT_CACHE_DROPPED`)가 base-then-revised 순서로
    /// `warnings` 에 실려야 한다.
    #[test]
    fn diff_surfaces_decode_warnings_from_both_inputs() {
        let path = fixture("layout/stale-line-cache.hwpx");
        let data = run_diff(&path, &path, None).unwrap();
        let count = data.warnings.iter().filter(|w| w.code == "LAYOUT_CACHE_DROPPED").count();
        assert_eq!(
            count, 2,
            "diffing the fixture against itself must surface one warning per side: {:?}",
            data.warnings
        );

        let value = serde_json::to_value(&data).unwrap();
        assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED");
        assert!(!value["warnings"][0]["message"].as_str().unwrap_or_default().is_empty());
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

    /// `diff` itself (an identical-pair report) is tiny here — the
    /// oversized total comes entirely from `warnings` — so this exercises
    /// the branch the whole-payload gate exists for (finding #3): a small
    /// diff and a large `warnings` list must still take the externalized
    /// path when a `report_path` is available, not slip past unnoticed.
    #[test]
    fn diff_oversized_warnings_alone_take_the_externalized_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe.hwpx");
        crate::tools::convert::run_convert("본문", false, path.to_str().unwrap(), "default")
            .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let out = ops::diff(&bytes, &bytes).unwrap();
        assert!(out.diff.identical, "guard: a probe diffed against itself must be identical");
        let summary = summarize(&out.diff);

        let report = dir.path().join("report.json");
        let huge =
            vec![ToolWarningInfo::new("STUB_OVERSIZED", "x".repeat(MAX_INLINE_RESPONSE + 1))];

        let data =
            build_diff_data(out.diff, summary, Some(report.to_str().unwrap().to_string()), huge)
                .expect("a report_path must externalize the diff instead of erroring");

        assert!(data.diff.is_none(), "diff must be dropped once the complete payload is oversized");
        assert_eq!(data.report_path.as_deref(), report.to_str());
        assert_eq!(data.warnings.len(), 1);
    }
}
