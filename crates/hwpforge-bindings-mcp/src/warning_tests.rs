//! Router-level WM-W fixtures: the `warnings` wire shape of every tool,
//! read from real JSON-RPC responses (`param_tests::call`).
//!
//! Every tool but `hwpforge_templates` reports warnings. Empty → the key is
//! omitted; non-empty → `{code, message, hint?}` objects in `OpsWarning`
//! order. `hwpforge_templates` has no warnings field at all.
use super::param_tests::call;
use super::HwpForgeServer;
use serde_json::{json, Value};

/// The 18 tools that report warnings. Written out, not derived from the
/// router, so a new tool must be classified here by hand.
const WARNING_TOOLS: [&str; 18] = [
    "hwpforge_convert",
    "hwpforge_inspect",
    "hwpforge_to_json",
    "hwpforge_from_json",
    "hwpforge_patch",
    "hwpforge_outline",
    "hwpforge_diff",
    "hwpforge_delete_para",
    "hwpforge_insert_para",
    "hwpforge_read",
    "hwpforge_fields",
    "hwpforge_fill",
    "hwpforge_stamp_plan",
    "hwpforge_stamp",
    "hwpforge_set_cell",
    "hwpforge_validate",
    "hwpforge_restyle",
    "hwpforge_to_md",
];

/// The one tool without a warnings channel.
const NO_WARNING_TOOL: &str = "hwpforge_templates";

fn fixture(rel: &str) -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(rel)
        .to_str()
        .unwrap()
        .to_string()
}

/// Calls `name` and returns the success envelope `{data, summary, next}`.
async fn envelope(name: &str, arguments: Value) -> Value {
    let result = call(name, arguments).await.unwrap();
    let wire = serde_json::to_value(&result).unwrap();
    assert_ne!(result.is_error, Some(true), "{name} must succeed: {wire}");
    serde_json::from_str(wire["content"][0]["text"].as_str().unwrap()).unwrap()
}

/// Asserts one warning entry is a `{code, message}` object carrying `code`,
/// with no `hint` key (none of these warnings has a hint) and no code
/// prefix folded into the message (the old structural string shape).
fn assert_warning(entry: &Value, code: &str) -> String {
    let object = entry.as_object().unwrap_or_else(|| panic!("warning is not an object: {entry}"));
    assert_eq!(entry["code"], code, "{entry}");
    let message = entry["message"].as_str().unwrap();
    assert!(!message.is_empty(), "{entry}");
    assert!(!message.starts_with(code), "code must not be folded into the message: {entry}");
    assert_eq!(object.len(), 2, "hint: None must be omitted: {entry}");
    message.to_string()
}

fn clean_doc(dir: &tempfile::TempDir, name: &str, markdown: &str) -> String {
    let path = dir.path().join(name);
    crate::tools::convert::run_convert(markdown, false, path.to_str().unwrap(), "default").unwrap();
    path.to_str().unwrap().to_string()
}

fn out(dir: &tempfile::TempDir, name: &str) -> String {
    dir.path().join(name).to_str().unwrap().to_string()
}

#[test]
// 이것을 실패시키는 것: WARNING_TOOLS 에서 도구 하나를 빼거나 templates 를 경고 도구로 옮기는 것,
// 또는 router 에 분류되지 않은 도구를 더하는 것.
fn wm_w_every_routed_tool_is_classified() {
    use std::collections::BTreeSet;
    let routed: BTreeSet<String> = HwpForgeServer::tool_router()
        .list_all()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();
    let classified: BTreeSet<String> = WARNING_TOOLS
        .iter()
        .chain(std::iter::once(&NO_WARNING_TOOL))
        .map(|name| name.to_string())
        .collect();
    assert_eq!(classified.len(), 19, "a tool is listed twice");
    assert_eq!(routed, classified);
}

#[tokio::test]
// 이것을 실패시키는 것: 경고 DTO 하나에서 `skip_serializing_if = "Vec::is_empty"` 를 빼는 것,
// 또는 templates 결과에 경고 필드를 더하는 것.
async fn wm_w_clean_calls_omit_warnings_and_templates_has_none() {
    let dir = tempfile::tempdir().unwrap();
    let doc = clean_doc(&dir, "clean.hwpx", "첫째 문단.\n\n둘째 문단.\n\n셋째 문단.");
    let table = clean_doc(&dir, "table.hwpx", "| 성명 | 값 |\n|---|---|\n| 주소 | 값 |\n");
    let full_json =
        crate::tools::to_json::run_to_json(&doc, None, None).unwrap().json_content.unwrap();
    let section_json = out(&dir, "section.json");
    crate::tools::to_json::run_to_json(&doc, Some(0), Some(&section_json)).unwrap();

    let calls: Vec<(&str, Value)> = vec![
        (
            "hwpforge_convert",
            json!({"markdown": "# 제목", "is_file": false, "output_path": out(&dir, "c.hwpx")}),
        ),
        ("hwpforge_inspect", json!({"file_path": doc})),
        ("hwpforge_to_json", json!({"file_path": doc})),
        ("hwpforge_from_json", json!({"structure": full_json, "output_path": out(&dir, "f.hwpx")})),
        (
            "hwpforge_patch",
            json!({"base_path": doc, "section": 0, "section_json_path": section_json,
                   "output_path": out(&dir, "p.hwpx")}),
        ),
        ("hwpforge_outline", json!({"file_path": doc})),
        ("hwpforge_diff", json!({"base_path": doc, "revised_path": doc})),
        (
            "hwpforge_delete_para",
            json!({"file_path": doc, "section": 0, "indices": [1], "output_path": out(&dir, "d.hwpx")}),
        ),
        (
            "hwpforge_insert_para",
            json!({"file_path": doc, "section": 0, "anchor": 1, "text": "끼움",
                   "output_path": out(&dir, "i.hwpx")}),
        ),
        ("hwpforge_read", json!({"file_path": doc, "section": 0})),
        ("hwpforge_fields", json!({"file_path": doc})),
        (
            "hwpforge_fill",
            json!({"file_path": fixture("fields/clickhere_named.hwpx"),
                   "values": {"user_email": "a@b.c"}, "output_path": out(&dir, "fill.hwpx")}),
        ),
        ("hwpforge_stamp_plan", json!({"file_path": doc})),
        (
            "hwpforge_stamp",
            json!({"file_path": doc, "specs": [], "output_path": out(&dir, "s.hwpx")}),
        ),
        (
            "hwpforge_set_cell",
            json!({"file_path": table, "specs": [{"table": 0, "at": {"row": 1, "col": 1}, "text": "서울"}],
                   "output_path": out(&dir, "cell.hwpx")}),
        ),
        ("hwpforge_validate", json!({"file_path": doc})),
        (
            "hwpforge_restyle",
            json!({"file_path": doc, "preset": "modern", "output_path": out(&dir, "r.hwpx")}),
        ),
        ("hwpforge_to_md", json!({"file_path": doc, "output_dir": out(&dir, "md")})),
    ];
    let called: Vec<&str> = calls.iter().map(|(name, _)| *name).collect();
    assert_eq!(called, WARNING_TOOLS, "every warning tool gets one clean call");

    for (name, args) in calls {
        let envelope = envelope(name, args).await;
        assert!(envelope["data"].is_object(), "{name}: {envelope}");
        assert!(envelope["data"].get("warnings").is_none(), "{name}: {envelope}");
    }

    let templates = envelope(NO_WARNING_TOOL, json!({})).await;
    assert!(templates["data"].is_object(), "{templates}");
    assert!(templates["data"].get("warnings").is_none(), "{templates}");
}

#[tokio::test]
// 이것을 실패시키는 것: `ConvertData::warnings` 를 `Vec<String>` 으로 되돌리는 것,
// summary 를 message 대신 code 로 잇는 것.
async fn wm_w_convert_reports_warning_objects_and_summarizes_messages() {
    let dir = tempfile::tempdir().unwrap();
    let envelope = envelope(
        "hwpforge_convert",
        json!({"markdown": "본문\n\n![그림](missing.png)\n", "is_file": false,
               "output_path": out(&dir, "c.hwpx")}),
    )
    .await;
    let warnings = envelope["data"]["warnings"].as_array().expect("warnings array");
    assert_eq!(warnings.len(), 1, "{envelope}");
    let message = assert_warning(&warnings[0], "IMAGE_EMBED_SKIPPED");
    let summary = envelope["summary"].as_str().unwrap();
    assert!(summary.ends_with(&format!(" — 1 warning(s): {message}")), "{summary}");
}

#[tokio::test]
// 이것을 실패시키는 것: `FromJsonData::warnings` 를 `Vec<String>` 으로 되돌리는 것.
async fn wm_w_from_json_reports_warning_objects() {
    let dir = tempfile::tempdir().unwrap();
    let structure = crate::tools::to_json::run_to_json(&fixture("shapes/rect.hwpx"), None, None)
        .unwrap()
        .json_content
        .unwrap();
    let envelope = envelope(
        "hwpforge_from_json",
        json!({"structure": structure, "output_path": out(&dir, "f.hwpx")}),
    )
    .await;
    let warnings = envelope["data"]["warnings"].as_array().expect("warnings array");
    assert!(!warnings.is_empty(), "{envelope}");
    for entry in warnings {
        assert_warning(entry, "LAYOUT_CACHE_DROPPED");
    }
}

#[tokio::test]
// 이것을 실패시키는 것: `RestyleData::warnings` 를 `Vec<String>` 으로 되돌리는 것.
async fn wm_w_restyle_reports_warning_objects() {
    let dir = tempfile::tempdir().unwrap();
    let envelope = envelope(
        "hwpforge_restyle",
        json!({"file_path": fixture("layout/stale-line-cache.hwpx"), "preset": "modern",
               "output_path": out(&dir, "r.hwpx")}),
    )
    .await;
    let warnings = envelope["data"]["warnings"].as_array().expect("warnings array");
    assert_eq!(warnings.len(), 1, "{envelope}");
    assert_warning(&warnings[0], "LAYOUT_CACHE_DROPPED");
}

#[tokio::test]
// 이것을 실패시키는 것: `StructuralData::warnings` 를 `Vec<String>` 으로 되돌리거나
// 옛 "CODE: message" 문자열을 message 에 남기는 것, 디코드·조언 순서를 바꾸는 것.
async fn wm_w_structural_tools_report_ordered_warning_objects() {
    let dir = tempfile::tempdir().unwrap();
    let stale = fixture("layout/stale-line-cache.hwpx");

    let deleted = envelope(
        "hwpforge_delete_para",
        json!({"file_path": stale, "section": 0, "indices": [2], "output_path": out(&dir, "d.hwpx")}),
    )
    .await;
    let warnings = deleted["data"]["warnings"].as_array().expect("warnings array");
    assert_eq!(warnings.len(), 2, "{deleted}");
    assert_warning(&warnings[0], "LAYOUT_CACHE_DROPPED");
    let advisory = assert_warning(&warnings[1], "INDEX_MARK_REMOVED");
    assert!(advisory.contains("index-mark"), "{advisory}");

    let inserted = envelope(
        "hwpforge_insert_para",
        json!({"file_path": stale, "section": 0, "anchor": 2, "text": "끼\u{1}움",
               "output_path": out(&dir, "i.hwpx")}),
    )
    .await;
    let warnings = inserted["data"]["warnings"].as_array().expect("warnings array");
    assert_eq!(warnings.len(), 2, "{inserted}");
    assert_warning(&warnings[0], "LAYOUT_CACHE_DROPPED");
    assert_warning(&warnings[1], "XML_FORBIDDEN_CHARS_REMOVED");
}
