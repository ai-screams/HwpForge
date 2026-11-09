use super::*;

// 이것을 실패시키는 것: 실패 결과를 캐시하지 않음, override 그룹마다 원본 문자열 재검사, 예산 경계 변경.
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
                    has_override: id > 1,
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
        assert_eq!(lists.groups.len(), 1024);
        let base = &lists.groups[&GroupKey::Abstract(1)].levels[0].as_ref().unwrap().compiled;
        let other = &lists.groups[&GroupKey::Instance(2)].levels[0].as_ref().unwrap().compiled;
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
                has_override: true,
                levels: if id == count && extra != 0 {
                    std::collections::BTreeMap::from([(1, level(1, extra))])
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

// 이것을 실패시키는 것: 64 MiB 경계를 >=로 거부, 같은 Instance 재출력에 중복 과금, 한 수준만 물질화.
#[test]
fn materialized_marker_bytes_exact_limit() {
    let defs = materialization_fixture(0, 128, 262144);
    let mut lists = Lists::new(&defs).unwrap();
    let mut registry = StyleRegistry::with_fonts(vec![]);
    for id in 1..=128 {
        let para = ParaDelta { num_id: Some(id), ..Default::default() };
        for _ in 0..2 {
            assert_eq!(
                lists.resolve(&defs, &mut registry, &para, "body", &mut vec![]).unwrap(),
                Some(ListRef::Number(id as usize - 1, 0))
            );
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

// 이것을 실패시키는 것: 출력 정의 개수 검사 제거, 경계 >=, 마지막 정상 정의 재사용에도 개수 과금.
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
    assert!(matches!(
        lists.resolve(&defs, &mut registry, &para, "body", &mut vec![]),
        Err(DocxError::Limit {
            resource: "output numbering definitions",
            observed: 16385,
            limit: 16384,
            ..
        })
    ));
    assert_eq!(registry.numberings.len(), 16384);
}
