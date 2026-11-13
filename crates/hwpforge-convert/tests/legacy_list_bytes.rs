#[path = "../../hwpforge-smithy-hwpx/tests/support/package.rs"]
mod package;
#[path = "../../hwpforge-smithy-hwpx/tests/support/xml.rs"]
mod xml;

#[path = "../../../tests/support/hwp5.rs"]
mod generated_hwp5;

fn numbered_hwp5() -> Vec<u8> {
    use generated_hwp5::{make_record, make_test_cfb};
    let mut numbering = vec![];
    for level in 1..=7 {
        numbering.extend(12u32.to_le_bytes()); // auto-indent and actual marker width
        numbering.extend(0i16.to_le_bytes());
        numbering.extend(50i16.to_le_bytes());
        numbering.extend(0u32.to_le_bytes());
        let text: Vec<_> = format!("^{level}.").encode_utf16().collect();
        numbering.extend((text.len() as u16).to_le_bytes());
        numbering.extend(text.into_iter().flat_map(u16::to_le_bytes));
    }
    numbering.extend(1u16.to_le_bytes());
    numbering.extend([1u32; 7].into_iter().flat_map(u32::to_le_bytes));
    let mut para = vec![0; 42];
    para[0..4].copy_from_slice(&(2u32 << 23).to_le_bytes());
    para[4..8].copy_from_slice(&1800i32.to_le_bytes());
    para[30..32].copy_from_slice(&1u16.to_le_bytes());
    let info = [
        make_record(0x15, 0, &[0; 68]),
        make_record(0x17, 0, &numbering),
        make_record(0x19, 0, &para),
        make_record(0x1a, 0, &[0; 14]),
    ]
    .concat();
    let mut section = vec![];
    for text in ["one", "two"] {
        let mut header = vec![0; 22];
        header[..4].copy_from_slice(&(text.encode_utf16().count() as u32).to_le_bytes());
        let units: Vec<_> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        section.extend(make_record(0x42, 0, &header));
        section.extend(make_record(0x43, 1, &units));
        section.extend(make_record(0x44, 1, &[0; 8]));
    }
    make_test_cfb(&info, &section)
}

// 구현 전 CLI로 같은 생성 입력을 변환한 해시다.
// Markdown 입력은 아래 문자열로 `hwpforge convert input.md --preset <preset> -o output.hwpx`,
// HWP5 입력은 numbered_hwp5()로 생성하여 `hwpforge convert-hwp5 input.hwp -o output.hwpx`.
// 이것을 실패시키는 것: 미설정 목록의 기본 폭·자동 간격 또는 생성 HWP5 목록 참조 제거.
#[test]
fn legacy_list_outputs_are_byte_stable() {
    let (bytes, _) = hwpforge_convert::hwp5_to_hwpx_bytes(&numbered_hwp5()).unwrap();
    let h = xml::Element::parse(&package::part(&bytes, "Contents/header.xml"));
    let n = h.descendants("hh:numbering").into_iter().find(|n| n.attr("id") == Some("1")).unwrap();
    let head =
        n.descendants("hh:paraHead").into_iter().find(|h| h.attr("level") == Some("1")).unwrap();
    assert_eq!(head.attr("useInstWidth"), Some("1"));
    assert_eq!(head.attr("textOffset"), Some("50"));
    let para = h.descendants("hh:paraPr").into_iter().find(|p| p.attr("id") == Some("0")).unwrap();
    assert_eq!(para.child("hh:heading").attr("type"), Some("NUMBER"));
    assert_eq!(para.child("hh:heading").attr("idRef"), Some("1"));
    assert_legacy_bytes(
        "HWP5",
        &bytes,
        9288367002320471023,
        &legacy_parts(11079467270218028339, 13958206635349407338),
    );
    for (preset, expected, header) in [
        ("default", 14924076433248044357_u64, 8052948759923284125),
        ("modern", 11652043317930802992, 4668469003302858826),
        ("classic", 8103332286910503263, 6322301246991677415),
    ] {
        let out = hwpforge::ops::convert_md(
            "1. one\n2. two\n\n- circle\n\n    continuation\n",
            None,
            &hwpforge::ops::ConvertMdOptions::default().with_preset(preset),
        )
        .unwrap();
        assert_legacy_bytes(
            preset,
            &out.bytes,
            expected,
            &legacy_parts(header, 15684901302956583263),
        );
    }
}

fn legacy_parts(header: u64, section: u64) -> [(&'static str, u64); 6] {
    [
        ("Contents/header.xml", header),
        ("Contents/section0.xml", section),
        ("META-INF/container.xml", 4559238386486593401),
        ("META-INF/manifest.xml", 15859762740291687696),
        ("settings.xml", 14377881348694768885),
        ("version.xml", 14611401900661807598),
    ]
}
fn legacy_byte_mismatch(
    label: &str,
    bytes: &[u8],
    expected: u64,
    parts: &[(&str, u64)],
) -> Option<String> {
    (package::fingerprint(bytes) != expected).then(|| {
        format!("{label}; XML part differences: {}", package::xml_part_differences(bytes, parts))
    })
}
fn assert_legacy_bytes(label: &str, bytes: &[u8], expected: u64, parts: &[(&str, u64)]) {
    if let Some(message) = legacy_byte_mismatch(label, bytes, expected, parts) {
        panic!("{message}");
    }
}

// 이것을 실패시키는 것: 같은 바이트도 불일치로 판정하거나 동일 XML 파트도 차이가 있다고 보고.
#[test]
fn legacy_hash_match_has_no_changed_xml_parts() {
    let (bytes, _) = hwpforge_convert::hwp5_to_hwpx_bytes(&numbered_hwp5()).unwrap();
    assert_eq!(
        legacy_byte_mismatch(
            "HWP5",
            &bytes,
            9288367002320471023,
            &legacy_parts(11079467270218028339, 13958206635349407338),
        ),
        None
    );
    assert_eq!(
        package::xml_part_differences(
            &bytes,
            &legacy_parts(11079467270218028339, 13958206635349407338)
        ),
        "none; ZIP metadata or non-XML parts differ"
    );
}

// 이것을 실패시키는 것: 바이트 불일치 검사를 건너뛰거나 진단에서 XML 파트 이름 제거.
#[test]
fn legacy_hash_mismatch_names_changed_xml_parts() {
    let (bytes, _) = hwpforge_convert::hwp5_to_hwpx_bytes(&numbered_hwp5()).unwrap();
    let changed =
        package::splice_part(&bytes, "Contents/header.xml", "align=\"LEFT\"", "align=\"RIGHT\"");
    let message = legacy_byte_mismatch(
        "HWP5",
        &changed,
        9288367002320471023,
        &legacy_parts(11079467270218028339, 13958206635349407338),
    )
    .expect("changed header changes package bytes");
    assert!(message.starts_with("HWP5; XML part differences: Contents/header.xml:"), "{message}");
}

// 이것을 실패시키는 것: 미지정 자동 간격을 JSON null 또는 fidelity 항목으로 추가.
#[test]
fn auto_spacing_none_preserves_existing_output_bytes() {
    let out = hwpforge::ops::convert_md(
        "# Heading\n\n한글 Latin 123\n\n- first\n- second\n",
        None,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(package::fingerprint(&out.bytes), 9816744291671968531);
    let json = hwpforge::ops::to_json(&out.bytes, &Default::default()).unwrap();
    assert_eq!(
        package::fingerprint(&serde_json::to_vec_pretty(&json.document).unwrap()),
        8033325665553933243
    );
    let roundtrip = hwpforge::ops::from_json(
        &serde_json::to_string(&json.document).unwrap(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(package::fingerprint(&roundtrip.bytes), 9816744291671968531);
    let template=hwpforge::blueprint::template::Template::from_yaml("meta:\n  name: custom\n  version: 1.0.0\nstyles:\n  body:\n    char_shape: {font: Test, size: 11pt}\n    para_shape: {alignment: Left, spacing: {after: 2pt}}\nmarkdown_mapping:\n  body: body\n").unwrap();
    let registry = hwpforge::blueprint::registry::StyleRegistry::from_template(&template).unwrap();
    assert_eq!(package::fingerprint(&serde_json::to_vec(&registry).unwrap()), 785943902290395916);
    let decoded = hwpforge::md::MdDecoder::decode("한글 Latin 123", &template).unwrap();
    let bridge =
        hwpforge::hwpx::HwpxRegistryBridge::from_registry(&decoded.style_registry).unwrap();
    assert_eq!(
        package::fingerprint(&serde_json::to_vec(bridge.style_store()).unwrap()),
        2708013691691149090
    );
    let doc = bridge.rebind_draft_document(decoded.document).unwrap().validate().unwrap();
    let bytes = hwpforge::hwpx::HwpxEncoder::encode(
        &doc,
        bridge.style_store(),
        &hwpforge::core::ImageStore::new(),
    )
    .unwrap();
    assert_legacy_bytes(
        "template",
        &bytes,
        16075229240325534192,
        &[
            ("version.xml", 14611401900661807598),
            ("META-INF/container.xml", 4559238386486593401),
            ("META-INF/manifest.xml", 15859762740291687696),
            ("settings.xml", 14377881348694768885),
            ("Contents/header.xml", 2494241547004455114),
            ("Contents/section0.xml", 6831112335007471991),
        ],
    );
    let (bytes, _) = hwpforge_convert::hwp5_to_hwpx_bytes(&numbered_hwp5()).unwrap();
    assert_legacy_bytes(
        "HWP5",
        &bytes,
        9288367002320471023,
        &legacy_parts(11079467270218028339, 13958206635349407338),
    );
}
