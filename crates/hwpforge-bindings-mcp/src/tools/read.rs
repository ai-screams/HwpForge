//! `hwpforge_read` — 표적 텍스트 읽기 (E5 읽기 표면).

use serde::Serialize;

use hwpforge::ops::{self, OpsError, ReadOptions};
use hwpforge_foundation::diagnostics::OpsCode;
use hwpforge_smithy_hwpx::{FieldInfo, ParagraphsView, TableView};

use crate::compat::{self, Tool};
use crate::output::{read_file_bytes, ToolErrorInfo};

/// Output data from a targeted read (exactly one member is set).
#[derive(Debug, Serialize)]
pub struct ReadData {
    /// Paragraph-range projection (`section` target).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paragraphs: Option<ParagraphsView>,
    /// Table grid projection (`table` target).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<TableView>,
    /// Field matches (`field` target).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<FieldInfo>>,
}

impl ReadData {
    /// One-line summary for the 3-layer output.
    pub fn summary(&self) -> String {
        if let Some(p) = &self.paragraphs {
            format!(
                "section {}: paragraphs {}..={} ({})",
                p.section,
                p.from,
                p.to,
                p.paragraphs.len()
            )
        } else if let Some(t) = &self.table {
            format!(
                "table {}: {}x{} grid, {} anchor cell(s)",
                t.ordinal,
                t.rows,
                t.cols,
                t.cells.len()
            )
        } else if let Some(f) = &self.fields {
            format!("{} field match(es)", f.len())
        } else {
            "empty read".to_string()
        }
    }
}

/// Perform a targeted read. Exactly one of `section`/`table`/`field` must be
/// set; `paras` ("A..B" inclusive or "N") requires `section`.
///
/// The target-count and paras-without-section rejections, and every read
/// error, are `ops::read`'s own (byte-for-byte the same rules this file used
/// to run locally — see `hwpforge/src/ops/read.rs`'s module docs); only the
/// legacy `(code, hint)` spelling comes from `compat::tool_error`. Decoder
/// warnings (`ops::ReadOutput::warnings`) are not surfaced: `ReadData` has no
/// field for them (schema freeze) — see the W2 report.
///
/// The two argument-shape rejections are re-run here, before the file is
/// read: `ops::read` performs the same checks, but only after the caller has
/// already decoded the bytes, and the legacy tool's precedence was argument
/// validation before file I/O — a nonexistent path with a bad argument shape
/// must still report `READ_TARGET_REQUIRED`/`READ_PARAS_WITHOUT_SECTION`,
/// not `FILE_NOT_FOUND`. These are the exact rejections `ops::read` would
/// raise for the same inputs (see its module docs), routed through the same
/// `compat::tool_error` so the `(code, hint)` cannot drift from the table.
pub fn run_read(
    file_path: &str,
    section: Option<usize>,
    paras: Option<&str>,
    table: Option<usize>,
    field: Option<&str>,
) -> Result<ReadData, ToolErrorInfo> {
    let targets = usize::from(section.is_some())
        + usize::from(table.is_some())
        + usize::from(field.is_some());
    if targets != 1 {
        return Err(compat::tool_error(
            Tool::Read,
            OpsError::Rejected {
                code: OpsCode::ReadTargetRequired,
                reason: "Pass exactly one of --section, --table, --field".into(),
            },
        ));
    }
    if paras.is_some() && section.is_none() {
        return Err(compat::tool_error(
            Tool::Read,
            OpsError::Rejected {
                code: OpsCode::ReadParasWithoutSection,
                reason: "--paras requires --section".into(),
            },
        ));
    }

    let bytes = read_file_bytes(file_path)?;

    let mut opts = ReadOptions::default();
    if let Some(section) = section {
        opts = opts.with_section(section);
    }
    if let Some(paras) = paras {
        opts = opts.with_paras(paras);
    }
    if let Some(table) = table {
        opts = opts.with_table(table);
    }
    if let Some(field) = field {
        opts = opts.with_field(field);
    }

    let out = ops::read(&bytes, &opts).map_err(|e| compat::tool_error(Tool::Read, e))?;
    Ok(ReadData { paragraphs: out.paragraphs, table: out.table, fields: out.fields })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_doc(dir: &tempfile::TempDir) -> String {
        let path = dir.path().join("probe.hwpx");
        crate::tools::convert::run_convert(
            "# 사업 개요\n\n본문 문단입니다.\n\n| 항목 | 값 |\n| --- | --- |\n| 성명 |  |",
            false,
            path.to_str().unwrap(),
            "default",
        )
        .unwrap();
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn read_section_via_mcp_surface_reports_kinds_and_markers() {
        let dir = tempfile::tempdir().unwrap();
        let path = probe_doc(&dir);

        let data = run_read(&path, Some(0), None, None, None).unwrap();
        let view = data.paragraphs.expect("paragraphs target");
        assert!(!view.paragraphs.is_empty());
        assert!(view.paragraphs.iter().any(|p| !p.contains.is_empty()), "table marker expected");
    }

    #[test]
    fn read_table_via_mcp_surface_returns_grid() {
        let dir = tempfile::tempdir().unwrap();
        let path = probe_doc(&dir);

        let data = run_read(&path, None, None, Some(0), None).unwrap();
        let table = data.table.expect("table target");
        assert_eq!((table.rows, table.cols), (2, 2));
        assert!(table.cells.iter().any(|c| c.text.contains("성명")));
    }

    #[test]
    fn read_rejects_zero_or_multiple_targets() {
        let dir = tempfile::tempdir().unwrap();
        let path = probe_doc(&dir);

        let err = run_read(&path, None, None, None, None).unwrap_err();
        assert_eq!(err.code, "READ_TARGET_REQUIRED");
        let err = run_read(&path, Some(0), None, Some(0), None).unwrap_err();
        assert_eq!(err.code, "READ_TARGET_REQUIRED");
    }

    #[test]
    fn argument_guards_run_before_file_access() {
        // Legacy precedence: a bad argument shape is reported even when the
        // path does not exist, because both guards run before `read_file_bytes`.
        let missing = "/nonexistent/e5-read-guard-probe.hwpx";

        let err = run_read(missing, None, None, None, None).unwrap_err();
        assert_eq!(err.code, "READ_TARGET_REQUIRED");

        let err = run_read(missing, None, Some("0..1"), Some(0), None).unwrap_err();
        assert_eq!(err.code, "READ_PARAS_WITHOUT_SECTION");
    }

    #[test]
    fn read_paras_validation_and_range_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = probe_doc(&dir);

        let err = run_read(&path, None, Some("0..1"), Some(0), None).unwrap_err();
        assert_eq!(err.code, "READ_PARAS_WITHOUT_SECTION");
        let err = run_read(&path, Some(0), Some("abc"), None, None).unwrap_err();
        assert_eq!(err.code, "READ_PARAS_INVALID");
        let err = run_read(&path, Some(0), Some("5..1"), None, None).unwrap_err();
        assert_eq!(err.code, "READ_PARA_RANGE_INVALID");
        let err = run_read(&path, Some(99), None, None, None).unwrap_err();
        assert_eq!(err.code, "READ_SECTION_OUT_OF_RANGE");

        let view = run_read(&path, Some(0), Some("0..0"), None, None).unwrap().paragraphs.unwrap();
        assert_eq!((view.from, view.to), (0, 0));
    }

    #[test]
    fn read_field_and_table_error_mappings() {
        let dir = tempfile::tempdir().unwrap();
        let path = probe_doc(&dir);

        let err = run_read(&path, None, None, Some(42), None).unwrap_err();
        assert_eq!(err.code, "READ_TABLE_OUT_OF_RANGE");
        let err = run_read(&path, None, None, None, Some("없는이름")).unwrap_err();
        assert_eq!(err.code, "READ_FIELD_NOT_FOUND");
    }

    #[test]
    fn read_non_hwpx_bytes_reports_decode_error() {
        let dir = tempfile::tempdir().unwrap();
        let garbage = dir.path().join("garbage.hwpx");
        std::fs::write(&garbage, b"not a zip").unwrap();

        let err = run_read(garbage.to_str().unwrap(), Some(0), None, None, None).unwrap_err();
        assert_eq!(err.code, "DECODE_ERROR");
    }

    #[test]
    fn summary_covers_every_target_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = probe_doc(&dir);

        let s = run_read(&path, Some(0), None, None, None).unwrap().summary();
        assert!(s.starts_with("section 0"), "summary: {s}");
        let s = run_read(&path, None, None, Some(0), None).unwrap().summary();
        assert!(s.starts_with("table 0"), "summary: {s}");

        let empty = ReadData { paragraphs: None, table: None, fields: None };
        assert_eq!(empty.summary(), "empty read");
        let fields = ReadData { paragraphs: None, table: None, fields: Some(Vec::new()) };
        assert_eq!(fields.summary(), "0 field match(es)");
    }
}
