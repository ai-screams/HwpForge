//! Defect-bearing HWP5 records against Hancom facts (no corpus dependency).
use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, PackageReader};
use quick_xml::{events::Event, Reader, XmlVersion};
use serde_json::{json, Value};
use std::path::PathBuf;
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name))
        .unwrap()
}
fn xml(bytes: &[u8]) -> String {
    PackageReader::new(bytes).unwrap().read_section_xml(0).unwrap()
}
fn convert(name: &str, carry: bool) -> Vec<u8> {
    hwp5_to_hwpx_bytes_with_diagnostics(
        &fixture(name),
        ConvertOptions::default().with_carry_layout_cache(carry),
    )
    .unwrap()
    .0
}
fn gold() -> Value {
    serde_json::from_slice(&fixture("nested_controls/hancom-facts.json")).unwrap()
}
fn compose_facts(xml: &str) -> Vec<Value> {
    let mut r = Reader::from_str(xml);
    r.config_mut().expand_empty_elements = true;
    let mut out = Vec::new();
    let mut current = None;
    loop {
        match r.read_event().unwrap() {
            Event::Start(e) if e.local_name().as_ref() == "compose" => {
                let mut value = serde_json::Map::new();
                for a in e.attributes().flatten() {
                    value.insert(a.key.as_ref().to_string(), json!(a.value));
                }
                value.insert("refs".into(), json!([]));
                current = Some(Value::Object(value));
            }
            Event::Start(e) if e.local_name().as_ref() == "charPr" && current.is_some() => {
                let id = e.attributes().flatten().find(|a| a.key.as_ref() == "prIDRef").unwrap();
                current.as_mut().unwrap()["refs"].as_array_mut().unwrap().push(json!(id.value));
            }
            Event::End(e) if e.local_name().as_ref() == "compose" => {
                out.push(current.take().unwrap())
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}
fn group_facts(xml: &str) -> Vec<Value> {
    let mut r = Reader::from_str(xml);
    r.config_mut().expand_empty_elements = true;
    let mut groups = 0;
    let mut draw = false;
    let mut text = false;
    let mut chars = String::new();
    let mut tables = 0;
    let mut out = Vec::new();
    loop {
        match r.read_event().unwrap() {
            Event::Start(e) => match e.local_name().as_ref() {
                "container" => groups += 1,
                "drawText" if groups > 0 => {
                    draw = true;
                    chars.clear();
                    tables = 0
                }
                "tbl" if draw => tables += 1,
                "t" if draw => text = true,
                _ => {}
            },
            Event::Text(t) if text => chars.push_str(&t.xml_content(XmlVersion::Explicit1_0)),
            Event::End(e) => match e.local_name().as_ref() {
                "container" => groups -= 1,
                "drawText" if draw => {
                    out.push(json!({"text":chars,"tables":tables}));
                    draw = false
                }
                "t" => text = false,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    out
}
/// 이것을 실패시키는 것: 중첩 tcps 타입화 삭제, 부호/센티널/참조 수 변경.
#[test]
fn nested_compose_matches_three_hancom_objects() {
    // The encoder pads missing references with MAX, so XML alone cannot
    // detect a reference that was lost during HWP5 projection.
    let decoded =
        hwpforge_smithy_hwp5::decode_hwp5_with_images(&fixture("nested_controls/compose-link.hwp"))
            .unwrap();
    let mut references = Vec::new();
    for section in decoded.document.sections() {
        section.for_each_paragraph(|p| {
            for run in &p.runs {
                if let hwpforge_core::RunContent::Control(c) = &run.content {
                    if let hwpforge_core::Control::Compose { char_pr_ids, .. } = c.as_ref() {
                        references.push(char_pr_ids.clone());
                    }
                }
            }
        });
    }
    let expected: Vec<Vec<u32>> = gold()["compose"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            c["refs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r.as_str().unwrap().parse().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(references, expected);
    for carry in [false, true] {
        let bytes = convert("nested_controls/compose-link.hwp", carry);
        assert_eq!(compose_facts(&xml(&bytes)), *gold()["compose"].as_array().unwrap());
    }
}
/// 이것을 실패시키는 것: 시작 위치 대신 직전 글자의 서식을 선택.
#[test]
fn nested_links_take_the_display_start_style() {
    let bytes = convert("nested_controls/compose-link.hwp", true);
    let doc = HwpxDecoder::decode(&bytes).unwrap();
    let mut found = 0;
    for section in doc.document.sections() {
        section.for_each_paragraph(|p| {
            for run in &p.runs {
                if let hwpforge_core::RunContent::Control(c) = &run.content {
                    if let hwpforge_core::Control::Hyperlink { text, .. } = c.as_ref() {
                        if text == "www.hometax.go.kr" {
                            assert_eq!(run.char_shape_id.get(), 215);
                            found += 1;
                        }
                    }
                }
            }
        });
    }
    assert_eq!(found, 3);
}
/// 이것을 실패시키는 것: 묶음 자식의 표 하위 레코드를 무시하거나 소유 문단을 바꾸기.
#[test]
fn nested_group_tables_match_hancom_147_characters() {
    for carry in [false, true] {
        let bytes = convert("nested_controls/group-table-page.hwp", carry);
        assert_eq!(group_facts(&xml(&bytes)), *gold()["group"].as_array().unwrap());
        let doc = HwpxDecoder::decode(&bytes).unwrap();
        assert_eq!(doc.document.sections()[0].paragraphs.len(), 3);
    }
}
/// 이것을 실패시키는 것: 표/그림 하위 레코드를 도형 자체 속성으로 흡수하기.
#[test]
fn nested_group_small_table_and_picture_match_hancom() {
    let bytes = convert("shapes/group-f2-textbox-table.hwp", true);
    assert_eq!(
        group_facts(&xml(&bytes)),
        group_facts(&xml(&fixture("shapes/group-f2-textbox-table.hwpx")))
    );
    assert!(group_facts(&xml(&bytes))[0]["text"].as_str().unwrap().contains("가나다라"));
    assert_eq!(xml(&bytes).matches("<hp:pic ").count(), 1);
}
fn page_number_paragraphs(xml: &str) -> Vec<(usize, usize, String)> {
    let mut r = Reader::from_str(xml);
    r.config_mut().expand_empty_elements = true;
    let mut depth = 0;
    let mut index = 0usize;
    let mut current = 0;
    let mut run_index = 0usize;
    let mut shape = String::new();
    let mut out = Vec::new();
    loop {
        match r.read_event().unwrap() {
            Event::Start(e) => {
                if e.local_name().as_ref() == "p" && depth == 1 {
                    current = index;
                    index += 1;
                    run_index = 0;
                }
                if e.local_name().as_ref() == "run" && depth == 2 {
                    run_index += 1;
                    shape = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == "charPrIDRef")
                        .unwrap()
                        .value
                        .to_string();
                }
                if e.local_name().as_ref() == "pageNum" {
                    out.push((current, run_index - 1, shape.clone()));
                }
                depth += 1;
            }
            Event::End(_) => depth -= 1,
            Event::Eof => break,
            _ => {}
        }
    }
    out
}
/// 이것을 실패시키는 것: pgnp 마커를 버리기, 첫 문단 주입을 무조건 켜기,
/// HWPX Ctrl 변환에서 pageNum 위치를 버리기. 캐시 켜고/끄기 모두 검사한다.
#[test]
fn nested_page_number_keeps_paragraph_run_and_cache_order() {
    for carry in [false, true] {
        let bytes = convert("nested_controls/group-table-page.hwp", carry);
        let before = xml(&bytes);
        assert_eq!(page_number_paragraphs(&before), [(1, 1, "97".to_string())]);
        assert!(before.contains(r#"charPrIDRef="97"><hp:ctrl><hp:pageNum"#));
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let doc = decoded.document.validate().unwrap();
        let after = HwpxEncoder::encode(&doc, &decoded.style_store, &decoded.image_store).unwrap();
        assert_eq!(page_number_paragraphs(&xml(&after)), [(1, 1, "97".to_string())]);
        if carry {
            assert!(doc.sections()[0].paragraphs[1].layout_cache.is_some());
        }
    }
}
/// Native HWPX must keep the same anchor too, not only our HWP5 output.
/// 이것을 실패시키는 것: HWPX pageNum을 구역 속성만으로 읽기.
#[test]
fn native_hwpx_page_number_stays_in_its_original_run() {
    let bytes = fixture("nested_controls/page-position-hancom.hwpx");
    let expected = page_number_paragraphs(&xml(&bytes));
    assert_eq!(expected, [(1, 1, "97".to_string())]);
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    let doc = decoded.document.validate().unwrap();
    let output = HwpxEncoder::encode(&doc, &decoded.style_store, &decoded.image_store).unwrap();
    assert_eq!(page_number_paragraphs(&xml(&output)), expected);
}
