use super::*;

fn values(input: &str) -> Result<Vec<String>> {
    let mut result = Vec::new();
    walk(input.as_bytes(), "test.xml", &mut TextBudget::default(), |event| {
        if let XmlEvent::Text(value) = event {
            result.push(value);
        }
        Ok(())
    })?;
    Ok(result)
}

// 이것을 실패시키는 것: GeneralRef/CDATA 누락 또는 comment/PI에서 값 경계 끊기.
#[test]
fn references_and_cdata_form_one_value() {
    assert_eq!(
        values("<a>A&amp;&#xAC00;<![CDATA[B]]><!--x--><?x y?>&lt;&gt;&apos;&quot;&#65;</a>")
            .unwrap(),
        ["A&가B<>'\"A"]
    );
}

// 이것을 실패시키는 것: XML 오류의 요소/사유를 일반 파서 오류로 대체.
#[test]
fn forbidden_xml_and_malformed_structure() {
    let cases = [
        "<!DOCTYPE a><a/>",
        "<a>&custom;</a>",
        "<a v='&custom;'/>",
        "<a>",
        "<a></b>",
        "<a/><b/>",
        "text<a/>",
        "<a/>text",
        "",
        "<p:a/>",
        "<a p:v='x'/>",
        "<a v='1' v='2'/>",
        "<a>&#0;</a>",
        "<a>&#xGG;</a>",
        "<a><!--bad--comment--></a>",
        "<a xmlns='http://purl.oclc.org/ooxml/wordprocessingml/main'/>",
        "<a xmlns:s='http://purl.oclc.org/ooxml/wordprocessingml/main'/>",
    ];
    let expected = [
        ("#document", "DTD is forbidden (observed 1, limit 0)"),
        ("a", "unsupported entity '&custom;'"),
        ("a", "unsupported entity '&custom;'"),
        ("a", "missing or unclosed document element"),
        ("a", "expected `</a>`, but `</b>` was found"),
        ("b", "document elements observed 2, limit 1"),
        ("#document", "text outside document element"),
        ("#document", "text outside document element"),
        ("#document", "missing or unclosed document element"),
        ("a", "unbound namespace prefix 'p'"),
        ("a", "unbound namespace prefix 'p'"),
        ("a", "duplicated attribute, previous declaration at position 2"),
        ("a", "0x0 character is not permitted in XML"),
        ("a", "invalid character reference: invalid digit found in string"),
        ("a", "forbidden string `--` was found in a comment"),
        ("a", "Strict OOXML URI 'http://purl.oclc.org/ooxml/wordprocessingml/main'"),
        ("a", "Strict OOXML URI 'http://purl.oclc.org/ooxml/wordprocessingml/main'"),
    ];
    assert_eq!(cases.len(), expected.len());
    for (input, (element, reason)) in cases.into_iter().zip(expected) {
        match values(input).unwrap_err() {
            DocxError::Invalid { part, element: actual, detail } => {
                assert_eq!(part, "test.xml");
                assert_eq!(actual, element);
                assert!(detail.contains(reason), "{input}: {detail}");
            }
            other => panic!("{input}: {other:?}"),
        }
    }
}

// 이것을 실패시키는 것: 깊이 비교 삭제/경계 변경 또는 빈 요소 깊이 미계산.
#[test]
fn depth_boundary() {
    for depth in [127, 128] {
        assert!(values(&format!("{}{}", "<a>".repeat(depth), "</a>".repeat(depth))).is_ok());
    }
    let error = values(&format!("{}<b/>{}", "<a>".repeat(128), "</a>".repeat(128))).unwrap_err();
    assert!(
        matches!(error, DocxError::Limit { observed: 129, limit: 128, ref element, .. } if element == "b")
    );
}

// 이것을 실패시키는 것: 속성 개수 검사 삭제/경계 변경.
#[test]
fn attribute_count_boundary() {
    for count in [255, 256, 257] {
        let attrs: String = (0..count).map(|n| format!(" a{n}='v'")).collect();
        let result = values(&format!("<a{attrs}/>"));
        if count <= 256 {
            assert!(result.is_ok());
        } else {
            assert!(matches!(
                result.unwrap_err(),
                DocxError::Limit { observed: 257, limit: 256, .. }
            ));
        }
    }
}

// 이것을 실패시키는 것: 단일 값 검사 삭제, event마다 초기화, 문자 수로 계산.
#[test]
fn value_byte_boundaries() {
    for length in [1_048_575, 1_048_576, 1_048_577] {
        let value = "x".repeat(length);
        for input in [
            format!("<a v='{value}'/>"),
            format!("<a>{value}</a>"),
            format!("<a><![CDATA[{value}]]></a>"),
        ] {
            let result = values(&input);
            if length <= 1_048_576 {
                assert!(result.is_ok());
            } else {
                assert!(matches!(
                    result.unwrap_err(),
                    DocxError::Limit { observed: 1_048_577, limit: 1_048_576, .. }
                ));
            }
        }
    }
    let error = values(&format!("<a>{}&#xAC00;</a>", "x".repeat(1_048_574))).unwrap_err();
    assert!(matches!(error, DocxError::Limit { observed: 1_048_577, limit: 1_048_576, .. }));
}

// 이것을 실패시키는 것: 누적 한도 오류를 다른 자원 이름이나 일반 오류로 대체.
#[test]
fn text_budget_boundaries_and_shared_parts() {
    let mut budget = TextBudget::default();
    budget.add("a.xml", "t", 67_108_863).unwrap();
    budget.add("a.xml", "t", 1).unwrap();
    let error = budget.add("b.xml", "t", 1).unwrap_err();
    assert!(
        matches!(error, DocxError::Limit { observed: 67_108_865, limit: 67_108_864, ref part, .. } if part == "b.xml")
    );
    assert!(matches!(
        budget.add("b.xml", "t", usize::MAX).unwrap_err(),
        DocxError::Limit { limit: 67_108_864, resource: "expanded UTF-8 text bytes", .. }
    ));
    let mut budget = TextBudget::default();
    budget.add("first.xml", "t", 67_108_863).unwrap();
    let error =
        walk(b"<a>&amp;<![CDATA[B]]></a>", "second.xml", &mut budget, |_| Ok(())).unwrap_err();
    assert!(matches!(error, DocxError::Limit { observed: 67_108_865, limit: 67_108_864, .. }));
}

// 이것을 실패시키는 것: callback 오류를 일반 파서 오류로 대체.
#[test]
fn namespace_attributes_and_callback_errors() {
    let mut starts = Vec::new();
    walk(
        b"<x:a xmlns:x='urn:test' x:v='B' v='A&amp;&#65;'/>",
        "test.xml",
        &mut TextBudget::default(),
        |event| {
            if let XmlEvent::Start(element) = event {
                starts.push(element);
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(starts[0].namespace.as_ref(), "urn:test");
    assert_eq!(starts[0].name, "a");
    assert_eq!(starts[0].attribute("v"), Some("A&A"));
    let error = walk(b"<a/>", "test.xml", &mut TextBudget::default(), |_| {
        Err(DocxError::invalid("callback", "a", "failure"))
    })
    .unwrap_err();
    assert!(
        matches!(error, DocxError::Invalid { ref part, ref element, ref detail } if part == "callback" && element == "a" && detail == "failure")
    );
}

// 이것을 실패시키는 것: 문서 밖 공백을 visitor에 전달.
#[test]
fn namespace_aliases_and_whitespace_boundaries() {
    let error = values("<a xmlns:x='urn:same' xmlns:y='urn:same' x:v='1' y:v='2'/>").unwrap_err();
    assert!(error.to_string().contains("duplicate expanded attribute"));
    assert!(values("<a xmlns:x='urn:one' xmlns:y='urn:two' x:v='1' y:v='2'/>").is_ok());
    assert_eq!(values(" \n<a>first<b/>second</a>\t").unwrap(), ["first", "second"]);
    assert!(values("<a xmlns:s='http://purl.oclc.org/ooxml/wordprocessingml/main' s:v='1'/>")
        .unwrap_err()
        .to_string()
        .contains("Strict"));
    // An ordinary attribute containing this string is not a namespace declaration.
    assert!(values("<a v='http://purl.oclc.org/ooxml/wordprocessingml/main'/>").is_ok());
}

// 이것을 실패시키는 것: 금지 XML 문자 오류의 요소/사유를 일반 오류로 대체.
#[test]
fn illegal_xml_characters_are_rejected() {
    for input in ["<a>&#1;</a>", "<a v='&#1;'/>", "<a><![CDATA[\u{1}]]></a>", "<a>\u{1}</a>"] {
        assert!(
            matches!(values(input).unwrap_err(), DocxError::Invalid { ref element, ref detail, .. } if element == "a" && detail == "invalid XML 1.0 character U+0001"),
            "{input}"
        );
    }
    assert_eq!(
        values("<a>&#9;&#10;&#13;&#x20;&#xD7FF;&#xE000;&#xFFFD;&#x10000;&#x10FFFF;</a>").unwrap(),
        ["\t\n\r \u{d7ff}\u{e000}\u{fffd}\u{10000}\u{10ffff}"]
    );
}

// 이것을 실패시키는 것: 문서 밖 공백을 visitor나 텍스트 예산에 포함.
#[test]
fn outer_whitespace_is_not_text() {
    let mut budget = TextBudget::default();
    budget.add("previous.xml", "t", 67_108_864).unwrap();
    let mut text = Vec::new();
    walk(b" \n<a/>\t\r\n", "test.xml", &mut budget, |event| {
        if let XmlEvent::Text(value) = event {
            text.push(value);
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(text, Vec::<String>::new());

    assert_eq!(budget.bytes, 67_108_864);
    assert_eq!(
        values(&format!("{}<a> </a>{}", " ".repeat(1_048_577), "\n".repeat(1_048_577))).unwrap(),
        [" "]
    );
    assert!(
        matches!(values("\u{a0}<a/>").unwrap_err(), DocxError::Invalid { ref element, ref detail, .. } if element == "#document" && detail == "text outside document element")
    );
}

// 이것을 실패시키는 것: UTF-16 BOM 검사/encoding 선언 거부 제거 또는 UTF-8 대소문자 구분.
#[test]
fn encoding_policy_is_explicit() {
    for input in [&b"\xFF\xFE<\x00a\x00/\x00>\x00"[..], &b"\xFE\xFF\x00<\x00a\x00/\x00>"[..]] {
        assert!(
            matches!(walk(input, "test.xml", &mut TextBudget::default(), |_| Ok(())).unwrap_err(), DocxError::UnsupportedEncoding { ref part, ref encoding } if part == "test.xml" && encoding == "UTF-16")
        );
    }
    for encoding in ["UTF-16", "utf-16", "UTF-16LE", "UTF-16BE", "ISO-8859-1", "windows-1252"] {
        let input = format!("<?xml version='1.0' encoding='{encoding}'?><a/>");
        assert!(
            matches!(values(&input).unwrap_err(), DocxError::UnsupportedEncoding { encoding: observed, .. } if observed == encoding)
        );
    }
    for input in [
        "<?xml version='1.0' encoding='utf-8'?><a>A</a>",
        "<?xml version='1.0' encoding='UTF-8'?><a>A</a>",
        "<?xml version='1.0'?><a>A</a>",
        "\u{feff}<a>A</a>",
    ] {
        assert_eq!(values(input).unwrap(), ["A"]);
    }
}

// 이것을 실패시키는 것: 정규화 전 namespace 등록 또는 local name 중복 검사 생략.
#[test]
fn normalized_namespace_aliases_collide() {
    match values("<a xmlns:p=\"urn:x\" xmlns:q=\"urn:&#x78;\" p:v=\"1\" q:v=\"2\"/>").unwrap_err() {
        DocxError::Invalid { part, element, detail } => {
            assert_eq!(part, "test.xml");
            assert_eq!(element, "a");
            assert_eq!(detail, "duplicate expanded attribute 'v'");
        }
        other => panic!("{other:?}"),
    }
}

// 이것을 실패시키는 것: 문서 상태별 참조/CDATA/선언 위치 검사 제거.
#[test]
fn document_event_grammar() {
    for (input, element, reason) in [
        ("&#32;<a/>", "#document", "reference or CDATA outside document element"),
        ("<![CDATA[ ]]><a/>", "#document", "reference or CDATA outside document element"),
        ("<a/>&#32;", "#document", "reference or CDATA outside document element"),
        ("<a/><![CDATA[ ]]>", "#document", "reference or CDATA outside document element"),
        ("<a><?xml version=\"1.0\"?></a>", "a", "XML declaration must be first"),
        (" <?xml version='1.0'?><a/>", "#document", "XML declaration must be first"),
        ("<!--x--><?xml version='1.0'?><a/>", "#document", "XML declaration must be first"),
        ("<?x y?><?xml version='1.0'?><a/>", "#document", "XML declaration must be first"),
        (
            "<?xml version='1.0'?><?xml version='1.0'?><a/>",
            "#document",
            "XML declaration must be first",
        ),
        ("<a/><?xml version='1.0'?>", "#document", "XML declaration must be first"),
    ] {
        match values(input).unwrap_err() {
            DocxError::Invalid { element: actual, detail, .. } => {
                assert_eq!(actual, element);
                assert_eq!(detail, reason);
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(values("<?xml version='1.0'?><!--x--><?x y?><a>A</a><!--y--><?z?>").unwrap(), ["A"]);
}

// 이것을 실패시키는 것: 원문 길이로 거부, 속성 합계 누락, 정규화 길이/CRLF/참조 계산 변경 또는 CDATA의 entity 해석.
#[test]
fn borrowed_admission_precedes_normalization() {
    assert_eq!(values("<a><![CDATA[&amp;]]></a>").unwrap(), ["&amp;"]);
    assert_eq!(admit("가", true, 0, "test.xml", "a", &mut TextBudget::default()).unwrap(), 3);
    assert!(matches!(
        admit("가", false, 1_048_574, "test.xml", "a", &mut TextBudget::default()).unwrap_err(),
        DocxError::Limit { observed: 1_048_577, limit: 1_048_576, .. }
    ));
    for (raw, attribute, expected) in [
        ("&#x41;".repeat(1_048_576), true, 1_048_576),
        ("\r\n".repeat(1_048_576), false, 1_048_576),
        ("\r\n".repeat(1_048_576), true, 1_048_576),
    ] {
        let mut budget = TextBudget::default();
        assert_eq!(admit(&raw, attribute, 0, "test.xml", "a", &mut budget).unwrap(), expected);
        assert_eq!(budget.bytes, 1_048_576);
        let value = normalized(&raw, attribute, expected, "test.xml", "a").unwrap();
        assert_eq!(value.len(), 1_048_576);
        assert_eq!(value.capacity(), 1_048_576);
    }
    for (raw, attribute) in [
        (format!("{}&amp;", "x".repeat(20 * 1024 * 1024)), true),
        (format!("{}\r", "x".repeat(20 * 1024 * 1024)), false),
    ] {
        let mut budget = TextBudget::default();
        // Admission takes a slice and returns only a scalar; no value buffer exists here.
        assert!(matches!(
            admit(&raw, attribute, 0, "test.xml", "a", &mut budget).unwrap_err(),
            DocxError::Limit { observed: 20_971_521, limit: 1_048_576, .. }
        ));
        assert_eq!(budget.bytes, 0);
    }
    for input in
        ["<a v='A'/>", "<a>A</a>", "<a><![CDATA[A]]></a>", "<a>&#65;</a>", "<a xmlns='A'/>"]
    {
        let mut budget = TextBudget::default();
        budget.add("previous", "a", 67_108_864).unwrap();
        assert!(matches!(
            walk(input.as_bytes(), "test.xml", &mut budget, |_| Ok(())).unwrap_err(),
            DocxError::Limit { observed: 67_108_865, limit: 67_108_864, .. }
        ));
    }
    for input in [
        format!("<a v='{}'/>", "&#65;".repeat(1_048_576)),
        format!("<a>{}</a>", "\r\n".repeat(1_048_576)),
        format!("<a><![CDATA[{}]]></a>", "\r\n".repeat(1_048_576)),
    ] {
        values(&input).unwrap();
    }
    let mut attributes = Vec::new();
    walk(
        b"<a v='\t\n\r\n&#9;&#10;&#13;&amp;&lt;&gt;&apos;&quot;'/>",
        "test.xml",
        &mut TextBudget::default(),
        |event| {
            if let XmlEvent::Start(element) = event {
                attributes.push(element.attribute("v").unwrap().to_owned());
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(attributes, ["   \t\n\r&<>'\""]);
    assert_eq!(values("<a>\r\n\r\t</a>").unwrap(), ["\n\n\t"]);
    match values("<a v='&missing'/>").unwrap_err() {
        DocxError::Invalid { detail, .. } => assert_eq!(detail, "unterminated entity reference"),
        other => panic!("{other:?}"),
    }
}

// 이것을 실패시키는 것: 선언 수/값 admission을 namespace 등록 뒤로 이동.
#[test]
fn namespace_admission_before_registration() {
    // An early invalid binding must not mask a later count/length rejection.
    let declarations: String = (0..256).map(|i| format!(" xmlns:p{i}='x'")).collect();
    for (input, observed, limit) in [
        (format!("<a xmlns:xml='invalid'{declarations}/>"), 257, 256),
        (
            format!("<a xmlns:xml='invalid' xmlns:p='{}'/>", "x".repeat(1_048_577)),
            1_048_577,
            1_048_576,
        ),
    ] {
        assert!(
            matches!(values(&input).unwrap_err(), DocxError::Limit { observed: actual, limit: maximum, .. } if actual == observed && maximum == limit)
        );
    }
}

// 이것을 실패시키는 것: ID 중복 검사를 URI 내용 비교로 되돌리거나 조회마다 URI를 다시 해시.
#[test]
fn long_namespace_is_shared_with_linear_comparisons() {
    let uri = "u".repeat(1_048_576);
    let attrs: String = (0..253).map(|i| format!(" p:v{i}='x'")).collect();
    let relationship = format!("<Relationship Id='r' Type='image' Target='a'{attrs}/>");
    let input =
        format!("<Relationships xmlns:p='{uri}'>{}</Relationships>", relationship.repeat(1_000));
    let mut budget = TextBudget::default();
    let mut count = 0;
    walk(input.as_bytes(), "test.xml", &mut budget, |event| {
        if let XmlEvent::Start(element) = event {
            for (namespace, _, _) in element.attributes {
                if namespace != NamespaceId::NONE {
                    assert_eq!(namespace, NamespaceId(9));
                    count += 1;
                }
            }
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(count, 253_000);
    assert_eq!(budget.namespace_hash_bytes, 1_048_576);
    assert_eq!(budget.namespace_comparison_bytes, 0);
}

// 이것을 실패시키는 것: 같은 local name의 ID를 URI 내용으로 비교, URI 전체 표 선형 검색,
// 등록 때 기존 URI 재해시, 또는 확장 이름 key에서 namespace ID 제거.
#[test]
fn long_common_prefix_namespaces_use_ids() {
    // Both declaration count and URI length double independently; only the final digits differ.
    for (length, prefixes, repetitions, hashed, compared, attributes) in [
        (65_536, 20, 8, 2_621_440, 1_310_720, 320),
        (16_384, 64, 16, 2_097_152, 1_048_576, 2_048),
        (32_768, 64, 16, 4_194_304, 2_097_152, 2_048),
        (16_384, 128, 16, 4_194_304, 2_097_152, 4_096),
    ] {
        let common = "u".repeat(length - 3);
        let declarations: String =
            (0..prefixes).map(|i| format!(" xmlns:p{i}='{common}{i:03}'")).collect();
        let attrs: String = (0..prefixes).map(|i| format!(" p{i}:v=''")).collect();
        let relationship = format!("<Relationship Id='r' Type='urn:custom' Target='a'{attrs}/>");
        let group = format!(
            "<Relationships{declarations}>{}</Relationships>",
            relationship.repeat(repetitions)
        );
        let input = format!("<root>{group}{group}</root>");
        let mut budget = TextBudget::default();
        let mut count = 0;
        walk(input.as_bytes(), "test.xml", &mut budget, |event| {
            if let XmlEvent::Start(element) = event {
                let ids: HashSet<_> = element
                    .attributes
                    .iter()
                    .filter(|(_, name, _)| name == "v")
                    .map(|(id, _, _)| *id)
                    .collect();
                count += ids.len();
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(count, attributes);
        assert_eq!(budget.namespace_hash_bytes, hashed);
        // Only the second group's repeated declarations compare URI content.
        assert_eq!(budget.namespace_comparison_bytes, compared);
    }
}

// 이것을 실패시키는 것: 해시만으로 동등성을 확정, 일치 URI에 새 ID 배정, 길이가 다르면 같다고 판정.
// 운영과 공통인 URI 판정을 != 또는 true로 바꾸는 변이도 검출.
#[test]
fn namespace_hash_collisions_verify_content() {
    let mut namespaces = Namespaces::default();
    let mut budget = TextBudget::default();
    let hash = namespaces.hasher.hash_one("urn:shared-a");
    for (uri, expected) in [
        ("urn:shared-a", NamespaceId(9)),
        ("urn:shared-b", NamespaceId(10)),
        ("urn:shared-a", NamespaceId(9)),
        ("short", NamespaceId(11)),
        ("urn:shared-b", NamespaceId(10)),
    ] {
        assert_eq!(
            namespaces.register_hashed("test.xml", "a", uri.into(), hash, &mut budget).unwrap(),
            expected
        );
    }
    assert_eq!(budget.namespace_comparison_bytes, 48);
    assert_eq!(namespaces.uris.len(), 12);
}

// 이것을 실패시키는 것: prefix 조회를 활성 binding 선형 검색으로 되돌림, End에서 전체 binding 제거,
// prefix 스택의 맨 앞 선택, 빈 스택 미삭제, 수준별 되돌림 목록 누락.
// miss까지 호출 수로 세는 변이도 검출하며, 선형 탐색 변이는 방문한 항목마다 계수한다.
#[test]
fn prefix_lookup_is_independent_of_active_bindings() {
    let mut namespaces = Namespaces::default();
    let mut budget = TextBudget::default();
    namespaces.push();
    namespaces.add("test.xml", "a", "outer", "urn:outer".into(), &mut budget).unwrap();
    for depth in 0..9 {
        budget.prefix_lookups = 0;
        for _ in 0..1_000 {
            assert_eq!(
                namespaces.resolve("test.xml", "a", QName("outer:a"), false, &mut budget).unwrap(),
                NamespaceId(9)
            );
        }
        assert_eq!(budget.prefix_lookups, 1_000);
        if depth < 8 {
            namespaces.push();
            for i in 0..64 {
                namespaces
                    .add(
                        "test.xml",
                        "a",
                        &format!("p{depth}_{i}"),
                        "urn:filler".into(),
                        &mut budget,
                    )
                    .unwrap();
            }
        }
    }
    assert_eq!(namespaces.bindings.len(), 513);
    budget.prefix_lookups = 0;
    assert_eq!(namespaces.lookup("missing", &mut budget), None);
    assert_eq!(budget.prefix_lookups, 0);
    for _ in 0..8 {
        namespaces.pop();
    }
    assert_eq!(namespaces.bindings.len(), 1);
    assert_eq!(namespaces.lookup("outer", &mut budget), Some(NamespaceId(9)));
    namespaces.pop();
    assert_eq!(namespaces.bindings.len(), 0);
    assert_eq!(namespaces.levels.len(), 0);

    let mut observed = Vec::new();
    walk(
        b"<p:a xmlns:p='one'><p:b xmlns:p='two'><p:c xmlns:p='three'/><p:d/></p:b><p:e/></p:a>",
        "test.xml",
        &mut budget,
        |event| {
            if let XmlEvent::Start(element) = event {
                observed.push((element.name, element.namespace.to_string()));
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        observed,
        [
            ("a".into(), "one".into()),
            ("b".into(), "two".into()),
            ("c".into(), "three".into()),
            ("d".into(), "two".into()),
            ("e".into(), "one".into())
        ]
    );
}

// 이것을 실패시키는 것: 빈 named 선언 허용, 사용 시점까지 거부 지연, 기본 namespace 해제 거부.
#[test]
fn empty_named_namespace_is_rejected_at_declaration() {
    for (input, expected_element) in
        [("<a xmlns:p=''/>", "a"), ("<a xmlns:p='one'><p:b xmlns:p=''/></a>", "b")]
    {
        match values(input).unwrap_err() {
            DocxError::Invalid { part, element, detail } => {
                assert_eq!(part, "test.xml");
                assert_eq!(element, expected_element);
                assert_eq!(detail, "prefixed namespace declaration must not be empty");
            }
            other => panic!("{other:?}"),
        }
    }
    let mut ids = Vec::new();
    walk(
        b"<a xmlns='outer'><b xmlns=''/><c/></a>",
        "test.xml",
        &mut TextBudget::default(),
        |event| {
            if let XmlEvent::Start(element) = event {
                ids.push(element.namespace_id);
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(ids, [NamespaceId(9), NamespaceId(0), NamespaceId(9)]);
}

// 이것을 실패시키는 것: namespace 스코프 pop/기본 namespace/빈 재선언/예약 prefix 규칙 변경.
#[test]
fn namespace_scopes_and_reserved_bindings() {
    let mut observed = Vec::new();
    walk(b"<a xmlns='outer' xmlns:p='one'><p:b xmlns:p='two'/><p:c/><d xmlns=''/><p:e xml:lang='en'/></a>", "test.xml", &mut TextBudget::default(), |event| {
        if let XmlEvent::Start(element) = event { observed.push((element.name, element.namespace.to_string())); }
        Ok(())
    }).unwrap();
    assert_eq!(
        observed,
        [
            ("a".into(), "outer".into()),
            ("b".into(), "two".into()),
            ("c".into(), "one".into()),
            ("d".into(), "".into()),
            ("e".into(), "one".into())
        ]
    );
    for input in [
        "<a xmlns:xml='bad'/>",
        "<a xmlns:xmlns='bad'/>",
        "<a xmlns:q='http://www.w3.org/XML/1998/namespace'/>",
        "<a xmlns:q='http://www.w3.org/2000/xmlns/'/>",
        "<a xmlns='http://www.w3.org/2000/xmlns/'/>",
    ] {
        match values(input).unwrap_err() {
            DocxError::Invalid { detail, .. } => {
                assert_eq!(detail, "invalid reserved namespace binding")
            }
            other => panic!("{other:?}"),
        }
    }
    walk(
        b"<xml:a xmlns:xml='http://www.w3.org/XML/1998/namespace' xml:lang='en'/>",
        "test.xml",
        &mut TextBudget::default(),
        |event| {
            if let XmlEvent::Start(element) = event {
                assert_eq!(element.namespace.as_ref(), "http://www.w3.org/XML/1998/namespace");
                assert_eq!(element.attributes[0].0, NamespaceId(1));
            }
            Ok(())
        },
    )
    .unwrap();
    match values("<a xmlns:p='one'><p:b xmlns:p=''/></a>").unwrap_err() {
        DocxError::Invalid { detail, .. } => {
            assert_eq!(detail, "prefixed namespace declaration must not be empty")
        }
        other => panic!("{other:?}"),
    }
    match values("<a xmlns='http://purl.oclc.org/ooxml/wordprocessingml/mai&#110;'/>").unwrap_err() {
        DocxError::Invalid { detail, .. } => assert_eq!(detail, "Strict OOXML URI 'http://purl.oclc.org/ooxml/wordprocessingml/main'; expected Transitional OOXML"),
        other => panic!("{other:?}"),
    }
}

// 이것을 실패시키는 것: 값 버퍼를 무제한 두 배 성장시키거나 재사용하지 않음.
#[test]
fn value_capacity_is_bounded() {
    let mut value = String::with_capacity(700_000);
    value.push_str(&"x".repeat(700_000));
    reserve_value(&mut value, 1);
    assert_eq!(value.capacity(), 1_048_576);
    let pointer = value.as_ptr();
    reserve_value(&mut value, 1);
    assert_eq!(value.as_ptr(), pointer);
    let mut small = String::with_capacity(8);
    small.push_str("12345678");
    reserve_value(&mut small, 1);
    assert_eq!(small.capacity(), 16);
    match values("<xmlns:a/>").unwrap_err() {
        DocxError::Invalid { detail, .. } => assert_eq!(detail, "unbound namespace prefix 'xmlns'"),
        other => panic!("{other:?}"),
    }
}

// 이것을 실패시키는 것: 요소/속성 QName 검사 생략, 빈 prefix/local 허용,
// 다중 colon 허용, 선언을 검증 전에 등록, PI target NCName 검사 생략.
#[test]
fn qnames_and_pi_targets_are_validated_before_resolution() {
    for (input, element, reason) in [
        ("<1a/>", "1a", "invalid QName '1a'"),
        ("<:a/>", "a", "invalid QName ':a'"),
        ("<a:/>", "", "invalid QName 'a:'"),
        ("<p:a:b xmlns:p='urn:p'/>", "a:b", "invalid QName 'p:a:b'"),
        ("<a :Id='x'/>", "a", "invalid QName ':Id'"),
        ("<a x:='x'/>", "a", "invalid QName 'x:'"),
        ("<a 1x='x'/>", "a", "invalid QName '1x'"),
        ("<a xmlns:='urn:p'/>", "a", "invalid QName 'xmlns:'"),
        ("<a xmlns:1p='urn:p'/>", "a", "invalid QName 'xmlns:1p'"),
        ("<a xmlns:p:q='urn:p'/>", "a", "invalid QName 'xmlns:p:q'"),
        ("<a xmlns:='http://purl.oclc.org/ooxml/invalid'/>", "a", "invalid QName 'xmlns:'"),
        ("<?p:q?><a/>", "#document", "invalid PI target NCName 'p:q'"),
        ("<a><?1p?></a>", "a", "invalid PI target NCName '1p'"),
        ("<a/><?:p?>", "#document", "invalid PI target NCName ':p'"),
    ] {
        match values(input).unwrap_err() {
            DocxError::Invalid { part, element: actual, detail } => {
                assert_eq!(part, "test.xml");
                assert_eq!(actual, element);
                assert_eq!(detail, reason);
            }
            other => panic!("{other:?}"),
        }
    }
}

// 이것을 실패시키는 것: NCName을 ASCII로 제한, NameStartChar 범위 경계 축소/확대,
// 첫 글자에 NameChar 허용, 연속 글자의 추가 NameChar 종류 누락.
#[test]
fn unicode_ncname_ranges_and_korean_names() {
    for ch in [
        'A',
        'Z',
        '_',
        'a',
        'z',
        '\u{C0}',
        '\u{D6}',
        '\u{D8}',
        '\u{F6}',
        '\u{F8}',
        '\u{2FF}',
        '\u{370}',
        '\u{37D}',
        '\u{37F}',
        '\u{1FFF}',
        '\u{200C}',
        '\u{200D}',
        '\u{2070}',
        '\u{218F}',
        '\u{2C00}',
        '\u{2FEF}',
        '\u{3001}',
        '\u{D7FF}',
        '\u{F900}',
        '\u{FDCF}',
        '\u{FDF0}',
        '\u{FFFD}',
        '\u{10000}',
        '\u{EFFFF}',
    ] {
        assert!(ncname(&ch.to_string()), "{ch:?}");
    }
    for ch in [
        '@',
        '[',
        '`',
        '{',
        ':',
        '\u{BF}',
        '\u{D7}',
        '\u{F7}',
        '\u{37E}',
        '\u{2000}',
        '\u{200B}',
        '\u{200E}',
        '\u{206F}',
        '\u{2190}',
        '\u{2BFF}',
        '\u{2FF0}',
        '\u{3000}',
        '\u{E000}',
        '\u{F8FF}',
        '\u{FDD0}',
        '\u{FDEF}',
        '\u{FFFE}',
        '\u{FFFF}',
        '\u{F0000}',
    ] {
        assert!(!ncname(&ch.to_string()), "{ch:?}");
        assert!(!ncname(&format!("a{ch}")), "{ch:?}");
    }
    for ch in ['-', '.', '0', '9', '\u{B7}', '\u{300}', '\u{36F}', '\u{203F}', '\u{2040}'] {
        assert!(!ncname(&ch.to_string()), "{ch:?}");
        assert!(ncname(&format!("a{ch}")), "{ch:?}");
    }
    assert!(!ncname(""));
    assert_eq!(values("<?처리 지시?><한:문서 xmlns:한='urn:한글' 한:속성='값' 이름='정상'><문단_1>한글</문단_1></한:문서>").unwrap(), ["한글"]);
}

// 이것을 실패시키는 것: 요소 namespace URI를 Rc 공유 대신 String 복제로 전달.
#[test]
fn namespace_uris_share_storage_across_elements() {
    let uri = "u".repeat(4_096);
    let input = format!("<a xmlns='{uri}' xmlns:p='{uri}'>{}</a>", "<p:b/><c/>".repeat(64));
    let mut retained = Vec::new();
    walk(input.as_bytes(), "test.xml", &mut TextBudget::default(), |event| {
        if let XmlEvent::Start(element) = event {
            retained.push(element.namespace);
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(retained.len(), 129);
    assert_eq!(retained[0].len(), 4_096);
    for uri in &retained {
        assert!(Rc::ptr_eq(&retained[0], uri));
    }
}

// 이것을 실패시키는 것: 예약 PI 허용, 선언 version/순서/값/중복/구문 검사 생략.
#[test]
fn declarations_and_reserved_processing_instructions() {
    for declaration in [
        "<?XML?>",
        "<?Xml data?>",
        "<?xml?>",
        "<?xml encoding='UTF-8'?>",
        "<?xml version='2.0'?>",
        "<?xml version='1.0' extra='x'?>",
        "<?xml version='1.0' standalone='maybe'?>",
        "<?xml version='1.0' standalone='yes' encoding='UTF-8'?>",
        "<?xml version='1.0' version='1.0'?>",
        "<?xml version=1.0?>",
        "<?xml version='1.0' encoding=''?>",
        "<?xml version='1.0'encoding='UTF-8'?>",
        "<?xml version='1.0' encoding='1bad'?>",
        "<?xml version='1.0' encoding='UTF*8'?>",
    ] {
        assert!(
            matches!(values(&format!("{declaration}<a/> ")), Err(DocxError::Invalid { .. })),
            "{declaration}"
        );
    }
    for declaration in [
        "",
        "\u{feff}",
        "<?한글 처리?>",
        "<?xml version = '1.0'?>",
        "\u{feff}<?xml version='1.0' encoding='utf-8' standalone='no'?>",
    ] {
        assert_eq!(values(&format!("{declaration}<a>A</a>")).unwrap(), ["A"]);
    }
}
