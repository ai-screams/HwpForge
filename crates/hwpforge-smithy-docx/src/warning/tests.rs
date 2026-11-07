use super::*;

// 이것을 실패시키는 것: 키에 위치/사유 추가, 첫 None 갱신, loss/action 구분 제거,
// checked_add 제거, 반복 메시지/순서/count 변경.
#[test]
fn aggregation_keeps_first_context_and_checked_counts() {
    let mut warnings = Vec::new();
    for i in 0..20_000 {
        record(&mut warnings, Kind::FormattingOmitted, "character_format", "use_defaults", || {
            (Some("body.xml"), Some(format!("r[{i}]")), format!("format {i}"))
        })
        .unwrap();
    }
    assert_eq!(warnings.len(), 1);
    let first = &warnings[0];
    assert_eq!(
        (first.code(), first.loss(), first.action(), first.count(), first.part(), first.location()),
        (
            "DOCX_FORMATTING_OMITTED",
            "character_format",
            "use_defaults",
            20_000,
            Some("body.xml"),
            Some("r[0]")
        )
    );
    assert_eq!(
        first.message(),
        "format 0 (total 20000 occurrences; first example only; subsequent locations omitted)"
    );
    for (loss, action) in [("other", "use_defaults"), ("character_format", "omit")] {
        record(&mut warnings, Kind::FormattingOmitted, loss, action, || {
            (None, None::<&str>, "first")
        })
        .unwrap();
        record(&mut warnings, Kind::FormattingOmitted, loss, action, || {
            (Some("later"), Some("later"), "later")
        })
        .unwrap();
    }
    assert_eq!(warnings.len(), 3);
    assert_eq!(warnings[1].part(), None);
    assert_eq!(warnings[1].location(), None);
    assert_eq!(warnings[1].count(), 2);
    assert_eq!(warnings[2].action(), "omit");
    *warnings[0].count_mut() = u64::MAX - 1;
    let next = (None, None::<&str>, "x");
    record(&mut warnings, Kind::FormattingOmitted, "character_format", "use_defaults", || next)
        .unwrap();
    assert_eq!(warnings[0].count(), u64::MAX);
    assert!(
        matches!(record(&mut warnings, Kind::FormattingOmitted, "character_format", "use_defaults", || next), Err(DocxError::Invalid { detail, .. }) if detail == "warning count overflow (u64)")
    );
    assert_eq!(warnings[0].count(), u64::MAX);
}

// 이것을 실패시키는 것: 첫 경고의 part/location/사유를 인용 제한 없이 보관.
#[test]
fn first_diagnostic_strings_are_bounded() {
    let long = "가".repeat(1_000);
    let warning =
        Kind::ContentOmitted.at(Some(&long), Some(&long), &long, "unknown_element", "omit");
    for value in [warning.part().unwrap(), warning.location().unwrap(), &warning.message()] {
        assert_eq!(value.chars().count(), 513);
        assert!(value.ends_with('…'));
    }
}

// 이것을 실패시키는 것: 기존 키에서도 example 클로저를 실행하거나 인용 문자열 생성.
#[test]
fn existing_group_builds_zero_diagnostic_bytes() {
    let mut warnings = Vec::new();
    let long = "가".repeat(4096);
    take_diagnostic_bytes();
    record(&mut warnings, Kind::ContentOmitted, "unknown_element", "omit", || {
        (Some(long.as_str()), Some(&long), &long)
    })
    .unwrap();
    assert_eq!(take_diagnostic_bytes(), 4617);
    for _ in 0..128 {
        record(
            &mut warnings,
            Kind::ContentOmitted,
            "unknown_element",
            "omit",
            || -> (Option<&str>, Option<&str>, &str) {
                panic!("an existing warning must not evaluate its diagnostic")
            },
        )
        .unwrap();
    }
    assert_eq!(take_diagnostic_bytes(), 0);
    assert_eq!(warnings[0].count(), 129);
}

// 이것을 실패시키는 것: record가 저장할 kind/loss/action을 집계 키와 다르게 조립,
// 튜플의 part/location/reason 전달 누락 또는 뒤바꿈.
#[test]
fn record_constructs_the_example_from_its_aggregation_key() {
    let mut warnings = Vec::new();
    for _ in 0..2 {
        for (kind, loss, action) in [
            (Kind::ContentOmitted, "header", "omit"),
            (Kind::FormattingOmitted, "header", "omit"),
            (Kind::ContentOmitted, "footer", "omit"),
            (Kind::ContentOmitted, "header", "keep_text"),
        ] {
            record(&mut warnings, kind, loss, action, || {
                (Some("part.xml"), Some("hdr#1"), "example")
            })
            .unwrap();
        }
    }
    assert_eq!(
        warnings.iter().map(|w| (w.code(), w.loss(), w.action(), w.count())).collect::<Vec<_>>(),
        [
            ("DOCX_CONTENT_OMITTED", "header", "omit", 2),
            ("DOCX_FORMATTING_OMITTED", "header", "omit", 2),
            ("DOCX_CONTENT_OMITTED", "footer", "omit", 2),
            ("DOCX_CONTENT_OMITTED", "header", "keep_text", 2),
        ]
    );
    for warning in warnings {
        assert_eq!(warning.part(), Some("part.xml"));
        assert_eq!(warning.location(), Some("hdr#1"));
        assert_eq!(
            warning.message(),
            "example (total 2 occurrences; first example only; subsequent locations omitted)"
        );
    }
}
