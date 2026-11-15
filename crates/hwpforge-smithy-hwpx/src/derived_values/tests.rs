use super::*;

// Build effects before checking stored results.
fn check_derived_values(facts: &DerivedFacts<'_>, edits: &[DerivedEdit<'_>]) -> HwpxResult<()> {
    facts.effects(edits)?.check_derived_values()
}

const NS: &str = "http://www.hancom.co.kr/hwpml/2011/paragraph";
fn p(content: &str) -> String {
    format!("<hp:p><hp:run>{content}</hp:run></hp:p>")
}
fn section(content: &str) -> String {
    format!("<hs:sec xmlns:hs='http://www.hancom.co.kr/hwpml/2011/section' xmlns:hp='{NS}'>{content}</hs:sec>")
}
fn facts(xmls: &[String]) -> DerivedFacts<'static> {
    DerivedFacts::from_parts(
        xmls.iter()
            .enumerate()
            .map(|(s, xml)| {
                let source = XmlSource::read(xml).unwrap().1;
                let flow = ParagraphFlow::from_source(&source);
                Part::read(Some(s), xml.clone(), source, flow, None).unwrap()
            })
            .collect(),
    )
}
fn table(height: &str, content: &str) -> String {
    format!("<hp:tbl id='7' textWrap='TOP_AND_BOTTOM' textFlow='BOTH_SIDES' pageBreak='CELL' repeatHeader='1' noAdjust='0'><hp:sz width='9000' widthRelTo='ABSOLUTE' height='{height}' heightRelTo='ABSOLUTE' protect='0'/><hp:pos treatAsChar='0' affectLSpacing='0' flowWithText='1' allowOverlap='0' holdAnchorAndSO='0' vertRelTo='PARA' horzRelTo='COLUMN' vertAlign='TOP' horzAlign='LEFT' vertOffset='0' horzOffset='0'/><hp:tr><hp:tc><hp:subList textDirection='HORIZONTAL' lineWrap='BREAK' textWidth='0' textHeight='0' hasTextRef='0' hasNumRef='0' linkListIDRef='0' linkListNextIDRef='0'>{}</hp:subList><hp:cellSpan rowSpan='1' colSpan='1'/><hp:cellSz width='9000' height='0'/></hp:tc></hp:tr></hp:tbl>",p(content))
}
fn field(kind: &str, params: &[(&str, &str)], display: &str) -> String {
    format!("<hp:ctrl><hp:fieldBegin id='10' type='{kind}' dirty='0'><hp:parameters>{}</hp:parameters></hp:fieldBegin></hp:ctrl><hp:t>{display}</hp:t><hp:ctrl><hp:fieldEnd beginIDRef='10'/></hp:ctrl>", params.iter().map(|(k,v)|format!("<hp:stringParam name='{k}'>{v}</hp:stringParam>")).collect::<String>())
}

// 이것을 실패시키는 것: 영향 여부 무시, 시작점 정렬 생략, 중첩 범위의 최대 끝점 누락, 접촉 경계의 겹침 처리.
#[test]
fn affected_table_index_matches_exhaustive_overlap() {
    let tables: Vec<_> = [7..8, 1..12, 3..5, 1..2, 12..14, 7..8, 5..7]
        .into_iter()
        .map(|outer| table_ledger::FlowTable { outer, host: None, parent: None, boundary: 0 })
        .collect();
    for mask in 0..1 << tables.len() {
        let affected: Vec<_> = (0..tables.len()).map(|t| mask & (1 << t) != 0).collect();
        let index = AffectedTables::new(affected.clone(), &tables);
        assert_eq!(index.by_table, affected);
        for start in 0..=15 {
            for end in start..=15 {
                let span = start..end;
                let expected = tables.iter().zip(&affected).any(|(table, &affected)| {
                    affected && table.outer.start < span.end && span.start < table.outer.end
                });
                assert_eq!(index.overlaps(&span), expected, "mask={mask}, span={span:?}");
            }
        }
    }
}
fn num(kind: &str) -> String {
    format!("<hp:ctrl><hp:autoNum num='0' numType='{kind}'><hp:autoNumFormat type='DIGIT' userChar='' prefixChar='' suffixChar='' supscript='0'/></hp:autoNum></hp:ctrl>")
}

// 이것을 실패시키는 것: 저장 SUM 무시, LastResult/Command/표시 중 하나만 S로 취급.
#[test]
fn sum_storage_sources_refuse_same_owner() {
    for (params, display) in [
        (vec![("Formula", "=SUM(LEFT)"), ("LastResult", "0")], ""),
        (vec![("Command", "=SUM(LEFT)??%g,;;0")], ""),
        (vec![("Formula", "=SUM(LEFT)")], "0"),
    ] {
        let f = facts(&[section(&p(&table(
            "900",
            &format!("<hp:t>입력</hp:t>{}", field("CALC", &params, display)),
        )))]);
        let offset = f.parts[0].xml.find("입력").unwrap();
        let slots = [(offset..offset + "입력".len(), "변경".into())];
        assert!(check_derived_values(
            &f,
            &[DerivedEdit::Text {
                section: 0,
                changed: &[FlowOrdinal(1)],
                proven: &[true, true],
                slots: &slots
            }]
        )
        .is_err());
    }
}

// 이것을 실패시키는 것: 번호 종류 통합, num=0 누락, 삭제된 carrier 수 대신 count 순증만 비교.
#[test]
fn numbering_changes_are_kind_specific() {
    for (remaining, refused) in [("TABLE", true), ("PICTURE", false)] {
        let f =
            facts(&[section(&format!("{}{}{}", p(&num("TABLE")), p(&num(remaining)), p("끝")))]);
        assert_eq!(
            check_derived_values(
                &f,
                &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
            )
            .is_err(),
            refused
        );
    }
}
// 이것을 실패시키는 것: TOTAL_PAGE carrier를 불명 재시작으로 취급해 다른 종류에 전파.
#[test]
fn total_page_carrier_removal_does_not_restart_object_numbers() {
    let f = facts(&[section(&format!("{}{}", p(&num("TOTAL_PAGE")), p(&num("TABLE"))))]);
    check_derived_values(&f, &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }])
        .unwrap();
}
// 이것을 실패시키는 것: TOTAL_PAGE를 모든 컨테이너로 확대 또는 관측 R을 삭제.
// 이것을 실패시키는 것: 본문 PAGE/TOTAL_PAGE 표시 허용 복원.
#[test]
fn page_results_keep_observed_containers_only() {
    for (content, accepted) in [
        (num("PAGE"), false),
        (num("TOTAL_PAGE"), false),
        (
            format!(
                "<hp:ctrl><hp:footer><hp:subList>{}</hp:subList></hp:footer></hp:ctrl>",
                p(&num("TOTAL_PAGE"))
            ),
            false,
        ),
    ] {
        let f = facts(&[section(&format!("{}{}", p("삭제"), p(&content)))]);
        assert_eq!(
            check_derived_values(
                &f,
                &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
            )
            .is_ok(),
            accepted
        );
    }
}

// 이것을 실패시키는 것: tbl@id로 소유자 결합, 전역 SUM 거부, 삭제한 결과의 최신성 검사.
#[test]
fn local_sum_survives_unrelated_table_and_body_changes() {
    let calculation = field("FORMULA", &[("Formula", "=SUM(A1:B2)"), ("LastResult", "3")], "3");
    let f = facts(&[section(&format!(
        "{}{}{}",
        p("삭제"),
        p(&table("9", &calculation)),
        p(&table("9", "다른 표"))
    ))]);
    for ordinal in [0, 3] {
        check_derived_values(
            &f,
            &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(ordinal)] }],
        )
        .unwrap();
    }
    for params in [
        vec![("Command", "=SUM(LEFT)??%g,;;3")],
        vec![("Command", "=SUM(LEFT)")],
        vec![("Formula", "=SUM(LEFT)"), ("Command", "=SUM(LEFT)??%g,;;3"), ("ResultFormat", "%g,")],
    ] {
        let xml =
            section(&format!("{}{}", p("削除"), p(&table("9", &field("FORMULA", &params, "3")))));
        check_derived_values(
            &facts(&[xml]),
            &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }],
        )
        .unwrap();
    }
    let f = facts(&[section(&format!("{}{}", p(&table("9", &calculation)), p("끝")))]);
    check_derived_values(&f, &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }])
        .unwrap();
    let f = facts(&[section(&p("삭제")), section(&p(&table("9", &calculation)))]);
    check_derived_values(&f, &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }])
        .unwrap();
}
// 이것을 실패시키는 것: SUM 부분 문자열 승인, 충돌 Formula/Command 무시, owner 불명을 무영향 처리.
#[test]
fn unknown_calculations_fail_closed() {
    for params in [
        vec![("Formula", "=X_SUM(LEFT)")],
        vec![("Formula", "=SUM(named)")],
        vec![("Formula", "=SUM(Table2.A1:B2)")],
        vec![("Formula", "=SUM(LEFT)"), ("Command", "=SUM(RIGHT)??%g,;;3")],
        vec![("Command", "=SUM(LEFT)??%g,;;3??unknown")],
    ] {
        let f = facts(&[section(&format!(
            "{}{}",
            p("削除"),
            p(&table("9", &field("FORMULA", &params, "3")))
        ))]);
        assert!(check_derived_values(
            &f,
            &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
        )
        .is_err());
    }
    let f = facts(&[section(&format!(
        "{}{}",
        p("削除"),
        p(&field("FORMULA", &[("Formula", "=SUM(LEFT)")], "3"))
    ))]);
    assert!(check_derived_values(
        &f,
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
    )
    .is_err());
}
// 이것을 실패시키는 것: 바깥 note number 누락, 각주/미주 통합, ON_PAGE/불명 정책 승인.
#[test]
fn outer_note_numbers_and_policies_are_independent() {
    for (note, kind, other) in
        [("footNote", "FOOTNOTE", "endNote"), ("endNote", "ENDNOTE", "footNote")]
    {
        let note_xml = format!(
            "<hp:ctrl><hp:{note} number='0'><hp:subList>{}</hp:subList></hp:{note}></hp:ctrl>",
            p("note")
        );
        let pr = format!("<hp:{note}Pr><hp:numbering type='CONTINUOUS'/></hp:{note}Pr>");
        let body =
            section(&format!("{}{}{}", p(&format!("{pr}{note_xml}")), p(&note_xml), p("끝")));
        let f = facts(&[body]);
        assert!(
            check_derived_values(
                &f,
                &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
            )
            .is_err(),
            "{kind}"
        );
        let f = facts(&[section(&format!("{}{}", p("삭제"), p(&format!("{pr}{note_xml}"))))]);
        check_derived_values(
            &f,
            &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }],
        )
        .unwrap();
        for policy in ["ON_SECTION", "ON_PAGE", "UNKNOWN"] {
            let f = facts(&[f.parts[0].xml.replace("CONTINUOUS", policy)]);
            assert_eq!(
                check_derived_values(
                    &f,
                    &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
                )
                .is_ok(),
                policy == "ON_SECTION"
            );
        }
        let f = facts(&[section(&format!(
            "{}{}{}",
            p(&format!("<hp:ctrl><hp:{other} number='1'/></hp:ctrl>")),
            p(&format!("{pr}{note_xml}")),
            p("끝")
        ))]);
        check_derived_values(
            &f,
            &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }],
        )
        .unwrap();
    }
}
// 이것을 실패시키는 것: PAGE 재시작 R 확대, PAGE 변화가 FOOTNOTE 변화로 전파.
#[test]
fn page_restart_is_not_a_note_restart() {
    for setting in [
        "<hp:newNum numType='FUTURE'/>",
        "<hp:newNum/>",
        "<hp:newNum numType='TOTAL_PAGE'/>",
        "<hp:startNum tbl='9'/>",
        "<hp:autoNum num='1' numType='FUTURE'/>",
    ] {
        let page_only = "<hp:startNum pageStartsOn='BOTH' page='3' pic='0' tbl='0' equation='0'/>";
        let f=facts(&[section(&format!("{}{}",p(page_only),p("<hp:footNotePr><hp:numbering type='CONTINUOUS'/></hp:footNotePr><hp:footNote number='1'/>")))]);
        check_derived_values(
            &f,
            &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }],
        )
        .unwrap();
        let f = facts(&[section(&format!("{}{}", p(setting), p(&num("TABLE"))))]);
        assert!(check_derived_values(
            &f,
            &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
        )
        .is_err());
    }
    let f = facts(&[section(&format!(
        "{}{}{}",
        p("삭제"),
        p("<hp:ctrl><hp:newNum numType='PAGE' num='3'/></hp:ctrl>"),
        p(&num("PAGE"))
    ))]);
    assert!(check_derived_values(
        &f,
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(1)] }]
    )
    .is_err());
    let f=facts(&[section(&format!("{}{}{}",p("<hp:newNum numType='PAGE' num='3'/>"),p("<hp:footNotePr><hp:numbering type='CONTINUOUS'/></hp:footNotePr><hp:footNote number='1'/>"),p("끝")))]);
    check_derived_values(&f, &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }])
        .unwrap();
}
// 이것을 실패시키는 것: 같은 count carrier 교체를 무변경으로 판정, EndOfBody의 전역 흐름 효과 누락.
#[test]
fn insertion_end_and_carrier_replacement_have_flow_effects() {
    let f = facts(&[section(&format!("{}{}", p(&num("TABLE")), p(&num("TABLE"))))]);
    assert!(check_derived_values(
        &f,
        &[
            DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] },
            DerivedEdit::Insert {
                section: 0,
                at: FlowBoundary::EndOfBody,
                fragment: &p(&num("TABLE"))
            }
        ]
    )
    .is_err());
    let f = facts(&[section(&p(&format!(
        "<hp:footer><hp:subList>{}</hp:subList></hp:footer>",
        p(&num("TOTAL_PAGE"))
    )))]);
    assert!(check_derived_values(
        &f,
        &[DerivedEdit::Insert { section: 0, at: FlowBoundary::EndOfBody, fragment: &p("추가") }]
    )
    .is_err());
}

fn try_part(xml: &str) -> HwpxResult<Part<'static>> {
    crate::structural::xml_values::scan_fields(xml, |_, _, _| Ok(()))?;
    let source = XmlSource::read(xml).map_err(|_| invalid())?.1;
    let flow = ParagraphFlow::from_source(&source);
    Part::read(Some(0), xml.into(), source, flow, None)
}
// 이것을 실패시키는 것: 별칭 태그 누락, foreign URI 승인, 짝/parameter/DTD 검증 생략.
#[test]
fn raw_field_collection_is_namespace_and_boundary_aware() {
    let good =
        section(&p(&field("FORMULA", &[("Formula", "=SUM(LEFT)"), ("LastResult", "0")], "0")));
    assert!(try_part(&good.replace("hp:", "alias:").replace("xmlns:hp", "xmlns:alias")).is_ok());
    for xml in [
        good.replace(NS, "urn:foreign"),
        good.replace("beginIDRef='10'", "beginIDRef='11'"),
        good.replace(
            "<hp:stringParam name='LastResult'>0</hp:stringParam>",
            "<hp:stringParam name='Formula'>=SUM(RIGHT)</hp:stringParam>",
        ),
        good.replace("<hp:fieldEnd beginIDRef='10'/>", ""),
        good.replace(
            "<hp:ctrl><hp:fieldBegin",
            "<hp:ctrl><hp:fieldEnd beginIDRef='10'/><hp:fieldBegin",
        ),
        format!("<!DOCTYPE x>{good}"),
        good.replace("dirty='0'", "dirty='0' dirty='1'"),
    ] {
        assert!(try_part(&xml).is_err(), "{xml}");
    }
    let encoded = good.replace("=SUM(LEFT)", "<![CDATA[=SUM(LE]]>FT&#41;");
    assert_eq!(
        try_part(&encoded).unwrap().fields.values().next().unwrap().params["Formula"],
        "=SUM(LEFT)"
    );
    let deep = section(&format!(
        "{}{}",
        "<hp:ctrl>".repeat(crate::decoder::XML_RECURSION_LIMIT),
        "</hp:ctrl>".repeat(crate::decoder::XML_RECURSION_LIMIT)
    ));
    assert!(try_part(&deep).is_err());
}
// 이것을 실패시키는 것: 결과가 있는 비편집 구역/독립 흐름 무시, 통계 SUMMERY/미지 필드 허용.
#[test]
fn remote_and_unknown_results_are_not_silently_skipped() {
    for remote in [
        p(&field("UNKNOWN", &[], "0")),
        p(&field("SUMMERY", &[("Command", "$pagecount")], "3")),
        p(&format!(
            "<hp:ctrl><hp:header><hp:subList>{}</hp:subList></hp:header></hp:ctrl>",
            p(&num("TOTAL_PAGE"))
        )),
        format!("<hp:masterPage>{}</hp:masterPage>", p(&num("TOTAL_PAGE"))),
    ] {
        let f = facts(&[section(&p("삭제")), section(&remote)]);
        assert!(check_derived_values(
            &f,
            &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
        )
        .is_err());
    }
    for kind in ["DATE", "TIME", "PATH", "SUMMERY", "DOC_SUMMARY", "USER_INFO"] {
        let f = facts(&[
            section(&p("삭제")),
            section(&p(&field(kind, &[("Command", "$author")], "고정"))),
        ]);
        check_derived_values(
            &f,
            &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }],
        )
        .unwrap();
    }
    let f = facts(&[section(&format!(
        "{}{}",
        p("삭제"),
        p("<hp:pageNum/><hp:newNum numType='PAGE'/><hp:t>1</hp:t>")
    ))]);
    check_derived_values(&f, &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }])
        .unwrap();
}
// 이것을 실패시키는 것: PAGE/TOTAL_PAGE 형식 gate 제거 또는 PAGE footer R 삭제.
#[test]
fn page_format_and_footer_are_checked_separately() {
    let footer = format!(
        "<hp:ctrl><hp:footer><hp:subList>{}</hp:subList></hp:footer></hp:ctrl>",
        p(&num("PAGE"))
    );
    let good = section(&format!("{}{}", p("삭제"), p(&footer)));
    check_derived_values(
        &facts(std::slice::from_ref(&good)),
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }],
    )
    .unwrap();
    for (from, to) in [
        ("DIGIT", "ROMAN_CAPITAL"),
        ("prefixChar=''", "prefixChar='p'"),
        ("suffixChar=''", "suffixChar='p'"),
        ("userChar=''", "userChar='x'"),
        ("supscript='0'", "supscript='1'"),
        ("numType='PAGE'", "numType='TOTAL_PAGE'"),
    ] {
        assert!(check_derived_values(
            &facts(&[good.replace(from, to)]),
            &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
        )
        .is_err());
    }
}
// 이것을 실패시키는 것: 표시 글자 없는 쪽 참조에서 대상 생존·종류·경로·Command 검사 생략.
// 이것을 실패시키는 것: 흐름 변경 때 본문 쪽 참조 허용 복원.
#[test]
fn bookmark_page_reference_requires_a_surviving_target() {
    let params = [
        ("RefType", "TARGET_BOOKMARK"),
        ("RefContentType", "OBJECT_TYPE_PAGE"),
        ("RefPath", "?mark;"),
        ("Command", "?mark;6;0;0;0;"),
    ];
    let cross = field("CROSSREF", &params, "3");
    let good = section(&format!(
        "{}{}{}",
        p("삭제"),
        p("<hp:ctrl><hp:bookmark name='mark'/></hp:ctrl>"),
        p(&cross)
    ));
    assert!(check_derived_values(
        &facts(std::slice::from_ref(&good)),
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }],
    )
    .is_err());
    assert!(check_derived_values(
        &facts(std::slice::from_ref(&good)),
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(1)] }]
    )
    .is_err());
    // Without displayed text the flow rule does not apply, so the target checks decide.
    let hidden = good.replace("<hp:t>3</hp:t>", "<hp:t></hp:t>");
    check_derived_values(
        &facts(std::slice::from_ref(&hidden)),
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }],
    )
    .unwrap();
    assert!(check_derived_values(
        &facts(std::slice::from_ref(&hidden)),
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(1)] }]
    )
    .is_err());
    for changed in [
        good.replace("OBJECT_TYPE_PAGE", "OBJECT_TYPE_NUMBER"),
        good.replace("OBJECT_TYPE_PAGE", "OBJECT_TYPE_CONTENTS"),
        good.replace("OBJECT_TYPE_PAGE", "OBJECT_TYPE_UPDOWN"),
        good.replace("TARGET_BOOKMARK", "TARGET_OUTLINE"),
        good.replace("?mark;", "?missing;"),
        good.replace("?mark;6;0;0;0;", "?other;0;0;0;0;"),
        section(&format!(
            "{}{}{}",
            p("삭제"),
            p("<hp:bookmark name='mark'/>"),
            p(&format!("<hp:footer><hp:subList>{}</hp:subList></hp:footer>", p(&cross)))
        )),
    ] {
        assert!(check_derived_values(
            &facts(&[changed]),
            &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
        )
        .is_err());
    }
    for (label, changed) in [
        ("number", hidden.replace("OBJECT_TYPE_PAGE", "OBJECT_TYPE_NUMBER")),
        ("contents", hidden.replace("OBJECT_TYPE_PAGE", "OBJECT_TYPE_CONTENTS")),
        ("updown", hidden.replace("OBJECT_TYPE_PAGE", "OBJECT_TYPE_UPDOWN")),
        ("outline", hidden.replace("TARGET_BOOKMARK", "TARGET_OUTLINE")),
        ("missing", hidden.replace("?mark;", "?missing;")),
        ("other", hidden.replace("?mark;6;0;0;0;", "?other;0;0;0;0;")),
        ("path", hidden.replace("'RefPath'>?mark;<", "'RefPath'>mark<")),
        (
            "duplicate",
            hidden.replace(
                "<hp:ctrl><hp:bookmark name='mark'/></hp:ctrl>",
                "<hp:ctrl><hp:bookmark name='mark'/></hp:ctrl><hp:ctrl><hp:bookmark name='mark'/></hp:ctrl>",
            ),
        ),
    ] {
        assert!(
            check_derived_values(
                &facts(&[changed]),
                &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }],
            )
            .is_err(),
            "{label}"
        );
    }
}

// 이것을 실패시키는 것: source/section/ordinal/proof/slot 불일치 또는 중복 요청 승인.
#[test]
fn invalid_edit_coordinates_return_errors() {
    let effect = Effect { deleted: std::iter::once(2..4).collect(), ..Default::default() };
    assert!(effect.survives(&(1..3)).is_err());
    assert!(effect.survives(&(3..5)).is_err());
    assert!(!effect.survives(&(2..4)).unwrap());
    assert!(effect.survives(&(0..2)).unwrap());
    let f = facts(&[section(&format!("{}{}", p("<hp:t>한글</hp:t>"), p("<hp:t>끝</hp:t>")))]);
    let offset = f.parts[0].xml.find("한글").unwrap();
    let slots = [(offset..offset + "한글".len(), "대체".into())];
    let good = [DerivedEdit::Text {
        section: 0,
        changed: &[FlowOrdinal(0)],
        proven: &[false, false],
        slots: &slots,
    }];
    check_derived_values(&f, &good).unwrap();
    let one = [FlowOrdinal(0)];
    let wrong = [FlowOrdinal(1)];
    for edits in [
        vec![DerivedEdit::Text { section: 0, changed: &one, proven: &[], slots: &slots }],
        vec![DerivedEdit::Text {
            section: 0,
            changed: &wrong,
            proven: &[false, false],
            slots: &slots,
        }],
        vec![DerivedEdit::Text { section: 0, changed: &one, proven: &[false, false], slots: &[] }],
        vec![DerivedEdit::Delete { section: 5, paragraphs: &one }],
        vec![DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(8)] }],
        vec![DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0), FlowOrdinal(0)] }],
        vec![DerivedEdit::Insert { section: 0, at: FlowBoundary::EndOfBody, fragment: "" }],
        vec![
            DerivedEdit::Insert { section: 0, at: FlowBoundary::EndOfBody, fragment: &p("a") },
            DerivedEdit::Insert { section: 0, at: FlowBoundary::EndOfBody, fragment: &p("b") },
        ],
    ] {
        assert!(check_derived_values(&f, &edits).is_err());
    }
    for span in [offset + 1..offset + 3, 0..usize::MAX, offset..offset] {
        let invalid_slots = [(span, "변경".into())];
        assert!(check_derived_values(
            &f,
            &[DerivedEdit::Text {
                section: 0,
                changed: &one,
                proven: &[false, false],
                slots: &invalid_slots
            }]
        )
        .is_err());
    }
    assert!(check_derived_values(&f, &[]).is_ok());
}
// 이것을 실패시키는 것: 선필터 음성으로 별칭/반대 짝/차트/불명 namespace/DTD를 건너뜀.
#[test]
fn negative_prefilter_requires_validated_plain_source() {
    let plain = section(&p("<hp:t>평문</hp:t>"));
    assert!(safe_negative(&plain));
    for xml in [
        section(&p("<hp:fieldEnd beginIDRef='1'/>")),
        section(&p(&table("0", "칸"))),
        section(&p("<hp:numCache/>")),
        section(&p("<hp:opaqueResult/>")),
        plain.replace("2011/section", "foreign"),
        plain.replace("<hp:t>", "<hp:t xmlns:hp='urn:foreign'>"),
        format!("<!DOCTYPE x>{plain}"),
        "<sec><p>plain</p></sec>".into(),
    ] {
        assert!(!safe_negative(&xml), "{xml}");
    }
}

// 이것을 실패시키는 것: 구역 재시작 각주를 문서 전체 번호 변화와 합침, 연속 번호를 구역 안으로 축소.
#[test]
fn note_numbering_respects_section_restart_policy() {
    for (policy, accepted) in [("CONTINUOUS", false), ("ON_SECTION", true)] {
        let remote=section(&p(&format!("<hp:footNotePr><hp:numbering type='{policy}'/></hp:footNotePr><hp:footNote number='1'/>")));
        let f =
            facts(&[section(&format!("{}{}", p("<hp:footNote number='1'/>"), p("끝"))), remote]);
        assert_eq!(
            check_derived_values(
                &f,
                &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
            )
            .is_ok(),
            accepted
        );
    }
}
// 이것을 실패시키는 것: header/footer 내부 seed가 바깥 host 사슬을 타게 하거나 소유 불명 표를 영향 집합에서 뺌.
#[test]
fn independent_seeds_and_unhosted_tables_are_conservative() {
    for container in ["header", "footer"] {
        let f = facts(&[section(&format!(
            "{}{}",
            p(&format!(
                "<hp:ctrl><hp:{container}><hp:subList>{}</hp:subList></hp:{container}></hp:ctrl>",
                p(&table("10", "안"))
            )),
            p(&table("20", "밖"))
        ))]);
        let flow = &f.parts[0].flow;
        let mut seeds = vec![false; flow.paragraph_count()];
        seeds[f.parts[0].tables.tables[0].host.unwrap()] = true;
        assert_eq!(f.parts[0].tables.affected_tables(flow, &seeds), [true, false]);
    }
}
// 이것을 실패시키는 것: 표 원문 주소 대신 같은 id의 첫 표를 소유자로 선택.
#[test]
fn equal_table_ids_do_not_merge_owners() {
    let formula = field("FORMULA", &[("Formula", "=SUM(LEFT)"), ("LastResult", "0")], "");
    let f =
        facts(&[section(&format!("{}{}", p(&table("10", "다른 표")), p(&table("20", &formula))))]);
    check_derived_values(&f, &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }])
        .unwrap();
}
// 이것을 실패시키는 것: 부분 삭제 필드를 완전 삭제로 취급하거나 완전 삭제 결과를 최신성 거부.
#[test]
fn field_results_require_complete_deletion() {
    let begin = "<hp:ctrl><hp:fieldBegin id='10' type='CLICK_HERE'/></hp:ctrl><hp:t>앞</hp:t>";
    let end = "<hp:t>뒤</hp:t><hp:ctrl><hp:fieldEnd beginIDRef='10'/></hp:ctrl>";
    let f = facts(&[section(&format!("{}{}{}", p(begin), p(end), p("끝")))]);
    assert!(check_derived_values(
        &f,
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
    )
    .is_err());
    check_derived_values(
        &f,
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0), FlowOrdinal(1)] }],
    )
    .unwrap();
}
// 이것을 실패시키는 것: 차트 캐시를 미지 결과로 일괄 거부 또는 namespace를 무시하고 승인.
#[test]
fn unchanged_chart_caches_are_not_body_results() {
    let chart="<c:chartSpace xmlns:c='http://schemas.openxmlformats.org/drawingml/2006/chart'><c:numCache/><c:strCache/></c:chartSpace>".to_owned();
    let f = facts(&[section(&p("삭제")), chart.clone()]);
    check_derived_values(&f, &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }])
        .unwrap();
    assert!(try_part(&chart.replace("drawingml/2006/chart", "wrong")).is_err());
}

// 이것을 실패시키는 것: 조상 컨테이너의 local-name만 보고 재계산 R을 승인.
#[test]
fn result_recalculation_requires_bound_ancestor_containers() {
    let good = section(&format!("{}{}", p("삭제"), p(&num("PAGE"))));
    for (open, close) in [("p", "p"), ("run", "run"), ("ctrl", "ctrl")] {
        let xml = good
            .replace(&format!("<hp:{open}>"), &format!("<x:{open} xmlns:x='urn:foreign'>"))
            .replace(&format!("</hp:{close}>"), &format!("</x:{close}>"));
        let f = facts(&[xml]);
        assert!(check_derived_values(
            &f,
            &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
        )
        .is_err());
    }
}
// 이것을 실패시키는 것: 구조 변경의 전역 거부, 빈 표시/대상 종류 혼동, 내용 참조의 대상 변경 검사 생략.
#[test]
fn object_number_references_use_the_actual_kind_delta() {
    let params = [
        ("RefType", "TARGET_TABLE"),
        ("RefContentType", "OBJECT_TYPE_NUMBER"),
        ("RefPath", "?#7;"),
        ("Command", "?#7;0;1;0;0;"),
    ];
    let cross = field("CROSSREF", &params, "1");
    let good =
        section(&format!("{}{}{}", p("삭제"), p(&table("0", "<hp:t>대상</hp:t>")), p(&cross)));
    check_derived_values(
        &facts(std::slice::from_ref(&good)),
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }],
    )
    .unwrap();
    let delta =
        good.replacen("<hp:run>삭제</hp:run>", &format!("<hp:run>{}</hp:run>", num("TABLE")), 1);
    assert!(check_derived_values(
        &facts(std::slice::from_ref(&delta)),
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
    )
    .is_err());
    let empty = delta.replace("<hp:t>1</hp:t>", "<hp:t></hp:t>");
    check_derived_values(
        &facts(&[empty]),
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }],
    )
    .unwrap();
    let content = good
        .replace("OBJECT_TYPE_NUMBER", "OBJECT_TYPE_CONTENTS")
        .replace("?#7;0;1;0;0;", "?#7;0;2;0;0;");
    check_derived_values(
        &facts(std::slice::from_ref(&content)),
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }],
    )
    .unwrap();
    let f = facts(&[content]);
    let at = f.parts[0].xml.find("대상").unwrap();
    let slots = [(at..at + "대상".len(), "바꿈".into())];
    let edits = [DerivedEdit::Text {
        section: 0,
        changed: &[FlowOrdinal(2)],
        proven: &[false; 4],
        slots: &slots,
    }];
    let effects = f.effects(&edits).expect("내용 참조의 좌표는 먼저 유효해야 한다");
    assert!(effects.check_derived_values().is_err());
    assert!(check_derived_values(
        &facts(&[good
            .replace("TARGET_TABLE", "TARGET_FIGURE")
            .replace("?#7;0;1;0;0;", "?#7;1;1;0;0;")]),
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
    )
    .is_err());
}
// 이것을 실패시키는 것: 같은 이름의 입력 필드를 책갈피 대상으로 승인.
#[test]
fn named_input_is_not_a_bookmark_target() {
    let params = [
        ("RefType", "TARGET_BOOKMARK"),
        ("RefContentType", "OBJECT_TYPE_PAGE"),
        ("RefPath", "?mark;"),
        ("Command", "?mark;6;0;0;0;"),
    ];
    let input = field("CLICK_HERE", &[], "입력")
        .replace("id='10'", "id='11'")
        .replace("beginIDRef='10'", "beginIDRef='11'")
        .replace("type='CLICK_HERE'", "type='CLICK_HERE' name='mark'");
    let f = facts(&[section(&format!(
        "{}{}{}",
        p("삭제"),
        p(&input),
        p(&field("CROSSREF", &params, "1"))
    ))]);
    assert!(check_derived_values(
        &f,
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
    )
    .is_err());
}

// 이것을 실패시키는 것: 필드마다 원문을 다시 읽거나 원장/흐름을 필드 수만큼 다시 수집.
#[test]
fn shared_collection_work_grows_with_source_size() {
    let measure = |n: usize| {
        let mut body = p("삭제");
        for _ in 0..n {
            body.push_str(&p(&table("20", &field("FORMULA", &[("Formula", "=SUM(LEFT)")], "3"))));
        }
        let xml = section(&body);
        crate::work::WORK.with(|w| w.set(0));
        let before = hwpforge_core::source_xml::source_work();
        let f = facts(&[xml]);
        let edits = [DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }];
        let effects = f.effects(&edits).unwrap();
        effects.check_derived_values().unwrap();

        assert_eq!(hwpforge_core::source_xml::source_work().0 - before.0, 1);
        crate::work::WORK.with(|w| w.get())
    };
    let small = measure(10);
    let large = measure(20);
    assert!(small > 0 && large > small && large < small * 3);
}

// 이것을 실패시키는 것: 저장한 정책 대신 번호 표현마다 collect_note_policies()로 전체 요소 재순회.
#[test]
fn note_policy_work_scales_with_each_number_representation() {
    for (note, kind, ref_type, command) in [
        ("footNote", "FOOTNOTE", "TARGET_FOOTNOTE", "?#7;3;1;0;0;"),
        ("endNote", "ENDNOTE", "TARGET_ENDNOTE", "?#7;4;1;0;0;"),
    ] {
        for representation in ["number", "autoNum", "reference"] {
            let measure = |n| {
                let mut body = p("삭제");
                body.push_str(&p(&format!(
                    "<hp:{note}Pr><hp:numbering type='CONTINUOUS'/></hp:{note}Pr><hp:{note} id='7' number='1'/>"
                )));
                for index in 0..n {
                    let content = match representation {
                        "number" => format!("<hp:{note} number='1'/>"),
                        "autoNum" => num(kind),
                        _ => field(
                            "CROSSREF",
                            &[
                                ("RefType", ref_type),
                                ("RefContentType", "OBJECT_TYPE_NUMBER"),
                                ("RefPath", "?#7;"),
                                ("Command", command),
                            ],
                            "1",
                        )
                        .replace("id='10'", &format!("id='{}'", index + 10))
                        .replace("beginIDRef='10'", &format!("beginIDRef='{}'", index + 10)),
                    };
                    body.push_str(&p(&content));
                }
                crate::work::WORK.with(|w| w.set(0));
                let f = facts(&[section(&body)]);
                let edits = [DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }];
                let effects = f.effects(&edits).unwrap();
                effects.check_derived_values().unwrap();
                crate::work::WORK.with(|w| w.get())
            };
            let small = measure(160);
            let large = measure(320);
            eprintln!("{note} {representation}: {small} -> {large}");
            assert!(
                small > 0 && large > small && large < small * 3,
                "{note} {representation}: {small} -> {large}"
            );
        }
    }
}

// 이것을 실패시키는 것: Command의 wire 번호와 의미 parameter의 충돌을 R로 승인.
// 이것을 실패시키는 것: 흐름 변경 때 본문 쪽 참조 허용 복원.
#[test]
fn cross_reference_command_agrees_with_typed_parameters() {
    let params = [
        ("RefType", "TARGET_BOOKMARK"),
        ("RefContentType", "OBJECT_TYPE_PAGE"),
        ("RefPath", "?mark;"),
        ("Command", "?mark;6;0;0;0;"),
    ];
    let good = section(&format!(
        "{}{}{}",
        p("삭제"),
        p("<hp:bookmark name='mark'/>"),
        p(&field("CROSSREF", &params, "3"))
    ));
    let edit = [DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }];
    assert!(check_derived_values(&facts(std::slice::from_ref(&good)), &edit).is_err());
    // Without displayed text the flow rule does not apply, so the command check decides.
    let hidden = good.replace("<hp:t>3</hp:t>", "<hp:t></hp:t>");
    check_derived_values(&facts(std::slice::from_ref(&hidden)), &edit).unwrap();
    for command in ["?mark;0;0;0;0;", "?mark;6;1;0;0;", "?mark;6;2;0;0;"] {
        assert!(check_derived_values(&facts(&[hidden.replace("?mark;6;0;0;0;", command)]), &edit)
            .is_err());
    }
}

// 이것을 실패시키는 것: 흐름 변경 때 머리말/꼬리말/바탕쪽/각주/미주/메모 책갈피를 가리키는 쪽 참조 표시 허용.
#[test]
fn page_reference_targets_require_observed_body_flows() {
    let params = [
        ("RefType", "TARGET_BOOKMARK"),
        ("RefContentType", "OBJECT_TYPE_PAGE"),
        ("RefPath", "?mark;"),
        ("Command", "?mark;6;0;0;0;"),
    ];
    let reference = field("CROSSREF", &params, "3");
    for (container, attrs, policy) in [
        ("header", "", ""),
        ("footer", "", ""),
        ("masterPage", "", ""),
        (
            "footNote",
            " number='1'",
            "<hp:footNotePr><hp:numbering type='CONTINUOUS'/></hp:footNotePr>",
        ),
        (
            "endNote",
            " number='1'",
            "<hp:endNotePr><hp:numbering type='CONTINUOUS'/></hp:endNotePr>",
        ),
    ] {
        let target=p(&format!("{policy}<hp:ctrl><hp:{container}{attrs}><hp:subList>{}</hp:subList></hp:{container}></hp:ctrl>",p("<hp:bookmark name='mark'/>")));
        let f = facts(&[section(&format!("{}{}{}", p("삭제"), target, p(&reference)))]);
        assert!(
            check_derived_values(
                &f,
                &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
            )
            .is_err(),
            "{container}"
        );
    }
    let memo=p(&format!("<hp:ctrl><hp:fieldBegin id='20' type='MEMO'><hp:subList>{}</hp:subList></hp:fieldBegin></hp:ctrl><hp:ctrl><hp:fieldEnd beginIDRef='20'/></hp:ctrl>",p("<hp:bookmark name='mark'/>")));
    let f = facts(&[section(&format!("{}{}{}", p("삭제"), memo, p(&reference)))]);
    assert!(check_derived_values(
        &f,
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
    )
    .is_err());
}
// 이것을 실패시키는 것: sec/p/run 모양만 보고 비구역 part의 쪽 결과/책갈피를 관측된 본문 R로 승인.
#[test]
fn page_recalculation_requires_registered_section_parts() {
    let mut f = facts(&[section(&p("삭제")), section(&p(&num("PAGE")))]);
    f.parts[1].section = None;
    assert!(check_derived_values(
        &f,
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
    )
    .is_err());
    let params = [
        ("RefType", "TARGET_BOOKMARK"),
        ("RefContentType", "OBJECT_TYPE_PAGE"),
        ("RefPath", "?mark;"),
        ("Command", "?mark;6;0;0;0;"),
    ];
    let reference = p(&field("CROSSREF", &params, "3"));
    let mut f = facts(&[
        section(&format!("{}{}", p("삭제"), p("<hp:bookmark name='mark'/>"))),
        section(&reference),
    ]);
    f.parts[1].section = None;
    assert!(check_derived_values(
        &f,
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
    )
    .is_err());
    let mut f = facts(&[
        section(&format!("{}{}", p("삭제"), reference)),
        section(&p("<hp:bookmark name='mark'/>")),
    ]);
    f.parts[1].section = None;
    assert!(check_derived_values(
        &f,
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
    )
    .is_err());
}

// Synthetic XML with independently stated observed values; no saved application bytes.
pub(crate) fn inline_table(cell_height: &str, no_adjust: &str, merged: bool) -> String {
    let rows = if merged { 3 } else { 2 };
    let horizontal = if merged { "PARA" } else { "COLUMN" };
    let mut xml = format!("<hp:tbl id='7' zOrder='0' numberingType='TABLE' textWrap='TOP_AND_BOTTOM' textFlow='BOTH_SIDES' lock='0' dropcapstyle='None' pageBreak='CELL' repeatHeader='1' rowCnt='{rows}' colCnt='3' cellSpacing='0' noAdjust='{no_adjust}'><hp:sz width='48189' widthRelTo='ABSOLUTE' height='10000' heightRelTo='ABSOLUTE' protect='0'/><hp:pos treatAsChar='1' affectLSpacing='0' flowWithText='1' allowOverlap='0' holdAnchorAndSO='0' vertRelTo='PARA' horzRelTo='{horizontal}' vertAlign='TOP' horzAlign='LEFT' vertOffset='0' horzOffset='0'/><hp:outMargin left='283' right='283' top='283' bottom='1000'/><hp:inMargin left='510' right='510' top='141' bottom='141'/>");
    for row in 0..rows {
        xml.push_str("<hp:tr>");
        for column in 0..3 {
            if merged
                && (row == 0 && column == 1 || row == 1 && column < 2 || row == 2 && column == 1)
            {
                continue;
            }
            let col_span = if merged && column == 0 { 2 } else { 1 };
            let row_span = if merged && row == 0 && column == 0 { 2 } else { 1 };
            let width = if col_span == 2 { 32126 } else { 16063 };
            let alignment = if !merged && row == 0 && column == 2 { "BOTTOM" } else { "CENTER" };
            xml.push_str(&format!("<hp:tc header='0' hasMargin='0' protect='0' editable='0' dirty='0'><hp:subList textDirection='HORIZONTAL' lineWrap='BREAK' vertAlign='{alignment}' textWidth='0' textHeight='0' hasTextRef='0' hasNumRef='0' linkListIDRef='0' linkListNextIDRef='0'>{}</hp:subList><hp:cellAddr rowAddr='{row}' colAddr='{column}'/><hp:cellSpan rowSpan='{row_span}' colSpan='{col_span}'/><hp:cellSz width='{width}' height='{cell_height}'/><hp:cellMargin left='510' right='510' top='141' bottom='141'/></hp:tc>",p("칸")));
        }
        xml.push_str("</hp:tr>");
    }
    xml.push_str("</hp:tbl>");
    xml
}

// 이것을 실패시키는 것: 시작 주소 맵 대신 매 요청마다 선형 탐색, 맵의 id를 이웃 원소로 이동.
#[test]
fn source_address_lookups_scale_with_elements_and_requests() {
    let measure = |n| {
        crate::work::WORK.with(|w| w.set(0));
        let body = (0..n)
            .map(|_| p(&table("10", &field("SUM", &[("Formula", "=SUM(LEFT)")], "1"))))
            .collect::<String>();
        let f = facts(&[section(&body)]);
        let part = &f.parts[0];
        for ordinal in (0..n * 2).step_by(2) {
            let start = part.flow.span(ordinal).unwrap().start;
            assert_eq!(part.ordinal_at(start, true).unwrap(), FlowOrdinal(ordinal));
        }
        let edit =
            [DerivedEdit::Insert {
                section: 0, at: FlowBoundary::EndOfBody, fragment: &p("추가")
            }];
        f.effects(&edit).unwrap().check_derived_values().unwrap();
        crate::work::WORK.with(|w| w.get())
    };
    let small = measure(160);
    let large = measure(320);
    assert!(small > 0 && large > small && large < small * 3, "{small} -> {large}");
}

// Delete always changes the flow, so a displayed page reference is refused with or without a restart.
// 이것을 실패시키는 것: 흐름 변경 때 본문 쪽 참조 허용 복원.
#[test]
fn page_cross_reference_refuses_restart_removal() {
    let params = [
        ("RefType", "TARGET_BOOKMARK"),
        ("RefContentType", "OBJECT_TYPE_PAGE"),
        ("RefPath", "?mark;"),
        ("Command", "?mark;6;0;0;0;"),
    ];
    for (carrier, accepted) in
        [("<hp:ctrl><hp:newNum numType='PAGE' num='3'/></hp:ctrl>", false), ("삭제", false)]
    {
        let f = facts(&[section(&format!(
            "{}{}{}",
            p(carrier),
            p("<hp:bookmark name='mark'/>"),
            p(&field("CROSSREF", &params, "3"))
        ))]);
        assert!(!f.parts[0].xml.contains("autoNum"));
        let edits = [DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }];
        let effects = f.effects(&edits).unwrap();
        assert_eq!(effects.check_derived_values().is_ok(), accepted);
    }
}

// 이것을 실패시키는 것: Part 수집에서 tr·tc namespace 검사를 누락하거나 가변 치수/2400 정책 축소.
#[test]
fn table_rows_and_cells_require_paragraph_namespace() {
    let variable = table("20", "칸").replace("width='9000'", "width='12345'");
    let fixed = variable
        .replace("noAdjust='0'", "noAdjust='1'")
        .replace("<hp:cellSz width='12345' height='0'", "<hp:cellSz width='12345' height='2400'");
    for (profile, table) in [
        variable,
        fixed,
        inline_table("0", "0", false),
        inline_table("4688", "0", true),
        inline_table("2148", "1", true),
    ]
    .into_iter()
    .enumerate()
    {
        let xml = section(&format!("{}{}", p("삭제"), p(&table)));
        let _ = facts(std::slice::from_ref(&xml));
        for tag in ["tr", "tc"] {
            let foreign = xml
                .replace(&format!("<hp:{tag}>"), &format!("<x:{tag} xmlns:x='urn:foreign'>"))
                .replace(&format!("<hp:{tag} "), &format!("<x:{tag} xmlns:x='urn:foreign' "))
                .replace(&format!("</hp:{tag}>"), &format!("</x:{tag}>"));
            let alias = foreign.replace("urn:foreign", NS);
            let _ = facts(&[alias]);
            let source = XmlSource::read(&foreign).unwrap().1;
            let flow = ParagraphFlow::from_source(&source);
            assert!(
                Part::read(Some(0), foreign, source, flow, None).is_err(),
                "profile {profile}, {tag}"
            );
        }
    }
}

// 이것을 실패시키는 것: BOM을 제외한 fieldBegin 주소 전달 또는 BOM 없는 주소에 3바이트 추가.
#[test]
fn field_offsets_and_collection_include_original_bom() {
    let empty = "<hp:ctrl><hp:fieldBegin id='10' type='DATE'/></hp:ctrl><hp:t>날짜</hp:t><hp:ctrl><hp:fieldEnd beginIDRef='10'/></hp:ctrl>";
    for prefix in ["", "\u{feff}"] {
        for body in [field("DATE", &[], "날짜"), empty.into()] {
            let xml = format!("{prefix}{}", section(&p(&body)));
            let source = XmlSource::read(&xml).unwrap().1;
            let expected =
                source.elements().iter().find(|e| e.name() == "fieldBegin").unwrap().outer().start;
            assert_eq!(expected, xml.find("<hp:fieldBegin").unwrap());
            let mut offsets = Vec::new();
            crate::structural::xml_values::scan_fields(&xml, |start, kind, _| {
                assert_eq!(kind, "DATE");
                offsets.push(start);
                Ok(())
            })
            .unwrap();
            assert_eq!(offsets, [expected]);
            let flow = ParagraphFlow::from_source(&source);
            let part = Part::read(Some(0), xml, source, flow, None).unwrap();
            assert_eq!(part.fields.len(), 1);
            assert_eq!(
                part.source.elements()[*part.fields.keys().next().unwrap()].outer().start,
                expected
            );
        }
    }
}

// 이것을 실패시키는 것: ON_SECTION 참조를 전역 종류 변화로 다시 거부하거나 CONTINUOUS/대상 구역 변화 승인.
#[test]
fn note_cross_references_respect_target_section_numbering() {
    for (note, ref_type, code) in
        [("footNote", "TARGET_FOOTNOTE", "3"), ("endNote", "TARGET_ENDNOTE", "4")]
    {
        for policy in ["ON_SECTION", "CONTINUOUS"] {
            for deleted_section in [0, 1] {
                let params = [
                    ("RefType", ref_type),
                    ("RefContentType", "OBJECT_TYPE_NUMBER"),
                    ("RefPath", "?#7;"),
                    ("Command", if code == "3" { "?#7;3;1;0;0;" } else { "?#7;4;1;0;0;" }),
                ];
                let remote =
                    section(&format!("{}{}", p(&format!("<hp:{note} number='1'/>")), p("끝")));
                let target_note = format!("<hp:{note}Pr><hp:numbering type='{policy}'/></hp:{note}Pr><hp:{note} id='7' number='2'/>");
                let target = section(&format!(
                    "{}{}{}",
                    p(&format!("<hp:{note} number='1'/>")),
                    p(&target_note),
                    p("끝")
                ));
                let reference = section(&p(&format!(
                    "<hp:{note}Pr><hp:numbering type='ON_PAGE'/></hp:{note}Pr>{}",
                    field("CROSSREF", &params, "2")
                )));
                let f = facts(&[remote, target, reference]);
                let edits = [DerivedEdit::Delete {
                    section: deleted_section,
                    paragraphs: &[FlowOrdinal(0)],
                }];
                let effects = f.effects(&edits).unwrap();
                let accepted = policy == "ON_SECTION" && deleted_section == 0;
                let (_, field) = f.parts[2].fields.iter().next().unwrap();
                assert_eq!(
                    crossref_unchanged(&effects, field).is_ok(),
                    accepted,
                    "{note} {policy} {deleted_section}"
                );
                assert_eq!(
                    effects.check_derived_values().is_ok(),
                    accepted,
                    "{note} {policy} {deleted_section}"
                );
            }
        }
    }
}

// 이것을 실패시키는 것: Part 수집에서 subList·cellSpan·cellSz 중 하나의 namespace 검사 누락.
#[test]
fn table_cell_evidence_requires_paragraph_namespace() {
    for table in [table("20", "칸"), inline_table("0", "0", false)] {
        let xml = section(&p(&table));
        for name in ["subList", "cellSpan", "cellSz"] {
            let foreign = xml
                .replace(&format!("<hp:{name} "), &format!("<x:{name} xmlns:x='urn:foreign' "))
                .replace(&format!("</hp:{name}>"), &format!("</x:{name}>"));
            assert_ne!(foreign, xml);
            let source = XmlSource::read(&foreign).unwrap().1;
            let flow = ParagraphFlow::from_source(&source);
            assert!(Part::read(Some(0), foreign.clone(), source, flow, None).is_err());
            let alias = foreign.replace("urn:foreign", NS);
            let _ = facts(&[alias]);
        }
    }
}

// 이것을 실패시키는 것: fieldBegin 후보가 없으면 종류와 무관하게 inventory 항목 생략.
#[test]
fn authored_field_requires_a_field_begin_candidate() {
    use hwpforge_core::{Control, Document, ImageStore, PageSettings, Paragraph, Run, Section};
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    let mut document = Document::new();
    document.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("원문", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let bytes = crate::HwpxEncoder::encode(
        &document.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
    let package = crate::raw_package::RawPackage::read(&bytes).unwrap();
    let mut decoded = crate::HwpxDecoder::decode(&bytes).unwrap();
    DerivedFacts::collect(&package, &decoded).unwrap();
    // The source records a text run. An authored field at that path has no wire begin.
    decoded.document.sections_mut()[0].paragraphs[0].runs[0] = Run::control(
        Control::Field {
            field_type: hwpforge_foundation::FieldType::ClickHere,
            hint_text: None,
            help_text: None,
            name: Some("input".into()),
            display_text: String::new(),
        },
        CharShapeIndex::new(0),
    );
    let inventory =
        hwpforge_core::field_inventory::FieldInventory::from_sections(decoded.document.sections());
    assert_eq!(inventory.fields().len(), 1);
    assert_eq!(inventory.fields()[0].kind(), "CLICK_HERE");
    let path = hwpforge_core::node_path::CorePath::new(0, inventory.fields()[0].start());
    assert!(decoded.source_maps[0].node(&path).is_some());
    assert!(matches!(
        DerivedFacts::collect(&package, &decoded),
        Err(crate::HwpxError::InvalidStructure { .. })
    ));
}

// 이것을 실패시키는 것: BOM 없는 reader 좌표로 원문 꼬리를 슬라이스.
#[test]
fn negative_scan_tail_uses_original_utf8_boundary() {
    for bom in ["", "\u{feff}"] {
        let xml = format!("{bom}<hs:sec xmlns:hs='http://www.hancom.co.kr/hwpml/2011/section' label='가나'><p/></hs:sec>");
        assert!(safe_negative(&xml));
    }
}

// 이것을 실패시키는 것: 공유 영향 표 집합에서 본문 구조 변경·host·독립 흐름 경계 조건 중 하나 생략.
#[test]
fn shared_affected_tables_keep_structure_hosts_and_boundaries() {
    let body = facts(&[section(&format!("{}{}", p("<hp:t>old</hp:t>"), table("20", "칸")))]);
    let end =
        [DerivedEdit::Insert { section: 0, at: FlowBoundary::EndOfBody, fragment: &p("추가") }];
    assert_eq!(body.effects(&end).unwrap().affected_tables[&0].by_table, [true]);
    let hosted = facts(&[section(&p(&table("20", "칸")))]);
    assert_eq!(hosted.effects(&end).unwrap().affected_tables[&0].by_table, [false]);
    let independent = facts(&[section(&format!(
        "{}<hp:header><hp:subList>{}</hp:subList></hp:header>",
        p("앞"),
        table("20", "칸")
    ))]);
    assert_eq!(independent.effects(&end).unwrap().affected_tables[&0].by_table, [false]);
}

// Synthetic table dimensions are stated directly in the fixture.
pub(crate) fn inline_six_column_table() -> String {
    let mut xml = "<hp:tbl id='7' zOrder='3' numberingType='TABLE' textWrap='TOP_AND_BOTTOM' textFlow='BOTH_SIDES' lock='0' dropcapstyle='None' pageBreak='CELL' repeatHeader='1' rowCnt='2' colCnt='6' cellSpacing='0' noAdjust='1'><hp:sz width='47816' widthRelTo='ABSOLUTE' height='3130' heightRelTo='ABSOLUTE' protect='0'/><hp:pos treatAsChar='1' affectLSpacing='0' flowWithText='1' allowOverlap='0' holdAnchorAndSO='0' vertRelTo='PARA' horzRelTo='PARA' vertAlign='TOP' horzAlign='LEFT' vertOffset='0' horzOffset='0'/><hp:outMargin left='283' right='283' top='283' bottom='283'/><hp:inMargin left='141' right='141' top='141' bottom='141'/>".to_owned();
    for row in 0..2 {
        xml.push_str("<hp:tr>");
        for (column, width) in [7969, 7969, 7969, 7969, 7969, 7971].iter().enumerate() {
            xml.push_str(&format!("<hp:tc header='0' hasMargin='0' protect='0' editable='0' dirty='0'><hp:subList textDirection='HORIZONTAL' lineWrap='BREAK' vertAlign='CENTER' textWidth='0' textHeight='0' hasTextRef='0' hasNumRef='0' linkListIDRef='0' linkListNextIDRef='0'>{}</hp:subList><hp:cellAddr rowAddr='{row}' colAddr='{column}'/><hp:cellSpan rowSpan='1' colSpan='1'/><hp:cellSz width='{width}' height='1565'/><hp:cellMargin left='510' right='510' top='141' bottom='141'/></hp:tc>", p("칸")));
        }
        xml.push_str("</hp:tr>");
    }
    xml.push_str("</hp:tbl>");
    xml
}

// 이것을 실패시키는 것: wire 속성 누락을 0으로 대체, style_ref None 기본화, strict shape/style 참조 대조 생략.
#[test]
fn outline_heading_references_are_strict() {
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    let store = crate::style_store_for_preset("default").unwrap();
    let mut p = hwpforge_core::Paragraph::with_runs(
        vec![hwpforge_core::Run::text("plain", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    p.style_ref = store.style_ref_at(0);
    let tag=Tag::read("<hp:p xmlns:hp='http://www.hancom.co.kr/hwpml/2011/paragraph' paraPrIDRef='0' styleIDRef='0'/>").unwrap();
    check_outline_plain_heading(&store, &p, &tag).unwrap();
    for attrs in [
        "styleIDRef='0'",
        "paraPrIDRef='0'",
        "paraPrIDRef='bad' styleIDRef='0'",
        "paraPrIDRef='0' styleIDRef='bad'",
        "paraPrIDRef='4294967296' styleIDRef='0'",
        "paraPrIDRef='0' styleIDRef='4294967296'",
    ] {
        let tag = Tag::read(&format!(
            "<hp:p xmlns:hp='http://www.hancom.co.kr/hwpml/2011/paragraph' {attrs}/>"
        ))
        .unwrap();
        assert!(check_outline_plain_heading(&store, &p, &tag).is_err(), "{attrs}");
    }
    let mut absent = p.clone();
    absent.style_ref = None;
    assert!(check_outline_plain_heading(&store, &absent, &tag).is_err());
    let mut different = p.clone();
    different.para_shape_id = ParaShapeIndex::new(1);
    assert!(check_outline_plain_heading(&store, &different, &tag).is_err());
    let mut different = p.clone();
    different.style_ref = store.style_ref_at(1);
    assert!(check_outline_plain_heading(&store, &different, &tag).is_err());
}

// 이것을 실패시키는 것: 삭제 changed/deleted/본문 index 대응 또는 secPr 소유 paragraph 유일성 검사 생략.
#[test]
fn outline_deletion_effects_require_complete_original_ownership() {
    use hwpforge_core::{
        Document, ImageStore, NumberingDef, PageSettings, Paragraph, Run, Section,
    };
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    let mut store = crate::style_store_for_preset("default").unwrap();
    store.push_numbering(NumberingDef::default_outline());
    store.push_numbering(NumberingDef::default_outline());
    let mut doc = Document::new();
    for _ in 0..2 {
        doc.add_section(Section::with_paragraphs(
            (0..3)
                .map(|_| {
                    Paragraph::with_runs(
                        vec![Run::text("plain", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )
                })
                .collect(),
            PageSettings::a4(),
        ));
    }
    let base =
        crate::HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new()).unwrap();
    let mut package = crate::raw_package::RawPackage::read(&base).unwrap();
    let xml = package
        .read_text_entry("Contents/section1.xml")
        .unwrap()
        .replace("outlineShapeIDRef=\"1\"", "outlineShapeIDRef=\"2\"");
    package.replace_text_entry("Contents/section1.xml", xml);
    let base = package.write().unwrap();
    let decoded = crate::HwpxDecoder::decode(&base).unwrap();
    let facts = DerivedFacts::collect(&package, &decoded).unwrap();
    let part = facts.parts.iter().find(|p| p.section == Some(1)).unwrap();
    let id = crate::decoder::source_map::section_property_origin(&part.xml, &part.source).unwrap();
    let span = part.source.element(id).unwrap().outer();
    use sha2::Digest;
    let locations =
        [(1, span.clone(), <[u8; 32]>::from(sha2::Sha256::digest(&part.xml.as_bytes()[span])))];
    let ordinals = [facts.body_ordinal(0, 1).unwrap()];
    let edits = [DerivedEdit::Delete { section: 0, paragraphs: &ordinals }];
    facts.effects(&edits).unwrap().check_section_outline_losses(&decoded, &locations).unwrap();
    for change in [
        "empty",
        "changed-count",
        "changed-ordinal",
        "not-body",
        "map-count",
        "map-duplicate",
        "property-owner",
        "property-owner-duplicate",
        "duplicate-location",
    ] {
        let mut decoded = crate::HwpxDecoder::decode(&base).unwrap();
        match change {
            "map-count" => {
                decoded.source_maps[0].paragraphs.pop();
            }
            "map-duplicate" => {
                decoded.source_maps[0].paragraphs[2] = decoded.source_maps[0].paragraphs[1].clone()
            }
            "property-owner" => {
                decoded.source_maps[1].paragraphs.remove(0);
            }
            "property-owner-duplicate" => {
                let p = decoded.source_maps[1].paragraphs[0].clone();
                decoded.source_maps[1].paragraphs.push(p);
            }
            _ => {}
        }
        let facts = DerivedFacts::collect(&package, &decoded).unwrap();
        let mut effects = facts.effects(&edits).unwrap();
        let effect = effects.effects.get_mut(&0).unwrap();
        match change {
            "empty" => {
                effect.deleted.clear();
                effect.changed.clear();
            }
            "changed-count" => {
                effect.changed.insert(2);
            }
            "changed-ordinal" => {
                effect.changed.clear();
                effect.changed.insert(2);
            }
            "not-body" => {
                effect.deleted[0].end -= 1;
            }
            _ => {}
        }
        let claims = match change {
            "duplicate-location" => vec![locations[0].clone(), locations[0].clone()],
            _ => locations.to_vec(),
        };
        assert!(effects.check_section_outline_losses(&decoded, &claims).is_err(), "{change}");
    }
}

// 이것을 실패시키는 것: 직접/스타일 shape를 생략하거나 para_list의 lossy None을 승인.
#[test]
fn outline_wire_heading_checks_are_independent() {
    use crate::style_store::{HwpxParaShape, HwpxStyle};
    use hwpforge_core::{NumberingDef, Paragraph, Run};
    use hwpforge_foundation::{CharShapeIndex, HeadingType, ParaShapeIndex};
    for inherited in [false, true] {
        for (kind, level, reference) in [
            (HeadingType::Number, 0, 1),
            (HeadingType::Outline, 0, 1),
            (HeadingType::Bullet, 0, 1),
            (HeadingType::Number, 10, 1),
            (HeadingType::Number, 0, 99),
            (HeadingType::Outline, 10, 0),
            (HeadingType::Bullet, 0, 99),
        ] {
            let mut store = crate::HwpxStyleStore::new();
            store.push_numbering(NumberingDef::default_outline());
            store.push_para_shape(HwpxParaShape::default());
            let shape = HwpxParaShape {
                heading_type: kind,
                heading_level: level,
                heading_id_ref: reference,
                ..HwpxParaShape::default()
            };
            store.push_para_shape(shape);
            let (direct, style) = match inherited {
                false => (1, 0),
                true => (0, 1),
            };
            store.push_style(HwpxStyle::new(70, "PARA", "Plain", "Plain", style, 0, 0, 1042, 0));
            let mut paragraph = Paragraph::with_runs(
                vec![Run::text("plain", CharShapeIndex::new(0))],
                ParaShapeIndex::new(direct),
            );
            paragraph.style_ref = store.style_ref_at(0);
            assert!(paragraph.heading_level.is_none());
            let tag=Tag::read(&format!("<hp:p xmlns:hp='http://www.hancom.co.kr/hwpml/2011/paragraph' paraPrIDRef='{direct}' styleIDRef='0'/>")).unwrap();
            assert!(
                check_outline_plain_heading(&store, &paragraph, &tag).is_err(),
                "{inherited} {kind:?} {level} {reference}"
            );
        }
    }
}

// 이것을 실패시키는 것: nested_cell_sublist 항상 false, 소유만으로 proof 생략, from_source-only 흐름의 seed 제거.
// 이것을 실패시키는 것: 텍스트 변경의 영향 표 계산 제거.
#[test]
fn nested_seeds_and_derived_effects_agree() {
    let xml = section(&format!(
        "{}{}{}",
        p("<hp:secPr textDirection='HORIZONTAL'/><hp:t>앞</hp:t>"),
        p(&table("10", &table("20", "<hp:t>안</hp:t>"))),
        p("<hp:t>뒤</hp:t>")
    ));
    let mut f = facts(&[xml]);
    let part = &mut f.parts[0];
    let changed = [false, false, false, true, false];
    let proven = [true; 5];
    assert_eq!(part.flow.effective_text_seeds(&changed, &proven), changed);
    part.flow.prove_text_tail(&part.source, &part.xml, &Default::default(), |_| None);
    for success in [true, false] {
        let mut proof = proven;
        proof[3] = success;
        let seeds = part.flow.effective_text_seeds(&changed, &proof);
        assert_eq!(seeds, if success { [false; 5] } else { changed });
        assert_eq!(
            part.flow.affected_paragraphs(&seeds),
            if success { [false; 5] } else { [false, true, true, true, true] }
        );
        assert_eq!(part.tables.affected_tables(&part.flow, &seeds), [!success; 2]);
    }
    for success in [true, false] {
        let mut proof = proven;
        proof[3] = success;
        let offset = f.parts[0].xml.find("안").unwrap();
        let slots = [(offset..offset + "안".len(), "나".into())];
        let edits = [DerivedEdit::Text {
            section: 0,
            changed: &[FlowOrdinal(3)],
            proven: &proof,
            slots: &slots,
        }];
        assert_eq!(f.effects(&edits).unwrap().affected_tables[&0].by_table, [!success; 2]);
    }
}

// 이것을 실패시키는 것: note 정책 수집에서 중복 numbering의 첫 자식을 정책으로 선택.
#[test]
fn duplicate_note_numbering_is_unknown_before_public_decode() {
    use hwpforge_core::{Document, ImageStore, PageSettings, Paragraph, Run, Section};
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        ["처음", "삭제", "생존"]
            .map(|text| {
                Paragraph::with_runs(
                    vec![Run::text(text, CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )
            })
            .to_vec(),
        PageSettings::a4(),
    ));
    let bytes = crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
    for note in ["footNote", "endNote"] {
        let package = crate::raw_package::RawPackage::read(&bytes).unwrap();
        let source =
            std::str::from_utf8(package.read_entry("Contents/section0.xml").unwrap()).unwrap();
        let start = source.find(&format!("<hp:{note}Pr>")).unwrap();
        let end = source[start..].find(&format!("</hp:{note}Pr>")).unwrap()
            + start
            + format!("</hp:{note}Pr>").len();
        let pr = &source[start..end];
        let numbering_start = pr.find("<hp:numbering ").unwrap();
        let numbering_end = pr[numbering_start..].find("/>").unwrap() + numbering_start + 2;
        let numbering = &pr[numbering_start..numbering_end];
        let duplicate = pr.replacen(numbering, &format!("{numbering}{numbering}"), 1);
        let source = source.replacen(pr, &duplicate, 1).replacen("<hp:t>생존</hp:t>", &format!("<hp:ctrl><hp:{note} number='1'><hp:subList><hp:p><hp:run><hp:t>주석</hp:t></hp:run></hp:p></hp:subList></hp:{note}></hp:ctrl>"), 1);
        let f = facts(std::slice::from_ref(&source));
        assert!(
            check_derived_values(
                &f,
                &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(1)] }]
            )
            .is_err(),
            "{note}"
        );
    }
}
