//! A list paragraph's left margin is reported unless the Markdown as written
//! gives it back: what counts is what a paragraph was written as, at what
//! depth, and every paragraph using its definition.

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::control::Control;
use hwpforge_core::{
    Document, ParagraphListRef, ParagraphMeasure, RunContent, StyleLookup, Validated,
};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

use super::guarded_tests::cases::*;
use super::measure::{Held, ListMeasures};
use super::MdWarning;

/// The test styles, with the left margins given by shape.
struct Margins<'a>(&'a [(usize, i32)]);

impl StyleLookup for Margins<'_> {
    fn char_font_name(&self, id: CharShapeIndex) -> Option<&str> {
        Styles.char_font_name(id)
    }
    fn para_heading_level(&self, id: ParaShapeIndex) -> Option<u8> {
        Styles.para_heading_level(id)
    }
    fn para_list(&self, id: ParaShapeIndex) -> Option<ParagraphListRef> {
        Styles.para_list(id)
    }
    fn para_style_name(&self, id: ParaShapeIndex) -> Option<&str> {
        Styles.para_style_name(id)
    }
    fn para_indent_left(&self, id: ParaShapeIndex) -> Option<ParagraphMeasure> {
        let (_, margin) = self.0.iter().find(|(shape, _)| *shape == id.get())?;
        Some(ParagraphMeasure::HwpUnit(HwpUnit::new(*margin).unwrap()))
    }
}

fn reported(warnings: &[MdWarning]) -> Vec<usize> {
    let lost = warnings.iter().filter_map(|w| match w {
        MdWarning::ParagraphMeasureLost { definition, .. } => Some(*definition),
        _ => None,
    });
    lost.collect()
}

/// The definitions the styled and the lossy view report.
fn lost(doc: &Document<Validated>, margins: &[(usize, i32)]) -> [Vec<usize>; 2] {
    let styled = super::styled::encode_styled(doc, &Margins(margins)).warnings;
    let (_, lossy) = super::lossy::encode_with_styles_report(doc, &Margins(margins)).unwrap();
    [reported(&styled), reported(&lossy)]
}

/// 이것을 실패시키는 것: 항목 깊이를 0 으로 · 이어지는 문단·코드 기록 삭제.
#[test]
fn margins_of_what_was_written_are_given_back() {
    let doc = doc(vec![
        p(BULLET, vec![t("a")]),
        p(CONT0, vec![t("b")]),
        p(CODE_CONT0, vec![t("c")]),
        p(BULLET1, vec![t("d")]),
        p(CONT1, vec![t("e")]),
    ]);
    let margins =
        &[(BULLET, 2000), (CONT0, 4000), (CODE_CONT0, 4000), (BULLET1, 4000), (CONT1, 6000)];
    assert_eq!(lost(&doc, margins), [Vec::<usize>::new(), Vec::new()]);
}

/// An item with no parent is written at the top, so it reads back there.
/// 이것을 실패시키는 것: 깊이 대신 문단 수준으로 기대값 찾기.
#[test]
fn an_item_is_measured_at_the_depth_it_was_written() {
    let doc = doc(vec![p(BULLET1, vec![t("x")])]);
    assert_eq!(lost(&doc, &[(BULLET1, 4000)]), [vec![BULLET1], vec![BULLET1]]);
    assert_eq!(lost(&doc, &[(BULLET1, 2000)]), [Vec::<usize>::new(), Vec::new()]);
}

/// 이것을 실패시키는 것: 이어붙지 못한 문단도 기록하기.
#[test]
fn a_continuation_with_no_open_item_is_reported() {
    for shape in [CONT0, CODE_CONT0] {
        let doc = doc(vec![p(shape, vec![t("x")])]);
        assert_eq!(lost(&doc, &[(shape, 4000)]), [vec![shape], vec![shape]]);
    }
}

/// 이것을 실패시키는 것: 제목으로 쓴 문단을 항목으로 기록하기.
#[test]
fn a_heading_over_a_list_is_reported() {
    let doc = doc(vec![p(HEADING_NUMBER, vec![t("x")])]);
    assert_eq!(lost(&doc, &[(HEADING_NUMBER, 2000)]), [vec![HEADING_NUMBER], vec![HEADING_NUMBER]]);
}

/// 이것을 실패시키는 것: 사용처 수와 비교하지 않기.
#[test]
fn a_definition_used_outside_the_list_is_reported() {
    let doc =
        doc(vec![p(BULLET, vec![t("a")]), body(vec![table(one_cell(p(BULLET, vec![t("b")])))])]);
    assert_eq!(lost(&doc, &[(BULLET, 2000)]), [vec![BULLET], vec![BULLET]]);
}

/// 이것을 실패시키는 것: 메모 앵커로 내려가지 않는 순회.
#[test]
fn measures_of_a_memo_anchor_are_reported() {
    let held = table(one_cell(p(BODY + 90, vec![t("x")])));
    let doc = doc(vec![body(vec![anchored_memo(vec![held])])]);
    assert_eq!(lost(&doc, &[(BODY + 90, 1234)]), [vec![BODY + 90], vec![BODY + 90]]);
}

/// 이것을 실패시키는 것: 그림 캡션으로 내려가지 않는 순회.
#[test]
fn measures_of_a_picture_caption_are_reported() {
    let captioned = |shape| image("p.png", Some(vec![p(shape, vec![t("x")])]));
    let alone = doc(vec![body(vec![captioned(BODY + 90)])]);
    assert_eq!(lost(&alone, &[(BODY + 90, 1234)]), [vec![BODY + 90], vec![BODY + 90]]);
    let shared = doc(vec![p(BULLET, vec![t("a")]), body(vec![captioned(BULLET)])]);
    assert_eq!(lost(&shared, &[(BULLET, 2000)]), [vec![BULLET], vec![BULLET]]);
}

/// The first text of an empty item reads back as the item's first paragraph,
/// at the item's margin.
/// 이것을 실패시키는 것: 빈 항목의 첫 글을 이어지는 문단으로 기록하기.
#[test]
fn the_first_text_of_an_empty_item_is_reported() {
    let doc = doc(vec![p(BULLET, vec![t("")]), p(CONT0, vec![t("b")])]);
    assert_eq!(lost(&doc, &[(BULLET, 2000), (CONT0, 4000)]), [vec![CONT0], vec![CONT0]]);
}

/// An item that holds a table, in a text box too, reads back as an empty
/// item and a block.
/// 이것을 실패시키는 것: 표 검사 삭제 · 문단에 직접 든 표만 보기.
#[test]
fn an_item_of_one_table_is_reported() {
    let held = || table(one_cell(body(vec![t("x")])));
    for run in [held(), text_box(vec![body(vec![held()])])] {
        let doc = doc(vec![p(BULLET, vec![run])]);
        assert_eq!(lost(&doc, &[(BULLET, 2000)]), [vec![BULLET], vec![BULLET]]);
    }
}

/// A table first in an empty item stays a continuation.
/// 이것을 실패시키는 것: 표를 빈 항목의 첫 글로 보기.
#[test]
fn a_table_first_in_an_empty_item_is_given_back() {
    let held = table(one_cell(body(vec![t("x")])));
    let doc = doc(vec![p(BULLET, vec![t("")]), p(CONT0, vec![held])]);
    assert_eq!(lost(&doc, &[(BULLET, 2000), (CONT0, 4000)]), [vec![], Vec::<usize>::new()]);
}

/// Text before a table is the empty item's first paragraph.
/// 이것을 실패시키는 것: 표 하나뿐인 문단으로 한정하지 않기.
#[test]
fn text_before_a_table_fills_an_empty_item() {
    let held = table(one_cell(body(vec![t("x")])));
    let doc = doc(vec![p(BULLET, vec![t("")]), p(CONT0, vec![t("a"), held])]);
    assert_eq!(lost(&doc, &[(BULLET, 2000), (CONT0, 4000)]), [vec![CONT0], vec![CONT0]]);
}

/// A table only in a text box's caption is not written, so the text box's
/// text is the empty item's first paragraph.
/// 이것을 실패시키는 것: 빈 항목 판정에 표 재귀 검사 쓰기.
#[test]
fn a_table_that_is_not_written_fills_nothing() {
    let mut boxed = text_box(vec![body(vec![t("x")])]);
    let RunContent::Control(control) = &mut boxed.content else { unreachable!() };
    let Control::TextBox { caption, .. } = control.as_mut() else { unreachable!() };
    let held = body(vec![table(one_cell(body(vec![t("y")])))]);
    *caption = Some(Caption::new(vec![held], CaptionSide::Bottom));
    let doc = doc(vec![p(BULLET, vec![t("")]), p(CONT0, vec![boxed])]);
    assert_eq!(lost(&doc, &[(BULLET, 2000), (CONT0, 4000)]), [vec![CONT0], vec![CONT0]]);
}

/// A picture stays in the item's first paragraph.
/// 이것을 실패시키는 것: 글만 담은 항목으로 한정하기.
#[test]
fn an_item_with_a_picture_is_given_back() {
    for runs in [vec![image("p.png", None)], vec![t("a"), image("p.png", None)]] {
        let doc = doc(vec![p(BULLET, runs)]);
        assert_eq!(lost(&doc, &[(BULLET, 2000)]), [vec![], Vec::<usize>::new()]);
    }
}

/// Text with a blank line reads back as two paragraphs, the second at the
/// continuation's margin.
/// 이것을 실패시키는 것: 빈 줄 검사 삭제.
#[test]
fn an_item_of_two_markdown_paragraphs_is_reported() {
    let doc = doc(vec![p(BULLET, vec![t("a\n\nb")])]);
    assert_eq!(lost(&doc, &[(BULLET, 2000)]), [vec![BULLET], vec![BULLET]]);
}

/// Styled writes a code-font item as an empty item holding a fence; lossy
/// has no code font and writes its text at the marker.
/// 이것을 실패시키는 것: `held` 항목을 기록하기.
#[test]
fn a_code_font_item_is_reported_where_it_moves() {
    let doc = doc(vec![p(BULLET, vec![styled_t("x", CODE)])]);
    assert_eq!(lost(&doc, &[(BULLET, 2000)]), [vec![BULLET], vec![]]);
}

/// 이것을 실패시키는 것: 평문으로 물러난 문단을 기록에서 지우지 않기.
#[test]
fn a_paragraph_that_fell_back_is_reported() {
    let doc = doc(vec![p(BULLET, vec![t("a")])]);
    let id = ParaShapeIndex::new(BULLET);
    let margins = Margins(&[(BULLET, 2000)]);
    let run = |fell_back: Vec<(usize, usize)>| {
        let mut measures = ListMeasures::default();
        measures.written((0, 0), id, Held::Item(0));
        let mut warnings = Vec::new();
        measures.report(fell_back, &doc, &margins, &mut warnings);
        reported(&warnings)
    };
    assert_eq!(run(vec![]), Vec::<usize>::new());
    assert_eq!(run(vec![(0, 0)]), [BULLET]);
}
