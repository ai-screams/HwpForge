use super::*;

// 이것을 실패시키는 것: 실패 결과를 캐시하지 않음, num마다 원본 문자열 재검사, 예산 경계 변경.
#[test]
fn marker_interpretation_caches_failures_and_successes() {
    for suffix in ["%1.", "%1.%2."] {
        let mut defs = Definitions::default();
        let text = format!("{}{suffix}", "x".repeat(524288));
        let size = text.len();
        let level = || Level {
            defined: true,
            format: Format::Number(NumberFormatType::Digit),
            text: Some(Rc::from(text.as_str())),
            ..Default::default()
        };
        defs.abstracts.insert(1, std::collections::BTreeMap::from([(0, level())]));
        for id in 1..=1024 {
            defs.numbers.insert(
                id,
                definitions::Number {
                    abstract_id: Some(1),
                    levels: if id == 1024 {
                        std::collections::BTreeMap::from([(
                            0,
                            Level { start_override: Some(5), ..level() },
                        )])
                    } else {
                        std::collections::BTreeMap::from([(
                            0,
                            Level { start_override: Some(5), ..Default::default() },
                        )])
                    },
                },
            );
        }
        let mut lists = Lists::new(&defs).unwrap();
        assert_eq!(lists.interpreted_bytes, size * 2);
        assert_eq!(lists.groups.len(), 1);
        let base = &lists.instances[&1].levels[0].as_ref().unwrap().compiled;
        let other = &lists.instances[&2].levels[0].as_ref().unwrap().compiled;
        assert!(Rc::ptr_eq(base, other));
        assert_eq!(matches!(base.marker, Marker::Lost), suffix == "%1.%2.");
        let mut registry = StyleRegistry::with_fonts(vec![]);
        let para = ParaDelta { num_id: Some(1), ..Default::default() };
        for _ in 0..100 {
            let result = lists.resolve(&defs, &mut registry, &para, "body", &mut vec![]).unwrap();
            assert_eq!(result, if suffix == "%1." { Some(ListRef::Number(0, 0)) } else { None });
        }
        assert_eq!(lists.interpreted_bytes, size * 2);
        let source = level();
        let mut bytes = 64 * 1024 * 1024 - size;
        compile(&source, 0, &defs.fonts, &mut bytes, "numbering.xml").unwrap();
        assert_eq!(bytes, 64 * 1024 * 1024);
        compile(&source, 0, &defs.fonts, &mut bytes, "numbering.xml").unwrap();
        assert_eq!(bytes, 64 * 1024 * 1024);
        assert!(matches!(
            compile(&level(), 0, &defs.fonts, &mut bytes, "numbering.xml"),
            Err(DocxError::Limit { resource: "interpreted marker bytes", .. })
        ));
    }
}

fn materialization_fixture(extra: usize, count: u32, size: usize) -> Definitions {
    let mut defs = Definitions::default();
    let level = |index, extra| Level {
        defined: true,
        start: Some(1),
        format: Format::Number(NumberFormatType::Digit),
        text: Some(Rc::from(format!("{}%{}", "x".repeat(size - 2 + extra), index + 1))),
        ..Default::default()
    };
    defs.abstracts
        .insert(1, std::collections::BTreeMap::from([(0, level(0, 0)), (1, level(1, 0))]));
    for id in 1..=count {
        defs.numbers.insert(
            id,
            definitions::Number {
                abstract_id: Some(1),
                levels: if id == count && extra != 0 {
                    std::collections::BTreeMap::from([
                        (0, Level { start_override: Some(1), ..Default::default() }),
                        (1, level(1, extra)),
                    ])
                } else {
                    std::collections::BTreeMap::from([(
                        0,
                        Level { start_override: Some(1), ..Default::default() },
                    )])
                },
            },
        );
    }
    defs
}

// 이것을 실패시키는 것: 64 MiB 경계를 >=로 거부, 같은 num 재출력에 중복 과금, 한 수준만 물질화.
#[test]
fn materialized_marker_bytes_exact_limit() {
    let defs = materialization_fixture(0, 128, 262144);
    let mut lists = Lists::new(&defs).unwrap();
    let mut registry = StyleRegistry::with_fonts(vec![]);
    for id in 1..=128 {
        let para = ParaDelta { num_id: Some(id), ..Default::default() };
        for _ in 0..2 {
            let result = lists.resolve(&defs, &mut registry, &para, "body", &mut vec![]);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), Some(ListRef::Number(id as usize - 1, 0)));
        }
    }
    assert_eq!(lists.owned_marker_bytes, 67108864);
    assert_eq!(lists.interpreted_bytes, 524288);
    assert_eq!(registry.numberings.len(), 128);
    assert_eq!(
        registry.numberings.iter().flat_map(|n| &n.levels).map(|l| l.text.len()).sum::<usize>(),
        67108864
    );
    assert_eq!(registry.numberings[127].levels[1].text.len(), 262144);
    assert!(registry.numberings[127].levels[1].text.ends_with("^2"));
}

// 이것을 실패시키는 것: 그룹별 예산 초기화, 누적/하위 수준 바이트 누락, 복사 뒤 검사, 한도 1바이트 완화.
#[test]
fn materialized_marker_bytes_one_over_limit() {
    let defs = materialization_fixture(1, 128, 262144);
    let mut lists = Lists::new(&defs).unwrap();
    let mut registry = StyleRegistry::with_fonts(vec![]);
    for id in 1..=127 {
        let para = ParaDelta { num_id: Some(id), ..Default::default() };
        assert_eq!(
            lists.resolve(&defs, &mut registry, &para, "body", &mut vec![]).unwrap(),
            Some(ListRef::Number(id as usize - 1, 0))
        );
    }
    let para = ParaDelta { num_id: Some(128), ..Default::default() };
    assert!(matches!(
        lists.resolve(&defs, &mut registry, &para, "body", &mut vec![]),
        Err(DocxError::Limit {
            resource: "materialized marker bytes",
            observed: 67108865,
            limit: 67108864,
            ..
        })
    ));
    assert_eq!(lists.owned_marker_bytes, 66584576);
    assert_eq!(registry.numberings.len(), 127);
    assert_eq!(
        registry.numberings.iter().flat_map(|n| &n.levels).map(|l| l.text.len()).sum::<usize>(),
        66584576
    );
}

// 이것을 실패시키는 것: 출력 정의 상한에서 전체 변환 실패 또는 마지막 정상 정의 재사용에도 개수 과금.
#[test]
fn output_numbering_definition_limit() {
    let defs = materialization_fixture(0, 16385, 2);
    let mut lists = Lists::new(&defs).unwrap();
    let mut registry = StyleRegistry::with_fonts(vec![]);
    for id in 1..=16384 {
        let para = ParaDelta { num_id: Some(id), ..Default::default() };
        assert_eq!(
            lists.resolve(&defs, &mut registry, &para, "body", &mut vec![]).unwrap(),
            Some(ListRef::Number(id as usize - 1, 0))
        );
    }
    let para = ParaDelta { num_id: Some(16384), ..Default::default() };
    assert_eq!(
        lists.resolve(&defs, &mut registry, &para, "body", &mut vec![]).unwrap(),
        Some(ListRef::Number(16383, 0))
    );
    let para = ParaDelta { num_id: Some(16385), ..Default::default() };
    let mut warnings = vec![];
    assert_eq!(
        lists.resolve(&defs, &mut registry, &para, "body", &mut warnings).unwrap(),
        Some(ListRef::Number(16383, 0))
    );
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.loss() == "list_numbering_budget")
            .map(|w| w.count())
            .sum::<u64>(),
        1
    );
    assert_eq!(registry.numberings.len(), 16384);
}

#[path = "../../../hwpforge-smithy-hwpx/tests/support/list_counter_cases.rs"]
mod list_counter_cases;
// 같은 입력 표를 독립 시험 모듈에서 각각 포함한다.
#[allow(clippy::duplicate_mod)]
#[path = "../../tests/support/word_counter_cases.rs"]
mod word_counter_cases;

// 이것을 실패시키는 것: 리터럴 0이 활성 정상 출력 카운터를 진행.
#[test]
fn literal_zero_does_not_advance_the_active_segment() {
    let mut defs = materialization_fixture(0, 2, 2);
    defs.numbers.get_mut(&2).unwrap().levels.get_mut(&0).unwrap().start_override = Some(0);
    let mut lists = Lists::new(&defs).unwrap();
    let mut registry = StyleRegistry::with_fonts(vec![]);
    for num in [1, 2] {
        lists
            .resolve(
                &defs,
                &mut registry,
                &ParaDelta { num_id: Some(num), ..Default::default() },
                "body",
                &mut vec![],
            )
            .unwrap();
    }
    let group = &lists.groups[&1].output;
    assert_eq!(lists.groups[&1].counter.values[0], 0);
    assert_eq!(group.active.as_ref().unwrap().counter.values[0], 1);
    assert_eq!(group.active.as_ref().unwrap().id, 0);
    assert_eq!(registry.numberings[1].levels[0].text, "0");
}
// 이것을 실패시키는 것: 캐시된 리터럴의 바이트를 다시 과금.
#[test]
fn cached_literal_zero_does_not_charge_marker_bytes_again() {
    let mut defs = materialization_fixture(0, 3, 2);
    for num in [2, 3] {
        defs.numbers.get_mut(&num).unwrap().levels.get_mut(&0).unwrap().start_override = Some(0);
    }
    let mut lists = Lists::new(&defs).unwrap();
    let mut registry = StyleRegistry::with_fonts(vec![]);
    let mut warnings = vec![];
    for num in [1, 2] {
        lists
            .resolve(
                &defs,
                &mut registry,
                &ParaDelta { num_id: Some(num), ..Default::default() },
                "body",
                &mut warnings,
            )
            .unwrap();
    }
    let before = lists.owned_marker_bytes;
    let result = lists
        .resolve(
            &defs,
            &mut registry,
            &ParaDelta { num_id: Some(3), ..Default::default() },
            "body",
            &mut warnings,
        )
        .unwrap();
    assert_eq!(result, Some(ListRef::Number(1, 0)));
    assert_eq!(lists.owned_marker_bytes, before);
    assert_eq!(registry.numberings.len(), 2);
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.loss() == "list_numbering_split")
            .map(|w| w.count())
            .sum::<u64>(),
        1
    );
}
fn two_groups() -> Definitions {
    let mut defs = materialization_fixture(0, 2, 2);
    defs.abstracts.insert(
        2,
        std::collections::BTreeMap::from([(
            0,
            Level {
                defined: true,
                start: Some(1),
                format: Format::Number(NumberFormatType::Digit),
                text: Some(Rc::from("%1.")),
                ..Default::default()
            },
        )]),
    );
    for num in [3, 4] {
        defs.numbers.insert(
            num,
            definitions::Number {
                abstract_id: Some(2),
                levels: std::collections::BTreeMap::from([(
                    0,
                    Level { start_override: Some(1), ..Default::default() },
                )]),
            },
        );
    }
    defs
}
// 이것을 실패시키는 것: 앞 그룹의 사본이 아직 출력하지 않은 그룹의 첫 정의 공간을 소진.
#[test]
fn first_definition_space_is_reserved_for_unused_groups() {
    let defs = two_groups();
    let mut lists = Lists::new(&defs).unwrap();
    let mut registry = StyleRegistry::with_fonts(vec![]);
    let mut warnings = vec![];
    lists
        .resolve(
            &defs,
            &mut registry,
            &ParaDelta { num_id: Some(1), ..Default::default() },
            "body",
            &mut warnings,
        )
        .unwrap();
    registry.numberings.resize(16_383, registry.numberings[0].clone());
    assert_eq!(
        lists
            .resolve(
                &defs,
                &mut registry,
                &ParaDelta { num_id: Some(2), ..Default::default() },
                "body",
                &mut warnings
            )
            .unwrap(),
        Some(ListRef::Number(0, 0))
    );
    assert_eq!(registry.numberings.len(), 16_383);
    assert_eq!(
        lists
            .resolve(
                &defs,
                &mut registry,
                &ParaDelta { num_id: Some(3), ..Default::default() },
                "body",
                &mut warnings
            )
            .unwrap(),
        Some(ListRef::Number(16_383, 0))
    );
    assert_eq!(registry.numberings.len(), 16_384);
}
// 이것을 실패시키는 것: budget 경고를 영향 문단마다 집계하거나 다른 그룹의 경고를 억제.
#[test]
fn budget_warning_counts_groups_not_paragraphs() {
    let defs = two_groups();
    let mut lists = Lists::new(&defs).unwrap();
    let mut registry = StyleRegistry::with_fonts(vec![]);
    let mut warnings = vec![];
    for num in [1, 3] {
        lists
            .resolve(
                &defs,
                &mut registry,
                &ParaDelta { num_id: Some(num), ..Default::default() },
                "body",
                &mut warnings,
            )
            .unwrap();
    }
    let original = registry.numberings[..2].to_vec();
    registry.numberings.resize(16_384, registry.numberings[0].clone());
    for num in [2, 1, 1] {
        lists
            .resolve(
                &defs,
                &mut registry,
                &ParaDelta { num_id: Some(num), ..Default::default() },
                "body",
                &mut warnings,
            )
            .unwrap();
    }
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.loss() == "list_numbering_budget")
            .map(|w| w.count())
            .sum::<u64>(),
        1
    );
    lists
        .resolve(
            &defs,
            &mut registry,
            &ParaDelta { num_id: Some(4), ..Default::default() },
            "body",
            &mut warnings,
        )
        .unwrap();
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.loss() == "list_numbering_budget")
            .map(|w| w.count())
            .sum::<u64>(),
        2
    );
    assert_eq!(registry.numberings[..2], original);
    assert_eq!(registry.numberings.len(), 16_384);
}
// 이것을 실패시키는 것: Word 값 증가를 wrapping으로 처리하거나 출력 start를 u32로 절삭.
#[test]
fn counter_arithmetic_keeps_checked_boundaries() {
    let case = word_counter_cases::counter_cases()
        .into_iter()
        .find(|c| c.name == "ordinary_counter_control")
        .unwrap();
    let mut state = CounterState::new([1; 10]);
    let mut rendered = vec![];
    for (_, level) in case.uses {
        state.advance_word(level as usize, None, &[None; 10], "body").unwrap();
        rendered.push(format!("{}.", state.values[0]));
    }
    assert_eq!(rendered, case.expected);
    state.values[0] = u64::MAX;
    assert!(state.advance_word(0, None, &[None; 10], "body").is_err());
    assert!(state.advance_core(0, "body").is_err());
    state.values[0] = u64::from(u32::MAX) + 1;
    assert!(counter::CounterState::segment_starts(state.values, state.starts, 0, "body").is_err());
}
// 이것을 실패시키는 것: 출력 카운터의 암묵 상위 소비나 하위 재시작을 Word 규칙과 함께 변경.
#[test]
fn core_counter_matches_literal_observations() {
    for case in list_counter_cases::CASES {
        let Some(expected) = case.expected else {
            continue;
        };
        let mut states: Vec<_> = case
            .definitions
            .iter()
            .map(|def| {
                let mut starts = [1; 10];
                for (i, &start) in def.starts.iter().enumerate() {
                    starts[i] = start;
                }
                CounterState::new(starts)
            })
            .collect();
        let mut output = vec![];
        for &(definition, level) in case.uses {
            if definition == 0 {
                continue;
            }
            let def = &case.definitions[definition - 1];
            let state = &mut states[definition - 1];
            state.advance_core(level, "body").unwrap();
            let mut text = def.texts[level].to_owned();
            for i in (0..def.starts.len()).rev() {
                let n = state.values[i] as usize;
                let value = match def.formats[i] {
                    "RomanCapital" => ["", "I", "II", "III", "IV"][n].to_owned(),
                    "HangulSyllable" => ["", "가", "나", "다", "라"][n].to_owned(),
                    _ => n.to_string(),
                };
                text = text.replace(&format!("^{}", i + 1), &value);
            }
            output.push(text);
        }
        assert_eq!(output, expected, "{}", case.name);
    }
}

// 이것을 실패시키는 것: 정상 정의가 없는 상한 fallback에서 추가 정의를 만들거나 변환을 실패.
#[test]
fn literal_only_budget_fallback_keeps_existing_definition() {
    let mut defs = materialization_fixture(0, 1, 2);
    defs.numbers.get_mut(&1).unwrap().levels.get_mut(&0).unwrap().start_override = Some(0);
    let mut lists = Lists::new(&defs).unwrap();
    let mut registry = StyleRegistry::with_fonts(vec![]);
    let mut warnings = vec![];
    let para = ParaDelta { num_id: Some(1), ..Default::default() };
    lists.resolve(&defs, &mut registry, &para, "body", &mut warnings).unwrap();
    let original = registry.numberings[0].clone();
    registry.numberings.resize(16_384, original.clone());
    assert_eq!(
        lists.resolve(&defs, &mut registry, &para, "body", &mut warnings).unwrap(),
        Some(ListRef::Number(0, 0))
    );
    assert_eq!(registry.numberings.len(), 16_384);
    assert_eq!(registry.numberings[0], original);
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.loss() == "list_numbering_budget")
            .map(|w| w.count())
            .sum::<u64>(),
        1
    );
}
// 이것을 실패시키는 것: 추가 분리 중지 뒤 캐시된 정확한 리터럴까지 정상 정의로 대체.
#[test]
fn budget_fallback_still_reuses_cached_literals() {
    let mut defs = materialization_fixture(0, 3, 2);
    for id in [2, 3] {
        defs.numbers.get_mut(&id).unwrap().levels.get_mut(&0).unwrap().start_override = Some(0);
    }
    let mut lists = Lists::new(&defs).unwrap();
    let mut registry = StyleRegistry::with_fonts(vec![]);
    let mut warnings = vec![];
    for id in [1, 2] {
        lists
            .resolve(
                &defs,
                &mut registry,
                &ParaDelta { num_id: Some(id), ..Default::default() },
                "body",
                &mut warnings,
            )
            .unwrap();
    }
    registry.numberings.resize(16_384, registry.numberings[0].clone());
    lists
        .resolve(
            &defs,
            &mut registry,
            &ParaDelta { num_id: Some(1), ..Default::default() },
            "body",
            &mut warnings,
        )
        .unwrap();
    assert!(lists.groups[&1].output.stopped);
    assert_eq!(
        lists
            .resolve(
                &defs,
                &mut registry,
                &ParaDelta { num_id: Some(3), ..Default::default() },
                "body",
                &mut warnings
            )
            .unwrap(),
        Some(ListRef::Number(1, 0))
    );
    assert_eq!(registry.numberings.len(), 16_384);
}
fn legal_definitions() -> Definitions {
    let mut defs = materialization_fixture(0, 2, 2);
    let parent = defs.abstracts.get_mut(&1).unwrap().get_mut(&0).unwrap();
    parent.format = Format::Number(NumberFormatType::RomanCapital);
    parent.text = Some(Rc::from("%1."));
    let child = defs.abstracts.get_mut(&1).unwrap().get_mut(&1).unwrap();
    child.text = Some(Rc::from("%1.%2"));
    child.legal = true;
    defs.numbers.get_mut(&1).unwrap().levels.clear();
    defs
}
fn budget_resolve(
    lists: &mut Lists,
    defs: &Definitions,
    registry: &mut StyleRegistry,
    warnings: &mut Vec<DocxWarning>,
    num: u32,
    level: u32,
    part: &str,
) -> Option<ListRef> {
    lists
        .resolve(
            defs,
            registry,
            &ParaDelta { num_id: Some(num), level: Some(level), ..Default::default() },
            part,
            warnings,
        )
        .unwrap()
}
fn warning_count(warnings: &[DocxWarning], loss: &str) -> u64 {
    warnings.iter().filter(|w| w.loss() == loss).map(|w| w.count()).sum()
}
// 이것을 실패시키는 것: 상한에서 로마 활성 정의 변경 또는 실제 legal fallback 경고 누락.
#[test]
fn legal_budget_fallback_keeps_original_active_definition() {
    let defs = legal_definitions();
    let mut lists = Lists::new(&defs).unwrap();
    let mut registry = StyleRegistry::with_fonts(vec![]);
    let mut warnings = vec![];
    assert_eq!(
        budget_resolve(&mut lists, &defs, &mut registry, &mut warnings, 1, 0, "parent"),
        Some(ListRef::Number(0, 0))
    );
    let original = registry.numberings[0].clone();
    registry.numberings.resize(16_384, original.clone());
    for part in ["first-fallback", "later-fallback"] {
        assert_eq!(
            budget_resolve(&mut lists, &defs, &mut registry, &mut warnings, 1, 1, part),
            Some(ListRef::Number(0, 1))
        );
    }
    assert_eq!(registry.numberings[0], original);
    assert_eq!(registry.numberings.len(), 16_384);
    assert_eq!(warning_count(&warnings, "list_numbering_budget"), 1);
    assert_eq!(warning_count(&warnings, "list_number_legal"), 1);
    assert_eq!(
        warnings.iter().find(|w| w.loss() == "list_number_legal").unwrap().part(),
        Some("first-fallback")
    );
}
// 이것을 실패시키는 것: 상한에서 십진 사본을 로마로 사후 변경하거나 과거 legal 문단에 소급 경고.
#[test]
fn budget_fallback_does_not_rewrite_decimal_copies() {
    for parent_first in [false, true] {
        let defs = legal_definitions();
        let mut lists = Lists::new(&defs).unwrap();
        let mut registry = StyleRegistry::with_fonts(vec![]);
        let mut warnings = vec![];
        if parent_first {
            budget_resolve(&mut lists, &defs, &mut registry, &mut warnings, 1, 0, "parent");
        }
        let reference =
            budget_resolve(&mut lists, &defs, &mut registry, &mut warnings, 1, 1, "child");
        let id = usize::from(parent_first);
        assert_eq!(reference, Some(ListRef::Number(id, 1)));
        let original = registry.numberings.clone();
        registry.numberings.resize(16_384, original[0].clone());
        assert_eq!(
            budget_resolve(
                &mut lists,
                &defs,
                &mut registry,
                &mut warnings,
                1,
                0,
                "returning-parent"
            ),
            Some(ListRef::Number(id, 0))
        );
        assert_eq!(&registry.numberings[..original.len()], original.as_slice());
        assert_eq!(registry.numberings[id].levels[0].num_format, NumberFormatType::Digit);
        assert_eq!(warning_count(&warnings, "list_numbering_budget"), 1);
        assert_eq!(warning_count(&warnings, "list_number_legal"), 0);
    }
}
// 이것을 실패시키는 것: 십진 형식이 유지된 값 근사만으로 legal 경고 발생.
#[test]
fn legal_value_budget_fallback_keeps_decimal_format() {
    let mut defs = legal_definitions();
    defs.numbers
        .get_mut(&2)
        .unwrap()
        .levels
        .insert(1, Level { start_override: Some(5), ..Default::default() });
    let mut lists = Lists::new(&defs).unwrap();
    let mut registry = StyleRegistry::with_fonts(vec![]);
    let mut warnings = vec![];
    budget_resolve(&mut lists, &defs, &mut registry, &mut warnings, 1, 1, "first-child");
    let original = registry.numberings[0].clone();
    registry.numberings.resize(16_384, original.clone());
    assert_eq!(
        budget_resolve(&mut lists, &defs, &mut registry, &mut warnings, 2, 1, "reset-child"),
        Some(ListRef::Number(0, 1))
    );
    assert_eq!(registry.numberings[0], original);
    assert_eq!(registry.numberings[0].levels[0].num_format, NumberFormatType::Digit);
    assert_eq!(warning_count(&warnings, "list_numbering_budget"), 1);
    assert_eq!(warning_count(&warnings, "list_number_legal"), 0);
}

// 이것을 실패시키는 것: 표시 근거가 없는 자기 수준 토큰 반복을 수용.
#[test]
fn repeated_own_marker_token_is_rejected() {
    assert!(template("%1.%1", 0).is_none());
    assert!(template("%1.%2.%2", 1).is_none());
    assert_eq!(template("%1.%2", 1), Some(("^1.^2".into(), 3)));
}

// 이것을 실패시키는 것: 배정할 때 현재 원본 값이나 이미 소비한 하위 override를 다시 조회.
#[test]
fn observed_numbering_keeps_values_and_pending_child_starts() {
    let mut defs = materialization_fixture(0, 1, 2);
    defs.numbers
        .get_mut(&1)
        .unwrap()
        .levels
        .insert(1, Level { start_override: Some(5), ..Default::default() });
    let mut lists = Lists::new(&defs).unwrap();
    let mut registry = StyleRegistry::with_fonts(vec![]);
    let mut warnings = vec![];
    let mut requests = vec![];
    for level in [0, 1, 0] {
        let observed = lists
            .observe(
                &defs,
                &mut registry,
                &ParaDelta { num_id: Some(1), level: Some(level), ..Default::default() },
                "body",
                &mut warnings,
            )
            .unwrap();
        let Some(ObservedList::Number(request)) = observed else { panic!("number request") };
        requests.push(request);
    }
    assert!(registry.numberings.is_empty());
    for request in &requests {
        lists.assign_numbering(&mut registry, request, "body", &mut warnings).unwrap();
    }
    assert_eq!(registry.numberings.len(), 1);
    assert_eq!(registry.numberings[0].levels[0].start, 1);
    assert_eq!(registry.numberings[0].levels[1].start, 5);
    assert_eq!(lists.groups[&1].output.active.as_ref().unwrap().counter.values[..2], [2, 5]);
}
