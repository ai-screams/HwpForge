use super::*;

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
