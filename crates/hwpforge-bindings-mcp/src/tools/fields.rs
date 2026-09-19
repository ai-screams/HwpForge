//! `hwpforge_fields` — 누름틀(ClickHere) 목록 조회 (fill 발견가능성 표면).

use serde::Serialize;

use hwpforge::ops;
use hwpforge_smithy_hwpx::FieldInfo;

use crate::compat::{self, Tool};
use crate::output::{read_file_bytes, ToolErrorInfo};

/// Output data from a fields listing.
#[derive(Debug, Serialize)]
pub struct FieldsData {
    /// All click-here fields in document order.
    pub fields: Vec<FieldInfo>,
    /// How many of them are fillable via `hwpforge_fill`.
    pub fillable_count: usize,
}

/// List named click-here fields in an HWPX document.
///
/// Decoder warnings (`ops::FieldsOutput::warnings`) are not surfaced:
/// `FieldsData` has no field for them (schema freeze) — see the W2 report.
pub fn run_fields(file_path: &str) -> Result<FieldsData, ToolErrorInfo> {
    let bytes = read_file_bytes(file_path)?;
    let out = ops::fields(&bytes).map_err(|e| compat::tool_error(Tool::Fields, e))?;
    let fillable_count = out.fields.iter().filter(|f| f.fillable).count();
    Ok(FieldsData { fields: out.fields, fillable_count })
}
