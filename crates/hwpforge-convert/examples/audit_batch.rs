//! Batch conversion warning audit.
//!
//! Scans `.hwp` inputs and groups conversion warnings by semantic category.
//! Labels and the complete `inputs` list are relative to the supplied root.
//!
//! Usage: `audit_batch <root> [relative-file-or-directory ...]`
//! Without selections, all `.hwp` files below the root are scanned.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hwpforge_convert::hwp5_to_hwpx_bytes;
use hwpforge_smithy_hwp5::Hwp5Warning;
use serde::Serialize;

#[derive(Default, Serialize)]
struct CategoryStats {
    count: usize,
    fixtures: BTreeMap<String, usize>,
}

#[derive(Default, Serialize)]
struct FixtureFailure {
    fixture: String,
    error: String,
}

#[derive(Serialize)]
struct AuditReport {
    schema_version: u32,
    inputs: Vec<String>,
    fixtures_scanned: usize,
    fixtures_decoded: usize,
    fixtures_failed: usize,
    total_warnings: usize,
    categories: BTreeMap<String, CategoryStats>,
    failures: Vec<FixtureFailure>,
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: audit_batch <root> [relative-file-or-directory ...]");
        return ExitCode::from(2);
    }

    let root = PathBuf::from(&args[0]);
    // 이것을 실패시키는 것: 선택한 파일 대신 루트 전체를 항상 순회하기.
    let selections = if args.len() == 1 {
        vec![root.clone()]
    } else {
        args[1..].iter().map(|arg| root.join(arg)).collect()
    };
    let mut hwp_paths: Vec<PathBuf> = Vec::new();
    for selection in selections {
        collect_hwp_files(&selection, &mut hwp_paths);
    }
    hwp_paths.sort();
    // 이것을 실패시키는 것: 중복 선택을 입력 목록에 두 번 기록하기.
    hwp_paths.dedup();

    let mut report = AuditReport {
        schema_version: 1,
        inputs: hwp_paths.iter().map(|path| relative_label(&root, path)).collect(),
        fixtures_scanned: hwp_paths.len(),
        fixtures_decoded: 0,
        fixtures_failed: 0,
        total_warnings: 0,
        categories: BTreeMap::new(),
        failures: Vec::new(),
    };

    for path in &hwp_paths {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(err) => {
                report.fixtures_failed += 1;
                report.failures.push(FixtureFailure {
                    fixture: relative_label(&root, path),
                    error: format!("io: {err}"),
                });
                continue;
            }
        };

        match hwp5_to_hwpx_bytes(&bytes) {
            Ok((_hwpx_bytes, warnings)) => {
                report.fixtures_decoded += 1;
                report.total_warnings += warnings.len();
                let fixture_label = relative_label(&root, path);
                for warning in &warnings {
                    let key = category_key(warning);
                    let entry = report.categories.entry(key).or_default();
                    entry.count += 1;
                    *entry.fixtures.entry(fixture_label.clone()).or_insert(0) += 1;
                }
            }
            Err(err) => {
                report.fixtures_failed += 1;
                report.failures.push(FixtureFailure {
                    fixture: relative_label(&root, path),
                    error: format!("{err}"),
                });
            }
        }
    }

    let json = serde_json::to_string_pretty(&report).expect("audit report must serialize");
    println!("{json}");
    ExitCode::SUCCESS
}

fn collect_hwp_files(root: &Path, out: &mut Vec<PathBuf>) {
    if root.is_file() {
        if root.extension().and_then(|e| e.to_str()) == Some("hwp") {
            out.push(root.to_path_buf());
        }
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_hwp_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("hwp") {
            out.push(path);
        }
    }
}

fn category_key(warning: &hwpforge_convert::ConvertWarning) -> String {
    let warning = match warning {
        hwpforge_convert::ConvertWarning::Hwp5(w) => w,
        // W1b: HWPX 인코드 경고 (캐시 드롭 등) — 자체 카테고리.
        hwpforge_convert::ConvertWarning::HwpxEncode(w) => {
            return format!("HwpxEncode:{w:?}")
                .split('{')
                .next()
                .unwrap_or("HwpxEncode")
                .trim()
                .to_string();
        }
        _ => return "Other".to_string(),
    };
    match warning {
        Hwp5Warning::UnsupportedTag { tag_id, .. } => {
            format!("UnsupportedTag:0x{tag_id:04X}")
        }
        Hwp5Warning::SkippedStream { name } => {
            format!("SkippedStream:{}", normalize_stream_name(name))
        }
        Hwp5Warning::DroppedControl { control, .. } => {
            format!("DroppedControl:{control}")
        }
        Hwp5Warning::ProjectionFallback { subject, .. } => {
            format!("ProjectionFallback:{subject}")
        }
        Hwp5Warning::ParserFallback { subject, .. } => {
            format!("ParserFallback:{subject}")
        }
        // W1b: 캐시 fail-closed 드롭 — 사유의 가변부(좌표 숫자)는 제거해
        // baseline 카테고리를 안정화한다.
        Hwp5Warning::LayoutCacheDropped { reason } => {
            let stable: String =
                reason.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).collect();
            format!("LayoutCacheDropped:{stable}")
        }
        // P1b: 묶음 내용 손실 — 종류(kind)로 묶는다(사유·위치는 가변).
        Hwp5Warning::ContentLost { kind, .. } => format!("ContentLost:{kind}"),
        _ => "Other:unknown".to_string(),
    }
}

fn normalize_stream_name(name: &str) -> String {
    // Section streams are numbered (Section0, Section1, ...). Collapse the
    // numeric suffix so the category key stays stable across fixtures.
    let trimmed = name.trim_start_matches('/');
    if let Some(prefix) = trimmed.strip_prefix("BodyText/Section") {
        if prefix.chars().all(|c| c.is_ascii_digit()) {
            return "BodyText/SectionN".to_string();
        }
    }
    trimmed.to_string()
}

// 이것을 실패시키는 것: 루트 상대 경로 대신 basename 또는 절대 경로를 기록하기.
fn relative_label(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).expect("input must be below the supplied root").display().to_string()
}
