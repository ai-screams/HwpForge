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
            [[1290, 1290], [2580, 3635], [3870, 4925]],
        ),
        (
            "맑은 고딕",
            ["", "</w:t><w:br/><w:t>", "</w:t><w:br/><w:br/><w:t>"],
            [[1290, 1290], [2580, 3635], [3870, 4925]],
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
        assert_eq!(t.rows[0].height.unwrap().as_i32(), 2580, "{kind}");
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
    assert_eq!(first_table(&d).rows[0].height.unwrap().as_i32(), 2345);
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

fn metric_run(font: &str, size: u32, content: &str) -> String {
    format!("<w:r><w:rPr><w:rFonts w:ascii='{font}' w:hAnsi='{font}' w:eastAsia='{font}'/><w:sz w:val='{size}'/></w:rPr>{content}</w:r>")
}
fn metric_paragraph(runs: &str, size: u32, spacing: &str) -> String {
    format!("<w:p><w:pPr>{spacing}<w:rPr><w:sz w:val='{size}'/></w:rPr></w:pPr>{runs}</w:p>")
}
fn metric_height(runs: &str, mark: u32, spacing: &str) -> i32 {
    first_table(&cell_height(&metric_paragraph(runs, mark, spacing), &mark_styles("맑은 고딕"), ""))
        .rows[0]
        .height
        .unwrap()
        .as_i32()
}
fn metric_loss(d: &crate::DocxDecoded, name: &str) -> u64 {
    d.warnings.iter().filter(|w| w.loss() == name).map(|w| w.count()).sum()
}

// 이것을 실패시키는 것: 글꼴별 확장 전에 극값 합성, 최대 단일 높이 선택, 기여별 반올림.
#[test]
fn logical_lines_combine_font_extrema() {
    for (left, size, right, right_size, text, expected) in [
        ("Cambria", 32, "맑은 고딕", 22, "가", 2006),
        ("Cambria", 40, "맑은 고딕", 18, "가", 2345),
        ("Calibri", 22, "맑은 고딕", 22, "가", 1902),
        ("Cambria", 32, "함초롬바탕", 22, "가", 1988),
    ] {
        let a = metric_run(left, size, "<w:t>A</w:t>");
        let b = metric_run(right, right_size, &format!("<w:t>{text}</w:t>"));
        for runs in [format!("{a}{b}"), format!("{b}{a}")] {
            assert_eq!(metric_height(&runs, 22, ""), expected, "{left}/{size}/{right}");
        }
    }
}

// 이것을 실패시키는 것: run 머리 빈 조각을 빈 줄로 측정하거나 run마다 줄 상태 초기화.
#[test]
fn line_break_run_boundaries_do_not_change_height() {
    for runs in [
        metric_run("Cambria", 22, "<w:t>A</w:t><w:br/>"),
        format!(
            "{}{}",
            metric_run("Cambria", 22, "<w:t>A</w:t>"),
            metric_run("Cambria", 60, "<w:br/>")
        ),
        format!(
            "{}{}{}",
            metric_run("Cambria", 60, ""),
            metric_run("Cambria", 22, "<w:t>A</w:t><w:br/>"),
            metric_run("Cambria", 60, "")
        ),
    ] {
        assert_eq!(metric_height(&runs, 40, ""), 3635, "{runs}");
    }
    for runs in [
        metric_run("Cambria", 22, "<w:t>A</w:t><w:br/><w:t>B</w:t>"),
        format!(
            "{}{}",
            metric_run("Cambria", 22, "<w:t>A</w:t>"),
            metric_run("Cambria", 22, "<w:br/><w:t>B</w:t>")
        ),
        format!(
            "{}{}{}",
            metric_run("Cambria", 22, "<w:t>A</w:t><w:br/>"),
            metric_run("Cambria", 60, ""),
            metric_run("Cambria", 22, "<w:t>B</w:t>")
        ),
    ] {
        assert_eq!(metric_height(&runs, 40, ""), 2580, "{runs}");
    }
}

// 이것을 실패시키는 것: 중간 빈 줄에 끝 표시 사용, 마지막 빈 줄에 줄바꿈 run 사용, 빈 마지막 줄 삭제.
#[test]
fn empty_lines_use_their_own_supplier() {
    let runs = format!(
        "{}{}",
        metric_run("Cambria", 22, "<w:t>A</w:t><w:br/>"),
        metric_run("Cambria", 40, "<w:br/>")
    );
    assert_eq!(metric_height(&runs, 22, ""), 4925);
}

// 이것을 실패시키는 것: 맑은 고딕 Latin 확장 삭제, Cambria/Calibri 확장, 미측정 한글 글꼴을 문자별로만 확장.
#[test]
fn malgun_latin_text_uses_expanded_metrics() {
    for (fonts, latin, east) in [
        (&["맑은 고딕", "Malgun Gothic"][..], 1902, 1902),
        (&["Cambria"][..], 1290, 1290),
        (&["Calibri"][..], 1343, 1343),
        (
            &[
                "바탕",
                "Batang",
                "바탕체",
                "BatangChe",
                "궁서",
                "Gungsuh",
                "궁서체",
                "GungsuhChe",
                "굴림",
                "Gulim",
                "굴림체",
                "GulimChe",
                "돋움",
                "Dotum",
                "돋움체",
                "DotumChe",
            ][..],
            1100,
            1430,
        ),
        (&["함초롬바탕", "HCR Batang", "함초롬돋움", "HCR Dotum"][..], 1430, 1859),
    ] {
        for font in fonts {
            for (text, expected) in [("A123", latin), ("가", east)] {
                assert_eq!(
                    metric_height(&metric_run(font, 22, &format!("<w:t>{text}</w:t>")), 40, ""),
                    expected,
                    "{font}/{text}"
                );
            }
        }
    }
    let a = metric_run("바탕", 22, "<w:t>가</w:t>");
    let b = metric_run("바탕", 40, "<w:t>123</w:t>");
    for runs in [format!("{a}{b}"), format!("{b}{a}")] {
        assert_eq!(metric_height(&runs, 22, ""), 2600);
    }
}

// 이것을 실패시키는 것: 실제 빈 줄에 East 칸 우선, ascii 우선, 모든 빈 줄에 1.3배.
#[test]
fn measured_empty_fonts_use_output_latin_slot() {
    for (font, expected) in
        [("Cambria", 1290), ("Calibri", 1343), ("맑은 고딕", 1902), ("Malgun Gothic", 1902)]
    {
        for hint in ["", "<w:lang w:val='en-US' w:eastAsia='ko-KR'/>"] {
            let p = format!("<w:p><w:pPr><w:rPr><w:rFonts w:ascii='Calibri' w:hAnsi='{font}' w:eastAsia='맑은 고딕' w:hint='eastAsia'/><w:sz w:val='22'/>{hint}</w:rPr></w:pPr><w:r/></w:p>");
            let d = cell_height(&p, &mark_styles("맑은 고딕"), "");
            assert_eq!(first_table(&d).rows[0].height.unwrap().as_i32(), expected, "{font}/{hint}");
            let p = &first_table(&d).rows[0].cells[0].paragraphs[0];
            let shape = &d.registry.char_shapes[p.end_char_shape.unwrap().get()];
            assert_eq!(shape.font_latin.as_deref().unwrap(), font);
        }
    }
}

// 이것을 실패시키는 것: 미측정 빈 줄의 East 선택 삭제, East 부재에도 배율 적용, 미등록 대체값 상향.
#[test]
fn unmeasured_empty_fonts_keep_slot_and_fallback() {
    for (latin, east, expected) in [
        ("바탕", None, 1100),
        ("바탕", Some("바탕"), 1430),
        ("HCR Batang", None, 1430),
        ("HCR Batang", Some("HCR Batang"), 1859),
        ("Arial", Some("맑은 고딕"), 1902),
        ("Unknown", Some("Unknown"), 1100),
    ] {
        let east = east.map_or(String::new(), |s| format!(" w:eastAsia='{s}'"));
        let styles = format!("<w:styles xmlns:w='{W}'><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii='{latin}' w:hAnsi='{latin}'{east}/><w:sz w:val='22'/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>");
        assert_eq!(
            first_table(&cell_height("<w:p/>", &styles, "")).rows[0].height.unwrap().as_i32(),
            expected,
            "{latin}/{east}"
        );
    }
}

// 이것을 실패시키는 것: unknown에 알려진 부분 높이 가산, 매 줄 경고, exact의 자연 메트릭 조회.
#[test]
fn unknown_line_metrics_keep_existing_fallback() {
    let runs = format!(
        "{}{}",
        metric_run("Unknown", 22, "<w:t>A</w:t>"),
        metric_run("맑은 고딕", 40, "<w:t>가</w:t>")
    );
    for (spacing, expected, warnings) in [
        ("", 2000, 1),
        ("<w:spacing w:line='160' w:lineRule='atLeast'/>", 800, 1),
        ("<w:spacing w:line='160' w:lineRule='exact'/>", 800, 0),
    ] {
        let d = cell_height(&metric_paragraph(&runs, 22, spacing), &mark_styles("맑은 고딕"), "");
        assert_eq!(first_table(&d).rows[0].height.unwrap().as_i32(), expected);
        assert_eq!(metric_loss(&d, "unknown_font_line_height"), warnings);
    }
    let d = cell_height(
        &metric_paragraph(&metric_run("Unknown", 22, "<w:t>A</w:t><w:br/><w:t>B</w:t>"), 22, ""),
        &mark_styles("맑은 고딕"),
        "",
    );
    assert_eq!(metric_loss(&d, "unknown_font_line_height"), 1);
}

// 이것을 실패시키는 것: 공백/탭을 진짜 빈 줄로 승격하거나 보수적 끝 표시 기여 삭제.
#[test]
fn whitespace_tails_keep_conservative_source_mark() {
    for text in ["<w:t xml:space='preserve'> </w:t>", "<w:tab/>"] {
        assert_eq!(metric_height(&metric_run("Cambria", 22, text), 40, ""), 2345);
    }
}

// 이것을 실패시키는 것: 첫 fragment에서 뒤 끝 표시를 빌리거나 선두 빈 fragment의 break 서식 무시.
#[test]
fn fragment_height_uses_only_retained_run_suppliers() {
    for kind in ["page", "column"] {
        let a = metric_run("Cambria", 22, "<w:t>A</w:t><w:br/>");
        let boundary = metric_run("Cambria", 40, &format!("<w:br w:type='{kind}'/>"));
        let b = metric_run("Cambria", 22, "<w:t>B</w:t><w:br/>");
        let d = cell_height(
            &metric_paragraph(&format!("{a}{boundary}{b}"), 40, ""),
            &mark_styles("Cambria"),
            "",
        );
        assert_eq!(first_table(&d).rows[0].height.unwrap().as_i32(), 2580, "{kind}");
        assert_eq!(first_table(&d).rows[0].cells[0].paragraphs[0].end_char_shape, None);
        assert_eq!(metric_height(&format!("{boundary}{b}"), 22, ""), 2345, "{kind}");
    }
}

// 이것을 실패시키는 것: 줄별 clamp 대신 문단 전체 max 사용, 원본 분수 배수의 선반올림, 합계 한 번 반올림.
#[test]
fn cell_minimum_clamps_and_rounds_each_logical_line() {
    let runs = format!(
        "{}{}",
        metric_run("Cambria", 22, "<w:t>A</w:t><w:br/>"),
        metric_run("맑은 고딕", 22, "<w:t>가</w:t>")
    );
    for (spacing, expected) in [
        ("<w:spacing w:line='360' w:lineRule='atLeast'/>", 3702),
        ("<w:spacing w:line='400' w:lineRule='atLeast'/>", 4000),
        ("", 3192),
        ("<w:spacing w:line='160' w:lineRule='exact'/>", 1600),
    ] {
        assert_eq!(metric_height(&runs, 22, spacing), expected);
    }
    assert_eq!(
        metric_height(&metric_run("Cambria", 22, "<w:t>A</w:t><w:br/><w:br/>"), 40, ""),
        4925
    );
    assert_eq!(
        metric_height(&metric_run("Cambria", 22, "<w:t>A</w:t><w:br/><w:br/>"), 22, ""),
        3870
    );
    for (spacing, expected) in [
        ("<w:spacing w:line='259'/>", 1392),
        ("<w:spacing w:line='259' w:lineRule='exact'/>", 1295),
        ("<w:spacing w:line='0'/>", 0),
    ] {
        assert_eq!(
            metric_height(&metric_run("Cambria", 22, "<w:t>A</w:t>"), 22, spacing),
            expected
        );
    }
}

// 이것을 실패시키는 것: 생략 증거 삭제/문단 전체 확대, 선택되지 않은 분기나 코드 구간의 생략 전달.
#[test]
fn omitted_content_keeps_conservative_height_on_its_own_line() {
    for object in ["<w:drawing/>", "<w:footnoteReference w:id='1'/>", "<w:endnoteReference w:id='1'/>", "<w:sym w:font='Symbol' w:char='F020'/>", "<w:object/>", "<w:commentReference w:id='1'/>", "<w:unknown/>", "<m:oMath xmlns:m='http://schemas.openxmlformats.org/officeDocument/2006/math'/>", "<x:thing xmlns:x='urn:unknown'/>", "<mc:AlternateContent xmlns:mc='http://schemas.openxmlformats.org/markup-compatibility/2006'><mc:Choice Requires='x'><w:drawing/></mc:Choice></mc:AlternateContent>", "<w:br w:type='unknown'/>", "visible-unplaced"] {
        for (content, expected) in [(object.to_owned(), 1902), (format!("{object}<w:br/>"), 3192), (format!("<w:br/>{object}"), 3192), (format!("<w:br/>{object}<w:br/>"), 4482)] {
            for runs in [format!("<w:r>{content}</w:r>"), format!("<w:r>{}</w:r>", content.replace("<w:br/>", "</w:r><w:r><w:br/></w:r><w:r>"))] {
                assert_eq!(metric_height(&runs, 22, ""), expected, "{runs}");
            }
        }
    }
    assert_eq!(metric_height("<w:r/>", 22, ""), 1290);
    assert_eq!(metric_height("<w:r><w:t>A</w:t><w:drawing/></w:r>", 40, ""), 1290);
    let fallback = "<mc:AlternateContent xmlns:mc='http://schemas.openxmlformats.org/markup-compatibility/2006'><mc:Choice Requires='x'><w:r><w:drawing/></w:r></mc:Choice><mc:Fallback><w:r><w:br/></w:r></mc:Fallback><mc:Fallback><w:r><w:drawing/></w:r></mc:Fallback></mc:AlternateContent>";
    assert_eq!(metric_height(fallback, 22, ""), 2580);
    for kind in ["page", "column"] {
        assert_eq!(
            metric_height(&format!("<w:r><w:br w:type='{kind}'/><w:drawing/></w:r>"), 22, ""),
            1290
        );
        assert_eq!(
            metric_height(&format!("<w:r><w:drawing/><w:br w:type='{kind}'/></w:r>"), 22, ""),
            1902
        );
    }
    let p = format!(
        "{}{}",
        metric_paragraph("<w:r><w:drawing/></w:r>", 22, ""),
        metric_paragraph("<w:r/>", 22, "")
    );
    assert_eq!(
        first_table(&cell_height(&p, &mark_styles("맑은 고딕"), "")).rows[0]
            .height
            .unwrap()
            .as_i32(),
        3192
    );
}

// 이것을 실패시키는 것: 비가시 필드 코드의 생략 전달, 저장 결과의 생략 누락, 결과 없는 필드를 진짜 빈 줄로 처리.
#[test]
fn field_visibility_controls_omission_evidence() {
    for (content, expected) in [("<w:fldChar w:fldCharType='begin'/><w:drawing/><w:br/><w:fldChar w:fldCharType='separate'/><w:t>A</w:t><w:fldChar w:fldCharType='end'/>", 1290), ("<w:fldChar w:fldCharType='begin'/><w:drawing/><w:fldChar w:fldCharType='separate'/><w:br/><w:fldChar w:fldCharType='end'/>", 2580), ("<w:fldChar w:fldCharType='begin'/><w:fldChar w:fldCharType='separate'/><w:drawing/><w:fldChar w:fldCharType='end'/>", 1902), ("<w:fldChar w:fldCharType='begin'/><w:drawing/><w:fldChar w:fldCharType='end'/>", 1902)] {
        assert_eq!(metric_height(&format!("<w:r>{content}</w:r>"), 22, ""), expected, "{content}");
    }
}

// 이것을 실패시키는 것: 모든 fragment 경계 전달 누락, 문단 전체 mixed 판정 유지, 칸 밖 측정 삭제.
#[test]
fn mixed_diagnostics_follow_logical_and_fragment_boundaries() {
    let a = metric_run("Cambria", 22, "<w:t>A</w:t>");
    let b = metric_run("Cambria", 32, "<w:t>B</w:t>");
    let c = metric_run("Cambria", 40, "<w:t>C</w:t>");
    for (separator, expected) in [
        ("", 1),
        ("<w:r><w:br/></w:r>", 0),
        ("<w:r><w:br w:type='page'/></w:r>", 0),
        ("<w:r><w:br w:type='column'/></w:r>", 0),
    ] {
        let p = metric_paragraph(
            &format!("{a}{separator}{b}{separator}{c}"),
            22,
            "<w:spacing w:line='160' w:lineRule='atLeast'/>",
        );
        let d = decode_docx(&package(&p, &[("styles", "styles.xml", &mark_styles("Cambria"))]))
            .unwrap();
        assert_eq!(metric_loss(&d, "mixed_font_line_height"), expected, "{separator}");
        for section in d.document.sections() {
            for p in &section.paragraphs {
                assert_eq!(
                    d.registry.para_shapes[p.para_shape_id.get()].line_spacing_value,
                    1600.0
                );
            }
        }
    }
}

// 이것을 실패시키는 것: 칸 밖 진단 조회 삭제, 문단별로 unknown 경고 반복, 크기 차이만으로 mixed 판정.
#[test]
fn minimum_diagnostics_measure_only_logical_contributors() {
    let unknown = metric_run("Unknown", 22, "<w:t>A</w:t><w:br/><w:t>B</w:t>");
    let composite = format!(
        "{}{}",
        metric_run("HCR Batang", 22, "<w:t>A</w:t>"),
        metric_run("Calibri", 22, "<w:t>B</w:t>")
    );
    for (runs, mixed, unknown_count) in [(&unknown, 0, 1), (&composite, 1, 0)] {
        for (spacing, expected_mixed, expected_unknown) in [
            ("<w:spacing w:line='160' w:lineRule='atLeast'/>", mixed, unknown_count),
            ("<w:spacing w:line='160' w:lineRule='exact'/>", 0, 0),
            ("<w:spacing w:line='240'/>", 0, 0),
        ] {
            let p = metric_paragraph(runs, 40, spacing);
            let d = decode_docx(&package(
                &p.repeat(2),
                &[("styles", "styles.xml", &mark_styles("Cambria"))],
            ))
            .unwrap();
            assert_eq!(metric_loss(&d, "mixed_font_line_height"), expected_mixed * 2);
            assert_eq!(metric_loss(&d, "unknown_font_line_height"), expected_unknown);
        }
    }
}

// 이것을 실패시키는 것: 생략 불확실성을 문단 전체로 확장하거나 생략만 든 줄을 진짜 빈 줄로 처리.
#[test]
fn omitted_middle_line_does_not_change_adjacent_empty_lines() {
    assert_eq!(metric_height("<w:r><w:br/><w:drawing/><w:br/></w:r>", 22, ""), 4482);
    assert_eq!(metric_height("<w:r><w:drawing/></w:r>", 22, "<w:spacing w:line='276'/>"), 2187);
    assert_eq!(metric_height("<w:r/>", 22, "<w:spacing w:line='276'/>"), 1483);
}
