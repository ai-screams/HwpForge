#![cfg(feature = "ops-docx")]
#[path = "support/docx.rs"]
mod docx;
#[path = "../../hwpforge-smithy-hwpx/tests/support/package.rs"]
mod package;
#[path = "../../hwpforge-smithy-hwpx/tests/support/xml.rs"]
mod xml;

// 이것을 실패시키는 것: 유효 자동 간격을 출력 전에 버리거나 참조하지 않는 기본 모양만 검사.
#[test]
fn imported_auto_spacing_matches_used_paragraphs() {
    for (default, direct, expected) in [
        ("<w:autoSpaceDE w:val='0'/><w:autoSpaceDN w:val='0'/>", "", [[0, 0], [0, 0], [0, 0]]),
        (
            "<w:autoSpaceDE w:val='0'/><w:autoSpaceDN w:val='0'/>",
            "<w:autoSpaceDE/><w:autoSpaceDN/>",
            [[0, 0], [1, 1], [0, 0]],
        ),
        ("", "", [[1, 1], [1, 1], [1, 1]]),
        ("<w:autoSpaceDN w:val='0'/>", "<w:autoSpaceDE/>", [[1, 0], [1, 0], [1, 0]]),
    ] {
        let p = |props: &str| {
            format!("<w:p><w:pPr>{props}</w:pPr><w:r><w:t>한글 Latin 123</w:t></w:r></w:p>")
        };
        let input = docx::package_with_defaults(
            &format!("{}{}<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>", p(""), p(direct), p("")),
            "",
        );
        let input=package::splice_part(&input,"styles.xml","</w:styles>",&format!("<w:style w:type='paragraph' w:default='1' w:styleId='a'><w:name w:val='Normal'/><w:pPr>{default}</w:pPr></w:style></w:styles>"));
        let out = hwpforge::ops::convert_docx(&input).unwrap();
        assert!(!out.warnings.iter().any(|w|matches!(w,hwpforge::ops::OpsWarning::Docx(w) if matches!(w.loss(),"auto_space_de"|"auto_space_dn"))));
        let h = xml::Element::parse(&package::part(&out.bytes, "Contents/header.xml"));
        let s = xml::Element::parse(&package::section_xml(&out.bytes));
        let ps: Vec<_> = s
            .descendants("hp:p")
            .into_iter()
            .filter(|p| {
                p.children
                    .iter()
                    .filter(|n| n.name == "hp:run")
                    .flat_map(|r| &r.children)
                    .any(|n| n.name == "hp:t" && !n.text.is_empty())
            })
            .collect();
        assert_eq!(ps.len(), 3);
        for (p, [de, dn]) in ps.iter().zip(expected) {
            let shape = h
                .descendants("hh:paraPr")
                .into_iter()
                .find(|s| s.attr("id") == p.attr("paraPrIDRef"))
                .unwrap();
            assert_eq!(shape.children.iter().filter(|n| n.name == "hh:autoSpacing").count(), 1);
            let a = shape.child("hh:autoSpacing");
            assert_eq!(a.attr("eAsianEng"), Some(de.to_string().as_str()));
            assert_eq!(a.attr("eAsianNum"), Some(dn.to_string().as_str()));
        }
    }
}
