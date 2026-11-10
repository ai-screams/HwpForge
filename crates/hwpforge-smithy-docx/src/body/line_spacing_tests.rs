use super::tests::{package, W};
use crate::{decode_docx, DocxDecoded};
use hwpforge_blueprint::style::ParaShape;
use hwpforge_foundation::LineSpacingType;

fn read(styles: &str, body: &str) -> DocxDecoded {
    let styles = format!("<w:styles xmlns:w='{W}'>{styles}</w:styles>");
    decode_docx(&package(body, &[("styles", "styles.xml", &styles)])).unwrap()
}
fn defaults(font: &str, size: u32) -> String {
    format!("<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii='{font}' w:hAnsi='{font}' w:eastAsia='{font}'/><w:sz w:val='{size}'/></w:rPr></w:rPrDefault></w:docDefaults>")
}
fn shapes(d: &DocxDecoded) -> Vec<&ParaShape> {
    d.document
        .sections()
        .iter()
        .flat_map(|s| &s.paragraphs)
        .map(|p| &d.registry.para_shapes[p.para_shape_id.get()])
        .collect()
}
fn loss(d: &DocxDecoded, name: &str) -> u64 {
    d.warnings.iter().filter(|w| w.loss() == name).map(|w| w.count()).sum()
}

// 이것을 실패시키는 것: auto 배수만 복사, 바탕 lineGap 포함, 물리 간격의 ×2 누락, 최소 자연 높이 누락.
#[test]
fn measured_line_height_contracts() {
    for (font, size, rule, line, kind, value) in [
        ("맑은 고딕", 22, "auto", 276, LineSpacingType::Percentage, 199.0),
        ("Malgun Gothic", 20, "auto", 240, LineSpacingType::Percentage, 173.0),
        ("바탕", 22, "auto", 360, LineSpacingType::Percentage, 195.0),
        ("굴림", 24, "exact", 400, LineSpacingType::Fixed, 4000.0),
        ("맑은 고딕", 22, "atLeast", 360, LineSpacingType::AtLeast, 3804.0),
        ("맑은 고딕", 22, "atLeast", 600, LineSpacingType::AtLeast, 6000.0),
    ] {
        let d = read(&defaults(font, size), &format!("<w:p><w:pPr><w:spacing w:line='{line}' w:lineRule='{rule}'/></w:pPr><w:r><w:t>한글</w:t></w:r></w:p>"));
        let s = shapes(&d)[0];
        assert_eq!(
            (
                s.line_spacing_type,
                if kind == LineSpacingType::Percentage {
                    s.line_spacing_value.round()
                } else {
                    s.line_spacing_value
                }
            ),
            (kind, value),
            "{font} {rule}"
        );
    }
}

// 이것을 실패시키는 것: 미지 글꼴 보정, run마다 경고, atLeast에서 근거 없는 높이 합성.
#[test]
fn unknown_metrics_warn_once_per_font_and_preserve_raw_values() {
    let p = "<w:p><w:pPr><w:spacing w:line='276'/></w:pPr><w:r><w:t>가</w:t></w:r><w:r><w:t>나</w:t></w:r></w:p>";
    let d = read(&defaults("Unknown", 22), &p.repeat(3));
    assert!(shapes(&d).iter().all(|s| s.line_spacing_value == 115.0));
    assert_eq!(loss(&d, "unknown_font_line_height"), 1);
    let d = read(&defaults("Unknown", 22), "<w:p><w:pPr><w:spacing w:line='360' w:lineRule='atLeast'/></w:pPr><w:r><w:t>가</w:t></w:r></w:p>");
    assert_eq!(shapes(&d)[0].line_spacing_value, 3600.0);
    assert_eq!(loss(&d, "unknown_font_line_height"), 1);
}

// 이것을 실패시키는 것: 라틴에도 1.3 적용, 사용하지 않는 동아시아 칸을 높이로 선택.
#[test]
fn script_selection_and_font_aliases() {
    for (font, text, value) in [
        ("Calibri", "abc", 122.0),
        ("Cambria", "abc", 117.0),
        ("HCR Batang", "한글", 169.0),
        ("함초롬돋움", "한글", 169.0),
        ("Batang", "漢字かなカナ", 130.0),
        ("Gulim", "한글", 130.0),
        ("함초롬바탕", "한글", 169.0),
        ("HCR Dotum", "한글", 169.0),
    ] {
        let d = read(&defaults(font, 22), &format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>"));
        assert_eq!(shapes(&d)[0].line_spacing_value.round(), value, "{font}");
    }
    let d = read("", "<w:p><w:r><w:rPr><w:rFonts w:ascii='Calibri' w:eastAsia='Unknown'/><w:sz w:val='22'/></w:rPr><w:t>abc</w:t></w:r></w:p>");
    assert_eq!(shapes(&d)[0].line_spacing_value.round(), 122.0);
    assert_eq!(loss(&d, "unknown_font_line_height"), 0);
}

// 이것을 실패시키는 것: 가장 큰 run만 선택, 기준 크기 합산, 혼합 경고 중복, 끝 서식 무시.
#[test]
fn mixed_runs_and_paragraph_mark_use_separate_extrema() {
    let d = read(&defaults("맑은 고딕", 22), "<w:p><w:r><w:t>가</w:t></w:r><w:r><w:rPr><w:sz w:val='40'/></w:rPr><w:t>나</w:t></w:r><w:r><w:t>다</w:t></w:r></w:p>");
    assert_eq!(shapes(&d)[0].line_spacing_value.round(), 173.0);
    assert_eq!(loss(&d, "mixed_font_line_height"), 1);
    let d = read(&defaults("Cambria", 22), "<w:p><w:pPr><w:rPr><w:rFonts w:ascii='Calibri' w:hAnsi='Calibri'/><w:sz w:val='40'/></w:rPr></w:pPr><w:r><w:t>abc</w:t></w:r></w:p>");
    assert_eq!(shapes(&d)[0].line_spacing_value.round(), 122.0);
    assert_eq!(loss(&d, "mixed_font_line_height"), 1);
    let d = read(&defaults("Cambria", 22), "<w:p><w:r><w:t>a</w:t></w:r><w:r><w:rPr><w:rFonts w:ascii='맑은 고딕' w:hAnsi='맑은 고딕'/></w:rPr><w:t>b</w:t></w:r><w:r><w:rPr><w:rFonts w:ascii='Calibri' w:hAnsi='Calibri'/></w:rPr><w:t>c</w:t></w:r></w:p>");
    assert_eq!(shapes(&d)[0].line_spacing_value.round(), 136.0);
    assert_eq!(loss(&d, "mixed_font_line_height"), 1);
}

// 이것을 실패시키는 것: named style 보정값 재사용 후 재보정, run 문자 스타일·직접 크기 무시.
#[test]
fn inherited_spacing_is_projected_once_after_run_resolution() {
    let styles = format!("{}<w:style w:type='paragraph' w:styleId='p'><w:pPr><w:spacing w:line='276'/></w:pPr></w:style><w:style w:type='character' w:styleId='c'><w:rPr><w:rFonts w:ascii='바탕' w:hAnsi='바탕' w:eastAsia='바탕'/></w:rPr></w:style>", defaults("맑은 고딕", 22));
    let d = read(&styles, "<w:p><w:pPr><w:pStyle w:val='p'/></w:pPr><w:r><w:t>한글</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val='p'/></w:pPr><w:r><w:rPr><w:rStyle w:val='c'/><w:sz w:val='40'/></w:rPr><w:t>한글</w:t></w:r></w:p>");
    assert_eq!(
        shapes(&d).iter().map(|s| s.line_spacing_value.round()).collect::<Vec<_>>(),
        [199.0, 150.0]
    );
    let named = d.registry.style_entries().values().next().unwrap();
    assert_eq!(d.registry.para_shapes[named.para_shape_id.get()].line_spacing_value.round(), 199.0);
}

fn contextual_styles() -> String {
    format!("{}<w:style w:type='paragraph' w:default='1' w:styleId='p'><w:pPr><w:contextualSpacing/><w:spacing w:before='40' w:after='160'/></w:pPr></w:style><w:style w:type='paragraph' w:styleId='q'><w:basedOn w:val='p'/></w:style>", defaults("맑은 고딕", 22))
}
fn gaps(d: &DocxDecoded) -> Vec<(i32, i32)> {
    shapes(d).iter().map(|s| (s.space_before.as_i32(), s.space_after.as_i32())).collect()
}

// 이것을 실패시키는 것: 일반 경계 생략, max 대신 합/min/한쪽 값 사용,
// 양쪽에 R 배치, 다른 스타일 제외, 다른 스타일의 contextual까지 차감.
#[test]
fn ordinary_boundaries_match_word_literal_groups() {
    for (group, after, before, expected) in [
        ("G2", 200, 240, 1200),
        ("G4", 240, 200, 1200),
        ("G5", 200, 0, 1000),
        ("G6", 0, 240, 1200),
        ("G1", 0, 0, 0),
    ] {
        for (style, contextual) in [("p", ""), ("q", ""), ("q", "<w:contextualSpacing/>")] {
            let styles = format!("{}<w:style w:type='paragraph' w:styleId='p'/><w:style w:type='paragraph' w:styleId='q'/>", defaults("맑은 고딕", 22));
            let d = read(&styles, &format!("<w:p><w:pPr><w:pStyle w:val='p'/><w:spacing w:before='60' w:after='{after}'/>{contextual}</w:pPr><w:r><w:t>가</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val='{style}'/><w:spacing w:before='{before}' w:after='80'/>{contextual}</w:pPr><w:r><w:t>나</w:t></w:r></w:p>"));
            let actual = gaps(&d);
            assert_eq!(actual, [(300, expected), (0, 400)], "{group} {style} {contextual}");
            assert_eq!(actual[0].1 + actual[1].0, expected);
            if expected != 0 {
                assert_ne!(actual[0].1 == 0, actual[1].0 == 0);
            }
        }
    }
}

// 이것을 실패시키는 것: 표 뒤 이웃 초기화 누락, 구역 경계를 건너 결합.
#[test]
fn ordinary_boundaries_stop_at_tables_and_sections() {
    for separator in [
        "</w:pPr></w:p><w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>",
        "<w:sectPr/></w:pPr></w:p>",
    ] {
        let d = read("", &format!("<w:p><w:pPr><w:spacing w:before='60' w:after='200'/>{separator}<w:p><w:pPr><w:spacing w:before='240' w:after='80'/></w:pPr></w:p><w:p><w:pPr><w:spacing w:before='100'/></w:pPr></w:p>"));
        let expected = if separator.contains("sectPr") {
            vec![(300, 1000), (0, 500), (0, 0)]
        } else {
            vec![(300, 1000), (0, 0), (1200, 500), (0, 0)]
        };
        assert_eq!(gaps(&d), expected);
    }
}

// 이것을 실패시키는 것: 분할 안쪽에 간격 복사, 마지막 조각 경계 생략,
// 끝 나누기 이동 전 page_break로 판정하거나 강제 경계의 before를 결합.
#[test]
fn ordinary_boundaries_keep_fragment_and_page_break_contracts() {
    for (runs, next_props, expected) in [
        ("<w:t>A</w:t><w:br w:type='page'/><w:t>B</w:t>", "", vec![(300, 0), (0, 1200), (0, 400)]),
        ("<w:t>A</w:t><w:br w:type='page'/>", "", vec![(300, 1000), (0, 400)]),
        ("<w:t>A</w:t>", "<w:pageBreakBefore/>", vec![(300, 1000), (0, 400)]),
    ] {
        let d = read("", &format!("<w:p><w:pPr><w:spacing w:before='60' w:after='200'/></w:pPr><w:r>{runs}</w:r></w:p><w:p><w:pPr><w:spacing w:before='240' w:after='80'/>{next_props}</w:pPr><w:r><w:t>C</w:t></w:r></w:p>"));
        assert_eq!(gaps(&d), expected);
        assert!(d.document.sections()[0].paragraphs[1].page_break);
    }
}

// 이것을 실패시키는 것: 같은 스타일 간격 유지, 다른 스타일까지 억제, false 무시, 한쪽 contextual 잔여 간격 미차감.
#[test]
fn contextual_spacing_respects_identity_and_each_side_switch() {
    let d = read(&contextual_styles(), "<w:p><w:r><w:t>가</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val='p'/></w:pPr><w:r><w:t>나</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val='q'/></w:pPr></w:p>");
    assert_eq!(gaps(&d), [(200, 0), (0, 800), (0, 800)]);
    assert_eq!(shapes(&d)[0].line_spacing_value.round(), 173.0);
    let d = read(
        &contextual_styles(),
        "<w:p/><w:p><w:pPr><w:contextualSpacing w:val='0'/></w:pPr></w:p><w:p/>",
    );
    assert_eq!(gaps(&d), [(200, 0), (0, 600), (0, 800)]);
}

// 이것을 실패시키는 것: 표·구역을 건너 억제, 분할 조각을 새 이웃으로 판단, 이동된 강제 경계에 contextual 적용.
#[test]
fn contextual_spacing_boundaries_and_split_fragments() {
    let d = read(&contextual_styles(), "<w:p/><w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl><w:p><w:pPr><w:sectPr/></w:pPr></w:p><w:p/>");
    assert_eq!(gaps(&d), [(200, 800), (0, 0), (200, 800), (0, 800)]);
    let d = read(
        &contextual_styles(),
        "<w:p/><w:p><w:r><w:t>가</w:t><w:br w:type='page'/><w:t>나</w:t></w:r></w:p><w:p/>",
    );
    assert_eq!(gaps(&d), [(200, 0), (0, 0), (0, 0), (0, 800)]);
    let d = read(
        &contextual_styles(),
        "<w:p/><w:p><w:r><w:t>가</w:t><w:br w:type='page'/></w:r></w:p><w:p/>",
    );
    assert_eq!(gaps(&d), [(200, 0), (0, 800), (0, 800)]);
}

// 이것을 실패시키는 것: 빈 run의 fallback으로 끝 서식 가리기, exact에서 미지 metric 경고, 누락 line의 auto 규칙 무시.
#[test]
fn empty_mark_exact_and_missing_line_edges() {
    let d = read("", "<w:p><w:pPr><w:rPr><w:rFonts w:ascii='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr></w:pPr></w:p>");
    assert_eq!(shapes(&d)[0].line_spacing_value.round(), 173.0);
    let d = read(&defaults("Unknown", 22), "<w:p><w:pPr><w:spacing w:line='400' w:lineRule='exact'/></w:pPr><w:r><w:t>한글</w:t></w:r></w:p>");
    assert_eq!(shapes(&d)[0].line_spacing_value, 4000.0);
    assert_eq!(loss(&d, "unknown_font_line_height"), 0);
    let d = read(&format!("{}<w:style w:type='paragraph' w:styleId='p'><w:pPr><w:spacing w:lineRule='exact'/></w:pPr></w:style>", defaults("맑은 고딕", 22)), "<w:p><w:pPr><w:pStyle w:val='p'/></w:pPr><w:r><w:t>한글</w:t></w:r></w:p>");
    let named = d.registry.style_entries().values().next().unwrap();
    let s = &d.registry.para_shapes[named.para_shape_id.get()];
    assert_eq!(
        (s.line_spacing_type, s.line_spacing_value.round()),
        (LineSpacingType::Percentage, 173.0)
    );
    assert_eq!(shapes(&d)[0].line_spacing_value.round(), 173.0);
}

// 이것을 실패시키는 것: 미지 글꼴을 문서당 하나로 합치기, 미지 metric만으로 혼합 경고, 원래 line=0을 기본값으로 대체.
#[test]
fn unknown_fonts_are_counted_by_id_and_zero_is_preserved() {
    let d = read(&defaults("Unknown", 22), "<w:p><w:r><w:t>가</w:t></w:r><w:r><w:rPr><w:rFonts w:eastAsia='Other'/></w:rPr><w:t>나</w:t></w:r></w:p>");
    assert_eq!(loss(&d, "unknown_font_line_height"), 2);
    assert_eq!(loss(&d, "mixed_font_line_height"), 0);
    assert_eq!(shapes(&d)[0].line_spacing_value, 100.0);
    let d = read(
        &defaults("맑은 고딕", 22),
        "<w:p><w:pPr><w:spacing w:line='0'/></w:pPr><w:r><w:t>가</w:t></w:r></w:p>",
    );
    assert_eq!(shapes(&d)[0].line_spacing_value, 0.0);
}

// 이것을 실패시키는 것: 동아시아 문자를 라틴 칸으로 처리, 한 run의 다른 문자권 높이 무시.
#[test]
fn selected_script_slots_both_contribute_to_height() {
    for (text, expected, mixed) in [("가", 173.0, 0), ("A가", 176.0, 1)] {
        let d = read("", &format!("<w:p><w:r><w:rPr><w:rFonts w:ascii='Calibri' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr><w:t>{text}</w:t></w:r></w:p>"));
        assert_eq!(shapes(&d)[0].line_spacing_value.round(), expected);
        assert_eq!(loss(&d, "mixed_font_line_height"), mixed);
    }
}

// 이것을 실패시키는 것: 명시하지 않은 끝 글꼴을 기본값으로 합성, 명시한 끝 서식까지 무시.
#[test]
fn absent_mark_follows_final_run_but_explicit_mark_is_inherited() {
    for (mark, expected, mixed) in [("", 195.0, 0), ("<w:rPr/>", 259.0, 0)] {
        let d = read(&defaults("맑은 고딕", 22), &format!("<w:p><w:pPr><w:spacing w:line='360'/>{mark}</w:pPr><w:r><w:rPr><w:rFonts w:ascii='바탕' w:hAnsi='바탕' w:eastAsia='바탕'/></w:rPr><w:t>한글</w:t></w:r></w:p>"));
        assert_eq!(shapes(&d)[0].line_spacing_value.round(), expected);
        assert_eq!(loss(&d, "mixed_font_line_height"), mixed);
    }
}

// 이것을 실패시키는 것: pageBreakBefore에서 contextual 결합, 자연 경계에서 명명하지 않은 기본 스타일 비교 제외.
#[test]
fn page_break_before_excludes_contextual_but_natural_implicit_style_still_matches() {
    let d = read(&contextual_styles(), "<w:p/><w:p><w:pPr><w:pageBreakBefore/></w:pPr></w:p>");
    assert_eq!(gaps(&d), [(200, 800), (0, 800)]);
    assert!(d.document.sections()[0].paragraphs[1].page_break);
    let p =
        "<w:p><w:pPr><w:contextualSpacing/><w:spacing w:before='40' w:after='160'/></w:pPr></w:p>";
    let d = read("", &p.repeat(2));
    assert_eq!(gaps(&d), [(200, 0), (0, 800)]);
}

// 이것을 실패시키는 것: 글꼴이 누락된 명시적 끝 서식을 무시하고 알려진 run만 환산.
#[test]
fn unresolved_explicit_mark_keeps_source_spacing_with_existing_font_warning() {
    for (rule, line, expected) in [("auto", 276, 115.0), ("atLeast", 360, 3600.0)] {
        let d = read("", &format!("<w:p><w:pPr><w:spacing w:line='{line}' w:lineRule='{rule}'/><w:rPr><w:sz w:val='40'/></w:rPr></w:pPr><w:r><w:rPr><w:rFonts w:ascii='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr><w:t>한글</w:t></w:r></w:p>"));
        assert_eq!(shapes(&d)[0].line_spacing_value, expected);
        assert_eq!(loss(&d, "font_family"), 1);
        assert_eq!(loss(&d, "unknown_font_line_height"), 0);
    }
}

// 이것을 실패시키는 것: 한쪽 contextual이면 상대 간격 그대로 유지, 합산 후 차감, 0 아래로 차감.
#[test]
fn contextual_one_sided_spacing_subtracts_from_collapsed_boundary() {
    for (after, before, left, right, expected) in [
        (200, 240, true, false, [(0, 0), (200, 0)]),
        (240, 200, false, true, [(0, 200), (0, 0)]),
        (240, 200, true, false, [(0, 0), (0, 0)]),
        (200, 240, false, true, [(0, 0), (0, 0)]),
        (200, 240, true, true, [(0, 0), (0, 0)]),
        (200, 240, false, false, [(0, 1200), (0, 0)]),
    ] {
        let d = read("", &format!("<w:p><w:pPr><w:spacing w:after='{after}'/><w:contextualSpacing w:val='{left}'/></w:pPr></w:p><w:p><w:pPr><w:spacing w:before='{before}'/><w:contextualSpacing w:val='{right}'/></w:pPr></w:p>"));
        assert_eq!(gaps(&d), expected, "{after} {before} {left} {right}");
    }
}

// 이것을 실패시키는 것: contextual 차감 잔여값을 raw before에 전달하지 않아 분할 때 원래 값 복원.
#[test]
fn contextual_residual_survives_page_break_fragment_derivation() {
    let d = read("<w:style w:type='paragraph' w:styleId='q'/>", "<w:p><w:pPr><w:spacing w:after='200'/><w:contextualSpacing/></w:pPr></w:p><w:p><w:pPr><w:spacing w:before='240'/></w:pPr><w:r><w:t>A</w:t><w:br w:type='page'/><w:t>B</w:t><w:br w:type='page'/></w:r></w:p><w:p><w:pPr><w:pStyle w:val='q'/></w:pPr></w:p>");
    assert_eq!(gaps(&d), [(0, 0), (200, 0), (0, 0), (0, 0)]);
}

// 이것을 실패시키는 것: 내용 없는 run/합성 경계의 eastAsia 칸으로 라틴 높이에 1.3 적용.
#[test]
fn empty_runs_and_moved_break_do_not_change_latin_height() {
    let styles = defaults("Calibri", 22);
    for runs in [
        "<w:r><w:t>abc</w:t></w:r>",
        "<w:r/><w:r><w:t>abc</w:t></w:r>",
        "<w:r><w:rPr><w:rFonts w:ascii='Unknown' w:eastAsia='Unknown'/><w:sz w:val='40'/></w:rPr></w:r><w:r><w:t>abc</w:t></w:r>",
        "<w:r><w:t>abc</w:t><w:br w:type='page'/></w:r>",
    ] {
        let d = read(&styles, &format!("<w:p><w:pPr><w:spacing w:line='240'/></w:pPr>{runs}</w:p><w:p><w:r><w:t>next</w:t></w:r></w:p>"));
        assert_eq!(shapes(&d)[0].line_spacing_value, 122.0703125);
        assert_eq!(loss(&d, "mixed_font_line_height"), 0);
        assert_eq!(loss(&d, "unknown_font_line_height"), 0);
        assert_eq!(loss(&d, "paragraph_split"), 0);
    }
}

// 이것을 실패시키는 것: 크기가 같고 한 글꼴이 양쪽 최대치를 제공해도 혼합 경고.
#[test]
fn mixed_warning_requires_composite_extrema_or_different_sizes() {
    for (latin, expected, warnings) in [
        ("Cambria", 172.91015625, 0),
        ("Calibri", 176.4013671875, 1),
        ("맑은 고딕", 172.91015625, 0),
    ] {
        for text in ["A가", "가A"] {
            let d = read("", &format!("<w:p><w:r><w:rPr><w:rFonts w:ascii='{latin}' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr><w:t>{text}</w:t></w:r></w:p>"));
            assert_eq!(shapes(&d)[0].line_spacing_value, expected);
            assert_eq!(loss(&d, "mixed_font_line_height"), warnings);
        }
    }
}

// 이것을 실패시키는 것: 스타일 정의의 빈 문단 정책을 본문 라틴 글자 판정에 재사용.
#[test]
fn named_style_uses_empty_mark_policy_but_body_uses_actual_script() {
    let styles = format!("{}<w:style w:type='paragraph' w:styleId='s'/>", defaults("Calibri", 22));
    let d = read(&styles, "<w:p><w:pPr><w:pStyle w:val='s'/></w:pPr><w:r><w:t>abc</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val='s'/></w:pPr></w:p>");
    let named = d.registry.style_entries().values().next().unwrap();
    assert_eq!(d.registry.para_shapes[named.para_shape_id.get()].line_spacing_value, 158.69140625);
    assert_eq!(
        shapes(&d).iter().map(|s| s.line_spacing_value).collect::<Vec<_>>(),
        [122.0703125, 158.69140625]
    );
}

// 이것을 실패시키는 것: 동일 TTC metric이 확인된 자매 face를 미지 글꼴로 처리.
#[test]
fn sister_faces_share_verified_natural_height() {
    for font in [
        "돋움",
        "Dotum",
        "돋움체",
        "DotumChe",
        "굴림체",
        "GulimChe",
        "바탕체",
        "BatangChe",
        "궁서",
        "Gungsuh",
        "궁서체",
        "GungsuhChe",
    ] {
        let d = read(&defaults(font, 22), "<w:p><w:r><w:t>한글</w:t></w:r></w:p>");
        assert_eq!(shapes(&d)[0].line_spacing_value, 130.0, "{font}");
        assert_eq!(loss(&d, "unknown_font_line_height"), 0, "{font}");
    }
}

// 이것을 실패시키는 것: 분할 원형·일반/contextual 경계·끝 조각 복구를 확정 전에 intern.
#[test]
fn only_final_output_shapes_are_interned() {
    let props = "<w:keepLines/><w:widowControl/><w:spacing w:before='40' w:after='160'/><w:ind w:firstLine='60'/>";
    let p = |extra: &str, text: &str| {
        format!("<w:p><w:pPr>{props}{extra}</w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>")
    };
    let split = "A</w:t><w:br w:type='page'/><w:t>B";
    let tail = "A</w:t><w:br w:type='page'/><w:t>";
    for (body, count, expected) in [
        (p("", "A").repeat(3), 2, vec![(200, 800), (0, 800), (0, 800)]),
        (format!("{}{}", p("", tail), p("", "B")), 2, vec![(200, 800), (0, 800)]),
        (p("", split), 2, vec![(200, 0), (0, 800)]),
        (p("", tail), 2, vec![(200, 0), (0, 800)]),
        (format!("{}<w:p/>", p("", tail)), 2, vec![(200, 800), (0, 0)]),
        (
            format!(
                "{}<w:p/>",
                p("", "A</w:t><w:br w:type='page'/><w:t>B</w:t><w:br w:type='page'/><w:t>")
            ),
            3,
            vec![(200, 0), (0, 800), (0, 0)],
        ),
        (p("<w:contextualSpacing/>", "A").repeat(3), 3, vec![(200, 0), (0, 0), (0, 800)]),
        (
            format!("{}{}", p("<w:contextualSpacing/>", "A"), p("", "B")),
            2,
            vec![(200, 0), (0, 800)],
        ),
        (
            format!("{}{}", p("", "A"), p("<w:contextualSpacing/>", "B")),
            2,
            vec![(200, 600), (0, 800)],
        ),
        (
            format!("{}{}", p("<w:contextualSpacing/>", tail), p("<w:contextualSpacing/>", "B")),
            2,
            vec![(200, 800), (0, 800)],
        ),
        (format!("{}<w:p/>", p("<w:sectPr/>", tail)), 3, vec![(200, 0), (0, 800), (0, 0)]),
    ] {
        let d = read(&defaults("Calibri", 22), &body);
        assert_eq!(gaps(&d), expected, "{body}");
        let used: std::collections::HashSet<_> = d
            .document
            .sections()
            .iter()
            .flat_map(|s| &s.paragraphs)
            .map(|p| p.para_shape_id.get())
            .collect();
        assert_eq!(used.len(), count, "{body}");
        assert_eq!(d.registry.para_shapes.len(), count, "{body}");
    }
}

// 이것을 실패시키는 것: 빈 문단의 앞 빈 run을 문단 끝 서식보다 우선하거나 높이에 합산.
#[test]
fn empty_paragraph_uses_only_final_mark_formatting() {
    let d = read(&defaults("Calibri", 22), "<w:p><w:r><w:rPr><w:rFonts w:eastAsia='Unknown'/><w:sz w:val='40'/></w:rPr></w:r><w:r/></w:p>");
    assert_eq!(shapes(&d)[0].line_spacing_value, 158.69140625);
    assert_eq!(loss(&d, "unknown_font_line_height"), 0);
    assert_eq!(loss(&d, "mixed_font_line_height"), 0);
}

// 이것을 실패시키는 것: 최종 page_break 무시, column 이동 누락, contextual을 강제 경계에 적용,
// suppressSpBfAfterPgBrk 꺼짐에서 before 보존, 자연 경계까지 억제.
#[test]
fn forced_boundaries_drop_before_independently_of_settings() {
    for (setting, before) in [
        ("", 0),
        ("<w:suppressSpBfAfterPgBrk/>", 0),
        ("<w:suppressSpBfAfterPgBrk w:val='1'/>", 0),
        ("<w:suppressSpBfAfterPgBrk w:val='true'/>", 0),
        ("<w:suppressSpBfAfterPgBrk w:val='on'/>", 0),
        ("<w:suppressSpBfAfterPgBrk w:val='0'/>", 0),
        ("<w:suppressSpBfAfterPgBrk w:val='false'/>", 0),
        ("<w:suppressSpBfAfterPgBrk w:val='off'/>", 0),
        ("<w:suppressSpBfAfterPgBrk/><w:suppressSpBfAfterPgBrk w:val='false'/>", 0),
    ] {
        let settings =
            format!("<w:settings xmlns:w='{W}'><w:compat>{setting}</w:compat></w:settings>");
        for (tail, next) in [
            ("", "<w:pageBreakBefore/>"),
            ("<w:br w:type='page'/>", "<w:pageBreakBefore w:val='0'/>"),
            ("<w:br w:type='column'/>", ""),
        ] {
            for (left, right) in [
                ("", ""),
                ("<w:contextualSpacing/>", ""),
                ("", "<w:contextualSpacing/>"),
                ("<w:contextualSpacing/>", "<w:contextualSpacing/>"),
            ] {
                let body = format!("<w:p><w:pPr><w:spacing w:after='200'/>{left}</w:pPr><w:r><w:t>A</w:t>{tail}</w:r></w:p><w:p><w:pPr><w:spacing w:before='240' w:after='80'/>{right}{next}</w:pPr><w:r><w:t>B</w:t></w:r></w:p><w:p><w:pPr><w:spacing w:before='100'/></w:pPr></w:p>");
                let d = decode_docx(&package(&body, &[("settings", "settings.xml", &settings)]))
                    .unwrap();
                // Only the forced boundary is excluded; the next natural boundary still collapses.
                let last_gap = if right.is_empty() { (500, 0) } else { (0, 100) };
                assert_eq!(
                    gaps(&d),
                    [(0, 1000), (before, last_gap.0), (last_gap.1, 0)],
                    "{setting} {tail} {next} {left} {right}"
                );
                assert!(d.document.sections()[0].paragraphs[1].page_break);
                assert_eq!(loss(&d, "document_settings"), 0);
            }
        }
        let d = decode_docx(&package("<w:p><w:pPr><w:spacing w:after='200'/></w:pPr></w:p><w:p><w:pPr><w:spacing w:before='240'/><w:pageBreakBefore w:val='0'/></w:pPr></w:p>", &[("settings", "settings.xml", &settings)])).unwrap();
        assert_eq!(gaps(&d), [(0, 1200), (0, 0)]);
        let d = decode_docx(&package(
            "<w:p><w:pPr><w:spacing w:before='240'/><w:pageBreakBefore/></w:pPr></w:p>",
            &[("settings", "settings.xml", &settings)],
        ))
        .unwrap();
        assert_eq!(gaps(&d), [(before, 0)]);
    }
}

// 이것을 실패시키는 것: settings/root/compat/namespace 경고 범위 변경,
// 잘못된 boolean 경고 제거, 나머지 compat 손실 경고 제거.
#[test]
fn suppress_before_setting_validation_is_scoped_and_other_settings_still_warn() {
    for (kind, root, children, before, omitted, invalid) in [
        ("settings", "settings", "<w:suppressSpBfAfterPgBrk/>", 0, 1, 0),
        ("settings", "settings", "<x:compat><w:suppressSpBfAfterPgBrk/></x:compat>", 0, 1, 0),
        ("settings", "settings", "<w:compat><x:suppressSpBfAfterPgBrk/></w:compat>", 0, 1, 0),
        (
            "settings",
            "settings",
            "<w:compat><w:other><w:suppressSpBfAfterPgBrk/></w:other></w:compat>",
            0,
            1,
            0,
        ),
        ("settings", "wrong", "<w:compat><w:suppressSpBfAfterPgBrk/></w:compat>", 0, 0, 0),
        ("header", "hdr", "<w:compat><w:suppressSpBfAfterPgBrk/></w:compat>", 0, 0, 0),
        (
            "settings",
            "settings",
            "<w:compat><w:suppressSpBfAfterPgBrk w:val='bad'/></w:compat>",
            0,
            0,
            1,
        ),
        (
            "settings",
            "settings",
            "<w:compat><w:suppressSpBfAfterPgBrk/><w:suppressSpBfAfterPgBrk w:val='bad'/></w:compat>",
            0,
            0,
            1,
        ),
        (
            "settings",
            "settings",
            "<w:compat><w:suppressSpBfAfterPgBrk/><w:other/><w:another/></w:compat>",
            0,
            1,
            0,
        ),
    ] {
        let settings = format!("<w:{root} xmlns:w='{W}' xmlns:x='urn:other'>{children}</w:{root}>");
        let d = decode_docx(&package("<w:p><w:pPr><w:spacing w:after='200'/></w:pPr></w:p><w:p><w:pPr><w:spacing w:before='240'/><w:pageBreakBefore/></w:pPr></w:p>", &[(kind, "settings.xml", &settings)])).unwrap();
        assert_eq!(gaps(&d), [(0, 1000), (before, 0)], "{settings}");
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.code() == "DOCX_DOCUMENT_SETTINGS_OMITTED")
                .map(|w| w.count())
                .sum::<u64>(),
            omitted
        );
        assert_eq!(
            d.warnings.iter().filter(|w| w.code() == "DOCX_UNSUPPORTED_VALUE").count(),
            invalid
        );
    }
}

// 이것을 실패시키는 것: leading break 판정 누락/반전, 빈 run을 내용으로 취급,
// contextual 잔여값을 leading page break의 빈 조각에 기록, 끝 나누기 이동 때 before 복원,
// 확정 전 모양 intern, 비어 있지 않은 첫 조각의 before 삭제, 빈/본문 조각의 widow 초기화.
#[test]
fn leading_break_discards_before_on_every_fragment_before_interning() {
    for kind in ["page", "column"] {
        for prefix in ["", "<w:r/>", "<w:r><w:t></w:t></w:r>"] {
            for contextual in ["", "<w:contextualSpacing/>"] {
                for tail in ["", "<w:br w:type='page'/>"] {
                    let body = format!("<w:p><w:pPr><w:spacing w:after='200'/>{contextual}</w:pPr></w:p><w:p><w:pPr><w:spacing w:before='240' w:after='80'/>{contextual}</w:pPr>{prefix}<w:r><w:br w:type='{kind}'/><w:t>B</w:t>{tail}</w:r></w:p><w:p><w:pPr><w:pageBreakBefore/></w:pPr></w:p>");
                    let d = read("", &body);
                    assert_eq!(gaps(&d), [(0, 1000), (0, 0), (0, 400), (0, 0)], "{body}");
                    let used: std::collections::HashSet<_> = d.document.sections()[0]
                        .paragraphs
                        .iter()
                        .map(|p| p.para_shape_id)
                        .collect();
                    assert_eq!(used.len(), d.registry.para_shapes.len(), "{body}");
                    assert!(shapes(&d)[1].widow_orphan && shapes(&d)[2].widow_orphan);
                }
            }
        }
    }
    // 이것을 실패시키는 것: 첫 조각의 공백/일반 줄바꿈까지 빈 내용으로 간주.
    for content in ["<w:t xml:space='preserve'> </w:t>", "<w:br/>"] {
        let d = read("", &format!("<w:p><w:pPr><w:spacing w:before='240'/></w:pPr><w:r>{content}<w:br w:type='page'/><w:t>B</w:t></w:r></w:p>"));
        assert_eq!(gaps(&d), [(1200, 0), (0, 0)]);
    }
}
