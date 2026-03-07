//! CLI integration tests using process-based invocation.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Path to the built binary.
fn hwpforge_bin() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates/
    path.pop(); // workspace root
    path.push("target");
    path.push("debug");
    path.push("hwpforge");
    path
}

/// Create a unique temp directory for each test.
fn test_tmp() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("hwpforge_test_{id}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Run hwpforge with given args, return (stdout, stderr, exit_code).
fn run(args: &[&str]) -> (String, String, i32) {
    let output =
        Command::new(hwpforge_bin()).args(args).output().expect("failed to execute hwpforge");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, stderr, code)
}

/// Run hwpforge with --json flag prepended.
fn run_json(args: &[&str]) -> (serde_json::Value, String, i32) {
    let mut full_args = vec!["--json"];
    full_args.extend_from_slice(args);
    let (stdout, stderr, code) = run(&full_args);
    if code == 0 {
        let value: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("invalid JSON output: {e}\nstdout: {stdout}"));
        (value, stderr, code)
    } else {
        (serde_json::Value::Null, stderr, code)
    }
}

/// Path to the test markdown file.
fn test_md_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("temp");
    path.push("flow_test.md");
    path
}

/// Path to a real HWPX fixture.
fn fixture_hwpx_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("tests");
    path.push("fixtures");
    path.push("rect.hwpx");
    path
}

// ─── convert ───

#[test]
fn convert_md_to_hwpx() {
    let md_path = test_md_path();
    if !md_path.exists() {
        return;
    }
    let tmp = test_tmp();
    let out = tmp.join("output.hwpx");
    let (_, _, code) = run(&["convert", md_path.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(out.exists());
    assert!(std::fs::metadata(&out).unwrap().len() > 0);
}

#[test]
fn convert_json_mode() {
    let md_path = test_md_path();
    if !md_path.exists() {
        return;
    }
    let tmp = test_tmp();
    let out = tmp.join("output.hwpx");
    let (val, _, code) =
        run_json(&["convert", md_path.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert!(val["size_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn convert_nonexistent_file() {
    let tmp = test_tmp();
    let out = tmp.join("output.hwpx");
    let (_, _, code) = run(&["convert", "/nonexistent/file.md", "-o", out.to_str().unwrap()]);
    assert_eq!(code, 1);
}

#[test]
fn convert_unknown_preset() {
    let md_path = test_md_path();
    if !md_path.exists() {
        return;
    }
    let tmp = test_tmp();
    let out = tmp.join("output.hwpx");
    let (_, _, code) = run(&[
        "convert",
        md_path.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--preset",
        "nonexistent",
    ]);
    assert_eq!(code, 1);
}

// ─── inspect ───

#[test]
fn inspect_hwpx() {
    let fixture = fixture_hwpx_path();
    if !fixture.exists() {
        return;
    }
    let (stdout, _, code) = run(&["inspect", fixture.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Document:"));
    assert!(stdout.contains("Sections:"));
}

#[test]
fn inspect_json_mode() {
    let fixture = fixture_hwpx_path();
    if !fixture.exists() {
        return;
    }
    let (val, _, code) = run_json(&["inspect", fixture.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert!(val["sections"].is_array());
}

#[test]
fn inspect_nonexistent_file() {
    let (_, _, code) = run(&["inspect", "/nonexistent/file.hwpx"]);
    assert_eq!(code, 1);
}

// ─── to-json + from-json round-trip ───

#[test]
fn to_json_full_document() {
    let fixture = fixture_hwpx_path();
    if !fixture.exists() {
        return;
    }
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let (_, _, code) =
        run(&["to-json", fixture.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(json_out.exists());

    let content = std::fs::read_to_string(&json_out).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(parsed["document"].is_object());
}

#[test]
fn to_json_section_extract() {
    let fixture = fixture_hwpx_path();
    if !fixture.exists() {
        return;
    }
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let (_, _, code) = run(&[
        "to-json",
        fixture.to_str().unwrap(),
        "-o",
        json_out.to_str().unwrap(),
        "--section",
        "0",
    ]);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(&json_out).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["section_index"], 0);
    assert!(parsed["section"].is_object());
}

#[test]
fn to_json_section_out_of_range() {
    let fixture = fixture_hwpx_path();
    if !fixture.exists() {
        return;
    }
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let (_, _, code) = run(&[
        "to-json",
        fixture.to_str().unwrap(),
        "-o",
        json_out.to_str().unwrap(),
        "--section",
        "999",
    ]);
    assert_eq!(code, 1);
}

#[test]
fn from_json_round_trip() {
    let fixture = fixture_hwpx_path();
    if !fixture.exists() {
        return;
    }
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let hwpx_out = tmp.join("roundtrip.hwpx");

    let (_, _, code) =
        run(&["to-json", fixture.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let (_, _, code) =
        run(&["from-json", json_out.to_str().unwrap(), "-o", hwpx_out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(hwpx_out.exists());
    assert!(std::fs::metadata(&hwpx_out).unwrap().len() > 0);
}

#[test]
fn from_json_with_base() {
    let fixture = fixture_hwpx_path();
    if !fixture.exists() {
        return;
    }
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let hwpx_out = tmp.join("roundtrip.hwpx");

    let (_, _, code) =
        run(&["to-json", fixture.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let (val, _, code) = run_json(&[
        "from-json",
        json_out.to_str().unwrap(),
        "-o",
        hwpx_out.to_str().unwrap(),
        "--base",
        fixture.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
}

#[test]
fn from_json_invalid_json() {
    let tmp = test_tmp();
    let bad_json = tmp.join("bad.json");
    std::fs::write(&bad_json, "not valid json").unwrap();
    let out = tmp.join("out.hwpx");
    let (_, _, code) = run(&["from-json", bad_json.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 2);
}

// ─── patch ───

#[test]
fn patch_section() {
    let fixture = fixture_hwpx_path();
    if !fixture.exists() {
        return;
    }
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) = run(&[
        "to-json",
        fixture.to_str().unwrap(),
        "-o",
        json_out.to_str().unwrap(),
        "--section",
        "0",
    ]);
    assert_eq!(code, 0);

    let (_, _, code) = run(&[
        "patch",
        fixture.to_str().unwrap(),
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert!(patched.exists());
}

#[test]
fn patch_section_index_mismatch_warns() {
    let fixture = fixture_hwpx_path();
    if !fixture.exists() {
        return;
    }
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) = run(&[
        "to-json",
        fixture.to_str().unwrap(),
        "-o",
        json_out.to_str().unwrap(),
        "--section",
        "0",
    ]);
    assert_eq!(code, 0);

    // Patch with matching index — no warning expected
    let (_, stderr, code) = run(&[
        "patch",
        fixture.to_str().unwrap(),
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert!(!stderr.contains("Warning:"), "No warning expected for matching indices");
}

#[test]
fn patch_section_index_mismatch_emits_warning() {
    let fixture = fixture_hwpx_path();
    if !fixture.exists() {
        return;
    }
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let patched = tmp.join("patched.hwpx");

    // Extract section 0
    let (_, _, code) = run(&[
        "to-json",
        fixture.to_str().unwrap(),
        "-o",
        json_out.to_str().unwrap(),
        "--section",
        "0",
    ]);
    assert_eq!(code, 0);

    // Modify section_index in JSON to 5 (mismatches --section 0)
    let content = std::fs::read_to_string(&json_out).unwrap();
    let modified = content.replacen("\"section_index\": 0", "\"section_index\": 5", 1);
    std::fs::write(&json_out, modified).unwrap();

    // Patch with --section 0 but JSON says section_index 5 — warning expected
    let (_, stderr, code) = run(&[
        "patch",
        fixture.to_str().unwrap(),
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert!(stderr.contains("Warning:"), "Expected mismatch warning, got: {stderr}");
}

// ─── templates ───

#[test]
fn templates_list() {
    let (stdout, _, code) = run(&["templates", "list"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("default"));
}

#[test]
fn templates_list_json() {
    let (val, _, code) = run_json(&["templates", "list"]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert!(val["presets"].is_array());
}

#[test]
fn templates_show() {
    let (stdout, _, code) = run(&["templates", "show", "default"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Preset: default"));
}

#[test]
fn templates_show_json() {
    let (val, _, code) = run_json(&["templates", "show", "default"]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert_eq!(val["preset"]["name"], "default");
}

#[test]
fn templates_show_nonexistent() {
    let (_, _, code) = run(&["templates", "show", "nonexistent"]);
    assert_eq!(code, 1);
}

// ─── schema ───

#[test]
fn schema_document() {
    let (stdout, _, code) = run(&["schema", "document"]);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(parsed["$schema"].is_string() || parsed["type"].is_string());
}

#[test]
fn schema_document_json_envelope() {
    let (val, _, code) = run_json(&["schema", "document"]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert_eq!(val["type"], "document");
    assert!(val["schema"].is_object());
}

#[test]
fn schema_exported_document() {
    let (val, _, code) = run_json(&["schema", "exported-document"]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert_eq!(val["type"], "exported-document");
}

#[test]
fn schema_exported_section() {
    let (val, _, code) = run_json(&["schema", "exported-section"]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert_eq!(val["type"], "exported-section");
}

#[test]
fn schema_unknown_type() {
    let (_, _, code) = run(&["schema", "foobar"]);
    assert_eq!(code, 1);
}

// ─── error format ───

#[test]
fn json_error_format() {
    let (_, stderr, code) = run(&["--json", "inspect", "/nonexistent/file.hwpx"]);
    assert_ne!(code, 0);
    let err: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(err["status"], "error");
    assert!(err["code"].is_string());
    assert!(err["message"].is_string());
}

// ─── full pipeline: MD → HWPX → JSON → patch → HWPX ───

#[test]
fn full_ai_workflow_pipeline() {
    let md_path = test_md_path();
    if !md_path.exists() {
        return;
    }
    let tmp = test_tmp();
    let hwpx1 = tmp.join("step1.hwpx");
    let json_out = tmp.join("section0.json");
    let hwpx2 = tmp.join("step2.hwpx");

    // Step 1: MD → HWPX
    let (_, _, code) = run(&["convert", md_path.to_str().unwrap(), "-o", hwpx1.to_str().unwrap()]);
    assert_eq!(code, 0, "convert failed");

    // Step 2: HWPX → JSON (section 0)
    let (_, _, code) = run(&[
        "to-json",
        hwpx1.to_str().unwrap(),
        "-o",
        json_out.to_str().unwrap(),
        "--section",
        "0",
    ]);
    assert_eq!(code, 0, "to-json failed");

    // Step 3: Patch section back
    let (_, _, code) = run(&[
        "patch",
        hwpx1.to_str().unwrap(),
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        hwpx2.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "patch failed");

    // Step 4: Inspect result
    let (val, _, code) = run_json(&["inspect", hwpx2.to_str().unwrap()]);
    assert_eq!(code, 0, "inspect failed");
    assert_eq!(val["status"], "ok");
    assert!(!val["sections"].as_array().unwrap().is_empty());
}
