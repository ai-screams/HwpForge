use hwpforge_blueprint::markdown::{
    parse_code_continuation_style_name, parse_list_continuation_style_name,
};
use hwpforge_core::field_inventory::FieldInventory;
use hwpforge_core::{ListItemKind, Paragraph, ParagraphListRef, Section, StyleLookup};
use hwpforge_foundation::NumberingIndex;
use std::collections::HashSet;

use super::MdWarning;

/// The open list items of the Markdown being written, so that a nested item
/// starts at its parent's content column. A fixed indent per level does not
/// nest under `1. ` (three columns wide), and a level that skips one would
/// land four columns deeper and read back as a code block.
///
/// What an item's or a continuation's document text may not do: make a
/// structure the text did not mean (a list, a heading, a code block — its
/// renderer escapes a line start that could open one, as for Body text, and
/// writes the text as plain text if a structure is left) or close the item
/// that holds it (every line after the first is at the item's content
/// column). The text may still be several Markdown paragraphs, and a lossy
/// paragraph's table is a block of its own. The open items therefore change
/// only by what each top-level paragraph is, never by reading the output:
///
/// - a list item ([`Self::item`], [`Self::mapped_item`]), its text empty or
///   not: the items of its level and deeper close, it opens;
/// - a continuation of the open item of its level ([`Self::resume_continuation`]):
///   the deeper items close; with no open item of its level, all close;
/// - any other block, and a table or picture written as a block of its own
///   ([`Self::reset`]): all close.
///
/// An item with no text is written as its marker alone. CommonMark ends
/// such an item at a blank line, so the first block it holds (a deeper
/// item or a continuation) is joined to it with one line ending: the block
/// being written takes that from its first laid-out piece
/// ([`Self::take_tight`]).
///
/// A numbered item writes `1.` or `1)`: two numbered lists side by side at
/// one level of one parent (no block between them) would read as one
/// list, so a numbered item whose numbering differs from the item closed
/// just before it in the same parent (or at the top) takes the other
/// delimiter; one of the same numbering keeps it. The numbering of an item
/// is its own; it is never guessed from the output.
///
/// Hancom counts a numbering's items at each level on, across whatever is
/// between them, until an item of that numbering at a shallower level
/// starts the level again; a Markdown list starts at 1. A numbered item
/// that starts a new Markdown list while its numbering is still counting
/// at its level (after another list, a block, or its parent's
/// continuation) is therefore not shown with its number: its numbering is
/// reported once ([`Self::take_presentation_lost`]).
#[derive(Debug, Default)]
pub(super) struct ListIndent {
    /// Open items, outermost first.
    open: Vec<Open>,
    /// The item closed last at the top (no open item holds it).
    top_closed: Option<Closed>,
    /// (numbering, level) pairs whose items are counting: an item written
    /// and no item of that numbering at a shallower level since.
    counting: HashSet<(NumberingIndex, u8)>,
    /// Numberings reported, in the order met, and as a set.
    presentation_lost: Vec<NumberingIndex>,
    reported: HashSet<NumberingIndex>,
    /// The last open item has no text and holds nothing written yet.
    empty_head: bool,
    /// How the block being written joins the one before, set by its first
    /// laid-out piece: `true` when that piece is the first an empty item
    /// holds.
    separator: Option<bool>,
}

/// An open item: its level, content column, numbered group, and the item
/// it held that closed last.
#[derive(Debug, Clone, Copy)]
struct Open {
    level: u8,
    column: usize,
    group: Option<Group>,
    closed: Option<Closed>,
}

/// A numbered item's numbering and the delimiter it was written with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Group {
    numbering: NumberingIndex,
    delimiter: char,
}

/// An item that closed: its level and, numbered, its group.
#[derive(Debug, Clone, Copy)]
struct Closed {
    level: u8,
    group: Option<Group>,
}

impl ListIndent {
    /// A list item at `level`, indented to the content column of its
    /// nearest open item of a smaller level (0 for none), so a level that
    /// skips one nests one step deeper. With no text, its marker alone. A
    /// numbered item's `numbering` picks its delimiter (see the type).
    /// 이것을 실패시키는 것: 수준당 고정 칸(`numbered_levels_nest_under_numbered_items`) ·
    /// 수준으로 들여쓰기(`skipped_level_nests_one_step_deeper`) · 번호 표지 폭을
    /// 2로(`mixed_markers_nest_by_parent_marker_width`).
    pub(super) fn item(
        &mut self,
        text: &str,
        kind: ListItemKind,
        level: u8,
        checked: Option<bool>,
        numbering: Option<NumberingIndex>,
    ) -> String {
        let open = self.open.len();
        self.close_from(level);
        self.separator.get_or_insert(self.empty_head && self.open.len() == open);
        self.empty_head = text.trim().is_empty();
        let indent = self.column();
        let group = match (kind, numbering) {
            (ListItemKind::Number, Some(numbering)) => Some(self.group(level, numbering)),
            _ => None,
        };
        let marker = match (kind, group) {
            (ListItemKind::Number, Some(Group { delimiter: ')', .. })) => "1) ",
            (ListItemKind::Number, _) => "1. ",
            (ListItemKind::Bullet, _) => "- ",
        };
        self.open.push(Open { level, column: indent + marker.len(), group, closed: None });
        // A numbered item has no task marker: its checked state is dropped.
        // 이것을 실패시키는 것: 번호 항목에도 `[x]`(`numbered_list_ignores_checked_state`).
        let task = match (kind, checked) {
            (ListItemKind::Bullet, Some(true)) => "[x] ",
            (ListItemKind::Bullet, Some(false)) => "[ ] ",
            _ => "",
        };
        let first = format!("{}{marker}{task}", " ".repeat(indent));
        // A task marker is read only with whitespace after it, even at the
        // end of the document.
        // 이것을 실패시키는 것: 체크 표지 뒤 공백 지우기(`empty_task_item_keeps_its_state`).
        if self.empty_head {
            return if task.is_empty() { first.trim_end().to_string() } else { first };
        }
        lay_out(&first, text, self.column())
    }

    /// A template-mapped item (no registry list): a level-0 bullet written
    /// at column 0, its text after `- ` whatever it starts with. A marker
    /// in its text (`10. `) is text, escaped by its renderer. An empty
    /// item is written as its marker alone.
    /// 이것을 실패시키는 것: 등록 삭제(`lossy_mapped_item_parents_children`).
    pub(super) fn mapped_item(&mut self, text: &str) -> String {
        self.reset();
        self.open.push(Open { level: 0, column: 2, group: None, closed: None });
        self.separator.get_or_insert(false);
        self.empty_head = text.trim().is_empty();
        if self.empty_head {
            return "-".to_string();
        }
        lay_out("- ", text, 2)
    }

    /// A continuation paragraph of the last item, at its content column,
    /// its leading blank lines dropped (after an empty item they would end
    /// it before its first block).
    /// 이것을 실패시키는 것: `2 * (level + 1)` 칸(`numbered_continuation_stays_in_its_item`) ·
    /// 앞 빈 줄 남기기(`lossy_empty_item_holds_its_first_block`).
    pub(super) fn continuation(&mut self, text: &str) -> String {
        self.held();
        let head = std::mem::take(&mut self.empty_head);
        self.separator.get_or_insert(head);
        let column = self.column();
        lay_out(&" ".repeat(column), text.trim_start_matches([' ', '\t', '\n']), column)
    }

    /// A finished block (a table, a picture) the last open item holds, each
    /// of its lines at the item's content column with its bytes kept (no
    /// line is trimmed, a blank line stays empty).
    /// 이것을 실패시키는 것: 줄 앞 공백 지우기(`list_paragraph_holds_its_one_table`).
    pub(super) fn block(&mut self, block: &str) -> String {
        self.held();
        let head = std::mem::take(&mut self.empty_head);
        self.separator.get_or_insert(head);
        let indent = " ".repeat(self.column());
        block
            .split('\n')
            .map(|line| if line.is_empty() { String::new() } else { format!("{indent}{line}") })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Selects the open item of `level` for a continuation of that level:
    /// the items deeper than it close, and it holds what is written next.
    /// With no open item of `level` (it was closed, or never written), all
    /// close and `false` says the paragraph is not a continuation: it is
    /// never joined to a shallower or the last item.
    /// 이것을 실패시키는 것: 마지막 항목 열 사용(`continuation_returns_to_its_parent`) ·
    /// 없는 수준을 마지막 항목에(`continuation_of_a_closed_level_stands_alone`) · 빈 머리
    /// 표시 남기기(`continuation_returns_to_its_parent`).
    pub(super) fn resume_continuation(&mut self, level: u8) -> bool {
        let Some(at) = self.open.iter().rposition(|open| open.level == level) else {
            self.reset();
            return false;
        };
        let resumed = at + 1 < self.open.len();
        while self.open.len() > at + 1 {
            self.pop();
        }
        if resumed {
            // The item resumed already holds the closed deeper items.
            self.empty_head = false;
        }
        true
    }

    /// Whether the block being written is joined to the one before with one
    /// line ending ([`super::guard::Block::tight`]), as its first laid-out
    /// piece decided; asked once per block.
    /// 이것을 실패시키는 것: 빈 항목 뒤에도 빈 줄(`empty_item_holds_its_first_block`) ·
    /// 형제 항목도 붙이기(`empty_item_holds_its_first_block`) · 뒤 조각이 정하기
    /// (`lossy_empty_item_holds_its_first_block`).
    pub(super) fn take_tight(&mut self) -> bool {
        self.separator.take().unwrap_or(false)
    }

    /// Closes the open items: a block was written at column 0. The block
    /// being written keeps the separator its first piece decided.
    /// 이것을 실패시키는 것: 비우기 삭제(`body_paragraph_resets_open_items`).
    pub(super) fn reset(&mut self) {
        self.open.clear();
        self.top_closed = None;
        self.empty_head = false;
    }

    /// The numberings reported for continuing past other blocks, each once,
    /// in the order met.
    /// 이것을 실패시키는 것: 보고 삭제(`numbering_continuing_past_a_block_is_reported`).
    pub(super) fn take_presentation_lost(&mut self) -> Vec<NumberingIndex> {
        std::mem::take(&mut self.presentation_lost)
    }

    /// The group of a numbered item at `level` of `numbering`, as the item
    /// closed just before it in the same parent decides: the same numbering
    /// keeps its delimiter (the item continues that Markdown list), another
    /// takes the other one. An item starting a new list while its numbering
    /// counts at its level is reported (see the type); it starts its level
    /// again for the deeper levels of its numbering.
    /// 이것을 실패시키는 것: 다른 번호에도 같은 구분자 · 같은 번호에 바꾼 구분자 · 다른 부모의
    /// 닫힌 항목과 비교(`adjacent_numbering_groups_alternate_delimiters`) · 수준 없이 번호만
    /// 기록 · 같은 목록 잇기에도 보고 · 깊은 수준 다시 세기 삭제
    /// (`numbering_continuing_past_a_block_is_reported`).
    fn group(&mut self, level: u8, numbering: NumberingIndex) -> Group {
        let before = match self.open.last() {
            Some(parent) => parent.closed,
            None => self.top_closed,
        };
        let sibling = before.filter(|closed| closed.level == level).and_then(|closed| closed.group);
        let delimiter = match sibling {
            Some(group) if group.numbering == numbering => group.delimiter,
            Some(Group { delimiter: '.', .. }) => ')',
            _ => '.',
        };
        let continues = sibling.is_some_and(|group| group.numbering == numbering);
        if !continues
            && self.counting.contains(&(numbering, level))
            && self.reported.insert(numbering)
        {
            self.presentation_lost.push(numbering);
        }
        self.counting.insert((numbering, level));
        for deeper in level + 1..=ParagraphListRef::MAX_LEVEL {
            self.counting.remove(&(numbering, deeper));
        }
        Group { numbering, delimiter }
    }

    /// The last open item holds a block now written: it ends the Markdown
    /// list of the items it held before, so its next one starts a new list.
    /// Only a block written counts: a continuation that writes nothing
    /// leaves that list going.
    /// 이것을 실패시키는 것: 이 비우기 삭제 · 쓰이지 않은 continuation 에서 비우기
    /// (`numbering_continuing_past_a_block_is_reported`).
    fn held(&mut self) {
        if let Some(item) = self.open.last_mut() {
            item.closed = None;
        }
    }

    /// Closes the last open item, recording it as the item closed last in
    /// the item that held it (or at the top).
    fn pop(&mut self) {
        if let Some(item) = self.open.pop() {
            let closed = Some(Closed { level: item.level, group: item.group });
            match self.open.last_mut() {
                Some(parent) => parent.closed = closed,
                None => self.top_closed = closed,
            }
        }
    }

    fn close_from(&mut self, level: u8) {
        while self.open.last().is_some_and(|open| open.level >= level) {
            self.pop();
        }
    }

    /// The content column of the last open item (0 for none).
    fn column(&self) -> usize {
        self.open.last().map_or(0, |open| open.column)
    }
}

/// Why a list paragraph's semantics did not reach the Markdown
/// ([`MdWarning::ListSemanticsLost`]): it is in a flow a view writes as text
/// (a table cell, a shape's text, a note, a caption, a header or footer).
pub(super) const CONTAINER_PROJECTION: &str = "container_projection";
/// … a block in the section-body list paragraph is written outside its item.
pub(super) const BODY_CARRIER_PROJECTION: &str = "body_carrier_projection";
/// … the section-body list paragraph was written as plain text.
pub(super) const MARKUP_FALLBACK: &str = "markup_fallback";

/// The list semantics a paragraph is written with as a Markdown list item:
/// [`StyleLookup::para_list`] other than an outline (a heading).
pub(super) fn item_list(
    paragraph: &Paragraph,
    styles: &dyn StyleLookup,
) -> Option<ParagraphListRef> {
    styles
        .para_list(paragraph.para_shape_id)
        .filter(|list| !matches!(list, ParagraphListRef::Outline { .. }))
}

/// The numbering of a numbered list paragraph ([`item_list`]).
pub(super) fn numbering_of(
    paragraph: &Paragraph,
    styles: &dyn StyleLookup,
) -> Option<NumberingIndex> {
    match item_list(paragraph, styles)? {
        ParagraphListRef::Number { numbering_id, .. } => Some(numbering_id),
        _ => None,
    }
}

/// The level of the list item a paragraph continues by its original
/// styles: its paragraph shape's reserved continuation style name, on a
/// shape with no list or heading semantics of its own. Both views read a
/// continuation through this one rule.
pub(super) fn continuation_level(paragraph: &Paragraph, styles: &dyn StyleLookup) -> Option<u8> {
    let id = paragraph.para_shape_id;
    if styles.para_heading_level(id).is_some() || styles.para_list(id).is_some() {
        return None;
    }
    styles.para_style_name(id).and_then(parse_list_continuation_style_name)
}

/// The level of the list item whose code block a paragraph is, by its
/// original styles: its paragraph shape's reserved code-continuation style
/// name, on a shape with no list or heading semantics of its own.
pub(super) fn code_continuation_level(
    paragraph: &Paragraph,
    styles: &dyn StyleLookup,
) -> Option<u8> {
    let id = paragraph.para_shape_id;
    if styles.para_heading_level(id).is_some() || styles.para_list(id).is_some() {
        return None;
    }
    styles.para_style_name(id).and_then(parse_code_continuation_style_name)
}

/// One [`MdWarning::ListSemanticsLost`] per list paragraph outside the
/// section body, found by its original styles: the views write those
/// flows as text, so no Markdown list holds them. Each paragraph is its
/// own warning, even when several share one paragraph shape.
/// 이것을 실패시키는 것: 본문 밖 흐름 건너뛰기 · 문단 모양별로 합치기
/// (`cell_list_paragraphs_are_reported_one_by_one`).
pub(super) fn container_list_losses(
    sections: &[Section],
    styles: &dyn StyleLookup,
    warnings: &mut Vec<MdWarning>,
) {
    FieldInventory::visit_flows(sections, |section, paragraphs, path| {
        if path == "paragraphs" {
            return;
        }
        for (index, paragraph) in paragraphs.iter().enumerate() {
            if let Some(list) = item_list(paragraph, styles) {
                warnings.push(MdWarning::ListSemanticsLost {
                    section,
                    path: format!("{path}[{index}]"),
                    list,
                    reason: CONTAINER_PROJECTION,
                });
            }
        }
    });
}

/// One [`MdWarning::ListSemanticsLost`] for the section-body list paragraph
/// at `at` (section, paragraph), unless it is not one or was reported.
pub(super) fn body_list_loss(
    warnings: &mut Vec<MdWarning>,
    sections: &[Section],
    styles: &dyn StyleLookup,
    at: (usize, usize),
    reason: &'static str,
) {
    let Some(list) = sections
        .get(at.0)
        .and_then(|section| section.paragraphs.get(at.1))
        .and_then(|paragraph| item_list(paragraph, styles))
    else {
        return;
    };
    let path = format!("paragraphs[{}]", at.1);
    let reported = warnings.iter().any(|w| {
        matches!(w, MdWarning::ListSemanticsLost { section, path: p, .. } if *section == at.0 && *p == path)
    });
    if !reported {
        warnings.push(MdWarning::ListSemanticsLost { section: at.0, path, list, reason });
    }
}

/// Whether an item's or a continuation's text, laid out in its item, would
/// make a structure its text did not mean ([`super::guard::body_structure`]):
/// pieces escaped one by one can still join into one (`1` then `. x`).
/// 이것을 실패시키는 것: 이 검사 삭제(`joined_pieces_make_no_list_in_an_item`).
pub(super) fn makes_structure(text: &str) -> bool {
    super::guard::body_structure(&lay_out("", text, 0))
}

/// `text` after `first` on its first line and at `column` on each later
/// one, every line's leading spaces and tabs dropped: a paragraph line's
/// leading whitespace is not shown, and kept after a blank line it could
/// start a code block. A blank line stays blank.
/// 이것을 실패시키는 것: 뒷줄 들여쓰기 삭제(`item_text_stays_in_its_item`) · 앞 공백
/// 남기기(`item_text_stays_in_its_item`).
fn lay_out(first: &str, text: &str, column: usize) -> String {
    let indent = " ".repeat(column);
    text.split('\n')
        .enumerate()
        .map(|(i, line)| {
            let line = line.trim_start_matches([' ', '\t']);
            match (i, line.is_empty()) {
                (0, _) => format!("{first}{line}"),
                (_, true) => String::new(),
                _ => format!("{indent}{line}"),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Each block's text in `markdown` (a paragraph's, a table cell's, a code
/// block's, its inline pieces joined, escapes resolved) with how many list
/// items hold it, as the decoder's parser reads it; a code block's text has
/// `None`.
#[cfg(test)]
pub(super) fn item_depths(markdown: &str) -> Vec<(String, Option<usize>)> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let mut depth = 0;
    let mut code = false;
    let mut text = String::new();
    let mut out = Vec::new();
    let mut flush = |text: &mut String, depth: usize, code: bool| {
        if !text.trim().is_empty() {
            out.push((text.trim_end().to_string(), (!code).then_some(depth)));
        }
        text.clear();
    };
    for event in Parser::new_ext(markdown, super::md_options()) {
        match event {
            Event::Text(t) | Event::Code(t) => text.push_str(&t),
            Event::SoftBreak | Event::HardBreak => text.push('\n'),
            Event::Start(Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. })
            | Event::End(
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link,
            )
            | Event::InlineHtml(_)
            | Event::TaskListMarker(_) => {}
            Event::Start(tag) => {
                flush(&mut text, depth, code);
                match tag {
                    Tag::Item => depth += 1,
                    Tag::CodeBlock(_) => code = true,
                    _ => {}
                }
            }
            Event::End(tag) => {
                flush(&mut text, depth, code);
                match tag {
                    TagEnd::Item => depth -= 1,
                    TagEnd::CodeBlock => code = false,
                    _ => {}
                }
            }
            _ => flush(&mut text, depth, code),
        }
    }
    flush(&mut text, depth, code);
    out
}
