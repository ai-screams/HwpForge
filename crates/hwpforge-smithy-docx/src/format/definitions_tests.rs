use super::*;

// 이것을 실패시키는 것: forward style ID 예산 누락, 해시 일치만으로 다른 ID 병합.
#[test]
fn forward_identifiers_are_bounded_and_collisions_compare_names() {
    let mut d = Definitions::default();
    let a = d.intern_style("a", "part", "style").unwrap();
    let b = d.intern_style("b", "part", "style").unwrap();
    let hash = d.style_hasher.hash_one("b");
    d.style_ids.get_mut(&hash).unwrap().insert(0, a);
    assert_eq!(d.style_key("b", "part", "style").unwrap(), b);
    d.styles.resize(131_072, None);
    assert_eq!(d.intern_style("a", "part", "style").unwrap(), a);
    assert!(matches!(
        d.intern_style("overflow", "part", "style"),
        Err(DocxError::Limit {
            resource: "style identifiers",
            observed: 131073,
            limit: 131072,
            ..
        })
    ));
}

// 이것을 실패시키는 것: numbering 정의/참조 ID와 theme typeface의 입력 한도 검사 삭제.
#[test]
fn definition_identifier_boundaries_are_checked_before_conversion() {
    for length in [4095, 4096, 4097] {
        let id = "0".repeat(length);
        let font = "f".repeat(length);
        for (root, xml) in [
                ("numbering", format!("<w:numbering xmlns:w='{}'><w:abstractNum w:abstractNumId='{id}'/><w:num w:numId='{id}'><w:abstractNumId w:val='{id}'/></w:num></w:numbering>", W_NS.1)),
                ("theme", format!("<a:theme xmlns:a='{}'><a:themeElements><a:fontScheme><a:majorFont><a:latin typeface='{font}'/></a:majorFont></a:fontScheme></a:themeElements></a:theme>", A_NS.1)),
            ] {
                let result = Definitions::default().read(xml.as_bytes(), "part", root, &mut TextBudget::default(), &mut vec![]);
                if length <= 4096 { assert!(result.is_ok()); }
                else { assert!(matches!(result, Err(DocxError::Limit { resource: "identifier bytes", observed: 4097, limit: 4096, .. }))); }
            }
    }
}

fn parse_definition(root: &str, content: &str) -> (Definitions, Vec<DocxWarning>) {
    let ns = if root == "theme" { A_NS.1 } else { W_NS.1 };
    let xml = format!("<w:{root} xmlns:w='{ns}' xmlns:x='urn:future'>{content}</w:{root}>");
    let mut d = Definitions::default();
    let mut warnings = vec![];
    assert!(d
        .read(xml.as_bytes(), "definitions.xml", root, &mut TextBudget::default(), &mut warnings)
        .unwrap());
    (d, warnings)
}

// 이것을 실패시키는 것: default bool의 잘못된 값을 false로 무경고 처리, true/false/부재 규칙 변경.
#[test]
fn definition_default_flags_share_property_boolean_rules() {
    for (flag, expected, count) in [
        ("", false, 0),
        (" w:default='maybe'", false, 1),
        (" w:default='1'", true, 0),
        (" w:default='true'", true, 0),
        (" w:default='on'", true, 0),
        (" w:default='0'", false, 0),
        (" w:default='false'", false, 0),
        (" w:default='off'", false, 0),
    ] {
        let (d, warnings) = parse_definition(
            "styles",
            &format!("<w:style w:type='paragraph' w:styleId='p'{flag}/>"),
        );
        assert_eq!(d.styles[0].as_ref().unwrap().default, expected);
        assert_eq!(warnings.iter().map(|w| w.count()).sum::<u64>(), count);
        if count == 1 {
            assert_eq!(
                (
                    warnings[0].code(),
                    warnings[0].loss(),
                    warnings[0].action(),
                    warnings[0].location()
                ),
                ("DOCX_UNSUPPORTED_VALUE", "format_value", "inherit_or_default", Some("style:p"))
            );
        }
    }
}

// 이것을 실패시키는 것: 정의별 속성 whitelist 검사 제거, 비-W 자식/속성을 W 이름으로 수용, Leaf 자손 무시.
#[test]
fn admitted_definition_containers_report_unknown_attributes_and_children() {
    for (root, before, node, attrs, after) in [
        ("styles", "", "docDefaults", "", ""),
        ("styles", "<w:docDefaults>", "rPrDefault", "", "</w:docDefaults>"),
        ("styles", "<w:docDefaults>", "pPrDefault", "", "</w:docDefaults>"),
        ("styles", "", "latentStyles", "", ""),
        ("styles", "<w:latentStyles>", "lsdException", " w:name='p'", "</w:latentStyles>"),
        ("styles", "", "style", " w:type='paragraph' w:styleId='p'", ""),
        (
            "styles",
            "<w:style w:type='paragraph' w:styleId='p'>",
            "name",
            " w:val='P'",
            "</w:style>",
        ),
        (
            "styles",
            "<w:style w:type='paragraph' w:styleId='p'>",
            "basedOn",
            " w:val='parent'",
            "</w:style>",
        ),
        (
            "styles",
            "<w:style w:type='table' w:styleId='p'>",
            "tblStylePr",
            " w:type='wholeTable'",
            "</w:style>",
        ),
        (
            "styles",
            "<w:style w:type='paragraph' w:styleId='p'>",
            "qFormat",
            " w:val='true'",
            "</w:style>",
        ),
        ("numbering", "", "abstractNum", " w:abstractNumId='1'", ""),
        (
            "numbering",
            "<w:abstractNum w:abstractNumId='1'>",
            "lvl",
            " w:ilvl='0'",
            "</w:abstractNum>",
        ),
        ("numbering", "", "num", " w:numId='1'", ""),
        ("numbering", "<w:num w:numId='1'>", "abstractNumId", " w:val='1'", "</w:num>"),
        ("numbering", "<w:num w:numId='1'>", "lvlOverride", " w:ilvl='0'", "</w:num>"),
        (
            "numbering",
            "<w:num w:numId='1'><w:lvlOverride w:ilvl='0'>",
            "lvl",
            " w:ilvl='0'",
            "</w:lvlOverride></w:num>",
        ),
        (
            "numbering",
            "<w:abstractNum w:abstractNumId='1'>",
            "nsid",
            " w:val='ABCD'",
            "</w:abstractNum>",
        ),
        ("theme", "", "themeElements", "", ""),
        ("theme", "<w:themeElements>", "fontScheme", " name='Fonts'", "</w:themeElements>"),
        (
            "theme",
            "<w:themeElements><w:fontScheme>",
            "majorFont",
            "",
            "</w:fontScheme></w:themeElements>",
        ),
        (
            "theme",
            "<w:themeElements><w:fontScheme><w:minorFont>",
            "latin",
            " typeface='A'",
            "</w:minorFont></w:fontScheme></w:themeElements>",
        ),
    ] {
        let (_, warnings) = parse_definition(root, &format!("{before}<w:{node}{attrs} w:future='x' x:val='x'><x:future/><w:future/></w:{node}>{after}"));
        let expected = match root {
            "styles" => "style_definition",
            "numbering" => "numbering_definition",
            _ => "theme_definition",
        };
        assert_eq!(
            warnings
                .iter()
                .map(|w| (w.code(), w.loss(), w.action(), w.count(), w.location()))
                .collect::<Vec<_>>(),
            [("DOCX_UNSUPPORTED_ATTRIBUTE", expected, "ignore", 4, Some(node))],
            "{root}/{node}"
        );
    }
}

// 이것을 실패시키는 것: mc:Ignorable 선언으로 확장 속성/요소의 진단을 생략, 루트 속성 검사 제거.
#[test]
fn ignorable_extensions_remain_diagnosed_in_definitions() {
    for (root, ns, loss) in [
        ("styles", W_NS.1, "style_definition"),
        ("numbering", W_NS.1, "numbering_definition"),
        ("theme", A_NS.1, "theme_definition"),
    ] {
        let xml = format!("<w:{root} xmlns:w='{ns}' xmlns:x='urn:future' xmlns:mc='{}' mc:Ignorable='x' x:name='ignored'><x:future/></w:{root}>", xml::MC_NS.1);
        let mut warnings = vec![];
        Definitions::default()
            .read(xml.as_bytes(), "part", root, &mut TextBudget::default(), &mut warnings)
            .unwrap();
        assert_eq!(
            warnings
                .iter()
                .map(|w| (w.code(), w.loss(), w.action(), w.count(), w.location()))
                .collect::<Vec<_>>(),
            [("DOCX_UNSUPPORTED_ATTRIBUTE", loss, "ignore", 3, Some(root))]
        );
    }
}

// 이것을 실패시키는 것: 명시적 무시 메타데이터에 손실 경고 추가, bool 메타데이터의 maybe를 무경고로 무시.
#[test]
fn metadata_allowlists_ignore_only_declared_meaning_and_validate_flags() {
    let (_, warnings) = parse_definition("styles", "<w:latentStyles w:count='1' w:defLockedState='0' w:defUIPriority='99' w:defSemiHidden='true' w:defUnhideWhenUsed='on' w:defQFormat='false'><w:lsdException w:name='p' w:locked='off' w:uiPriority='9' w:semiHidden='1' w:unhideWhenUsed='0' w:qFormat='true'/></w:latentStyles><w:style w:type='paragraph' w:styleId='p' w:customStyle='true'><w:next w:val='p'/><w:link w:val='c'/><w:aliases w:val='P'/><w:uiPriority w:val='1'/><w:qFormat/><w:semiHidden/><w:unhideWhenUsed/><w:locked/><w:personal/><w:personalCompose/><w:personalReply/><w:rsid w:val='ABCD'/><w:autoRedefine/></w:style>");
    assert_eq!(warnings.len(), 0);
    let (_, warnings) = parse_definition("numbering", "<w:abstractNum w:abstractNumId='1'><w:nsid w:val='A'/><w:tmpl w:val='B'/><w:name w:val='C'/><w:lvl w:ilvl='0' w:tplc='D' w:tentative='true'/></w:abstractNum><w:numIdMacAtCleanup w:val='2'/>");
    assert_eq!(warnings.len(), 0);
    for (root, content, count) in [
        ("styles", "<w:style w:type='paragraph' w:styleId='p' w:customStyle='maybe'><w:qFormat w:val='maybe'/><w:semiHidden w:val='maybe'/><w:unhideWhenUsed w:val='maybe'/><w:locked w:val='maybe'/><w:personal w:val='maybe'/><w:personalCompose w:val='maybe'/><w:personalReply w:val='maybe'/><w:autoRedefine w:val='maybe'/></w:style>", 9),
        ("styles", "<w:latentStyles w:defLockedState='maybe' w:defSemiHidden='maybe' w:defUnhideWhenUsed='maybe' w:defQFormat='maybe'><w:lsdException w:locked='maybe' w:semiHidden='maybe' w:unhideWhenUsed='maybe' w:qFormat='maybe'/></w:latentStyles>", 8),
        ("numbering", "<w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0' w:tentative='maybe'/></w:abstractNum>", 1),
    ] {
        let (_, warnings) = parse_definition(root, content);
        assert_eq!(warnings.iter().map(|w| (w.code(), w.loss(), w.action(), w.count())).collect::<Vec<_>>(), [("DOCX_UNSUPPORTED_VALUE", "format_value", "inherit_or_default", count)]);
    }
}

// 이것을 실패시키는 것: 번호 스타일의 미해석 basedOn 연결을 무경고로 제외.
#[test]
fn numbering_style_parent_loss_is_explicit() {
    let (d, warnings) = parse_definition(
        "styles",
        "<w:style w:type='numbering' w:styleId='n'><w:basedOn w:val='parent'/></w:style>",
    );
    assert_eq!(d.styles[0].as_ref().unwrap().parent, None);
    assert_eq!(
        warnings
            .iter()
            .map(|w| (w.code(), w.loss(), w.action(), w.count(), w.location()))
            .collect::<Vec<_>>(),
        [("DOCX_UNSUPPORTED_ATTRIBUTE", "style_definition", "ignore", 1, Some("basedOn"))]
    );
}

// 이것을 실패시키는 것: 이미 경고한 하위 트리를 재계수, theme 표시 이름을 손실로 분류.
#[test]
fn definition_subtree_losses_are_counted_once_at_the_boundary() {
    for (root, content, expected) in [
        ("styles", "<w:style w:type='table' w:styleId='t'><w:tblStylePr w:type='firstRow'><x:future/></w:tblStylePr></w:style>", "conditional_table_style"),
        ("numbering", "<w:abstractNum w:abstractNumId='1'><w:numStyleLink w:val='n'><x:future/></w:numStyleLink></w:abstractNum>", "numbering_style_link"),
        ("theme", "<w:themeElements><w:fontScheme><w:majorFont><w:font script='Test' typeface='A'><x:future/></w:font></w:majorFont></w:fontScheme></w:themeElements>", "theme_script_font"),
    ] {
        let (_, warnings) = parse_definition(root, content);
        assert_eq!(warnings.iter().map(|w| (w.code(), w.loss(), w.action(), w.count())).collect::<Vec<_>>(), [("DOCX_UNSUPPORTED_ATTRIBUTE", expected, "ignore", 1)]);
    }
    let xml = format!("<a:theme xmlns:a='{}' name='Theme'><a:themeElements><a:fontScheme name='Fonts'><a:majorFont><a:latin typeface='A'/><a:ea typeface='B'/><a:cs typeface='C'/></a:majorFont></a:fontScheme></a:themeElements></a:theme>", A_NS.1);
    let mut warnings = vec![];
    Definitions::default()
        .read(xml.as_bytes(), "theme.xml", "theme", &mut TextBudget::default(), &mut warnings)
        .unwrap();
    assert_eq!(warnings.len(), 0);
}

// 이것을 실패시키는 것: 정의 속성 이름/요소 누락 또는 집계 전 반복 사유 생성.
#[test]
fn definition_attribute_reason_keeps_attribute_and_element_lazy() {
    let one = "<w:docDefaults w:future='x'/>";
    crate::warning::take_diagnostic_bytes();
    let (_, warnings) = parse_definition("styles", one);
    let first = crate::warning::take_diagnostic_bytes();
    let (_, repeated) = parse_definition("styles", &one.repeat(32));
    let bytes = crate::warning::take_diagnostic_bytes();
    assert_eq!(warnings[0].message(), "Unsupported definition attribute 'future' on 'docDefaults'");
    assert_eq!((warnings[0].count(), repeated[0].count()), (1, 32));
    assert_eq!(bytes - first, 0);
}
