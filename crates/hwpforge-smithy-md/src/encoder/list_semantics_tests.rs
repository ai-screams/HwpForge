//! Typed list semantics in the readable views given the original styles
//! (#296): a continuation joins the open item of its level, a list paragraph
//! of one table or picture holds it under its marker, and a list paragraph
//! the views write without its list semantics is reported, one warning per
//! paragraph.

use hwpforge_core::{
    Document, ParagraphListRef, StyleLookup, Table, TableCell, TableRow, Validated,
};
use hwpforge_foundation::{BulletIndex, NumberingIndex};
use pulldown_cmark::{Event, Parser};

use super::guarded_tests::cases::*;
use super::list_format::{self, item_depths};
use super::{md_options, MdWarning};

/// The styled view and the lossy view given the same styles: (name,
/// Markdown, warnings).
fn views(doc: &Document<Validated>) -> [(&'static str, String, Vec<MdWarning>); 2] {
    let styled = super::styled::encode_styled(doc, &Styles);
    let (lossy, warnings) = super::lossy::encode_with_styles_report(doc, &Styles).unwrap();
    [("styled", styled.markdown, styled.warnings), ("lossy", lossy, warnings)]
}

fn depth(markdown: &str, text: &str) -> Option<usize> {
    item_depths(markdown)
        .into_iter()
        .find(|(t, _)| t == text)
        .unwrap_or_else(|| panic!("{text:?} not in {markdown:?}"))
        .1
}

/// How many list items hold the HTML that has `needle` (an HTML table).
fn html_depth(markdown: &str, needle: &str) -> usize {
    use pulldown_cmark::{Tag, TagEnd};
    let mut depth = 0;
    for event in Parser::new_ext(markdown, md_options()) {
        match event {
            Event::Start(Tag::Item) => depth += 1,
            Event::End(TagEnd::Item) => depth -= 1,
            Event::Html(html) | Event::InlineHtml(html) if html.contains(needle) => return depth,
            _ => {}
        }
    }
    panic!("{needle:?} not in {markdown:?}")
}

fn list_losses(warnings: &[MdWarning]) -> Vec<(usize, &str, ParagraphListRef, &'static str)> {
    warnings
        .iter()
        .filter_map(|w| match w {
            MdWarning::ListSemanticsLost { section, path, list, reason } => {
                Some((*section, path.as_str(), *list, *reason))
            }
            _ => None,
        })
        .collect()
}

/// A continuation of level 0 after a level-1 item is the parent's: the
/// child closes, its text, table or empty head too. After an empty child
/// the parent's tail is a block of its own (a blank line before it).
/// 이것을 실패시키는 것: 마지막 항목 열 사용(깊은 항목 닫기 삭제) · 빈 머리 표시 남기기.
#[test]
fn continuation_returns_to_its_parent() {
    let tail_table = table(one_cell(body(vec![t("x")])));
    let filled = doc(vec![
        p(BULLET, vec![t("parent")]),
        p(BULLET1, vec![t("child")]),
        p(CONT0, vec![t("tail")]),
        p(BULLET1, vec![t("child2")]),
        p(CONT0, vec![tail_table]),
    ]);
    for (view, md, _) in views(&filled) {
        let depths: Vec<_> = ["parent", "child", "tail", "child2", "x"]
            .into_iter()
            .map(|text| depth(&md, text))
            .collect();
        assert_eq!(depths, [Some(1), Some(2), Some(1), Some(2), Some(1)], "{view}: {md}");
    }
    let empty =
        doc(vec![p(BULLET, vec![t("")]), p(BULLET1, vec![t("")]), p(CONT0, vec![t("tail")])]);
    for (view, md, _) in views(&empty) {
        assert_eq!(md, "-\n  -\n\n  tail", "{view}");
    }
}

/// A continuation of level 0 after a numbered child returns to the
/// numbered parent: at the parent's content column (`1. ` is three
/// columns), not the child's.
/// 이것을 실패시키는 것: 깊은 항목 닫기 삭제 · 마지막 항목 열 사용.
#[test]
fn continuation_returns_to_its_numbered_parent() {
    let doc = doc(vec![
        p(NUMBER, vec![t("parent")]),
        p(NUMBER1, vec![t("child")]),
        p(CONT0, vec![t("tail")]),
    ]);
    for (view, md, _) in views(&doc) {
        let depths: Vec<_> =
            ["parent", "child", "tail"].into_iter().map(|text| depth(&md, text)).collect();
        assert_eq!(depths, [Some(1), Some(2), Some(1)], "{view}: {md}");
        assert!(md.lines().any(|line| line == "   tail"), "{view}: {md:?}");
    }
}

/// The reserved continuation name on a shape with its own list or heading
/// semantics is no continuation, in both views: the bullet is a nested
/// item, the heading a heading.
/// 이것을 실패시키는 것: 공용 판정의 목록 검사 삭제 · 제목 검사 삭제.
#[test]
fn continuation_name_on_a_list_or_heading_shape_is_ignored() {
    let doc = doc(vec![
        p(BULLET, vec![t("parent")]),
        p(NAMED_BULLET1, vec![t("child")]),
        p(NAMED_HEADING, vec![t("title")]),
    ]);
    for (view, md, _) in views(&doc) {
        let depths: Vec<_> =
            ["parent", "child", "title"].into_iter().map(|text| depth(&md, text)).collect();
        assert_eq!(depths, [Some(1), Some(2), Some(0)], "{view}: {md}");
        assert!(md.ends_with("\n# title"), "{view}: {md:?}");
    }
    // An item closes the open items a continuation would, so the bullet's
    // output alone cannot tell them apart; the rule says it is no
    // continuation.
    let levels: Vec<_> = [NAMED_BULLET1, NAMED_HEADING, CONT0]
        .into_iter()
        .map(|shape| list_format::continuation_level(&p(shape, vec![t("x")]), &Styles))
        .collect();
    assert_eq!(levels, [None, None, Some(0)]);
}

/// A shape whose lookup gives both a heading and a number list is written
/// as the heading (the classification order), so its list is lost: both
/// views write `## x` and report it once.
/// 이것을 실패시키는 것: lossy 의 한 곳 판정 삭제 · styled 의 한 곳 판정 삭제.
#[test]
fn heading_over_a_list_is_reported_once() {
    let doc = doc(vec![p(HEADING_NUMBER, vec![t("x")])]);
    let number = ParagraphListRef::Number { numbering_id: NumberingIndex::new(0), level: 0 };
    for (view, md, warnings) in views(&doc) {
        assert_eq!(md, "## x", "{view}");
        assert_eq!(
            list_losses(&warnings),
            [(0, "paragraphs[0]", number, list_format::BODY_CARRIER_PROJECTION)],
            "{view}"
        );
    }
}

/// A body paragraph of code-font text with a table, or with a picture, is
/// no code block in the styled view: it is a body paragraph that keeps
/// the table or the picture.
/// 이것을 실패시키는 것: `is_code_paragraph` 의 표·그림 검사 삭제.
#[test]
fn code_font_text_with_a_block_is_a_body_paragraph() {
    for (block, want) in [
        (table(one_cell(body(vec![t("w")]))), "let\n| w |\n| --- |"),
        (image("p.png", None), "let ![p](images/p.png)"),
    ] {
        let doc = doc(vec![body(vec![styled_t("let", CODE), block])]);
        let md = super::styled::encode_styled(&doc, &Styles).markdown;
        assert_eq!(md, want, "a code block holds only text");
    }
}

/// A list paragraph the styled view writes as a code line (all its text
/// in a code font) loses its list: one loss each, a checked bullet too. A
/// code-font list paragraph with a table is no code line: it stays an item
/// holding its table, no loss. The lossy view reads no code font: each is
/// an item, the table written outside its item as always
/// (`lossy_block_outside_its_item_is_reported`).
/// 이것을 실패시키는 것: 한 곳의 보고 삭제 · 코드 문단을 항목으로 세기 · 코드 판정의
/// 표·그림 검사 삭제.
#[test]
fn code_list_paragraph_is_reported_once() {
    let doc = doc(vec![
        p(NUMBER, vec![styled_t("x", CODE)]),
        p(CHECKED, vec![styled_t("y", CODE)]),
        p(BULLET, vec![styled_t("z", CODE), table(one_cell(body(vec![t("w")])))]),
    ]);
    let number = ParagraphListRef::Number { numbering_id: NumberingIndex::new(0), level: 0 };
    let checked =
        ParagraphListRef::CheckBullet { bullet_id: BulletIndex::new(1), level: 0, checked: true };
    let [(_, styled, styled_warnings), (_, lossy, lossy_warnings)] = views(&doc);
    assert_eq!((depth(&styled, "x\ny"), depth(&styled, "w")), (None, Some(1)), "{styled}");
    assert_eq!(
        list_losses(&styled_warnings),
        [
            (0, "paragraphs[0]", number, list_format::BODY_CARRIER_PROJECTION),
            (0, "paragraphs[1]", checked, list_format::BODY_CARRIER_PROJECTION),
        ]
    );
    assert_eq!(depth(&lossy, "x"), Some(1), "{lossy}");
    let bullet = ParagraphListRef::Bullet { bullet_id: BulletIndex::new(0), level: 0 };
    assert_eq!(
        list_losses(&lossy_warnings),
        [(0, "paragraphs[2]", bullet, list_format::BODY_CARRIER_PROJECTION)]
    );
}

/// A continuation of a level with no open item (a level-1 item opened
/// alone) stands alone at column 0; it is not joined to the last item.
/// 이것을 실패시키는 것: 없는 수준을 마지막 항목에 붙이기.
#[test]
fn continuation_of_a_closed_level_stands_alone() {
    let doc = doc(vec![p(BULLET1, vec![t("deep")]), p(CONT0, vec![t("tail")])]);
    for (view, md, _) in views(&doc) {
        assert_eq!((depth(&md, "deep"), depth(&md, "tail")), (Some(1), Some(0)), "{view}: {md}");
    }
}

/// A list paragraph whose one run is a table (GFM or HTML) or a picture is
/// an item whose marker holds it, with its kind and checked state (a number
/// and a checked bullet each with a table and a picture); it is not a loss.
/// The block's lines keep their bytes.
/// 이것을 실패시키는 것: 분류 전에 블록으로 반환 · 블록 줄 앞 공백 지우기.
#[test]
fn list_paragraph_holds_its_one_table() {
    let doc = doc(vec![
        p(NUMBER, vec![table(one_cell(body(vec![t("x")])))]),
        p(BODY, vec![t("between")]),
        p(BULLET, vec![table(merged(body(vec![t("y")])))]),
        p(BODY, vec![t("between2")]),
        p(CHECKED, vec![image("p.png", None)]),
        p(BODY, vec![t("between3")]),
        p(NUMBER, vec![image("q.png", None)]),
        p(BODY, vec![t("between4")]),
        p(CHECKED, vec![table(one_cell(body(vec![t("z")])))]),
    ]);
    for (view, md, warnings) in views(&doc) {
        let depths: Vec<_> =
            ["x", "p", "q", "z"].into_iter().map(|text| depth(&md, text)).collect();
        assert_eq!(depths, [Some(1); 4], "{view}: {md}");
        // The styled view draws the merged cell as HTML, the lossy view as GFM.
        let merged = if view == "styled" { Some(html_depth(&md, ">y<")) } else { depth(&md, "y") };
        assert_eq!(merged, Some(1), "{view}: {md}");
        if view == "styled" {
            // Each line of the table keeps its bytes, at the item's column.
            assert!(md.contains("\n  <tr>\n    <td colspan=\"2\">y</td>"), "{md}");
        }
        let checked = Parser::new_ext(&md, md_options())
            .filter(|e| matches!(e, Event::TaskListMarker(true)))
            .count();
        assert_eq!(checked, 2, "{view}: {md}");
        assert!(md.starts_with("1.\n"), "{view}: {md}");
        assert_eq!(list_losses(&warnings), [], "{view}");
    }
}

/// Every list paragraph of a table cell is reported on its own path, the
/// two sharing one paragraph shape too; the body paragraph holding the
/// table is not.
/// 이것을 실패시키는 것: 본문 밖 흐름 건너뛰기를 본문 흐름만 보기로.
#[test]
fn cell_list_paragraphs_are_reported_one_by_one() {
    let cell = TableCell::new(
        vec![p(NUMBER, vec![t("a")]), p(NUMBER, vec![t("b")]), p(CHECKED, vec![t("c")])],
        u(1000),
    );
    let doc = doc(vec![body(vec![table(Table::new(vec![TableRow::new(vec![cell])]))])]);
    let number = ParagraphListRef::Number { numbering_id: NumberingIndex::new(0), level: 0 };
    let checked =
        ParagraphListRef::CheckBullet { bullet_id: BulletIndex::new(1), level: 0, checked: true };
    let path = |i| format!("paragraphs[0].runs[0].table.rows[0].cells[0].paragraphs[{i}]");
    let (a, b, c) = (path(0), path(1), path(2));
    let want = [
        (0, a.as_str(), number, list_format::CONTAINER_PROJECTION),
        (0, b.as_str(), number, list_format::CONTAINER_PROJECTION),
        (0, c.as_str(), checked, list_format::CONTAINER_PROJECTION),
    ];
    for (view, _, warnings) in views(&doc) {
        assert_eq!(list_losses(&warnings), want, "{view}");
    }
}

/// The lossy view writes a table among a list paragraph's text outside its
/// item: one loss for the paragraph. The styled view keeps the table in
/// the item text and reports none.
/// 이것을 실패시키는 것: 이 표시 삭제.
#[test]
fn lossy_block_outside_its_item_is_reported() {
    let doc = doc(vec![p(
        BULLET,
        vec![
            t("a"),
            table(one_cell(body(vec![t("x")]))),
            t("b"),
            table(one_cell(body(vec![t("y")]))),
        ],
    )]);
    let [(_, styled, styled_warnings), (_, lossy, lossy_warnings)] = views(&doc);
    assert_eq!(depth(&styled, "x"), Some(1), "{styled}");
    assert_eq!(list_losses(&styled_warnings), []);
    assert_eq!(depth(&lossy, "x"), Some(0), "{lossy}");
    let bullet = ParagraphListRef::Bullet { bullet_id: BulletIndex::new(0), level: 0 };
    assert_eq!(
        list_losses(&lossy_warnings),
        [(0, "paragraphs[0]", bullet, list_format::BODY_CARRIER_PROJECTION)]
    );
}

/// A body paragraph is reported once, whatever reported it first; a
/// paragraph with no list semantics is not.
/// 이것을 실패시키는 것: 중복 검사 삭제.
#[test]
fn body_list_loss_is_once_per_paragraph() {
    let doc = doc(vec![p(BULLET, vec![t("a")]), body(vec![t("b")])]);
    let mut warnings = Vec::new();
    for (at, reason) in [
        ((0, 0), list_format::BODY_CARRIER_PROJECTION),
        ((0, 0), list_format::MARKUP_FALLBACK),
        ((0, 1), list_format::MARKUP_FALLBACK),
    ] {
        list_format::body_list_loss(&mut warnings, doc.sections(), &Styles, at, reason);
    }
    let bullet = Styles.para_list(hwpforge_foundation::ParaShapeIndex::new(BULLET)).unwrap();
    assert_eq!(
        list_losses(&warnings),
        [(0, "paragraphs[0]", bullet, list_format::BODY_CARRIER_PROJECTION)]
    );
}
