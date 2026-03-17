//! Convert HWP5 to HWPX.

use std::path::Path;

use serde::Serialize;

use hwpforge_smithy_hwp5::{hwp5_to_hwpx, inspect_hwp5_file};

use crate::error::{check_file_size, CliError};

#[derive(Serialize)]
struct ConvertHwp5Result {
    status: &'static str,
    input: String,
    output: String,
    version: String,
    sections: usize,
    paragraphs: usize,
    warnings: usize,
    size_bytes: u64,
}

/// Run the convert-hwp5 command: HWP5 -> HWPX.
pub fn run(input: &Path, output: &Path, json_mode: bool) {
    check_file_size(input, json_mode);

    let summary = inspect_hwp5_file(input).unwrap_or_else(|err| {
        CliError::new("HWP5_DECODE_FAILED", format!("Cannot decode '{}': {err}", input.display()))
            .with_hint("Check that the file is a valid HWP5 document")
            .exit(json_mode, 2)
    });

    let warnings = hwp5_to_hwpx(input, output).unwrap_or_else(|err| {
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

    let result = ConvertHwp5Result {
        status: "ok",
        input: input.display().to_string(),
        output: output.display().to_string(),
        version: summary.version,
        sections: summary.totals.sections,
        paragraphs: summary.totals.paragraphs,
        warnings: warnings.len(),
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
