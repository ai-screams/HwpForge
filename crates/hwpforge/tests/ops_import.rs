#![cfg(feature = "ops-docx")]

use hwpforge::ops::{convert_docx, ImportMeta, OpsError};
use serde_json::json;

#[path = "support/docx.rs"]
mod docx;

// 이것을 실패시키는 것: DOCX 대신 HWPX 디코더 호출, 본문/개수 변경, meta 재집계.
#[test]
fn imported_text_and_metadata_survive_encoding() {
    let output = convert_docx(&docx::package(
        "<w:p><w:r><w:t>첫째 &amp; 본문</w:t></w:r></w:p><w:p><w:r><w:t>둘째</w:t></w:r></w:p>",
    ))
    .unwrap();
    let decoded = hwpforge::hwpx::HwpxDecoder::decode(&output.bytes).unwrap();
    let text: Vec<_> =
        decoded.document.sections()[0].paragraphs.iter().map(|p| p.text_content()).collect();
    assert_eq!(text, ["첫째 & 본문", "둘째"]);
    let first = serde_json::to_value(output.meta()).unwrap();
    assert_eq!(first, json!({"sections":1,"paragraphs":2,"warnings":[]}));
    assert_eq!(serde_json::to_value(output.meta()).unwrap(), first);
    let restored: ImportMeta = serde_json::from_value(first.clone()).unwrap();
    assert_eq!(serde_json::to_value(restored).unwrap(), first);
}

// 이것을 실패시키는 것: 미해석 관계 context 누락/분류 변경, count=1 방출, 20,000을 1로 초기화.
#[test]
fn decoder_counts_and_first_location_are_copied_once() {
    for count in [1, 20_000] {
        let relationships: String = (0..count)
            .map(|id| {
                format!(
                    "<Relationship Id='opaque{id}' Type='urn:test:opaque' Target='opaque.xml'/>"
                )
            })
            .collect();
        let output = convert_docx(&docx::package_with_relationships(
            "<w:p><w:r><w:t>가</w:t></w:r></w:p>",
            &relationships,
        ))
        .unwrap();
        let meta = serde_json::to_value(output.meta()).unwrap();
        let mut context = json!({"format":"docx","stage":"decode","part":"opaque.xml","location":"relationship:opaque0","loss":"uninterpreted_part","action":"omit"});
        if count > 1 {
            context["count"] = json!(20_000);
        }
        assert_eq!(meta["warnings"].as_array().unwrap().len(), 1);
        assert_eq!(meta["warnings"][0]["code"], "DOCX_OPTIONAL_PART_OMITTED");
        assert_eq!(meta["warnings"][0]["context"], context);
        assert_eq!(serde_json::to_value(output.meta()).unwrap(), meta);
        if count == 1 {
            let expected: serde_json::Value = serde_json::from_str(include_str!(
                "../../hwpforge-bindings-py/tests/data/import-report.json"
            ))
            .unwrap();
            assert_eq!(meta, expected);
        }
    }
}

// 이것을 실패시키는 것: 오류를 문자열 오류로 교체하여 원본 variant/위치/안내 소실.
#[test]
fn tracked_changes_preserve_the_original_error_and_guidance() {
    let err = convert_docx(&docx::package("<w:p><w:ins><w:r><w:t>거부</w:t></w:r></w:ins></w:p>"))
        .unwrap_err();
    assert_eq!(err.code().as_str(), "DOCX_DECODE_FAILED");
    assert_eq!(err.to_string(), "DOCX part 'body.xml' at ins#4: tracked change 'ins'; accept all changes in Word or LibreOffice and retry");
    assert!(matches!(err, OpsError::DocxDecode(hwpforge::docx::DocxError::TrackedChange { .. })));
    assert_eq!(convert_docx(b"not a ZIP").unwrap_err().code().as_str(), "DOCX_DECODE_FAILED");
}

// 이것을 실패시키는 것: ImportMeta의 schema derive 또는 warnings/context 참조 제거.
#[cfg(feature = "schemars")]
#[test]
fn import_schema_contains_the_context_contract() {
    let schema = serde_json::to_value(schemars::schema_for!(ImportMeta)).unwrap();
    assert_eq!(schema["required"], json!(["sections", "paragraphs", "warnings"]));
    assert_eq!(schema["$defs"]["WarningContext"]["properties"]["count"]["minimum"], 1);
    assert_eq!(schema["properties"]["warnings"]["items"]["$ref"], "#/$defs/WarningInfo");
}
// 이것을 실패시키는 것: projection의 format/stage/part/위치/loss/action/count 변경.
#[test]
fn projection_context_copies_the_bridge_without_inventing_a_part() {
    use hwpforge::hwpx::RegistryBridgeWarning;
    use hwpforge::ops::OpsWarning;
    for (slot, loss) in
        [("font", "font"), ("latin", "font_latin"), ("east_asian", "font_east_asian")]
    {
        for count in [1, 20_000] {
            let warning = RegistryBridgeWarning::FontFallback {
                requested_font: "Missing".into(),
                slot,
                char_shape: 7,
                count,
            };
            let mut context = json!({"format":"hwpx","stage":"project","location":format!("char_shapes[7].{slot}"),"loss":loss,"action":"use_first_font"});
            if count > 1 {
                context["count"] = json!(20_000);
            }
            let info = OpsWarning::Projection(warning).info();
            assert_eq!(info.code, "FONT_FALLBACK");
            assert_eq!(serde_json::to_value(info.context).unwrap(), context);
            assert_eq!(
                info.message,
                if count == 1 {
                    "Requested font 'Missing' is absent; using the first registered font"
                } else {
                    "Requested font 'Missing' is absent; using the first registered font (total 20000 occurrences; first example only; subsequent locations omitted)"
                }
            );
        }
    }
}

// 이것을 실패시키는 것: None 위치 추측, 원본 count 변경, meta에서 경고 정렬/합산.
#[test]
fn unknown_source_positions_and_stage_order_survive_metadata() {
    use hwpforge::docx::DocxWarning;
    use hwpforge::hwpx::{EncodeWarning, RegistryBridgeWarning};
    use hwpforge::ops::OpsWarning;
    let mut output =
        convert_docx(&docx::package("<w:p><w:r><w:br w:type='page'/><w:t>가</w:t></w:r></w:p>"))
            .unwrap();
    let OpsWarning::Docx(DocxWarning::BreakOmitted { part, location, count, .. }) =
        &mut output.warnings[0]
    else {
        panic!("break warning");
    };
    *part = None;
    *location = None;
    *count = 20_000;
    output.warnings.push(OpsWarning::Projection(RegistryBridgeWarning::FontFallback {
        requested_font: "Absent".into(),
        slot: "font",
        char_shape: 0,
        count: 2,
    }));
    output.warnings.push(OpsWarning::Encode(EncodeWarning::XmlForbiddenCharsRemoved {
        part: "Contents/section0.xml".into(),
        location: "hp:t".into(),
        count: 1,
    }));
    let meta = serde_json::to_value(output.meta()).unwrap();
    assert_eq!(
        meta["warnings"][0]["context"],
        json!({"format":"docx","stage":"decode","loss":"page_break","action":"omit","count":20_000})
    );
    let codes: Vec<_> =
        meta["warnings"].as_array().unwrap().iter().map(|w| w["code"].as_str().unwrap()).collect();
    assert_eq!(codes, ["DOCX_BREAK_OMITTED", "FONT_FALLBACK", "XML_FORBIDDEN_CHARS_REMOVED"]);
    assert_eq!(meta["warnings"][1]["context"]["stage"], "project");
    assert_eq!(meta["warnings"][1]["context"]["count"], 2);
    assert_eq!(
        meta["warnings"][2],
        json!({"code":"XML_FORBIDDEN_CHARS_REMOVED","message":"removed 1 XML-forbidden character(s) from hp:t in Contents/section0.xml"})
    );
    assert_eq!(serde_json::to_value(output.meta()).unwrap(), meta);
}
// 이것을 실패시키는 것: docx 호출이 Template 정책으로 바뀌어 제목 번호가 삽입됨.
#[test]
fn import_uses_an_empty_outline_head() {
    let output = convert_docx(&docx::package("<w:p><w:r><w:t>제목</w:t></w:r></w:p>")).unwrap();
    let decoded = hwpforge::hwpx::HwpxDecoder::decode(&output.bytes).unwrap();
    assert_eq!(decoded.style_store.iter_numberings().next().unwrap().levels[0].text, "");
}

// 이것을 실패시키는 것: decoder 경고를 역순으로 연결하거나 같은 코드의 다른 loss를 합산.
#[test]
fn distinct_decoder_losses_keep_first_occurrence_order() {
    let output = convert_docx(&docx::package(
        "<w:p><w:r><w:br w:type='page'/><w:br w:type='column'/><w:t>가</w:t></w:r></w:p>",
    ))
    .unwrap();
    let warnings = output.meta().warnings;
    let actual: Vec<_> = warnings
        .iter()
        .map(|w| {
            let c = w.context.as_ref().unwrap();
            (w.code.as_str(), c.loss.as_str(), c.location.as_deref(), c.count)
        })
        .collect();
    assert_eq!(
        actual,
        [
            ("DOCX_BREAK_OMITTED", "page_break", Some("br#5"), 1),
            ("DOCX_BREAK_OMITTED", "column_break", Some("br#6"), 1),
        ]
    );
}
