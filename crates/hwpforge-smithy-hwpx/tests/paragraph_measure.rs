//! #242 fixtures and positional references: W3 Hancom measurements override W1 ID lookup.
//! 이것을 실패시키는 것: 정의의 원본 id로 조회/출력하여 실제 한컴 선택과 달라지는 것.
use hwpforge_core::ParagraphMeasure;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxDocument, HwpxEncoder, PackageReader};
use std::io::{Cursor, Read, Write};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/paragraph_measure/{name}.hwpx",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}
fn replace_part(bytes: &[u8], part: &str, xml: &str) -> Vec<u8> {
    let mut input = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut entry = input.by_index(i).unwrap();
        let name = entry.name().unwrap().to_string();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        out.start_file(&name, options).unwrap();
        out.write_all(if name == part { xml.as_bytes() } else { &data }).unwrap();
    }
    out.finish().unwrap().into_inner()
}
fn replace_header(bytes: &[u8], xml: &str) -> Vec<u8> {
    replace_part(bytes, "Contents/header.xml", xml)
}
fn signature(d: &HwpxDocument) -> Vec<(u32, Vec<u32>, Option<u32>)> {
    let mut out = Vec::new();
    d.document.for_each_paragraph(|p| {
        out.push((
            p.para_shape_id.get() as u32,
            p.runs.iter().map(|r| r.char_shape_id.get() as u32).collect(),
            p.end_char_shape.map(|i| i.get() as u32),
        ));
    });
    out
}
fn measures(d: &HwpxDocument) -> Vec<(u32, [ParagraphMeasure; 5], i32)> {
    d.style_store
        .iter_para_shapes()
        .enumerate()
        .map(|(index, p)| {
            (
                index as u32,
                [p.margin_left, p.margin_right, p.indent, p.spacing_before, p.spacing_after],
                p.line_spacing,
            )
        })
        .collect()
}
#[test]
fn w1_original_fixtures_preserve_units_values_and_ids() {
    // 이것을 실패시키는 것: 변이 2·3·7 — E21/E22/E25의 CHAR 삭제 또는 default=case.
    for name in [
        "para-margins",
        "para-margins-char",
        "ch-native-tab2ch",
        "para-margins-wrap",
        "field-mix",
        "line-height-mixed",
    ] {
        let original = HwpxDecoder::decode(&fixture(name)).unwrap();
        let expected = measures(&original);
        let tabs: Vec<_> =
            hwpforge_core::TabDef::merged_with_defaults(original.style_store.iter_tabs());
        let json = serde_json::to_value(&original.style_store).unwrap();
        let store = serde_json::from_value(json).unwrap();
        let output = HwpxEncoder::encode(
            &original.document.validate().unwrap(),
            &store,
            &original.image_store,
        )
        .unwrap();
        let decoded = HwpxDecoder::decode(&output).unwrap();
        assert_eq!(measures(&decoded), expected, "{name}");
        assert_eq!(decoded.style_store.iter_tabs().cloned().collect::<Vec<_>>(), tabs, "{name}");
    }
}
#[test]
fn w3_reordered_char7_changes_the_selected_definition() {
    for name in ["para-margins-char", "field-mix", "line-height-mixed"] {
        let bytes = fixture(name);
        let original = HwpxDecoder::decode(&bytes).unwrap();
        let mut xml = PackageReader::new(&bytes).unwrap().read_header_xml().unwrap();
        let moved_id = if name == "line-height-mixed" { 6 } else { 7 };
        let moved_tag = format!("<hh:charPr id=\"{moved_id}\"");
        let start = xml.find(&moved_tag).unwrap();
        let end = start + xml[start..].find("</hh:charPr>").unwrap() + "</hh:charPr>".len();
        let char7 = xml[start..end].to_string();
        xml.replace_range(start..end, "");
        let first = xml.find("<hh:charPr ").unwrap();
        xml.insert_str(first, &char7);
        let reordered = HwpxDecoder::decode(&replace_header(&bytes, &xml)).unwrap();
        // 한컴 실측: 첫 위치를 옮기면 ref 0의 정의도 바뀐다.
        // 이것을 실패시키는 것: 원본 id 키로 같은 정의를 계속 고르는 것.
        assert_eq!(
            reordered.style_store.iter_char_shapes().next().unwrap(),
            original.style_store.iter_char_shapes().nth(moved_id as usize).unwrap()
        );
        assert_eq!(signature(&original), signature(&reordered), "{name}");
        let expected = signature(&reordered);
        let out = HwpxEncoder::encode(
            &reordered.document.validate().unwrap(),
            &reordered.style_store,
            &reordered.image_store,
        )
        .unwrap();
        let decoded = HwpxDecoder::decode(&out).unwrap();
        assert_eq!(signature(&decoded), expected, "{name}");
        let out_xml = PackageReader::new(&out).unwrap().read_header_xml().unwrap();
        assert!(out_xml.find("<hh:charPr id=\"0\"").unwrap() < out_xml.find(&moved_tag).unwrap());
    }
}
#[test]
fn w1_duplicate_and_broken_wire_ids_are_errors() {
    let bytes = fixture("para-margins-char");
    let xml = PackageReader::new(&bytes).unwrap().read_header_xml().unwrap();
    for broken in [
        xml.replacen("charPrIDRef=\"0\"", "charPrIDRef=\"999999\"", 1),
        xml.replacen("tabPrIDRef=\"0\"", "tabPrIDRef=\"999999\"", 1),
        xml.replacen("borderFillIDRef=\"2\"", "borderFillIDRef=\"999999\"", 1),
    ] {
        assert_ne!(broken, xml, "mutation must change the fixture");
        assert!(HwpxDecoder::decode(&replace_header(&bytes, &broken)).is_err());
    }
}

#[test]
fn w1_sparse_para_and_style_ids_preserve_font_line_height_and_cycles() {
    let bytes = fixture("line-height-mixed");
    let mut pkg = PackageReader::new(&bytes).unwrap();
    let header = pkg.read_header_xml().unwrap();
    let section = pkg.read_section_xml(0).unwrap();
    // 한컴 실측: 정의 label만 900으로 바꿔도 위치 0을 계속 참조한다.
    let header = header.replace("<hh:paraPr id=\"0\"", "<hh:paraPr id=\"900\"");
    // References remain positions; only the ignored label changes.
    let changed = replace_part(&replace_header(&bytes, &header), "Contents/section0.xml", &section);
    let decoded = HwpxDecoder::decode(&changed).unwrap();
    let expected = signature(&decoded);
    let fidelity = serde_json::to_value(&decoded.style_store).unwrap()["header_fidelity"].clone();
    let out = HwpxEncoder::encode(
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
    )
    .unwrap();
    let after = HwpxDecoder::decode(&out).unwrap();
    assert_eq!(signature(&after), expected);
    assert_eq!(serde_json::to_value(&after.style_store).unwrap()["header_fidelity"], fidelity);
    assert!(PackageReader::new(&out)
        .unwrap()
        .read_header_xml()
        .unwrap()
        .contains("<hh:paraPr id=\"0\""));
}

#[test]
fn w1_sparse_char_without_zero_and_measure_lookup_keep_units() {
    use hwpforge_core::{CharCount, ImageStore, StyleLookup};
    use hwpforge_smithy_hwpx::HwpxStyleLookup;
    let bytes = fixture("field-mix");
    let mut package = PackageReader::new(&bytes).unwrap();
    let header =
        package.read_header_xml().unwrap().replace("<hh:charPr id=\"0\"", "<hh:charPr id=\"900\"");
    let section = package.read_section_xml(0).unwrap();
    let changed = replace_part(&replace_header(&bytes, &header), "Contents/section0.xml", &section);
    let mut decoded = HwpxDecoder::decode(&changed).unwrap();
    let expected = signature(&decoded);
    let id = decoded.document.sections()[0].paragraphs[0].para_shape_id;
    let margin = ParagraphMeasure::from_char_count(CharCount::from_ticks(500));
    decoded.style_store.para_shape_mut(id).unwrap().margin_left = margin;
    decoded.style_store.para_shape_mut(id).unwrap().indent = margin;
    assert_eq!(decoded.style_store.para_indent_left(id), Some(margin));
    let images = ImageStore::new();
    let bridge = HwpxStyleLookup::new(&decoded.style_store, &images);
    assert_eq!(bridge.para_indent_first_line(id), Some(margin));
    let output = HwpxEncoder::encode(
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
    )
    .unwrap();
    let after = HwpxDecoder::decode(&output).unwrap();
    assert_eq!(signature(&after), expected);
}

#[test]
fn w1_readme_answers_are_exact() {
    use hwpforge_core::CharCount;
    let char_value = |v| ParagraphMeasure::from_char_count(CharCount::from_ticks(v));
    let e21 = HwpxDecoder::decode(&fixture("para-margins-char")).unwrap();
    let p = |id| e21.style_store.para_shape(hwpforge_foundation::ParaShapeIndex::new(id)).unwrap();
    assert_eq!(p(37).margin_left, char_value(1000));
    assert_eq!(p(38).indent, char_value(500));
    assert_eq!(p(39).spacing_before, char_value(500));
    assert_eq!(p(40).margin_right, char_value(250));
    assert_eq!(p(41).indent, char_value(-750));
    let e22 = HwpxDecoder::decode(&fixture("ch-native-tab2ch")).unwrap();
    assert_eq!(
        e22.style_store.iter_tabs().find(|t| t.id == 3).unwrap().stops[0].position,
        char_value(1000)
    );
    let e25 = HwpxDecoder::decode(&fixture("line-height-mixed")).unwrap();
    let p = e25.style_store.para_shape(hwpforge_foundation::ParaShapeIndex::new(21)).unwrap();
    assert_eq!(p.margin_left.as_hwp_unit().unwrap().as_i32(), 1000);
    assert_eq!(p.indent, char_value(1000));
}
