use super::*;
use crate::raw_package::RawPackage;
use crate::structural::{
    paragraph_replacements, splice_paragraphs, HwpxStructuralEditor, ParagraphLocator,
};
use crate::test_support::xml;

fn map(xml: &str) -> crate::decoder::source_map::SourceMap {
    let parsed = crate::decoder::section::parse_section(xml, 0, &Default::default()).unwrap();
    let section = hwpforge_core::Section::with_paragraphs(
        parsed.paragraphs,
        hwpforge_core::PageSettings::a4(),
    );
    parsed.source.finish(&section, 0, "Contents/section0.xml")
}

// 이것을 실패시키는 것: 공용 splice의 겹침/중복/문자 경계 검사를 제거.
#[test]
fn invalid_replacements_return_no_bytes() {
    for replacements in [
        vec![(1..5, String::new()), (4..6, String::new())],
        vec![(1..4, String::new()), (1..4, String::new())],
        vec![(1..1, "x".into()), (1..1, "y".into())],
        vec![(0..9, String::new())],
        vec![(Range { start: 5, end: 4 }, String::new())],
        vec![(2..4, String::new())],
        vec![(1..2, String::new())],
        vec![(0..7, String::new()), (4..5, "0".into())],
    ] {
        let result = crate::cache_flow::splice("a한bcd", replacements).map_err(verification_error);
        assert!(matches!(
            result,
            Err(StructuralEditError::Codec(crate::CodecFailure {
                stage: crate::CodecStage::Verification,
                ..
            }))
        ));
    }
}

// 이것을 실패시키는 것: 삭제 부모에서 continue 제거, body 대신 내부 흐름 캐시까지 삭제.
#[test]
fn deleted_parent_excludes_child_replacements_and_keeps_nested_tail() {
    let original = r#"<sec><p id="0" paraPrIDRef="0"><run charPrIDRef="0"><t>처음</t></run><linesegarray/></p><p id="1" paraPrIDRef="0"><run charPrIDRef="0"><t>삭제</t><ctrl><footNote><subList><p id="0" paraPrIDRef="0"><run charPrIDRef="0"><t>삭제 내부</t></run><linesegarray/></p></subList></footNote></ctrl></run><linesegarray/></p><p id="2" paraPrIDRef="0"><run charPrIDRef="0"><ctrl><footNote><subList><p id="0" paraPrIDRef="0"><run charPrIDRef="0"><t>내부</t></run><linesegarray/></p></subList></footNote></ctrl></run><linesegarray/></p></sec>"#;
    let map = map(original);
    let output = splice_paragraphs(original, &map, &[1], None).unwrap();
    let parsed = xml::Element::parse(&output);
    assert_eq!(parsed.child_names(), ["p", "p"]);
    assert_eq!(parsed.children[0].attr("id"), Some("0"));
    assert_eq!(parsed.children[1].attr("id"), Some("1"));
    assert_eq!(parsed.children[0].child_names(), ["run", "linesegarray"]);
    assert_eq!(parsed.children[1].child_names(), ["run"]);
    assert_eq!(parsed.find("subList").child("p").child_names(), ["run", "linesegarray"]);
    verify_section(
        original,
        &output,
        paragraph_replacements(&map, &[1], None, super::super::ParagraphIds::Reindex).unwrap(),
    )
    .unwrap();
}

// 이것을 실패시키는 것: 허용 원문 간격 또는 마지막 꼬리 비교 생략.
#[test]
fn independent_declaration_rejects_extra_replacements() {
    let original = "abcdef";
    let allowed = vec![(2..3, "XYZ".into())];
    let good = crate::cache_flow::splice(original, allowed.clone()).unwrap();
    verify_section(original, &good, allowed.clone()).unwrap();
    for extra in [0..1, 5..6, 6..6] {
        let mut rogue = allowed.clone();
        rogue.push((extra, "!".into()));
        let output = crate::cache_flow::splice(original, rogue).unwrap();
        assert!(verify_section(original, &output, allowed.clone()).is_err());
    }
    assert!(verify_section(original, "abBADdef", allowed).is_err());
}

pub(super) fn generated(paragraphs: Vec<hwpforge_core::Paragraph>) -> Vec<u8> {
    let mut doc = hwpforge_core::Document::new();
    doc.add_section(hwpforge_core::Section::with_paragraphs(
        paragraphs,
        hwpforge_core::PageSettings::a4(),
    ));
    crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &hwpforge_core::ImageStore::new(),
    )
    .unwrap()
}

pub(super) fn paragraph(text: &str) -> hwpforge_core::Paragraph {
    hwpforge_core::Paragraph::with_runs(
        vec![hwpforge_core::Run::text(text, hwpforge_foundation::CharShapeIndex::new(0))],
        hwpforge_foundation::ParaShapeIndex::new(0),
    )
}

// 이것을 실패시키는 것: finish_edit의 최종 구간 검사 호출 제거.
#[test]
fn final_zip_rejects_semantically_invisible_extra_edit() {
    let base = generated(vec![paragraph("앞"), paragraph("삭제"), paragraph("뒤")]);
    let (decoded, _) = crate::structural::admit(&base).unwrap();
    let output = HwpxStructuralEditor::delete_paragraphs(
        &base,
        &[ParagraphLocator { section: 0, index: 1 }],
    )
    .unwrap();
    let mut package = RawPackage::read(&output).unwrap();
    let path = "Contents/section0.xml";
    let xml = package.read_text_entry(path).unwrap();
    package.replace_text_entry(path, format!("{xml}<!--unexpected-->"));
    let mut expected = decoded.document.clone();
    expected.sections_mut()[0].paragraphs.remove(1);
    let result = crate::structural::finish_edit(
        &base,
        package,
        &decoded,
        expected,
        &crate::raw_package::OutputChanges { rewritten: vec![path.into()], ..Default::default() },
        &[SectionChange {
            ids: super::super::ParagraphIds::Reindex,
            section: 0,
            path: path.into(),
            original: RawPackage::read(&base).unwrap().read_text_entry(path).unwrap(),
            deleted: vec![1],
            inserted: None,
        }],
    );
    assert!(matches!(
        result,
        Err(StructuralEditError::Codec(crate::CodecFailure {
            stage: crate::CodecStage::Verification,
            ..
        }))
    ));
}

// 이것을 실패시키는 것: 캐시 원장에 없는/중간 오프셋을 문단 시작으로 승인.
#[test]
fn structural_caches_require_original_paragraph_start() {
    let source = r#"<sec><p id="0" paraPrIDRef="0"><run charPrIDRef="0"><t>본문</t></run><linesegarray/></p></sec>"#;
    let mut map = map(source);
    let start = map.paragraphs[0].outer.start;
    assert!(map.cache_flow.paragraph_caches(start + 1).is_none());
    assert!(map.cache_flow.paragraph_caches(usize::MAX).is_none());
    map.cache_flow = Default::default();
    assert!(matches!(
        splice_paragraphs(source, &map, &[], Some((0, 0, ""))),
        Err(StructuralEditError::Invariant { .. })
    ));
}

// 이것을 실패시키는 것: 최종 검사에서 RawPackage 전체 읽기를 재도입하거나,
// 삭제 정의가 없는데 전체 XML을 읽거나, XML 필터 없이 BinData를 읽기.
#[test]
fn final_checks_read_only_required_xml_with_large_binary() {
    use std::io::{Cursor, Write};
    let base = generated(vec![paragraph("앞"), paragraph("삭제"), paragraph("뒤")]);
    let (decoded, _) = crate::structural::admit(&base).unwrap();
    let original =
        RawPackage::read(&base).unwrap().read_text_entry("Contents/section0.xml").unwrap();
    let edited = HwpxStructuralEditor::delete_paragraphs(
        &base,
        &[ParagraphLocator { section: 0, index: 1 }],
    )
    .unwrap();
    let mut writer = zip::ZipWriter::new_append(Cursor::new(edited)).unwrap();
    writer
        .start_file(
            "BinData/large.bin",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
    writer.write_all(&vec![0x5a; 8 * 1024 * 1024]).unwrap();
    let output = writer.finish().unwrap().into_inner();
    crate::raw_package::PACKAGE_IO.with(|count| count.set((0, 0)));
    XML_READS.with(|reads| reads.borrow_mut().clear());
    verify_output(
        &output,
        &decoded,
        &[SectionChange {
            ids: super::super::ParagraphIds::Reindex,
            section: 0,
            path: "Contents/section0.xml".into(),
            original,
            deleted: vec![1],
            inserted: None,
        }],
    )
    .unwrap();
    assert_eq!(crate::raw_package::PACKAGE_IO.with(|count| count.get()), (0, 0));
    assert_eq!(XML_READS.with(|reads| reads.borrow().clone()), ["Contents/section0.xml"]);

    // A real removed definition scans every XML/HPF part, still no binary payload.
    let mut removed = crate::structural::references::Removed::default();
    removed.record(r#"<p><bookmark name="gone"/></p>"#).unwrap();
    let mut package = open_package(&output, Default::default()).unwrap();
    let expected: Vec<_> = package
        .entries()
        .iter()
        .filter(|e| e.path.ends_with(".xml") || e.path.ends_with(".hpf"))
        .map(|e| e.path.clone())
        .collect();
    XML_READS.with(|reads| reads.borrow_mut().clear());
    assert!(removed.surviving_references_are_safe(&mut package).unwrap());
    assert_eq!(XML_READS.with(|reads| reads.borrow().clone()), expected);
}

// 이것을 실패시키는 것: opaque XML의 의미 조회 거부 생략.
#[test]
fn reference_scan_rejects_opaque_xml_parts() {
    use std::io::{Cursor, Write};
    let base = generated(vec![paragraph("본문")]);
    let decoded = crate::HwpxDecoder::decode(&base).unwrap();
    let mut writer = zip::ZipWriter::new_append(Cursor::new(base)).unwrap();
    writer.start_file("extra/./part.xml", zip::write::SimpleFileOptions::default()).unwrap();
    writer.write_all(b"<root/>").unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    let mut package = open_package(&bytes, Default::default()).unwrap();
    let mut removed = crate::structural::references::Removed::default();
    removed.record(r#"<p><bookmark name="gone"/></p>"#).unwrap();
    assert!(removed.surviving_references_are_safe(&mut package).is_err());
    let raw = RawPackage::read(&bytes).unwrap();
    assert!(crate::derived_values::DerivedFacts::collect(&raw, &decoded).is_err());
}

fn check_derived_values(
    facts: &crate::derived_values::DerivedFacts<'_>,
    edits: &[crate::derived_values::DerivedEdit<'_>],
) -> crate::HwpxResult<()> {
    facts.effects(edits)?.check_derived_values()
}

const CELL_CACHE: &str = r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="99" vertsize="1000" textheight="1000" baseline="850" spacing="600" horzpos="0" horzsize="9000" flags="393216"/></hp:linesegarray>"#;

fn generated_tables() -> Vec<u8> {
    use hwpforge_core::{Run, Table, TableCell, TableRow};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit};
    let table_paragraph = |text: &str| {
        let cell = TableCell::new(vec![paragraph(text)], HwpUnit::new(9000).unwrap());
        let mut host = paragraph(text);
        host.runs
            .push(Run::table(Table::new(vec![TableRow::new(vec![cell])]), CharShapeIndex::new(0)));
        host
    };
    let bytes = generated(vec![table_paragraph("앞"), paragraph("삭제"), table_paragraph("뒤")]);
    let mut package = RawPackage::read(&bytes).unwrap();
    let path = "Contents/section0.xml";
    let xml = package
        .read_text_entry(path)
        .unwrap()
        .replace("height=\"0\" heightRelTo=", "height=\"321\" heightRelTo=");
    let xml = xml.replace("</hp:p>", &format!("{CELL_CACHE}</hp:p>"));
    package.replace_text_entry(path, xml);
    package.write().unwrap()
}

// 이것을 실패시키는 것: 기존 공개 구조 편집/빈 요청에서 전역 파생값 원장을 수집.
#[test]
fn public_edits_do_not_collect_derived_facts() {
    use crate::structural::InsertPosition;
    let base = generated(vec![paragraph("앞"), paragraph("삭제"), paragraph("뒤")]);
    crate::derived_values::COLLECTIONS.with(|n| n.set(0));
    let targets = [ParagraphLocator { section: 0, index: 1 }];
    HwpxStructuralEditor::delete_paragraphs(&base, &targets).unwrap();
    assert_eq!(HwpxStructuralEditor::delete_paragraphs(&base, &[]).unwrap(), base);
    HwpxStructuralEditor::insert_paragraphs(
        &base,
        targets[0],
        InsertPosition::After,
        &["추가".into()],
    )
    .unwrap();
    let decoded = crate::HwpxDecoder::decode(&base).unwrap();
    let replacement = &decoded.document.sections()[0];
    let preservation =
        crate::HwpxPatcher::export_section_preservation(&base, 0, replacement).unwrap();
    let mut changed = replacement.clone();
    changed.paragraphs[1].runs[0].content = hwpforge_core::RunContent::Text("변경".into());
    crate::HwpxPatcher::patch_section_preserving(&base, 0, &changed, None, Some(&preservation))
        .unwrap();
    let mut named = paragraph("앞");
    named.runs.push(hwpforge_core::Run::control(
        hwpforge_core::Control::Field {
            field_type: hwpforge_foundation::FieldType::ClickHere,
            hint_text: Some("힌트".into()),
            help_text: None,
            name: Some("입력".into()),
            display_text: "힌트".into(),
        },
        hwpforge_foundation::CharShapeIndex::new(0),
    ));
    let named_base = generated(vec![named, paragraph("뒤")]);
    crate::HwpxFiller::fill(
        &named_base,
        &std::collections::BTreeMap::from([("입력".into(), "값".into())]),
    )
    .unwrap();
    assert_eq!(crate::derived_values::COLLECTIONS.with(|n| n.get()), 0);
}

// 이것을 실패시키는 것: Core FieldInventory/SourceMap 조인 생략, decoder 슬롯 대신 임의 t 범위 승인.
#[test]
fn field_inventory_and_text_slots_use_decoder_addresses() {
    use crate::derived_values::{DerivedEdit, DerivedFacts};
    let mut named = paragraph("앞");
    named.runs.push(hwpforge_core::Run::control(
        hwpforge_core::Control::Field {
            field_type: hwpforge_foundation::FieldType::ClickHere,
            hint_text: Some("힌트".into()),
            help_text: None,
            name: Some("입력".into()),
            display_text: "힌트".into(),
        },
        hwpforge_foundation::CharShapeIndex::new(0),
    ));
    let base = generated(vec![named, paragraph("본문")]);
    let decoded = crate::HwpxDecoder::decode(&base).unwrap();
    let package = RawPackage::read(&base).unwrap();
    let facts = DerivedFacts::collect(&package, &decoded).unwrap();
    let changed = [facts.body_ordinal(0, 1).unwrap()];
    let xml = package.read_text_entry("Contents/section0.xml").unwrap();
    let at = xml.find("본문").unwrap();
    let slots = [(at..at + "본문".len(), "바꿈".into())];
    let proof = vec![false; decoded.source_maps[0].cache_flow.paragraph_count()];
    check_derived_values(
        &facts,
        &[DerivedEdit::Text { section: 0, changed: &changed, proven: &proof, slots: &slots }],
    )
    .unwrap();
    let bad = [(at + "본".len()..at + "본문".len(), "바꿈".into())];
    assert!(check_derived_values(
        &facts,
        &[DerivedEdit::Text { section: 0, changed: &changed, proven: &proof, slots: &bad }]
    )
    .is_err());
}

// 이것을 실패시키는 것: 디코더 digest 전달 손실, 수집 시 원문 재파싱 누락.
#[test]
fn source_map_keeps_digest_and_collect_rebuilds_elements() {
    let base = generated_tables();
    let decoded = crate::HwpxDecoder::decode(&base).unwrap();
    let package = RawPackage::read(&base).unwrap();
    let xml = package.read_text_entry("Contents/section0.xml").unwrap();
    use sha2::Digest;
    assert_eq!(
        decoded.source_maps[0].source_digest,
        <[u8; 32]>::from(sha2::Sha256::digest(xml.as_bytes()))
    );
    let debug = format!("{:?}", decoded.source_maps[0]);
    assert!(!debug.contains("XmlSource"));
    let count =
        package.entry_paths().filter(|p| p.ends_with(".xml") || p.ends_with(".hpf")).count();
    let before = hwpforge_core::source_xml::source_work();
    let facts = crate::derived_values::DerivedFacts::collect(&package, &decoded).unwrap();
    assert_eq!(hwpforge_core::source_xml::source_work().0 - before.0, count);
    assert!(facts.body_ordinal(0, 2).is_ok());
    let mut changed = package;
    changed.replace_text_entry("Contents/section0.xml", xml.replace("321", "322"));
    assert!(crate::derived_values::DerivedFacts::collect(&changed, &decoded).is_err());
}

// 이것을 실패시키는 것: BOM section의 fieldBegin 콜백 주소와 decoder 주소 불일치.
#[test]
fn bom_sections_collect_fields_from_decoder_addresses() {
    let mut date = paragraph("앞");
    date.runs.push(hwpforge_core::Run::control(
        hwpforge_core::Control::Field {
            field_type: hwpforge_foundation::FieldType::ClickHere,
            hint_text: Some("날짜".into()),
            help_text: None,
            name: Some("입력".into()),
            display_text: "날짜".into(),
        },
        hwpforge_foundation::CharShapeIndex::new(0),
    ));
    let base = generated(vec![date, paragraph("뒤")]);
    for prefix in ["", "\u{feff}"] {
        let mut package = RawPackage::read(&base).unwrap();
        let xml = package.read_text_entry("Contents/section0.xml").unwrap();
        assert!(xml.contains("type=\"CLICK_HERE\""));
        let xml = format!("{prefix}{}", xml.replace("type=\"CLICK_HERE\"", "type=\"DATE\""));
        package.replace_text_entry("Contents/section0.xml", xml.clone());
        let bytes = package.write().unwrap();
        let decoded = crate::HwpxDecoder::decode(&bytes).unwrap();
        let inventory = hwpforge_core::field_inventory::FieldInventory::from_sections(
            decoded.document.sections(),
        );
        assert_eq!(inventory.fields().len(), 1);
        // The decoder normalizes legacy DATE to its summary-field representation.
        assert_eq!(inventory.fields()[0].kind(), "SUMMERY");
        let facts = crate::derived_values::DerivedFacts::collect(&package, &decoded).unwrap();
        assert!(facts.body_ordinal(0, 1).is_ok());
        let expected = xml.find("<hp:fieldBegin ").unwrap();
        let mut offsets = Vec::new();
        crate::structural::xml_values::scan_fields(&xml, |start, _, _| {
            offsets.push(start);
            Ok(())
        })
        .unwrap();
        assert_eq!(offsets, [expected]);
    }
}
