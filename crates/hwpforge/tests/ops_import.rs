#![cfg(feature = "ops-docx")]

use hwpforge::ops::{convert_docx, ImportMeta, OpsError};
use serde_json::json;

#[path = "support/docx.rs"]
mod docx;

// 이것을 실패시키는 것: DOCX 대신 HWPX 디코더 호출, 본문/개수 변경, 미해결 서식 경고 제거 또는 meta count를 1로 초기화.
// 알려진 글꼴의 한 줄 환산을 무시하고 경고를 무조건 100%라고 표기하는 경우도 실패한다.
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
    // Two paragraphs with one run each leave six categories unresolved twice.
    let mut warnings: Vec<_> = [
        ("line_spacing", "pPr", "Unresolved paragraph formatting uses single-line spacing or zero length (total 2 occurrences; first example only; subsequent locations omitted)"),
        ("paragraph_spacing", "pPr", "Unresolved paragraph formatting uses single-line spacing or zero length (total 2 occurrences; first example only; subsequent locations omitted)"),
        ("indentation", "pPr", "Unresolved paragraph formatting uses single-line spacing or zero length (total 2 occurrences; first example only; subsequent locations omitted)"),
        ("font_size", "rPr", "Unresolved character formatting uses 10pt, 함초롬바탕 or black (total 2 occurrences; first example only; subsequent locations omitted)"),
        ("font_family", "rPr", "Unresolved character formatting uses 10pt, 함초롬바탕 or black (total 2 occurrences; first example only; subsequent locations omitted)"),
        ("text_color", "rPr", "Unresolved character formatting uses 10pt, 함초롬바탕 or black (total 2 occurrences; first example only; subsequent locations omitted)"),
    ].into_iter().map(|(loss, location, message)| json!({
        "code":"DOCX_FORMAT_DEFAULTED", "message":message,
        "context":{"format":"docx","stage":"decode","part":"body.xml","location":location,"loss":loss,"action":"use_fallback","count":2}
    })).collect();
    warnings.extend(page_default_warnings());
    assert_eq!(first, json!({"sections":1,"paragraphs":2,"warnings":warnings}));
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
        let output = convert_docx(&docx::package_with_defaults(
            "<w:p><w:r><w:t>가</w:t></w:r></w:p>",
            &relationships,
        ))
        .unwrap();
        let meta = serde_json::to_value(output.meta()).unwrap();
        let mut context = json!({"format":"docx","stage":"decode","part":"opaque.xml","location":"relationship:opaque0","loss":"uninterpreted_part","action":"omit"});
        if count > 1 {
            context["count"] = json!(20_000);
        }
        assert_eq!(meta["warnings"].as_array().unwrap().len(), 3);
        assert_eq!(&meta["warnings"].as_array().unwrap()[1..], &page_default_warnings());
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
    let mut output = convert_docx(&docx::package_with_defaults(
        "<w:p><w:r><w:t>가</w:t></w:r></w:p>",
        "<Relationship Id='opaque0' Type='urn:test:opaque' Target='opaque.xml'/>",
    ))
    .unwrap();
    let OpsWarning::Docx(DocxWarning::OptionalPartOmitted { part, location, count, .. }) =
        &mut output.warnings[0]
    else {
        panic!("uninterpreted relationship warning");
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
        json!({"format":"docx","stage":"decode","loss":"uninterpreted_part","action":"omit","count":20_000})
    );
    let codes: Vec<_> =
        meta["warnings"].as_array().unwrap().iter().map(|w| w["code"].as_str().unwrap()).collect();
    assert_eq!(
        codes,
        [
            "DOCX_OPTIONAL_PART_OMITTED",
            "DOCX_PAGE_LAYOUT_APPROXIMATED",
            "DOCX_PAGE_LAYOUT_APPROXIMATED",
            "FONT_FALLBACK",
            "XML_FORBIDDEN_CHARS_REMOVED"
        ]
    );
    assert_eq!(meta["warnings"][3]["context"]["stage"], "project");
    assert_eq!(meta["warnings"][3]["context"]["count"], 2);
    assert_eq!(
        meta["warnings"][4],
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
    let output = convert_docx(&docx::package_with_defaults(
        "<w:p><w:hyperlink w:anchor='local'><w:r><w:t>가</w:t></w:r></w:hyperlink><w:fldSimple w:instr='DATE'><w:r><w:t>나</w:t></w:r></w:fldSimple></w:p>",
        "<Relationship Id='opaque0' Type='urn:test:opaque' Target='opaque.xml'/><Relationship Id='missing' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/header' Target='missing.xml'/><Relationship Id='opaque1' Type='urn:test:opaque' Target='other.xml'/>",
    )).unwrap();
    let warnings = output.meta().warnings;
    let actual: Vec<_> = warnings
        .iter()
        .map(|w| {
            let c = w.context.as_ref().unwrap();
            (
                w.code.as_str(),
                c.loss.as_str(),
                c.action.as_str(),
                c.part.as_deref(),
                c.location.as_deref(),
                c.count,
            )
        })
        .collect();
    assert_eq!(
        actual,
        [
            (
                "DOCX_OPTIONAL_PART_OMITTED",
                "uninterpreted_part",
                "omit",
                Some("opaque.xml"),
                Some("relationship:opaque0"),
                2
            ),
            (
                "DOCX_OPTIONAL_PART_OMITTED",
                "optional_part",
                "omit",
                Some("missing.xml"),
                Some("relationship:missing"),
                1
            ),
            (
                "DOCX_LINK_TARGET_OMITTED",
                "hyperlink_target",
                "keep_text",
                Some("body.xml"),
                Some("hyperlink#4"),
                1
            ),
            (
                "DOCX_FIELD_INSTRUCTION_OMITTED",
                "field_function",
                "use_saved_result",
                Some("body.xml"),
                Some("fldSimple#7"),
                1
            ),
            (
                "DOCX_PAGE_LAYOUT_APPROXIMATED",
                "page_size",
                "use_defaults",
                Some("body.xml"),
                Some("body#0"),
                1
            ),
            (
                "DOCX_PAGE_LAYOUT_APPROXIMATED",
                "page_margins",
                "use_defaults",
                Some("body.xml"),
                Some("body#0"),
                1
            ),
        ]
    );
}

// 이것을 실패시키는 것: 공유 후반에서 구역·빈 조각·쪽 나누기 삭제 또는 경고 위치/count 재작성.
#[test]
fn sections_fragments_and_geometry_survive_shared_encoding() {
    let output = convert_docx(&docx::package_with_defaults(
        "<w:p><w:pPr><w:sectPr><w:pgSz w:w='15840' w:h='12240' w:orient='landscape'/></w:sectPr></w:pPr><w:r><w:t>A</w:t><w:br w:type='page'/><w:t>B</w:t><w:br w:type='column'/></w:r></w:p><w:sectPr><w:pgSz w:w='12240' w:h='15840' w:orient='portrait'/></w:sectPr>", "",
    )).unwrap();
    let meta = output.meta();
    assert_eq!((meta.sections, meta.paragraphs), (2, 4));
    let warnings: Vec<_> = meta
        .warnings
        .iter()
        .map(|w| {
            let c = w.context.as_ref().unwrap();
            (w.code.as_str(), c.location.as_deref(), c.loss.as_str(), c.action.as_str(), c.count)
        })
        .collect();
    assert_eq!(
        warnings,
        [
            ("DOCX_PARAGRAPH_SPLIT_APPROXIMATED", Some("br#9"), "paragraph_split", "split", 1),
            ("DOCX_COLUMN_BREAK_APPROXIMATED", Some("br#11"), "column_break", "page_break", 1),
            ("DOCX_PAGE_LAYOUT_APPROXIMATED", Some("sectPr#5"), "page_margins", "use_defaults", 2),
        ]
    );
    let decoded = hwpforge::hwpx::HwpxDecoder::decode(&output.bytes).unwrap();
    let sections = decoded.document.sections();
    assert_eq!(
        sections
            .iter()
            .map(|s| s.paragraphs.iter().map(|p| p.text_content()).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        [vec!["A", "B", ""], vec![""]]
    );
    assert_eq!(
        sections[0].paragraphs.iter().map(|p| p.page_break).collect::<Vec<_>>(),
        [false, true, true]
    );
    assert_eq!(
        sections
            .iter()
            .map(|s| (
                s.page_settings.width.as_i32(),
                s.page_settings.height.as_i32(),
                s.page_settings.landscape
            ))
            .collect::<Vec<_>>(),
        [(61200, 79200, true), (61200, 79200, false)]
    );
}

// 이것을 실패시키는 것: 기본 쪽 경고의 loss/action/첫 위치/count를 바꾸거나 삭제.
fn page_default_warnings() -> [serde_json::Value; 2] {
    ["page_size", "page_margins"].map(|loss| json!({
        "code":"DOCX_PAGE_LAYOUT_APPROXIMATED",
        "message":"Missing section page setting uses Core defaults",
        "context":{"format":"docx","stage":"decode","part":"body.xml","location":"body#0","loss":loss,"action":"use_defaults"}
    }))
}

// 이것을 실패시키는 것: DOCX 물리 줄 높이를 HwpUnit으로 전달해 HWPX case에서 다시 반감.
#[test]
fn physical_line_spacing_survives_hwpx_encoding() {
    for (rule, line, expected) in [("exact", 400, 4000), ("atLeast", 360, 3804)] {
        let output = convert_docx(&docx::package(&format!("<w:p><w:pPr><w:spacing w:line='{line}' w:lineRule='{rule}'/></w:pPr><w:r><w:rPr><w:rFonts w:ascii='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr><w:t>한글</w:t></w:r></w:p>"))).unwrap();
        let decoded = hwpforge::hwpx::HwpxDecoder::decode(&output.bytes).unwrap();
        let p = &decoded.document.sections()[0].paragraphs[0];
        let shape = decoded.style_store.para_shape(p.para_shape_id).unwrap();
        assert_eq!(shape.line_spacing, expected);
    }
}
