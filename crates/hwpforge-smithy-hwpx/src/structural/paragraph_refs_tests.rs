use super::*;
use crate::structural::{HwpxStructuralEditor, InsertPosition, ParagraphLocator};
use crate::test_support::xml::Element;
use hwpforge_core::{
    control::RefTarget, Control, Document, ImageStore, ObjectId, PageSettings, Paragraph, Run,
    Section,
};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex, RefContentType, RefType};

fn paragraph(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn generated(other_section: bool) -> Vec<u8> {
    let reference = Paragraph::with_runs(
        vec![Run::control(
            Control::CrossRef {
                target: RefTarget::Object(ObjectId::new(2)),
                ref_type: RefType::Outline,
                content_type: RefContentType::Page,
                as_hyperlink: false,
                display_text: "참조".into(),
            },
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    );
    let mut doc = Document::new();
    let mut body = vec![paragraph("첫 문단"), paragraph("중간"), paragraph("개요 대상")];
    if !other_section {
        body.push(reference.clone());
    }
    doc.add_section(Section::with_paragraphs(body, PageSettings::a4()));
    if other_section {
        doc.add_section(Section::with_paragraphs(vec![reference], PageSettings::a4()));
    }
    crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap()
}

fn replace(bytes: &[u8], path: &str, from: &str, to: &str) -> Vec<u8> {
    let mut package = RawPackage::read(bytes).unwrap();
    let xml = package.read_text_entry(path).unwrap();
    assert!(xml.contains(from), "{from}");
    package.replace_text_entry(path, xml.replacen(from, to, 1));
    package.write().unwrap()
}

fn plain_document() -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![paragraph("처음"), paragraph("중간"), paragraph("끝")],
        PageSettings::a4(),
    ));
    crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap()
}

fn edits(bytes: &[u8], index: usize) -> [Result<Vec<u8>, StructuralEditError>; 2] {
    let at = ParagraphLocator { section: 0, index };
    [
        HwpxStructuralEditor::delete_paragraphs(bytes, &[at]),
        HwpxStructuralEditor::insert_paragraphs(bytes, at, InsertPosition::After, &["추가".into()]),
    ]
}

// 이것을 실패시키는 것: 원본 참조 검사 생략, 개요 허용 또는 삭제되는 참조 검사 제외.
#[test]
fn outline_consumers_refuse_structural_edits_even_when_deleted() {
    for other_section in [false, true] {
        let base = generated(other_section);
        assert!(check(&RawPackage::read(&base).unwrap()).is_err());
        for index in if other_section { vec![1] } else { vec![1, 3] } {
            for result in edits(&base, index) {
                assert!(result.is_err());
            }
        }
    }
}

fn hyperlink(value: &str) -> String {
    format!("<hp:fieldBegin type='HYPERLINK'><hp:parameters><hp:stringParam name='TargetType'>{value}</hp:stringParam><hp:stringParam name='Path'>?#2;</hp:stringParam></hp:parameters></hp:fieldBegin>")
}

// 이것을 실패시키는 것: 값 기반 선검사, CDATA/문자 참조 누락, 개요 하이퍼링크 허용.
#[test]
fn encoded_outline_hyperlinks_return_no_bytes() {
    for value in [
        "<![CDATA[HWPHYPERLINK_TARGET_OUTLINE]]>",
        "HWPHYPERLINK_TARGET_OUT<![CDATA[LINE]]>",
        "HWPHYPERLINK_TARGET_OUTL&#73;NE",
        "&#72;WPHYPERLINK_TARGET_OUTLINE",
        "HWPHYPERLINK_TARGET_OUTLINE",
    ] {
        let xml = hyperlink(value);
        assert!(check_xml(&xml, false).is_err(), "{value}");
        // A valid generated HYPERLINK keeps admission independent of TargetType;
        // the decoder represents its Path but cannot represent its outline kind.
        let base = replace(
            &generated(false),
            "Contents/section0.xml",
            "type=\"CROSSREF\"",
            "type=\"HYPERLINK\"",
        );
        let base = replace(
            &base,
            "Contents/section0.xml",
            "name=\"RefType\">TARGET_OUTLINE",
            &format!("name=\"TargetType\">{value}"),
        );
        super::super::admit(&base).expect("generated hyperlink passes existing admission");
        for result in edits(&base, 1) {
            let error = result.unwrap_err();
            assert!(
                error.to_string().contains("unresolved paragraph reference"),
                "{value}: {error}"
            );
        }
        // Also exercise candidate selection in a part the decoder does not model.
        let base = replace(
            &plain_document(),
            "Contents/header.xml",
            "</hh:head>",
            &format!("{xml}</hh:head>"),
        );
        assert!(check(&RawPackage::read(&base).unwrap()).is_err(), "{value}");
    }
    // 이것을 실패시키는 것: CDATA/문자 참조 표기를 해석하지 않고 전부 거부.
    check_xml(&hyperlink("HWPHYPERLINK_TARGET_DOC<![CDATA[UMENT]]>_DONT&#67;ARE"), false).unwrap();
    check_xml(&hyperlink("HWPHYPERLINK_TARGET_DOCUMENT_DONTCARE"), false).unwrap();
}

// 이것을 실패시키는 것: 주석/PI/자식 요소/중복 parameter, 중첩 필드 또는 미지 CROSSREF 허용.
#[test]
fn ambiguous_parameters_are_refused() {
    for value in [
        "HWPHYPERLINK_TARGET_DOCUMENT_DONT<!-- split -->CARE",
        "HWPHYPERLINK_TARGET_DOCUMENT_DONT<?split x?>CARE",
        "HWPHYPERLINK_TARGET_DOCUMENT_DONT<x/>CARE",
        "HWPHYPERLINK_TARGET_OUTLINE</hp:stringParam><hp:stringParam name='TargetType'>safe",
    ] {
        assert!(check_xml(&hyperlink(value), false).is_err(), "{value}");
    }
    for xml in [
        "<fieldBegin type='CROSSREF'/>",
        "<fieldBegin type='DATE'><parameters><stringParam name='RefType'>TARGET_OUTLINE</stringParam></parameters></fieldBegin>",
        "<fieldBegin type='CROSSREF'><fieldBegin type='DATE'/></fieldBegin>",
        "<!DOCTYPE r><r/>",
    ] {
        assert!(check_xml(xml, false).is_err(), "{xml}");
    }
    for kind in [
        "TARGET_BOOKMARK",
        "TARGET_TABLE",
        "TARGET_FIGURE",
        "TARGET_EQUATION",
        "TARGET_FOOTNOTE",
        "TARGET_ENDNOTE",
    ] {
        check_xml(&format!("<fieldBegin type='CROSSREF'><parameters><stringParam name='RefType'>{kind}</stringParam></parameters></fieldBegin>"), false).unwrap();
    }
}

// 이것을 실패시키는 것: 비구역 part/namespace 있는 paraIDRef 누락 또는 XML 깊이 제한 제거.
#[test]
fn references_in_any_xml_part_are_refused() {
    for path in
        ["settings.xml", "Contents/header.xml", "Contents/section0.xml", "Contents/content.hpf"]
    {
        let mut package = RawPackage::read(&plain_document()).unwrap();
        package.replace_text_entry(path, "<x xmlns:u='urn:unknown' u:paraIDRef='1'/>".into());
        assert!(check(&package).is_err(), "{path}");
    }
    for (path, from, to) in [
        ("Contents/section0.xml", "<hp:p id=\"0\"", "<hp:p paraIDRef=\"2\" id=\"0\""),
        ("settings.xml", "ha:CaretPosition", "ha:OtherPosition"),
    ] {
        let base = replace(&plain_document(), path, from, to);
        for result in edits(&base, 1) {
            assert!(result.is_err(), "{path}");
        }
    }
    let limit = crate::decoder::XML_RECURSION_LIMIT;
    check_xml(&format!("{}{}", "<x>".repeat(limit), "</x>".repeat(limit)), false).unwrap();
    assert!(check_xml(&format!("{}{}", "<x>".repeat(limit + 1), "</x>".repeat(limit + 1)), false)
        .is_err());
}

// 이것을 실패시키는 것: 저장 커서 거부, settings 재작성 또는 미지 namespace/속성/자식/텍스트까지 제외.
#[test]
fn saved_cursor_is_the_only_paragraph_address_exception() {
    let base = replace(&plain_document(), "settings.xml", "paraIDRef=\"0\"", "paraIDRef=\"1\"");
    let base = replace(&base, "settings.xml", "listIDRef=\"0\"", "listIDRef=\"9\"");
    let package = RawPackage::read(&base).unwrap();
    let original = package.read_entry("settings.xml").unwrap();
    for output in edits(&base, 1) {
        let after = RawPackage::read(&output.unwrap()).unwrap();
        assert_eq!(after.read_entry("settings.xml").unwrap(), original);
        let tree = Element::parse(&after.read_text_entry("settings.xml").unwrap());
        assert_eq!(tree.child("ha:CaretPosition").attr("paraIDRef"), Some("1"));
        assert_eq!(tree.child("ha:CaretPosition").attr("listIDRef"), Some("9"));
    }
    let safe = "<ha:CaretPosition xmlns:ha='http://www.hancom.co.kr/hwpml/2011/app' listIDRef='9' paraIDRef='1' pos='0'> </ha:CaretPosition>";
    let scan = crate::layout_edit::settings_paragraph_refs_are_safe;
    assert!(scan(safe).unwrap());
    for unsafe_xml in [
        safe.replace("2011/app", "2011/other"),
        safe.replace("CaretPosition", "OtherPosition"),
        safe.replace("pos='0'", "future='0'"),
        safe.replace("paraIDRef='1'", "paraIDRef='&unknown;'"),
        format!("<r unbound:x='1'>{safe}</r>"),
        safe.replace("> </", "><x/></"),
        safe.replace("> </", ">x</"),
        safe.replace("> </", "><![CDATA[x]]></"),
        safe.replace("> </", ">&#120;</"),
        "<!DOCTYPE r><r/>".into(),
    ] {
        assert!(!scan(&unsafe_xml).unwrap_or(false), "{unsafe_xml}");
    }
}

// 이것을 실패시키는 것: settings 깊이 검사 제거 또는 64를 63으로 축소.
// nextest runs this case in its own process, so stack overflow is a test failure.
#[test]
fn deep_settings_returns_a_depth_error_without_recursive_drop() {
    let scan = crate::layout_edit::settings_paragraph_refs_are_safe;
    assert!(scan(&format!("{}{}", "<x>".repeat(64), "</x>".repeat(64))).unwrap());
    let xml = format!("{}<x paraIDRef='1'/>{}", "<x>".repeat(100_000), "</x>".repeat(100_000));
    let error = scan(&xml).unwrap_err();
    assert!(error.to_string().contains("invalid XML depth/root count"));
    let mut package = RawPackage::read(&plain_document()).unwrap();
    package.replace_text_entry("settings.xml", xml);
    for result in edits(&package.write().unwrap(), 1) {
        let error = result.unwrap_err();
        assert!(error.to_string().contains("invalid XML depth/root count"), "{error}");
    }
}
