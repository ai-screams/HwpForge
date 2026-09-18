//! Edit table cells by logical grid address (E3, admission-gated).
//!
//! A cell is addressed by table ordinal (`--table`, matching `to-json`
//! export order) plus either a grid coordinate (`--at r,c` — covered
//! positions resolve to their merge anchor) or a label-relative direction
//! (`--right-of` / `--below`, normalized exact match). Batches go through
//! `--map` (JSON array of CellSpec). All-or-nothing behind the same
//! fail-closed admission gate as `stamp`.

use std::path::PathBuf;

use hwpforge::ops::edit::{set_cell as ops_set_cell, SetCellOptions};
use hwpforge::ops::OpsError;
use hwpforge_smithy_hwpx::{CellResolution, CellSpec};

use crate::compat::{self, Command};
use crate::error::{check_file_size, CliError};

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
    check_file_size(file, json_mode);
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {e}", file.display()))
                .exit(json_mode, 1);
        }
    };

    let opts = build_options(single, map, json_mode);

    let result = match ops_set_cell(&bytes, &opts) {
        Ok(r) => r,
        Err(e) => exit_ops_error(Command::SetCell, e, json_mode),
    };

    if let Err(e) = std::fs::write(output, &result.bytes) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 1);
    }

    if json_mode {
        let out = serde_json::json!({
            "status": "ok",
            "output": output.display().to_string(),
            "cells": result.results,
            "size_bytes": result.bytes.len(),
        });
        println!("{}", serde_json::to_string(&out).unwrap());
    } else {
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
/// consumer (see the W3 report). The `--map`/single-target mutual-exclusion
/// guard is restored here instead (W3 remediation) — legacy checked it
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
            "INVALID_SET_CELL_ARGS",
            "--map cannot be combined with --table/--at/--right-of/--below/--text",
        )
        .exit(json_mode, 1);
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
        let map_text = match std::fs::read_to_string(map_path) {
            Ok(t) => t,
            Err(e) => {
                CliError::new(
                    "FILE_READ_FAILED",
                    format!("Cannot read '{}': {e}", map_path.display()),
                )
                .exit(json_mode, 1);
            }
        };
        let specs: Vec<CellSpec> = match serde_json::from_str(&map_text) {
            Ok(specs) => specs,
            Err(e) => {
                CliError::new("INVALID_SET_CELL_MAP", format!("'{}': {e}", map_path.display()))
                    .with_hint(
                        "맵은 CellSpec JSON 배열: [{\"table\":0,\"at\":{\"row\":1,\"col\":2},\"text\":\"값\"}] \
                         또는 {\"right_of\":\"성명\"} / {\"below\":\"항목\"}",
                    )
                    .exit(json_mode, 1);
            }
        };
        opts = opts.with_specs(specs);
    }
    opts
}

/// Maps an `ops::edit::set_cell` failure onto the frozen contract and exits.
fn exit_ops_error(cmd: Command, err: OpsError, json_mode: bool) -> ! {
    let ce = compat::cli_error(cmd, err);
    // set-cell's legacy exit is 1 for every code it emits (compat.rs module
    // docs); `TABLE` and `DYNAMIC_EXIT` both record it that way, so the
    // shared lookup reproduces it without a local constant.
    let exit = compat::exit_code(cmd, &ce);
    ce.exit(json_mode, exit);
}
