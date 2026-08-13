//! Convert HWP5 to HWPX.

use std::path::Path;

use serde::Serialize;

use hwpforge_convert::{hwp5_to_hwpx_with_options, ConvertOptions};
use hwpforge_smithy_hwp5::inspect_hwp5_file;

use crate::error::{check_file_size, CliError};

/// JSON 모드에 싣는 경고 상세 상한 (집계 경고라 문서당 소수 — 폭주 방지 겸).
const MAX_WARNING_DETAILS: usize = 32;

#[derive(Serialize)]
struct ConvertHwp5Result {
    status: &'static str,
    input: String,
    output: String,
    version: String,
    sections: usize,
    paragraphs: usize,
    warnings: usize,
    /// 경고 상세 (최대 [`MAX_WARNING_DETAILS`]건 — W4: 개수만으로는 무엇이
    /// 드롭됐는지 알 수 없다는 지적 상환).
    warning_details: Vec<String>,
    size_bytes: u64,
}

/// Run the convert-hwp5 command: HWP5 -> HWPX.
pub fn run(input: &Path, output: &Path, carry_layout_cache: bool, json_mode: bool) {
    check_file_size(input, json_mode);

    let summary = inspect_hwp5_file(input).unwrap_or_else(|err| {
        CliError::new("HWP5_DECODE_FAILED", format!("Cannot decode '{}': {err}", input.display()))
            .with_hint("Check that the file is a valid HWP5 document")
            .exit(json_mode, 2)
    });

    let options = ConvertOptions::default().with_carry_layout_cache(carry_layout_cache);
    let warnings = hwp5_to_hwpx_with_options(input, output, options).unwrap_or_else(|err| {
        CliError::new(
            "HWP5_CONVERT_FAILED",
            format!("Cannot convert '{}' to HWPX: {err}", input.display()),
        )
        .with_hint(
            "Check that the source is a supported HWP5 document and the output path is writable",
        )
        .exit(json_mode, 2)
    });

    let size_bytes = std::fs::metadata(output).map(|meta| meta.len()).unwrap_or_else(|err| {
        CliError::new(
            "FILE_WRITE_FAILED",
            format!("Converted output '{}' is not readable: {err}", output.display()),
        )
        .exit(json_mode, 1)
    });

    // 집계 드롭 경고(unknown_control)를 우선 배치 — 선행 decode 경고가
    // 상한을 다 먹어 집계가 가려지는 일 방지 (독립 리뷰 Medium #6).
    let is_aggregate = |w: &&hwpforge_convert::ConvertWarning| {
        matches!(
            w.as_hwp5(),
            Some(hwpforge_smithy_hwp5::Hwp5Warning::DroppedControl {
                control: "unknown_control",
                ..
            })
        )
    };
    let warning_details: Vec<String> = warnings
        .iter()
        .filter(is_aggregate)
        .chain(warnings.iter().filter(|w| !is_aggregate(w)))
        .take(MAX_WARNING_DETAILS)
        .map(|w| format!("{w:?}"))
        .collect();
    let result = ConvertHwp5Result {
        status: "ok",
        input: input.display().to_string(),
        output: output.display().to_string(),
        version: summary.version,
        sections: summary.totals.sections,
        paragraphs: summary.totals.paragraphs,
        warnings: warnings.len(),
        warning_details,
        size_bytes,
    };

    if json_mode {
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!(
            "Converted {} -> {} (HWP {} , {} sections, {} paragraphs, {} warnings, {} bytes)",
            result.input,
            result.output,
            result.version,
            result.sections,
            result.paragraphs,
            result.warnings,
            result.size_bytes
        );
    }
}
