//! Hancom evidence for signed HWPUNIT tabs and 23-byte bullet definitions.
use hwpforge_foundation::HeadingType;
use hwpforge_smithy_hwp5::decode_hwp5_to_core;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, PackageReader};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/hwp5/w1-fix1")
            .join(name),
    )
    .unwrap()
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
fn fix1_native_39_tab_positions_match_hancom_case_and_default() {
    // 이것을 실패시키는 것: HWP5 unsigned 해석, 음수 clamp/삭제, default unsigned 출력.
    for (index, source_id, twin, twin_id, first) in [
        (74, 2, "fsc.hwpx", 0, -130144),
        (32, 3, "moe.hwpx", 0, -130144),
        (226, 7, "mss.hwpx", 0, -130208),
    ] {
        let twin_bytes = fixture(twin);
        let expected = tab_positions(&twin_bytes, twin_id);
        assert_eq!(expected.0.len(), 39);
        assert_eq!(expected.1.len(), 39);
        assert_eq!(expected.0[0], first);
        assert!(expected.0.iter().zip(&expected.1).all(|(c, d)| c * 2 == *d));
        let raw = decode_hwp5_to_core(&fixture(&format!("hwp5-{index}.hwp"))).unwrap();
        let raw_tab = raw
            .style_store
            .tab_defs
            .iter()
            .find(|t| t.raw_id == source_id)
            .unwrap()
            .tab_def
            .as_ref()
            .unwrap();
        assert_eq!(
            raw_tab.tab_stops.iter().map(|s| i64::from(s.position as i32)).collect::<Vec<_>>(),
            expected.1
        );
        let (store, _) = hwpforge_convert::hwp5_style_store_to_hwpx(&raw.style_store).unwrap();
        let projected: Vec<_> = store
            .iter_tabs()
            .find(|t| t.id == source_id)
            .unwrap()
            .stops
            .iter()
            .map(|s| i64::from(s.position.as_hwp_unit().unwrap().as_i32()))
            .collect();
        assert_eq!(projected, expected.0);
        let output =
            HwpxEncoder::encode(&raw.document.validate().unwrap(), &store, &raw.image_store)
                .unwrap();
        assert_eq!(tab_positions(&output, source_id), expected);
        let after = HwpxDecoder::decode(&output).unwrap();
        assert_eq!(
            after.style_store.iter_tabs().find(|t| t.id == source_id),
            store.iter_tabs().find(|t| t.id == source_id)
        );
        // 이것을 실패시키는 것: 한컴 원본의 음수 case 읽기/쓰기 거부.
        let native = HwpxDecoder::decode(&twin_bytes).unwrap();
        let output = HwpxEncoder::encode(
            &native.document.validate().unwrap(),
            &native.style_store,
            &native.image_store,
        )
        .unwrap();
        assert_eq!(tab_positions(&output, twin_id), expected);
    }
}

#[test]
fn fix1_six_short_bullets_keep_slots_and_two_dash_paragraphs() {
    // 이것을 실패시키는 것: 23바이트 거부, bullet slot 재번호화/누락, 문단 heading 연결 손실.
    let mut total = 0;
    for (index, expected_glyphs) in
        [(33, vec!["◦"]), (34, vec!["-", "◦"]), (35, vec!["◦"]), (36, vec!["◦"]), (37, vec!["◦"])]
    {
        let raw = decode_hwp5_to_core(&fixture(&format!("hwp5-{index}.hwp"))).unwrap();
        assert_eq!(raw.style_store.bullets.len(), expected_glyphs.len());
        for (i, glyph) in expected_glyphs.iter().enumerate() {
            let slot = &raw.style_store.bullets[i];
            assert_eq!(slot.id, i as u32 + 1);
            let bullet = slot.bullet.as_ref().unwrap();
            assert_eq!(&bullet.bullet_char, glyph);
            assert_eq!(bullet.check_bullet_char, None);
            total += 1;
        }
        let (store, warnings) =
            hwpforge_convert::hwp5_style_store_to_hwpx(&raw.style_store).unwrap();
        assert!(!format!("{warnings:?}").contains("bullet.slot"));
        let output =
            HwpxEncoder::encode(&raw.document.validate().unwrap(), &store, &raw.image_store)
                .unwrap();
        let after = HwpxDecoder::decode(&output).unwrap();
        let bullets: Vec<_> = after
            .style_store
            .iter_bullets()
            .map(|b| (b.id, b.bullet_char.as_str(), b.checked_char.as_deref()))
            .collect();
        assert_eq!(
            bullets,
            expected_glyphs
                .iter()
                .enumerate()
                .map(|(i, g)| (i as u32 + 1, *g, None))
                .collect::<Vec<_>>()
        );
        if index == 34 {
            let mut paragraphs = Vec::new();
            after.document.for_each_paragraph(|p| {
                let shape = after.style_store.para_shape(p.para_shape_id).unwrap();
                if shape.heading_type == HeadingType::Bullet && shape.heading_id_ref == 1 {
                    // HWP5 slot 60 remains position 60; no redundant source_id.
                    paragraphs.push(p.text_content());
                }
            });
            assert_eq!(paragraphs.len(), 2);
            assert!(paragraphs.iter().any(|p| p.contains("가상 문장 한 줄을 남긴 문단")));
        }
    }
    assert_eq!(total, 6);
}
