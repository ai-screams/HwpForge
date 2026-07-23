//! `hwpforge_read` — 표적 텍스트 읽기 (E5 읽기 표면).

use serde::Serialize;

use hwpforge_smithy_hwpx::{FieldInfo, HwpxReader, ParagraphsView, ReadError, TableView};

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
        return Err(ToolErrorInfo::new(
            "READ_TARGET_REQUIRED",
            "Pass exactly one of section, table, field",
            "section reads a paragraph range; table reads a grid text matrix; field reads a named click-here field.",
        ));
    }
    if paras.is_some() && section.is_none() {
        return Err(ToolErrorInfo::new(
            "READ_PARAS_WITHOUT_SECTION",
            "paras requires section",
            "Pass section together with paras.",
        ));
    }

    let bytes = read_file_bytes(file_path)?;

    if let Some(section) = section {
        let range = match paras {
            Some(spec) => Some(parse_paras(spec)?),
            None => None,
        };
        let view = HwpxReader::read_paragraphs(&bytes, section, range).map_err(map_read_error)?;
        return Ok(ReadData { paragraphs: Some(view), table: None, fields: None });
    }

    if let Some(ordinal) = table {
        let view = HwpxReader::read_table(&bytes, ordinal).map_err(map_read_error)?;
        return Ok(ReadData { paragraphs: None, table: Some(view), fields: None });
    }

    let name = field.expect("target validation guarantees field");
    let fields = HwpxReader::read_field(&bytes, name).map_err(map_read_error)?;
    Ok(ReadData { paragraphs: None, table: None, fields: Some(fields) })
}

fn parse_paras(spec: &str) -> Result<(usize, usize), ToolErrorInfo> {
    let parsed = match spec.split_once("..") {
        Some((a, b)) => a
            .trim()
            .parse::<usize>()
            .and_then(|from| b.trim().parse::<usize>().map(|to| (from, to)))
            .ok(),
        None => spec.trim().parse::<usize>().map(|n| (n, n)).ok(),
    };
    parsed.ok_or_else(|| {
        ToolErrorInfo::new(
            "READ_PARAS_INVALID",
            format!("Cannot parse paras {spec:?}"),
            "Use \"A..B\" (inclusive) or a single \"N\".",
        )
    })
}

fn map_read_error(err: ReadError) -> ToolErrorInfo {
    let (code, hint) = match &err {
        ReadError::Codec(_) => (
            "DECODE_ERROR",
            "Check that the file is valid HWPX. For .hwp files, convert with hwpforge_convert first.",
        ),
        ReadError::SectionOutOfRange { .. } => {
            ("READ_SECTION_OUT_OF_RANGE", "Use hwpforge_outline to see section indexes.")
        }
        ReadError::ParaRangeInvalid { .. } => {
            ("READ_PARA_RANGE_INVALID", "Use hwpforge_outline to see paragraph counts per section.")
        }
        ReadError::TableOutOfRange { .. } => {
            ("READ_TABLE_OUT_OF_RANGE", "Use hwpforge_outline to see table ordinals.")
        }
        ReadError::TableUnaddressable { .. } => {
            ("TABLE_GRID_INVALID", "This table's strict grid cannot be derived; use hwpforge_to_json.")
        }
        ReadError::FieldNotFound { .. } => {
            ("READ_FIELD_NOT_FOUND", "Use hwpforge_fields to list available field names.")
        }
    };
    ToolErrorInfo::new(code, err.to_string(), hint)
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
