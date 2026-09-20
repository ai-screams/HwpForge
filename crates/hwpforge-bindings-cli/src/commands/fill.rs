//! Fill named click-here fields with values (delta edit, preserve-first).

use std::collections::BTreeMap;
use std::path::PathBuf;

use hwpforge::ops::edit::{fill as ops_fill, FillOptions};
use hwpforge::ops::{OpsError, OpsWarning};
use hwpforge_foundation::diagnostics::WarningInfo;

use crate::compat::{self, Command};
use crate::error::{check_file_size, read_input, CliError};

/// Run the fill command.
pub fn run(file: &PathBuf, sets: &[String], output: &PathBuf, json_mode: bool) {
    let values = parse_sets(sets, json_mode);
    if values.is_empty() {
        CliError::new("NO_VALUES", "no --set name=value pairs given")
            .with_hint("예: hwpforge fill doc.hwpx --set 과제명=\"AI 문서 자동화\" -o out.hwpx")
            .exit(json_mode, 1);
    }

    check_file_size(file, json_mode);
    let bytes = read_input(file, json_mode);

    let values: Vec<(String, String)> = values.into_iter().collect();
    let outcome = match ops_fill(&bytes, &values, &FillOptions::default()) {
        Ok(o) => o,
        Err(e) => exit_ops_error(Command::Fill, e, json_mode),
    };
    // Decoder warnings (`FillOutput::warnings`, from the name-resolution
    // decode) were not surfaced pre-W5. W5 follow-up: additive — a new,
    // omit-if-empty `warnings` key in `--json`, and one `[fill]`-prefixed
    // stderr line each in text mode.
    let warnings: Vec<WarningInfo> = outcome.warnings.iter().map(OpsWarning::info).collect();

    if let Err(e) = std::fs::write(output, &outcome.bytes) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 1);
    }

    if json_mode {
        let mut result = serde_json::json!({
            "status": "ok",
            "output": output.display().to_string(),
            "filled": outcome.filled,
            "size_bytes": outcome.bytes.len(),
        });
        if !warnings.is_empty() {
            result["warnings"] = serde_json::to_value(&warnings).unwrap();
        }
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        for w in &warnings {
            eprintln!("[fill] {}: {}", w.code, w.message);
        }
        println!("Filled {} field(s) -> {}", outcome.filled.len(), output.display());
        for f in &outcome.filled {
            println!("  [{}] {}: {:?} -> filled", f.section, f.name, f.previous);
        }
    }
}

/// `name=value` 인자 목록을 맵으로 파싱한다. `=` 누락·중복 이름은 거부.
fn parse_sets(sets: &[String], json_mode: bool) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    for set in sets {
        let Some((name, value)) = set.split_once('=') else {
            CliError::new("INVALID_SET", format!("'--set {set}' is not name=value"))
                .with_hint("--set 이름=값 형식으로 지정하세요 (값에 공백이 있으면 따옴표)")
                .exit(json_mode, 1);
        };
        if values.insert(name.to_string(), value.to_string()).is_some() {
            CliError::new("DUPLICATE_SET", format!("'--set {name}=…' given more than once"))
                .exit(json_mode, 1);
        }
    }
    values
}

/// Maps an `ops::edit::fill` failure onto the frozen contract and exits.
fn exit_ops_error(cmd: Command, err: OpsError, json_mode: bool) -> ! {
    let ce = compat::cli_error(cmd, err);
    let exit = compat::exit_code(cmd, &ce);
    ce.exit(json_mode, exit);
}
