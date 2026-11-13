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
    let body=format!("{}<w:p><w:r><w:t>body</w:t></w:r></w:p>{}<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>{}{}{}",item(7,0,"","A"),item(8,0,"","B"),item(9,0,"","cell"),item(7,0,"<w:sectPr/>","C"),item(8,0,"","D"),item(9,0,"","E"));
    let d = read(&defs, &body);
    // 이것을 실패시키는 것: 구역 마지막 목록 문단 앞에 합성 표 holder를 남기기.
    assert_eq!(
        lists(&d),
        [number(0, 0), None, number(0, 0), number(0, 0), number(0, 0), number(1, 0)]
    );
    assert_eq!(d.registry.numberings.len(), 2);
    assert_eq!(d.registry.numberings.iter().map(|n| n.start).collect::<Vec<_>>(), [1, 1]);
}

// 이것을 실패시키는 것: override 목록 공유, 시작값 1 고정, 하위 startOverride 손실 또는 비기본 재시작 허용.
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
        ("&#xF06C;", "Wingdings", "Wingdings", Some("●"), true, false),
        ("&#xF06E;", "Wingdings", "Wingdings", Some("■"), true, false),
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

fn marker_shape(d: &DocxDecoded, index: usize) -> &hwpforge_blueprint::style::ParaShape {
    &d.registry.para_shapes[d.document.sections()[0].paragraphs[index].para_shape_id.get()]
}
fn layout_input(extra: &str, direct: &str) -> DocxDecoded {
    read(
        &definitions(
            &level(
                0,
                "decimal",
                "%1.",
                &format!("<w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr>{extra}"),
            ),
            &num(7, 1, ""),
        ),
        &item(7, 0, direct, "body"),
    )
}
// 이것을 실패시키는 것: 내어쓰기를 접은 뒤 간격 계산 또는 수준 들여쓰기만 사용.
#[test]
fn word_list_hanging_becomes_anchor_and_gap() {
    use hwpforge_blueprint::{ListMarkerAlign, ListTextStart};
    for (direct, anchor, gap) in [
        ("", 1800, 1800),
        ("<w:ind w:left='960' w:hanging='480'/>", 2400, 2400),
        ("<w:ind w:left='720' w:hanging='480'/>", 1200, 2400),
    ] {
        let d = layout_input("", direct);
        let s = marker_shape(&d, 0);
        let m = s.list_marker.unwrap();
        assert_eq!(s.indent_left.as_i32(), anchor);
        assert_eq!(s.indent_first_line.as_i32(), 0);
        assert_eq!(m.align, ListMarkerAlign::Left);
        assert_eq!(
            m.text_start,
            ListTextStart::FromAnchor(hwpforge_foundation::HwpUnit::new(gap).unwrap())
        );
        assert!(m.char_shape.is_some());
    }
}
// 이것을 실패시키는 것: end를 Left로 처리하거나 center 무시, 미지원 값의 경고 생략.
#[test]
fn word_list_alignment_and_suffix_defaults() {
    use hwpforge_blueprint::ListMarkerAlign::*;
    for (value, expected, warn) in [
        ("left", Left, false),
        ("start", Left, false),
        ("end", Right, false),
        ("right", Right, false),
        ("center", Center, false),
        ("bad", Left, true),
    ] {
        let d = layout_input(&format!("<w:lvlJc w:val='{value}'/>"), "");
        assert_eq!(marker_shape(&d, 0).list_marker.unwrap().align, expected);
        assert_eq!(d.warnings.iter().any(|w| w.loss() == "list_marker_alignment"), warn);
    }
}
// 이것을 실패시키는 것: space를 비율 offset으로 바꾸거나 nothing의 명시적 0 제거.
#[test]
fn word_suffix_space_and_nothing() {
    use hwpforge_blueprint::ListTextStart;
    for (suffix, size, distance) in
        [("space", 22, 306), ("space", 28, 389), ("space", 24, 334), ("nothing", 22, 0)]
    {
        let d = layout_input(
            &format!("<w:suff w:val='{suffix}'/><w:rPr><w:sz w:val='{size}'/></w:rPr>"),
            "",
        );
        assert_eq!(
            marker_shape(&d, 0).list_marker.unwrap().text_start,
            ListTextStart::AfterMarker(hwpforge_foundation::HwpUnit::new(distance).unwrap())
        );
        assert!(d.warnings.iter().any(|w| w.loss() == "list_marker_continuation"));
        assert_eq!(
            d.warnings.iter().any(|w| w.loss() == "list_marker_space_width"),
            suffix == "space"
        );
    }
}
// 이것을 실패시키는 것: L 뒤 탭을 항상 선택하거나 clear 무시.
#[test]
fn word_number_tab_selects_earlier_text_start() {
    use hwpforge_blueprint::ListTextStart;
    for (pos, gap, continuation) in [(600, 1200, true), (840, 1800, false), (360, 1800, false)] {
        let extra=format!("<w:pPr><w:ind w:left='720' w:hanging='360'/><w:tabs><w:tab w:val='num' w:pos='{pos}'/></w:tabs></w:pPr>");
        let d = read(
            &definitions(&level(0, "decimal", "%1.", &extra), &num(7, 1, "")),
            &item(7, 0, "", "body"),
        );
        assert_eq!(
            marker_shape(&d, 0).list_marker.unwrap().text_start,
            ListTextStart::FromAnchor(hwpforge_foundation::HwpUnit::new(gap).unwrap())
        );
        assert_eq!(d.warnings.iter().any(|w| w.loss() == "list_marker_continuation"), continuation);
        assert_eq!(d.warnings.iter().any(|w| w.loss() == "list_marker_tab"), pos == 360);
    }
    let d=layout_input("<w:pPr><w:ind w:left='720' w:hanging='360'/><w:tabs><w:tab w:val='num' w:pos='600'/><w:tab w:val='num' w:pos='500'/></w:tabs></w:pPr>","");
    assert_eq!(
        marker_shape(&d, 0).list_marker.unwrap().text_start,
        hwpforge_blueprint::ListTextStart::FromAnchor(
            hwpforge_foundation::HwpUnit::new(700).unwrap()
        )
    );
}
// 이것을 실패시키는 것: 번호 문자 속성을 본문 Delta에 합치기.
#[test]
fn word_marker_rpr_does_not_change_body() {
    let d = layout_input(
        "<w:rPr><w:rFonts w:ascii='Arial' w:hAnsi='Arial'/><w:sz w:val='28'/><w:b/></w:rPr>",
        "",
    );
    let p = &d.document.sections()[0].paragraphs[0];
    let body = &d.registry.char_shapes[p.runs[0].char_shape_id.get()];
    let marker =
        &d.registry.char_shapes[marker_shape(&d, 0).list_marker.unwrap().char_shape.unwrap().get()];
    assert_eq!(marker.size.as_i32(), 1400);
    assert!(marker.bold);
    assert!(!body.bold);
    assert_ne!(marker.font, body.font);
    assert!(d.warnings.iter().any(|w| w.loss() == "list_marker_line_height"));
    assert!(!d.warnings.iter().any(|w| w.loss() == "marker_format"));
}
// 이것을 실패시키는 것: 글꼴 조건 없이 PUA 치환 또는 Wingdings 출력 폰트 재사용.
#[test]
fn wingdings_circle_and_square_are_mapped() {
    for (font, glyph, expected) in [
        ("Wingdings", "&#xF06C;", Some("●")),
        ("wingdings", "&#xF06E;", Some("■")),
        ("Wingdings 2", "&#xF06C;", None),
        ("Wingdings 2", "&#xF06E;", None),
    ] {
        let d = read(
            &definitions(
                &level(
                    0,
                    "bullet",
                    glyph,
                    &format!("<w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr><w:rPr><w:rFonts w:ascii='{font}' w:hAnsi='{font}'/></w:rPr>"),
                ),
                &num(7, 1, ""),
            ),
            &item(7, 0, "", "body"),
        );
        assert_eq!(d.registry.bullets.first().map(|b| b.bullet_char.as_str()), expected);
        assert_eq!(count(&d, "DOCX_BULLET_GLYPH_APPROXIMATED"), u64::from(expected.is_some()));
        assert_eq!(count(&d, "DOCX_BULLET_GLYPH_LOST"), u64::from(expected.is_none()));
        if expected.is_some() {
            assert!(d
                .warnings
                .iter()
                .find(|w| w.code() == "DOCX_BULLET_GLYPH_APPROXIMATED")
                .unwrap()
                .message()
                .contains("paragraph's ordinary font"));
            let m = marker_shape(&d, 0).list_marker.unwrap();
            assert_ne!(
                d.registry.char_shapes[m.char_shape.unwrap().get()].font.to_lowercase(),
                "wingdings"
            );
        }
    }
}

// 이것을 실패시키는 것: 수준 ind로 문단 직접 ind를 대체.
#[test]
fn word_list_direct_indent_overrides_level() {
    let d = layout_input("", "<w:ind w:left='960' w:hanging='480'/>");
    assert_eq!(marker_shape(&d, 0).indent_left.as_i32(), 2400);
    assert_eq!(
        marker_shape(&d, 0).list_marker.unwrap().text_start,
        hwpforge_blueprint::ListTextStart::FromAnchor(
            hwpforge_foundation::HwpUnit::new(2400).unwrap()
        )
    );
    let extra="<w:pPr><w:ind w:left='720' w:hanging='360'/><w:tabs><w:tab w:val='num' w:pos='600'/></w:tabs></w:pPr>";
    let d = read(
        &definitions(&level(0, "decimal", "%1.", extra), &num(7, 1, "")),
        &item(7, 0, "<w:ind w:left='960' w:hanging='480'/>", "body"),
    );
    assert_eq!(
        marker_shape(&d, 0).list_marker.unwrap().text_start,
        hwpforge_blueprint::ListTextStart::FromAnchor(
            hwpforge_foundation::HwpUnit::new(600).unwrap()
        )
    );
}
// 이것을 실패시키는 것: override clear 무시 또는 외부 namespace의 tab 수용.
#[test]
fn word_number_tab_override_and_clear() {
    let base=level(0,"decimal","%1.","<w:pPr><w:ind w:left='720' w:hanging='360'/><w:tabs><w:tab w:val='num' w:pos='600'/></w:tabs></w:pPr>");
    for tab in [
        "<w:tab w:val='clear' w:pos='600'/>",
        "<w:tab w:val='clear' w:pos='600'/><x:tab xmlns:x='urn:foreign' w:val='num' w:pos='500'/>",
        "<w:tab w:val='clear' w:pos='600'/></w:tabs><x:tabs xmlns:x='urn:foreign'><w:tab w:val='num' w:pos='500'/></x:tabs><w:tabs>",
    ] {
        let over = format!(
            "<w:lvlOverride w:ilvl='0'>{}</w:lvlOverride>",
            level(0, "decimal", "%1.", &format!("<w:pPr><w:tabs>{tab}</w:tabs></w:pPr>"))
        );
        let d = read(&definitions(&base, &num(7, 1, &over)), &item(7, 0, "", "body"));
        assert_eq!(
            marker_shape(&d, 0).list_marker.unwrap().text_start,
            hwpforge_blueprint::ListTextStart::FromAnchor(
                hwpforge_foundation::HwpUnit::new(1800).unwrap()
            )
        );
    }
}
// 이것을 실패시키는 것: 겹침 위험 경고 대신 고정 슬롯 자동 확대.
#[test]
fn word_list_overflow_is_only_diagnosed() {
    for (start, expected) in [(9, 0), (99, 1)] {
        let l = level(
            0,
            "decimal",
            "%1.",
            "<w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr><w:rPr><w:sz w:val='22'/></w:rPr>",
        )
        .replace("w:val='1'", &format!("w:val='{start}'"));
        let d = read(
            &definitions(&l, &num(7, 1, "")),
            &format!("{}{}", item(7, 0, "", "a"), item(7, 0, "", "b")),
        );
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.loss() == "list_marker_overflow")
                .map(|w| w.count())
                .sum::<u64>(),
            expected
        );
        assert_eq!(
            marker_shape(&d, 1).list_marker.unwrap().text_start,
            hwpforge_blueprint::ListTextStart::FromAnchor(
                hwpforge_foundation::HwpUnit::new(1800).unwrap()
            )
        );
    }
    let l = level(
        0,
        "decimal",
        "%1.",
        "<w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr><w:rPr><w:sz w:val='22'/></w:rPr>",
    );
    let d = read(
        &definitions(
            &l,
            &num(7, 1, "<w:lvlOverride w:ilvl='0'><w:startOverride w:val='99'/></w:lvlOverride>"),
        ),
        &format!("{}{}", item(7, 0, "", "a"), item(7, 0, "", "b")),
    );
    assert_eq!(
        d.warnings
            .iter()
            .filter(|w| w.loss() == "list_marker_overflow")
            .map(|w| w.count())
            .sum::<u64>(),
        1
    );
}
// 이것을 실패시키는 것: 파싱 시점에 false를 즉시 경고하거나 cascade에서 true 무시.
#[test]
fn word_auto_spacing_false_is_diagnosed_after_cascade() {
    for (props, expected) in [
        ("<w:autoSpaceDE w:val='false'/><w:autoSpaceDN/>", true),
        ("<w:autoSpaceDE w:val='false'/><w:autoSpaceDE/>", false),
    ] {
        let d = layout_input("", props);
        assert_eq!(d.warnings.iter().any(|w| w.loss() == "auto_space_de"), expected);
        assert!(!d.warnings.iter().any(|w| w.loss() == "auto_space_dn"));
    }
}

// 이것을 실패시키는 것: 기본 수준 없는 override를 배치 해석에서 누락.
#[test]
fn word_marker_override_can_define_missing_level() {
    let over = format!(
        "<w:lvlOverride w:ilvl='0'>{}</w:lvlOverride>",
        level(0, "decimal", "%1.", "<w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr>")
    );
    let d = read(&definitions("", &num(7, 1, &over)), &item(7, 0, "", "body"));
    assert_eq!(
        marker_shape(&d, 0).list_marker.unwrap().text_start,
        hwpforge_blueprint::ListTextStart::FromAnchor(
            hwpforge_foundation::HwpUnit::new(1800).unwrap()
        )
    );
}
// 이것을 실패시키는 것: 일반 Unicode 글머리의 명시적 일반 폰트까지 제거.
#[test]
fn word_unicode_bullet_retains_ordinary_marker_font() {
    let d = read(
        &definitions(
            &level(0, "bullet", "•", "<w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr><w:rPr><w:rFonts w:ascii='Arial' w:hAnsi='Arial'/></w:rPr>"),
            &num(7, 1, ""),
        ),
        &item(7, 0, "", "body"),
    );
    let m = marker_shape(&d, 0).list_marker.unwrap();
    assert_eq!(d.registry.char_shapes[m.char_shape.unwrap().get()].font, "Arial");
}

// 이것을 실패시키는 것: 양수 first 무시 또는 상한 합 검사 선행.
#[test]
fn word_marker_indent_edges_keep_legacy_layout() {
    let d = layout_input("", "<w:ind w:left='720' w:firstLine='100'/>");
    assert!(marker_shape(&d, 0).list_marker.is_none());
    assert_eq!(marker_shape(&d, 0).indent_first_line.as_i32(), 500);
    assert!(d.warnings.iter().any(|w| w.loss() == "list_marker_indent"));
    let d = layout_input("", "<w:ind w:left='20000000' w:firstLine='20000000'/>");
    assert!(marker_shape(&d, 0).list_marker.is_none());
    assert_eq!(marker_shape(&d, 0).indent_left.as_i32(), 100_000_000);
    assert_eq!(marker_shape(&d, 0).indent_first_line.as_i32(), 100_000_000);
    assert!(d.warnings.iter().any(|w| w.loss() == "list_marker_indent"));
    let d = layout_input("<w:suff w:val='bad'/>", "");
    assert!(d.warnings.iter().any(|w| w.loss() == "list_marker_suffix"));
    assert_eq!(
        marker_shape(&d, 0).list_marker.unwrap().text_start,
        hwpforge_blueprint::ListTextStart::FromAnchor(
            hwpforge_foundation::HwpUnit::new(1800).unwrap()
        )
    );
}
// 이것을 실패시키는 것: 첫 본문 run의 직접 서식으로 번호 모양을 대체하거나 동일 폰트도 근사 경고.
#[test]
fn word_marker_character_context_ignores_first_run_format() {
    let defs = definitions(
        &level(
            0,
            "decimal",
            "%1.",
            "<w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr><w:rPr><w:rFonts w:ascii='함초롬바탕' w:hAnsi='함초롬바탕'/></w:rPr>",
        ),
        &num(7, 1, ""),
    );
    let d=read(&defs,"<w:p><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr><w:rPr><w:sz w:val='24'/></w:rPr></w:pPr><w:r><w:rPr><w:sz w:val='60'/><w:b/></w:rPr><w:t>body</w:t></w:r></w:p>");
    let m = marker_shape(&d, 0).list_marker.unwrap();
    let marker = &d.registry.char_shapes[m.char_shape.unwrap().get()];
    assert_eq!(marker.size.as_i32(), 1200);
    assert!(!marker.bold);
    let body =
        &d.registry.char_shapes[d.document.sections()[0].paragraphs[0].runs[0].char_shape_id.get()];
    assert_eq!(body.size.as_i32(), 3000);
    assert!(body.bold);
    assert!(!d.warnings.iter().any(|w| w.loss() == "list_marker_line_height"));
}
// 이것을 실패시키는 것: 수준 탭 예산 또는 중복 진단 누락, 일반 tabs까지 지원으로 오인.
#[test]
fn word_number_tabs_are_bounded_and_diagnosed() {
    let tabs="<w:tabs><w:tab w:val='num' w:pos='600'/><w:tab w:val='num' w:pos='600' w:extra='1'/><w:tab w:val='left' w:pos='500'/></w:tabs>";
    let d = read(
        &definitions(
            &level(
                0,
                "decimal",
                "%1.",
                &format!("<w:pPr><w:ind w:left='720' w:hanging='360'/>{tabs}</w:pPr>"),
            ),
            &num(7, 1, ""),
        ),
        &item(7, 0, "<w:tabs><w:tab w:val='left' w:pos='400'/></w:tabs>", "body"),
    );
    assert!(d.warnings.iter().any(|w| w.loss() == "list_marker_tab"));
    assert!(d.warnings.iter().any(|w| w.loss() == "paragraph_effect"));
    assert_eq!(
        d.warnings
            .iter()
            .filter(|w| w.loss() == "numbering_definition")
            .map(|w| w.count())
            .sum::<u64>(),
        2
    );
    let styles=format!("<w:styles xmlns:w='{W}'><w:docDefaults><w:pPrDefault><w:pPr><w:tabs><w:tab w:val='num' w:pos='600'/></w:tabs></w:pPr></w:pPrDefault></w:docDefaults></w:styles>");
    let d = decode_docx(&package("<w:p/>", &[("styles", "styles.xml", &styles)])).unwrap();
    assert!(d.warnings.iter().any(|w| w.loss() == "paragraph_effect"));
    let d = layout_input("<w:rPr><w:tabs><w:tab w:val='num' w:pos='600'/></w:tabs></w:rPr>", "");
    assert_eq!(
        marker_shape(&d, 0).list_marker.unwrap().text_start,
        hwpforge_blueprint::ListTextStart::FromAnchor(
            hwpforge_foundation::HwpUnit::new(1800).unwrap()
        )
    );
    assert!(d.warnings.iter().any(|w| w.loss() == "marker_format"));
    let defs = definitions(
        &level(
            0,
            "decimal",
            "%1.",
            &format!(
                "<w:pPr><w:tabs>{}</w:tabs></w:pPr>",
                (0..16385)
                    .map(|pos| format!("<w:tab w:val='num' w:pos='{pos}'/>"))
                    .collect::<String>()
            ),
        ),
        &num(7, 1, ""),
    );
    let xml = format!("<w:numbering xmlns:w='{W}'>{defs}</w:numbering>");
    assert!(matches!(
        decode_docx(&package(&item(7, 0, "", "body"), &[("numbering", "numbering.xml", &xml)])),
        Err(crate::DocxError::Limit { resource: "number tabs", .. })
    ));
}

// 이것을 실패시키는 것: 미사용 스타일의 false를 경고하거나 뒤의 true 재정의를 무시.
#[test]
fn word_auto_spacing_uses_only_effective_paragraph_values() {
    let styles=format!("<w:styles xmlns:w='{W}'><w:style w:type='paragraph' w:styleId='de'><w:pPr><w:autoSpaceDE w:val='false'/><w:autoSpaceDN/></w:pPr></w:style><w:style w:type='paragraph' w:styleId='unused'><w:pPr><w:autoSpaceDN w:val='false'/></w:pPr></w:style></w:styles>");
    for (direct, de, dn) in [
        ("", true, false),
        ("<w:autoSpaceDE/>", false, false),
        ("<w:autoSpaceDE/><w:autoSpaceDN w:val='false'/>", false, true),
    ] {
        let body = format!(
            "<w:p><w:pPr><w:pStyle w:val='de'/>{direct}</w:pPr><w:r><w:t>body</w:t></w:r></w:p>"
        );
        let d = decode_docx(&package(&body, &[("styles", "styles.xml", &styles)])).unwrap();
        assert_eq!(d.warnings.iter().any(|w| w.loss() == "auto_space_de"), de);
        assert_eq!(d.warnings.iter().any(|w| w.loss() == "auto_space_dn"), dn);
    }
}

// 이것을 실패시키는 것: override보다 기본 수준의 문자 서식·정렬·suffix 선택.
#[test]
fn word_marker_override_selects_its_own_character_and_alignment() {
    use hwpforge_blueprint::{ListMarkerAlign, ListTextStart};
    let base = level(
        0,
        "decimal",
        "%1.",
        "<w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr><w:rPr><w:sz w:val='22'/></w:rPr>",
    );
    let over = format!(
        "<w:lvlOverride w:ilvl='0'>{}</w:lvlOverride>",
        level(
            0,
            "decimal",
            "%1.",
            "<w:lvlJc w:val='center'/><w:suff w:val='space'/><w:rPr><w:sz w:val='28'/></w:rPr>"
        )
    );
    let d = read(&definitions(&base, &num(7, 1, &over)), &item(7, 0, "", "body"));
    let marker = marker_shape(&d, 0).list_marker.unwrap();
    assert_eq!(marker.align, ListMarkerAlign::Center);
    assert_eq!(
        marker.text_start,
        ListTextStart::AfterMarker(hwpforge_foundation::HwpUnit::new(389).unwrap())
    );
    assert_eq!(d.registry.char_shapes[marker.char_shape.unwrap().get()].size.as_i32(), 1400);
}
// 이것을 실패시키는 것: 크기·일반 폰트·Latin 폰트 중 한 축의 줄 높이 근사 경고 누락.
#[test]
fn word_marker_line_height_warns_for_each_changed_font_context() {
    let styles=format!("<w:styles xmlns:w='{W}'><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii='Arial' w:hAnsi='Arial' w:eastAsia='함초롬바탕'/><w:sz w:val='22'/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>");
    for (props, expected) in [
        ("", false),
        ("<w:sz w:val='28'/>", true),
        ("<w:rFonts w:eastAsia='맑은 고딕'/>", true),
        ("<w:rFonts w:ascii='Courier New' w:hAnsi='Courier New'/>", true),
    ] {
        let defs = definitions(
            &level(
                0,
                "decimal",
                "%1.",
                &format!(
                    "<w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr><w:rPr>{props}</w:rPr>"
                ),
            ),
            &num(7, 1, ""),
        );
        let numbering = format!("<w:numbering xmlns:w='{W}'>{defs}</w:numbering>");
        let d = decode_docx(&package(
            &item(7, 0, "", "body"),
            &[("styles", "styles.xml", &styles), ("numbering", "numbering.xml", &numbering)],
        ))
        .unwrap();
        assert_eq!(d.warnings.iter().any(|w| w.loss() == "list_marker_line_height"), expected);
    }
}
// 이것을 실패시키는 것: 지원하지 않은 형식의 명목 폭까지 계산하거나 CENTER 슬롯을 g로 계산.
#[test]
fn word_marker_overflow_skips_unmeasured_formats() {
    for (format, text, align) in [
        ("upperRoman", "%1.", "left"),
        ("decimal", "가%1.", "left"),
        ("decimal", "%1.가", "left"),
        ("decimal", "^a%1.", "left"),
        ("decimal", "%1.^a", "left"),
        ("decimal", "%1.", "center"),
    ] {
        let l=level(0,format,text,&std::format!("<w:lvlJc w:val='{align}'/><w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr><w:rPr><w:sz w:val='22'/></w:rPr>")).replace("w:val='1'","w:val='99'");
        let d = read(
            &definitions(&l, &num(7, 1, "")),
            &std::format!("{}{}", item(7, 0, "", "a"), item(7, 0, "", "b")),
        );
        assert!(
            !d.warnings.iter().any(|w| w.loss() == "list_marker_overflow"),
            "{format}/{text}/{align}"
        );
    }
}
// 이것을 실패시키는 것: 상위 항목을 관찰한 뒤 하위 번호의 명목 폭 카운터를 재시작하지 않음.
#[test]
fn word_marker_overflow_follows_source_level_restarts() {
    let props =
        "<w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr><w:rPr><w:sz w:val='22'/></w:rPr>";
    let levels = format!(
        "{}{}",
        level(0, "decimal", "%1.", props),
        level(1, "decimal", "%2.", props).replace("w:val='1'", "w:val='99'")
    );
    let d = read(
        &definitions(&levels, &num(7, 1, "")),
        &format!(
            "{}{}{}{}",
            item(7, 1, "", "a"),
            item(7, 1, "", "b"),
            item(7, 0, "", "parent"),
            item(7, 1, "", "c")
        ),
    );
    assert_eq!(
        d.warnings
            .iter()
            .filter(|w| w.loss() == "list_marker_overflow")
            .map(|w| w.count())
            .sum::<u64>(),
        1
    );
}

// 이것을 실패시키는 것: 번호 문자 기준에서 문단 끝 표시의 문자 스타일을 생략.
#[test]
fn word_marker_character_style_follows_paragraph_mark() {
    let styles=format!("<w:styles xmlns:w='{W}'><w:style w:type='character' w:styleId='mark'><w:rPr><w:i/><w:sz w:val='26'/></w:rPr></w:style></w:styles>");
    let defs = definitions(
        &level(0, "decimal", "%1.", "<w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr>"),
        &num(7, 1, ""),
    );
    let numbering = format!("<w:numbering xmlns:w='{W}'>{defs}</w:numbering>");
    let body="<w:p><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr><w:rPr><w:rStyle w:val='mark'/><w:sz w:val='24'/></w:rPr></w:pPr><w:r><w:rPr><w:b/><w:sz w:val='60'/></w:rPr><w:t>body</w:t></w:r></w:p>";
    let d = decode_docx(&package(
        body,
        &[("styles", "styles.xml", &styles), ("numbering", "numbering.xml", &numbering)],
    ))
    .unwrap();
    let m = marker_shape(&d, 0).list_marker.unwrap();
    let c = &d.registry.char_shapes[m.char_shape.unwrap().get()];
    assert!(c.italic);
    assert!(!c.bold);
    assert_eq!(c.size.as_i32(), 1200);
}

// 이것을 실패시키는 것: 번호 수준의 직계 pPr가 아닌 곳에서도 번호 탭을 읽음.
#[test]
fn word_number_tabs_require_the_level_paragraph_owner() {
    for (nested, direct) in [
        ("<w:numPr><w:tabs><w:tab w:val='num' w:pos='600'/></w:tabs></w:numPr>", ""),
        ("", "<w:tabs><w:tab w:val='num' w:pos='600'/></w:tabs>"),
    ] {
        let extra = format!("<w:pPr><w:ind w:left='720' w:hanging='360'/>{nested}</w:pPr>");
        let d = read(
            &definitions(&level(0, "decimal", "%1.", &extra), &num(7, 1, "")),
            &item(7, 0, direct, "body"),
        );
        assert_eq!(
            marker_shape(&d, 0).list_marker.unwrap().text_start,
            hwpforge_blueprint::ListTextStart::FromAnchor(
                hwpforge_foundation::HwpUnit::new(1800).unwrap()
            )
        );
        assert!(d.warnings.iter().any(|w| w.loss() == "paragraph_effect"));
    }
}

// 이것을 실패시키는 것: 번호 rPr의 복합 스크립트 서식이 축소되어도 기존 진단을 생략.
#[test]
fn word_marker_script_format_loss_is_diagnosed() {
    let d = layout_input("<w:rPr><w:sz w:val='22'/><w:szCs w:val='28'/></w:rPr>", "");
    assert!(d.warnings.iter().any(|w| w.loss() == "script_format"));
    assert!(!d.warnings.iter().any(|w| w.loss() == "marker_format"));
}

// 이것을 실패시키는 것: 미지원 종류의 탭 위치까지 숫자로 해석하여 입력을 거부.
#[test]
fn unsupported_number_tabs_do_not_parse_positions() {
    for tab in ["<w:tab w:val='left'/>", "<w:tab w:val='left' w:pos='bad'/>"] {
        let d = layout_input(&format!("<w:pPr><w:tabs>{tab}</w:tabs></w:pPr>"), "");
        assert_eq!(
            marker_shape(&d, 0).list_marker.unwrap().text_start,
            hwpforge_blueprint::ListTextStart::FromAnchor(
                hwpforge_foundation::HwpUnit::new(1800).unwrap()
            )
        );
        assert!(d.warnings.iter().any(|w| w.loss() == "numbering_definition"));
    }
}

// 이것을 실패시키는 것: 같은 출력 정의·수준에서 목록 진단 횟수를 문단 수만큼 증가.
#[test]
fn word_marker_diagnostics_count_each_definition_level_once() {
    for (extra, loss) in [
        ("<w:suff w:val='space'/>", "list_marker_space_width"),
        ("<w:rPr><w:sz w:val='22'/><w:szCs w:val='28'/></w:rPr>", "script_format"),
        ("<w:pPr><w:ind w:left='720' w:firstLine='100'/></w:pPr>", "list_marker_indent"),
        ("<w:suff w:val='nothing'/>", "list_marker_continuation"),
        ("<w:rPr><w:sz w:val='48'/></w:rPr>", "list_marker_line_height"),
        ("<w:pPr><w:tabs><w:tab w:val='num' w:pos='100'/></w:tabs></w:pPr>", "list_marker_tab"),
    ] {
        let l = level(
            0,
            "decimal",
            "%1.",
            &format!("<w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr>{extra}"),
        );
        let defs = definitions(&l, &num(7, 1, ""));
        let body = item(7, 0, "", "body").repeat(3);
        let d = read(&defs, &body);
        assert_eq!(
            d.warnings.iter().filter(|w| w.loss() == loss).map(|w| w.count()).sum::<u64>(),
            1,
            "{loss}"
        );
        assert!(d
            .registry
            .para_shapes
            .iter()
            .filter(|p| p.list_marker.is_some())
            .all(|p| p.indent_first_line.as_i32() == 0));
    }
    let l = level(
        0,
        "decimal",
        "%1.",
        "<w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr><w:rPr><w:sz w:val='22'/></w:rPr>",
    )
    .replace("w:val='1'", "w:val='999'");
    let d = read(&definitions(&l, &num(7, 1, "")), &item(7, 0, "", "body").repeat(3));
    assert_eq!(
        d.warnings
            .iter()
            .filter(|w| w.loss() == "list_marker_overflow")
            .map(|w| w.count())
            .sum::<u64>(),
        1
    );
    // Distinct levels must each contribute one occurrence to the existing diagnostic aggregate.
    let levels = format!("{l}{}", level(1, "decimal", "%2.", "<w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr><w:start w:val='999'/><w:rPr><w:sz w:val='22'/></w:rPr>"));
    let d = read(
        &definitions(&levels, &num(7, 1, "")),
        &format!("{}{}", item(7, 0, "", "body").repeat(3), item(7, 1, "", "body").repeat(3)),
    );
    assert_eq!(
        d.warnings
            .iter()
            .filter(|w| w.loss() == "list_marker_overflow")
            .map(|w| w.count())
            .sum::<u64>(),
        2
    );
    let defs = format!(
        "{}<w:abstractNum w:abstractNumId='2'>{l}</w:abstractNum>{}",
        definitions(&l, &num(7, 1, "")),
        num(8, 2, "")
    );
    let d = read(
        &defs,
        &format!("{}{}", item(7, 0, "", "body").repeat(3), item(8, 0, "", "body").repeat(3)),
    );
    assert_eq!(
        d.warnings
            .iter()
            .filter(|w| w.loss() == "list_marker_overflow")
            .map(|w| w.count())
            .sum::<u64>(),
        2
    );
}

// 이것을 실패시키는 것: 공유 출력 글머리 head의 근사 진단을 소스·중첩 수준·문단마다 누적.
#[test]
fn word_bullet_diagnostics_count_each_output_head_once() {
    for (glyph, font, expected) in [("&#xF0B7;", "Symbol", 1), ("&#xF0A7;", "Wingdings", 2)] {
        let props = |font: &str| {
            format!("<w:suff w:val='space'/><w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr><w:rPr><w:rFonts w:ascii='Arial' w:hAnsi='{font}'/></w:rPr>")
        };
        let first = level(0, "bullet", "&#xF0B7;", &props("Symbol"));
        let second = level(1, "bullet", glyph, &props(font));
        let defs = format!(
            "{}<w:abstractNum w:abstractNumId='2'>{second}</w:abstractNum>{}",
            definitions(&first, &num(7, 1, "")),
            num(8, 2, "")
        );
        let d = read(
            &defs,
            &format!("{}{}", item(7, 0, "", "body").repeat(3), item(8, 1, "", "body").repeat(3)),
        );
        assert_eq!(d.registry.bullets.len(), expected);
        assert_eq!(count(&d, "DOCX_BULLET_GLYPH_APPROXIMATED"), expected as u64);
        for loss in ["list_marker_space_width", "list_marker_continuation"] {
            assert_eq!(
                d.warnings.iter().filter(|w| w.loss() == loss).map(|w| w.count()).sum::<u64>(),
                expected as u64,
                "{loss}"
            );
        }
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.loss() == "bullet_font_slots")
                .map(|w| w.count())
                .sum::<u64>(),
            expected as u64
        );
    }
    let defs = definitions(
        &level(0, "bullet", "•", "<w:rPr><w:rFonts w:ascii='Arial' w:hAnsi='Symbol'/></w:rPr>"),
        &num(7, 1, ""),
    );
    let d = read(&defs, &item(7, 0, "", "body").repeat(3));
    assert!(d.registry.bullets.is_empty());
    assert_eq!(count(&d, "DOCX_BULLET_GLYPH_LOST"), 3);
    assert_eq!(
        d.warnings
            .iter()
            .filter(|w| w.loss() == "bullet_font_slots")
            .map(|w| w.count())
            .sum::<u64>(),
        3
    );
}

fn zero_read(level: &str, body: &str, settings: Option<&str>, styles: &str) -> DocxDecoded {
    let number =
        format!("<w:numbering xmlns:w='{W}'>{}</w:numbering>", definitions(level, &num(7, 1, "")));
    let styles = format!("<w:styles xmlns:w='{W}'><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val='22'/></w:rPr></w:rPrDefault></w:docDefaults>{styles}</w:styles>");
    let mut parts = vec![
        ("numbering", "numbering.xml", number.as_str()),
        ("styles", "styles.xml", styles.as_str()),
    ];
    if let Some(settings) = settings {
        parts.push(("settings", "options/arbitrary.xml", settings));
    }
    decode_docx(&package(body, &parts)).unwrap()
}
fn zero_level(anchor: i32, indent: &str, extra: &str) -> String {
    level(
        0,
        "decimal",
        "%1.",
        &format!("<w:pPr><w:ind w:left='{anchor}' {indent}/></w:pPr>{extra}"),
    )
}
fn gap(d: &DocxDecoded, index: usize) -> i32 {
    match marker_shape(d, index).list_marker.unwrap().text_start {
        hwpforge_blueprint::ListTextStart::FromAnchor(g) => g.as_i32(),
        other => panic!("{other:?}"),
    }
}
fn loss_count(d: &DocxDecoded, loss: &str) -> u64 {
    d.warnings.iter().filter(|w| w.loss() == loss).map(|w| w.count()).sum()
}
// 이것을 실패시키는 것: 720 하드코딩, anchor 기준 격자, ceiling 대신 floor 또는 현재 번호 대신 고정 자리수.
#[test]
fn word_zero_hanging_uses_default_tab_grid() {
    for (tab, expected) in
        [(720, [3600, 1100, 4100, 3600, 3600, 3600]), (800, [4400, 1500, 900, 4000, 4400, 4400])]
    {
        let settings =
            format!("<w:settings xmlns:w='{W}'><w:defaultTabStop w:val='{tab}'/></w:settings>");
        for (i, anchor) in [720, 500, 620, 0, 720, 720].into_iter().enumerate() {
            let mut l = zero_level(anchor, "w:hanging='0'", "");
            if i == 4 {
                l = l.replace("<w:start w:val='1'/>", "<w:start w:val='9'/>");
            }
            if i == 5 {
                l = level(0,"bullet","&#xF0B7;", "<w:pPr><w:ind w:left='720' w:hanging='0'/></w:pPr><w:rPr><w:rFonts w:ascii='Arial' w:hAnsi='Symbol'/></w:rPr>");
            }
            let d = zero_read(&l, &item(7, 0, "", "body").repeat(2), Some(&settings), "");
            for n in 0..2 {
                assert_eq!(gap(&d, n), expected[i], "tab={tab} anchor={anchor}");
            }
            assert_eq!(loss_count(&d, "list_marker_default_tab"), 1);
            assert_eq!(loss_count(&d, "list_marker_continuation"), 1);
            assert_eq!(loss_count(&d, "list_marker_zero_slot"), 0);
            assert_eq!(loss_count(&d, "list_marker_overflow"), 0);
            assert_eq!(
                loss_count(&d, "list_marker_tab_boundary"),
                u64::from(tab == 800 && anchor == 620)
            );
        }
    }
}
// 이것을 실패시키는 것: 현재 항목의 새 탭만 검사하거나 첫 슬롯 덮어쓰기·넘침 누락·그룹 중복 경고.
#[test]
fn word_default_tab_group_overflow_uses_first_slot() {
    for (tab, anchor, expected) in [(720, 500, [1100, 4700, 4700]), (800, 620, [900, 4900, 4900])] {
        let settings =
            format!("<w:settings xmlns:w='{W}'><w:defaultTabStop w:val='{tab}'/></w:settings>");
        let l = zero_level(anchor, "w:hanging='0'", "")
            .replace("<w:start w:val='1'/>", "<w:start w:val='9'/>");
        let d = zero_read(&l, &item(7, 0, "", "body").repeat(3), Some(&settings), "");
        assert_eq!([gap(&d, 0), gap(&d, 1), gap(&d, 2)], expected);
        assert_eq!(loss_count(&d, "list_marker_default_tab"), 1);
        assert_eq!(loss_count(&d, "list_marker_continuation"), 1);
        assert_eq!(loss_count(&d, "list_marker_overflow"), 1);
        let overflow: Vec<_> =
            d.warnings.iter().filter(|w| w.loss() == "list_marker_overflow").collect();
        assert_eq!(overflow.len(), 1);
        assert_eq!(overflow[0].action(), "retain_slot");
        assert_eq!(
            overflow[0].message(),
            "Estimated marker end exceeds the shared first-use slot; overlap is possible"
        );
    }
}
// 이것을 실패시키는 것: 가운데 번호 끝을 전체 폭으로 계산하거나 오른쪽 끝을 폭만큼 이동·경계도 넘침 판정.
#[test]
fn word_default_tab_group_overflow_respects_alignment() {
    for (align, anchor, expected, overflow) in [
        ("center", 500, [1100, 1100], 0),
        ("center", 600, [600, 4200], 1),
        ("right", 720, [0, 0], 0),
    ] {
        let l = zero_level(anchor, "w:hanging='0'", &format!("<w:lvlJc w:val='{align}'/>"))
            .replace("<w:start w:val='1'/>", "<w:start w:val='9'/>");
        let d = zero_read(&l, &item(7, 0, "", "body").repeat(2), None, "");
        assert_eq!([gap(&d, 0), gap(&d, 1)], expected);
        assert_eq!(loss_count(&d, "list_marker_overflow"), overflow, "{align} {anchor}");
    }
}
// 이것을 실패시키는 것: 후속 항목의 작은 슬롯으로 첫 대표 슬롯을 덮어쓰기.
#[test]
fn word_default_tab_group_overflow_keeps_first_large_slot() {
    let l = zero_level(500, "w:hanging='0'", "")
        .replace("<w:start w:val='1'/>", "<w:start w:val='10'/>");
    let body = format!(
        "{}{}{}",
        item(7, 0, "", "body"),
        item(7, 0, "<w:rPr><w:sz w:val='2'/></w:rPr>", "body"),
        item(7, 0, "", "body")
    );
    let d = zero_read(&l, &body, None, "");
    assert_eq!([gap(&d, 0), gap(&d, 1), gap(&d, 2)], [4700, 1100, 4700]);
    assert_eq!(loss_count(&d, "list_marker_overflow"), 0);
}
// 이것을 실패시키는 것: 무요청 첫 사용처를 빼고 뒤의 작은 슬롯을 대표로 선택.
#[test]
fn word_default_tab_group_overflow_keeps_unspecified_first() {
    let l = zero_level(500, "w:hanging='0'", "")
        .replace("<w:start w:val='1'/>", "<w:start w:val='8'/>");
    let body = format!(
        "{}{}",
        item(7, 0, "<w:ind w:firstLine='20'/>", "body"),
        item(7, 0, "", "body").repeat(2)
    );
    let d = zero_read(&l, &body, None, "");
    assert!(marker_shape(&d, 0).list_marker.is_none());
    assert_eq!([gap(&d, 1), gap(&d, 2)], [1100, 4700]);
    assert_eq!(loss_count(&d, "list_marker_overflow"), 0);

    let props = "<w:pPr><w:ind w:left='700' w:hanging='0'/></w:pPr><w:rPr><w:rFonts w:ascii='Symbol' w:hAnsi='Symbol'/></w:rPr>";
    let first = level(
        0,
        "bullet",
        "&#xF0B7;",
        &props.replace("</w:pPr>", "<w:tabs><w:tab w:val='num' w:pos='700'/></w:tabs></w:pPr>"),
    );
    let later = level(1, "bullet", "&#xF0B7;", props);
    let body = format!(
        "{}{}{}",
        item(7, 0, "", "body"),
        item(7, 1, "<w:rPr><w:sz w:val='2'/></w:rPr>", "body"),
        item(7, 1, "", "body")
    );
    let d = zero_read(&format!("{first}{later}"), &body, None, "");
    assert_eq!(
        lists(&d),
        [0, 1, 1]
            .map(|level| Some(ParagraphListRef::Bullet {
                bullet_id: hwpforge_foundation::BulletIndex::new(0),
                level
            }))
            .to_vec()
    );
    assert!(marker_shape(&d, 0).list_marker.is_none());
    assert_eq!([gap(&d, 1), gap(&d, 2)], [100, 3700]);
    assert_eq!(loss_count(&d, "list_marker_overflow"), 0);
}
// 이것을 실패시키는 것: 경로 하드코딩, namespace/직계 자식 검사 제거, 0 허용, 이전 선언 부활, 기본 탭과 일반 설정 진단 합치기 또는 중복 action을 use_default로 변경.
#[test]
fn word_default_tab_setting_is_scoped_and_validated() {
    let l = zero_level(720, "w:hanging='0'", "");
    let cases = [
        (None,3600,0),
        (Some(format!("<w:settings xmlns:w='{W}'/>")),3600,0),
        (Some(format!("<w:settings xmlns:w='{W}'><w:defaultTabStop w:val='800'/></w:settings>")),4400,0),
        (Some(format!("<w:settings xmlns:w='{W}' xmlns:x='urn:foreign'><x:defaultTabStop w:val='800'/></w:settings>")),3600,0),
        (Some(format!("<w:settings xmlns:w='{W}' xmlns:x='urn:foreign'><w:defaultTabStop x:val='800'/></w:settings>")),3600,1),
        (Some(format!("<w:settings xmlns:w='{W}'><w:compat><w:defaultTabStop w:val='800'/></w:compat></w:settings>")),3600,0),
        (Some(format!("<w:wrong xmlns:w='{W}'><w:defaultTabStop w:val='800'/></w:wrong>")),3600,0),
        (Some(format!("<x:settings xmlns:w='{W}' xmlns:x='urn:foreign'><w:defaultTabStop w:val='800'/></x:settings>")),3600,0),
        (Some(format!("<w:settings xmlns:w='{W}'><w:defaultTabStop w:val='720'/><w:defaultTabStop w:val='800'/></w:settings>")),4400,1),
        (Some(format!("<w:settings xmlns:w='{W}'><w:defaultTabStop w:val='800'/><w:defaultTabStop w:val='0'/></w:settings>")),3600,2),
    ];
    for (xml, want, warnings) in cases {
        let d = zero_read(&l, &item(7, 0, "", "body"), xml.as_deref(), "");
        assert_eq!(gap(&d, 0), want, "{xml:?}");
        assert_eq!(loss_count(&d, "list_marker_default_tab_stop"), warnings, "{xml:?}");
        for w in d.warnings.iter().filter(|w| w.loss() == "list_marker_default_tab_stop") {
            assert_eq!(w.code(), "DOCX_UNSUPPORTED_VALUE");
            assert_eq!(
                w.action(),
                if w.message().starts_with("Duplicate") { "use_last" } else { "use_default" }
            );
        }
    }
    let settings = format!("<w:settings xmlns:w='{W}'><w:defaultTabStop w:val='800'/><w:defaultTabStop w:val='0'/></w:settings>");
    let d = zero_read(&l, &item(7, 0, "", "body"), Some(&settings), "");
    let records: Vec<_> =
        d.warnings.iter().filter(|w| w.loss() == "list_marker_default_tab_stop").collect();
    assert_eq!(records.len(), 2);
    assert_eq!((records[0].action(), records[0].count()), ("use_last", 1));
    assert_eq!((records[1].action(), records[1].count()), ("use_default", 1));
    for value in [
        "",
        "w:val='0'",
        "w:val='-1'",
        "w:val='bad'",
        "w:val='20000001'",
        "w:val='999999999999999999999999999999999999999999'",
    ] {
        let settings =
            format!("<w:settings xmlns:w='{W}'><w:defaultTabStop {value}/></w:settings>");
        let d = zero_read(&l, &item(7, 0, "", "body"), Some(&settings), "");
        assert_eq!(gap(&d, 0), 3600);
        assert_eq!(loss_count(&d, "list_marker_default_tab_stop"), 1);
    }

    // Each setting retains its own diagnostic example regardless of visitation order.
    for prefix in
        ["<w:zoom w:percent='100'/>", "<w:compat><w:compatSetting w:name='x'/></w:compat>"]
    {
        let settings = format!(
            "<w:settings xmlns:w='{W}'>{prefix}<w:defaultTabStop w:val='800'/></w:settings>"
        );
        let d = zero_read(&l, &item(7, 0, "", "body"), Some(&settings), "");
        assert_eq!(gap(&d, 0), 4400);
        assert_eq!(loss_count(&d, "document_settings"), 1);
        let ordinary = d.warnings.iter().find(|w| w.loss() == "document_settings").unwrap();
        assert_eq!(
            ordinary.message(),
            if prefix.starts_with("<w:zoom") {
                "Document setting is not applied"
            } else {
                "Unsupported compatibility settings are not applied"
            }
        );
        assert_eq!(loss_count(&d, "list_marker_default_tab_settings"), 1);
        assert!(d
            .warnings
            .iter()
            .find(|w| w.loss() == "list_marker_default_tab_settings")
            .unwrap()
            .message()
            .contains("ordinary body tab positions are not applied"));
    }
}
// 이것을 실패시키는 것: 원시 hanging 속성 존재만 검사.
#[test]
fn word_zero_hanging_equivalent_indents_agree() {
    for indent in ["w:hanging='0'", "w:firstLine='0'", ""] {
        let d = zero_read(&zero_level(720, indent, ""), &item(7, 0, "", "body"), None, "");
        assert_eq!(gap(&d, 0), 3600);
        assert_eq!(marker_shape(&d, 0).indent_first_line.as_i32(), 0);
    }
}
// 이것을 실패시키는 것: 알 수 없는 문자에 0폭/1em 부여 또는 출력 글머리만으로 Symbol 판정.
#[test]
fn word_zero_hanging_unsupported_markers_keep_legacy_layout() {
    for (fmt, text, font) in [
        ("upperRoman", "%1.", "Arial"),
        ("lowerLetter", "%1.", "Arial"),
        ("decimal", "(%1)", "Arial"),
        ("decimal", "%1 ", "Arial"),
        ("bullet", "&#xF0A7;", "Wingdings"),
        ("bullet", "&#xF06C;", "Wingdings"),
        ("bullet", "&#xF06E;", "Wingdings"),
        ("bullet", "o", "Courier New"),
        ("bullet", "•", "Arial"),
    ] {
        let l=level(0,fmt,text,&format!("<w:pPr><w:ind w:left='720' w:hanging='0'/></w:pPr><w:rPr><w:rFonts w:ascii='{font}' w:hAnsi='{font}'/></w:rPr>"));
        let d = zero_read(&l, &item(7, 0, "", "body").repeat(2), None, "");
        assert!(marker_shape(&d, 0).list_marker.is_none(), "{fmt} {text}");
        assert_eq!(loss_count(&d, "list_marker_zero_slot"), 1);
        assert_eq!(loss_count(&d, "list_marker_default_tab"), 0);
    }
}
// 이것을 실패시키는 것: 활성 번호 탭 무시 또는 clear 후에도 차단.
#[test]
fn word_zero_hanging_number_tabs_keep_existing_policy() {
    let base = zero_level(720, "w:hanging='0'", "")
        .replace("</w:pPr>", "<w:tabs><w:tab w:val='num' w:pos='720'/></w:tabs></w:pPr>");
    let d = zero_read(&base, &item(7, 0, "", "body"), None, "");
    assert!(marker_shape(&d, 0).list_marker.is_none());
    assert_eq!(loss_count(&d, "list_marker_zero_slot"), 1);
    let over = format!(
        "<w:lvlOverride w:ilvl='0'>{}</w:lvlOverride>",
        level(
            0,
            "decimal",
            "%1.",
            "<w:pPr><w:tabs><w:tab w:val='clear' w:pos='720'/></w:tabs></w:pPr>"
        )
    );
    let d = read(&definitions(&base, &num(7, 1, &over)), &item(7, 0, "", "body"));
    assert_eq!(gap(&d, 0), 3600);
}
// 이것을 실패시키는 것: 후보에서 anchor 제거 → 800에서 g=400, 또는 g=0을 None 처리.
#[test]
fn word_zero_hanging_right_uses_anchor_as_marker_end() {
    for tab in [720, 800] {
        let settings =
            format!("<w:settings xmlns:w='{W}'><w:defaultTabStop w:val='{tab}'/></w:settings>");
        let d = zero_read(
            &zero_level(720, "w:firstLine='0'", "<w:lvlJc w:val='right'/>"),
            &item(7, 0, "", "body"),
            Some(&settings),
            "",
        );
        assert_eq!(gap(&d, 0), 0);
        assert_eq!(loss_count(&d, "list_marker_default_tab"), 1);
        for loss in [
            "list_marker_zero_slot",
            "list_marker_continuation",
            "list_marker_tab_boundary",
            "list_marker_overflow",
        ] {
            assert_eq!(loss_count(&d, loss), 0, "{loss}");
        }
    }
}
// 이것을 실패시키는 것: 가운데 끝을 A+w로 계산.
#[test]
fn word_zero_hanging_center_uses_half_width() {
    for (tab, anchor, want) in [(720, 720, 3600), (800, 720, 4400), (800, 660, 700)] {
        let settings =
            format!("<w:settings xmlns:w='{W}'><w:defaultTabStop w:val='{tab}'/></w:settings>");
        let d = zero_read(
            &zero_level(anchor, "w:hanging='0'", "<w:lvlJc w:val='center'/>"),
            &item(7, 0, "", "body"),
            Some(&settings),
            "",
        );
        assert_eq!(gap(&d, 0), want);
    }
}
// 이것을 실패시키는 것: exact 탭을 다음으로 이동, 경계 <100을 <=100으로 변경.
#[test]
fn word_default_tab_boundary_is_inclusive() {
    for (left, want, boundary) in [("550.4", 848, 1), ("530.4", 948, 0), ("550.6", 4447, 0)] {
        let l =
            zero_level(0, "w:hanging='0'", "").replace("w:left='0'", &format!("w:left='{left}'"));
        let d = zero_read(&l, &item(7, 0, "", "body"), None, "");
        assert_eq!(gap(&d, 0), want);
        assert_eq!(loss_count(&d, "list_marker_tab_boundary"), boundary);
    }
}
// 이것을 실패시키는 것: 절대 쪽 여백을 더하거나 셀/다단 원점 근사 경고 제거.
#[test]
fn word_default_tab_uses_local_content_origin() {
    let item = item(7, 0, "", "body");
    let l = zero_level(720, "w:hanging='0'", "");
    for body in [
        item.clone(),
        format!("<w:tbl><w:tr><w:tc>{item}</w:tc></w:tr></w:tbl>"),
        format!(
            "{item}<w:sectPr><w:pgMar w:left='2000' w:right='1000'/><w:cols w:num='2'/></w:sectPr>"
        ),
    ] {
        let d = zero_read(&l, &body, None, "");
        let mut gaps = vec![];
        d.document.sections()[0].for_each_paragraph(|p| {
            let shape = &d.registry.para_shapes[p.para_shape_id.get()];
            if let Some(m) = shape.list_marker {
                if let hwpforge_blueprint::ListTextStart::FromAnchor(g) = m.text_start {
                    gaps.push(g.as_i32());
                }
            }
        });
        assert_eq!(gaps, [3600]);
        let w = d.warnings.iter().find(|w| w.loss() == "list_marker_default_tab").unwrap();
        assert!(w
            .message()
            .contains("cells, columns and zero-hanging center alignment are unmeasured"));
    }
}
// 이것을 실패시키는 것: 첫 run으로 번호 서식 결정, mark/lvl overlay 순서 역전 또는 폭에 본문 크기 사용.
#[test]
fn word_marker_format_follows_paragraph_mark() {
    for (mark, run, level_size, want, bold) in [
        ("", "<w:b/><w:sz w:val='32'/>", "", 1100, false),
        ("<w:b/><w:sz w:val='32'/>", "", "", 1600, true),
        (
            "<w:b/><w:sz w:val='32'/>",
            "<w:sz w:val='32'/>",
            "<w:rPr><w:sz w:val='28'/></w:rPr>",
            1400,
            true,
        ),
    ] {
        let body=format!("<w:p><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr><w:rPr>{mark}</w:rPr></w:pPr><w:r><w:rPr>{run}</w:rPr><w:t>body</w:t></w:r></w:p>");
        let d = zero_read(&zero_level(490, "w:hanging='0'", level_size), &body, None, "");
        let m = marker_shape(&d, 0).list_marker.unwrap();
        let c = &d.registry.char_shapes[m.char_shape.unwrap().get()];
        assert_eq!((c.size.as_i32(), c.bold), (want, bold));
        let p = &d.document.sections()[0].paragraphs[0];
        let c = &d.registry.char_shapes[p.runs[0].char_shape_id.get()];
        assert_eq!(c.size.as_i32(), if run.is_empty() { 1100 } else { 1600 });
        assert_eq!(gap(&d, 0), if want == 1600 { 4750 } else { 1150 });
    }
}
// 이것을 실패시키는 것: 걸침 양수 경로까지 새 추정 폭으로 확대.
#[test]
fn word_hanging_overflow_keeps_fixed_slot() {
    for (hanging, want) in [(360, 1800), (180, 900)] {
        let l =
            zero_level(720, &format!("w:hanging='{hanging}'"), "<w:rPr><w:sz w:val='22'/></w:rPr>")
                .replace("<w:start w:val='1'/>", "<w:start w:val='99'/>");
        let d = zero_read(&l, &item(7, 0, "", "body").repeat(2), None, "");
        assert_eq!(gap(&d, 0), want);
        assert_eq!(gap(&d, 1), want);
        assert_eq!(loss_count(&d, "list_marker_overflow"), 1);
        assert_eq!(loss_count(&d, "list_marker_default_tab"), 0);
    }
}
