//! List named click-here fields in an HWPX document (fill discoverability).

use std::path::PathBuf;

use hwpforge::ops;

use crate::compat::{self, Command};
use crate::error::{check_file_size, CliError};

/// Run the fields command.
pub fn run(file: &PathBuf, json_mode: bool) {
    check_file_size(file, json_mode);
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", file.display()))
                .exit(json_mode, 1);
        }
    };

    // Decoder warnings (`FieldsOutput::warnings`) are not surfaced (W3
    // report: warnings not surfaced — matches the pre-migration behaviour,
    // which never captured them either).
    let fields = match ops::fields(&bytes) {
        Ok(out) => out.fields,
        Err(e) => {
            let err = compat::cli_error(Command::Fields, e);
            let exit = compat::exit_code(Command::Fields, &err);
            err.exit(json_mode, exit);
        }
    };

    if json_mode {
        let result = serde_json::json!({ "status": "ok", "fields": fields });
        println!("{}", serde_json::to_string(&result).unwrap());
        return;
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
