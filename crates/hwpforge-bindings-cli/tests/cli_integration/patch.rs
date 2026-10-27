use super::*;

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
