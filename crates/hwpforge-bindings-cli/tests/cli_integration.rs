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

fn resolve_workspace_fixture_path(name: &str) -> PathBuf {
    let root = {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop();
        path.pop();
        path.push("tests");
        path.push("fixtures");
        path
    };
    let direct = root.join(name);
    if direct.exists() {
        return direct;
    }

    let file_name = Path::new(name)
        .file_name()
        .unwrap_or_else(|| panic!("fixture name must include a file name: {name}"));

    let mut stack = vec![root.clone()];
    let mut matches = Vec::new();
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).unwrap_or_else(|err| {
            panic!("failed to read fixture directory {}: {err}", dir.display())
        });
        for entry in entries {
            let entry = entry.unwrap_or_else(|err| {
                panic!("failed to read fixture entry under {}: {err}", dir.display())
            });
            let path = entry.path();
            let file_type = entry.file_type().unwrap_or_else(|err| {
                panic!("failed to stat fixture entry {}: {err}", path.display())
            });
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if path.file_name() == Some(file_name) {
                matches.push(path);
            }
        }
    }

    match matches.len() {
        0 => direct,
        1 => matches.pop().expect("one match implies pop succeeds"),
        _ => {
            matches.sort();
            let paths = matches
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            panic!("fixture name is ambiguous: {name} -> [{paths}]");
        }
    }
}

/// Path to any fixture in tests/fixtures/ by name.
fn fixture(name: &str) -> PathBuf {
    let path = resolve_workspace_fixture_path(name);
    assert!(path.exists(), "fixture not found: {}", path.display());
    path
}

/// Path to the curated hwpx_complete_guide showcase artifact in examples/.
fn guide_hwpx_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("examples");
    path.push("showcase");
    path.push("guides");
    path.push("hwpx_complete_guide");
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
            match &mut run.content {
                RunContent::Text(text) => {
                    *text = replacement.to_string();
                    return true;
                }
                // Downgrade-on-modify: same policy as the MCP patch
                // helper. See debug doc §3a-C19.
                RunContent::InlineText(_) => {
                    run.content = RunContent::Text(replacement.to_string());
                    return true;
                }
                _ => {}
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

    let bytes = std::fs::read(path).expect("read hwpx");
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).expect("open hwpx zip");
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("zip entry by index");
        if !(file.name().ends_with(".xml") || file.name().ends_with(".hpf")) {
            continue;
        }

        let mut content = Vec::new();
        file.read_to_end(&mut content).expect("read xml-ish zip entry");
        assert!(
            !content.contains(&0),
            "xml entry {} contains NUL byte in {}",
            file.name(),
            path.display()
        );
    }
}

fn convert_hwp5_fixture_and_audit_ok(
    fixture_name: &str,
    tmp: &Path,
) -> (PathBuf, serde_json::Value) {
    let source = fixture(fixture_name);
    let output = tmp.join(fixture_name.replace(".hwp", ".hwpx"));
    hwpforge_convert::hwp5_to_hwpx(&source, &output)
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
fn to_json_rejects_non_json_output_extension() {
    // to-json must guard its output extension like convert/patch guard .hwpx.
    let source = fixture("hwp5_01.hwp");
    let tmp = test_tmp();
    let hwpx = tmp.join("to_json_guard.hwpx");
    hwpforge_convert::hwp5_to_hwpx(&source, &hwpx).expect("convert hwp5 fixture");
    let bad_out = tmp.join("export.txt");

    let (_, stderr, code) =
        run(&["--json", "to-json", hwpx.to_str().unwrap(), "-o", bad_out.to_str().unwrap()]);
    assert_ne!(code, 0, "non-.json output must be rejected");
    let err: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(err["code"], "INVALID_EXTENSION");
    assert!(!bad_out.exists(), "no file should be written when the extension is rejected");
}

#[test]
fn audit_hwp5_human_report() {
    let source = fixture("hwp5_01.hwp");
    let tmp = test_tmp();
    let out = tmp.join("hwp5_01.hwpx");
    hwpforge_convert::hwp5_to_hwpx(&source, &out).expect("convert hwp5 fixture");

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
    hwpforge_convert::hwp5_to_hwpx(&source, &out).expect("convert hwp5 fixture");

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
    // Wave 4c (chart-as-OLE carry) inverted this test's expectations:
    // the converter now carries the chart through as an HWPX OLE fallback
    // (`<hp:default><hp:ole …>`), so the output side reports an OLE object
    // matching the source. Status flips from `mismatch` (drop) to `ok`
    // (parity).
    let source = fixture("chart_01_single_column.hwp");
    let tmp = test_tmp();
    let out = tmp.join("chart_01.hwpx");
    hwpforge_convert::hwp5_to_hwpx(&source, &out).expect("convert hwp5 chart fixture");

    let (val, _, code) = run_json(&["audit-hwp5", source.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert!(val["source"]["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note.as_str() == Some("ole-backed-gso-evidence: 1")));
    assert_eq!(val["source"]["totals"]["ole_objects"], 1);
    assert_eq!(val["output"]["totals"]["ole_objects"], 1);
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
    hwpforge_convert::hwp5_to_hwpx(&source, &out).expect("convert hwp5 line fixture");

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
        hwpforge_convert::hwp5_to_hwpx(&source, &output).expect("convert hwp5 table fixture");

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
        hwpforge_convert::hwp5_to_hwpx(&source, &output)
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
        hwpforge_convert::hwp5_to_hwpx(&source, &output).expect("convert hwp5 table fixture");

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
    hwpforge_convert::hwp5_to_hwpx(&source, &output).expect("convert hwp5 table fixture");

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
        hwpforge_convert::hwp5_to_hwpx(&source, &output).expect("convert hwp5 table fixture");

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
        hwpforge_convert::hwp5_to_hwpx(&source, &output).expect("convert hwp5 table fixture");

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
    hwpforge_convert::hwp5_to_hwpx(&source, &output).expect("convert nested table fixture");

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
fn audit_hwp5_rect_fixture_now_matches_after_carry() {
    // After Wave 4a's Rect carry (commit 86b99c8), the pure-rect projection
    // no longer emits the DroppedControl{"rect", ..} warning and the
    // converted HWPX preserves the rectangle. Audit therefore reports a
    // clean match instead of the prior mismatch.
    let source = fixture("rect_simple.hwp");
    let tmp = test_tmp();
    let out = tmp.join("rect_simple.hwpx");
    let warnings =
        hwpforge_convert::hwp5_to_hwpx(&source, &out).expect("convert hwp5 rect fixture");
    assert!(
        !warnings.iter().any(|warning| matches!(
            warning,
            hwpforge_smithy_hwp5::Hwp5Warning::DroppedControl { control, .. }
                if *control == "rect"
        )),
        "Wave 4a Rect carry should suppress the DroppedControl{{\"rect\", ..}} warning"
    );

    let (val, _, code) = run_json(&["audit-hwp5", source.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert_eq!(val["source"]["warning_count"], 0);
    assert_eq!(val["source"]["totals"]["rectangles"], 1);
    assert_eq!(val["output"]["totals"]["rectangles"], 1);
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
fn convert_hwp5_rect_fixture_reports_no_warnings_after_carry() {
    // After Wave 4a's Rect carry, converting the rect fixture no longer
    // emits a projection warning. The CLI must therefore report
    // "0 warnings" — anything else is a regression.
    let source = fixture("rect_simple.hwp");
    let tmp = test_tmp();
    let out = tmp.join("rect_simple.hwpx");

    let (stdout, _, code) =
        run(&["convert-hwp5", source.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Converted"));
    assert!(stdout.contains("0 warnings"));
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
fn patch_matching_section_index_has_no_warning() {
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
fn patch_section_index_mismatch_fails_fast() {
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
    assert_ne!(code, 0);
    assert!(stderr.contains("SECTION_INDEX_MISMATCH"), "Expected mismatch error, got: {stderr}");
    assert!(stderr.contains("Use --section 5"), "Expected corrective hint, got: {stderr}");
    assert!(!patched.exists(), "patch output should not be written on mismatch");
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
    let content = std::fs::read_to_string(&json_out).unwrap();
    let modified = content.replacen("\"section_index\": 0", "\"section_index\": 999", 1);
    std::fs::write(&json_out, modified).unwrap();

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
    let content = std::fs::read_to_string(&json_out).unwrap();
    let modified = content.replacen("\"section_index\": 0", "\"section_index\": 999", 1);
    std::fs::write(&json_out, modified).unwrap();

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
    let err: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
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
    let base = fixture("user_samples/tabs/sample-tab.hwpx");
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
    let base = fixture("user_samples/tabs/sample-table-tab.hwpx");
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
    let base = fixture("user_samples/tabs/sample-tab.hwpx");
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
    let base = fixture("user_samples/tables/sample-table-cell.hwpx");
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
    let base = fixture("user_samples/tables/sample-table-cell.hwpx");
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
fn patch_json_mismatch_errors() {
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
    assert_ne!(code, 0);
    let err: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(err["status"], "error");
    assert_eq!(err["code"], "SECTION_INDEX_MISMATCH");
    assert!(err["hint"].as_str().unwrap().contains("Use --section 5"));
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

// ═══════════════════════════════════════════════════════════════
// fields / fill — E2 누름틀 델타 API 게이트
// ═══════════════════════════════════════════════════════════════

#[test]
fn fields_lists_named_clickhere_with_fillability() {
    let f = fixture("clickhere_named.hwpx");
    let (value, _, code) = run_json(&["fields", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["fields"][0]["name"], "user_email");
    assert_eq!(value["fields"][0]["fillable"], true);
}

#[test]
fn outline_reports_navigation_map_for_table_fixture() {
    let f = fixture("table_01_basic_2x2.hwpx");
    let (value, _, code) = run_json(&["outline", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    let outline = &value["outline"];
    assert_eq!(outline["sections"][0]["tables"], 1);
    assert_eq!(outline["tables"][0]["ordinal"], 0);
    assert_eq!(outline["tables"][0]["rows"], 2);
    assert_eq!(outline["tables"][0]["cols"], 2);
    assert_eq!(outline["tables"][0]["addressable"], true);
}

#[test]
fn outline_exposes_named_fields_axis() {
    let f = fixture("clickhere_named.hwpx");
    let (value, _, code) = run_json(&["outline", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["outline"]["fields"][0]["name"], "user_email");
}

#[test]
fn read_table_grid_matrix_via_cli() {
    let f = fixture("table_01_basic_2x2.hwpx");
    let (value, _, code) = run_json(&["read", f.to_str().unwrap(), "--table", "0"]);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["table"]["rows"], 2);
    assert_eq!(value["table"]["cols"], 2);
    assert_eq!(value["table"]["cells"].as_array().unwrap().len(), 4);
}

#[test]
fn read_paragraph_range_and_field_via_cli() {
    let f = fixture("clickhere_named.hwpx");
    let (value, _, code) = run_json(&["read", f.to_str().unwrap(), "--field", "user_email"]);
    assert_eq!(code, 0);
    assert_eq!(value["fields"][0]["name"], "user_email");

    let (v2, _, c2) = run_json(&["read", f.to_str().unwrap(), "--section", "0", "--paras", "0"]);
    assert_eq!(c2, 0);
    assert_eq!(v2["paragraphs"]["from"], 0);
    assert_eq!(v2["paragraphs"]["to"], 0);
    assert_eq!(v2["paragraphs"]["paragraphs"].as_array().unwrap().len(), 1);
}

#[test]
fn read_rejects_conflicting_targets() {
    let f = fixture("clickhere_named.hwpx");
    let (err, _, code) = run_json(&["read", f.to_str().unwrap(), "--table", "0", "--field", "x"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "READ_TARGET_REQUIRED");
}

#[test]
fn diff_self_is_identical_via_cli() {
    let f = fixture("table_01_basic_2x2.hwpx");
    let (value, _, code) = run_json(&["diff", f.to_str().unwrap(), f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["diff"]["identical"], true);
}

#[test]
fn diff_verifies_fill_delta_end_to_end() {
    let f = fixture("clickhere_named.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("filled.hwpx");
    let (_, _, fill_code) = run_json(&[
        "fill",
        f.to_str().unwrap(),
        "--set",
        "user_email=diff@cli.io",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(fill_code, 0);

    let report = tmp.join("report.json");
    let (value, _, code) = run_json(&[
        "diff",
        f.to_str().unwrap(),
        out.to_str().unwrap(),
        "-o",
        report.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    let semantic = &value["diff"]["semantic"];
    assert_eq!(semantic["field_values"][0]["name"], "user_email");
    assert_eq!(semantic["field_values"][0]["after"], "diff@cli.io");
    assert!(semantic["paragraphs"].as_array().unwrap().is_empty());
    assert!(semantic["raw"].as_array().unwrap().is_empty());
    // Full report file written alongside inline output.
    let report_value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(report_value["identical"], false);
}

#[test]
fn outline_text_mode_renders_navigation_sections() {
    let tables = fixture("table_01_basic_2x2.hwpx");
    let (stdout, _, code) = run(&["outline", tables.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Tables:"), "stdout: {stdout}");
    assert!(stdout.contains("[0] 2x2"), "stdout: {stdout}");

    let fields = fixture("clickhere_named.hwpx");
    let (stdout, _, code) = run(&["outline", fields.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Fields:"), "stdout: {stdout}");
    assert!(stdout.contains("user_email"), "stdout: {stdout}");
}

#[test]
fn read_text_mode_renders_all_three_targets() {
    let f = fixture("clickhere_named.hwpx");
    let (stdout, _, code) = run(&["read", f.to_str().unwrap(), "--section", "0"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("[p0]"), "stdout: {stdout}");
    assert!(stdout.contains("control:field"), "stdout: {stdout}");

    let t = fixture("tables/merged_grid_form.hwpx");
    let (stdout, _, code) = run(&["read", t.to_str().unwrap(), "--table", "0"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("table 0 ("), "stdout: {stdout}");
    assert!(stdout.contains("[0,0"), "stdout: {stdout}");

    let (stdout, _, code) = run(&["read", f.to_str().unwrap(), "--field", "user_email"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("user_email"), "stdout: {stdout}");
}

#[test]
fn read_error_paths_report_stable_codes() {
    let f = fixture("clickhere_named.hwpx");

    let (err, _, code) =
        run_json(&["read", f.to_str().unwrap(), "--section", "0", "--paras", "abc"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "READ_PARAS_INVALID");

    let (err, _, code) = run_json(&["read", f.to_str().unwrap(), "--section", "9"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "READ_SECTION_OUT_OF_RANGE");

    let (err, _, code) = run_json(&["read", f.to_str().unwrap(), "--field", "없는이름"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "READ_FIELD_NOT_FOUND");

    let (err, _, code) = run_json(&["read", f.to_str().unwrap(), "--table", "42"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "READ_TABLE_OUT_OF_RANGE");

    let (err, _, code) = run_json(&["read", f.to_str().unwrap(), "--paras", "0"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "READ_TARGET_REQUIRED");

    let (err, _, code) = run_json(&["read", f.to_str().unwrap(), "--table", "0", "--paras", "0"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "READ_PARAS_WITHOUT_SECTION");

    // Text-mode error rendering path.
    let (_, stderr, code) = run(&["read", f.to_str().unwrap(), "--field", "없는이름"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("READ_FIELD_NOT_FOUND"), "stderr: {stderr}");
}

#[test]
fn diff_text_mode_renders_identical_and_delta() {
    let f = fixture("clickhere_named.hwpx");
    let (stdout, _, code) = run(&["diff", f.to_str().unwrap(), f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stdout.contains("identical"), "stdout: {stdout}");

    let tmp = test_tmp();
    let out = tmp.join("filled.hwpx");
    let (_, _, fill_code) = run_json(&[
        "fill",
        f.to_str().unwrap(),
        "--set",
        "user_email=text@mode.io",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(fill_code, 0);

    let (stdout, _, code) = run(&["diff", f.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Fields:"), "stdout: {stdout}");
    assert!(stdout.contains("user_email"), "stdout: {stdout}");
    assert!(stdout.contains("Package entries:"), "stdout: {stdout}");
    assert!(stdout.contains("Note:"), "stdout: {stdout}");

    let (err, _, code) = run_json(&["diff", f.to_str().unwrap(), "/nonexistent/x.hwpx"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "FILE_READ_FAILED");
}

#[test]
fn diff_text_mode_renders_cell_and_paragraph_changes() {
    let base = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();

    // Self-adapting target: first empty anchor cell of table 0 via `read`.
    let (view, _, read_code) = run_json(&["read", base.to_str().unwrap(), "--table", "0"]);
    assert_eq!(read_code, 0);
    let empty = view["table"]["cells"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["text"].as_str().unwrap_or("x").trim().is_empty())
        .expect("empty cell in fixture");
    let at = format!("{},{}", empty["row"], empty["col"]);

    let out = tmp.join("cell.hwpx");
    let (_, _, code) = run_json(&[
        "set-cell",
        base.to_str().unwrap(),
        "--table",
        "0",
        "--at",
        &at,
        "--text",
        "텍스트모드",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);

    let (stdout, _, code) = run(&["diff", base.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Cells:"), "stdout: {stdout}");
    assert!(stdout.contains("텍스트모드"), "stdout: {stdout}");
}

#[test]
fn outline_and_read_text_mode_render_headings_and_lists() {
    let tmp = test_tmp();
    let md = create_test_md(
        &tmp,
        "# 사업 개요\n\n본문\n\n## 세부 목표\n\n- 항목 하나\n\n1. 번호 항목\n\n- [x] 완료 항목\n\n- [ ] 미완 항목\n",
    );
    let doc = tmp.join("outline_probe.hwpx");
    let (_, _, code) = run(&["convert", md.to_str().unwrap(), "-o", doc.to_str().unwrap()]);
    assert_eq!(code, 0);

    let (stdout, _, code) = run(&["outline", doc.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Headings:"), "stdout: {stdout}");
    assert!(stdout.contains("# 사업 개요"), "stdout: {stdout}");
    assert!(stdout.contains("## 세부 목표"), "stdout: {stdout}");

    let (stdout, _, code) = run(&["read", doc.to_str().unwrap(), "--section", "0"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("# 사업 개요"), "stdout: {stdout}");
    assert!(stdout.contains("- 항목 하나"), "stdout: {stdout}");
    assert!(stdout.contains("1. 번호 항목"), "stdout: {stdout}");
    assert!(stdout.contains("- [x] 완료 항목"), "stdout: {stdout}");
    assert!(stdout.contains("- [ ] 미완 항목"), "stdout: {stdout}");
}

#[test]
fn diff_text_mode_renders_paragraph_and_structure_changes() {
    let tmp = test_tmp();
    let md_a = create_test_md(&tmp, "# 제목\n\n본문 하나\n");
    let a = tmp.join("a.hwpx");
    let (_, _, code) = run(&["convert", md_a.to_str().unwrap(), "-o", a.to_str().unwrap()]);
    assert_eq!(code, 0);

    let md_b = tmp.join("b.md");
    std::fs::write(&md_b, "# 제목\n\n본문 둘\n\n추가 문단\n").unwrap();
    let b = tmp.join("b.hwpx");
    let (_, _, code) = run(&["convert", md_b.to_str().unwrap(), "-o", b.to_str().unwrap()]);
    assert_eq!(code, 0);

    let (stdout, _, code) = run(&["diff", a.to_str().unwrap(), b.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Paragraphs:"), "stdout: {stdout}");
    assert!(stdout.contains("Structure:"), "stdout: {stdout}");
    assert!(stdout.contains("Note:"), "stdout: {stdout}");
}

#[test]
fn insert_para_adds_paragraph_and_diff_confirms_delta() {
    let f = fixture("plain_paragraphs.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("inserted.hwpx");
    let (value, _, code) = run_json(&[
        "insert-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--anchor",
        "1",
        "--text",
        "삽입된 문단",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");

    // The E5 diff verifies exactly one paragraph was added.
    let (d, _, dc) = run_json(&["diff", f.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(dc, 0);
    let added: Vec<_> = d["diff"]["semantic"]["paragraphs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["kind"] == "added")
        .collect();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0]["after"], "삽입된 문단");
}

#[test]
fn insert_para_batch_adds_block_and_diff_confirms_delta() {
    // Repeated --text inserts a contiguous block in one verified edit; the
    // E5 diff must report exactly the declared block, in order.
    let f = fixture("plain_paragraphs.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("inserted-batch.hwpx");
    let (value, _, code) = run_json(&[
        "insert-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--anchor",
        "1",
        "--text",
        "블록 하나",
        "--text",
        "블록 둘",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["inserted"], 2);

    let (d, _, dc) = run_json(&["diff", f.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(dc, 0);
    let added: Vec<_> = d["diff"]["semantic"]["paragraphs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["kind"] == "added")
        .collect();
    assert_eq!(added.len(), 2);
    assert_eq!(added[0]["after"], "블록 하나");
    assert_eq!(added[1]["after"], "블록 둘");
}

#[test]
fn delete_para_removes_paragraph_and_rejects_section_properties() {
    let f = fixture("plain_paragraphs.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("deleted.hwpx");
    let (value, _, code) = run_json(&[
        "delete-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--index",
        "2",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert_eq!(value["deleted"], 1);
    // Plain paragraphs produce no advisories; the field is always present.
    assert_eq!(value["warnings"].as_array().map(Vec::len), Some(0));

    // Deleting paragraph 0 (secPr carrier) is refused.
    let (err, _, ec) = run_json(&[
        "delete-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--index",
        "0",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(ec, 1);
    assert_eq!(err["code"], "SECTION_PROPERTIES_PARAGRAPH");
}

#[test]
fn structural_edit_text_mode_and_error_codes() {
    let f = fixture("plain_paragraphs.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("o.hwpx");

    // Text-mode success prints "Wrote ...".
    let (stdout, _, code) = run(&[
        "insert-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--anchor",
        "1",
        "--text",
        "텍스트모드",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Wrote"), "stdout: {stdout}");

    // delete with no --index.
    let (err, _, c) = run_json(&[
        "delete-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(c, 1);
    assert_eq!(err["code"], "DELETE_NO_TARGET");

    // multiline insert text.
    let (err, _, c) = run_json(&[
        "insert-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--anchor",
        "1",
        "--text",
        "줄1\n줄2",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(c, 1);
    assert_eq!(err["code"], "MULTI_PARAGRAPH_TEXT");
}

#[test]
fn structural_edit_reference_and_roundtrip_rejections() {
    let tmp = test_tmp();
    let out = tmp.join("o.hwpx");

    // Reference-bearing paragraph (footnote at index 0 of the crossref fixture).
    let cr = fixture("crossref_para.hwpx");
    let (err, _, c) = run_json(&[
        "delete-para",
        cr.to_str().unwrap(),
        "--section",
        "0",
        "--index",
        "0",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(c, 1);
    assert_eq!(err["code"], "REFERENCE_STRANDED");

    // Hard page break paragraph.
    let pb = fixture("page_break.hwpx");
    let (err, _, c) = run_json(&[
        "delete-para",
        pb.to_str().unwrap(),
        "--section",
        "0",
        "--index",
        "1",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(c, 1);
    assert_eq!(err["code"], "HARD_BREAK_LOSS");

    // Hancom-authored input that is not round-trip-safe.
    let pi = fixture("plain_inserted.hwpx");
    let (err, _, c) = run_json(&[
        "delete-para",
        pi.to_str().unwrap(),
        "--section",
        "0",
        "--index",
        "1",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(c, 1);
    assert_eq!(err["code"], "INPUT_NOT_ROUNDTRIP_SAFE");
}

#[test]
fn structural_edit_batch_and_structural_rejections() {
    let f = fixture("plain_paragraphs.hwpx"); // 4 paragraphs, index 0 = secPr
    let tmp = test_tmp();
    let out = tmp.join("o.hwpx");

    // Deleting every paragraph empties the section (this check precedes the
    // secPr guard, so it reports EMPTY_SECTION even though index 0 is secPr).
    let (err, _, c) = run_json(&[
        "delete-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--index",
        "0",
        "--index",
        "1",
        "--index",
        "2",
        "--index",
        "3",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(c, 1);
    assert_eq!(err["code"], "EMPTY_SECTION");

    // Duplicate target.
    let (err, _, c) = run_json(&[
        "delete-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--index",
        "1",
        "--index",
        "1",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(c, 1);
    assert_eq!(err["code"], "DUPLICATE_TARGET");

    // Insert-before the secPr carrier.
    let (err, _, c) = run_json(&[
        "insert-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--anchor",
        "0",
        "--before",
        "--text",
        "맨앞",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(c, 1);
    assert_eq!(err["code"], "INSERT_BEFORE_SECTION_PROPERTIES");

    // Insert JSON output shape (before an anchor).
    let (v, _, c) = run_json(&[
        "insert-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--anchor",
        "2",
        "--before",
        "--text",
        "앞삽입",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(c, 0);
    assert_eq!(v["position"], "before");
    assert_eq!(v["anchor"], 2);

    // delete-para text-mode success prints "Wrote ...".
    let (stdout, _, c) = run(&[
        "delete-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--index",
        "2",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(c, 0);
    assert!(stdout.contains("Wrote"), "stdout: {stdout}");

    // Non-HWPX input → codec error, exit code 2.
    let garbage = tmp.join("g.hwpx");
    std::fs::write(&garbage, b"not a zip").unwrap();
    let (err, _, c) = run_json(&[
        "delete-para",
        garbage.to_str().unwrap(),
        "--section",
        "0",
        "--index",
        "0",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(c, 2);
    assert_eq!(err["code"], "STRUCTURAL_CODEC");
}

#[test]
fn structural_edit_write_failure_and_batch_json_shape() {
    let f = fixture("plain_paragraphs.hwpx");
    let tmp = test_tmp();

    // Unwritable output path → FILE_WRITE_FAILED, exit code 2.
    let bad = "/nonexistent-dir-e4/out.hwpx";
    let (err, _, c) = run_json(&[
        "insert-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--anchor",
        "1",
        "--text",
        "x",
        "-o",
        bad,
    ]);
    assert_eq!(c, 2);
    assert_eq!(err["code"], "FILE_WRITE_FAILED");

    let (err, _, c) = run_json(&[
        "delete-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--index",
        "1",
        "-o",
        bad,
    ]);
    assert_eq!(c, 2);
    assert_eq!(err["code"], "FILE_WRITE_FAILED");

    // Batch delete success JSON carries the requested indices.
    let out = tmp.join("o.hwpx");
    let (v, _, c) = run_json(&[
        "delete-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--index",
        "1",
        "--index",
        "3",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(c, 0);
    assert_eq!(v["deleted"], 2);
    assert_eq!(v["indices"], serde_json::json!([1, 3]));
}

#[test]
fn fill_named_field_end_to_end() {
    let f = fixture("clickhere_named.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("filled.hwpx");
    let (value, _, code) = run_json(&[
        "fill",
        f.to_str().unwrap(),
        "--set",
        "user_email=e2e@gate.io",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert_eq!(value["filled"][0]["name"], "user_email");

    // 채워진 값이 재조회에서 살아있어야 한다.
    let (again, _, code2) = run_json(&["fields", out.to_str().unwrap()]);
    assert_eq!(code2, 0);
    assert_eq!(again["fields"][0]["current"], "e2e@gate.io");
}

#[test]
fn fill_unknown_name_reports_available_fields() {
    let f = fixture("clickhere_named.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("never.hwpx");
    let (value, _, code) = run_json(&[
        "fill",
        f.to_str().unwrap(),
        "--set",
        "없는필드=x",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "FIELD_NOT_FOUND");
    assert!(
        value["hint"].as_str().unwrap_or("").contains("user_email"),
        "hint 에 사용 가능한 필드 목록이 있어야 한다: {value}"
    );
    assert!(!out.exists(), "preflight 실패 시 산출물이 없어야 한다 (all-or-nothing)");
}

#[test]
fn fill_merged_run_field_rejected_as_not_fillable() {
    let f = fixture("clickhere_filled.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("never2.hwpx");
    let (value, _, code) = run_json(&[
        "fill",
        f.to_str().unwrap(),
        "--set",
        "user_email=x@y.z",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "FIELD_NOT_FILLABLE");
    assert!(!out.exists());
}

// ═══════════════════════════════════════════════════════════════
// stamp-plan / stamp — E6 템플릿 스탬핑 게이트
// ═══════════════════════════════════════════════════════════════

/// stamp-plan candidates 에서 시험용 spec 맵을 만든다: 무가드 후보는
/// 순번 이름으로 승격, 가드 후보는 ignore.
fn stamp_map_from_plan(plan: &serde_json::Value, skip_first: bool) -> serde_json::Value {
    let mut specs = Vec::new();
    let mut n = 0usize;
    for (idx, c) in plan["candidates"].as_array().unwrap().iter().enumerate() {
        if skip_first && idx == 0 {
            continue;
        }
        // 가드 후보는 맵에서 제외 — 스펙 없는 가드 후보의 skip 경로 검증.
        if !c["guard"].is_null() {
            continue;
        }
        n += 1;
        let action = serde_json::json!({"field": {"name": format!("게이트필드{n}"), "hint": null}});
        specs.push(serde_json::json!({
            "section": c["section"],
            "path": c["path"],
            "span": c["span"],
            "marker": c["marker"],
            "action": action,
        }));
    }
    serde_json::Value::Array(specs)
}

#[test]
fn stamp_plan_lists_candidates_with_guards() {
    let f = fixture("stamp/placeholder_basic.hwpx");
    let (value, _, code) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    let candidates = value["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 3, "paren blank + cell checkbox + guarded checkbox: {value}");
    let guarded: Vec<_> = candidates.iter().filter(|c| !c["guard"].is_null()).collect();
    assert_eq!(guarded.len(), 1, "※ 안내문의 □ 만 가드되어야 한다");
    assert_eq!(guarded[0]["marker"], "□");
}

#[test]
fn stamp_end_to_end_then_fillable() {
    let f = fixture("stamp/placeholder_basic.hwpx");
    let tmp = test_tmp();
    let (plan, _, code) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code, 0);

    let map = tmp.join("map.json");
    std::fs::write(&map, stamp_map_from_plan(&plan, false).to_string()).unwrap();
    let out = tmp.join("stamped.hwpx");
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "stamp must succeed: {value}");
    assert_eq!(value["stamped"].as_array().unwrap().len(), 2);
    assert_eq!(value["skipped_guarded"], 1);
    let manifest_path = value["manifest"].as_str().unwrap().to_string();
    assert!(std::path::Path::new(&manifest_path).exists(), "manifest must be written");

    // 스탬프 산출물은 즉시 fields/fill 로 소비 가능해야 한다.
    let (fields, _, code) = run_json(&["fields", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(fields["fields"].as_array().unwrap().len(), 2);

    let filled = tmp.join("filled.hwpx");
    let (fill, _, code) = run_json(&[
        "fill",
        out.to_str().unwrap(),
        "--set",
        "게이트필드1=홍길동",
        "-o",
        filled.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "fill on stamped output must succeed: {fill}");
}

#[test]
fn stamp_uncovered_candidate_rejected_all_or_nothing() {
    let f = fixture("stamp/placeholder_basic.hwpx");
    let tmp = test_tmp();
    let (plan, _, _) = run_json(&["stamp-plan", f.to_str().unwrap()]);

    // 첫 후보를 맵에서 누락 → preflight 거부, 산출물 없음
    let map = tmp.join("partial-map.json");
    std::fs::write(&map, stamp_map_from_plan(&plan, true).to_string()).unwrap();
    let out = tmp.join("never-stamped.hwpx");
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "STAMP_CANDIDATE_UNCOVERED");
    assert!(!out.exists(), "preflight 실패 시 산출물이 없어야 한다 (fail-closed)");
}

#[test]
fn stamp_error_codes_for_bad_maps() {
    let f = fixture("stamp/placeholder_basic.hwpx");
    let tmp = test_tmp();
    let (plan, _, _) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    let out = tmp.join("never3.hwpx");
    let run_map = |specs: &serde_json::Value| {
        let map = tmp.join("bad-map.json");
        std::fs::write(&map, specs.to_string()).unwrap();
        run_json(&[
            "stamp",
            f.to_str().unwrap(),
            "--map",
            map.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
    };
    let base = stamp_map_from_plan(&plan, false);

    // stale span → STAMP_SPEC_STALE
    let mut stale = base.clone();
    stale[0]["span"] = serde_json::json!({"start": 0, "end": 1});
    let (value, _, code) = run_map(&stale);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "STAMP_SPEC_STALE");

    // marker 불일치 → STAMP_MARKER_MISMATCH
    let mut mismatch = base.clone();
    mismatch[0]["marker"] = serde_json::json!("(다름)");
    let (value, _, code) = run_map(&mismatch);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "STAMP_MARKER_MISMATCH");

    // 이름 중복 → STAMP_NAME_DUPLICATE
    let mut dup = base.clone();
    dup[0]["action"] = serde_json::json!({"field": {"name": "같음", "hint": null}});
    dup[1]["action"] = serde_json::json!({"field": {"name": "같음", "hint": null}});
    let (value, _, code) = run_map(&dup);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "STAMP_NAME_DUPLICATE");

    // 깨진 맵 JSON → INVALID_STAMP_MAP
    let map = tmp.join("broken-map.json");
    std::fs::write(&map, "{ not json").unwrap();
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_STAMP_MAP");

    assert!(!out.exists(), "모든 거부에서 산출물이 없어야 한다 (fail-closed)");
}

#[test]
fn stamp_manifest_write_failure_removes_output() {
    // Review L1: manifest 기록 실패 시 .hwpx 산출물도 남기지 않아야 한다.
    let f = fixture("stamp/placeholder_basic.hwpx");
    let tmp = test_tmp();
    let (plan, _, _) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    let map = tmp.join("map.json");
    std::fs::write(&map, stamp_map_from_plan(&plan, false).to_string()).unwrap();
    let out = tmp.join("orphan.hwpx");
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--manifest",
        "/nonexistent-dir/never.manifest.json",
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "FILE_WRITE_FAILED");
    assert!(!out.exists(), "manifest 실패 시 산출물이 제거되어야 한다 (fail-closed)");
}

// ═══════════════════════════════════════════════════════════════
// E3 Wave 2: 표 격자 주소 (grid addresses on JSON exports)
// ═══════════════════════════════════════════════════════════════

/// (paragraph, run) indices of the first table run in a document export.
fn first_table_run_indices(root: &serde_json::Value) -> (usize, usize) {
    let paragraphs = root["document"]["sections"][0]["paragraphs"].as_array().expect("paragraphs");
    for (pi, paragraph) in paragraphs.iter().enumerate() {
        for (ri, run) in paragraph["runs"].as_array().expect("runs").iter().enumerate() {
            if run["content"]["Table"].is_object() {
                return (pi, ri);
            }
        }
    }
    panic!("no table run in export");
}

#[test]
fn to_json_annotates_cell_grid_addresses() {
    let f = fixture("tables/table_02_merge_col_row.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("grid_addr.json");
    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json_out).unwrap()).unwrap();
    let (pi, ri) = first_table_run_indices(&parsed);
    let table =
        &parsed["document"]["sections"][0]["paragraphs"][pi]["runs"][ri]["content"]["Table"];
    let rows = table["rows"].as_array().expect("rows");
    assert!(!rows.is_empty());
    for row in rows {
        for cell in row["cells"].as_array().expect("cells") {
            let addr = cell.get("addr").expect("cell addr annotated");
            assert!(addr["row"].is_u64() && addr["col"].is_u64(), "addr shape: {addr}");
        }
    }
    assert_eq!(rows[0]["cells"][0]["addr"], serde_json::json!({"row": 0, "col": 0}));
}

#[test]
fn from_json_accepts_annotated_export_and_rejects_tampered_addr() {
    let f = fixture("tables/table_02_merge_col_row.hwpx");
    let tmp = test_tmp();
    let json_out = tmp.join("grid_addr_roundtrip.json");
    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    // Annotated export must import as-is (validate-then-drop).
    let hwpx_out = tmp.join("grid_addr_roundtrip.hwpx");
    let (_, stderr, code) =
        run(&["from-json", json_out.to_str().unwrap(), "-o", hwpx_out.to_str().unwrap()]);
    assert_eq!(code, 0, "annotated export must round-trip: {stderr}");
    assert!(hwpx_out.exists());

    // Tampered address → GRID_ADDR_INVALID, no output.
    let mut parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json_out).unwrap()).unwrap();
    let (pi, ri) = first_table_run_indices(&parsed);
    parsed["document"]["sections"][0]["paragraphs"][pi]["runs"][ri]["content"]["Table"]["rows"]
        [0]["cells"][0]["addr"] = serde_json::json!({"row": 0, "col": 99});
    let tampered = tmp.join("grid_addr_tampered.json");
    std::fs::write(&tampered, parsed.to_string()).unwrap();
    let bad_out = tmp.join("grid_addr_tampered.hwpx");
    let (value, _, code) =
        run_json(&["from-json", tampered.to_str().unwrap(), "-o", bad_out.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(value["code"], "GRID_ADDR_INVALID");
    assert!(!bad_out.exists(), "rejected import must not produce output");

    // Absence = no check: stripping every addr must import cleanly.
    fn strip_addr(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("addr");
                for child in map.values_mut() {
                    strip_addr(child);
                }
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(strip_addr),
            _ => {}
        }
    }
    strip_addr(&mut parsed);
    let stripped = tmp.join("grid_addr_stripped.json");
    std::fs::write(&stripped, parsed.to_string()).unwrap();
    let clean_out = tmp.join("grid_addr_stripped.hwpx");
    let (_, stderr, code) =
        run(&["from-json", stripped.to_str().unwrap(), "-o", clean_out.to_str().unwrap()]);
    assert_eq!(code, 0, "addr-free import must succeed: {stderr}");
}

// ═══════════════════════════════════════════════════════════════
// E3 Wave 3: set-cell (격자 주소 셀 편집)
// ═══════════════════════════════════════════════════════════════

#[test]
fn set_cell_edits_anchor_and_resolves_covered_coordinate() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();

    // export 로 첫 표의 병합 앵커(row_span > 1)를 찾는다.
    let json_out = tmp.join("grid.json");
    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);
    let parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json_out).unwrap()).unwrap();
    let (pi, ri) = first_table_run_indices(&parsed);
    let table =
        &parsed["document"]["sections"][0]["paragraphs"][pi]["runs"][ri]["content"]["Table"];
    let merged = table["rows"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|row| row["cells"].as_array().unwrap())
        .find(|cell| cell["row_span"].as_u64().unwrap_or(1) > 1)
        .expect("merge fixture must contain a row-span anchor");
    let (arow, acol) =
        (merged["addr"]["row"].as_u64().unwrap(), merged["addr"]["col"].as_u64().unwrap());

    // 피병합 좌표(앵커 바로 아래)를 지정 → 앵커로 resolve 되어야 한다.
    let covered = format!("{},{}", arow + 1, acol);
    let out = tmp.join("edited.hwpx");
    let (value, _, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--table",
        "0",
        "--at",
        &covered,
        "--text",
        "격자값",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["cells"][0]["resolution"], "covered_to_anchor");
    assert_eq!(value["cells"][0]["anchor"], serde_json::json!({"row": arow, "col": acol}));

    // 편집 결과 검증: 재-export 에서 앵커 셀 텍스트가 바뀌었는지.
    let verify_json = tmp.join("verify.json");
    let (_, _, code) =
        run(&["to-json", out.to_str().unwrap(), "-o", verify_json.to_str().unwrap()]);
    assert_eq!(code, 0);
    let verify: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&verify_json).unwrap()).unwrap();
    let (vpi, vri) = first_table_run_indices(&verify);
    let vtable =
        &verify["document"]["sections"][0]["paragraphs"][vpi]["runs"][vri]["content"]["Table"];
    let edited = vtable["rows"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|row| row["cells"].as_array().unwrap())
        .find(|cell| cell["addr"] == serde_json::json!({"row": arow, "col": acol}))
        .expect("anchor cell present");
    let text = edited["paragraphs"][0]["runs"][0]["content"]["Text"].as_str().unwrap_or("");
    assert_eq!(text, "격자값");
}

#[test]
fn set_cell_error_codes_and_all_or_nothing() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("never.hwpx");
    let run_edit = |args: &[&str]| {
        let mut full = vec!["set-cell", f.to_str().unwrap()];
        full.extend_from_slice(args);
        full.extend_from_slice(&["-o", out.to_str().unwrap()]);
        run_json(&full)
    };

    let (value, _, code) = run_edit(&["--table", "99", "--at", "0,0", "--text", "x"]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "TABLE_NOT_FOUND");

    let (value, _, code) = run_edit(&["--table", "0", "--at", "9999,0", "--text", "x"]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "CELL_NOT_FOUND");

    let (value, _, code) =
        run_edit(&["--table", "0", "--right-of", "존재하지않는라벨", "--text", "x"]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "CELL_NOT_FOUND");

    let (value, _, code) = run_edit(&["--table", "0", "--at", "abc", "--text", "x"]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_SET_CELL_ARGS");

    let (value, _, code) = run_edit(&["--table", "0", "--text", "x"]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_SET_CELL_ARGS");

    assert!(!out.exists(), "rejected edits must not produce output (all-or-nothing)");
}

// ═══════════════════════════════════════════════════════════════
// E3 Wave 4: convert 경로 병합셀 격자 회귀 잠금
// ═══════════════════════════════════════════════════════════════

/// HWP5 record → cellAddr 구성 경로가 병합 표에서도 well-formed 격자를
/// 산출함을 잠근다: 변환 산출물의 모든 표가 addr 주석을 받아야 하며
/// (addr 부재 = 타일링 불변식 위반 = TABLE_GRID_UNADDRESSABLE 경고),
/// 리서치가 확인한 커버리지 공백(병합 convert fixture 0개)을 메운다.
#[test]
fn convert_hwp5_merged_tables_produce_addressable_grids() {
    for name in ["tables/table_02_merge_col_row.hwp", "tables/table_08_nested_table.hwp"] {
        let source = fixture(name);
        let tmp = test_tmp();
        let out = tmp.join("converted.hwpx");
        let (_, _, code) =
            run(&["convert-hwp5", source.to_str().unwrap(), "-o", out.to_str().unwrap()]);
        assert_eq!(code, 0, "{name}: convert must succeed");

        let json_out = tmp.join("converted.json");
        let (_, stderr, code) =
            run(&["to-json", out.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
        assert_eq!(code, 0, "{name}: export must succeed");
        assert!(
            !stderr.contains("TABLE_GRID_UNADDRESSABLE")
                && !stderr.contains("without grid addresses"),
            "{name}: converted tables must tile a well-formed grid: {stderr}"
        );

        // 모든 표의 모든 셀이 addr 를 받았는지 전수 확인.
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&json_out).unwrap()).unwrap();
        fn assert_cells_addressed(value: &serde_json::Value, ctx: &str) {
            match value {
                serde_json::Value::Object(map) => {
                    if let (Some(rows), true) = (map.get("rows"), map.contains_key("cell_spacing"))
                    {
                        for row in rows.as_array().into_iter().flatten() {
                            for cell in
                                row.get("cells").and_then(|c| c.as_array()).into_iter().flatten()
                            {
                                assert!(
                                    cell.get("addr").is_some(),
                                    "{ctx}: cell without addr: {cell}"
                                );
                            }
                        }
                    }
                    map.values().for_each(|v| assert_cells_addressed(v, ctx));
                }
                serde_json::Value::Array(items) => {
                    items.iter().for_each(|v| assert_cells_addressed(v, ctx));
                }
                _ => {}
            }
        }
        assert_cells_addressed(&parsed, name);
    }
}

// ═══════════════════════════════════════════════════════════════
// to-md 모드 게이트 (E3 Wave 4 경고 E2E + 커버리지 공백 해소)
// ═══════════════════════════════════════════════════════════════

#[test]
fn to_md_styled_keeps_merges_and_lossy_warns_flattening() {
    let f = fixture("tables/merged_grid_form.hwpx");

    // styled(기본): 병합 표는 rowspan HTML 로 보존 — 평탄화 경고 없음.
    let tmp = test_tmp();
    let (_, stderr, code) = run(&["to-md", f.to_str().unwrap(), "-o", tmp.to_str().unwrap()]);
    assert_eq!(code, 0, "{stderr}");
    let md = std::fs::read_to_string(tmp.join("merged_grid_form.md")).unwrap();
    assert!(md.contains("rowspan=\"2\""), "styled must keep merges as HTML: {md}");
    assert!(!stderr.contains("TABLE_MERGE_FLATTENED"));

    // lossy: GFM 평탄화 + TABLE_MERGE_FLATTENED 경고 (Wave 4 warning-first).
    let tmp = test_tmp();
    let (_, stderr, code) =
        run(&["to-md", f.to_str().unwrap(), "-o", tmp.to_str().unwrap(), "--mode", "lossy"]);
    assert_eq!(code, 0);
    assert!(stderr.contains("merged cell"), "lossy must warn about flattening: {stderr}");
    // json 모드는 구조화 경고 코드로.
    let tmp = test_tmp();
    let (_, _, stderr, code) = run_json_with_stdout(&[
        "to-md",
        f.to_str().unwrap(),
        "-o",
        tmp.to_str().unwrap(),
        "--mode",
        "lossy",
    ]);
    assert_eq!(code, 0);
    assert!(stderr.contains("TABLE_MERGE_FLATTENED"), "{stderr}");
}

#[test]
fn to_md_lossless_mode_runs() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let (_, _, code) =
        run(&["to-md", f.to_str().unwrap(), "-o", tmp.to_str().unwrap(), "--mode", "lossless"]);
    assert_eq!(code, 0);
    assert!(tmp.join("merged_grid_form.md").exists());
}

// ═══════════════════════════════════════════════════════════════
// set-cell --map 배치·인자 충돌 게이트
// ═══════════════════════════════════════════════════════════════

#[test]
fn set_cell_map_batch_reports_resolution_and_clear() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let map = tmp.join("cells.json");
    std::fs::write(
        &map,
        serde_json::json!([
            {"table": 0, "at": {"row": 2, "col": 0}, "text": "세로병합값"},
            {"table": 0, "right_of": "성명", "text": ""}
        ])
        .to_string(),
    )
    .unwrap();
    let out = tmp.join("batch.hwpx");

    // 사람용 출력 경로: covered → anchor 리다이렉트와 clear 마커가 보여야 한다.
    let (stdout, stderr, code) = run(&[
        "set-cell",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("Set 2 cell(s)"), "{stdout}");
    assert!(stdout.contains("covered -> anchor (1, 0)"), "{stdout}");
    assert!(stdout.contains("[cleared]"), "{stdout}");
    assert!(out.exists());
}

#[test]
fn set_cell_map_arg_conflicts_rejected() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("never2.hwpx");
    let map = tmp.join("cells.json");
    std::fs::write(&map, "[]").unwrap();

    // --map 과 단건 플래그 동시 사용 금지.
    let (value, _, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "--table",
        "0",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_SET_CELL_ARGS");

    // 빈 맵 거부.
    let (value, _, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_SET_CELL_MAP");

    // 맵 파싱 실패.
    std::fs::write(&map, "{not json").unwrap();
    let (value, _, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_SET_CELL_MAP");

    // --at 와 --right-of 동시 지정.
    let (value, _, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--table",
        "0",
        "--at",
        "0,0",
        "--right-of",
        "성명",
        "--text",
        "x",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_SET_CELL_ARGS");
    assert!(!out.exists());
}

// ═══════════════════════════════════════════════════════════════
// stamp v2 — E6 Wave 2 클래스-B 셀 스탬핑 게이트
// ═══════════════════════════════════════════════════════════════

/// merged_grid_form 의 셀 후보 3개(성명·비고·비고-병합경계)에 대한 v2 맵.
fn stamp_v2_map(plan: &serde_json::Value) -> serde_json::Value {
    let sha = plan["source_sha256"].as_str().unwrap();
    serde_json::json!({
        "schema_version": 2,
        "source_sha256": sha,
        "cells": [
            {"table": 0, "at": {"row": 0, "col": 1},
             "label": {"at": {"row": 0, "col": 0}, "text": "성명"},
             "action": {"field": {"name": "성명값", "hint": "성명 입력"}}},
            {"table": 0, "at": {"row": 1, "col": 1},
             "action": {"field": {"name": "비고값", "hint": "비고 입력"}}},
            {"table": 0, "at": {"row": 2, "col": 1}, "action": "ignore"},
        ],
    })
}

#[test]
fn stamp_plan_v2_lists_cell_candidates_with_source_hash() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let (value, _, code) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["schema_version"], 2);
    let sha = value["source_sha256"].as_str().unwrap();
    assert_eq!(sha.len(), 64, "hex sha256: {sha}");
    let cells = value["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 3, "성명·비고·병합경계 비고: {value}");
    assert!(value["skipped_tables"].as_array().unwrap().is_empty());
    // 병합 라벨(비고, rowspan 2)이 (2,1) 의 shared-boundary 라벨로 잡혀야 한다.
    let covered_boundary = &cells[2];
    assert_eq!(covered_boundary["at"], serde_json::json!({"row": 2, "col": 1}));
    assert_eq!(covered_boundary["labels"][0]["normalized"], "비고");
    // duplicate_count 는 문서 내 라벨 "텍스트" 중복 기준 — 비고 셀은 유일하므로
    // 두 후보 모두 같은 suggested_name 을 받는다 (제안은 비구속; 그대로 복사해
    // 쓰면 preflight DuplicateName 이 거부).
    assert_eq!(cells[1]["suggested_name"], "비고");
    assert_eq!(covered_boundary["suggested_name"], "비고");
}

#[test]
fn stamp_v2_cells_end_to_end_then_fillable() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let (plan, _, code) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code, 0);

    let map = tmp.join("v2-map.json");
    std::fs::write(&map, stamp_v2_map(&plan).to_string()).unwrap();
    let out = tmp.join("v2-stamped.hwpx");
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "v2 stamp must succeed: {value}");
    assert_eq!(value["stamped_cells"].as_array().unwrap().len(), 2);
    assert_eq!(value["ignored"], 1);
    let manifest_path = value["manifest"].as_str().unwrap().to_string();
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["schema_version"], 2);
    let cell_origins: Vec<_> = manifest["fields"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| !f["stamp"]["cell"].is_null())
        .collect();
    assert_eq!(cell_origins.len(), 2, "{manifest}");

    // 산출물은 즉시 fields/fill 로 소비 가능.
    let (fields, _, code) = run_json(&["fields", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(fields["fields"].as_array().unwrap().len(), 2);
    let filled = tmp.join("v2-filled.hwpx");
    let (fill, _, code) = run_json(&[
        "fill",
        out.to_str().unwrap(),
        "--set",
        "성명값=홍길동",
        "-o",
        filled.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "fill on cell-stamped output must succeed: {fill}");
}

#[test]
fn stamp_v2_source_hash_mismatch_rejected() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let (plan, _, _) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    let mut map_value = stamp_v2_map(&plan);
    map_value["source_sha256"] = serde_json::Value::String("0".repeat(64));
    let map = tmp.join("stale-map.json");
    std::fs::write(&map, map_value.to_string()).unwrap();
    let out = tmp.join("never-v2.hwpx");
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "STAMP_SOURCE_HASH_MISMATCH");
    assert!(!out.exists(), "sha 불일치 시 산출물이 없어야 한다 (fail-closed)");
}

#[test]
fn stamp_v2_uncovered_cell_and_unknown_field_rejected() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let (plan, _, _) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    let sha = plan["source_sha256"].as_str().unwrap();

    // 후보 3개 중 1개만 커버 → STAMP_CANDIDATE_UNCOVERED.
    let partial = serde_json::json!({
        "schema_version": 2, "source_sha256": sha,
        "cells": [{"table": 0, "at": {"row": 0, "col": 1},
                   "action": {"field": {"name": "성명값", "hint": "h"}}}],
    });
    let map = tmp.join("partial-v2.json");
    std::fs::write(&map, partial.to_string()).unwrap();
    let out = tmp.join("never-v2b.hwpx");
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "STAMP_CANDIDATE_UNCOVERED");
    assert!(!out.exists());

    // unknown field 가 든 v2 맵은 parse 단계에서 거부.
    let typo = serde_json::json!({
        "schema_version": 2, "source_sha256": sha, "cell": [],
    });
    std::fs::write(&map, typo.to_string()).unwrap();
    let (value, _, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_STAMP_MAP");
}

// ─── W6a: to-pdf (조판 캐시 재생 렌더) ───

/// 한컴 폰트 번들 (fixture-optional 관례 — CI 에는 없음).
fn hancom_ttf_dir() -> Option<PathBuf> {
    let dir =
        PathBuf::from("/Applications/Hancom Office HWP.app/Contents/Resources/Hnc/Shared/TTF");
    dir.exists().then_some(dir)
}

#[test]
fn to_pdf_renders_hwpx_fixture() {
    if hancom_ttf_dir().is_none() {
        return;
    }
    let dir = test_tmp();
    let out = dir.join("pagenum.pdf");
    let (value, _stderr, code) = run_json(&[
        "to-pdf",
        fixture("pdf-rules/rules-pagenum.hwpx").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--discovery",
        "hancom",
    ]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["status"], "ok");
    assert_eq!(value["detected_format"], "hwpx");
    assert_eq!(value["warning_counts"]["render"], 0);
    let bytes = std::fs::read(&out).expect("output pdf");
    assert!(bytes.starts_with(b"%PDF-"), "PDF 헤더");
}

#[test]
fn to_pdf_sniffs_content_over_extension() {
    // corpus 실측(.hwpx 탈 HWP5 79건)의 역방향 재현: HWPX 를 .hwp 로 위장 —
    // 확장자가 아니라 콘텐츠로 라우팅하고 불일치를 경고한다.
    if hancom_ttf_dir().is_none() {
        return;
    }
    let dir = test_tmp();
    let misnamed = dir.join("misnamed.hwp");
    std::fs::copy(fixture("pdf-rules/rules-headerfooter.hwpx"), &misnamed).expect("copy");
    let out = dir.join("misnamed.pdf");
    let (value, _stderr, code) = run_json(&[
        "to-pdf",
        misnamed.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--discovery",
        "hancom",
    ]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["detected_format"], "hwpx");
    let mismatch = value["warnings"]
        .as_array()
        .expect("warnings array")
        .iter()
        .any(|w| w["code"] == "EXTENSION_MISMATCH");
    assert!(mismatch, "{value}");
}

#[test]
fn to_pdf_rejects_unrecognized_container() {
    let dir = test_tmp();
    let garbage = dir.join("garbage.hwpx");
    std::fs::write(&garbage, b"not a container at all").expect("write");
    let (value, _stderr, code) = run_json(&["to-pdf", garbage.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(value["code"], "UNRECOGNIZED_FORMAT");
}

#[test]
fn to_pdf_hwp5_path_fails_closed_on_unnormalized_textpos() {
    // 실측 잠금 (W6a): convert 의 HWP5 텍스트 위치 정규화 미완으로 carry 캐시가
    // admission(textpos 정합)에서 거부된다 — 조용한 오출력 대신 깨끗한 에러.
    // convert 정규화가 개선되면 이 게이트를 성공 게이트로 갱신할 것.
    let (value, _stderr, code) =
        run_json(&["to-pdf", fixture("pdf-rules/rules-header-multi.hwp").to_str().unwrap()]);
    assert_eq!(code, 2, "{value}");
    assert_eq!(value["status"], "error");
    assert_eq!(value["code"], "PDF_RENDER_FAILED");
}
