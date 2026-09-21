//! List named click-here fields in an HWPX document (fill discoverability).

use std::path::PathBuf;

use hwpforge::ops::{self, OpsWarning};
use hwpforge_foundation::diagnostics::WarningInfo;

use crate::compat::{self, Command};
use crate::error::{check_file_size, read_input};

/// Run the fields command.
pub fn run(file: &PathBuf, json_mode: bool) {
    check_file_size(file, json_mode);
    let bytes = read_input(file, json_mode);

    // Decoder warnings (`FieldsOutput::warnings`) were not surfaced pre-W5
    // (matches the pre-migration behaviour, which never captured them
    // either). W5 follow-up: additive — a new, omit-if-empty `warnings` key
    // in `--json`, and one `[fields]`-prefixed stderr line each in text mode.
    let out = match ops::fields(&bytes) {
        Ok(out) => out,
        Err(e) => {
            let err = compat::cli_error(Command::Fields, e);
            let exit = compat::exit_code(Command::Fields, &err);
            err.exit(json_mode, exit);
        }
    };
    let warnings: Vec<WarningInfo> = out.warnings.iter().map(OpsWarning::info).collect();
    let fields = out.fields;

    if json_mode {
        let mut result = serde_json::json!({ "status": "ok", "fields": fields });
        if !warnings.is_empty() {
            result["warnings"] = serde_json::to_value(&warnings).unwrap();
        }
        println!("{}", serde_json::to_string(&result).unwrap());
        return;
    }
    for w in &warnings {
        eprintln!("[fields] {}: {}", w.code, w.message);
    }

    if fields.is_empty() {
        println!("No click-here fields found in {}", file.display());
        return;
    }
    println!("{} field(s) in {}:", fields.len(), file.display());
    for f in &fields {
        let name = f.name.as_deref().unwrap_or("(이름 없음)");
        let fillable = if f.fillable { "fillable" } else { "NOT fillable" };
        println!(
            "  [{}] {} = {:?} ({}; hint: {})",
            f.section,
            name,
            f.current,
            fillable,
            f.hint.as_deref().unwrap_or("-"),
        );
    }
}
