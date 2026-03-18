//! CLI integration tests using process-based invocation.
//!
//! 79 tests covering all 7 commands with output content verification.
//! All fixtures are git-tracked — no silent skips in CI.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use hwpforge_core::control::Control;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::RunContent;
use hwpforge_smithy_hwpx::ExportedSection;
use serde_json::json;
use zip::ZipArchive;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

// ─── Helpers ───

/// Path to the built binary (set by cargo for integration tests).
fn hwpforge_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hwpforge"))
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

fn read_hwpx_entry(path: &Path, entry: &str) -> String {
    let bytes = std::fs::read(path).expect("read hwpx");
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).expect("open hwpx zip");
    let mut file = archive.by_name(entry).expect("zip entry exists");
    let mut content = String::new();
    file.read_to_string(&mut content).expect("read zip entry as string");
    content
}

fn hwpx_has_entry(path: &Path, entry: &str) -> bool {
    let bytes = std::fs::read(path).expect("read hwpx");
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).expect("open hwpx zip");
    let exists = archive.by_name(entry).is_ok();
    exists
}

fn hwpx_changed_entries(base: &Path, patched: &Path) -> Vec<String> {
    let base_bytes = std::fs::read(base).expect("read base hwpx");
    let patched_bytes = std::fs::read(patched).expect("read patched hwpx");
    let mut base_zip = ZipArchive::new(std::io::Cursor::new(base_bytes)).expect("open base zip");
    let mut patched_zip =
        ZipArchive::new(std::io::Cursor::new(patched_bytes)).expect("open patched zip");

    let mut changed: Vec<String> = Vec::new();
    for index in 0..base_zip.len() {
        let name = {
            let file = base_zip.by_index(index).expect("base entry by index");
            file.name().to_string()
        };
        let mut base_file = base_zip.by_name(&name).expect("base entry exists");
        let mut patched_file = patched_zip.by_name(&name).expect("patched entry exists");
        let mut base_data = Vec::new();
        let mut patched_data = Vec::new();
        base_file.read_to_end(&mut base_data).expect("read base entry");
        patched_file.read_to_end(&mut patched_data).expect("read patched entry");
        if base_data != patched_data {
            changed.push(name);
        }
    }
    changed
}

fn replace_first_table_text_in_section(exported: &mut ExportedSection, replacement: &str) -> bool {
    replace_first_table_text_in_paragraphs(&mut exported.section.paragraphs, replacement)
}

fn replace_first_text_in_section(exported: &mut ExportedSection, replacement: &str) -> bool {
    replace_first_text_run(&mut exported.section.paragraphs, replacement)
}

fn replace_first_table_text_in_paragraphs(paragraphs: &mut [Paragraph], replacement: &str) -> bool {
    for paragraph in paragraphs {
        if replace_first_table_text_in_runs(&mut paragraph.runs, replacement) {
            return true;
        }
    }
    false
}

fn replace_first_table_text_in_runs(
    runs: &mut [hwpforge_core::run::Run],
    replacement: &str,
) -> bool {
    for run in runs {
        match &mut run.content {
            RunContent::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        if replace_first_text_run(&mut cell.paragraphs, replacement) {
                            return true;
                        }
                    }
                }
            }
            RunContent::Control(control) => {
                let paragraphs: Option<&mut Vec<Paragraph>> = match control.as_mut() {
                    Control::TextBox { paragraphs, .. }
                    | Control::Footnote { paragraphs, .. }
                    | Control::Endnote { paragraphs, .. }
                    | Control::Ellipse { paragraphs, .. }
                    | Control::Polygon { paragraphs, .. } => Some(paragraphs),
                    _ => None,
                };
                if let Some(paragraphs) = paragraphs {
                    if replace_first_table_text_in_paragraphs(paragraphs, replacement) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn replace_first_text_run(paragraphs: &mut [Paragraph], replacement: &str) -> bool {
    for paragraph in paragraphs {
        for run in &mut paragraph.runs {
            if let RunContent::Text(text) = &mut run.content {
                *text = replacement.to_string();
                return true;
            }
        }
    }
    false
}

fn extract_u32_attribute_values_after(
    xml: &str,
    scope_prefix: &str,
    attribute: &str,
) -> std::collections::BTreeSet<u32> {
    let scope = format!("{scope_prefix}{attribute}=\"");
    let mut values = std::collections::BTreeSet::new();
    let mut search_from = 0usize;
    while let Some(start) = xml[search_from..].find(&scope) {
        let value_start = search_from + start + scope.len();
        let Some(value_end_rel) = xml[value_start..].find('"') else {
            break;
        };
        let value_end = value_start + value_end_rel;
        if let Ok(value) = xml[value_start..value_end].parse::<u32>() {
            values.insert(value);
        }
        search_from = value_end + 1;
    }
    values
}

fn extract_xml_u32_attribute_values(xml: &str, attribute: &str) -> std::collections::BTreeSet<u32> {
    let needle = format!(r#"{attribute}=""#);
    let mut values = std::collections::BTreeSet::new();
    let mut search_from = 0usize;
    while let Some(start) = xml[search_from..].find(&needle) {
        let value_start = search_from + start + needle.len();
        let Some(value_end_rel) = xml[value_start..].find('"') else {
            break;
        };
        let value_end = value_start + value_end_rel;
        if let Ok(value) = xml[value_start..value_end].parse::<u32>() {
            values.insert(value);
        }
        search_from = value_end + 1;
    }
    values
}

fn assert_single_chart_ole_evidence(
    value: &serde_json::Value,
    expected_chart_xml: &str,
    expected_source_ole_bindata: &str,
    expected_companion_ole_bindata: &str,
) {
    assert_eq!(value["chart_evidence"]["assessment"], "ole-backed-gso-evidence");
    assert_eq!(value["chart_evidence"]["source"]["gso_ctrl_count"], 1);
    assert_eq!(value["chart_evidence"]["source"]["shape_component_ole_count"], 1);
    assert_eq!(value["chart_evidence"]["source"]["chart_data_tag_count"], 0);

    let source_ole_paths =
        value["chart_evidence"]["source"]["ole_bin_data_paths"].as_array().unwrap();
    assert_eq!(source_ole_paths.len(), 1);
    assert_eq!(source_ole_paths[0], expected_source_ole_bindata);

    let companion = &value["chart_evidence"]["companion"];
    let chart_xml_paths = companion["chart_xml_paths"].as_array().unwrap();
    assert_eq!(chart_xml_paths.len(), 1);
    assert_eq!(chart_xml_paths[0], expected_chart_xml);

    let ole_bindata_paths = companion["ole_bindata_paths"].as_array().unwrap();
    assert_eq!(ole_bindata_paths.len(), 1);
    assert_eq!(ole_bindata_paths[0], expected_companion_ole_bindata);

    assert_eq!(companion["case_chart_count"], 1);
    assert_eq!(companion["default_ole_count"], 1);
    assert!(companion["switch_count"].as_u64().unwrap() >= 1);
}

fn comparison_verdict<'a>(value: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    value["comparisons"].as_array().and_then(|comparisons| {
        comparisons.iter().find_map(|comparison| {
            (comparison["field"].as_str() == Some(field)).then(|| comparison["verdict"].as_str())
        })
    })?
}

fn csv_to_json_array(csv: &str) -> serde_json::Value {
    if csv.is_empty() {
        return json!([]);
    }

    let values: Vec<i32> = csv
        .split(',')
        .map(|value| value.parse::<i32>().expect("csv sizing metric must be integer"))
        .collect();
    json!(values)
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
    let (_, value, stderr, code) = run_json_with_stdout(args);
    (value, stderr, code)
}

/// Run hwpforge with --json flag prepended and return raw stdout too.
fn run_json_with_stdout(args: &[&str]) -> (String, serde_json::Value, String, i32) {
    let mut full_args = vec!["--json"];
    full_args.extend_from_slice(args);
    let (stdout, stderr, code) = run(&full_args);
    if code == 0 {
        let value: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("invalid JSON output: {e}\nstdout: {stdout}"));
        (stdout, value, stderr, code)
    } else {
        // Try to parse stderr as JSON error
        let err_value = serde_json::from_str(&stderr).unwrap_or(serde_json::Value::Null);
        (stdout, err_value, stderr, code)
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

fn convert_hwp5_fixture_and_audit_ok(
    fixture_name: &str,
    tmp: &Path,
) -> (PathBuf, serde_json::Value) {
    let source = fixture(fixture_name);
    let output = tmp.join(fixture_name.replace(".hwp", ".hwpx"));
    hwpforge_smithy_hwp5::hwp5_to_hwpx(&source, &output)
        .expect("convert hwp5 fixture for CLI integration");

    let (val, _, code) =
        run_json(&["audit-hwp5", source.to_str().unwrap(), output.to_str().unwrap()]);
    assert_eq!(code, 0, "audit exit code for {fixture_name}");
    assert_eq!(val["status"], "ok", "audit status for {fixture_name}");
    (output, val)
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
    assert_valid_hwpx(&out);
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
    assert_eq!(styles["fonts"].as_array().unwrap().len(), 14);
    assert_eq!(styles["char_shapes"].as_array().unwrap().len(), 8);
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
fn inspect_deep_counts_image_in_table_cell() {
    let f = fixture("img_05_image_in_table_cell.hwpx");
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    let sec0 = &val["sections"][0];
    assert_eq!(sec0["tables"], 1);
    assert_eq!(sec0["images"], 1);
    assert_eq!(sec0["text_boxes"], 0);
    assert_eq!(sec0["deep_paragraphs"], 7);
}

#[test]
fn inspect_deep_counts_header_footer_image_fixture() {
    let f = fixture("mixed_02a_header_image_footer_text_real.hwpx");
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    let sec0 = &val["sections"][0];
    assert_eq!(sec0["images"], 1);
    assert_eq!(sec0["has_header"], true);
    assert_eq!(sec0["has_footer"], true);
    assert_eq!(sec0["deep_non_empty_paragraphs"], 1);
}

#[test]
fn inspect_deep_counts_textbox_with_image_fixture() {
    let f = fixture("mixed_02b_textbox_with_image_real.hwpx");
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    let sec0 = &val["sections"][0];
    assert_eq!(sec0["images"], 1);
    assert_eq!(sec0["text_boxes"], 1);
    assert_eq!(sec0["deep_non_empty_paragraphs"], 3);
}

#[test]
fn inspect_deep_counts_line_fixture() {
    let f = fixture("line_simple.hwpx");
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["sections"][0]["lines"], 1);
    assert_eq!(val["sections"][0]["rectangles"], 0);
    assert_eq!(val["sections"][0]["polygons"], 0);
}

#[test]
fn inspect_deep_counts_rect_fixture() {
    let f = fixture("rect_simple.hwpx");
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["sections"][0]["rectangles"], 1);
    assert_eq!(val["sections"][0]["text_boxes"], 0);
}

#[test]
fn inspect_deep_counts_polygon_fixture() {
    let f = fixture("polygon_simple.hwpx");
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["sections"][0]["polygons"], 1);
    assert_eq!(val["sections"][0]["lines"], 0);
}

#[test]
fn inspect_json_error() {
    let (_, stderr, code) = run(&["--json", "inspect", "/nonexistent/file.hwpx"]);
    assert_ne!(code, 0);
    let err: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(err["status"], "error");
    assert!(err["code"].is_string());
}

#[test]
fn audit_hwp5_human_report() {
    let source = fixture("hwp5_01.hwp");
    let tmp = test_tmp();
    let out = tmp.join("hwp5_01.hwpx");
    hwpforge_smithy_hwp5::hwp5_to_hwpx(&source, &out).expect("convert hwp5 fixture");

    let (stdout, _, code) = run(&["audit-hwp5", source.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Audit:"));
    assert!(stdout.contains("Status:"));
    assert!(stdout.contains("Source metrics come from parser-backed HWP5 semantic truth."));
    assert!(stdout.contains("Visual Checklist:"));
    assert!(stdout.contains("tables"));
}

#[test]
fn audit_hwp5_json_report() {
    let source = fixture("hwp5_02.hwp");
    let tmp = test_tmp();
    let out = tmp.join("hwp5_02.hwpx");
    hwpforge_smithy_hwp5::hwp5_to_hwpx(&source, &out).expect("convert hwp5 fixture");

    let (val, _, code) = run_json(&["audit-hwp5", source.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert_eq!(val["source"]["format"], "HWP5");
    assert_eq!(val["output"]["format"], "HWPX");
    assert!(val["comparisons"].as_array().unwrap().len() >= 8);
    assert!(!val["section_comparisons"].as_array().unwrap().is_empty());
    assert!(val["checklist"].as_array().unwrap().len() >= 3);
}

#[test]
fn audit_hwp5_chart_reports_ole_evidence_note() {
    let source = fixture("chart_01_single_column.hwp");
    let tmp = test_tmp();
    let out = tmp.join("chart_01.hwpx");
    hwpforge_smithy_hwp5::hwp5_to_hwpx(&source, &out).expect("convert hwp5 chart fixture");

    let (val, _, code) = run_json(&["audit-hwp5", source.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "mismatch");
    assert!(val["source"]["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note.as_str() == Some("ole-backed-gso-evidence: 1")));
    assert_eq!(val["source"]["totals"]["ole_objects"], 1);
    assert_eq!(val["output"]["totals"]["ole_objects"], 0);
}

#[test]
fn audit_hwp5_chart_reports_ole_backed_source_evidence() {
    let source = fixture("chart_01_single_column.hwp");
    let companion = fixture("chart_01_single_column.hwpx");

    let (val, _, code) =
        run_json(&["audit-hwp5", source.to_str().unwrap(), companion.to_str().unwrap()]);
    assert_eq!(code, 0);
    let source_notes = val["source"]["notes"].as_array().unwrap();
    let output_notes = val["output"]["notes"].as_array().unwrap();
    assert!(source_notes.iter().any(|note| note == "ole-backed-gso-evidence: 1"));
    assert!(source_notes.iter().any(|note| note == "ole-high-confidence: 1"));
    assert!(output_notes.iter().any(|note| note == "hwpx-ole-fallback-present: 1"));
    assert_eq!(val["source"]["totals"]["ole_objects"], 1);
    assert_eq!(val["output"]["totals"]["ole_objects"], 1);
}

#[test]
fn audit_hwp5_line_fixture_reports_line_metric() {
    let source = fixture("line_simple.hwp");
    let tmp = test_tmp();
    let out = tmp.join("line_simple.hwpx");
    hwpforge_smithy_hwp5::hwp5_to_hwpx(&source, &out).expect("convert hwp5 line fixture");

    let (val, _, code) = run_json(&["audit-hwp5", source.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert_eq!(val["source"]["totals"]["lines"], 1);
    assert_eq!(val["output"]["totals"]["lines"], 1);
    assert_eq!(val["source"]["totals"]["polygons"], 0);
    assert_eq!(val["output"]["totals"]["polygons"], 0);
}

#[test]
fn audit_hwp5_table_repeat_header_notes_source_truth() {
    let source = fixture("table_06_repeat_header_row.hwp");
    let companion = fixture("table_06_repeat_header_row.hwpx");

    let (val, _, code) =
        run_json(&["audit-hwp5", source.to_str().unwrap(), companion.to_str().unwrap()]);
    assert_eq!(code, 0);
    let source_notes = val["source"]["notes"].as_array().unwrap();
    assert!(source_notes.iter().any(|note| note == "table-page-break-cell: 1"));
    assert!(source_notes.iter().any(|note| note == "table-repeat-header-on: 1"));
    assert_eq!(val["source"]["table_properties"]["repeat_header_tables"], 1);
    assert_eq!(val["output"]["table_properties"]["repeat_header_tables"], 1);
    assert_eq!(comparison_verdict(&val, "table_repeat_header_tables"), Some("MATCH"));
    assert_eq!(comparison_verdict(&val, "table_page_break_cell"), Some("MATCH"));
}

#[test]
fn audit_hwp5_table_repeat_header_multi_page_source_truth() {
    let cases = [
        ("table_06c_repeat_header_multi_page.hwp", "table_06c_repeat_header_multi_page.hwpx", 1),
        (
            "table_06d_no_repeat_header_multi_page.hwp",
            "table_06d_no_repeat_header_multi_page.hwpx",
            0,
        ),
    ];

    for (source_name, companion_name, expected_repeat_header_tables) in cases {
        let source = fixture(source_name);
        let companion = fixture(companion_name);
        let (val, _, code) =
            run_json(&["audit-hwp5", source.to_str().unwrap(), companion.to_str().unwrap()]);
        assert_eq!(code, 0, "audit exit code for {source_name}");
        assert_eq!(val["status"], "ok", "audit status for {source_name}");
        assert_eq!(val["source"]["table_properties"]["page_break_cell"], 1);
        assert_eq!(val["output"]["table_properties"]["page_break_cell"], 1);
        assert_eq!(val["source"]["table_properties"]["header_rows"], 1);
        assert_eq!(val["output"]["table_properties"]["header_rows"], 1);
        assert_eq!(
            val["source"]["table_properties"]["repeat_header_tables"],
            expected_repeat_header_tables
        );
        assert_eq!(
            val["output"]["table_properties"]["repeat_header_tables"],
            expected_repeat_header_tables
        );
        assert_eq!(comparison_verdict(&val, "table_page_break_cell"), Some("MATCH"));
        assert_eq!(comparison_verdict(&val, "table_repeat_header_tables"), Some("MATCH"));
        assert_eq!(comparison_verdict(&val, "table_header_rows"), Some("MATCH"));
    }
}

#[test]
fn audit_hwp5_table_page_break_modes_source_truth() {
    let table_mode = fixture("table_09a_page_break_cell.hwp");
    let none_mode = fixture("table_09c_page_break_none.hwp");
    let cell_mode = fixture("table_09d_page_break_cell_explicit.hwp");

    let (table_val, _, table_code) = run_json(&[
        "audit-hwp5",
        table_mode.to_str().unwrap(),
        fixture("table_09a_page_break_cell.hwpx").to_str().unwrap(),
    ]);
    assert_eq!(table_code, 0);
    let table_notes = table_val["source"]["notes"].as_array().unwrap();
    assert!(table_notes.iter().any(|note| note == "table-page-break-table: 1"));
    assert_eq!(table_val["source"]["table_properties"]["page_break_table"], 1);
    assert_eq!(table_val["output"]["table_properties"]["page_break_table"], 1);
    assert_eq!(comparison_verdict(&table_val, "table_page_break_table"), Some("MATCH"));

    let (none_val, _, none_code) = run_json(&[
        "audit-hwp5",
        none_mode.to_str().unwrap(),
        fixture("table_09c_page_break_none.hwpx").to_str().unwrap(),
    ]);
    assert_eq!(none_code, 0);
    let none_notes = none_val["source"]["notes"].as_array().unwrap();
    assert!(none_notes.iter().any(|note| note == "table-page-break-none: 1"));
    assert_eq!(none_val["source"]["table_properties"]["page_break_none"], 1);
    assert_eq!(none_val["output"]["table_properties"]["page_break_none"], 1);
    assert_eq!(comparison_verdict(&none_val, "table_page_break_none"), Some("MATCH"));

    let (cell_val, _, cell_code) = run_json(&[
        "audit-hwp5",
        cell_mode.to_str().unwrap(),
        fixture("table_09d_page_break_cell_explicit.hwpx").to_str().unwrap(),
    ]);
    assert_eq!(cell_code, 0);
    let cell_notes = cell_val["source"]["notes"].as_array().unwrap();
    assert!(cell_notes.iter().any(|note| note == "table-page-break-cell: 1"));
    assert_eq!(cell_val["source"]["table_properties"]["page_break_cell"], 1);
    assert_eq!(cell_val["output"]["table_properties"]["page_break_cell"], 1);
    assert_eq!(comparison_verdict(&cell_val, "table_page_break_cell"), Some("MATCH"));
}

#[test]
fn audit_hwp5_table_border_fill_notes_source_truth() {
    let source = fixture("table_03_border_fill_variants.hwp");
    let companion = fixture("table_03_border_fill_variants.hwpx");

    let (val, _, code) =
        run_json(&["audit-hwp5", source.to_str().unwrap(), companion.to_str().unwrap()]);
    assert_eq!(code, 0);
    let source_notes = val["source"]["notes"].as_array().unwrap();
    assert!(source_notes.iter().any(|note| {
        note.as_str().is_some_and(|note| note.starts_with("table-cell-border-fill-ids: "))
    }));
    assert_eq!(val["source"]["table_properties"]["table_border_fill_ids"], json!([3]));
    assert_eq!(val["output"]["table_properties"]["table_border_fill_ids"], json!([3]));
    assert_eq!(val["source"]["table_properties"]["cell_border_fill_ids"], json!([4, 5, 6, 7]));
    assert_eq!(val["output"]["table_properties"]["cell_border_fill_ids"], json!([4, 5, 6, 7]));
    assert_eq!(comparison_verdict(&val, "table_border_fill_ids"), Some("MATCH"));
    assert_eq!(comparison_verdict(&val, "table_cell_border_fill_ids"), Some("MATCH"));
}

#[test]
fn convert_hwp5_table_page_break_and_repeat_header_parity() {
    let cases = [
        ("table_06_repeat_header_row.hwp", "table_repeat_header_tables", "MATCH"),
        ("table_06b_no_repeat_header_row.hwp", "table_repeat_header_tables", "MATCH"),
        ("table_09a_page_break_cell.hwp", "table_page_break_table", "MATCH"),
        ("table_09c_page_break_none.hwp", "table_page_break_none", "MATCH"),
        ("table_09d_page_break_cell_explicit.hwp", "table_page_break_cell", "MATCH"),
    ];

    let tmp = test_tmp();
    for (fixture_name, field, expected_verdict) in cases {
        let source = fixture(fixture_name);
        let output = tmp.join(fixture_name.replace(".hwp", ".hwpx"));
        hwpforge_smithy_hwp5::hwp5_to_hwpx(&source, &output).expect("convert hwp5 table fixture");

        let (val, _, code) =
            run_json(&["audit-hwp5", source.to_str().unwrap(), output.to_str().unwrap()]);
        assert_eq!(code, 0, "audit exit code for {fixture_name}");
        assert_eq!(
            comparison_verdict(&val, field),
            Some(expected_verdict),
            "table parity field {field} for {fixture_name}"
        );
    }
}

#[test]
fn convert_hwp5_table_repeat_header_multi_page_visual_gate() {
    let cases = [
        ("table_06c_repeat_header_multi_page.hwp", "repeatHeader=\"1\""),
        ("table_06d_no_repeat_header_multi_page.hwp", "repeatHeader=\"0\""),
    ];

    let tmp = test_tmp();
    for (fixture_name, expected_repeat_header_attr) in cases {
        let source = fixture(fixture_name);
        let output = tmp.join(fixture_name.replace(".hwp", ".hwpx"));
        hwpforge_smithy_hwp5::hwp5_to_hwpx(&source, &output)
            .expect("convert hwp5 repeat-header multi-page fixture");

        let (val, _, code) =
            run_json(&["audit-hwp5", source.to_str().unwrap(), output.to_str().unwrap()]);
        assert_eq!(code, 0, "audit exit code for {fixture_name}");
        assert_eq!(val["status"], "ok", "audit status for {fixture_name}");
        assert_eq!(comparison_verdict(&val, "table_page_break_cell"), Some("MATCH"));
        assert_eq!(comparison_verdict(&val, "table_repeat_header_tables"), Some("MATCH"));
        assert_eq!(comparison_verdict(&val, "table_header_rows"), Some("MATCH"));

        let section_xml = read_hwpx_entry(&output, "Contents/section0.xml");
        assert!(
            section_xml.contains("pageBreak=\"CELL\""),
            "generated section0.xml must keep pageBreak=CELL for {fixture_name}"
        );
        assert!(
            section_xml.contains(expected_repeat_header_attr),
            "generated section0.xml must keep {expected_repeat_header_attr} for {fixture_name}"
        );
        assert_eq!(
            section_xml.matches(" header=\"1\"").count(),
            3,
            "generated section0.xml must preserve first-row header markers for {fixture_name}"
        );
        assert!(
            section_xml.contains("rowCnt=\"100\""),
            "generated section0.xml must preserve multi-page row count for {fixture_name}"
        );
        assert!(
            section_xml.contains("colCnt=\"3\""),
            "generated section0.xml must preserve 3-column layout for {fixture_name}"
        );
    }
}

#[test]
fn convert_hwp5_table_border_fill_and_cell_height_parity() {
    let cases = [
        (
            "table_03_border_fill_variants.hwp",
            "table_border_fill_ids",
            json!([3]),
            json!([4, 5, 6, 7]),
            json!([282]),
        ),
        (
            "table_04_vertical_align.hwp",
            "table_cell_heights_hwp",
            json!([3]),
            json!([3]),
            json!([7697]),
        ),
        (
            "table_05_cell_margin_padding.hwp",
            "table_cell_heights_hwp",
            json!([3]),
            json!([3]),
            json!([282, 1281]),
        ),
    ];

    let tmp = test_tmp();
    for (fixture_name, focus_field, expected_table_ids, expected_cell_ids, expected_heights) in
        cases
    {
        let source = fixture(fixture_name);
        let output = tmp.join(fixture_name.replace(".hwp", ".hwpx"));
        hwpforge_smithy_hwp5::hwp5_to_hwpx(&source, &output).expect("convert hwp5 table fixture");

        let (val, _, code) =
            run_json(&["audit-hwp5", source.to_str().unwrap(), output.to_str().unwrap()]);
        assert_eq!(code, 0, "audit exit code for {fixture_name}");
        assert_eq!(val["source"]["table_properties"]["table_border_fill_ids"], expected_table_ids);
        assert_eq!(val["output"]["table_properties"]["table_border_fill_ids"], expected_table_ids);
        assert_eq!(val["source"]["table_properties"]["cell_border_fill_ids"], expected_cell_ids);
        assert_eq!(val["output"]["table_properties"]["cell_border_fill_ids"], expected_cell_ids);
        assert_eq!(val["source"]["table_properties"]["cell_heights_hwp"], expected_heights);
        assert_eq!(val["output"]["table_properties"]["cell_heights_hwp"], expected_heights);
        assert_eq!(comparison_verdict(&val, "table_border_fill_ids"), Some("MATCH"));
        assert_eq!(comparison_verdict(&val, "table_cell_border_fill_ids"), Some("MATCH"));
        assert_eq!(comparison_verdict(&val, "table_cell_heights_hwp"), Some("MATCH"));
        assert_eq!(comparison_verdict(&val, focus_field), Some("MATCH"));
    }
}

#[test]
fn convert_hwp5_table_border_fill_materializes_header_definitions() {
    let source = fixture("table_03_border_fill_variants.hwp");
    let output = test_tmp().join("table_03_border_fill_variants.hwpx");
    hwpforge_smithy_hwp5::hwp5_to_hwpx(&source, &output).expect("convert hwp5 table fixture");

    let header_xml = read_hwpx_entry(&output, "Contents/header.xml");
    let section_xml = read_hwpx_entry(&output, "Contents/section0.xml");
    assert!(
        header_xml.contains(r#"<hh:borderFills itemCnt="7">"#),
        "generated header.xml must materialize custom border fills 4..7"
    );
    assert!(
        header_xml.contains(
            r#"<hh:borderFill id="4" threeD="0" shadow="0" centerLine="NONE" breakCellSeparateLine="0">"#
        ),
        "custom border fill id=4 must exist in header.xml"
    );
    assert!(
        header_xml.contains(r##"<hh:bottomBorder type="SOLID" width="1.0 mm" color="#000000"/>"##),
        "id=4 bottom border width must be emitted"
    );
    assert!(
        header_xml
            .contains(r##"<hc:winBrush faceColor="#CA56A7" hatchColor="#C0FFFFFF" alpha="0"/>"##),
        "custom fill brush must be emitted"
    );
    assert!(
        header_xml
            .contains(r##"<hc:winBrush faceColor="#85BF4C" hatchColor="#C0FFFFFF" alpha="0"/>"##),
        "second custom fill brush must be emitted"
    );
    let defined_ids = extract_u32_attribute_values_after(&header_xml, "<hh:borderFill ", "id");
    let referenced_ids = extract_xml_u32_attribute_values(&section_xml, "borderFillIDRef");
    let missing_ids: Vec<u32> =
        referenced_ids.into_iter().filter(|id| !defined_ids.contains(id)).collect();
    assert!(
        missing_ids.is_empty(),
        "every table/cell borderFillIDRef must have a header.xml definition, missing: {missing_ids:?}"
    );
}

#[test]
fn convert_hwp5_table_border_fill_phase2_materializes_gradient_image_and_diagonal() {
    let cases = [
        (
            "table_15_gradient_fill.hwp",
            Some(
                r#"<hc:gradation type="LINEAR" angle="90" centerX="0" centerY="0" step="255" colorNum="2" stepCenter="50" alpha="0">"#,
            ),
            None,
            None,
            Some(r#"<hh:borderFill id="4""#),
        ),
        (
            "table_16_image_fill.hwp",
            None,
            Some(r#"<hc:imgBrush mode="TOTAL"><hc:img binaryItemIDRef="BIN0001""#),
            Some("BinData/BIN0001.png"),
            Some(r#"<hh:borderFill id="4""#),
        ),
        (
            "table_15b_gradient_fill_radial.hwp",
            Some(
                r#"<hc:gradation type="RADIAL" angle="90" centerX="0" centerY="0" step="255" colorNum="2" stepCenter="50" alpha="0">"#,
            ),
            None,
            None,
            Some(r#"<hh:borderFill id="4""#),
        ),
        (
            "table_16b_image_fill_center.hwp",
            None,
            Some(r#"<hc:imgBrush mode="CENTER"><hc:img binaryItemIDRef="BIN0001""#),
            Some("BinData/BIN0001.png"),
            Some(r#"<hh:borderFill id="4""#),
        ),
        (
            "table_16c_image_fill_tile.hwp",
            None,
            Some(r#"<hc:imgBrush mode="TILE"><hc:img binaryItemIDRef="BIN0001""#),
            Some("BinData/BIN0001.jpg"),
            Some(r#"<hh:borderFill id="4""#),
        ),
        (
            "table_17_diagonal_border.hwp",
            None,
            None,
            None,
            Some(r#"<hh:backSlash type="CENTER" Crooked="0" isCounter="0"/>"#),
        ),
        (
            "table_17b_diagonal_border_variant.hwp",
            None,
            None,
            None,
            Some(r#"<hh:slash type="CENTER" Crooked="0" isCounter="0"/>"#),
        ),
    ];

    let tmp = test_tmp();
    for (
        fixture_name,
        expected_gradation,
        expected_img_brush,
        expected_image_entry,
        expected_xml,
    ) in cases
    {
        let (output, _val) = convert_hwp5_fixture_and_audit_ok(fixture_name, &tmp);

        let header_xml = read_hwpx_entry(&output, "Contents/header.xml");
        let content_hpf = read_hwpx_entry(&output, "Contents/content.hpf");
        if let Some(expected_gradation) = expected_gradation {
            assert!(
                header_xml.contains(expected_gradation),
                "generated header.xml must materialize gradation fill for {fixture_name}"
            );
        }
        if let Some(expected_img_brush) = expected_img_brush {
            assert!(
                header_xml.contains(expected_img_brush),
                "generated header.xml must materialize image fill for {fixture_name}"
            );
        }
        if let Some(expected_xml) = expected_xml {
            assert!(
                header_xml.contains(expected_xml),
                "generated header.xml must preserve expected border/fill evidence for {fixture_name}"
            );
        }
        if let Some(expected_image_entry) = expected_image_entry {
            assert!(
                hwpx_has_entry(&output, expected_image_entry),
                "generated package must include {expected_image_entry} for {fixture_name}"
            );
            assert!(
                content_hpf.contains(&format!(r#"href="{expected_image_entry}""#)),
                "generated content.hpf must list {expected_image_entry} for {fixture_name}"
            );
        }
    }
}

#[test]
fn convert_hwp5_table_public_document_composite_preserves_border_fill_modes() {
    let tmp = test_tmp();
    let (output, _val) =
        convert_hwp5_fixture_and_audit_ok("table_18_public_document_composite.hwp", &tmp);

    let header_xml = read_hwpx_entry(&output, "Contents/header.xml");
    assert!(
        header_xml.contains(
            r#"<hc:gradation type="LINEAR" angle="0" centerX="80" centerY="40" step="255" colorNum="2" stepCenter="50" alpha="0">"#
        ),
        "generated header.xml must preserve the composite gradient fill"
    );
    assert!(
        header_xml.contains(r#"<hc:imgBrush mode="ZOOM"><hc:img binaryItemIDRef="BIN0001""#),
        "generated header.xml must preserve the composite image fill mode"
    );
    assert!(
        header_xml.contains(r#"<hh:slash type="CENTER" Crooked="0" isCounter="0"/>"#),
        "generated header.xml must preserve the composite slash diagonal"
    );
    assert!(
        header_xml.contains(r#"<hh:backSlash type="CENTER" Crooked="0" isCounter="0"/>"#),
        "generated header.xml must preserve the composite backslash diagonal"
    );
    assert!(
        hwpx_has_entry(&output, "BinData/BIN0001.jpg"),
        "generated package must include composite image-fill bindata"
    );
}

#[test]
fn convert_hwp5_table_completion_representatives_hold_acceptance_parity() {
    let cases = [
        (
            "table_19_public_document_multi_page_composite.hwp",
            Some("MATCH"),
            Some("MATCH"),
            Some("MATCH"),
            Some("MATCH"),
        ),
        (
            "table_20_real_world_ministry_style.hwp",
            Some("MATCH"),
            Some("MATCH"),
            Some("MATCH"),
            Some("MATCH"),
        ),
    ];

    let tmp = test_tmp();
    for (
        fixture_name,
        expected_repeat_header,
        expected_header_rows,
        expected_structural_evidence,
        expected_cell_evidence,
    ) in cases
    {
        let (output, val) = convert_hwp5_fixture_and_audit_ok(fixture_name, &tmp);
        assert_eq!(
            comparison_verdict(&val, "table_repeat_header_tables"),
            expected_repeat_header,
            "repeat-header parity for {fixture_name}"
        );
        assert_eq!(
            comparison_verdict(&val, "table_header_rows"),
            expected_header_rows,
            "header-row parity for {fixture_name}"
        );
        assert_eq!(
            comparison_verdict(&val, "table_structural_evidence"),
            expected_structural_evidence,
            "structural evidence parity for {fixture_name}"
        );
        assert_eq!(
            comparison_verdict(&val, "table_cell_evidence"),
            expected_cell_evidence,
            "cell evidence parity for {fixture_name}"
        );

        let (inspect, _, inspect_code) = run_json(&["inspect", "--json", output.to_str().unwrap()]);
        assert_eq!(inspect_code, 0, "inspect exit code for {fixture_name}");
        assert_eq!(inspect["status"], "ok", "inspect status for {fixture_name}");
        assert_eq!(
            inspect["sections"][0]["tables"].as_u64(),
            Some(1),
            "representative fixture {fixture_name} must remain a single top-level table"
        );
    }
}

#[test]
fn convert_hwp5_table_cell_presentation_parity() {
    let cases = [
        (
            "table_04_vertical_align.hwp",
            "table_cell_evidence",
            json!([
                {
                    "section_index": 0,
                    "table_ordinal": 0,
                    "row": 0,
                    "column": 0,
                    "col_span": 1,
                    "row_span": 1,
                    "border_fill_id": 3,
                    "height_hwp": 7697,
                    "width_hwp": 41954,
                    "margin_hwp": { "left": 510, "right": 510, "top": 141, "bottom": 141 },
                    "vertical_align": "top"
                },
                {
                    "section_index": 0,
                    "table_ordinal": 0,
                    "row": 1,
                    "column": 0,
                    "col_span": 1,
                    "row_span": 1,
                    "border_fill_id": 3,
                    "height_hwp": 7697,
                    "width_hwp": 41954,
                    "margin_hwp": { "left": 510, "right": 510, "top": 141, "bottom": 141 },
                    "vertical_align": "center"
                },
                {
                    "section_index": 0,
                    "table_ordinal": 0,
                    "row": 2,
                    "column": 0,
                    "col_span": 1,
                    "row_span": 1,
                    "border_fill_id": 3,
                    "height_hwp": 7697,
                    "width_hwp": 41954,
                    "margin_hwp": { "left": 510, "right": 510, "top": 141, "bottom": 141 },
                    "vertical_align": "bottom"
                }
            ]),
        ),
        (
            "table_05_cell_margin_padding.hwp",
            "table_cell_evidence",
            json!([
                {
                    "section_index": 0,
                    "table_ordinal": 0,
                    "row": 0,
                    "column": 0,
                    "col_span": 1,
                    "row_span": 1,
                    "border_fill_id": 3,
                    "height_hwp": 282,
                    "width_hwp": 20977,
                    "margin_hwp": { "left": 510, "right": 510, "top": 141, "bottom": 141 },
                    "vertical_align": "center"
                },
                {
                    "section_index": 0,
                    "table_ordinal": 0,
                    "row": 0,
                    "column": 1,
                    "col_span": 1,
                    "row_span": 1,
                    "border_fill_id": 3,
                    "height_hwp": 1281,
                    "width_hwp": 20977,
                    "margin_hwp": { "left": 4251, "right": 5669, "top": 2834, "bottom": 1417 },
                    "vertical_align": "center"
                }
            ]),
        ),
    ];

    let tmp = test_tmp();
    for (fixture_name, field, expected_cell_evidence) in cases {
        let source = fixture(fixture_name);
        let output = tmp.join(fixture_name.replace(".hwp", ".hwpx"));
        hwpforge_smithy_hwp5::hwp5_to_hwpx(&source, &output).expect("convert hwp5 table fixture");

        let (val, _, code) =
            run_json(&["audit-hwp5", source.to_str().unwrap(), output.to_str().unwrap()]);
        assert_eq!(code, 0, "audit exit code for {fixture_name}");
        assert_eq!(val["source"]["table_properties"]["cell_evidence"], expected_cell_evidence);
        assert_eq!(val["output"]["table_properties"]["cell_evidence"], expected_cell_evidence);
        assert_eq!(comparison_verdict(&val, field), Some("MATCH"));
    }
}

#[test]
fn convert_hwp5_table_sizing_parity() {
    let cases = [
        ("table_10_row_height_fixed.hwp", "20977", "41954", "4317"),
        ("table_11_row_height_mixed.hwp", "20977", "41954", "850,2834,9354"),
        ("table_12_table_width_explicit.hwp", "6236", "18708", "1281"),
        ("table_13_column_width_variants.hwp", "2947,15116,23889", "41952", "282"),
        ("table_14_wrapped_text_height_growth.hwp", "41954", "41954", "282"),
    ];

    let tmp = test_tmp();
    for (fixture_name, cell_widths, table_widths, row_max_cell_heights) in cases {
        let source = fixture(fixture_name);
        let output = tmp.join(fixture_name.replace(".hwp", ".hwpx"));
        hwpforge_smithy_hwp5::hwp5_to_hwpx(&source, &output).expect("convert hwp5 table fixture");

        let (val, _, code) =
            run_json(&["audit-hwp5", source.to_str().unwrap(), output.to_str().unwrap()]);
        assert_eq!(code, 0, "audit exit code for {fixture_name}");
        assert_eq!(val["status"], "ok", "audit status for {fixture_name}");
        assert_eq!(
            val["source"]["table_properties"]["cell_widths_hwp"],
            csv_to_json_array(cell_widths)
        );
        assert_eq!(
            val["output"]["table_properties"]["cell_widths_hwp"],
            csv_to_json_array(cell_widths)
        );
        assert_eq!(
            val["source"]["table_properties"]["table_widths_hwp"],
            csv_to_json_array(table_widths)
        );
        assert_eq!(
            val["output"]["table_properties"]["table_widths_hwp"],
            csv_to_json_array(table_widths)
        );
        assert_eq!(
            val["source"]["table_properties"]["row_max_cell_heights_hwp"],
            csv_to_json_array(row_max_cell_heights)
        );
        assert_eq!(
            val["output"]["table_properties"]["row_max_cell_heights_hwp"],
            csv_to_json_array(row_max_cell_heights)
        );
        assert_eq!(comparison_verdict(&val, "table_cell_widths_hwp"), Some("MATCH"));
        assert_eq!(comparison_verdict(&val, "table_structural_widths_hwp"), Some("MATCH"));
        assert_eq!(comparison_verdict(&val, "table_row_max_cell_heights_hwp"), Some("MATCH"));
        assert_eq!(comparison_verdict(&val, "table_structural_evidence"), Some("MATCH"));
    }
}

#[test]
fn convert_hwp5_table_nested_table_parity() {
    let source = fixture("table_08_nested_table.hwp");
    let tmp = test_tmp();
    let output = tmp.join("table_08_nested_table.hwpx");
    hwpforge_smithy_hwp5::hwp5_to_hwpx(&source, &output).expect("convert nested table fixture");

    let (audit_val, _, audit_code) =
        run_json(&["audit-hwp5", source.to_str().unwrap(), output.to_str().unwrap()]);
    assert_eq!(audit_code, 0);
    assert_eq!(audit_val["status"], "ok");
    assert_eq!(audit_val["source"]["totals"]["tables"], 2);
    assert_eq!(audit_val["output"]["totals"]["tables"], 2);
    assert_eq!(comparison_verdict(&audit_val, "table_structural_evidence"), Some("MATCH"));
    assert_eq!(comparison_verdict(&audit_val, "table_cell_evidence"), Some("MATCH"));

    let (inspect_val, _, inspect_code) = run_json(&["inspect", output.to_str().unwrap()]);
    assert_eq!(inspect_code, 0);
    let sec0 = &inspect_val["sections"][0];
    assert_eq!(sec0["tables"], 2);
    assert_eq!(sec0["deep_paragraphs"], 11);
    assert_eq!(sec0["deep_non_empty_paragraphs"], 9);
}

#[test]
fn audit_hwp5_rect_fixture_reports_mismatch_and_warning() {
    let source = fixture("rect_simple.hwp");
    let tmp = test_tmp();
    let out = tmp.join("rect_simple.hwpx");
    let warnings =
        hwpforge_smithy_hwp5::hwp5_to_hwpx(&source, &out).expect("convert hwp5 rect fixture");
    assert!(warnings.iter().any(|warning| matches!(
        warning,
        hwpforge_smithy_hwp5::Hwp5Warning::DroppedControl { control, reason }
            if *control == "rect"
                && reason == "pure_rect_projection_requires_core_hwpx_capability"
    )));

    let (val, _, code) = run_json(&["audit-hwp5", source.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "mismatch");
    assert_eq!(val["source"]["warning_count"], 1);
    assert_eq!(val["source"]["totals"]["rectangles"], 1);
    assert_eq!(val["output"]["totals"]["rectangles"], 0);
}

#[test]
fn audit_hwp5_nonexistent_source() {
    let tmp = test_tmp();
    let out = tmp.join("output.hwpx");
    let (_, _, code) = run(&["audit-hwp5", "/nonexistent/file.hwp", out.to_str().unwrap()]);
    assert_eq!(code, 1);
}

#[test]
fn convert_hwp5_fixture() {
    let source = fixture("hwp5_01.hwp");
    let tmp = test_tmp();
    let out = tmp.join("hwp5_01.hwpx");

    let (stdout, _, code) =
        run(&["convert-hwp5", source.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Converted"));
    assert!(stdout.contains("HWP 5."));
    assert!(out.exists());
    assert_valid_hwpx(&out);
}

#[test]
fn convert_hwp5_rect_fixture_reports_projection_warning_count() {
    let source = fixture("rect_simple.hwp");
    let tmp = test_tmp();
    let out = tmp.join("rect_simple.hwpx");

    let (stdout, _, code) =
        run(&["convert-hwp5", source.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Converted"));
    assert!(stdout.contains("1 warnings"));
    assert_valid_hwpx(&out);
}

#[test]
fn convert_hwp5_json_mode() {
    let source = fixture("hwp5_02.hwp");
    let tmp = test_tmp();
    let out = tmp.join("hwp5_02.hwpx");

    let (val, _, code) =
        run_json(&["convert-hwp5", source.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert!(val["version"].as_str().unwrap().starts_with("5."));
    assert!(val["warnings"].is_number());
    assert!(val["size_bytes"].as_u64().unwrap() > 0);
    assert!(out.exists());
}

#[test]
fn convert_hwp5_nonexistent_file() {
    let tmp = test_tmp();
    let out = tmp.join("missing.hwpx");

    let (_, _, code) = run(&["convert-hwp5", "/nonexistent/file.hwp", "-o", out.to_str().unwrap()]);
    assert_eq!(code, 2);
}

#[test]
fn census_hwp5_json_with_companion() {
    let source = fixture("mixed_02b_textbox_with_image_real.hwp");
    let companion = fixture("mixed_02b_textbox_with_image_real.hwpx");

    let (val, _, code) = run_json(&[
        "census-hwp5",
        source.to_str().unwrap(),
        "--companion",
        companion.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert_eq!(val["hwp5"]["sections"][0]["index"], 0);
    assert!(val["companion"]["path_inventory"].as_array().unwrap().iter().any(|entry| entry
        ["path"]
        .as_str()
        .unwrap()
        .contains("/rect/drawText/subList")));
}

#[test]
fn census_hwp5_writes_output_file() {
    let source = fixture("mixed_02a_header_image_footer_text_real.hwp");
    let companion = fixture("mixed_02a_header_image_footer_text_real.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("census.json");
    let canonical_path: &str = "/\\u0005HwpSummaryInformation";

    let (_, _, code) = run(&[
        "census-hwp5",
        source.to_str().unwrap(),
        "--companion",
        companion.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert!(out.exists());

    let content = std::fs::read_to_string(&out).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["status"], "ok");
    assert!(parsed["companion"]["path_inventory"].as_array().unwrap().iter().any(|entry| entry
        ["path"]
        .as_str()
        .unwrap()
        .contains("/header/subList")));
    let package_entries = parsed["hwp5"]["package_entries"].as_array().unwrap();
    assert!(package_entries.iter().any(|entry| entry["path"].as_str() == Some(canonical_path)));
}

#[test]
fn census_hwp5_json_uses_canonical_escaped_paths_across_transports() {
    let source = fixture("chart_01_single_column.hwp");
    let companion = fixture("chart_01_single_column.hwpx");
    let tmp = test_tmp();
    let canonical_path: &str = "/\\u0005HwpSummaryInformation";
    let (direct_json, _, direct_code) = run_json(&[
        "census-hwp5",
        source.to_str().unwrap(),
        "--companion",
        companion.to_str().unwrap(),
    ]);
    assert_eq!(direct_code, 0);
    let direct_package_entries = direct_json["hwp5"]["package_entries"].as_array().unwrap();
    assert!(direct_package_entries
        .iter()
        .any(|entry| entry["path"].as_str() == Some(canonical_path)));

    let file_out = tmp.join("canonical.json");
    let (_, _, file_code) = run(&[
        "census-hwp5",
        source.to_str().unwrap(),
        "--companion",
        companion.to_str().unwrap(),
        "-o",
        file_out.to_str().unwrap(),
    ]);
    assert_eq!(file_code, 0);
    let file_content = std::fs::read_to_string(&file_out).unwrap();
    let file_parsed: serde_json::Value = serde_json::from_str(&file_content).unwrap();
    let file_package_entries = file_parsed["hwp5"]["package_entries"].as_array().unwrap();
    assert!(file_package_entries
        .iter()
        .any(|entry| entry["path"].as_str() == Some(canonical_path)));

    let out = tmp.join("aggregated.json");
    let (json_stdout, aggregated_json, aggregated_stderr, aggregated_code) =
        run_json_with_stdout(&[
            "census-hwp5",
            source.to_str().unwrap(),
            "--companion",
            companion.to_str().unwrap(),
        ]);
    assert_eq!(aggregated_code, 0, "stderr: {aggregated_stderr}");
    std::fs::write(&out, &json_stdout).expect("write aggregated census json");
    assert_eq!(
        aggregated_json["hwp5"]["package_entries"].as_array().unwrap(),
        direct_package_entries
    );

    let content = std::fs::read_to_string(&out).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let package_entries = parsed["hwp5"]["package_entries"].as_array().unwrap();
    assert!(package_entries.iter().any(|entry| entry["path"].as_str() == Some(canonical_path)));
    assert!(package_entries.iter().all(|entry| {
        let path = entry["path"].as_str().unwrap();
        !path.chars().any(char::is_control)
    }));
}

#[test]
fn census_hwp5_dataset_regeneration_preserves_canonical_escaped_paths() {
    let first = fixture("chart_01_single_column.hwp");
    let first_companion = fixture("chart_01_single_column.hwpx");
    let second = fixture("mixed_02a_header_image_footer_text_real.hwp");
    let second_companion = fixture("mixed_02a_header_image_footer_text_real.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("fixture-census.json");
    let canonical_path: &str = "/\\u0005HwpSummaryInformation";
    let (first_stdout, first_json, first_stderr, first_code) = run_json_with_stdout(&[
        "census-hwp5",
        first.to_str().unwrap(),
        "--companion",
        first_companion.to_str().unwrap(),
    ]);
    assert_eq!(first_code, 0, "stderr: {first_stderr}");
    let (second_stdout, second_json, second_stderr, second_code) = run_json_with_stdout(&[
        "census-hwp5",
        second.to_str().unwrap(),
        "--companion",
        second_companion.to_str().unwrap(),
    ]);
    assert_eq!(second_code, 0, "stderr: {second_stderr}");
    std::fs::write(&out, format!("[{first_stdout},{second_stdout}]"))
        .expect("write fixture census json");

    let content = std::fs::read_to_string(&out).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let reports = parsed.as_array().unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0], first_json);
    assert_eq!(reports[1], second_json);
    assert!(reports.iter().all(|report| {
        report["hwp5"]["package_entries"].as_array().unwrap().iter().all(|entry| {
            let path = entry["path"].as_str().unwrap();
            !path.chars().any(char::is_control)
        })
    }));
    assert!(reports.iter().any(|report| {
        report["hwp5"]["package_entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["path"].as_str() == Some(canonical_path))
    }));
}

#[test]
fn census_hwp5_chart_01_reports_ole_backed_chart_evidence() {
    let source = fixture("chart_01_single_column.hwp");
    let companion = fixture("chart_01_single_column.hwpx");

    let (val, _, code) = run_json(&[
        "census-hwp5",
        source.to_str().unwrap(),
        "--companion",
        companion.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert_single_chart_ole_evidence(
        &val,
        "Chart/chart1.xml",
        "BinData/BIN0001.OLE",
        "BinData/ole1.ole",
    );
}

#[test]
fn census_hwp5_chart_02_reports_ole_backed_chart_evidence() {
    let source = fixture("chart_02_single_pie.hwp");
    let companion = fixture("chart_02_single_pie.hwpx");

    let (val, _, code) = run_json(&[
        "census-hwp5",
        source.to_str().unwrap(),
        "--companion",
        companion.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert_single_chart_ole_evidence(
        &val,
        "Chart/chart1.xml",
        "BinData/BIN0001.OLE",
        "BinData/ole1.ole",
    );
}

#[test]
fn census_hwp5_chart_03_reports_ole_backed_chart_evidence() {
    let source = fixture("chart_03_line_or_scatter.hwp");
    let companion = fixture("chart_03_line_or_scatter.hwpx");

    let (val, _, code) = run_json(&[
        "census-hwp5",
        source.to_str().unwrap(),
        "--companion",
        companion.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert_single_chart_ole_evidence(
        &val,
        "Chart/chart1.xml",
        "BinData/BIN0001.OLE",
        "BinData/ole1.ole",
    );
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
fn patch_text_only_edit_preserves_untouched_package_entries() {
    let base = guide_hwpx_path();
    let tmp = test_tmp();
    let json_out = tmp.join("section0.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) = run(&[
        "to-json",
        base.to_str().unwrap(),
        "-o",
        json_out.to_str().unwrap(),
        "--section",
        "0",
    ]);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(&json_out).expect("read exported section");
    let mut exported: ExportedSection =
        serde_json::from_str(&content).expect("deserialize exported section");
    assert!(
        replace_first_table_text_in_section(&mut exported, "[TEST] preserving patch"),
        "expected at least one text run inside a table",
    );
    std::fs::write(
        &json_out,
        serde_json::to_string_pretty(&exported).expect("serialize edited section"),
    )
    .expect("write edited section");

    let (_, _, code) = run(&[
        "patch",
        base.to_str().unwrap(),
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);

    let changed = hwpx_changed_entries(&base, &patched);
    assert_eq!(changed, vec!["Contents/section0.xml".to_string()]);
    assert_eq!(read_hwpx_entry(&base, "version.xml"), read_hwpx_entry(&patched, "version.xml"));
    assert_eq!(
        read_hwpx_entry(&base, "Contents/content.hpf"),
        read_hwpx_entry(&patched, "Contents/content.hpf")
    );
    assert_eq!(read_hwpx_entry(&base, "settings.xml"), read_hwpx_entry(&patched, "settings.xml"));
    assert_eq!(
        read_hwpx_entry(&base, "Contents/header.xml"),
        read_hwpx_entry(&patched, "Contents/header.xml")
    );
}

#[test]
fn patch_noop_preserves_tab_markup_in_plain_paragraph_sample() {
    let base = fixture("user_samples/sample-tab.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section0.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) = run(&[
        "to-json",
        base.to_str().unwrap(),
        "-o",
        json_out.to_str().unwrap(),
        "--section",
        "0",
    ]);
    assert_eq!(code, 0);

    let (_, _, code) = run(&[
        "patch",
        base.to_str().unwrap(),
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);

    let base_section = read_hwpx_entry(&base, "Contents/section0.xml");
    let patched_section = read_hwpx_entry(&patched, "Contents/section0.xml");
    assert!(base_section.contains("<hp:tab "));
    assert_eq!(patched_section, base_section);
}

#[test]
fn patch_noop_preserves_tab_markup_in_table_cell_sample() {
    let base = fixture("user_samples/sample-table-tab.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section0.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) = run(&[
        "to-json",
        base.to_str().unwrap(),
        "-o",
        json_out.to_str().unwrap(),
        "--section",
        "0",
    ]);
    assert_eq!(code, 0);

    let (_, _, code) = run(&[
        "patch",
        base.to_str().unwrap(),
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);

    let base_section = read_hwpx_entry(&base, "Contents/section0.xml");
    let patched_section = read_hwpx_entry(&patched, "Contents/section0.xml");
    assert!(base_section.contains("<hp:tab "));
    assert_eq!(patched_section, base_section);
}

#[test]
fn patch_rejects_editing_plain_paragraph_tab_slot() {
    let base = fixture("user_samples/sample-tab.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section0.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) = run(&[
        "to-json",
        base.to_str().unwrap(),
        "-o",
        json_out.to_str().unwrap(),
        "--section",
        "0",
    ]);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(&json_out).expect("read exported section");
    let mut exported: ExportedSection =
        serde_json::from_str(&content).expect("deserialize exported section");
    assert!(
        replace_first_text_in_section(&mut exported, "LEFT CHANGED RIGHT"),
        "expected at least one text run",
    );
    std::fs::write(
        &json_out,
        serde_json::to_string_pretty(&exported).expect("serialize edited section"),
    )
    .expect("write edited section");

    let (_, stderr, code) = run(&[
        "patch",
        base.to_str().unwrap(),
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_ne!(code, 0);
    assert!(stderr.contains("inline HWPX markup"));
}

#[test]
fn patch_rejects_tampered_preservation_locator_metadata() {
    let base = fixture("user_samples/sample-table-cell.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section0.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) = run(&[
        "to-json",
        base.to_str().unwrap(),
        "-o",
        json_out.to_str().unwrap(),
        "--section",
        "0",
    ]);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(&json_out).expect("read exported section");
    let mut exported: serde_json::Value =
        serde_json::from_str(&content).expect("deserialize exported section as value");
    exported["preservation"]["text_slots"][0]["locator"]["TextElement"]["element_start"] =
        serde_json::json!(0);
    std::fs::write(
        &json_out,
        serde_json::to_string_pretty(&exported).expect("serialize tampered section"),
    )
    .expect("write tampered section");

    let (_, stderr, code) = run(&[
        "patch",
        base.to_str().unwrap(),
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_ne!(code, 0);
    assert!(stderr.contains("stale or tampered preservation metadata"));
}

#[test]
fn patch_rejects_legacy_section_preservation_metadata() {
    let base = fixture("user_samples/sample-table-cell.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("section0.json");
    let patched = tmp.join("patched.hwpx");

    let (_, _, code) = run(&[
        "to-json",
        base.to_str().unwrap(),
        "-o",
        json_out.to_str().unwrap(),
        "--section",
        "0",
    ]);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(&json_out).expect("read exported section");
    let mut exported: serde_json::Value =
        serde_json::from_str(&content).expect("deserialize exported section as value");
    exported["preservation"]
        .as_object_mut()
        .expect("preservation object")
        .remove("preservation_version");
    std::fs::write(
        &json_out,
        serde_json::to_string_pretty(&exported).expect("serialize legacy section"),
    )
    .expect("write legacy section");

    let (_, stderr, code) = run(&[
        "patch",
        base.to_str().unwrap(),
        "--section",
        "0",
        json_out.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_ne!(code, 0);
    assert!(stderr.contains("re-export the section with the current to-json command"));
    assert!(stderr.contains("preservation metadata version"));
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
