use super::*;

// ═══════════════════════════════════════════════════════════════
// 7. schema — 7 tests
// ═══════════════════════════════════════════════════════════════

#[test]
fn schema_document() {
    let (stdout, _, code) = run(&["schema", "document"]);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(parsed["$schema"].is_string() || parsed["type"].is_string());
}

#[test]
fn schema_document_json_envelope() {
    let (val, _, code) = run_json(&["schema", "document"]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert_eq!(val["type"], "document");
    assert!(val["schema"].is_object());
}

#[test]
fn schema_exported_document() {
    let (val, _, code) = run_json(&["schema", "exported-document"]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert_eq!(val["type"], "exported-document");
}

#[test]
fn schema_exported_section() {
    let (val, _, code) = run_json(&["schema", "exported-section"]);
    assert_eq!(code, 0);
    assert_eq!(val["status"], "ok");
    assert_eq!(val["type"], "exported-section");
}

#[test]
fn schema_unknown_type() {
    let (_, _, code) = run(&["schema", "foobar"]);
    assert_eq!(code, 1);
}

#[test]
fn schema_default_type() {
    // No argument -> defaults to "document"
    let (stdout_default, _, code1) = run(&["schema"]);
    assert_eq!(code1, 0);
    let (stdout_explicit, _, code2) = run(&["schema", "document"]);
    assert_eq!(code2, 0);
    assert_eq!(stdout_default, stdout_explicit);
}

#[test]
fn schema_has_properties() {
    let (stdout, _, code) = run(&["schema", "document"]);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    // JSON Schema should have either "properties" or "$defs"
    assert!(
        parsed.get("properties").is_some() || parsed.get("$defs").is_some(),
        "schema should have properties or $defs"
    );
}
