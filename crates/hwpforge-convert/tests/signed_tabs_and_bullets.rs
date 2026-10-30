//! Synthetic wire contracts for signed HWPUNIT tabs and 23-byte bullet definitions.
use hwpforge_foundation::HeadingType;
use hwpforge_smithy_hwp5::decode_hwp5_to_core;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, PackageReader};

mod wire {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/support/hwp5.rs"));
}
use wire::{make_record, make_test_cfb};

fn synthetic_hwp(positions: &[i32], glyphs: &[char]) -> Vec<u8> {
    let mut info = make_record(21, 0, &[0; 68]); // CHAR_SHAPE
    if !positions.is_empty() {
        info.extend(make_record(22, 0, &[0; 8])); // empty TAB_DEF, ID 0
    }
    let mut tab = 0u32.to_le_bytes().to_vec();
    tab.extend((positions.len() as i32).to_le_bytes());
    for position in positions {
        tab.extend(position.to_le_bytes());
        tab.extend([0; 4]);
    }
    info.extend(make_record(22, 0, &tab));
    for glyph in glyphs {
        let mut bullet = vec![0; 23];
        bullet[12..14].copy_from_slice(&(*glyph as u16).to_le_bytes());
        info.extend(make_record(24, 0, &bullet));
    }
    let mut para = vec![0; 42];
    if !positions.is_empty() {
        para[28..30].copy_from_slice(&1u16.to_le_bytes()); // TAB_DEF ID 1
    }
    if !glyphs.is_empty() {
        para[..4].copy_from_slice(&(3u32 << 23).to_le_bytes());
        para[30..32].copy_from_slice(&1u16.to_le_bytes());
    }
    info.extend(make_record(25, 0, &para));
    info.extend(make_record(26, 0, &[0; 14]));
    let mut section = Vec::new();
    for text in ["가상 탐험대의 첫 번째 문장", "가상 탐험대의 두 번째 문장"] {
        let mut header = vec![0; 22];
        header[..4].copy_from_slice(&(text.encode_utf16().count() as u32).to_le_bytes());
        section.extend(make_record(66, 0, &header));
        section.extend(make_record(
            67,
            1,
            &text.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>(),
        ));
    }
    make_test_cfb(&info, &section)
}

fn synthetic_hwpx(positions: &[i64]) -> Vec<u8> {
    use hwpforge_core::{Document, ImageStore, PageSettings, Paragraph, Run, Section};
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    use hwpforge_smithy_hwpx::HwpxStyleStore;
    use std::io::{Cursor, Read, Write};
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("가상 탭 문장", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let base = HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &HwpxStyleStore::with_default_fonts("바탕"),
        &ImageStore::new(),
    )
    .unwrap();
    let mut header = PackageReader::new(&base).unwrap().read_header_xml().unwrap();
    let case: String = positions
        .iter()
        .map(|p| format!(r#"<hh:tabItem pos="{p}" type="LEFT" leader="NONE" unit="HWPUNIT"/>"#))
        .collect();
    let default: String = positions
        .iter()
        .map(|p| format!(r#"<hh:tabItem pos="{}" type="LEFT" leader="NONE"/>"#, p * 2))
        .collect();
    let tabs = format!(
        r#"<hh:tabProperties itemCnt="2"><hh:tabPr id="0" autoTabLeft="0" autoTabRight="0"/><hh:tabPr id="1" autoTabLeft="0" autoTabRight="0"><hp:switch><hp:case required-namespace="http://www.hancom.co.kr/hwpml/2016/HwpUnitChar">{case}</hp:case><hp:default>{default}</hp:default></hp:switch></hh:tabPr></hh:tabProperties>"#
    );
    let start = header.find("<hh:tabProperties ").unwrap();
    let end =
        start + header[start..].find("</hh:tabProperties>").unwrap() + "</hh:tabProperties>".len();
    header.replace_range(start..end, &tabs);
    header = header.replace(r#"tabPrIDRef="0""#, r#"tabPrIDRef="1""#);
    let mut input = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut entry = input.by_index(i).unwrap();
        let name = entry.name().unwrap().to_string();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        out.start_file(&name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(if name == "Contents/header.xml" { header.as_bytes() } else { &data })
            .unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn tab_positions(bytes: &[u8], id: u32) -> (Vec<i64>, Vec<i64>) {
    let xml = PackageReader::new(bytes).unwrap().read_header_xml().unwrap();
    let mut reader = quick_xml::Reader::from_str(&xml);
    let mut selected = false;
    let mut in_case = false;
    let mut in_default = false;
    let mut case = Vec::new();
    let mut default = Vec::new();
    loop {
        use quick_xml::events::Event;
        match reader.read_event().unwrap() {
            Event::Start(e) | Event::Empty(e) => {
                let attr = |name: &str| {
                    e.attributes()
                        .map(Result::unwrap)
                        .find(|a| a.key.local_name().as_ref() == name)
                        .map(|a| a.value.into_owned())
                };
                match e.local_name().as_ref() {
                    "tabPr" => selected = attr("id").unwrap().parse::<u32>().unwrap() == id,
                    "case" => {
                        in_case = attr("required-namespace").as_deref()
                            == Some("http://www.hancom.co.kr/hwpml/2016/HwpUnitChar")
                    }
                    "default" => in_default = true,
                    "tabItem" if selected && (in_case || in_default) => {
                        let pos = attr("pos").unwrap().parse().unwrap();
                        if in_case {
                            assert_eq!(attr("unit").as_deref(), Some("HWPUNIT"));
                            case.push(pos);
                        } else {
                            default.push(pos);
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) => match e.local_name().as_ref() {
                "tabPr" => selected = false,
                "case" => in_case = false,
                "default" => in_default = false,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    (case, default)
}

#[test]
fn synthetic_39_signed_tabs_match_case_and_default() {
    // 이것을 실패시키는 것: HWP5 탭의 부호 있는 packed 값 대신 unsigned 값으로 변환.
    // 실제 변이 실행: 변환 결과의 탭 ID를 0으로 고정하면 [0, 0] != [0, 1]로 실패한다.
    let case: Vec<i64> = (-19..20).map(|i| i * 1200).collect();
    let default: Vec<i64> = case.iter().map(|p| p * 2).collect();
    let expected = (case.clone(), default.clone());
    let raw = decode_hwp5_to_core(&synthetic_hwp(
        &default.iter().map(|p| *p as i32).collect::<Vec<_>>(),
        &[],
    ))
    .unwrap();
    assert_eq!(raw.style_store.tab_defs.len(), 2);
    assert!(raw.style_store.tab_defs[0].tab_def.as_ref().unwrap().tab_stops.is_empty());
    assert_eq!(raw.style_store.para_shapes[0].tab_def_id, 1);
    let raw_tab = raw.style_store.tab_defs[1].tab_def.as_ref().unwrap();
    assert_eq!(
        raw_tab.tab_stops.iter().map(|s| i64::from(s.position as i32)).collect::<Vec<_>>(),
        default
    );
    let (store, _) = hwpforge_convert::hwp5_style_store_to_hwpx(&raw.style_store).unwrap();
    assert_eq!(store.iter_tabs().map(|t| t.id).collect::<Vec<_>>(), [0, 1]);
    assert_eq!(store.iter_para_shapes().next().unwrap().tab_pr_id_ref, 1);
    assert!(store.iter_tabs().find(|t| t.id == 0).unwrap().stops.is_empty());
    assert_eq!(
        store
            .iter_tabs()
            .find(|t| t.id == 1)
            .unwrap()
            .stops
            .iter()
            .map(|s| i64::from(s.position.as_hwp_unit().unwrap().as_i32()))
            .collect::<Vec<_>>(),
        case
    );
    let output =
        HwpxEncoder::encode(&raw.document.validate().unwrap(), &store, &raw.image_store).unwrap();
    assert_eq!(tab_positions(&output, 1), expected);
    let after = HwpxDecoder::decode(&output).unwrap();
    assert_eq!(after.style_store.iter_para_shapes().next().unwrap().tab_pr_id_ref, 1);
    for id in 0..=1 {
        assert_eq!(
            after.style_store.iter_tabs().find(|t| t.id == id).unwrap(),
            store.iter_tabs().find(|t| t.id == id).unwrap()
        );
    }
    let native_bytes = synthetic_hwpx(&case);
    assert_eq!(tab_positions(&native_bytes, 1), expected);
    let native = HwpxDecoder::decode(&native_bytes).unwrap();
    assert_eq!(native.style_store.iter_para_shapes().next().unwrap().tab_pr_id_ref, 1);
    let output = HwpxEncoder::encode(
        &native.document.validate().unwrap(),
        &native.style_store,
        &native.image_store,
    )
    .unwrap();
    assert_eq!(tab_positions(&output, 1), expected);
    let after = HwpxDecoder::decode(&output).unwrap();
    assert_eq!(after.style_store.iter_para_shapes().next().unwrap().tab_pr_id_ref, 1);
    assert_eq!(
        after.style_store.iter_tabs().find(|t| t.id == 1).unwrap(),
        native.style_store.iter_tabs().find(|t| t.id == 1).unwrap()
    );
}

#[test]
fn synthetic_six_short_bullets_keep_slots_and_two_dash_paragraphs() {
    // 이것을 실패시키는 것: 23바이트 bullet 거부 또는 bullet slot 재번호화.
    let glyphs = ['-', '○', '◇', '☆', '+', '※'];
    let raw = decode_hwp5_to_core(&synthetic_hwp(&[], &glyphs)).unwrap();
    assert_eq!(raw.style_store.bullets.len(), glyphs.len());
    for (i, glyph) in glyphs.iter().enumerate() {
        let slot = &raw.style_store.bullets[i];
        assert_eq!(slot.id, i as u32 + 1);
        let bullet = slot.bullet.as_ref().unwrap();
        assert_eq!(bullet.bullet_char, glyph.to_string());
        assert_eq!(bullet.check_bullet_char, None);
    }
    let (store, warnings) = hwpforge_convert::hwp5_style_store_to_hwpx(&raw.style_store).unwrap();
    assert!(!format!("{warnings:?}").contains("bullet.slot"));
    let output =
        HwpxEncoder::encode(&raw.document.validate().unwrap(), &store, &raw.image_store).unwrap();
    let after = HwpxDecoder::decode(&output).unwrap();
    assert_eq!(
        after
            .style_store
            .iter_bullets()
            .map(|b| (b.id, b.bullet_char.clone(), b.checked_char.clone()))
            .collect::<Vec<_>>(),
        glyphs
            .iter()
            .enumerate()
            .map(|(i, g)| (i as u32 + 1, g.to_string(), None))
            .collect::<Vec<_>>()
    );
    let mut paragraphs = Vec::new();
    after.document.for_each_paragraph(|p| {
        let shape = after.style_store.para_shape(p.para_shape_id).unwrap();
        if shape.heading_type == HeadingType::Bullet && shape.heading_id_ref == 1 {
            paragraphs.push(p.text_content());
        }
    });
    assert_eq!(paragraphs, ["가상 탐험대의 첫 번째 문장", "가상 탐험대의 두 번째 문장"]);
}

/// A converted HWP5 store answers the shared typed list lookup: the
/// paragraphs on the first bullet slot read as level-0 bullets of the
/// first definition, the rest as no list.
/// 이것을 실패시키는 것: 저장소 조회에서 `idRef − 1` 대신 `idRef`.
#[test]
fn converted_bullet_paragraphs_read_as_typed_bullets() {
    use hwpforge_core::{ParagraphListRef, StyleLookup};
    use hwpforge_foundation::BulletIndex;
    let glyphs = ['-', '○'];
    let raw = decode_hwp5_to_core(&synthetic_hwp(&[], &glyphs)).unwrap();
    let (store, _) = hwpforge_convert::hwp5_style_store_to_hwpx(&raw.style_store).unwrap();
    let mut lists = Vec::new();
    raw.document.for_each_paragraph(|p| {
        if let Some(list) = store.para_list(p.para_shape_id) {
            lists.push((p.text_content(), list));
        }
    });
    let bullet = ParagraphListRef::Bullet { bullet_id: BulletIndex::new(0), level: 0 };
    assert_eq!(
        lists,
        [
            ("가상 탐험대의 첫 번째 문장".to_string(), bullet),
            ("가상 탐험대의 두 번째 문장".to_string(), bullet),
        ]
    );
}
