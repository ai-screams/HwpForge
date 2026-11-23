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

// 이것을 실패시키는 것: 문단/명명 스타일을 자연 최소값으로 올리거나 물리 간격의 두 분기 단위를 바꿈.
#[test]
fn minimum_spacing_preserves_source_in_referenced_xml_shapes() {
    for (line, case_value, default_value) in
        [(0, "0", "0"), (160, "800", "1600"), (360, "1800", "3600")]
    {
        let body = format!("<w:p><w:pPr><w:pStyle w:val='minimum'/></w:pPr></w:p><w:p><w:pPr><w:pStyle w:val='minimum'/></w:pPr><w:r><w:t>A</w:t></w:r></w:p><w:p><w:pPr><w:spacing w:line='{line}' w:lineRule='atLeast'/></w:pPr><w:r><w:t>A</w:t></w:r></w:p>");
        let source = docx::package_with_defaults(&body, "");
        let mut source = zip::ZipArchive::new(Cursor::new(source)).unwrap();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..source.len() {
            let mut f = source.by_index(i).unwrap();
            let name = f.name().unwrap().to_string();
            let mut content = String::new();
            f.read_to_string(&mut content).unwrap();
            if name == "styles.xml" {
                content = content.replace("</w:styles>", &format!("<w:style w:type='paragraph' w:styleId='minimum'><w:name w:val='Minimum'/><w:pPr><w:spacing w:line='{line}' w:lineRule='atLeast'/></w:pPr></w:style></w:styles>"));
            }
            out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            out.write_all(content.as_bytes()).unwrap();
        }
        let result = convert_docx(&out.finish().unwrap().into_inner()).unwrap();
        let header = tree(&result.bytes, "Contents/header.xml");
        let section = tree(&result.bytes, "Contents/section0.xml");
        let refs: Vec<_> = section
            .children
            .iter()
            .filter(|p| p.name == "hp:p")
            .map(|p| p.attr("paraPrIDRef").unwrap())
            .chain(
                header
                    .descendants("hh:style")
                    .into_iter()
                    .filter(|s| {
                        s.attr("id")
                            == section
                                .children
                                .iter()
                                .find(|p| p.name == "hp:p")
                                .unwrap()
                                .attr("styleIDRef")
                    })
                    .map(|s| s.attr("paraPrIDRef").unwrap()),
            )
            .collect();
        assert_eq!(refs.len(), 4);
        for reference in refs {
            let shape = header
                .descendants("hh:paraPr")
                .into_iter()
                .find(|p| p.attr("id") == Some(reference))
                .unwrap();
            let switch = shape.child("hp:switch");
            assert_eq!(switch.children.len(), 2);
            for (branch, expected) in [("hp:case", case_value), ("hp:default", default_value)] {
                let spacing = switch.child(branch).child("hh:lineSpacing");
                assert_eq!(spacing.attr("type"), Some("AT_LEAST"));
                assert_eq!(spacing.attr("value"), Some(expected));
            }
        }
    }
}

// 이것을 실패시키는 것: 쪽/단 fragment 경계 전달 누락, 칸 밖 atLeast 진단 삭제, 원본 간격 선투영.
#[test]
fn fragment_diagnostics_keep_source_minimum_in_xml() {
    for (separator, mixed, paragraphs) in
        [("", 1, 1), ("<w:br w:type='page'/>", 0, 3), ("<w:br w:type='column'/>", 0, 3)]
    {
        let runs: String = [(22, "A"), (32, "B"), (40, "C")].into_iter().enumerate().map(|(i, (size, text))| format!("<w:r><w:rPr><w:rFonts w:ascii='Cambria' w:hAnsi='Cambria' w:eastAsia='Cambria'/><w:sz w:val='{size}'/></w:rPr>{}<w:t>{text}</w:t></w:r>", if i == 0 { "" } else { separator })).collect();
        let bytes = docx::package(&format!(
            "<w:p><w:pPr><w:spacing w:line='160' w:lineRule='atLeast'/></w:pPr>{runs}</w:p>"
        ));
        let source = hwpforge::docx::decode_docx(&bytes).unwrap();
        assert_eq!(
            source
                .warnings
                .iter()
                .filter(|w| w.loss() == "mixed_font_line_height")
                .map(|w| w.count())
                .sum::<u64>(),
            mixed
        );
        for section in source.document.sections() {
            for p in &section.paragraphs {
                assert_eq!(
                    source.registry.para_shapes[p.para_shape_id.get()].line_spacing_value,
                    1600.0
                );
            }
        }
        let result = convert_docx(&bytes).unwrap();
        let header = tree(&result.bytes, "Contents/header.xml");
        let section = tree(&result.bytes, "Contents/section0.xml");
        let ps: Vec<_> = section.children.iter().filter(|p| p.name == "hp:p").collect();
        assert_eq!(ps.len(), paragraphs);
        for p in ps {
            let shape = header
                .descendants("hh:paraPr")
                .into_iter()
                .find(|s| s.attr("id") == p.attr("paraPrIDRef"))
                .unwrap();
            for (branch, expected) in [("hp:case", "800"), ("hp:default", "1600")] {
                let spacing = shape.child("hp:switch").child(branch).child("hh:lineSpacing");
                assert_eq!(spacing.attr("type"), Some("AT_LEAST"));
                assert_eq!(spacing.attr("value"), Some(expected));
            }
        }
    }
}
