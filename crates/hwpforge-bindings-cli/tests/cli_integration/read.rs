use super::*;

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
    // legacy checked target-count/paras-without-section
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
fn read_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let (value, _, code) = run_json(&["read", f.to_str().unwrap(), "--field", "user_email"]);
    assert_eq!(code, 0);
    assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED", "{value}");

    let (_, text_stderr, code2) = run(&["read", f.to_str().unwrap(), "--field", "user_email"]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("[read] LAYOUT_CACHE_DROPPED"), "{text_stderr}");
}
