use hwpforge_core::{
    Document, ImageStore, PageSettings, Paragraph, Run, Section, Table, TableCell, TableRow,
};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{HwpxEncoder, HwpxStructuralEditor, InsertPosition, ParagraphLocator};
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;
use hwpforge_core::source_xml::XmlSource;
use std::io::{Cursor, Read};

fn paragraph(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

// 이것을 실패시키는 것: 문단 ID 재배정 생략, 선언 밖 section 주석 추가 또는 비편집 part 변경.
#[test]
fn generated_structural_bytes_preserve_parts_and_exact_replacements() {
    for variant in 0..3 {
        let mut paragraphs = vec![paragraph("처음"), paragraph("가 & <나>"), paragraph("끝")];
        if variant == 1 {
            let cell = TableCell::new(vec![paragraph("안쪽")], HwpUnit::new(10000).unwrap());
            paragraphs.push(Paragraph::with_runs(
                vec![Run::table(
                    Table::new(vec![TableRow::new(vec![cell])]),
                    CharShapeIndex::new(0),
                )],
                ParaShapeIndex::new(0),
            ));
        }
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
        if variant == 2 {
            doc.add_section(Section::with_paragraphs(
                vec![paragraph("다른 구역"), paragraph("유지")],
                PageSettings::a4(),
            ));
        }
        let styles = hwpforge_smithy_hwpx::style_store_for_preset("default").unwrap();
        let base =
            HwpxEncoder::encode(&doc.validate().unwrap(), &styles, &ImageStore::new()).unwrap();
        let target = ParagraphLocator { section: 0, index: 1 };
        let deleted = HwpxStructuralEditor::delete_paragraphs(&base, &[target]).unwrap();
        let inserted = HwpxStructuralEditor::insert_paragraphs(
            &base,
            target,
            InsertPosition::After,
            &["새 문단".into(), "&둘".into()],
        )
        .unwrap();
        let original = package::section_xml(&base);
        let (_, source) = XmlSource::read(&original).unwrap();
        let spans: Vec<_> = source
            .elements()
            .iter()
            .filter(|e| e.name() == "p" && e.parent() == 1)
            .map(|e| e.outer())
            .collect();
        for (operation, output) in [("delete", deleted), ("insert", inserted)] {
            let mut expected = original[..spans[0].start].to_owned();
            expected.push_str(&original[spans[0].clone()]);
            if operation == "insert" {
                expected.push_str(&original[spans[1].clone()]);
                // Only the inserted paragraphs are literal wire expectations;
                // surviving paragraphs keep all bytes except their ID value.
                expected.push_str(r#"<hp:p id="2" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>새 문단</hp:t></hp:run></hp:p><hp:p id="3" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>&amp;둘</hp:t></hp:run></hp:p>"#);
            }
            let ids = if operation == "delete" {
                [("2", "1"), ("3", "2")]
            } else {
                [("2", "4"), ("3", "5")]
            };
            for (span, (old, new)) in spans.iter().skip(2).zip(ids) {
                expected.push_str(&original[span.clone()].replacen(
                    &format!(r#"id="{old}""#),
                    &format!(r#"id="{new}""#),
                    1,
                ));
            }
            expected.push_str(&original[spans.last().unwrap().end..]);
            let actual = package::section_xml(&output);
            assert_eq!(actual, expected, "variant {variant}, {operation}: section replacement");
            let tree = xml::Element::parse(&actual);
            assert_eq!(tree.children[1].attr("id"), Some("1"));
            assert_eq!(
                tree.children[1].find("hp:t").text,
                if operation == "delete" { "끝" } else { "가 & <나>" }
            );
            if operation == "insert" {
                assert_eq!(tree.children[2].find("hp:t").text, "새 문단");
                assert_eq!(tree.children[3].find("hp:t").text, "&둘");
                assert_eq!(tree.children[4].attr("id"), Some("4"));
            }
            let mut before = zip::ZipArchive::new(Cursor::new(&base)).unwrap();
            let mut after = zip::ZipArchive::new(Cursor::new(&output)).unwrap();
            assert_eq!(before.len(), after.len(), "part count");
            for index in 0..before.len() {
                let mut entry = before.by_index(index).unwrap();
                let name = entry.name().unwrap().to_string();
                if name == "Contents/section0.xml" {
                    continue;
                }
                let mut expected = Vec::new();
                entry.read_to_end(&mut expected).unwrap();
                let mut actual = Vec::new();
                after.by_name(&name).unwrap().read_to_end(&mut actual).unwrap();
                assert_eq!(
                    actual, expected,
                    "variant {variant}, {operation}: untouched part {name}"
                );
            }
        }
    }
}

fn tables_document(reference: Option<hwpforge_core::Control>) -> Vec<u8> {
    let table = |text, id| {
        let cell = TableCell::new(vec![paragraph(text)], HwpUnit::new(10000).unwrap());
        let mut table = Table::new(vec![TableRow::new(vec![cell])]);
        table.inst_id = Some(hwpforge_core::ObjectId::new(id));
        Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )
    };
    let mut last =
        if reference.is_some() { paragraph("링크 문단") } else { table("남은 표", 88) };
    if let Some(reference) = reference {
        last.runs.push(Run::control(reference, CharShapeIndex::new(0)));
    }
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![paragraph("처음"), table("삭제 표", 77), last],
        PageSettings::a4(),
    ));
    HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &hwpforge_smithy_hwpx::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap()
}

// 이것을 실패시키는 것: 플래그/목록 기본값을 참조로 거부하거나 생존 문단 ID 재배정 생략.
#[test]
fn generated_table_paragraph_deletion_preserves_surviving_table() {
    let base = tables_document(None);
    let output = HwpxStructuralEditor::delete_paragraphs(
        &base,
        &[ParagraphLocator { section: 0, index: 1 }],
    )
    .unwrap();
    use sha2::{Digest, Sha256};
    // Literal digest of the established output for this generated input.
    assert_eq!(
        Sha256::digest(&output).iter().map(|b| format!("{b:02x}")).collect::<String>(),
        "392db3db5dbd18ac8cdc95054179c64a48eb81fca7e683c5739ff1690da86bce"
    );
    let tree = xml::Element::parse(&package::section_xml(&output));
    assert_eq!(tree.children.len(), 2);
    assert_eq!(tree.children[1].find("hp:t").text, "남은 표");
    assert_eq!(tree.children[1].find("hp:tbl").attr("id"), Some("88"));
}

fn tables_with_internal_reference(reference: hwpforge_core::Control) -> Vec<u8> {
    let hyperlink = matches!(reference, hwpforge_core::Control::Hyperlink { .. });
    let base = tables_document(Some(reference));
    if hyperlink {
        // The encoder prefixes schemeless URLs. Exercise a real internal Path.
        let base = package::splice(&base, "http://#77", "#77");
        let tree = xml::Element::parse(&package::section_xml(&base));
        assert_eq!(tree.find("hp:parameters").children[0].text, "#77");
        base
    } else {
        base
    }
}

// 이것을 실패시키는 것: CDATA/문자 참조 누락, 숫자 대상을 문자열 비교, 삭제 wire ID 숫자 수집 누락.
#[test]
fn deleted_wire_table_rejects_encoded_surviving_field_targets() {
    use hwpforge_core::control::RefTarget;
    use hwpforge_core::{Control, ObjectId};
    use hwpforge_foundation::{RefContentType, RefType};
    for reference in [
        Control::Hyperlink { text: "링크".into(), url: "#77".into() },
        Control::CrossRef {
            target: RefTarget::Object(ObjectId::new(77)),
            ref_type: RefType::Table,
            content_type: RefContentType::Page,
            as_hyperlink: false,
            display_text: "참조".into(),
        },
    ] {
        let hyperlink = matches!(&reference, Control::Hyperlink { .. });
        let base = tables_with_internal_reference(reference);
        for definition in ["77", "077", "+77", "0&#55;7"] {
            let base = package::splice(
                &base,
                "<hp:tbl id=\"77\"",
                &format!("<hp:tbl id=\"{definition}\""),
            );
            for encoded in [
                "#<![CDATA[77]]>",
                "<![CDATA[#77]]>",
                "#7<![CDATA[7]]>",
                "#&#55;7",
                "#077",
                "#+77",
                "#0<![CDATA[7]]>&#55;",
                "#<![CDATA[+7]]>&#55;",
            ] {
                let base = package::splice(&base, "#77", encoded);
                let result = HwpxStructuralEditor::delete_paragraphs(
                    &base,
                    &[ParagraphLocator { section: 0, index: 1 }],
                );
                // Bare internal HYPERLINKs retain the existing admission refusal;
                // CROSSREFs reach the final surviving-reference check.
                let error = result.unwrap_err();
                if hyperlink {
                    assert!(
                        matches!(
                            error,
                            hwpforge_smithy_hwpx::StructuralEditError::NotRoundTripSafe { .. }
                        ),
                        "{definition}/{encoded}: {error:?}"
                    );
                } else {
                    assert!(
                        matches!(
                            error,
                            hwpforge_smithy_hwpx::StructuralEditError::ReferenceStranded { .. }
                        ),
                        "{definition}/{encoded}: {error:?}"
                    );
                }
            }
        }
    }
}

// 이것을 실패시키는 것: 해석 불가능한 객체 대상(Raw) 또는 잘못된 숫자 fragment 승인.
#[test]
fn deleted_wire_table_rejects_invalid_numeric_targets() {
    let base = tables_document(None);
    for definition in ["-77", "77.0", "0x4d", "18446744073709551616", "+", " 77", "77 "] {
        let base =
            package::splice(&base, "<hp:tbl id=\"77\"", &format!("<hp:tbl id=\"{definition}\""));
        assert!(
            HwpxStructuralEditor::delete_paragraphs(
                &base,
                &[ParagraphLocator { section: 0, index: 1 }]
            )
            .is_err(),
            "invalid definition {definition}: output bytes returned"
        );
    }
    use hwpforge_core::control::RefTarget;
    use hwpforge_core::{Control, ObjectId};
    use hwpforge_foundation::{RefContentType, RefType};
    for reference in [
        Control::Hyperlink { text: "링크".into(), url: "#77".into() },
        Control::CrossRef {
            target: RefTarget::Object(ObjectId::new(77)),
            ref_type: RefType::Table,
            content_type: RefContentType::Page,
            as_hyperlink: false,
            display_text: "참조".into(),
        },
    ] {
        let base = tables_with_internal_reference(reference);
        for target in ["-77", "77.0", "0x4d", "18446744073709551616", "+", " 77", "77 "] {
            let base = package::splice(&base, "#77", &format!("#{target}"));
            assert!(
                HwpxStructuralEditor::delete_paragraphs(
                    &base,
                    &[ParagraphLocator { section: 0, index: 1 }]
                )
                .is_err(),
                "{target}: output bytes returned"
            );
        }
    }
}

// 이것을 실패시키는 것: 최종 XML 검사 깊이 제한 제거.
#[test]
fn deleted_table_with_deep_unmodeled_part_returns_depth_error() {
    let base = tables_document(None);
    let original = package::part(&base, "version.xml");
    let nested = format!("{}{}", "<x>".repeat(100_000), "</x>".repeat(100_000));
    let base = package::splice_part(&base, "version.xml", &original, &nested);
    let error = HwpxStructuralEditor::delete_paragraphs(
        &base,
        &[ParagraphLocator { section: 0, index: 1 }],
    )
    .unwrap_err();
    assert!(error.to_string().contains("invalid XML depth/root count"), "{error}");
}
