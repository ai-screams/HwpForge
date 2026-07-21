//! Stamp prose placeholders into named click-here fields (E6, 2-phase).
//!
//! `stamp-plan` discovers class-A candidates; the caller authors a spec map
//! (every unguarded candidate named or ignored); `stamp` applies it
//! all-or-nothing behind the fail-closed admission gate and writes the
//! stamped `.hwpx` plus a manifest.

use std::path::{Path, PathBuf};

use hwpforge_smithy_hwpx::stamp::{HwpxStamper, StampError, StampSpec, StamperError};

use crate::error::{check_file_size, CliError};

/// Run the `stamp-plan` command (candidate discovery).
pub fn run_plan(file: &PathBuf, json_mode: bool) {
    check_file_size(file, json_mode);
    let bytes = read_file(file, json_mode);
    let candidates = match HwpxStamper::plan_bytes(&bytes) {
        Ok(c) => c,
        Err(e) => exit_stamper_error(e, json_mode),
    };

    if json_mode {
        let result = serde_json::json!({
            "status": "ok",
            "file": file.display().to_string(),
            "candidates": candidates,
        });
        println!("{}", serde_json::to_string(&result).unwrap());
    } else if candidates.is_empty() {
        println!("No stamp candidates in {}", file.display());
    } else {
        println!("{} candidate(s) in {}:", candidates.len(), file.display());
        for c in &candidates {
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
        println!(
            "\n맵 작성: 각 후보를 {{\"action\":{{\"field\":{{\"name\":\"…\"}}}}}} 또는 \
             \"ignore\" 로 분류한 JSON 배열을 만들어 `stamp --map` 에 전달"
        );
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
    let specs: Vec<StampSpec> = match serde_json::from_str(&map_text) {
        Ok(s) => s,
        Err(e) => {
            CliError::new("INVALID_STAMP_MAP", format!("'{}': {e}", map.display()))
                .with_hint(
                    "맵은 StampSpec JSON 배열 — `stamp-plan --json` 의 candidates 에 \
                     action(field{name}/ignore)을 붙인 형태",
                )
                .exit(json_mode, 1);
        }
    };

    let result = match HwpxStamper::stamp(&bytes, &specs) {
        Ok(r) => r,
        Err(e) => exit_stamper_error(e, json_mode),
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
    let manifest_json = serde_json::to_string_pretty(&result.manifest).unwrap();
    if let Err(e) = std::fs::write(output, &result.bytes) {
        CliError::new("FILE_WRITE_FAILED", format!("Cannot write '{}': {e}", output.display()))
            .exit(json_mode, 1);
    }
    if let Err(e) = std::fs::write(&manifest_file, manifest_json) {
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
