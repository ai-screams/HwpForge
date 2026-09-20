//! Export HWPX to editable JSON.

use std::path::PathBuf;

use hwpforge::ops::{self, ExportSectionOptions, OpsWarning, ToJsonOptions};

use crate::compat::{self, Command};
use crate::error::{check_file_size, CliError};

// Re-export shared exchange types so existing imports (`crate::commands::to_json::Exported*`) keep working.
pub use hwpforge_smithy_hwpx::{ExportedDocument, ExportedSection};

/// Prints one export warning in this command's shape.
///
/// `OpsWarning::info()` reproduces the exact legacy wording for the two
/// kinds this command printed pre-migration: `GridAddr`
/// (`TABLE_GRID_UNADDRESSABLE`, same `format!` as the old `finish_annotation`)
/// and `SectionWorkflow` (`warning.code()`/`warning.message()`, same as the
/// old inline `outcome.warning` print). Decoder warnings (`OpsWarning::Decode`,
/// for example `LAYOUT_CACHE_DROPPED`) were not surfaced pre-W5 — the
/// pre-migration CLI never captured them either — and are now printed
/// through this same shape (W5 follow-up, additive).
fn print_warning(warning: &OpsWarning, json_mode: bool) {
    let info = warning.info();
    if json_mode {
        let warn = serde_json::json!({
            "status": "warning",
            "code": info.code,
            "message": info.message,
        });
        eprintln!("{}", serde_json::to_string(&warn).unwrap());
    } else {
        eprintln!("Warning: {}", info.message);
    }
}

fn exit_ops_error(err: ops::OpsError, json_mode: bool) -> ! {
    let cli_err = compat::cli_error(Command::ToJson, err);
    let exit = compat::exit_code(Command::ToJson, &cli_err);
    cli_err.exit(json_mode, exit);
}

/// Pretty-prints the annotated export value.
fn render_pretty(value: &serde_json::Value, json_mode: bool) -> String {
    match serde_json::to_string_pretty(value) {
        Ok(s) => s,
        Err(e) => {
            CliError::new("JSON_SERIALIZE_FAILED", format!("Failed to serialize export: {e}"))
                .exit(json_mode, 2);
        }
    }
}

/// Run the to-json command.
pub fn run(
    file: &PathBuf,
    output: &PathBuf,
    section_idx: Option<usize>,
    no_styles: bool,
    json_mode: bool,
) {
    // Guard the output extension up front (same `extension()` idiom as the
    // `to-md` command), so a mistyped path fails before any work is done.
    if output.extension().and_then(|e| e.to_str()) != Some("json") {
        CliError::new(
            "INVALID_EXTENSION",
            format!("Output path must end with .json: {}", output.display()),
        )
        .exit(json_mode, 1);
    }
    check_file_size(file, json_mode);
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", file.display()))
                .exit(json_mode, 1);
        }
    };

    let json_string = if let Some(idx) = section_idx {
        let opts = ExportSectionOptions::default().with_section(idx).with_styles(!no_styles);
        let out = match ops::export_section(&bytes, &opts) {
            Ok(o) => o,
            Err(e) => exit_ops_error(e, json_mode),
        };
        for warning in &out.warnings {
            print_warning(warning, json_mode);
        }
        render_pretty(&out.section, json_mode)
    } else {
        let opts = ToJsonOptions::default().with_styles(!no_styles);
        let out = match ops::to_json(&bytes, &opts) {
            Ok(o) => o,
            Err(e) => exit_ops_error(e, json_mode),
        };
        for warning in &out.warnings {
            print_warning(warning, json_mode);
        }
        render_pretty(&out.document, json_mode)
    };

    if let Err(e) = std::fs::write(output, &json_string) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 1);
    }

    let result = serde_json::json!({
        "status": "ok",
        "output": output.display().to_string(),
        "size_bytes": json_string.len(),
        "section_only": section_idx.is_some(),
    });

    if json_mode {
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!(
            "Exported {} ({} bytes{})",
            output.display(),
            json_string.len(),
            if let Some(i) = section_idx { format!(", section {i} only") } else { String::new() }
        );
    }
}
