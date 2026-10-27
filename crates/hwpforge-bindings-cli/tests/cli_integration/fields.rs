use super::*;

// ═══════════════════════════════════════════════════════════════
// fields / fill — E2 누름틀 델타 API 게이트
// ═══════════════════════════════════════════════════════════════

#[test]
fn fields_lists_named_clickhere_with_fillability() {
    let f = fixture("clickhere_named.hwpx");
    let (value, _, code) = run_json(&["fields", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["fields"][0]["name"], "user_email");
    assert_eq!(value["fields"][0]["fillable"], true);
}

#[test]
fn fields_surfaces_layout_cache_dropped_decode_warning() {
    let f = fixture("layout/stale-line-cache.hwpx");
    let (value, _, code) = run_json(&["fields", f.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(value["warnings"][0]["code"], "LAYOUT_CACHE_DROPPED", "{value}");

    let (_, text_stderr, code2) = run(&["fields", f.to_str().unwrap()]);
    assert_eq!(code2, 0);
    assert!(text_stderr.contains("[fields] LAYOUT_CACHE_DROPPED"), "{text_stderr}");
}
