use super::tests::{package, W};
use crate::{decode_docx, DocxDecoded};
use hwpforge_foundation::{Color, LineSpacingType};

fn read(styles: &str, body: &str) -> DocxDecoded {
    let styles = format!("<w:styles xmlns:w='{W}'>{styles}</w:styles>");
    decode_docx(&package(body, &[("styles", "styles.xml", &styles)])).unwrap()
}
fn shape(d: &DocxDecoded, run: usize) -> &hwpforge_blueprint::style::CharShape {
    let p = &d.document.sections()[0].paragraphs[0];
    &d.registry.char_shapes[p.runs[run].char_shape_id.get()]
}

// 이것을 실패시키는 것: 상속 체인에서 XOR하거나 종류 사이에서 덮어쓰기.
#[test]
fn inherited_toggles_and_direct_absolute_values() {
    let styles = "<w:style w:type='paragraph' w:styleId='p'><w:basedOn w:val='base'/><w:rPr><w:b/></w:rPr></w:style><w:style w:type='paragraph' w:styleId='base'><w:rPr><w:b/></w:rPr></w:style><w:style w:type='character' w:styleId='c'><w:rPr><w:b/></w:rPr></w:style>";
    let d = read(styles, "<w:p><w:pPr><w:pStyle w:val='p'/></w:pPr><w:r><w:t>A</w:t></w:r><w:r><w:rPr><w:rStyle w:val='c'/></w:rPr><w:t>B</w:t></w:r><w:r><w:rPr><w:rStyle w:val='c'/><w:b/></w:rPr><w:t>C</w:t></w:r><w:r><w:rPr><w:b w:val='0'/></w:rPr><w:t>D</w:t></w:r></w:p>");
    assert_eq!((0..4).map(|i| shape(&d, i).bold).collect::<Vec<_>>(), [true, false, true, false]);
}

// 이것을 실패시키는 것: 표시 이름으로 identity를 합치거나 직접 서식을 named style로 등록.
#[test]
fn names_keep_identity_and_direct_shapes_are_anonymous() {
    let styles = "<w:style w:type='paragraph' w:styleId='z'><w:name w:val='제목 1'/></w:style><w:style w:type='paragraph' w:styleId='a'><w:name w:val='제목 1'/></w:style>";
    let d = read(styles,"<w:p><w:pPr><w:pStyle w:val='z'/></w:pPr></w:p><w:p><w:pPr><w:pStyle w:val='a'/></w:pPr></w:p>");
    let p = &d.document.sections()[0].paragraphs;
    assert_ne!(p[0].style_ref, p[1].style_ref);
    assert_eq!(p[0].style_ref.unwrap().get(), 1);
    for p in p {
        assert_eq!(d.registry.style_display_name(p.style_ref.unwrap()), Some("제목 1"));
    }
    let d = read("", "<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>A</w:t></w:r></w:p>");
    assert_eq!(d.registry.style_count(), 0);
}

// 이것을 실패시키는 것: hAnsi 우선순위·BGR 변환·군별 크기 비교 제거.
#[test]
fn fonts_sizes_and_rgb_are_effective_values() {
    let d = read("", "<w:p><w:r><w:rPr><w:rFonts w:ascii='A' w:hAnsi='B' w:eastAsia='C' w:cs='D'/><w:sz w:val='20'/><w:szCs w:val='24'/><w:color w:val='112233'/></w:rPr><w:t>A</w:t></w:r></w:p>");
    let s = shape(&d, 0);
    assert_eq!((s.font_latin.as_deref(), s.font_east_asian.as_deref()), (Some("B"), Some("C")));
    assert_eq!(s.size.as_i32(), 1000);
    assert_eq!(s.color, Color::from_rgb(0x11, 0x22, 0x33));
    assert!(d.warnings.iter().any(|w| w.code() == "DOCX_SCRIPT_FORMAT_COLLAPSED"));
}

// 이것을 실패시키는 것: 241을 정수로 절삭하거나 길이형 줄 간격에 비율 계수 사용.
#[test]
fn paragraph_spacing_keeps_precision_and_types() {
    for (rule, value, kind) in [
        ("auto", 100.41666666666667, LineSpacingType::Percentage),
        ("exact", 1205.0, LineSpacingType::Fixed),
        ("atLeast", 1205.0, LineSpacingType::AtLeast),
    ] {
        let d=read("",&format!("<w:p><w:pPr><w:spacing w:line='241' w:lineRule='{rule}'/><w:ind w:left='360' w:firstLine='120' w:hanging='240'/></w:pPr></w:p>"));
        let p = &d.document.sections()[0].paragraphs[0];
        let s = &d.registry.para_shapes[p.para_shape_id.get()];
        assert_eq!(s.line_spacing_type, kind);
        assert_eq!(s.line_spacing_value, value);
        assert_eq!(s.indent_left.as_i32(), 1800);
        assert_eq!(s.indent_first_line.as_i32(), -1200);
    }
}

// 이것을 실패시키는 것: 선택 서식 숫자 오류를 전역 오류로 올리거나 상위 유효값 지우기.
#[test]
fn invalid_optional_numbers_retain_parent_values() {
    let d=read("<w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val='20'/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:line='360'/></w:pPr></w:pPrDefault></w:docDefaults>","<w:p><w:pPr><w:spacing w:line='oops'/></w:pPr><w:r><w:rPr><w:sz w:val='oops'/></w:rPr><w:t>A</w:t></w:r></w:p>");
    assert_eq!(shape(&d, 0).size.as_i32(), 1000);
    let p = &d.document.sections()[0].paragraphs[0];
    assert_eq!(d.registry.para_shapes[p.para_shape_id.get()].line_spacing_value, 150.0);
    assert_eq!(
        d.warnings
            .iter()
            .filter(|w| w.code() == "DOCX_UNSUPPORTED_VALUE")
            .map(|w| w.count())
            .sum::<u64>(),
        2
    );
}

// 이것을 실패시키는 것: 선택 서식 산술 overflow를 문서 전체 오류로 올리기.
#[test]
fn overflowing_optional_numbers_retain_parent_values() {
    let d=read("<w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val='20'/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:line='360'/></w:pPr></w:pPrDefault></w:docDefaults>","<w:p><w:pPr><w:spacing w:line='9999999999999999999999999999999999999999999999999999999999'/></w:pPr><w:r><w:rPr><w:sz w:val='9999999999999999999999999999999999999999999999999999999999'/></w:rPr><w:t>A</w:t></w:r></w:p>");
    assert_eq!(shape(&d, 0).size.as_i32(), 1000);
    let p = &d.document.sections()[0].paragraphs[0];
    assert_eq!(d.registry.para_shapes[p.para_shape_id.get()].line_spacing_value, 150.0);
    assert_eq!(
        d.warnings
            .iter()
            .filter(|w| w.code() == "DOCX_UNSUPPORTED_VALUE")
            .map(|w| w.count())
            .sum::<u64>(),
        2
    );
}

// 이것을 실패시키는 것: 부모 재지정·미지정·false의 구별 제거, 잘못된 연결 적용.
#[test]
fn inheritance_references_cycles_and_depth_are_bounded() {
    for (child, expected) in [("<w:b/>", true), ("", true), ("<w:b w:val='false'/>", false)] {
        let d=read(&format!("<w:style w:type='paragraph' w:styleId='parent'><w:rPr><w:b/></w:rPr></w:style><w:style w:type='paragraph' w:styleId='child'><w:basedOn w:val='parent'/><w:rPr>{child}</w:rPr></w:style>"),"<w:p><w:pPr><w:pStyle w:val='child'/></w:pPr></w:p>");
        assert_eq!(shape(&d, 0).bold, expected);
    }
    for parent in [
        "",
        "<w:style w:type='character' w:styleId='missing'><w:rPr><w:b/></w:rPr></w:style>",
        "<w:style w:type='paragraph' w:styleId='missing'><w:basedOn w:val='child'/></w:style>",
    ] {
        let d=read(&format!("{parent}<w:style w:type='paragraph' w:styleId='child'><w:basedOn w:val='missing'/><w:rPr><w:i/></w:rPr></w:style>"),"<w:p><w:pPr><w:pStyle w:val='child'/></w:pPr></w:p>");
        assert!(shape(&d, 0).italic);
        assert!(!shape(&d, 0).bold);
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.code() == "DOCX_STYLE_REFERENCE_LOST")
                .map(|w| w.count())
                .sum::<u64>(),
            1
        );
    }
    for count in [64, 65] {
        let mut styles = String::new();
        for i in 0..count {
            styles.push_str(&format!(
                "<w:style w:type='paragraph' w:styleId='s{i:03}'>{}</w:style>",
                if i == 0 { String::new() } else { format!("<w:basedOn w:val='s{:03}'/>", i - 1) }
            ));
        }
        let styles = format!("<w:styles xmlns:w='{W}'>{styles}</w:styles>");
        let result = decode_docx(&package("<w:p/>", &[("styles", "styles.xml", &styles)]));
        if count == 64 {
            assert!(result.is_ok());
        } else {
            assert!(matches!(
                result,
                Err(crate::DocxError::Limit {
                    resource: "style inheritance depth",
                    observed: 65,
                    limit: 64,
                    ..
                })
            ));
        }
    }
}

// 이것을 실패시키는 것: 번호 서식 순서 반전, override 누락, 번호 basedOn 연결 적용.
#[test]
fn numbering_precedes_paragraph_and_overrides_are_local() {
    let styles=format!("<w:styles xmlns:w='{W}'><w:style w:type='paragraph' w:styleId='p'><w:pPr><w:ind w:left='360'/><w:numPr><w:numId w:val='7'/></w:numPr></w:pPr></w:style><w:style w:type='numbering' w:styleId='n'><w:basedOn w:val='missing'/></w:style></w:styles>");
    let numbers=format!("<w:numbering xmlns:w='{W}'><w:abstractNum w:abstractNumId='1'><w:numStyleLink w:val='n'/><w:styleLink w:val='p'/><w:lvl w:ilvl='0'><w:pPr><w:ind w:left='720'/></w:pPr></w:lvl></w:abstractNum><w:num w:numId='7'><w:abstractNumId w:val='1'/><w:lvlOverride w:ilvl='1'><w:lvl w:ilvl='1'><w:pPr><w:ind w:left='900'/></w:pPr></w:lvl></w:lvlOverride></w:num></w:numbering>");
    let body="<w:p><w:pPr><w:pStyle w:val='p'/></w:pPr></w:p><w:p><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr></w:pPr></w:p><w:p><w:pPr><w:numPr><w:numId w:val='7'/><w:ilvl w:val='1'/></w:numPr></w:pPr></w:p>";
    let d = decode_docx(&package(
        body,
        &[("numbering", "numbering.xml", &numbers), ("styles", "styles.xml", &styles)],
    ))
    .unwrap();
    let values: Vec<_> = d.document.sections()[0]
        .paragraphs
        .iter()
        .map(|p| d.registry.para_shapes[p.para_shape_id.get()].indent_left.as_i32())
        .collect();
    assert_eq!(values, [1800, 3600, 4500]);
    assert!(!d.warnings.iter().any(|w| w.code() == "DOCX_STYLE_REFERENCE_LOST"));
    assert_eq!(d.warnings.iter().find(|w| w.loss() == "numbering_style_link").unwrap().count(), 2);
}

// 이것을 실패시키는 것: false를 미지정으로 취급, pageBreakBefore를 break_type에도 중복 저장.
#[test]
fn paragraph_booleans_and_mark_formatting() {
    let d=read("<w:style w:type='paragraph' w:styleId='p'><w:pPr><w:keepNext/><w:keepLines/><w:widowControl/><w:pageBreakBefore/></w:pPr></w:style>","<w:p><w:pPr><w:pStyle w:val='p'/><w:keepNext w:val='0'/><w:keepLines w:val='off'/><w:widowControl w:val='false'/><w:pageBreakBefore w:val='1'/><w:rPr><w:b/><w:sz w:val='30'/></w:rPr></w:pPr><w:r><w:t>A</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val='p'/><w:pageBreakBefore w:val='0'/></w:pPr></w:p>");
    let p = &d.document.sections()[0].paragraphs[0];
    let s = &d.registry.para_shapes[p.para_shape_id.get()];
    assert!(!s.keep_with_next && !s.keep_lines_together && !s.widow_orphan);
    assert!(p.page_break);
    assert_eq!(s.break_type, hwpforge_foundation::BreakType::None);
    assert!(!shape(&d, 0).bold);
    let mark = &d.registry.char_shapes[p.end_char_shape.unwrap().get()];
    assert!(mark.bold);
    assert_eq!(mark.size.as_i32(), 1500);
    assert!(!d.document.sections()[0].paragraphs[1].page_break);
}

// 이것을 실패시키는 것: theme 우선순위·부족 슬롯 fallback·다른 prefix namespace 해석 제거.
#[test]
fn theme_fonts_resolve_before_styles_and_direct_fonts() {
    let theme="<a:theme xmlns:a='http://schemas.openxmlformats.org/drawingml/2006/main'><a:themeElements><a:fontScheme><a:majorFont><a:latin typeface='Major'/><a:ea typeface='동아시아'/><a:cs typeface='Complex'/><a:font script='Hang' typeface='Ignored'/></a:majorFont><a:minorFont><a:latin typeface='Minor'/><a:ea typeface=''/><a:cs typeface=''/></a:minorFont></a:fontScheme></a:themeElements></a:theme>";
    let styles=format!("<x:styles xmlns:x='{W}'><x:docDefaults><x:rPrDefault><x:rPr><x:rFonts x:hAnsi='Literal' x:hAnsiTheme='majorHAnsi' x:eastAsiaTheme='majorEastAsia' x:cstheme='majorBidi'/></x:rPr></x:rPrDefault></x:docDefaults></x:styles>");
    let body="<w:p><w:r><w:rPr><w:rFonts w:asciiTheme='minorAscii'/></w:rPr><w:t>A</w:t></w:r><w:r><w:rPr><w:rFonts w:hAnsi='Fallback' w:hAnsiTheme='unknown'/></w:rPr><w:t>B</w:t></w:r></w:p>";
    let d = decode_docx(&package(
        body,
        &[("styles", "styles.xml", &styles), ("theme", "theme.xml", theme)],
    ))
    .unwrap();
    assert_eq!(shape(&d, 0).font_latin.as_deref(), Some("Major"));
    assert_eq!(shape(&d, 0).font_east_asian.as_deref(), Some("동아시아"));
    assert_eq!(shape(&d, 1).font_latin.as_deref(), Some("Fallback"));
    assert!(d.warnings.iter().any(|w| w.code() == "DOCX_UNSUPPORTED_VALUE"));
    assert!(d.warnings.iter().any(|w| w.loss() == "theme_script_font"));
}

// 이것을 실패시키는 것: 색 RGB fallback·숨김 텍스트 유지·명시 효과 해제 제거.
#[test]
fn effects_colors_and_unsupported_properties_are_explicit() {
    let d=read("<w:docDefaults><w:rPrDefault><w:rPr><w:color w:val='112233'/><w:u/><w:strike/></w:rPr></w:rPrDefault></w:docDefaults>","<w:p><w:r><w:rPr><w:color w:val='445566' w:themeColor='accent1'/><w:highlight w:val='yellow'/><w:vertAlign w:val='superscript'/><w:caps/><w:vanish/><w:w w:val='90'/><w:spacing w:val='2'/><w:other><w:b/></w:other></w:rPr><w:t>A</w:t></w:r><w:r><w:rPr><w:color w:val='auto'/><w:u w:val='none'/><w:strike w:val='false'/><w:highlight w:val='none'/><w:vertAlign w:val='subscript'/></w:rPr><w:t>B</w:t></w:r></w:p>");
    let a = shape(&d, 0);
    let b = shape(&d, 1);
    assert_eq!(a.color, Color::from_rgb(0x44, 0x55, 0x66));
    assert_eq!(b.color, Color::from_rgb(0x11, 0x22, 0x33));
    assert_eq!(a.shade_color, Some(Color::from_rgb(255, 255, 0)));
    assert_eq!(b.shade_color, None);
    assert_eq!(a.vertical_position, hwpforge_foundation::VerticalPosition::Superscript);
    assert_eq!(b.vertical_position, hwpforge_foundation::VerticalPosition::Subscript);
    assert!(!a.bold);
    assert_eq!(b.underline_type, hwpforge_foundation::UnderlineType::None);
    assert_eq!(b.strikeout_shape, hwpforge_foundation::StrikeoutShape::None);
    assert_eq!(d.document.sections()[0].paragraphs[0].text_content(), "AB");
    assert_eq!(
        d.warnings.iter().find(|w| w.code() == "DOCX_HIDDEN_TEXT_EXPOSED").unwrap().count(),
        1
    );
}

// 이것을 실패시키는 것: 해결되지 않은 원본을 템플릿 160%·한컴바탕으로 대체, fallback 경고 누락.
#[test]
fn unresolved_categories_have_import_defaults_and_counts() {
    let d = read("", "<w:p/><w:p/>");
    let a = shape(&d, 0);
    assert_eq!((a.size.as_i32(), a.font.as_str(), a.color), (1000, "함초롬바탕", Color::BLACK));
    assert_eq!(d.registry.para_shapes[0].line_spacing_value, 100.0);
    assert_eq!(d.warnings.len(), 6);
    assert!(d.warnings.iter().all(|w| w.code() == "DOCX_FORMAT_DEFAULTED" && w.count() == 2));
}

// 이것을 실패시키는 것: 같은 직접 서식마다 모양 추가, 직접 서식을 named style로 등록.
#[test]
fn repeated_direct_formatting_interns_one_shape() {
    let body = format!(
        "<w:p>{}</w:p>",
        "<w:r><w:rPr><w:b/><w:sz w:val='24'/></w:rPr><w:t>A</w:t></w:r>".repeat(100_000)
    );
    let bytes = package(&body, &[]);
    let start = std::time::Instant::now();
    let d = decode_docx(&bytes).unwrap();
    eprintln!(
        "repeated-direct runs=100000 chars={} paras={} named={} decode_ms={}",
        d.registry.char_shape_count(),
        d.registry.para_shape_count(),
        d.registry.style_count(),
        start.elapsed().as_millis()
    );
    assert_eq!(
        (d.registry.char_shape_count(), d.registry.para_shape_count(), d.registry.style_count()),
        (1, 1, 0)
    );
    assert_eq!(d.document.sections()[0].paragraphs[0].runs.len(), 100_000);
}

// 이것을 실패시키는 것: 정의 중복 덮어쓰기·종류별 정의 예산·입력 참조 수 검증 제거.
#[test]
fn definition_duplicates_and_limits_are_errors() {
    for xml in ["<w:style w:type='paragraph' w:styleId='x'/><w:style w:type='character' w:styleId='x'/>","<w:style w:type='paragraph'/>","<w:style w:styleId='x'/>","<w:style w:type='paragraph' w:styleId='x' w:default='1'/><w:style w:type='paragraph' w:styleId='y' w:default='true'/>"] {
        let styles=format!("<w:styles xmlns:w='{W}'>{xml}</w:styles>");assert!(decode_docx(&package("<w:p/>",&[("styles","styles.xml",&styles)])).is_err());
    }
    for xml in [
        "<w:abstractNum w:abstractNumId='1'/><w:abstractNum w:abstractNumId='1'/>",
        "<w:num w:numId='1'/><w:num w:numId='1'/>",
        "<w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'/><w:lvl w:ilvl='0'/></w:abstractNum>",
        "<w:num w:numId='1'><w:lvlOverride w:ilvl='0'/><w:lvlOverride w:ilvl='0'/></w:num>",
        "<w:num w:numId='1'><w:lvlOverride w:ilvl='0'><w:lvl w:ilvl='1'/></w:lvlOverride></w:num>",
        "<w:num w:numId='oops'/>",
    ] {
        let xml = format!("<w:numbering xmlns:w='{W}'>{xml}</w:numbering>");
        assert!(decode_docx(&package("<w:p/>", &[("numbering", "numbering.xml", &xml)])).is_err());
    }
    for kind in ["paragraph", "character", "table", "numbering"] {
        let mut xml = format!("<w:styles xmlns:w='{W}'>");
        for i in 0..16_385 {
            xml.push_str(&format!("<w:style w:type='{kind}' w:styleId='s{i}'/>"));
        }
        xml.push_str("</w:styles>");
        assert!(matches!(
            decode_docx(&package("<w:p/>", &[("styles", "styles.xml", &xml)])),
            Err(crate::DocxError::Limit {
                resource: "style definitions",
                observed: 16385,
                limit: 16384,
                ..
            })
        ));
    }
}

// 이것을 실패시키는 것: 숫자 오류/0을 유효값으로 사용, 미지원 값의 상위값 복구 제거.
#[test]
fn optional_value_matrix_recovers_only_the_bad_property() {
    for props in [
        "<w:sz w:val='0'/>",
        "<w:sz w:val='-1'/>",
        "<w:sz w:val='oops'/>",
        "<w:b w:val='maybe'/>",
        "<w:u w:val='wave'/>",
        "<w:highlight w:val='orange'/>",
        "<w:vertAlign w:val='other'/>",
        "<w:color w:val='bad'/>",
    ] {
        let d=read("<w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val='20'/><w:color w:val='112233'/></w:rPr></w:rPrDefault></w:docDefaults>",&format!("<w:p><w:r><w:rPr>{props}</w:rPr><w:t>A</w:t></w:r></w:p>"));
        assert_eq!(shape(&d, 0).size.as_i32(), 1000);
        assert_eq!(shape(&d, 0).color, Color::from_rgb(0x11, 0x22, 0x33));
        assert_eq!(
            d.warnings.iter().find(|w| w.code() == "DOCX_UNSUPPORTED_VALUE").unwrap().count(),
            1
        );
    }
    for props in [
        "<w:spacing w:line='oops'/>",
        "<w:spacing w:line='-1'/>",
        "<w:spacing w:line='240' w:lineRule='other'/>",
        "<w:spacing w:line='20000001' w:lineRule='exact'/>",
        "<w:jc w:val='distribute'/>",
    ] {
        let d=read("<w:docDefaults><w:pPrDefault><w:pPr><w:spacing w:line='360'/></w:pPr></w:pPrDefault></w:docDefaults>",&format!("<w:p><w:pPr>{props}</w:pPr></w:p>"));
        let p = &d.document.sections()[0].paragraphs[0];
        assert_eq!(d.registry.para_shapes[p.para_shape_id.get()].line_spacing_value, 150.0);
        assert_eq!(
            d.warnings.iter().find(|w| w.code() == "DOCX_UNSUPPORTED_VALUE").unwrap().count(),
            1
        );
    }
}

// 이것을 실패시키는 것: 상위 글자/문단 서식을 직접 값으로 덮거나 미지원 단위의 숫자를 적용.
#[test]
fn script_and_paragraph_properties_keep_independent_inheritance() {
    let d=read("<w:style w:type='paragraph' w:styleId='p' w:default='1'><w:rPr><w:b/><w:i/><w:bCs/><w:iCs/></w:rPr><w:pPr><w:ind w:left='100' w:right='200'/><w:spacing w:before='60' w:after='80'/></w:pPr></w:style><w:style w:type='character' w:styleId='c'><w:rPr><w:b w:val='0'/><w:i w:val='0'/><w:bCs/><w:iCs/></w:rPr></w:style>","<w:p><w:pPr><w:ind w:left='999' w:leftChars='100'/><w:spacing w:before='999' w:beforeLines='100' w:after='999' w:afterAutospacing='1'/><w:tabs><w:tab w:pos='720'/></w:tabs><w:pBdr/><w:bidi/><w:other/><w:outlineLvl w:val='0'/></w:pPr><w:r><w:rPr><w:rStyle w:val='c'/></w:rPr><w:t>A</w:t></w:r></w:p>");
    assert!(shape(&d, 0).bold && shape(&d, 0).italic);
    assert!(d.warnings.iter().any(|w| w.code() == "DOCX_SCRIPT_FORMAT_COLLAPSED"));
    let p = &d.document.sections()[0].paragraphs[0];
    let s = &d.registry.para_shapes[p.para_shape_id.get()];
    assert_eq!(
        (
            s.indent_left.as_i32(),
            s.indent_right.as_i32(),
            s.space_before.as_i32(),
            s.space_after.as_i32()
        ),
        (500, 1000, 300, 400)
    );
    assert!(p.style_ref.is_some());
    assert!(d.warnings.iter().any(|w| w.loss() == "outline_level"));
}

// 이것을 실패시키는 것: 같은 name의 정의 순서에 따라 StyleRef 변경, 스타일/직접 표현 중복 모양 생성.
#[test]
fn stable_identity_and_equivalent_representations() {
    for styles in ["<w:style w:type='paragraph' w:styleId='Heading 1'><w:name w:val='제목 1'/><w:rPr><w:sz w:val='20'/></w:rPr></w:style><w:style w:type='paragraph' w:styleId='Heading_1'><w:name w:val='제목 1'/></w:style>","<w:style w:type='paragraph' w:styleId='Heading_1'><w:name w:val='제목 1'/></w:style><w:style w:type='paragraph' w:styleId='Heading 1'><w:name w:val='제목 1'/><w:rPr><w:sz w:val='20'/></w:rPr></w:style>"] {
        let d=read(styles,"<w:p><w:pPr><w:pStyle w:val='Heading 1'/></w:pPr><w:r><w:rPr><w:sz w:val='20'/></w:rPr><w:t>A</w:t></w:r></w:p>");
        assert_eq!(d.document.sections()[0].paragraphs[0].style_ref.unwrap().get(),0);assert_eq!(d.registry.style_count(),2);assert_eq!(d.registry.char_shape_count(),1);
    }
}

// 이것을 실패시키는 것: 구조 치수를 선택 서식처럼 복구하거나 overflow를 잘라 성공시키기.
#[test]
fn malformed_structure_is_fatal_while_formatting_is_local() {
    for xml in [
        "<w:pgSz w:w='oops' w:h='15840'/>",
        "<w:pgSz w:w='0'/>",
        "<w:pgSz w:w='20000001'/>",
        "<w:pgMar w:left='-1'/>",
        "<w:pgSz w:w='9999999999999999999999999999999999999999999'/>",
    ] {
        assert!(decode_docx(&package(&format!("<w:p/><w:sectPr>{xml}</w:sectPr>"), &[])).is_err());
    }
    let d = read(
        "",
        "<w:p/><w:sectPr><w:pgSz w:w='11906' w:h='16838'/><w:pgMar w:left='0'/></w:sectPr>",
    );
    assert_eq!(
        d.document.sections()[0].page_settings,
        hwpforge_core::page::PageSettings::default()
    );
    assert_eq!(d.warnings.iter().find(|w| w.loss() == "section_layout").unwrap().count(), 1);
}

// 이것을 실패시키는 것: 기본 highlight 색표·정렬·baseline/none 대응을 다른 값으로 바꿈.
#[test]
fn supported_value_tables_are_literal() {
    for (name, rgb) in [
        ("black", 0x000000),
        ("blue", 0x0000ff),
        ("cyan", 0x00ffff),
        ("green", 0x00ff00),
        ("magenta", 0xff00ff),
        ("red", 0xff0000),
        ("yellow", 0xffff00),
        ("white", 0xffffff),
        ("darkBlue", 0x000080),
        ("darkCyan", 0x008080),
        ("darkGreen", 0x008000),
        ("darkMagenta", 0x800080),
        ("darkRed", 0x800000),
        ("darkYellow", 0x808000),
        ("darkGray", 0x808080),
        ("lightGray", 0xc0c0c0),
    ] {
        let d=read("",&format!("<w:p><w:r><w:rPr><w:highlight w:val='{name}'/><w:vertAlign w:val='baseline'/></w:rPr><w:t>A</w:t></w:r></w:p>"));
        assert_eq!(
            shape(&d, 0).shade_color,
            Some(Color::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8))
        );
        assert_eq!(shape(&d, 0).vertical_position, hwpforge_foundation::VerticalPosition::Normal);
    }
    for (name, alignment) in [
        ("left", hwpforge_foundation::Alignment::Left),
        ("start", hwpforge_foundation::Alignment::Left),
        ("end", hwpforge_foundation::Alignment::Right),
        ("center", hwpforge_foundation::Alignment::Center),
        ("right", hwpforge_foundation::Alignment::Right),
        ("both", hwpforge_foundation::Alignment::Justify),
    ] {
        let d=read("",&format!("<w:p><w:pPr><w:jc w:val='{name}'/><w:spacing w:before='20' w:after='40' w:line='240'/><w:ind w:firstLine='120'/></w:pPr></w:p>"));
        let p = &d.document.sections()[0].paragraphs[0];
        let s = &d.registry.para_shapes[p.para_shape_id.get()];
        assert_eq!(s.alignment, alignment);
        assert_eq!(d.warnings.iter().filter(|w| w.code() == "DOCX_UNSUPPORTED_VALUE").count(), 0);
        assert_eq!(
            (
                s.space_before.as_i32(),
                s.space_after.as_i32(),
                s.indent_first_line.as_i32(),
                s.line_spacing_value
            ),
            (100, 200, 600, 100.0)
        );
    }
}

// 이것을 실패시키는 것: 조건부 표·모르는 속성 자손을 일반 속성처럼 주입, 0번 목록에 경고 추가.
#[test]
fn unsupported_subtrees_and_missing_references_do_not_inject_formatting() {
    let d=read("<w:style w:type='table' w:styleId='t'><w:tblStylePr w:type='firstRow'><w:rPr><w:b/></w:rPr></w:tblStylePr><w:tblStylePr w:type='wholeTable'><w:rPr><w:i/></w:rPr></w:tblStylePr></w:style><w:style w:type='paragraph' w:styleId='p' w:default='1'><w:unknown><w:rPr><w:b/></w:rPr></w:unknown></w:style>","<w:p><w:pPr><w:pStyle w:val='missing'/><w:numPr><w:numId w:val='0'/></w:numPr></w:pPr><w:r><w:rPr><w:rStyle w:val='missing'/><x:unknown xmlns:x='urn:other'/><w:vanish w:val='0'/></w:rPr><w:t>A</w:t></w:r></w:p>");
    assert!(!shape(&d, 0).bold);
    assert!(d.document.sections()[0].paragraphs[0].style_ref.is_none());
    assert_eq!(d.warnings.iter().find(|w| w.loss() == "style_reference").unwrap().count(), 2);
    assert!(!d
        .warnings
        .iter()
        .any(|w| w.loss() == "list_markers" || w.loss() == "hidden_text_effect"));
    let d = read("", "<w:p><w:pPr><w:numPr><w:numId w:val='42'/></w:numPr></w:pPr></w:p>");
    assert!(d.warnings.iter().any(|w| w.loss() == "numbering_reference"));
}

// 이것을 실패시키는 것: 군별 값 비교를 생략하거나 style false의 명시 여부를 지움.
#[test]
fn complex_script_differences_are_detected_independently() {
    for (properties, different) in [
        ("<w:sz w:val='20'/><w:szCs w:val='20'/>", false),
        ("<w:sz w:val='20'/><w:szCs w:val='24'/>", true),
        ("<w:b/><w:bCs/>", false),
        ("<w:b/><w:bCs w:val='false'/>", true),
        ("<w:i/><w:iCs/>", false),
        ("<w:i/><w:iCs w:val='false'/>", true),
    ] {
        for styled in [false, true] {
            let d = if styled {
                read(&format!("<w:style w:type='paragraph' w:styleId='p' w:default='1'><w:rPr>{properties}</w:rPr></w:style>"),"<w:p/>")
            } else {
                read("", &format!("<w:p><w:r><w:rPr>{properties}</w:rPr><w:t>A</w:t></w:r></w:p>"))
            };
            assert_eq!(
                d.warnings.iter().any(|w| w.code() == "DOCX_SCRIPT_FORMAT_COLLAPSED"),
                different,
                "{properties} styled={styled}"
            );
        }
    }
    let d = read(
        "",
        "<w:p><w:r><w:rPr><w:u w:val='single'/><w:strike/></w:rPr><w:t>A</w:t></w:r></w:p>",
    );
    assert_eq!(shape(&d, 0).underline_type, hwpforge_foundation::UnderlineType::Bottom);
    assert_eq!(shape(&d, 0).strikeout_shape, hwpforge_foundation::StrikeoutShape::Solid);
}

// 이것을 실패시키는 것: 선택된 스타일의 미지원 자손 서식 적용, 같은 미해결 run 참조의 경고 횟수 누락.
#[test]
fn selected_style_unknown_subtrees_and_cached_missing_references() {
    let d=read("<w:style w:type='paragraph' w:styleId='p' w:default='1'><w:unknown><w:rPr><w:b/></w:rPr></w:unknown></w:style>","<w:p><w:r><w:rPr><w:rStyle w:val='missing'/></w:rPr><w:t>A</w:t></w:r><w:r><w:rPr><w:rStyle w:val='missing'/></w:rPr><w:t>B</w:t></w:r></w:p>");
    assert!(!shape(&d, 0).bold);
    assert!(!shape(&d, 1).bold);
    assert_eq!(d.warnings.iter().find(|w| w.loss() == "style_reference").unwrap().count(), 2);
}

// 이것을 실패시키는 것: 필수 값 없는 선택 속성을 무경고로 제외하거나 빈 글꼴을 저장.
#[test]
fn missing_optional_values_are_reported_without_losing_text() {
    let d=read("<w:style w:type='paragraph' w:styleId='p'><w:basedOn/></w:style>","<w:p><w:pPr><w:pStyle/></w:pPr><w:r><w:rPr><w:rStyle/><w:sz/><w:rFonts w:hAnsi=''/></w:rPr><w:t>A</w:t></w:r></w:p>");
    assert_eq!(shape(&d, 0).size.as_i32(), 1000);
    assert_eq!(shape(&d, 0).font, "함초롬바탕");
    assert_eq!(
        d.warnings.iter().find(|w| w.code() == "DOCX_UNSUPPORTED_VALUE").unwrap().count(),
        4
    );
    assert_eq!(d.warnings.iter().find(|w| w.loss() == "style_parent").unwrap().count(), 1);
}

// 이것을 실패시키는 것: 직접 속성 컨테이너의 알 수 없는 속성을 무경고로 제외.
#[test]
fn property_container_attributes_have_finite_losses() {
    for body in [
        "<w:p><w:pPr w:future='x'/></w:p>",
        "<w:p><w:r><w:rPr w:future='x'/><w:t>A</w:t></w:r></w:p>",
        "<w:p><w:pPr><w:rPr w:future='x'/></w:pPr></w:p>",
    ] {
        let d = read("", body);
        assert_eq!(
            d.warnings
                .iter()
                .filter(
                    |w| w.code() == "DOCX_UNSUPPORTED_ATTRIBUTE" && w.loss() == "format_attribute"
                )
                .map(|w| w.count())
                .sum::<u64>(),
            1
        );
    }
}

// 이것을 실패시키는 것: 정의 속성 컨테이너에서 알 수 없는 속성 손실을 누락.
#[test]
fn definition_container_attributes_have_finite_losses() {
    for props in ["<w:pPr w:future='x'/>", "<w:rPr w:future='x'/>"] {
        let d =
            read(&format!("<w:style w:type='paragraph' w:styleId='p'>{props}</w:style>"), "<w:p/>");
        assert_eq!(
            d.warnings
                .iter()
                .filter(
                    |w| w.code() == "DOCX_UNSUPPORTED_ATTRIBUTE" && w.loss() == "format_attribute"
                )
                .map(|w| w.count())
                .sum::<u64>(),
            1
        );
    }
}

// 이것을 실패시키는 것: 줄 간격 값/유형의 개별 상속 제거, 문맥에서 생긴 선택 서식 범위 오류의 전역 거부.
#[test]
fn inherited_line_rules_and_contextual_range_errors_are_local() {
    for (parent, child, kind, value, warning) in [
        ("w:line='240' w:lineRule='exact'", "w:line='480'", LineSpacingType::Fixed, 2400.0, false),
        (
            "w:line='240' w:lineRule='exact'",
            "w:line='100000000'",
            LineSpacingType::Fixed,
            1200.0,
            true,
        ),
        (
            "w:line='100000000' w:lineRule='auto'",
            "w:lineRule='exact'",
            LineSpacingType::Percentage,
            41666666.666666664,
            true,
        ),
        (
            "w:line='360'",
            "w:line='12pt' w:lineRule='exact'",
            LineSpacingType::Percentage,
            150.0,
            true,
        ),
    ] {
        let d=read(&format!("<w:docDefaults><w:pPrDefault><w:pPr><w:spacing {parent}/></w:pPr></w:pPrDefault></w:docDefaults>"),&format!("<w:p><w:pPr><w:spacing {child}/></w:pPr></w:p>"));
        let p = &d.document.sections()[0].paragraphs[0];
        let s = &d.registry.para_shapes[p.para_shape_id.get()];
        assert_eq!((s.line_spacing_type, s.line_spacing_value), (kind, value));
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.code() == "DOCX_UNSUPPORTED_VALUE")
                .map(|w| w.count())
                .sum::<u64>(),
            u64::from(warning)
        );
    }
}

// 이것을 실패시키는 것: line이 없을 때 exact/atLeast 길이를 합성.
#[test]
fn missing_line_overrides_length_rule_with_percentage() {
    for rule in ["exact", "atLeast"] {
        let d = read("", &format!("<w:p><w:pPr><w:spacing w:lineRule='{rule}'/></w:pPr></w:p>"));
        let p = &d.document.sections()[0].paragraphs[0];
        let s = &d.registry.para_shapes[p.para_shape_id.get()];
        assert_eq!(
            (s.line_spacing_type, s.line_spacing_value),
            (LineSpacingType::Percentage, 100.0)
        );
        let w = d.warnings.iter().find(|w| w.loss() == "line_spacing").unwrap();
        assert_eq!(
            (w.code(), w.action(), w.count(), w.message()),
            (
                "DOCX_FORMAT_DEFAULTED",
                "use_fallback",
                1,
                "Unresolved paragraph formatting uses 100% line spacing or zero length".to_owned()
            )
        );
    }
}

// 이것을 실패시키는 것: 미사용 named style의 군별 축약 경고 누락 또는 미출력 character 정의 경고.
#[test]
fn exported_named_style_losses_have_definition_location() {
    let d = read("<w:style w:type='paragraph' w:styleId='unused'><w:rPr><w:rFonts w:ascii='A' w:hAnsi='B' w:eastAsia='C' w:cs='D'/></w:rPr></w:style><w:style w:type='character' w:styleId='raw'><w:rPr><w:sz w:val='20'/><w:szCs w:val='24'/></w:rPr></w:style>", "<w:p/>");
    assert_eq!(d.registry.style_count(), 1);
    let losses: Vec<_> = d
        .warnings
        .iter()
        .filter(|w| w.code() == "DOCX_SCRIPT_FORMAT_COLLAPSED")
        .map(|w| (w.loss(), w.action(), w.count(), w.part(), w.location()))
        .collect();
    assert_eq!(
        losses,
        [(
            "script_format",
            "select_supported_slots",
            1,
            Some("text/styles.xml"),
            Some("style:unused")
        )]
    );
}

// 이것을 실패시키는 것: 동아시아 대체 경고 누락, theme 글꼴/색과 구문 오류를 같은 loss로 집계.
#[test]
fn fallback_font_and_value_losses_are_separate() {
    let d = read("", "<w:p><w:r><w:rPr><w:rFonts w:ascii='Calibri' w:hAnsi='Calibri' w:eastAsiaTheme='minorEastAsia'/><w:color w:val='112233' w:themeColor='accent1'/><w:sz w:val='oops'/></w:rPr><w:t>A</w:t></w:r></w:p>");
    let c = shape(&d, 0);
    assert_eq!(
        (c.font.as_str(), c.font_latin.as_deref(), c.font_east_asian.as_deref()),
        ("Calibri", Some("Calibri"), Some("Calibri"))
    );
    assert_eq!(
        d.warnings
            .iter()
            .filter(|w| w.code() == "DOCX_UNSUPPORTED_VALUE" || w.loss() == "east_asian_font")
            .map(|w| (w.code(), w.loss(), w.action(), w.count()))
            .collect::<Vec<_>>(),
        [
            ("DOCX_UNSUPPORTED_VALUE", "theme_font", "inherit_or_default", 1),
            ("DOCX_UNSUPPORTED_VALUE", "theme_color", "inherit_or_default", 1),
            ("DOCX_UNSUPPORTED_VALUE", "format_value", "inherit_or_default", 1),
            ("DOCX_FORMAT_DEFAULTED", "east_asian_font", "use_general_font", 1),
        ]
    );
    let named = read("<w:style w:type='paragraph' w:styleId='unused'><w:rPr><w:rFonts w:ascii='Calibri' w:hAnsi='Calibri'/></w:rPr></w:style>", "<w:p/>");
    assert_eq!(
        named
            .warnings
            .iter()
            .filter(|w| w.loss() == "east_asian_font")
            .map(|w| (w.code(), w.action(), w.count(), w.location()))
            .collect::<Vec<_>>(),
        [("DOCX_FORMAT_DEFAULTED", "use_general_font", 1, Some("style:unused"))]
    );
}

// 이것을 실패시키는 것: 부모 이름만으로 owner를 승인, 바깥 상태 해제, 정의 손실 분류 변경.
#[test]
fn unadmitted_definition_frames_never_own_properties() {
    for subtree in ["<w:unknown><w:style><w:name w:val='bad'/></w:style></w:unknown>", "<w:style w:type='paragraph' w:styleId='p' w:default='1'><w:unknown><w:tblStylePr w:type='wholeTable'><w:rPr><w:b/></w:rPr></w:tblStylePr></w:unknown><w:rPr><w:i/></w:rPr></w:style>"] {
        let d = read(subtree, "<w:p/>");
        assert!(!shape(&d, 0).bold);
        assert_eq!(shape(&d, 0).italic, subtree.starts_with("<w:style"));
        assert_eq!(d.warnings.iter().filter(|w| w.loss() == "style_definition").map(|w| (w.code(), w.action(), w.count())).collect::<Vec<_>>(), [("DOCX_UNSUPPORTED_ATTRIBUTE", "ignore", 1)]);
    }
    let numbering = format!("<w:numbering xmlns:w='{W}'><w:unknown><w:num><w:abstractNumId w:val='9'/></w:num></w:unknown><w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:pPr><w:ind w:left='720'/></w:pPr></w:lvl></w:abstractNum><w:num w:numId='7'><w:unknown><w:num><w:abstractNumId w:val='9'/></w:num></w:unknown><w:abstractNumId w:val='1'/></w:num></w:numbering>");
    let d = decode_docx(&package(
        "<w:p><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr></w:pPr></w:p>",
        &[("numbering", "numbering.xml", &numbering)],
    ))
    .unwrap();
    let p = &d.document.sections()[0].paragraphs[0];
    assert_eq!(d.registry.para_shapes[p.para_shape_id.get()].indent_left.as_i32(), 3600);
    assert_eq!(d.warnings.iter().find(|w| w.loss() == "list_definitions").map(|w| (w.code(), w.action(), w.count(), w.message())), Some(("DOCX_FORMATTING_OMITTED", "omit", 1, "Numbering markers and list semantics are not applied; level paragraph formatting is retained".to_owned())));
}

// 이것을 실패시키는 것: 글꼴·스타일·번호 식별자의 해시 전 길이 상한 검사 제거 또는 >=로 경계 변경.
#[test]
fn format_identifier_utf8_boundaries_are_literal() {
    for bytes in [4095, 4096, 4097] {
        let font = format!("{}{}", "가".repeat(bytes / 3), "a".repeat(bytes % 3));
        let id = "s".repeat(bytes);
        for (styles, body) in [
            (
                String::new(),
                format!(
                    "<w:p><w:r><w:rPr><w:rFonts w:hAnsi='{font}'/></w:rPr><w:t>A</w:t></w:r></w:p>"
                ),
            ),
            (format!("<w:style w:type='paragraph' w:styleId='{id}'/>"), "<w:p/>".into()),
            (String::new(), format!("<w:p><w:pPr><w:pStyle w:val='{id}'/></w:pPr></w:p>")),
            (
                String::new(),
                format!(
                    "<w:p><w:r><w:rPr><w:rStyle w:val='{id}'/></w:rPr><w:t>A</w:t></w:r></w:p>"
                ),
            ),
            (
                format!(
                    "<w:style w:type='paragraph' w:styleId='p'><w:basedOn w:val='{id}'/></w:style>"
                ),
                "<w:p/>".into(),
            ),
            (
                String::new(),
                format!(
                    "<w:p><w:pPr><w:numPr><w:numId w:val='{}'/></w:numPr></w:pPr></w:p>",
                    "0".repeat(bytes)
                ),
            ),
        ] {
            let styles = format!("<w:styles xmlns:w='{W}'>{styles}</w:styles>");
            let result = decode_docx(&package(&body, &[("styles", "styles.xml", &styles)]));
            if bytes <= 4096 {
                assert!(result.is_ok(), "{result:?}");
            } else {
                assert!(matches!(
                    result,
                    Err(crate::DocxError::Limit {
                        resource: "identifier bytes",
                        observed: 4097,
                        limit: 4096,
                        ..
                    })
                ));
            }
        }
    }
}

// 이것을 실패시키는 것: named 등록의 공통 진단 호출 제거 또는 missing 비트별 경고 누락.
#[test]
fn exported_named_defaults_are_diagnosed_before_direct_body_formatting() {
    let character =
        "<w:rFonts w:hAnsi='A' w:eastAsia='A'/><w:sz w:val='20'/><w:color w:val='112233'/>";
    let paragraph = "<w:spacing w:line='240' w:before='0' w:after='0'/><w:ind w:left='0' w:right='0' w:firstLine='0'/>";
    for (remove, loss) in [
        ("<w:color w:val='112233'/>", "text_color"),
        ("<w:sz w:val='20'/>", "font_size"),
        ("<w:rFonts w:hAnsi='A' w:eastAsia='A'/>", "font_family"),
        (" w:eastAsia='A'", "east_asian_font"),
        (" w:line='240'", "line_spacing"),
        (" w:before='0'", "paragraph_spacing"),
        (" w:after='0'", "paragraph_spacing"),
        (" w:left='0'", "indentation"),
        (" w:right='0'", "indentation"),
        (" w:firstLine='0'", "indentation"),
    ] {
        let defaults = format!("<w:docDefaults><w:rPrDefault><w:rPr>{character}</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr>{paragraph}</w:pPr></w:pPrDefault></w:docDefaults>").replace(remove, "");
        let d = read(&format!("{defaults}<w:style w:type='paragraph' w:styleId='unused'/>") , &format!("<w:p><w:pPr>{paragraph}</w:pPr><w:r><w:rPr>{character}</w:rPr><w:t>A</w:t></w:r></w:p>"));
        assert_eq!(d.registry.style_count(), 1);
        let warnings: Vec<_> = d
            .warnings
            .iter()
            .map(|w| (w.code(), w.loss(), w.count(), w.part(), w.location()))
            .collect();
        assert_eq!(
            warnings,
            [("DOCX_FORMAT_DEFAULTED", loss, 1, Some("text/styles.xml"), Some("style:unused"))],
            "{remove}"
        );
        let entry = d.registry.style_entries().values().next().unwrap();
        if loss == "text_color" {
            assert_eq!(
                d.registry.char_shapes[entry.char_shape_id.get()].color,
                Color::from_rgb(0, 0, 0)
            );
            assert_eq!(shape(&d, 0).color, Color::from_rgb(0x11, 0x22, 0x33));
        }
    }
}

// 이것을 실패시키는 것: 최솟값 검사를 반올림 뒤로 이동, 최소 길이를 0으로 완화, 정확한 -0 거부.
#[test]
fn optional_fractional_lengths_check_source_range_before_rounding() {
    for (value, expected, count) in
        [("-0.004pt", (100, 100, -100), 3), ("-0pt", (0, 0, 0), 0), ("0.004pt", (0, 0, 0), 0)]
    {
        let d = read("<w:docDefaults><w:pPrDefault><w:pPr><w:spacing w:before='20' w:after='20'/><w:ind w:hanging='20'/></w:pPr></w:pPrDefault></w:docDefaults>", &format!("<w:p><w:pPr><w:spacing w:before='{value}' w:after='{value}'/><w:ind w:hanging='{value}'/></w:pPr></w:p>"));
        let p = &d.document.sections()[0].paragraphs[0];
        let s = &d.registry.para_shapes[p.para_shape_id.get()];
        assert_eq!(
            (s.space_before.as_i32(), s.space_after.as_i32(), s.indent_first_line.as_i32()),
            expected
        );
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.code() == "DOCX_UNSUPPORTED_VALUE")
                .map(|w| w.count())
                .sum::<u64>(),
            count
        );
    }
    let d = read("<w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val='20'/></w:rPr></w:rPrDefault></w:docDefaults>", "<w:p><w:r><w:rPr><w:sz w:val='0.009pt'/></w:rPr><w:t>A</w:t></w:r></w:p>");
    assert_eq!(shape(&d, 0).size.as_i32(), 1000);
    assert_eq!(
        d.warnings
            .iter()
            .filter(|w| w.code() == "DOCX_UNSUPPORTED_VALUE")
            .map(|w| w.count())
            .sum::<u64>(),
        1
    );
}

// 이것을 실패시키는 것: 구조 치수에서 반올림된 0을 수용하거나 pgSz의 양수 최솟값 제거.
#[test]
fn structural_fractional_lengths_reject_negative_nonzero_sources() {
    for dimensions in ["<w:pgMar w:left='-0.004pt'/>", "<w:pgSz w:w='0.009pt'/>"] {
        assert!(decode_docx(&package(&format!("<w:p/><w:sectPr>{dimensions}</w:sectPr>"), &[]))
            .is_err());
    }
    let d = read("", "<w:p/><w:sectPr><w:pgMar w:left='-0pt'/><w:pgSz w:w='0.01pt'/></w:sectPr>");
    assert_eq!(
        d.document.sections()[0].page_settings,
        hwpforge_core::page::PageSettings::default()
    );
}

// 이것을 실패시키는 것: named 등록이 공통 문맥 대신 defaults→paragraph만 계산, numPr 컨테이너를 leaf로 덮어쓰기.
#[test]
fn named_and_body_share_numbering_indentation_without_direct_overrides() {
    let styles = format!("<w:styles xmlns:w='{W}'><w:docDefaults><w:pPrDefault><w:pPr><w:ind w:left='20'/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type='paragraph' w:styleId='p'><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr></w:pPr></w:style></w:styles>");
    let numbering = format!("<w:numbering xmlns:w='{W}'><w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:pPr><w:ind w:left='720'/></w:pPr></w:lvl></w:abstractNum><w:num w:numId='7'><w:abstractNumId w:val='1'/></w:num></w:numbering>");
    let d = decode_docx(&package(
        "<w:p><w:pPr><w:pStyle w:val='p'/></w:pPr><w:r><w:t>A</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr></w:pPr></w:p>",
        &[("styles", "styles.xml", &styles), ("numbering", "numbering.xml", &numbering)],
    ))
    .unwrap();
    let named = d.registry.style_entries().values().next().unwrap();
    let p = &d.document.sections()[0].paragraphs[0];
    assert_eq!(d.registry.para_shapes[named.para_shape_id.get()].indent_left.as_i32(), 3600);
    assert_eq!(d.registry.para_shapes[p.para_shape_id.get()].indent_left.as_i32(), 3600);
    assert_eq!(p.style_ref.unwrap().get(), 0);
    let direct = &d.document.sections()[0].paragraphs[1];
    assert_eq!(d.registry.para_shapes[direct.para_shape_id.get()].indent_left.as_i32(), 3600);
    assert_eq!(direct.style_ref, None);
    assert_eq!(d.warnings.iter().find(|w| w.loss() == "list_markers").unwrap().count(), 3);
}

// 이것을 실패시키는 것: 처리한 leaf를 제외 subtree로 표시, 자손 재경고 또는 자손 서식 주입.
#[test]
fn property_leaf_extensions_are_reported_once_in_definitions_and_body() {
    let children = "<x:future xmlns:x='urn:future'><x:deeper/><w:i/><w:rPr w:future='x'><w:i/></w:rPr></x:future>";
    for (styles, body) in [
        (format!("<w:style w:type='paragraph' w:styleId='p' w:default='1'><w:rPr><w:b>{children}</w:b></w:rPr></w:style>"), "<w:p/>".to_owned()),
        (String::new(), format!("<w:p><w:r><w:rPr><w:b>{children}</w:b></w:rPr><w:t>A</w:t></w:r></w:p>")),
    ] {
        let d = read(&styles, &body);
        assert!(shape(&d, 0).bold);
        assert!(!shape(&d, 0).italic);
        assert_eq!(d.warnings.iter().filter(|w| w.code() == "DOCX_UNSUPPORTED_ATTRIBUTE").map(|w| (w.loss(), w.action(), w.count())).collect::<Vec<_>>(), [("format_extension", "ignore", 1)]);
    }
    for properties in [
        format!("<w:ind w:left='720'>{children}</w:ind>"),
        format!("<w:spacing w:lineRule='invalid'>{children}</w:spacing><w:ind w:left='720'/>"),
    ] {
        for definition in [true, false] {
            let d = if definition {
                read(&format!("<w:style w:type='paragraph' w:styleId='p' w:default='1'><w:pPr>{properties}</w:pPr></w:style>"), "<w:p/>")
            } else {
                read("", &format!("<w:p><w:pPr>{properties}</w:pPr></w:p>"))
            };
            let p = &d.document.sections()[0].paragraphs[0];
            assert_eq!(d.registry.para_shapes[p.para_shape_id.get()].indent_left.as_i32(), 3600);
            assert_eq!(
                d.warnings
                    .iter()
                    .filter(|w| w.loss() == "format_extension")
                    .map(|w| w.count())
                    .collect::<Vec<_>>(),
                [1]
            );
        }
    }
}

// 이것을 실패시키는 것: 이미 제외한 속성 subtree를 Leaf로 바꾸어 자손을 재계수.
#[test]
fn excluded_property_subtrees_keep_one_boundary_warning() {
    for (properties, character, loss) in [
        ("<w:caps><w:b/><w:unknown/></w:caps>", true, "character_effect"),
        ("<w:unknown><w:b/></w:unknown>", true, "unknown_character_property"),
        ("<x:future xmlns:x='urn:future'><w:b/></x:future>", true, "unknown_character_property"),
        ("<w:tabs><w:ind w:left='720'/></w:tabs>", false, "paragraph_effect"),
        ("<w:unknown><w:ind w:left='720'/></w:unknown>", false, "unknown_paragraph_property"),
        (
            "<x:future xmlns:x='urn:future'><w:ind w:left='720'/></x:future>",
            false,
            "unknown_paragraph_property",
        ),
    ] {
        let tag = if character { "rPr" } else { "pPr" };
        for definition in [true, false] {
            let d = if definition {
                read(&format!("<w:style w:type='paragraph' w:styleId='p' w:default='1'><w:{tag}>{properties}</w:{tag}></w:style>"), "<w:p/>")
            } else if character {
                read("", &format!("<w:p><w:r><w:rPr>{properties}</w:rPr><w:t>A</w:t></w:r></w:p>"))
            } else {
                read("", &format!("<w:p><w:pPr>{properties}</w:pPr></w:p>"))
            };
            assert!(!shape(&d, 0).bold);
            let p = &d.document.sections()[0].paragraphs[0];
            assert_eq!(d.registry.para_shapes[p.para_shape_id.get()].indent_left.as_i32(), 0);
            assert_eq!(
                d.warnings
                    .iter()
                    .filter(|w| w.code() == "DOCX_UNSUPPORTED_ATTRIBUTE")
                    .map(|w| (w.loss(), w.count()))
                    .collect::<Vec<_>>(),
                [(loss, 1)]
            );
        }
    }
}

// 이것을 실패시키는 것: 표식 StyleKey 저장/전달/캐시 키 누락, toggle 덮어쓰기, 직접 표식 우선순위 역전, 문단 간 참조 누출.
#[test]
fn paragraph_mark_character_styles_use_toggles_direct_values_and_distinct_cache_keys() {
    let styles = "<w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val='20'/></w:rPr></w:rPrDefault></w:docDefaults><w:style w:type='character' w:styleId='c'><w:rPr><w:sz w:val='30'/><w:b/></w:rPr></w:style><w:style w:type='paragraph' w:styleId='p'><w:rPr><w:b/></w:rPr></w:style>";
    let d = read(styles, "<w:p><w:pPr><w:rPr><w:rStyle w:val='c'/></w:rPr></w:pPr><w:r><w:t>A</w:t></w:r></w:p><w:p><w:pPr><w:rPr/></w:pPr></w:p><w:p/><w:p><w:pPr><w:pStyle w:val='p'/><w:rPr><w:rStyle w:val='c'/></w:rPr></w:pPr></w:p><w:p><w:pPr><w:pStyle w:val='p'/><w:rPr><w:rStyle w:val='c'/><w:b/><w:sz w:val='24'/></w:rPr></w:pPr></w:p><w:p><w:pPr><w:rPr><w:rStyle w:val='c'/></w:rPr></w:pPr></w:p>");
    assert_eq!(shape(&d, 0).size.as_i32(), 1000);
    assert_eq!(
        d.document.sections()[0]
            .paragraphs
            .iter()
            .map(|p| p.end_char_shape.map(|id| {
                let c = &d.registry.char_shapes[id.get()];
                (c.size.as_i32(), c.bold)
            }))
            .collect::<Vec<_>>(),
        [
            Some((1500, true)),
            Some((1000, false)),
            None,
            Some((1500, false)),
            Some((1200, true)),
            Some((1500, true))
        ]
    );
    assert_eq!(d.warnings.iter().filter(|w| w.loss() == "style_reference").count(), 0);
}

// 이것을 실패시키는 것: 표식의 없는/다른 종류 참조 경고 누락, 캐시 적중 누락, 정상 참조에 허위 경고.
#[test]
fn paragraph_mark_missing_references_count_each_cached_use() {
    for reference in ["missing", "p"] {
        let mark =
            format!("<w:p><w:pPr><w:rPr><w:rStyle w:val='{reference}'/></w:rPr></w:pPr></w:p>");
        let d = read("<w:style w:type='paragraph' w:styleId='p'/>", &mark.repeat(3));
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.loss() == "style_reference")
                .map(|w| (w.code(), w.action(), w.count()))
                .collect::<Vec<_>>(),
            [("DOCX_STYLE_REFERENCE_LOST", "ignore_link", 3)]
        );
        for p in &d.document.sections()[0].paragraphs {
            assert_eq!(d.registry.char_shapes[p.end_char_shape.unwrap().get()].size.as_i32(), 1000);
        }
    }
}

// 이것을 실패시키는 것: 속성 사유에서 속성 이름/소유 요소 누락 또는 반복 사유를 즉시 생성.
#[test]
fn formatting_attribute_reasons_name_attribute_and_element_lazily() {
    for (properties, expected) in [
        ("<w:ind w:leftChars='1'/>", "Unsupported formatting attribute 'leftChars' on 'ind'"),
        ("", "Unsupported formatting attribute 'future' on 'pPr'"),
    ] {
        let body = if properties.is_empty() {
            "<w:p><w:pPr w:future='x'/></w:p>".to_owned()
        } else {
            format!("<w:p><w:pPr>{properties}</w:pPr></w:p>")
        };
        crate::warning::take_diagnostic_bytes();
        let d = read("", &body);
        let first = crate::warning::take_diagnostic_bytes();
        let repeated = read("", &body.repeat(32));
        let repeated_bytes = crate::warning::take_diagnostic_bytes();
        let warning = d.warnings.iter().find(|w| w.loss() == "format_attribute").unwrap();
        assert_eq!(warning.message(), expected);
        assert_eq!(warning.count(), 1);
        assert_eq!(
            repeated.warnings.iter().find(|w| w.loss() == "format_attribute").unwrap().count(),
            32
        );
        assert_eq!(repeated_bytes - first, 0);
    }
}

// 이것을 실패시키는 것: 잘못된 lineRule의 조기 반환이 leaf 속성 검사를 우회.
#[test]
fn invalid_line_rule_still_reports_unsupported_attributes() {
    let properties = "<w:spacing w:lineRule='invalid' w:beforeLines='100'/>";
    for definition in [true, false] {
        let d = if definition {
            read(&format!("<w:style w:type='paragraph' w:styleId='p'><w:pPr>{properties}</w:pPr></w:style>"), "<w:p/>")
        } else {
            read("", &format!("<w:p><w:pPr>{properties}</w:pPr></w:p>"))
        };
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.code() != "DOCX_FORMAT_DEFAULTED")
                .map(|w| (w.code(), w.loss(), w.action(), w.count()))
                .collect::<Vec<_>>(),
            [
                ("DOCX_UNSUPPORTED_VALUE", "format_value", "inherit_or_default", 1),
                ("DOCX_UNSUPPORTED_ATTRIBUTE", "format_attribute", "ignore", 1),
            ]
        );
        assert_eq!(
            d.warnings.iter().find(|w| w.loss() == "format_attribute").unwrap().message(),
            "Unsupported formatting attribute 'beforeLines' on 'spacing'"
        );
    }
}

// 이것을 실패시키는 것: 내장 대체 글꼴이 출력 등록에서 원본 한도를 소비하거나 원본 초과를 허용.
#[test]
fn internal_fallback_does_not_consume_source_font_limit_through_decode() {
    let mut body = String::from("<w:p/><w:p>");
    for i in 0..16_384 {
        body.push_str(&format!(
            "<w:r><w:rPr><w:rFonts w:hAnsi='f{i}' w:eastAsia='f{i}'/></w:rPr><w:t>A</w:t></w:r>"
        ));
    }
    let d = decode_docx(&package(&format!("{body}</w:p>"), &[])).unwrap();
    assert_eq!(d.registry.fonts.len(), 16_385);
    let paragraphs = &d.document.sections()[0].paragraphs;
    assert_eq!(paragraphs.len(), 2);
    assert_eq!(paragraphs[0].text_content(), "");
    assert_eq!(paragraphs[1].runs.len(), 16_384);
    let first = paragraphs[0].runs[0].char_shape_id;
    let last = paragraphs[1].runs[16_383].char_shape_id;
    assert_eq!(d.registry.char_shapes[first.get()].font, "함초롬바탕");
    assert_eq!(d.registry.char_shapes[last.get()].font, "f16383");
    body.push_str("<w:r><w:rPr><w:rFonts w:hAnsi='overflow' w:eastAsia='overflow'/></w:rPr><w:t>A</w:t></w:r></w:p>");
    assert!(matches!(
        decode_docx(&package(&body, &[])),
        Err(crate::DocxError::Limit {
            resource: "source font definitions",
            observed: 16385,
            limit: 16384,
            ..
        })
    ));
}

// 이것을 실패시키는 것: outlineLvl 숫자 오류를 문서 오류로 전파, 범위 검사/값 경고 누락, 이웃 들여쓰기 제외.
#[test]
fn optional_outline_values_recover_in_body_and_unused_definitions() {
    let defaults = "<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:hAnsi='F' w:eastAsia='F'/><w:sz w:val='20'/><w:color w:val='000000'/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:line='240' w:before='0' w:after='0'/><w:ind w:left='0' w:right='0' w:firstLine='0'/></w:pPr></w:pPrDefault></w:docDefaults>";
    for value in ["oops", "-1", "10", "4294967296", ""] {
        for definition in [true, false] {
            let outline = format!("<w:outlineLvl w:val='{value}'/>");
            let d = if definition {
                read(&format!("{defaults}<w:style w:type='paragraph' w:styleId='unused'><w:pPr>{outline}</w:pPr></w:style>"), "<w:p><w:pPr><w:ind w:left='720'/></w:pPr><w:r><w:t>A</w:t></w:r></w:p>")
            } else {
                read(defaults, &format!("<w:p><w:pPr><w:ind w:left='720'/>{outline}</w:pPr><w:r><w:t>A</w:t></w:r></w:p>"))
            };
            let p = &d.document.sections()[0].paragraphs[0];
            assert_eq!(p.text_content(), "A");
            assert_eq!(d.registry.para_shapes[p.para_shape_id.get()].indent_left.as_i32(), 3600);
            assert_eq!(p.style_ref, None);
            assert_eq!(
                d.warnings
                    .iter()
                    .map(|w| (w.code(), w.loss(), w.action(), w.count()))
                    .collect::<Vec<_>>(),
                [("DOCX_UNSUPPORTED_VALUE", "format_value", "inherit_or_default", 1)]
            );
        }
    }
    for value in ["0", "9"] {
        let d =
            read(defaults, &format!("<w:p><w:pPr><w:outlineLvl w:val='{value}'/></w:pPr></w:p>"));
        assert_eq!(
            d.warnings
                .iter()
                .map(|w| (w.code(), w.loss(), w.action(), w.count()))
                .collect::<Vec<_>>(),
            [("DOCX_UNSUPPORTED_ATTRIBUTE", "outline_level", "ignore", 1)]
        );
    }
}
