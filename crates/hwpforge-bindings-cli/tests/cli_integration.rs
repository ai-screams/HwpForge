#[path = "support/cli_integration.rs"]
mod shared;
use shared::*;

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

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

/// W6 §12b B2 게이트: bare 파일명 입력(`convert a.md`)에서도 md 옆 이미지가
/// 임베드된다 — `parent()` 의 빈 경로가 base 를 잃게 하던 회귀 잠금.
#[test]
fn convert_bare_filename_embeds_sibling_image() {
    let tmp = test_tmp();
    let png: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
    std::fs::write(tmp.join("i.png"), png).expect("write png");
    std::fs::write(tmp.join("a.md"), "본문 ![아이콘](i.png) 끝.\n").expect("write md");
    let output = Command::new(hwpforge_bin())
        .current_dir(&tmp)
        .args(["convert", "a.md", "-o", "out.hwpx"])
        .output()
        .expect("run hwpforge");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "stderr: {stderr}");
    assert!(!stderr.contains("not embedded"), "임베드 경고 없어야 함: {stderr}");
    let bytes = std::fs::read(tmp.join("out.hwpx")).expect("read out");
    assert!(
        bytes.windows(b"BinData/image1.png".len()).any(|w| w == b"BinData/image1.png"),
        "zip 에 BinData 엔트리"
    );
}

/// W6 §12b B1 게이트: 해결 불가 참조(원격 URL)가 문단의 유일한 run 이어도
/// 변환은 성공한다 — 빈 문단 validate 거부로 문서 전체가 죽던 회귀 잠금.
#[test]
fn convert_remote_image_only_paragraph_still_succeeds() {
    let tmp = test_tmp();
    let md = tmp.join("r.md");
    std::fs::write(&md, "# 제목\n\n![remote](https://example.com/logo.png)\n\n본문.\n")
        .expect("write md");
    let out = tmp.join("r.hwpx");
    let (_, stderr, code) = run(&["convert", md.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stderr.contains("not embedded"), "typed 경고 표면화: {stderr}");
    assert!(out.exists());
}

/// `![x]()` (shared fixture) is reported and left out, as in 0.17.0.
/// 이것을 실패시키는 것: 디코더의 빈 대상 경고 삭제(빈 틀이 경고 없이 남는다).
#[test]
fn convert_reports_and_drops_an_empty_image_target() {
    let tmp = test_tmp();
    let md = fixture("markdown/empty-image-target.md");
    let out = tmp.join("e.hwpx");
    let (_, stderr, code) = run(&["convert", md.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stderr.matches(r#"image "" not embedded (file unreadable) — image dropped"#).count(),
        1,
        "stderr: {stderr}"
    );
    let bytes = std::fs::read(&out).expect("read out");
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("Contents/section0.xml").unwrap(), &mut xml)
        .unwrap();
    assert!(!xml.contains("<hp:pic "), "no empty frame may remain");
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
// 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
fn convert_unknown_preset_rejected_before_missing_input_is_read() {
    // W3 remediation finding 6: legacy validated `--preset` before any
    // input I/O — a bogus preset must still report UNKNOWN_PRESET even
    // against a nonexistent input file, not FILE_READ_FAILED.
    let tmp = test_tmp();
    let out = tmp.join("output.hwpx");
    let (err, _, code) = run_json(&[
        "convert",
        "/nonexistent/input.md",
        "-o",
        out.to_str().unwrap(),
        "--preset",
        "bogus",
    ]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "PRESET_NOT_FOUND");
    assert_eq!(err["hint"], "Available presets: default, modern, classic, latest");
}

#[test]
// 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
fn convert_unknown_preset_rejected_before_stdin_is_read() {
    // Same guard, `-` (stdin) input this time — an empty stdin would
    // otherwise either block or surface as a different failure; the
    // preset check must fire first and never touch stdin.
    let (_, stderr, code) =
        run_with_stdin(&["--json", "convert", "-", "-o", "/dev/null", "--preset", "bogus"], "");
    assert_eq!(code, 1);
    let err: serde_json::Value = serde_json::from_str(&stderr)
        .unwrap_or_else(|e| panic!("invalid JSON stderr: {e}\n{stderr}"));
    assert_eq!(err["code"], "PRESET_NOT_FOUND");
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

#[test]
fn inspect_nonexistent_file() {
    let (_, _, code) = run(&["inspect", "/nonexistent/file.hwpx"]);
    assert_eq!(code, 1);
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
    // 이것을 실패시키는 것: 생성 문서의 문단 하나를 제거하기.
    let f = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../hwpforge-smithy-hwpx/tests/fixtures/garden_columns.hwpx");
    // 이것을 실패시키는 것: 생성 문서의 단 설정을 단일 단으로 바꾸기.
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode_file(&f).unwrap();
    assert_eq!(decoded.document.sections()[0].column_settings.as_ref().unwrap().columns.len(), 2);
    let (val, _, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(val["sections"][0]["paragraphs"], 6);
}

/// Value-parity regression (independent review round 2, finding A): a
/// table/image/chart nested inside an *image's* caption is invisible to
/// `ops::inspect`'s shared paragraph traversal — Core's
/// `image_caption_paragraphs_are_skipped_documents_known_gap` test
/// documents that image captions (unlike table/textbox captions) are not
/// walked. `inspect.rs`'s `tables`/`images`/`charts`/`deep_paragraphs`
/// fields must keep reading from the pre-migration local scanner (via
/// `SectionInfo::merge`'s `deep` parameter — the raw-XML
/// `count_occurrences` scan for the first three, `count_paragraphs_recursive`
/// for the last), or these counts silently drop content or change value on
/// any document exercising these two independent blind spots. No fixture
/// under `tests/fixtures/**` contains an `<hp:caption` at all (checked by
/// scanning every `.hwpx` fixture's section XML for the element), so this
/// test builds the document with Core's API and encodes it, rather than
/// editing fixture bytes by hand.
///
/// This fixture pins two *different* legacy-vs-`ops` divergences, so a
/// regression on either field is caught even though the mechanisms differ:
///
/// - `tables`/`images`/`charts`: a table, a second image and a chart are
///   nested inside the outer image's caption. The raw-XML scan sees every
///   `<hp:tbl>`/`<hp:pic>`/`<hp:chart>` in the section regardless of
///   nesting; `ops::inspect`'s traversal never enters the caption at all,
///   so it would see only the outer image and nothing nested in it.
///   **If `SectionInfo::merge` read `ops_section.{tables,images,charts}`
///   instead of `deep.{tables,images,charts}`, this test would see
///   `tables=0, images=1, charts=0` instead of the asserted `1, 2, 1`.**
/// - `deep_paragraphs`: the section also carries one master-page paragraph.
///   `ops::inspect`'s traversal (`Section::for_each_paragraph` /
///   `walk_paragraphs`, `crates/hwpforge-core/src/section.rs`) explicitly
///   walks `section.master_pages`; the CLI's own local
///   `count_paragraphs_recursive` (`analysis/deep_counts.rs`) sums only
///   `section.paragraphs` plus headers and footers — no `master_pages` term
///   exists in that file at all. Both scopes independently treat an
///   `Image` run as a paragraph-less leaf (skip its caption), which is why
///   the caption's three nested paragraphs contribute to neither scope's
///   count — the master page is the only paragraph source that tells them
///   apart. **If `SectionInfo::merge` read `ops_section.paragraphs` instead
///   of `deep.deep_paragraphs`, this test would see `deep_paragraphs=2`
///   (1 body + 1 master page) instead of the asserted `1`.**
#[test]
fn inspect_deep_counts_table_image_chart_nested_in_image_caption_and_master_page() {
    use hwpforge_core::caption::{Caption, CaptionSide};
    use hwpforge_core::chart::{ChartData, ChartGrouping, ChartType, LegendPosition};
    use hwpforge_core::control::Control;
    use hwpforge_core::image::{Image, ImageFormat, ImageStore};
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::{MasterPage, Section};
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_core::{Document, Paragraph};
    use hwpforge_foundation::{ApplyPageType, CharShapeIndex, HwpUnit, ParaShapeIndex};
    use hwpforge_smithy_hwpx::style_store::{
        HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore,
    };
    use hwpforge_smithy_hwpx::HwpxEncoder;

    fn text_para(text: &str) -> Paragraph {
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
    }

    let mut store = HwpxStyleStore::new();
    for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());

    // Caption content: a table, a second image and a chart, all nested
    // inside the *outer* image's caption — exactly the traversal's blind
    // spot for these three fields.
    let nested_table = Table::new(vec![TableRow::new(vec![TableCell::new(
        vec![text_para("caption-table-cell")],
        HwpUnit::from_pt(100.0).unwrap(),
    )])]);
    let nested_image = Image::new(
        "BinData/nested.png",
        HwpUnit::from_pt(10.0).unwrap(),
        HwpUnit::from_pt(10.0).unwrap(),
        ImageFormat::Png,
    );
    let nested_chart = Control::Chart {
        chart_type: ChartType::Bar,
        data: ChartData::category(&["A", "B"], &[("Series1", [1.0, 2.0].as_slice())]),
        width: HwpUnit::new(10000).unwrap(),
        height: HwpUnit::new(8000).unwrap(),
        title: None,
        legend: LegendPosition::default(),
        grouping: ChartGrouping::Clustered,
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    };
    let mut caption_table_para = text_para("caption-table-host");
    caption_table_para.add_run(Run::table(nested_table, CharShapeIndex::new(0)));
    let mut caption_image_para = text_para("caption-image-host");
    caption_image_para.add_run(Run::image(nested_image, CharShapeIndex::new(0)));
    let mut caption_chart_para = text_para("caption-chart-host");
    caption_chart_para.add_run(Run::control(nested_chart, CharShapeIndex::new(0)));

    let mut host_image = Image::new(
        "BinData/host.png",
        HwpUnit::from_pt(10.0).unwrap(),
        HwpUnit::from_pt(10.0).unwrap(),
        ImageFormat::Png,
    );
    host_image.caption = Some(Caption::new(
        vec![text_para("caption text"), caption_table_para, caption_image_para, caption_chart_para],
        CaptionSide::Bottom,
    ));

    let mut host = text_para("host");
    host.add_run(Run::image(host_image, CharShapeIndex::new(0)));

    let mut section = Section::with_paragraphs(vec![host], PageSettings::a4());
    // Second, independent blind spot: a master-page paragraph, present
    // only in `ops_section.paragraphs`'s scope (see doc comment above).
    section.master_pages =
        Some(vec![MasterPage::new(ApplyPageType::Both, vec![text_para("master-page-para")])]);

    let mut doc = Document::new();
    doc.add_section(section);
    let validated = doc.validate().expect("validate");
    // `HwpxEncoder` silently skips images missing from the `ImageStore`
    // (XML reference only, no binary data) — no real image bytes are
    // needed to prove the structural-count divergence. Neither `validate`
    // nor the encoder inspect an `Image`'s caption or a section's master
    // pages any deeper than the round-trip needs (`RunContent::Image` is a
    // validation no-op; master pages aren't in `validate_sections` at all).
    let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).expect("encode");

    let dir = test_tmp();
    let src = dir.join("image-caption-and-masterpage-nested-controls.hwpx");
    std::fs::write(&src, &bytes).expect("write fixture");

    let (val, _, code) = run_json(&["inspect", src.to_str().unwrap()]);
    assert_eq!(code, 0, "{val}");
    let sec0 = &val["sections"][0];
    // Legacy (pinned) values. `ops` scope, listed for contrast, is in the
    // doc comment above.
    assert_eq!(sec0["images"], 2, "{val}");
    assert_eq!(sec0["tables"], 1, "{val}");
    assert_eq!(sec0["charts"], 1, "{val}");
    assert_eq!(sec0["deep_paragraphs"], 1, "{val}");
}

/// An empty reference carries Hancom's assetless frame; both inspect counts see it.
/// 이것을 실패시키는 것: convert_picture 의 빈 참조 조기 반환 복원.
#[test]
fn crafted_assetless_pic_is_counted_by_raw_scan_and_decode() {
    check_crafted_picture_counts(false);
}

/// 기존 raw-scan 계약 고정. 이것을 실패시키는 것: CLI images를 all_images로 대체.
#[test]
fn crafted_pic_without_img_is_counted_only_by_raw_scan() {
    check_crafted_picture_counts(true);
}

fn check_crafted_picture_counts(remove_img: bool) {
    use hwpforge_core::image::{Image, ImageFormat, ImageStore};
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::Section;
    use hwpforge_core::{Document, Paragraph};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
    use hwpforge_smithy_hwpx::style_store::{
        HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore,
    };
    use hwpforge_smithy_hwpx::HwpxEncoder;

    fn text_para(text: &str) -> Paragraph {
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
    }

    let mut store = HwpxStyleStore::new();
    for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());

    let image = Image::new(
        "BinData/host.png",
        HwpUnit::from_pt(10.0).unwrap(),
        HwpUnit::from_pt(10.0).unwrap(),
        ImageFormat::Png,
    );
    let mut host = text_para("host");
    host.add_run(Run::image(image, CharShapeIndex::new(0)));

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![host], PageSettings::a4()));
    let validated = doc.validate().expect("validate");
    let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).expect("encode");

    // Hancom clears a lost asset reference while retaining the picture frame.
    let mut archive = ZipArchive::new(std::io::Cursor::new(bytes)).expect("open zip");
    let mut section_xml = String::new();
    archive
        .by_name("Contents/section0.xml")
        .expect("section0")
        .read_to_string(&mut section_xml)
        .expect("read section0");
    assert!(
        section_xml.contains(r#"binaryItemIDRef="host""#),
        "encoder must have written the reference to tamper with: {section_xml}"
    );
    let tampered_xml = if remove_img {
        let start = section_xml.find("<hc:img ").expect("img start");
        let end = start + section_xml[start..].find("/>").expect("img end") + 2;
        format!("{}{}", &section_xml[..start], &section_xml[end..])
    } else {
        section_xml.replace(r#"binaryItemIDRef="host""#, r#"binaryItemIDRef="""#)
    };

    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).expect("entry").name().expect("ZIP entry name").to_string())
        .collect();
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (index, name) in names.iter().enumerate() {
        if name == "Contents/section0.xml" {
            continue;
        }
        let entry = archive.by_index_raw(index).expect("raw entry");
        writer.raw_copy_file(entry).expect("copy entry");
    }
    writer
        .start_file("Contents/section0.xml", zip::write::SimpleFileOptions::default())
        .expect("start section0");
    writer.write_all(tampered_xml.as_bytes()).expect("write section0");
    let tampered_bytes = writer.finish().expect("finish zip").into_inner();

    let dir = test_tmp();
    let path = dir.join("crafted-pic-no-binary-ref.hwpx");
    std::fs::write(&path, &tampered_bytes).expect("write fixture");

    let out = hwpforge::ops::inspect(&tampered_bytes, &hwpforge::ops::InspectOptions::default())
        .expect("assetless picture is valid");
    assert_eq!(
        out.report.section_details[0].all_images,
        usize::from(!remove_img),
        "decoded-object count requires hc:img, even with an empty reference: {:?}",
        out.report.section_details[0]
    );

    // CLI `--json`: the raw scan still sees the `<hp:pic>` element, exactly
    // as the pre-migration scanner would have.
    let (val, _, code) = run_json(&["inspect", path.to_str().unwrap()]);
    assert_eq!(code, 0, "{val}");
    assert_eq!(
        val["sections"][0]["images"], 1,
        "raw scan includes hp:pic regardless of hc:img presence: {val}"
    );
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
fn audit_hwp5_nonexistent_source() {
    let tmp = test_tmp();
    let out = tmp.join("output.hwpx");
    let (_, _, code) = run(&["audit-hwp5", "/nonexistent/file.hwp", out.to_str().unwrap()]);
    assert_eq!(code, 1);
}

#[test]
// 이것을 실패시키는 것: to-md/convert-hwp5 입력 I/O 종료를 2에서 1로 변경.
fn convert_hwp5_nonexistent_file() {
    let tmp = test_tmp();
    let out = tmp.join("missing.hwpx");

    let (_, _, code) = run(&["convert-hwp5", "/nonexistent/file.hwp", "-o", out.to_str().unwrap()]);
    assert_eq!(code, 2);
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
    // 이것을 실패시키는 것: 생성 문서의 문단 하나를 제거하기.
    let f = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../hwpforge-smithy-hwpx/tests/fixtures/garden_columns.hwpx");
    // 이것을 실패시키는 것: 생성 문서의 단 설정을 단일 단으로 바꾸기.
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode_file(&f).unwrap();
    assert_eq!(decoded.document.sections()[0].column_settings.as_ref().unwrap().columns.len(), 2);
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let (_, _, code) = run(&["to-json", f.to_str().unwrap(), "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(&json_out).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let paras = parsed["document"]["sections"][0]["paragraphs"].as_array().unwrap();
    assert_eq!(paras.len(), 6);
}

#[test]
fn to_json_nonexistent_file() {
    let tmp = test_tmp();
    let json_out = tmp.join("doc.json");
    let (_, stderr, code) =
        run(&["--json", "to-json", "/nonexistent/file.hwpx", "-o", json_out.to_str().unwrap()]);
    assert_eq!(code, 1);
    let err: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(err["code"], "FILE_NOT_FOUND");
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
fn from_json_syntax_error_reports_no_hint() {
    // W3 remediation: the pre-`ops` classification of JSON_PARSE_FAILED —
    // a raw syntax error never carried a hint, only the schema-mismatch
    // case did (`from_json_schema_mismatch_reports_the_legacy_hint`).
    let tmp = test_tmp();
    let bad_json = tmp.join("bad.json");
    std::fs::write(&bad_json, "not valid json").unwrap();
    let out = tmp.join("out.hwpx");
    let (err, _, code) =
        run_json(&["from-json", bad_json.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(err["code"], "JSON_PARSE_FAILED");
    assert!(err.get("hint").is_none(), "syntax errors carry no hint: {err:?}");
}

#[test]
fn from_json_schema_mismatch_points_at_the_exported_document_schema() {
    // Valid JSON, but not an ExportedDocument — the schema-mismatch call
    // site, distinct from the raw-syntax one above.
    let tmp = test_tmp();
    let wrong_shape = tmp.join("wrong_shape.json");
    std::fs::write(&wrong_shape, r#"{"foo": 1}"#).unwrap();
    let out = tmp.join("out.hwpx");
    let (err, _, code) =
        run_json(&["from-json", wrong_shape.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(err["code"], "JSON_PARSE_FAILED");
    assert_eq!(
        err["hint"],
        "Ensure the JSON matches the HwpForge document schema (run 'hwpforge schema exported-document')"
    );
}

#[test]
fn from_json_nonexistent_input() {
    let tmp = test_tmp();
    let out = tmp.join("out.hwpx");
    let (_, _, code) = run(&["from-json", "/nonexistent/file.json", "-o", out.to_str().unwrap()]);
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
fn outline_exposes_named_fields_axis() {
    let f = fixture("clickhere_named.hwpx");
    let (value, _, code) = run_json(&["outline", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["outline"]["fields"][0]["name"], "user_email");
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
fn read_arg_guards_run_before_the_file_is_read() {
    // W3 remediation: legacy checked target-count/paras-without-section
    // before `check_file_size`/`fs::read`, so these reject even against a
    // file that does not exist — FILE_READ_FAILED must not shadow them.
    let (err, _, code) = run_json(&["read", "/nonexistent/file.hwpx"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "READ_TARGET_REQUIRED");

    // `--table` supplies the single required target so this isolates the
    // paras-without-section guard, same as `read_error_paths_report_stable_
    // codes`'s `--table 0 --paras 0` case.
    let (err, _, code) =
        run_json(&["read", "/nonexistent/file.hwpx", "--table", "0", "--paras", "0..1"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "READ_PARAS_WITHOUT_SECTION");
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
    // W1b: fill 은 stale linesegarray 도 제거 — raw 축에 `$.layout_cache`
    // 변경 하나가 정직하게 보고된다.
    let raw = semantic["raw"].as_array().unwrap();
    assert_eq!(raw.len(), 1, "raw: {raw:?}");
    assert_eq!(raw[0]["detail"], "$.layout_cache");
    // Full report file written alongside inline output.
    let report_value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(report_value["identical"], false);
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
// 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
fn read_error_paths_report_stable_codes() {
    let f = fixture("clickhere_named.hwpx");

    let (err, _, code) =
        run_json(&["read", f.to_str().unwrap(), "--section", "0", "--paras", "abc"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "READ_PARAS_INVALID");

    let (err, _, code) = run_json(&["read", f.to_str().unwrap(), "--section", "9"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "SECTION_OUT_OF_RANGE");

    let (err, _, code) = run_json(&["read", f.to_str().unwrap(), "--field", "없는이름"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "FIELD_NOT_FOUND");

    let (err, _, code) = run_json(&["read", f.to_str().unwrap(), "--table", "42"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "TABLE_NOT_FOUND");

    let (err, _, code) = run_json(&["read", f.to_str().unwrap(), "--paras", "0"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "READ_TARGET_REQUIRED");

    let (err, _, code) = run_json(&["read", f.to_str().unwrap(), "--table", "0", "--paras", "0"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "READ_PARAS_WITHOUT_SECTION");

    // Text-mode error rendering path.
    let (_, stderr, code) = run(&["read", f.to_str().unwrap(), "--field", "없는이름"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("FIELD_NOT_FOUND"), "stderr: {stderr}");
}

#[test]
fn read_shape_rejections_spell_this_frontend_s_flag_names() {
    // The three rejections `read` makes before it has a document: two are
    // this command's own pre-read guards (`commands/read.rs`), the third is
    // `ops::read`'s paras parse, which words the argument the way every
    // frontend shares (`paras`) and gets the CLI's `--paras` spelling back in
    // `compat::cli_message`. All three are the CLI's user-facing text, so
    // they are pinned byte-for-byte here rather than by code alone.
    let f = fixture("clickhere_named.hwpx");

    let (err, _, code) = run_json(&["read", f.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "READ_TARGET_REQUIRED");
    assert_eq!(err["message"], "Pass exactly one of --section, --table, --field");

    let (err, _, code) = run_json(&["read", f.to_str().unwrap(), "--table", "0", "--paras", "0"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "READ_PARAS_WITHOUT_SECTION");
    assert_eq!(err["message"], "--paras requires --section");

    let (err, _, code) =
        run_json(&["read", f.to_str().unwrap(), "--section", "0", "--paras", "abc"]);
    assert_eq!(code, 1);
    assert_eq!(err["code"], "READ_PARAS_INVALID");
    assert_eq!(
        err["message"],
        r#"invalid input: Cannot parse --paras "abc": use "A..B" (inclusive) or a single "N""#
    );

    // Text mode renders the same message, so the flag spelling is pinned on
    // both output paths.
    let (_, stderr, code) = run(&["read", f.to_str().unwrap(), "--section", "0", "--paras", "abc"]);
    assert_eq!(code, 1);
    assert!(stderr.contains(r#"Cannot parse --paras "abc""#), "stderr: {stderr}");
}

#[test]
// 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
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
    assert_eq!(err["code"], "FILE_NOT_FOUND");
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
fn structural_edit_batch_and_structural_rejections() {
    // 이것을 실패시키는 것: T10 StructuralEditError::Codec 매핑을 InvalidInput으로 변경.
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
    assert_eq!(err["code"], "DECODE_FAILED");
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
fn fill_reports_a_removed_control_character() {
    // 이것을 실패시키는 것: ops `fill` 이 인코드 경고 채널을 `warnings` 에 합치지 않는 것.
    let f = fixture("clickhere_named.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("filled_ctrl.hwpx");
    let (value, _, code) = run_json(&[
        "fill",
        f.to_str().unwrap(),
        "--set",
        "user_email=a\u{1}b",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert_eq!(value["warnings"][0]["code"], "XML_FORBIDDEN_CHARS_REMOVED", "{value}");
    let (again, _, _) = run_json(&["fields", out.to_str().unwrap()]);
    assert_eq!(again["fields"][0]["current"], "ab");
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

#[test]
fn set_cell_map_arg_conflict_rejected_before_map_file_is_read() {
    // W3 remediation: the --map/single-target mutual-exclusion guard must
    // run before the (possibly nonexistent) --map file is read — a
    // combined-flags misuse should never surface as FILE_READ_FAILED just
    // because the map path happens not to exist.
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("never3.hwpx");

    let (value, _, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--map",
        "/nonexistent/missing.json",
        "--table",
        "0",
        "--at",
        "0,0",
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
fn to_pdf_rejects_cacheless_hwpx_fail_closed() {
    // 폰트 무관 CI 경로: 순수 생성 HWPX(캐시 없음) → 스니핑·디코드·검증까지
    // 통과 후 렌더에서 fail-closed (한컴 재저장 안내 힌트).
    let dir = test_tmp();
    let md = dir.join("doc.md");
    std::fs::write(&md, "# 제목\n\n본문 문단.\n").expect("write md");
    let hwpx = dir.join("doc.hwpx");
    let (_v, _s, code) = run_json(&["convert", md.to_str().unwrap(), "-o", hwpx.to_str().unwrap()]);
    assert_eq!(code, 0);
    let (value, _stderr, code) = run_json(&["to-pdf", hwpx.to_str().unwrap()]);
    assert_eq!(code, 2, "{value}");
    assert_eq!(value["code"], "PDF_RENDER_FAILED");
    assert_eq!(value["cause"]["stage"], "render", "{value}");
    assert_eq!(value["cause"]["code"], "NO_RENDERABLE_CACHE", "{value}");
    assert_eq!(value["cause"]["location"], "s0", "{value}");
}

#[test]
fn to_pdf_extension_mismatch_detected_without_fonts() {
    // 위장 확장자(.hwp 탈 HWPX)도 폰트 없이 스니핑 라우팅까지는 CI 에서 검증
    // 가능 — 이후 캐시 결손 fail-closed 는 위 게이트와 동일.
    let dir = test_tmp();
    let md = dir.join("doc.md");
    std::fs::write(&md, "# 제목\n").expect("write md");
    let hwpx = dir.join("doc.hwpx");
    let (_v, _s, code) = run_json(&["convert", md.to_str().unwrap(), "-o", hwpx.to_str().unwrap()]);
    assert_eq!(code, 0);
    let misnamed = dir.join("doc-misnamed.hwp");
    std::fs::copy(&hwpx, &misnamed).expect("copy");
    let (value, _stderr, code) = run_json(&["to-pdf", misnamed.to_str().unwrap()]);
    assert_eq!(code, 2, "{value}"); // cacheless — 렌더 거부는 동일
    assert_eq!(value["code"], "PDF_RENDER_FAILED");
    assert_eq!(value["cause"]["code"], "NO_RENDERABLE_CACHE", "{value}");
}

#[test]
fn to_pdf_human_mode_error_output() {
    let dir = test_tmp();
    let garbage = dir.join("garbage.bin");
    std::fs::write(&garbage, b"???").expect("write");
    let (_stdout, stderr, code) = run(&["to-pdf", garbage.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(stderr.contains("UNRECOGNIZED_FORMAT"), "{stderr}");
}

/// R1 F4 회귀 잠금 (CLI stamp): 의미 손상은 typed `StamperError::SemanticLoss`
/// 를 canonical `ENCODE_SEMANTIC_LOSS`로 내보내며 exit 2를 유지한다.
/// 의미 손상을 포괄 실패로 축약하면 이 계약이 깨진다.
///
/// fixture 는 `smithy-hwpx/tests/note_numbering_roundtrip.rs` 의
/// `stamper_fails_closed_on_note_head_skip` 과 같은 구성이다: 각주 본문의
/// 첫 문단이 heading 이라 번호 머리를 넣을 안전한 지점이 없다.
#[test]
// 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
fn stamp_semantic_loss_keeps_the_codec_error_contract() {
    use hwpforge_core::control::Control;
    use hwpforge_core::image::ImageStore;
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::Section;
    use hwpforge_core::{Document, Paragraph};
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    use hwpforge_smithy_hwpx::style_store::{
        HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore,
    };
    use hwpforge_smithy_hwpx::HwpxEncoder;

    let mut store = HwpxStyleStore::new();
    for &lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
    }
    store.push_char_shape(HwpxCharShape::default());
    store.push_para_shape(HwpxParaShape::default());

    let mut heading_body = Paragraph::with_runs(
        vec![Run::text("제목 각주", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    heading_body.heading_level = Some(1);
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::control(Control::footnote(vec![heading_body]), CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let validated = doc.validate().expect("validate");
    let base = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).expect("encode");

    let tmp = test_tmp();
    let src = tmp.join("titlemark-note.hwpx");
    std::fs::write(&src, &base).unwrap();
    // 빈 legacy 맵 = `HwpxStamper::stamp(&base, &[])` 와 같은 경로.
    let map = tmp.join("empty-map.json");
    std::fs::write(&map, "[]").unwrap();
    let out = tmp.join("stamped.hwpx");

    let (err, _, code) = run_json(&[
        "stamp",
        src.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);

    assert_eq!(code, 2, "exit code 는 과거 Codec 경로와 같아야 한다: {err}");
    assert_eq!(err["code"], "ENCODE_SEMANTIC_LOSS", "{err}");
    let message = err["message"].as_str().unwrap_or_default();
    assert!(
        message.starts_with("encode produced a semantic-loss warning (fail-closed): "),
        "메시지가 드리프트했다 (접두사 없는 형태여야 한다): {message}"
    );
    assert!(message.contains("note number head skipped"), "{message}");
    assert!(!out.exists(), "fail-closed 인데 산출물이 생성됐다");
}

// ═══════════════════════════════════════════════════════════════
// W5 후속: MCP/Python 처럼 ops 디코드 경고(`LAYOUT_CACHE_DROPPED` 등)를
// CLI 도 노출한다 — `--json`의 `warnings` 키(비었으면 생략) +
// 텍스트 모드 stderr 한 줄씩. `delete-para`/`insert-para`는 기존
// `warnings` 관례(항상 존재, 구조적 자문과 같은 배열)를 따른다.
// ═══════════════════════════════════════════════════════════════

#[test]
fn inspect_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let (value, stderr, code) = run_json(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED", "{value}");
    assert!(!value["warnings"][0]["message"].as_str().unwrap_or_default().is_empty());

    let (_, text_stderr, code2) = run(&["inspect", f.to_str().unwrap()]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("[inspect] LAYOUT_CACHE_DROPPED"), "{text_stderr}");
    let _ = stderr; // --json 모드는 stdout 의 warnings 키만 쓰고 stderr 는 비운다.
}

#[test]
fn outline_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let (value, _, code) = run_json(&["outline", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED", "{value}");

    let (_, text_stderr, code2) = run(&["outline", f.to_str().unwrap()]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("[outline] LAYOUT_CACHE_DROPPED"), "{text_stderr}");
}

#[test]
fn fields_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let (value, _, code) = run_json(&["fields", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED", "{value}");

    let (_, text_stderr, code2) = run(&["fields", f.to_str().unwrap()]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("[fields] LAYOUT_CACHE_DROPPED"), "{text_stderr}");
}

#[test]
fn read_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let (value, _, code) = run_json(&["read", f.to_str().unwrap(), "--field", "user_email"]);
    assert_eq!(code, 0);
    assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED", "{value}");

    let (_, text_stderr, code2) = run(&["read", f.to_str().unwrap(), "--field", "user_email"]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("[read] LAYOUT_CACHE_DROPPED"), "{text_stderr}");
}

#[test]
fn fill_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("filled.hwpx");
    let (value, _, code) = run_json(&[
        "fill",
        f.to_str().unwrap(),
        "--set",
        "user_email=w5@example.com",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED", "{value}");

    let out2 = tmp.join("filled2.hwpx");
    let (_, text_stderr, code2) = run(&[
        "fill",
        f.to_str().unwrap(),
        "--set",
        "user_email=w5@example.com",
        "-o",
        out2.to_str().unwrap(),
    ]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("[fill] LAYOUT_CACHE_DROPPED"), "{text_stderr}");
}

#[test]
fn fill_has_no_warnings_key_on_a_clean_fixture() {
    let f = fixture("fields/clickhere_named.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("filled.hwpx");
    let (value, stderr, code) = run_json(&[
        "fill",
        f.to_str().unwrap(),
        "--set",
        "user_email=w5@example.com",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{value}");
    assert!(!value.as_object().unwrap().contains_key("warnings"), "{value}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn to_json_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("out.json");
    let (_, _, stderr, code) =
        run_json_with_stdout(&["to-json", f.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stderr.contains("LAYOUT_CACHE_DROPPED"), "{stderr}");

    let out2 = tmp.join("out2.json");
    let (_, text_stderr, code2) =
        run(&["to-json", f.to_str().unwrap(), "-o", out2.to_str().unwrap()]);
    assert_eq!(code2, 0);
    // Text mode keeps this command's pre-existing shape: `Warning: {message}`,
    // no code (the `--json` assertion above is where the code shows up).
    assert!(text_stderr.contains("layout cache dropped"), "{text_stderr}");
}

#[test]
fn to_md_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let tmp = test_tmp();
    let (_, _, stderr, code) =
        run_json_with_stdout(&["to-md", f.to_str().unwrap(), "-o", tmp.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(stderr.contains("LAYOUT_CACHE_DROPPED"), "{stderr}");

    let tmp2 = test_tmp();
    let (_, text_stderr, code2) =
        run(&["to-md", f.to_str().unwrap(), "-o", tmp2.to_str().unwrap()]);
    assert_eq!(code2, 0);
    // Text mode keeps this command's pre-existing shape: `Warning: {message}`,
    // no code (the `--json` assertion above is where the code shows up).
    assert!(text_stderr.contains("layout cache dropped"), "{text_stderr}");
}

#[test]
fn set_cell_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("sc.hwpx");
    let (value, _, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--table",
        "0",
        "--at",
        "0,1",
        "--text",
        "값",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED", "{value}");

    let out2 = tmp.join("sc2.hwpx");
    let (_, text_stderr, code2) = run(&[
        "set-cell",
        f.to_str().unwrap(),
        "--table",
        "0",
        "--at",
        "0,1",
        "--text",
        "값",
        "-o",
        out2.to_str().unwrap(),
    ]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("[set-cell] LAYOUT_CACHE_DROPPED"), "{text_stderr}");
}

#[test]
fn set_cell_has_no_warnings_key_on_a_clean_fixture() {
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("sc.hwpx");
    let (value, stderr, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--table",
        "0",
        "--at",
        "0,0",
        "--text",
        "값",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{value}");
    assert!(!value.as_object().unwrap().contains_key("warnings"), "{value}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn patch_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let tmp = test_tmp();
    let section_json = tmp.join("section.json");
    let (_, _, code) = run(&[
        "to-json",
        f.to_str().unwrap(),
        "-o",
        section_json.to_str().unwrap(),
        "--section",
        "0",
    ]);
    assert_eq!(code, 0);

    let patched = tmp.join("patched.hwpx");
    let (value, _, code) = run_json(&[
        "patch",
        f.to_str().unwrap(),
        "--section",
        "0",
        section_json.to_str().unwrap(),
        "-o",
        patched.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED", "{value}");

    let patched2 = tmp.join("patched2.hwpx");
    let (_, text_stderr, code2) = run(&[
        "patch",
        f.to_str().unwrap(),
        "--section",
        "0",
        section_json.to_str().unwrap(),
        "-o",
        patched2.to_str().unwrap(),
    ]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("[patch] LAYOUT_CACHE_DROPPED"), "{text_stderr}");
}

#[test]
fn stamp_plan_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let (value, _, code) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED", "{value}");

    let (_, text_stderr, code2) = run(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("[stamp-plan] LAYOUT_CACHE_DROPPED"), "{text_stderr}");
}

#[test]
fn stamp_plan_has_no_warnings_key_on_a_clean_fixture() {
    let f = fixture("stamp/placeholder_basic.hwpx");
    let (value, stderr, code) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(!value.as_object().unwrap().contains_key("warnings"), "{value}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn stamp_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let (plan, _, code) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code, 0);

    let tmp = test_tmp();
    let map = tmp.join("map.json");
    // Legacy 배열 맵 — 셀 후보는 legacy 스펙이 표현 못 하므로 커버 대상이 아니다
    // (본문 하나짜리 placeholder 만 커버해도 all-or-nothing 을 만족한다).
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
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED", "{value}");

    let out2 = tmp.join("stamped2.hwpx");
    let (_, text_stderr, code2) = run(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out2.to_str().unwrap(),
    ]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("[stamp] LAYOUT_CACHE_DROPPED"), "{text_stderr}");
}

#[test]
fn stamp_has_no_warnings_key_on_a_clean_fixture() {
    let f = fixture("stamp/placeholder_basic.hwpx");
    let (plan, _, code) = run_json(&["stamp-plan", f.to_str().unwrap()]);
    assert_eq!(code, 0);

    let tmp = test_tmp();
    let map = tmp.join("map.json");
    std::fs::write(&map, stamp_map_from_plan(&plan, false).to_string()).unwrap();
    let out = tmp.join("stamped.hwpx");
    let (value, stderr, code) = run_json(&[
        "stamp",
        f.to_str().unwrap(),
        "--map",
        map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{value}");
    assert!(!value.as_object().unwrap().contains_key("warnings"), "{value}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn insert_para_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("inserted.hwpx");
    let (value, _, code) = run_json(&[
        "insert-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--anchor",
        "0",
        "--text",
        "새 문단",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{value}");
    assert!(
        value["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap_or_default().starts_with("LAYOUT_CACHE_DROPPED")),
        "{value}"
    );

    let out2 = tmp.join("inserted2.hwpx");
    let (_, text_stderr, code2) = run(&[
        "insert-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--anchor",
        "0",
        "--text",
        "새 문단",
        "-o",
        out2.to_str().unwrap(),
    ]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("warning: LAYOUT_CACHE_DROPPED"), "{text_stderr}");
}

#[test]
fn insert_para_has_empty_warnings_on_a_clean_fixture() {
    let f = fixture("structural/plain_paragraphs.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("inserted.hwpx");
    let (value, stderr, code) = run_json(&[
        "insert-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--anchor",
        "1",
        "--text",
        "새 문단",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["warnings"].as_array().map(Vec::len), Some(0), "{value}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn delete_para_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("deleted.hwpx");
    let (value, _, code) = run_json(&[
        "delete-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--index",
        "3",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{value}");
    assert!(
        value["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap_or_default().starts_with("LAYOUT_CACHE_DROPPED")),
        "{value}"
    );

    let out2 = tmp.join("deleted2.hwpx");
    let (_, text_stderr, code2) = run(&[
        "delete-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--index",
        "3",
        "-o",
        out2.to_str().unwrap(),
    ]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("warning: LAYOUT_CACHE_DROPPED"), "{text_stderr}");
}

#[test]
fn delete_para_has_empty_warnings_on_a_clean_fixture() {
    let f = fixture("structural/plain_paragraphs.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("deleted.hwpx");
    let (value, stderr, code) = run_json(&[
        "delete-para",
        f.to_str().unwrap(),
        "--section",
        "0",
        "--index",
        "2",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["warnings"].as_array().map(Vec::len), Some(0), "{value}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn diff_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let (value, _, code) = run_json(&["diff", f.to_str().unwrap(), f.to_str().unwrap()]);
    assert_eq!(code, 0);
    // 자기 자신과의 diff 는 base·revised 양쪽 디코드에서 한 번씩, 두 건이 온다.
    let warnings = value["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 2, "{value}");
    assert!(warnings.iter().all(|w| w["code"] == "LAYOUT_CACHE_DROPPED"), "{value}");

    let (_, text_stderr, code2) = run(&["diff", f.to_str().unwrap(), f.to_str().unwrap()]);
    assert_eq!(code2, 0);
    assert_eq!(text_stderr.matches("[diff] LAYOUT_CACHE_DROPPED").count(), 2, "{text_stderr}");
}

#[cfg(unix)]
#[test]
// 이것을 실패시키는 것: to-md InputTooLarge 종료를 2로 변경하거나 convert 상한 힌트를 UTF-8 안내로 교체.
fn fifo_input_over_the_size_cap_reports_input_too_large() {
    for command in ["inspect", "to-md", "convert"] {
        let tmp = test_tmp();
        let fifo_path = tmp.join("oversized.hwpx");
        create_fifo(&fifo_path);

        let writer_path = fifo_path.clone();
        let writer = std::thread::spawn(move || {
            let Ok(mut f) = std::fs::File::create(&writer_path) else { return };
            let chunk = vec![0u8; 1024 * 1024];
            // Comfortably over the 100 MB cap; the reader (`read_input`'s
            // `take(MAX_FILE_SIZE + 1)`) stops consuming once it has the extra
            // byte it needs to detect the overflow and exits, so this loop
            // reliably breaks on the first write past that point — `write_all`
            // then reports `BrokenPipe` (Rust ignores `SIGPIPE`, so a write past
            // a closed reader is an ordinary `io::Error`, not a fatal signal).
            for _ in 0..150 {
                if f.write_all(&chunk).is_err() {
                    break;
                }
            }
        });

        let output = tmp.join("out.hwpx");
        let args = if command == "convert" {
            vec![command, fifo_path.to_str().unwrap(), "-o", output.to_str().unwrap()]
        } else {
            vec![command, fifo_path.to_str().unwrap()]
        };
        let (val, stderr, code) = run_json(&args);
        let _ = writer.join();

        assert_eq!(code, 1, "stderr: {stderr}, value: {val}");
        if command == "to-md" {
            assert_cli_manifest_case("io.to-md.fifo_over_limit", &val, code);
        }
        assert_eq!(val["code"], "INPUT_TOO_LARGE", "{val}");
        if command == "convert" {
            assert_eq!(val["hint"], "Use an input smaller than the 100 MB limit");
        }
    }
}

#[cfg(unix)]
#[test]
// 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
fn set_cell_fifo_map_over_the_size_cap_reports_input_too_large() {
    // `set-cell --map` had no `check_file_size` preflight at all before the
    // `read_to_string` bounded-read fix (audit follow-up) — unlike every
    // other read site, it never had an `INPUT_TOO_LARGE` code of its own to
    // reuse. The cap surfaces through this site's existing
    // `FILE_READ_FAILED`/exit 1 (`read_bounded_string`'s `Err` arm), not a
    // newly introduced `INPUT_TOO_LARGE`.
    let f = fixture("tables/merged_grid_form.hwpx");
    let tmp = test_tmp();
    let out = tmp.join("never.hwpx");
    let fifo_path = tmp.join("oversized_map.json");
    create_fifo(&fifo_path);

    let writer_path = fifo_path.clone();
    let writer = std::thread::spawn(move || {
        let Ok(mut w) = std::fs::File::create(&writer_path) else { return };
        let chunk = vec![b'a'; 1024 * 1024];
        for _ in 0..150 {
            if w.write_all(&chunk).is_err() {
                break;
            }
        }
    });

    let (val, stderr, code) = run_json(&[
        "set-cell",
        f.to_str().unwrap(),
        "--map",
        fifo_path.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    let _ = writer.join();

    assert_eq!(code, 1, "stderr: {stderr}, value: {val}");
    assert_eq!(val["code"], "INPUT_TOO_LARGE", "{val}");
    assert!(!out.exists());
}

#[cfg(unix)]
#[test]
// 이것을 실패시키는 것: convert-hwp5 InputTooLarge 종료를 1에서 2로 변경.
fn convert_hwp5_fifo_over_the_size_cap_reports_input_too_large() {
    // `convert-hwp5` used to read its input path twice — an unbounded
    // `std::fs::read` inside the (now removed) `inspect_hwp5_file` call,
    // then a bounded `read_bounded` for conversion. That let an oversized
    // FIFO grow memory without limit through the first, unbounded read
    // (this test's chunk loop is comfortably over the 100 MB cap), and
    // would have hung a *finite* FIFO on the second `open` once the
    // original writer had already exited. The command now reads the FIFO
    // exactly once through `read_bounded`, so this exits promptly with the
    // canonical `INPUT_TOO_LARGE`/exit 1, matching regular-file preflight.
    let tmp = test_tmp();
    let out = tmp.join("never.hwpx");
    let fifo_path = tmp.join("oversized.hwp");
    create_fifo(&fifo_path);

    let writer_path = fifo_path.clone();
    let writer = std::thread::spawn(move || {
        let Ok(mut f) = std::fs::File::create(&writer_path) else { return };
        let chunk = vec![0u8; 1024 * 1024];
        // Comfortably over the 100 MB cap; `read_bounded`'s single capped
        // read stops consuming once it has confirmed the overflow, so
        // `write_all` reliably reports `BrokenPipe` and this loop breaks —
        // proving the writer thread does not outlive the reader.
        for _ in 0..150 {
            if f.write_all(&chunk).is_err() {
                break;
            }
        }
    });

    let (val, stderr, code) =
        run_json(&["convert-hwp5", fifo_path.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    writer.join().expect("writer thread panicked");

    assert_eq!(code, 1, "stderr: {stderr}, value: {val}");
    assert_eq!(val["code"], "INPUT_TOO_LARGE", "{val}");
    assert_cli_manifest_case("io.convert-hwp5.fifo_over_limit", &val, code);
    assert!(!out.exists());
}

#[test]
fn w2_legacy_json_tabs_share_code_reason_and_remedy() {
    let dir = test_tmp();
    let input = dir.join("legacy.json");
    let output = dir.join("out.hwpx");
    std::fs::write(&input, r#"{"styles":{"tabs":[{"stops":[{"position":2001}]}]}}"#).unwrap();
    let out = Command::new(hwpforge_bin())
        .arg("--json")
        .arg("from-json")
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let text =
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(text.contains("LEGACY_JSON_TABS"), "{text}");
    assert!(text.contains("원래 단위와 case/default 척도를 복구할 수 없어"), "{text}");
    assert!(text.contains("$.styles.tabs[0].stops[0].position"), "{text}");
    assert!(text.contains("1건"), "{text}");
    assert!(text.contains("원본 HWPX/HWP5에서 새 버전의 to-json을 다시 실행한 뒤 편집하십시오."));
    assert!(!output.exists());
}

#[test]
// 이것을 실패시키는 것: 변환 명령 입력 I/O 종료를 1로 변경, InputTooLarge 종료를 2로 변경, 사전 검사 코드를 FileReadFailed로 변경.
fn w1_input_path_and_metadata_boundaries() {
    let dir = test_tmp();
    let missing = dir.join("missing.hwpx");
    let output = dir.join("out.hwpx");
    for args in [
        vec!["inspect", missing.to_str().unwrap()],
        vec!["to-md", missing.to_str().unwrap()],
        vec!["convert-hwp5", missing.to_str().unwrap(), "-o", output.to_str().unwrap()],
        vec!["convert", missing.to_str().unwrap(), "-o", output.to_str().unwrap()],
    ] {
        let (err, _, exit) = run_json(&args);
        assert_eq!(err["code"], "FILE_NOT_FOUND", "{args:?}");
        assert_eq!(exit, if matches!(args[0], "to-md" | "convert-hwp5") { 2 } else { 1 });
    }
    for command in ["to-md", "convert-hwp5"] {
        let args = if command == "convert-hwp5" {
            vec![command, dir.to_str().unwrap(), "-o", output.to_str().unwrap()]
        } else {
            vec![command, dir.to_str().unwrap()]
        };
        let (err, _, exit) = run_json(&args);
        assert_eq!(err["code"], "FILE_READ_FAILED");
        assert_eq!(exit, 2, "unreadable directory: {command}");
    }
    let exact = dir.join("exact.hwpx");
    std::fs::File::create(&exact).unwrap().set_len(hwpforge::ops::fs::MAX_FILE_SIZE).unwrap();
    let (err, _, exit) = run_json(&["inspect", exact.to_str().unwrap()]);
    assert_eq!(err["code"], "DECODE_FAILED", "exact cap must reach the codec");
    assert_eq!(exit, 2);
    let large = dir.join("large.hwpx");
    std::fs::File::create(&large).unwrap().set_len(hwpforge::ops::fs::MAX_FILE_SIZE + 1).unwrap();
    for command in ["to-md", "convert-hwp5"] {
        let args = if command == "convert-hwp5" {
            vec![command, large.to_str().unwrap(), "-o", output.to_str().unwrap()]
        } else {
            vec![command, large.to_str().unwrap()]
        };
        let (err, _, exit) = run_json(&args);
        assert_eq!(err["code"], "INPUT_TOO_LARGE");
        assert_eq!(exit, 1, "metadata preflight: {command}");
        assert_cli_manifest_case(&format!("io.{command}.input_size_preflight"), &err, exit);
    }
    let (err, _, exit) = run_json(&["inspect", large.to_str().unwrap()]);
    assert_eq!(err["code"], "INPUT_TOO_LARGE");
    assert!(err["message"].as_str().unwrap().contains("is 100 MB"));
    assert_eq!(exit, 1);
}

#[test]
// 이것을 실패시키는 것: ReadError::Codec을 InvalidInput으로 매핑하거나 typed 직렬화의 code 키를 바꿈.
fn decode_input_shared_fixture() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../hwpforge/tests/data/error_contract_018.json"))
            .unwrap();
    let case =
        manifest["cases"].as_array().unwrap().iter().find(|c| c["id"] == "decode.input").unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../hwpforge/tests/data/corrupt-input.hwpx");
    let (err, _, exit) = run_json(&["read", path.to_str().unwrap(), "--section", "0"]);
    assert_eq!(err["code"], case["expected_code"]);
    assert_eq!(exit, case["surfaces"]["cli"]["exit"].as_i64().unwrap() as i32);
}

#[test]
// 이것을 실패시키는 것: patch 원본 decode 를 Rejected 로 감싸기(PATCH_FAILED), metadata 검사를
// 원본 decode 뒤로 옮기기(DECODE_FAILED), fill 입력 decode 를 Workflow 로 되돌리기(FILL_FAILED).
fn w2_patch_and_fill_shared_fixtures() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../hwpforge/tests/data");
    let out = test_tmp().join("out.hwpx");
    let base = data.join("patch-inner-decode.hwpx");
    for (id, json) in [
        ("patch.input_decode", "patch-inner-decode.section.json"),
        ("patch.missing_metadata_first", "patch-no-metadata.section.json"),
    ] {
        let section = data.join(json);
        let (err, _, exit) = run_json(&[
            "patch",
            base.to_str().unwrap(),
            section.to_str().unwrap(),
            "--section",
            "0",
            "-o",
            out.to_str().unwrap(),
        ]);
        assert_cli_manifest_case(id, &err, exit);
    }
    let corrupt = data.join("corrupt-input.hwpx");
    let (err, _, exit) =
        run_json(&["fill", corrupt.to_str().unwrap(), "--set", "a=b", "-o", out.to_str().unwrap()]);
    assert_cli_manifest_case("fill.input_decode", &err, exit);
    assert!(!out.exists(), "a refused edit writes no output");
}

#[test]
// 이것을 실패시키는 것(W4): convert ops 의 decoder Core 갈래를 HWP5_CONVERT_FAILED 로 되돌리기
// (inspect 단계는 이 입력을 통과시키므로 convert-hwp5 도 ops 분류를 그대로 보인다).
fn w4_hwp5_decoder_core_shared_fixture() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../hwpforge/tests/data");
    let source = data.join("hwp5-dangling-table-border-fill.hwp");
    let out = test_tmp().join("out.hwpx");
    let (err, _, exit) =
        run_json(&["convert-hwp5", source.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_cli_manifest_case("hwp5.decode_core", &err, exit);
    assert!(!out.exists(), "a refused conversion writes no output");
    let pdf = test_tmp().join("out.pdf");
    let (err, _, exit) =
        run_json(&["to-pdf", source.to_str().unwrap(), "-o", pdf.to_str().unwrap()]);
    assert_cli_manifest_case("hwp5.to_pdf.decode_core", &err, exit);
    assert!(!pdf.exists(), "a refused render writes no output");
}

fn assert_cli_manifest_case(id: &str, error: &serde_json::Value, exit: i32) {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../hwpforge/tests/data/error_contract_018.json"))
            .unwrap();
    let case = manifest["cases"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap();
    assert_eq!(error["code"], case["expected_code"], "{id}");
    assert_eq!(exit, case["surfaces"]["cli"]["exit"].as_i64().unwrap() as i32, "{id}");
}

#[test]
fn w3_editors_share_decode_and_admission_fixtures() {
    // 이것을 실패시키는 것: E2/E4 cell·stamp admission 호출 목적을 Output으로 변경.
    // 이것을 실패시키는 것: F4 v2 admission 호출 목적을 Output으로 변경.
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../hwpforge/tests/data");
    let dir = test_tmp();
    let out = dir.join("out.hwpx");
    let map = dir.join("map.json");
    std::fs::write(&map, "[]").unwrap();
    for (fixture, ids) in [
        (
            "corrupt-input.hwpx",
            [
                "cell.input_decode",
                "structural.input_decode",
                "structural_delete.input_decode",
                "stamp.input_decode",
            ],
        ),
        (
            "edit-empty-section.hwpx",
            [
                "cell.admission",
                "structural.admission",
                "structural_delete.admission",
                "stamp.admission",
            ],
        ),
    ] {
        let input = data.join(fixture);
        let file = input.to_str().unwrap();
        let output = out.to_str().unwrap();
        for (id, args) in [
            (
                ids[0],
                vec![
                    "set-cell", file, "--table", "0", "--at", "0,0", "--text", "text", "-o", output,
                ],
            ),
            (
                ids[1],
                vec![
                    "insert-para",
                    file,
                    "--section",
                    "0",
                    "--anchor",
                    "0",
                    "--text",
                    "text",
                    "-o",
                    output,
                ],
            ),
            (ids[2], vec!["delete-para", file, "--section", "0", "--index", "0", "-o", output]),
            (ids[3], vec!["stamp", file, "--map", map.to_str().unwrap(), "-o", output]),
        ] {
            let (error, _, exit) = run_json(&args);
            assert_cli_manifest_case(id, &error, exit);
            assert!(!out.exists());
        }
    }
    let input = data.join("edit-empty-section.hwpx");
    let v2_map = data.join("stamp-v2-admission.json");
    let (error, _, exit) = run_json(&[
        "stamp",
        input.to_str().unwrap(),
        "--map",
        v2_map.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_cli_manifest_case("stamp.v2_admission", &error, exit);
    assert!(!out.exists());
    let input = data.join("corrupt-input.hwpx");
    let (error, _, exit) = run_json(&["stamp-plan", input.to_str().unwrap()]);
    assert_cli_manifest_case("stamp_plan.input_decode", &error, exit);
    for name in ["depth-table-42", "depth-table-33", "depth-group-32", "depth-xml-broken"] {
        let input = data.join(format!("{name}.hwpx"));
        let (error, _, exit) = run_json(&[
            "set-cell",
            input.to_str().unwrap(),
            "--table",
            "0",
            "--at",
            "0,0",
            "--text",
            "text",
            "-o",
            out.to_str().unwrap(),
        ]);
        assert_cli_manifest_case(&format!("cell.{name}"), &error, exit);
        assert!(!error["hint"].as_str().unwrap_or_default().contains("Report this as a bug."));
    }
}
