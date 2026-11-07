use super::*;
use crate::raw_package::RawPackage;
use crate::structural::{splice_paragraphs, HwpxStructuralEditor, ParagraphLocator};
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
    verify_section(original, &output, paragraph_replacements(&map, &[1], None).unwrap()).unwrap();
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
        &crate::raw_package::OutputChanges { rewritten: vec![path.into()] },
        &[SectionChange {
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
    let mut writer = zip::ZipWriter::new_append(Cursor::new(base)).unwrap();
    writer.start_file("extra/./part.xml", zip::write::SimpleFileOptions::default()).unwrap();
    writer.write_all(b"<root/>").unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    let mut package = open_package(&bytes, Default::default()).unwrap();
    let mut removed = crate::structural::references::Removed::default();
    removed.record(r#"<p><bookmark name="gone"/></p>"#).unwrap();
    assert!(removed.surviving_references_are_safe(&mut package).is_err());
}
