use super::*;
use crate::test_support::xml;
use hwpforge_blueprint::{ListMarkerAlign, ListMarkerLayout, ListTextStart};
use hwpforge_core::{ParaHead, Run};
use hwpforge_foundation::CharShapeIndex;
fn store() -> HwpxStyleStore {
    let mut s = HwpxStyleStore::with_default_fonts("맑은 고딕");
    for bold in [false, true] {
        s.push_char_shape(crate::HwpxCharShape {
            height: HwpUnit::new(1100).unwrap(),
            bold,
            ..Default::default()
        });
    }
    for id in 1..=2 {
        s.push_numbering(NumberingDef {
            id,
            start: 1,
            levels: vec![ParaHead {
                level: 1,
                start: 1,
                num_format: NumberFormatType::Digit,
                text: "^1.".into(),
                checkable: false,
            }],
        });
    }
    for (i, def, c) in [(0, 1, 0), (1, 1, 1), (2, 2, 0), (3, 2, 1)] {
        let p = crate::HwpxParaShape {
            heading_type: HeadingType::Number,
            heading_id_ref: def,
            margin_left: ParagraphMeasure::from_hwp_unit(HwpUnit::new(3600).unwrap()),
            indent: ParagraphMeasure::from_hwp_unit(HwpUnit::new(-1800).unwrap()),
            ..Default::default()
        };
        s.push_para_shape(p);
        let mut m = ListMarkerLayout::new(
            ListMarkerAlign::Left,
            ListTextStart::FromAnchor(HwpUnit::new(1800).unwrap()),
        )
        .unwrap();
        m.char_shape = Some(CharShapeIndex::new(c));
        s.list_marker_layouts.insert(i, m);
    }
    s
}
fn paragraph(id: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::text("body", CharShapeIndex::new(0))], ParaShapeIndex::new(id))
}
fn parts(xml: String) -> Vec<super::super::section::SectionEncodeResult> {
    vec![super::super::section::SectionEncodeResult {
        xml,
        master_pages: vec![],
        warnings: vec![],
        dropped_caches: vec![],
        charts: vec![],
        embedded_oles: vec![],
    }]
}
fn record(
    ledger: &mut ListLedger,
    ids: &[usize],
) -> Vec<super::super::section::SectionEncodeResult> {
    let mut xml = "<hs:sec>".to_owned();
    for &id in ids {
        let marker = ledger.record(&paragraph(id), &[]).unwrap().unwrap();
        xml.push_str(&format!("<hp:p hwpforgeList=\"{marker}\" paraPrIDRef=\"{id}\"/>"));
    }
    xml.push_str("</hs:sec>");
    parts(xml)
}
// 이것을 실패시키는 것: 요청 없는 저장소에도 장부를 설치.
#[test]
fn unrequested_lists_keep_original_bytes() {
    let mut s = store();
    s.list_marker_layouts.clear();
    assert!(ListLedger::new(&s).unwrap().is_none());
}
// 이것을 실패시키는 것: 표지 제거를 생략하거나 모든 기록의 최종 출현을 강제.
#[test]
fn private_markers_are_removed_from_all_emitted_parts() {
    let mut l = ListLedger::new(&store()).unwrap().unwrap();
    let mut p = record(&mut l, &[0, 1]);
    let unused = l.record(&paragraph(0), &[]).unwrap().unwrap();
    assert_eq!(unused, 2);
    p[0].master_pages.push((
        "Contents/masterpage0.xml".into(),
        "<masterPage><hp:p paraPrIDRef=\"0\">hwpforgeList</hp:p></masterPage>".into(),
    ));
    l.finish(&mut p).unwrap();
    for text in [&p[0].xml, &p[0].master_pages[0].1] {
        let tree = xml::Element::parse(text);
        assert!(tree.descendants("hp:p").iter().all(|p| p.attr("hwpforgeList").is_none()));
    }
    assert_eq!(
        xml::Element::parse(&p[0].master_pages[0].1).descendants("hp:p")[0].text,
        "hwpforgeList"
    );
}
// 이것을 실패시키는 것: 모르는 표지·참조 누락을 허용하거나 중복 표지를 하나로 정규화.
#[test]
fn invalid_private_markers_are_rejected() {
    for xml in [
        "<hp:p hwpforgeList=\"999999\" paraPrIDRef=\"0\"/>",
        "<hp:p hwpforgeList=\"0\" hwpforgeList=\"0\" paraPrIDRef=\"0\"/>",
        "<hp:p hwpforgeList=\"0\"/>",
        "<hp:p hwpforgeList=\"bad\" paraPrIDRef=\"0\"/>",
    ] {
        let mut l = ListLedger::new(&store()).unwrap().unwrap();
        l.record(&paragraph(0), &[]).unwrap();
        assert!(matches!(
            l.finish(&mut parts(xml.into())),
            Err(crate::HwpxError::InvalidStructure { .. })
        ));
    }
}
// 이것을 실패시키는 것: head·left·intent의 끝 검사 중 하나를 제거.
#[test]
fn final_validation_rejects_wrong_heads_and_coordinates() {
    let l = ListLedger::new(&store()).unwrap().unwrap();
    let requirement = paragraph_requirement(&l.source, &paragraph(0)).unwrap().unwrap();
    let expected = vec![AssignedUse {
        use_id: 0,
        shape: ParaShapeIndex::new(0),
        definition: 1,
        head: requirement.metrics.clone(),
    }];
    let uses = vec![ListUse {
        original: ParaShapeIndex::new(0),
        requirement: requirement.clone(),
        unsupported: false,
        cells: vec![],
        caption: None,
    }];
    let mut out = l.source;
    set_head(&mut out, false, 1, 1, requirement.metrics.clone()).unwrap();
    set_coordinates(
        out.para_shape_mut(ParaShapeIndex::new(0)).unwrap(),
        &final_coordinates(&requirement, &requirement.metrics).unwrap(),
    )
    .unwrap();
    validate(&out, &uses, &expected).unwrap();
    for field in ["head", "left", "intent"] {
        let mut bad = out.clone();
        match field {
            "head" => {
                bad.header_fidelity
                    .numbering
                    .get_mut(&1)
                    .unwrap()
                    .get_mut(&1)
                    .unwrap()
                    .char_pr_id_ref = 1
            }
            "left" => {
                bad.para_shape_mut(ParaShapeIndex::new(0)).unwrap().margin_left =
                    ParagraphMeasure::from_hwp_unit(HwpUnit::new(1801).unwrap())
            }
            _ => {
                bad.para_shape_mut(ParaShapeIndex::new(0)).unwrap().indent =
                    ParagraphMeasure::from_hwp_unit(HwpUnit::new(1).unwrap())
            }
        };
        assert!(
            matches!(
                validate(&bad, &uses, &expected),
                Err(crate::HwpxError::InvalidStructure { .. })
            ),
            "{field}"
        );
    }
}
// 이것을 실패시키는 것: 예산 초과 정의의 임시 참조·fidelity·예산을 남김.
#[test]
fn exceeded_budgets_discard_the_entire_definition_plan() {
    for budget in [
        SplitBudget { definitions: 1, ..Default::default() },
        SplitBudget { text: 3, ..Default::default() },
        SplitBudget { paragraphs: 0, ..Default::default() },
    ] {
        let s = store();
        let mut l = ListLedger::new(&s).unwrap().unwrap();
        l.budget = budget;
        let mut p = record(&mut l, &[0, 1, 0, 2, 3]);
        let (out, w) = l.finish(&mut p).unwrap();
        assert_eq!(out.numbering_count(), 3);
        assert_eq!(out.para_shape_count(), 4);
        assert!(out.header_fidelity.numbering.keys().all(|&id| id <= 3));
        assert!(matches!(
            w.iter().find(|w| matches!(w, EncodeWarning::ListNumberingSplit { .. })),
            Some(EncodeWarning::ListNumberingSplit {
                definition_id: 2,
                additional_definitions: 1,
                ..
            })
        ));
        assert_eq!(w.iter().filter(|w|matches!(w,EncodeWarning::ContentLost{kind,..}if kind=="list_marker_conflict")).count(),1);
        let tree = xml::Element::parse(&p[0].xml);
        assert_eq!(
            tree.descendants("hp:p")
                .iter()
                .map(|p| p.attr("paraPrIDRef").unwrap())
                .collect::<Vec<_>>(),
            ["0", "1", "0", "2", "3"]
        );
    }
}
// 이것을 실패시키는 것: 운영 상한 또는 checked 누적 경계의 1 차이.
#[test]
fn split_budget_boundaries_are_checked() {
    let b = SplitBudget::default();
    assert_eq!((b.definitions, b.text, b.paragraphs), (16_384, 67_108_864, 16_384));
    for (d, t, p, allowed) in [
        (16_384, 0, 0, true),
        (16_385, 0, 0, false),
        (0, 67_108_864, 0, true),
        (0, 67_108_865, 0, false),
        (0, 0, 16_384, true),
        (0, 0, 16_385, false),
        (usize::MAX, 0, 0, false),
    ] {
        assert_eq!(b.remaining(d, t, p).is_some(), allowed);
    }
    let mut c = CounterState::new([1; 10]);
    c.values[0] = 4_294_967_295;
    c.used[0] = true;
    assert_eq!(c.segment_starts(0).unwrap()[0], 4_294_967_295);
    c.values[0] = 4_294_967_296;
    assert!(c.segment_starts(0).is_none());
}
// 이것을 실패시키는 것: 조상의 미지원을 자식의 지원 자리로 덮음.
#[test]
fn unsupported_scopes_remain_inherited() {
    let mut l = ListLedger::new(&store()).unwrap().unwrap();
    l.push_scope(ListScope::Unsupported);
    l.push_scope(ListScope::TableCells(0));
    let mut p = record(&mut l, &[0, 1]);
    l.pop_scope();
    l.pop_scope();
    let (out, w) = l.finish(&mut p).unwrap();
    assert_eq!(out.numbering_count(), 2);
    assert_eq!(
        w.iter()
            .filter(
                |w| matches!(w,EncodeWarning::ContentLost{kind,..}if kind=="list_marker_conflict")
            )
            .count(),
        1
    );
}
// 이것을 실패시키는 것: 새 start를 u32로 잘라 저장하거나 초과 정의의 예산을 소비.
#[test]
fn overflowing_starts_leave_the_next_definition_available() {
    let mut s = store();
    let mut value = serde_json::to_value(&s).unwrap();
    value["numberings"][0]["levels"][0]["start"] = u32::MAX.into();
    s = serde_json::from_value(value).unwrap();
    let mut l = ListLedger::new(&s).unwrap().unwrap();
    let mut p = record(&mut l, &[0, 1, 2, 3]);
    let (out, w) = l.finish(&mut p).unwrap();
    assert_eq!(out.numbering_count(), 3);
    assert_eq!(out.numbering_at(0).unwrap().levels[0].start, u32::MAX);
    assert!(w
        .iter()
        .any(|w| matches!(w, EncodeWarning::ListNumberingSplit { definition_id: 2, .. })));
    assert!(!w
        .iter()
        .any(|w| matches!(w, EncodeWarning::ListNumberingSplit { definition_id: 1, .. })));
}
// 이것을 실패시키는 것: 실패한 자리 closure 뒤 scope 복구를 생략.
#[test]
fn failed_scopes_restore_the_parent_placement() {
    let mut context = super::super::section::DocumentEncodeContext::new(0);
    context.list_ledger = ListLedger::new(&store()).unwrap();
    let result: HwpxResult<()> =
        context.with_list_scope(ListScope::Unsupported, |_| Err(invalid("injected failure")));
    assert!(result.is_err());
    let l = context.list_ledger.as_mut().unwrap();
    l.record(&paragraph(0), &[]).unwrap();
    assert!(!l.uses[0].unsupported);
}
// 이것을 실패시키는 것: 잔류 표지 끝 검사를 생략.
#[test]
fn residual_marker_attributes_are_rejected() {
    assert!(matches!(
        rewrite(
            "<hp:p hwpforgeList=\"0\" paraPrIDRef=\"0\"/>",
            PartEdits { marks: vec![] },
            &BTreeMap::new()
        ),
        Err(crate::HwpxError::InvalidStructure { .. })
    ));
}
// 이것을 실패시키는 것: 근사 충돌 경고 누락·중복을 허용.
#[test]
fn approximate_warning_counts_are_validated() {
    let key = DefinitionKey { bullet: false, position: 1 };
    let w = conflict(key, "unsupported paragraph placement".into());
    validate_approximations(true, std::slice::from_ref(&w)).unwrap();
    validate_approximations(false, &[]).unwrap();
    assert!(validate_approximations(false, std::slice::from_ref(&w)).is_err());
    assert!(validate_approximations(true, &[]).is_err());
    assert!(validate_approximations(true, &[w.clone(), w]).is_err());
}

// 이것을 실패시키는 것: 뒤 정의를 검사할 때 앞 정의의 충돌 경고까지 함께 셈.
#[test]
fn approximation_warning_validation_is_local_to_each_definition() {
    let mut l = ListLedger::new(&store()).unwrap().unwrap();
    l.push_scope(ListScope::Unsupported);
    let mut p = record(&mut l, &[0, 1, 2, 3]);
    let (_, warnings) = l.finish(&mut p).unwrap();
    assert_eq!(warnings.iter().filter(|w| matches!(w, EncodeWarning::ContentLost{kind,..} if kind == "list_marker_conflict")).count(), 2);
}

// 이것을 실패시키는 것: 장부 쪽에서 빈 문단의 끝 표시보다 style을 먼저 선택.
#[test]
fn empty_marker_requirements_keep_end_before_style() {
    let mut s = store();
    s.push_style(crate::style_store::HwpxStyle::new(0, "PARA", "body", "body", 0, 1, 0, 1042, 0));
    s.list_marker_layouts.get_mut(&0).unwrap().char_shape = None;
    let mut p = Paragraph::new(ParaShapeIndex::new(0));
    p.end_char_shape = Some(CharShapeIndex::new(0));
    p.style_ref = Some(hwpforge_foundation::StyleRef::new(0));
    let mut l = ListLedger::new(&s).unwrap().unwrap();
    for (end, expected) in [(Some(CharShapeIndex::new(0)), 1100), (None, 1100)] {
        p.end_char_shape = end;
        let id = l.record(&p, &[]).unwrap().unwrap();
        // Both characters are 11pt here; their inherited bold context must still come from the selected character.
        assert_eq!(l.uses[id].requirement.base, expected);
        assert_eq!(l.uses[id].requirement.measured_size, end.map(|_| 1100));
    }
}

#[path = "../../../tests/support/list_counter_cases.rs"]
mod list_counter_cases;
// 이것을 실패시키는 것: 상위 사용 뒤 하위 used 비트를 유지하거나 암묵 상위 진행을 생략.
#[test]
fn logical_counter_matches_literal_observations() {
    for case in list_counter_cases::CASES {
        let Some(expected) = case.expected else {
            continue;
        };
        let mut states: Vec<_> = case
            .definitions
            .iter()
            .map(|d| {
                let mut starts = [1; 10];
                starts[..d.starts.len()].copy_from_slice(d.starts);
                CounterState::new(starts)
            })
            .collect();
        let mut output = Vec::new();
        for &(definition, level) in case.uses {
            if definition == 0 {
                continue;
            }
            let d = &case.definitions[definition - 1];
            let state = &mut states[definition - 1];
            state.advance(level).unwrap();
            let mut text = d.texts[level].to_owned();
            for i in (0..d.starts.len()).rev() {
                let n = state.values[i];
                let value = match d.formats[i] {
                    "RomanCapital" => ["", "I", "II", "III", "IV"][n as usize].to_owned(),
                    "HangulSyllable" => ["", "가", "나", "다", "라"][n as usize].to_owned(),
                    _ => n.to_string(),
                };
                text = text.replace(&format!("^{}", i + 1), &value);
            }
            output.push(text);
        }
        assert_eq!(output, expected, "{}", case.name);
    }
}

// 이것을 실패시키는 것: 미지원 자리라는 이유만으로 같은 head·이동에도 충돌을 추가.
#[test]
fn identical_unsupported_uses_are_exact_and_keep_projected_bytes() {
    for path in
        [vec![crate::PathSeg::Footer(0)], vec![crate::PathSeg::Endnote], vec![crate::PathSeg::Memo]]
    {
        let s = store();
        let mut l = ListLedger::new(&s).unwrap().unwrap();
        let mut p = parts("<hs:sec>".into());
        let r = paragraph_requirement(&s, &paragraph(0)).unwrap().unwrap();
        let mut expected = s.clone();
        crate::list_marker_layout::project_first_uses(
            &mut expected,
            [(ParaShapeIndex::new(0), &r)],
            None,
        )
        .unwrap();
        crate::list_marker_layout::fold_requests(&mut expected).unwrap();
        for _ in 0..2 {
            let id = l.record(&paragraph(0), &path).unwrap().unwrap();
            p[0].xml.push_str(&format!("<hp:p hwpforgeList=\"{id}\" paraPrIDRef=\"0\"/>"));
        }
        p[0].xml.push_str("</hs:sec>");
        let (out, warnings) = l.finish(&mut p).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(serde_json::to_value(out).unwrap(), serde_json::to_value(expected).unwrap());
        assert_two_paragraphs_use_shape_zero(&p[0].xml);
    }
}
// 이것을 실패시키는 것: 방출 안 된 요청 때문에 무요청 미지원 사용처에 거짓 충돌.
#[test]
fn unemitted_requests_do_not_make_unsupported_uses_approximate() {
    let mut s = store();
    s.list_marker_layouts.remove(&0);
    let mut expected = s.clone();
    crate::list_marker_layout::fold_requests(&mut expected).unwrap();
    let mut l = ListLedger::new(&s).unwrap().unwrap();
    l.push_scope(ListScope::Unsupported);
    let mut p = record(&mut l, &[0, 0]);
    let (out, warnings) = l.finish(&mut p).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(serde_json::to_value(out).unwrap(), serde_json::to_value(expected).unwrap());
}
// 이것을 실패시키는 것: 사용하지 않는 수준 start 0만으로 근사 경고.
#[test]
fn unused_zero_start_level_does_not_approximate_identical_uses() {
    let mut value = serde_json::to_value(store()).unwrap();
    value["numberings"][0]["levels"].as_array_mut().unwrap().push(serde_json::json!({"level":2,"start":0,"num_format":"Digit","text":"^2.","checkable":false}));
    let s = serde_json::from_value(value).unwrap();
    let mut l = ListLedger::new(&s).unwrap().unwrap();
    let mut p = record(&mut l, &[0, 0]);
    let (out, w) = l.finish(&mut p).unwrap();
    assert!(w.is_empty());
    assert_eq!(out.numbering_at(0).unwrap().levels[1].start, 0);
}
/// Both emitted paragraphs keep the original shape and no private marker attribute.
// 이것을 실패시키는 것: 표지 속성을 지운 자리에 다른 속성을 남김.
fn assert_two_paragraphs_use_shape_zero(section_xml: &str) {
    let section = xml::Element::parse(section_xml);
    assert_eq!(section.name, "hs:sec");
    assert_eq!(section.child_names(), ["hp:p", "hp:p"]);
    for paragraph in &section.children {
        assert_eq!(
            paragraph.attributes,
            BTreeMap::from([("paraPrIDRef".to_string(), "0".to_string())])
        );
        assert!(paragraph.children.is_empty());
    }
}
fn encoded_header(store: &HwpxStyleStore) -> String {
    super::super::header::encode_header(store, 1, None, "", hwpforge_core::LayoutProfile::default())
        .unwrap()
}

// 이것을 실패시키는 것: 쓰이지 않는 head·문단 수준만으로 같은 head에 근사 경고를 추가.
#[test]
fn unused_invalid_levels_preserve_identical_heads_and_bytes() {
    for unused_shape in [false, true] {
        let mut value = serde_json::to_value(store()).unwrap();
        if unused_shape {
            value["para_shapes"][1]["heading_level"] = 11.into();
            value["list_marker_layouts"].as_object_mut().unwrap().remove("1");
        } else {
            value["numberings"][0]["levels"].as_array_mut().unwrap().push(serde_json::json!({"level":11,"start":1,"num_format":"Digit","text":"^1.","checkable":false}));
        }
        let s = serde_json::from_value(value).unwrap();
        let r = paragraph_requirement(&s, &paragraph(0)).unwrap().unwrap();
        let mut expected = s.clone();
        crate::list_marker_layout::project_first_uses(
            &mut expected,
            [(ParaShapeIndex::new(0), &r)],
            None,
        )
        .unwrap();
        crate::list_marker_layout::fold_requests(&mut expected).unwrap();
        let mut l = ListLedger::new(&s).unwrap().unwrap();
        // A paragraph whose shape has an out-of-range level and no marker request is
        // not recorded, so it cannot make the definition approximate.
        if unused_shape {
            l.record(&paragraph(1), &[]).unwrap();
        }
        let mut p = record(&mut l, &[0, 0]);
        let first = l.uses.len() - 2;
        assert!(!l
            .reasons(DefinitionKey { bullet: false, position: 1 }, &[first, first + 1])
            .contains(&ApproximationReason::InvalidLevel));
        let (out, warnings) = l.finish(&mut p).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(encoded_header(&out), encoded_header(&expected));
        assert_two_paragraphs_use_shape_zero(&p[0].xml);
    }
}

// 이것을 실패시키는 것: 쓰이지 않는 head 수준을 사본 start 배열로 인덱싱하거나 분리를 막음.
#[test]
fn unused_invalid_heads_preserve_or_split_without_panicking() {
    for level in [0, 11] {
        for different in [false, true] {
            let mut value = serde_json::to_value(store()).unwrap();
            value["numberings"][0]["levels"].as_array_mut().unwrap().push(serde_json::json!({"level":level,"start":7,"num_format":"Digit","text":"literal","checkable":false}));
            if !different {
                value["list_marker_layouts"]["1"] = value["list_marker_layouts"]["0"].clone();
            }
            let s = serde_json::from_value(value).unwrap();
            let mut l = ListLedger::new(&s).unwrap().unwrap();
            let mut p = record(&mut l, &[0, 1]);
            let (out, w) = l.finish(&mut p).unwrap();
            assert_eq!(out.numbering_count(), 2 + usize::from(different));
            assert_eq!(out.numbering_at(0).unwrap().levels[1].start, 7);
            if different {
                assert_eq!(out.numbering_at(2).unwrap().levels[1].start, 7);
            }
            assert_eq!(
                w.iter().filter(|w| matches!(w, EncodeWarning::ListNumberingSplit { .. })).count(),
                usize::from(different)
            );
            assert!(!w.iter().any(|w| matches!(w, EncodeWarning::ContentLost{kind,..} if kind == "list_marker_conflict")));
        }
    }
}

// 이것을 실패시키는 것: 방출한 범위 밖 수준을 배열로 접근하거나 같은 head에도 충돌을 생성.
#[test]
fn emitted_invalid_levels_warn_only_for_different_heads_without_panicking() {
    for different in [false, true] {
        let mut value = serde_json::to_value(store()).unwrap();
        value["numberings"][0]["levels"].as_array_mut().unwrap().push(serde_json::json!({"level":12,"start":1,"num_format":"Digit","text":"literal","checkable":false}));
        if !different {
            value["list_marker_layouts"]["1"] = value["list_marker_layouts"]["0"].clone();
        }
        let mut unused = value["para_shapes"][1].clone();
        unused["heading_level"] = 11.into();
        value["para_shapes"].as_array_mut().unwrap().push(unused);
        let s: HwpxStyleStore = serde_json::from_value(value).unwrap();
        let mut l = ListLedger::new(&s).unwrap().unwrap();
        let mut p = record(&mut l, &[0, 1, 4]);
        let (_, warnings) = l.finish(&mut p).unwrap();
        let conflicts: Vec<_> = warnings
            .iter()
            .filter_map(|w| match w {
                EncodeWarning::ContentLost { kind, reason, .. }
                    if kind == "list_marker_conflict" =>
                {
                    Some(reason)
                }
                _ => None,
            })
            .collect();
        assert_eq!(conflicts.len(), usize::from(different));
        if different {
            assert!(conflicts[0].contains("outside 1..=10"));
        }
        assert!(!warnings.iter().any(|w| matches!(w, EncodeWarning::ListNumberingSplit { .. })));
    }
}

// 이것을 실패시키는 것: 글머리표의 wire 수준만 검사해 범위 밖 native head를 정확 배정으로 승인.
#[test]
fn invalid_bullet_heads_use_projection_only_when_heads_differ() {
    for level in [0, 11] {
        for different in [false, true] {
            let mut s = store();
            s.push_bullet(BulletDef {
                id: 1,
                bullet_char: "•".into(),
                checked_char: None,
                use_image: false,
                para_head: ParaHead {
                    start: 1,
                    level,
                    num_format: NumberFormatType::Digit,
                    text: "".into(),
                    checkable: false,
                },
            });
            for id in [0, 1] {
                s.para_shape_mut(ParaShapeIndex::new(id)).unwrap().heading_type =
                    HeadingType::Bullet;
            }
            if !different {
                s.list_marker_layouts.insert(1, s.list_marker_layouts[&0]);
            }
            let mut l = ListLedger::new(&s).unwrap().unwrap();
            let mut p = record(&mut l, &[0, 1]);
            assert!(matches!(
                l.plan(DefinitionKey { bullet: true, position: 1 }, &[0, 1]).unwrap(),
                Err(ApproximationReason::InvalidLevel)
            ));
            let (_, warnings) = l.finish(&mut p).unwrap();
            assert_eq!(warnings.iter().filter(|w| matches!(w, EncodeWarning::ContentLost{kind,..} if kind == "list_marker_conflict")).count(), usize::from(different));
            assert!(!warnings
                .iter()
                .any(|w| matches!(w, EncodeWarning::ListNumberingSplit { .. })));
        }
    }
}

fn store_with_deep_paragraph(bullet: bool, heading_level: u32) -> HwpxStyleStore {
    let mut s = store();
    if bullet {
        s.push_bullet(BulletDef {
            id: 1,
            bullet_char: "•".into(),
            checked_char: None,
            use_image: false,
            para_head: ParaHead {
                start: 1,
                level: 1,
                num_format: NumberFormatType::Digit,
                text: "".into(),
                checkable: false,
            },
        });
        for id in [0, 1] {
            s.para_shape_mut(ParaShapeIndex::new(id)).unwrap().heading_type = HeadingType::Bullet;
        }
    } else {
        // A numbering paragraph is a list use only when its own head exists.
        let mut d = s.numbering_at(0).unwrap().clone();
        d.levels.push(ParaHead {
            start: 1,
            level: heading_level + 1,
            num_format: NumberFormatType::Digit,
            text: "literal".into(),
            checkable: false,
        });
        let mut value = serde_json::to_value(&s).unwrap();
        value["numberings"][0] = serde_json::to_value(d).unwrap();
        s = serde_json::from_value(value).unwrap();
    }
    // A request on an out-of-range paragraph level is rejected up front, so the deep
    // paragraph shares the definition without one. The stored head already equals the
    // first shape's request; the deep bullet paragraph then matches it exactly.
    let first = paragraph_requirement(&s, &paragraph(0)).unwrap().unwrap();
    set_head(&mut s, bullet, 1, first.key.level, first.metrics).unwrap();
    let mut deep = s.para_shape(ParaShapeIndex::new(0)).unwrap().clone();
    deep.heading_level = heading_level;
    let id = s.push_para_shape(deep);
    assert_eq!(id.get(), 4);
    s
}

// 이것을 실패시키는 것: 방출 문단의 수준 검사를 빼고 선택된 head 수준만 검사(글머리표가 정의를 나눔).
// 이것을 실패시키는 것: 문단 수준 상한을 10까지 허용.
// 이것을 실패시키는 것: 방출하지 않은 문단의 범위 밖 수준으로 정의를 근사 처리.
// 이것을 실패시키는 것: 충돌 사유에 head 범위만 적고 문단 범위를 빼기.
#[test]
fn emitted_paragraph_levels_outside_the_range_keep_the_first_use_projection() {
    // (emitted, different layouts, split warnings, conflict warnings)
    let cases =
        [(true, true, 0, 1), (true, false, 0, 0), (false, true, 1, 0), (false, false, 0, 0)];
    for bullet in [false, true] {
        for heading_level in [10, 11] {
            for (emitted, different, splits, conflicts) in cases {
                let mut s = store_with_deep_paragraph(bullet, heading_level);
                if !different {
                    s.list_marker_layouts.insert(1, s.list_marker_layouts[&0]);
                }
                let mut l = ListLedger::new(&s).unwrap().unwrap();
                let mut p = if emitted {
                    record(&mut l, &[0, 1, 4])
                } else {
                    let p = record(&mut l, &[0, 1]);
                    // Recorded, but its paragraph never reaches the section XML.
                    assert_eq!(l.record(&paragraph(4), &[]).unwrap(), Some(2));
                    p
                };
                let (_, warnings) = l.finish(&mut p).unwrap();
                let case = format!("bullet={bullet} level={heading_level} emitted={emitted} different={different}: {warnings:?}");
                assert_eq!(
                    warnings
                        .iter()
                        .filter(|w| matches!(w, EncodeWarning::ListNumberingSplit { .. }))
                        .count(),
                    splits,
                    "{case}"
                );
                let reasons: Vec<_> = warnings
                    .iter()
                    .filter_map(|w| match w {
                        EncodeWarning::ContentLost { kind, reason, .. }
                            if kind == "list_marker_conflict" =>
                        {
                            Some(reason)
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(reasons.len(), conflicts, "{case}");
                // The reason names the paragraph range, not only the head range.
                assert!(
                    reasons.iter().all(|r| r.contains("paragraph level is outside 0..=9")),
                    "{case}"
                );
            }
        }
    }
}

// 이것을 실패시키는 것: 양수 intent 진단을 정의 전체에서 한 번으로 접거나 경로마다 다르게 집계.
// 이것을 실패시키는 것: 무요청·0 intent에도 후속 줄 경고를 생성.
#[test]
fn positive_intent_is_diagnosed_once_per_level_in_both_paths() {
    assert!(crate::list_marker_layout::continuation_warning(
        ParaShapeIndex::new(0),
        4100,
        3600,
        0,
        true
    )
    .is_none());
    assert!(crate::list_marker_layout::continuation_warning(
        ParaShapeIndex::new(0),
        4100,
        3600,
        500,
        false
    )
    .is_none());
    let mut value = serde_json::to_value(store()).unwrap();
    value["numberings"][0]["levels"].as_array_mut().unwrap().push(serde_json::json!({"level":2,"start":1,"num_format":"Digit","text":"^2.","checkable":false}));
    value["para_shapes"][1]["heading_level"] = 1.into();
    for id in [0, 1] {
        value["para_shapes"][id]["indent"]["value"] = 500.into();
    }
    value["list_marker_layouts"]["1"] = value["list_marker_layouts"]["0"].clone();
    let s: HwpxStyleStore = serde_json::from_value(value).unwrap();
    let requirements: Vec<_> = [0, 1, 0, 1]
        .into_iter()
        .map(|id| {
            (ParaShapeIndex::new(id), paragraph_requirement(&s, &paragraph(id)).unwrap().unwrap())
        })
        .collect();
    let (base_warnings, _) = crate::list_marker_layout::project_first_uses(
        &mut s.clone(),
        requirements.iter().map(|(id, r)| (*id, r)),
        None,
    )
    .unwrap();
    let mut l = ListLedger::new(&s).unwrap().unwrap();
    let mut p = record(&mut l, &[0, 1, 0, 1]);
    let (_, split_warnings) = l.finish(&mut p).unwrap();
    let (approximate_warnings, _) = crate::list_marker_layout::project_first_uses(
        &mut s.clone(),
        requirements.iter().map(|(id, r)| (*id, r)),
        Some("unsupported paragraph placement"),
    )
    .unwrap();
    // Each used level reports its own positive intent, so two levels give two warnings.
    for warnings in [base_warnings, split_warnings, approximate_warnings] {
        assert_eq!(warnings.iter().filter(|w| matches!(w, EncodeWarning::ContentLost{kind,..} if kind == "list_marker_continuation")).count(), 2);
    }
}

// 이것을 실패시키는 것: 빈 근사 사유를 구분자 사이에 넣거나 head 비교 없이 근사 경고를 생성.
#[test]
fn approximation_messages_contain_only_nonempty_details() {
    let s = store();
    for reason in ["", "unsupported paragraph placement"] {
        for different in [false, true] {
            let requirements: Vec<_> = [0, usize::from(different)]
                .into_iter()
                .map(|id| {
                    (
                        ParaShapeIndex::new(id),
                        paragraph_requirement(&s, &paragraph(id)).unwrap().unwrap(),
                    )
                })
                .collect();
            let (warnings, has_conflicts) = crate::list_marker_layout::project_first_uses(
                &mut s.clone(),
                requirements.iter().map(|(id, r)| (*id, r)),
                Some(reason),
            )
            .unwrap();
            assert_eq!(has_conflicts, different);
            assert_eq!(warnings.len(), usize::from(different));
            for warning in warnings {
                if let EncodeWarning::ContentLost { reason, .. } = warning {
                    assert!(!reason.contains("; ;"), "{reason}");
                    assert!(!reason.contains(": ;"), "{reason}");
                    assert!(reason.contains("versus"), "{reason}");
                }
            }
        }
    }
}
// 이것을 실패시키는 것: 근사 투영에서 양수 intent 경고나 두 head·이동·사용 수를 제거.
#[test]
fn approximation_uses_shared_projection_diagnostics() {
    let mut s = store();
    s.para_shape_mut(ParaShapeIndex::new(0)).unwrap().indent =
        ParagraphMeasure::from_hwp_unit(HwpUnit::new(500).unwrap());
    let mut l = ListLedger::new(&s).unwrap().unwrap();
    l.push_scope(ListScope::Unsupported);
    let mut p = record(&mut l, &[0, 1]);
    let (_, w) = l.finish(&mut p).unwrap();
    assert_eq!(w.iter().filter(|w| matches!(w, EncodeWarning::ContentLost {kind,..} if kind == "list_marker_continuation")).count(), 1);
    let reason = w
        .iter()
        .find_map(|w| match w {
            EncodeWarning::ContentLost { kind, reason, .. } if kind == "list_marker_conflict" => {
                Some(reason)
            }
            _ => None,
        })
        .unwrap();
    assert!(
        reason.contains("charPrIDRef=0")
            && reason.contains("charPrIDRef=1")
            && reason.contains("2 uses")
            && reason.contains("versus"),
        "{reason}"
    );
}
// 이것을 실패시키는 것: 정의 예산 검사를 배정 루프 끝으로 미룸.
#[test]
fn definition_budget_stops_before_visiting_the_remaining_uses() {
    let mut l = ListLedger::new(&store()).unwrap().unwrap();
    l.budget.definitions = 0;
    record(&mut l, &[0, 1, 0, 1]);
    // An invalid tail proves allocation returns at the first excess segment.
    let plan = l.plan(DefinitionKey { bullet: false, position: 1 }, &[0, 1, usize::MAX]).unwrap();
    assert!(matches!(plan, Err(ApproximationReason::Budget)));
    let mut bounded = ListLedger::new(&store()).unwrap().unwrap();
    bounded.budget.definitions = 0;
    let mut p = record(&mut bounded, &[0, 1, 0, 1]);
    let (out, w) = bounded.finish(&mut p).unwrap();
    let mut fallback = ListLedger::new(&store()).unwrap().unwrap();
    fallback.push_scope(ListScope::Unsupported);
    let mut q = record(&mut fallback, &[0, 1, 0, 1]);
    let (expected, _) = fallback.finish(&mut q).unwrap();
    assert_eq!(p[0].xml, q[0].xml);
    assert_eq!(serde_json::to_value(out).unwrap(), serde_json::to_value(expected).unwrap());
    assert_eq!(w.iter().filter(|w| matches!(w, EncodeWarning::ContentLost {kind,..} if kind == "list_marker_conflict")).count(), 1);
}
// 이것을 실패시키는 것: ^1을 ^10의 일부로 읽거나 숫자 리터럴 접미를 거부.
#[test]
fn decimal_width_matches_whole_level_tokens() {
    assert_eq!(decimal_width("^10.", 1, 99, 1100).unwrap(), None);
    assert_eq!(decimal_width("^10.", 10, 99, 1100).unwrap(), Some(1454));
    assert_eq!(decimal_width("^19.", 1, 99, 1100).unwrap(), Some(2060));
}
// 이것을 실패시키는 것: setter를 우회해 기본 metrics를 fidelity에 남기거나 잘못된 정렬을 허용.
#[test]
fn assigned_heads_use_setter_validation_and_normalization() {
    let mut s = store();
    set_head(&mut s, false, 1, 1, HeadMetrics::generated(1)).unwrap();
    assert!(!s.header_fidelity.numbering.contains_key(&1));
    let mut invalid_metrics = HeadMetrics::generated(1);
    invalid_metrics.align = "JUSTIFY".into();
    assert!(set_head(&mut s, false, 1, 1, invalid_metrics).is_err());
    assert!(set_head(&mut s, false, 99, 1, HeadMetrics::generated(1)).is_err());
}

// 이것을 실패시키는 것: 깊은 수준 허용과 함께 미지원 자리·0 시작·공유 캡션·예산 보호를 제거.
#[test]
fn deep_splits_keep_other_approximation_guards() {
    for reason in [
        ApproximationReason::Unsupported,
        ApproximationReason::ZeroStart,
        ApproximationReason::SharedCaption,
        ApproximationReason::Budget,
    ] {
        let mut s = store();
        let mut def = s.numbering_at(0).unwrap().clone();
        def.levels = (1..=3)
            .map(|level| ParaHead {
                level,
                start: 1,
                text: format!("^{level}."),
                num_format: NumberFormatType::Digit,
                checkable: false,
            })
            .collect();
        if reason == ApproximationReason::ZeroStart {
            def.levels[0].start = 0;
        }
        let mut value = serde_json::to_value(&s).unwrap();
        value["numberings"][0] = serde_json::to_value(def).unwrap();
        s = serde_json::from_value(value).unwrap();
        for id in [0, 1] {
            s.para_shape_mut(ParaShapeIndex::new(id)).unwrap().heading_level = 2;
        }
        let mut ledger = ListLedger::new(&s).unwrap().unwrap();
        let mut parts = record(&mut ledger, &[0, 1]);
        match reason {
            ApproximationReason::Unsupported => ledger.uses[0].unsupported = true,
            ApproximationReason::SharedCaption => {
                ledger.uses[0].caption = Some(0);
                ledger.uses[1].cells.push(0);
            }
            ApproximationReason::Budget => ledger.budget.definitions = 0,
            _ => {}
        }
        let (out, warnings) = ledger.finish(&mut parts).unwrap();
        assert_eq!(out.numbering_count(), 2, "{reason:?}");
        assert_eq!(warnings.iter().filter(|w| matches!(w,
            EncodeWarning::ContentLost { kind, reason: message, .. } if kind == "list_marker_conflict" && message.contains(reason.message())
        )).count(), 1, "{reason:?}");
        assert!(!warnings.iter().any(|w| matches!(w, EncodeWarning::ListNumberingSplit { .. })));
    }
}
