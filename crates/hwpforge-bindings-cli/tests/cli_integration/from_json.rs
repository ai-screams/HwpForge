use super::*;

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
