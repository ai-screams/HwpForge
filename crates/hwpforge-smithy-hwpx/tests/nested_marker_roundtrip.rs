#[path = "support/encoding.rs"]
mod encoding;
use encoding::minimal_store;
#[path = "support/nested_marker_roundtrip.rs"]
mod shared;
use shared::*;

use std::io::Read;

use hwpforge_core::control::Control;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::{Document, PageSettings};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::HwpxEncoder;

// 이것을 실패시키는 것: 단일 pass 회귀 — 메모 안 하이퍼링크(HWPHL 전체-run
// 마커)도 메모 조각 안에 숨어 있다.
#[test]
fn memo_body_hyperlink_is_emitted() {
    let link = Run::control(
        Control::Hyperlink { text: "LINK".to_string(), url: "https://example.com".to_string() },
        CharShapeIndex::new(0),
    );
    let memo = Control::memo(vec![Paragraph::with_runs(vec![link], ParaShapeIndex::new(0))]);
    let section = Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![
                Run::text("본문", CharShapeIndex::new(0)),
                Run::control(memo, CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    );
    let mut doc = Document::new();
    doc.add_section(section);
    let validated = doc.validate().expect("validate");
    let out =
        HwpxEncoder::encode(&validated, &minimal_store(), &ImageStore::new()).expect("encode");
    let xml = zip_entry(&out, "Contents/section0.xml");
    let sub = memo_sublist(&xml);
    assert!(!xml.contains("__HWP"), "internal marker leaked: {sub}");
    assert!(sub.contains(r#"type="HYPERLINK""#), "memo must carry the hyperlink field: {sub}");
    assert!(sub.contains("LINK"), "hyperlink text must be emitted: {sub}");
}

// 이것을 실패시키는 것: 단일 pass 회귀 — 묶음 개체의 자식 글상자 조각은
// `encode_group_to_xml` 이 문자열로 만들어 HWPGRP 조각 안에 넣는다.
#[test]
fn group_text_box_tab_is_emitted() {
    let hu = |v: i32| HwpUnit::new(v).expect("unit");
    let text_box = Control::text_box(
        vec![Paragraph::with_runs(
            vec![Run::text("A\tB", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        hu(8000),
        hu(4000),
    );
    let group = Control::Group {
        frame: hwpforge_core::ObjectFrame::new(),
        children: vec![hwpforge_core::control::GroupChild::Shape(text_box)],
        width: hu(8000),
        height: hu(4000),
        placement: None,
        inst_id: None,
    };
    let section = Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::control(group, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    );
    let mut doc = Document::new();
    doc.add_section(section);
    let validated = doc.validate().expect("validate");
    let out =
        HwpxEncoder::encode(&validated, &minimal_store(), &ImageStore::new()).expect("encode");
    let xml = zip_entry(&out, "Contents/section0.xml");
    assert!(!xml.contains("__HWP"), "internal marker leaked: {xml}");
    let container = &xml[xml.find("<hp:container").expect("container")..];
    assert!(container.contains("<hp:t>A<hp:tab"), "group text box must carry the tab: {container}");
}

/// 3단 중첩: 메모 → 메모 → `"A\tB"` (HWPME → HWPME → HWPTXT). 2단 테스트만으로는
/// "pass 2회 고정" 루프가 통과한다.
// 이것을 실패시키는 것: `apply_run_xml_replacements` 의 루프를 pass 2회로 제한하기
// — 가장 안쪽 HWPTXT 가 남아 안전망 오류로 인코드가 실패한다.
#[test]
fn memo_inside_memo_tab_is_emitted() {
    let inner = Control::memo(vec![Paragraph::with_runs(
        vec![Run::text("A\tB", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )]);
    let outer = Control::memo(vec![Paragraph::with_runs(
        vec![Run::control(inner, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )]);
    let section = Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![
                Run::text("본문", CharShapeIndex::new(0)),
                Run::control(outer, CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    );
    let mut doc = Document::new();
    doc.add_section(section);
    let validated = doc.validate().expect("validate");
    let out =
        HwpxEncoder::encode(&validated, &minimal_store(), &ImageStore::new()).expect("encode");
    let xml = zip_entry(&out, "Contents/section0.xml");
    assert!(!xml.contains("__HWP"), "internal marker leaked: {xml}");
    assert_eq!(xml.matches(r#"type="MEMO""#).count(), 2, "both memos emitted: {xml}");
    let inner_sub = &xml[xml.rfind("<hp:subList").expect("inner subList")..];
    assert!(inner_sub.contains("<hp:t>A<hp:tab"), "innermost memo must carry the tab: {inner_sub}");
}
