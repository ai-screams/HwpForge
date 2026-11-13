use super::tests::{package, W};
use crate::{decode_docx, DocxDecoded};
use hwpforge_blueprint::EastAsianAutoSpacing;
use hwpforge_core::RunContent;

fn read(default: &str, body: &str) -> DocxDecoded {
    let styles = format!("<w:styles xmlns:w='{W}'><w:style w:type='paragraph' w:default='1' w:styleId='a'><w:name w:val='Normal'/><w:pPr>{default}</w:pPr></w:style><w:style w:type='paragraph' w:styleId='child'><w:basedOn w:val='a'/></w:style></w:styles>");
    decode_docx(&package(body, &[("styles", "styles.xml", &styles)])).unwrap()
}
const OFF: &str = "<w:autoSpaceDE w:val='0'/><w:autoSpaceDN w:val='0'/>";
fn p(props: &str) -> String {
    format!("<w:p><w:pPr>{props}</w:pPr><w:r><w:t>한글 Latin 123</w:t></w:r></w:p>")
}
fn settings(d: &DocxDecoded) -> Vec<Option<EastAsianAutoSpacing>> {
    fn visit(
        d: &DocxDecoded,
        ps: &[hwpforge_core::Paragraph],
        out: &mut Vec<Option<EastAsianAutoSpacing>>,
    ) {
        for p in ps {
            out.push(d.registry.para_shapes[p.para_shape_id.get()].east_asian_auto_spacing);
            for r in &p.runs {
                if let RunContent::Table(t) = &r.content {
                    for row in &t.rows {
                        for cell in &row.cells {
                            visit(d, &cell.paragraphs, out);
                        }
                    }
                }
            }
        }
    }
    let mut out = vec![];
    for s in d.document.sections() {
        visit(d, &s.paragraphs, &mut out);
    }
    out
}
// 이것을 실패시키는 것: 기본 문단 스타일 ID를 Normal로 가정하거나 표 셀 상속 누락.
#[test]
fn normal_disables_auto_spacing_in_used_paragraphs() {
    let body = format!(
        "{}{}<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>",
        p(""),
        p("<w:pStyle w:val='child'/>"),
        p("")
    );
    let d = read(OFF, &body);
    let values = settings(&d);
    assert!(values.len() >= 3);
    assert!(values.iter().all(|v| *v == Some(EastAsianAutoSpacing::new(false, false))));
    assert!(!d.warnings.iter().any(|w| matches!(w.loss(), "auto_space_de" | "auto_space_dn")));
}
// 이것을 실패시키는 것: 직접 자동 간격을 전역 적용하거나 서로 다른 모양을 병합.
#[test]
fn direct_auto_spacing_overrides_only_its_paragraph() {
    let d = read(OFF, &format!("{}{}{}", p(""), p("<w:autoSpaceDE/><w:autoSpaceDN/>"), p("")));
    assert_eq!(
        settings(&d),
        [
            Some(EastAsianAutoSpacing::new(false, false)),
            Some(EastAsianAutoSpacing::new(true, true)),
            Some(EastAsianAutoSpacing::new(false, false))
        ]
    );
}
// 이것을 실패시키는 것: DOCX 미지정을 명시적인 0/0으로 채움.
#[test]
fn unspecified_docx_auto_spacing_uses_word_default() {
    let d = read("", &p(""));
    assert_eq!(settings(&d), [None]);
    for (props, value) in [
        ("<w:autoSpaceDE w:val='0'/>", EastAsianAutoSpacing::new(false, true)),
        ("<w:autoSpaceDN w:val='0'/>", EastAsianAutoSpacing::new(true, false)),
    ] {
        assert_eq!(settings(&read("", &p(props))), [Some(value)]);
    }
}
