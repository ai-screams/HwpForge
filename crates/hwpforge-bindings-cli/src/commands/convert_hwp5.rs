//! Convert HWP5 to HWPX.

use std::path::Path;

use serde::Serialize;

use hwpforge_convert::ops::{convert_hwp5, ConvertHwp5Options, ConvertOpsWarning};
use hwpforge_convert::ConvertWarning;
use hwpforge_smithy_hwp5::inspect_hwp5_file;

use crate::compat::{self, Command};
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

    // `hwpforge_convert::ops::convert_hwp5` takes bytes and reports no
    // document summary (version/sections/paragraphs) — `inspect_hwp5_file`
    // stays for that, unchanged, which also keeps the legacy
    // `HWP5_DECODE_FAILED` message/hint byte-identical for the most-hit
    // failure (unreadable/corrupt file).
    let summary = inspect_hwp5_file(input).unwrap_or_else(|err| {
        CliError::new("HWP5_DECODE_FAILED", format!("Cannot decode '{}': {err}", input.display()))
            .with_hint("Check that the file is a valid HWP5 document")
            .exit(json_mode, 2)
    });

    // Re-reads the file `inspect_hwp5_file` already read once — the same
    // double-read the pre-migration code already did (once inside
    // `inspect_hwp5_file`, once inside `hwp5_to_hwpx_with_options`), not a
    // new cost. A failure here (the file vanishing between the two reads)
    // is mapped like the legacy second-read failure was: `HWP5_CONVERT_FAILED`.
    let bytes = std::fs::read(input).unwrap_or_else(|err| {
        CliError::new(
            "HWP5_CONVERT_FAILED",
            format!("Cannot convert '{}' to HWPX: {err}", input.display()),
        )
        .with_hint(
            "Check that the source is a supported HWP5 document and the output path is writable",
        )
        .exit(json_mode, 2)
    });

    let opts = ConvertHwp5Options::default().with_carry_layout_cache(carry_layout_cache);
    let converted = convert_hwp5(&bytes, &opts).unwrap_or_else(|err| {
        let cli_err = compat::convert_error(Command::ConvertHwp5, err);
        let exit = compat::exit_code(Command::ConvertHwp5, &cli_err);
        cli_err.exit(json_mode, exit)
    });

    // Legacy wrote the output *inside* `hwp5_to_hwpx_with_options` itself,
    // so a write failure there (missing parent dir, permission denial) was
    // just another failure of that one call — `HWP5_CONVERT_FAILED`, exit
    // 2, with the convert hint, message keyed on the *input* path (W3
    // remediation finding 8). `convert_hwp5()` now returns bytes and this
    // command does the write itself, but the failure envelope must stay
    // the one that call site produced.
    if let Err(e) = std::fs::write(output, &converted.bytes) {
        CliError::new(
            "HWP5_CONVERT_FAILED",
            format!("Cannot convert '{}' to HWPX: {e}", input.display()),
        )
        .with_hint(
            "Check that the source is a supported HWP5 document and the output path is writable",
        )
        .exit(json_mode, 2);
    }
    // Legacy re-`stat`'d the file it just wrote for `size_bytes` — a
    // failure there (e.g. the output vanishing between write and stat) was
    // `FILE_WRITE_FAILED`, exit 1, no hint (W3 remediation finding 9).
    let size_bytes = std::fs::metadata(output).map(|meta| meta.len()).unwrap_or_else(|e| {
        CliError::new(
            "FILE_WRITE_FAILED",
            format!("Converted output '{}' is not readable: {e}", output.display()),
        )
        .exit(json_mode, 1)
    });

    // `convert_hwp5` only ever emits `ConvertOpsWarning::Convert` (it never
    // decodes HWPX or renders); the `_` arm exists only because the enum is
    // `#[non_exhaustive]`.
    let inner_warnings: Vec<&ConvertWarning> = converted
        .warnings
        .iter()
        .filter_map(|w| match w {
            ConvertOpsWarning::Convert(inner) => Some(inner),
            _ => None,
        })
        .collect();

    // 집계 드롭 경고(unknown_control)를 우선 배치 — 선행 decode 경고가
    // 상한을 다 먹어 집계가 가려지는 일 방지 (독립 리뷰 Medium #6).
    let is_aggregate = |w: &&ConvertWarning| {
        matches!(
            w.as_hwp5(),
            Some(hwpforge_smithy_hwp5::Hwp5Warning::DroppedControl {
                control: "unknown_control",
                ..
            })
        )
    };
    let warning_details: Vec<String> = inner_warnings
        .iter()
        .copied()
        .filter(is_aggregate)
        .chain(inner_warnings.iter().copied().filter(|w| !is_aggregate(w)))
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
        warnings: converted.warnings.len(),
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
