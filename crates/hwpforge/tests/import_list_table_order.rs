#![cfg(feature = "ops-docx")]
#[path = "support/docx.rs"]
mod docx;
#[path = "../../hwpforge-smithy-hwpx/tests/support/list_counter_output.rs"]
mod list_counter_output;
#[path = "support/list_table_inputs.rs"]
mod list_table_inputs;
#[path = "../../hwpforge-smithy-hwpx/tests/support/package.rs"]
mod package;
#[path = "../../hwpforge-smithy-hwpx/tests/support/xml.rs"]
mod xml;
use hwpforge::{
    core::RunContent,
    hwpx::{EncodeOptions, EncodeWarning, HwpxDecoder, HwpxEncoder},
    ops::{convert_docx, OpsWarning},
};
use list_table_inputs::{cases, item, source, table};
use std::collections::BTreeMap;
fn loss(warnings: &[OpsWarning], name: &str) -> u64 {
    warnings
        .iter()
        .filter_map(|w| match w {
            OpsWarning::Docx(w) if w.loss() == name => Some(w.count()),
            _ => None,
        })
        .sum()
}
fn own_text(p: &xml::Element) -> String {
    p.children
        .iter()
        .filter(|r| r.name == "hp:run")
        .flat_map(|r| &r.children)
        .filter(|t| t.name == "hp:t")
        .map(|t| t.text.as_str())
        .collect()
}
fn values(bytes: &[u8]) -> BTreeMap<String, String> {
    let rows = list_counter_output::rendered_items(bytes);
    let map: BTreeMap<_, _> = rows.iter().cloned().collect();
    assert_eq!(map.len(), rows.len());
    map
}
fn expected(literals: &[&str]) -> BTreeMap<String, String> {
    literals
        .iter()
        .enumerate()
        .map(|(i, v)| (char::from(b'a' + i as u8).to_string(), v.to_string()))
        .collect()
}
fn no_internal_attributes(node: &xml::Element) {
    assert!(!node
        .attributes
        .keys()
        .any(|key| key.starts_with("hwpforge") || key.starts_with("pending")));
    for child in &node.children {
        no_internal_attributes(child);
    }
}
// 이것을 실패시키는 것: 부모보다 칸 먼저 배정, 연속 표 역전, 중첩 표 누락, 번호를 본문 글자로 삽입.
#[test]
fn table_numbering_xml_keeps_values_references_and_ownership() {
    for case in cases() {
        let input = source(&case.body);
        let core = hwpforge::docx::decode_docx(&input).unwrap();
        assert_eq!(core.registry.numberings.len(), case.definitions, "{}", case.name);
        let converted = convert_docx(&input).unwrap();
        assert_eq!(values(&converted.bytes), expected(case.expected), "{}", case.name);
        assert_eq!(loss(&converted.warnings, "list_numbering_split"), case.splits);
        for name in ["list_numbering_budget", "list_number_zero", "list_number_legal"] {
            assert_eq!(loss(&converted.warnings, name), 0);
        }
        assert!(!converted.warnings.iter().any(|w| matches!(
            w,
            OpsWarning::Encode(EncodeWarning::ListNumberingSplit { .. })
                | OpsWarning::Encode(EncodeWarning::ContentLost { .. })
        )));
        let header = xml::Element::parse(&package::part(&converted.bytes, "Contents/header.xml"));
        let section =
            xml::Element::parse(&package::part(&converted.bytes, "Contents/section0.xml"));
        assert_eq!(header.descendants("hh:numbering").len(), case.definitions + 1);
        let mut hosts = vec![];
        for p in section.descendants("hp:p") {
            let tables: Vec<_> = p
                .children
                .iter()
                .filter(|r| r.name == "hp:run")
                .flat_map(|r| &r.children)
                .filter(|n| n.name == "hp:tbl")
                .collect();
            if !tables.is_empty() {
                assert_eq!(p.children[0].name, "hp:run");
                assert!(p.children[0].children.iter().any(|n| n.name == "hp:tbl"));
                hosts.push((own_text(p), tables.len()));
            }
            let shape = header
                .descendants("hh:paraPr")
                .into_iter()
                .find(|s| s.attr("id") == p.attr("paraPrIDRef"))
                .unwrap();
            let heading = shape.child("hh:heading");
            if heading.attr("type") == Some("NUMBER") {
                assert_ne!(heading.attr("idRef"), Some("1"));
                assert_eq!(
                    heading.attr("level"),
                    Some(if case.name == "cell paragraphs" && own_text(p) == "d" {
                        "1"
                    } else {
                        "0"
                    })
                );
            }
        }
        assert_eq!(
            hosts,
            case.hosts.iter().map(|(text, count)| (text.to_string(), *count)).collect::<Vec<_>>()
        );
        no_internal_attributes(&header);
        no_internal_attributes(&section);
        // The XML evaluator itself follows parent-first consumption.
        assert_eq!(list_counter_output::rendered(&converted.bytes).len(), case.expected.len());
    }
}
// 이것을 실패시키는 것: 독립 abstract 병합, 예약 개요를 일반 목록으로 사용, 표현 분리를 값 분리로 오인.
#[test]
fn combined_table_lists_have_fixed_starts_without_representation_splits() {
    let mut body = String::new();
    let mut defs = String::new();
    let mut literals = BTreeMap::new();
    for (i, case) in cases().into_iter().take(8).enumerate() {
        let mut part = case.body;
        for key in ['a', 'b', 'c', 'd', 'e', 'f', 'g'] {
            part = part.replace(&format!(">{key}</w:t>"), &format!(">{} {key}</w:t>", case.name));
        }
        let mut numbering = list_table_inputs::definitions();
        for id in [1, 2] {
            let new = 101 + i * 2 + id;
            part = part
                .replace(&format!("<w:numId w:val='{id}'/>"), &format!("<w:numId w:val='{new}'/>"));
            numbering = numbering
                .replace(&format!("w:abstractNumId='{id}'"), &format!("w:abstractNumId='{new}'"))
                .replace(
                    &format!("<w:abstractNumId w:val='{id}'/>"),
                    &format!("<w:abstractNumId w:val='{new}'/>"),
                )
                .replace(&format!("w:numId='{id}'"), &format!("w:numId='{new}'"));
        }
        for (key, value) in expected(case.expected) {
            literals.insert(format!("{} {key}", case.name), value);
        }
        body.push_str(&part);
        defs.push_str(&numbering);
    }
    let input = docx::package_with_numbering(&body, &defs);
    let decoded = hwpforge::docx::decode_docx(&input).unwrap();
    assert_eq!(decoded.registry.numberings.len(), 19);
    let out = convert_docx(&input).unwrap();
    assert_eq!(values(&out.bytes), literals);
    assert_eq!(loss(&out.warnings, "list_numbering_split"), 10);
    assert!(!out
        .warnings
        .iter()
        .any(|w| matches!(w, OpsWarning::Encode(EncodeWarning::ListNumberingSplit { .. }))));
    assert_eq!(
        xml::Element::parse(&package::part(&out.bytes, "Contents/header.xml"))
            .descendants("hh:numbering")
            .len(),
        20
    );
}
fn reencode(
    bytes: &[u8],
    edit: impl FnOnce(&mut hwpforge::core::Document<hwpforge::core::Draft>),
) -> Vec<u8> {
    let mut decoded = HwpxDecoder::decode(bytes).unwrap();
    edit(&mut decoded.document);
    HwpxEncoder::encode_with_diagnostics(
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
        EncodeOptions::default(),
    )
    .unwrap()
    .bytes
}
// 이것을 실패시키는 것: 구간 사이 시작값 자동 전파, 같은 구간 자동 번호 제거, 왕복에서 정의 참조 상실.
#[test]
fn edits_advance_only_their_fixed_numbering_segment() {
    let case = cases().remove(0);
    let out = convert_docx(&source(&case.body)).unwrap();
    assert_eq!(values(&reencode(&out.bytes, |_| {})), expected(case.expected));
    let insert = |paragraphs: &mut Vec<hwpforge::core::Paragraph>, index: usize| {
        let mut added = paragraphs[index].clone();
        let run = added.runs.iter_mut().find(|r| matches!(r.content, RunContent::Text(_))).unwrap();
        run.content = RunContent::Text("inserted".into());
        paragraphs.insert(index, added);
    };
    let body_edit =
        reencode(&out.bytes, |document| insert(&mut document.sections_mut()[0].paragraphs, 0));
    let mut want = expected(&["2.", "2.", "3.", "4.", "5."]);
    want.insert("inserted".into(), "1.".into());
    assert_eq!(values(&body_edit), want);
    let cell_edit = reencode(&out.bytes, |document| {
        let host = &mut document.sections_mut()[0].paragraphs[1];
        let RunContent::Table(table) = &mut host.runs[0].content else { panic!("host table") };
        insert(&mut table.rows[0].cells[0].paragraphs, 0);
    });
    let mut want = expected(&["1.", "3.", "4.", "4.", "5."]);
    want.insert("inserted".into(), "2.".into());
    assert_eq!(values(&cell_edit), want);
    let imported =
        convert_docx(&source(&format!("{}{}", item(1, 0, "inserted"), case.body))).unwrap();
    let mut want = expected(&["2.", "3.", "4.", "5.", "6."]);
    want.insert("inserted".into(), "1.".into());
    assert_eq!(values(&imported.bytes), want);
}
// 이것을 실패시키는 것: 두 칸의 표식 굵기 차이를 무시하거나 표현 분리 시 값·시작값을 다시 1로 설정.
#[test]
fn value_segments_and_marker_representation_segments_compose() {
    let bold = item(1, 0, "c").replace("</w:pPr>", "<w:rPr><w:b/></w:rPr></w:pPr>");
    let body = format!(
        "{}{}{}{}",
        item(1, 0, "a"),
        table(&[vec![item(1, 0, "b"), bold]]),
        item(1, 0, "d"),
        item(1, 0, "e")
    );
    let input = source(&body);
    assert_eq!(hwpforge::docx::decode_docx(&input).unwrap().registry.numberings.len(), 4);
    let out = convert_docx(&input).unwrap();
    assert_eq!(values(&out.bytes), expected(&["1.", "2.", "3.", "4.", "5."]));
    assert_eq!(loss(&out.warnings, "list_numbering_split"), 3);
    assert!(out
        .warnings
        .iter()
        .any(|w| matches!(w, OpsWarning::Encode(EncodeWarning::ListNumberingSplit { .. }))));
    assert!(!out.warnings.iter().any(|w|matches!(w,OpsWarning::Encode(EncodeWarning::ContentLost{kind,..}) if kind=="list_marker_conflict")));
    assert_eq!(values(&reencode(&out.bytes, |_| {})), expected(&["1.", "2.", "3.", "4.", "5."]));
}
