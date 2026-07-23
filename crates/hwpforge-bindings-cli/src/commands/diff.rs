//! Two-document diff (E5): verify what an edit actually changed.

use std::path::PathBuf;

use hwpforge_smithy_hwpx::HwpxDiffer;

use crate::error::{check_file_size, CliError};

/// Run the diff command.
pub fn run(base: &PathBuf, revised: &PathBuf, output: Option<&PathBuf>, json_mode: bool) {
    check_file_size(base, json_mode);
    check_file_size(revised, json_mode);
    let read = |path: &PathBuf| -> Vec<u8> {
        match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", path.display()))
                    .exit(json_mode, 1);
            }
        }
    };
    let base_bytes = read(base);
    let revised_bytes = read(revised);

    let diff = match HwpxDiffer::diff(&base_bytes, &revised_bytes) {
        Ok(d) => d,
        Err(e) => {
            CliError::new("DECODE_FAILED", format!("Cannot diff: {e}")).exit(json_mode, 2);
        }
    };

    if let Some(path) = output {
        let report = serde_json::to_string_pretty(&diff).unwrap();
        if let Err(e) = std::fs::write(path, report) {
            CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", path.display()))
                .exit(json_mode, 1);
        }
    }

    if json_mode {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({ "status": "ok", "diff": diff })).unwrap()
        );
        return;
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
