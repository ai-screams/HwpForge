//! Paragraph measures the readable views cannot hold.

use std::collections::{BTreeMap, HashSet};

use hwpforge_blueprint::builtins::builtin_default;
use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_core::field_inventory::FieldInventory;
use hwpforge_core::{Document, ListItemKind, Paragraph, StyleLookup, Validated};
use hwpforge_foundation::{HwpUnit, ParaShapeIndex};

use super::{list_format, MdWarning};
use crate::mapper::{resolve_mapping, MdMapping};

/// What a section-body paragraph was written as, at its depth in the
/// Markdown ([`super::list_format::ListIndent::depth`]).
#[derive(Debug, Clone, Copy)]
pub(super) enum Held {
    /// A list item whose marker holds the whole paragraph in one Markdown
    /// paragraph, so it reads back as the item's first paragraph.
    Item(u8),
    /// A later text or code block of the open item at that depth, so it
    /// reads back as one.
    Continuation(u8),
}

/// The paragraphs Markdown's list structure holds, by what the views wrote,
/// so the measures of everything else are reported. What the structure
/// holds under a different role is everything else: a code-font item is an
/// empty item holding a fence, text with a blank line is two paragraphs,
/// and the first block of an empty item is that item's first paragraph.
///
/// `to-md` takes no template, so what that structure gives back is the
/// default template's: a document made with another one, or a list with
/// margins of its own, is reported, because reading its Markdown back gives
/// other values. A block quote is too: the views write no quote marker.
#[derive(Debug, Default)]
pub(super) struct ListMeasures {
    written: BTreeMap<(usize, usize), (ParaShapeIndex, Held)>,
}

impl ListMeasures {
    /// The paragraph at `at` (section, paragraph) was written as `held`.
    pub(super) fn written(&mut self, at: (usize, usize), id: ParaShapeIndex, held: Held) {
        self.written.insert(at, (id, held));
    }

    /// Ends a view: a list paragraph the guard wrote as plain text after all
    /// (`fell_back`) lost its list, and each paragraph definition's
    /// first-line indent that is not 0 and left margin that is not 0 is
    /// reported, unless every paragraph using the definition was written
    /// where Markdown gives that margin back.
    ///
    /// 이것을 실패시키는 것: 기대값 비교 없이 쓴 문단 면제
    /// (`changed_role_margins_are_reported`) · 사용처 하나만 보기
    /// (`a_definition_used_outside_the_list_is_reported`) · 다른 순회로 메모 앵커 놓치기
    /// (`measures_of_a_memo_anchor_are_reported`) · 평문이 된 문단을 기록에 남기기
    /// (`a_paragraph_that_fell_back_is_reported`).
    pub(super) fn report(
        mut self,
        fell_back: Vec<(usize, usize)>,
        document: &Document<Validated>,
        styles: &dyn StyleLookup,
        warnings: &mut Vec<MdWarning>,
    ) {
        for at in fell_back {
            self.written.remove(&at);
            list_format::body_list_loss(
                warnings,
                document.sections(),
                styles,
                at,
                list_format::MARKUP_FALLBACK,
            );
        }
        let template = builtin_default().expect("built-in template");
        let (mapping, registry) = resolve_mapping(&template).expect("built-in template");
        let mut given_back = BTreeMap::<ParaShapeIndex, usize>::new();
        for (id, held) in self.written.values() {
            let margin = styles.para_indent_left(*id).and_then(|m| m.as_hwp_unit());
            if margin.is_some() && margin == given_margin(*held, &mapping, &registry) {
                *given_back.entry(*id).or_default() += 1;
            }
        }
        // Neither walker of Core sees every paragraph: one skips a picture's
        // caption, the other a memo's anchor. A paragraph behind both, in a
        // caption's memo's anchor, goes uncounted.
        let mut uses = BTreeMap::<ParaShapeIndex, usize>::new();
        let mut seen = HashSet::new();
        let mut count = |paragraph: &Paragraph| {
            if seen.insert(std::ptr::from_ref(paragraph)) {
                *uses.entry(paragraph.para_shape_id).or_default() += 1;
            }
        };
        for section in document.sections() {
            section.for_each_paragraph(&mut count);
        }
        FieldInventory::visit_flows(document.sections(), |_, paragraphs, _| {
            paragraphs.iter().for_each(&mut count);
        });
        for (id, count) in uses {
            let left = styles.para_indent_left(id).filter(|_| given_back.get(&id) != Some(&count));
            for (field, measure) in
                [("margin_left", left), ("indent", styles.para_indent_first_line(id))]
            {
                if let Some(measure) = measure.filter(|m| m.as_hwp_unit() != Some(HwpUnit::ZERO)) {
                    warnings.push(MdWarning::ParagraphMeasureLost {
                        definition: id.get(),
                        field: field.into(),
                        measure,
                    });
                }
            }
        }
    }
}

/// The left margin `convert` gives what was written as `held`. Every list
/// kind's margin at a level is the same, and so is a code block's in an item.
fn given_margin(held: Held, mapping: &MdMapping, registry: &StyleRegistry) -> Option<HwpUnit> {
    let style = match held {
        Held::Item(depth) => mapping.list_item_style(ListItemKind::Bullet, None, depth)?,
        Held::Continuation(depth) => mapping.list_continuation(depth),
    };
    Some(registry.para_shape(style.para_shape_id)?.indent_left)
}
