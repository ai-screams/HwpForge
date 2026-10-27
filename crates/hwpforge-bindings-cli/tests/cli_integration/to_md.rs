use super::*;

// ═══════════════════════════════════════════════════════════════
// to-md 모드 게이트 (경고 E2E + 커버리지 공백 해소)
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

    // lossy: GFM 평탄화 + TABLE_MERGE_FLATTENED 경고 (warning-first).
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
