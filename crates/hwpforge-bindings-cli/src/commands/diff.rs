//! Two-document diff (E5): verify what an edit actually changed.

use std::path::PathBuf;

use hwpforge::ops::{self, OpsWarning};
use hwpforge_foundation::diagnostics::WarningInfo;

use crate::compat::{self, Command};
use crate::error::{check_file_size, read_input, CliError};

/// Run the diff command.
pub fn run(base: &PathBuf, revised: &PathBuf, output: Option<&PathBuf>, json_mode: bool) {
    check_file_size(base, json_mode);
    check_file_size(revised, json_mode);
    let base_bytes = read_input(base, json_mode);
    let revised_bytes = read_input(revised, json_mode);

    // Decoder warnings from both inputs (`DiffOutput::warnings`) were not
    // surfaced pre-W5 (the pre-migration CLI never captured them either). W5
    // follow-up: additive — a new, omit-if-empty `warnings` key in `--json`,
    // and one `[diff]`-prefixed stderr line each in text mode.
    let out = match ops::diff(&base_bytes, &revised_bytes) {
        Ok(out) => out,
        Err(e) => {
            let err = compat::cli_error(Command::Diff, e);
            let exit = compat::exit_code(Command::Diff, &err);
            err.exit(json_mode, exit);
        }
    };
    let warnings: Vec<WarningInfo> = out.warnings.iter().map(OpsWarning::info).collect();
    let diff = out.diff;

    if let Some(path) = output {
        let report = serde_json::to_string_pretty(&diff).unwrap();
        if let Err(e) = std::fs::write(path, report) {
            CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", path.display()))
                .exit(json_mode, 1);
        }
    }

    if json_mode {
        let mut result = serde_json::json!({ "status": "ok", "diff": diff });
        if !warnings.is_empty() {
            result["warnings"] = serde_json::to_value(&warnings).unwrap();
        }
        println!("{}", serde_json::to_string(&result).unwrap());
        return;
    }
    for w in &warnings {
        eprintln!("[diff] {}: {}", w.code, w.message);
    }

    if diff.identical {
        println!("identical: no semantic or package differences");
        return;
    }

    let s = &diff.semantic;
    if !s.field_values.is_empty() {
        println!("Fields:");
        for f in &s.field_values {
            println!(
                "  {} {:?}: {:?} -> {:?}",
                f.name,
                f.kind,
                f.before.as_deref().unwrap_or("-"),
                f.after.as_deref().unwrap_or("-"),
            );
        }
    }
    if !s.cells.is_empty() {
        println!("Cells:");
        for c in &s.cells {
            println!("  table {} [{},{}]: {:?} -> {:?}", c.table, c.row, c.col, c.before, c.after);
        }
    }
    if !s.paragraphs.is_empty() {
        println!("Paragraphs:");
        for p in &s.paragraphs {
            println!(
                "  [s{} p{}] {:?}: {:?} -> {:?}",
                p.at.section,
                p.at.para,
                p.kind,
                p.before.as_deref().unwrap_or("-"),
                p.after.as_deref().unwrap_or("-"),
            );
        }
    }
    if !s.structure.is_empty() {
        println!("Structure:");
        for c in &s.structure {
            println!("  {}: {} -> {}", c.scope, c.before, c.after);
        }
    }
    if !s.raw.is_empty() {
        println!("Unclassified:");
        for r in &s.raw {
            println!("  {} — {}", r.path, r.detail);
        }
        if s.raw_dropped > 0 {
            println!("  … {} more unclassified change(s) dropped", s.raw_dropped);
        }
    }
    if !diff.package.is_empty() {
        println!(
            "Package entries: {} added, {} removed, {} changed",
            diff.package.added.len(),
            diff.package.removed.len(),
            diff.package.changed.len(),
        );
        for path in &diff.package.changed {
            println!("  changed: {path}");
        }
        for path in &diff.package.added {
            println!("  added:   {path}");
        }
        for path in &diff.package.removed {
            println!("  removed: {path}");
        }
    }
    println!("Note: {}", diff.note);
}
