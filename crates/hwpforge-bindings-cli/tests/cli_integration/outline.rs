use super::*;

#[test]
fn outline_exposes_named_fields_axis() {
    let f = fixture("clickhere_named.hwpx");
    let (value, _, code) = run_json(&["outline", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["outline"]["fields"][0]["name"], "user_email");
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
fn outline_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let (value, _, code) = run_json(&["outline", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED", "{value}");

    let (_, text_stderr, code2) = run(&["outline", f.to_str().unwrap()]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("[outline] LAYOUT_CACHE_DROPPED"), "{text_stderr}");
}
