//! Convert JSON back to HWPX.

use std::path::PathBuf;

use crate::compat::{self, Command};
use crate::error::{check_file_size, read_bounded, read_input_string, CliError};
use hwpforge::ops::{self, FromJsonOptions};

/// Run the from-json command.
pub fn run(input: &PathBuf, output: &PathBuf, base: &Option<PathBuf>, json_mode: bool) {
    check_file_size(input, json_mode);

    let json_str = read_input_string(input, json_mode);

    // Legacy two-step JSON_PARSE_FAILED classification (5ff81af `run`):
    // syntax errors ("Invalid JSON: {e}", no hint) versus a schema mismatch
    // (same code/message shape, plus the "matches the HwpForge document
    // schema" hint). `ops::from_json` folds both into one `OpsError::Json`
    // (its own `serde_json::from_str::<Value>` reparse and the
    // `ExportedDocument::deserialize` step share that variant — no way to
    // tell them apart from inside `ops`). This preflight keeps the syntax
    // case byte-identical and pre-clears it, so any `JSON_PARSE_FAILED`
    // `ops::from_json` still returns below is necessarily the schema case;
    // the hint is attached there.
    if let Err(e) = serde_json::from_str::<serde_json::Value>(&json_str) {
        CliError::new("JSON_PARSE_FAILED", format!("Invalid JSON: {e}")).exit(json_mode, 2);
    }

    // Image store: inherit from base HWPX if provided. Read up front —
    // `ops::from_json` takes the base bytes directly and decodes them
    // itself, but a missing/oversized base file is still a local
    // FILE_READ_FAILED/INPUT_TOO_LARGE, same as before.
    let mut opts = FromJsonOptions::default();
    if let Some(base_path) = base {
        check_file_size(base_path, json_mode);
        let base_bytes = match read_bounded(base_path) {
            Ok(b) => b,
            Err(e) => {
                CliError::new(
                    "FILE_READ_FAILED",
                    format!("Cannot read base '{}': {e}", base_path.display()),
                )
                .exit(json_mode, 1);
            }
        };
        opts = opts.with_base(base_bytes);
    }

    // `ops::from_json` parses, verifies any supplied grid addresses, falls
    // back to the "default" preset when the JSON carries no styles,
    // validates, inherits images from `base`, and encodes — reproducing
    // this command's pre-migration logic in one call.
    let outcome = match ops::from_json(&json_str, &opts) {
        Ok(o) => o,
        Err(e) => {
            let err = compat::cli_error(Command::FromJson, e);
            let exit = compat::exit_code(Command::FromJson, &err);
            // The syntax case already exited above, so a JSON_PARSE_FAILED
            // reaching here is necessarily the schema-mismatch case —
            // restores the legacy hint the shared table can't carry (two
            // call sites, one `OpsError::Json` variant; see the preflight
            // comment above).
            let err = if err.code == "JSON_PARSE_FAILED" {
                err.with_hint(
                    "Ensure the JSON matches the HwpForge document schema (run 'hwpforge schema document')",
                )
            } else {
                err
            };
            err.exit(json_mode, exit);
        }
    };
    let bytes = outcome.bytes;
    // 인코드 경고(각주 번호 머리 생략 등)를 무음 폐기하지 않는다.
    let encode_warnings: Vec<String> = outcome.warnings.iter().map(|w| w.info().message).collect();
    for w in &encode_warnings {
        eprintln!("[from-json] {w}");
    }

    if let Err(e) = std::fs::write(output, &bytes) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 1);
    }

    let result = serde_json::json!({
        "status": "ok",
        "output": output.display().to_string(),
        "sections": outcome.sections,
        "size_bytes": bytes.len(),
        "warnings": encode_warnings,
    });

    if json_mode {
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!(
            "Generated {} ({} sections, {} bytes)",
            output.display(),
            outcome.sections,
            bytes.len()
        );
    }
}
