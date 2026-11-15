#![cfg(feature = "ops-docx")]
#[path = "support/docx.rs"]
mod docx;
#[path = "../../hwpforge-smithy-hwpx/tests/support/list_counter_cases.rs"]
mod list_counter_cases;
#[path = "../../hwpforge-smithy-hwpx/tests/support/package.rs"]
mod package;
#[path = "../../hwpforge-smithy-hwpx/tests/support/xml.rs"]
mod xml;
use hwpforge::{
    hwpx::EncodeWarning,
    ops::{convert_docx, OpsWarning},
};
use std::collections::BTreeMap;
fn input(start: u32, align: &str, items: &[(i32, i32, &str)]) -> Vec<u8> {
    input_with_font(start, align, items, "Malgun Gothic")
}
fn input_with_font(start: u32, align: &str, items: &[(i32, i32, &str)], font: &str) -> Vec<u8> {
    let numbering = format!("<w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:start w:val='{start}'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%1.'/><w:lvlJc w:val='{align}'/><w:suff w:val='tab'/><w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr></w:lvl></w:abstractNum><w:num w:numId='7'><w:abstractNumId w:val='1'/></w:num>");
    let body = items.iter().map(|(left,hanging,mark)| format!("<w:p><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr><w:ind w:left='{left}' w:hanging='{hanging}'/><w:rPr><w:rFonts w:ascii='{font}' w:hAnsi='{font}' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/>{mark}</w:rPr></w:pPr><w:r><w:rPr><w:rFonts w:ascii='{font}' w:hAnsi='{font}' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr><w:t>body</w:t></w:r></w:p>")).collect::<String>();
    docx::package_with_numbering_and_settings(&body, &numbering, Some("720"))
}
struct Item {
    definition: u32,
    text: String,
    left: i32,
    intent: i32,
    width: i32,
    bold: bool,
    size: i32,
    body_size: i32,
}
fn items(bytes: &[u8]) -> Vec<Item> {
    let h = xml::Element::parse(&package::part(bytes, "Contents/header.xml"));
    let sec = xml::Element::parse(&package::section_xml(bytes));
    let mut counts = BTreeMap::<u32, u32>::new();
    sec.descendants("hp:p")
        .iter()
        .map(|p| {
            let shape = h
                .descendants("hh:paraPr")
                .into_iter()
                .find(|s| s.attr("id") == p.attr("paraPrIDRef"))
                .unwrap();
            let heading = shape.child("hh:heading");
            let definition = heading.attr("idRef").unwrap().parse::<u32>().unwrap();
            let head = h
                .descendants("hh:numbering")
                .into_iter()
                .find(|n| n.attr("id") == heading.attr("idRef"))
                .unwrap()
                .descendants("hh:paraHead")[0];
            let n = counts
                .entry(definition)
                .or_insert_with(|| head.attr("start").unwrap().parse().unwrap());
            let text = head.text.replace("^1", &n.to_string());
            *n += 1;
            let char_shape =
                |id| h.descendants("hh:charPr").into_iter().find(|c| c.attr("id") == id).unwrap();
            let character = char_shape(head.attr("charPrIDRef"));
            let body = p
                .children
                .iter()
                .find(|r| {
                    r.name == "hp:run" && r.descendants("hp:t").iter().any(|t| t.text == "body")
                })
                .unwrap();
            let body = char_shape(body.attr("charPrIDRef"));
            assert!(body.descendants("hh:bold").is_empty());
            let margin = shape.child("hp:switch").child("hp:case").child("hh:margin");
            Item {
                definition,
                text,
                left: margin.child("hc:left").attr("value").unwrap().parse().unwrap(),
                intent: margin.child("hc:intent").attr("value").unwrap().parse().unwrap(),
                width: head.attr("widthAdjust").unwrap().parse().unwrap(),
                bold: !character.descendants("hh:bold").is_empty(),
                size: character.attr("height").unwrap().parse().unwrap(),
                body_size: body.attr("height").unwrap().parse().unwrap(),
            }
        })
        .collect()
}
fn check_warnings(warnings: &[OpsWarning], split: usize) {
    assert_eq!(
        warnings
            .iter()
            .filter(|w| matches!(w, OpsWarning::Encode(EncodeWarning::ListNumberingSplit { .. })))
            .count(),
        split
    );
    assert!(!warnings.iter().any(|w| matches!(w, OpsWarning::Encode(EncodeWarning::ContentLost { kind, .. }) if kind == "list_marker_conflict")));
}
// 이것을 실패시키는 것: 넘치는 LEFT 항목을 원래 칸으로 고정하거나 복제 start를 1로 저장.
#[test]
fn imported_decimal_overflow_preserves_values_and_continuation() {
    let o = convert_docx(&input(99, "left", &[(720, 360, ""); 3])).unwrap();
    let ps = items(&o.bytes);
    assert_eq!(ps.iter().map(|p| p.definition).collect::<Vec<_>>(), [2, 3, 3]);
    assert_eq!(
        ps.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(),
        list_counter_cases::CASES
            .iter()
            .find(|c| c.name == "decimal width transition")
            .unwrap()
            .expected
            .unwrap()
    );
    assert_eq!(
        ps.iter().map(|p| (p.left, p.intent, p.width)).collect::<Vec<_>>(),
        [(1800, 0, 700), (1800, -1800, 4300), (1800, -1800, 4300)]
    );
    assert!(!o
        .warnings
        .iter()
        .any(|w| matches!(w, OpsWarning::Docx(w) if w.loss() == "list_marker_overflow")));
    check_warnings(&o.warnings, 1);
}
// 이것을 실패시키는 것: 문단별 걸침 차이를 첫 head나 첫 이동량으로 투영.
#[test]
fn imported_individual_hanging_slots_keep_four_through_seven() {
    let o = convert_docx(&input(
        4,
        "left",
        &[(720, 360, ""), (960, 480, ""), (720, 480, ""), (720, 360, "")],
    ))
    .unwrap();
    let ps = items(&o.bytes);
    assert_eq!(ps.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(), ["4.", "5.", "6.", "7."]);
    assert_eq!(
        ps.iter().map(|p| (p.left, p.intent, p.width)).collect::<Vec<_>>(),
        [(1800, 0, 700), (2400, 0, 1300), (1200, 0, 1300), (1800, 0, 700)]
    );
    check_warnings(&o.warnings, 1);
}
// 이것을 실패시키는 것: 번호 문자 참조를 모두 첫 항목으로 고정하거나 본문에 번호 표시 서식을 적용.
#[test]
fn imported_paragraph_marks_style_only_their_own_markers() {
    for (mark, bold, size) in [("<w:b/>", true, 1100), ("<w:sz w:val='32'/>", false, 1600)] {
        let o =
            convert_docx(&input(1, "left", &[(720, 360, ""), (720, 360, mark), (720, 360, "")]))
                .unwrap();
        let ps = items(&o.bytes);
        assert_eq!(ps.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(), ["1.", "2.", "3."]);
        assert_eq!(
            ps.iter().map(|p| (p.bold, p.size, p.body_size)).collect::<Vec<_>>(),
            [(false, 1100, 1100), (bold, size, 1100), (false, 1100, 1100)]
        );
        check_warnings(&o.warnings, 1);
    }
}
// 이것을 실패시키는 것: RIGHT 칸을 넓힌 뒤 원래 여백을 남김.
#[test]
fn imported_right_decimal_slots_keep_the_marker_end() {
    let o = convert_docx(&input(99, "right", &[(720, 360, ""); 3])).unwrap();
    let ps = items(&o.bytes);
    assert_eq!(
        ps.iter().map(|p| (p.definition, p.left, p.intent, p.width)).collect::<Vec<_>>(),
        [(2, -260, 0, 960); 3]
    );
    assert_eq!(ps.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(), ["99.", "100.", "101."]);
    check_warnings(&o.warnings, 0);
}

// 이것을 실패시키는 것: 두 형식의 숫자 606·점 242 또는 측정 글꼴 이름 중 하나를 변경.
#[test]
fn docx_and_hwpx_share_decimal_measurements_and_font_names() {
    for font in ["Malgun Gothic", "맑은 고딕"] {
        for (value, ratio) in [(22, 1454), (333, 2060), (4444, 2666)] {
            let mark = "<w:sz w:val='110'/>";
            let right =
                convert_docx(&input_with_font(value, "right", &[(7200, 0, mark)], font)).unwrap();
            let marker = &items(&right.bytes)[0];
            let width = ratio * 5;
            assert_eq!(marker.width + marker.size, width, "{font}/{value}");
            // At 55pt the error band is 25 units and each ratio unit is 5 units.
            // These adjacent 5-unit slots detect a single-unit table disagreement.
            for (gap, uncertain) in [(width + 25, false), (width + 20, true)] {
                let left =
                    convert_docx(&input_with_font(value, "left", &[(7200, gap / 5, mark)], font))
                        .unwrap();
                let marker = &items(&left.bytes)[0];
                assert_eq!(marker.width + marker.size, gap, "{font}/{value}");
                assert_eq!(
                    left.warnings.iter().any(
                        |w| matches!(w, OpsWarning::Docx(w) if w.loss() == "list_marker_overflow")
                    ),
                    uncertain,
                    "{font}/{value}/{gap}"
                );
            }
        }
    }
}
