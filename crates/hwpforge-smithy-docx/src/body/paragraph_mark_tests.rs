use super::tests::{package, W};
use crate::decode_docx;
// 이것을 실패시키는 것: 글꼴/크기/bold/italic 비교 제거, 색만의 차이 진단, 조각마다 중복, family 필터 추가.
#[test]
fn paragraph_mark_line_height_is_diagnosed_once_from_effective_values() {
    for family in ["Cambria", "Arial"] {
        let styles=format!("<w:styles xmlns:w='{W}'><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii='{family}' w:hAnsi='{family}' w:eastAsia='{family}'/><w:sz w:val='22'/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>");
        for (mark, expected) in [
            ("<w:rFonts w:ascii='Calibri' w:hAnsi='Calibri'/>", 1),
            ("<w:rFonts w:eastAsia='바탕'/>", 1),
            ("<w:sz w:val='40'/>", 1),
            ("<w:b/>", 1),
            ("<w:i/>", 1),
            ("<w:color w:val='FF0000'/>", 0),
            ("", 0),
        ] {
            for text in [
                "<w:r><w:t>A</w:t></w:r>",
                "<w:r><w:t>A</w:t><w:br w:type='page'/><w:t>B</w:t></w:r>",
            ] {
                let body = format!("<w:p><w:pPr><w:rPr>{mark}</w:rPr></w:pPr>{text}</w:p>");
                let d = decode_docx(&package(&body, &[("styles", "styles.xml", &styles)])).unwrap();
                let warnings: Vec<_> = d
                    .warnings
                    .iter()
                    .filter(|w| w.loss() == "paragraph_mark_line_height")
                    .collect();
                assert_eq!(
                    warnings.iter().map(|w| w.count()).sum::<u64>(),
                    expected,
                    "{family}/{mark}"
                );
                if expected == 1 {
                    assert_eq!(warnings[0].code(), "DOCX_VALUE_APPROXIMATED");
                    assert_eq!(warnings[0].action(), "retain_spacing");
                    assert_eq!(warnings[0].message(),"Paragraph text and end mark use different effective fonts, size, or face; line height is retained");
                }
                for section in d.document.sections() {
                    for p in &section.paragraphs {
                        assert_eq!(
                            d.registry.para_shapes[p.para_shape_id.get()].line_spacing_value,
                            100.
                        );
                    }
                }
            }
        }
        for body in [
            "<w:p><w:pPr><w:rPr><w:sz w:val='40'/></w:rPr></w:pPr></w:p>",
            "<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>A</w:t></w:r></w:p>",
        ] {
            let d = decode_docx(&package(body, &[("styles", "styles.xml", &styles)])).unwrap();
            assert!(d.warnings.iter().all(|w| w.loss() != "paragraph_mark_line_height"));
        }
    }
}

// 이것을 실패시키는 것: 첫 run 비교, 빈 마지막 run을 유효 텍스트로 비교.
#[test]
fn paragraph_mark_line_height_compares_last_nonempty_text() {
    let body="<w:p><w:pPr><w:rPr><w:b w:val='0'/><w:i w:val='0'/></w:rPr></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>first</w:t></w:r><w:r><w:t>last</w:t></w:r><w:r><w:rPr><w:sz w:val='40'/></w:rPr><w:t/></w:r></w:p>";
    let d = decode_docx(&package(body, &[])).unwrap();
    assert!(d.warnings.iter().all(|w| w.loss() != "paragraph_mark_line_height"));
}
