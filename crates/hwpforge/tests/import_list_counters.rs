#![cfg(feature = "ops-docx")]
#[path = "support/list_counter_inputs.rs"]
mod list_counter_inputs;
#[path = "../../hwpforge-smithy-hwpx/tests/support/list_counter_output.rs"]
mod list_counter_output;
#[path = "../../hwpforge-smithy-hwpx/tests/support/package.rs"]
mod package;
#[path = "../../hwpforge-smithy-docx/tests/support/word_counter_cases.rs"]
mod word_counter_cases;
#[path = "../../hwpforge-smithy-hwpx/tests/support/xml.rs"]
mod xml;
use hwpforge::hwpx::EncodeWarning;
use hwpforge::ops::{convert_docx, OpsWarning};
fn case(name: &str) -> word_counter_cases::Case {
    word_counter_cases::counter_cases().into_iter().find(|c| c.name == name).unwrap()
}
fn docx_loss(warnings: &[OpsWarning], loss: &str) -> u64 {
    warnings
        .iter()
        .filter_map(|w| match w {
            OpsWarning::Docx(w) if w.loss() == loss => Some(w.count()),
            _ => None,
        })
        .sum()
}
fn no_conflict(warnings: &[OpsWarning]) {
    assert!(!warnings.iter().any(|w| matches!(w,OpsWarning::Encode(EncodeWarning::ContentLost{kind,..}) if kind=="list_marker_conflict")));
}
fn assert_unique_level_properties(source: &xml::Element) {
    for level in source.descendants("w:lvl") {
        for property in ["w:pPr", "w:rPr"] {
            assert!(
                level.children.iter().filter(|child| child.name == property).count() <= 1,
                "at most one direct {property} in level {:?}",
                level.attr("w:ilvl")
            );
        }
    }
}
// 이것을 실패시키는 것: 생성 수준에 문단·문자 속성을 두 번 삽입.
#[test]
fn generated_levels_have_unique_property_children() {
    for source in word_counter_cases::counter_cases()
        .iter()
        .map(list_counter_inputs::word_case)
        .chain([list_counter_inputs::roman_compound(), list_counter_inputs::legal_and_deep()])
    {
        assert_unique_level_properties(&xml::Element::parse(&package::part(
            &source,
            "word/numbering.xml",
        )));
    }
}
// 이것을 실패시키는 것: 가져온 정의의 참조 재배정 또는 깊은 사본의 값 씨앗 누락.
#[test]
fn imported_counters_match_literal_sequences() {
    for case in word_counter_cases::counter_cases() {
        let result = convert_docx(&list_counter_inputs::word_case(&case)).unwrap();
        assert_eq!(list_counter_output::rendered(&result.bytes), case.expected, "{}", case.name);
        let header = xml::Element::parse(&package::part(&result.bytes, "Contents/header.xml"));
        for def in
            header.descendants("hh:numbering").into_iter().filter(|n| n.attr("id") != Some("1"))
        {
            assert!(def.descendants("hh:paraHead").iter().all(|h| h
                .attr("start")
                .unwrap()
                .parse::<u32>()
                .unwrap()
                > 0));
        }
        no_conflict(&result.warnings);
    }
}
// 이것을 실패시키는 것: 하류에서 원래 경고의 count·part·location 또는 decode 문맥 폐기.
#[test]
fn imported_counter_warnings_keep_their_context() {
    let result = convert_docx(&list_counter_inputs::word_case(&case(
        "legal_markers_preserve_returning_parent_values",
    )))
    .unwrap();
    let warning = result
        .warnings
        .iter()
        .find(|w| matches!(w,OpsWarning::Docx(w) if w.loss()=="list_numbering_split"))
        .unwrap();
    let wire = serde_json::to_value(warning.info()).unwrap();
    assert_eq!(wire["code"], "DOCX_VALUE_APPROXIMATED");
    assert_eq!(wire["context"]["format"], "docx");
    assert_eq!(wire["context"]["stage"], "decode");
    assert_eq!(wire["context"]["loss"], "list_numbering_split");
    assert_eq!(wire["context"]["action"], "approximate");
    assert_eq!(wire["context"]["count"], 3);
    assert_eq!(wire["context"]["part"], "word/document.xml");
    assert_eq!(wire["context"]["location"], "numPr");
}
struct Item {
    level: usize,
    width: i32,
    left: i32,
    intent: i32,
    bold: bool,
    format: String,
    definition: String,
}
fn items(bytes: &[u8]) -> Vec<Item> {
    let h = xml::Element::parse(&package::part(bytes, "Contents/header.xml"));
    let s = xml::Element::parse(&package::part(bytes, "Contents/section0.xml"));
    s.descendants("hp:p")
        .into_iter()
        .filter_map(|p| {
            let shape = h
                .descendants("hh:paraPr")
                .into_iter()
                .find(|s| s.attr("id") == p.attr("paraPrIDRef"))
                .unwrap();
            let heading = shape.child("hh:heading");
            if heading.attr("type") != Some("NUMBER") {
                return None;
            }
            let level = heading.attr("level").unwrap().parse::<usize>().unwrap();
            let definition = heading.attr("idRef").unwrap();
            let def = h
                .descendants("hh:numbering")
                .into_iter()
                .find(|n| n.attr("id") == Some(definition))
                .unwrap();
            let head = def
                .descendants("hh:paraHead")
                .into_iter()
                .find(|h| h.attr("level") == Some((level + 1).to_string().as_str()))
                .unwrap();
            let character = h
                .descendants("hh:charPr")
                .into_iter()
                .find(|c| c.attr("id") == head.attr("charPrIDRef"));
            assert!(character.is_some(), "assertion failed: explicit marker character reference");
            let character = character.unwrap();
            assert_eq!(character.attr("height"), Some("1100"));
            for run in p
                .children
                .iter()
                .filter(|r| r.name == "hp:run" && !r.descendants("hp:t").is_empty())
            {
                let body = h
                    .descendants("hh:charPr")
                    .into_iter()
                    .find(|c| c.attr("id") == run.attr("charPrIDRef"))
                    .unwrap();
                assert!(body.descendants("hh:bold").is_empty());
            }
            let margin = shape.child("hp:switch").child("hp:case").child("hh:margin");
            assert_eq!(head.attr("textOffsetType"), Some("PERCENT"));
            assert_eq!(head.attr("textOffset"), Some("0"));
            Some(Item {
                level,
                width: head.attr("widthAdjust").unwrap().parse().unwrap(),
                left: margin.child("hc:left").attr("value").unwrap().parse().unwrap(),
                intent: margin.child("hc:intent").attr("value").unwrap().parse().unwrap(),
                bold: !character.descendants("hh:bold").is_empty(),
                format: head.attr("numFormat").unwrap().to_owned(),
                definition: definition.to_owned(),
            })
        })
        .collect()
}
// 이것을 실패시키는 것: 하위 음수 widthAdjust를 0으로 자르거나 표식의 굵기를 본문에 적용.
#[test]
fn imported_roman_compound_markers_keep_coordinates() {
    let result = convert_docx(&list_counter_inputs::roman_compound()).unwrap();
    assert_eq!(list_counter_output::rendered(&result.bytes), ["I.", "I.1", "I.2", "II.", "II.1"]);
    let rows = items(&result.bytes);
    assert_eq!(rows.iter().map(|r| r.width).collect::<Vec<_>>(), [150, -400, -400, 150, -400]);
    assert_eq!(rows.iter().map(|r| r.bold).collect::<Vec<_>>(), [false, false, true, false, false]);
    assert_eq!(
        rows.iter().map(|r| (r.left, r.intent)).collect::<Vec<_>>(),
        [(1800, 0), (3600, 0), (3600, 0), (1800, 0), (3600, 0)]
    );
    assert_eq!(rows.iter().map(|r| r.level).collect::<Vec<_>>(), [0, 1, 1, 0, 1]);
    no_conflict(&result.warnings);
}
// 이것을 실패시키는 것: 법률식 하위를 위해 원래 상위 정의의 로마 형식을 변경.
#[test]
fn imported_legal_markers_use_decimal_ancestor_copies() {
    let result = convert_docx(&list_counter_inputs::word_case(&case(
        "legal_markers_use_decimal_reference_segments",
    )))
    .unwrap();
    assert_eq!(list_counter_output::rendered(&result.bytes), ["I.", "1.1", "1.2"]);
    let rows = items(&result.bytes);
    assert_eq!(rows[0].format, "ROMAN_CAPITAL");
    assert_ne!(rows[0].definition, rows[1].definition);
    assert_eq!(rows[1].definition, rows[2].definition);
    let header = xml::Element::parse(&package::part(&result.bytes, "Contents/header.xml"));
    let def = header
        .descendants("hh:numbering")
        .into_iter()
        .find(|n| n.attr("id") == Some(rows[1].definition.as_str()))
        .unwrap();
    assert_eq!(def.descendants("hh:paraHead")[0].attr("numFormat"), Some("DIGIT"));
    assert_eq!(docx_loss(&result.warnings, "list_number_legal"), 0);
}
// 이것을 실패시키는 것: 십진 리터럴 0에서 기본 탭 요청을 생략.
#[test]
fn imported_literal_zero_keeps_marker_layout() {
    let mut case = case("omitted_start_uses_a_literal_zero");
    case.definitions = case.definitions.replace(
        "<w:lvl w:ilvl='0'>",
        "<w:lvl w:ilvl='0'><w:pPr><w:ind w:left='720' w:hanging='0'/></w:pPr>",
    );
    let result = convert_docx(&list_counter_inputs::word_case(&case)).unwrap();
    assert_eq!(list_counter_output::rendered(&result.bytes), ["0.", "1.", "2."]);
    let rows = items(&result.bytes);
    assert_eq!(rows[0].left, 3600);
    assert_eq!(rows[0].intent, 0);
    assert_eq!(rows[0].width, 2500);
    assert_eq!(docx_loss(&result.warnings, "list_marker_zero_slot"), 0);
}
// 이것을 실패시키는 것: 명목 폭 지원을 기존 십진·라틴 실제 폭 정책에 적용.
#[test]
fn imported_plain_list_layouts_remain_unchanged() {
    let result =
        convert_docx(&list_counter_inputs::word_case(&case("default_restart_resets_children")))
            .unwrap();
    assert_eq!(list_counter_output::rendered(&result.bytes), ["1.", "a)", "b)", "2.", "a)"]);
    assert_eq!(
        items(&result.bytes).iter().map(|r| (r.left, r.intent, r.width)).collect::<Vec<_>>(),
        [(1800, 0, 700), (3600, 0, 700), (3600, 0, 700), (1800, 0, 700), (3600, 0, 700)]
    );
    no_conflict(&result.warnings);
}
// HWPFORGE_LIST_VISUAL_OUTPUT을 지정하면 시각 확인용 DOCX를 해당 디렉터리에 저장합니다.
// 이것을 실패시키는 것: 여섯 독립 목록의 정의 공유 또는 마지막 표식의 굵기 누락.
#[test]
fn generated_legal_and_deep_inputs_preserve_observations() {
    let bytes = list_counter_inputs::legal_and_deep();
    let source = xml::Element::parse(&package::part(&bytes, "word/numbering.xml"));
    assert_unique_level_properties(&source);
    assert_eq!(source.descendants("w:abstractNum").len(), 6);
    assert_eq!(
        source.descendants("w:multiLevelType").iter().map(|e| e.attr("w:val")).collect::<Vec<_>>(),
        [
            Some("multilevel"),
            Some("singleLevel"),
            Some("singleLevel"),
            Some("multilevel"),
            Some("multilevel"),
            Some("multilevel")
        ]
    );
    let document = xml::Element::parse(&package::part(&bytes, "word/document.xml"));
    assert_eq!(document.find("w:pgSz").attr("w:w"), Some("11906"));
    assert_eq!(document.find("w:pgSz").attr("w:h"), Some("16838"));
    assert_eq!(document.find("w:pgMar").attr("w:left"), Some("1417"));
    assert_eq!(document.descendants("w:b").len(), 1);
    let styles = xml::Element::parse(&package::part(&bytes, "word/styles.xml"));
    for slot in ["w:ascii", "w:hAnsi", "w:eastAsia", "w:cs"] {
        assert_eq!(styles.find("w:rFonts").attr(slot), Some("Malgun Gothic"));
    }
    assert_eq!(styles.find("w:sz").attr("w:val"), Some("22"));
    assert_eq!(styles.find("w:spacing").attr("w:after"), Some("80"));
    assert_eq!(styles.find("w:spacing").attr("w:line"), Some("240"));
    let result = convert_docx(&bytes).unwrap();
    assert_eq!(
        list_counter_output::rendered(&result.bytes),
        [
            "I.", "1.1", "1.2", "II.", "2.1", "1.", "2.", "3.", "I.", "II.", "III.", "2.3.4", "3.",
            "3.", "3.3.4", "2.", "3.", "2.3.4", "2.3.5", "1.", "I.1", "I.2", "2.", "II.1"
        ]
    );
    let rows = items(&result.bytes);
    assert_eq!(
        rows.iter().enumerate().filter_map(|(i, r)| r.bold.then_some(i)).collect::<Vec<_>>(),
        [18]
    );
    no_conflict(&result.warnings);
    if let Some(directory) = std::env::var_os("HWPFORGE_LIST_VISUAL_OUTPUT") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("roman-compound.docx"),
            list_counter_inputs::roman_compound(),
        )
        .unwrap();
        std::fs::write(directory.join("legal-and-deep.docx"), bytes).unwrap();
    }
}

// 이것을 실패시키는 것: 복합 번호를 측정에서 제외하거나 마지막 굵은 번호를 원래 칸에 유지.
#[test]
fn imported_decimal_compounds_advance_to_the_next_tab() {
    let result = convert_docx(&list_counter_inputs::legal_and_deep()).unwrap();
    let rows = items(&result.bytes);
    for index in [11, 14, 17, 18] {
        assert_eq!(rows[index].width, 2650, "item {index}");
    }
    // 로마 복합 번호의 두 정의·수준 쌍만 경고하고 로마 단일 번호는 유지한다.
    assert_eq!(docx_loss(&result.warnings, "list_marker_overflow"), 2);
}
// 이것을 실패시키는 것: 모든 복합 번호의 칸을 무조건 다음 탭으로 확대.
#[test]
fn imported_short_decimal_compounds_keep_their_slot() {
    for name in
        ["legal_markers_use_decimal_reference_segments", "child_first_consumes_its_parent_once"]
    {
        let result = convert_docx(&list_counter_inputs::word_case(&case(name))).unwrap();
        for row in items(&result.bytes).iter().filter(|row| row.level == 1) {
            assert_eq!(row.width, 150);
            assert_eq!(row.left, 3600);
        }
        assert_eq!(docx_loss(&result.warnings, "list_marker_overflow"), 0);
    }
}
// 이것을 실패시키는 것: 다자리 상위 참조 값을 한 자리로 측정하거나 다음 탭을 하나로 고정.
#[test]
fn imported_wide_decimal_compounds_can_cross_another_tab() {
    let mut source = case("deep_first_use_preserves_nonunit_starts");
    source.definitions = source
        .definitions
        .replace("w:val='2'/><w:numFmt", "w:val='2222'/><w:numFmt")
        .replace("w:val='3'/><w:numFmt", "w:val='3333'/><w:numFmt")
        .replace("w:val='4'/><w:numFmt", "w:val='4444'/><w:numFmt");
    source.uses = vec![(7, 2)];
    let result = convert_docx(&list_counter_inputs::word_case(&source)).unwrap();
    // 기존 비율·탭 산식으로 정한 구조 단언이며 Word 실측값이 아니다.
    assert_eq!(items(&result.bytes)[0].width, 6250);
    assert_eq!(docx_loss(&result.warnings, "list_marker_overflow"), 0);
}
// 이것을 실패시키는 것: legal 참조 상위의 원래 로마 형식으로 폭 측정을 거부.
#[test]
fn imported_legal_decimal_compounds_use_normalized_reference_widths() {
    let mut source = case("deep_first_use_preserves_nonunit_starts");
    source.definitions = source
        .definitions
        .replacen("w:val='decimal'", "w:val='upperRoman'", 1)
        .replace("<w:lvl w:ilvl='2'>", "<w:lvl w:ilvl='2'><w:isLgl/>");
    source.uses = vec![(7, 2)];
    let result = convert_docx(&list_counter_inputs::word_case(&source)).unwrap();
    assert_eq!(items(&result.bytes)[0].width, 2650);
    assert_eq!(docx_loss(&result.warnings, "list_marker_overflow"), 0);
}
// 이것을 실패시키는 것: 실제 폭이 확인되지 않은 비십진 복합 번호를 경고 없이 유지.
#[test]
fn imported_unmeasured_compounds_warn_and_retain_the_slot() {
    let mut source = case("deep_first_use_preserves_nonunit_starts");
    source.definitions = source.definitions.replacen("w:val='decimal'", "w:val='upperRoman'", 1);
    source.uses = vec![(7, 2)];
    let result = convert_docx(&list_counter_inputs::word_case(&source)).unwrap();
    assert_eq!(docx_loss(&result.warnings, "list_marker_overflow"), 1);
    let warning = result
        .warnings
        .iter()
        .find_map(|warning| match warning {
            OpsWarning::Docx(w) if w.loss() == "list_marker_overflow" => Some(w),
            _ => None,
        })
        .unwrap();
    assert_eq!(warning.action(), "retain_slot");
    assert_eq!(items(&result.bytes)[0].width, -1500);
}
// 이것을 실패시키는 것: 로마 상위 값에 따라 폭 미확인 경고를 생략.
#[test]
fn imported_roman_compounds_warn_independently_of_counter_values() {
    for start in [1, 3] {
        let mut source = case("compound_markers_keep_roman_ancestors");
        source.definitions = source.definitions.replacen(
            "w:val='1'/><w:numFmt",
            &format!("w:val='{start}'/><w:numFmt"),
            1,
        );
        source.uses = vec![(7, 1), (7, 1)];
        let result = convert_docx(&list_counter_inputs::word_case(&source)).unwrap();
        assert_eq!(
            list_counter_output::rendered(&result.bytes)[0],
            if start == 1 { "I.1" } else { "III.1" }
        );
        assert_eq!(docx_loss(&result.warnings, "list_marker_overflow"), 1);
        for row in items(&result.bytes) {
            assert_eq!(row.width, -400);
        }
    }
}

fn marker_style(font: &str, bold: bool, size: u32) -> String {
    format!("<w:rFonts w:ascii='{font}' w:hAnsi='{font}' w:eastAsia='{font}' w:cs='{font}'/><w:sz w:val='{size}'/><w:szCs w:val='{size}'/>{}", if bold { "<w:b/>" } else { "" })
}
fn assert_marker_slot(input: &[u8], gap: i32, nominal: i32, warnings: u64) {
    let decoded = hwpforge::docx::decode_docx(input).unwrap();
    let p = &decoded.document.sections()[0].paragraphs[0];
    let shape = &decoded.registry.para_shapes[p.para_shape_id.get()];
    assert_eq!(
        shape.list_marker.unwrap().text_start,
        hwpforge::blueprint::ListTextStart::FromAnchor(
            hwpforge::foundation::HwpUnit::new(gap).unwrap()
        )
    );
    let align = shape.list_marker.unwrap().align;
    let slot = if align == hwpforge::blueprint::ListMarkerAlign::Center { gap * 2 } else { gap };
    let result = convert_docx(input).unwrap();
    assert_eq!(docx_loss(&result.warnings, "list_marker_overflow"), warnings);
    for warning in &result.warnings {
        if let OpsWarning::Docx(w) = warning {
            if w.loss() == "list_marker_overflow" {
                assert_eq!(w.action(), "retain_slot");
                assert!(matches!(
                    w.message().as_str(),
                    "Marker width is uncertain in the fixed slot; overlap is possible"
                        | "Nominal marker width exceeds the fixed slot; overlap is possible"
                ));
            }
        }
    }
    let h = xml::Element::parse(&package::part(&result.bytes, "Contents/header.xml"));
    let s = xml::Element::parse(&package::part(&result.bytes, "Contents/section0.xml"));
    let p = s.descendants("hp:p")[0];
    let shape = h
        .descendants("hh:paraPr")
        .into_iter()
        .find(|s| s.attr("id") == p.attr("paraPrIDRef"))
        .unwrap();
    let heading = shape.child("hh:heading");
    let level = heading.attr("level").unwrap().parse::<usize>().unwrap() + 1;
    let def = h
        .descendants("hh:numbering")
        .into_iter()
        .find(|n| n.attr("id") == heading.attr("idRef"))
        .unwrap();
    let head = def
        .descendants("hh:paraHead")
        .into_iter()
        .find(|h| h.attr("level") == Some(level.to_string().as_str()))
        .unwrap();
    // 기존 RIGHT 배치는 max(칸, 명목 폭)을 사용한다.
    let width = if align == hwpforge::blueprint::ListMarkerAlign::Right {
        (slot - nominal).max(0)
    } else {
        slot - nominal
    };
    assert_eq!(head.attr("widthAdjust").unwrap().parse::<i32>().unwrap(), width);
}
fn marker_bands(compound: bool, nondecimal: bool, font: &str, bold: bool, layout: (&str, bool)) {
    // 미측정 형식의 띠는 십진 대조군의 칸이며 새 글자 폭을 가정하지 않는다.
    let boundary = if compound { 2300 } else { 850 };
    let nominal = match (compound, nondecimal) {
        (false, false) => 1100,
        (false, true) => 1650,
        (true, false) => 2750,
        (true, true) => 3300,
    };
    for (band, gap) in [("keep", 3000), ("boundary", boundary), ("overflow", 500)] {
        let measured = !nondecimal && font == "Malgun Gothic" && layout == ("left", false);
        let advance = measured && band == "overflow";
        let expected = if advance {
            if compound {
                5400
            } else {
                1800
            }
        } else {
            gap
        };
        let warn = if nondecimal {
            compound
        } else if measured {
            band == "boundary"
        } else {
            let count = if compound { 5 } else { 2 };
            count * 1100 > gap * 2 * if layout.0 == "center" { 2 } else { 1 }
        };
        let input = list_counter_inputs::marker_slot_layout(
            compound,
            nondecimal,
            &marker_style(font, bold, 22),
            gap,
            1800,
            layout,
        );
        assert_marker_slot(&input, expected, nominal, u64::from(warn));
    }
}
macro_rules! marker_band_test {
    ($name:ident, $compound:expr, $nondecimal:expr, $font:expr, $bold:expr) => {
        // 이것을 실패시키는 것: 해당 띠의 경고 유무를 반전하거나 유지·이동 칸을 변경.
        #[test]
        fn $name() {
            marker_bands($compound, $nondecimal, $font, $bold, ("left", false));
        }
    };
}
marker_band_test!(single_decimal_regular_marker_bands, false, false, "Malgun Gothic", false);
marker_band_test!(single_decimal_bold_marker_bands, false, false, "Malgun Gothic", true);
marker_band_test!(single_decimal_other_font_marker_bands, false, false, "Times New Roman", false);
marker_band_test!(single_nondecimal_regular_marker_bands, false, true, "Malgun Gothic", false);
marker_band_test!(single_nondecimal_bold_marker_bands, false, true, "Malgun Gothic", true);
marker_band_test!(single_nondecimal_other_font_marker_bands, false, true, "Times New Roman", false);
marker_band_test!(compound_decimal_regular_marker_bands, true, false, "Malgun Gothic", false);
marker_band_test!(compound_decimal_bold_marker_bands, true, false, "Malgun Gothic", true);
marker_band_test!(compound_decimal_other_font_marker_bands, true, false, "Times New Roman", false);
marker_band_test!(compound_nondecimal_regular_marker_bands, true, true, "Malgun Gothic", false);
marker_band_test!(compound_nondecimal_bold_marker_bands, true, true, "Malgun Gothic", true);
marker_band_test!(
    compound_nondecimal_other_font_marker_bands,
    true,
    true,
    "Times New Roman",
    false
);

// 이것을 실패시키는 것: 다른 글꼴의 십진 복합 번호를 경고 없이 유지하거나 칸을 확대.
#[test]
fn imported_other_font_decimal_compounds_warn_in_fixed_slots() {
    for font in ["바탕", "Times New Roman"] {
        let input = list_counter_inputs::marker_slot(
            true,
            false,
            &marker_style(font, false, 22),
            1800,
            1800,
        );
        assert_marker_slot(&input, 1800, 2750, 1);
    }
}
// 이것을 실패시키는 것: 굵은 복합 번호의 확정 넘침을 특정 크기에만 허용.
#[test]
fn imported_bold_decimal_compounds_advance_at_twelve_points() {
    let input = list_counter_inputs::marker_slot(
        true,
        false,
        &marker_style("Malgun Gothic", true, 24),
        1800,
        1800,
    );
    assert_marker_slot(&input, 5400, 3000, 0);
}
// 이것을 실패시키는 것: 보통체 하한이 맞는 굵은 번호의 명목 상한 검사를 생략하거나 무조건 경고.
#[test]
fn imported_bold_decimal_compounds_check_nominal_bound_when_lower_bound_fits() {
    for (gap, warnings) in [(3000, 0), (2600, 1)] {
        let input = list_counter_inputs::marker_slot(
            true,
            false,
            &marker_style("Malgun Gothic", true, 24),
            gap,
            1800,
        );
        assert_marker_slot(&input, gap, 3000, warnings);
    }
}
// 이것을 실패시키는 것: 다음 탭 경계에 가까운 보통체 하한을 굵은 번호의 확정 이동으로 취급.
#[test]
fn imported_bold_markers_warn_near_the_next_tab() {
    for (compound, anchor, nominal) in [(false, 2750, 1100), (true, 1295, 2750)] {
        let input = list_counter_inputs::marker_slot(
            compound,
            false,
            &marker_style("Malgun Gothic", true, 22),
            500,
            anchor,
        );
        assert_marker_slot(&input, 500, nominal, 1);
    }
}

// 이것을 실패시키는 것: Times의 짧은 십진 번호에도 측정 불확실 경고를 추가.
#[test]
fn imported_other_font_short_decimal_keeps_nominal_slot() {
    let input = list_counter_inputs::marker_slot(
        false,
        false,
        &marker_style("Times New Roman", false, 22),
        1800,
        1800,
    );
    assert_marker_slot(&input, 1800, 1100, 0);
}

// 이것을 실패시키는 것: 굵은 두 수준 십진 번호의 명목 상한이 칸 안이어도 경고.
#[test]
fn imported_bold_short_decimal_compounds_keep_nominal_slot() {
    let mut source = case("compound_markers_keep_roman_ancestors");
    source.definitions = source.definitions.replace("upperRoman", "decimal");
    source.uses = vec![(7, 1)];
    source.marker_bold = Some(0);
    assert_marker_slot(&list_counter_inputs::word_case(&source), 1800, 1650, 0);
}

// 이것을 실패시키는 것: 가운데 정렬 복합 번호를 명목 검사에서 제외하거나 칸을 확대.
#[test]
fn imported_centered_decimal_compounds_warn_on_nominal_overflow() {
    let input = list_counter_inputs::marker_slot(
        true,
        false,
        &marker_style("Times New Roman", false, 22),
        500,
        1800,
    );
    let input = package::splice_part(
        &input,
        "word/numbering.xml",
        "w:lvlJc w:val='left'",
        "w:lvlJc w:val='center'",
    );
    assert_marker_slot(&input, 500, 2750, 1);
}

macro_rules! fixed_slot_band_test {
    ($name:ident, $compound:expr, $nondecimal:expr, $font:expr, $bold:expr, $align:expr, $tab:expr) => {
        // 이것을 실패시키는 것: 정렬·명시 탭 경로에서 경고를 반전하거나 고정 칸을 변경.
        #[test]
        fn $name() {
            marker_bands($compound, $nondecimal, $font, $bold, ($align, $tab));
        }
    };
}
fixed_slot_band_test!(
    single_decimal_regular_left_tab_marker_bands,
    false,
    false,
    "Malgun Gothic",
    false,
    "left",
    true
);
fixed_slot_band_test!(
    single_decimal_regular_center_marker_bands,
    false,
    false,
    "Malgun Gothic",
    false,
    "center",
    false
);
fixed_slot_band_test!(
    single_decimal_regular_right_marker_bands,
    false,
    false,
    "Malgun Gothic",
    false,
    "right",
    false
);
fixed_slot_band_test!(
    single_decimal_bold_left_tab_marker_bands,
    false,
    false,
    "Malgun Gothic",
    true,
    "left",
    true
);
fixed_slot_band_test!(
    single_decimal_bold_center_marker_bands,
    false,
    false,
    "Malgun Gothic",
    true,
    "center",
    false
);
fixed_slot_band_test!(
    single_decimal_bold_right_marker_bands,
    false,
    false,
    "Malgun Gothic",
    true,
    "right",
    false
);
fixed_slot_band_test!(
    single_decimal_other_font_left_tab_marker_bands,
    false,
    false,
    "Times New Roman",
    false,
    "left",
    true
);
fixed_slot_band_test!(
    single_decimal_other_font_center_marker_bands,
    false,
    false,
    "Times New Roman",
    false,
    "center",
    false
);
fixed_slot_band_test!(
    single_decimal_other_font_right_marker_bands,
    false,
    false,
    "Times New Roman",
    false,
    "right",
    false
);
fixed_slot_band_test!(
    single_nondecimal_regular_left_tab_marker_bands,
    false,
    true,
    "Malgun Gothic",
    false,
    "left",
    true
);
fixed_slot_band_test!(
    single_nondecimal_regular_center_marker_bands,
    false,
    true,
    "Malgun Gothic",
    false,
    "center",
    false
);
fixed_slot_band_test!(
    single_nondecimal_regular_right_marker_bands,
    false,
    true,
    "Malgun Gothic",
    false,
    "right",
    false
);
fixed_slot_band_test!(
    single_nondecimal_bold_left_tab_marker_bands,
    false,
    true,
    "Malgun Gothic",
    true,
    "left",
    true
);
fixed_slot_band_test!(
    single_nondecimal_bold_center_marker_bands,
    false,
    true,
    "Malgun Gothic",
    true,
    "center",
    false
);
fixed_slot_band_test!(
    single_nondecimal_bold_right_marker_bands,
    false,
    true,
    "Malgun Gothic",
    true,
    "right",
    false
);
fixed_slot_band_test!(
    single_nondecimal_other_font_left_tab_marker_bands,
    false,
    true,
    "Times New Roman",
    false,
    "left",
    true
);
fixed_slot_band_test!(
    single_nondecimal_other_font_center_marker_bands,
    false,
    true,
    "Times New Roman",
    false,
    "center",
    false
);
fixed_slot_band_test!(
    single_nondecimal_other_font_right_marker_bands,
    false,
    true,
    "Times New Roman",
    false,
    "right",
    false
);
fixed_slot_band_test!(
    compound_decimal_regular_left_tab_marker_bands,
    true,
    false,
    "Malgun Gothic",
    false,
    "left",
    true
);
fixed_slot_band_test!(
    compound_decimal_regular_center_marker_bands,
    true,
    false,
    "Malgun Gothic",
    false,
    "center",
    false
);
fixed_slot_band_test!(
    compound_decimal_regular_right_marker_bands,
    true,
    false,
    "Malgun Gothic",
    false,
    "right",
    false
);
fixed_slot_band_test!(
    compound_decimal_bold_left_tab_marker_bands,
    true,
    false,
    "Malgun Gothic",
    true,
    "left",
    true
);
fixed_slot_band_test!(
    compound_decimal_bold_center_marker_bands,
    true,
    false,
    "Malgun Gothic",
    true,
    "center",
    false
);
fixed_slot_band_test!(
    compound_decimal_bold_right_marker_bands,
    true,
    false,
    "Malgun Gothic",
    true,
    "right",
    false
);
fixed_slot_band_test!(
    compound_decimal_other_font_left_tab_marker_bands,
    true,
    false,
    "Times New Roman",
    false,
    "left",
    true
);
fixed_slot_band_test!(
    compound_decimal_other_font_center_marker_bands,
    true,
    false,
    "Times New Roman",
    false,
    "center",
    false
);
fixed_slot_band_test!(
    compound_decimal_other_font_right_marker_bands,
    true,
    false,
    "Times New Roman",
    false,
    "right",
    false
);
fixed_slot_band_test!(
    compound_nondecimal_regular_left_tab_marker_bands,
    true,
    true,
    "Malgun Gothic",
    false,
    "left",
    true
);
fixed_slot_band_test!(
    compound_nondecimal_regular_center_marker_bands,
    true,
    true,
    "Malgun Gothic",
    false,
    "center",
    false
);
fixed_slot_band_test!(
    compound_nondecimal_regular_right_marker_bands,
    true,
    true,
    "Malgun Gothic",
    false,
    "right",
    false
);
fixed_slot_band_test!(
    compound_nondecimal_bold_left_tab_marker_bands,
    true,
    true,
    "Malgun Gothic",
    true,
    "left",
    true
);
fixed_slot_band_test!(
    compound_nondecimal_bold_center_marker_bands,
    true,
    true,
    "Malgun Gothic",
    true,
    "center",
    false
);
fixed_slot_band_test!(
    compound_nondecimal_bold_right_marker_bands,
    true,
    true,
    "Malgun Gothic",
    true,
    "right",
    false
);
fixed_slot_band_test!(
    compound_nondecimal_other_font_left_tab_marker_bands,
    true,
    true,
    "Times New Roman",
    false,
    "left",
    true
);
fixed_slot_band_test!(
    compound_nondecimal_other_font_center_marker_bands,
    true,
    true,
    "Times New Roman",
    false,
    "center",
    false
);
fixed_slot_band_test!(
    compound_nondecimal_other_font_right_marker_bands,
    true,
    true,
    "Times New Roman",
    false,
    "right",
    false
);
