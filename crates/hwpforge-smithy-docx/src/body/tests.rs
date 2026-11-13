use crate::{DocxDecoded, DocxError};

// Structural tests isolate their losses; format and page defaults are asserted separately.
fn decode_structure(bytes: &[u8]) -> crate::DocxResult<DocxDecoded> {
    let mut d = crate::decode_docx(bytes)?;
    d.warnings.retain(|w| {
        w.code() != "DOCX_FORMAT_DEFAULTED" && !matches!(w.loss(), "page_size" | "page_margins")
    });
    Ok(d)
}
use std::io::{Cursor, Write};
use zip::{write::SimpleFileOptions, ZipWriter};

pub(super) const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
fn zip(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, text) in entries {
        writer
            .start_file(
                *name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(text.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
pub(super) fn package(body: &str, extras: &[(&str, &str, &str)]) -> Vec<u8> {
    let main = format!("<w:document xmlns:w='{W}' xmlns:mc='{MC}' xmlns:m='http://schemas.openxmlformats.org/officeDocument/2006/math'><w:body>{body}</w:body></w:document>");
    let root = "<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='main' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument' Target='text/body.xml'/></Relationships>";
    let mut types = String::from("<Types xmlns='http://schemas.openxmlformats.org/package/2006/content-types'><Default Extension='rels' ContentType='application/vnd.openxmlformats-package.relationships+xml'/><Override PartName='/text/body.xml' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml'/>");
    let mut rels = String::from(
        "<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'>",
    );
    for (i, (kind, name, _)) in extras.iter().enumerate() {
        let mime = if *kind == "theme" {
            "application/vnd.openxmlformats-officedocument.theme+xml".to_owned()
        } else {
            format!("application/vnd.openxmlformats-officedocument.wordprocessingml.{kind}+xml")
        };
        types.push_str(&format!("<Override PartName='/text/{name}' ContentType='{mime}'/>"));
        rels.push_str(&format!("<Relationship Id='r{i}' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}' Target='{name}'/>"));
    }
    types.push_str("</Types>");
    rels.push_str("</Relationships>");
    let names: Vec<_> = extras.iter().map(|(_, name, _)| format!("text/{name}")).collect();
    let mut entries = vec![
        ("[Content_Types].xml", types.as_str()),
        ("_rels/.rels", root),
        ("text/body.xml", main.as_str()),
        ("text/_rels/body.xml.rels", rels.as_str()),
    ];
    for (i, (_, _, text)) in extras.iter().enumerate() {
        entries.push((&names[i], text));
    }
    zip(&entries)
}
fn decoded(body: &str) -> DocxDecoded {
    decode_structure(&package(body, &[])).unwrap()
}
fn texts(d: &DocxDecoded) -> Vec<String> {
    d.document.sections()[0].paragraphs.iter().map(|p| p.text_content()).collect()
}
fn counts(d: &DocxDecoded) -> Vec<usize> {
    d.document.sections()[0].paragraphs.iter().map(|p| p.run_count()).collect()
}

// 이것을 실패시키는 것: run 병합, 빈 문단 제거, 문자 참조/CDATA 누락, 탭/줄바꿈 변경.
#[test]
fn text_runs_empty_paragraphs_and_controls() {
    for (body, run_count) in [
        ("<w:p><w:r><w:t>AB</w:t></w:r></w:p>", 1),
        ("<w:p><w:r><w:t>A</w:t></w:r><w:r><w:t>B</w:t></w:r></w:p>", 2),
    ] {
        let d = decoded(body);
        assert_eq!(texts(&d), ["AB"]);
        assert_eq!(counts(&d), [run_count]);
    }
    let controls = decoded("<w:p><w:r><w:t>A</w:t><w:tab/><w:t>B</w:t><w:br w:type='textWrapping'/><w:t>C</w:t></w:r></w:p>");
    assert_eq!(texts(&controls), ["A\tB\nC"]);
    assert!(controls.warnings.is_empty());
    let d = decoded("<w:p><w:r><w:t>A</w:t></w:r><w:r><w:t>&#65;<![CDATA[B]]></w:t></w:r></w:p><w:p/><w:p><w:r><w:t>A</w:t><w:tab/><w:t>B</w:t><w:br/><w:t>C</w:t><w:cr/></w:r></w:p>");
    assert_eq!(texts(&d), ["AAB", "", "A\tB\nC\n"]);
    assert_eq!(counts(&d), [2, 1, 1]);
    assert!(d.warnings.is_empty());
    assert_eq!(d.registry.char_shape_count(), 1);
    assert_eq!(d.registry.para_shape_count(), 1);
    assert_eq!(d.registry.char_shapes[0].font, "함초롬바탕");
    assert_eq!(
        d.document.sections()[0].page_settings,
        hwpforge_core::page::PageSettings::default()
    );
}

// 이것을 실패시키는 것: trim, preserve 무시, 경계 공백 없는 텍스트에 경고 추가.
// 공백 근사 경고 코드를 DOCX_WHITESPACE_PRESERVED로 되돌리기.
#[test]
fn whitespace_preserves_source_and_warns_once_per_value() {
    let d = decoded("<w:p><w:r><w:t xml:space='preserve'> A  B </w:t><w:t> A&#32;<![CDATA[ B ]]></w:t><w:t>A</w:t></w:r></w:p>");
    assert_eq!(texts(&d), [" A  B  A  B A"]);
    assert_eq!(d.warnings.len(), 1);
    let w = &d.warnings[0];
    assert_eq!(
        (w.code(), w.loss(), w.action(), w.count(), w.part(), w.location()),
        (
            "DOCX_WHITESPACE_APPROXIMATED",
            "whitespace_semantics",
            "preserve_source",
            1,
            Some("text/body.xml"),
            Some("t#6")
        )
    );
}

// 이것을 실패시키는 것: 필드 명령 출력, 결과 제외, 중첩 flatten 제거, 빈 결과 경고 제거.
#[test]
fn saved_field_results_and_nested_fields() {
    let separate_runs = decoded("<w:p><w:r><w:fldChar w:fldCharType='begin'/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r><w:r><w:fldChar w:fldCharType='separate'/></w:r><w:r><w:t>7</w:t></w:r><w:r><w:fldChar w:fldCharType='end'/></w:r></w:p>");
    assert_eq!(texts(&separate_runs), ["7"]);
    assert_eq!(counts(&separate_runs), [1]);
    for body in ["<w:p><w:fldSimple w:instr='PAGE'><w:r><w:t>7</w:t></w:r></w:fldSimple></w:p>",
        "<w:p><w:r><w:fldChar w:fldCharType='begin'/><w:instrText>PAGE</w:instrText><w:fldChar w:fldCharType='separate'/><w:t>7</w:t><w:fldChar w:fldCharType='end'/></w:r></w:p>"] {
        let d = decoded(body);
        assert_eq!(texts(&d), ["7"]);
        assert_eq!(d.warnings.len(), 1);
        assert_eq!(d.warnings[0].code(), "DOCX_FIELD_INSTRUCTION_OMITTED");
    }
    for body in ["<w:p><w:fldSimple w:instr='PAGE'/></w:p>", "<w:p><w:r><w:fldChar w:fldCharType='begin'/><w:instrText>PAGE</w:instrText><w:fldChar w:fldCharType='end'/></w:r></w:p>"] {
        let d = decoded(body);
        assert_eq!(texts(&d), [""]);
        assert_eq!(counts(&d), [1]);
        assert!(d.warnings.iter().any(|w| w.code() == "DOCX_FIELD_RESULT_MISSING"));
    }
    let d = decoded("<w:p><w:r><w:fldChar w:fldCharType='begin'/><w:instrText>OUTER</w:instrText><w:fldChar w:fldCharType='separate'/><w:t>A</w:t></w:r><w:fldSimple w:instr='INNER'><w:r><w:t>7</w:t></w:r></w:fldSimple><w:r><w:t>B</w:t><w:fldChar w:fldCharType='end'/></w:r></w:p>");
    assert_eq!(texts(&d), ["A7B"]);
    assert_eq!(
        d.warnings.iter().find(|w| w.code() == "DOCX_FIELD_INSTRUCTION_OMITTED").unwrap().count(),
        2
    );
    assert!(d.warnings.iter().any(|w| w.code() == "DOCX_NESTED_FIELD_FLATTENED"));
}

// 이것을 실패시키는 것: begin/separate/end 대응 검사 제거.
#[test]
fn ambiguous_field_boundaries_are_errors() {
    for content in ["<w:fldChar w:fldCharType='begin'/>", "<w:fldChar w:fldCharType='end'/>",
        "<w:fldChar w:fldCharType='separate'/>", "<w:fldChar w:fldCharType='begin'/><w:fldChar w:fldCharType='separate'/><w:fldChar w:fldCharType='separate'/><w:fldChar w:fldCharType='end'/>",
        "<w:fldChar/>"] {
        assert!(matches!(decode_structure(&package(&format!("<w:p><w:r>{content}</w:r></w:p>"), &[])), Err(DocxError::Invalid { detail, .. }) if detail.contains("field")));
    }
}

// 이것을 실패시키는 것: 변경 요소 목록 축소, namespace 대신 prefix/local 비교,
// 부속 part/제외 객체/Choice/SDT에서 검사 끄기, 위치/수락 안내 제거.
// customXmlIns/Del/MoveFrom/MoveTo RangeStart/End 각각의 감지 목록 제거.
#[test]
fn revisions_are_rejected_in_every_selected_location() {
    let elements = [
        "ins",
        "del",
        "moveFrom",
        "moveTo",
        "moveFromRangeStart",
        "moveFromRangeEnd",
        "moveToRangeStart",
        "moveToRangeEnd",
        "pPrChange",
        "rPrChange",
        "sectPrChange",
        "tblPrChange",
        "tblPrExChange",
        "tblGridChange",
        "trPrChange",
        "tcPrChange",
        "cellIns",
        "cellDel",
        "cellMerge",
        "numberingChange",
        "customXmlInsRangeStart",
        "customXmlInsRangeEnd",
        "customXmlDelRangeStart",
        "customXmlDelRangeEnd",
        "customXmlMoveFromRangeStart",
        "customXmlMoveFromRangeEnd",
        "customXmlMoveToRangeStart",
        "customXmlMoveToRangeEnd",
    ];
    for element in elements {
        let change = format!("<w:{element}/>");
        for (before, after) in [
            ("<w:p>", "</w:p>"),
            ("<w:tbl>", "</w:tbl>"),
            ("<w:drawing>", "</w:drawing>"),
            ("<w:pict>", "</w:pict>"),
            ("<w:altChunk>", "</w:altChunk>"),
            ("<m:oMath>", "</m:oMath>"),
            ("<w:sdt><w:sdtPr>", "</w:sdtPr></w:sdt>"),
            ("<w:p><w:r><w:object>", "</w:object></w:r></w:p>"),
            (
                "<mc:AlternateContent><mc:Choice>",
                "</mc:Choice><mc:Fallback><w:p/></mc:Fallback></mc:AlternateContent>",
            ),
            ("<w:sdt><w:sdtContent>", "</w:sdtContent></w:sdt>"),
        ] {
            revision_error(
                &package(&format!("{before}{change}{after}"), &[]),
                element,
                "text/body.xml",
            );
        }
        revision_error(
            &package(&format!("<w:p><x:{element} xmlns:x='{W}'/></w:p>"), &[]),
            element,
            "text/body.xml",
        );
        for (kind, root) in [
            ("styles", "styles"),
            ("numbering", "numbering"),
            ("settings", "settings"),
            ("header", "hdr"),
            ("footer", "ftr"),
            ("footnotes", "footnotes"),
            ("endnotes", "endnotes"),
            ("comments", "comments"),
        ] {
            let part = format!("<w:{root} xmlns:w='{W}'>{change}</w:{root}>");
            revision_error(
                &package("<w:p/>", &[(kind, "extra.xml", &part)]),
                element,
                "text/extra.xml",
            );
        }
        let d = decoded(&format!(
            "<w:p><x:{element} xmlns:x='urn:unrelated'/><w:r><w:t>A</w:t></w:r></w:p>"
        ));
        assert_eq!(texts(&d), ["A"]);
    }
}
fn revision_error(bytes: &[u8], expected: &str, expected_part: &str) {
    let error = decode_structure(bytes).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("accept all changes in Word or LibreOffice and retry"));
    match error {
        DocxError::TrackedChange { part, location, element } => {
            assert_eq!(part, expected_part);
            assert_eq!(element, expected);
            assert!(location.starts_with(&format!("{expected}#")));
            assert!(message.contains(&location));
        }
        other => panic!("{other:?}"),
    }
}

// 이것을 실패시키는 것: trackRevisions를 실제 변경으로 취급하거나 경고 생성.
#[test]
fn recording_setting_without_changes_is_accepted() {
    let settings = format!("<w:settings xmlns:w='{W}'><w:trackRevisions/></w:settings>");
    let d = decode_structure(&package(
        "<w:p><w:r><w:t>A</w:t></w:r></w:p>",
        &[("settings", "settings.xml", &settings)],
    ))
    .unwrap();
    assert_eq!(texts(&d), ["A"]);
    assert!(d.warnings.is_empty());
}

// 이것을 실패시키는 것: 객체 내부 텍스트 유출, 객체 종류별 손실 분류 변경, 좁은 무시 목록 제거.
#[test]
fn excluded_objects_do_not_leak_text_or_orphan_marks() {
    let cases = [
        ("w:drawing", "image_shape"),
        ("w:pict", "image_shape"),
        ("w:object", "embedded_object"),
        ("w:altChunk", "embedded_object"),
        ("w:footnoteReference", "notes"),
        ("w:endnoteReference", "notes"),
        ("w:commentReference", "comments"),
        ("w:commentRangeStart", "comments"),
        ("w:commentRangeEnd", "comments"),
        ("m:oMath", "math"),
        ("m:oMathPara", "math"),
        ("w:sym", "symbol_character"),
        ("w:unknown", "unknown_element"),
    ];
    for (element, loss) in cases {
        let d = decoded(&format!(
            "<w:p><w:r><w:t>A</w:t><{element}><w:t>LEAK</w:t></{element}><w:t>B</w:t></w:r></w:p>"
        ));
        assert_eq!(texts(&d), ["AB"], "{element}");
        assert_eq!(d.warnings.len(), 1);
        assert_eq!(
            (
                d.warnings[0].code(),
                d.warnings[0].loss(),
                d.warnings[0].action(),
                d.warnings[0].count()
            ),
            ("DOCX_CONTENT_OMITTED", loss, "omit", 1)
        );
    }
    let d = decoded("<w:p><w:bookmarkStart w:id='1'/><w:proofErr w:type='spellStart'/><w:r><w:lastRenderedPageBreak/><w:noBreakHyphen/><w:softHyphen/></w:r><w:bookmarkEnd w:id='1'/></w:p>");
    assert_eq!(texts(&d), ["\u{2011}\u{ad}"]);
    assert!(d.warnings.is_empty());
}

// 이것을 실패시키는 것: 링크/SDT 텍스트 누락, sdtPr/Choice 텍스트 유출,
// Fallback 없음 손실 생략, 두 번째 Fallback 선택.
#[test]
fn links_controls_and_fallback_keep_only_selected_text() {
    let d = decoded("<w:p><w:hyperlink xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships' r:id='r1'><w:r><w:t>L</w:t></w:r></w:hyperlink><w:sdt><w:sdtPr><w:r><w:t>BAD</w:t></w:r></w:sdtPr><w:sdtContent><w:r><w:t>S</w:t></w:r></w:sdtContent></w:sdt><mc:AlternateContent><mc:Choice Requires='x'><w:r><w:t>BAD</w:t></w:r></mc:Choice><mc:Fallback><w:r><w:t>F</w:t></w:r></mc:Fallback><mc:Fallback><w:r><w:t>BAD</w:t></w:r></mc:Fallback></mc:AlternateContent></w:p>");
    assert_eq!(texts(&d), ["LSF"]);
    assert_eq!(counts(&d), [3]);
    let actual: Vec<_> =
        d.warnings.iter().map(|w| (w.code(), w.loss(), w.action(), w.count())).collect();
    assert_eq!(
        actual,
        [
            ("DOCX_LINK_TARGET_OMITTED", "hyperlink_target", "keep_text", 1),
            ("DOCX_CONTENT_CONTROL_FLATTENED", "content_control_function", "keep_content", 1),
            ("DOCX_ALTERNATE_CONTENT_SELECTED", "alternate_content_selection", "use_fallback", 1)
        ]
    );
    let d = decoded("<mc:AlternateContent><mc:Choice><w:p><w:r><w:t>BAD</w:t></w:r></w:p></mc:Choice></mc:AlternateContent><w:p/>");
    assert_eq!(texts(&d), [""]);
    assert_eq!(d.warnings[0].loss(), "alternate_content");
    let d = decoded("<w:p><mc:Fallback><w:r><w:t>BAD</w:t></w:r></mc:Fallback></w:p>");
    assert_eq!(texts(&d), [""]);
    assert_eq!(d.warnings[0].loss(), "unknown_element");
}

// 이것을 실패시키는 것: 서식 무음 수용, 속성별로 무한 집계, page/column 나누기 누락.
#[test]
fn formatting_breaks_attributes_and_values_have_explicit_losses() {
    let d = decoded("<w:p w:rsidR='1'><w:pPr><w:numPr/><w:rPr><w:b/></w:rPr><w:sectPr/></w:pPr><w:r><w:rPr><w:i/></w:rPr><w:t>A</w:t><w:br w:type='page'/><w:br w:type='column'/><w:br w:type='odd'/><w:t xml:space='odd' w:other='v'>B</w:t></w:r></w:p><w:sectPr/>");
    assert_eq!(texts(&d), ["A", "", "B"]);
    assert_eq!(d.document.sections().len(), 2);
    let actual: Vec<_> =
        d.warnings.iter().map(|w| (w.code(), w.loss(), w.action(), w.count())).collect();
    assert_eq!(
        actual,
        [
            ("DOCX_PARAGRAPH_SPLIT_APPROXIMATED", "paragraph_split", "split", 1),
            ("DOCX_COLUMN_BREAK_APPROXIMATED", "column_break", "page_break", 1),
            ("DOCX_VALUE_APPROXIMATED", "unsupported_value", "omit", 1),
            ("DOCX_VALUE_APPROXIMATED", "unsupported_value", "preserve_source", 1),
            ("DOCX_ATTRIBUTE_OMITTED", "unsupported_attribute", "ignore", 1),
            ("DOCX_LIST_REFERENCE_LOST", "list_reference", "omit", 1),
        ]
    );
    let p = &d.document.sections()[0].paragraphs[0];
    assert!(!p.page_break && !p.column_break);
}

// 이것을 실패시키는 것: 본문 서식 경고 미집계/첫 위치 갱신/위치를 키에 포함.
#[test]
fn repeated_run_loss_is_one_group_with_first_location() {
    let runs = "<w:r><w:rPr><w:caps/></w:rPr><w:t>A</w:t></w:r>".repeat(20_000);
    let d = decoded(&format!("<w:p>{runs}</w:p>"));
    assert_eq!(counts(&d), [20_000]);
    assert_eq!(d.warnings.len(), 1);
    assert_eq!(d.warnings[0].count(), 20_000);
    assert_eq!(d.warnings[0].location(), Some("caps#6"));
    assert_eq!(d.warnings[0].part(), Some("text/body.xml"));
}

// 이것을 실패시키는 것: 부속 part 미순회, 번호 정의의 미지원 자식 무경고, 잘못된 root 무경고, 공유 ZIP part 재읽기.
#[test]
fn ancillary_parts_are_scanned_once_without_collecting_text() {
    for (kind, root, loss) in [
        ("numbering", "numbering", "numbering_definition"),
        ("header", "hdr", "header"),
        ("footer", "ftr", "footer"),
        ("footnotes", "footnotes", "notes"),
        ("endnotes", "endnotes", "notes"),
        ("comments", "comments", "comments"),
    ] {
        let xml =
            format!("<w:{root} xmlns:w='{W}'><w:p><w:r><w:t>HIDDEN</w:t></w:r></w:p></w:{root}>");
        let d = decode_structure(&package("<w:p/>", &[(kind, "extra.xml", &xml)])).unwrap();
        assert_eq!(texts(&d), [""]);
        if kind == "numbering" {
            assert_eq!(
                d.warnings
                    .iter()
                    .map(|w| (w.code(), w.loss(), w.action(), w.count()))
                    .collect::<Vec<_>>(),
                [("DOCX_UNSUPPORTED_ATTRIBUTE", "numbering_definition", "ignore", 1),]
            );
        } else {
            assert_eq!(d.warnings.len(), 1);
        }
        assert_eq!(d.warnings.last().unwrap().loss(), loss);
        assert_eq!(d.warnings[0].part(), Some("text/extra.xml"));
        let xml = format!("<w:wrong xmlns:w='{W}'/>");
        let d = decode_structure(&package("<w:p/>", &[(kind, "extra.xml", &xml)])).unwrap();
        assert_eq!(d.warnings[0].code(), "DOCX_OPTIONAL_PART_OMITTED");
    }
    let bytes = package(
        "<w:p/>",
        &[(
            "header",
            "extra.xml",
            "<w:hdr xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'/>",
        )],
    );
    let bytes = rewrite(&bytes, "text/_rels/body.xml.rels", |s| {
        s.replace("</Relationships>", "<Relationship Id='alias' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/header' Target='./EXTRA.xml'/></Relationships>")
    });
    let d = decode_structure(&bytes).unwrap();
    assert_eq!(d.warnings.len(), 1);
    assert_eq!(d.warnings[0].count(), 1);
}
fn rewrite(bytes: &[u8], target: &str, change: impl FnOnce(String) -> String) -> Vec<u8> {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut entries = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let mut text = String::new();
        entry.read_to_string(&mut text).unwrap();
        entries.push((entry.name().unwrap().into_owned(), text));
    }
    let item = entries.iter_mut().find(|(name, _)| name == target).unwrap();
    item.1 = change(std::mem::take(&mut item.1));
    zip(&entries.iter().map(|(name, text)| (name.as_str(), text.as_str())).collect::<Vec<_>>())
}

// 이것을 실패시키는 것: opaque/형식 누락/불일치/외부 part를 복구하기, 누락 part를 치명 처리.
// 순회하지 않는 theme/image/Other의 외부 관계까지 변경 추적 검사 대상으로 거부.
#[test]
fn only_missing_optional_parts_are_recoverable() {
    let bytes = package(
        "<w:p/>",
        &[(
            "header",
            "extra.xml",
            "<w:hdr xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'/>",
        )],
    );
    for target in ["missing.xml", "external.xml"] {
        let changed = rewrite(&bytes, "text/_rels/body.xml.rels", |s| {
            let s = s.replace("Target='extra.xml'", &format!("Target='{target}'"));
            if target == "external.xml" {
                s.replace("Target='external.xml'", "Target='external.xml' TargetMode='External'")
            } else {
                s
            }
        });
        if target == "external.xml" {
            match decode_structure(&changed).unwrap_err() {
                DocxError::Invalid { part, element, detail } => {
                    assert_eq!(part, "text/body.xml");
                    assert_eq!(element, "relationship:r0");
                    assert_eq!(
                        detail,
                        "external WordprocessingML part cannot be checked for tracked changes"
                    );
                }
                other => panic!("{other:?}"),
            }
            for kind in ["theme", "image", "custom"] {
                let unselected = rewrite(&changed, "text/_rels/body.xml.rels", |s| {
                    s.replace("relationships/header", &format!("relationships/{kind}"))
                });
                let d = decode_structure(&unselected).unwrap();
                assert_eq!(texts(&d), [""]);
                assert_eq!(d.warnings.len(), 1);
                assert_eq!(
                    (d.warnings[0].code(), d.warnings[0].loss(), d.warnings[0].count()),
                    ("DOCX_OPTIONAL_PART_OMITTED", "uninterpreted_part", 1)
                );
            }
        } else {
            let d = decode_structure(&changed).unwrap();
            assert_eq!(d.warnings[0].code(), "DOCX_OPTIONAL_PART_OMITTED");
            assert_eq!(d.warnings[0].loss(), "optional_part");
        }
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes.clone())).unwrap();
    let mut entries = Vec::new();
    use std::io::Read;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();
        let mut content = String::new();
        file.read_to_string(&mut content).unwrap();
        entries.push((file.name().unwrap().into_owned(), content));
    }
    entries.push(("text/extra.xml/".into(), "".into()));
    let opaque = zip(&entries.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect::<Vec<_>>());
    assert!(matches!(decode_structure(&opaque), Err(DocxError::OpaquePart { .. })));
    for content_type in ["application/unknown", ""] {
        let changed = rewrite(&bytes, "[Content_Types].xml", |s| {
            if content_type.is_empty() {
                s.replace("<Override PartName='/text/extra.xml' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml'/>", "")
            } else {
                s.replace(
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
                    content_type,
                )
            }
        });
        if content_type.is_empty() {
            assert!(matches!(
                decode_structure(&changed),
                Err(DocxError::MissingContentType { .. })
            ));
        } else {
            assert!(matches!(
                decode_structure(&changed),
                Err(DocxError::ContentTypeMismatch { .. })
            ));
        }
    }
    for xml in [
        "<!DOCTYPE hdr><hdr/>",
        "<w:hdr xmlns:w='http://purl.oclc.org/ooxml/wordprocessingml/main'/>",
        "<hdr>",
    ] {
        let changed = rewrite(&bytes, "text/extra.xml", |_| xml.into());
        assert!(matches!(decode_structure(&changed), Err(DocxError::Invalid { .. })));
    }
    let mut corrupt = bytes.clone();
    let needle = b"<w:hdr xmlns:w=";
    let offset = corrupt.windows(needle.len()).position(|v| v == needle).unwrap();
    corrupt[offset] = b'!';
    assert!(matches!(decode_structure(&corrupt), Err(DocxError::Package { .. })));
}

// 이것을 실패시키는 것: body/root/문단/run 문맥 검사를 제거하거나 잘못된 구조 수용.
#[test]
fn body_structure_and_unknown_semantics_are_explicit() {
    let bytes = package("<w:p/>", &[]);
    for xml in ["<root xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'><w:body><w:p/></w:body></root>", "<document/>", "<w:document xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'/>", "<w:document xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'><w:body/><w:body/></w:document>"] {
        assert!(matches!(decode_structure(&rewrite(&bytes, "text/body.xml", |_| xml.into())), Err(DocxError::Invalid { .. })));
    }
    for body in [
        "<w:p><w:p/></w:p>",
        "<w:r/>",
        "<w:p><w:r><w:r/></w:r></w:p>",
        "<w:p><w:t>A</w:t></w:p>",
        "<w:p><w:tab/></w:p>",
    ] {
        assert!(matches!(decode_structure(&package(body, &[])), Err(DocxError::Invalid { .. })));
    }
    let d = decoded("<w:p>discard<w:r><w:instrText>COMMAND</w:instrText><w:t>A</w:t></w:r></w:p>");
    assert_eq!(texts(&d), ["A"]);
    assert_eq!(
        d.warnings.iter().map(|w| w.loss()).collect::<Vec<_>>(),
        ["unplaced_text", "field_function"]
    );
    assert_eq!(d.warnings.iter().map(|w| w.count()).collect::<Vec<_>>(), [1, 1]);
    let bytes =
        rewrite(&bytes, "text/body.xml", |s| s.replace("<w:body>", "<w:background/><w:body>"));
    assert_eq!(decode_structure(&bytes).unwrap().warnings[0].loss(), "outside_body");
}

// 이것을 실패시키는 것: prefix 종속, mc:Ignorable 무음 수용, 미해석 관계를 무경고 제외.
#[test]
fn namespaces_and_uninterpreted_relationships_are_diagnosed() {
    let bytes = package("<w:p><w:r><w:t>A</w:t></w:r></w:p>", &[]);
    let bytes = rewrite(&bytes, "text/body.xml", |s| {
        s.replace("w:", "한:").replace("xmlns:w=", "xmlns:한=")
    });
    assert_eq!(texts(&decode_structure(&bytes).unwrap()), ["A"]);
    let bytes = rewrite(&bytes, "text/body.xml", |s| {
        s.replace("<한:document ", "<한:document mc:Ignorable='w14' ")
    });
    assert_eq!(decode_structure(&bytes).unwrap().warnings[0].code(), "DOCX_ATTRIBUTE_OMITTED");
    let d = decoded(
        "<w:p><w:r><w:t>A</w:t><w:br xmlns:x='urn:other' x:type='page'/><w:t>B</w:t></w:r></w:p>",
    );
    assert_eq!(texts(&d), ["A\nB"]);
    assert_eq!(d.warnings[0].code(), "DOCX_ATTRIBUTE_OMITTED");
    for kind in ["image", "custom"] {
        let bytes = package("<w:p/>", &[(kind, "data.xml", "not XML")]);
        let d = decode_structure(&bytes).unwrap();
        assert_eq!(d.warnings[0].loss(), "uninterpreted_part");
    }
}

// 이것을 실패시키는 것: 문단/run 경계 비교 변경, saturating 증가 제거, 오류 위치/수량 변경.
#[test]
fn output_count_limits_are_checked_before_growth() {
    for (resource, limit) in [("paragraphs", 1_000_000), ("runs", 4_000_000)] {
        assert_eq!(
            super::admit_count("body", "p#3", resource, limit - 2, limit).unwrap(),
            limit - 1
        );
        assert_eq!(super::admit_count("body", "p#3", resource, limit - 1, limit).unwrap(), limit);
        for current in [limit, usize::MAX] {
            match super::admit_count("body", "p#3", resource, current, limit).unwrap_err() {
                DocxError::Limit {
                    part,
                    element,
                    resource: observed_resource,
                    observed,
                    limit: observed_limit,
                } => {
                    assert_eq!(
                        (part.as_str(), element.as_str(), observed_resource, observed_limit),
                        ("body", "p#3", resource, limit)
                    );
                    assert_eq!(observed, current.saturating_add(1));
                }
                other => panic!("{other:?}"),
            }
        }
    }
    assert_eq!((super::MAX_PARAGRAPHS, super::MAX_RUNS), (1_000_000, 4_000_000));
}

// 이것을 실패시키는 것: part별 텍스트 예산 초기화, 제외 영역의 텍스트 예산 검사 끄기.
#[test]
fn excluded_text_and_ancillary_parts_share_the_text_budget() {
    let mut budget = crate::xml::TextBudget::default();
    budget.add("previous", "t", 67_108_860).unwrap();
    let mut warnings = Vec::new();
    super::scan(b"<a>1234</a>", "first", "settings", false, &mut budget, &mut warnings).unwrap();
    assert!(matches!(
        super::scan(b"<a>5</a>", "second", "settings", false, &mut budget, &mut warnings),
        Err(DocxError::Limit { observed: 67_108_865, limit: 67_108_864, .. })
    ));
}

// 이것을 실패시키는 것: 바깥 명령 영역의 중첩 결과 출력, 존재하는 안쪽 결과를 없음으로 오판,
// 단순 필드 경계를 넘는 complex end 허용, 복합 필드 스택 한도 생략.
#[test]
fn field_results_respect_all_enclosing_boundaries() {
    let d = decoded("<w:p><w:r><w:fldChar w:fldCharType='begin'/><w:fldChar w:fldCharType='begin'/><w:fldChar w:fldCharType='separate'/><w:t>HIDDEN</w:t><w:fldChar w:fldCharType='end'/><w:fldChar w:fldCharType='separate'/><w:t>7</w:t><w:fldChar w:fldCharType='end'/></w:r></w:p>");
    assert_eq!(texts(&d), ["7"]);
    assert!(!d.warnings.iter().any(|w| w.code() == "DOCX_FIELD_RESULT_MISSING"));
    for body in ["<w:p><w:fldSimple w:instr='X'><w:r><w:fldChar w:fldCharType='end'/></w:r></w:fldSimple></w:p>", "<w:p><w:fldSimple w:instr='X'><w:r><w:fldChar w:fldCharType='begin'/></w:r></w:fldSimple></w:p>"] {
        assert!(matches!(decode_structure(&package(body, &[])), Err(DocxError::Invalid { detail, .. }) if detail == "unmatched field end"));
    }
    let begin = "<w:fldChar w:fldCharType='begin'/>".repeat(129);
    assert!(matches!(
        decode_structure(&package(&format!("<w:p><w:r>{begin}</w:r></w:p>"), &[])),
        Err(DocxError::Limit { resource: "field nesting", observed: 129, limit: 128, .. })
    ));
}

// 이것을 실패시키는 것: 내용 제외 후 빈 구역 방치, 최소 빈 문단/run 생성 생략.
#[test]
fn empty_body_and_excluded_only_body_have_valid_minimal_structure() {
    for body in ["", "<w:drawing><w:p><w:r><w:t>hidden</w:t></w:r></w:p></w:drawing>"] {
        let d = decoded(body);
        assert_eq!(texts(&d), [""]);
        assert_eq!(counts(&d), [1]);
        assert_eq!(d.warnings.len(), usize::from(!body.is_empty()));
        assert!(d.document.validate().is_ok());
    }
}

// 이것을 실패시키는 것: 설정 경고 누락, trackRevisions를 손실로 집계, 중첩 설정 중복 집계, 기본 탭 진단을 document_settings와 합침.
// settings/root/namespace 범위를 넓혀 다른 part·잘못된 root·동명이종 요소를 혼동.
#[test]
fn document_settings_have_explicit_losses() {
    for (children, loss, expected) in [
        ("<w:defaultTabStop w:val='1440'/>", "list_marker_default_tab_settings", 1),
        ("<w:trackRevisions/>", "document_settings", 0),
        ("<w:compat><w:balanceSingleByteDoubleByteWidth/></w:compat>", "document_settings", 0),
        ("<w:compat><w:balanceSingleByteDoubleByteWidth/><w:compatSetting w:name='x'/></w:compat>", "document_settings", 1),
        ("<w:trackRevisions/><w:defaultTabStop w:val='1440'/>", "list_marker_default_tab_settings", 1),
        ("<w:compat><w:compatSetting w:name='x'/></w:compat><x:trackRevisions xmlns:x='urn:other'/>", "document_settings", 2),
    ] {
        let settings = format!("<w:settings xmlns:w='{W}'>{children}</w:settings>");
        let d = decode_structure(&package("<w:p><w:r><w:t>A</w:t><w:tab/><w:t>B</w:t></w:r></w:p>", &[("settings", "settings.xml", &settings)])).unwrap();
        assert_eq!(texts(&d), ["A\tB"]);
        if expected == 0 {
            assert!(d.warnings.is_empty());
        } else {
            assert_eq!(d.warnings.len(), 1);
            let warning = &d.warnings[0];
            assert_eq!((warning.code(), warning.loss(), warning.action(), warning.count(), warning.part()),
                ("DOCX_DOCUMENT_SETTINGS_OMITTED", loss, "ignore", expected, Some("text/settings.xml")));
        }
    }
    let header = format!("<w:hdr xmlns:w='{W}'><w:defaultTabStop/></w:hdr>");
    let d = decode_structure(&package("<w:p/>", &[("header", "header.xml", &header)])).unwrap();
    assert_eq!(d.warnings.len(), 1);
    assert_eq!((d.warnings[0].code(), d.warnings[0].loss()), ("DOCX_CONTENT_OMITTED", "header"));
    let wrong = format!("<w:wrong xmlns:w='{W}'><w:defaultTabStop/></w:wrong>");
    let d = decode_structure(&package("<w:p/>", &[("settings", "settings.xml", &wrong)])).unwrap();
    assert_eq!(d.warnings.len(), 1);
    assert_eq!(d.warnings[0].code(), "DOCX_OPTIONAL_PART_OMITTED");
}

// 이것을 실패시키는 것: 각 wrapper를 차단하거나 속성 자식의 텍스트를 출력, wrapper 경고 제거.
#[test]
fn structured_wrappers_keep_content_and_discard_properties() {
    for wrapper in ["smartTag", "customXml", "dir", "bdo"] {
        let d = decoded(&format!(
            "<w:p><w:{wrapper} w:val='x'><w:r><w:t>2026-11-07</w:t></w:r></w:{wrapper}></w:p>"
        ));
        assert_eq!(texts(&d), ["2026-11-07"]);
        assert_eq!(d.warnings.len(), 1);
        let w = &d.warnings[0];
        assert_eq!(
            (w.code(), w.loss(), w.action(), w.count()),
            ("DOCX_STRUCTURED_WRAPPER_FLATTENED", "structured_wrapper", "keep_content", 1)
        );
    }
    let d = decoded("<w:customXml><w:p><w:r><w:t>A</w:t></w:r></w:p></w:customXml>");
    assert_eq!(texts(&d), ["A"]);
    assert_eq!(d.warnings[0].count(), 1);
    for property in ["customXmlPr", "smartTagPr"] {
        let d = decoded(&format!("<w:p><w:customXml><w:{property}><w:r><w:t>HIDDEN</w:t></w:r></w:{property}><w:r><w:t>B</w:t></w:r></w:customXml></w:p>"));
        assert_eq!(texts(&d), ["B"]);
        assert_eq!(d.warnings.len(), 1);
        assert_eq!(d.warnings[0].count(), 2);
        revision_error(
            &package(
                &format!("<w:customXml><w:{property}><w:ins/></w:{property}></w:customXml>"),
                &[],
            ),
            "ins",
            "text/body.xml",
        );
    }
}

fn repeated_selection_package(
    relations: usize,
    entries: usize,
    length: usize,
    missing: bool,
    mime: &str,
) -> Vec<u8> {
    let directory = "a".repeat(length);
    let main = format!("{directory}/body.xml");
    let root = format!("<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='main' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument' Target='{main}'/></Relationships>");
    let types = format!("<Types xmlns='http://schemas.openxmlformats.org/package/2006/content-types'><Default Extension='rels' ContentType='application/vnd.openxmlformats-package.relationships+xml'/><Override PartName='/{main}' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml'/><Override PartName='/{directory}/h000.xml' ContentType='{mime}'/></Types>");
    let mut rels = String::from(
        "<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'>",
    );
    for i in 0..relations {
        let target = if missing {
            "h999.xml"
        } else if i % 2 == 0 {
            "h000.xml"
        } else {
            "./H000.xml"
        };
        rels.push_str(&format!("<Relationship Id='r{i}' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/header' Target='{target}'/>"));
    }
    rels.push_str("</Relationships>");
    let mut all = vec![
        ("[Content_Types].xml".to_owned(), types), ("_rels/.rels".to_owned(), root),
        (main, format!("<w:document xmlns:w='{W}'><w:body><w:p><w:r><w:t>A</w:t></w:r></w:p></w:body></w:document>")),
        (format!("{directory}/_rels/body.xml.rels"), rels),
    ];
    for i in 0..entries {
        all.push((format!("{directory}/h{i:03}.xml"), format!("<w:hdr xmlns:w='{W}'/>")));
    }
    zip(&all.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect::<Vec<_>>())
}

// 이것을 실패시키는 것: canonical index/MIME 캐시 우회, raw hit/별칭의 ID 새 발급,
// 실패한 누락 조회 재검색, 읽기 경로에서 긴 MIME/ZIP 검색 재실행, 서로 다른 part의 ID 재사용.
// 관계 경고의 context를 키 조회 전에 생성. 이름 색인을 매 선택마다 재구축하거나
// find_index를 전체 ZIP 선형 비교로 바꾸면 바뀐 계수 단언도 실패한다.
#[test]
fn decode_selection_work_is_independent_of_repeated_relationships() {
    use crate::package::take_selection_work;
    let mime = "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
    for missing in [false, true] {
        for (entries, length, success_bytes, missing_bytes) in
            [(4, 32, 164, 123), (8, 32, 164, 123), (4, 64, 260, 187)]
        {
            let mut measured = Vec::new();
            let mut diagnostic_bytes = Vec::new();
            for (relations, expected_count) in [(1, 1), (8, 8), (64, 64)] {
                let bytes = repeated_selection_package(relations, entries, length, missing, mime);
                take_selection_work();
                crate::warning::take_diagnostic_bytes();
                let d = decode_structure(&bytes).unwrap();
                diagnostic_bytes.push(crate::warning::take_diagnostic_bytes());
                let work = take_selection_work();
                assert_eq!(texts(&d), ["A"]);
                assert_eq!(d.warnings.len(), 1);
                if missing {
                    assert_eq!(d.warnings[0].count(), expected_count);
                    assert_eq!(d.warnings[0].loss(), "optional_part");
                } else {
                    assert_eq!(d.warnings[0].count(), 1);
                    assert_eq!(d.warnings[0].loss(), "header");
                }
                measured.push(work);
            }
            eprintln!(
                "selection missing={missing} entries={entries} length={length}: {measured:?}"
            );
            assert_eq!(diagnostic_bytes[0], diagnostic_bytes[1]);
            assert_eq!(diagnostic_bytes[1], diagnostic_bytes[2]);
            assert_eq!(measured[0], measured[1]);
            assert_eq!(measured[1], measured[2]);
            assert_eq!(measured[0].lookups, 5);
            assert_eq!(measured[0].lookup_bytes, if length == 32 { 164 } else { 260 });
            assert_eq!(measured[0].type_checks, if missing { 1 } else { 2 });
            assert_eq!(measured[0].path_bytes, if missing { missing_bytes } else { success_bytes });
            assert_eq!(
                (measured[0].mime_calls, measured[0].mime_bytes),
                if missing { (4, 539) } else { (5, 685) }
            );
        }
    }
    // A mismatched MIME aborts decode, so later relationships must not be visited.
    for (length, expected_bytes) in [(32, 571), (4096, 4635)] {
        let wrong = "x".repeat(length);
        for relations in [1, 64] {
            let bytes = repeated_selection_package(relations, 4, 32, false, &wrong);
            take_selection_work();
            assert!(matches!(decode_structure(&bytes), Err(DocxError::ContentTypeMismatch { .. })));
            let work = take_selection_work();
            eprintln!("mismatch length={length} relations={relations}: {work:?}");
            assert_eq!(
                (work.lookups, work.path_bytes, work.mime_calls, work.mime_bytes),
                (5, 164, 5, expected_bytes)
            );
        }
    }
}

// 이것을 실패시키는 것: 본문 요소/속성 경고의 사유/part/location을 매번 생성.
#[test]
fn repeated_body_diagnostics_only_build_first_example() {
    let unknown = "q".repeat(4096);
    let unit = format!("<w:r {unknown}='x'><w:t> A </w:t><w:{unknown}/></w:r>");
    crate::warning::take_diagnostic_bytes();
    let first = decoded(&format!("<w:p>{unit}</w:p>"));
    let first_bytes = crate::warning::take_diagnostic_bytes();
    let repeated = decoded(&format!("<w:p>{}</w:p>", unit.repeat(64)));
    let repeated_bytes = crate::warning::take_diagnostic_bytes();
    assert_eq!(first.warnings.len(), 3);
    assert_eq!(repeated.warnings.len(), 3);
    assert_eq!(repeated.warnings.iter().map(|w| w.count()).collect::<Vec<_>>(), [64, 64, 64]);
    // 이것을 실패시키는 것: 쪽 기본값 진단 두 건 누락 또는 반복 횟수만큼 사유 생성.
    // Two page defaults add 2 * (13-byte part + 6-byte location + 47-byte reason).
    // Three fallback reasons replace "100% line" (9 bytes) with "single-line" (11).
    assert_eq!((first_bytes, repeated_bytes), (3840, 3840));
    eprintln!("body diagnostic bytes: {first_bytes} / {repeated_bytes}");
}

fn unique_selection_package(
    targets: usize,
    entries: usize,
    length: usize,
    missing: bool,
) -> Vec<u8> {
    let directory = "a".repeat(length);
    let main = format!("{directory}/body.xml");
    let root = format!("<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='main' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument' Target='{main}'/></Relationships>");
    let types = format!("<Types xmlns='http://schemas.openxmlformats.org/package/2006/content-types'><Default Extension='rels' ContentType='application/vnd.openxmlformats-package.relationships+xml'/><Default Extension='xml' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml'/><Override PartName='/{main}' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml'/></Types>");
    let mut rels = String::from(
        "<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'>",
    );
    for i in 0..targets {
        let prefix = if missing { 'z' } else { 'h' };
        rels.push_str(&format!("<Relationship Id='r{i}' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/header' Target='{prefix}{i:04}.xml'/>"));
    }
    rels.push_str("</Relationships>");
    let mut all = vec![
        ("[Content_Types].xml".to_owned(), types),
        ("_rels/.rels".to_owned(), root),
        (main, format!("<w:document xmlns:w='{W}'><w:body><w:p><w:r><w:t>A</w:t></w:r></w:p></w:body></w:document>")),
        (format!("{directory}/_rels/body.xml.rels"), rels),
    ];
    for i in 0..entries - 4 {
        all.push((format!("{directory}/h{i:04}.xml"), format!("<w:hdr xmlns:w='{W}'/>")));
    }
    zip(&all.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect::<Vec<_>>())
}

// 이것을 실패시키는 것: 고유 대상마다 ZIP 전체 선형 비교로 회귀,
// 이름 색인 재구축/누락 조회 재검색, 메타데이터의 이름 재조회,
// 이름 소문자화 누락, 성공/누락 혼동.
#[test]
fn unique_targets_only_hash_their_names_and_index_entries_once() {
    for missing in [false, true] {
        // Expected (entries, lookup, hash, successful comparison, missing comparison)
        // counts are literals, including the fixed metadata lookups.
        for (targets, entries, length, expected) in [
            (2, 16, 32, (627, 207, 834, 207, 123)),
            (4, 16, 32, (627, 291, 918, 291, 123)),
            (2, 32, 32, (1299, 207, 1506, 207, 123)),
            (2, 16, 64, (1075, 335, 1410, 335, 187)),
            (4, 32, 64, (2259, 483, 2742, 483, 187)),
        ] {
            let bytes = unique_selection_package(targets, entries, length, missing);
            crate::package::take_selection_work();
            let d = decode_structure(&bytes).unwrap();
            let work = crate::package::take_selection_work();
            eprintln!("unique missing={missing} targets={targets} entries={entries} length={length}: {work:?}");
            assert_eq!(work.lookups, if targets == 2 { 6 } else { 8 });
            assert_eq!(
                (work.entry_bytes, work.lookup_bytes, work.hash_bytes),
                (expected.0, expected.1, expected.2)
            );
            assert_eq!(work.path_bytes, if missing { expected.4 } else { expected.3 });
            assert_eq!(texts(&d), ["A"]);
            assert_eq!(d.warnings.len(), 1);
            assert_eq!(d.warnings[0].count(), if targets == 2 { 2 } else { 4 });
            assert_eq!(d.warnings[0].loss(), if missing { "optional_part" } else { "header" });
        }
    }
}

// 이것을 실패시키는 것: 지원 문단의 잘못 놓인 텍스트/문서 배경을 객체 종류 제외로 분류.
#[test]
fn misplaced_text_and_document_decoration_are_structural_losses() {
    let d = decoded("<w:p>discard<w:r><w:t>A</w:t></w:r></w:p>");
    assert_eq!(d.warnings[0].code(), "DOCX_CONTENT_LOST");
    assert_eq!(d.warnings[0].loss(), "unplaced_text");
    let bytes = rewrite(&package("<w:p/>", &[]), "text/body.xml", |s| {
        s.replace("<w:body>", "<w:background/><w:body>")
    });
    let d = decode_structure(&bytes).unwrap();
    assert_eq!(d.warnings[0].code(), "DOCX_CONTENT_LOST");
    assert_eq!(d.warnings[0].loss(), "outside_body");
}
