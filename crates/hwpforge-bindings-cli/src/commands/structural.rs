//! Structural paragraph editing (E4): insert / delete top-level paragraphs.

use std::path::PathBuf;

use hwpforge::ops::edit::{
    delete_para as ops_delete_para, insert_para as ops_insert_para, DeleteParaOptions,
    InsertParaOptions,
};
use hwpforge::ops::{OpsError, OpsWarning};

/// Renders one warning into this file's `Vec<String>` shape: a
/// [`OpsWarning::Structural`] advisory keeps its own `Display` (unchanged,
/// pre-migration behaviour); every other kind — chiefly `Decode`, for
/// example `LAYOUT_CACHE_DROPPED` (W5 follow-up: previously dropped here,
/// see the `run_delete`/`run_insert` comments) — renders as `"{code}:
/// {message}"` via [`OpsWarning::info`].
fn render_warning(warning: &OpsWarning) -> String {
    match warning {
        OpsWarning::Structural(sw) => sw.to_string(),
        other => {
            let info = other.info();
            format!("{}: {}", info.code, info.message)
        }
    }
}

use crate::compat::{self, Command};
use crate::error::{check_file_size, CliError};

/// Run `delete-para`.
pub fn run_delete(
    file: &PathBuf,
    section: usize,
    indices: &[usize],
    output: &PathBuf,
    json_mode: bool,
) {
    if indices.is_empty() {
        CliError::new("DELETE_NO_TARGET", "Pass at least one --index").exit(json_mode, 1);
    }
    let bytes = read_input(file, json_mode);
    let opts = DeleteParaOptions::default().with_section(section).with_indexes(indices.to_vec());
    match ops_delete_para(&bytes, &opts) {
        Ok(out) => {
            // Pre-W5, only the advisory scan's own warnings were ever
            // printed here — the decode warnings `ops::edit::delete_para`
            // also carries stayed unsurfaced (the legacy command never
            // reported decode warnings either). W5 follow-up: additive —
            // every warning now renders via `render_warning`, so a decode
            // warning (for example `LAYOUT_CACHE_DROPPED`) joins the same
            // `warnings` array/stderr lines an `INDEX_MARK_REMOVED` advisory
            // already used.
            let warnings: Vec<String> = out.warnings.iter().map(render_warning).collect();
            if !json_mode {
                for warning in &warnings {
                    eprintln!("warning: {warning}");
                }
            }
            write_output(&out.bytes, output, json_mode, |v| {
                *v = serde_json::json!({
                    "status": "ok",
                    "deleted": indices.len(),
                    "section": section,
                    "indices": indices,
                    "warnings": warnings,
                    "output": output.display().to_string(),
                });
            });
        }
        Err(e) => exit_ops_error(Command::DeletePara, e, json_mode),
    }
}

/// Run `insert-para`.
#[allow(clippy::too_many_arguments)]
pub fn run_insert(
    file: &PathBuf,
    section: usize,
    anchor: usize,
    before: bool,
    texts: &[String],
    output: &PathBuf,
    json_mode: bool,
) {
    let bytes = read_input(file, json_mode);
    let opts = InsertParaOptions::default()
        .with_section(section)
        .with_anchor(anchor)
        .with_text(texts.to_vec())
        .with_before(before);
    match ops_insert_para(&bytes, &opts) {
        Ok(out) => {
            // `insert_para` has no advisory scan (module docs,
            // `hwpforge/src/ops/edit.rs`), so every entry here is a decode
            // warning (for example `LAYOUT_CACHE_DROPPED`) — unsurfaced
            // pre-W5 (this command printed no warnings at all). W5
            // follow-up: additive — mirrors `run_delete`'s shape (`warnings`
            // key always present, one `warning: {…}` stderr line each).
            let warnings: Vec<String> = out.warnings.iter().map(render_warning).collect();
            if !json_mode {
                for warning in &warnings {
                    eprintln!("warning: {warning}");
                }
            }
            write_output(&out.bytes, output, json_mode, |v| {
                *v = serde_json::json!({
                    "status": "ok",
                    "inserted": texts.len(),
                    "section": section,
                    "anchor": anchor,
                    "position": if before { "before" } else { "after" },
                    "warnings": warnings,
                    "output": output.display().to_string(),
                });
            })
        }
        Err(e) => exit_ops_error(Command::InsertPara, e, json_mode),
    }
}

fn read_input(file: &PathBuf, json_mode: bool) -> Vec<u8> {
    check_file_size(file, json_mode);
    match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", file.display()))
                .exit(json_mode, 1);
        }
    }
}

fn write_output(
    bytes: &[u8],
    output: &PathBuf,
    json_mode: bool,
    fill: impl FnOnce(&mut serde_json::Value),
) {
    if let Err(e) = std::fs::write(output, bytes) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 2);
    }
    if json_mode {
        let mut v = serde_json::Value::Null;
        fill(&mut v);
        println!("{}", serde_json::to_string(&v).unwrap());
    } else {
        println!("Wrote {}", output.display());
    }
}

/// Maps an `ops::edit::{delete_para,insert_para}` failure onto the frozen
/// contract and exits. No dynamic/dual-source gap applies here: every
/// `StructuralEditError` variant these two operations can reach has a static
/// `TABLE` row for both commands (`compat.rs`), so the generic lookup is
/// correct as-is.
fn exit_ops_error(cmd: Command, err: OpsError, json_mode: bool) -> ! {
    let ce = compat::cli_error(cmd, err);
    let exit = compat::exit_code(cmd, &ce);
    ce.exit(json_mode, exit);
}
