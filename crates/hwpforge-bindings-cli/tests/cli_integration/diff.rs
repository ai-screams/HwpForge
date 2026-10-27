use super::*;

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
    // fill 은 stale linesegarray 도 제거 — raw 축에 `$.layout_cache`
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
