use super::ParagraphFlow;
use hwpforge_core::source_xml::XmlSource;

fn flow(body: &str) -> ParagraphFlow {
    let body = body.replacen("<p>", "<p><secPr textDirection='HORIZONTAL'/>", 1);
    let xml = format!("<sec>{body}</sec>");
    let (_, source) = XmlSource::read(&xml).unwrap();
    let mut flow = ParagraphFlow::from_source(&source);
    // No fields in these fixtures: an empty inventory proves the section.
    flow.prove_text_tail(&source, &xml, &Default::default(), |_| None);
    flow
}

fn p(content: &str) -> String {
    format!("<p><run charPrIDRef='0'>{content}</run><linesegarray><lineseg/></linesegarray></p>")
}

fn dropped(flow: &ParagraphFlow, changed: &[usize]) -> Vec<usize> {
    let ranges = flow.caches_to_drop_for_text_edits(changed.iter().copied(), &[]);
    assert_eq!(ranges.len(), ranges.iter().collect::<std::collections::HashSet<_>>().len());
    flow.caches
        .iter()
        .enumerate()
        .filter_map(|(p, spans)| spans.iter().any(|s| ranges.contains(s)).then_some(p))
        .collect()
}

/// 이것을 실패시키는 것: 편집 문단도 비허용 게이트에 포함, 기존 꼬리 전체 제거.
#[test]
fn edited_paragraph_is_excluded_from_gate() {
    let f = flow(&format!(
        "{}{}{}",
        p("앞"),
        p("<secPr textDirection='HORIZONTAL'/><colPr colCount='1'/><tbl><pos treatAsChar='1'/></tbl>"),
        p("뒤")
    ));
    assert_eq!(dropped(&f, &[1]), [1]);
}

/// 이것을 실패시키는 것: 목록 시작부터 제거, 비허용 문단만 제거, 캐시 없는 위험 문단 무시.
#[test]
fn block_from_keeps_the_plain_prefix_of_the_tail() {
    for cached in [true, false] {
        let danger = if cached {
            p("<tbl><pos treatAsChar='1'/></tbl>")
        } else {
            "<p><run><tbl><pos treatAsChar='1'/></tbl></run></p>".into()
        };
        let f = flow(&format!("{}{}{}{}{}", p("앞"), p("편집"), p("평문"), danger, p("뒤")));
        assert_eq!(dropped(&f, &[1]), if cached { vec![1, 3, 4] } else { vec![1, 4] });
    }
}

/// 이것을 실패시키는 것: 캐시 없는 편집점 제외, stale 밖 비허용 문단으로 목록 차단.
#[test]
fn uncached_edits_and_prefix_objects() {
    let f = flow(&format!(
        "{}<p><run>편집</run></p>{}{}{}",
        p("<tbl><pos treatAsChar='1'/></tbl>"),
        p("평문"),
        p("<switch/>"),
        p("뒤")
    ));
    assert_eq!(dropped(&f, &[1]), [3, 4]);
}

/// 이것을 실패시키는 것: Param 접미사 허용, ctrl 하위 요소 무시, 허용 이름(fieldBegin 등) 누락.
#[test]
fn tail_allowlist_is_closed() {
    for name in [
        "run",
        "t",
        "linesegarray",
        "lineseg",
        "ctrl",
        "fieldBegin type='CLICK_HERE'",
        "fieldEnd",
        "parameters",
        "stringParam",
        "integerParam",
        "booleanParam",
        "tab",
        "lineBreak",
        "nbSpace",
        "fwSpace",
        "markpenBegin",
        "markpenEnd",
        "titleMark",
        "bookmark",
    ] {
        let f = flow(&format!("{}{}{}", p("편집"), p(&format!("<{name}/>")), p("뒤")));
        assert_eq!(dropped(&f, &[0]), [0], "{name}");
    }
    for name in [
        "listParam",
        "arrayParam",
        "unsignedintegerParam",
        "switch",
        "autoNum",
        "pageNum",
        "unknown",
        "pic",
        "rect",
        "footNote",
    ] {
        let f = flow(&format!("{}{}{}", p("편집"), p(&format!("<ctrl><{name}/></ctrl>")), p("뒤")));
        assert_eq!(dropped(&f, &[0]), [0, 1, 2], "{name}");
    }
}

/// 이것을 실패시키는 것: 중첩 편집에 본문 완화 적용, 중첩 제거 집합의 우선권 삭제.
#[test]
fn nested_and_mixed_edits_keep_legacy_drop_precedence() {
    for container in ["tbl", "caption", "drawText"] {
        let host = p(&format!(
            "<{container}><pos treatAsChar='1'/><subList>{}{}{}</subList></{container}>",
            p("칸 앞"),
            p("칸 편집"),
            p("칸 뒤")
        ));
        let f = flow(&format!("{}{}{}{}", p("본문 편집"), p("평문"), host, p("본문 뒤")));
        assert_eq!(dropped(&f, &[4]), [2, 4, 5, 6]);
        assert_eq!(dropped(&f, &[0, 4]), [0, 2, 4, 5, 6]);
        assert_eq!(dropped(&f, &[2]), [2]); // Owned cache only, inner flow survives.
    }
    let inner_host = p(&format!(
        "<tbl><pos treatAsChar='1'/><subList>{}{}{}</subList></tbl>",
        p("안 앞"),
        p("안 편집"),
        p("안 뒤")
    ));
    let outer_host = p(&format!(
        "<tbl><pos treatAsChar='1'/><subList>{}{}{}</subList></tbl>",
        p("바깥 앞"),
        inner_host,
        p("바깥 뒤")
    ));
    let f = flow(&format!("{}{}{}{}", p("본문 편집"), p("본문 평문"), outer_host, p("본문 뒤")));
    // 이것을 실패시키는 것: 조상 host가 있는 목록의 host 전파를 끊기.
    assert_eq!(dropped(&f, &[0, 6]), [0, 2, 4, 6, 7, 8, 9]);
}

/// 이것을 실패시키는 것: header/footer host 차단 삭제, footNote/endNote host 차단 추가.
#[test]
fn header_footer_stop_and_notes_propagate() {
    for container in ["header", "footer", "footNote", "endNote"] {
        let f = flow(&format!(
            "{}{}",
            p(&format!(
                "<{container}><subList>{}{}</subList></{container}>",
                p("안 편집"),
                p("안 뒤")
            )),
            p("본문 뒤")
        ));
        let expected =
            if matches!(container, "header" | "footer") { vec![1, 2] } else { vec![0, 1, 2, 3] };
        assert_eq!(dropped(&f, &[1]), expected, "{container}");
    }
}

/// 이것을 실패시키는 것: 첫 편집만 처리, 중복 span 반환, 편집 문단을 게이트에서 제외하지 않기.
#[test]
fn batch_order_and_duplicates_do_not_change_drop_set() {
    let f = flow(&format!(
        "{}{}{}{}{}",
        p("<tbl><pos treatAsChar='1'/></tbl>"),
        p("평문"),
        p("<switch/>"),
        p("평문"),
        p("끝")
    ));
    for edits in [vec![0, 2], vec![2, 0], vec![2, 0, 2, 0]] {
        assert_eq!(dropped(&f, &edits), [0, 2]);
    }
    assert!(dropped(&f, &[]).is_empty());
}

/// 이것을 실패시키는 것: secPr 방향 조건 삭제, colCount/sameSz 어느 한쪽 무시, 마지막 colPr만 검사.
#[test]
fn section_direction_and_every_column_setting_gate_preservation() {
    let mixed_columns =
        "<secPr textDirection='HORIZONTAL'/><colPr colCount='2' sameSz='0'/><colPr colCount='1'/>";
    for (settings, keep) in [
        ("<secPr textDirection='HORIZONTAL'/><colPr colCount='1' sameSz='0'/>", true),
        ("<secPr textDirection='HORIZONTAL'/><colPr colCount='2' sameSz='1'/>", true),
        ("<secPr textDirection='VERTICAL'/><colPr colCount='1'/>", false),
        ("<secPr/><colPr colCount='1'/>", false),
        ("<secPr textDirection='HORIZONTAL'/><colPr colCount='2' sameSz='0'/>", false),
        (mixed_columns, false),
    ] {
        let f = flow(&format!("{}{}{}", p(settings), p("편집"), p("뒤")));
        assert_eq!(dropped(&f, &[1]), if keep { vec![1] } else { vec![1, 2] }, "{settings}");
    }
}

/// Any object that text wraps around and that is not placed as a character
/// keeps the legacy removal set for the whole section, wherever it sits and
/// whatever it is anchored to; other wrap styles and inline objects do not.
/// 이것을 실패시키는 것: 둘러싸기 종류를 SQUARE 하나로 줄이기, textWrap 없음을 둘러싸지 않음으로
/// 보기, treatAsChar 없음을 글자처럼으로 보기, PAGE 앵커만 막기.
#[test]
fn wrapping_object_anywhere_in_the_section_keeps_legacy_invalidation() {
    for wrap in [
        " textWrap='SQUARE'",
        " textWrap='TIGHT'",
        " textWrap='THROUGH'",
        "",
        " textWrap='TOP_AND_BOTTOM'",
        " textWrap='BEHIND_TEXT'",
        " textWrap='IN_FRONT_OF_TEXT'",
    ] {
        for inline in [" treatAsChar='0'", " treatAsChar='1'", ""] {
            for anchor in [" vertRelTo='PARA'", " vertRelTo='PAGE'", " vertRelTo='PAPER'", ""] {
                for location in [0, 1, 3] {
                    let object = format!("<pic{wrap}><pos{inline}{anchor}/></pic>");
                    let mut paragraphs = [p("앞"), p("편집"), p("평문"), p("끝")];
                    paragraphs[location] = p(&object);
                    let f = flow(&paragraphs.concat());
                    let wrapped = !wrap.contains("TOP_AND_BOTTOM")
                        && !wrap.contains("_TEXT")
                        && inline != " treatAsChar='1'";
                    let expected = if wrapped {
                        vec![1, 2, 3]
                    } else if location == 3 {
                        vec![1, 3]
                    } else {
                        vec![1]
                    };
                    assert_eq!(dropped(&f, &[1]), expected, "{object} at {location}");
                }
            }
        }
    }
}

/// 이것을 실패시키는 것: 둘러싸기 판정을 본문 목록의 개체로 제한.
#[test]
fn wrapping_object_in_another_list_blocks_body_preservation() {
    let object = "<pic textWrap='SQUARE'><pos treatAsChar='0' vertRelTo='PARA'/></pic>";
    let f = flow(&format!(
        "{}{}{}",
        p("편집"),
        p("평문"),
        p(&format!("<tbl><pos treatAsChar='1'/><subList>{}</subList></tbl>", p(object)))
    ));
    assert_eq!(dropped(&f, &[0]), [0, 1, 2]);
    for container in ["header", "footer"] {
        let f = flow(&format!(
            "{}{}{}",
            p(&format!("<{container}><subList>{}</subList></{container}>", p(object))),
            p("편집"),
            p("뒤")
        ));
        assert_eq!(dropped(&f, &[2]), [2, 3], "{container}");
    }
    let inline = "<pic textWrap='SQUARE'><pos treatAsChar='1' vertRelTo='PARA'/></pic>";
    let f = flow(&format!(
        "{}{}{}",
        p("편집"),
        p("평문"),
        p(&format!("<tbl><pos treatAsChar='1'/><subList>{}</subList></tbl>", p(inline)))
    ));
    assert_eq!(dropped(&f, &[0]), [0, 2]);
}

/// An object without its own `pos` has no proof of being inline. A group's
/// children carry no `pos` and follow their container's.
/// 이것을 실패시키는 것: pos 없는 개체 검사 삭제, 묶음 자식 예외 삭제,
/// 둘러싸지 않는 배치 예외 삭제.
#[test]
fn objects_without_their_own_pos_count_as_floating() {
    for (object, keep) in [
        ("<pic/>", false),
        ("<rect textWrap='SQUARE'/>", false),
        ("<tbl textWrap='TOP_AND_BOTTOM'/>", true),
        ("<pic textWrap='BEHIND_TEXT'/>", true),
        ("<container><pos treatAsChar='1'/><rect/><pic/></container>", true),
        ("<container><pos treatAsChar='0'/><rect/></container>", false),
    ] {
        let f = flow(&format!("{}{}{}", p("편집"), p("평문"), p(object)));
        assert_eq!(dropped(&f, &[0]), if keep { vec![0, 2] } else { vec![0, 1, 2] }, "{object}");
    }
}

/// Dynamic fields (cross references, page numbers, dates, formulas) may
/// change their shown text when pages move, so only ClickHere stays.
/// 이것을 실패시키는 것: fieldBegin type 검사 삭제, type 없는 fieldBegin 허용.
#[test]
fn only_clickhere_fields_stay_in_a_preserved_tail() {
    for (field, keep) in [
        ("<fieldBegin type='CLICK_HERE'/><fieldBegin type='CLICK_HERE'/>", true),
        ("<fieldBegin type='CLICK_HERE'/><fieldBegin type='CROSSREF'/>", false),
        ("<fieldBegin type='CROSSREF'/>", false),
        ("<fieldBegin type='DATE'/>", false),
        ("<fieldBegin type='FORMULA'/>", false),
        ("<fieldBegin type='click_here'/>", false),
        ("<fieldBegin/>", false),
    ] {
        let f = flow(&format!("{}{}{}", p("편집"), p(&format!("<ctrl>{field}</ctrl>")), p("뒤")));
        assert_eq!(dropped(&f, &[0]), if keep { vec![0] } else { vec![0, 1, 2] }, "{field}");
    }
}

/// 이것을 실패시키는 것: 쌍따옴표 또는 원시 속성값만 해석, 문단 밖 둘러싸기 개체 무시.
#[test]
fn source_ranges_handle_prefixes_quotes_and_entities() {
    let f = flow(&format!(
        "<unknown/>{}{}",
        p("<hp:secPr textDirection='HORIZ&#79;NTAL' note='a > b'></hp:secPr>편집"),
        p("뒤")
    ));
    assert_eq!(dropped(&f, &[0]), [0]);
    let f = flow(&format!(
        "<pic textWrap='SQUARE'><pos treatAsChar='0' vertRelTo='PAGE'/></pic>{}{}",
        p("편집"),
        p("뒤")
    ));
    // No owning paragraph: the section-wide gate still sees the object.
    assert_eq!(dropped(&f, &[0]), [0, 1]);
}

/// 이것을 실패시키는 것: 소유 플래그를 nearest 대신 hosts에 기록.
#[test]
fn source_flag_follows_innermost_owner() {
    let xml = "<sec><p><header><p><run><unknown/></run></p></header><run><t>바깥</t></run></p><p><t>끝</t></p></sec>";
    let (_, source) = XmlSource::read(xml).unwrap();
    let mut f = ParagraphFlow::from_source(&source);
    f.prove_text_tail(&source, xml, &Default::default(), |_| None);
    assert_eq!(f.has_disallowed_element, [true, true, false]);
}

/// 이것을 실패시키는 것: 구역 방향 증거가 없어도 보존, 단 속성이 없으면 허용값으로 보충.
#[test]
fn missing_geometry_evidence_keeps_conservative_invalidation() {
    let xml = format!("<sec>{}{}</sec>", p("편집"), p("뒤"));
    let (_, source) = XmlSource::read(&xml).unwrap();
    let mut f = ParagraphFlow::from_source(&source);
    f.prove_text_tail(&source, &xml, &Default::default(), |_| None);
    assert_eq!(dropped(&f, &[0]), [0, 1]);
    let f = flow(&format!("{}{}{}", p("<colPr/>"), p("편집"), p("뒤")));
    assert_eq!(dropped(&f, &[1]), [1, 2]);
}

/// The flow of a minimal HWPX section built the way the decoder builds it,
/// field ranges projected from the section's inventory as the preserving
/// patch does.
fn mapped(body: &str) -> ParagraphFlow {
    let xml = format!("<hs:sec>{body}</hs:sec>");
    let parsed =
        crate::decoder::section::parse_section(&xml, 0, &Default::default()).expect("parse");
    let mut section = hwpforge_core::Section::with_paragraphs(
        parsed.paragraphs,
        hwpforge_core::PageSettings::a4(),
    );
    section.headers = parsed.headers;
    section.footers = parsed.footers;
    let mut map = parsed.source.finish(&section, 0, "Contents/section0.xml");
    let inventory = hwpforge_core::field_inventory::FieldInventory::from_sections(
        std::slice::from_ref(&section),
    );
    map.prove_cache_text_tail(&xml, &inventory, 0);
    map.cache_flow
}

/// A cached body paragraph; the first one also declares horizontal text.
fn hp(content: &str) -> String {
    format!(
        r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="0">{content}</hp:run><hp:linesegarray><hp:lineseg textpos="0" vertpos="0"/></hp:linesegarray></hp:p>"#
    )
}

fn body(paragraphs: &[String]) -> String {
    let mut paragraphs = paragraphs.iter().map(|c| hp(c)).collect::<String>();
    paragraphs = paragraphs.replacen(
        r#"<hp:run charPrIDRef="0">"#,
        r#"<hp:run charPrIDRef="0"><hp:secPr textDirection="HORIZONTAL"/>"#,
        1,
    );
    paragraphs
}

fn begin(id: &str, kind: &str) -> String {
    format!(
        r#"<hp:ctrl><hp:fieldBegin{id} type="{kind}" name="" editable="0" dirty="0" zorder="0" fieldid="1"/></hp:ctrl>"#
    )
}

fn end(id: &str) -> String {
    format!(r#"<hp:ctrl><hp:fieldEnd beginIDRef="{id}" fieldid="1"/></hp:ctrl>"#)
}

fn t(text: &str) -> String {
    format!("<hp:t>{text}</hp:t>")
}

/// An inline one-cell table holding `content` in its only paragraph.
fn table(content: &str) -> String {
    format!(
        r#"<hp:tbl rowCnt="1" colCnt="1"><hp:sz width="10000" height="5000"/><hp:pos treatAsChar="1"/><hp:tr><hp:tc><hp:subList>{}</hp:subList><hp:cellAddr colAddr="0" rowAddr="0"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:cellSz width="10000" height="5000"/></hp:tc></hp:tr></hp:tbl>"#,
        hp(content)
    )
}

/// A field other than ClickHere marks every paragraph it covers (begin,
/// body-only middle, end), with ranges from the shared field inventory. A
/// range the inventory does not prove keeps the legacy removal set.
/// 이것을 실패시키는 것: 인벤토리 범위 투영 삭제(아무것도 표시하지 않기), 입증 실패 시
/// 기존 제거로 돌아가지 않기, 흐름 구분 무시(시작·끝 목록 비교 삭제).
#[test]
fn field_ranges_from_the_inventory_mark_every_paragraph_they_cover() {
    let cross = begin(r#" id="77""#, "CROSSREF");
    let click = |id: &str| {
        format!("{}{}{}", begin(&format!(r#" id="{id}""#), "CLICK_HERE"), t("값"), end(id))
    };
    for (name, paragraphs, expected, proven) in [
        (
            "begun in the edited paragraph, ends in the next",
            vec![
                format!("{}{cross}{}", t("편집"), t("앞")),
                format!("{}{}", t("뒤"), end("77")),
                t("꼬리"),
            ],
            vec![0, 1, 2],
            true,
        ),
        (
            "body-only middle paragraph",
            vec![format!("{}{cross}", t("편집")), t("본문"), end("77"), t("꼬리")],
            vec![0, 1, 2, 3],
            true,
        ),
        (
            "begun later, ends two paragraphs on",
            vec![
                t("편집"),
                t("평문"),
                format!("{cross}{}", t("시작")),
                t("본문"),
                end("77"),
                t("꼬리"),
            ],
            vec![0, 2, 3, 4, 5],
            true,
        ),
        (
            "ClickHere spanning paragraphs",
            vec![
                format!("{}{}{}", t("편집"), begin(r#" id="5""#, "CLICK_HERE"), t("앞")),
                format!("{}{}", t("뒤"), end("5")),
                t("꼬리"),
            ],
            vec![0],
            true,
        ),
        (
            "begin that never ends",
            vec![format!("{}{cross}", t("편집")), t("평문"), t("꼬리")],
            vec![0, 1, 2],
            false,
        ),
        (
            "begin without an id",
            vec![format!("{}{}", t("편집"), begin("", "DATE")), end("77"), t("꼬리")],
            vec![0, 1, 2],
            false,
        ),
        ("end without a begin", vec![t("편집"), end("42"), t("꼬리")], vec![0, 1, 2], false),
        (
            "same id ClickHere folded inside the field",
            vec![
                format!("{}{cross}{}{}", t("편집"), click("77"), t("앞")),
                t("본문"),
                end("77"),
                t("꼬리"),
            ],
            vec![0, 1, 2, 3],
            true,
        ),
        (
            "same id ClickHere in a table cell inside the field",
            vec![
                format!("{}{cross}{}", t("편집"), table(&click("77"))),
                t("본문"),
                end("77"),
                t("꼬리"),
            ],
            vec![0, 2, 3, 4],
            true,
        ),
        (
            "begin in the body, end in a table cell",
            vec![format!("{}{cross}{}", t("편집"), table(&end("77"))), t("꼬리")],
            vec![0, 2],
            false,
        ),
        (
            "id reused after the first field closed",
            vec![
                format!("{}{cross}", t("편집")),
                end("77"),
                t("평문"),
                format!("{cross}{}", t("다시")),
                end("77"),
                t("꼬리"),
            ],
            vec![0, 1, 2, 3, 4, 5],
            false,
        ),
    ] {
        let f = mapped(&body(&paragraphs));
        assert_eq!(!f.unproven_fields, proven, "{name}");
        assert_eq!(dropped(&f, &[0]), expected, "{name}");
    }
}

/// Doubling the paragraphs, fields and edits at most doubles the counted
/// units: source elements visited by the flow build, run-node lookups and
/// paragraph-search steps and paragraphs visited by the field projection,
/// and the query's passes. Work outside these counters (building the field
/// inventory, parsing) is not measured here.
/// 이것을 실패시키는 것: 필드마다 모든 문단을 다시 훑어 표시하기(이차 투영),
/// run 노드 조회를 키 조회 대신 전체 순회로 바꾸기(이차 조회),
/// 편집점마다 stale_paragraphs 를 다시 부르기(이차 재순회), 편집 문단 캐시 보존 무시.
#[test]
fn counted_text_edit_gate_work_stays_linear() {
    let work = |n: usize| {
        let paragraphs: Vec<String> = (0..n)
            .map(|i| match i % 4 {
                1 => format!("{}{}", begin(&format!(r#" id="{i}""#), "CROSSREF"), t("시작")),
                2 => format!("{}{}", t("끝"), end(&(i - 1).to_string())),
                _ => t("평문"),
            })
            .collect();
        let xml = body(&paragraphs);
        crate::work::WORK.set(0);
        let f = mapped(&xml);
        assert!(!f.unproven_fields);
        assert_eq!(f.has_disallowed_self_element.len(), n);
        let edits: Vec<usize> = (0..n).step_by(4).collect();
        // Without preservation, the first field blocks the rest of the body list.
        assert_eq!(dropped(&f, &edits).len(), n);
        let mut keep = vec![false; n];
        // A proven edit starts no propagation: the first active edit is paragraph 4.
        keep[0] = true;
        assert_eq!(f.caches_to_drop_for_text_edits(edits, &keep).len(), n - 4);
        crate::work::WORK.get()
    };
    let small = work(128);
    let medium = work(256);
    let large = work(512);
    assert!(small > 0, "the counted passes count their work");
    // Compare growth, cancelling the fixed section prefix counted once per input.
    assert!(large - medium <= 2 * (medium - small) + 8, "{small} -> {medium} -> {large}");
}

// 이것을 실패시키는 것: subList마다 부모 칸의 자식을 다시 세기, 중복 subList 승인.
#[test]
fn counted_cell_sublist_uniqueness_work_stays_linear() {
    let work = |n: usize| {
        let sublist = "<subList textDirection='HORIZONTAL' lineWrap='BREAK'>";
        let lists = format!("{sublist}{}</subList>", p("칸")).repeat(n);
        let cell = format!("<tbl><pos treatAsChar='1'/><tr><tc>{lists}</tc></tr></tbl>");
        crate::work::WORK.set(0);
        let f = flow(&p(&cell));
        let work = crate::work::WORK.get();
        assert_eq!(f.supported_owner[1..], vec![n == 1; n]);
        work
    };
    work(1);
    let small = work(128);
    let medium = work(256);
    let large = work(512);
    assert!(small > 0, "the counted passes count their work");
    assert!(large - medium <= 2 * (medium - small) + 8, "{small} -> {medium} -> {large}");
}

/// If the inventory's field and the source lists disagree (the begin and
/// the end land in different source lists), the range is not proven.
/// 이것을 실패시키는 것: 시작·끝 목록 비교 삭제(흐름 구분 무시).
#[test]
fn a_field_whose_ends_land_in_different_lists_is_unproven() {
    let xml = format!(
        "<hs:sec>{}</hs:sec>",
        body(&[
            format!("{}{}", t("편집"), begin(r#" id="77""#, "CROSSREF")),
            format!("{}{}", table(&t("칸")), end("77")),
            t("꼬리"),
        ])
    );
    let parsed =
        crate::decoder::section::parse_section(&xml, 0, &Default::default()).expect("parse");
    let section = hwpforge_core::Section::with_paragraphs(
        parsed.paragraphs,
        hwpforge_core::PageSettings::a4(),
    );
    let mut map = parsed.source.finish(&section, 0, "Contents/section0.xml");
    let inventory = hwpforge_core::field_inventory::FieldInventory::from_sections(
        std::slice::from_ref(&section),
    );
    map.prove_cache_text_tail(&xml, &inventory, 0);
    assert!(!map.cache_flow.unproven_fields, "premise: the inventory proves the range");
    let end_path = inventory.fields()[0].end().clone().expect("closed");
    let (_, source) = XmlSource::read(&xml).unwrap();
    let mut f = ParagraphFlow::from_source(&source);
    f.scan_text_tail(&source, &xml).expect("premise: the section scans");
    let cell = f.spans[2].start;
    f.project_fields(&inventory, |path| {
        if path == end_path {
            return Some(cell);
        }
        map.node(&hwpforge_core::node_path::CorePath::new(0, path))
            .and_then(|n| n.ranges().first().map(|r| r.start))
    });
    assert!(f.unproven_fields);
    assert_eq!(dropped(&f, &[0]), [0, 1, 3]);
}

/// A flow whose gates were never built (every decode; only the preserving
/// patch builds them) cannot prove a plain tail, so a body edit keeps the
/// legacy removal set.
/// 이것을 실패시키는 것: from_source 가 가로쓰기 구역 증거를 미리 채우기.
#[test]
fn an_unprojected_flow_keeps_legacy_invalidation() {
    let xml = format!(
        "<sec>{}{}</sec>",
        "<p><secPr textDirection='HORIZONTAL'/><run>편집</run><linesegarray/></p>",
        "<p><run>뒤</run><linesegarray/></p>"
    );
    let (_, source) = XmlSource::read(&xml).unwrap();
    let f = ParagraphFlow::from_source(&source);
    assert_eq!(dropped(&f, &[0]), [0, 1]);
}

/// Many `pos` children of one element with a long `textWrap`: the scan
/// reads each start tag once, so the counted attribute bytes (and visited
/// elements) grow linearly with the input.
/// 이것을 실패시키는 것: pos 마다 부모 시작 태그를 다시 읽기.
#[test]
fn pos_children_read_their_parent_start_tag_once() {
    let work = |n: usize| {
        let xml = format!(
            "<sec><p><secPr textDirection='HORIZONTAL'/><run><unknown textWrap='{}'>{}</unknown></run><linesegarray/></p></sec>",
            "X".repeat(100 * n),
            "<pos/>".repeat(n)
        );
        let (_, source) = XmlSource::read(&xml).unwrap();
        let mut f = ParagraphFlow::from_source(&source);
        crate::work::WORK.set(0);
        f.scan_text_tail(&source, &xml).expect("scans");
        crate::work::WORK.get()
    };
    let small = work(500);
    let large = work(1000);
    assert!(small >= 100 * 500, "the parent's attribute bytes are counted: {small}");
    assert!(large <= 2 * small + 8, "{small} -> {large}");
}

// 이것을 실패시키는 것: fieldBegin을 꼬리 판정과 자기 문단 판정에서 각각 읽기.
#[test]
fn field_begin_start_tag_is_read_once() {
    let work = |n: usize| {
        let xml = format!(
            "<sec><p paraPrIDRef='0'><run charPrIDRef='0'><ctrl><fieldBegin type='CLICK_HERE' name='{}'/></ctrl><t>가</t></run></p></sec>",
            "X".repeat(n)
        );
        let (_, source) = XmlSource::read(&xml).unwrap();
        let mut f = ParagraphFlow::from_source(&source);
        crate::work::WORK.set(0);
        f.scan_text_tail(&source, &xml).unwrap();
        assert!(!f.has_disallowed_element[0]);
        assert!(!f.has_disallowed_self_element[0]);
        crate::work::WORK.get()
    };
    assert_eq!(work(200) - work(100), 100);
}

/// The document's virtual root has an empty range: it is never a start tag.
/// 이것을 실패시키는 것: 기록된 범위를 확인 없이 잘라 읽기.
#[test]
fn an_empty_element_range_is_not_a_start_tag() {
    let empty = hwpforge_core::source_xml::XmlElement::default();
    assert!(super::source_start("<a/>", &empty).is_none());
}

/// The gates are read from a fresh copy of the section text; if its
/// paragraphs do not match the flow's (different or extra paragraphs), the
/// flow stays unproven.
/// 이것을 실패시키는 것: 문단 범위 대조 삭제, 끝의 문단 수 대조 삭제.
#[test]
fn a_scan_of_other_text_leaves_the_flow_unproven() {
    let xml = format!("<sec>{}{}</sec>", p("<secPr textDirection='HORIZONTAL'/>편집"), p("뒤"));
    let (_, source) = XmlSource::read(&xml).unwrap();
    let longer = format!("{}{}</sec>", &xml[..xml.len() - "</sec>".len()], p("더"));
    let (_, longer_source) = XmlSource::read(&longer).unwrap();
    let other =
        format!("<sec>{}{}</sec>", p("<secPr textDirection='HORIZONTAL'/>다른 편집"), p("뒤"));
    for (flow_source, scanned, expected) in [
        // Another text with the same paragraph count.
        (&source, other.as_str(), vec![0, 1]),
        // One paragraph more in the scanned text.
        (&source, longer.as_str(), vec![0, 1]),
        // One paragraph fewer in the scanned text.
        (&longer_source, xml.as_str(), vec![0, 1, 2]),
    ] {
        let (_, scanned_source) = XmlSource::read(scanned).unwrap();
        let mut f = ParagraphFlow::from_source(flow_source);
        f.prove_text_tail(&scanned_source, scanned, &Default::default(), |_| None);
        assert!(f.unproven_fields, "{scanned}");
        assert_eq!(dropped(&f, &[0]), expected, "{scanned}");
    }
}

// 이것을 실패시키는 것: keep을 그대로 전파 면제로 신뢰, 성공한 편집에서 장벽 전파, 구역 실패 무시.
#[test]
fn self_keep_never_overrides_nested_blocked_or_section_invalidation() {
    // 이것을 실패시키는 것: 최종 제거 집합에서 성공한 뒤 본문 문단을 강제로 제외.
    let blocked = flow(&format!("{}{}{}{}", p("가"), p("<autoNum/>"), p("나"), p("다")));
    let ranges = blocked.caches_to_drop_for_text_edits([0, 2], &[false, false, true, false]);
    let actual: Vec<_> = blocked
        .caches
        .iter()
        .enumerate()
        .filter_map(|(p, caches)| caches.iter().any(|c| ranges.contains(c)).then_some(p))
        .collect();
    assert_eq!(actual, [0, 1, 2, 3]);
    let cases = [
        (flow(&format!("{}{}{}", p("가"), p("나"), p("다"))), vec![1], vec![]),
        (
            flow(&format!("{}{}{}{}", p("가"), p("<autoNum/>"), p("나"), p("다"))),
            vec![0, 2],
            vec![], // Proven body edits no longer reach the later barrier.
        ),
        (
            flow(&format!(
                "{}{}",
                p(&format!("<tbl><pos treatAsChar='1'/><subList>{}</subList></tbl>", p("칸"))),
                p("뒤")
            )),
            vec![0, 1],
            vec![0, 1, 2],
        ),
        (
            flow(&format!(
                "{}{}",
                p(&format!("<tbl><pos treatAsChar='1'/><subList>{}</subList></tbl>", p("칸"))),
                p("뒤")
            )),
            vec![1],
            vec![0, 1, 2],
        ),
        (
            flow(&format!("{}{}", p("<colPr colCount='2' sameSz='0'/>"), p("나"))),
            vec![0],
            vec![0, 1],
        ),
    ];
    // 내부 단독 편집을 혼합 편집보다 먼저 검사해 소유 관계 실패를 직접 드러낸다.
    for (case, (flow, changed, expected)) in cases.into_iter().enumerate().rev() {
        crate::work::WORK.set(0);
        let ranges = flow
            .caches_to_drop_for_text_edits(changed.iter().copied(), &vec![true; flow.spans.len()]);
        if case == 3 {
            // 이것을 실패시키는 것: 내부 단독 편집에서 제거 질의를 중복 실행.
            assert_eq!(crate::work::WORK.get(), 17, "nested-only uses two propagation passes");
        }
        let actual: Vec<_> = flow
            .caches
            .iter()
            .enumerate()
            .filter_map(|(p, caches)| caches.iter().any(|c| ranges.contains(c)).then_some(p))
            .collect();
        assert_eq!(actual, expected, "changed paragraphs: {changed:?}");
    }
}

// 이것을 실패시키는 것: SourceMap/flow 범위 동등성 또는 직접 캐시 배열 유일성 검사 삭제.
#[test]
fn self_evidence_requires_the_exact_body_span() {
    let xml = "<sec><p paraPrIDRef='3'><run charPrIDRef='0'><secPr textDirection='HORIZONTAL'><grid charGrid='0' wonggojiFormat='0'/></secPr><t>가</t></run><linesegarray><lineseg/></linesegarray></p></sec>";
    let (_, source) = XmlSource::read(xml).unwrap();
    let mut f = ParagraphFlow::from_source(&source);
    f.prove_text_tail(&source, xml, &Default::default(), |_| None);
    assert_eq!(f.self_candidate(0, &f.spans[0]), Some(3));
    assert_eq!(f.self_candidate(0, &(f.spans[0].start + 1..f.spans[0].end)), None);
    let duplicate = xml.replace("</p>", "<linesegarray><lineseg/></linesegarray></p>");
    let (_, source) = XmlSource::read(&duplicate).unwrap();
    let mut f = ParagraphFlow::from_source(&source);
    f.prove_text_tail(&source, &duplicate, &Default::default(), |_| None);
    assert_eq!(f.self_candidate(0, &f.spans[0]), None);
}

// 이것을 실패시키는 것: 자기 허용 목록에 tab/lineBreak/nbSpace/fwSpace/autoNum 또는 임의 ctrl 자식을 허용, 빈 run의 다른 글자 모양 무시.
#[test]
fn self_element_allowlist_is_distinct_from_the_untouched_tail() {
    for (content, allowed) in [
        ("<t>가</t>", true),
        ("</run><run charPrIDRef='1'><t/>", false),
        ("<t>가<tab/></t>", false),
        ("<t>가<lineBreak/></t>", false),
        ("<t>가<nbSpace/></t>", false),
        ("<t>가<fwSpace/></t>", false),
        ("<ctrl><autoNum/></ctrl>", false),
        ("<ctrl><footNote/></ctrl>", false),
        ("<ctrl><unknown/></ctrl>", false),
        ("<ctrl><colPr colCount='1'/></ctrl>", true),
        ("<secPr textDirection='HORIZONTAL'><grid/><unknown/></secPr>", true),
    ] {
        let xml = format!("<sec><p paraPrIDRef='0'><run charPrIDRef='0'>{content}</run><linesegarray><lineseg/></linesegarray></p></sec>");
        let (_, source) = XmlSource::read(&xml).unwrap();
        let mut f = ParagraphFlow::from_source(&source);
        f.prove_text_tail(&source, &xml, &Default::default(), |_| None);
        assert_eq!(f.self_candidate(0, &f.spans[0]).is_some(), allowed, "{content}");
    }
}

// 이것을 실패시키는 것: 편집 불가 CLICK_HERE/BOOKMARK 자기 투영 생략, 필드 범위의 시작 문단만 표시.
#[test]
fn bookmark_and_other_field_ranges_block_self_including_middle_and_end() {
    for kind in ["BOOKMARK", "CROSSREF", "CLICK_HERE"] {
        let xml = body(&[
            format!("{}{}", t("앞"), begin(r#" id="77""#, kind)),
            t("본문"),
            format!("{}{}", end("77"), t("뒤")),
            t("꼬리"),
        ]);
        let f = mapped(&xml);
        assert!(!f.unproven_fields);
        assert_eq!(f.has_disallowed_self_element[..3], [true, true, true]);
    }
}

// 이것을 실패시키는 것: 위치 없는 BOOKMARK/편집 불가 CLICK_HERE의 자기 차단과 꼬리 입증 실패를 다시 합치기.
#[test]
fn unlocated_bookmark_and_uneditable_clickhere_block_all_self_caches_only() {
    for kind in ["BOOKMARK", "CLICK_HERE"] {
        let xml = format!(
            "<hs:sec>{}</hs:sec>",
            body(&[
                format!("{}{}", t("앞"), begin(r#" id="77""#, kind)),
                t("본문"),
                format!("{}{}", end("77"), t("뒤")),
                t("꼬리"),
            ])
        );
        let parsed =
            crate::decoder::section::parse_section(&xml, 0, &Default::default()).expect("parse");
        let section = hwpforge_core::Section::with_paragraphs(
            parsed.paragraphs,
            hwpforge_core::PageSettings::a4(),
        );
        let inventory = hwpforge_core::field_inventory::FieldInventory::from_sections(
            std::slice::from_ref(&section),
        );
        assert_eq!(inventory.fields().len(), 1);
        assert_eq!(inventory.fields()[0].kind(), kind);
        assert_ne!(
            inventory.fields()[0].editability(),
            hwpforge_core::field_inventory::FieldEditability::Editable
        );
        let (_, source) = XmlSource::read(&xml).unwrap();
        let mut f = ParagraphFlow::from_source(&source);
        f.prove_text_tail(&source, &xml, &inventory, |_| None);
        assert!(!f.unproven_fields, "{kind}");
        assert_eq!(f.has_disallowed_self_element, [true, true, true, true], "{kind}");
    }
}

// 이것을 실패시키는 것: 구역 증거를 한 진입점에서만 수집, 중복 grid 또는 secPr 허용.
#[test]
fn section_adapters_agree_on_explicit_grid_evidence() {
    let duplicate_section = "<secPr><grid charGrid='0' wonggojiFormat='0'/></secPr><secPr><grid charGrid='0' wonggojiFormat='0'/></secPr>";
    let duplicate_grid = "<secPr><grid charGrid='0' wonggojiFormat='0'/><grid charGrid='0' wonggojiFormat='0'/></secPr>";
    let horizontal_grid = "<secPr textDirection='HORIZONTAL'><grid charGrid='0' wonggojiFormat='0' lineGrid='9'/></secPr>";
    for (properties, expected) in [
        (horizontal_grid, true),
        (
            "<secPr textDirection='HORIZONTAL'><grid charGrid='1' wonggojiFormat='0'/></secPr>",
            false,
        ),
        (
            "<secPr textDirection='HORIZONTAL'><grid charGrid='0' wonggojiFormat='1'/></secPr>",
            false,
        ),
        ("<secPr textDirection='HORIZONTAL'><grid charGrid='0'/></secPr>", false),
        ("<secPr textDirection='HORIZONTAL'/>", false),
        (duplicate_grid, false),
        (duplicate_section, false),
    ] {
        let xml = format!(
            "<sec><p paraPrIDRef='0'><run charPrIDRef='0'>{properties}<t>가</t></run></p></sec>"
        );
        let (_, source) = XmlSource::read(&xml).unwrap();
        let mut f = ParagraphFlow::from_source(&source);
        f.prove_text_tail(&source, &xml, &Default::default(), |_| None);
        let cell = crate::line_keep::SectionEvidence::read(&xml).unwrap();
        assert_eq!(crate::line_keep::source_allows(&cell, true), expected);
        assert_eq!(crate::line_keep::source_allows(&f.section_evidence, true), expected);
        assert!(!crate::line_keep::source_allows(&cell, false));
    }
}

// 이것을 실패시키는 것: 직접 소유 계층 생략, subList 유일성 생략, 방향/줄바꿈 누락 승인,
// 자기 비허용 검사 생략, 구역 검사 전에 성공한 편집을 제거.
#[test]
fn direct_cell_source_ownership_and_gates_are_required_even_with_keep() {
    for (attrs, extra, content, section, expected) in [
        ("textDirection='HORIZONTAL' lineWrap='BREAK'", "", "글", "", vec![]),
        ("lineWrap='BREAK'", "", "글", "", vec![1, 2, 3]),
        ("textDirection='HORIZONTAL'", "", "글", "", vec![1, 2, 3]),
        ("textDirection='VERTICAL' lineWrap='BREAK'", "", "글", "", vec![1, 2, 3]),
        ("textDirection='HORIZONTAL' lineWrap='SQUEEZE'", "", "글", "", vec![1, 2, 3]),
        ("textDirection='HORIZONTAL' lineWrap='BREAK'", "<subList/>", "글", "", vec![1, 2, 3]),
        ("textDirection='HORIZONTAL' lineWrap='BREAK'", "", "<autoNum/>", "", vec![1, 2, 3]),
        (
            "textDirection='HORIZONTAL' lineWrap='BREAK'",
            "",
            "글",
            "<colPr colCount='2' sameSz='0'/>",
            vec![1, 2, 3],
        ),
    ] {
        let cell = format!(
            "<tbl><pos treatAsChar='1'/><tr><tc><subList {attrs}>{}</subList>{extra}</tc></tr></tbl>",
            p(content)
        );
        let f = flow(&format!("{}{}{}", p(section), p(&cell), p("뒤")));
        let ranges = f.caches_to_drop_for_text_edits([2], &[true; 4]);
        let actual: Vec<_> = f
            .caches
            .iter()
            .enumerate()
            .filter_map(|(p, c)| c.iter().any(|r| ranges.contains(r)).then_some(p))
            .collect();
        assert_eq!(actual, expected, "{attrs} {extra} {content} {section}");
    }
    // A table ancestor alone cannot establish direct body-cell ownership.
    let table_open =
        "<tbl><pos treatAsChar='1'/><tr><tc><subList textDirection='HORIZONTAL' lineWrap='BREAK'>";
    for (open, close, expected) in [
        ("<drawText><subList>", "</subList></drawText>", vec![0, 1, 2, 3]),
        ("<footNote><subList>", "</subList></footNote>", vec![0, 1, 2, 3]),
        ("<header><subList>", "</subList></header>", vec![1, 2]),
        ("<footer><subList>", "</subList></footer>", vec![1, 2]),
        (table_open, "</subList></tc></tr></tbl>", vec![0, 1, 2, 3]),
    ] {
        let inner = p(&format!("{table_open}{}</subList></tc></tr></tbl>", p("칸")));
        let f = flow(&format!("{}{}", p(&format!("{open}{inner}{close}")), p("뒤")));
        let ranges = f.caches_to_drop_for_text_edits([2], &[true; 4]);
        let actual: Vec<_> = f
            .caches
            .iter()
            .enumerate()
            .filter_map(|(p, c)| c.iter().any(|r| ranges.contains(r)).then_some(p))
            .collect();
        assert_eq!(actual, expected, "{open}");
    }
}
