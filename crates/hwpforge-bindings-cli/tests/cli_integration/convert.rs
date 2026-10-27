use super::*;

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

/// 게이트: bare 파일명 입력(`convert a.md`)에서도 md 옆 이미지가
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

/// 게이트: 해결 불가 참조(원격 URL)가 문단의 유일한 run 이어도
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
    // Legacy validated `--preset` before any
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
