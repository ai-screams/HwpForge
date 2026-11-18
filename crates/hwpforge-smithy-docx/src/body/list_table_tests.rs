use super::tests::{package, W};
use crate::{decode_docx, DocxDecoded};
use hwpforge_core::{ParagraphListRef, RunContent};
use std::collections::BTreeMap;

fn item(id: u32, level: u8, text: &str) -> String {
    format!("<w:p><w:pPr><w:numPr><w:numId w:val='{id}'/><w:ilvl w:val='{level}'/></w:numPr></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>")
}
fn table(rows: &[Vec<String>]) -> String {
    let rows: String = rows
        .iter()
        .map(|cells| {
            let cells: String = cells.iter().map(|body| format!("<w:tc>{body}</w:tc>")).collect();
            format!("<w:tr>{cells}</w:tr>")
        })
        .collect();
    format!("<w:tbl>{rows}</w:tbl>")
}
fn read(body: &str) -> DocxDecoded {
    let defs: String = (1..=2).map(|id| format!(
        "<w:abstractNum w:abstractNumId='{id}'><w:lvl w:ilvl='0'><w:start w:val='1'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%1.'/></w:lvl><w:lvl w:ilvl='1'><w:start w:val='1'/><w:numFmt w:val='lowerLetter'/><w:lvlText w:val='%2)'/></w:lvl></w:abstractNum><w:num w:numId='{id}'><w:abstractNumId w:val='{id}'/></w:num>"
    )).collect();
    let numbering = format!("<w:numbering xmlns:w='{W}'>{defs}</w:numbering>");
    decode_docx(&package(body, &[("numbering", "numbering.xml", &numbering)])).unwrap()
}
fn values(d: &DocxDecoded) -> BTreeMap<String, String> {
    let mut counters = BTreeMap::<usize, [Option<u32>; 10]>::new();
    let mut result = BTreeMap::new();
    for section in d.document.sections() {
        for root in &section.paragraphs {
            root.for_each_paragraph(|p| {
                let Some(ParagraphListRef::Number { numbering_id, level }) =
                    d.registry.para_shapes[p.para_shape_id.get()].list
                else {
                    return;
                };
                let head = &d.registry.numberings[numbering_id.get()].levels[usize::from(level)];
                let state = counters.entry(numbering_id.get()).or_insert([None; 10]);
                let value = state[usize::from(level)].map_or(head.start, |v| v + 1);
                state[usize::from(level)] = Some(value);
                state[usize::from(level) + 1..].fill(None);
                let rendered = if level == 0 {
                    format!("{value}.")
                } else {
                    format!("{})", char::from_u32(u32::from('a') + value - 1).unwrap())
                };
                let text: String = p
                    .runs
                    .iter()
                    .filter_map(|r| match &r.content {
                        RunContent::Text(t) => Some(t.as_str()),
                        _ => None,
                    })
                    .collect();
                assert!(result.insert(text, rendered).is_none());
            });
        }
    }
    result
}

// 이것을 실패시키는 것: 표 이동 전 배정, 칸 재귀 누락, 부모보다 칸 우선 배정, 과거 정의 복귀.
#[test]
fn numbered_table_hosts_preserve_source_values() {
    let a = item(1, 0, "a");
    let b = item(1, 0, "b");
    let c = item(1, 0, "c");
    let d = item(1, 0, "d");
    let e = item(1, 0, "e");
    let f = item(1, 0, "f");
    let g = item(1, 0, "g");
    let plain = "<w:p><w:r><w:t>plain</w:t></w:r></w:p>";
    let pair = table(&[vec![b.clone(), c.clone()]]);
    let nested = table(&[vec![c.clone(), d.clone()]]);
    let cases = [
        (format!("{a}{pair}{d}{e}"), vec!["1.", "2.", "3.", "4.", "5."], 4, 3),
        (
            format!("{a}{}{f}", table(&[vec![b.clone(), c.clone()], vec![d.clone(), e.clone()]])),
            vec!["1.", "2.", "3.", "4.", "5.", "6."],
            3,
            2,
        ),
        (
            format!("{a}{}{e}", table(&[vec![format!("{b}{c}"), item(1, 1, "d")]])),
            vec!["1.", "2.", "3.", "a)", "4."],
            3,
            2,
        ),
        (format!("{}{c}", table(&[vec![a.clone(), b.clone()]])), vec!["1.", "2.", "3."], 2, 1),
        (
            format!("{a}{}{d}", table(&[vec![item(2, 0, "b"), item(2, 0, "c")]])),
            vec!["1.", "1.", "2.", "2."],
            2,
            0,
        ),
        (
            format!("{a}{}{e}", table(&[vec![format!("{b}{nested}<w:p/>")]])),
            vec!["1.", "2.", "3.", "4.", "5."],
            3,
            2,
        ),
        (format!("{a}{pair}{plain}{d}"), vec!["1.", "2.", "3.", "4."], 1, 0),
        (format!("{a}{pair}"), vec!["1.", "2.", "3."], 1, 0),
        (
            format!("{a}{}{plain}{f}", table(&[vec![format!("{b}{nested}{e}")]])),
            vec!["1.", "2.", "3.", "4.", "5.", "6."],
            4,
            3,
        ),
        (
            format!("{a}{pair}{}{f}{g}", table(&[vec![d, e]])),
            vec!["1.", "2.", "3.", "4.", "5.", "6.", "7."],
            4,
            3,
        ),
    ];
    for (body, expected, definitions, splits) in cases {
        let decoded = read(&body);
        assert_eq!(values(&decoded).into_values().collect::<Vec<_>>(), expected);
        assert_eq!(decoded.registry.numberings.len(), definitions);
        assert_eq!(
            decoded
                .warnings
                .iter()
                .filter(|w| w.loss() == "list_numbering_split")
                .map(|w| w.count())
                .sum::<u64>(),
            splits
        );
    }
}

fn diagnostic_input(properties: &str, start: u32, body: &str) -> DocxDecoded {
    let numbering = format!("<w:numbering xmlns:w='{W}'><w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:start w:val='{start}'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%1.'/>{properties}</w:lvl></w:abstractNum><w:num w:numId='1'><w:abstractNumId w:val='1'/></w:num></w:numbering>");
    decode_docx(&package(body, &[("numbering", "numbering.xml", &numbering)])).unwrap()
}

// 이것을 실패시키는 것: 진단을 원본 그룹 또는 문단별로 억제하여 네 정의의 count를 1 또는 5로 변경.
#[test]
fn split_table_lists_deduplicate_marker_warnings_by_definition() {
    let body = format!(
        "{}{}{}{}",
        item(1, 0, "a"),
        table(&[vec![item(1, 0, "b"), item(1, 0, "c")]]),
        item(1, 0, "d"),
        item(1, 0, "e")
    );
    let hanging = "<w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr>";
    for (properties, start, loss, action, message) in [
        ("".to_owned(), 1, "list_marker_default_tab", "use_measured_ratio", "Default tab selected"),
        (format!("{hanging}<w:rPr><w:sz w:val='22'/><w:szCs w:val='28'/></w:rPr>"), 1, "script_format", "select_supported_slots", ""),
        (format!("{hanging}<w:rPr><w:sz w:val='48'/></w:rPr>"), 1, "list_marker_line_height", "retain_body_line_height", "Marker font or size"),
        ("<w:pPr><w:ind w:firstLine='100'/></w:pPr>".to_owned(), 1, "list_marker_indent", "retain_indent", "List indent"),
        ("<w:pPr><w:tabs><w:tab w:val='num' w:pos='0'/></w:tabs></w:pPr>".to_owned(), 1, "list_marker_zero_slot", "retain_indent", "Zero marker slot"),
        ("<w:pPr><w:ind w:left='720' w:hanging='360'/><w:tabs><w:tab w:val='num' w:pos='100'/></w:tabs></w:pPr>".to_owned(), 1, "list_marker_tab", "use_indent", "Number tab"),
        ("<w:pPr><w:ind w:left='560'/></w:pPr>".to_owned(), 1, "list_marker_tab_boundary", "retain_selected_tab", "Estimated marker end is less"),
        (format!("{hanging}<w:suff w:val='space'/>"), 1, "list_marker_space_width", "use_measured_ratio", "Space advance"),
        (hanging.to_owned(), 999, "list_marker_overflow", "retain_slot", "Nominal marker width"),
    ] {
        let d = diagnostic_input(&properties, start, &body);
        assert_eq!(d.registry.numberings.len(), 4);
        let warning = d.warnings.iter().find(|w| w.loss() == loss).unwrap_or_else(|| panic!("{loss}: {:?}", d.warnings));
        assert_eq!(warning.count(), 4, "{loss}");
        assert_eq!(warning.action(), action, "{loss}");
        assert!(warning.message().starts_with(message), "{loss}: {}", warning.message());
        assert_eq!(warning.part(), Some("text/body.xml"));
        assert!(!d.warnings.iter().any(|w| w.loss() == "list_marker_script"));
    }
}

// 이것을 실패시키는 것: 뒤 조각에 요청을 복제하거나 표를 나눔 경계 밖으로 이동.
#[test]
fn split_numbered_paragraphs_consume_once_and_keep_table_boundaries() {
    for kind in ["page", "column"] {
        let broken = item(1, 0, "b")
            .replace("b</w:t>", &format!("b</w:t><w:br w:type='{kind}'/><w:t>tail</w:t>"));
        let body = format!(
            "{}{}{broken}{}",
            item(1, 0, "a"),
            table(&[vec![item(1, 0, "cell")]]),
            item(1, 0, "c")
        );
        let d = read(&body);
        assert_eq!(
            values(&d),
            BTreeMap::from([
                ("a".into(), "1.".into()),
                ("cell".into(), "2.".into()),
                ("b".into(), "3.".into()),
                ("c".into(), "4.".into())
            ])
        );
        let paragraphs = &d.document.sections()[0].paragraphs;
        let tail = paragraphs.iter().find(|p| p.text_content() == "tail").unwrap();
        assert_eq!(d.registry.para_shapes[tail.para_shape_id.get()].list, None);
        assert!(tail.page_break);
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.loss() == "paragraph_split")
                .map(|w| w.count())
                .sum::<u64>(),
            1
        );
        assert_eq!(d.registry.numberings.len(), 1);
        let holder = paragraphs
            .iter()
            .find(|p| p.runs.iter().any(|r| matches!(r.content, RunContent::Table(_))))
            .unwrap();
        assert_eq!(d.registry.para_shapes[holder.para_shape_id.get()].list, None);
    }
}

// 이것을 실패시키는 것: 지연 번호 배정을 본문 해석 중 수행하거나 시작값 범위를 포화 처리.
#[test]
fn deferred_numbering_errors_require_a_successfully_parsed_body() {
    let numbering = format!("<w:numbering xmlns:w='{W}'><w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:start w:val='4294967295'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%1.'/></w:lvl></w:abstractNum><w:num w:numId='1'><w:abstractNumId w:val='1'/></w:num></w:numbering>");
    let prefix = format!("{}{}", table(&[vec![item(1, 0, "a")]]), item(1, 0, "b"));
    let bad = "<w:p><w:r><w:t>bad</w:r></w:p>";
    let decode = |body: &str| {
        decode_docx(&package(body, &[("numbering", "numbering.xml", &numbering)])).unwrap_err()
    };
    assert!(
        matches!(decode(&prefix), crate::DocxError::Invalid { part, element, detail } if part=="text/body.xml" && element=="numPr" && detail=="numbering start exceeds u32")
    );
    let body_error = decode(bad);
    assert_eq!(decode(&format!("{prefix}{bad}")).to_string(), body_error.to_string());
}

// 이것을 실패시키는 것: 나눔 경고 누락 또는 지연 진단의 원본 발생 순서 변경.
#[test]
fn split_marker_diagnostics_retain_source_order_and_first_example() {
    let first = item(1, 0, "a").replace("a</w:t>", "a</w:t><w:br w:type='page'/><w:t>tail</w:t>");
    let body = format!(
        "{first}{}{}{}",
        table(&[vec![item(1, 0, "b"), item(1, 0, "c")]]),
        item(1, 0, "d"),
        item(1, 0, "e")
    );
    let decoded = read(&body);
    let warnings: Vec<_> = decoded
        .warnings
        .iter()
        .filter(|w| {
            matches!(
                w.loss(),
                "paragraph_split" | "list_marker_default_tab" | "list_numbering_split"
            )
        })
        .collect();
    assert_eq!(
        warnings.iter().map(|w| (w.loss(), w.count())).collect::<Vec<_>>(),
        [("paragraph_split", 1), ("list_marker_default_tab", 4), ("list_numbering_split", 3)]
    );
    assert_eq!(warnings[1].location(), Some("numPr"));
    assert_eq!(warnings[1].action(), "use_measured_ratio");
    assert_eq!(warnings[2].location(), Some("numPr"));
}

// 이것을 실패시키는 것: 구역마다 원본·출력 번호 상태를 초기화하거나 구역 끝 표를 뒤로 이동.
#[test]
fn section_boundary_retains_table_and_shared_abstract_counter() {
    let defs=format!("<w:numbering xmlns:w='{W}'><w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:start w:val='1'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%1.'/></w:lvl></w:abstractNum><w:num w:numId='1'><w:abstractNumId w:val='1'/></w:num><w:num w:numId='2'><w:abstractNumId w:val='1'/></w:num></w:numbering>");
    let body = format!(
        "{}{}<w:p><w:pPr><w:sectPr/></w:pPr></w:p>{}",
        item(1, 0, "a"),
        table(&[vec![item(2, 0, "b")]]),
        item(1, 0, "c")
    );
    let d = decode_docx(&package(&body, &[("numbering", "numbering.xml", &defs)])).unwrap();
    assert_eq!(d.document.sections().len(), 2);
    assert_eq!(values(&d).into_values().collect::<Vec<_>>(), ["1.", "2.", "3."]);
    assert_eq!(d.registry.numberings.len(), 1);
    assert_eq!(
        d.document.sections()[0]
            .paragraphs
            .iter()
            .flat_map(|p| &p.runs)
            .filter(|r| matches!(r.content, RunContent::Table(_)))
            .count(),
        1
    );
}

// 이것을 실패시키는 것: 첫 정의의 실제 상한 오류를 budget fallback으로 바꾸거나 본문 실패 뒤 배정.
#[test]
fn definition_limit_is_checked_only_after_body_parsing() {
    let xml=format!("<w:numbering xmlns:w='{W}'><w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:start w:val='1'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%1.'/></w:lvl></w:abstractNum><w:num w:numId='1'><w:abstractNumId w:val='1'/></w:num></w:numbering>");
    for malformed in [false, true] {
        let mut defs = super::Definitions::default();
        defs.read(
            xml.as_bytes(),
            "numbering.xml",
            "numbering",
            &mut super::TextBudget::default(),
            &mut vec![],
        )
        .unwrap();
        let mut resolver = super::Resolver::new(defs, false, &mut vec![]).unwrap();
        resolver
            .registry
            .numberings
            .resize(16384, hwpforge_core::NumberingDef { id: 1, levels: vec![], start: 1 });
        let suffix = if malformed { "<w:p><w:r><w:t>bad</w:r></w:p>" } else { "" };
        let body = format!(
            "<w:document xmlns:w='{W}'><w:body>{}{suffix}</w:body></w:document>",
            item(1, 0, "a")
        );
        let mut warnings = vec![];
        let parsed = super::scan_formatted(
            body.as_bytes(),
            "body",
            &mut super::TextBudget::default(),
            &mut warnings,
            &mut resolver,
            &super::Relations::default(),
            None,
        );
        assert_eq!(resolver.registry.numberings.len(), 16384);
        if malformed {
            assert!(matches!(parsed,Err(crate::DocxError::Invalid{element,..}) if element=="t"));
        } else {
            let mut sections = parsed.unwrap();
            let error = resolver
                .finalize_numbering(
                    &mut sections.iter_mut().map(|s| &mut s.core).collect::<Vec<_>>(),
                    "body",
                    &mut warnings,
                )
                .unwrap_err();
            assert!(
                matches!(error,crate::DocxError::Limit{part,element,resource:"output numbering definitions",observed:16385,limit:16384} if part=="body" && element=="numPr")
            );
        }
    }
}

#[path = "../../tests/support/word_counter_cases.rs"]
mod word_counter_cases;

// 이것을 실패시키는 것: 최종 원본 상태로 override 씨앗을 덮어쓰거나 legal 참조 수준·0 리터럴을 일반 번호로 소비.
#[test]
fn counter_rules_survive_tables_before_numbered_hosts() {
    // The existing source counter literals also apply when the first two items
    // are in cells. This is a structural combination, not a new visual observation.
    for case in word_counter_cases::counter_cases() {
        let items: Vec<_> = case
            .uses
            .iter()
            .enumerate()
            .map(|(i, &(num, level))| item(num, level as u8, &format!("item {i}")))
            .collect();
        let body = format!(
            "{}{}",
            table(&[items[..2.min(items.len())].to_vec()]),
            items[2.min(items.len())..].concat()
        );
        let numbering = format!("<w:numbering xmlns:w='{W}'>{}</w:numbering>", case.definitions);
        let styles = format!("<w:styles xmlns:w='{W}'>{}</w:styles>", case.styles);
        let mut decoded = decode_docx(&package(
            &body,
            &[("numbering", "numbering.xml", &numbering), ("styles", "styles.xml", &styles)],
        ))
        .unwrap();
        let mut flattened = vec![];
        for root in &decoded.document.sections()[0].paragraphs {
            root.for_each_paragraph(|p| {
                if matches!(
                    decoded.registry.para_shapes[p.para_shape_id.get()].list,
                    Some(ParagraphListRef::Number { .. })
                ) {
                    let mut own = p.clone();
                    own.runs.retain(|r| matches!(r.content, RunContent::Text(_)));
                    flattened.push(own);
                }
            });
        }
        let labels: Vec<_> = flattened.iter().map(|p| p.text_content()).collect();
        decoded.document.sections_mut()[0].paragraphs = flattened;
        let actual: BTreeMap<_, _> =
            labels.into_iter().zip(super::list_tests::displayed_core_numbers(&decoded)).collect();
        let expected: BTreeMap<_, _> = case
            .expected
            .iter()
            .enumerate()
            .map(|(i, value)| (format!("item {i}"), value.to_string()))
            .collect();
        assert_eq!(actual, expected, "{}", case.name);
    }
}

// 이것을 실패시키는 것: 표 안에서 생략한 원본 번호를 출력 요청으로 복원하거나 절대 override 복구를 누락.
#[test]
fn omitted_cell_numbers_resume_only_at_an_explicit_reset() {
    let levels = word_counter_cases::level(0, Some(1), "decimal", "%1.", "");
    let defs = format!(
        "{}{}{}",
        word_counter_cases::definition(1, &levels),
        word_counter_cases::num(1, 1, ""),
        word_counter_cases::num(2, 1, &word_counter_cases::reset(0, 5))
    );
    let heading = item(1, 0, "heading").replace("</w:pPr>", "<w:outlineLvl w:val='0'/></w:pPr>");
    let body = format!(
        "{}{}{}",
        table(&[vec![heading, item(1, 0, "lost")]]),
        item(2, 0, "reset"),
        item(1, 0, "after")
    );
    let numbering = format!("<w:numbering xmlns:w='{W}'>{defs}</w:numbering>");
    let decoded =
        decode_docx(&package(&body, &[("numbering", "numbering.xml", &numbering)])).unwrap();
    assert_eq!(
        values(&decoded),
        BTreeMap::from([("reset".into(), "5.".into()), ("after".into(), "6.".into())])
    );
    assert_eq!(decoded.registry.numberings.len(), 1);
    assert_eq!(
        decoded
            .warnings
            .iter()
            .filter(|w| w.loss() == "numbering_sequence")
            .map(|w| w.count())
            .sum::<u64>(),
        1
    );
}

// 이것을 실패시키는 것: 병합 연속 칸의 문단에도 번호 요청을 생성.
#[test]
fn numbered_merge_continuation_is_omitted_before_number_requests() {
    let body = format!(
        "{}<w:tbl><w:tblGrid><w:gridCol w:w='2400'/></w:tblGrid>\
         <w:tr><w:tc><w:tcPr><w:vMerge w:val='restart'/></w:tcPr>{}</w:tc></w:tr>\
         <w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr>{}</w:tc></w:tr></w:tbl>{}",
        item(1, 0, "before"),
        item(1, 0, "cell"),
        item(1, 0, "omitted"),
        item(1, 0, "after"),
    );
    let decoded = read(&body);
    assert_eq!(
        values(&decoded),
        BTreeMap::from([
            ("before".into(), "1.".into()),
            ("cell".into(), "2.".into()),
            ("after".into(), "3.".into()),
        ])
    );
    assert!(decoded.warnings.iter().any(|w| w.loss() == "merge_continuation"));
    let table = decoded.document.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .find_map(|r| match &r.content {
            RunContent::Table(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(table.rows[0].cells[0].row_span, 2);
}
