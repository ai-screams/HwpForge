//! Generated source numbering and independently observed marker strings.
#![allow(dead_code)]
pub struct Case {
    pub name: &'static str,
    pub definitions: String,
    pub uses: Vec<(u32, u32)>,
    pub expected: &'static [&'static str],
    pub styles: &'static str,
}
pub fn level(index: u32, start: Option<u32>, format: &str, text: &str, extra: &str) -> String {
    let start = start.map_or_else(String::new, |n| format!("<w:start w:val='{n}'/>"));
    format!("<w:lvl w:ilvl='{index}'>{start}<w:numFmt w:val='{format}'/><w:lvlText w:val='{text}'/>{extra}</w:lvl>")
}
pub fn num(id: u32, abstract_id: u32, extra: &str) -> String {
    format!("<w:num w:numId='{id}'><w:abstractNumId w:val='{abstract_id}'/>{extra}</w:num>")
}
pub fn reset(level: u32, start: u32) -> String {
    format!("<w:lvlOverride w:ilvl='{level}'><w:startOverride w:val='{start}'/></w:lvlOverride>")
}
pub fn definition(id: u32, levels: &str) -> String {
    format!("<w:abstractNum w:abstractNumId='{id}'>{levels}</w:abstractNum>")
}
fn case(
    name: &'static str,
    levels: &str,
    a: &str,
    b: &str,
    uses: &[(u32, u32)],
    expected: &'static [&'static str],
) -> Case {
    Case {
        name,
        definitions: format!("{}{}{}", definition(1, levels), num(7, 1, a), num(8, 1, b)),
        uses: uses.to_vec(),
        expected,
        styles: "",
    }
}
pub fn counter_cases() -> Vec<Case> {
    let decimal = level(0, Some(1), "decimal", "%1.", "");
    let alphabet = level(1, Some(1), "lowerLetter", "%2)", "");
    let basic = format!("{decimal}{alphabet}");
    let shared = [(7, 0), (7, 0), (8, 0), (8, 0), (7, 0)];
    let returning = [(7, 0), (7, 0), (8, 0), (8, 0), (8, 0), (7, 0)];
    let parent = [(7, 0), (7, 1), (7, 1), (7, 0), (7, 1)];
    let mut cases = vec![
        // 이것을 실패시키는 것: 생략 시작값을 1로 처리.
        case(
            "omitted_start_uses_a_literal_zero",
            &level(0, None, "decimal", "%1.", ""),
            "",
            "",
            &[(7, 0); 3],
            &["0.", "1.", "2."],
        ),
        // 이것을 실패시키는 것: 명시적 0을 Core start 0으로 직접 기록.
        case(
            "explicit_zero_uses_a_literal_zero",
            &level(0, Some(0), "decimal", "%1.", ""),
            "",
            "",
            &[(7, 0); 3],
            &["0.", "1.", "2."],
        ),
        // 이것을 실패시키는 것: num별 독립 카운터로 복귀.
        case(
            "instances_share_the_abstract_counter",
            &decimal,
            "",
            "",
            &shared,
            &["1.", "2.", "3.", "4.", "5."],
        ),
        // 이것을 실패시키는 것: 첫 override를 무시.
        case(
            "first_override_resets_the_shared_counter",
            &decimal,
            "",
            &reset(0, 1),
            &shared,
            &["1.", "2.", "1.", "2.", "3."],
        ),
        // 이것을 실패시키는 것: 서식 override가 카운터를 나눔.
        case(
            "format_only_overrides_keep_progression",
            &decimal,
            "",
            &format!(
                "<w:lvlOverride w:ilvl='0'>{}</w:lvlOverride>",
                level(0, Some(1), "decimal", "%1.", "<w:rPr><w:b/></w:rPr>")
            ),
            &shared,
            &["1.", "2.", "3.", "4.", "5."],
        ),
        // 이것을 실패시키는 것: 덮어쓴 lvl의 start로 공유 값을 재설정.
        case(
            "overridden_level_start_does_not_reset",
            &decimal,
            "",
            &format!(
                "<w:lvlOverride w:ilvl='0'>{}</w:lvlOverride>",
                level(0, Some(5), "decimal", "%1.", "")
            ),
            &shared,
            &["1.", "2.", "3.", "4.", "5."],
        ),
        // 이것을 실패시키는 것: 기본 재시작 뒤 하위 used를 유지.
        case(
            "default_restart_resets_children",
            &basic,
            "",
            "",
            &parent,
            &["1.", "a)", "b)", "2.", "a)"],
        ),
        // 이것을 실패시키는 것: restart 0도 재시작.
        case(
            "zero_restart_preserves_child_progression",
            &format!(
                "{decimal}{}",
                level(1, Some(1), "lowerLetter", "%2)", "<w:lvlRestart w:val='0'/>")
            ),
            "",
            "",
            &parent,
            &["1.", "a)", "b)", "2.", "c)"],
        ),
        // 이것을 실패시키는 것: 재시작 임계값을 바로 위 수준으로 축약.
        case(
            "restart_threshold_uses_its_value",
            &format!(
                "{basic}{}",
                level(2, Some(1), "lowerRoman", "%3.", "<w:lvlRestart w:val='1'/>")
            ),
            "",
            "",
            &[(7, 0), (7, 1), (7, 2), (7, 2), (7, 1), (7, 2), (7, 0), (7, 2)],
            &["1.", "a)", "i.", "ii.", "b)", "iii.", "2.", "i."],
        ),
        // 이것을 실패시키는 것: override를 num의 매 등장마다 재적용.
        case(
            "override_then_return_uses_shared_progression",
            &decimal,
            "",
            &reset(0, 1),
            &returning,
            &["1.", "2.", "1.", "2.", "3.", "4."],
        ),
        // 이것을 실패시키는 것: 재설정 뒤 다른 num이 3으로 복귀.
        case(
            "nonunit_override_then_return_uses_shared_progression",
            &decimal,
            "",
            &reset(0, 10),
            &returning,
            &["1.", "2.", "10.", "11.", "12.", "13."],
        ),
        // 이것을 실패시키는 것: abstract당 override를 한 번만 소비.
        case(
            "each_instance_consumes_its_override_once",
            &decimal,
            &reset(0, 1),
            &reset(0, 1),
            &returning,
            &["1.", "2.", "1.", "2.", "3.", "4."],
        ),
        // 이것을 실패시키는 것: override를 하위의 재시작 start로 유지.
        case(
            "child_override_expires_after_parent_restart",
            &basic,
            "",
            &reset(1, 5),
            &[(7, 0), (7, 1), (7, 1), (8, 1), (8, 1), (7, 1), (7, 0), (7, 1)],
            &["1.", "a)", "b)", "e)", "f)", "g)", "2.", "a)"],
        ),
        // 이것을 실패시키는 것: 깊은 사용이 상위의 대기 override를 소비.
        case(
            "deeper_use_does_not_consume_parent_override",
            &format!("{basic}{}", level(2, Some(1), "lowerRoman", "%3.", "")),
            "",
            &reset(1, 5),
            &[(7, 0), (7, 1), (8, 2), (7, 1), (8, 1)],
            &["1.", "a)", "i.", "b)", "e)"],
        ),
        // 이것을 실패시키는 것: 하위 사용이 최상위 override를 소비.
        case(
            "child_use_does_not_consume_top_override",
            &basic,
            "",
            &reset(0, 5),
            &[(7, 0), (8, 1), (7, 0), (8, 0)],
            &["1.", "a)", "2.", "5."],
        ),
        // 이것을 실패시키는 것: 하위 override를 명시적 재등장에서 다시 적용.
        case(
            "explicit_child_use_consumes_its_override",
            &basic,
            "",
            &reset(1, 5),
            &[(7, 0), (7, 1), (8, 1), (7, 1), (8, 1)],
            &["1.", "a)", "e)", "f)", "g)"],
        ),
        // 이것을 실패시키는 것: 건너뛴 중간 수준 used를 누락.
        case(
            "skipped_middle_level_is_consumed",
            &format!(
                "{decimal}{}{}",
                level(1, Some(1), "lowerLetter", "%2.", ""),
                level(2, Some(1), "decimal", "%3)", "")
            ),
            "",
            "",
            &[(7, 0), (7, 1), (7, 0), (7, 2), (7, 1)],
            &["1.", "a.", "2.", "1)", "b."],
        ),
        // 이것을 실패시키는 것: 평범한 연속 번호를 매번 재시작.
        case("ordinary_counter_control", &decimal, "", "", &[(7, 0); 3], &["1.", "2.", "3."]),
    ];
    cases.push(case(
        "format_override_can_be_the_first_instance",
        &decimal,
        "",
        &format!(
            "<w:lvlOverride w:ilvl='0'>{}</w:lvlOverride>",
            level(0, Some(1), "decimal", "%1.", "<w:rPr><w:b/></w:rPr>")
        ),
        &[(8, 0), (8, 0), (7, 0), (7, 0), (8, 0)],
        &["1.", "2.", "3.", "4.", "5."],
    ));
    let mut independent = case(
        "interleaved_lists_keep_independent_counters",
        &decimal,
        "",
        "",
        &[(7, 0), (7, 0), (9, 0), (7, 0)],
        &["1.", "2.", "a)", "3."],
    );
    independent.definitions += &format!(
        "{}{}",
        definition(2, &level(0, Some(1), "lowerLetter", "%1)", "")),
        num(9, 2, "")
    );
    cases.push(independent);
    for (name, uses, expected) in [
        (
            "linked_abstracts_keep_separate_counters",
            &[(7, 0), (7, 0), (8, 0), (8, 0), (7, 0)][..],
            &["1.", "2.", "1.", "2.", "3."][..],
        ),
        (
            "linked_counter_order_is_independent",
            &[(7, 0), (7, 0), (8, 0), (8, 0), (8, 0), (7, 0)][..],
            &["1.", "2.", "1.", "2.", "3.", "3."][..],
        ),
        (
            "linked_counter_reverse_order_is_independent",
            &[(8, 0), (8, 0), (7, 0), (7, 0), (7, 0), (8, 0)][..],
            &["1.", "2.", "1.", "2.", "3.", "3."][..],
        ),
    ] {
        cases.push(Case { name, definitions: format!("{}{}{}{}",definition(1,&decimal),definition(2,"<w:numStyleLink w:val='linked'/>"),num(7,1,""),num(8,2,"")), uses:uses.to_vec(),expected,
            styles:"<w:style w:type='numbering' w:styleId='linked'><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr></w:pPr></w:style>" });
    }
    cases
}
