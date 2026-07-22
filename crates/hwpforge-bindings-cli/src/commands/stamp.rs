//! Stamp prose placeholders into named click-here fields (E6, 2-phase).
//!
//! `stamp-plan` discovers class-A candidates; the caller authors a spec map
//! (every unguarded candidate named or ignored); `stamp` applies it
//! all-or-nothing behind the fail-closed admission gate and writes the
//! stamped `.hwpx` plus a manifest.

use std::path::{Path, PathBuf};

use hwpforge_smithy_hwpx::stamp::{
    parse_stamp_map, CellStampError, HwpxStamper, StampError, StampMap, StamperError,
};

use crate::error::{check_file_size, CliError};

/// Run the `stamp-plan` command (candidate discovery, both classes).
pub fn run_plan(file: &PathBuf, json_mode: bool) {
    check_file_size(file, json_mode);
    let bytes = read_file(file, json_mode);
    let plan = match HwpxStamper::plan_bytes_v2(&bytes) {
        Ok(p) => p,
        Err(e) => exit_stamper_error(e, json_mode),
    };

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

    match parsed {
        StampMap::Legacy(specs) => {
            let result = match HwpxStamper::stamp(&bytes, &specs) {
                Ok(r) => r,
                Err(e) => exit_stamper_error(e, json_mode),
            };
            let manifest_json = serde_json::to_string_pretty(&result.manifest).unwrap();
            write_artifacts(output, &manifest_file, &result.bytes, &manifest_json, json_mode);

            if json_mode {
                let out = serde_json::json!({
                    "status": "ok",
                    "output": output.display().to_string(),
                    "manifest": manifest_file.display().to_string(),
                    "stamped": result.outcome.stamped,
                    "ignored": result.outcome.ignored,
                    "skipped_guarded": result.outcome.skipped_guarded.len(),
                    "size_bytes": result.bytes.len(),
                });
                println!("{}", serde_json::to_string(&out).unwrap());
            } else {
                println!(
                    "Stamped {} field(s) (ignored {}, guarded-skipped {}) -> {}",
                    result.outcome.stamped.len(),
                    result.outcome.ignored,
                    result.outcome.skipped_guarded.len(),
                    output.display()
                );
                for s in &result.outcome.stamped {
                    println!("  + {} = {:?} ({})", s.name, s.marker, s.pattern.id());
                }
                println!("Manifest -> {}", manifest_file.display());
            }
        }
        StampMap::V2(request) => {
            let result = match HwpxStamper::stamp_v2(&bytes, &request) {
                Ok(r) => r,
                Err(e) => exit_stamper_error(e, json_mode),
            };
            let manifest_json = serde_json::to_string_pretty(&result.manifest).unwrap();
            write_artifacts(output, &manifest_file, &result.bytes, &manifest_json, json_mode);

            if json_mode {
                let out = serde_json::json!({
                    "status": "ok",
                    "output": output.display().to_string(),
                    "manifest": manifest_file.display().to_string(),
                    "stamped_text": result.outcome.text.stamped,
                    "stamped_cells": result.outcome.cells.stamped,
                    "ignored": result.outcome.text.ignored + result.outcome.cells.ignored,
                    "skipped_guarded": result.outcome.text.skipped_guarded.len()
                        + result.outcome.cells.skipped_guarded.len(),
                    "size_bytes": result.bytes.len(),
                });
                println!("{}", serde_json::to_string(&out).unwrap());
            } else {
                println!(
                    "Stamped {} text + {} cell field(s) -> {}",
                    result.outcome.text.stamped.len(),
                    result.outcome.cells.stamped.len(),
                    output.display()
                );
                for s in &result.outcome.text.stamped {
                    println!("  + {} = {:?} ({})", s.name, s.marker, s.pattern.id());
                }
                for s in &result.outcome.cells.stamped {
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

fn exit_stamper_error(error: StamperError, json_mode: bool) -> ! {
    match error {
        StamperError::NotRoundTripSafe { component, diff_path } => CliError::new(
            "INPUT_NOT_ROUNDTRIP_SAFE",
            format!("input is not round-trip-safe: {component} differs at {diff_path}"),
        )
        .with_hint(
            "이 입력은 무손실 재인코드가 증명되지 않아 스탬핑을 거부합니다 (fail-closed). \
             코덱 갭 수정 또는 E4 preserve-first 경로가 필요합니다",
        )
        .exit(json_mode, 1),
        // Review L2: entry names are untrusted — {:?} escapes control chars.
        StamperError::UncarriedZipEntries { entries } => CliError::new(
            "INPUT_ENTRIES_NOT_CARRIED",
            format!("encoder does not carry input entries: {entries:?}"),
        )
        .with_hint("재인코드 시 유실될 ZIP 엔트리가 있어 거부합니다 (fail-closed)")
        .exit(json_mode, 1),
        StamperError::Stamp(inner) => exit_stamp_error(inner, json_mode),
        StamperError::ManifestInvariant { detail } => {
            CliError::new("STAMP_MANIFEST_INVARIANT", detail).exit(json_mode, 2)
        }
        StamperError::Codec(msg) => CliError::new("STAMP_CODEC_FAILED", msg).exit(json_mode, 2),
        StamperError::SourceHashMismatch { expected, actual } => CliError::new(
            "STAMP_SOURCE_HASH_MISMATCH",
            format!("map is pinned to {expected}, input is {actual}"),
        )
        .with_hint(
            "문서가 변경됐습니다 — `stamp-plan` 을 다시 실행해 맵의 source_sha256 을 갱신하세요",
        )
        .exit(json_mode, 1),
        StamperError::CellStamp(inner) => exit_cell_stamp_error(inner, json_mode),
        StamperError::DeltaMismatch { stage, detail } => CliError::new(
            "STAMP_DELTA_MISMATCH",
            format!("post-encode verification failed at {stage}: {detail}"),
        )
        .with_hint("산출물 검증 실패 — 코덱 버그 가능성이 있어 무출력으로 거부했습니다")
        .exit(json_mode, 2),
        other => CliError::new("STAMP_FAILED", other.to_string()).exit(json_mode, 2),
    }
}

fn exit_cell_stamp_error(error: CellStampError, json_mode: bool) -> ! {
    match error {
        CellStampError::TableNotFound { table } => {
            CliError::new("TABLE_NOT_FOUND", format!("table ordinal {table} does not exist"))
                .exit(json_mode, 1)
        }
        CellStampError::TableGridInvalid { table, detail } => {
            CliError::new("TABLE_GRID_INVALID", format!("table {table}: {detail}"))
                .exit(json_mode, 1)
        }
        CellStampError::NotAnAnchor { table, requested, anchor } => {
            let mut err = CliError::new(
                "STAMP_CELL_NOT_ANCHOR",
                format!("table {table}: ({},{}) is not an anchor", requested.row, requested.col),
            );
            if let Some(anchor) = anchor {
                err = err.with_hint(format!(
                    "이 좌표는 병합 피복 위치입니다 — anchor ({},{}) 를 지정하세요",
                    anchor.row, anchor.col
                ));
            }
            err.exit(json_mode, 1)
        }
        CellStampError::TargetNotStampable { table, at } => CliError::new(
            "STAMP_CELL_NOT_EMPTY",
            format!("table {table}: cell ({},{}) has authored content", at.row, at.col),
        )
        .with_hint("클래스-B 대상은 whitespace-only 빈 셀이어야 합니다")
        .exit(json_mode, 1),
        CellStampError::LabelDrift { table, at, claimed, found } => CliError::new(
            "STAMP_LABEL_DRIFT",
            format!(
                "table {table} ({},{}): claimed label {claimed:?}, live {found:?}",
                at.row, at.col
            ),
        )
        .with_hint("문서가 변경됐습니다 — `stamp-plan` 을 다시 실행해 맵을 갱신하세요")
        .exit(json_mode, 1),
        CellStampError::UnknownCandidate { table, at } => CliError::new(
            "STAMP_CELL_NOT_CANDIDATE",
            format!("table {table}: ({},{}) is not a live candidate", at.row, at.col),
        )
        .exit(json_mode, 1),
        CellStampError::DuplicateTarget { table, at } => CliError::new(
            "STAMP_CELL_TARGET_DUPLICATE",
            format!("table {table}: cell ({},{}) targeted twice", at.row, at.col),
        )
        .exit(json_mode, 1),
        CellStampError::EmptyName => {
            CliError::new("STAMP_NAME_EMPTY", "field name must not be empty").exit(json_mode, 1)
        }
        CellStampError::BlankHint { name } => {
            CliError::new("STAMP_HINT_BLANK", format!("cell spec {name:?}: hint must not be blank"))
                .with_hint("빈 셀엔 마커가 없어 hint 가 필수입니다 (plan 의 suggested_hint 참고)")
                .exit(json_mode, 1)
        }
        CellStampError::DuplicateName { name } => {
            CliError::new("STAMP_NAME_DUPLICATE", format!("duplicate field name {name:?}"))
                .exit(json_mode, 1)
        }
        CellStampError::NameCollision { name } => CliError::new(
            "STAMP_NAME_COLLISION",
            format!("field name {name:?} already exists in the document"),
        )
        .exit(json_mode, 1),
        CellStampError::UncoveredCandidate { table, at } => CliError::new(
            "STAMP_CANDIDATE_UNCOVERED",
            format!(
                "unguarded cell candidate at table {table} ({},{}) has no spec",
                at.row, at.col
            ),
        )
        .with_hint("모든 무가드 셀 후보는 이름을 붙이거나 ignore 로 명시해야 합니다")
        .exit(json_mode, 1),
        other => CliError::new("STAMP_FAILED", other.to_string()).exit(json_mode, 2),
    }
}

fn exit_stamp_error(error: StampError, json_mode: bool) -> ! {
    match error {
        StampError::UncoveredCandidate { section, path, span, marker } => CliError::new(
            "STAMP_CANDIDATE_UNCOVERED",
            format!(
                "unguarded candidate {marker:?} (section {section}, {path} [{}..{}]) has no spec",
                span.start, span.end
            ),
        )
        .with_hint("모든 무가드 후보는 이름을 붙이거나 ignore 로 명시해야 합니다 — `stamp-plan` 출력을 빠짐없이 분류하세요")
        .exit(json_mode, 1),
        StampError::UnknownSpec { section, path, span } => CliError::new(
            "STAMP_SPEC_STALE",
            format!("spec matches no live candidate: section {section}, {path} [{}..{}]", span.start, span.end),
        )
        .with_hint("문서가 변경됐거나 span 이 어긋났습니다 — `stamp-plan` 을 다시 실행해 맵을 갱신하세요")
        .exit(json_mode, 1),
        StampError::MarkerMismatch { path, expected, found } => CliError::new(
            "STAMP_MARKER_MISMATCH",
            format!("marker mismatch at {path}: spec {expected:?}, document {found:?}"),
        )
        .exit(json_mode, 1),
        StampError::DuplicateSpec { path, span } => CliError::new(
            "STAMP_SPEC_DUPLICATE",
            format!("duplicate specs for {path} [{}..{}]", span.start, span.end),
        )
        .exit(json_mode, 1),
        StampError::DuplicateName { name } => {
            CliError::new("STAMP_NAME_DUPLICATE", format!("duplicate field name {name:?}"))
                .exit(json_mode, 1)
        }
        StampError::NameCollision { name } => CliError::new(
            "STAMP_NAME_COLLISION",
            format!("field name {name:?} already exists in the document"),
        )
        .with_hint("기존 누름틀과 이름이 겹칩니다 — `fields` 로 기존 이름을 확인하세요")
        .exit(json_mode, 1),
        StampError::EmptyName => {
            CliError::new("STAMP_NAME_EMPTY", "field name must not be empty").exit(json_mode, 1)
        }
        other => CliError::new("STAMP_FAILED", other.to_string()).exit(json_mode, 2),
    }
}
