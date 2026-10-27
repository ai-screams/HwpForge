use super::*;

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
