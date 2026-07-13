//! `hwpforge_fields` — 누름틀(ClickHere) 목록 조회 (fill 발견가능성 표면).

use serde::Serialize;

use hwpforge_smithy_hwpx::{FieldInfo, HwpxFiller};

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
pub fn run_fields(file_path: &str) -> Result<FieldsData, ToolErrorInfo> {
    let bytes = read_file_bytes(file_path)?;
    let fields = HwpxFiller::list_fields(&bytes).map_err(|e| {
        ToolErrorInfo::new(
            "DECODE_ERROR",
            format!("HWPX decode failed: {e}"),
            "Check that the file is valid HWPX. For .hwp files, convert with hwpforge_convert first.",
        )
    })?;
    let fillable_count = fields.iter().filter(|f| f.fillable).count();
    Ok(FieldsData { fields, fillable_count })
}
