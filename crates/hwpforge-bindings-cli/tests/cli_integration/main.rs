mod convert;
mod diff;
mod errors;
mod fields;
mod fill;
mod from_json;
mod hwp5;
mod inspect;
mod outline;
mod patch;
mod read;
mod schema;
mod set_cell;
mod stamp;
mod structural_edit;
mod templates;
mod to_json;
mod to_md;
mod to_pdf;
mod workflow;

#[path = "../support/document.rs"]
mod document;
use document::text_para;
#[path = "../support/cli_integration.rs"]
mod shared;
use shared::*;

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use hwpforge_core::control::Control;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::RunContent;
use hwpforge_smithy_hwpx::ExportedSection;
use zip::ZipArchive;

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
            file.name().expect("ZIP entry name").to_string()
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

/// Create a temporary markdown file with given content. Returns path.
fn create_test_md(dir: &Path, content: &str) -> PathBuf {
    let path = dir.join("input.md");
    std::fs::write(&path, content).expect("write test md");
    path
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

fn assert_cli_manifest_case(id: &str, error: &serde_json::Value, exit: i32) {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../hwpforge/tests/data/error_contract_018.json"))
            .unwrap();
    let case = manifest["cases"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap();
    assert_eq!(error["code"], case["expected_code"], "{id}");
    assert_eq!(exit, case["surfaces"]["cli"]["exit"].as_i64().unwrap() as i32, "{id}");
}
