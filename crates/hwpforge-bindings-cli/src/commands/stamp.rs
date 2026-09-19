//! Stamp prose placeholders into named click-here fields (E6, 2-phase).
//!
//! `stamp-plan` discovers class-A candidates; the caller authors a spec map
//! (every unguarded candidate named or ignored); `stamp` applies it
//! all-or-nothing behind the fail-closed admission gate and writes the
//! stamped `.hwpx` plus a manifest.

use std::path::{Path, PathBuf};

use hwpforge::ops::stamp::{stamp as ops_stamp, stamp_plan as ops_stamp_plan, StampOptions};
use hwpforge::ops::OpsError;
use hwpforge_smithy_hwpx::stamp::{parse_stamp_map, StampMap};

use crate::compat::{self, Command};
use crate::error::{check_file_size, CliError};

/// Run the `stamp-plan` command (candidate discovery, both classes).
pub fn run_plan(file: &PathBuf, json_mode: bool) {
    check_file_size(file, json_mode);
    let bytes = read_file(file, json_mode);
    let out = match ops_stamp_plan(&bytes) {
        Ok(o) => o,
        Err(e) => exit_ops_error(Command::StampPlan, e, json_mode),
    };
    let plan = out.plan;

    if json_mode {
        let result = serde_json::json!({
            "status": "ok",
            "file": file.display().to_string(),
            "schema_version": plan.schema_version,
            "source_sha256": plan.source_sha256,
            "candidates": plan.text,
            "cells": plan.cells,
            "skipped_tables": plan.skipped_tables,
        });
        println!("{}", serde_json::to_string(&result).unwrap());
        return;
    }

    if plan.text.is_empty() && plan.cells.is_empty() {
        println!("No stamp candidates in {}", file.display());
    } else {
        println!(
            "{} text + {} cell candidate(s) in {}:",
            plan.text.len(),
            plan.cells.len(),
            file.display()
        );
        for c in &plan.text {
            let guard = match c.guard {
                Some(_) => " [guarded: instruction context]",
                None => "",
            };
            println!(
                "  [{}] {:?} ({}) @ {} [{}..{}]{}",
                c.section,
                c.marker,
                c.pattern.id(),
                c.path,
                c.span.start,
                c.span.end,
                guard
            );
        }
        for c in &plan.cells {
            let labels: Vec<String> = c
                .labels
                .iter()
                .map(|l| format!("{:?}({},{}) {:?}", l.direction, l.at.row, l.at.col, l.normalized))
                .collect();
            let guard = if c.guarded { " [guarded]" } else { "" };
            let name = c.suggested_name.as_deref().unwrap_or("-");
            println!(
                "  cell t{} ({},{}) suggested={name:?} labels=[{}]{guard}",
                c.table,
                c.at.row,
                c.at.col,
                labels.join(", ")
            );
        }
        println!(
            "\n맵 작성(v2): {{\"schema_version\":2, \"source_sha256\":\"{}\", \
             \"text\":[…], \"cells\":[{{\"table\":…, \"at\":{{…}}, \"label\":{{…}}, \
             \"action\":{{\"field\":{{\"name\":\"…\",\"hint\":\"…\"}}}}}}]}} 를 `stamp --map` 에 전달 \
             (셀 hint 는 필수)",
            plan.source_sha256
        );
    }
    for s in &plan.skipped_tables {
        eprintln!("경고: 표 {} 격자 무효 — 셀 탐지 제외 ({}): {}", s.table, s.path, s.error);
    }
}

/// Run the `stamp` command (admission-gated apply + manifest).
pub fn run(
    file: &PathBuf,
    map: &PathBuf,
    output: &PathBuf,
    manifest_path: Option<&Path>,
    json_mode: bool,
) {
    check_file_size(file, json_mode);
    let bytes = read_file(file, json_mode);

    let map_text = match std::fs::read_to_string(map) {
        Ok(t) => t,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", map.display()))
                .exit(json_mode, 1);
        }
    };
    let parsed = match parse_stamp_map(&map_text) {
        Ok(p) => p,
        Err(e) => {
            CliError::new("INVALID_STAMP_MAP", format!("'{}': {e}", map.display()))
                .with_hint(
                    "맵은 StampSpec JSON 배열(legacy) 또는 {schema_version:2, source_sha256, \
                     text[], cells[]} 객체(v2) — `stamp-plan --json` 출력을 기반으로 작성",
                )
                .exit(json_mode, 1);
        }
    };

    let manifest_file: PathBuf = manifest_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| output.with_extension("manifest.json"));
    // R2: identical paths would silently overwrite the stamped .hwpx with
    // the manifest JSON and still report success.
    if output == &manifest_file {
        CliError::new(
            "MANIFEST_PATH_CONFLICT",
            format!("manifest path equals output path: {}", output.display()),
        )
        .with_hint("--manifest 경로는 -o 경로와 달라야 합니다")
        .exit(json_mode, 1);
    }

    // `ops::stamp::stamp` handles both the legacy spec-array and v2 envelope
    // shapes itself and returns one unified `StampOutput` — `parsed` is only
    // matched again below to pick which of the two legacy JSON/text shapes
    // to print.
    let result = match ops_stamp(&bytes, &parsed, &StampOptions::default()) {
        Ok(r) => r,
        Err(e) => exit_ops_error(Command::Stamp, e, json_mode),
    };
    let manifest =
        result.manifest.as_ref().expect("StampOptions::default() always requests a manifest");
    let manifest_json = serde_json::to_string_pretty(manifest).unwrap();
    write_artifacts(output, &manifest_file, &result.bytes, &manifest_json, json_mode);

    match &parsed {
        StampMap::Legacy(_) => {
            if json_mode {
                let out = serde_json::json!({
                    "status": "ok",
                    "output": output.display().to_string(),
                    "manifest": manifest_file.display().to_string(),
                    "stamped": result.stamped,
                    "ignored": result.ignored,
                    "skipped_guarded": result.skipped_guarded,
                    "size_bytes": result.bytes.len(),
                });
                println!("{}", serde_json::to_string(&out).unwrap());
            } else {
                println!(
                    "Stamped {} field(s) (ignored {}, guarded-skipped {}) -> {}",
                    result.stamped.len(),
                    result.ignored,
                    result.skipped_guarded,
                    output.display()
                );
                for s in &result.stamped {
                    println!("  + {} = {:?} ({})", s.name, s.marker, s.pattern.id());
                }
                println!("Manifest -> {}", manifest_file.display());
            }
        }
        StampMap::V2(_) => {
            if json_mode {
                let out = serde_json::json!({
                    "status": "ok",
                    "output": output.display().to_string(),
                    "manifest": manifest_file.display().to_string(),
                    "stamped_text": result.stamped,
                    "stamped_cells": result.stamped_cells,
                    "ignored": result.ignored,
                    "skipped_guarded": result.skipped_guarded,
                    "size_bytes": result.bytes.len(),
                });
                println!("{}", serde_json::to_string(&out).unwrap());
            } else {
                println!(
                    "Stamped {} text + {} cell field(s) -> {}",
                    result.stamped.len(),
                    result.stamped_cells.len(),
                    output.display()
                );
                for s in &result.stamped {
                    println!("  + {} = {:?} ({})", s.name, s.marker, s.pattern.id());
                }
                for s in &result.stamped_cells {
                    println!("  + {} @ t{} ({},{})", s.name, s.table, s.at.row, s.at.col);
                }
                println!("Manifest -> {}", manifest_file.display());
            }
        }
    }
}

/// Writes the stamped output + manifest, fail-closed (no partial artifacts).
fn write_artifacts(
    output: &PathBuf,
    manifest_file: &Path,
    bytes: &[u8],
    manifest_json: &str,
    json_mode: bool,
) {
    if let Err(e) = std::fs::write(output, bytes) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 1);
    }
    if let Err(e) = std::fs::write(manifest_file, manifest_json) {
        // Review L1: a stamped .hwpx without its manifest is a partial
        // artifact — remove it so a failed command leaves nothing behind.
        let _ = std::fs::remove_file(output);
        CliError::new(
            "FILE_WRITE_FAILED",
            format!("Cannot write '{}': {e}", manifest_file.display()),
        )
        .with_hint("manifest 기록 실패로 산출물을 남기지 않았습니다 (fail-closed)")
        .exit(json_mode, 1);
    }
}

fn read_file(file: &PathBuf, json_mode: bool) -> Vec<u8> {
    match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", file.display()))
                .exit(json_mode, 1);
        }
    }
}

/// Maps an `ops::stamp::{stamp_plan,stamp}` failure onto the frozen contract
/// and exits.
fn exit_ops_error(cmd: Command, err: OpsError, json_mode: bool) -> ! {
    let ce = compat::cli_error(cmd, err);
    let exit = compat::exit_code(cmd, &ce);
    ce.exit(json_mode, exit);
}
