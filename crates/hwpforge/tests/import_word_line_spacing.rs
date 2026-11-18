#![cfg(feature = "ops-docx")]
use hwpforge::ops::convert_docx;
#[path = "support/docx.rs"]
mod docx;
#[path = "../../hwpforge-smithy-hwpx/tests/support/xml.rs"]
mod xml;
use std::io::{Cursor, Read, Write};
fn input(text: &str, hint: &str, font: &str) -> Vec<u8> {
    let props=format!("<w:rFonts w:ascii='Cambria' w:hAnsi='Cambria' w:eastAsia='{font}' {hint}/><w:sz w:val='22'/>");
    let body=format!("<w:p><w:pPr><w:pStyle w:val='named'/><w:spacing w:line='240'/><w:rPr>{props}</w:rPr></w:pPr><w:r><w:rPr>{props}</w:rPr><w:t xml:space='preserve'>{text}</w:t></w:r></w:p>");
    let bytes = docx::package_with_defaults(&body, "");
    let mut source = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..source.len() {
        let mut f = source.by_index(i).unwrap();
        let name = f.name().unwrap().to_string();
        let mut text = String::new();
        f.read_to_string(&mut text).unwrap();
        if name == "styles.xml" {
            text=text.replace("함초롬바탕","맑은 고딕").replace("</w:styles>","<w:style w:type='paragraph' w:styleId='named'><w:name w:val='Named'/><w:rPr><w:rFonts w:ascii='맑은 고딕' w:hAnsi='맑은 고딕' w:eastAsia='맑은 고딕'/></w:rPr></w:style></w:styles>");
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(text.as_bytes()).unwrap();
    }
    out.finish().unwrap().into_inner()
}
fn tree(bytes: &[u8], name: &str) -> xml::Element {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    z.by_name(name).unwrap().read_to_string(&mut text).unwrap();
    xml::Element::parse(&text)
}
fn fonts(h: &xml::Element, id: &str) -> Vec<String> {
    let c = h
        .descendants("hh:charPr")
        .into_iter()
        .find(|c| c.attr("id") == Some(id))
        .unwrap()
        .child("hh:fontRef");
    ["hangul", "latin", "hanja", "japanese", "other", "symbol", "user"]
        .iter()
        .map(|slot| {
            let group = slot.to_uppercase();
            let face = h
                .descendants("hh:fontface")
                .into_iter()
                .find(|f| f.attr("lang") == Some(group.as_str()))
                .unwrap();
            face.children
                .iter()
                .find(|f| f.attr("id") == c.attr(slot))
                .unwrap()
                .attr("face")
                .unwrap()
                .to_owned()
        })
        .collect()
}
fn styles(h: &xml::Element) -> Vec<(Vec<(String, String)>, Vec<String>)> {
    h.descendants("hh:style")
        .iter()
        .map(|s| {
            (
                s.attributes.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                fonts(h, s.attr("charPrIDRef").unwrap()),
            )
        })
        .collect()
}
// 이것을 실패시키는 것: hint/기호 분리 활성화, 따옴표 보정, 기본 charPr 또는 named style 글꼴 변경.
#[test]
fn import_word_line_spacing_keeps_fonts_runs_and_named_styles() {
    for font in ["맑은 고딕", "바탕"] {
        let plain = convert_docx(&input("ASCII", "", font)).unwrap();
        let baseline = tree(&plain.bytes, "Contents/header.xml");
        for (text, expected) in [
            ("2009–2013", "100"),
            ("don’t", "100"),
            ("‘single’", "100"),
            ("“double”", "100"),
            ("·", "100"),
            ("날짜", "100"),
            ("ABC", "87"),
            ("'single' &quot;double&quot;", "87"),
        ] {
            for hint in
                ["", "w:hint='default'", "w:hint='eastAsia'", "w:hint='cs'", "w:hint='unknown'"]
            {
                let out = convert_docx(&input(text, hint, font)).unwrap();
                let h = tree(&out.bytes, "Contents/header.xml");
                let s = tree(&out.bytes, "Contents/section0.xml");
                let p = s.child("hp:p");
                let shape = h
                    .descendants("hh:paraPr")
                    .into_iter()
                    .find(|d| d.attr("id") == p.attr("paraPrIDRef"))
                    .unwrap();
                for line in shape.descendants("hh:lineSpacing") {
                    assert_eq!(line.attr("type"), Some("PERCENT"));
                    assert_eq!(line.attr("value"), Some(expected));
                }
                assert_eq!(styles(&h), styles(&baseline));
                assert_eq!(fonts(&h, "0"), vec![font; 7]);
                assert_eq!(fonts(&h, "3"), vec![font; 7]);
                let text_runs: Vec<_> = p
                    .children
                    .iter()
                    .filter(|r| {
                        r.name == "hp:run"
                            && r.children.iter().any(|c| c.name == "hp:t" && !c.text.is_empty())
                    })
                    .collect();
                assert_eq!(text_runs.len(), 1);
                let names = fonts(&h, text_runs[0].attr("charPrIDRef").unwrap());
                assert_eq!(names[0], font);
                assert_eq!(names[1], "Cambria");
                assert_eq!(names[5], font);
                assert_eq!(
                    h.descendants("hh:charPr").len(),
                    baseline.descendants("hh:charPr").len()
                );
            }
        }
    }
}
