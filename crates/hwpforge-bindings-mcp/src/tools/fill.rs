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
use crate::output::{read_file_bytes, write_output_file, ToolErrorInfo};

/// Output data from a successful fill operation.
#[derive(Debug, Serialize)]
pub struct FillData {
    /// Path to the generated HWPX file.
    pub output_path: String,
    /// Fields that were filled (document order).
    pub filled: Vec<FilledField>,
    /// Size of the output file in bytes.
    pub size_bytes: u64,
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
    Ok(FillData { output_path: output_path.to_string(), filled: outcome.filled, size_bytes })
}
