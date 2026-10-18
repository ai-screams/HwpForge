//! W1 branch and wire-contract regression tests.
use super::header::parse_header;
use crate::{HwpxParaShape, HwpxStyleStore};
use hwpforge_core::{CharCount, ParagraphMeasure};
use hwpforge_foundation::{HwpUnit, LineSpacingType, ParaShapeIndex};

const NS: &str = "http://www.hancom.co.kr/hwpml/2016/HwpUnitChar";
fn margin(value: i32, unit: &str) -> String {
    format!(
        "<margin>{}</margin>",
        ["left", "right", "intent", "prev", "next"]
            .map(|f| format!(r#"<{f} value="{value}" unit="{unit}"/>"#))
            .join("")
    )
}
fn head(payload: &str) -> String {
    format!(
        r#"<head><refList><paraProperties><paraPr id="7">{payload}</paraPr></paraProperties></refList></head>"#
    )
}
fn branch(payload: &str) -> String {
    format!(r#"<case required-namespace="{NS}">{payload}</case>"#)
}
fn para(xml: &str) -> HwpxParaShape {
    parse_header(&head(xml))
        .unwrap()
        .style_store
        .para_shape(ParaShapeIndex::new(0))
        .unwrap()
        .clone()
}

#[test]
fn w1_case_precedes_mismatched_default_and_keeps_char_zero() {
    // 이것을 실패시키는 것: 변이 2·3·4·14 — CHAR 제거, default 우선, 단위 없는 0.
    for value in [-750, 0, 1000] {
        let xml = format!(
            "<switch>{}<default>{}</default></switch>",
            branch(&margin(value, "CHAR")),
            margin(4000, "HWPUNIT")
        );
        let parsed = parse_header(&head(&xml)).unwrap();
        let p = parsed.style_store.para_shape(ParaShapeIndex::new(0)).unwrap();
        assert_eq!(p.margin_left, ParagraphMeasure::from_char_count(CharCount::from_ticks(value)));
        assert_ne!(p.margin_left, ParagraphMeasure::from_hwp_unit(HwpUnit::new(value).unwrap()));
        assert_eq!(parsed.warnings.len(), 5);
        assert!(parsed
            .warnings
            .iter()
            .all(|w| !w.is_content_loss() && w.code() == "PARAGRAPH_MEASURE_NORMALIZED"));
    }
}
#[test]
fn w1_direct_and_default_negative_odd_keep_arithmetic_shift() {
    // 이것을 실패시키는 것: 변이 1·5 — /2 절삭, direct를 0으로 반환.
    for raw in [-1499, -2000, 1, 2001, 3000] {
        let direct = para(&margin(raw, "HWPUNIT"));
        let default =
            para(&format!("<switch><default>{}</default></switch>", margin(raw, "HWPUNIT")));
        assert_eq!(direct.margin_left, crate::paragraph_measure::unpack_margin(raw).unwrap());
        assert_eq!(direct.margin_left, default.margin_left);
    }
}
#[test]
fn w1_namespace_selection_and_invalid_payloads() {
    // 이것을 실패시키는 것: 변이 13 — unknown-first를 선택, 접두사 없는 속성 무시.
    let unknown =
        format!(r#"<case required-namespace="urn:unknown">{}</case>"#, margin(777, "HWPUNIT"));
    let supported = branch(&margin(1000, "CHAR"));
    assert_eq!(
        para(&format!("<switch>{unknown}{supported}</switch>"))
            .margin_left
            .as_char_count()
            .unwrap()
            .ticks(),
        1000
    );
    assert_eq!(
        para(&format!("<switch>{unknown}<default>{}</default></switch>", margin(2001, "HWPUNIT")))
            .margin_left
            .as_char_count()
            .unwrap()
            .ticks(),
        1000
    );
    for payload in [
        format!("<switch>{unknown}</switch>"),
        format!("<switch>{}</switch>", branch(&margin(1, ""))),
        format!("{}<switch>{supported}</switch>", margin(0, "HWPUNIT")),
        format!("<switch>{supported}</switch><switch>{supported}</switch>"),
        format!(
            "<switch>{}</switch>",
            branch("<margin><left value=\"0\" unit=\"CHAR\"/></margin>")
        ),
    ] {
        assert!(parse_header(&head(&payload)).is_err(), "{payload}");
    }
}
#[test]
fn w1_default_tabs_and_verified_unitless_direct() {
    // 이것을 실패시키는 것: 변이 6 — unit 생략 default를 case 값으로 읽기.
    for (raw, ticks, unit) in [(30000, 15000, "hwpunit"), (2001, 1000, "char")] {
        for wrap in [false, true] {
            let items = format!(r#"<tabItem pos="{raw}" type="LEFT" leader="NONE"/>"#);
            let payload =
                if wrap { format!("<switch><default>{items}</default></switch>") } else { items };
            let parsed = parse_header(&format!(r#"<head><refList><tabProperties><tabPr id="3">{payload}</tabPr></tabProperties></refList></head>"#)).unwrap();
            let value = parsed.style_store.iter_tabs().next().unwrap().stops[0].position;
            assert_eq!(
                serde_json::to_value(value).unwrap(),
                serde_json::json!({"unit":unit,"value":ticks})
            );
        }
    }
}
#[test]
fn w1_line_spacing_separate_scale_and_odd_output_error() {
    for kind in ["FIXED", "AT_LEAST", "BETWEEN_LINES", "PERCENT"] {
        let ls = |v| format!(r#"<lineSpacing type="{kind}" value="{v}" unit="HWPUNIT"/>"#);
        let p =
            para(&format!("<switch>{}<default>{}</default></switch>", branch(&ls(1000)), ls(2001)));
        assert_eq!(p.line_spacing, if kind == "PERCENT" { 1000 } else { 2000 });
        for payload in [ls(2001), format!("<switch><default>{}</default></switch>", ls(2001))] {
            let p = para(&payload);
            assert_eq!(p.line_spacing, 2001);
            let mut store = HwpxStyleStore::new();
            store.push_para_shape(p);
            assert_eq!(
                crate::paragraph_measure::validate_measure_output(&store).is_err(),
                kind != "PERCENT"
            );
        }
    }
    for invalid in [
        r#"<lineSpacing type="FIXED" unit="HWPUNIT"/>"#,
        r#"<lineSpacing type="FIXED" value="-1" unit="HWPUNIT"/>"#,
        r#"<lineSpacing type="FIXED" value="1000" unit="CHAR"/>"#,
        r#"<lineSpacing type="UNKNOWN" value="1000" unit="HWPUNIT"/>"#,
    ] {
        assert!(parse_header(&head(invalid)).is_err());
    }
    // 이것을 실패시키는 것: 변이 12 — lineSpacing overflow를 clamp 성공으로 바꾸기.
    assert!(parse_header(&head(&format!(
        "<switch>{}</switch>",
        branch(r#"<lineSpacing type="FIXED" value="2147483647" unit="HWPUNIT"/>"#)
    )))
    .is_err());
}
#[test]
fn w1_output_packs_both_units_and_preserves_source_id() {
    // 이것을 실패시키는 것: 변이 7 — default=case 방출.
    for (value, unit, raw) in
        [(1000, "CHAR", 2001), (-750, "CHAR", -1499), (1000, "HWPUNIT", 2000), (0, "CHAR", 1)]
    {
        let p = para(&format!("<switch>{}</switch>", branch(&margin(value, unit))));
        let mut store = HwpxStyleStore::new();
        store.push_para_shape(p);
        let output = crate::encoder::header::encode_header(&store, 1, None).unwrap();
        assert!(output.contains(&format!(r#"value="{raw}" unit="HWPUNIT""#)), "{output}");
        assert!(output.contains(&format!(r#"value="{value}" unit="{unit}""#)), "{output}");
        assert!(output.contains(r#"paraPr id="7""#));
    }
    assert!(crate::paragraph_measure::encode_line_spacing(3, LineSpacingType::Fixed).is_err());
}

#[test]
fn fix1_signed_hwpunit_tabs_keep_unsigned_char_contract() {
    let tab = |payload: &str| {
        format!(
            r#"<head><refList><tabProperties><tabPr id="3">{payload}</tabPr></tabProperties></refList></head>"#
        )
    };
    // 이것을 실패시키는 것: 음수 direct/default 거부, signed 범위 초과 clamp.
    for (raw, expected) in [
        (-260288, -130144),
        (-260416, -130208),
        (-200_000_000, -100_000_000),
        (200_000_000, 100_000_000),
    ] {
        let item = format!(r#"<tabItem pos="{raw}"/>"#);
        for payload in [
            item.clone(),
            format!("<switch><default>{item}</default></switch>"),
            format!(
                "<switch>{}<default>{item}</default></switch>",
                branch(&format!(r#"<tabItem pos="{expected}" unit="HWPUNIT"/>"#))
            ),
        ] {
            let parsed = parse_header(&tab(&payload)).unwrap();
            assert_eq!(
                parsed.style_store.iter_tabs().next().unwrap().stops[0]
                    .position
                    .as_hwp_unit()
                    .unwrap()
                    .as_i32(),
                expected
            );
        }
    }
    // 이것을 실패시키는 것: 음수 CHAR 허용 또는 i64 XML 값을 검사 없이 잘라내기.
    for payload in [
        format!("<switch>{}</switch>", branch(r#"<tabItem pos="-1" unit="CHAR"/>"#)),
        format!("<switch>{}</switch>", branch(r#"<tabItem pos="-100000001" unit="HWPUNIT"/>"#)),
        format!("<switch>{}</switch>", branch(r#"<tabItem pos="2147483648" unit="CHAR"/>"#)),
        r#"<tabItem pos="-1"/>"#.into(),
        r#"<tabItem pos="-200000002"/>"#.into(),
        r#"<tabItem pos="200000002"/>"#.into(),
        r#"<tabItem pos="-4294967296"/>"#.into(),
        r#"<tabItem pos="4294967296"/>"#.into(),
    ] {
        assert!(parse_header(&tab(&payload)).is_err(), "{payload}");
    }
    // 이것을 실패시키는 것: 상위 비트가 켜진 CHAR raw를 signed로 해석.
    let parsed = parse_header(&tab(r#"<tabItem pos="4294967295"/>"#)).unwrap();
    assert_eq!(
        parsed.style_store.iter_tabs().next().unwrap().stops[0]
            .position
            .as_char_count()
            .unwrap()
            .ticks(),
        i32::MAX
    );
}

#[test]
fn fix2_margin_xml_cannot_wrap_into_another_valid_value() {
    // 이것을 실패시키는 것: XML i64를 검사 없이 i32로 잘라 음수 여백으로 수용.
    for raw in ["4294967295", "4294967294", "4294967296", "-4294967297"] {
        for unit in ["CHAR", "HWPUNIT"] {
            let payload = margin(0, unit).replace("value=\"0\"", &format!("value=\"{raw}\""));
            for xml in [
                format!("<switch>{}</switch>", branch(&payload)),
                format!("<switch><default>{payload}</default></switch>"),
                payload,
            ] {
                let err = parse_header(&head(&xml)).unwrap_err().to_string();
                assert!(err.contains("paraPr 7.left"), "{xml}: {err}");
                assert!(err.contains("range"), "{err}");
            }
        }
    }
    // Signed packed values remain signed, including the CHAR low bit.
    assert_eq!(para(&margin(-1, "HWPUNIT")).margin_left.as_char_count().unwrap().ticks(), -1);
}

#[test]
fn fix2_case_only_tabs_warn_with_definition_and_field() {
    // 이것을 실패시키는 것: case-only 탭 default 생성 경고 삭제.
    let xml = format!(
        r#"<head><refList><tabProperties><tabPr id="19"><switch>{}</switch></tabPr></tabProperties></refList></head>"#,
        branch(r#"<tabItem pos="1000" type="LEFT" leader="NONE" unit="CHAR"/>"#)
    );
    let parsed = parse_header(&xml).unwrap();
    assert!(parsed.warnings.iter().any(|w| matches!(w,
        super::DecodeWarning::ParagraphMeasureNormalized { definition: "tabPr", id: 19, field, reason }
        if field == "stops" && reason.contains("case-only"))));
    assert_eq!(
        parsed.style_store.iter_tabs().next().unwrap().stops[0]
            .position
            .as_char_count()
            .unwrap()
            .ticks(),
        1000
    );
}

#[test]
fn fix2_direct_line_spacing_warns_with_definition_and_field() {
    // 이것을 실패시키는 것: direct 줄 간격 switch 정규화 경고 삭제.
    let parsed =
        parse_header(&head(r#"<lineSpacing type="FIXED" value="2000" unit="HWPUNIT"/>"#)).unwrap();
    assert!(parsed.warnings.iter().any(|w| matches!(w,
        super::DecodeWarning::ParagraphMeasureNormalized { definition: "paraPr", id: 7, field, reason }
        if field == "lineSpacing" && reason.contains("direct"))));
    assert_eq!(parsed.style_store.para_shape(ParaShapeIndex::new(0)).unwrap().line_spacing, 2000);
}

#[test]
fn fix3_empty_supported_tab_case_is_authoritative() {
    // 이것을 실패시키는 것: 빈 case 거부, default 우선, 개수 차이 경고 삭제.
    for default in ["", r#"<tabItem pos="2000"/>"#, r#"<tabItem pos="1" unit="INVALID"/>"#] {
        let xml = format!(
            r#"<head><refList><tabProperties><tabPr id="99"><switch>{}<default>{default}</default></switch></tabPr></tabProperties></refList></head>"#,
            branch("")
        );
        let parsed = parse_header(&xml).unwrap();
        let tab = parsed.style_store.iter_tabs().next().unwrap();
        assert_eq!(tab.id, 99);
        assert!(tab.stops.is_empty());
        assert_eq!(parsed.warnings.len(), usize::from(!default.is_empty()));
        if !default.is_empty() {
            assert!(
                matches!(&parsed.warnings[0], super::DecodeWarning::ParagraphMeasureNormalized {
                definition: "tabPr", id: 99, field, reason,
            } if field == "stops" && reason.contains("default stop count differs"))
            );
        }
        let output = crate::encoder::header::encode_header(&parsed.style_store, 1, None).unwrap();
        let after = parse_header(&output).unwrap();
        let tab = after.style_store.iter_tabs().find(|t| t.id == 99).unwrap();
        assert!(tab.stops.is_empty());
    }
}
