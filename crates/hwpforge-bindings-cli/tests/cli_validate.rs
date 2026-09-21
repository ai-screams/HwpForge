//! Integration tests for the `validate` subcommand — process-based, same
//! style as `cli_integration.rs`, kept in its own file per the W5b lane
//! split (another lane owns `cli_integration.rs` and the rest of
//! `src/commands/*.rs`).

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn hwpforge_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hwpforge"))
}

fn test_tmp() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir =
        std::env::temp_dir().join(format!("hwpforge_validate_test_{id}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn fixture(rel: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(rel);
    assert!(path.exists(), "fixture not found: {}", path.display());
    path
}

/// Runs the built `hwpforge` binary, returning `(stdout, stderr, exit_code)`.
fn run(args: &[&str]) -> (String, String, i32) {
    let output =
        Command::new(hwpforge_bin()).args(args).output().expect("failed to execute hwpforge");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, stderr, code)
}

/// Runs with `--json` prepended and parses **stdout** as JSON regardless of
/// exit code. `validate`'s "decodes but invalid" case exits 1 while still
/// printing a full `{"status":"ok",...}` result to stdout (report
/// semantics, not an error envelope — see `commands/validate.rs`'s module
/// docs) — gating the parse on `exit == 0`, the way `cli_integration.rs`'s
/// shared `run_json_with_stdout` does for every other command, would
/// silently miss that path, which is the one this test file exists to
/// cover.
fn run_json(args: &[&str]) -> (Value, String, i32) {
    let mut full_args = vec!["--json"];
    full_args.extend_from_slice(args);
    let (stdout, stderr, code) = run(&full_args);
    let value: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("stdout is not JSON (exit {code}): {e}\nstdout: {stdout}\nstderr: {stderr}")
    });
    (value, stderr, code)
}

/// A minimal but genuinely valid HWPX, built via `convert` rather than a
/// committed fixture — this way the test does not depend on some other
/// fixture's continuing to pass `Document::validate`.
fn make_valid_hwpx(dir: &Path) -> PathBuf {
    let md = dir.join("valid.md");
    std::fs::write(&md, "# 제목\n\n본문 문단.\n").expect("write md");
    let out = dir.join("valid.hwpx");
    let (_, stderr, code) = run(&["convert", md.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 0, "setup: convert must succeed to produce a valid fixture: {stderr}");
    out
}

/// Builds a hand-tampered package: `stale-line-cache.hwpx`'s own entries
/// plus a second, empty `Contents/section1.xml` — decodes cleanly (two
/// sections) but fails `Document::validate` (`ValidationError::EmptySection`).
/// Same recipe as the MCP tool's own regression test
/// (`crates/hwpforge-bindings-mcp/src/tools/validate.rs`'s
/// `validate_reports_decode_warnings_and_a_real_validation_failure_together`),
/// so the same fixture also carries a `LAYOUT_CACHE_DROPPED` decode warning
/// alongside the validation failure.
fn make_decodable_but_invalid_hwpx(dir: &Path) -> PathBuf {
    let bytes = std::fs::read(fixture("layout/stale-line-cache.hwpx")).expect("read fixture");
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("open zip");
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i).expect("entry");
        writer.raw_copy_file(entry).expect("copy");
    }
    writer
        .start_file("Contents/section1.xml", zip::write::SimpleFileOptions::default())
        .expect("start section1");
    writer
        .write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><hs:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section"></hs:sec>"#,
        )
        .expect("write section1");
    let tampered = writer.finish().expect("finish").into_inner();

    let path = dir.join("two-section-one-empty.hwpx");
    std::fs::write(&path, &tampered).expect("write tampered fixture");
    path
}

#[test]
fn valid_document_exits_0_with_a_pinned_key_set() {
    let dir = test_tmp();
    let hwpx = make_valid_hwpx(&dir);

    let (value, stderr, code) = run_json(&["validate", hwpx.to_str().unwrap()]);
    assert_eq!(code, 0, "{value:?}");
    assert_eq!(value["status"], "ok");
    assert_eq!(value["valid"], true);
    assert!(value["sections"].as_u64().unwrap() >= 1);
    assert!(value["paragraphs"].as_u64().unwrap() >= 1);
    assert_eq!(value["errors"], serde_json::json!([]));
    assert_eq!(value["warnings"], serde_json::json!([]));
    assert!(stderr.is_empty(), "a clean document must not warn on stderr either: {stderr:?}");

    // Pinned key set: a machine consumer can rely on exactly these six
    // keys, present in both verdicts (no `skip_serializing_if` on
    // `errors`/`warnings` — see `ValidateResult`'s doc comment).
    let mut keys: Vec<&str> = value.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["errors", "paragraphs", "sections", "status", "valid", "warnings"]);
}

#[test]
fn stale_line_cache_fixture_is_valid_but_still_warns() {
    let path = fixture("layout/stale-line-cache.hwpx");

    let (value, stderr, code) = run_json(&["validate", path.to_str().unwrap()]);
    assert_eq!(code, 0, "{value:?}");
    assert_eq!(value["valid"], true, "{value:?}");
    let warnings = value["warnings"].as_array().expect("warnings array");
    assert!(
        warnings.iter().any(|w| w["code"] == "LAYOUT_CACHE_DROPPED"),
        "must surface the decode warning of a successful decode: {warnings:?}"
    );
    assert!(
        stderr.is_empty(),
        "--json carries warnings in the payload, not a stderr echo: {stderr:?}"
    );
}

#[test]
fn stale_line_cache_warning_prints_to_stderr_in_text_mode_only() {
    let path = fixture("layout/stale-line-cache.hwpx");

    let (_, stderr, code) = run(&["validate", path.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(
        stderr.lines().any(|l| l.starts_with("[validate] ")),
        "text mode must print one [validate] stderr line per warning: {stderr:?}"
    );
}

#[test]
fn decodable_but_invalid_package_reports_valid_false_not_an_error_envelope() {
    let dir = test_tmp();
    let path = make_decodable_but_invalid_hwpx(&dir);

    let (value, stderr, code) = run_json(&["validate", path.to_str().unwrap()]);
    assert_eq!(code, 1, "an invalid-but-decoded document is report semantics, exit 1: {value:?}");
    assert_eq!(value["status"], "ok", "not an error envelope: {value:?}");
    assert_eq!(value["valid"], false);
    assert_eq!(value["sections"], 2, "the decode itself sees both sections");
    let errors = value["errors"].as_array().expect("errors array");
    assert!(
        errors.iter().any(|e| e["message"]
            .as_str()
            .unwrap_or_default()
            .contains("Section 1 has no paragraphs")),
        "the validation error must name the rule it tripped: {errors:?}"
    );
    assert_eq!(errors[0]["code"], "VALIDATION_FAILED");
    let warnings = value["warnings"].as_array().expect("warnings array");
    assert!(
        warnings.iter().any(|w| w["code"] == "LAYOUT_CACHE_DROPPED"),
        "the same decode's warning survives alongside the validation failure: {warnings:?}"
    );
    assert!(stderr.is_empty(), "--json carries both in the payload, not stderr: {stderr:?}");
}

#[test]
fn non_hwpx_file_is_decode_failed_exit_2() {
    let dir = test_tmp();
    let path = dir.join("garbage.hwpx");
    std::fs::write(&path, b"not a zip file").expect("write garbage");

    let (stdout, stderr, code) = run(&["--json", "validate", path.to_str().unwrap()]);
    assert_eq!(code, 2, "stdout: {stdout:?} stderr: {stderr:?}");
    assert!(stdout.is_empty(), "an error goes to stderr, not stdout: {stdout:?}");
    let err: Value = serde_json::from_str(stderr.trim()).expect("stderr is JSON");
    assert_eq!(err["status"], "error");
    assert_eq!(err["code"], "DECODE_FAILED");
}

#[test]
fn missing_file_is_file_read_failed_exit_1() {
    let (stdout, stderr, code) = run(&["--json", "validate", "/nonexistent/does-not-exist.hwpx"]);
    assert_eq!(code, 1, "stdout: {stdout:?} stderr: {stderr:?}");
    assert!(stdout.is_empty());
    let err: Value = serde_json::from_str(stderr.trim()).expect("stderr is JSON");
    assert_eq!(err["status"], "error");
    assert_eq!(err["code"], "FILE_READ_FAILED");
}
