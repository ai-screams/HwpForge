//! Purpose-made Hancom twins, compared with independent section XML oracles.
use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, PackageReader};
use quick_xml::{events::Event, Reader, XmlVersion};
use serde_json::{json, Value};
use std::io::{Cursor, Read, Write};

fn fixture(name: &str, ext: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../tests/fixtures/hancom_controls/{name}.{ext}")),
    )
    .unwrap()
}
fn xml(bytes: &[u8]) -> String {
    PackageReader::new(bytes).unwrap().read_section_xml(0).unwrap()
}
fn convert(name: &str, carry: bool) -> Vec<u8> {
    convert_bytes(&fixture(name, "hwp"), carry)
}
fn convert_bytes(bytes: &[u8], carry: bool) -> Vec<u8> {
    hwp5_to_hwpx_bytes_with_diagnostics(
        bytes,
        ConvertOptions::default().with_carry_layout_cache(carry),
    )
    .unwrap()
    .0
}
fn roundtrip(bytes: &[u8]) -> Vec<u8> {
    let decoded = HwpxDecoder::decode(bytes).unwrap();
    HwpxEncoder::encode(
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
    )
    .unwrap()
}
fn rewrite_section(bytes: &[u8], section: &str) -> Vec<u8> {
    let mut input = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut entry = input.by_index(i).unwrap();
        let name = entry.name().unwrap().to_string();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        output.start_file(&name, zip::write::SimpleFileOptions::default()).unwrap();
        output
            .write_all(if name == "Contents/section0.xml" { section.as_bytes() } else { &data })
            .unwrap();
    }
    output.finish().unwrap().into_inner()
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
fn top_level_paragraph_count(xml: &str) -> usize {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut depth = 0;
    let mut count = 0;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) => {
                if depth == 1 && e.local_name().as_ref() == "p" {
                    count += 1;
                }
                depth += 1;
            }
            Event::End(_) => depth -= 1,
            Event::Eof => return count,
            _ => {}
        }
    }
}

fn group_facts(xml: &str) -> Vec<Value> {
    let mut r = Reader::from_str(xml);
    r.config_mut().expand_empty_elements = true;
    let mut groups = 0;
    let mut depth = 0;
    let mut owner = 0usize;
    let mut draw = false;
    let mut text = false;
    let mut chars = String::new();
    let mut tables = 0;
    let mut out = Vec::new();
    loop {
        match r.read_event().unwrap() {
            Event::Start(e) => {
                if depth == 1 && e.local_name().as_ref() == "p" {
                    owner += 1;
                }
                depth += 1;
                match e.local_name().as_ref() {
                    "container" => groups += 1,
                    "drawText" if groups > 0 => {
                        draw = true;
                        chars.clear();
                        tables = 0
                    }
                    "tbl" if draw => tables += 1,
                    "t" if draw => text = true,
                    _ => {}
                }
            }
            Event::Text(t) if text => chars.push_str(&t.xml_content(XmlVersion::Explicit1_0)),
            Event::End(e) => {
                depth -= 1;
                match e.local_name().as_ref() {
                    "container" => groups -= 1,
                    "drawText" if draw => {
                        out.push(json!({"text":chars,"tables":tables,"owner":owner - 1}));
                        draw = false
                    }
                    "t" => text = false,
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
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

// Field bodies are the hp:t events between begin/end, not text in parameters.
// Normalize adjacent same-style pieces; keep every actual style boundary.
fn field_facts(xml: &str, kind: &str) -> Vec<Value> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut shape = String::new();
    let mut current: Option<Value> = None;
    let mut parameter = None;
    let mut text = false;
    let mut out = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) => {
                let attr = |key: &str| {
                    e.attributes()
                        .map(Result::unwrap)
                        .find(|a| a.key.as_ref() == key)
                        .map(|a| a.value.to_string())
                        .unwrap_or_default()
                };
                match e.local_name().as_ref() {
                    "run" => shape = attr("charPrIDRef"),
                    "fieldBegin" if attr("type") == kind => {
                        current = Some(
                            json!({"name":attr("name"), "editable":attr("editable"), "spans":[], "params":{}}),
                        );
                    }
                    "stringParam" if current.is_some() => parameter = Some(attr("name")),
                    "t" => text = true,
                    "fieldEnd" => {
                        if let Some(value) = current.take() {
                            out.push(value);
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(t) => {
                let value = t.xml_content(XmlVersion::Explicit1_0).to_string();
                if let Some(field) = current.as_mut() {
                    if let Some(key) = &parameter {
                        // Only metadata shared by both wire formats is compared.
                        if kind == "CLICK_HERE" && ["Command", "Direction"].contains(&key.as_str())
                        {
                            field["params"][key] = json!(value);
                        }
                    } else if text && !value.is_empty() {
                        let spans = field["spans"].as_array_mut().unwrap();
                        if let Some(last) = spans.last_mut().filter(|last| last["shape"] == shape) {
                            last["text"] =
                                json!(format!("{}{}", last["text"].as_str().unwrap(), value));
                        } else {
                            spans.push(json!({"shape":shape,"text":value}));
                        }
                    }
                }
            }
            Event::End(e) => match e.local_name().as_ref() {
                "stringParam" => parameter = None,
                "t" => text = false,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

/// 이것을 실패시키는 것: 중첩 tcps 타입화 삭제, 음수 크기의 부호 제거,
/// MAX 센티널 변경, 참조 벡터 축소(encoder의 MAX padding으로 가려지는 손실),
/// 길이값을 2/3으로 다시 제한하여 f2의 efg(low=4)를 누락하기.
#[test]
fn nested_compose_matches_three_hancom_objects() {
    let expected = compose_facts(&xml(&fixture("f2-compose-links", "hwpx")));
    assert_eq!(expected.len(), 3); // f2: ab, cd, efg, each a Hancom compose.
    let decoded =
        hwpforge_smithy_hwp5::decode_hwp5_with_images(&fixture("f2-compose-links", "hwp")).unwrap();
    let mut refs = Vec::new();
    for section in decoded.document.sections() {
        section.for_each_paragraph(|p| {
            for run in &p.runs {
                if let hwpforge_core::RunContent::Control(c) = &run.content {
                    if let hwpforge_core::Control::Compose { char_pr_ids, .. } = c.as_ref() {
                        refs.push(char_pr_ids.clone());
                    }
                }
            }
        });
    }
    let expected_refs: Vec<Vec<u32>> = expected
        .iter()
        .map(|c| {
            c["refs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().parse().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(refs, expected_refs);
    for carry in [false, true] {
        assert_eq!(compose_facts(&xml(&convert("f2-compose-links", carry))), expected);
    }
    assert_eq!(compose_facts(&xml(&roundtrip(&fixture("f2-compose-links", "hwpx")))), expected);
}

/// 이것을 실패시키는 것: 링크 시작 위치 대신 직전 글자의 서식을 선택.
#[test]
fn nested_links_take_the_display_start_style() {
    let expected = field_facts(&xml(&fixture("f2-compose-links", "hwpx")), "HYPERLINK");
    assert_eq!(expected.len(), 3); // f2: links a/b/c, all display text in its own 10pt run.
    for carry in [false, true] {
        let actual = field_facts(&xml(&convert("f2-compose-links", carry)), "HYPERLINK");
        assert_eq!(actual, expected);
    }
}

/// 이것을 실패시키는 것: 묶음 자식 표의 하위 레코드 무시 / 소유 문단 이동.
/// 실제 nextest 변이 확인: 변환 XML과 왕복 XML 끝에 각각 <hp:p/>를 추가하면
/// group_facts는 통과하지만 해당 문단 수 단언은 각각 13 != 12로 실패한다.
#[test]
fn nested_group_tables_match_hancom_text_and_owners() {
    let source_xml = xml(&fixture("f3-group-tables", "hwpx"));
    let expected = group_facts(&source_xml);
    let paragraphs = top_level_paragraph_count(&source_xml);
    assert_eq!(expected.len(), 4); // f3: three 2x2 textboxes, then one 1x1 textbox.
    for carry in [false, true] {
        let bytes = convert("f3-group-tables", carry);
        let converted_xml = xml(&bytes);
        assert_eq!(group_facts(&converted_xml), expected);
        assert_eq!(top_level_paragraph_count(&converted_xml), paragraphs);
        let roundtrip_xml = xml(&roundtrip(&bytes));
        assert_eq!(group_facts(&roundtrip_xml), expected);
        assert_eq!(top_level_paragraph_count(&roundtrip_xml), paragraphs);
    }
}

/// 이것을 실패시키는 것: 표/그림 하위 레코드를 도형 자체 속성으로 흡수하기.
/// 실제 nextest 변이 확인: HwpxDecoder의 반환 image_store를 공통으로 비우면
/// XML pic 수 단언은 통과하지만 ZIP BinData 기대값과의 비교는 0 != 1로 실패한다.
#[test]
fn nested_group_small_table_and_picture_match_hancom() {
    let source = fixture("f3-group-tables", "hwpx");
    let expected = group_facts(&xml(&source));
    // This fixture's BinData entries are images; exclude ZIP directory entries and previews.
    let mut archive = zip::ZipArchive::new(Cursor::new(&source)).unwrap();
    let images = (0..archive.len())
        .filter(|&i| {
            let entry = archive.by_index(i).unwrap();
            !entry.is_dir() && entry.name().unwrap().starts_with("BinData/")
        })
        .count();
    let bytes = convert("f3-group-tables", true);
    assert_eq!(group_facts(&xml(&bytes)), expected);
    // f3's second group contains one textbox with 가나다라 and one mascot.
    assert_eq!(expected.last().unwrap()["text"], "가나다라");
    assert_eq!(xml(&bytes).matches("<hp:pic ").count(), xml(&source).matches("<hp:pic ").count());
    assert_eq!(HwpxDecoder::decode(&bytes).unwrap().image_store.len(), images);
}

fn assert_page_roundtrip(bytes: &[u8], expected: &[(usize, usize, String)], carry: bool) {
    assert_eq!(page_number_paragraphs(&xml(bytes)), expected);
    let decoded = HwpxDecoder::decode(bytes).unwrap();
    let owner = expected[0].0;
    assert_eq!(decoded.document.sections()[0].paragraphs[owner].layout_cache.is_some(), carry);
    assert_eq!(page_number_paragraphs(&xml(&roundtrip(bytes))), expected);
}

/// 이것을 실패시키는 것: pgnp 마커 버리기 / HWPX Ctrl 위치 버리기 / 캐시 누락.
#[test]
fn nested_page_number_keeps_paragraph_run_and_cache_order() {
    let expected = page_number_paragraphs(&xml(&fixture("f4-page-number", "hwpx")));
    for carry in [false, true] {
        assert_page_roundtrip(&convert("f4-page-number", carry), &expected, carry);
    }
}

/// 이것을 실패시키는 것: pageNum을 구역 속성만으로 읽기 / 첫 문단 주입 무조건 켜기.
#[test]
fn native_hwpx_page_number_stays_in_its_original_run() {
    let source = fixture("f4-page-number", "hwpx");
    let expected = page_number_paragraphs(&xml(&source));
    assert_page_roundtrip(&source, &expected, true);
    let mut section = xml(&source);
    let start = section.find("<hp:ctrl><hp:pageNum ").unwrap();
    let end = start + section[start..].find("</hp:ctrl>").unwrap() + "</hp:ctrl>".len();
    let control = section[start..end].to_string();
    section.replace_range(start..end, "");
    // Use an existing distinct style from this document, not an old corpus id.
    let header = PackageReader::new(&source).unwrap().read_header_xml().unwrap();
    let mut reader = Reader::from_str(&header);
    let shape = loop {
        match reader.read_event().unwrap() {
            Event::Start(e) if e.local_name().as_ref() == "charPr" => {
                let id = e
                    .attributes()
                    .map(Result::unwrap)
                    .find(|a| a.key.as_ref() == "id")
                    .unwrap()
                    .value
                    .to_string();
                if id != expected[0].2 {
                    break id;
                }
            }
            Event::Eof => panic!("distinct style"),
            _ => {}
        }
    };
    let second = section.find("</hp:p>").unwrap() + "</hp:p>".len();
    let end_run = second + section[second..].find("</hp:run>").unwrap() + "</hp:run>".len();
    section.insert_str(end_run, &format!(r#"<hp:run charPrIDRef="{shape}">{control}</hp:run>"#));
    let moved = rewrite_section(&source, &section);
    let expected = vec![(1, 1, shape)]; // Explicit test transformation: paragraph 2, second run.
    assert_page_roundtrip(&moved, &expected, true);
}

/// 이것을 실패시키는 것: 채워진 본문 비우기 / 다중 서식 분기 끄기 /
/// 이름·안내문 버리기 / editable을 0으로 변경하기.
#[test]
fn hancom_clickhere_preserves_filled_body_styles_and_metadata() {
    let source = fixture("f5-clickhere", "hwpx");
    let expected = field_facts(&xml(&source), "CLICK_HERE");
    assert_eq!(expected.len(), 3); // f5: addr, mail, rich.
    assert_eq!(expected[2]["spans"].as_array().unwrap().len(), 4); // Hancom's normal/bold/16pt/normal.
    assert_eq!(field_facts(&xml(&roundtrip(&source)), "CLICK_HERE"), expected);
    for carry in [false, true] {
        let output = convert("f5-clickhere", carry);
        assert_eq!(field_facts(&xml(&output), "CLICK_HERE"), expected);
        assert_eq!(field_facts(&xml(&roundtrip(&output)), "CLICK_HERE"), expected);
    }
}

mod hwp5_wire {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/support/hwp5.rs"));
}

/// 이것을 실패시키는 것: HWP5 pgnp 버리기 / 시작 서식 대신 직전 서식 선택 /
/// 첫 문단 주입 무조건 켜기. 합성 TLV의 문단·문자 위치가 독립 입력 오라클이다.
#[test]
fn synthetic_hwp5_page_number_keeps_second_paragraph_style() {
    use hwp5_wire::{make_record, make_test_cfb};
    let mut info = make_record(21, 0, &[0; 68]);
    info.extend(make_record(21, 0, &[0; 68]));
    info.extend(make_record(25, 0, &[0; 42]));
    info.extend(make_record(26, 0, &[0; 14]));
    let mut section = Vec::new();
    for index in 0..3 {
        let mut text: Vec<u8> = "앞".encode_utf16().flat_map(u16::to_le_bytes).collect();
        if index == 1 {
            text.extend(0x15u16.to_le_bytes());
            text.extend(u32::from_be_bytes(*b"pgnp").to_le_bytes());
            text.extend([0; 8]);
            text.extend(0x15u16.to_le_bytes());
        }
        text.extend(0x0du16.to_le_bytes());
        let mut header = vec![0; 22];
        header[..4].copy_from_slice(&((text.len() / 2) as u32).to_le_bytes());
        header[12..14].copy_from_slice(&(if index == 1 { 2u16 } else { 1 }).to_le_bytes());
        section.extend(make_record(66, 0, &header));
        section.extend(make_record(67, 1, &text));
        let mut shapes = vec![0; 8];
        if index == 1 {
            shapes.extend(1u32.to_le_bytes());
            shapes.extend(1u32.to_le_bytes());
        }
        section.extend(make_record(68, 1, &shapes));
        if index == 1 {
            let mut page = u32::from_be_bytes(*b"pgnp").to_le_bytes().to_vec();
            page.extend(0x500u32.to_le_bytes());
            page.extend([0; 6]);
            page.extend(('-' as u16).to_le_bytes());
            section.extend(make_record(71, 1, &page));
        }
    }
    let hwp = make_test_cfb(&info, &section);
    for carry in [false, true] {
        let output = convert_bytes(&hwp, carry);
        let expected = [(1, 1, "1".into())]; // paragraph 2, after 앞, char-shape record at UTF-16 offset 1.
        assert_page_roundtrip(&output, &expected, false); // synthetic source has no line cache.
        let native = xml(&fixture("f4-page-number", "hwpx"));
        let start = native.find("<hp:pageNum ").unwrap();
        let end = start + native[start..].find("/>").unwrap() + 2;
        assert!(xml(&output).contains(&native[start..end]));
    }
}
