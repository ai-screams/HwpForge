use super::*;

// Single-decision fixtures keep the result and height assertions independent.
fn check_derived_values(facts: &DerivedFacts<'_>, edits: &[DerivedEdit<'_>]) -> HwpxResult<()> {
    facts.effects(edits)?.check_derived_values()
}
fn collect_height_invalidations(
    facts: &DerivedFacts<'_>,
    edits: &[DerivedEdit<'_>],
) -> HwpxResult<Vec<HeightInvalidation>> {
    facts.effects(edits)?.collect_height_invalidations()
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
// 이것을 실패시키는 것: 모든 표/미지 표를 무조건 승인하거나 영향 높이를 누락.
#[test]
fn deletion_zeroes_only_surviving_tail_table() {
    let f = facts(&[section(&format!(
        "{}{}{}",
        p(&table("11", "앞")),
        p("삭제"),
        p(&table("22", "뒤"))
    ))]);
    let heights = collect_height_invalidations(
        &f,
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(2)] }],
    )
    .unwrap();
    assert_eq!(heights.len(), 1);
    assert_eq!(&f.parts[0].xml[heights[0].value.clone()], "22");
    // Inline eligibility also requires the observed dimensions, margins and complete cell layout.
    let bad = facts(&[f.parts[0].xml.replace("treatAsChar='0'", "treatAsChar='1'")]);
    assert!(collect_height_invalidations(
        &bad,
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(2)] }]
    )
    .is_err());
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
#[test]
fn page_results_keep_observed_containers_only() {
    for (content, accepted) in [
        (num("PAGE"), true),
        (num("TOTAL_PAGE"), true),
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
    assert!(collect_height_invalidations(
        &f,
        &[DerivedEdit::Insert { section: 0, at: FlowBoundary::EndOfBody, fragment: &p("추가") }]
    )
    .unwrap()
    .is_empty());
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
// 이것을 실패시키는 것: CROSSREF R을 번호/내용/위아래/다른 대상/원격 컨테이너로 확대, 삭제 책갈피 승인.
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
    check_derived_values(
        &facts(std::slice::from_ref(&good)),
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }],
    )
    .unwrap();
    assert!(check_derived_values(
        &facts(std::slice::from_ref(&good)),
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
}

// 이것을 실패시키는 것: 재배치 프로필 축 생략, 누락을 기본값으로 보충, cellSz/다른 sz를 높이로 선택.
#[test]
fn height_profile_checks_each_observed_axis() {
    let good = section(&format!("{}{}", p("삭제"), p(&table("&#50;2", "칸"))));
    let edit = [DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }];
    let f = facts(std::slice::from_ref(&good));
    assert_eq!(&good[collect_height_invalidations(&f, &edit).unwrap()[0].value.clone()], "&#50;2");
    let variants = [
        ("pageBreak='CELL'", "pageBreak='NONE'"),
        ("pageBreak='CELL'", "pageBreak='TABLE'"),
        ("textWrap='TOP_AND_BOTTOM'", "textWrap='SQUARE'"),
        ("textFlow='BOTH_SIDES'", "textFlow='LEFT_ONLY'"),
        ("repeatHeader='1'", "repeatHeader='0'"),
        ("noAdjust='0'", "noAdjust='2'"),
        ("heightRelTo='ABSOLUTE'", "heightRelTo='RELATIVE'"),
        ("heightRelTo='ABSOLUTE'", ""),
        ("treatAsChar='0'", "treatAsChar='1'"),
        ("flowWithText='1'", "flowWithText='0'"),
        ("affectLSpacing='0'", "affectLSpacing='1'"),
        ("allowOverlap='0'", "allowOverlap='1'"),
        ("holdAnchorAndSO='0'", "holdAnchorAndSO='1'"),
        ("vertRelTo='PARA'", "vertRelTo='PAGE'"),
        ("horzRelTo='COLUMN'", "horzRelTo='PARA'"),
        ("vertAlign='TOP'", "vertAlign='CENTER'"),
        ("horzAlign='LEFT'", "horzAlign='RIGHT'"),
        ("vertOffset='0'", "vertOffset='1'"),
        ("horzOffset='0'", "horzOffset='1'"),
        ("colSpan='1'", "colSpan='2'"),
        ("rowSpan='1'", "rowSpan='2'"),
        ("textDirection='HORIZONTAL'", "textDirection='VERTICAL'"),
        ("lineWrap='BREAK'", "lineWrap='SQUEEZE'"),
        ("textWidth='0'", "textWidth='1'"),
        ("textHeight='0'", "textHeight='1'"),
        ("hasTextRef='0'", "hasTextRef='1'"),
        ("hasNumRef='0'", "hasNumRef='1'"),
        ("linkListIDRef='0'", "linkListIDRef='2'"),
        ("linkListNextIDRef='0'", "linkListNextIDRef='2'"),
        ("height='&#50;2'", "height='-1'"),
        ("height='&#50;2'", "height='4294967296'"),
        ("height='&#50;2'", "height='1' height='2'"),
        ("height='0'", "height='2401'"),
        ("height='0'", "height='2400'"),
    ];
    for (from, to) in variants {
        let changed = good.replace(from, to);
        let refused = try_part(&changed)
            .and_then(|part| {
                collect_height_invalidations(&DerivedFacts::from_parts(vec![part]), &edit)
            })
            .is_err();
        assert!(refused, "{from} => {to}");
    }
    for changed in [
        good.replace("<hp:sz", "<hp:sz height='1' heightRelTo='ABSOLUTE'/><hp:sz"),
        good.replace("<hp:pos", "<hp:pos/><hp:pos"),
        good.replace("<hp:sz", "<hp:pic><hp:sz height='100'/></hp:pic><hp:sz"),
    ] {
        let f = facts(&[changed]);
        let result = collect_height_invalidations(&f, &edit);
        if f.parts[0].xml.contains("<hp:pic>") {
            assert_eq!(&f.parts[0].xml[result.unwrap()[0].value.clone()], "&#50;2");
        } else {
            assert!(result.is_err());
        }
    }
    for height in ["0", ""] {
        let changed = if height.is_empty() {
            good.replace("height='&#50;2'", "")
        } else {
            good.replace("height='&#50;2'", "height='0'")
        };
        assert!(collect_height_invalidations(&facts(&[changed]), &edit).unwrap().is_empty());
    }
    let no_adjust = good.replace("noAdjust='0'", "noAdjust='1'");
    assert_eq!(
        collect_height_invalidations(&facts(std::slice::from_ref(&no_adjust)), &edit)
            .unwrap()
            .len(),
        1
    );
    assert!(collect_height_invalidations(
        &facts(&[no_adjust.replace("height='0'", "height='2401'")]),
        &edit
    )
    .is_err());
    for lexical in [
        good.replace("height='&#50;2'", "height=\"&#50;2\""),
        good.replace("width='9000'", "width='9000' extra='quoted > value'"),
    ] {
        let f = facts(&[lexical]);
        let h = collect_height_invalidations(&f, &edit).unwrap();
        assert_eq!(&f.parts[0].xml[h[0].value.clone()], "&#50;2");
    }
    let fixed = good.replace("noAdjust='0'", "noAdjust='1'").replace("height='0'", "height='2400'");
    assert_eq!(collect_height_invalidations(&facts(&[fixed]), &edit).unwrap().len(), 1);
}
// 이것을 실패시키는 것: 호스트/조상 표 전파 누락, header/footer 경계 제거, 삭제 안 표를 높이 목록에 남김.
#[test]
fn height_flow_includes_ancestors_and_respects_independent_hosts() {
    let header = p(&format!(
        "<hp:ctrl><hp:header><hp:subList>{}</hp:subList></hp:header></hp:ctrl>",
        p(&table("10", "머리말"))
    ));
    let f = facts(&[section(&format!("{}{}{}", p("삭제"), header, p(&table("20", "뒤"))))]);
    let heights = collect_height_invalidations(
        &f,
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }],
    )
    .unwrap();
    assert_eq!(heights.len(), 1);
    assert_eq!(&f.parts[0].xml[heights[0].value.clone()], "20");
    let f = facts(&[section(&format!("{}{}", p(&table("20", "삭제 표")), p("끝")))]);
    assert!(collect_height_invalidations(
        &f,
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
    )
    .unwrap()
    .is_empty());
    for container in ["footNote", "endNote", "memo"] {
        let f = facts(&[section(&format!(
            "{}{}",
            p("삭제"),
            p(&format!(
                "<hp:ctrl><hp:{container}><hp:subList>{}</hp:subList></hp:{container}></hp:ctrl>",
                p(&table("20", "안"))
            ))
        ))]);
        assert!(collect_height_invalidations(
            &f,
            &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
        )
        .is_err());
    }
    let nested = table("20", &table("10", "안"));
    let f = facts(&[section(&format!("{}{}", p("앞"), p(&nested)))]);
    assert!(collect_height_invalidations(
        &f,
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
    )
    .is_err());
    let seeds = vec![false, false, false, true];
    assert_eq!(f.parts[0].tables.affected_tables(&f.parts[0].flow, &seeds), [true, true]);
}
// 이것을 실패시키는 것: Before를 본문 인덱스로 해석, 끝 삽입을 마지막 중첩 문단 뒤 seed로 계산.
#[test]
fn insertion_boundaries_follow_original_body_flow() {
    let f = facts(&[section(&format!(
        "{}{}{}",
        p(&table("10", "앞")),
        p("사이"),
        p(&table("20", "뒤"))
    ))]);
    for (boundary, expected) in [
        (FlowBoundary::Before(FlowOrdinal(0)), 2),
        (FlowBoundary::Before(FlowOrdinal(2)), 1),
        (FlowBoundary::Before(FlowOrdinal(3)), 1),
        (FlowBoundary::EndOfBody, 0),
    ] {
        assert_eq!(
            collect_height_invalidations(
                &f,
                &[DerivedEdit::Insert { section: 0, at: boundary, fragment: &p("추가") }]
            )
            .unwrap()
            .len(),
            expected
        );
    }
    assert!(collect_height_invalidations(
        &f,
        &[DerivedEdit::Insert {
            section: 0,
            at: FlowBoundary::Before(FlowOrdinal(1)),
            fragment: &p("추가")
        }]
    )
    .is_err());
}

// 이것을 실패시키는 것: 최종 캐시 목록으로 높이 영향 계산, 입증/self_owned/전역 fallback 생략.
#[test]
fn height_seeds_precede_the_plain_tail_cache_exception() {
    let xml = section(&format!(
        "{}{}{}{}",
        p("<hp:secPr textDirection='HORIZONTAL'/><hp:t>입력</hp:t>"),
        p(&table("10", "<hp:t>칸</hp:t>")),
        p("<hp:t>본문</hp:t>"),
        p(&table("20", "<hp:t>끝</hp:t>"))
    ));
    let mut f = facts(&[xml]);
    let part = &mut f.parts[0];
    part.flow.prove_text_tail(&part.source, &part.xml, &Default::default(), |_| None);
    let n = part.flow.paragraph_count();
    let proven = vec![true; n];
    let changed = vec![true, false, true, false, false, false];
    let seeds = part.flow.effective_text_seeds(&changed, &proven);
    assert_eq!(seeds, vec![false; n]);
    for fail in [0, 2] {
        let mut proof = proven.clone();
        proof[fail] = false;
        let seeds = part.flow.effective_text_seeds(&changed, &proof);
        assert!(seeds[fail]);
        assert_eq!(part.tables.affected_tables(&part.flow, &seeds), [true, true]);
        assert!(part.flow.caches_to_drop_for_text_edits([fail], &proof).is_empty());
    }
    let incompatible = section(&format!(
        "{}{}",
        p("<hp:secPr textDirection='HORIZONTAL'/><hp:colPr colCount='2' sameSz='0'/>"),
        p("<hp:t>入力</hp:t>")
    ));
    let mut global = facts(&[incompatible]);
    let global = &mut global.parts[0];
    global.flow.prove_text_tail(&global.source, &global.xml, &Default::default(), |_| None);
    assert_eq!(global.flow.effective_text_seeds(&[false, true], &[true, true]), [false, true]);
    let fallback = facts(std::slice::from_ref(&part.xml));
    assert_eq!(fallback.parts[0].flow.effective_text_seeds(&changed, &proven), changed);
    let unowned=section(&p("<hp:secPr textDirection='HORIZONTAL'/><hp:ctrl><hp:footNote><hp:subList><hp:p><hp:run><hp:t>안</hp:t></hp:run></hp:p></hp:subList></hp:footNote></hp:ctrl>"));
    let mut f = facts(&[unowned]);
    let part = &mut f.parts[0];
    part.flow.prove_text_tail(&part.source, &part.xml, &Default::default(), |_| None);
    assert!(part.flow.effective_text_seeds(&[false, true], &[true, true])[1]);
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
        assert!(collect_height_invalidations(&f, &edits).is_err());
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
    assert!(collect_height_invalidations(&f, &[]).unwrap().is_empty());
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
    let f = facts(&[section(&format!("{}{}", p("삭제"), table("20", "소유 불명")))]);
    assert!(collect_height_invalidations(
        &f,
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }]
    )
    .is_err());
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
        assert_eq!(effects.collect_height_invalidations().unwrap().len(), n);
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
    check_derived_values(&facts(std::slice::from_ref(&good)), &edit).unwrap();
    for command in ["?mark;0;0;0;0;", "?mark;6;1;0;0;", "?mark;6;2;0;0;"] {
        assert!(check_derived_values(&facts(&[good.replace("?mark;6;0;0;0;", command)]), &edit)
            .is_err());
    }
}

// 이것을 실패시키는 것: 끝 삽입의 seed 부재를 소유 불명 표의 무영향 증명으로 사용.
#[test]
fn end_insertion_requires_known_table_ownership() {
    let f = facts(&[section(&format!("{}{}", p("본문"), table("20", "소유 불명")))]);
    let edits =
        [DerivedEdit::Insert { section: 0, at: FlowBoundary::EndOfBody, fragment: &p("추가") }];
    check_derived_values(&f, &edits).unwrap();
    assert!(collect_height_invalidations(&f, &edits).is_err());
}
// 이것을 실패시키는 것: 머리말/꼬리말/바탕쪽/각주/미주/메모 책갈피에 본문 page 참조 R을 전용.
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
fn assert_inline_profile(cell_height: &str, no_adjust: &str, merged: bool) {
    // 새 범주의 높이 상한 밖에서 기존 병합 프로필의 축 결합만 검사한다.
    let table_height = if merged { "60000" } else { "10000" };
    let table = inline_table(cell_height, no_adjust, merged).replacen(
        "height='10000'",
        &format!("height='{table_height}'"),
        1,
    );
    let good = section(&format!("{}{}", p("삭제"), p(&table)));
    let edit = [DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }];
    let f = facts(std::slice::from_ref(&good));
    let heights = collect_height_invalidations(&f, &edit).unwrap();
    assert_eq!(heights.len(), 1);
    assert_eq!(&good[heights[0].value.clone()], table_height);
    let horizontal = if merged { "PARA" } else { "COLUMN" };
    let other_horizontal = if merged { "COLUMN" } else { "PARA" };
    let opposite_adjust = if no_adjust == "0" { "1" } else { "0" };
    let rows = if merged { "3" } else { "2" };
    let mut variants = vec![
        ("treatAsChar='1'".into(), "treatAsChar='2'".into()),
        (format!("horzRelTo='{horizontal}'"), format!("horzRelTo='{other_horizontal}'")),
        (format!("height='{cell_height}'"), "height='999'".into()),
        (format!("noAdjust='{no_adjust}'"), format!("noAdjust='{opposite_adjust}'")),
        (format!("rowCnt='{rows}'"), "rowCnt='4'".into()),
        ("colCnt='3'".into(), "colCnt='4'".into()),
        ("rowSpan='1'".into(), "rowSpan='2'".into()),
        ("colSpan='1'".into(), "colSpan='2'".into()),
        ("rowAddr='0'".into(), "rowAddr='1'".into()),
        ("colAddr='0'".into(), "colAddr='1'".into()),
        ("width='16063'".into(), "width='16064'".into()),
        ("width='48189'".into(), "width='48190'".into()),
        ("widthRelTo='ABSOLUTE'".into(), "widthRelTo='RELATIVE'".into()),
        ("protect='0'".into(), "protect='1'".into()),
        ("zOrder='0'".into(), "zOrder='1'".into()),
        ("numberingType='TABLE'".into(), "numberingType='PICTURE'".into()),
        ("lock='0'".into(), "lock='1'".into()),
        ("dropcapstyle='None'".into(), "dropcapstyle='DoubleLine'".into()),
        ("cellSpacing='0'".into(), "cellSpacing='1'".into()),
        ("left='283'".into(), "left='284'".into()),
        ("left='510'".into(), "left='511'".into()),
        ("vertAlign='CENTER'".into(), "vertAlign='TOP'".into()),
        ("header='0'".into(), "header='1'".into()),
        ("hasMargin='0'".into(), "hasMargin='1'".into()),
        ("editable='0'".into(), "editable='1'".into()),
        ("dirty='0'".into(), "dirty='1'".into()),
    ];
    for (from, to) in [
        ("pageBreak='CELL'", "pageBreak='NONE'"),
        ("repeatHeader='1'", "repeatHeader='0'"),
        ("textWrap='TOP_AND_BOTTOM'", "textWrap='SQUARE'"),
        ("textFlow='BOTH_SIDES'", "textFlow='LEFT_ONLY'"),
        ("heightRelTo='ABSOLUTE'", "heightRelTo='RELATIVE'"),
        ("affectLSpacing='0'", "affectLSpacing='1'"),
        ("flowWithText='1'", "flowWithText='0'"),
        ("allowOverlap='0'", "allowOverlap='1'"),
        ("holdAnchorAndSO='0'", "holdAnchorAndSO='1'"),
        ("vertRelTo='PARA'", "vertRelTo='PAGE'"),
        ("vertAlign='TOP'", "vertAlign='BOTTOM'"),
        ("horzAlign='LEFT'", "horzAlign='RIGHT'"),
        ("vertOffset='0'", "vertOffset='1'"),
        ("horzOffset='0'", "horzOffset='1'"),
        ("textDirection='HORIZONTAL'", "textDirection='VERTICAL'"),
        ("lineWrap='BREAK'", "lineWrap='SQUEEZE'"),
        ("textWidth='0'", "textWidth='1'"),
        ("textHeight='0'", "textHeight='1'"),
        ("hasTextRef='0'", "hasTextRef='1'"),
        ("hasNumRef='0'", "hasNumRef='1'"),
        ("linkListIDRef='0'", "linkListIDRef='1'"),
        ("linkListNextIDRef='0'", "linkListNextIDRef='1'"),
    ] {
        variants.push((from.into(), to.into()));
    }
    if merged {
        variants.push(("treatAsChar='1'".into(), "treatAsChar='0'".into()));
        variants.push(("width='32126'".into(), "width='32127'".into()));
    }
    if !merged {
        variants.push(("vertAlign='BOTTOM'".into(), "vertAlign='CENTER'".into()));
    }
    for (from, to) in variants {
        let changed = good.replacen(&from, &to, 1);
        assert_ne!(changed, good, "{from}");
        assert!(collect_height_invalidations(&facts(&[changed]), &edit).is_err(), "{from} => {to}");
    }
    for element in ["outMargin", "inMargin", "cellMargin", "tc", "sz"] {
        let start = good.find(&format!("<hp:{element} ")).unwrap();
        let end = start + good[start..].find('>').unwrap() + 1;
        let opening = &good[start..end];
        let pairs = match element {
            "outMargin" => {
                vec![("left", "283"), ("right", "283"), ("top", "283"), ("bottom", "1000")]
            }
            "inMargin" | "cellMargin" => {
                vec![("left", "510"), ("right", "510"), ("top", "141"), ("bottom", "141")]
            }
            "tc" => vec![
                ("header", "0"),
                ("hasMargin", "0"),
                ("protect", "0"),
                ("editable", "0"),
                ("dirty", "0"),
            ],
            _ => vec![("width", "48189"), ("widthRelTo", "ABSOLUTE"), ("protect", "0")],
        };
        for (key, value) in pairs {
            let changed_opening =
                opening.replace(&format!("{key}='{value}'"), &format!("{key}='999'"));
            assert_ne!(changed_opening, opening);
            let changed = format!("{}{}{}", &good[..start], changed_opening, &good[end..]);
            assert!(
                collect_height_invalidations(&facts(&[changed]), &edit).is_err(),
                "{element}@{key}"
            );
        }
    }
    // Swapping a 2x2 span with a singleton preserves span counts, but changes placement.
    if merged {
        let swapped = good
            .replacen("rowSpan='2' colSpan='2'", "SWAPPED_SPAN", 1)
            .replacen("rowSpan='1' colSpan='1'", "rowSpan='2' colSpan='2'", 1)
            .replace("SWAPPED_SPAN", "rowSpan='1' colSpan='1'");
        assert_ne!(swapped, good);
        assert!(collect_height_invalidations(&facts(&[swapped]), &edit).is_err());
    } else {
        // Changing inline to floating lands on the established default-flow profile.
        // Its variable dimensions and zero cell heights remain eligible.
        let floating = good.replace("treatAsChar='1'", "treatAsChar='0'");
        assert_eq!(collect_height_invalidations(&facts(&[floating]), &edit).unwrap().len(), 1);
    }
    let first_row_end = good.find("</hp:tr>").unwrap();
    let cell_start = good[..first_row_end].rfind("<hp:tc ").unwrap();
    let cell_end = cell_start + good[cell_start..].find("</hp:tc>").unwrap() + "</hp:tc>".len();
    let cell = &good[cell_start..cell_end];
    let moved = good.replacen(cell, "", 1).replacen(
        "</hp:tr><hp:tr>",
        &format!("</hp:tr><hp:tr>{cell}"),
        1,
    );
    assert!(collect_height_invalidations(&facts(&[moved]), &edit).is_err());
    let last_cell_start = good.rfind("<hp:tc ").unwrap();
    let last_cell_end =
        last_cell_start + good[last_cell_start..].find("</hp:tc>").unwrap() + "</hp:tc>".len();
    let missing_cell = format!("{}{}", &good[..last_cell_start], &good[last_cell_end..]);
    assert!(collect_height_invalidations(&facts(&[missing_cell]), &edit).is_err());
    for name in ["tr", "tc"] {
        let changed = good
            .replace(
                &format!("<hp:{name}"),
                &format!("<foreign:{name} xmlns:foreign='urn:foreign'"),
            )
            .replace(&format!("</hp:{name}>"), &format!("</foreign:{name}>"));
        // 이것을 실패시키는 것: foreign tr·tc가 프로필 판정 전 수집을 통과.
        let alias = changed.replace("urn:foreign", NS);
        assert_eq!(collect_height_invalidations(&facts(&[alias]), &edit).unwrap().len(), 1);
        let source = XmlSource::read(&changed).unwrap().1;
        let flow = ParagraphFlow::from_source(&source);
        assert!(Part::read(Some(0), changed, source, flow, None).is_err());
    }
    for changed in [
        good.replacen("</hp:tbl>", "<hp:pic/></hp:tbl>", 1),
        good.replacen("</hp:tc>", "<hp:unknown/></hp:tc>", 1),
        good.replacen("</hp:subList>", "<hp:tc/></hp:subList>", 1),
        good.replacen("</hp:subList>", "<hp:subList/></hp:subList>", 1),
        good.replacen("</hp:tr>", "<hp:tc/></hp:tr>", 1),
        good.replacen("</hp:tbl>", "<hp:tr/></hp:tbl>", 1),
        good.replacen("<hp:cellAddr", "<hp:cellAddr rowAddr='0' colAddr='0'/><hp:cellAddr", 1),
        good.replacen("</hp:tbl>", "<hp:tc/></hp:tbl>", 1),
        good.replacen("</hp:tbl>", "<hp:subList/></hp:tbl>", 1),
    ] {
        assert!(collect_height_invalidations(&facts(&[changed]), &edit).is_err());
    }
}

// 이것을 실패시키는 것: inline zero-cell 프로필 누락, 단일 축 완화, 다른 행의 축을 섞어 승인.
#[test]
fn inline_zero_cell_table_preserves_established_profile() {
    assert_inline_profile("0", "0", false);
}
// 이것을 실패시키는 것: 기존 병합 프로필의 noAdjust와 셀 높이를 서로 다른 행에서 OR로 조합.
#[test]
fn inline_merged_minimum_cells_preserve_established_profile() {
    assert_inline_profile("4688", "0", true);
}
// 이것을 실패시키는 것: 기존 병합 프로필의 noAdjust와 셀 높이를 서로 다른 행에서 OR로 조합.
#[test]
fn inline_merged_fixed_cells_preserve_established_profile() {
    assert_inline_profile("2148", "1", true);
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

// 이것을 실패시키는 것: autoNum이 없는 PAGE 참조에서 재시작 변경 검사 생략.
#[test]
fn page_cross_reference_refuses_restart_removal() {
    let params = [
        ("RefType", "TARGET_BOOKMARK"),
        ("RefContentType", "OBJECT_TYPE_PAGE"),
        ("RefPath", "?mark;"),
        ("Command", "?mark;6;0;0;0;"),
    ];
    for (carrier, accepted) in
        [("<hp:ctrl><hp:newNum numType='PAGE' num='3'/></hp:ctrl>", false), ("삭제", true)]
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
    let edits = [DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }];
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
        assert_eq!(
            collect_height_invalidations(&facts(std::slice::from_ref(&xml)), &edits).unwrap().len(),
            1
        );
        for tag in ["tr", "tc"] {
            let foreign = xml
                .replace(&format!("<hp:{tag}>"), &format!("<x:{tag} xmlns:x='urn:foreign'>"))
                .replace(&format!("<hp:{tag} "), &format!("<x:{tag} xmlns:x='urn:foreign' "))
                .replace(&format!("</hp:{tag}>"), &format!("</x:{tag}>"));
            let alias = foreign.replace("urn:foreign", NS);
            assert_eq!(collect_height_invalidations(&facts(&[alias]), &edits).unwrap().len(), 1);
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
                let (&id, field) = f.parts[2].fields.iter().next().unwrap();
                assert_eq!(
                    crossref_unchanged(&effects, &f.parts[2], id, field).is_ok(),
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
            let aliased = facts(&[alias]);
            let part = &aliased.parts[0];
            let table_id = part.source.elements().iter().position(|e| e.name() == "tbl").unwrap();
            assert!(table_profiles::matches(part, table_id));
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

// Synthetic values are stated independently of the production profile constants.
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

pub(crate) fn inline_single_axis_tables() -> Vec<String> {
    let base = inline_six_column_table();
    vec![
        base.clone(),
        base.replace("zOrder='3'", "zOrder='0'"),
        base.replace("width='47816'", "width='48189'")
            .replace("width='7969'", "width='8031'")
            .replace("width='7971'", "width='8034'"),
        base.replace("inMargin left='141' right='141'", "inMargin left='510' right='510'"),
        base.replace("bottom='283'", "bottom='1000'"),
    ]
}

// 이것을 실패시키는 것: 승인 축을 exact 조합 목록으로 제한, 범위 밖 폭·여백·셀 높이 승인,
// DefaultFlow의 셀 높이 계약을 inline 표와 혼동.
#[test]
fn inline_six_column_tables_combine_approved_axes() {
    let base = inline_six_column_table();
    let edit = [DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }];
    for mask in 0u32..16 {
        if mask.count_ones() < 2 {
            continue;
        }
        let mut changed = base.clone();
        for (bit, from, to) in [
            (1, "zOrder='3'", "zOrder='0'"),
            (2, "width='47816'", "width='48189'"),
            (4, "inMargin left='141' right='141'", "inMargin left='510' right='510'"),
            (8, "bottom='283'", "bottom='1000'"),
        ] {
            if mask & bit != 0 {
                changed = changed.replace(from, to);
            }
        }
        if mask & 2 != 0 {
            changed = changed
                .replace("width='7969'", "width='8031'")
                .replace("width='7971'", "width='8034'");
        }
        let xml = section(&format!("{}{}", p("삭제"), p(&changed)));
        assert_eq!(
            collect_height_invalidations(&facts(&[xml]), &edit).unwrap().len(),
            1,
            "mask={mask}"
        );
    }
    for (from, to) in [
        ("zOrder='3'", "zOrder='238'"),
        ("width='47816'", "width='0'"),
        ("inMargin left='141'", "inMargin left='142'"),
        ("bottom='283'", "bottom='284'"),
        ("treatAsChar='1'", "treatAsChar='0'"),
        ("flowWithText='1'", "flowWithText='0'"),
        ("horzRelTo='PARA'", "horzRelTo='COLUMN'"),
        ("vertOffset='0'", "vertOffset='1'"),
        ("height='1565'", "height='0'"),
        ("rowCnt='2'", "rowCnt='3'"),
        ("colCnt='6'", "colCnt='5'"),
        ("colAddr='5'", "colAddr='4'"),
        ("rowSpan='1'", "rowSpan='2'"),
        ("header='0'", "header='1'"),
        ("cellMargin left='510'", "cellMargin left='141'"),
        ("lineWrap='BREAK'", "lineWrap='SQUEEZE'"),
    ] {
        let changed = base.replacen(from, to, 1);
        assert_ne!(changed, base);
        let xml = section(&format!("{}{}", p("삭제"), p(&changed)));
        assert!(collect_height_invalidations(&facts(&[xml]), &edit).is_err(), "{from} => {to}");
    }
    // Floating COLUMN tables must retain their own cell-height contract.
    let floating = base
        .replace("treatAsChar='1'", "treatAsChar='0'")
        .replace("horzRelTo='PARA'", "horzRelTo='COLUMN'");
    let xml = section(&format!("{}{}", p("삭제"), p(&floating)));
    assert!(collect_height_invalidations(&facts(&[xml]), &edit).is_err());
}

// 이것을 실패시키는 것: 범주 안의 폭·zOrder·여백 축을 제한하거나 연속 영향 표를 하나만 수집.
#[test]
fn inline_six_column_tables_invalidate_each_surviving_table() {
    let edit = [DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }];
    let mut cases: Vec<_> = inline_single_axis_tables().into_iter().map(|t| p(&t)).collect();
    cases.push(
        p(&inline_six_column_table()) + &p(&inline_six_column_table().replace("id='7'", "id='8'")),
    );
    for (index, tables) in cases.into_iter().enumerate() {
        let xml = section(&(p("삭제") + &tables));
        let heights =
            collect_height_invalidations(&facts(std::slice::from_ref(&xml)), &edit).unwrap();
        assert_eq!(heights.len(), [1, 1, 1, 1, 1, 2][index]);
        for height in heights {
            assert_eq!(&xml[height.value], "3130");
        }
    }
}

// A small raw grid builder: the first anchor may cover a rectangle; covered cells are absent.
pub(crate) fn inline_grid(rows: usize, columns: usize, row_span: usize, col_span: usize) -> String {
    let base = inline_six_column_table();
    let head = base
        .split("<hp:tr>")
        .next()
        .unwrap()
        .replace("rowCnt='2'", &format!("rowCnt='{rows}'"))
        .replace("colCnt='6'", &format!("colCnt='{columns}'"));
    let start = base.find("<hp:tc ").unwrap();
    let end = start + base[start..].find("</hp:tc>").unwrap() + 8;
    let cell = &base[start..end];
    let mut xml = head;
    for row in 0..rows {
        xml.push_str("<hp:tr>");
        for col in 0..columns {
            if (row != 0 || col != 0) && row < row_span && col < col_span {
                continue;
            }
            let mut anchor = cell
                .replace("rowAddr='0'", &format!("rowAddr='{row}'"))
                .replace("colAddr='0'", &format!("colAddr='{col}'"));
            if row == 0 && col == 0 {
                anchor = anchor
                    .replace("rowSpan='1'", &format!("rowSpan='{row_span}'"))
                    .replace("colSpan='1'", &format!("colSpan='{col_span}'"));
            }
            xml.push_str(&anchor);
        }
        xml.push_str("</hp:tr>");
    }
    xml.push_str("</hp:tbl>");
    xml
}
pub(crate) fn inline_tall_table() -> String {
    let grid = inline_grid(22, 6, 2, 2)
        .replace("noAdjust='1'", "noAdjust='0'")
        .replace("zOrder='3'", "zOrder='237'")
        .replace(
            "inMargin left='141' right='141' top='141' bottom='141'",
            "inMargin left='283' right='283' top='283' bottom='283'",
        )
        .replace(
            "outMargin left='283' right='283' top='283' bottom='283'",
            "outMargin left='0' right='0' top='0' bottom='0'",
        )
        .replace("width='47816'", "width='16084'")
        .replace("height='3130'", "height='52523'");
    let mut chunks = grid.split("<hp:tr>");
    let mut xml = chunks.next().unwrap().to_owned();
    for (row, cells) in chunks.enumerate() {
        xml.push_str("<hp:tr>");
        let height = match row {
            2 => "282",
            3 => "19637",
            _ => "1565",
        };
        let mut cells = cells.replace("height='1565'", &format!("height='{height}'"));
        for (column, width) in [1303, 2000, 2500, 3000, 3500, 3781].into_iter().enumerate() {
            let prefix = format!(
                "colAddr='{column}'/><hp:cellSpan rowSpan='1' colSpan='1'/><hp:cellSz width="
            );
            cells = cells.replace(&format!("{prefix}'7969'"), &format!("{prefix}'{width}'"));
        }
        xml.push_str(&if row == 0 {
            cells.replacen("height='1565'", "height='3130'", 1).replacen(
                "width='7969'",
                "width='3303'",
                1,
            )
        } else {
            cells
        });
    }
    xml
}
fn inline_result(table: &str) -> HwpxResult<Vec<HeightInvalidation>> {
    let xml = section(&(p("삭제") + &p(table)));
    let source = XmlSource::read(&xml).map_err(|_| invalid())?.1;
    let flow = ParagraphFlow::from_source(&source);
    let part = Part::read(Some(0), xml, source, flow, None)?;
    collect_height_invalidations(
        &DerivedFacts::from_parts(vec![part]),
        &[DerivedEdit::Delete { section: 0, paragraphs: &[FlowOrdinal(0)] }],
    )
}
fn inline_case(label: &str, xml: &str, accepted: bool) {
    assert_eq!(inline_result(xml).is_ok(), accepted, "{label}");
}

// 이것을 실패시키는 것: H 검사 제거·exact fallback 잔존·H를 기존 경로에도 적용·0/누락 조기 성공.
#[test]
fn inline_category_stored_height_edges() {
    for base in inline_single_axis_tables() {
        for (height, accepted) in [
            ("1", true),
            ("52523", true),
            ("52524", false),
            ("0", false),
            ("-1", false),
            ("1.5", false),
            ("4294967296", false),
            ("+1", false),
        ] {
            inline_case(
                height,
                &base.replace("height='3130'", &format!("height='{height}'")),
                accepted,
            );
        }
        inline_case("missing height", &base.replace("height='3130'", ""), false);
    }
    for base in [
        inline_table("0", "0", false),
        inline_table("4688", "0", true),
        inline_table("2148", "1", true),
        table("10000", "칸"),
    ] {
        for h in ["0", ""] {
            let changed =
                base.replacen("height='10000'", if h.is_empty() { "" } else { "height='0'" }, 1);
            assert!(inline_result(&changed).unwrap().is_empty());
        }
        inline_case(
            "old path height above category",
            &base.replacen("height='10000'", "height='52524'", 1),
            true,
        );
        for h in ["-1", "1.5", "4294967296"] {
            inline_case(h, &base.replacen("height='10000'", &format!("height='{h}'"), 1), false);
        }
    }
}

// 이것을 실패시키는 것: NA1에 NA0 범위 사용·NA0 상하한 생략·미지 NA 기본값·0 허용.
// NA1 6260은 별도 승인 상한이며 직접 저장 실측의 극값으로 간주하지 않는다.
#[test]
fn inline_category_cell_height_policies() {
    for (na, lo, hi) in [(0, 282, 19637), (1, 1282, 6260)] {
        let base = inline_grid(2, 6, 1, 1).replace("noAdjust='1'", &format!("noAdjust='{na}'"));
        for h in [lo, 1565, 5107, hi] {
            inline_case(
                &format!("NA{na} {h}"),
                &base.replacen("height='1565'", &format!("height='{h}'"), 1),
                true,
            );
        }
        for h in [0, lo - 1, hi + 1] {
            inline_case(
                &format!("NA{na} {h}"),
                &base.replacen("height='1565'", &format!("height='{h}'"), 1),
                false,
            );
        }
        inline_case("all zero", &base.replace("height='1565'", "height='0'"), false);
        inline_case(
            "merged upper",
            &inline_grid(2, 6, 2, 1).replace("noAdjust='1'", &format!("noAdjust='{na}'")).replacen(
                "height='1565'",
                &format!("height='{hi}'"),
                1,
            ),
            true,
        );
    }
    let base = inline_grid(2, 6, 1, 1);
    let split = base.find("</hp:tr>").unwrap();
    let varied = base[..split].replace("height='1565'", "height='1282'")
        + &base[split..].replace("height='1565'", "height='5107'");
    inline_case("policy with distinct row heights", &varied, true);
    for value in ["noAdjust='2'", ""] {
        inline_case(
            "unknown policy",
            &inline_six_column_table().replace("noAdjust='1'", value),
            false,
        );
    }
    let varied = inline_grid(2, 6, 1, 1)
        .replace("noAdjust='1'", "noAdjust='0'")
        .replacen("height='1565'", "height='282'", 1)
        .replacen("height='1565'", "height='19637'", 1);
    inline_case("different heights within one row", &varied, true);
}

// 이것을 실패시키는 것: 행·열 범위/실제 행 수/점유 누락, 임의 블록 병합 허용, 주소·span 검사 생략.
#[test]
fn inline_category_grid_and_merge_edges() {
    for (rows, cols) in [(1, 1), (1, 6), (22, 1), (22, 6)] {
        inline_case("grid edge", &inline_grid(rows, cols, 1, 1), true);
    }
    for (rows, cols) in [(0, 1), (23, 1), (1, 0), (1, 7)] {
        inline_case("grid outside", &inline_grid(rows, cols, 1, 1), false);
    }
    for (r, c, ok) in [
        (2, 1, true),
        (1, 2, true),
        (22, 1, true),
        (1, 6, true),
        (2, 2, true),
        (2, 3, false),
        (3, 2, false),
    ] {
        inline_case(&format!("span {r}/{c}"), &inline_grid(22, 6, r, c), ok);
    }
    let base = inline_grid(2, 6, 1, 1);
    for (label, from, to) in [
        ("actual rows", "rowCnt='2'", "rowCnt='3'"),
        ("duplicate address", "colAddr='1'", "colAddr='0'"),
        ("wrong row", "rowAddr='0'", "rowAddr='1'"),
        ("span zero", "rowSpan='1'", "rowSpan='0'"),
        ("column span zero", "colSpan='1'", "colSpan='0'"),
        ("row overflow", "rowSpan='1'", "rowSpan='3'"),
        ("column overflow", "colSpan='1'", "colSpan='7'"),
        ("overlap", "colSpan='1'", "colSpan='2'"),
        ("address overflow", "colAddr='0'", "colAddr='4294967296'"),
        ("address outside", "colAddr='0'", "colAddr='6'"),
        ("span overflow", "rowSpan='1'", "rowSpan='4294967296'"),
    ] {
        inline_case(label, &base.replacen(from, to, 1), false);
    }
    inline_case(
        "row addresses exchanged",
        &base
            .replace("rowAddr='0'", "rowAddr='swap'")
            .replace("rowAddr='1'", "rowAddr='0'")
            .replace("rowAddr='swap'", "rowAddr='1'"),
        false,
    );
    inline_case(
        "extra empty direct row",
        &base.replacen("</hp:tbl>", "<hp:tr/></hp:tbl>", 1),
        false,
    );
    // Admission also checks row counts; exercise the profile itself so that its
    // direct-row check cannot be masked by that earlier rejection.
    let extra_row = base.replacen("</hp:tbl>", "<hp:tr/></hp:tbl>", 1);
    let f = facts(&[section(&p(&extra_row))]);
    let part = &f.parts[0];
    let id = part.source.elements().iter().position(|e| e.name() == "tbl").unwrap();
    assert!(!table_profiles::matches(part, id), "extra direct row in profile");
    inline_case(
        "last row span beyond grid",
        &base.replacen(
            "rowAddr='1' colAddr='0'/><hp:cellSpan rowSpan='1'",
            "rowAddr='1' colAddr='0'/><hp:cellSpan rowSpan='2'",
            1,
        ),
        false,
    );
    inline_case(
        "last column span beyond grid",
        &base.replacen(
            "colAddr='5'/><hp:cellSpan rowSpan='1' colSpan='1'",
            "colAddr='5'/><hp:cellSpan rowSpan='1' colSpan='2'",
            1,
        ),
        false,
    );
    let merged = inline_grid(2, 6, 2, 2);
    let start = base.find("<hp:tc ").unwrap();
    let end = start + base[start..].find("</hp:tc>").unwrap() + 8;
    for (from, to) in [("rowSpan='1'", "rowSpan='0'"), ("colSpan='1'", "colSpan='0'")] {
        let ghost = base[start..end].replace("colAddr='0'", "colAddr='1'").replace(from, to);
        inline_case(
            "empty anchor on covered coordinate",
            &merged.replacen("</hp:tr>", &(ghost + "</hp:tr>"), 1),
            false,
        );
    }
    let start = base.rfind("<hp:tc ").unwrap();
    let end = start + base[start..].find("</hp:tc>").unwrap() + 8;
    inline_case(
        "last cell crosses column boundary",
        &base.replacen(
            &base[start..end],
            &base[start..end].replace("colSpan='1'", "colSpan='2'"),
            1,
        ),
        false,
    );
    inline_case("hole", &(base[..start].to_owned() + &base[end..]), false);
}

// 이것을 실패시키는 것: 폭·zOrder 상하한 완화/축소, ASCII 숫자 검사 제거, exact 폭 배열 잔존.
#[test]
fn inline_category_numeric_edges() {
    let base = inline_six_column_table();
    for (from, cases) in [
        (
            "width='47816'",
            vec![
                ("16084", true),
                ("48545", true),
                ("30000", true),
                ("16083", false),
                ("48546", false),
            ],
        ),
        (
            "width='7969'",
            vec![
                ("1303", true),
                ("48545", true),
                ("2000", true),
                ("1302", false),
                ("48546", false),
            ],
        ),
        (
            "zOrder='3'",
            vec![("0", true), ("1", true), ("237", true), ("238", false), ("-1", false)],
        ),
    ] {
        let key = from.split('=').next().unwrap();
        for (n, ok) in cases.into_iter().chain([
            ("+16084", false),
            ("1.2", false),
            ("4294967296", false),
            ("", false),
        ]) {
            inline_case(
                &format!("{key}={n}"),
                &base.replacen(from, &format!("{key}='{n}'"), 1),
                ok,
            );
        }
        inline_case("missing number", &base.replacen(from, "", 1), false);
    }
}

// 이것을 실패시키는 것: 새 범주의 숫자 helper에서 앞자리 0 검사를 제거.
#[test]
fn inline_category_rejects_leading_zero_numbers() {
    let base = inline_six_column_table();
    for (from, to) in [
        ("zOrder='3'", "zOrder='0237'"),
        ("height='3130'", "height='052523'"),
        ("zOrder='3'", "zOrder='00'"),
    ] {
        inline_case(to, &base.replacen(from, to, 1), false);
    }
}

// 이것을 실패시키는 것: 유한 여백을 면별 범위로 확장·승인 튜플 누락·셀 여백/flag/subList/위치/흐름 검사 생략.
// O3·O5는 관측·판정 합집합 승인으로, 독립 시각 판정으로 세지 않는다.
#[test]
fn inline_category_semantic_attributes() {
    let base = inline_six_column_table();
    for (name, tuples) in [
        ("inMargin", vec![(141, 141, 141, 141), (510, 510, 141, 141), (283, 283, 283, 283)]),
        (
            "outMargin",
            vec![
                (283, 283, 283, 283),
                (0, 0, 0, 0),
                (141, 141, 141, 141),
                (141, 141, 0, 141),
                (283, 283, 0, 0),
                (283, 283, 283, 1000),
            ],
        ),
    ] {
        let start = base.find(&format!("<hp:{name} ")).unwrap();
        let end = start + base[start..].find("/>").unwrap() + 2;
        for (l, r, t, b) in tuples {
            inline_case(
                name,
                &format!(
                    "{}<hp:{name} left='{l}' right='{r}' top='{t}' bottom='{b}'/>{}",
                    &base[..start],
                    &base[end..]
                ),
                true,
            );
        }
        for (l, r, t, b) in [(141, 283, 141, 141), (0, 283, 0, 0), (1000, 1000, 1000, 1000)] {
            inline_case(
                "unapproved tuple",
                &format!(
                    "{}<hp:{name} left='{l}' right='{r}' top='{t}' bottom='{b}'/>{}",
                    &base[..start],
                    &base[end..]
                ),
                false,
            );
        }
    }
    let mixed = base.replacen("vertAlign='CENTER'", "vertAlign='TOP'", 1).replacen(
        "vertAlign='CENTER'",
        "vertAlign='BOTTOM'",
        1,
    );
    inline_case("three alignments", &mixed, true);
    for (from, to) in [
        ("cellMargin left='510'", "cellMargin left='511'"),
        ("header='0'", "header='1'"),
        ("hasMargin='0'", "hasMargin='1'"),
        ("protect='0' editable", "protect='1' editable"),
        ("editable='0'", "editable='1'"),
        ("dirty='0'", "dirty='1'"),
        ("vertAlign='CENTER'", "vertAlign='UNKNOWN'"),
        ("textDirection='HORIZONTAL'", "textDirection='VERTICAL'"),
        ("lineWrap='BREAK'", "lineWrap='SQUEEZE'"),
        ("textWidth='0'", "textWidth='1'"),
        ("textHeight='0'", "textHeight='1'"),
        ("hasTextRef='0'", "hasTextRef='1'"),
        ("hasNumRef='0'", "hasNumRef='1'"),
        ("linkListIDRef='0'", "linkListIDRef='1'"),
        ("linkListNextIDRef='0'", "linkListNextIDRef='1'"),
        ("treatAsChar='1'", "treatAsChar='0'"),
        ("flowWithText='1'", "flowWithText='0'"),
        ("horzRelTo='PARA'", "horzRelTo='COLUMN'"),
        ("vertRelTo='PARA'", "vertRelTo='PAGE'"),
        ("vertAlign='TOP'", "vertAlign='CENTER'"),
        ("horzAlign='LEFT'", "horzAlign='RIGHT'"),
        ("vertOffset='0'", "vertOffset='1'"),
        ("horzOffset='0'", "horzOffset='1'"),
        ("affectLSpacing='0'", "affectLSpacing='1'"),
        ("allowOverlap='0'", "allowOverlap='1'"),
        ("holdAnchorAndSO='0'", "holdAnchorAndSO='1'"),
        ("widthRelTo='ABSOLUTE'", "widthRelTo='RELATIVE'"),
        ("heightRelTo='ABSOLUTE'", "heightRelTo='RELATIVE'"),
        ("protect='0'/>", "protect='1'/>"),
        ("pageBreak='CELL'", "pageBreak='NONE'"),
        ("repeatHeader='1'", "repeatHeader='0'"),
        ("textWrap='TOP_AND_BOTTOM'", "textWrap='SQUARE'"),
        ("textFlow='BOTH_SIDES'", "textFlow='LEFT_ONLY'"),
        ("numberingType='TABLE'", "numberingType='PICTURE'"),
        ("lock='0'", "lock='1'"),
        ("dropcapstyle='None'", "dropcapstyle='DoubleLine'"),
        ("cellSpacing='0'", "cellSpacing='1'"),
    ] {
        assert!(base.contains(from));
        inline_case(&format!("{from} -> {to}"), &base.replacen(from, to, 1), false);
    }
}

// 이것을 실패시키는 것: 자식 허용 목록·one_child 유일성·후손 소유 검사 생략.
#[test]
fn inline_category_children_and_ownership() {
    let base = inline_six_column_table();
    // 이것을 실패시키는 것: 정상 tc까지 거부하도록 행 자식 검사를 반전.
    let f = facts(&[section(&p(&base))]);
    let part = &f.parts[0];
    let id = part.source.elements().iter().position(|e| e.name() == "tbl").unwrap();
    assert!(table_profiles::matches(part, id), "direct cells in profile");
    for host in ["tbl", "tc", "tr"] {
        for child in ["caption", "cellzoneList", "label", "parameterset", "unknownChild"] {
            inline_case(
                &format!("{host}/{child}"),
                &base.replacen(&format!("</hp:{host}>"), &format!("<hp:{child}/></hp:{host}>"), 1),
                false,
            );
        }
    }
    for name in [
        "sz",
        "pos",
        "outMargin",
        "inMargin",
        "subList",
        "cellAddr",
        "cellSpan",
        "cellSz",
        "cellMargin",
    ] {
        let start = base.find(&format!("<hp:{name} ")).unwrap();
        let end = if name == "subList" {
            start + base[start..].find("</hp:subList>").unwrap() + 13
        } else {
            start + base[start..].find("/>").unwrap() + 2
        };
        let node = &base[start..end];
        inline_case(
            &format!("duplicate {name}"),
            &base.replacen(node, &(node.to_owned() + node), 1),
            false,
        );
        inline_case(&format!("missing {name}"), &base.replacen(node, "", 1), false);
    }
    for child in ["<hp:tc/>", "<hp:subList/>", "<hp:tbl/>"] {
        let changed = base.replacen("</hp:subList>", &format!("{child}</hp:subList>"), 1);
        inline_case("hidden descendants", &changed, false);
        let f = facts(&[section(&p(&changed))]);
        let part = &f.parts[0];
        let id = part.source.elements().iter().position(|e| e.name() == "tbl").unwrap();
        assert!(!table_profiles::matches(part, id), "matcher must reject {child}");
    }
    for name in ["tr", "tc", "sz", "pos", "subList"] {
        let alias = base.replace(&format!("hp:{name}"), &format!("alias:{name}"));
        let alias = alias.replacen("<hp:tbl ", &format!("<hp:tbl xmlns:alias='{NS}' "), 1);
        inline_case("same namespace alias", &alias, true);
        inline_case(
            "wrong namespace",
            &alias.replace(&format!("xmlns:alias='{NS}'"), "xmlns:alias='urn:foreign'"),
            false,
        );
    }
}

// 이것을 실패시키는 것: zOrder 237·표 폭 16084·안 여백 283·바깥 여백 0의 결합을 거부.
// 이것을 실패시키는 것: 표 높이=셀 합·행 안 동일 높이·단일 문단/텍스트 전용 조건 추가·work tick 제거.
#[test]
fn inline_category_preserves_content_and_visits_all_cells() {
    let base = inline_tall_table();
    inline_case("tall mixed grid", &base, true);
    let mut wrapped = inline_grid(2, 6, 1, 1).replace("height='3130'", "height='4447'");
    for width in [1303, 2000, 3000, 5000, 10000, 26513, 1303, 2000, 3000, 5000, 10000, 26513] {
        wrapped = wrapped.replacen("width='7969'", &format!("width='{width}'"), 1);
    }
    inline_case("stored height independent of cells", &wrapped, true);
    inline_case(
        "two paragraphs",
        &wrapped.replacen("</hp:subList>", &format!("{}</hp:subList>", p("두 번째")), 1),
        true,
    );
    let measure = |rows| {
        let f = facts(&[section(&p(&inline_grid(rows, 6, 1, 1)))]);
        let part = &f.parts[0];
        let id = part.source.elements().iter().position(|e| e.name() == "tbl").unwrap();
        crate::work::WORK.with(|w| w.set(0));
        assert!(table_profiles::matches(part, id));
        crate::work::WORK.with(|w| w.get())
    };
    assert_eq!(measure(1), 6);
    assert_eq!(measure(22), 132);
}
