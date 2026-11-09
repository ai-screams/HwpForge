use super::tests::{package, W};
use crate::{decode_docx, DocxDecoded};
use hwpforge_core::ParagraphListRef;
use hwpforge_foundation::{NumberFormatType, NumberingIndex};

fn read(definitions: &str, body: &str) -> DocxDecoded {
    let xml = format!("<w:numbering xmlns:w='{W}'>{definitions}</w:numbering>");
    decode_docx(&package(body, &[("numbering", "numbering.xml", &xml)])).unwrap()
}
fn level(index: u32, format: &str, text: &str, extra: &str) -> String {
    format!("<w:lvl w:ilvl='{index}'><w:start w:val='1'/><w:numFmt w:val='{format}'/><w:lvlText w:val='{text}'/>{extra}</w:lvl>")
}
fn definitions(levels: &str, nums: &str) -> String {
    format!("<w:abstractNum w:abstractNumId='1'>{levels}</w:abstractNum>{nums}")
}
fn num(id: u32, abstract_id: u32, extra: &str) -> String {
    format!("<w:num w:numId='{id}'><w:abstractNumId w:val='{abstract_id}'/>{extra}</w:num>")
}
fn item(id: u32, level: u32, extra: &str, text: &str) -> String {
    format!("<w:p><w:pPr><w:numPr><w:numId w:val='{id}'/><w:ilvl w:val='{level}'/></w:numPr>{extra}</w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>")
}
fn lists(d: &DocxDecoded) -> Vec<Option<ParagraphListRef>> {
    d.document
        .sections()
        .iter()
        .flat_map(|s| &s.paragraphs)
        .map(|p| d.registry.para_shapes[p.para_shape_id.get()].list)
        .collect()
}
fn number(id: usize, level: u8) -> Option<ParagraphListRef> {
    Some(ParagraphListRef::Number { numbering_id: NumberingIndex::new(id), level })
}
fn count(d: &DocxDecoded, code: &str) -> u64 {
    d.warnings.iter().filter(|w| w.code() == code).map(|w| w.count()).sum()
}

// 이것을 실패시키는 것: 해제 경고 추가, 누락 참조 기본 번호 합성, 범위 절삭, 정수 오류 무시.
#[test]
fn list_references_and_integer_boundaries() {
    let defs = definitions(&level(0, "decimal", "%1.", ""), &num(7, 1, ""));
    for (id, ilvl, code) in [
        (0, 0, ""),
        (99, 0, "DOCX_LIST_REFERENCE_LOST"),
        (7, 1, "DOCX_LIST_REFERENCE_LOST"),
        (7, 10, "DOCX_UNSUPPORTED_VALUE"),
        (7, u32::MAX, "DOCX_UNSUPPORTED_VALUE"),
    ] {
        let d = read(&defs, &item(id, ilvl, "", "A"));
        assert_eq!(lists(&d), [None]);
        assert_eq!(count(&d, code), u64::from(!code.is_empty()));
        if id == 0 {
            assert!(!d.warnings.iter().any(|w| w.loss().starts_with("list")));
        }
    }
    let d = read(&definitions("", &num(7, 99, "")), &item(7, 0, "", "A"));
    assert_eq!(count(&d, "DOCX_LIST_REFERENCE_LOST"), 1);
    for value in ["-1".to_owned(), "bad".into(), "4294967296".into(), "0".repeat(4097)] {
        for field in ["numId", "ilvl", "outlineLvl"] {
            let prop = format!("<w:{field} w:val='{value}'/>");
            let body = format!(
                "<w:p><w:pPr>{}</w:pPr></w:p>",
                if field == "outlineLvl" { prop } else { format!("<w:numPr>{prop}</w:numPr>") }
            );
            assert!(decode_docx(&package(&body, &[])).is_err(), "{field} {value}");
        }
    }
}

// 이것을 실패시키는 것: numId 단위 분리, 다른 abstract 병합, 표/구역/본문에서 진행 초기화.
#[test]
fn numbering_groups_survive_body_and_sections() {
    let a = level(0, "decimal", "%1.", "");
    let defs = format!(
        "{}<w:abstractNum w:abstractNumId='2'>{a}</w:abstractNum>",
        definitions(&a, &format!("{}{}{}", num(7, 1, ""), num(8, 1, ""), num(9, 2, "")))
    );
    let body=format!("{}<w:p><w:r><w:t>body</w:t></w:r></w:p>{}<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>{}{}{}",item(7,0,"","A"),item(8,0,"","B"),item(9,0,"","hidden"),item(7,0,"<w:sectPr/>","C"),item(8,0,"","D"),item(9,0,"","E"));
    let d = read(&defs, &body);
    assert_eq!(
        lists(&d),
        [number(0, 0), None, number(0, 0), number(0, 0), number(0, 0), number(1, 0)]
    );
    assert_eq!(d.registry.numberings.len(), 2);
    assert_eq!(d.registry.numberings.iter().map(|n| n.start).collect::<Vec<_>>(), [1, 1]);
}

// 이것을 실패시키는 것: 시작값을 1로 고정, 하위 startOverride를 진행 손실로 처리, 비기본 재시작 허용.
#[test]
fn starts_overrides_and_partial_restarts() {
    let a = format!("{}{}", level(0, "decimal", "%1.", ""), level(1, "lowerLetter", "%2)", ""));
    let over = "<w:lvlOverride w:ilvl='0'><w:startOverride w:val='5'/></w:lvlOverride>";
    let d = read(
        &definitions(&a, &format!("{}{}", num(7, 1, ""), num(8, 1, over))),
        &format!("{}{}{}", item(7, 0, "", "A"), item(8, 0, "", "B"), item(8, 0, "", "C")),
    );
    assert_eq!(lists(&d), [number(0, 0), number(1, 0), number(1, 0)]);
    assert_eq!((d.registry.numberings[1].start, d.registry.numberings[1].levels[0].start), (5, 5));
    let d = read(
        &definitions(&a.replace("w:val='1'/><w:numFmt", "w:val='5'/><w:numFmt"), &num(7, 1, "")),
        &item(7, 0, "", "A"),
    );
    assert_eq!((d.registry.numberings[0].start, d.registry.numberings[0].levels[0].start), (5, 5));
    for extra in ["<w:lvlOverride w:ilvl='1'><w:startOverride w:val='5'/></w:lvlOverride>", ""] {
        let levels = if extra.is_empty() {
            a.replace(
                "<w:numFmt w:val='lowerLetter'/>",
                "<w:lvlRestart w:val='0'/><w:numFmt w:val='lowerLetter'/>",
            )
        } else {
            a.clone()
        };
        let d = read(
            &definitions(&levels, &num(7, 1, extra)),
            &format!(
                "{}{}{}{}",
                item(7, 0, "", "A"),
                item(7, 1, "", "B"),
                item(7, 1, "", "C"),
                item(7, 0, "", "D")
            ),
        );
        if !extra.is_empty() {
            assert_eq!(lists(&d), [number(0, 0), number(0, 1), number(0, 1), number(0, 0)]);
            assert_eq!(d.registry.numberings[0].levels[1].start, 5);
            assert_eq!(count(&d, "DOCX_LIST_NUMBERING_LOST"), 0);
            continue;
        }
        assert_eq!(lists(&d), [number(0, 0), None, None, number(0, 0)]);
        assert_eq!(count(&d, "DOCX_LIST_NUMBERING_LOST"), 2);
        assert_eq!(
            d.warnings
                .iter()
                .find(|w| w.code() == "DOCX_LIST_NUMBERING_LOST")
                .map(|w| (w.loss(), w.action())),
            Some(("numbering_sequence", "omit"))
        );
    }
    let over =
        format!("<w:lvlOverride w:ilvl='0'>{}</w:lvlOverride>", level(0, "upperRoman", "(%1)", ""));
    let d = read(&definitions(&a, &num(7, 1, &over)), &item(7, 0, "", "A"));
    assert_eq!(d.registry.numberings[0].levels[0].text, "(^1)");
    assert_eq!(d.registry.numberings[0].levels[0].num_format, NumberFormatType::RomanCapital);
}

// 이것을 실패시키는 것: 형식 잘못 대응, 자리표시자 여러 개 수용, 혼합 목록을 독립 정상 목록으로 표시.
#[test]
fn number_formats_and_templates() {
    for (format, expected) in [
        ("decimal", NumberFormatType::Digit),
        ("upperRoman", NumberFormatType::RomanCapital),
        ("lowerRoman", NumberFormatType::RomanSmall),
        ("upperLetter", NumberFormatType::LatinCapital),
        ("lowerLetter", NumberFormatType::LatinSmall),
    ] {
        let d = read(
            &definitions(&level(1, format, "(%2).", ""), &num(7, 1, "")),
            &item(7, 1, "", "A"),
        );
        assert_eq!(lists(&d), [number(0, 1)]);
        assert_eq!(
            (
                &*d.registry.numberings[0].levels[1].text,
                d.registry.numberings[0].levels[1].num_format
            ),
            ("(^2).", expected)
        );
    }
    for (format, text) in [
        ("ganada", "%1."),
        ("decimal", "%1.%2."),
        ("decimal", "%2."),
        ("decimal", "%1^2"),
        ("decimal", "fixed"),
        ("decimal", "%10."),
    ] {
        let d =
            read(&definitions(&level(0, format, text, ""), &num(7, 1, "")), &item(7, 0, "", "A"));
        assert_eq!(lists(&d), [None], "{format} {text}");
        assert_eq!(count(&d, "DOCX_LIST_NUMBERING_LOST"), 1);
    }
    let d = read(
        &definitions(
            &format!("{}{}", level(0, "decimal", "%1.", ""), level(1, "bullet", "•", "")),
            &num(7, 1, ""),
        ),
        &format!("{}{}", item(7, 0, "", "A"), item(7, 1, "", "B")),
    );
    assert_eq!(lists(&d), [None, None]);
    assert_eq!(count(&d, "DOCX_LIST_NUMBERING_LOST"), 2);
}

// 이것을 실패시키는 것: 문자만 보고 치환, 슬롯 우선순위 반전, 표 밖 기호 허용, glyph 수준 병합.
#[test]
fn bullet_mapping_and_slot_selection() {
    for (glyph, ascii, hansi, expected, approx, slots) in [
        ("&#xF0B7;", "Symbol", "Symbol", Some("•"), true, false),
        ("&#xF0A7;", "Wingdings", "Wingdings", Some("▪"), true, false),
        ("o", "Courier New", "Arial", Some("◦"), true, true),
        ("&#xF0B7;", "Arial", "sYmBoL", Some("•"), true, true),
        ("•", "Arial", "Arial", Some("•"), false, false),
        ("•", "Symbol", "Arial", Some("•"), false, true),
        ("•", "Arial", "Symbol", None, false, true),
        ("&#xF0B7;", "Symbol", "sYmBoL", Some("•"), true, true),
        ("◦", "Arial", "Arial", Some("◦"), false, false),
        ("&#xE001;", "Arial", "Arial", None, false, false),
        ("&#xF0B8;", "Symbol", "Symbol", None, false, false),
        ("A", "Wingdings", "Wingdings", None, false, false),
        ("&#xF0A7;", "Wingdings 2", "Wingdings 2", None, false, false),
        ("&#xF06C;", "Wingdings", "Wingdings", None, false, false),
        ("&#xF06E;", "Wingdings", "Wingdings", None, false, false),
        ("ab", "Arial", "Arial", None, false, false),
        ("&#xF0B7;", "", "", None, false, false),
    ] {
        let fonts = format!(
            "<w:rPr><w:rFonts w:ascii='{ascii}' w:hAnsi='{hansi}' w:eastAsia='Symbol'/></w:rPr>"
        );
        let d = read(
            &definitions(&level(0, "bullet", glyph, &fonts), &num(7, 1, "")),
            &item(7, 0, "", "A"),
        );
        assert_eq!(
            d.registry.bullets.first().map(|b| b.bullet_char.as_str()),
            expected,
            "{glyph} {ascii} {hansi}"
        );
        assert_eq!(lists(&d)[0].is_some(), expected.is_some());
        assert_eq!(count(&d, "DOCX_BULLET_GLYPH_APPROXIMATED"), u64::from(approx));
        assert_eq!(count(&d, "DOCX_BULLET_GLYPH_LOST"), u64::from(expected.is_none()));
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.loss() == "bullet_font_slots")
                .map(|w| w.count())
                .sum::<u64>(),
            u64::from(slots)
        );
    }
}

// 이것을 실패시키는 것: 이름에서 개요 추정, 번호 동시 출력, 9를 개요로 처리, 수준 상속 무시.
#[test]
fn outline_levels_override_numbering() {
    let defs = definitions(&level(0, "decimal", "%1.", ""), &num(7, 1, ""));
    for level in 0..=9 {
        let d = read(&defs, &item(7, 0, &format!("<w:outlineLvl w:val='{level}'/>"), "A"));
        assert_eq!(
            lists(&d),
            [if level < 9 { Some(ParagraphListRef::Outline { level }) } else { number(0, 0) }]
        );
        assert_eq!(count(&d, "DOCX_HEADING_NUMBER_LOST"), u64::from(level < 9));
    }
    let styles=format!("<w:styles xmlns:w='{W}'><w:style w:type='paragraph' w:styleId='named'><w:name w:val='Heading 1'/></w:style><w:style w:type='paragraph' w:styleId='outline'><w:pPr><w:outlineLvl w:val='2'/></w:pPr></w:style></w:styles>");
    let d=decode_docx(&package("<w:p><w:pPr><w:pStyle w:val='named'/></w:pPr></w:p><w:p><w:pPr><w:pStyle w:val='outline'/></w:pPr></w:p>",&[("styles","styles.xml",&styles)])).unwrap();
    assert_eq!(lists(&d), [None, Some(ParagraphListRef::Outline { level: 2 })]);
}

// 이것을 실패시키는 것: 계속 조각에 목록 복사, run 분리마다 진행, 끝 나누기 복원 때 목록 삭제.
#[test]
fn split_list_markers_only_on_first_fragment() {
    let defs = definitions(&level(0, "decimal", "%1.", ""), &num(7, 1, ""));
    for runs in [
        "<w:r><w:t>A</w:t><w:br w:type='page'/><w:t>B</w:t></w:r>",
        "<w:r><w:t>A</w:t></w:r><w:r><w:br w:type='page'/></w:r><w:r><w:t>B</w:t></w:r>",
    ] {
        let body=format!("<w:p><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr><w:spacing w:before='20' w:after='40'/><w:ind w:hanging='60'/><w:keepNext/></w:pPr>{runs}</w:p>{}",item(7,0,"<w:pageBreakBefore/>","C"));
        let d = read(&defs, &body);
        assert_eq!(lists(&d), [number(0, 0), None, number(0, 0)]);
        let p = &d.document.sections()[0].paragraphs;
        let shapes: Vec<_> =
            p.iter().map(|p| &d.registry.para_shapes[p.para_shape_id.get()]).collect();
        assert_eq!(
            shapes
                .iter()
                .take(2)
                .map(|s| (
                    s.space_before.as_i32(),
                    s.space_after.as_i32(),
                    s.indent_first_line.as_i32(),
                    s.keep_with_next
                ))
                .collect::<Vec<_>>(),
            [(100, 0, 0, false), (0, 200, 0, true)]
        );
        // The marker fragment folds the hanging indent into its left margin; the continuation keeps it.
        assert_eq!(
            shapes.iter().take(2).map(|s| s.indent_left.as_i32()).collect::<Vec<_>>(),
            [-300, 0]
        );
        let unique: std::collections::HashSet<_> = p.iter().map(|p| p.para_shape_id).collect();
        assert_eq!(d.registry.para_shapes.len(), unique.len());
    }
}

// 이것을 실패시키는 것: 첫 쪽 앞 간격 제거, continuous에도 제거, 구역 시작을 일반 경계로 결합.
#[test]
fn section_start_spacing_respects_continuous() {
    for kind in ["nextPage", "oddPage", "evenPage", "continuous"] {
        let body=format!("<w:p><w:pPr><w:spacing w:before='240'/><w:sectPr/></w:pPr></w:p><w:p><w:pPr><w:spacing w:before='240'/></w:pPr></w:p><w:sectPr><w:type w:val='{kind}'/></w:sectPr>");
        let d = read("", &body);
        let values: Vec<_> = d
            .document
            .sections()
            .iter()
            .map(|s| {
                d.registry.para_shapes[s.paragraphs[0].para_shape_id.get()].space_before.as_i32()
            })
            .collect();
        assert_eq!(values, [1200, if kind == "continuous" { 1200 } else { 0 }]);
    }
}

// 이것을 실패시키는 것: 테마 미해석, 슬롯 대체 순서 변경, eastAsia를 Latin 대응에 사용, 글머리 글꼴을 본문에 주입.
#[test]
fn bullet_theme_fallback_and_level_identity() {
    for (fonts, glyph, expected) in [
        ("w:ascii='Symbol'", "&#xF0B7;", "•"),
        ("w:hAnsi='Courier New'", "o", "◦"),
        ("w:asciiTheme='majorAscii' w:hAnsiTheme='majorHAnsi'", "&#xF0B7;", "•"),
    ] {
        let defs = definitions(
            &level(
                0,
                "bullet",
                glyph,
                &format!("<w:rPr><w:rFonts {fonts} w:eastAsia='Arial'/></w:rPr>"),
            ),
            &num(7, 1, ""),
        );
        let numbering = format!("<w:numbering xmlns:w='{W}'>{defs}</w:numbering>");
        let theme="<a:theme xmlns:a='http://schemas.openxmlformats.org/drawingml/2006/main'><a:themeElements><a:fontScheme><a:majorFont><a:latin typeface='Symbol'/></a:majorFont></a:fontScheme></a:themeElements></a:theme>";
        let d = decode_docx(&package(
            &item(7, 0, "", "A"),
            &[("numbering", "numbering.xml", &numbering), ("theme", "theme.xml", theme)],
        ))
        .unwrap();
        assert_eq!(d.registry.bullets[0].bullet_char, expected);
        let p = &d.document.sections()[0].paragraphs[0];
        assert_eq!(
            d.registry.char_shapes[p.runs[0].char_shape_id.get()].font_latin.as_deref(),
            Some("함초롬바탕")
        );
    }
    let fonts = "<w:rPr><w:rFonts w:ascii='Arial'/></w:rPr>";
    let defs = definitions(
        &format!("{}{}", level(0, "bullet", "•", fonts), level(1, "bullet", "◦", fonts)),
        &format!("{}{}", num(7, 1, ""), num(8, 1, "")),
    );
    let d = read(
        &defs,
        &format!("{}{}{}", item(7, 0, "", "A"), item(7, 1, "", "B"), item(8, 0, "", "C")),
    );
    assert_eq!(
        d.registry.bullets.iter().map(|b| b.bullet_char.as_str()).collect::<Vec<_>>(),
        ["•", "◦"]
    );
    assert_eq!(lists(&d).iter().map(|l| l.unwrap().level()).collect::<Vec<_>>(), [0, 1, 0]);
    assert_eq!(lists(&d)[0], lists(&d)[2]);
    for (text, extra) in [
        ("", fonts),
        ("•", "<w:lvlPicBulletId w:val='1'/><w:rPr><w:rFonts w:ascii='Arial'/></w:rPr>"),
        ("&#xF0B7;", "<w:rPr><w:rFonts w:eastAsia='Symbol'/></w:rPr>"),
        ("A", "<w:rPr><w:rFonts w:ascii='Unknown Marker'/></w:rPr>"),
    ] {
        let d = read(
            &definitions(&level(0, "bullet", text, extra), &num(7, 1, "")),
            &item(7, 0, "", "A"),
        );
        assert_eq!(lists(&d), [None]);
        assert_eq!(count(&d, "DOCX_BULLET_GLYPH_LOST"), 1);
    }
}

// 이것을 실패시키는 것: 정의 숫자 오류 허용, 빈 override를 독립 그룹으로 처리, 하위 수준부터 시작할 때 전역 start 오선택.
#[test]
fn numbering_definition_edges_and_initial_lower_level() {
    for tag in ["start", "lvlRestart", "lvlPicBulletId"] {
        for value in ["bad", "-1", ""] {
            let defs = definitions(
                &level(0, "decimal", "%1.", &format!("<w:{tag} w:val='{value}'/>")),
                &num(7, 1, ""),
            );
            let xml = format!("<w:numbering xmlns:w='{W}'>{defs}</w:numbering>");
            assert!(decode_docx(&package(
                &item(7, 0, "", "A"),
                &[("numbering", "numbering.xml", &xml)]
            ))
            .is_err());
        }
    }
    let defs = definitions(
        &format!("{}{}", level(0, "decimal", "%1.", ""), level(1, "decimal", "%2.", "")),
        &format!("{}{}", num(7, 1, "<w:lvlOverride w:ilvl='0'/>"), num(8, 1, "")),
    );
    let d = read(&defs, &format!("{}{}", item(7, 0, "", "A"), item(8, 0, "", "B")));
    assert_eq!(lists(&d), [number(0, 0), number(0, 0)]);
    let defs = definitions(
        &format!("{}{}", level(0, "decimal", "%1.", ""), level(1, "decimal", "%2.", "")),
        &num(7, 1, "<w:lvlOverride w:ilvl='1'><w:startOverride w:val='5'/></w:lvlOverride>"),
    );
    let d = read(&defs, &format!("{}{}", item(7, 1, "", "A"), item(7, 0, "", "B")));
    assert_eq!(lists(&d), [number(0, 1), number(0, 0)]);
    assert_eq!((d.registry.numberings[0].start, d.registry.numberings[0].levels[1].start), (1, 5));
    let d = read(&defs, &format!("{}{}", item(7, 0, "", "A"), item(7, 1, "", "B")));
    assert_eq!(lists(&d), [number(0, 0), number(0, 1)]);
    assert_eq!(d.registry.numberings[0].levels[1].start, 5);
    assert_eq!(count(&d, "DOCX_LIST_NUMBERING_LOST"), 0);
}

// 이것을 실패시키는 것: 선행/연속 빈 조각 삭제, 끝 break 이동으로 번호 중복/소실, 개요 계속 조각 유지.
#[test]
fn leading_repeated_trailing_breaks_keep_one_marker() {
    let defs = definitions(&level(0, "decimal", "%1.", ""), &num(7, 1, ""));
    for (run, expected) in [
        (
            "<w:br w:type='page'/><w:t>A</w:t><w:br w:type='page'/><w:br w:type='page'/>",
            vec![number(0, 0), None, None, number(0, 0)],
        ),
        ("<w:t>A</w:t><w:br w:type='page'/>", vec![number(0, 0), number(0, 0)]),
    ] {
        let body = format!(
            "<w:p><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr></w:pPr><w:r>{run}</w:r></w:p>{}",
            item(7, 0, "", "B")
        );
        let d = read(&defs, &body);
        assert_eq!(lists(&d), expected);
        let refs: std::collections::HashSet<_> = d
            .document
            .sections()
            .iter()
            .flat_map(|s| &s.paragraphs)
            .map(|p| p.para_shape_id)
            .collect();
        assert_eq!(d.registry.para_shapes.len(), refs.len());
    }
    let d=read("","<w:p><w:pPr><w:outlineLvl w:val='2'/></w:pPr><w:r><w:t>A</w:t><w:br w:type='page'/><w:t>B</w:t></w:r></w:p>");
    assert_eq!(lists(&d), [Some(ParagraphListRef::Outline { level: 2 }), None]);
}

// 이것을 실패시키는 것: 스타일 개요 누락, 본문/모양/이름 변경.
#[test]
fn outline_preserves_content_and_style() {
    let styles=format!("<w:styles xmlns:w='{W}'><w:style w:type='paragraph' w:styleId='p'><w:name w:val='Title'/><w:pPr><w:outlineLvl w:val='0'/><w:ind w:left='60'/></w:pPr><w:rPr><w:b/></w:rPr></w:style></w:styles>");
    let d = decode_docx(&package(
        "<w:p><w:pPr><w:pStyle w:val='p'/></w:pPr><w:r><w:t>A</w:t></w:r></w:p>",
        &[("styles", "styles.xml", &styles)],
    ))
    .unwrap();
    assert_eq!(lists(&d), [Some(ParagraphListRef::Outline { level: 0 })]);
    let p = &d.document.sections()[0].paragraphs[0];
    assert_eq!(p.text_content(), "A");
    assert_eq!(p.heading_level, None);
    assert_eq!(d.registry.style_display_name(p.style_ref.unwrap()), Some("Title"));
    assert_eq!(d.registry.para_shapes[p.para_shape_id.get()].indent_left.as_i32(), 300);
    assert!(d.registry.char_shapes[p.runs[0].char_shape_id.get()].bold);
    let style = d.registry.style_entries().values().next().unwrap();
    assert_eq!(d.registry.para_shapes[style.para_shape_id.get()].list, None);
}

// 이것을 실패시키는 것: 유효 수준 최댓값 제외, 시작 0을 1로 바꾸기, 고정 % 문자 제거, 보충 사용자 영역 허용.
#[test]
fn marker_boundary_values_remain_explicit() {
    let defs = definitions(&level(9, "decimal", "%10 50%", "<w:start w:val='0'/>"), &num(7, 1, ""));
    let d = read(&defs, &item(7, 9, "", "A"));
    assert_eq!(lists(&d), [number(0, 9)]);
    assert_eq!(d.registry.numberings[0].start, 0);
    assert_eq!(
        (
            d.registry.numberings[0].levels[9].start,
            d.registry.numberings[0].levels[9].text.as_str()
        ),
        (0, "^10 50%")
    );
    for (glyph, font) in [
        ("&#xF0000;", "Arial"),
        ("&#x100000;", "Arial"),
        ("&#x9;", "Arial"),
        (" ", "Arial"),
        ("A", "Wingdings 3"),
        ("A", "Webdings"),
        ("A", "Zapf Dingbats"),
        ("A", "MT Extra"),
    ] {
        let d = read(
            &definitions(
                &level(0, "bullet", glyph, &format!("<w:rPr><w:rFonts w:ascii='{font}'/></w:rPr>")),
                &num(7, 1, ""),
            ),
            &item(7, 0, "", "A"),
        );
        assert_eq!(lists(&d), [None]);
        assert_eq!(count(&d, "DOCX_BULLET_GLYPH_LOST"), 1);
    }
    for body in [
        "<w:p><w:pPr><w:numPr/></w:pPr></w:p>",
        "<w:p><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr></w:pPr></w:p>",
    ] {
        let d = read("<w:num w:numId='7'/>", body);
        assert_eq!(lists(&d), [None]);
        assert_eq!(count(&d, "DOCX_LIST_REFERENCE_LOST"), 1);
    }
}

// 이것을 실패시키는 것: 개요 문단의 원본 번호 진행 누락, 후속 손실 경고 중복 또는 생략.
#[test]
fn numbered_outline_consumes_source_sequence() {
    let defs = definitions(&level(0, "decimal", "%1.", ""), &num(7, 1, ""));
    for first in [true, false] {
        let body = format!(
            "{}{}{}{}",
            if first { String::new() } else { item(7, 0, "", "A") },
            item(7, 0, "<w:outlineLvl w:val='0'/>", "H"),
            item(7, 0, "", "B"),
            item(7, 0, "", "C")
        );
        let d = read(&defs, &body);
        let expected = if first {
            vec![Some(ParagraphListRef::Outline { level: 0 }), None, None]
        } else {
            vec![number(0, 0), Some(ParagraphListRef::Outline { level: 0 }), None, None]
        };
        assert_eq!(lists(&d), expected);
        assert_eq!(count(&d, "DOCX_HEADING_NUMBER_LOST"), 1);
        let loss = d.warnings.iter().find(|w| w.code() == "DOCX_LIST_NUMBERING_LOST").unwrap();
        assert_eq!((loss.loss(), loss.action(), loss.count()), ("numbering_sequence", "omit", 2));
        if !first {
            assert_eq!(d.registry.numberings[0].start, 1);
        }
    }
}

// 이것을 실패시키는 것: singleLevel 누락 수준을 별도 진행으로 취급, 들여쓰기 조회 불일치, 명시 수준 덮어쓰기.
#[test]
fn single_level_missing_level_uses_zero_consistently() {
    let base = level(0, "decimal", "%1.", "<w:pPr><w:ind w:left='720'/></w:pPr>");
    for kind in ["singleLevel", "multilevel"] {
        let defs =
            definitions(&format!("<w:multiLevelType w:val='{kind}'/>{base}"), &num(7, 1, ""));
        let body = [0, 0, 1, 1, 0].iter().map(|&l| item(7, l, "", "A")).collect::<String>();
        let d = read(&defs, &body);
        if kind == "singleLevel" {
            assert_eq!(lists(&d), [number(0, 0); 5]);
            assert_eq!(d.registry.numberings[0].start, 1);
            for p in &d.document.sections()[0].paragraphs {
                assert_eq!(
                    d.registry.para_shapes[p.para_shape_id.get()].indent_left.as_i32(),
                    3600
                );
            }
            assert_eq!(count(&d, "DOCX_LIST_REFERENCE_LOST"), 0);
        } else {
            assert_eq!(lists(&d), [number(0, 0), number(0, 0), None, None, number(0, 0)]);
            assert_eq!(count(&d, "DOCX_LIST_REFERENCE_LOST"), 2);
        }
    }
    for over in [false, true] {
        let second = level(1, "decimal", "%2.", "<w:pPr><w:ind w:left='1440'/></w:pPr>");
        let levels = format!(
            "<w:multiLevelType w:val='singleLevel'/>{base}{}",
            if over { "" } else { &second }
        );
        let extra = if over {
            format!("<w:lvlOverride w:ilvl='1'>{second}</w:lvlOverride>")
        } else {
            String::new()
        };
        let d = read(&definitions(&levels, &num(7, 1, &extra)), &item(7, 1, "", "A"));
        assert_eq!(lists(&d), [number(0, 1)]);
        assert_eq!(
            d.registry.para_shapes[d.document.sections()[0].paragraphs[0].para_shape_id.get()]
                .indent_left
                .as_i32(),
            7200
        );
    }
}

// 이것을 실패시키는 것: 기본 lvlRestart를 손실 처리하거나 비기본 재시작을 허용.
#[test]
fn explicit_default_restart_keeps_markers() {
    for value in [0, 1, 2] {
        let defs = definitions(
            &format!(
                "{}{}",
                level(0, "decimal", "%1.", ""),
                level(1, "decimal", "%2.", &format!("<w:lvlRestart w:val='{value}'/>"))
            ),
            &num(7, 1, ""),
        );
        let d = read(
            &defs,
            &format!("{}{}{}", item(7, 0, "", "A"), item(7, 1, "", "B"), item(7, 1, "", "C")),
        );
        assert_eq!(
            lists(&d),
            if value == 1 {
                vec![number(0, 0), number(0, 1), number(0, 1)]
            } else {
                vec![number(0, 0), None, None]
            }
        );
        assert_eq!(count(&d, "DOCX_LIST_NUMBERING_LOST"), if value == 1 { 0 } else { 2 });
    }
}

// 이것을 실패시키는 것: numStyleLink 무시, 두 번째 연결 추적, 잘못된 스타일 종류 수용, 식별자 상한 누락.
#[test]
fn numbering_style_link_follows_one_definition() {
    for (kind, target, expected) in [
        ("numbering", "2", true),
        ("paragraph", "2", false),
        ("numbering", "7", false),
        ("numbering", "99", false),
    ] {
        let styles = format!("<w:styles xmlns:w='{W}'><w:style w:type='{kind}' w:styleId='linked'><w:pPr><w:numPr><w:numId w:val='{target}'/></w:numPr></w:pPr></w:style></w:styles>");
        for deeper in [false, true] {
            let dest = if deeper {
                "<w:numStyleLink w:val='linked'/>".to_owned()
            } else {
                level(
                    0,
                    "decimal",
                    "%1.",
                    "<w:start w:val='3'/><w:pPr><w:ind w:left='720'/></w:pPr>",
                )
            };
            let numbering = format!("<w:numbering xmlns:w='{W}'><w:abstractNum w:abstractNumId='1'><w:numStyleLink w:val='linked'/></w:abstractNum><w:abstractNum w:abstractNumId='2'>{dest}</w:abstractNum>{}{}</w:numbering>", num(7,1,""), num(2,2,""));
            let d = decode_docx(&package(
                &item(7, 0, "", "A"),
                &[("styles", "styles.xml", &styles), ("numbering", "numbering.xml", &numbering)],
            ))
            .unwrap();
            assert_eq!(lists(&d), [if expected && !deeper { number(0, 0) } else { None }]);
            if expected && !deeper {
                assert_eq!(d.registry.numberings[0].start, 3);
                assert_eq!(
                    d.registry.para_shapes
                        [d.document.sections()[0].paragraphs[0].para_shape_id.get()]
                    .indent_left
                    .as_i32(),
                    3600
                );
                assert!(!d.warnings.iter().any(|w| w.loss() == "numbering_style_link"));
            } else {
                assert!(d.warnings.iter().any(|w| w.loss() == "numbering_style_link"));
            }
        }
    }
    let xml = format!("<w:numbering xmlns:w='{W}'><w:abstractNum w:abstractNumId='1'><w:numStyleLink w:val='{}'/></w:abstractNum></w:numbering>", "x".repeat(4097));
    assert!(matches!(
        decode_docx(&package("<w:p/>", &[("numbering", "numbering.xml", &xml)])),
        Err(crate::DocxError::Limit { .. })
    ));
}

// 이것을 실패시키는 것: 출력되지 않은 상위 번호가 하위 번호를 재시작한 것으로 처리, 출력한 상위 번호의 정상 재시작 누락.
#[test]
fn outline_sequence_loss_respects_level_resets() {
    let defs = definitions(
        &format!("{}{}", level(0, "decimal", "%1.", ""), level(1, "decimal", "%2.", "")),
        &num(7, 1, ""),
    );
    let d = read(
        &defs,
        &format!(
            "{}{}{}{}",
            item(7, 1, "", "A"),
            item(7, 0, "<w:outlineLvl w:val='0'/>", "H"),
            item(7, 0, "", "B"),
            item(7, 1, "", "C")
        ),
    );
    assert_eq!(lists(&d), [number(0, 1), Some(ParagraphListRef::Outline { level: 0 }), None, None]);
    assert_eq!(count(&d, "DOCX_LIST_NUMBERING_LOST"), 2);
    let d = read(
        &defs,
        &format!(
            "{}{}{}{}",
            item(7, 1, "<w:outlineLvl w:val='0'/>", "H"),
            item(7, 1, "", "A"),
            item(7, 0, "", "B"),
            item(7, 1, "", "C")
        ),
    );
    assert_eq!(
        lists(&d),
        [Some(ParagraphListRef::Outline { level: 0 }), None, number(0, 0), number(0, 1)]
    );
    assert_eq!(count(&d, "DOCX_LIST_NUMBERING_LOST"), 1);
}

// 이것을 실패시키는 것: 연결 두 단계를 따라감, 없는 스타일/번호를 합성, 자체 수준보다 연결을 우선.
#[test]
fn numbering_links_reject_missing_and_second_hops() {
    let styles = format!("<w:styles xmlns:w='{W}'><w:style w:type='numbering' w:styleId='one'><w:pPr><w:numPr><w:numId w:val='2'/></w:numPr></w:pPr></w:style><w:style w:type='numbering' w:styleId='two'><w:pPr><w:numPr><w:numId w:val='3'/></w:numPr></w:pPr></w:style></w:styles>");
    for (link, own) in [("one", false), ("missing", false), ("one", true)] {
        let numbering = format!("<w:numbering xmlns:w='{W}'><w:abstractNum w:abstractNumId='1'><w:numStyleLink w:val='{link}'/>{}</w:abstractNum><w:abstractNum w:abstractNumId='2'><w:numStyleLink w:val='two'/></w:abstractNum><w:abstractNum w:abstractNumId='3'>{}</w:abstractNum>{}{}{}</w:numbering>", if own {level(0,"decimal","(%1)","")} else {String::new()}, level(0,"decimal","%1.",""),num(7,1,""),num(2,2,""),num(3,3,""));
        let d = decode_docx(&package(
            &item(7, 0, "", "A"),
            &[("numbering", "numbering.xml", &numbering), ("styles", "styles.xml", &styles)],
        ))
        .unwrap();
        assert_eq!(lists(&d), [if own { number(0, 0) } else { None }]);
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.loss() == "numbering_style_link")
                .map(|w| w.count())
                .sum::<u64>(),
            if own { 0 } else { 1 }
        );
        if own {
            assert_eq!(d.registry.numberings[0].levels[0].text, "(^1)");
        }
    }
}

// 이것을 실패시키는 것: singleLevel 보정 전 수준 bit로 개요 진행 기록, 글머리 개요를 번호 진행 손실로 처리.
#[test]
fn outline_observation_uses_effective_number_level() {
    let defs = definitions(
        &format!("<w:multiLevelType w:val='singleLevel'/>{}", level(0, "decimal", "%1.", "")),
        &num(7, 1, ""),
    );
    let d = read(
        &defs,
        &format!("{}{}", item(7, 1, "<w:outlineLvl w:val='0'/>", "H"), item(7, 0, "", "A")),
    );
    assert_eq!(lists(&d), [Some(ParagraphListRef::Outline { level: 0 }), None]);
    assert_eq!(count(&d, "DOCX_LIST_NUMBERING_LOST"), 1);
    let defs = definitions(
        &level(0, "bullet", "•", "<w:rPr><w:rFonts w:ascii='Arial'/></w:rPr>"),
        &num(7, 1, ""),
    );
    let d = read(
        &defs,
        &format!("{}{}", item(7, 0, "<w:outlineLvl w:val='0'/>", "H"), item(7, 0, "", "A")),
    );
    assert!(matches!(lists(&d)[1], Some(ParagraphListRef::Bullet { level: 0, .. })));
    assert_eq!(count(&d, "DOCX_LIST_NUMBERING_LOST"), 0);
}

// 이것을 실패시키는 것: 진행 손실로 빠진 상위 문단의 관측 생략, 미지원/누락 표식의 하위 손실 미전파, 경고 중복.
#[test]
fn omitted_ancestors_propagate_source_restarts() {
    for (format, text) in
        [("decimal", "%1."), ("decimal", "%1.%2."), ("ganada", "%1."), ("missing", "")]
    {
        let top = if format == "missing" { String::new() } else { level(0, format, text, "") };
        let defs =
            definitions(&format!("{}{}", top, level(1, "decimal", "%2.", "")), &num(7, 1, ""));
        let d = read(
            &defs,
            &format!(
                "{}{}{}{}",
                item(7, 0, "<w:outlineLvl w:val='0'/>", "H"),
                item(7, 1, "", "A"),
                item(7, 0, "", "B"),
                item(7, 1, "", "C")
            ),
        );
        assert_eq!(
            lists(&d),
            [Some(ParagraphListRef::Outline { level: 0 }), number(0, 1), None, None],
            "{format} {text}"
        );
        assert_eq!(count(&d, "DOCX_HEADING_NUMBER_LOST"), 1);
        assert_eq!(
            d.warnings
                .iter()
                .filter(
                    |w| w.code() == "DOCX_LIST_NUMBERING_LOST" && w.loss() == "numbering_sequence"
                )
                .map(|w| w.count())
                .sum::<u64>(),
            if format == "missing" { 1 } else { 2 }
        );
    }
    for (format, text) in [("decimal", "%1.%2."), ("ganada", "%1.")] {
        let defs = definitions(
            &format!("{}{}", level(0, format, text, ""), level(1, "decimal", "%2.", "")),
            &num(7, 1, ""),
        );
        let d = read(
            &defs,
            &format!(
                "{}{}{}{}",
                item(7, 0, "", "H"),
                item(7, 1, "", "A"),
                item(7, 0, "", "B"),
                item(7, 1, "", "C")
            ),
        );
        assert_eq!(lists(&d), [None, number(0, 1), None, None]);
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.code() == "DOCX_LIST_NUMBERING_LOST")
                .map(|w| (w.loss(), w.count()))
                .collect::<Vec<_>>(),
            [("numbering_format", 1), ("numbering_sequence", 2)]
        );
    }
}

// 이것을 실패시키는 것: 수준 0의 명시적 재시작 0을 비기본 재시작으로 제외.
#[test]
fn zero_restart_at_level_zero_is_default() {
    let d = read(
        &definitions(&level(0, "decimal", "%1.", "<w:lvlRestart w:val='0'/>"), &num(7, 1, "")),
        &format!("{}{}", item(7, 0, "", "A"), item(7, 0, "", "B")),
    );
    assert_eq!(lists(&d), [number(0, 0), number(0, 0)]);
    assert_eq!(count(&d, "DOCX_LIST_NUMBERING_LOST"), 0);
}

// 이것을 실패시키는 것: 미지원/비기본 재시작의 하위 손실 미전파, 생략된 상위에서 회복, 정상 상위 출력에서도 손실 유지.
#[test]
fn emitted_ancestor_recovers_omitted_marker_descendants() {
    for (text, extra, loss) in [
        ("%2.%3.", "", "numbering_format"),
        ("%2.", "<w:lvlRestart w:val='0'/>", "numbering_sequence"),
    ] {
        let defs = definitions(
            &format!(
                "{}{}{}",
                level(0, "decimal", "%1.", ""),
                level(1, "decimal", text, extra),
                level(2, "decimal", "%3.", "")
            ),
            &num(7, 1, ""),
        );
        let d = read(
            &defs,
            &format!(
                "{}{}{}{}{}",
                item(7, 2, "", "A"),
                item(7, 1, "", "B"),
                item(7, 2, "", "C"),
                item(7, 0, "", "D"),
                item(7, 2, "", "E")
            ),
        );
        assert_eq!(lists(&d), [number(0, 2), None, None, number(0, 0), number(0, 2)]);
        assert_eq!(count(&d, "DOCX_LIST_NUMBERING_LOST"), 2);
        assert_eq!(
            d.warnings.iter().find(|w| w.code() == "DOCX_LIST_NUMBERING_LOST").unwrap().loss(),
            loss
        );
    }
}

// 이것을 실패시키는 것: 접기를 빼기, 표식 없는 문단·개요까지 접기, 양수 첫 줄도 접기, 글머리 제외.
#[test]
fn marker_paragraphs_fold_hanging_indent_into_left_margin() {
    let defs = format!(
        "<w:abstractNum w:abstractNumId='1'>{}</w:abstractNum><w:abstractNum w:abstractNumId='2'><w:lvl w:ilvl='0'><w:numFmt w:val='bullet'/><w:lvlText w:val='•'/><w:rPr><w:rFonts w:ascii='Arial' w:hAnsi='Arial'/></w:rPr></w:lvl></w:abstractNum>{}{}",
        level(0, "decimal", "%1.", ""),
        num(7, 1, ""),
        num(8, 2, "")
    );
    let ind = "<w:ind w:left='720' w:hanging='360'/>";
    let body = format!(
        "{}{}<w:p><w:pPr>{ind}</w:pPr><w:r><w:t>plain</w:t></w:r></w:p>{}<w:p><w:pPr><w:outlineLvl w:val='0'/>{ind}</w:pPr><w:r><w:t>head</w:t></w:r></w:p>",
        item(7, 0, ind, "number"),
        item(8, 0, ind, "bullet"),
        item(7, 0, "<w:ind w:left='720' w:firstLine='360'/>", "positive"),
    );
    let d = read(&defs, &body);
    let got: Vec<_> = d.document.sections()[0]
        .paragraphs
        .iter()
        .map(|p| {
            let s = &d.registry.para_shapes[p.para_shape_id.get()];
            (s.indent_left.as_i32(), s.indent_first_line.as_i32())
        })
        .collect();
    assert_eq!(got, [(1800, 0), (1800, 0), (3600, -1800), (3600, 1800), (3600, -1800)]);
}
