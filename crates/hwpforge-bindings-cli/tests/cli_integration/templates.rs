use super::*;

// ═══════════════════════════════════════════════════════════════
// 6. templates — 6 tests
// ═══════════════════════════════════════════════════════════════

#[test]
fn templates_list() {
    let (stdout, _, code) = run(&["templates", "list"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("default"));
}

#[test]
fn templates_list_json() {
    let (val, _, code) = run_json(&["templates", "list"]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert!(val["presets"].is_array());
}

#[test]
fn templates_show() {
    let (stdout, _, code) = run(&["templates", "show", "default"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Preset: default"));
}

#[test]
fn templates_show_json() {
    let (val, _, code) = run_json(&["templates", "show", "default"]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert_eq!(val["preset"]["name"], "default");
}

#[test]
fn templates_show_nonexistent() {
    let (_, _, code) = run(&["templates", "show", "nonexistent"]);
    assert_eq!(code, 1);
}

#[test]
fn templates_show_json_fields() {
    let (val, _, code) = run_json(&["templates", "show", "default"]);
    assert_eq!(code, 0);
    assert!(val["preset"]["font"].is_string(), "missing font field");
    assert!(val["preset"]["page_size"].is_string(), "missing page_size field");
}
