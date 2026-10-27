//! Convert HWP5 to HWPX.
use crate::error::CliCode;
use hwpforge_foundation::diagnostics::OpsCode;

use std::path::Path;

use serde::Serialize;

use hwpforge_convert::ops::{convert_hwp5, ConvertHwp5Options, ConvertOpsWarning};
use hwpforge_convert::ConvertWarning;
use hwpforge_smithy_hwp5::inspect_hwp5;

use crate::compat::{self, Command};
use crate::error::{check_file_size, read_bounded, CliError};

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
    check_file_size(input, json_mode, Command::ConvertHwp5);

    // Read once with a hard cap, including FIFOs. Read failures exit 2; size refusals exit 1.
    let bytes = read_bounded(input).unwrap_or_else(|err| {
        CliError::new(CliCode::Ops(err.code()), format!("Cannot read '{}': {err}", input.display()))
            .exit(json_mode, Command::ConvertHwp5)
    });

    // Inspect the same bytes for the summary; corrupt HWP5 stays HWP5_DECODE_FAILED.
    let summary = inspect_hwp5(&bytes).unwrap_or_else(|err| {
        CliError::new(
            CliCode::Ops(OpsCode::Hwp5DecodeFailed),
            format!("Cannot decode '{}': {err}", input.display()),
        )
        .with_hint("Check that the file is a valid HWP5 document")
        .exit(json_mode, Command::ConvertHwp5)
    });

    let opts = ConvertHwp5Options::default().with_carry_layout_cache(carry_layout_cache);
    let converted = convert_hwp5(&bytes, &opts)
        .unwrap_or_else(|err| compat::exit_convert_error(Command::ConvertHwp5, err, json_mode));

    // Legacy wrote the output *inside* `hwp5_to_hwpx_with_options` itself
    // (`std::fs::write(...).map_err(Hwp5Error::Io)?`,
    // `crates/hwpforge-convert/src/lib.rs`), so a write failure there
    // (missing parent dir, permission denial) surfaced as a `Hwp5Error::Io`
    // and failed as `HWP5_CONVERT_FAILED`, exit 2, with the convert hint,
    // message keyed on the *input* path.
    // `convert_hwp5()` now returns bytes and this command does the write
    // itself. W1 uses FileWriteFailed and exit 2 while preserving the
    // message, including `Hwp5Error::Io`'s `"I/O error: {0}"` Display
    // wrapping the raw `io::Error`, not the raw error alone.
    if let Err(e) = std::fs::write(output, &converted.bytes) {
        let wrapped = hwpforge_smithy_hwp5::Hwp5Error::Io(e);
        CliError::new(
            CliCode::Ops(OpsCode::FileWriteFailed),
            format!("Cannot convert '{}' to HWPX: {wrapped}", input.display()),
        )
        .with_hint(
            "Check that the source is a supported HWP5 document and the output path is writable",
        )
        .exit(json_mode, Command::ConvertHwp5);
    }
    // Legacy re-`stat`'d the file it just wrote for `size_bytes` — a
    // failure there (e.g. the output vanishing between write and stat) was
    // `FILE_WRITE_FAILED`, exit 1, no hint. The approved 0.18 policy now
    // uses exit 2 for both write and post-write stat failures.
    let size_bytes =
        output_size(output).unwrap_or_else(|err| err.exit(json_mode, Command::ConvertHwp5));

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
    // 상한을 다 먹어 집계가 가려지는 일 방지.
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

// Both output failure paths use the same typed code and exit policy.
fn output_size(output: &Path) -> Result<u64, Box<CliError>> {
    std::fs::metadata(output).map(|meta| meta.len()).map_err(|e| {
        Box::new(CliError::new(
            CliCode::Ops(OpsCode::FileWriteFailed),
            format!("Converted output '{}' is not readable: {e}", output.display()),
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // 이것을 실패시키는 것: 후속 stat 코드를 FileReadFailed로 변경하거나 manifest 후속 stat 기대 종료를 1로 변경.
    fn output_metadata_failure_uses_write_exit() {
        let err =
            output_size(Path::new("/nonexistent-hwpforge-output-size/vanished.hwpx")).unwrap_err();
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../hwpforge/tests/data/error_contract_018.json"
        ))
        .unwrap();
        let case = manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "io.convert-hwp5.output_metadata")
            .unwrap();
        assert_eq!(serde_json::to_value(&err).unwrap()["code"], case["expected_code"]);
        assert_eq!(
            compat::exit_code(Command::ConvertHwp5, err.code),
            case["surfaces"]["cli"]["exit"].as_i64().unwrap() as i32
        );
        assert!(err.hint.is_none());
    }
}
