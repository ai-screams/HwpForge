//! Plan phase (E6 Wave 1A): enumerate class-A placeholder candidates over a
//! Core document.
//!
//! Coverage mirrors `patch.rs::collect_semantic_text_slots` exactly — the
//! same walker is reused, so any text a patch can address, a stamp plan can
//! see (and nothing else). Existing ClickHere field bodies (`…control.field`
//! slots) are excluded: stamping inside `fieldBegin`~`fieldEnd` would split
//! the field.
//!
//! Instruction-context guards are scoped to the surrounding **paragraph**
//! (all runs concatenated) and, inside tables, the surrounding **cell** —
//! never the whole body (a `(예시)` in one cell must not guard the document).

use std::collections::HashMap;
use std::ops::Range;

use hwpforge_core::Document;

use super::detect::{detect_markers, paragraph_guard, BuiltinPattern, GuardReason};
use crate::patch::collect_semantic_text_slots;

/// One placeholder candidate produced by the plan phase.
///
/// Candidates are proposals only — nothing is applied until the whole plan
/// preflights and every candidate is approved (named or ignored) by the
/// caller's map (design decision §3-2/§3-3).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StampCandidate {
    /// Zero-based section index.
    pub section: usize,
    /// Semantic slot path of the text payload (same address space as the
    /// patch slot model, e.g. `paragraphs[0].runs[1].text`).
    pub path: String,
    /// UTF-8 byte span of the marker within that slot's text.
    pub span: Range<usize>,
    /// The marker text, verbatim.
    pub marker: String,
    /// Which built-in pattern matched.
    pub pattern: BuiltinPattern,
    /// Instruction-context downgrade, when the surrounding paragraph or
    /// table cell reads as instructions/example prose. Guarded candidates
    /// are never auto-applied.
    pub guard: Option<GuardReason>,
}

/// Enumerates class-A placeholder candidates across all sections.
///
/// Read-only: the document is not modified. Candidates are ordered by
/// section, then slot order (document order), then span start.
pub fn plan<S>(document: &Document<S>) -> Vec<StampCandidate> {
    let mut out = Vec::new();
    for (section_idx, section) in document.sections().iter().enumerate() {
        let slots = collect_semantic_text_slots(section);

        // Guard scopes: paragraph text (runs concatenated, slot order) and
        // table-cell text. Keys are path prefixes of the slot paths.
        let mut para_text: HashMap<String, String> = HashMap::new();
        let mut cell_text: HashMap<String, String> = HashMap::new();
        for slot in &slots {
            if is_field_body_slot(&slot.path) {
                continue;
            }
            if let Some(pk) = paragraph_key(&slot.path) {
                para_text.entry(pk.to_string()).or_default().push_str(&slot.text);
            }
            if let Some(ck) = cell_key(&slot.path) {
                cell_text.entry(ck.to_string()).or_default().push_str(&slot.text);
            }
        }

        for slot in &slots {
            // Field bodies are never stamp targets; tab-bearing InlineText
            // runs still contribute guard context above, but splitting them
            // is lossy so they produce no candidates (Wave 1A).
            if is_field_body_slot(&slot.path) || slot.inline {
                continue;
            }
            let hits = detect_markers(&slot.text);
            if hits.is_empty() {
                continue;
            }
            let guard = paragraph_key(&slot.path)
                .and_then(|pk| para_text.get(pk))
                .and_then(|text| paragraph_guard(text))
                .or_else(|| {
                    cell_key(&slot.path)
                        .and_then(|ck| cell_text.get(ck))
                        .and_then(|text| paragraph_guard(text))
                });
            for hit in hits {
                out.push(StampCandidate {
                    section: section_idx,
                    path: slot.path.clone(),
                    span: hit.span,
                    marker: hit.marker,
                    pattern: hit.pattern,
                    guard,
                });
            }
        }
    }
    out
}

/// Existing ClickHere bodies are patch slots but never stamp targets.
fn is_field_body_slot(path: &str) -> bool {
    path.ends_with(".field")
}

/// Slot path minus its `.runs[j]…` tail — identifies the paragraph.
fn paragraph_key(path: &str) -> Option<&str> {
    path.rfind(".runs[").map(|i| &path[..i])
}

/// The `…cells[k].paragraphs` container prefix, for slots inside a table
/// cell; `None` outside tables (body-level guard scoping is intentionally
/// NOT provided).
fn cell_key(path: &str) -> Option<&str> {
    let pk = paragraph_key(path)?;
    if !pk.contains(".cells[") {
        return None;
    }
    pk.rfind('[').map(|i| &pk[..i])
}

#[cfg(test)]
mod tests {
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::run::Run;
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_core::{Control, Document, Paragraph, Section};
    use hwpforge_foundation::{CharShapeIndex, FieldType, HwpUnit, ParaShapeIndex};

    use super::*;

    fn text_para(text: &str) -> Paragraph {
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
    }

    fn doc_with_paras(paras: Vec<Paragraph>) -> Document {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(paras, PageSettings::default()));
        doc
    }

    // ── edge cases first ────────────────────────────────────────────

    #[test]
    fn empty_document_has_no_candidates() {
        assert!(plan(&Document::new()).is_empty());
    }

    #[test]
    fn prose_only_document_has_no_candidates() {
        let doc = doc_with_paras(vec![text_para("신청 기관의 명칭을 기재하시오.")]);
        assert!(plan(&doc).is_empty());
    }

    #[test]
    fn existing_clickhere_body_is_not_a_candidate() {
        // A ClickHere whose body happens to look like a paren blank must be
        // excluded — stamping inside fieldBegin~fieldEnd splits the field.
        let mut para = Paragraph::new(ParaShapeIndex::new(0));
        para.add_run(Run::control(
            Control::Field {
                field_type: FieldType::ClickHere,
                hint_text: Some("힌트".to_string()),
                help_text: None,
                name: Some("slot".to_string()),
                display_text: "(   )".to_string(),
            },
            CharShapeIndex::new(0),
        ));
        let doc = doc_with_paras(vec![para]);
        assert!(plan(&doc).is_empty());
    }

    // ── body paragraphs ─────────────────────────────────────────────

    #[test]
    fn paren_blank_in_body_paragraph() {
        let doc = doc_with_paras(vec![text_para("성명: (   )")]);
        let got = plan(&doc);
        assert_eq!(got.len(), 1);
        let c = &got[0];
        assert_eq!(c.section, 0);
        assert_eq!(c.path, "paragraphs[0].runs[0].text");
        assert_eq!(c.marker, "(   )");
        assert_eq!(c.pattern, BuiltinPattern::ParenBlank);
        assert_eq!(c.guard, None);
    }

    #[test]
    fn instruction_paragraph_candidates_are_guarded() {
        let doc = doc_with_paras(vec![text_para("※ 해당하는 항목의 □에 표시")]);
        let got = plan(&doc);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].pattern, BuiltinPattern::Checkbox);
        assert_eq!(got[0].guard, Some(GuardReason::InstructionContext));
    }

    #[test]
    fn guard_spans_all_runs_of_the_paragraph() {
        // run 0 marks the paragraph as example prose; run 1's marker must
        // inherit the paragraph-level guard.
        let para = Paragraph::with_runs(
            vec![
                Run::text("(예시) 안내 ", CharShapeIndex::new(0)),
                Run::text("(   )", CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        );
        let doc = doc_with_paras(vec![para]);
        let got = plan(&doc);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].path, "paragraphs[0].runs[1].text");
        assert_eq!(got[0].guard, Some(GuardReason::InstructionContext));
    }

    // ── table cells ─────────────────────────────────────────────────

    fn table_doc(cells: Vec<Vec<Paragraph>>) -> Document {
        let width = HwpUnit::new(1000).unwrap();
        let row =
            TableRow::new(cells.into_iter().map(|paras| TableCell::new(paras, width)).collect());
        let mut para = Paragraph::new(ParaShapeIndex::new(0));
        para.add_run(Run::table(Table::new(vec![row]), CharShapeIndex::new(0)));
        doc_with_paras(vec![para])
    }

    #[test]
    fn cell_candidate_has_cell_path() {
        let doc = table_doc(vec![vec![text_para("□ 동의")]]);
        let got = plan(&doc);
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].path,
            "paragraphs[0].runs[0].table.rows[0].cells[0].paragraphs[0].runs[0].text"
        );
        assert_eq!(got[0].pattern, BuiltinPattern::Checkbox);
        assert_eq!(got[0].guard, None);
    }

    #[test]
    fn cell_guard_stays_inside_its_cell() {
        // cell 0 is example prose (guard applies to its own candidates,
        // even in a different paragraph of the same cell); cell 1 is a
        // clean slot and must NOT inherit cell 0's guard.
        let doc = table_doc(vec![
            vec![text_para("(예시) 보기 내용"), text_para("□ 선택")],
            vec![text_para("(   )")],
        ]);
        let got = plan(&doc);
        assert_eq!(got.len(), 2);
        let checkbox = got.iter().find(|c| c.pattern == BuiltinPattern::Checkbox).unwrap();
        assert_eq!(
            checkbox.guard,
            Some(GuardReason::InstructionContext),
            "cell-level guard must reach sibling paragraphs in the same cell"
        );
        let blank = got.iter().find(|c| c.pattern == BuiltinPattern::ParenBlank).unwrap();
        assert_eq!(blank.guard, None, "guard must not leak into a different cell");
    }

    #[test]
    fn body_guard_does_not_leak_across_paragraphs() {
        // `(예시)` in paragraph 0 must not guard paragraph 1 (body has no
        // container-level guard scope by design).
        let doc =
            doc_with_paras(vec![text_para("(예시) 교육자료 개발비"), text_para("금액: (   )")]);
        let got = plan(&doc);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].guard, None);
    }
}
