#![cfg(feature = "ops-hwpx")]
#[path = "support/ops_json_layout.rs"]
mod shared;
use shared::*;

use hwpforge::core::image::ImageStore;
use hwpforge::core::{Document, PageSettings, Paragraph, Run, Section};
use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge::hwpx::{style_store_for_preset, EncodeOptions, HwpxDecoder, HwpxEncoder};
use hwpforge::ops::{from_json, to_json, FromJsonOptions, ToJsonOptions};
use serde_json::{json, Value};
use std::io::{Cursor, Read};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../../tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn cell_paragraphs(value: &mut Value) -> &mut Vec<Value> {
    value["document"]["sections"][0]["paragraphs"][1]["runs"][1]["content"]["Table"]["rows"][0]
        ["cells"][0]["paragraphs"]
        .as_array_mut()
        .unwrap()
}

// 이것을 실패시키는 것: host 사슬을 끊거나 다른 셀의 독립 흐름까지 무효화하기.
#[test]
fn nested_edits_and_deletions_invalidate_the_host_chain_but_not_sibling_cells() {
    use hwpforge::core::RunContent;
    let base = nested_base();
    for delete in [false, true] {
        let mut value = export(&base);
        let ps = cell_paragraphs(&mut value);
        if delete {
            ps.pop();
        } else {
            ps[1]["runs"][0]["content"]["Text"] = json!("changed");
        }
        let out = roundtrip(&base, &value);
        assert_eq!(flags(&out.bytes), [true, false, false]);
        let d = HwpxDecoder::decode(&out.bytes).unwrap();
        let s = &d.document.sections()[0];
        let RunContent::Table(t) = &s.paragraphs[1].runs[1].content else { panic!("table") };
        assert!(t.rows[0].cells[1].paragraphs.iter().all(|p| p.layout_cache.is_some()));
        assert!(t.rows[0].cells[0].paragraphs[0].layout_cache.is_some());
        assert_eq!(t.rows[0].cells[0].paragraphs[1].layout_cache.is_some(), delete);
        assert!(s.headers[0].paragraphs.iter().all(|p| p.layout_cache.is_some()));
        assert!(s.master_pages.as_ref().unwrap()[0]
            .paragraphs
            .iter()
            .all(|p| p.layout_cache.is_some()));
    }
}

// 이것을 실패시키는 것: 머리말·바탕쪽을 본문 host 사슬에 연결하거나 바탕쪽 carry를 누락하기.
#[test]
fn header_and_master_page_flows_are_independent_of_the_body() {
    let base = nested_base();
    for key in ["headers", "master_pages"] {
        let mut value = export(&base);
        value["document"]["sections"][0][key][0]["paragraphs"][1]["runs"][0]["content"]["Text"] =
            json!("edited");
        let out = roundtrip(&base, &value);
        assert_eq!(flags(&out.bytes), [true, true, true]);
        let d = HwpxDecoder::decode(&out.bytes).unwrap();
        let s = &d.document.sections()[0];
        let changed = if key == "headers" {
            &s.headers[0].paragraphs
        } else {
            &s.master_pages.as_ref().unwrap()[0].paragraphs
        };
        assert!(changed[0].layout_cache.is_some());
        assert!(changed[1].layout_cache.is_none());
        let untouched = if key == "headers" {
            &s.master_pages.as_ref().unwrap()[0].paragraphs
        } else {
            &s.headers[0].paragraphs
        };
        assert!(untouched.iter().all(|p| p.layout_cache.is_some()));
    }
}

// 이것을 실패시키는 것: 셀 너비 변경에도 하위 문단의 기존 좌표를 유지하기.
#[test]
fn container_geometry_changes_invalidate_its_nested_caches() {
    let base = nested_base();
    let mut value = export(&base);
    let cell = &mut value["document"]["sections"][0]["paragraphs"][1]["runs"][1]["content"]
        ["Table"]["rows"][0]["cells"][0];
    cell["width"] = json!(8000);
    let out = roundtrip(&base, &value);
    assert_eq!(flags(&out.bytes), [true, false, false]);
    assert!(cell_paragraphs(&mut export(&out.bytes))
        .iter()
        .all(|p| p.get("layout_cache").is_none()));
}

// 이것을 실패시키는 것: host 자체의 텍스트 편집을 자식 목록의 편집으로도 취급하기.
#[test]
fn host_text_edits_do_not_invalidate_unchanged_cell_flows() {
    let base = nested_base();
    let mut value = export(&base);
    value["document"]["sections"][0]["paragraphs"][1]["runs"][0]["content"]["Text"] =
        json!("changed host");
    let out = roundtrip(&base, &value);
    assert_eq!(flags(&out.bytes), [true, false, false]);
    assert!(cell_paragraphs(&mut export(&out.bytes))
        .iter()
        .all(|p| p.get("layout_cache").is_some()));
}

// 이것을 실패시키는 것: 컨테이너가 참조하는 테두리 정의 변경을 하위 흐름에서 무시하기.
#[test]
fn changed_container_styles_invalidate_nested_paragraphs() {
    let base = nested_base();
    let mut value = export(&base);
    let borders = value["styles"]["border_fills"].as_array_mut().unwrap();
    let border = borders.iter_mut().find(|v| v["id"] == 3).unwrap();
    border["left"]["width"] = json!("1.0 mm");
    let out = roundtrip(&base, &value);
    assert_eq!(flags(&out.bytes), [true, false, false]);
    assert!(cell_paragraphs(&mut export(&out.bytes))
        .iter()
        .all(|p| p.get("layout_cache").is_none()));
}

// 이것을 실패시키는 것: Core에 도달하지 못한 base 컨테이너의 손실 경고를 버리기.
#[test]
fn base_decoder_content_losses_are_reported_as_provenance_failures() {
    let base = fixture("shapes/group-unsupported-children.hwpx");
    let decoded = HwpxDecoder::decode(&base).unwrap();
    assert!(decoded.warnings.iter().any(|w| w.is_content_loss()));
    let out = roundtrip(&base, &export(&base));
    assert!(out
        .meta()
        .warnings
        .iter()
        .any(|w| w.code == "CONTENT_LOST" && w.message.contains("base decode:")));
}

// 이것을 실패시키는 것: 떠 있는 개체의 앞 문단·독립 목록 또는 삭제 전 영향 범위 누락.
#[test]
fn floating_objects_invalidate_the_entire_section_including_preceding_text() {
    use hwpforge::core::placement::{ObjectPlacement, ObjectRelativeTo, ObjectTextWrap};
    let placement = serde_json::to_value(ObjectPlacement::new(
        ObjectTextWrap::Square,
        ObjectRelativeTo::Page,
        ObjectRelativeTo::Page,
    ))
    .unwrap();
    for default_table in [false, true] {
        let mut value = export(&nested_base());
        let table =
            &mut value["document"]["sections"][0]["paragraphs"][1]["runs"][1]["content"]["Table"];
        table["placement"] = if default_table { Value::Null } else { placement.clone() };
        let base = encode_fixture(&value);
        assert_eq!(flags(&roundtrip(&base, &export(&base)).bytes), [true; 3]);
        for edit in
            ["width", "offset", "wrap", "inline", "delete", "nested_text", "font", "preceding_text"]
        {
            let mut edited = export(&base);
            let table = &mut edited["document"]["sections"][0]["paragraphs"][1]["runs"][1]
                ["content"]["Table"];
            if table.get("placement").is_none_or(Value::is_null) {
                table["placement"] = placement.clone();
            }
            match edit {
                "width" => table["rows"][0]["cells"][0]["width"] = json!(9000),
                "offset" => table["placement"]["vert_offset"] = json!(-5000),
                "wrap" => table["placement"]["text_wrap"] = json!("BehindText"),
                "inline" => table["placement"]["treat_as_char"] = json!(true),
                "nested_text" => {
                    table["rows"][0]["cells"][0]["paragraphs"][0]["runs"][0]["content"]["Text"] =
                        json!("longer cell content")
                }
                "font" => edited["styles"]["char_shapes"][0]["height"] = json!(2000),
                "preceding_text" => {
                    edited["document"]["sections"][0]["paragraphs"][0]["runs"][0]["content"]
                        ["Text"] = json!("changed")
                }
                _ => {
                    edited["document"]["sections"][0]["paragraphs"][1]["runs"]
                        .as_array_mut()
                        .unwrap()
                        .pop();
                }
            }
            let out = roundtrip(&base, &edited);
            assert_section_dropped(&out, 0, "floating-object influence");
            let decoded = HwpxDecoder::decode(&out.bytes).unwrap();
            assert!(decoded.document.sections()[0].headers[0]
                .paragraphs
                .iter()
                .all(|p| p.layout_cache.is_none()));
        }
    }
}

// 이것을 실패시키는 것: 바탕쪽의 방출 의존성을 소유 구역의 비지역 판정에서 누락하기.
#[test]
fn master_page_linked_flow_invalidates_its_preceding_paragraph_and_section() {
    let mut value = export(&nested_base());
    let mut linked = value["styles"]["para_shapes"][0].clone();
    linked["keep_with_next"] = json!(true);
    let id = value["styles"]["para_shapes"].as_array().unwrap().len();
    value["styles"]["para_shapes"].as_array_mut().unwrap().push(linked);
    value["document"]["sections"][0]["master_pages"][0]["paragraphs"][0]["para_shape_id"] =
        json!(id);
    let base = encode_fixture(&value);
    assert_eq!(flags(&roundtrip(&base, &export(&base)).bytes), [true; 3]);
    let mut edited = export(&base);
    edited["document"]["sections"][0]["master_pages"][0]["paragraphs"][1]["runs"][0]["content"]
        ["Text"] = json!("늘어난 바탕쪽 본문".repeat(100));
    let out = roundtrip(&base, &edited);
    assert_section_dropped(&out, 0, "linked paragraphs");
    let doc = HwpxDecoder::decode(&out.bytes).unwrap();
    assert!(doc.document.sections()[0].master_pages.as_ref().unwrap()[0]
        .paragraphs
        .iter()
        .all(|p| p.layout_cache.is_none()));
}

// 이것을 실패시키는 것: 해석하지 못하는 흐름의 unknown_changed 전파를 제외하기.
#[test]
fn ambiguous_anchored_header_order_invalidates_the_section_on_edit() {
    let mut value = export(&nested_base());
    let mut header = value["document"]["sections"][0]["headers"][0].clone();
    header["anchor_paragraph"] = json!(2);
    value["document"]["sections"][0]["headers"].as_array_mut().unwrap().push(header);
    let base = encode_fixture(&value);
    assert_eq!(flags(&roundtrip(&base, &export(&base)).bytes), [true; 3]);
    let mut edited = export(&base);
    edited["document"]["sections"][0]["headers"][0]["paragraphs"][1]["runs"][0]["content"]
        ["Text"] = json!("changed");
    assert_section_dropped(&roundtrip(&base, &edited), 0, "unknown/unresolved");
}

// 이것을 실패시키는 것: 인코더 origin 수집·최종 방출 순서 정렬·marker 제거 중 하나를 생략하기.
#[test]
fn emitted_origins_follow_final_xml_and_do_not_change_output_bytes() {
    use hwpforge::hwpx::{ParagraphPath, PathSeg};
    let decoded = HwpxDecoder::decode(&nested_base()).unwrap();
    let doc = decoded.document.validate().unwrap();
    let ordinary = HwpxEncoder::encode_with_diagnostics(
        &doc,
        &decoded.style_store,
        &decoded.image_store,
        EncodeOptions::default(),
    )
    .unwrap();
    let (emitted, origins) = HwpxEncoder::encode_with_origins(
        &doc,
        &decoded.style_store,
        &decoded.image_store,
        EncodeOptions::default(),
    )
    .unwrap();
    assert_eq!(ordinary.bytes, emitted.bytes);
    let paths = &origins.0["Contents/section0.xml"];
    assert_eq!(paths.len(), 11);
    assert_eq!(paths[0], Some(ParagraphPath(vec![PathSeg::Section(0), PathSeg::BodyParagraph(0)])));
    assert_eq!(
        paths[1],
        Some(ParagraphPath(vec![
            PathSeg::Section(0),
            PathSeg::Header(0),
            PathSeg::NestedParagraph(0)
        ]))
    );
    assert!(paths.iter().all(Option::is_some));
    assert_eq!(origins.0["Contents/masterpage0.xml"].len(), 2);
    assert!(!xml(&emitted.bytes).contains("hwpforgeOrigin"));
}

// 이것을 실패시키는 것: 공통 계획에서 변경 집합을 무시하고 JSON 어댑터 안에서만 정책을 적용하기.
#[test]
fn shared_planner_closes_explicit_edits_without_json_or_reencoding() {
    use hwpforge::hwpx::{
        layout_edit::{plan_invalidation, ChangeSet, PackageAnalysis, ParagraphCorrespondence},
        ParagraphPath, PathSeg,
    };
    for policy in ["local", "linked", "floating", "numbering", "backwards", "unknown"] {
        let mut value = export(&nested_base());
        match policy {
            "linked" => value["styles"]["para_shapes"][0]["keep_with_next"] = json!(true),
            "floating" => {
                value["document"]["sections"][0]["paragraphs"][1]["runs"][1]["content"]["Table"]
                    ["placement"]["treat_as_char"] = json!(false)
            }
            "numbering" => push_control(
                &mut value,
                0,
                1,
                serde_json::to_value(hwpforge::core::control::Control::AutoNumber {
                    kind: hwpforge::core::control::AutoNumberKind::Picture,
                    num: 1,
                    format: Default::default(),
                })
                .unwrap(),
            ),
            "backwards" => {
                push_control(&mut value, 0, 1, json!({"InlinePageNumber":{"kind":"TotalPages"}}))
            }
            _ => {}
        }
        let mut base = encode_fixture(&value);
        if policy == "unknown" {
            base = edit_package(&base, "Contents/section0.xml", |x| {
                x.replacen("<hp:ctrl>", "<hp:ctrl><hp:futureControl/>", 1)
            });
        }
        let analysis = PackageAnalysis::read(&base).unwrap();
        let correspondence = ParagraphCorrespondence::identity(&analysis);
        let mut changes = ChangeSet::default();
        changes.paragraph(
            ParagraphPath(vec![PathSeg::Section(0), PathSeg::BodyParagraph(2)]),
            "row editor changed geometry",
        );
        let plan = plan_invalidation(&analysis, &analysis, &correspondence, &changes);
        if policy == "local" {
            assert!(plan.paragraphs[0].is_empty());
        } else {
            assert!(!plan.paragraphs[0].is_empty(), "{policy}");
        }
        assert!(plan.paragraphs.iter().any(|r| r.contains("row editor changed geometry")));
        changes.document.insert("ContentLost:future layout setting".into());
        let loss = plan_invalidation(&analysis, &analysis, &correspondence, &changes);
        assert!(loss.paragraphs.iter().all(|r| r.contains("ContentLost:future layout setting")));
    }
}

// 이것을 실패시키는 것: 방출 origin 대신 XML 순서에서 Core header 인덱스를 다시 추정하기.
#[test]
fn analysis_uses_encoder_origins_when_anchor_order_differs_from_core_order() {
    use hwpforge::hwpx::{layout_edit::PackageAnalysis, PathSeg};
    let mut value = export(&nested_base());
    let mut header = value["document"]["sections"][0]["headers"][0].clone();
    header["anchor_paragraph"] = json!(0);
    value["document"]["sections"][0]["headers"][0]["anchor_paragraph"] = json!(2);
    value["document"]["sections"][0]["headers"].as_array_mut().unwrap().push(header);
    let doc: Document<hwpforge::core::Draft> =
        serde_json::from_value(value["document"].clone()).unwrap();
    let styles = serde_json::from_value(value["styles"].clone()).unwrap();
    let (out, origins) = HwpxEncoder::encode_with_origins(
        &doc.validate().unwrap(),
        &styles,
        &ImageStore::new(),
        EncodeOptions::default(),
    )
    .unwrap();
    let raw = PackageAnalysis::read(&out.bytes).unwrap();
    let emitted = PackageAnalysis::read_emitted(&out.bytes, &origins).unwrap();
    let first_header = |a: &PackageAnalysis| {
        a.paths()
            .find_map(|p| {
                p.0.iter().find_map(|s| if let PathSeg::Header(i) = s { Some(*i) } else { None })
            })
            .unwrap()
    };
    assert_eq!(first_header(&raw), 0);
    assert_eq!(first_header(&emitted), 1);
}
