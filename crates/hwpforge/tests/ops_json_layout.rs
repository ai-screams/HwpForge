//! Package-backed JSON round trips select trusted caches through Core and the
//! common flow invalidator, then re-emit coordinates through the token ledger.
#![cfg(feature = "ops-hwpx")]

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

fn xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut text).unwrap();
    text
}

fn caches(bytes: &[u8]) -> Vec<String> {
    let xml = xml(bytes);
    xml.split("<hp:linesegarray")
        .skip(1)
        .map(|s| format!("<hp:linesegarray{}", &s[..s.find("</hp:linesegarray>").unwrap() + 18]))
        .collect()
}

fn export(bytes: &[u8]) -> Value {
    // Typed export has no grid-address annotations to become stale when a
    // test intentionally inserts or removes a paragraph.
    serde_json::to_value(to_json(bytes, &ToJsonOptions::default()).unwrap().exported).unwrap()
}

fn roundtrip(base: &[u8], value: &Value) -> hwpforge::ops::FromJsonOutput {
    from_json(&value.to_string(), &FromJsonOptions::default().with_base(base)).unwrap()
}

fn flags(bytes: &[u8]) -> Vec<bool> {
    HwpxDecoder::decode(bytes).unwrap().document.sections()[0]
        .paragraphs
        .iter()
        .map(|p| p.layout_cache.is_some())
        .collect()
}

fn simple_base() -> Vec<u8> {
    let decoded =
        HwpxDecoder::decode(&fixture("user_samples/sample-para-line-spacing.hwpx")).unwrap();
    let cache = decoded.document.sections()[0]
        .paragraphs
        .iter()
        .find_map(|p| p.layout_cache.clone())
        .unwrap();
    let mut doc = Document::new();
    let paragraphs = (0..5)
        .map(|i| {
            let mut p = Paragraph::with_runs(
                vec![Run::text(
                    format!("문단 {i}의 여러 줄 본문입니다. ").repeat(20),
                    CharShapeIndex::new(0),
                )],
                ParaShapeIndex::new(0),
            );
            p.layout_cache = Some(cache.clone());
            p
        })
        .collect();
    doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
    HwpxEncoder::encode_with_options(
        &doc.validate().unwrap(),
        &style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap()
}

// 이것을 실패시키는 것: base 경로에서 emit_layout_cache=false인 일반 인코더 사용.
#[test]
fn three_native_multiline_roundtrips_keep_line_arrays_byte_for_byte() {
    for name in [
        "user_samples/sample-para-line-spacing.hwpx",
        "user_samples/sample-para-page-break.hwpx",
        "tables/table_20_real_world_ministry_style.hwpx",
    ] {
        let base = fixture(name);
        let original = caches(&base);
        assert!(original.iter().any(|s| s.matches("<hp:lineseg ").count() > 1), "{name}");
        let rebuilt = roundtrip(&base, &export(&base));
        assert_eq!(caches(&rebuilt.bytes), original, "{name}: {:?}", rebuilt.warnings);
        assert!(
            !rebuilt.meta().warnings.iter().any(|w| w.code == "LAYOUT_CACHE_DROPPED"),
            "{name}: {:?}",
            rebuilt.warnings
        );
    }
}

// 이것을 실패시키는 것: 같은 흐름의 뒤 문단을 무효화하지 않기.
#[test]
fn a_text_edit_invalidates_only_its_flow_tail() {
    let base = simple_base();
    let mut value = export(&base);
    value["document"]["sections"][0]["paragraphs"][2]["runs"][0]["content"]["Text"] = json!("수정");
    let out = roundtrip(&base, &value);
    assert_eq!(flags(&out.bytes), [true, true, false, false, false]);
    assert_eq!(&caches(&out.bytes), &caches(&base)[..2]);
    assert_eq!(
        out.meta()
            .warnings
            .iter()
            .filter(|w| w.message.contains("intentionally invalidated"))
            .count(),
        3
    );
}

// 이것을 실패시키는 것: 길이가 다른 목록에서 동일한 접두부도 버리거나 뒤를 대응시키기.
#[test]
fn insertion_and_deletion_keep_only_the_identical_prefix_and_warn() {
    let base = simple_base();
    for insert in [true, false] {
        let mut value = export(&base);
        let ps = value["document"]["sections"][0]["paragraphs"].as_array_mut().unwrap();
        if insert {
            let mut p = ps[0].clone();
            p["runs"][0]["content"]["Text"] = json!("새 문단");
            ps.insert(2, p);
        } else {
            ps.remove(2);
        }
        let out = roundtrip(&base, &value);
        let expected = if insert {
            vec![true, true, false, false, false, false]
        } else {
            vec![true, true, false, false]
        };
        assert_eq!(flags(&out.bytes), expected);
        assert!(out
            .meta()
            .warnings
            .iter()
            .any(|w| w.message.contains("paragraph structure changed")));
    }
}

// 이것을 실패시키는 것: 재배열된 문단을 텍스트로 다시 찾아 캐시를 되살리기.
#[test]
fn reordering_invalidates_from_the_first_moved_paragraph() {
    let base = simple_base();
    let mut value = export(&base);
    value["document"]["sections"][0]["paragraphs"].as_array_mut().unwrap().swap(1, 3);
    assert_eq!(flags(&roundtrip(&base, &value).bytes), [true, false, false, false, false]);
}

// 이것을 실패시키는 것: JSON의 수동 변경 캐시를 base 캐시 대신 신뢰하기.
#[test]
fn manually_modified_or_removed_json_caches_do_not_override_the_base() {
    let base = simple_base();
    for remove in [false, true] {
        let mut value = export(&base);
        let p = &mut value["document"]["sections"][0]["paragraphs"][1];
        if remove {
            p.as_object_mut().unwrap().remove("layout_cache");
        } else {
            p["layout_cache"]["lines"][0]["vertpos"] = json!(123456);
        }
        let out = roundtrip(&base, &value);
        assert_eq!(caches(&out.bytes), caches(&base));
    }
}

// 이것을 실패시키는 것: 참조 스타일 정의 변경을 문단 편집 판정에서 제외하기.
#[test]
fn referenced_style_changes_invalidate_but_unused_styles_do_not() {
    let base = simple_base();
    let mut value = export(&base);
    value["styles"]["char_shapes"][0]["height"] = json!(2200);
    assert_eq!(flags(&roundtrip(&base, &value).bytes), [false; 5]);
    let mut value = export(&base);
    value["styles"]["char_shapes"][6]["height"] = json!(2300);
    assert_eq!(caches(&roundtrip(&base, &value).bytes), caches(&base));
}

// 이것을 실패시키는 것: 쪽 설정 변경을 무시하고 문단 내용만 비교하기.
#[test]
fn page_settings_change_invalidates_the_section() {
    let base = simple_base();
    let mut value = export(&base);
    let page = value["document"]["sections"][0]["page_settings"].as_object_mut().unwrap();
    let width = page.get_mut("width").unwrap();
    *width = json!(width.as_i64().unwrap() + 1000);
    assert_eq!(flags(&roundtrip(&base, &value).bytes), [false; 5]);
}

// 이것을 실패시키는 것: base 없는 입력의 캐시를 켜거나 기존 경고를 누락하기.
#[test]
fn without_base_caches_are_dropped_with_the_existing_warning() {
    let base = simple_base();
    let out = from_json(&export(&base).to_string(), &FromJsonOptions::default()).unwrap();
    assert_eq!(flags(&out.bytes), [false; 5]);
    assert_eq!(out.meta().warnings.iter().filter(|w| w.code == "LAYOUT_CACHE_DROPPED").count(), 1);
}

fn nested_base() -> Vec<u8> {
    use hwpforge::core::layout::{LayoutCache, LineSeg};
    use hwpforge::core::section::{HeaderFooter, MasterPage};
    use hwpforge::core::table::{Table, TableCell, TableRow};
    use hwpforge::foundation::{ApplyPageType, HwpUnit};
    let p = |text: &str| {
        let mut p = Paragraph::with_runs(
            vec![Run::text(text, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let mut line = LineSeg::default();
        line.vertsize = 1000;
        line.horzsize = 42520;
        p.layout_cache = Some(LayoutCache::new(vec![line]));
        p
    };
    let cell = |prefix: &str| {
        TableCell::new(
            (0..3).map(|i| p(&format!("{prefix}{i}"))).collect(),
            HwpUnit::new(4000).unwrap(),
        )
    };
    let mut host = p("host");
    host.runs.push(Run::table(
        Table::new(vec![TableRow::new(vec![cell("a"), cell("b")])]),
        CharShapeIndex::new(0),
    ));
    let mut sec = Section::with_paragraphs(vec![p("before"), host, p("after")], PageSettings::a4());
    sec.headers.push(HeaderFooter::new(vec![p("h0"), p("h1")], ApplyPageType::Both));
    sec.master_pages = Some(vec![MasterPage::new(ApplyPageType::Both, vec![p("m0"), p("m1")])]);
    let mut doc = Document::new();
    doc.add_section(sec);
    HwpxEncoder::encode_with_options(
        &doc.validate().unwrap(),
        &style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap()
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

// 이것을 실패시키는 것: 글꼴 참조의 대상 정의 변경을 비교에서 누락하기.
#[test]
fn referenced_font_changes_invalidate_the_cache() {
    let base = simple_base();
    let mut value = export(&base);
    for font in value["styles"]["fonts"].as_array_mut().unwrap() {
        font["face_name"] = json!("다른 글꼴");
    }
    assert_eq!(flags(&roundtrip(&base, &value).bytes), [false; 5]);
}

// 이것을 실패시키는 것: base에 없는 캐시를 JSON에서 되살리기.
#[test]
fn a_base_without_caches_never_trusts_supplied_caches() {
    let cached = simple_base();
    let d = HwpxDecoder::decode(&cached).unwrap();
    let base = HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store)
        .unwrap();
    let out = roundtrip(&base, &export(&cached));
    assert_eq!(flags(&out.bytes), [false; 5]);
    assert_eq!(
        out.meta().warnings.iter().filter(|w| w.message.contains("no trusted base cache")).count(),
        5
    );
}

// 이것을 실패시키는 것: 구역 수 변경 때 대응 가능한 접두부를 버리거나 경고를 누락하기.
#[test]
fn added_sections_keep_the_unchanged_prefix_and_report_missing_correspondence() {
    let base = simple_base();
    let mut value = export(&base);
    let sections = value["document"]["sections"].as_array_mut().unwrap();
    sections.push(sections[0].clone());
    let out = roundtrip(&base, &value);
    assert_eq!(flags(&out.bytes), [true; 5]);
    let d = HwpxDecoder::decode(&out.bytes).unwrap();
    assert!(d.document.sections()[1].paragraphs.iter().all(|p| p.layout_cache.is_none()));
    assert!(out.meta().warnings.iter().any(|w| w.message.contains("section structure changed")));
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
