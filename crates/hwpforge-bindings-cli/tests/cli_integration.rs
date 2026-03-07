//! CLI integration tests using process-based invocation.
//!
//! 79 tests covering all 7 commands with output content verification.
//! All fixtures are git-tracked — no silent skips in CI.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

// ─── Helpers ───

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

/// Path to any fixture in tests/fixtures/ by name.
fn fixture(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("tests");
    path.push("fixtures");
    path.push(name);
    assert!(path.exists(), "fixture not found: {}", path.display());
    path
}

/// Path to hwpx_complete_guide.hwpx in examples/.
fn guide_hwpx_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("examples");
    path.push("hwpx_complete_guide.hwpx");
    assert!(path.exists(), "guide fixture not found: {}", path.display());
    path
}

/// Create a temporary markdown file with given content. Returns path.
fn create_test_md(dir: &Path, content: &str) -> PathBuf {
    let path = dir.join("input.md");
    std::fs::write(&path, content).expect("write test md");
    path
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
        // Try to parse stderr as JSON error
        let err_value = serde_json::from_str(&stderr).unwrap_or(serde_json::Value::Null);
        (err_value, stderr, code)
    }
}

/// Run hwpforge with stdin piped. Returns (stdout, stderr, exit_code).
fn run_with_stdin(args: &[&str], stdin_data: &str) -> (String, String, i32) {
    let mut child = Command::new(hwpforge_bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn hwpforge");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(stdin_data.as_bytes()).expect("write stdin");
    }

    let output = child.wait_with_output().expect("wait for hwpforge");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, stderr, code)
}

/// Assert a file is a valid HWPX by running inspect on it.
fn assert_valid_hwpx(path: &Path) {
    let (_, _, code) = run(&["inspect", path.to_str().unwrap()]);
    assert_eq!(code, 0, "inspect failed on {}", path.display());
}

/// Standard test markdown content (Korean proposal).
const TEST_MD: &str = "\
# 제안서

## 서론

이것은 테스트 문서입니다. AI가 편집할 수 있는 마크다운 파일입니다.

## 본론

| 항목 | 설명 | 비용 |
|------|------|------|
| 서버 | AWS EC2 | 100만원 |
| DB | RDS | 50만원 |

## 결론

프로젝트를 승인해 주시기 바랍니다.
";

/// Korean-heavy markdown content.
const KOREAN_MD: &str = "\
# 한국어 테스트 문서

대한민국 헌법 제1조: 대한민국은 민주공화국이다.

## 특수문자 테스트

가나다라마바사아자차카타파하
ㄱㄴㄷㄹㅁㅂㅅㅇㅈㅊㅋㅌㅍㅎ
";

/// Full-featured markdown with headings, table, list, link.
const FULL_FEATURED_MD: &str = "\
# 제목 1

## 제목 2

### 제목 3

본문 텍스트입니다.

- 항목 1
- 항목 2
- 항목 3

1. 번호 1
2. 번호 2

| 열1 | 열2 |
|-----|-----|
| A   | B   |

[링크](https://example.com)

**굵게** *기울임* ~~취소선~~
";

// ═══════════════════════════════════════════════════════════════
// 1. convert (MD → HWPX) — 10 tests
// ═══════════════════════════════════════════════════════════════

#[test]
fn convert_md_to_hwpx() {
    let tmp = test_tmp();
    let md = create_test_md(&tmp, TEST_MD);
    let out = tmp.join("output.hwpx");
    let (_, _, code) = run(&["convert", md.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(out.exists());
    assert!(std::fs::metadata(&out).unwrap().len() > 0);
}

#[test]
fn convert_json_mode() {
    let tmp = test_tmp();
    let md = create_test_md(&tmp, TEST_MD);
    let out = tmp.join("output.hwpx");
    let (val, _, code) = run_json(&["convert", md.to_str().unwrap(), "-o", out.to_str().unwrap()]);
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
    let tmp = test_tmp();
    let md = create_test_md(&tmp, TEST_MD);
    let out = tmp.join("output.hwpx");
    let (_, _, code) = run(&[
        "convert",
        md.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--preset",
        "nonexistent",
    ]);
    assert_eq!(code, 1);
}

#[test]
fn convert_stdin() {
    let tmp = test_tmp();
    let out = tmp.join("output.hwpx");
    let (_, _, code) = run_with_stdin(&["convert", "-", "-o", out.to_str().unwrap()], TEST_MD);
    assert_eq!(code, 0);
    assert!(out.exists());
    assert!(std::fs::metadata(&out).unwrap().len() > 0);
}

#[test]
fn convert_json_fields() {
    let tmp = test_tmp();
    let md = create_test_md(&tmp, TEST_MD);
    let out = tmp.join("output.hwpx");
    let (val, _, code) = run_json(&["convert", md.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert!(val["output"].is_string(), "missing 'output' field");
    assert!(val["sections"].is_number(), "missing 'sections' field");
    assert!(val["paragraphs"].is_number(), "missing 'paragraphs' field");
    assert!(val["size_bytes"].is_number(), "missing 'size_bytes' field");
}

#[test]
fn convert_empty_md() {
    let tmp = test_tmp();
    let md = create_test_md(&tmp, "");
    let out = tmp.join("output.hwpx");
    let (_, _, code) = run(&["convert", md.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(out.exists());
}

#[test]
fn convert_korean_heavy_md() {
    let tmp = test_tmp();
    let md = create_test_md(&tmp, KOREAN_MD);
    let out = tmp.join("output.hwpx");
    let (_, _, code) = run(&["convert", md.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_valid_hwpx(&out);
}

#[test]
fn convert_full_featured_md() {
    let tmp = test_tmp();
    let md = create_test_md(&tmp, FULL_FEATURED_MD);
    let out = tmp.join("output.hwpx");
    let (_, _, code) = run(&["convert", md.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 0);

    // Verify the result has expected structure
    let (val, _, code) = run_json(&["inspect", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    let sec = &val["sections"][0];
    assert!(sec["paragraphs"].as_u64().unwrap() > 3, "expected multiple paragraphs");
    assert!(sec["tables"].as_u64().unwrap() >= 1, "expected at least 1 table");
}

#[test]
fn convert_output_is_valid_hwpx() {
    let tmp = test_tmp();
    let md = create_test_md(&tmp, TEST_MD);
    let out = tmp.join("output.hwpx");
    let (_, _, code) = run(&["convert", md.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_valid_hwpx(&out);
}

// ═══════════════════════════════════════════════════════════════
// 2. inspect — 12 tests
// ═══════════════════════════════════════════════════════════════

#[test]
fn inspect_hwpx() {
    let f = fixture("rect.hwpx");
    let (stdout, _, code) = run(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Document:"));
    assert!(stdout.contains("Sections:"));
}

#[test]
fn inspect_json_mode() {
    let f = fixture("rect.hwpx");
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert!(val["sections"].is_array());
}

#[test]
fn inspect_nonexistent_file() {
    let (_, _, code) = run(&["inspect", "/nonexistent/file.hwpx"]);
    assert_eq!(code, 1);
}

#[test]
fn inspect_styles_flag() {
    let f = fixture("rect.hwpx");
    let (stdout, _, code) = run(&["inspect", f.to_str().unwrap(), "--styles"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Fonts:"), "missing Fonts output");
    assert!(stdout.contains("CharShapes:"), "missing CharShapes output");
    assert!(stdout.contains("ParaShapes:"), "missing ParaShapes output");
}

#[test]
fn inspect_styles_json() {
    let f = fixture("rect.hwpx");
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap(), "--styles"]);
    assert_eq!(code, 0);
    assert!(val["styles"]["fonts"].is_array(), "missing fonts array");
    assert!(val["styles"]["char_shapes"].is_array(), "missing char_shapes array");
    assert!(val["styles"]["para_shapes"].is_array(), "missing para_shapes array");
}

#[test]
fn inspect_complex_doc() {
    let f = guide_hwpx_path();
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["sections"].as_array().unwrap().len(), 4);
}

#[test]
fn inspect_complex_section_counts() {
    let f = guide_hwpx_path();
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    let sec0 = &val["sections"][0];
    assert_eq!(sec0["paragraphs"], 31);
    assert_eq!(sec0["tables"], 1);
    assert_eq!(sec0["images"], 1);
    assert_eq!(sec0["has_header"], true);
    assert_eq!(sec0["has_footer"], true);
}

#[test]
fn inspect_complex_charts() {
    let f = guide_hwpx_path();
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    let sec3 = &val["sections"][3];
    assert_eq!(sec3["charts"], 4);
    assert_eq!(sec3["has_page_number"], true);
}

#[test]
fn inspect_complex_styles() {
    let f = guide_hwpx_path();
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap(), "--styles"]);
    assert_eq!(code, 0);
    let styles = &val["styles"];
    assert_eq!(styles["fonts"].as_array().unwrap().len(), 7);
    assert_eq!(styles["char_shapes"].as_array().unwrap().len(), 9);
    assert_eq!(styles["para_shapes"].as_array().unwrap().len(), 5);
}

#[test]
fn inspect_multicol_paragraphs() {
    let f = fixture("MultiColumn.hwpx");
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["sections"][0]["paragraphs"], 43);
}

#[test]
fn inspect_rect_styles() {
    let f = fixture("rect.hwpx");
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap(), "--styles"]);
    assert_eq!(code, 0);
    let styles = &val["styles"];
    assert_eq!(styles["fonts"].as_array().unwrap().len(), 14);
    assert_eq!(styles["char_shapes"].as_array().unwrap().len(), 7);
    assert_eq!(styles["para_shapes"].as_array().unwrap().len(), 20);
}

#[test]
fn inspect_json_error() {
    let (_, stderr, code) = run(&["--json", "inspect", "/nonexistent/file.hwpx"]);
    assert_ne!(code, 0);
    let err: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(err["status"], "error");
    assert!(err["code"].is_string());
}

// ═══════════════════════════════════════════════════════════════
// 3. to-json — 14 tests
// ═══════════════════════════════════════════════════════════════

#[test]
fn to_json_full_document() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(&json_out).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(parsed["document"].is_object());
}

#[test]
fn to_json_section_extract() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let (_, _, code) =
        run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap(), "--section", "0"]);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(&json_out).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["section_index"], 0);
    assert!(parsed["section"].is_object());
}

#[test]
fn to_json_section_out_of_range() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let (_, _, code) = run(&[
        "to-json",
        f.to_str().unwrap(),
        "-o",
        json_out.to_str().unwrap(),
        "--section",
        "999",
    ]);
    assert_eq!(code, 1);
}

#[test]
fn to_json_json_mode() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let (val, _, code) =
        run_json(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
}

#[test]
fn to_json_no_styles() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let (_, _, code) =
        run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap(), "--no-styles"]);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(&json_out).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(parsed["styles"].is_null(), "styles should be null with --no-styles");
}

#[test]
fn to_json_with_styles() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(&json_out).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(parsed["styles"].is_object(), "styles should be present by default");
    assert!(parsed["styles"]["fonts"].is_array(), "styles.fonts should be array");
}

#[test]
fn to_json_complex_doc() {
    let f = guide_hwpx_path();
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(&json_out).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let sections = parsed["document"]["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 4);
}

#[test]
fn to_json_complex_section_3() {
    let f = guide_hwpx_path();
    let tmp = test_tmp();
    let json_out = tmp.join("section3.json");
    let (_, _, code) =
        run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap(), "--section", "3"]);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(&json_out).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["section_index"], 3);
    assert!(parsed["section"]["paragraphs"].is_array());
}

#[test]
fn to_json_section_4_out_of_range() {
    let f = guide_hwpx_path();
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let (_, stderr, code) = run(&[
        "--json",
        "to-json",
        f.to_str().unwrap(),
        "-o",
        json_out.to_str().unwrap(),
        "--section",
        "4",
    ]);
    assert_eq!(code, 1);
    let err: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert!(
        err["hint"].as_str().unwrap().contains("Valid range"),
        "expected 'Valid range' in hint, got: {}",
        err["hint"]
    );
}

#[test]
fn to_json_multicol_paragraphs() {
    let f = fixture("MultiColumn.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(&json_out).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let paras = parsed["document"]["sections"][0]["paragraphs"].as_array().unwrap();
    assert_eq!(paras.len(), 43);
}

#[test]
fn to_json_roundtrip_content() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json1 = tmp.join("step1.json");
    let hwpx = tmp.join("roundtrip.hwpx");
    let json2 = tmp.join("step2.json");

    // HWPX -> JSON
    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json1.to_str().unwrap()]);
    assert_eq!(code, 0);

    // JSON -> HWPX
    let (_, _, code) = run(&["from-json", json1.to_str().unwrap(), "-o", hwpx.to_str().unwrap()]);
    assert_eq!(code, 0);

    // HWPX -> JSON again
    let (_, _, code) = run(&["to-json", hwpx.to_str().unwrap(), "-o", json2.to_str().unwrap()]);
    assert_eq!(code, 0);

    // Compare section count
    let j1: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json1).unwrap()).unwrap();
    let j2: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json2).unwrap()).unwrap();
    assert_eq!(
        j1["document"]["sections"].as_array().unwrap().len(),
        j2["document"]["sections"].as_array().unwrap().len(),
    );
}

#[test]
fn to_json_nonexistent_file() {
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let (_, stderr, code) =
        run(&["--json", "to-json", "/nonexistent/file.hwpx", "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 1);
    let err: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(err["code"], "FILE_READ_FAILED");
}

#[test]
fn to_json_section_no_styles() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let (_, _, code) = run(&[
        "to-json",
        f.to_str().unwrap(),
        "-o",
        json_out.to_str().unwrap(),
        "--section",
        "0",
        "--no-styles",
    ]);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(&json_out).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(parsed["styles"].is_null(), "styles should be null with --no-styles");
}

#[test]
fn to_json_date_field_paragraphs() {
    let f = fixture("date_field.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(&json_out).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let paras = parsed["document"]["sections"][0]["paragraphs"].as_array().unwrap();
    assert_eq!(paras.len(), 6);
}

// ═══════════════════════════════════════════════════════════════
// 4. from-json — 9 tests
// ═══════════════════════════════════════════════════════════════

#[test]
fn from_json_round_trip() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let hwpx_out = tmp.join("roundtrip.hwpx");

    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let (_, _, code) =
        run(&["from-json", json_out.to_str().unwrap(), "-o", hwpx_out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(hwpx_out.exists());
    assert!(std::fs::metadata(&hwpx_out).unwrap().len() > 0);
}

#[test]
fn from_json_with_base() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let hwpx_out = tmp.join("roundtrip.hwpx");

    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let (val, _, code) = run_json(&[
        "from-json",
        json_out.to_str().unwrap(),
        "-o",
        hwpx_out.to_str().unwrap(),
        "--base",
        f.to_str().unwrap(),
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

#[test]
fn from_json_json_mode() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let hwpx_out = tmp.join("roundtrip.hwpx");

    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let (val, _, code) =
        run_json(&["from-json", json_out.to_str().unwrap(), "-o", hwpx_out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert!(val["output"].is_string(), "missing 'output' field");
    assert!(val["size_bytes"].is_number(), "missing 'size_bytes' field");
}

#[test]
fn from_json_nonexistent_input() {
    let tmp = test_tmp();
    let out = tmp.join("out.hwpx");
    let (_, _, code) = run(&["from-json", "/nonexistent/file.json", "-o", out.to_str().unwrap()]);
    assert_eq!(code, 1);
}

#[test]
fn from_json_nonexistent_base() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let hwpx_out = tmp.join("roundtrip.hwpx");

    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let (_, _, code) = run(&[
        "from-json",
        json_out.to_str().unwrap(),
        "-o",
        hwpx_out.to_str().unwrap(),
        "--base",
        "/nonexistent/base.hwpx",
    ]);
    assert_eq!(code, 1);
}

#[test]
fn from_json_complex_roundtrip() {
    let f = guide_hwpx_path();
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let hwpx_out = tmp.join("roundtrip.hwpx");

    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let (_, _, code) =
        run(&["from-json", json_out.to_str().unwrap(), "-o", hwpx_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    // Verify roundtrip preserves structure
    let (val, _, code) = run_json(&["inspect", hwpx_out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["sections"].as_array().unwrap().len(), 4);
}

#[test]
fn from_json_output_is_valid_hwpx() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let hwpx_out = tmp.join("roundtrip.hwpx");

    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let (_, _, code) =
        run(&["from-json", json_out.to_str().unwrap(), "-o", hwpx_out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_valid_hwpx(&hwpx_out);
}

#[test]
fn from_json_preserves_styles() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json1 = tmp.join("doc.json");
    let hwpx_out = tmp.join("roundtrip.hwpx");
    let json2 = tmp.join("doc2.json");

    // Export with styles
    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json1.to_str().unwrap()]);
    assert_eq!(code, 0);

    // Round-trip
    let (_, _, code) =
        run(&["from-json", json1.to_str().unwrap(), "-o", hwpx_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    // Re-export and check styles
    let (_, _, code) = run(&["to-json", hwpx_out.to_str().unwrap(), "-o", json2.to_str().unwrap()]);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(&json2).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(parsed["styles"].is_object(), "styles should be preserved after roundtrip");
}

// ═══════════════════════════════════════════════════════════════
// 5. patch — 10 tests
// ═══════════════════════════════════════════════════════════════

#[test]
fn patch_section() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) =
        run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap(), "--section", "0"]);
    assert_eq!(code, 0);

    let (_, _, code) = run(&[
        "patch",
        f.to_str().unwrap(),
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
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) =
        run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap(), "--section", "0"]);
    assert_eq!(code, 0);

    let (_, stderr, code) = run(&[
        "patch",
        f.to_str().unwrap(),
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
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) =
        run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap(), "--section", "0"]);
    assert_eq!(code, 0);

    // Modify section_index in JSON to 5
    let content = std::fs::read_to_string(&json_out).unwrap();
    let modified = content.replacen("\"section_index\": 0", "\"section_index\": 5", 1);
    std::fs::write(&json_out, modified).unwrap();

    let (_, stderr, code) = run(&[
        "patch",
        f.to_str().unwrap(),
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert!(stderr.contains("Warning:"), "Expected mismatch warning, got: {stderr}");
}

#[test]
fn patch_json_mode() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) =
        run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap(), "--section", "0"]);
    assert_eq!(code, 0);

    let (val, _, code) = run_json(&[
        "patch",
        f.to_str().unwrap(),
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert_eq!(val["patched_section"], 0);
    assert!(val["sections"].is_number());
}

#[test]
fn patch_out_of_range() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) =
        run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap(), "--section", "0"]);
    assert_eq!(code, 0);

    let (_, _, code) = run(&[
        "patch",
        f.to_str().unwrap(),
        "--section",
        "999",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
}

#[test]
fn patch_out_of_range_hint() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) =
        run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap(), "--section", "0"]);
    assert_eq!(code, 0);

    let (_, stderr, code) = run(&[
        "--json",
        "patch",
        f.to_str().unwrap(),
        "--section",
        "999",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    // stderr may contain both a warning and an error JSON (one per line); parse the last line.
    let last_line = stderr.trim().lines().last().expect("no stderr output");
    let err: serde_json::Value = serde_json::from_str(last_line).unwrap();
    assert!(
        err["hint"].as_str().unwrap().contains("Valid range"),
        "expected 'Valid range' in hint"
    );
}

#[test]
fn patch_complex_doc() {
    let f = guide_hwpx_path();
    let tmp = test_tmp();
    let json_out = tmp.join("section0.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) =
        run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap(), "--section", "0"]);
    assert_eq!(code, 0);

    let (_, _, code) = run(&[
        "patch",
        f.to_str().unwrap(),
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);

    // Verify patched document still has 4 sections
    let (val, _, code) = run_json(&["inspect", patched.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["sections"].as_array().unwrap().len(), 4);
}

#[test]
fn patch_result_is_valid_hwpx() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let patched = tmp.join("patched.hwpx");
    let json_verify = tmp.join("verify.json");

    let (_, _, code) =
        run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap(), "--section", "0"]);
    assert_eq!(code, 0);

    let (_, _, code) = run(&[
        "patch",
        f.to_str().unwrap(),
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);

    // Verify patched file can be exported to JSON
    let (_, _, code) =
        run(&["to-json", patched.to_str().unwrap(), "-o", json_verify.to_str().unwrap()]);
    assert_eq!(code, 0);
}

#[test]
fn patch_json_warning() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) =
        run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap(), "--section", "0"]);
    assert_eq!(code, 0);

    // Modify section_index to create mismatch
    let content = std::fs::read_to_string(&json_out).unwrap();
    let modified = content.replacen("\"section_index\": 0", "\"section_index\": 5", 1);
    std::fs::write(&json_out, modified).unwrap();

    let (_, stderr, code) = run(&[
        "--json",
        "patch",
        f.to_str().unwrap(),
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    let warn: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(warn["status"], "warning");
    assert_eq!(warn["code"], "SECTION_INDEX_MISMATCH");
}

#[test]
fn patch_nonexistent_base() {
    let f = fixture("rect.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) =
        run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap(), "--section", "0"]);
    assert_eq!(code, 0);

    let (_, _, code) = run(&[
        "patch",
        "/nonexistent/base.hwpx",
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
}

// ═══════════════════════════════════════════════════════════════
// 6. templates — 6 tests
// ═══════════════════════════════════════════════════════════════

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

#[test]
fn templates_show_json_fields() {
    let (val, _, code) = run_json(&["templates", "show", "default"]);
    assert_eq!(code, 0);
    assert!(val["preset"]["font"].is_string(), "missing font field");
    assert!(val["preset"]["page_size"].is_string(), "missing page_size field");
}

// ═══════════════════════════════════════════════════════════════
// 7. schema — 7 tests
// ═══════════════════════════════════════════════════════════════

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

#[test]
fn schema_default_type() {
    // No argument -> defaults to "document"
    let (stdout_default, _, code1) = run(&["schema"]);
    assert_eq!(code1, 0);
    let (stdout_explicit, _, code2) = run(&["schema", "document"]);
    assert_eq!(code2, 0);
    assert_eq!(stdout_default, stdout_explicit);
}

#[test]
fn schema_has_properties() {
    let (stdout, _, code) = run(&["schema", "document"]);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    // JSON Schema should have either "properties" or "$defs"
    assert!(
        parsed.get("properties").is_some() || parsed.get("$defs").is_some(),
        "schema should have properties or $defs"
    );
}

// ═══════════════════════════════════════════════════════════════
// 8. Cross-cutting — 11 tests
// ═══════════════════════════════════════════════════════════════

#[test]
fn json_error_format() {
    let (_, stderr, code) = run(&["--json", "inspect", "/nonexistent/file.hwpx"]);
    assert_ne!(code, 0);
    let err: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(err["status"], "error");
    assert!(err["code"].is_string());
    assert!(err["message"].is_string());
}

#[test]
fn json_error_has_hint() {
    let (_, stderr, code) = run(&["--json", "schema", "foobar"]);
    assert_ne!(code, 0);
    let err: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(err["status"], "error");
    assert!(err["hint"].is_string(), "error should include hint field");
}

#[test]
fn unknown_subcommand() {
    let (_, _, code) = run(&["nonexistent-command"]);
    assert_ne!(code, 0);
}

#[test]
fn full_ai_workflow_pipeline() {
    let tmp = test_tmp();
    let md = create_test_md(&tmp, TEST_MD);
    let hwpx1 = tmp.join("step1.hwpx");
    let json_out = tmp.join("section0.json");
    let hwpx2 = tmp.join("step2.hwpx");

    // Step 1: MD -> HWPX
    let (_, _, code) = run(&["convert", md.to_str().unwrap(), "-o", hwpx1.to_str().unwrap()]);
    assert_eq!(code, 0, "convert failed");

    // Step 2: HWPX -> JSON (section 0)
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

#[test]
fn complex_full_pipeline() {
    let f = guide_hwpx_path();
    let tmp = test_tmp();
    let json_out = tmp.join("section0.json");
    let patched = tmp.join("patched.hwpx");

    // Inspect original
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["sections"].as_array().unwrap().len(), 4);

    // Extract section 0
    let (_, _, code) =
        run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap(), "--section", "0"]);
    assert_eq!(code, 0);

    // Patch section 0 back
    let (_, _, code) = run(&[
        "patch",
        f.to_str().unwrap(),
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);

    // Inspect patched — should still have 4 sections
    let (val, _, code) = run_json(&["inspect", patched.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["sections"].as_array().unwrap().len(), 4);
}

#[test]
fn roundtrip_preserves_section_count() {
    let f = guide_hwpx_path();
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let hwpx_out = tmp.join("roundtrip.hwpx");

    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let (_, _, code) =
        run(&["from-json", json_out.to_str().unwrap(), "-o", hwpx_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let (val, _, code) = run_json(&["inspect", hwpx_out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["sections"].as_array().unwrap().len(), 4);
}

#[test]
fn invalid_file_format() {
    let tmp = test_tmp();
    // Create a valid ZIP but not HWPX
    let not_hwpx = tmp.join("not_hwpx.hwpx");
    std::fs::write(&not_hwpx, b"PK\x03\x04not a real zip content").unwrap();
    let (_, _, code) = run(&["inspect", not_hwpx.to_str().unwrap()]);
    assert_eq!(code, 2, "expected exit code 2 for decode failure");
}

#[test]
fn binary_garbage_input() {
    let tmp = test_tmp();
    let garbage = tmp.join("garbage.hwpx");
    std::fs::write(&garbage, [0xFF, 0xFE, 0x00, 0x01, 0xAB, 0xCD]).unwrap();
    let (_, _, code) = run(&["inspect", garbage.to_str().unwrap()]);
    assert_ne!(code, 0, "garbage input should fail");
}

#[test]
fn empty_file_input() {
    let tmp = test_tmp();
    let empty = tmp.join("empty.hwpx");
    std::fs::write(&empty, b"").unwrap();
    let (_, _, code) = run(&["inspect", empty.to_str().unwrap()]);
    assert_ne!(code, 0, "empty file should fail");
}

#[test]
fn bad_output_directory() {
    let tmp = test_tmp();
    let md = create_test_md(&tmp, TEST_MD);
    let (_, _, code) =
        run(&["convert", md.to_str().unwrap(), "-o", "/nonexistent/dir/output.hwpx"]);
    assert_eq!(code, 1, "writing to nonexistent directory should fail");
}
