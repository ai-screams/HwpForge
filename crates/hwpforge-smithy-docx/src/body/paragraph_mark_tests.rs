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
                    assert_eq!(warnings[0].action(), "preserve_end_mark");
                    assert_eq!(warnings[0].message(),"Paragraph text and end mark use different effective fonts, size, or face; the source end-mark formatting is preserved in Core.");
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

fn mark_styles(east: &str) -> String {
    format!("<w:styles xmlns:w='{W}'><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii='Cambria' w:hAnsi='Cambria' w:eastAsia='{east}'/><w:sz w:val='22'/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>")
}
fn marked_paragraph(text: &str, size: u32, spacing: &str) -> String {
    format!("<w:p><w:pPr>{spacing}<w:rPr><w:sz w:val='{size}'/></w:rPr></w:pPr><w:r><w:t>A{text}</w:t></w:r></w:p>")
}
fn cell_height(paragraph: &str, styles: &str, row: &str) -> crate::DocxDecoded {
    let body = format!("<w:tbl><w:tr><w:trPr>{row}</w:trPr><w:tc><w:tcPr><w:tcMar><w:top w:w='0'/><w:bottom w:w='0'/></w:tcMar></w:tcPr>{paragraph}</w:tc></w:tr></w:tbl>");
    decode_docx(&package(&body, &[("styles", "styles.xml", styles)])).unwrap()
}
fn first_table(d: &crate::DocxDecoded) -> &hwpforge_core::Table {
    let hwpforge_core::RunContent::Table(t) =
        &d.document.sections()[0].paragraphs[0].runs[0].content
    else {
        panic!("table")
    };
    t
}
// 이것을 실패시키는 것: 글자가 있는 마지막 줄에도 끝 높이를 가산하거나 빈 마지막 줄을 문단 전체 nonempty로 제외.
#[test]
fn populated_lines_exclude_mark_and_empty_lines_keep_source_height() {
    for (east, tails, expected) in [
        (
            "Cambria",
            ["", "</w:t><w:br/><w:t>", "</w:t><w:br/><w:br/><w:t>"],
            [[1290, 1290], [2966, 4338], [4642, 6014]],
        ),
        (
            "맑은 고딕",
            ["", "</w:t><w:br/><w:t>", "</w:t><w:br/><w:br/><w:t>"],
            [[1290, 1290], [3192, 4748], [5094, 6650]],
        ),
    ] {
        for (tail, heights) in tails.into_iter().zip(expected) {
            for (size, height) in [22, 40].into_iter().zip(heights) {
                let d = cell_height(&marked_paragraph(tail, size, ""), &mark_styles(east), "");
                let t = first_table(&d);
                assert_eq!(t.rows[0].height.unwrap().as_i32(), height, "{east}/{tail}/{size}");
                let p = &t.rows[0].cells[0].paragraphs[0];
                assert_eq!(
                    d.registry.char_shapes[p.end_char_shape.unwrap().get()].size.as_i32(),
                    if size == 22 { 1100 } else { 2000 }
                );
            }
        }
    }
}
// 이것을 실패시키는 것: 마지막 논리 줄 추적을 measure_lines 안에서만 실행하거나 끝 표시를 height.project에서 전부 제외.
#[test]
fn minimum_spacing_tracks_tail_without_cell_measurement() {
    for (tail, values) in [("", [2580., 2580.]), ("</w:t><w:br/><w:t>", [2580., 4690.])] {
        for (size, expected) in [22, 40].into_iter().zip(values) {
            let p = marked_paragraph(tail, size, "<w:spacing w:line='160' w:lineRule='atLeast'/>");
            let styles = mark_styles("Cambria");
            let d = decode_docx(&package(&p, &[("styles", "styles.xml", &styles)])).unwrap();
            let p = &d.document.sections()[0].paragraphs[0];
            let shape = &d.registry.para_shapes[p.para_shape_id.get()];
            assert_eq!(shape.line_spacing_type, hwpforge_foundation::LineSpacingType::AtLeast);
            assert_eq!(shape.line_spacing_value, expected, "{tail}/{size}");
        }
    }
}
// 이것을 실패시키는 것: 조각 끝 조건 제거로 뒤 조각 끝20이 첫 빈 줄의 본문11 fallback을 대체.
#[test]
fn fragment_minimum_does_not_borrow_later_mark() {
    for kind in ["page", "column"] {
        let p = marked_paragraph(
            &format!("</w:t><w:br/><w:br w:type='{kind}'/><w:t>B</w:t><w:br/><w:t>"),
            40,
            "",
        );
        let d = cell_height(&p, &mark_styles("Cambria"), "");
        let t = first_table(&d);
        assert_eq!(t.rows[0].height.unwrap().as_i32(), 2966, "{kind}");
        assert_eq!(t.rows[0].cells[0].paragraphs[0].end_char_shape, None);
    }
}
// 이것을 실패시키는 것: 크기가 큰 끝 표시를 exact 높이 예산에 더하거나 빈 문단의 원본 끝 높이를 삭제.
#[test]
fn empty_marks_and_exact_rows_keep_existing_budget() {
    let styles = mark_styles("Cambria");
    let p = marked_paragraph("", 40, "");
    let d = cell_height(&p, &styles, "<w:trHeight w:val='200' w:hRule='exact'/>");
    assert_eq!(first_table(&d).rows[0].height.unwrap().as_i32(), 1000);
    assert!(first_table(&d).frame.size_protect);
    let exact = marked_paragraph("", 40, "<w:spacing w:line='160' w:lineRule='exact'/>");
    let d = cell_height(&exact, &styles, "");
    assert_eq!(first_table(&d).rows[0].height.unwrap().as_i32(), 800);
    let p = p.replace("<w:t>A</w:t>", "<w:t/>");
    let d = cell_height(&p, &styles, "");
    assert_eq!(first_table(&d).rows[0].height.unwrap().as_i32(), 3048);
}

// 이것을 실패시키는 것: 유효 동아시아 칸이 없는 빈 줄에도 East 배율을 적용하거나 끝 표시를 삭제.
#[test]
fn latin_only_empty_lines_use_the_existing_latin_fallback() {
    let styles = mark_styles("Cambria").replace(" w:eastAsia='Cambria'", "");
    for (tail, values) in
        [("</w:t><w:br/><w:t>", [2580, 3635]), ("</w:t><w:br/><w:br/><w:t>", [3870, 4925])]
    {
        for (size, expected) in [22, 40].into_iter().zip(values) {
            let d = cell_height(&marked_paragraph(tail, size, ""), &styles, "");
            assert_eq!(first_table(&d).rows[0].height.unwrap().as_i32(), expected);
        }
    }
}
// 이것을 실패시키는 것: 본문 최소에서 끝 표시 제외와 함께 원래 테두리 예산까지 제거하거나 exact 행을 확장.
#[test]
fn mark_exclusion_keeps_original_border_and_exact_row_budgets() {
    let p = marked_paragraph("", 40, "");
    let styles = mark_styles("Cambria");
    for (row,borders,expected) in [("","",1290),("","<w:tblBorders><w:top w:val='single' w:sz='4'/><w:bottom w:val='single' w:sz='4'/></w:tblBorders>",1390),("<w:trHeight w:val='400' w:hRule='exact'/>","",2000)] {
        let body=format!("<w:tbl><w:tblPr>{borders}<w:tblCellMar><w:top w:w='0'/><w:bottom w:w='0'/></w:tblCellMar></w:tblPr><w:tr><w:trPr>{row}</w:trPr><w:tc>{p}</w:tc></w:tr></w:tbl>");let d=decode_docx(&package(&body,&[("styles","styles.xml",&styles)])).unwrap();assert_eq!(first_table(&d).rows[0].height.unwrap().as_i32(),expected);
    }
}
