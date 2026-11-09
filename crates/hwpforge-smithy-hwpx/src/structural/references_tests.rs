use super::*;
use crate::raw_package::RawPackage;
use crate::structural::{HwpxStructuralEditor, ParagraphLocator};
use hwpforge_core::{Control, Document, ImageStore, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, FieldType, ParaShapeIndex};

fn paragraph(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}
fn generated(control: Control, reference: Option<Control>) -> Vec<u8> {
    let mut target = paragraph("");
    target.runs = vec![Run::control(control, CharShapeIndex::new(0))];
    let mut survivor = paragraph("살아남음");
    if let Some(reference) = reference {
        survivor.runs.push(Run::control(reference, CharShapeIndex::new(0)));
    }
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![paragraph("첫 문단"), target, survivor],
        PageSettings::a4(),
    ));
    crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap()
}
fn field() -> Control {
    Control::Field {
        field_type: FieldType::ClickHere,
        hint_text: None,
        help_text: None,
        name: Some("대상".into()),
        display_text: "내용".into(),
    }
}

// 이것을 실패시키는 것: 삭제 정의 수집 또는 href/idref/필드 참조 비교를 생략, itemref 이름만으로 면제.
#[test]
fn removed_definitions_reject_surviving_reference_forms() {
    for definition in [
        r#"<bookmark name="77"/>"#,
        r#"<fieldBegin id="77" type="CLICK_HERE" name="77"/>"#,
        r#"<footNote instId="77"/>"#,
        r#"<endNote instId="77"/>"#,
        r#"<pic id="77" instid="77"><img binaryItemIDRef="image1"/></pic>"#,
        r#"<fieldBegin id="77" type="MEMO"/>"#,
    ] {
        let mut removed = Removed::default();
        removed.record(&format!("<p>{definition}</p>")).unwrap();
        for reference in [
            r##"<p href="#77"/>"##,
            r##"<p href="Contents/section0.xml#77;1;0;0;"/>"##,
            r#"<p xmlns:x="urn:unknown" x:idref="77"/>"#,
            r#"<p xmlns:x="urn:unknown" x:idref="other"/>"#,
            r#"<connectLine subjectIDRef="77"/>"#,
            r#"<p mysteryRef="77"/>"#,
            r#"<p href="unresolved-location"/>"#,
            r#"<p href="http://#77"/>"#,
            r#"<p href="https:// /#77"/>"#,
            r#"<p href="mailto:"/>"#,
            r#"<p href="mailto:#77"/>"#,
            r##"<p href="#%74arget"/>"##,
            r#"<fieldBegin type="FORMULA"/>"#,
            r#"<fieldBegin type="HYPERLINK"><parameters><stringParam name="Path">#77</stringParam></parameters></fieldBegin>"#,
        ] {
            assert!(!removed.xml_is_safe(reference).unwrap(), "{definition} / {reference}");
        }
        assert!(removed
            .xml_is_safe(
                r##"<p href="#other" charPrIDRef="77"><img binaryItemIDRef="image1"/></p>"##
            )
            .unwrap());
    }
}

// 이것을 실패시키는 것: 이름 XML entity 해제 생략 또는 미해석 필드 구문 승인.
#[test]
fn field_parameters_fail_closed_and_external_links_survive() {
    let mut removed = Removed::default();
    removed.record(r#"<fieldBegin id="7" name="a&amp;b"/>"#).unwrap();
    assert!(!removed.xml_is_safe(r##"<p href="#a&amp;b"/>"##).unwrap());
    for parameters in [
        "",
        r#"<stringParam name="RefPath">?other;</stringParam>"#,
        r#"<stringParam name="RefPath">?other;</stringParam><stringParam name="Command">?a&amp;b;6;0;0;0;</stringParam>"#,
        r#"<stringParam name="RefPath">??other;;</stringParam><stringParam name="Command">?other;6;0;0;0;</stringParam>"#,
        r#"<stringParam name="RefPath">?other;</stringParam><stringParam name="RefPath">?other;</stringParam>"#,
        r#"<stringParam name="RefPath"><unknown/></stringParam>"#,
        r#"<stringParam>other</stringParam>"#,
        r#"<stringParam name="UnknownTarget">a&amp;b</stringParam><stringParam name="RefPath">?other;</stringParam><stringParam name="Command">?other;6;0;0;0;</stringParam>"#,
        r#"<stringParam name="Prop"><child/></stringParam><stringParam name="RefPath">?other;</stringParam><stringParam name="Command">?other;6;0;0;0;</stringParam>"#,
        r#"<stringParam>ignored</stringParam><stringParam name="RefPath">?other;</stringParam><stringParam name="Command">?other;6;0;0;0;</stringParam>"#,
        r#"<stringParam name="RefPath">?other;</stringParam><stringParam name="Command">?other;unknown;</stringParam>"#,
        r#"<stringParam name="RefPath">?%6Fther;</stringParam><stringParam name="Command">?%6Fther;6;0;0;0;</stringParam>"#,
        r#"<stringParam name="RefPath">?other;</stringParam><stringParam name="RefPath">?other;</stringParam><stringParam name="Command">?other;6;0;0;0;</stringParam>"#,
    ] {
        assert!(!removed
            .xml_is_safe(&format!(
                r#"<fieldBegin type="CROSSREF"><parameters>{parameters}</parameters></fieldBegin>"#
            ))
            .unwrap_or(false));
    }
    for kind in ["CLICK_HERE", "BOOKMARK", "MEMO", "SUMMERY", "DATE", "PATH", "PAGE_NUM"] {
        assert!(removed.xml_is_safe(&format!(r#"<fieldBegin type="{kind}"/>"#)).unwrap());
    }
    assert!(removed.xml_is_safe(r#"<p href=""/>"#).unwrap());
    assert!(removed.xml_is_safe(r#"<p xmlns:otherRef="urn:reference"/>"#).unwrap());
    for url in ["https://example.com/#a&amp;b", "http://example.com", "mailto:a@example.com"] {
        assert!(removed.xml_is_safe(&format!(r#"<p href="{url}"/>"#)).unwrap());
    }
    assert!(removed.xml_is_safe(r#"<fieldBegin type="CROSSREF"><parameters><stringParam name="RefPath">?#99;</stringParam><stringParam name="Command">?#99;3;0;0;0;</stringParam></parameters></fieldBegin>"#).unwrap());
}

// Exercise the final checker independently of the deliberately unchanged preflight.
fn finish_delete(base: &[u8]) -> Result<Vec<u8>, StructuralEditError> {
    let (decoded, _) = crate::structural::admit(base).unwrap();
    let mut package = RawPackage::read(base).unwrap();
    let path = "Contents/section0.xml";
    let xml = package.read_text_entry(path).unwrap();
    let output =
        crate::structural::splice_paragraphs(&xml, &decoded.source_maps[0], &[1], None).unwrap();
    package.replace_text_entry(path, output);
    let expected = crate::HwpxDecoder::decode(&package.write().unwrap()).unwrap().document;
    crate::structural::finish_edit(
        base,
        package,
        &decoded,
        expected,
        &crate::raw_package::OutputChanges { rewritten: vec![path.into()], ..Default::default() },
        &[crate::structural::SectionChange {
            section: 0,
            path: path.into(),
            original: xml,
            deleted: vec![1],
            inserted: None,
            heights: Vec::new(),
        }],
    )
}

// 이것을 실패시키는 것: 최종 생존 참조 검사 호출 제거 또는 다른 section 검사 생략.
#[test]
fn generated_deleted_definitions_check_survivors() {
    for (control, destination) in [
        (field(), "#대상"),
        (Control::bookmark("대상"), "#대상"),
        (Control::footnote_with_id(77, vec![paragraph("각주")]), "#77"),
    ] {
        let plain = generated(control.clone(), None);
        finish_delete(&plain).unwrap();
        let is_note = destination == "#77";
        let reference = Control::CrossRef {
            target: if is_note {
                hwpforge_core::control::RefTarget::Object(hwpforge_core::ObjectId::new(77))
            } else {
                hwpforge_core::control::RefTarget::Name("대상".into())
            },
            ref_type: if is_note {
                hwpforge_foundation::RefType::Footnote
            } else {
                hwpforge_foundation::RefType::Bookmark
            },
            content_type: hwpforge_foundation::RefContentType::Page,
            as_hyperlink: false,
            display_text: "참조".into(),
        };
        let linked = generated(control, Some(reference));
        assert_eq!(
            finish_delete(&linked).err(),
            Some(StructuralEditError::ReferenceStranded { section: 0, index: 1 }),
            "{destination}"
        );
        // Move the surviving reference into a different section.
        let decoded = crate::HwpxDecoder::decode(&linked).unwrap();
        let mut doc = decoded.document;
        let last = doc.sections_mut()[0].paragraphs.pop().unwrap();
        doc.add_section(Section::with_paragraphs(vec![last], PageSettings::a4()));
        let linked = crate::HwpxEncoder::encode(
            &doc.validate().unwrap(),
            &decoded.style_store,
            &decoded.image_store,
        )
        .unwrap();
        assert!(matches!(
            finish_delete(&linked),
            Err(StructuralEditError::ReferenceStranded { .. })
        ));
    }
}

// 이것을 실패시키는 것: 기존 책갈피/각주 보수 거부 제거. 참조 없는 입력도 개방하지 않는다.
#[test]
fn generated_bookmark_and_note_keep_existing_rejections() {
    for reference in [None, Some(Control::Hyperlink { text: "이동".into(), url: "#대상".into() })]
    {
        let base = generated(field(), reference);
        assert!(matches!(
            HwpxStructuralEditor::delete_paragraphs(
                &base,
                &[ParagraphLocator { section: 0, index: 1 }]
            ),
            Err(StructuralEditError::ProtectedFieldRange { .. })
        ));
    }

    for control in
        [Control::bookmark("대상"), Control::footnote_with_id(77, vec![paragraph("각주")])]
    {
        for reference in
            [None, Some(Control::Hyperlink { text: "이동".into(), url: "#대상".into() })]
        {
            let base = generated(control.clone(), reference);
            crate::structural::admit(&base).unwrap();
            assert!(matches!(
                HwpxStructuralEditor::delete_paragraphs(
                    &base,
                    &[ParagraphLocator { section: 0, index: 1 }]
                ),
                Err(StructuralEditError::ReferenceStranded { .. })
            ));
        }
    }
}

// 이것을 실패시키는 것: 원장 호출 전 깊이 보호 제거 또는 경계 깊이를 한 단계 축소.
#[test]
fn source_ledger_is_guarded_before_ancestor_search() {
    let nested = |depth| format!("{}{}", "<x>".repeat(depth), "</x>".repeat(depth));
    source(&nested(crate::decoder::XML_RECURSION_LIMIT)).unwrap();
    for depth in [crate::decoder::XML_RECURSION_LIMIT + 1, 100_000] {
        let xml = nested(depth);
        let before = hwpforge_core::source_xml::source_work();
        let error = source(&xml).unwrap_err();
        assert!(error.to_string().contains("invalid XML depth/root count"), "{error}");
        assert_eq!(hwpforge_core::source_xml::source_work(), before);
        let error = crate::structural::encoded_paragraph_spans(&xml).unwrap_err();
        assert!(error.to_string().contains("invalid XML depth/root count"), "{error}");
        assert_eq!(hwpforge_core::source_xml::source_work(), before);
    }
}

// 이것을 실패시키는 것: 플래그를 참조로 분류, 목록/연결점 0 기본값 거부 또는 비영 참조까지 면제.
#[test]
fn reference_attributes_distinguish_flags_and_absent_list_links() {
    let mut removed = Removed::default();
    removed.record("<tbl id='0'/>").unwrap();
    for attribute in ["hasTextRef", "hasNumRef"] {
        for value in ["0", "1"] {
            assert!(removed.xml_is_safe(&format!("<subList {attribute}='{value}'/>")).unwrap());
        }
    }
    assert!(removed.xml_is_safe("<point subjectIDRef='0'/>").unwrap());
    removed.record("<pic id='77'/>").unwrap();
    assert!(!removed.xml_is_safe("<point subjectIDRef='77'/>").unwrap());
    for attribute in ["linkListIDRef", "linkListNextIDRef"] {
        assert!(removed.xml_is_safe(&format!("<subList {attribute}='0'/>")).unwrap());
        assert!(!removed.xml_is_safe(&format!("<subList {attribute}='1'/>")).unwrap());
    }
}

// 이것을 실패시키는 것: 공용 파서에서 주석/PI/자식/중복 이름/미해석 소유 경로 허용, Text 누락, CDATA 전부 거부 또는 정상 메모 본문 거부.
#[test]
fn surviving_fields_use_unambiguous_event_values() {
    let mut removed = Removed::default();
    removed.record("<tbl id='77'/>").unwrap();
    for parameter in [
        "<stringParam name='Path'>#8<!-- split -->8</stringParam>",
        "<stringParam name='Path'>#8<?split x?>8</stringParam>",
        "<stringParam name='Path'>#8<x/>8</stringParam>",
        "<stringParam name='Path'>#8<stringParam name='Other'>88</stringParam>8</stringParam>",
        "<stringParam name='Path'>#77</stringParam><stringParam name='Path'>#88</stringParam>",
        "<unknown name='Path'>#88</unknown>",
    ] {
        let xml = format!(
            "<fieldBegin type='HYPERLINK'><parameters>{parameter}</parameters></fieldBegin>"
        );
        assert!(removed.xml_is_safe(&xml).is_err(), "{parameter}");
    }
    assert!(removed
        .xml_is_safe(
            "<fieldBegin type='HYPERLINK'><stringParam name='Path'>#88</stringParam></fieldBegin>"
        )
        .is_err());
    assert!(removed.xml_is_safe("<fieldBegin type='HYPERLINK'><parameters><parameters><stringParam name='Path'>#88</stringParam></parameters></parameters></fieldBegin>").is_err());
    assert!(removed.xml_is_safe("<fieldBegin type='HYPERLINK'><parameters><parameters/><stringParam name='Path'>#88</stringParam></parameters></fieldBegin>").is_err());
    assert!(removed.xml_is_safe("<fieldBegin type='HYPERLINK'><parameters><stringParam name='Path'>#8<![CDATA[8]]></stringParam></parameters></fieldBegin>").unwrap());
    assert!(removed.xml_is_safe("<fieldBegin type='CROSSREF'><parameters><stringParam name='RefPath'>?#<![CDATA[88]]>;</stringParam><stringParam name='Command'>?<![CDATA[#88]]>;6;0;0;0;</stringParam></parameters></fieldBegin>").unwrap());
}

// 이것을 실패시키는 것: parameter 컨테이너 상태 초기화 누락 또는 필드 본문 전체를 값으로 취급.
#[test]
fn memo_body_is_outside_field_parameter_values() {
    let mut removed = Removed::default();
    removed.record("<tbl id='77'/>").unwrap();
    for parameters in [
        "<parameters/>",
        "<parameters></parameters>",
        "<parameters><stringParam name='Command'>memo</stringParam></parameters>",
    ] {
        let xml = format!("<fieldBegin type='MEMO'>{parameters}<subList><p><t>memo</t></p></subList></fieldBegin>");
        assert!(removed.xml_is_safe(&xml).unwrap());
    }
}

fn crossref_xml(kind: &str, path: &str, command: &str) -> String {
    format!("<fieldBegin type='CROSSREF'><parameters><stringParam name='RefType'>{kind}</stringParam><stringParam name='RefPath'>{path}</stringParam><stringParam name='Command'>{command};0;0;0;0;</stringParam></parameters></fieldBegin>")
}

// 이것을 실패시키는 것: 숫자 정의/대상 문자열 비교, 책갈피 숫자 변환, RefPath/Command 문자열 비교 또는 불일치 승인.
#[test]
fn reference_targets_share_decoder_identity() {
    use hwpforge_foundation::RefType;
    for definition in ["77", "077", "+77", "0&#55;7"] {
        for key in ["id", "instid", "instId"] {
            let mut removed = Removed::default();
            removed
                .record(&format!("<p><tbl {key}='{definition}'/><bookmark name='077'/></p>"))
                .unwrap();
            for target in ["77", "077", "+77", "0<![CDATA[7]]>&#55;"] {
                let hyperlink = format!("<fieldBegin type='HYPERLINK'><parameters><stringParam name='Path'>#{target}</stringParam></parameters></fieldBegin>");
                assert!(!removed.xml_is_safe(&hyperlink).unwrap());
                assert!(!removed
                    .xml_is_safe(&crossref_xml("TARGET_TABLE", &format!("?#{target};"), "?#77"))
                    .unwrap());
                for attribute in ["href", "idRef", "subjectIDRef"] {
                    let prefix = if attribute == "href" { "#" } else { "" };
                    // Attributes cannot contain CDATA; entity syntax is still exercised.
                    let value = target.replace("<![CDATA[7]]>", "7");
                    assert!(
                        !removed
                            .xml_is_safe(&format!("<x {attribute}='{prefix}{value}'/>"))
                            .unwrap(),
                        "{definition}/{attribute}/{value}"
                    );
                }
            }
            assert!(!removed
                .xml_is_safe(&crossref_xml("TARGET_BOOKMARK", "?077;", "?077"))
                .unwrap());
            assert!(removed.xml_is_safe(&crossref_xml("TARGET_BOOKMARK", "?77;", "?77")).unwrap());
            assert!(removed.xml_is_safe(&crossref_xml("TARGET_TABLE", "?#088;", "?#+88")).unwrap());
            for command in ["?#89", "??#88", "#88"] {
                assert!(!removed
                    .xml_is_safe(&crossref_xml("TARGET_TABLE", "?#88;", command))
                    .unwrap());
            }
            assert!(!removed
                .xml_is_safe(&crossref_xml("TARGET_BOOKMARK", "?88;", "?088"))
                .unwrap());
        }
    }
    let mut names_only = Removed::default();
    names_only.record("<bookmark name='77'/>").unwrap();
    assert!(names_only.xml_is_safe(&crossref_xml("TARGET_TABLE", "?#77;", "?#077")).unwrap());
    assert_eq!(
        decode_crossref_target("?#+77;", &RefType::Table),
        RefTarget::Object(hwpforge_core::ObjectId::new(77))
    );
    assert_eq!(decode_crossref_target("?#077;", &RefType::Bookmark), RefTarget::Name("077".into()));
}

// 이것을 실패시키는 것: Raw 객체 참조 승인 또는 숫자 해석 시 trim/부호/기수/overflow 보정.
#[test]
fn unresolved_object_reference_syntax_is_refused() {
    use hwpforge_foundation::RefType;
    let mut removed = Removed::default();
    removed.record("<tbl id='77'/>").unwrap();
    for fragment in ["-77", "77.0", "0x4d", "18446744073709551616", "+", " 77", "77 "] {
        let hyperlink = format!("<fieldBegin type='HYPERLINK'><parameters><stringParam name='Path'>#{fragment}</stringParam></parameters></fieldBegin>");
        assert!(!removed.xml_is_safe(&hyperlink).unwrap(), "{fragment}");
    }
    for value in
        ["#-77", "#77.0", "#0x4d", "#18446744073709551616", "#+", "# 77", "#77 ", "77", "#other"]
    {
        let path = format!("?{value};");
        assert!(
            matches!(decode_crossref_target(&path, &RefType::Table), RefTarget::Raw(_)),
            "{value}"
        );
        assert!(
            !removed
                .xml_is_safe(&crossref_xml("TARGET_TABLE", &path, &format!("?{value}")))
                .unwrap(),
            "{value}"
        );
    }
}

// 이것을 실패시키는 것: 잘못된 숫자 정의 무시 또는 fieldBegin의 문자열 ID를 숫자로 강제.
#[test]
fn wire_definitions_keep_numeric_and_string_id_contracts() {
    for value in ["-77", "77.0", "0x4d", "18446744073709551616", "+", " 77", "77 "] {
        for key in ["id", "instid", "instId"] {
            let mut removed = Removed::default();
            assert!(removed.record(&format!("<tbl {key}='{value}'/>")).is_err(), "{key}/{value}");
        }
    }
    let mut removed = Removed::default();
    removed.record("<fieldBegin id='opaque'/>").unwrap();
    assert!(!removed.xml_is_safe("<fieldEnd beginIDRef='opaque'/>").unwrap());
    assert!(removed.xml_is_safe("<fieldEnd beginIDRef='other'/>").unwrap());
    removed.record("<pic id='' instid=''/>").unwrap();
    removed.record("<fieldBegin id='077'/>").unwrap();
    assert!(removed.xml_is_safe(&crossref_xml("TARGET_TABLE", "?#77;", "?#77")).unwrap());
    removed.record("<tbl id='077'/>").unwrap();
    for key in ["idref", "beginIDRef"] {
        assert!(!removed.xml_is_safe(&format!("<x {key}='077'/>")).unwrap());
        assert!(removed.xml_is_safe(&format!("<x {key}='77'/>")).unwrap());
    }
}
