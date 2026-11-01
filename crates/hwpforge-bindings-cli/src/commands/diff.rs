//! Two-document diff: verify what an edit actually changed.
use crate::error::CliCode;
use hwpforge_foundation::diagnostics::OpsCode;

use std::path::{Path, PathBuf};

use hwpforge::ops::{self, OpsWarning};
use hwpforge_foundation::diagnostics::WarningInfo;

use crate::compat::{self, Command};
use crate::error::{check_file_size, CliError};

/// Run the diff command.
pub fn run(base: &Path, revised: &Path, output: Option<&PathBuf>, json_mode: bool) {
    check_file_size(base, json_mode, Command::Diff);
    check_file_size(revised, json_mode, Command::Diff);
    let base_bytes = crate::input::read_input_hwpx(base, json_mode, Command::Diff);
    let revised_bytes = crate::error::read_input(revised, json_mode, Command::Diff);
    let revised_bytes = match crate::input::try_open_input_hwpx(revised_bytes, json_mode) {
        Ok(revised) => revised,
        Err(error) => {
            // 이것을 실패시키는 것: revised 변환 실패 시 base 디코드 오류 확인 생략.
            if let Err(error) = ops::inspect(&base_bytes, &ops::InspectOptions::default()) {
                compat::exit_ops_error(Command::Diff, error, json_mode);
            }
            compat::convert_error(Command::Diff, error).exit(json_mode, Command::Diff)
        }
    };

    // Decoder warnings from both inputs (`DiffOutput::warnings`) were not
    // surfaced previously (the pre-migration CLI never captured them either).
    // follow-up: additive — a new, omit-if-empty `warnings` key in `--json`,
    // and one `[diff]`-prefixed stderr line each in text mode.
    let out = match ops::diff(&base_bytes, &revised_bytes) {
        Ok(out) => out,
        Err(e) => compat::exit_ops_error(Command::Diff, e, json_mode),
    };
    let warnings: Vec<WarningInfo> = out.warnings.iter().map(OpsWarning::info).collect();
    let diff = out.diff;

    if let Some(path) = output {
        base_bytes.protect_source(base, path, json_mode, Command::Diff);
        revised_bytes.protect_source(revised, path, json_mode, Command::Diff);
        let report = serde_json::to_string_pretty(&diff).unwrap();
        if let Err(e) = std::fs::write(path, report) {
            CliError::new(
                CliCode::Ops(OpsCode::FileWriteFailed),
                format!("Cannot write '{}': {e}", path.display()),
            )
            .exit(json_mode, Command::Diff);
        }
    }

    if json_mode {
        let mut result = serde_json::json!({ "status": "ok", "diff": diff });
        if !warnings.is_empty() {
            result["warnings"] = serde_json::to_value(&warnings).unwrap();
        }
        let conversions: serde_json::Map<String, serde_json::Value> =
            [("base", base_bytes.metadata()), ("revised", revised_bytes.metadata())]
                .into_iter()
                .filter_map(|(role, meta)| {
                    meta.map(|m| (role.into(), serde_json::to_value(m).unwrap()))
                })
                .collect();
        if !conversions.is_empty() {
            result["conversion"] = conversions.into();
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
