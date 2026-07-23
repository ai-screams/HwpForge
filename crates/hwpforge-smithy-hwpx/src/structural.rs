//! Structural paragraph editing (E4): preserve-first byte-splice insert/delete
//! of top-level `<hp:p>` paragraphs.
//!
//! Scope (E4 = paragraphs only; table-row editing is the separate E4b epic):
//! insert-after / insert-before / delete of **top-level body-flow**
//! paragraphs. Paragraphs nested inside table cells, text boxes, footnotes,
//! or other containers are an explicit non-goal — they are addressed by
//! neither this module nor E5 `read`/`outline` (which are top-level too).
//!
//! Safety is a **closed set of fail-closed rejections** (warning-first): a
//! delete that would strand a reference, or lose a hard page/column break,
//! is refused rather than silently corrupting the document.
//!
//! This module owns the conservative reference scan; precise reference
//! resolution (which id is referenced from where, to allow deleting an
//! unreferenced bookmark) is a documented follow-up.

use hwpforge_core::paragraph::Paragraph;
use serde_json::Value;

/// Result of scanning a paragraph subtree for reference material that makes a
/// structural delete unsafe under the **conservative** rule 1 (dangling-ref).
///
/// The scan is an exhaustive `serde_json` structure walk: serde visits every
/// field of the paragraph and everything nested under it — captions, group
/// children, table cells, and container paragraphs (text box / footnote /
/// endnote / ellipse / polygon / memo bodies) — so **no container can be
/// missed**. A typed walker would risk overlooking one of the ~15 nested
/// container sites; for a safety rule where a miss means silent corruption,
/// provable completeness is worth more than type-elegance.
///
/// The conservative rule needs only *presence*; measuring which id is
/// referenced by whom (to allow deleting a bookmark that nothing points at)
/// is the precise follow-up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ReferenceScan {
    /// A `Control::Bookmark` is present somewhere in the subtree.
    pub bookmark: bool,
    /// A `Control::CrossRef` is present somewhere in the subtree.
    pub cross_ref: bool,
    /// A non-null `inst_id` is present — i.e. an identifiable object
    /// (footnote / endnote / equation / group / text-art / image / table)
    /// that a cross-reference elsewhere could point at.
    pub object_id: bool,
}

impl ReferenceScan {
    /// `true` when deleting this paragraph could strand a reference and the
    /// conservative rule must refuse.
    pub fn has_reference_material(&self) -> bool {
        self.bookmark || self.cross_ref || self.object_id
    }
}

/// Scans a paragraph subtree for conservative-rule reference material.
///
/// Fail-closed: a serialization failure (practically impossible for owned
/// `Paragraph` data) yields `object_id = true` so the caller refuses rather
/// than assuming the paragraph is safe to delete.
pub(crate) fn scan_paragraph_references(paragraph: &Paragraph) -> ReferenceScan {
    let mut scan = ReferenceScan::default();
    match serde_json::to_value(paragraph) {
        Ok(value) => walk_value(&value, &mut scan),
        Err(_) => scan.object_id = true, // unknown ⇒ reject (fail-closed)
    }
    scan
}

fn walk_value(value: &Value, scan: &mut ReferenceScan) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                // Control is externally tagged: a bookmark serializes as
                // `{"Bookmark": {..}}`, a cross-ref as `{"CrossRef": {..}}`.
                // These PascalCase keys are enum discriminants and never
                // collide with the snake_case struct field names.
                match key.as_str() {
                    "Bookmark" => scan.bookmark = true,
                    "CrossRef" => scan.cross_ref = true,
                    "inst_id" if !child.is_null() => scan.object_id = true,
                    _ => {}
                }
                walk_value(child, scan);
                if scan.bookmark && scan.cross_ref && scan.object_id {
                    return; // saturated — nothing more to learn
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_value(item, scan);
                if scan.bookmark && scan.cross_ref && scan.object_id {
                    return;
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::control::{Control, RefTarget};
    use hwpforge_core::{ObjectId, Paragraph, Run, Table, TableCell, TableRow};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, RefContentType, RefType};

    fn para() -> Paragraph {
        Paragraph::new(ParaShapeIndex::new(0))
    }

    fn with_control(control: Control) -> Paragraph {
        let mut p = para();
        p.runs.push(Run::control(control, CharShapeIndex::new(0)));
        p
    }

    #[test]
    fn plain_text_paragraph_has_no_reference_material() {
        let mut p = para();
        p.runs.push(Run::text("본문", CharShapeIndex::new(0)));
        let scan = scan_paragraph_references(&p);
        assert!(!scan.has_reference_material());
        assert_eq!(scan, ReferenceScan::default());
    }

    #[test]
    fn bookmark_is_detected() {
        let scan = scan_paragraph_references(&with_control(Control::bookmark("장시작")));
        assert!(scan.bookmark);
        assert!(scan.has_reference_material());
    }

    #[test]
    fn footnote_inst_id_is_detected() {
        let ctrl = Control::footnote_with_id(4242, vec![para()]);
        let scan = scan_paragraph_references(&with_control(ctrl));
        assert!(scan.object_id);
        assert!(scan.has_reference_material());
    }

    #[test]
    fn footnote_without_inst_id_is_not_object_id() {
        // A footnote with inst_id = None is self-contained; nothing can
        // reference it, so it is not blocking on the object-id axis.
        let ctrl = Control::footnote(vec![para()]);
        let scan = scan_paragraph_references(&with_control(ctrl));
        assert!(!scan.object_id);
    }

    #[test]
    fn reference_nested_in_group_children_is_detected() {
        // Serde recurses into Group children — the fill.rs walker's Group gap
        // cannot cause a miss here.
        let inner = Control::footnote_with_id(7, vec![para()]);
        let group = Control::Group {
            children: vec![inner],
            inst_id: None,
            width: HwpUnit::ZERO,
            height: HwpUnit::ZERO,
            horz_offset: 0,
            vert_offset: 0,
        };
        let scan = scan_paragraph_references(&with_control(group));
        assert!(scan.object_id, "footnote hidden inside a group must be seen");
    }

    #[test]
    fn reference_nested_in_table_cell_is_detected() {
        let mut cell_para = para();
        cell_para.runs.push(Run::control(Control::bookmark("셀책갈피"), CharShapeIndex::new(0)));
        let cell = TableCell::new(vec![cell_para], HwpUnit::new(1000).unwrap());
        let table = Table::new(vec![TableRow::new(vec![cell])]);
        let mut p = para();
        p.runs.push(Run::table(table, CharShapeIndex::new(0)));
        let scan = scan_paragraph_references(&p);
        assert!(scan.bookmark, "bookmark inside a table cell must be seen");
    }

    #[test]
    fn cross_ref_is_detected() {
        let ctrl = Control::CrossRef {
            target: RefTarget::Object(ObjectId::new(9)),
            ref_type: RefType::Table,
            content_type: RefContentType::Number,
            as_hyperlink: false,
            display_text: String::new(),
        };
        let scan = scan_paragraph_references(&with_control(ctrl));
        assert!(scan.cross_ref);
        assert!(scan.has_reference_material());
    }
}
