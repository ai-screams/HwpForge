//! Edit table cells by logical grid address (admission-gated).
//!
//! A cell is addressed by table ordinal (`--table`, matching `to-json`
//! export order) plus either a grid coordinate (`--at r,c` — covered
//! positions resolve to their merge anchor) or a label-relative direction
//! (`--right-of` / `--below`, normalized exact match). Batches go through
//! `--map` (JSON array of CellSpec). All-or-nothing behind the same
//! fail-closed admission gate as `stamp`.
use crate::error::CliCode;
use hwpforge_foundation::diagnostics::OpsCode;

use std::path::PathBuf;

use hwpforge::ops::edit::{set_cell as ops_set_cell, SetCellOptions};
use hwpforge::ops::OpsWarning;
use hwpforge_foundation::diagnostics::WarningInfo;
use hwpforge_smithy_hwpx::{CellResolution, CellSpec};

use crate::compat::{self, Command};
use crate::error::{check_file_size, read_bounded_string, read_input, CliError};

/// Flag bundle for a single-target invocation.
pub struct SingleTarget<'a> {
    /// `--table` ordinal.
    pub table: Option<usize>,
    /// `--at "r,c"` coordinate.
    pub at: Option<&'a str>,
    /// `--right-of` label.
    pub right_of: Option<&'a str>,
    /// `--below` label.
    pub below: Option<&'a str>,
    /// `--text` replacement (empty string clears).
    pub text: Option<&'a str>,
}

/// Run the `set-cell` command.
pub fn run(
    file: &PathBuf,
    output: &PathBuf,
    single: SingleTarget<'_>,
    map: Option<&PathBuf>,
    json_mode: bool,
) {
    check_file_size(file, json_mode, Command::SetCell);
    let bytes = read_input(file, json_mode, Command::SetCell);

    let opts = build_options(single, map, json_mode);

    let result = match ops_set_cell(&bytes, &opts) {
        Ok(r) => r,
        // Command and typed code determine the exit status.
        Err(e) => compat::exit_ops_error(Command::SetCell, e, json_mode),
    };
    // Decoder warnings, then the successful encode's non-semantic warnings
    // (`SetCellOutput::warnings`) were not surfaced previously.
    // additive — a new, omit-if-empty `warnings` key in `--json`, and one
    // `[set-cell]`-prefixed stderr line each in text mode.
    let warnings: Vec<WarningInfo> = result.warnings.iter().map(OpsWarning::info).collect();

    if let Err(e) = std::fs::write(output, &result.bytes) {
        CliError::new(
            CliCode::Ops(OpsCode::FileWriteFailed),
            format!("Cannot write '{}': {e}", output.display()),
        )
        .exit(json_mode, Command::SetCell);
    }

    if json_mode {
        let mut out = serde_json::json!({
            "status": "ok",
            "output": output.display().to_string(),
            "cells": result.results,
            "size_bytes": result.bytes.len(),
        });
        if !warnings.is_empty() {
            out["warnings"] = serde_json::to_value(&warnings).unwrap();
        }
        println!("{}", serde_json::to_string(&out).unwrap());
    } else {
        for w in &warnings {
            eprintln!("[set-cell] {}: {}", w.code, w.message);
        }
        println!("Set {} cell(s) -> {}", result.results.len(), output.display());
        for c in &result.results {
            let resolved = match c.resolution {
                CellResolution::Exact => String::new(),
                CellResolution::CoveredToAnchor => {
                    format!(" (covered -> anchor ({}, {}))", c.anchor.row, c.anchor.col)
                }
            };
            println!(
                "  table #{} ({}, {}){}{}",
                c.table,
                c.requested.row,
                c.requested.col,
                resolved,
                if c.cleared { " [cleared]" } else { "" }
            );
        }
    }
}

/// Builds the options `ops::edit::set_cell` itself validates and interprets:
/// "table/text required" and "exactly one direction", and the `--map`
/// batch's own empty-list rejection, now live in `SetCellOptions`'s
/// consumer. The `--map`/single-target mutual-exclusion
/// guard is restored here instead — legacy checked it
/// before ever reading the `--map` file, and `ops::edit::set_cell` cannot
/// reproduce that ordering since it only sees the already-built options,
/// not the raw flags.
fn build_options(
    single: SingleTarget<'_>,
    map: Option<&PathBuf>,
    json_mode: bool,
) -> SetCellOptions {
    // Legacy guard (5ff81af `build_specs`): checked before the `--map` file
    // is even read, so a combined-flags misuse fails fast without touching
    // a map path that might not exist.
    let has_single_flags = single.table.is_some()
        || single.at.is_some()
        || single.right_of.is_some()
        || single.below.is_some()
        || single.text.is_some();
    if map.is_some() && has_single_flags {
        CliError::new(
            CliCode::Ops(OpsCode::InvalidSetCellArgs),
            "--map cannot be combined with --table/--at/--right-of/--below/--text",
        )
        .exit(json_mode, Command::SetCell);
    }

    let mut opts = SetCellOptions::default();
    if let Some(table) = single.table {
        opts = opts.with_table(table);
    }
    if let Some(at) = single.at {
        opts = opts.with_at(at);
    }
    if let Some(right_of) = single.right_of {
        opts = opts.with_right_of(right_of);
    }
    if let Some(below) = single.below {
        opts = opts.with_below(below);
    }
    if let Some(text) = single.text {
        opts = opts.with_text(text);
    }
    if let Some(map_path) = map {
        let map_text = match read_bounded_string(map_path) {
            Ok(t) => t,
            Err(e) => {
                CliError::new(
                    CliCode::Ops(e.code()),
                    format!("Cannot read '{}': {e}", map_path.display()),
                )
                .exit(json_mode, Command::SetCell);
            }
        };
        let specs: Vec<CellSpec> = match serde_json::from_str(&map_text) {
            Ok(specs) => specs,
            Err(e) => {
                CliError::new(CliCode::Ops(OpsCode::InvalidSetCellMap), format!("'{}': {e}", map_path.display()))
                    .with_hint(
                        "맵은 CellSpec JSON 배열: [{\"table\":0,\"at\":{\"row\":1,\"col\":2},\"text\":\"값\"}] \
                         또는 {\"right_of\":\"성명\"} / {\"below\":\"항목\"}",
                    )
                    .exit(json_mode, Command::SetCell);
            }
        };
        opts = opts.with_specs(specs);
    }
    opts
}
