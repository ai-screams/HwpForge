//! `hwpforge_fill` — 이름 붙은 누름틀(ClickHere) 채우기 (delta edit).
//!
//! 섹션 JSON 왕복 없이 `이름 → 값` 맵만으로 문서를 채운다. 전량 preflight
//! 후 전량 적용(all-or-nothing)이며, 채워진 섹션 XML 외의 패키지 엔트리는
//! 바이트 그대로 보존된다.

use std::collections::BTreeMap;

use serde::Serialize;

use hwpforge_smithy_hwpx::{FillError, FilledField, HwpxFiller};

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
    let outcome = HwpxFiller::fill(&bytes, values).map_err(map_fill_error)?;
    write_output_file(output_path, &outcome.bytes)?;

    let size_bytes = outcome.bytes.len() as u64;
    Ok(FillData { output_path: output_path.to_string(), filled: outcome.filled, size_bytes })
}

fn map_fill_error(error: FillError) -> ToolErrorInfo {
    match error {
        FillError::EmptyValue { name } => ToolErrorInfo::new(
            "EMPTY_FIELD_VALUE",
            format!("field '{name}': empty value is not fillable"),
            "빈 값 채우기는 미지원 — 값을 지우려면 한컴에서 편집하세요.",
        ),
        FillError::UnknownField { name, available } => ToolErrorInfo::new(
            "FIELD_NOT_FOUND",
            format!("field '{name}' not found in document"),
            format!(
                "Available fields: [{}]. Use hwpforge_fields to list them.",
                available.join(", ")
            ),
        ),
        FillError::DuplicateFieldName { name, count } => ToolErrorInfo::new(
            "FIELD_NAME_AMBIGUOUS",
            format!("field '{name}' appears {count} times"),
            "같은 이름의 누름틀이 여러 개라 대상이 모호합니다 — 문서에서 이름을 유일하게 하세요.",
        ),
        FillError::UnfillableField { name, section } => ToolErrorInfo::new(
            "FIELD_NOT_FILLABLE",
            format!("field '{name}' in section {section} has no patchable body"),
            "병합-run 모호 필드 또는 빈 본문 — 한컴 재저장 또는 from-json --base 재생성이 필요합니다.",
        ),
        FillError::Workflow(e) => ToolErrorInfo::new(
            "FILL_ERROR",
            format!("fill workflow error: {e}"),
            "Check that the file is valid HWPX.",
        ),
    }
}
