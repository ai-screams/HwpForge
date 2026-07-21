//! Edit table cells by logical grid address (E3, admission-gated).
//!
//! A cell is addressed by table ordinal (`--table`, matching `to-json`
//! export order) plus either a grid coordinate (`--at r,c` — covered
//! positions resolve to their merge anchor) or a label-relative direction
//! (`--right-of` / `--below`, normalized exact match). Batches go through
//! `--map` (JSON array of CellSpec). All-or-nothing behind the same
//! fail-closed admission gate as `stamp`.

use std::path::PathBuf;

use hwpforge_smithy_hwpx::{CellEditError, CellSpec, CellTarget, HwpxCellEditor};

use hwpforge_core::table::grid::GridCoord;

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

    let specs = build_specs(single, map, json_mode);

    let result = match HwpxCellEditor::set_cells(&bytes, &specs) {
        Ok(r) => r,
        Err(e) => exit_cell_edit_error(e, json_mode),
    };

    if let Err(e) = std::fs::write(output, &result.bytes) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 1);
    }

    if json_mode {
        let out = serde_json::json!({
            "status": "ok",
            "output": output.display().to_string(),
            "cells": result.outcome.cells,
            "size_bytes": result.bytes.len(),
        });
        println!("{}", serde_json::to_string(&out).unwrap());
    } else {
        println!("Set {} cell(s) -> {}", result.outcome.cells.len(), output.display());
        for c in &result.outcome.cells {
            let resolved = match c.resolution {
                hwpforge_smithy_hwpx::CellResolution::Exact => String::new(),
                hwpforge_smithy_hwpx::CellResolution::CoveredToAnchor => {
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

fn build_specs(single: SingleTarget<'_>, map: Option<&PathBuf>, json_mode: bool) -> Vec<CellSpec> {
    let has_single_flags = single.table.is_some()
        || single.at.is_some()
        || single.right_of.is_some()
        || single.below.is_some()
        || single.text.is_some();

    if let Some(map_path) = map {
        if has_single_flags {
            CliError::new(
                "INVALID_SET_CELL_ARGS",
                "--map cannot be combined with --table/--at/--right-of/--below/--text",
            )
            .exit(json_mode, 1);
        }
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
        match serde_json::from_str::<Vec<CellSpec>>(&map_text) {
            Ok(specs) if !specs.is_empty() => specs,
            Ok(_) => {
                CliError::new("INVALID_SET_CELL_MAP", "spec map is empty").exit(json_mode, 1);
            }
            Err(e) => {
                CliError::new("INVALID_SET_CELL_MAP", format!("'{}': {e}", map_path.display()))
                    .with_hint(
                        "맵은 CellSpec JSON 배열: [{\"table\":0,\"at\":{\"row\":1,\"col\":2},\"text\":\"값\"}] \
                         또는 {\"right_of\":\"성명\"} / {\"below\":\"항목\"}",
                    )
                    .exit(json_mode, 1);
            }
        }
    } else {
        let Some(table) = single.table else {
            CliError::new("INVALID_SET_CELL_ARGS", "--table is required (or use --map)")
                .exit(json_mode, 1);
        };
        let Some(text) = single.text else {
            CliError::new("INVALID_SET_CELL_ARGS", "--text is required (empty string clears)")
                .exit(json_mode, 1);
        };
        let target = match (single.at, single.right_of, single.below) {
            (Some(at), None, None) => CellTarget::At(parse_coord(at, json_mode)),
            (None, Some(label), None) => CellTarget::RightOf(label.to_string()),
            (None, None, Some(label)) => CellTarget::Below(label.to_string()),
            _ => {
                CliError::new(
                    "INVALID_SET_CELL_ARGS",
                    "exactly one of --at / --right-of / --below is required",
                )
                .exit(json_mode, 1);
            }
        };
        vec![CellSpec { table, target, text: text.to_string() }]
    }
}

fn parse_coord(at: &str, json_mode: bool) -> GridCoord {
    let parts: Vec<&str> = at.split(',').map(str::trim).collect();
    if parts.len() == 2 {
        if let (Ok(row), Ok(col)) = (parts[0].parse(), parts[1].parse()) {
            return GridCoord::new(row, col);
        }
    }
    CliError::new("INVALID_SET_CELL_ARGS", format!("--at expects \"row,col\", got '{at}'"))
        .exit(json_mode, 1);
}

fn exit_cell_edit_error(error: CellEditError, json_mode: bool) -> ! {
    let (code, hint): (&str, Option<String>) = match &error {
        CellEditError::TableNotFound { tables, .. } => (
            "TABLE_NOT_FOUND",
            Some(format!("문서에 표가 {tables}개 있습니다 (to-json export 순서 기준 0-base)")),
        ),
        CellEditError::TableGridInvalid { .. } => (
            "TABLE_GRID_INVALID",
            Some("이 표는 셀 span 이 well-formed 격자를 이루지 않아 주소 지정이 불가합니다".into()),
        ),
        CellEditError::CellNotFound { .. } => ("CELL_NOT_FOUND", None),
        CellEditError::LabelAmbiguous { .. } => (
            "CELL_LABEL_AMBIGUOUS",
            Some("라벨이 여러 셀과 일치합니다 — --at 좌표로 직접 지정하세요".into()),
        ),
        CellEditError::NonTextContent { .. } => (
            "CELL_HAS_NON_TEXT_CONTENT",
            Some("표/이미지/컨트롤이 든 셀은 파괴 방지를 위해 교체를 거부합니다".into()),
        ),
        CellEditError::TargetDuplicate { .. } => ("CELL_TARGET_DUPLICATE", None),
        CellEditError::TargetConflict { .. } => (
            "CELL_TARGET_CONFLICT",
            Some("바깥 셀 교체가 다른 편집이 노리는 중첩 표를 파괴합니다".into()),
        ),
        CellEditError::NotRoundTripSafe { .. } => (
            "INPUT_NOT_ROUNDTRIP_SAFE",
            Some(
                "이 입력은 무손실 재인코드가 증명되지 않아 편집을 거부합니다 (fail-closed)".into(),
            ),
        ),
        CellEditError::UncarriedZipEntries { .. } => ("INPUT_ENTRIES_NOT_CARRIED", None),
        CellEditError::Codec(_) => ("SET_CELL_CODEC_FAILED", None),
        _ => ("SET_CELL_FAILED", None),
    };
    let mut err = CliError::new(code, error.to_string());
    if let Some(hint) = hint {
        err = err.with_hint(hint);
    }
    err.exit(json_mode, 1);
}
