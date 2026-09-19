//! `hwpforge_fill` — 이름 붙은 누름틀(ClickHere) 채우기 (delta edit).
//!
//! 섹션 JSON 왕복 없이 `이름 → 값` 맵만으로 문서를 채운다. 전량 preflight
//! 후 전량 적용(all-or-nothing)이며, 채워진 섹션 XML 외의 패키지 엔트리는
//! 바이트 그대로 보존된다.

use std::collections::BTreeMap;

use serde::Serialize;

use hwpforge::ops;
use hwpforge_smithy_hwpx::FilledField;

use crate::compat::{self, Tool};
use crate::output::{read_file_bytes, write_output_file, ToolErrorInfo, ToolWarningInfo};

/// Output data from a successful fill operation.
#[derive(Debug, Serialize)]
pub struct FillData {
    /// Path to the generated HWPX file.
    pub output_path: String,
    /// Fields that were filled (document order).
    pub filled: Vec<FilledField>,
    /// Size of the output file in bytes.
    pub size_bytes: u64,
    /// Decode warnings from resolving the field names (e.g. a dropped
    /// layout cache) — `ops::FillOutput::warnings`. Omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ToolWarningInfo>,
}

/// Fill named click-here fields with values.
pub fn run_fill(
    file_path: &str,
    values: &BTreeMap<String, String>,
    output_path: &str,
) -> Result<FillData, ToolErrorInfo> {
    if !output_path.ends_with(".hwpx") {
        return Err(ToolErrorInfo::new(
            "INVALID_EXTENSION",
            format!("Output path must end with .hwpx: {output_path}"),
            "Use a .hwpx extension for the output file.",
        ));
    }
    if values.is_empty() {
        return Err(ToolErrorInfo::new(
            "NO_VALUES",
            "values map is empty",
            "Pass at least one name→value pair. Use hwpforge_fields to discover names.",
        ));
    }

    let bytes = read_file_bytes(file_path)?;
    // `values` is already a `BTreeMap`, so keys are unique by construction —
    // `ops::fill`'s own "given more than once" rejection can never trigger
    // through this call site.
    let pairs: Vec<(String, String)> =
        values.iter().map(|(name, value)| (name.clone(), value.clone())).collect();
    let outcome = ops::fill(&bytes, &pairs, &ops::FillOptions::default())
        .map_err(|e| compat::tool_error(Tool::Fill, e))?;
    write_output_file(output_path, &outcome.bytes)?;

    let size_bytes = outcome.bytes.len() as u64;
    let warnings: Vec<ToolWarningInfo> = outcome.warnings.iter().map(compat::warning).collect();
    Ok(FillData {
        output_path: output_path.to_string(),
        filled: outcome.filled,
        size_bytes,
        warnings,
    })
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
    fn fill_via_mcp_surface_has_no_warnings_on_a_clean_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe.hwpx");
        crate::tools::convert::run_convert("성명: (   )", false, path.to_str().unwrap(), "default")
            .unwrap();

        // 클린 문서는 채울 필드가 없으므로 NO_VALUES 인 값 맵이 아니라, 채워질
        // 필드가 하나도 없는 문서에서도 warnings 만은 확인 가능해야 한다 —
        // 여기선 실패 경로(FIELD_NOT_FOUND)라도 채우기 전 디코드 자체가
        // 경고를 내지 않는다는 것만 확인한다.
        let values = std::collections::BTreeMap::from([("없는이름".to_string(), "x".to_string())]);
        let out = dir.path().join("out.hwpx");
        let err = run_fill(path.to_str().unwrap(), &values, out.to_str().unwrap()).unwrap_err();
        assert_eq!(err.code, "FIELD_NOT_FOUND");
    }

    /// 줄 조판 캐시가 낡은 fixture 를 채우면, fill 이 이름 해석을 위해 돌린
    /// 디코드의 경고(`LAYOUT_CACHE_DROPPED`)가 `warnings` 에 실려야 한다.
    #[test]
    fn fill_surfaces_decode_warnings() {
        let path = fixture("layout/stale-line-cache.hwpx");
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.hwpx");
        let values =
            std::collections::BTreeMap::from([("user_email".to_string(), "a@b.c".to_string())]);

        let data = run_fill(&path, &values, out.to_str().unwrap()).unwrap();
        assert!(
            data.warnings.iter().any(|w| w.code == "LAYOUT_CACHE_DROPPED"),
            "fill must surface the decode warning: {:?}",
            data.warnings
        );
    }
}
