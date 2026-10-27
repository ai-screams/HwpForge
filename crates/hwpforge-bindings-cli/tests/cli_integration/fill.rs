use super::*;

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
