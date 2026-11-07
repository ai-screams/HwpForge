use super::*;

// 이것을 실패시키는 것: 값 오류의 ?가 공통 속성 검사를 건너뛰거나 참조 정수 오류를 복구.
#[test]
fn leaf_attributes_are_checked_even_when_value_reading_fails() {
    for (name, character, value) in [
        ("numId", false, "oops".to_owned()),
        ("ilvl", false, "oops".to_owned()),
        ("rFonts", true, "f".repeat(4097)),
    ] {
        let key = if character { "hAnsi" } else { "val" };
        let xml = format!("<w:{name} xmlns:w='{}' w:{key}='{value}' w:future='x'/>", W_NS.1);
        let mut warnings = vec![];
        let result = crate::xml::walk(
            xml.as_bytes(),
            "part",
            &mut crate::xml::TextBudget::default(),
            |event| {
                if let crate::xml::XmlEvent::Start(e) = event {
                    read_property(
                        &mut Delta::default(),
                        &e,
                        character,
                        &Theme::default(),
                        &mut Fonts::default(),
                        "part",
                        "leaf",
                        &mut warnings,
                    )?;
                }
                Ok(())
            },
        );
        if character {
            assert!(matches!(
                result,
                Err(DocxError::Limit {
                    resource: "identifier bytes",
                    observed: 4097,
                    limit: 4096,
                    ..
                })
            ));
        } else {
            assert!(matches!(result, Err(DocxError::Invalid { .. })));
        }
        assert_eq!(
            warnings
                .iter()
                .map(|w| (w.code(), w.loss(), w.action(), w.count()))
                .collect::<Vec<_>>(),
            [("DOCX_UNSUPPORTED_ATTRIBUTE", "format_attribute", "ignore", 1)]
        );
    }
}
