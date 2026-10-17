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

// 이것을 실패시키는 것: 실제 같은 조판 XML의 base 캐시를 일반 인코더처럼 버리기.
#[test]
fn three_native_multiline_roundtrips_keep_line_arrays_byte_for_byte() {
    // 이것을 실패시키는 것: 미편집 원본을 재생성해 sz/offset 등의 값을 잃기.
    // Raw no-op output preserves both the inputs and arrays, including values
    // absent from Core. The ordinary encoder fixture still exercises carry.
    for name in [
        "user_samples/sample-para-line-spacing.hwpx",
        "user_samples/sample-para-page-break.hwpx",
        "tables/table_20_real_world_ministry_style.hwpx",
    ] {
        let native = fixture(name);
        let native_out = roundtrip(&native, &export(&native));
        assert_eq!(native_out.bytes, native, "{name}");
        let base = encode_fixture(&export(&native));
        let original = caches(&base);
        assert!(original.iter().any(|s| s.matches("<hp:lineseg ").count() > 1), "{name}");
        let rebuilt = roundtrip(&base, &export(&base));
        assert_eq!(caches(&rebuilt.bytes), original, "{name}: {:?}", rebuilt.warnings);
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
    let mut table = Table::new(vec![TableRow::new(vec![cell("a"), cell("b")])]);
    table.placement = Some(hwpforge::core::placement::ObjectPlacement::legacy_inline_defaults());
    host.runs.push(Run::table(table, CharShapeIndex::new(0)));
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

/// Construct trusted synthetic fixtures with caches before exercising the
/// public JSON route. Changes under test are always made after this encode.
fn encode_fixture(value: &Value) -> Vec<u8> {
    let doc: Document<hwpforge::core::Draft> =
        serde_json::from_value(value["document"].clone()).unwrap();
    let styles = serde_json::from_value(value["styles"].clone()).unwrap();
    HwpxEncoder::encode_with_options(
        &doc.validate().unwrap(),
        &styles,
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap()
}

fn section_flags(bytes: &[u8], section: usize) -> Vec<bool> {
    HwpxDecoder::decode(bytes).unwrap().document.sections()[section]
        .paragraphs
        .iter()
        .map(|p| p.layout_cache.is_some())
        .collect()
}

fn two_sections() -> Value {
    let mut value = export(&simple_base());
    let section = value["document"]["sections"][0].clone();
    value["document"]["sections"].as_array_mut().unwrap().push(section);
    value
}

fn push_control(value: &mut Value, section: usize, paragraph: usize, control: Value) {
    value["document"]["sections"][section]["paragraphs"][paragraph]["runs"]
        .as_array_mut()
        .unwrap()
        .push(json!({"char_shape_id":0, "content":{"Control":control}}));
}

fn assert_section_dropped(out: &hwpforge::ops::FromJsonOutput, section: usize, reason: &str) {
    assert!(
        section_flags(&out.bytes, section).iter().all(|&cached| !cached),
        "section {section}: {:?}",
        out.warnings
    );
    assert!(
        out.meta()
            .warnings
            .iter()
            .any(|w| w.code == "LAYOUT_CACHE_DROPPED" && w.message.contains(reason)),
        "{:?}",
        out.warnings
    );
}

// 이것을 실패시키는 것: 비연속 스타일 ID의 참조 변경을 Null==Null로 통과시키기.
#[test]
fn explicit_style_ids_survive_reordering_and_detect_reference_edits() {
    let mut value = export(&simple_base());
    value["styles"]["styles"] = json!([value["styles"]["styles"][0], value["styles"]["styles"][7]]);
    for p in value["document"]["sections"][0]["paragraphs"].as_array_mut().unwrap() {
        p["style_id"] = json!(7);
    }
    let base = encode_fixture(&value);
    let mut value = export(&base);
    value["styles"]["styles"].as_array_mut().unwrap().reverse();
    assert_eq!(flags(&roundtrip(&base, &value).bytes), [true; 5]);
    for key in ["para_pr_id_ref", "char_pr_id_ref"] {
        let mut edited = value.clone();
        let style = edited["styles"]["styles"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|v| v["id"] == 7)
            .unwrap();
        style[key] = json!(if style[key] == 0 { 1 } else { 0 });
        assert_section_dropped(&roundtrip(&base, &edited), 0, "intentionally invalidated");
    }
}

// 이것을 실패시키는 것: 해석할 수 없는 스타일·글꼴을 무편집이라는 이유로 보존하기.
#[test]
fn unresolved_and_ambiguous_style_references_drop_with_warning() {
    let base = simple_base();
    for kind in ["styles", "fonts", "tabs", "border_fills"] {
        let mut value = export(&base);
        value["styles"][kind] = json!([]);
        assert_section_dropped(&roundtrip(&base, &value), 0, "intentionally invalidated");
    }
    let mut value = export(&base);
    let style = value["styles"]["styles"][0].clone();
    value["styles"]["styles"].as_array_mut().unwrap().push(style);
    assert_section_dropped(&roundtrip(&base, &value), 0, "intentionally invalidated");
}

// 이것을 실패시키는 것: 시작번호 변경의 영향 범위를 현재 구역으로 제한하기.
#[test]
fn begin_numbers_invalidate_later_sections_for_all_counter_kinds() {
    let base = encode_fixture(&two_sections());
    for kind in ["page", "footnote", "endnote", "pic", "tbl", "equation", "page_starts_on"] {
        let mut value = export(&base);
        value["document"]["sections"][0]["begin_num"][kind] =
            if kind == "page_starts_on" { json!("Odd") } else { json!(1001) };
        let out = roundtrip(&base, &value);
        assert_section_dropped(&out, 0, "numbering/page dependencies");
        assert_section_dropped(&out, 1, "numbering/page dependencies");
    }
}

// 이것을 실패시키는 것: 각주/미주 삽입·삭제·NewNumber 후 다음 구역 host를 보존하기.
#[test]
fn note_sequence_edits_invalidate_hosts_across_sections() {
    use hwpforge::core::control::Control;
    for endnote in [false, true] {
        let mut value = two_sections();
        let mut p: Paragraph =
            serde_json::from_value(value["document"]["sections"][0]["paragraphs"][0].clone())
                .unwrap();
        p.layout_cache = None;
        let note = serde_json::to_value(if endnote {
            Control::endnote(vec![p])
        } else {
            Control::footnote(vec![p])
        })
        .unwrap();
        for section in 0..2 {
            push_control(&mut value, section, 1, note.clone());
        }
        let base = encode_fixture(&value);
        assert_eq!(section_flags(&base, 1), [true; 5]);
        assert_eq!(section_flags(&roundtrip(&base, &export(&base)).bytes, 1), [true; 5]);
        for edit in ["start", "insert", "delete", "restart", "restart_edit", "restart_delete"] {
            let mut before = export(&base);
            if edit.starts_with("restart_") {
                push_control(
                    &mut before,
                    0,
                    0,
                    json!({"NewNumber":{"kind":if endnote {"Endnote"} else {"Footnote"},"number":9}}),
                );
            }
            let base = encode_fixture(&before);
            let mut edited = export(&base);
            match edit {
                "start" => {
                    edited["document"]["sections"][0]["begin_num"]
                        [if endnote { "endnote" } else { "footnote" }] = json!(1001)
                }
                "insert" => push_control(&mut edited, 0, 1, note.clone()),
                "delete" => {
                    edited["document"]["sections"][0]["paragraphs"][1]["runs"]
                        .as_array_mut()
                        .unwrap()
                        .pop();
                }
                "restart" => push_control(
                    &mut edited,
                    0,
                    0,
                    json!({"NewNumber":{"kind":if endnote {"Endnote"} else {"Footnote"},"number":1001}}),
                ),
                "restart_edit" => {
                    edited["document"]["sections"][0]["paragraphs"][0]["runs"][1]["content"]
                        ["Control"]["NewNumber"]["number"] = json!(1001)
                }
                _ => {
                    edited["document"]["sections"][0]["paragraphs"][0]["runs"]
                        .as_array_mut()
                        .unwrap()
                        .pop();
                }
            }
            let out = roundtrip(&base, &edited);
            assert_section_dropped(&out, 1, "numbering/page dependencies");
            let expected = match edit {
                "insert" => 3,
                "delete" => 1,
                "restart_delete" => 2,
                _ => 1002,
            };
            let mut zip = zip::ZipArchive::new(Cursor::new(&out.bytes)).unwrap();
            let mut section = String::new();
            zip.by_name("Contents/section1.xml").unwrap().read_to_string(&mut section).unwrap();
            let kind = if endnote { "ENDNOTE" } else { "FOOTNOTE" };
            assert!(
                section.contains(&format!("<hp:autoNum num=\"{expected}\" numType=\"{kind}\"")),
                "{edit}: {section}"
            );
        }
    }
}

// 이것을 실패시키는 것: 그림·표·수식 자동번호, 개요/목록을 구역별 독립 상태로 비교하기.
#[test]
fn caption_and_outline_numbering_propagate_across_sections() {
    use hwpforge::core::control::{AutoNumberFormat, AutoNumberKind, Control};
    for kind in [AutoNumberKind::Picture, AutoNumberKind::Table, AutoNumberKind::Equation] {
        let mut value = two_sections();
        let auto = serde_json::to_value(Control::AutoNumber {
            kind,
            num: 1,
            format: AutoNumberFormat::default(),
        })
        .unwrap();
        for section in 0..2 {
            push_control(&mut value, section, 1, auto.clone());
        }
        let base = encode_fixture(&value);
        for remove in [false, true] {
            let mut edited = export(&base);
            if remove {
                edited["document"]["sections"][0]["paragraphs"][1]["runs"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            } else {
                push_control(&mut edited, 0, 0, auto.clone());
            }
            assert_section_dropped(&roundtrip(&base, &edited), 1, "numbering/page dependencies");
        }
    }
    for heading in ["Outline", "Number"] {
        let mut value = two_sections();
        value["styles"]["para_shapes"][0]["heading_type"] = json!(heading);
        value["styles"]["para_shapes"][0]["heading_id_ref"] = json!(1);
        let base = encode_fixture(&value);
        let mut edited = export(&base);
        edited["document"]["sections"][0]["paragraphs"].as_array_mut().unwrap().remove(1);
        assert_section_dropped(&roundtrip(&base, &edited), 1, "numbering/page dependencies");
        let mut edited = export(&base);
        edited["styles"]["numberings"][0]["start"] = json!(1001);
        assert_section_dropped(&roundtrip(&base, &edited), 1, "numbering/page dependencies");
    }
}

// 이것을 실패시키는 것: 뒤 구역의 페이지 수 변화가 앞 구역의 전체 쪽 수에 미치는 영향 무시.
#[test]
fn total_pages_propagates_backwards_and_plain_sections_stay_independent() {
    let mut value = two_sections();
    push_control(&mut value, 0, 1, json!({"InlinePageNumber":{"kind":"TotalPages"}}));
    let base = encode_fixture(&value);
    let mut edited = export(&base);
    edited["document"]["sections"][1]["paragraphs"][2]["runs"][0]["content"]["Text"] =
        json!("changed");
    assert_section_dropped(&roundtrip(&base, &edited), 0, "numbering/page dependencies");
    let base = encode_fixture(&two_sections());
    let mut edited = export(&base);
    edited["document"]["sections"][0]["paragraphs"][2]["runs"][0]["content"]["Text"] =
        json!("changed");
    assert_eq!(section_flags(&roundtrip(&base, &edited).bytes, 1), [true; 5]);
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

// 이것을 실패시키는 것: 그림의 너비·위치·회피·추가를 표 전용 판정에서 놓치기.
#[test]
fn floating_picture_changes_and_insertion_invalidate_preceding_paragraphs() {
    use hwpforge::core::image::{Image, ImageFormat};
    use hwpforge::core::placement::{ObjectPlacement, ObjectRelativeTo, ObjectTextWrap};
    use hwpforge::foundation::HwpUnit;
    let mut image = Image::new(
        "BinData/image1.png",
        HwpUnit::new(5000).unwrap(),
        HwpUnit::new(3000).unwrap(),
        ImageFormat::Png,
    );
    image.placement = Some(ObjectPlacement::new(
        ObjectTextWrap::Square,
        ObjectRelativeTo::Page,
        ObjectRelativeTo::Page,
    ));
    let run = serde_json::to_value(Run::image(image, CharShapeIndex::new(0))).unwrap();
    let mut value = two_sections();
    value["document"]["sections"][0]["paragraphs"][1]["runs"]
        .as_array_mut()
        .unwrap()
        .push(run.clone());
    let doc: Document<hwpforge::core::Draft> =
        serde_json::from_value(value["document"].clone()).unwrap();
    let styles = serde_json::from_value(value["styles"].clone()).unwrap();
    let mut images = ImageStore::new();
    images.insert("image1.png", fixture("images/main-charactor.png"));
    let base = HwpxEncoder::encode_with_options(
        &doc.validate().unwrap(),
        &styles,
        &images,
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    let identity = roundtrip(&base, &export(&base));
    assert_eq!(flags(&identity.bytes), [true; 5], "{:?}", identity.warnings);
    for (pointer, replacement) in [
        ("/width", json!(15000)),
        ("/placement/vert_offset", json!(-7000)),
        ("/placement/text_wrap", json!("Tight")),
        ("/placement/out_margin", json!({"left":1000,"right":1000,"top":1000,"bottom":1000})),
    ] {
        let mut edited = export(&base);
        let picture =
            &mut edited["document"]["sections"][0]["paragraphs"][1]["runs"][1]["content"]["Image"];
        if pointer == "/placement/out_margin" {
            picture["placement"]["out_margin"] = replacement;
        } else {
            *picture.pointer_mut(pointer).unwrap() = replacement;
        }
        let out = roundtrip(&base, &edited);
        assert_section_dropped(&out, 0, "floating-object influence");
        assert_eq!(section_flags(&out.bytes, 1), [true; 5]);
    }
    let base = simple_base();
    let mut edited = export(&base);
    edited["document"]["sections"][0]["paragraphs"][1]["runs"].as_array_mut().unwrap().push(run);
    // The package need not render the absent image for cache selection to be conservative.
    assert_section_dropped(&roundtrip(&base, &edited), 0, "floating-object influence");
}

// 이것을 실패시키는 것: 방출된 treatAsChar=0 대신 Core의 treat_as_char만 검사하기.
#[test]
fn emitted_floating_chart_insertion_invalidates_preceding_text() {
    use hwpforge::core::control::Control;
    use hwpforge::core::placement::ObjectPlacement;
    use hwpforge::foundation::HwpUnit;
    let base = simple_base();
    let mut value = export(&base);
    let mut placement = ObjectPlacement::legacy_inline_defaults();
    placement.vert_offset = HwpUnit::new(-5000).unwrap();
    assert!(placement.treat_as_char);
    let chart = Control::EmbeddedChart {
        chart_xml:
            r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"/>"#
                .into(),
        ole_bytes: vec![0; 16],
        width: HwpUnit::new(20000).unwrap(),
        height: HwpUnit::new(10000).unwrap(),
        placement: Some(placement),
    };
    push_control(&mut value, 0, 1, serde_json::to_value(chart).unwrap());
    let out = roundtrip(&base, &value);
    assert!(xml(&out.bytes).contains("treatAsChar=\"0\""));
    assert!(xml(&out.bytes).contains(&format!("vertOffset=\"{}\"", -5000_i32 as u32)));
    assert_section_dropped(&out, 0, "floating-object influence");
}

// 이것을 실패시키는 것: 방출된 keepWithNext/widowOrphan의 앞 방향 연결을 무시하기.
#[test]
fn keep_with_next_and_keep_lines_at_page_bottom_invalidate_preceding_chain() {
    let mut value = export(&simple_base());
    let mut linked = value["styles"]["para_shapes"][0].clone();
    linked["keep_with_next"] = json!(true);
    linked["widow_orphan"] = json!(false);
    let id = value["styles"]["para_shapes"].as_array().unwrap().len();
    value["styles"]["para_shapes"].as_array_mut().unwrap().push(linked);
    let mut together = value["styles"]["para_shapes"][0].clone();
    together["keep_lines_together"] = json!(true);
    value["styles"]["para_shapes"].as_array_mut().unwrap().push(together);
    for i in 0..2 {
        value["document"]["sections"][0]["paragraphs"][i]["para_shape_id"] = json!(id);
        value["document"]["sections"][0]["paragraphs"][i]["layout_cache"]["lines"][0]["vertpos"] =
            json!(65000 + i * 1000);
    }
    value["document"]["sections"][0]["paragraphs"][2]["para_shape_id"] = json!(id + 1);
    let base = encode_fixture(&value);
    let identity = roundtrip(&base, &export(&base));
    assert_eq!(flags(&identity.bytes), [true; 5], "{:?}", identity.warnings);
    for edit in ["text", "insert", "delete"] {
        let mut value = export(&base);
        let paragraphs = value["document"]["sections"][0]["paragraphs"].as_array_mut().unwrap();
        match edit {
            "text" => {
                paragraphs[2]["runs"][0]["content"]["Text"] =
                    json!("다음 쪽으로 밀려날 긴 본문 ".repeat(200))
            }
            "insert" => paragraphs.insert(2, paragraphs[2].clone()),
            _ => {
                paragraphs.remove(2);
            }
        }
        assert!(flags(&roundtrip(&base, &value).bytes).iter().all(|&v| !v), "{edit}");
    }
}

// 이것을 실패시키는 것: 실제 XML의 hp:charPr@prIDRef를 참조 해석에서 제외하기.
#[test]
fn compose_only_char_style_reference_changes_invalidate_host_and_tail() {
    let mut value = export(&simple_base());
    let shapes = value["styles"]["char_shapes"].as_array_mut().unwrap();
    while shapes.len() < 8 {
        shapes.push(shapes[0].clone());
    }
    push_control(
        &mut value,
        0,
        1,
        json!({"Compose": {
            "compose_text":"가나", "circle_type":"CIRCLE", "char_sz":100,
            "compose_type":"SPREAD", "char_pr_ids":[7]
        }}),
    );
    let base = encode_fixture(&value);
    assert!(xml(&base).contains("prIDRef=\"7\""));
    let identity = roundtrip(&base, &export(&base));
    assert_eq!(flags(&identity.bytes), [true; 5], "{:?}", identity.warnings);
    for edit in ["height", "ratio", "font", "unresolved"] {
        let mut value = export(&base);
        match edit {
            "height" => value["styles"]["char_shapes"][7]["height"] = json!(3000),
            "ratio" => value["styles"]["char_shapes"][7]["ratio"] = json!(150),
            "font" => {
                let mut font = value["styles"]["fonts"][0].clone();
                font["id"] = json!(777);
                font["face_name"] = json!("Compose 전용 변경 글꼴");
                value["styles"]["fonts"].as_array_mut().unwrap().push(font);
                value["styles"]["char_shapes"][7]["font_ref"]["hangul"] = json!(777);
            }
            _ => value["styles"]["char_shapes"][7]["font_ref"]["hangul"] = json!(9999),
        }
        let out = roundtrip(&base, &value);
        let flags = flags(&out.bytes);
        assert!(flags[1..].iter().all(|&v| !v), "{edit}: {flags:?}");
        if edit == "unresolved" {
            assert!(flags.iter().all(|&v| !v), "unresolved dependency closes the section");
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

// 이것을 실패시키는 것: 번호 의존성의 원본/편집본 합집합에서 한쪽만 검사하기.
#[test]
fn numbering_union_includes_inserted_and_removed_last_consumer() {
    use hwpforge::core::control::{AutoNumberFormat, AutoNumberKind, Control};
    let auto = serde_json::to_value(Control::AutoNumber {
        kind: AutoNumberKind::Picture,
        num: 1,
        format: AutoNumberFormat::default(),
    })
    .unwrap();
    for insert in [false, true] {
        let mut value = two_sections();
        if !insert {
            push_control(&mut value, 0, 1, auto.clone());
        }
        let base = encode_fixture(&value);
        let mut edited = export(&base);
        if insert {
            push_control(&mut edited, 0, 1, auto.clone());
        } else {
            edited["document"]["sections"][0]["paragraphs"][1]["runs"]
                .as_array_mut()
                .unwrap()
                .pop();
        }
        assert_section_dropped(&roundtrip(&base, &edited), 1, "numbering/page dependencies");
    }
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

fn edit_package(base: &[u8], part: &str, edit: impl Fn(&str) -> String) -> Vec<u8> {
    use std::io::Write;
    let mut input = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut entry = input.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if entry.name().expect("ZIP entry name") == part {
            bytes = edit(std::str::from_utf8(&bytes).unwrap()).into_bytes();
        }
        output
            .start_file(
                entry.name().expect("ZIP entry name"),
                zip::write::SimpleFileOptions::default().compression_method(entry.compression()),
            )
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output.finish().unwrap().into_inner()
}

// The layout planner's JSON projection must retain independently verified
// source claims when an unrelated paragraph changes.
#[test]
fn unclosed_field_provenance_survives_layout_preparation() {
    let base = edit_package(&simple_base(), "Contents/section0.xml", |xml| {
        xml.replace(
            "</hs:sec>",
            r#"<hp:p id="99" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0"><hp:ctrl><hp:fieldBegin id="77" type="FORMULA" name="kept" editable="0" dirty="0" zorder="-1" fieldid="627469685" metaTag=""/></hp:ctrl><hp:t>kept body</hp:t></hp:run></hp:p></hs:sec>"#,
        )
    });
    let mut value = export(&base);
    let noop = roundtrip(&base, &value);
    assert_eq!(noop.bytes, base);
    assert!(noop.warnings.iter().any(|w| w.info().code.as_str() == "UNCLOSED_FIELD_BOUNDARY"));

    value["document"]["sections"][0]["paragraphs"][2]["runs"][0]["content"]["Text"] =
        json!("unrelated edit");
    let output = roundtrip(&base, &value);
    let decoded = HwpxDecoder::decode(&output.bytes).unwrap();
    let fields =
        hwpforge::core::field_inventory::FieldInventory::from_sections(decoded.document.sections());
    assert_eq!(fields.fields().len(), 1);
    assert_eq!(fields.fields()[0].current(), "kept body");
    // An unresolved field reference conservatively closes the section's cache
    // proof; retaining the field does not authorize reusing its layout inputs.
    assert!(flags(&output.bytes).iter().all(|kept| !kept));
    decoded.document.validate().unwrap();
}

// 이것을 실패시키는 것: 원본 대신 encode(decode(base))를 비교 기준으로 사용하기.
#[test]
fn original_font_line_height_survives_raw_and_regenerated_public_json_roundtrip() {
    let base = simple_base();
    let identity = roundtrip(&base, &export(&base));
    assert_eq!(flags(&identity.bytes), [true; 5], "{:?}", identity.warnings);
    let original = edit_package(&base, "Contents/header.xml", |xml| {
        xml.replace("fontLineHeight=\"0\"", "fontLineHeight=\"1\"")
    });
    // 이것을 실패시키는 것: fontLineHeight=1 원본의 미편집 출력에서 0을 쓰기.
    assert_eq!(roundtrip(&original, &export(&original)).bytes, original);
    // A real edit must leave the raw path; the fixed encoder keeps this scalar.
    let mut edited = export(&original);
    edited["document"]["sections"][0]["paragraphs"][4]["runs"][0]["content"]["Text"] =
        json!("edit");
    let out = roundtrip(&original, &edited);
    assert_eq!(flags(&out.bytes), [true, true, true, true, false]);
    let mut zip = zip::ZipArchive::new(Cursor::new(&out.bytes)).unwrap();
    let mut header = String::new();
    zip.by_name("Contents/header.xml").unwrap().read_to_string(&mut header).unwrap();
    assert!(header.contains("fontLineHeight=\"1\""));
}

// 이것을 실패시키는 것: isEmbedded 차이를 지문과 ContentLost 무효화에서 모두 제외하기.
#[test]
fn original_embedded_font_is_retained_raw_and_loss_invalidates_regeneration() {
    let original = edit_package(&simple_base(), "Contents/header.xml", |xml| {
        xml.replace("isEmbedded=\"0\"", "isEmbedded=\"1\"")
    });
    // 이것을 실패시키는 것: 원본 패키지 경로에서 embedded font 원본을 잃기.
    assert_eq!(roundtrip(&original, &export(&original)).bytes, original);
    let mut edited = export(&original);
    edited["document"]["sections"][0]["paragraphs"][4]["runs"][0]["content"]["Text"] =
        json!("edit");
    let out = roundtrip(&original, &edited);
    assert_section_dropped(&out, 0, "font@isEmbedded");
    assert!(out.meta().warnings.iter().any(|w| w.code == "CONTENT_LOST"));
    assert!(out.meta().warnings.iter().any(|w| w.message.contains("ContentLost:")));
}

// 이것을 실패시키는 것: refList 밖의 호환 설정을 전역 입력에서 제외하기.
#[test]
fn original_layout_compatibility_is_retained_raw_and_loss_invalidates_regeneration() {
    let original = edit_package(&simple_base(), "Contents/header.xml", |xml| {
        xml.replace(
            "<hh:layoutCompatibility/>",
            "<hh:layoutCompatibility><hh:fontWeight/></hh:layoutCompatibility>",
        )
    });
    assert_ne!(original, simple_base());
    // 이것을 실패시키는 것: 미편집 호환 설정을 버리거나 편집 시 무조건 base를 반환하기.
    assert_eq!(roundtrip(&original, &export(&original)).bytes, original);
    let mut edited = export(&original);
    edited["document"]["sections"][0]["paragraphs"][4]["runs"][0]["content"]["Text"] =
        json!("edit");
    assert_section_dropped(&roundtrip(&original, &edited), 0, "fontWeight");
}

// 이것을 실패시키는 것: content.hpf 비교 또는 자동 필드의 문서 입력 의존성을 제거하기.
#[test]
fn metadata_only_lastsaveby_edit_invalidates_field_flow_through_public_json() {
    let mut value = export(&simple_base());
    value["document"]["metadata"]["last_saved_by"] = json!("김");
    push_control(
        &mut value,
        0,
        1,
        json!({"UnknownSummary":{"token":"$lastsaveby","display_text":"김"}}),
    );
    let base = encode_fixture(&value);
    assert!(xml(&base).contains("$lastsaveby"));
    let identity = roundtrip(&base, &export(&base));
    assert_eq!(flags(&identity.bytes), [true; 5], "{:?}", identity.warnings);
    let mut edited = export(&base);
    edited["document"]["metadata"]["last_saved_by"] =
        json!("매우 긴 마지막 저장자 이름".repeat(30));
    assert_eq!(xml(&base), xml(&encode_fixture(&edited)));
    let out = roundtrip(&base, &edited);
    assert_section_dropped(&out, 0, "document input changed");
    let mut plain = export(&simple_base());
    plain["document"]["metadata"]["last_saved_by"] = json!("long name");
    assert_eq!(flags(&roundtrip(&simple_base(), &plain).bytes), [true; 5]);
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

// 이것을 실패시키는 것: 명시적 정의 변경 집합 또는 문단의 전이 의존성을 공통 계획에서 무시하기.
#[test]
fn shared_planner_resolves_definition_change_sets_without_reencoding() {
    use hwpforge::hwpx::layout_edit::{
        plan_invalidation, ChangeSet, PackageAnalysis, ParagraphCorrespondence,
    };
    let base = simple_base();
    let analysis = PackageAnalysis::read(&base).unwrap();
    let correspondence = ParagraphCorrespondence::identity(&analysis);
    let mut changes = ChangeSet::default();
    assert!(plan_invalidation(&analysis, &analysis, &correspondence, &changes)
        .paragraphs
        .iter()
        .all(|r| r.is_empty()));
    let font = analysis.definition_dependencies()[&("charPr".into(), "0".into())]
        .iter()
        .find(|(kind, _)| kind == "font:HANGUL")
        .expect("resolved Hangul font")
        .clone();
    let reason = format!("definition changed:{}/{}", font.0, font.1);
    changes.definitions.insert(font);
    let plan = plan_invalidation(&analysis, &analysis, &correspondence, &changes);
    assert!(plan.paragraphs.iter().all(|r| r.contains(&reason)));
    changes.definitions.clear();
    changes.definitions.insert(("charPr".into(), "6".into()));
    assert!(plan_invalidation(&analysis, &analysis, &correspondence, &changes)
        .paragraphs
        .iter()
        .all(|r| r.is_empty()));
}

// 이것을 실패시키는 것: stringParam 문서 의존성을 fieldBegin/page 역방향 정책에만 맡기기.
#[test]
fn shared_planner_invalidates_standalone_document_consumers() {
    use hwpforge::hwpx::layout_edit::{
        plan_invalidation, ChangeSet, PackageAnalysis, ParagraphCorrespondence,
    };
    let base = edit_package(&simple_base(), "Contents/section0.xml", |xml| {
        xml.replacen("</hp:t>", "</hp:t><hp:stringParam>$lastsaveby</hp:stringParam>", 1)
    });
    let edited = edit_package(&base, "Contents/content.hpf", |xml| {
        xml.replacen(
            "</opf:metadata>",
            "<opf:meta name=\"lastsaveby\">long name</opf:meta></opf:metadata>",
            1,
        )
    });
    let original = PackageAnalysis::read(&base).unwrap();
    let output = PackageAnalysis::read(&edited).unwrap();
    let correspondence = ParagraphCorrespondence::identity(&original);
    let plan = plan_invalidation(&original, &output, &correspondence, &ChangeSet::default());
    assert!(plan.paragraphs.iter().all(|r| r.contains("policy: document input consumer")));
}

// 이것을 실패시키는 것: raw 보존 조건에서 이미지, 스타일 또는 문서 메타데이터
// 비교를 빼기. 캐시만 바뀐 JSON은 기존 base-cache 시험에서 대조한다.
#[test]
fn raw_noop_guard_compares_images_styles_metadata_and_nested_content() {
    let base = fixture("tables/table_07_image_in_merged_cell.hwpx");
    assert_eq!(roundtrip(&base, &export(&base)).bytes, base);
    let d = HwpxDecoder::decode(&base).unwrap();
    let doc = d.document.validate().unwrap();
    let mut images = d.image_store.clone();
    let key = images.iter().next().unwrap().0.to_string();
    images.insert(&key, vec![1, 2, 3]);
    let changed = HwpxEncoder::encode_with_base(
        &base,
        &doc,
        &d.style_store,
        &images,
        EncodeOptions::default(),
    )
    .unwrap();
    assert_ne!(changed.bytes, base);
    let mut zip = zip::ZipArchive::new(Cursor::new(&changed.bytes)).unwrap();
    let mut payload = Vec::new();
    zip.by_name(&format!("BinData/{key}")).unwrap().read_to_end(&mut payload).unwrap();
    assert_eq!(payload, [1, 2, 3]);
    let base = nested_base();
    for pointer in [
        "/document/metadata/title",
        "/styles/fonts/0/face_name",
        "/document/sections/0/headers/0/paragraphs/0/runs/0/content/Text",
    ] {
        let mut edited = export(&base);
        *edited.pointer_mut(pointer).expect(pointer) = json!("changed raw guard input");
        assert_ne!(roundtrip(&base, &edited).bytes, base, "{pointer}");
    }
}

// 이것을 실패시키는 것: version/container/커서 차이를 다시 전역 입력에 넣기,
// 또는 PrintInfo와 모르는 설정까지 제거하기. raw 경로가 아닌 공통 계획기를 직접 시험한다.
#[test]
fn package_policy_ignores_provenance_but_keeps_print_and_unknown_settings() {
    use hwpforge::hwpx::layout_edit::{
        plan_invalidation, ChangeSet, PackageAnalysis, ParagraphCorrespondence,
    };
    let base = simple_base();
    let original = PackageAnalysis::read(&base).unwrap();
    let correspondence = ParagraphCorrespondence::identity(&original);
    let changed_plan = |bytes: &[u8]| {
        plan_invalidation(
            &original,
            &PackageAnalysis::read(bytes).unwrap(),
            &correspondence,
            &ChangeSet::default(),
        )
    };
    for (part, from, to) in [
        ("version.xml", "appVersion=", "writerVersion="),
        ("META-INF/container.xml", "ocf:", "pack:"),
        ("settings.xml", "pos=\"0\"", "pos=\"99\""),
    ] {
        let changed = edit_package(&base, part, |s| {
            assert!(s.contains(from));
            s.replace(from, to).replace("xmlns:ocf=", "xmlns:pack=")
        });
        assert!(changed_plan(&changed).paragraphs.iter().all(|r| r.is_empty()), "{part}");
    }
    for setting in [
        "<config:config-item-set config:name='PrintInfo'><config:config-item config:name='PrintAutoFootNote' config:type='boolean'>true</config:config-item></config:config-item-set>",
        "<config:config-item config:name='UnknownLayout' config:type='int'>1</config:config-item>",
        "<ha:FutureLayout value='1'/>",
    ] {
        let changed = edit_package(&base, "settings.xml", |s| s.replace("</ha:HWPApplicationSetting>", &format!("{setting}</ha:HWPApplicationSetting>")));
        assert!(changed_plan(&changed).paragraphs.iter().all(|r| !r.is_empty()), "{setting}");
    }
}

fn reverse_spine(base: &[u8]) -> Vec<u8> {
    edit_package(base, "Contents/content.hpf", |xml| {
        let (before, spine) = xml.split_once("<opf:spine>").unwrap();
        format!(
            "{before}<opf:spine>{}",
            spine
                .replace("section0", "SWAP")
                .replace("section1", "section0")
                .replace("SWAP", "section1")
        )
    })
}

// 이것을 실패시키는 것: PackageReader가 manifest 대신 파일명 번호로 구역을 읽기,
// 또는 인코더가 디코더에서 받은 Core 구역 순서를 다시 파일명 순서로 돌리기.
#[test]
fn spine_controls_decoded_and_regenerated_body_order() {
    let mut value = two_sections();
    value["document"]["sections"][1]["paragraphs"][0]["runs"][0]["content"]["Text"] =
        json!("두 번째 파일의 본문".repeat(100));
    let normal = encode_fixture(&value);
    let reversed = reverse_spine(&normal);
    let normal_doc = export(&normal);
    let mut edited = export(&reversed);
    assert_eq!(edited["document"]["sections"][0], normal_doc["document"]["sections"][1]);
    assert_eq!(edited["document"]["sections"][1], normal_doc["document"]["sections"][0]);
    assert_eq!(roundtrip(&reversed, &edited).bytes, reversed);
    edited["document"]["metadata"]["title"] = json!("순서를 유지한 재생성");
    let output = roundtrip(&reversed, &edited);
    let emitted = export(&output.bytes);
    for s in 0..2 {
        assert_eq!(
            emitted["document"]["sections"][s]["paragraphs"][0]["runs"],
            edited["document"]["sections"][s]["paragraphs"][0]["runs"]
        );
    }
}

// 이것을 실패시키는 것: 분석기가 spine 순서 변화를 생략하기. 두 구역 XML이
// 동일해 본문/스타일 지문으로는 차이가 없고, 제목 수정만으로 재생성을 강제한다.
#[test]
fn title_only_regeneration_invalidates_reversed_spine_even_with_identical_sections() {
    for numbered in [false, true] {
        let mut value = two_sections();
        if numbered {
            value["styles"]["para_shapes"][0]["heading_type"] = json!("Number");
            value["styles"]["para_shapes"][0]["heading_id_ref"] = json!(1);
        }
        let normal = encode_fixture(&value);
        let reversed = reverse_spine(&normal);
        let mut edited = export(&reversed);
        edited["document"]["metadata"]["title"] = json!("제목만 수정");
        let output = roundtrip(&reversed, &edited);
        for section in 0..2 {
            assert_section_dropped(&output, section, "spine reading order changed");
        }
        // Positive control: metadata has no automatic field consumer here.
        let mut edited = export(&normal);
        edited["document"]["metadata"]["title"] = json!("제목만 수정");
        let output = roundtrip(&normal, &edited);
        for section in 0..2 {
            assert_eq!(section_flags(&output.bytes, section), [true; 5]);
        }
    }
}

// 이것을 실패시키는 것: 분석기의 구역 탐색을 다시 section{s}.xml로 만들기.
// 두 파일의 글자 크기 정의가 달라, 해석된 spine 경로와 의존 그래프를 함께 검증한다.
#[test]
fn analyzer_uses_decoder_spine_indices_for_dependencies() {
    use hwpforge::hwpx::layout_edit::PackageAnalysis;
    use hwpforge::hwpx::{ParagraphPath, PathSeg};
    let mut value = two_sections();
    let mut shape = value["styles"]["char_shapes"][0].clone();
    shape["height"] = json!(3333);
    let index = value["styles"]["char_shapes"].as_array().unwrap().len();
    value["styles"]["char_shapes"].as_array_mut().unwrap().push(shape);
    value["document"]["sections"][1]["paragraphs"][0]["runs"][0]["char_shape_id"] = json!(index);
    let normal = encode_fixture(&value);
    let reversed = reverse_spine(&normal);
    let a = PackageAnalysis::read(&normal).unwrap();
    let b = PackageAnalysis::read(&reversed).unwrap();
    let path = |s| ParagraphPath(vec![PathSeg::Section(s), PathSeg::BodyParagraph(0)]);
    assert_ne!(a.paragraph_dependencies(&path(0)), a.paragraph_dependencies(&path(1)));
    assert_eq!(a.paragraph_dependencies(&path(1)), b.paragraph_dependencies(&path(0)));
    assert_eq!(a.paragraph_dependencies(&path(0)), b.paragraph_dependencies(&path(1)));
    // 이것을 실패시키는 것: emitted origin의 part를 Core 구역 번호로 추측하기.
    let decoded = HwpxDecoder::decode(&normal).unwrap();
    let (_, mut origins) = HwpxEncoder::encode_with_origins(
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
        EncodeOptions::default(),
    )
    .unwrap();
    for origin in origins.0.values_mut().flatten().flatten() {
        if let Some(PathSeg::Section(s)) = origin.0.first_mut() {
            *s = 1 - *s;
        }
    }
    let emitted = PackageAnalysis::read_emitted(&reversed, &origins).unwrap();
    assert_eq!(b.paragraph_dependencies(&path(0)), emitted.paragraph_dependencies(&path(0)));
    assert_eq!(b.paragraph_dependencies(&path(1)), emitted.paragraph_dependencies(&path(1)));
}

// 이것을 실패시키는 것: 미지원 spine/rootfile을 조용히 무시하거나 동일한
// 미지원 패키지라는 이유로 캐시 분석을 허용하기. 없는 항목/비구역 대상을 포함한다.
#[test]
fn unsupported_package_order_warns_and_globally_invalidates_even_against_itself() {
    use hwpforge::hwpx::layout_edit::{
        plan_invalidation, ChangeSet, PackageAnalysis, ParagraphCorrespondence,
    };
    let base = encode_fixture(&two_sections());
    for (part, from, to) in [
        ("Contents/content.hpf", "idref=\"section1\"", "idref=\"absent\""),
        ("Contents/content.hpf", "idref=\"section1\"", "idref=\"settings\""),
        ("Contents/content.hpf", "href=\"Contents/section1.xml\"", "href=\"Contents/missing.xml\""),
        ("Contents/content.hpf", "idref=\"section1\"", "idref=\"section0\""),
        ("META-INF/container.xml", "Contents/content.hpf", "Other/content.hpf"),
        ("META-INF/container.xml", "media-type=", "packaging-media-type="),
    ] {
        let broken = edit_package(&base, part, |xml| {
            assert!(xml.contains(from));
            xml.replace(from, to)
        });
        let decoded = HwpxDecoder::decode(&broken).unwrap();
        assert!(decoded.warnings.iter().any(|w| matches!(w,
            hwpforge::hwpx::DecodeWarning::ContentLost { kind, .. } if kind == "package_reading_order")), "{to}");
        let analysis = PackageAnalysis::read(&broken).unwrap();
        let plan = plan_invalidation(
            &analysis,
            &analysis,
            &ParagraphCorrespondence::identity(&analysis),
            &ChangeSet::default(),
        );
        assert_eq!(plan.paragraphs.len(), 10);
        assert!(
            plan.paragraphs.iter().all(|reasons| reasons.contains("policy: unresolved reference")),
            "{to}: {plan:?}"
        );
    }
}

// 이것을 실패시키는 것: clean/shell이 이름만 보고 모든 layout_cache를 제거하기.
// extras의 runs/para_shape_id도 문자열 사용자 데이터이며 문단으로 오인하면 안 된다.
#[test]
fn metadata_layout_cache_user_data_edits_and_deletion_survive_public_json() {
    let mut value = export(&simple_base());
    value["document"]["metadata"]["extras"] =
        json!({"layout_cache":"old", "runs":"user", "para_shape_id":"user"});
    let base = encode_fixture(&value);
    for delete in [false, true] {
        let mut edited = export(&base);
        if delete {
            edited["document"]["metadata"]["extras"]
                .as_object_mut()
                .unwrap()
                .remove("layout_cache");
        } else {
            edited["document"]["metadata"]["extras"]["layout_cache"] = json!("new");
        }
        let output = roundtrip(&base, &edited);
        assert_ne!(output.bytes, base);
        let decoded = export(&output.bytes);
        assert_eq!(
            decoded["document"]["metadata"]["extras"],
            edited["document"]["metadata"]["extras"]
        );
        assert_eq!(flags(&output.bytes), [true; 5]);
    }
}
