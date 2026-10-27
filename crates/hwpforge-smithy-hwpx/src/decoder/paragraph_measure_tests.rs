//! branch and wire-contract regression tests.
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
        r#"<head><refList><paraProperties><paraPr id="0">{payload}</paraPr></paraProperties></refList></head>"#
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
fn case_precedes_mismatched_default_and_keeps_char_zero() {
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
fn direct_and_default_negative_odd_keep_arithmetic_shift() {
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
fn namespace_selection_and_invalid_payloads() {
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
fn default_tabs_and_verified_unitless_direct() {
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
fn line_spacing_separate_scale_and_odd_output_error() {
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
fn output_packs_both_units_and_numbers_positions() {
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
        assert!(output.contains(r#"paraPr id="0""#));
    }
    assert!(crate::paragraph_measure::encode_line_spacing(3, LineSpacingType::Fixed).is_err());
}

#[test]
fn signed_hwpunit_tabs_keep_unsigned_char_contract() {
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
fn margin_xml_cannot_wrap_into_another_valid_value() {
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
                assert!(err.contains("paraPr 0.left"), "{xml}: {err}");
                assert!(err.contains("range"), "{err}");
            }
        }
    }
    // Signed packed values remain signed, including the CHAR low bit.
    assert_eq!(para(&margin(-1, "HWPUNIT")).margin_left.as_char_count().unwrap().ticks(), -1);
}

#[test]
fn case_only_tabs_warn_with_definition_and_field() {
    // 이것을 실패시키는 것: case-only 탭 default 생성 경고 삭제.
    let xml = format!(
        r#"<head><refList><tabProperties><tabPr id="0"><switch>{}</switch></tabPr></tabProperties></refList></head>"#,
        branch(r#"<tabItem pos="1000" type="LEFT" leader="NONE" unit="CHAR"/>"#)
    );
    let parsed = parse_header(&xml).unwrap();
    assert!(parsed.warnings.iter().any(|w| matches!(w,
        super::DecodeWarning::ParagraphMeasureNormalized { definition: "tabPr", id: 0, field, reason }
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
fn direct_line_spacing_warns_with_definition_and_field() {
    // 이것을 실패시키는 것: direct 줄 간격 switch 정규화 경고 삭제.
    let parsed =
        parse_header(&head(r#"<lineSpacing type="FIXED" value="2000" unit="HWPUNIT"/>"#)).unwrap();
    assert!(parsed.warnings.iter().any(|w| matches!(w,
        super::DecodeWarning::ParagraphMeasureNormalized { definition: "paraPr", id: 0, field, reason }
        if field == "lineSpacing" && reason.contains("direct"))));
    assert_eq!(parsed.style_store.para_shape(ParaShapeIndex::new(0)).unwrap().line_spacing, 2000);
}

#[test]
fn empty_supported_tab_case_is_authoritative() {
    // 이것을 실패시키는 것: 빈 case 거부, default 우선, 개수 차이 경고 삭제.
    for default in ["", r#"<tabItem pos="2000"/>"#, r#"<tabItem pos="1" unit="INVALID"/>"#] {
        let xml = format!(
            r#"<head><refList><tabProperties><tabPr id="0"><switch>{}<default>{default}</default></switch></tabPr></tabProperties></refList></head>"#,
            branch("")
        );
        let parsed = parse_header(&xml).unwrap();
        let tab = parsed.style_store.iter_tabs().next().unwrap();
        assert_eq!(tab.id, 0);
        assert!(tab.stops.is_empty());
        assert_eq!(parsed.warnings.len(), usize::from(!default.is_empty()));
        if !default.is_empty() {
            assert!(
                matches!(&parsed.warnings[0], super::DecodeWarning::ParagraphMeasureNormalized {
                definition: "tabPr", id: 0, field, reason,
            } if field == "stops" && reason.contains("default stop count differs"))
            );
        }
        let output = crate::encoder::header::encode_header(&parsed.style_store, 1, None).unwrap();
        let after = parse_header(&output).unwrap();
        let tab = after.style_store.iter_tabs().find(|t| t.id == 0).unwrap();
        assert!(tab.stops.is_empty());
    }
}

// 이것을 실패시키는 것: HeaderFidelity를 원본 ID/배열 위치로 별도 조회하거나,
// setter·Clone·JSON 중 한 경로에서 fontLineHeight를 버리는 것.
#[test]
fn font_line_height_rust_clone_at_new_position() {
    let mut store = HwpxStyleStore::new();
    let original = store.push_para_shape(HwpxParaShape::default());
    store.set_para_font_line_height(original, 1).unwrap();
    let copy = store.para_shape(original).unwrap().clone();
    let clone = store.push_para_shape(copy);
    store.set_para_font_line_height(original, 0).unwrap();
    let json = serde_json::to_string(&store).unwrap();
    let store: HwpxStyleStore = serde_json::from_str(&json).unwrap();
    let xml = crate::encoder::header::encode_header(&store, 1, None).unwrap();
    let parsed = parse_header(&xml).unwrap();
    assert_eq!(parsed.style_store.para_shape(original).unwrap().font_line_height, 0);
    assert_eq!(parsed.style_store.para_shape(clone).unwrap().font_line_height, 1);
}

// 이것을 실패시키는 것: 경고를 정규화 사실로 사용, case/default 일부만 비교,
// unit·분기 유무 차이를 무시, 또는 무관한 switch 때문에 정상 입력을 정규화.
#[test]
fn measure_normalization_compares_encoder_payloads() {
    use crate::schema::header::{HxHead, HxSwitch, HxSwitchCase, HxSwitchDefault};
    let mut store = HwpxStyleStore::new();
    store.push_para_shape(HwpxParaShape::default());
    let xml = crate::encoder::header::encode_header(&store, 1, None).unwrap();
    assert!(!parse_header(&xml).unwrap().requires_header_normalization);
    for change in [
        "direct-margin",
        "direct-line",
        "missing-margin",
        "missing-line",
        "margin-default",
        "line-default",
        "case-only",
        "default-only",
        "unit",
        "split",
        "unrelated",
    ] {
        let mut head: HxHead = crate::decoder::xml_from_str(&xml).unwrap();
        let para = &mut head.ref_list.as_mut().unwrap().para_properties.as_mut().unwrap().items[0];
        let sw = &mut para.switches[0];
        match change {
            "direct-margin" => {
                sw.cases[0].margin = None;
                para.margin = sw.default.as_mut().unwrap().margin.take();
            }
            "direct-line" => {
                sw.cases[0].line_spacing = None;
                para.line_spacing = sw.default.as_mut().unwrap().line_spacing.take();
            }
            "missing-margin" => {
                sw.cases[0].margin = None;
                sw.default.as_mut().unwrap().margin = None;
            }
            "missing-line" => {
                sw.cases[0].line_spacing = None;
                sw.default.as_mut().unwrap().line_spacing = None;
            }
            "margin-default" => {
                sw.default.as_mut().unwrap().margin.as_mut().unwrap().left.as_mut().unwrap().value =
                    2
            }
            "line-default" => {
                sw.default.as_mut().unwrap().line_spacing.as_mut().unwrap().value = 170
            }
            "case-only" => sw.default = None,
            "default-only" => sw.cases.clear(),
            "unit" => sw.default.as_mut().unwrap().line_spacing.as_mut().unwrap().unit.clear(),
            "split" => {
                let line = HxSwitch {
                    cases: vec![HxSwitchCase {
                        required_namespace: NS.into(),
                        margin: None,
                        line_spacing: sw.cases[0].line_spacing.take(),
                    }],
                    default: Some(HxSwitchDefault {
                        margin: None,
                        line_spacing: sw.default.as_mut().unwrap().line_spacing.take(),
                    }),
                };
                para.switches.push(line);
            }
            "unrelated" => para.switches.insert(
                0,
                HxSwitch {
                    cases: vec![HxSwitchCase {
                        required_namespace: "urn:unrelated".into(),
                        margin: None,
                        line_spacing: None,
                    }],
                    default: None,
                },
            ),
            _ => unreachable!(),
        }
        #[allow(clippy::disallowed_methods)] // Normalize the serialized test header below.
        let xml = quick_xml::se::to_string(&head).unwrap();
        let xml = crate::wire_xml::normalize_attr_control_whitespace(&xml);
        let parsed = parse_header(&xml).unwrap();
        assert_eq!(
            parsed.requires_header_normalization,
            !matches!(change, "split" | "unrelated"),
            "{change}"
        );
    }
}

// 이것을 실패시키는 것: direct/empty/case-only/default-only 탭 또는 출력과 다른
// default 척도·단위·개수를 원본 반환 가능으로 분류하는 것.
#[test]
fn tab_normalization_compares_encoder_payloads() {
    use crate::schema::header::HxHead;
    let mut store = HwpxStyleStore::new();
    store.push_tab(hwpforge_core::TabDef {
        id: 3,
        auto_tab_left: false,
        auto_tab_right: false,
        stops: vec![hwpforge_core::TabStop {
            position: ParagraphMeasure::from_char_count(CharCount::from_ticks(1000)),
            align: hwpforge_foundation::TabAlign::Left,
            leader: hwpforge_foundation::TabLeader::none(),
        }],
    });
    let xml = crate::encoder::header::encode_header(&store, 1, None).unwrap();
    assert!(!parse_header(&xml).unwrap().requires_header_normalization);
    for change in ["direct", "empty", "case-only", "default-only", "value", "unit", "count"] {
        let mut head: HxHead = crate::decoder::xml_from_str(&xml).unwrap();
        // the sole explicit definition occupies position 0, irrespective of id=3.
        let tab = &mut head.ref_list.as_mut().unwrap().tab_properties.as_mut().unwrap().items[0];
        let sw = &mut tab.switches[0];
        match change {
            "direct" => {
                tab.items = sw.default.take().unwrap().items;
                tab.switches.clear();
            }
            "empty" => {
                sw.cases[0].items.clear();
                sw.default.as_mut().unwrap().items.clear();
            }
            "case-only" => sw.default = None,
            "default-only" => sw.cases.clear(),
            "value" => sw.default.as_mut().unwrap().items[0].pos = 2000,
            "unit" => sw.default.as_mut().unwrap().items[0].unit = "HWPUNIT".into(),
            "count" => sw.default.as_mut().unwrap().items.clear(),
            _ => unreachable!(),
        }
        #[allow(clippy::disallowed_methods)] // Normalize the serialized test header below.
        let xml = quick_xml::se::to_string(&head).unwrap();
        let parsed =
            parse_header(&crate::wire_xml::normalize_attr_control_whitespace(&xml)).unwrap();
        assert!(parsed.requires_header_normalization, "{change}");
    }
}

// 이것을 실패시키는 것: switch 개수 비교 / case·default의 단위·순서·branch 검사를 생략하기.
#[test]
fn split_tab_switches_compare_ordered_case_and_default_payloads() {
    use crate::schema::header::{HxHead, HxTabSwitch, HxTabSwitchCase, HxTabSwitchDefault};
    let mut store = HwpxStyleStore::new();
    store.push_tab(hwpforge_core::TabDef {
        id: 0,
        auto_tab_left: false,
        auto_tab_right: false,
        stops: [1000, 2000, 3000]
            .map(|value| hwpforge_core::TabStop {
                position: ParagraphMeasure::from_char_count(CharCount::from_ticks(value)),
                align: hwpforge_foundation::TabAlign::Left,
                leader: hwpforge_foundation::TabLeader::none(),
            })
            .to_vec(),
    });
    let xml = crate::encoder::header::encode_header(&store, 1, None).unwrap();
    let parsed = parse_header(&xml).unwrap();
    assert!(!parsed.requires_header_normalization);
    for change in [
        "split",
        "default-unit",
        "case-unit",
        "default-order",
        "default-value",
        "default-count",
        "missing-default",
        "unknown-case",
        "unknown-only",
        "duplicate-case",
    ] {
        let mut head: HxHead = crate::decoder::xml_from_str(&xml).unwrap();
        let tab = &mut head.ref_list.as_mut().unwrap().tab_properties.as_mut().unwrap().items[0];
        let original = tab.switches.remove(0);
        tab.switches = original.cases[0]
            .items
            .iter()
            .zip(&original.default.as_ref().unwrap().items)
            .map(|(case, default)| HxTabSwitch {
                cases: vec![HxTabSwitchCase {
                    required_namespace: NS.into(),
                    items: vec![case.clone()],
                }],
                default: Some(HxTabSwitchDefault { items: vec![default.clone()] }),
            })
            .collect();
        match change {
            "split" => {}
            "default-unit" => {
                tab.switches[0].default.as_mut().unwrap().items[0].unit = "HWPUNIT".into()
            }
            "case-unit" => tab.switches[0].cases[0].items[0].unit = String::new(),
            "default-order" => {
                let first = tab.switches[0].default.clone();
                tab.switches[0].default = tab.switches[1].default.clone();
                tab.switches[1].default = first;
            }
            "default-value" => tab.switches[0].default.as_mut().unwrap().items[0].pos += 1,
            "default-count" => tab.switches[0].default.as_mut().unwrap().items.clear(),
            "missing-default" => tab.switches[0].default = None,
            "unknown-case" | "duplicate-case" => {
                let mut extra = tab.switches[0].cases[0].clone();
                if change == "unknown-case" {
                    extra.required_namespace = "urn:unknown".into();
                }
                tab.switches[0].cases.push(extra);
            }
            "unknown-only" => tab.switches[0].cases[0].required_namespace = "urn:unknown".into(),
            _ => unreachable!(),
        }
        #[allow(clippy::disallowed_methods)] // Normalize serialized test attributes below.
        let xml = quick_xml::se::to_string(&head).unwrap();
        let result = parse_header(&crate::wire_xml::normalize_attr_control_whitespace(&xml));
        if change == "case-unit" {
            // An unverified measure unit must never enter the original-return path.
            assert!(result.is_err(), "{change}");
        } else {
            let result = result.unwrap();
            assert_eq!(result.requires_header_normalization, change != "split", "{change}");
            if change == "split" {
                assert_eq!(
                    result.style_store.iter_tabs().collect::<Vec<_>>(),
                    parsed.style_store.iter_tabs().collect::<Vec<_>>()
                );
            }
        }
    }
}
