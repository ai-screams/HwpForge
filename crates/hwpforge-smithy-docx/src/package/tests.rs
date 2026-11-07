// Selection test helper constructs the owning relationship explicitly.
impl DocxPackage<'_> {
    fn read_test_target(&mut self, target: &Target, kind: &RelationshipKind) -> Result<Vec<u8>> {
        if *kind == RelationshipKind::OfficeDocument {
            return self.read_main();
        }
        self.read_relationship(&Relationship {
            id: "test".into(),
            target: target.clone(),
            kind: kind.clone(),
        })
    }
}
use super::*;
fn invalid(error: DocxError, element: &str, reason: &str) {
    match error {
        DocxError::Invalid { element: actual, detail, .. } => {
            assert_eq!(actual, element);
            assert!(detail.contains(reason), "expected {reason:?}: {detail}");
        }
        other => panic!("expected Invalid at {element}: {other:?}"),
    }
}

use std::io::{Cursor, Write};
use zip::{write::SimpleFileOptions, ZipWriter};

fn zip(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in entries {
        zip.start_file(
            *name,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn rel(id: &str, kind: &str, target: &str, mode: &str) -> String {
    format!(
        r#"<r:Relationship Id="{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}" Target="{target}"{mode}/>"#
    )
}
fn rels(children: &str) -> String {
    format!(
        r#"<r:Relationships xmlns:r="http://schemas.openxmlformats.org/package/2006/relationships">{children}</r:Relationships>"#
    )
}
fn types(children: &str) -> String {
    format!(
        r#"<c:Types xmlns:c="http://schemas.openxmlformats.org/package/2006/content-types"><c:Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>{children}</c:Types>"#
    )
}
fn main_types() -> String {
    types(
        r#"<c:Default Extension="xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
    )
}
fn document(root: &str, ct: &str, related: Option<&str>, extras: &[(&str, &str)]) -> Vec<u8> {
    let mut entries = vec![
        ("[Content_Types].xml", ct),
        ("_rels/.rels", root),
        (
            "custom/main.xml",
            "<w:document xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'/>",
        ),
    ];
    if let Some(related) = related {
        entries.push(("custom/_rels/main.xml.rels", related));
    }
    entries.extend_from_slice(extras);
    zip(&entries)
}
fn root() -> String {
    rels(&rel("main", "officeDocument", "custom/main.xml", ""))
}

// 이것을 실패시키는 것: 본문 경로 고정, Default 무시, 외부 문자열 변경, 재읽기 허용.
#[test]
fn custom_main_defaults_external_and_single_read() {
    let related = rels(&rel(
        "link",
        "hyperlink",
        "https://example.invalid/a?b=1&amp;c=2",
        r#" TargetMode="External""#,
    ));
    let bytes = document(&root(), &main_types(), Some(&related), &[("unused//opaque", "x")]);
    let mut package = DocxPackage::open(&bytes).unwrap();
    assert_eq!(package.main.name.as_ref(), "custom/main.xml");
    assert_eq!(package.relationships[0].id, "link");
    assert_eq!(
        package.relationships[0].kind,
        RelationshipKind::Other(
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink".into()
        )
    );
    assert_eq!(
        package.relationships[0].target,
        Target::External("https://example.invalid/a?b=1&c=2".into())
    );
    let external = package.relationships[0].target.clone();
    assert!(package
        .read_test_target(&external, &RelationshipKind::Other("hyperlink".into()))
        .unwrap_err()
        .to_string()
        .contains("external"));
    let main = Target::Internal(package.main.clone());
    assert!(package
        .read_test_target(&main, &RelationshipKind::OfficeDocument)
        .unwrap()
        .starts_with(b"<w:document"));
    assert!(package
        .read_test_target(&main, &RelationshipKind::OfficeDocument)
        .unwrap_err()
        .to_string()
        .contains("already read"));
}

// 이것을 실패시키는 것: 필수 본문의 오류 variant 또는 원인 문구를 다른 오류로 대체.
#[test]
fn main_relationship_rejection_matrix() {
    let cases = [
        (rels(""), main_types(), "observed 0"),
        (
            rels(
                &(rel("a", "officeDocument", "custom/main.xml", "")
                    + &rel("b", "officeDocument", "custom/main.xml", "")),
            ),
            main_types(),
            "observed 2",
        ),
        (rels(&rel("a", "officeDocument", "missing.xml", "")), main_types(), "missing.xml"),
        (
            rels(&rel(
                "a",
                "officeDocument",
                "https://example.invalid/",
                r#" TargetMode="External""#,
            )),
            main_types(),
            "external",
        ),
        (root(), types(r#"<c:Default Extension="xml" ContentType="text/xml"/>"#), "content type"),
        (
            root(),
            types(
                r#"<c:Default Extension="xml" ContentType="application/vnd.ms-word.document.macroEnabled.main+xml"/>"#,
            ),
            "macroEnabled",
        ),
        (root(), types(""), "content type"),
        (
            root().replace(
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/",
                "http://purl.oclc.org/ooxml/officeDocument/relationships/",
            ),
            main_types(),
            "Strict",
        ),
    ];
    for (root, ct, expected) in cases {
        let bytes = document(&root, &ct, None, &[]);
        let error = DocxPackage::open(&bytes).unwrap_err();
        match expected {
            "missing.xml" => assert!(matches!(error, DocxError::MissingPart { ref part } if part == "missing.xml")),
            "content type" if ct == types("") => assert!(matches!(error, DocxError::MissingContentType { ref part } if part == "custom/main.xml")),
            "content type" => assert!(matches!(error, DocxError::ContentTypeMismatch { ref part, ref actual, expected: "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml" } if part == "custom/main.xml" && actual == "text/xml")),
            "macroEnabled" => invalid(error, "#content-type", "macroEnabled content type 'application/vnd.ms-word.document.macroEnabled.main+xml' is forbidden"),
            "Strict" => invalid(error, "Relationship", "Strict OOXML URI"),
            "external" => invalid(error, "Relationship", "external officeDocument target 'https://example.invalid/' cannot be read as a part"),
            "observed 0" => invalid(error, "Relationship", "officeDocument relationships observed 0, required 1 (limit 1)"),
            "observed 2" => invalid(error, "Relationship", "officeDocument relationships observed 2, required 1 (limit 1)"),
            _ => unreachable!(),
        }
    }
}

// 이것을 실패시키는 것: metadata 누락을 일반 오류로 바꾸거나 ZIP 오류 문맥을 변경.
#[test]
fn framing_and_required_metadata() {
    let error = DocxPackage::open(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1])
        .unwrap_err()
        .to_string();
    assert!(error.contains("encrypted or legacy binary format"));
    assert!(
        matches!(DocxPackage::open(b"not zip").unwrap_err(), DocxError::Package { source: hwpforge_core::CoreError::InvalidStructure { ref context, .. }, .. } if context == "ZIP")
    );
    assert!(
        matches!(DocxPackage::open(&zip(&[])).unwrap_err(), DocxError::MissingPart { ref part } if part == "[Content_Types].xml")
    );
    invalid(
        DocxPackage::open(&zip(&[("[Content_Types].xml", "<a/>")])).unwrap_err(),
        "a",
        "namespace '' does not match",
    );
    assert!(
        matches!(DocxPackage::open(&zip(&[("[Content_Types].xml", &main_types())])).unwrap_err(), DocxError::MissingPart { ref part } if part == "_rels/.rels")
    );
    assert!(DocxPackage::open(&document(&root(), &main_types(), None, &[("unused//opaque", "x")]))
        .is_ok());
}

// 이것을 실패시키는 것: 선택 part를 open에서 검증, 읽기 선택의 누락/opaque/형식 오류를 혼동하거나 형식 대조 생략.
#[test]
fn related_kind_validation_matrix() {
    let cases = [
        (
            "styles",
            RelationshipKind::Styles,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",
            true,
        ),
        (
            "numbering",
            RelationshipKind::Numbering,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml",
            true,
        ),
        (
            "settings",
            RelationshipKind::Settings,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml",
            true,
        ),
        (
            "header",
            RelationshipKind::Header,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
            false,
        ),
        (
            "footer",
            RelationshipKind::Footer,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml",
            false,
        ),
        (
            "footnotes",
            RelationshipKind::Footnotes,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml",
            true,
        ),
        (
            "endnotes",
            RelationshipKind::Endnotes,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml",
            true,
        ),
        (
            "comments",
            RelationshipKind::Comments,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml",
            true,
        ),
        (
            "theme",
            RelationshipKind::Theme,
            "application/vnd.openxmlformats-officedocument.theme+xml",
            true,
        ),
        ("image", RelationshipKind::Image, "image/png", false),
    ];
    for (name, kind, mime, singleton) in cases {
        let ct = types(&format!(
            r#"<c:Override PartName="/custom/main.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><c:Default Extension="bin" ContentType="{mime}"/>"#
        ));
        let relation = rel("one", name, "../assets/part.bin", "");
        let bytes =
            document(&root(), &ct, Some(&rels(&relation)), &[("assets/part.bin", "not read yet")]);
        let package = DocxPackage::open(&bytes).unwrap();
        assert_eq!(package.relationships[0].kind, kind);
        assert!(
            matches!(&package.relationships[0].target, Target::Internal(part) if part.name.as_ref() == "assets/part.bin" && part.content_type.as_deref() == Some(mime))
        );
        let mut package = package;
        let target = package.relationships[0].target.clone();
        assert_eq!(package.read_test_target(&target, &kind).unwrap(), b"not read yet");
        for (target, extras, expected) in [
            ("../assets/absent.bin", vec![], "missing"),
            (
                "../assets/part.bin",
                vec![("assets/part.bin", "x"), ("assets/part.bin/", "")],
                "opaque",
            ),
        ] {
            let bytes = document(&root(), &ct, Some(&rels(&rel("one", name, target, ""))), &extras);
            let mut package = DocxPackage::open(&bytes).unwrap();
            let target = package.relationships[0].target.clone();
            let error = package.read_test_target(&target, &kind).unwrap_err();
            match expected {
                "missing" => assert!(
                    matches!(error, DocxError::MissingPart { ref part } if part == "assets/absent.bin")
                ),
                "opaque" => assert!(
                    matches!(error, DocxError::OpaquePart { ref part } if part == "assets/part.bin")
                ),
                _ => unreachable!(),
            }
            assert_eq!(package.consumed.len(), 3);
        }
        let bytes =
            document(&root(), &ct, Some(&rels(&rel("one", name, "../../escape.bin", ""))), &[]);
        invalid(
            DocxPackage::open(&bytes).unwrap_err(),
            "Relationship",
            "path escapes package root",
        );
        let bytes = document(
            &root(),
            &ct.replace(mime, "text/plain"),
            Some(&rels(&relation)),
            &[("assets/part.bin", "x")],
        );
        let mut package = DocxPackage::open(&bytes).unwrap();
        let target = package.relationships[0].target.clone();
        let expected = if name == "image" { "image/*" } else { mime };
        match package.read_test_target(&target, &kind).unwrap_err() {
            DocxError::ContentTypeMismatch { part, actual, expected: observed } => {
                assert_eq!(part, "assets/part.bin");
                assert_eq!(actual, "text/plain");
                assert_eq!(observed, expected);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(package.consumed.len(), 3);
        let bytes = document(
            &root(),
            &types(
                r#"<c:Override PartName="/custom/main.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
            ),
            Some(&rels(&relation)),
            &[("assets/part.bin", "x")],
        );
        let mut package = DocxPackage::open(&bytes).unwrap();
        let target = package.relationships[0].target.clone();
        assert!(matches!(&target, Target::Internal(part) if part.content_type.is_none()));
        assert!(
            matches!(package.read_test_target(&target, &kind).unwrap_err(), DocxError::MissingContentType { ref part } if part == "assets/part.bin")
        );
        assert_eq!(package.consumed.len(), 3);
        let bytes = document(
            &root(),
            &ct,
            Some(&rels(&(relation.clone() + &rel("two", name, "../assets/part.bin", "")))),
            &[("assets/part.bin", "x")],
        );
        if singleton {
            invalid(
                DocxPackage::open(&bytes).unwrap_err(),
                "Relationship",
                "relationships observed 2, limit 1",
            );
        } else {
            assert_eq!(DocxPackage::open(&bytes).unwrap().relationships.len(), 2);
        }
        let bytes = document(
            &root(),
            &ct,
            Some(&rels(&rel("one", name, "file:///outside", r#" TargetMode="External""#))),
            &[],
        );
        let mut package = DocxPackage::open(&bytes).unwrap();
        let target = package.relationships[0].target.clone();
        invalid(
            package.read_test_target(&target, &kind).unwrap_err(),
            "Relationship",
            "external target cannot be read as a package part",
        );
    }
}

// 이것을 실패시키는 것: 원본 ../ 일괄 거부, URI scheme/디렉터리 검사 삭제, 뒤쪽 colon 거부, percent 해제 또는 최종 이름 검사 누락.
#[test]
fn target_uri_matrix() {
    assert_eq!(
        resolve_target("custom/main.xml", "test.rels", "assets/a:b.xml").unwrap().as_ref(),
        "custom/assets/a:b.xml"
    );
    for target in [".", "..", "dir/.", "dir/.."] {
        assert!(resolve_target("custom/main.xml", "test.rels", target)
            .unwrap_err()
            .to_string()
            .contains("directory"));
    }

    for (source, target, expected) in [
        ("custom/main.xml", "../assets/a.xml", "assets/a.xml"),
        ("", "custom/./main.xml", "custom/main.xml"),
        ("a/b/main.xml", "../c/%41.xml", "a/c/%41.xml"),
        ("a/main.xml", "%2e%2e/x.xml", "a/%2e%2e/x.xml"),
    ] {
        assert_eq!(resolve_target(source, "test.rels", target).unwrap().as_ref(), expected);
    }
    for (target, reason) in [
        ("", "empty or network path"),
        ("//host/a", "empty or network path"),
        ("../../out.xml", "path escapes package root"),
        ("/../out.xml", "path escapes package root"),
        ("C:/a", "URI schemes and absolute drive paths are forbidden"),
        ("C:\\a", "URI schemes and absolute drive paths are forbidden"),
        ("file:///a", "URI schemes and absolute drive paths are forbidden"),
        ("https://example.invalid/a", "URI schemes and absolute drive paths are forbidden"),
        ("a\\b", "invalid relative URI character"),
        ("a b", "invalid relative URI character"),
        ("a?x", "invalid relative URI character"),
        ("a#x", "invalid relative URI character"),
        ("a%", "invalid percent escape"),
        ("a%0X", "invalid percent escape"),
        ("가.xml", "invalid relative URI character"),
        ("a|b", "invalid relative URI character"),
    ] {
        invalid(
            resolve_target("custom/main.xml", "test.rels", target).unwrap_err(),
            "Relationship",
            reason,
        );
    }
    for target in ["a//b", "../", "/a//b"] {
        assert!(matches!(
            resolve_target("custom/main.xml", "test.rels", target).unwrap_err(),
            DocxError::Package { source: hwpforge_core::CoreError::InvalidStructure { .. }, .. }
        ));
    }
}

// 이것을 실패시키는 것: metadata 오류의 요소/사유를 일반 오류로 대체.
#[test]
fn metadata_validation_matrix() {
    let override_ct = types(
        r#"<c:Default Extension="xml" ContentType="text/plain"/><c:Override PartName="/custom/main.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
    );
    assert!(DocxPackage::open(&document(&root(), &override_ct, None, &[])).is_ok());
    let cases = [
        root().replace(" Id=\"main\"", ""),
        root().replace("officeDocument", ""),
        root().replace("Target=", "Other="),
        root().replace("Target=", "TargetMode='bad' Target="),
        rels(
            &(rel("a", "officeDocument", "custom/main.xml", "") + &rel("a", "image", "a.png", "")),
        ),
        root().replace("package/2006/relationships", "wrong"),
        root().replace("r:Relationship ", "r:Other "),
        format!("<!DOCTYPE x>{}", root()),
        root().replace("custom/main.xml", "&evil;"),
        root()
            .replace("<r:Relationship ", "<r:wrapper><r:Relationship ")
            .replace("/></r:Relationships>", "/></r:wrapper></r:Relationships>"),
    ];
    let expected = [
        ("Relationship", "attribute 'Id' observed missing/empty, required 1"),
        ("Relationship", "officeDocument relationships observed 0, required 1 (limit 1)"),
        ("Relationship", "attribute 'Target' observed missing/empty, required 1"),
        ("Relationship", "TargetMode 'bad'; expected Internal or External"),
        ("Relationship", "duplicate relationship Id 'a' (observed 2, limit 1)"),
        ("Relationships", "namespace 'http://schemas.openxmlformats.org/wrong' does not match"),
        ("Other", "expected Relationship"),
        ("#document", "DTD is forbidden (observed 1, limit 0)"),
        ("Relationship", "unsupported entity '&evil;'"),
        ("wrapper", "expected Relationship"),
    ];
    assert_eq!(cases.len(), expected.len());
    for (bad, (element, reason)) in cases.into_iter().zip(expected) {
        invalid(
            DocxPackage::open(&document(&bad, &main_types(), None, &[])).unwrap_err(),
            element,
            reason,
        );
    }
    let cases = [
        types("<c:Default/>"),
        types(r#"<c:Override PartName="custom/main.xml" ContentType="x"/>"#),
        types(
            r#"<c:Default Extension="xml" ContentType="x"/><c:Default Extension="xml" ContentType="x"/>"#,
        ),
        types(
            r#"<c:Override PartName="/custom/main.xml" ContentType="x"/><c:Override PartName="/custom/main.xml" ContentType="x"/>"#,
        ),
        types("<c:Unknown/>"),
        main_types().replace("package/2006/content-types", "wrong"),
        types("bad text"),
    ];
    let expected = [
        ("Default", "attribute 'ContentType' observed missing/empty, required 1"),
        ("Override", "PartName 'custom/main.xml'; expected leading '/'"),
        ("Default", "duplicate content type definition (observed 2, limit 1)"),
        ("Override", "duplicate content type definition (observed 2, limit 1)"),
        ("Unknown", "attribute 'ContentType' observed missing/empty, required 1"),
        ("Types", "namespace 'http://schemas.openxmlformats.org/wrong' does not match"),
        ("Types", "unexpected metadata text"),
    ];
    assert_eq!(cases.len(), expected.len());
    for (bad, (element, reason)) in cases.into_iter().zip(expected) {
        invalid(
            DocxPackage::open(&document(&root(), &bad, None, &[])).unwrap_err(),
            element,
            reason,
        );
    }
}

// 이것을 실패시키는 것: 필수 part의 opaque/누락 variant를 서로 교환,
// 메타데이터 index 재사용 경로에서 opaque 검사를 제거.
#[test]
fn selected_parts_must_exist_and_be_transparent() {
    for path in
        ["[Content_Types].xml/", "_rels/.rels/", "custom/main.xml/", "custom/_rels/main.xml.rels/"]
    {
        let bytes = document(&root(), &main_types(), Some(&rels("")), &[(path, "")]);
        assert!(
            matches!(DocxPackage::open(&bytes).unwrap_err(), DocxError::OpaquePart { .. }),
            "{path}"
        );
    }
    let bytes = zip(&[("[Content_Types].xml", &main_types()), ("_rels/.rels", &root())]);
    assert!(
        matches!(DocxPackage::open(&bytes).unwrap_err(), DocxError::MissingPart { ref part } if part == "custom/main.xml")
    );
}

// 이것을 실패시키는 것: Other 형식 검증을 main 형식으로 바꾸거나 중복 관계 오류의 종류를 삭제.
#[test]
fn root_level_main_and_unknown_internal_targets() {
    let ct = types(
        r#"<c:Override PartName="/main" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><c:Default Extension="bin" ContentType="application/custom"/>"#,
    );
    let root = rels(
        &(rel("main", "officeDocument", "main", r#" TargetMode="Internal""#)
            + &rel("ignored", "metadata", "ignored.bin", "")),
    );
    let related = rels(&(rel("one", "custom", "%41.bin", "") + &rel("two", "custom", "A.bin", "")));
    let bytes = zip(&[
        ("[Content_Types].xml", &ct),
        ("_rels/.rels", &root),
        ("main", "main"),
        ("_rels/main.rels", &related),
        ("%41.bin", "percent"),
        ("A.bin", "letter"),
    ]);
    let mut package = DocxPackage::open(&bytes).unwrap();
    assert_eq!(package.relationships.len(), 2);
    assert_eq!(
        package
            .read_test_target(
                &package.relationships[0].target.clone(),
                &package.relationships[0].kind.clone()
            )
            .unwrap(),
        b"percent"
    );
    assert_eq!(
        package
            .read_test_target(
                &package.relationships[1].target.clone(),
                &package.relationships[1].kind.clone()
            )
            .unwrap(),
        b"letter"
    );
    assert!(
        matches!(&package.relationships[0].kind, RelationshipKind::Other(uri) if uri.ends_with("/custom"))
    );
    let duplicate = rels(
        &(rel("one", "officeDocument", "main", "") + &rel("two", "officeDocument", "main", "")),
    );
    let bytes = zip(&[
        ("[Content_Types].xml", &ct),
        ("_rels/.rels", &root),
        ("main", "main"),
        ("_rels/main.rels", &duplicate),
    ]);
    invalid(
        DocxPackage::open(&bytes).unwrap_err(),
        "Relationship",
        "OfficeDocument relationships observed 2, limit 1",
    );
}

// 이것을 실패시키는 것: 존재/opaque 오류 variant를 일반 오류로 바꾸거나 사전 검사를 뒤로 이동.
#[test]
fn failed_reads_are_consumed_and_selection_precedes_read() {
    let mut bytes = document(&root(), &main_types(), None, &[]);
    let at = bytes.windows(11).position(|v| v == b"<w:document").unwrap();
    bytes[at] = b'!'; // CRC mismatch only appears when this selected part is read.
    let mut package = DocxPackage::open(&bytes).unwrap();
    let main = Target::Internal(package.main.clone());
    assert!(matches!(
        package.read_test_target(&main, &RelationshipKind::OfficeDocument).unwrap_err(),
        DocxError::Package { .. }
    ));
    assert!(package
        .read_test_target(&main, &RelationshipKind::OfficeDocument)
        .unwrap_err()
        .to_string()
        .contains("already read"));
    assert!(
        matches!(package.read_part("absent").unwrap_err(), DocxError::MissingPart { ref part } if part == "absent")
    );
    let bytes = document(&root(), &main_types(), None, &[("opaque", "x"), ("opaque/", "")]);
    let mut package = DocxPackage::open(&bytes).unwrap();
    assert!(
        matches!(package.read_part("opaque").unwrap_err(), DocxError::OpaquePart { ref part } if part == "opaque")
    );
    assert_eq!(package.consumed.len(), 2);
}

// 이것을 실패시키는 것: 필수 속성/metadata 오류의 사유를 일반 오류로 대체.
#[test]
fn metadata_empty_values_namespaces_and_safe_values() {
    for (attr, reason) in [
        ("Id", "attribute 'Id' observed missing/empty, required 1"),
        ("Type", "attribute 'Type' observed missing/empty, required 1"),
        ("Target", "attribute 'Target' observed missing/empty, required 1"),
    ] {
        let input = rels(&r#"<r:Relationship Id="a" Type="urn:custom" Target="a.xml"/>"#.replace(
            &format!(
                "{attr}=\"{}\"",
                match attr {
                    "Id" => "a",
                    "Type" => "urn:custom",
                    _ => "a.xml",
                }
            ),
            &format!("{attr}=\"\""),
        ));
        let error = parse_relationships(input.as_bytes(), "test.rels", &mut TextBudget::default())
            .unwrap_err();
        invalid(error, "Relationship", reason);
    }
    for (input, element, reason) in [
        (
            root().replace("Relationships", "Wrong"),
            "Wrong",
            "unexpected metadata element at depth 1",
        ),
        (
            root().replace("custom/main.xml", ""),
            "Relationship",
            "attribute 'Target' observed missing/empty, required 1",
        ),
        (rels("text"), "Relationships", "unexpected metadata text"),
        (
            rels("<r:Relationship Id='a' Type='' Target='a'/>"),
            "Relationship",
            "attribute 'Type' observed missing/empty, required 1",
        ),
    ] {
        invalid(
            parse_relationships(input.as_bytes(), "test.rels", &mut TextBudget::default())
                .unwrap_err(),
            element,
            reason,
        );
    }
    for (input, element, reason) in [
        (types("<c:Unknown ContentType='x'/>"), "Unknown", "expected Override or Default"),
        (
            types("<c:Default Extension='' ContentType='x'/>"),
            "Default",
            "attribute 'Extension' observed missing/empty, required 1",
        ),
    ] {
        invalid(
            ContentTypes::parse(input.as_bytes(), &mut TextBudget::default()).err().unwrap(),
            element,
            reason,
        );
    }
    assert!(matches!(
        ContentTypes::parse(
            types("<c:Override PartName='/a//b' ContentType='x'/>").as_bytes(),
            &mut TextBudget::default()
        )
        .err()
        .unwrap(),
        DocxError::Package { source: hwpforge_core::CoreError::InvalidStructure { .. }, .. }
    ));
    let input = rels(&format!("\n{}\n", rel("a&amp;b", "styles", "a&#x41;.xml", "")));
    let parsed =
        parse_relationships(input.as_bytes(), "test.rels", &mut TextBudget::default()).unwrap();
    assert_eq!(parsed[0].id, "a&b");
    assert_eq!(parsed[0].target, "aA.xml");
    for (target, expected) in
        [("", "empty or network path"), ("a%XX", "percent escape"), ("a|b", "URI character")]
    {
        assert!(resolve_target("main.xml", "test.rels", target)
            .unwrap_err()
            .to_string()
            .contains(expected));
    }
}

// 이것을 실패시키는 것: 임베딩에도 macroEnabled 거부 적용, 중복 정의/중첩 metadata 오류를 일반 오류로 대체.
#[test]
fn independent_metadata_rejections() {
    for child in [
        r#"<c:Default Extension="xml" ContentType="text/xml"/>"#,
        r#"<c:Override PartName="/main.xml" ContentType="text/xml"/>"#,
    ] {
        let input = types(&child.repeat(2));
        let error =
            ContentTypes::parse(input.as_bytes(), &mut TextBudget::default()).err().unwrap();
        assert!(error.to_string().contains("duplicate content type"));
    }
    let input = rels("<r:Relationship Id='a' Type='urn:a' Target='a'><r:Relationship Id='b' Type='urn:b' Target='b'/></r:Relationship>");
    assert!(parse_relationships(input.as_bytes(), "test.rels", &mut TextBudget::default())
        .unwrap_err()
        .to_string()
        .contains("depth 3"));
    let ct = types(
        r#"<c:Override PartName="/custom/main.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><c:Default Extension="bin" ContentType="application/vnd.ms-excel.sheet.macroEnabled.12"/>"#,
    );
    let bytes = document(
        &root(),
        &ct,
        Some(&rels(&rel("a", "custom", "a.bin", ""))),
        &[("custom/a.bin", "x")],
    );
    let mut package = DocxPackage::open(&bytes).unwrap();
    let relation = &package.relationships[0];
    assert_eq!(
        package.read_test_target(&relation.target.clone(), &relation.kind.clone()).unwrap(),
        b"x"
    );
}

// 이것을 실패시키는 것: 루트 part 참조 거부 또는 원본 디렉터리에서 해석.
#[test]
fn absolute_part_references() {
    assert_eq!(
        resolve_target("custom/main.xml", "test.rels", "/tmp/a.xml").unwrap().as_ref(),
        "tmp/a.xml"
    );
    assert_eq!(resolve_target("custom/main.xml", "test.rels", "/a").unwrap().as_ref(), "a");
    let ct = types(
        r#"<c:Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><c:Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>"#,
    );
    let bytes = zip(&[
        ("[Content_Types].xml", &ct),
        ("_rels/.rels", &rels(&rel("main", "officeDocument", "/word/document.xml", ""))),
        ("word/document.xml", "body"),
        ("word/_rels/document.xml.rels", &rels(&rel("styles", "styles", "/word/styles.xml", ""))),
        ("word/styles.xml", "styles"),
    ]);
    let package = DocxPackage::open(&bytes).unwrap();
    assert_eq!(package.main.name.as_ref(), "word/document.xml");
    assert!(
        matches!(&package.relationships[0].target, Target::Internal(part) if part.name.as_ref() == "word/styles.xml")
    );
}

// 이것을 실패시키는 것: Override/Default 조회나 등록을 대소문자 구분, 부모 디렉터리에서 확장자 추출.
#[test]
fn content_type_names_ignore_ascii_case() {
    let input = types(
        r#"<c:Default Extension="PnG" ContentType="image/png"/><c:Override PartName="/WORD/STYLES.XML" ContentType="application/custom"/>"#,
    );
    let parsed = ContentTypes::parse(input.as_bytes(), &mut TextBudget::default()).unwrap();
    assert_eq!(parsed.get("media/image1.PNG"), Some("image/png"));
    assert_eq!(parsed.get("word/styles.xml"), Some("application/custom"));
    assert_eq!(parsed.get("WoRd/Styles.XmL"), Some("application/custom"));
    assert_eq!(parsed.get("a.d/b"), None);
    let input = types(r#"<c:Default Extension="d/b" ContentType="application/custom"/>"#);
    let parsed = ContentTypes::parse(input.as_bytes(), &mut TextBudget::default()).unwrap();
    assert_eq!(parsed.get("a.d/b"), None);
}

// 이것을 실패시키는 것: metadata/main/관계 entry 조회 중 하나를 대소문자 구분하거나 alias를 재읽기.
#[test]
fn entry_lookup_ignores_case_and_preserves_single_read() {
    let ct = types(
        r#"<c:Override PartName="/CUSTOM/MAIN.XML" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><c:Default Extension="png" ContentType="image/png"/>"#,
    );
    let bytes = zip(&[
        ("[CONTENT_TYPES].XML", &ct),
        ("_RELS/.RELS", &root()),
        ("CUSTOM/MAIN.XML", "body"),
        ("CUSTOM/_RELS/MAIN.XML.RELS", &rels(&rel("image", "image", "../media/image1.PNG", ""))),
        ("MEDIA/IMAGE1.png", "image"),
    ]);
    let mut package = DocxPackage::open(&bytes).unwrap();
    assert_eq!(package.main.name.as_ref(), "custom/main.xml");
    assert_eq!(package.relationships.len(), 1);
    let target = package.relationships[0].target.clone();
    assert!(
        matches!(&target, Target::Internal(part) if part.name.as_ref() == "media/image1.PNG" && part.content_type.as_deref() == Some("image/png"))
    );
    assert_eq!(package.read_test_target(&target, &RelationshipKind::Image).unwrap(), b"image");
    invalid(
        package.read_part("MEDIA/IMAGE1.PNG").unwrap_err(),
        "#part",
        "part already read (attempts observed 2, limit 1)",
    );
    let main = Target::Internal(package.main.clone());
    assert_eq!(
        package.read_test_target(&main, &RelationshipKind::OfficeDocument).unwrap(),
        b"body"
    );
}

// 이것을 실패시키는 것: Override/Default 등록의 소문자 변환 제거.
#[test]
fn case_only_content_type_duplicates_are_rejected() {
    for (children, element) in [
        (
            r#"<c:Default Extension="png" ContentType="image/png"/><c:Default Extension="PNG" ContentType="image/png"/>"#,
            "Default",
        ),
        (
            r#"<c:Override PartName="/word/styles.xml" ContentType="text/xml"/><c:Override PartName="/WORD/STYLES.XML" ContentType="text/xml"/>"#,
            "Override",
        ),
    ] {
        invalid(
            ContentTypes::parse(types(children).as_bytes(), &mut TextBudget::default())
                .err()
                .unwrap(),
            element,
            "duplicate content type definition (observed 2, limit 1)",
        );
    }
}

// 이것을 실패시키는 것: Core의 mimetype 예외에서 모호한 entry를 처음 항목으로 선택.
#[test]
fn ambiguous_case_aliases_cannot_be_selected() {
    let ct = types(
        r#"<c:Override PartName="/mimetype" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
    );
    let bytes = zip(&[
        ("[Content_Types].xml", &ct),
        ("_rels/.rels", &rels(&rel("main", "officeDocument", "mimetype", ""))),
        ("mimetype", "body"),
        ("MIMETYPE", "other"),
    ]);
    assert!(
        matches!(DocxPackage::open(&bytes).unwrap_err(), DocxError::AmbiguousPart { ref part } if part == "mimetype")
    );
    let bytes = document(
        &root(),
        &main_types(),
        Some(&rels(&rel("one", "custom", "../mimetype", ""))),
        &[("mimetype", "one"), ("MIMETYPE", "two")],
    );
    let mut package = DocxPackage::open(&bytes).unwrap();
    let relation = &package.relationships[0];
    assert!(
        matches!(package.read_test_target(&relation.target.clone(), &relation.kind.clone()).unwrap_err(), DocxError::AmbiguousPart { ref part } if part == "mimetype")
    );
    assert_eq!(package.consumed.len(), 3);
}

// 이것을 실패시키는 것: 관계 metadata 형식 검사를 생략하거나 main .rels 존재 검사를 대소문자 구분.
#[test]
fn relationship_metadata_types_are_required_when_present() {
    let bytes = document(
        &root(),
        &main_types().replace("Extension=\"rels\"", "Extension=\"other\""),
        None,
        &[],
    );
    assert!(
        matches!(DocxPackage::open(&bytes).unwrap_err(), DocxError::MissingContentType { ref part } if part == "_rels/.rels")
    );
    for (ct, part) in [
        (
            main_types()
                .replace("application/vnd.openxmlformats-package.relationships+xml", "text/plain"),
            "_rels/.rels",
        ),
        (
            types(
                r#"<c:Default Extension="xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><c:Override PartName="/custom/_rels/main.xml.rels" ContentType="text/plain"/>"#,
            ),
            "custom/_rels/main.xml.rels",
        ),
    ] {
        let bytes = document(&root(), &ct, Some(&rels("")), &[]);
        assert!(
            matches!(DocxPackage::open(&bytes).unwrap_err(), DocxError::ContentTypeMismatch { part: observed_part, ref actual, expected: "application/vnd.openxmlformats-package.relationships+xml" } if actual == "text/plain" && observed_part == part)
        );
    }
    let ct = types(r#"<c:Override PartName="/_rels/.rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><c:Default Extension="xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#).replace("Extension=\"rels\"", "Extension=\"other\"");
    let bytes = document(&root(), &ct, Some(&rels("")), &[]);
    assert!(
        matches!(DocxPackage::open(&bytes).unwrap_err(), DocxError::MissingContentType { ref part } if part == "custom/_rels/main.xml.rels")
    );
}

// 이것을 실패시키는 것: Other 대상 존재/형식을 open에서 검사하거나 읽기 선택에서 누락 검사 생략.
#[test]
fn unknown_targets_remain_lazy() {
    for extras in [
        vec![],
        vec![("custom/unknown.bin", "x")],
        vec![("custom/unknown.bin", "x"), ("custom/unknown.bin/", "")],
    ] {
        let bytes = document(
            &root(),
            &main_types(),
            Some(&rels(&rel("one", "custom", "unknown.bin", ""))),
            &extras,
        );
        let mut package = DocxPackage::open(&bytes).unwrap();
        let relationship = &package.relationships[0];
        assert!(
            matches!(&relationship.target, Target::Internal(part) if part.name.as_ref() == "custom/unknown.bin" && part.content_type.is_none())
        );
        let error = package
            .read_test_target(&relationship.target.clone(), &relationship.kind.clone())
            .unwrap_err();
        match extras.len() {
            0 => assert!(
                matches!(error, DocxError::MissingPart { ref part } if part == "custom/unknown.bin")
            ),
            1 => assert!(
                matches!(error, DocxError::MissingContentType { ref part } if part == "custom/unknown.bin")
            ),
            2 => assert!(
                matches!(error, DocxError::OpaquePart { ref part } if part == "custom/unknown.bin")
            ),
            _ => unreachable!(),
        }
        assert_eq!(package.consumed.len(), 3);
    }
}

// 이것을 실패시키는 것: MIME 대소문자 구분, 이미지 exact 비교, 매크로 검사 제거 또는 매개변수 소문자화.
#[test]
fn mime_case_and_parameters() {
    let ct = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content&#x2D;types\"><Default Extension=\"rels\" ContentType=\"Application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"Application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Default Extension=\"png\" ContentType=\"IMAGE/PNG\"/></Types>";
    let bytes = document(
        &root(),
        ct,
        Some(&rels(&rel("pic", "image", "a.png", ""))),
        &[("custom/a.png", "pixels")],
    );
    let mut package = DocxPackage::open(&bytes).unwrap();
    let target = package.relationships[0].target.clone();
    assert_eq!(package.read_test_target(&target, &RelationshipKind::Image).unwrap(), b"pixels");
    for mime in [
        "Application/vnd.ms-word.document.MACROENABLED.main+xml",
        "application/vnd.ms-word.document.macroenabled.main+xml",
    ] {
        let bytes = document(
            &root(),
            &types(&format!("<c:Default Extension='xml' ContentType='{mime}'/>")),
            None,
            &[],
        );
        invalid(
            DocxPackage::open(&bytes).unwrap_err(),
            "#content-type",
            "macroEnabled content type",
        );
    }
}

// 이것을 실패시키는 것: URI 기준 part를 진단 위치로 재사용.
#[test]
fn target_errors_locate_relationship_part() {
    for (root_xml, related, expected_part) in [
        (rels(&rel("main", "officeDocument", "../escape.xml", "")), None, "_rels/.rels"),
        (
            root(),
            Some(rels(&rel("bad", "image", "../../escape.xml", ""))),
            "custom/_rels/main.xml.rels",
        ),
    ] {
        let bytes = document(&root_xml, &main_types(), related.as_deref(), &[]);
        match DocxPackage::open(&bytes).unwrap_err() {
            DocxError::Invalid { part, element, detail } => {
                assert_eq!(part, expected_part);
                assert_eq!(element, "Relationship");
                assert!(detail.ends_with("path escapes package root"));
            }
            other => panic!("{other:?}"),
        }
    }
}

// 이것을 실패시키는 것: MIME 매개변수까지 case-fold 또는 macro 검사에 매개변수 포함.
#[test]
fn mime_parameters_remain_exact() {
    assert!(mime_matches("Application/X;name=Ab", MimeRule::Exact("application/x;name=Ab")));
    assert!(!mime_matches("application/x;name=ab", MimeRule::Exact("application/x;name=Ab")));
    assert!(!mime_matches("application/x;name=macroEnabled", MimeRule::Macro));
    assert!(!mime_matches("img", MimeRule::Image));
    assert_eq!(MimeRule::Macro.label(), "macroEnabled");
}

// 이것을 실패시키는 것: 진단 입력을 자르기 전에 복제하거나 위치/값/Target 인용 helper 누락.
#[test]
fn diagnostic_inputs_are_bounded() {
    let long = "가".repeat(513);
    let short = "가".repeat(512);
    assert_eq!(quote(&short), short);
    assert_eq!(quote(&long), format!("{short}…"));
    assert_eq!(
        quote(format_args!("{}{}", "가".repeat(511), "나다")),
        format!("{}나…", "가".repeat(511))
    );
    match DocxError::invalid(&long, &long, "detail") {
        DocxError::Invalid { part, element, detail } => {
            assert_eq!(part.chars().count(), 513);
            assert_eq!(element.chars().count(), 513);
            assert_eq!(detail, "detail");
        }
        other => panic!("{other:?}"),
    }
    let input = format!("<{long}>{}</{long}>", "x".repeat(1_048_577));
    match xml::walk(input.as_bytes(), &long, &mut TextBudget::default(), |_| Ok(())).unwrap_err() {
        DocxError::Limit { part, element, observed, limit, .. } => {
            assert_eq!(part, format!("{short}…"));
            assert_eq!(element, format!("{short}…"));
            assert_eq!(observed, 1_048_577);
            assert_eq!(limit, 1_048_576);
        }
        other => panic!("{other:?}"),
    }
    let target = format!("{}?", "a".repeat(1_048_000));
    match resolve_target("", "_rels/.rels", &target).unwrap_err() {
        DocxError::Invalid { detail, .. } => assert_eq!(
            detail,
            format!("target '{}…': invalid relative URI character", "a".repeat(512))
        ),
        other => panic!("{other:?}"),
    }
    for input in [format!("<a>&{long};</a>"), format!("<a v='&{long};'/>")] {
        match xml::walk(input.as_bytes(), "test.xml", &mut TextBudget::default(), |_| Ok(())).unwrap_err() {
            DocxError::Invalid { detail, .. } => assert_eq!(detail, format!("unsupported entity '&{short}…;'; only numeric and five predefined references are allowed")),
            other => panic!("{other:?}"),
        }
    }
}

// 이것을 실패시키는 것: 각 진단 생성 지점에서 입력 인용 제한 제거.
#[test]
fn diagnostic_fields_use_bounded_quotes() {
    let long = "x".repeat(513);
    let mut errors = Vec::new();
    let bytes = document(&root(), &main_types(), None, &[]);
    let package = DocxPackage::open(&bytes).unwrap();
    errors.push(package.index(&long).unwrap_err());
    errors.push(check_content_type(&long, None, None).unwrap_err());
    errors.push(
        check_content_type(&long, Some(&long), Some(MimeRule::Exact("application/x"))).unwrap_err(),
    );
    errors.push(DocxError::Package {
        part: "test.xml".into(),
        source: hwpforge_core::CoreError::InvalidStructure {
            context: "x".into(),
            reason: long.clone(),
        },
    });
    let external = rels(&rel("main", "officeDocument", &long, " TargetMode='External'"));
    errors.push(DocxPackage::open(&document(&external, &main_types(), None, &[])).unwrap_err());
    errors.push(
        parse_relationships(
            rels(&format!("{}{}", rel(&long, "image", "a", ""), rel(&long, "image", "b", "")))
                .as_bytes(),
            "test.xml",
            &mut TextBudget::default(),
        )
        .unwrap_err(),
    );
    errors.push(
        parse_relationships(
            rels(&rel("a", "image", "a", &format!(" TargetMode='{long}'"))).as_bytes(),
            "test.xml",
            &mut TextBudget::default(),
        )
        .unwrap_err(),
    );
    errors.push(
        ContentTypes::parse(
            types(&format!("<c:Override PartName='{long}' ContentType='x'/> ")).as_bytes(),
            &mut TextBudget::default(),
        )
        .err()
        .unwrap(),
    );
    for input in [
        format!("<a xmlns='{long}'/>"),
        format!("<a xmlns:p='x' xmlns:q='x' p:{long}='1' q:{long}='2'/>"),
        format!("<{long}:a/>"),
        format!("<?xml version='1.0' encoding='{long}'?><a/>"),
        format!("<{long}>{}</{long}>", "x".repeat(1_048_577)),
    ] {
        errors.push(
            metadata(input.as_bytes(), &long, TYPE_NS, "a", &mut TextBudget::default(), |_| Ok(()))
                .unwrap_err(),
        );
    }
    for error in errors {
        let message = error.to_string();
        assert!(!message.contains(&long), "{message}");
        assert!(message.contains('…'), "{message}");
    }
    for (source, location) in
        [("", "_rels/.rels"), ("custom/main.xml", "custom/_rels/main.xml.rels")]
    {
        match resolve_target(source, location, "a//b").unwrap_err() {
            DocxError::Package { part, .. } => assert_eq!(part, location),
            other => panic!("{other:?}"),
        }
    }
    let macro_type = format!("application/macroEnabled{long}");
    let error = validate_content_type(
        &Part::new(0, long.clone().into(), Some(macro_type.clone().into())),
        &RelationshipKind::OfficeDocument,
    )
    .unwrap_err();
    assert!(!error.to_string().contains(&macro_type));
    let strict = format!("http://purl.oclc.org/ooxml/{long}");
    assert!(!xml::reject_strict("x", "a", &strict).unwrap_err().to_string().contains(&strict));
}

// 이것을 실패시키는 것: main MIME 매개변수 무시 또는 앞뒤 공백 trim으로 지원하지 않는 형식 수용.
#[test]
fn main_content_type_parameters_and_whitespace_are_unsupported() {
    for mime in [
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml; charset=UTF-8",
        " application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml ",
    ] {
        let ct = types(&format!("<c:Default Extension='xml' ContentType='{mime}'/>"));
        let bytes = document(&root(), &ct, None, &[]);
        match DocxPackage::open(&bytes).unwrap_err() {
            DocxError::ContentTypeMismatch { part, actual, expected } => {
                assert_eq!(part, "custom/main.xml");
                assert_eq!(actual, mime);
                assert_eq!(expected, "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml");
            }
            other => panic!("{other:?}"),
        }
    }
}

// 이것을 실패시키는 것: Default/Override 정의를 관계마다 String으로 복제하거나 공유 조회를 복제로 대체.
#[test]
fn content_type_definitions_are_shared_by_relationships() {
    for definition in [
        format!("<c:Default Extension='bin' ContentType='{}'/>", "x".repeat(4_096)),
        format!("<c:Override PartName='/custom/a.bin' ContentType='{}'/>", "x".repeat(4_096)),
    ] {
        let ct = main_types().replace("</c:Types>", &format!("{definition}</c:Types>"));
        let parsed = ContentTypes::parse(ct.as_bytes(), &mut TextBudget::default()).unwrap();
        let first = parsed.shared("custom/a.bin").unwrap();
        assert!(Rc::ptr_eq(first, parsed.shared("CUSTOM/A.BIN").unwrap()));
        let related =
            rels(&(0..128).map(|i| rel(&i.to_string(), "custom", "a.bin", "")).collect::<String>());
        let bytes = document(&root(), &ct, Some(&related), &[]);
        let package = DocxPackage::open(&bytes).unwrap();
        let Target::Internal(first) = &package.relationships[0].target else { panic!() };
        let mut allocations = std::collections::HashSet::new();
        let mut retained = 0;
        for relationship in &package.relationships {
            let Target::Internal(part) = &relationship.target else { panic!() };
            let mime = part.content_type.as_ref().unwrap();
            assert!(Rc::ptr_eq(first.content_type.as_ref().unwrap(), mime));
            if allocations.insert(mime.as_ptr()) {
                retained += mime.len();
            }
        }
        assert_eq!(package.relationships.len(), 128);
        assert_eq!(retained, 4_096);
    }
}

// 이것을 실패시키는 것: 관계별 경로 복제, raw 캐시 우회, canonical 별칭 공유 제거,
// source별 캐시 혼합, 해시 충돌을 동일 경로로 취급, 경로 예산을 관계별로 초기화.
// 긴 source를 hit마다 해시하거나 content type 조회에서 전체 경로를 다시 소문자 복사.
#[test]
fn resolved_paths_share_repetitions_and_canonical_aliases() {
    let directory = "a".repeat(4_096);
    let source = format!("{directory}/main.xml");
    let mut paths = ResolvedPaths::default();
    let types = ContentTypes::default();
    let mut targets = HashMap::new();
    let first = paths.resolve(&source, "test.rels", "a.bin", &mut targets, &types).unwrap();
    for _ in 0..128 {
        for target in ["a.bin", "A.BIN", "./a.bin", "sub/../a.bin"] {
            let part = paths.resolve(&source, "test.rels", target, &mut targets, &types).unwrap();
            assert!(Rc::ptr_eq(&first.name, &part.name));
        }
    }
    assert_eq!(paths.resolutions, 4);
    assert_eq!(paths.allocated_bytes, 4_102);
    assert_eq!(targets.len(), 4);
    assert_eq!(types.lowercase_bytes.get(), 4_105);
    assert_eq!(paths.hash_bytes, 20_129);
    assert_eq!(paths.bytes, 65_796);
    assert_eq!(paths.canonical.values().map(Vec::len).sum::<usize>(), 1);
    // A hash collision must still compare every component and the component count.
    for name in ["wrong/part".to_string(), format!("{directory}/a.bin/extra"), directory.clone()] {
        paths
            .canonical
            .values_mut()
            .next()
            .unwrap()
            .insert(0, Part::new(usize::MAX, name.into(), None));
    }
    let absolute = format!("/{directory}/A.bin");
    let mut other_targets = HashMap::new();
    let same = paths
        .resolve("other/main.xml", "test.rels", &absolute, &mut other_targets, &types)
        .unwrap();
    assert!(Rc::ptr_eq(&first.name, &same.name));
    let other =
        paths.resolve("other/main.xml", "test.rels", "a.bin", &mut other_targets, &types).unwrap();
    assert_eq!(other.name.as_ref(), "other/a.bin");
    assert!(!Rc::ptr_eq(&first.name, &other.name));
    assert_eq!(paths.resolutions, 6);

    let related = rels(
        &(0..128)
            .map(|i| rel(&i.to_string(), "custom", if i % 2 == 0 { "a.bin" } else { "A.BIN" }, ""))
            .collect::<String>(),
    );
    let root = rels(&rel("main", "officeDocument", &source, ""));
    let relpath = relationship_part_name(&source);
    let bytes = zip(&[
        (CONTENT_TYPES, &main_types()),
        (ROOT_RELS, &root),
        (&source, "<a/>"),
        (&relpath, &related),
        (&format!("{directory}/a.bin"), "x"),
    ]);
    let package = DocxPackage::open(&bytes).unwrap();
    let Target::Internal(first) = &package.relationships[0].target else { panic!() };
    let mut allocations = std::collections::HashSet::new();
    let mut retained = 0;
    for relationship in &package.relationships {
        let Target::Internal(part) = &relationship.target else { panic!() };
        assert!(Rc::ptr_eq(&first.name, &part.name));
        if allocations.insert(part.name.as_ptr()) {
            retained += part.name.len();
        }
    }
    assert_eq!(retained, 4_102);
}

// 이것을 실패시키는 것: 서로 다른 짧은 Target의 누적 예산 생략/초기화,
// 할당 뒤 admission, separator 바이트 누락, 경계값 거부 또는 위치/관측값/상한 변경.
#[test]
fn distinct_short_targets_have_a_preallocation_path_budget() {
    for (length, count, expected) in
        [(1_024, 64, 266_240), (2_048, 64, 528_384), (1_024, 128, 532_480)]
    {
        let source = format!("{}/main.xml", "a".repeat(length));
        let mut paths = ResolvedPaths::default();
        let mut targets = HashMap::new();
        for i in 0..count {
            paths
                .resolve(
                    &source,
                    "body.rels",
                    &format!("{i:03}.bin"),
                    &mut targets,
                    &ContentTypes::default(),
                )
                .unwrap();
        }
        assert_eq!(paths.bytes, expected);
        assert_eq!(paths.allocated_bytes, (length + 8) * count);
    }
    let mut paths = ResolvedPaths { bytes: 67_108_792, ..ResolvedPaths::default() };
    let mut targets = HashMap::new();
    let types = ContentTypes::default();
    let last = paths.resolve("dir/main.xml", "body.rels", "aa.bin", &mut targets, &types).unwrap();
    assert_eq!(last.name.as_ref(), "dir/aa.bin");
    assert_eq!(paths.bytes, 67_108_864);
    // Cache hits still work with an exhausted processing budget.
    paths.resolve("dir/main.xml", "body.rels", "aa.bin", &mut targets, &types).unwrap();
    for target in ["bb.bin", "AA.bin"] {
        match paths.resolve("dir/main.xml", "body.rels", target, &mut targets, &types).unwrap_err()
        {
            DocxError::Limit { part, element, resource, observed, limit } => {
                assert_eq!(part, "body.rels");
                assert_eq!(element, "Relationship");
                assert_eq!(resource, "resolved path processing bytes");
                assert_eq!(observed, 67_108_936);
                assert_eq!(limit, 67_108_864);
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(paths.bytes, 67_108_864);
    assert_eq!(targets.len(), 1);
    assert_eq!(paths.allocated_bytes, 10);
}

// 이것을 실패시키는 것: 빈 xmlns: prefix를 기본 namespace로 등록.
#[test]
fn empty_namespace_prefix_cannot_select_the_main_part() {
    let root = "<Relationships xmlns:='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='r' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument' Target='custom/main.xml'/></Relationships>";
    let bytes = document(root, &main_types(), None, &[]);
    match DocxPackage::open(&bytes).unwrap_err() {
        DocxError::Invalid { part, element, detail } => {
            assert_eq!(part, "_rels/.rels");
            assert_eq!(element, "Relationships");
            assert_eq!(detail, "invalid QName 'xmlns:'");
        }
        other => panic!("{other:?}"),
    }
}

// 이것을 실패시키는 것: 실패 결과 미보관, 관계 종류별 MIME 캐시 혼합,
// raw/canonical alias마다 index 셀 새 생성, 매 read에서 선택 재검사.
#[test]
fn canonical_selection_caches_success_and_every_failure() {
    for (extra, mime, expected) in [
        (
            vec![("custom/a.xml", "<a/>")],
            "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
            Ok(Some(4)),
        ),
        (vec![], "application/x", Err(SelectionFailure::Missing)),
        (
            vec![("custom/a.xml", "<a/>"), ("custom/a.xml/", "")],
            "application/x",
            Err(SelectionFailure::Opaque),
        ),
        (vec![("custom/a.xml", "<a/>")], "", Err(SelectionFailure::MissingType)),
        (
            vec![("custom/a.xml", "<a/>")],
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            Err(SelectionFailure::Mismatch(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
            )),
        ),
    ] {
        let ct = types(&format!(
            "<c:Override PartName='/custom/main.xml' ContentType='{MAIN_TYPE}'/>{}",
            if mime.is_empty() {
                String::new()
            } else {
                format!("<c:Override PartName='/custom/a.xml' ContentType='{mime}'/>")
            }
        ));
        let related = rels(&format!(
            "{}{}",
            rel("one", "header", "a.xml", ""),
            rel("two", "header", "./A.xml", "")
        ));
        let bytes = document(&root(), &ct, Some(&related), &extra);
        let package = DocxPackage::open(&bytes).unwrap();
        let Target::Internal(first) = &package.relationships[0].target else { panic!() };
        let Target::Internal(alias) = &package.relationships[1].target else { panic!() };
        assert_eq!(first.id, alias.id);
        assert!(Rc::ptr_eq(&first.cache, &alias.cache));
        take_selection_work();
        assert_eq!(package.relationship_index(&package.relationships[0]), expected);
        let initial = take_selection_work();
        assert_eq!(initial.lookups, 1);
        for _ in 0..64 {
            assert_eq!(package.relationship_index(&package.relationships[1]), expected);
        }
        assert_eq!(
            take_selection_work(),
            SelectionWork {
                entry_bytes: 0,
                hash_bytes: 0,
                lookups: 0,
                path_bytes: 0,
                lookup_bytes: 0,
                type_checks: 0,
                mime_calls: 0,
                mime_bytes: 0
            }
        );
        if expected.is_ok() {
            let other = Relationship {
                id: "footer".into(),
                kind: RelationshipKind::Footer,
                target: package.relationships[0].target.clone(),
            };
            assert_eq!(
                package.relationship_index(&other),
                Err(SelectionFailure::Mismatch(
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"
                ))
            );
            let work = take_selection_work();
            assert_eq!(work.lookups, 0);
            assert_eq!(work.mime_calls, 1);
            assert_eq!(
                package.relationship_index(&other),
                Err(SelectionFailure::Mismatch(
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"
                ))
            );
            assert_eq!(
                take_selection_work(),
                SelectionWork {
                    entry_bytes: 0,
                    hash_bytes: 0,
                    lookups: 0,
                    path_bytes: 0,
                    lookup_bytes: 0,
                    type_checks: 0,
                    mime_calls: 0,
                    mime_bytes: 0
                }
            );
        }
    }
    let bytes = document(
        &root(),
        &main_types(),
        Some(&rels(&rel("one", "header", "../mimetype", ""))),
        &[("mimetype", "one"), ("MIMETYPE", "two")],
    );
    let package = DocxPackage::open(&bytes).unwrap();
    take_selection_work();
    assert_eq!(
        package.relationship_index(&package.relationships[0]),
        Err(SelectionFailure::Ambiguous)
    );
    assert_eq!(take_selection_work().lookups, 1);
    for _ in 0..64 {
        assert_eq!(
            package.relationship_index(&package.relationships[0]),
            Err(SelectionFailure::Ambiguous)
        );
    }
    assert_eq!(
        take_selection_work(),
        SelectionWork {
            entry_bytes: 0,
            hash_bytes: 0,
            lookups: 0,
            path_bytes: 0,
            lookup_bytes: 0,
            type_checks: 0,
            mime_calls: 0,
            mime_bytes: 0
        }
    );
}

// 이것을 실패시키는 것: ZIP 이름/질의 소문자화 제거, 충돌에서 첫/마지막 index 선택,
// 해시 충돌의 문자열 확인 생략, 서로 다른 이름을 모호한 이름으로 합침.
#[test]
fn name_index_preserves_case_ambiguity_and_verifies_hash_collisions() {
    for extra in [
        vec![("mimetype", "a"), ("MIMETYPE", "b"), ("MimeType", "c")],
        vec![("MimeType", "c"), ("MIMETYPE", "b"), ("mimetype", "a")],
    ] {
        let bytes = document(&root(), &main_types(), None, &extra);
        let package = DocxPackage::open(&bytes).unwrap();
        assert_eq!(package.names.get("MiMeTyPe"), Err(SelectionFailure::Ambiguous));
        assert_eq!(package.names.get("CUSTOM/MAIN.XML"), Ok(Some(2)));
    }
    let bytes = document(&root(), &main_types(), None, &[("UPPER.XML", "x")]);
    let mut package = DocxPackage::open(&bytes).unwrap();
    assert_eq!(package.names.get("upper.xml"), Ok(Some(3)));
    // Force two different names into a full-hash bucket; no probabilistic input.
    let hash = package.names.hash("upper.xml");
    package.names.buckets.get_mut(&hash).unwrap().insert(0, ("other.xml".into(), None));
    assert_eq!(package.names.get("UPPER.XML"), Ok(Some(3)));
    let hash = package.names.hash("absent.xml");
    package.names.buckets.insert(hash, vec![("another.xml".into(), Some(3))]);
    assert_eq!(package.names.get("absent.xml"), Ok(None));
}

// 이것을 실패시키는 것: 관계 ID/확장자 키의 길이 검사 누락 또는 경계 4096 거부.
#[test]
fn metadata_identifier_boundaries_are_checked_before_keys() {
    for length in [4095, 4096, 4097] {
        let key = "a".repeat(length);
        let rels = format!("<Relationships xmlns='{}'><Relationship Id='{key}' Type='urn:other' Target='unused'/></Relationships>", REL_NS.1);
        let types = format!(
            "<Types xmlns='{}'><Default Extension='{key}' ContentType='application/xml'/></Types>",
            TYPE_NS.1
        );
        for result in [
            parse_relationships(rels.as_bytes(), "rels", &mut TextBudget::default()).map(|_| ()),
            ContentTypes::parse(types.as_bytes(), &mut TextBudget::default()).map(|_| ()),
        ] {
            if length <= 4096 {
                assert!(result.is_ok());
            } else {
                assert!(matches!(
                    result,
                    Err(DocxError::Limit {
                        resource: "identifier bytes",
                        observed: 4097,
                        limit: 4096,
                        ..
                    })
                ));
            }
        }
    }
}
