#![cfg(feature = "ops-docx")]
use hwpforge::hwpx::{
    EncodeOptions, EncodeWarning, HancomStyleSet, HwpxDecoder, HwpxEncoder, HwpxRegistryBridge,
    ParagraphPath, PathSeg,
};
use hwpforge::ops::{convert_docx, OpsWarning};
#[path = "support/docx.rs"]
mod docx;
#[path = "../../hwpforge-smithy-hwpx/tests/support/xml.rs"]
mod xml;
fn tree(bytes: &[u8], part: &str) -> xml::Element {
    use std::io::Read;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut x = String::new();
    z.by_name(part).unwrap().read_to_string(&mut x).unwrap();
    xml::Element::parse(&x)
}
fn margins(bytes: &[u8]) -> Vec<[i32; 4]> {
    tree(bytes, "Contents/section0.xml")
        .descendants("hp:tc")
        .iter()
        .map(|c| {
            ["left", "right", "top", "bottom"]
                .map(|k| c.child("hp:cellMargin").attr(k).unwrap().parse().unwrap())
        })
        .collect()
}
// 이것을 실패시키는 것: import 전용 bridge 대신 일반 bridge 사용, 보류 폭 정의 합침, decode/JSON 재가산.
#[test]
fn imported_exact_and_admitted_tables_have_distinct_definitions_and_lifetimes() {
    let input = docx::package_with_defaults(
        &format!("{}{}", docx::border_space_table(4, true), docx::border_space_table(4, false)),
        "",
    );
    let out = convert_docx(&input).unwrap();
    assert_eq!(margins(&out.bytes), [[540, 540, 0, 0], [565, 515, 50, 50]]);
    assert_eq!(tree(&out.bytes, "Contents/header.xml").descendants("hh:borderFill").len(), 5);
    let warnings: Vec<_> = out
        .meta()
        .warnings
        .into_iter()
        .filter(|w| w.context.as_ref().is_some_and(|c| c.loss == "table_border_space"))
        .collect();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].code, "DOCX_TABLE_BORDER_APPROXIMATED");
    let mut bytes = out.bytes;
    for _ in 0..3 {
        let d = HwpxDecoder::decode(&bytes).unwrap();
        let json = serde_json::to_value(&d.style_store).unwrap();
        let s = serde_json::from_value(json).unwrap();
        bytes = HwpxEncoder::encode(&d.document.validate().unwrap(), &s, &d.image_store).unwrap();
        assert_eq!(margins(&bytes), [[540, 540, 0, 0], [565, 515, 50, 50]]);
    }
    let decoded = hwpforge_smithy_docx::decode_docx(&input).unwrap();
    let registry =
        serde_json::from_value(serde_json::to_value(&decoded.registry).unwrap()).unwrap();
    let b = HwpxRegistryBridge::from_registry_for_import(&registry, HancomStyleSet::Modern)
        .unwrap()
        .value;
    let d = b.rebind_draft_document(decoded.document).unwrap().validate().unwrap();
    assert_eq!(
        margins(
            &HwpxEncoder::encode_with_diagnostics(
                &d,
                b.style_store(),
                &decoded.image_store,
                EncodeOptions::default()
            )
            .unwrap()
            .bytes
        ),
        [[540, 540, 0, 0], [565, 515, 50, 50]]
    );
}
// 이것을 실패시키는 것: typed path/count 전달 대신 Display 파싱, 알 수 없는 사유를 project로 처리, 기존 경고 context 변경.
#[test]
fn approximation_context_is_derived_from_typed_fields_for_every_reason() {
    for reason in [
        "outside_scope",
        "size_protected",
        "nonzero_cell_spacing",
        "cell_zones",
        "incomplete_widths",
        "inconsistent_edges",
        "missing_margins",
        "unverified_mixed_widths",
        "later_left_wider",
        "negative_right_margin",
        "unverified_uniform_width",
        "unknown",
    ] {
        for count in [1, 2] {
            let w = EncodeWarning::TableBorderSpaceApproximated {
                path: ParagraphPath(vec![
                    PathSeg::Section(0),
                    PathSeg::BodyParagraph(2),
                    PathSeg::Run(1),
                ]),
                reason,
                count,
            };
            assert!(!w.is_semantic_loss());
            assert_eq!(w.part(), None);
            assert_eq!(w.location().as_deref(), Some("section[0].para[2].run[1]"));
            let value = serde_json::to_value(OpsWarning::Encode(w.clone()).info()).unwrap();
            let mut expected = serde_json::json!({"code":"TABLE_BORDER_SPACE_APPROXIMATED","message":w.to_string(),"context":{"format":"hwpx","stage":"encode","location":"section[0].para[2].run[1]","loss":"table_border_space","action":if reason=="unverified_uniform_width" {"project_unverified"}else{"retain_margins"}}});
            if count == 2 {
                expected["context"]["count"] = 2.into();
            }
            assert_eq!(value, expected);
        }
    }
    let old = OpsWarning::Encode(EncodeWarning::LayoutCacheDropped {
        path: ParagraphPath(vec![PathSeg::Section(0)]),
        reason: "old".into(),
    })
    .info();
    assert_eq!(
        serde_json::to_value(old).unwrap(),
        serde_json::json!({"code":"LAYOUT_CACHE_DROPPED","message":"layout cache dropped at section[0]: old"})
    );
}
// 이것을 실패시키는 것: 경고를 표별 객체로 분리하거나 첫 위치를 마지막 위치로 덮어쓰기.
#[test]
fn unmeasured_widths_are_aggregated_once_per_document() {
    let input = docx::package_with_defaults(&docx::border_space_table(3, false).repeat(2), "");
    let out = convert_docx(&input).unwrap();
    assert_eq!(margins(&out.bytes), [[559, 521, 38, 38], [559, 521, 38, 38]]);
    let warnings: Vec<_> = out
        .meta()
        .warnings
        .into_iter()
        .filter(|w| w.code == "TABLE_BORDER_SPACE_APPROXIMATED")
        .collect();
    assert_eq!(warnings.len(), 1);
    let context = warnings[0].context.as_ref().unwrap();
    assert_eq!(context.count, 2);
    assert_eq!(context.location.as_deref(), Some("section[0].para[0].run[0]"));
    assert_eq!(context.action, "project_unverified");
}

// 이것을 실패시키는 것: patch/stamp/fill이 출력 여백에 테두리 몫을 다시 더함 또는 편집 텍스트 소실.
#[test]
fn patch_stamp_and_fill_keep_the_projected_output_margins() {
    use hwpforge::hwpx::{
        stamp::{HwpxStamper, StampAction, StampSpec},
        HwpxFiller, HwpxPatcher,
    };
    use std::collections::BTreeMap;
    let source = docx::border_space_table(4, false).replace("ASCII", "□");
    let imported = convert_docx(&docx::package_with_defaults(&source, "")).unwrap();
    let mut decoded = HwpxDecoder::decode(&imported.bytes).unwrap();
    let preservation = HwpxPatcher::export_section_preservation(
        &imported.bytes,
        0,
        &decoded.document.sections()[0],
    )
    .unwrap();
    decoded.document.for_each_table_mut(|t| {
        t.rows[0].cells[0].paragraphs[0].runs[0].content =
            hwpforge::core::RunContent::Text("□ edited".into())
    });
    let patched = HwpxPatcher::patch_section_preserving(
        &imported.bytes,
        0,
        &decoded.document.sections()[0],
        None,
        Some(&preservation),
    )
    .unwrap();
    assert_eq!(margins(&patched), [[565, 515, 50, 50]]);
    let candidates = HwpxStamper::plan_bytes(&patched).unwrap();
    assert_eq!(candidates.len(), 1);
    let c = &candidates[0];
    let stamped = HwpxStamper::stamp(
        &patched,
        &[StampSpec {
            section: c.section,
            path: c.path.clone(),
            span: c.span.clone(),
            marker: c.marker.clone(),
            action: StampAction::Field { name: "cell_value".into(), hint: None },
        }],
    )
    .unwrap();
    assert_eq!(margins(&stamped.bytes), [[565, 515, 50, 50]]);
    let filled =
        HwpxFiller::fill(&stamped.bytes, &BTreeMap::from([("cell_value".into(), "filled".into())]))
            .unwrap();
    assert_eq!(margins(&filled.bytes), [[565, 515, 50, 50]]);
    let decoded = HwpxDecoder::decode(&filled.bytes).unwrap();
    let mut text = String::new();
    decoded.document.for_each_table(|t| text = t.rows[0].cells[0].paragraphs[0].text_content());
    assert_eq!(text, " edited");
    assert_eq!(HwpxFiller::list_fields(&filled.bytes).unwrap()[0].current, "filled");
}

// 이것을 실패시키는 것: exact 원본 판정을 자식 표에 전파하거나 바깥 표에 자식 공간을 재가산.
#[test]
fn exact_outer_tables_keep_nested_admission_independent() {
    let inner = docx::border_space_table(4, false);
    let outer = docx::border_space_table(4, true).replace("</w:tc>", &format!("{inner}</w:tc>"));
    let out = convert_docx(&docx::package_with_defaults(&outer, "")).unwrap();
    assert_eq!(margins(&out.bytes), [[540, 540, 0, 0], [565, 515, 50, 50]]);
    let warnings: Vec<_> = out
        .meta()
        .warnings
        .into_iter()
        .filter(|w| w.context.as_ref().is_some_and(|c| c.loss == "table_border_space"))
        .collect();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].context.as_ref().unwrap().count, 1);
    assert_eq!(warnings[0].code, "DOCX_TABLE_BORDER_APPROXIMATED");
}

// 이것을 실패시키는 것: 행 식별자로 폭을 버려 기존 테두리 여백 투영이 사라짐.
#[test]
fn row_identity_preserves_imported_border_space_projection() {
    let body = docx::border_space_table(4, false).replace(
        "<w:tr>",
        "<w:tr xmlns:w14='http://schemas.microsoft.com/office/word/2010/wordml' w14:paraId='12345678' w14:textId='87654321'>",
    );
    assert!(body.contains("w14:paraId"));
    let out = convert_docx(&docx::package_with_defaults(&body, "")).unwrap();
    assert_eq!(margins(&out.bytes), [[565, 515, 50, 50]]);
    assert!(!out
        .meta()
        .warnings
        .iter()
        .any(|w| { w.context.as_ref().is_some_and(|c| c.loss == "table_border_space") }));
}
