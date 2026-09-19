//! `hwpforge_fields` — 누름틀(ClickHere) 목록 조회 (fill 발견가능성 표면).

use serde::Serialize;

use hwpforge::ops;
use hwpforge_smithy_hwpx::FieldInfo;

use crate::compat::{self, Tool};
use crate::output::{read_file_bytes, ToolErrorInfo, ToolWarningInfo};

/// Output data from a fields listing.
#[derive(Debug, Serialize)]
pub struct FieldsData {
    /// All click-here fields in document order.
    pub fields: Vec<FieldInfo>,
    /// How many of them are fillable via `hwpforge_fill`.
    pub fillable_count: usize,
    /// Decoder warnings for this document (`ops::FieldsOutput::warnings`).
    /// Omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ToolWarningInfo>,
}

/// List named click-here fields in an HWPX document.
pub fn run_fields(file_path: &str) -> Result<FieldsData, ToolErrorInfo> {
    let bytes = read_file_bytes(file_path)?;
    let out = ops::fields(&bytes).map_err(|e| compat::tool_error(Tool::Fields, e))?;
    let fillable_count = out.fields.iter().filter(|f| f.fillable).count();
    let warnings: Vec<ToolWarningInfo> = out.warnings.iter().map(compat::warning).collect();
    Ok(FieldsData { fields: out.fields, fillable_count, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(rel: &str) -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(rel)
            .to_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn fields_via_mcp_surface_has_no_warnings_on_a_clean_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe.hwpx");
        crate::tools::convert::run_convert("성명: (   )", false, path.to_str().unwrap(), "default")
            .unwrap();

        let data = run_fields(path.to_str().unwrap()).unwrap();
        assert!(data.warnings.is_empty(), "a clean document must not warn: {:?}", data.warnings);
    }

    /// 줄 조판 캐시가 낡은 fixture 를 조회하면, 디코드 경고
    /// (`LAYOUT_CACHE_DROPPED`)가 `warnings` 에 실려야 한다.
    #[test]
    fn fields_surfaces_decode_warnings() {
        let path = fixture("layout/stale-line-cache.hwpx");
        let data = run_fields(&path).unwrap();
        assert!(
            data.warnings.iter().any(|w| w.code == "LAYOUT_CACHE_DROPPED"),
            "fields must surface the decode warning: {:?}",
            data.warnings
        );
    }
}
