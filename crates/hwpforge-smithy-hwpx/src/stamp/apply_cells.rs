//! Class-B apply — Wave 2B: preflight + all-or-nothing cell promotion.
//!
//! The engine is split into [`preflight_cells`] and [`commit_cells`] so a
//! combined text+cell stamp can preflight EVERYTHING against the pristine
//! document before any mutation (a second preflight failing after a first
//! mutation would leave a half-stamped document). The commit phase is
//! mechanical: every coordinate was resolved and every invariant checked
//! against the same document snapshot, and class-A text mutation cannot
//! invalidate a cell target (markers require non-whitespace text, stampable
//! targets are whitespace-only, and run splits never move tables).
//!
//! Contract (design §7.3, Codex-settled):
//! - spec `at` must be the canonical anchor — covered coordinates are
//!   rejected, never silently resolved (unlike set-cell's read surface).
//! - a spec with a label claim approves a DETECTED candidate: the claim is
//!   re-verified against the live plan (target still stampable, label cell
//!   still normalize-matches). A spec without a claim is an EXPLICIT orphan
//!   authoring — new capability, still fully preflighted.
//! - every unguarded candidate must be covered by a spec (field or ignore);
//!   guarded candidates without a spec are skipped and reported.
//! - promoted display = hint (caller-required); char shape comes from the
//!   target cell's own first run (authored value preserved since the
//!   empty-run charPrIDRef decoder fix), para shape untouched.
//! - only the FIRST paragraph's run is replaced; trailing empty padding
//!   paragraphs are preserved so table geometry cannot move. The replaced
//!   run's text is recorded verbatim for the reverse-delta gate.

use std::collections::{HashMap, HashSet};

use hwpforge_core::document::{Document, Draft};
use hwpforge_core::run::Run;
use hwpforge_core::table::grid::{GridCoord, TableGrid};
use hwpforge_core::Control;
use hwpforge_foundation::FieldType;

use super::cells::{is_stampable_empty, plan_cells, CellStampCandidate};
use super::request::{CellLabelClaim, CellStampAction, CellStampSpec};
use crate::cell_edit::normalize_label;
use crate::fill::visit_section_fields;
use crate::table_inventory::{for_each_table_mut, tables_in_document};

/// One field created by [`commit_cells`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CellStampedField {
    /// The field name (unique in the output document).
    pub name: String,
    /// Table ordinal (shared inventory DFS pre-order).
    pub table: usize,
    /// Anchor coordinate of the stamped cell.
    pub at: GridCoord,
    /// The label claim the spec carried (`None` = explicit orphan).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<CellLabelClaim>,
    /// The unfilled body/hint the field was created with.
    pub hint: String,
    /// Original first-paragraph run text (whitespace padding), recorded
    /// verbatim for the reverse-delta gate.
    pub original_text: String,
}

/// Result of a successful cell apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellStampOutcome {
    /// Fields created, in spec order.
    pub stamped: Vec<CellStampedField>,
    /// Number of explicitly ignored candidates.
    pub ignored: usize,
    /// Guarded candidates left untouched because no spec approved them.
    pub skipped_guarded: Vec<CellStampCandidate>,
}

/// Cell-apply preflight failure — nothing was modified.
#[derive(Debug)]
#[non_exhaustive]
pub enum CellStampError {
    /// Spec table ordinal does not exist.
    TableNotFound {
        /// The requested ordinal.
        table: usize,
    },
    /// The table's logical grid cannot be built.
    TableGridInvalid {
        /// Table ordinal.
        table: usize,
        /// First grid violation (human-readable).
        detail: String,
    },
    /// Spec `at` is not a canonical anchor (out of bounds or covered).
    NotAnAnchor {
        /// Table ordinal.
        table: usize,
        /// The requested coordinate.
        requested: GridCoord,
        /// The anchor covering it, when the coordinate is merely covered.
        anchor: Option<GridCoord>,
    },
    /// The target cell is not stampable empty (authored content).
    TargetNotStampable {
        /// Table ordinal.
        table: usize,
        /// Target anchor.
        at: GridCoord,
    },
    /// The label claim no longer matches the live document/plan.
    LabelDrift {
        /// Table ordinal.
        table: usize,
        /// Target anchor.
        at: GridCoord,
        /// Label text the spec claimed.
        claimed: String,
        /// Normalized label actually adjacent at the claimed coordinate.
        found: Option<String>,
    },
    /// An `ignore` spec names a coordinate with no live candidate.
    UnknownCandidate {
        /// Table ordinal.
        table: usize,
        /// Target anchor.
        at: GridCoord,
    },
    /// Two specs target the same cell.
    DuplicateTarget {
        /// Table ordinal.
        table: usize,
        /// Duplicated anchor.
        at: GridCoord,
    },
    /// A field spec has an empty name.
    EmptyName,
    /// A field spec has a blank hint.
    BlankHint {
        /// The offending field name.
        name: String,
    },
    /// Two specs (cell or text) claim the same field name.
    DuplicateName {
        /// The duplicated name.
        name: String,
    },
    /// A spec's name collides with an existing field in the document.
    NameCollision {
        /// The colliding name.
        name: String,
    },
    /// An unguarded candidate has no spec — every candidate must be named
    /// or explicitly ignored.
    UncoveredCandidate {
        /// Table ordinal.
        table: usize,
        /// Candidate anchor.
        at: GridCoord,
    },
}

impl std::fmt::Display for CellStampError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TableNotFound { table } => write!(f, "table ordinal {table} does not exist"),
            Self::TableGridInvalid { table, detail } => {
                write!(f, "table {table}: grid invalid: {detail}")
            }
            Self::NotAnAnchor { table, requested, anchor: Some(anchor) } => write!(
                f,
                "table {table}: ({},{}) is covered by the merge anchored at ({},{}) — specs must \
                 target the anchor",
                requested.row, requested.col, anchor.row, anchor.col
            ),
            Self::NotAnAnchor { table, requested, anchor: None } => write!(
                f,
                "table {table}: ({},{}) is outside the logical grid",
                requested.row, requested.col
            ),
            Self::TargetNotStampable { table, at } => write!(
                f,
                "table {table}: cell ({},{}) is not a stampable empty cell",
                at.row, at.col
            ),
            Self::LabelDrift { table, at, claimed, found } => write!(
                f,
                "table {table}: label claim {claimed:?} for ({},{}) no longer matches (found \
                 {found:?})",
                at.row, at.col
            ),
            Self::UnknownCandidate { table, at } => {
                write!(f, "table {table}: ({},{}) is not a live candidate", at.row, at.col)
            }
            Self::DuplicateTarget { table, at } => {
                write!(f, "table {table}: cell ({},{}) targeted twice", at.row, at.col)
            }
            Self::EmptyName => write!(f, "field name must not be empty"),
            Self::BlankHint { name } => write!(f, "cell spec {name:?}: hint must not be blank"),
            Self::DuplicateName { name } => write!(f, "duplicate field name {name:?}"),
            Self::NameCollision { name } => {
                write!(f, "field name {name:?} already exists in the document")
            }
            Self::UncoveredCandidate { table, at } => write!(
                f,
                "unguarded candidate at table {table} ({},{}) has no spec — name it or ignore it",
                at.row, at.col
            ),
        }
    }
}

impl std::error::Error for CellStampError {}

/// One fully-resolved pending edit (internal to preflight→commit).
#[derive(Debug)]
struct PlannedCellEdit {
    table: usize,
    row_idx: usize,
    cell_idx: usize,
    at: GridCoord,
    name: String,
    hint: String,
    label: Option<CellLabelClaim>,
}

/// Everything [`commit_cells`] needs, produced against a pristine document.
#[derive(Debug)]
pub(crate) struct CellApplyPlan {
    edits: Vec<PlannedCellEdit>,
    ignored: usize,
    skipped_guarded: Vec<CellStampCandidate>,
}

/// Preflights every cell spec against the CURRENT document state.
///
/// `reserved_names` carries field names claimed by class-A text specs in
/// the same combined request so cross-class collisions fail here, before
/// either class mutates.
pub(crate) fn preflight_cells(
    document: &mut Document<Draft>,
    specs: &[CellStampSpec],
    reserved_names: &HashSet<String>,
) -> Result<CellApplyPlan, CellStampError> {
    let plan = plan_cells(document);
    let mut candidates: HashMap<(usize, GridCoord), &CellStampCandidate> = HashMap::new();
    for candidate in &plan.candidates {
        candidates.insert((candidate.table, candidate.at), candidate);
    }
    let skipped_grid: HashMap<usize, &str> =
        plan.skipped_tables.iter().map(|s| (s.table, s.error.as_str())).collect();

    // Existing field names (fill's visitor — the single definition of
    // field-visit coverage).
    let mut existing: HashSet<String> = HashSet::new();
    for (idx, section) in document.sections_mut().iter_mut().enumerate() {
        visit_section_fields(section, idx, &mut |slot| {
            if let Control::Field { name: Some(name), .. } = &*slot.control {
                existing.insert(name.clone());
            }
        });
    }

    let entries = tables_in_document(document);
    let mut grids: HashMap<usize, TableGrid> = HashMap::new();

    let mut edits = Vec::new();
    let mut ignored = 0usize;
    let mut covered: HashSet<(usize, GridCoord)> = HashSet::new();
    let mut names: HashSet<String> = HashSet::new();

    for spec in specs {
        let Some(entry) = entries.get(spec.table) else {
            return Err(CellStampError::TableNotFound { table: spec.table });
        };
        if let Some(detail) = skipped_grid.get(&spec.table) {
            return Err(CellStampError::TableGridInvalid {
                table: spec.table,
                detail: (*detail).to_string(),
            });
        }
        if let std::collections::hash_map::Entry::Vacant(slot) = grids.entry(spec.table) {
            let grid = TableGrid::from_table(entry.table).map_err(|e| {
                CellStampError::TableGridInvalid { table: spec.table, detail: e.to_string() }
            })?;
            slot.insert(grid);
        }
        let grid = &grids[&spec.table];

        let Some(anchor) = grid.resolve(spec.at) else {
            return Err(CellStampError::NotAnAnchor {
                table: spec.table,
                requested: spec.at,
                anchor: None,
            });
        };
        if anchor.anchor != spec.at {
            return Err(CellStampError::NotAnAnchor {
                table: spec.table,
                requested: spec.at,
                anchor: Some(anchor.anchor),
            });
        }
        if !covered.insert((spec.table, spec.at)) {
            return Err(CellStampError::DuplicateTarget { table: spec.table, at: spec.at });
        }

        let candidate = candidates.get(&(spec.table, spec.at));

        // Label claim: must match a live candidate's label reference.
        if let Some(claim) = &spec.label {
            let claimed_norm = normalize_label(&claim.text);
            let matched = candidate.is_some_and(|c| {
                c.labels.iter().any(|l| l.at == claim.at && l.normalized == claimed_norm)
            });
            if !matched {
                let found = candidate.map(|c| {
                    c.labels.iter().map(|l| l.normalized.clone()).collect::<Vec<_>>().join(" | ")
                });
                return Err(CellStampError::LabelDrift {
                    table: spec.table,
                    at: spec.at,
                    claimed: claim.text.clone(),
                    found,
                });
            }
        }

        match &spec.action {
            CellStampAction::Ignore => {
                if candidate.is_none() {
                    return Err(CellStampError::UnknownCandidate {
                        table: spec.table,
                        at: spec.at,
                    });
                }
                ignored += 1;
            }
            CellStampAction::Field { name, hint } => {
                // Explicit orphan targets are allowed without a candidate,
                // but the cell itself must still be stampable.
                let cell = &entry.table.rows[anchor.row_idx].cells[anchor.cell_idx];
                if !is_stampable_empty(cell) {
                    return Err(CellStampError::TargetNotStampable {
                        table: spec.table,
                        at: spec.at,
                    });
                }
                if name.trim().is_empty() {
                    return Err(CellStampError::EmptyName);
                }
                if hint.trim().is_empty() {
                    return Err(CellStampError::BlankHint { name: name.clone() });
                }
                if !names.insert(name.clone()) || reserved_names.contains(name) {
                    return Err(CellStampError::DuplicateName { name: name.clone() });
                }
                if existing.contains(name) {
                    return Err(CellStampError::NameCollision { name: name.clone() });
                }
                edits.push(PlannedCellEdit {
                    table: spec.table,
                    row_idx: anchor.row_idx,
                    cell_idx: anchor.cell_idx,
                    at: spec.at,
                    name: name.clone(),
                    hint: hint.clone(),
                    label: spec.label.clone(),
                });
            }
        }
    }

    // Every unguarded candidate must be covered; uncovered guarded ones are
    // skipped and reported.
    let mut skipped_guarded = Vec::new();
    for candidate in &plan.candidates {
        if covered.contains(&(candidate.table, candidate.at)) {
            continue;
        }
        if candidate.guarded {
            skipped_guarded.push(candidate.clone());
        } else {
            return Err(CellStampError::UncoveredCandidate {
                table: candidate.table,
                at: candidate.at,
            });
        }
    }

    Ok(CellApplyPlan { edits, ignored, skipped_guarded })
}

/// Applies a preflighted [`CellApplyPlan`]. Mechanical: every invariant was
/// checked by [`preflight_cells`] against the same document snapshot.
pub(crate) fn commit_cells(
    document: &mut Document<Draft>,
    plan: CellApplyPlan,
) -> CellStampOutcome {
    let mut by_table: HashMap<usize, Vec<PlannedCellEdit>> = HashMap::new();
    for edit in plan.edits {
        by_table.entry(edit.table).or_default().push(edit);
    }

    let mut stamped = Vec::new();
    for_each_table_mut(document, &mut |ordinal, table| {
        let Some(edits) = by_table.remove(&ordinal) else {
            return;
        };
        for edit in edits {
            let paragraph = &mut table.rows[edit.row_idx].cells[edit.cell_idx].paragraphs[0];
            let run = &mut paragraph.runs[0];
            let original_text = run.content.as_text().unwrap_or_default().to_string();
            let char_shape_id = run.char_shape_id;
            *run = Run::control(
                Control::Field {
                    field_type: FieldType::ClickHere,
                    hint_text: Some(edit.hint.clone()),
                    help_text: None,
                    name: Some(edit.name.clone()),
                    display_text: edit.hint.clone(),
                },
                char_shape_id,
            );
            stamped.push(CellStampedField {
                name: edit.name,
                table: edit.table,
                at: edit.at,
                label: edit.label,
                hint: edit.hint,
                original_text,
            });
        }
    });
    // Spec order, not walk order: mirror class-A outcome semantics.
    stamped.sort_by_key(|s| (s.table, s.at.row, s.at.col));

    CellStampOutcome { stamped, ignored: plan.ignored, skipped_guarded: plan.skipped_guarded }
}

/// Standalone cell apply: preflight + commit, all-or-nothing.
///
/// # Errors
///
/// Any [`CellStampError`] leaves the document untouched.
pub fn apply_cells(
    document: &mut Document<Draft>,
    specs: &[CellStampSpec],
) -> Result<CellStampOutcome, CellStampError> {
    let plan = preflight_cells(document, specs, &HashSet::new())?;
    Ok(commit_cells(document, plan))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::RunContent;
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_core::Section;
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    fn text_cell_shaped(text: &str, shape: usize) -> TableCell {
        TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text(text, CharShapeIndex::new(shape))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::ZERO,
        )
    }

    fn text_cell(text: &str) -> TableCell {
        text_cell_shaped(text, 0)
    }

    fn table(rows: Vec<Vec<TableCell>>) -> Table {
        Table::new(rows.into_iter().map(TableRow::new).collect())
    }

    fn doc_with_tables(tables: Vec<Table>) -> Document {
        let paras = tables
            .into_iter()
            .map(|t| {
                Paragraph::with_runs(
                    vec![Run {
                        content: RunContent::Table(Box::new(t)),
                        char_shape_id: CharShapeIndex::new(0),
                    }],
                    ParaShapeIndex::new(0),
                )
            })
            .collect();
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(paras, PageSettings::default()));
        doc
    }

    fn field_spec(table: usize, row: u32, col: u32, name: &str, hint: &str) -> CellStampSpec {
        CellStampSpec {
            table,
            at: GridCoord::new(row, col),
            label: None,
            action: CellStampAction::Field { name: name.into(), hint: hint.into() },
        }
    }

    fn labeled_spec(
        table: usize,
        row: u32,
        col: u32,
        label_at: (u32, u32),
        label: &str,
        name: &str,
    ) -> CellStampSpec {
        CellStampSpec {
            table,
            at: GridCoord::new(row, col),
            label: Some(CellLabelClaim {
                at: GridCoord::new(label_at.0, label_at.1),
                text: label.into(),
            }),
            action: CellStampAction::Field { name: name.into(), hint: format!("{label} 입력") },
        }
    }

    fn cell_text_at(doc: &Document<Draft>, row: usize, col: usize) -> String {
        let section = &doc.sections()[0];
        let RunContent::Table(t) = &section.paragraphs[0].runs[0].content else {
            panic!("expected table");
        };
        t.rows[row].cells[col].paragraphs[0].text_content()
    }

    // ── happy paths ─────────────────────────────────────────────

    #[test]
    fn stamps_detected_candidate_and_preserves_char_shape() {
        let t = table(vec![vec![text_cell("성명"), text_cell_shaped("", 7)]]);
        let mut doc = doc_with_tables(vec![t]);
        let outcome =
            apply_cells(&mut doc, &[labeled_spec(0, 0, 1, (0, 0), "성명", "이름")]).unwrap();
        assert_eq!(outcome.stamped.len(), 1);
        let stamped = &outcome.stamped[0];
        assert_eq!(stamped.name, "이름");
        assert_eq!(stamped.original_text, "");

        let section = &doc.sections()[0];
        let RunContent::Table(t) = &section.paragraphs[0].runs[0].content else {
            panic!("expected table");
        };
        let run = &t.rows[0].cells[1].paragraphs[0].runs[0];
        assert_eq!(run.char_shape_id.get(), 7, "char shape must come from the target run");
        let RunContent::Control(control) = &run.content else { panic!("expected field run") };
        let Control::Field { name, display_text, hint_text, .. } = control.as_ref() else {
            panic!("expected Field");
        };
        assert_eq!(name.as_deref(), Some("이름"));
        assert_eq!(display_text, "성명 입력");
        assert_eq!(hint_text.as_deref(), Some("성명 입력"));
    }

    #[test]
    fn multi_paragraph_padding_keeps_paragraph_count_and_records_original() {
        let padded = TableCell::new(
            vec![
                Paragraph::with_runs(
                    vec![Run::text("\u{3000}", CharShapeIndex::new(3))],
                    ParaShapeIndex::new(0),
                ),
                Paragraph::with_runs(
                    vec![Run::text("", CharShapeIndex::new(3))],
                    ParaShapeIndex::new(0),
                ),
            ],
            HwpUnit::ZERO,
        );
        let t = table(vec![vec![text_cell("주소"), padded]]);
        let mut doc = doc_with_tables(vec![t]);
        let outcome = apply_cells(&mut doc, &[field_spec(0, 0, 1, "주소필드", "주소")]).unwrap();
        assert_eq!(outcome.stamped[0].original_text, "\u{3000}");

        let section = &doc.sections()[0];
        let RunContent::Table(t) = &section.paragraphs[0].runs[0].content else {
            panic!("expected table");
        };
        let cell = &t.rows[0].cells[1];
        assert_eq!(cell.paragraphs.len(), 2, "padding paragraph must survive");
        assert!(matches!(&cell.paragraphs[1].runs[0].content, RunContent::Text(t) if t.is_empty()));
    }

    #[test]
    fn ignore_covers_candidate_and_guarded_skips_without_spec() {
        let t = table(vec![
            vec![text_cell("성명"), text_cell("")],
            vec![text_cell("※ 기재 금지"), text_cell("")],
        ]);
        let mut doc = doc_with_tables(vec![t]);
        let spec = CellStampSpec {
            table: 0,
            at: GridCoord::new(0, 1),
            label: None,
            action: CellStampAction::Ignore,
        };
        let outcome = apply_cells(&mut doc, &[spec]).unwrap();
        assert_eq!(outcome.ignored, 1);
        assert_eq!(outcome.skipped_guarded.len(), 1);
        assert_eq!(outcome.skipped_guarded[0].at, GridCoord::new(1, 1));
        assert_eq!(cell_text_at(&doc, 0, 1), "", "ignored cell untouched");
    }

    // ── preflight rejections (document untouched) ───────────────

    fn assert_untouched(doc: &Document<Draft>, reference: &Document<Draft>) {
        assert_eq!(doc, reference, "failed preflight must leave the document untouched");
    }

    #[test]
    fn rejects_unknown_table_and_out_of_grid() {
        let mut doc = doc_with_tables(vec![table(vec![vec![text_cell("성명"), text_cell("")]])]);
        let reference = doc.clone();
        let err = apply_cells(&mut doc, &[field_spec(9, 0, 1, "x", "h")]).unwrap_err();
        assert!(matches!(err, CellStampError::TableNotFound { table: 9 }));
        let err = apply_cells(&mut doc, &[field_spec(0, 5, 5, "x", "h")]).unwrap_err();
        assert!(matches!(err, CellStampError::NotAnAnchor { anchor: None, .. }));
        assert_untouched(&doc, &reference);
    }

    #[test]
    fn rejects_covered_coordinate_with_anchor_pointer() {
        // 병합 target: (0,1) rowspan 2 → (1,1) 은 covered.
        let t = table(vec![
            vec![
                text_cell("비고"),
                TableCell::with_span(
                    vec![Paragraph::with_runs(
                        vec![Run::text("", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    HwpUnit::ZERO,
                    1,
                    2,
                ),
            ],
            vec![text_cell("이월")],
        ]);
        let mut doc = doc_with_tables(vec![t]);
        let err = apply_cells(&mut doc, &[field_spec(0, 1, 1, "x", "h")]).unwrap_err();
        match err {
            CellStampError::NotAnAnchor { requested, anchor: Some(anchor), .. } => {
                assert_eq!(requested, GridCoord::new(1, 1));
                assert_eq!(anchor, GridCoord::new(0, 1));
            }
            other => panic!("expected covered rejection, got {other:?}"),
        }
    }

    #[test]
    fn rejects_authored_target_and_label_drift() {
        let t = table(vec![vec![text_cell("성명"), text_cell("홍길동")]]);
        let mut doc = doc_with_tables(vec![table(vec![vec![text_cell("성명"), text_cell("")]])]);
        // authored target
        let mut doc2 = doc_with_tables(vec![t]);
        let err = apply_cells(&mut doc2, &[field_spec(0, 0, 1, "x", "h")]).unwrap_err();
        assert!(matches!(err, CellStampError::TargetNotStampable { .. }));

        // label drift: claim says "전화번호" but the live label is "성명"
        let err =
            apply_cells(&mut doc, &[labeled_spec(0, 0, 1, (0, 0), "전화번호", "x")]).unwrap_err();
        match err {
            CellStampError::LabelDrift { claimed, found, .. } => {
                assert_eq!(claimed, "전화번호");
                assert_eq!(found.as_deref(), Some("성명"));
            }
            other => panic!("expected label drift, got {other:?}"),
        }
    }

    #[test]
    fn label_claim_matches_normalized_text() {
        // 스펙이 raw "성 명"(공백 포함)을 claim 해도 normalize 매치로 통과.
        let mut doc = doc_with_tables(vec![table(vec![vec![text_cell(" 성 명 "), text_cell("")]])]);
        let outcome =
            apply_cells(&mut doc, &[labeled_spec(0, 0, 1, (0, 0), "성 명", "성명필드")]).unwrap();
        assert_eq!(outcome.stamped.len(), 1);
    }

    #[test]
    fn rejects_duplicate_target_duplicate_name_and_existing_collision() {
        let two =
            table(vec![vec![text_cell("성명"), text_cell(""), text_cell("주소"), text_cell("")]]);
        let mut doc = doc_with_tables(vec![two]);
        let err =
            apply_cells(&mut doc, &[field_spec(0, 0, 1, "a", "h"), field_spec(0, 0, 1, "b", "h")])
                .unwrap_err();
        assert!(matches!(err, CellStampError::DuplicateTarget { .. }));

        let err = apply_cells(
            &mut doc,
            &[field_spec(0, 0, 1, "같음", "h"), field_spec(0, 0, 3, "같음", "h")],
        )
        .unwrap_err();
        assert!(matches!(err, CellStampError::DuplicateName { .. }));

        // existing ClickHere named "기존" in a body paragraph
        let mut para = Paragraph::new(ParaShapeIndex::new(0));
        para.add_run(Run::control(
            Control::Field {
                field_type: FieldType::ClickHere,
                hint_text: Some("h".into()),
                help_text: None,
                name: Some("기존".into()),
                display_text: "h".into(),
            },
            CharShapeIndex::new(0),
        ));
        doc.sections_mut()[0].paragraphs.push(para);
        let err = apply_cells(
            &mut doc,
            &[field_spec(0, 0, 1, "기존", "h"), field_spec(0, 0, 3, "b", "h")],
        )
        .unwrap_err();
        assert!(matches!(err, CellStampError::NameCollision { .. }));
    }

    #[test]
    fn rejects_uncovered_unguarded_candidate() {
        let two =
            table(vec![vec![text_cell("성명"), text_cell(""), text_cell("주소"), text_cell("")]]);
        let mut doc = doc_with_tables(vec![two]);
        let reference = doc.clone();
        let err = apply_cells(&mut doc, &[field_spec(0, 0, 1, "성명만", "h")]).unwrap_err();
        match err {
            CellStampError::UncoveredCandidate { table: 0, at } => {
                assert_eq!(at, GridCoord::new(0, 3));
            }
            other => panic!("expected uncovered candidate, got {other:?}"),
        }
        assert_untouched(&doc, &reference);
    }

    #[test]
    fn ignore_on_non_candidate_is_rejected() {
        // orphan 빈 셀(라벨 없음)은 후보가 아니므로 ignore 할 것도 없다.
        let t = table(vec![vec![text_cell(""), text_cell("성명"), text_cell("")]]);
        // (0,0): 라벨이 오른쪽뿐 → orphan. (0,2): left 라벨 후보.
        let mut doc = doc_with_tables(vec![t]);
        let orphan_ignore = CellStampSpec {
            table: 0,
            at: GridCoord::new(0, 0),
            label: None,
            action: CellStampAction::Ignore,
        };
        let cover = field_spec(0, 0, 2, "성명값", "h");
        let err = apply_cells(&mut doc, &[orphan_ignore, cover]).unwrap_err();
        assert!(matches!(err, CellStampError::UnknownCandidate { .. }));
    }

    #[test]
    fn explicit_orphan_field_spec_is_allowed() {
        let t = table(vec![vec![text_cell(""), text_cell("성명"), text_cell("")]]);
        let mut doc = doc_with_tables(vec![t]);
        let orphan = field_spec(0, 0, 0, "머리빈칸", "값 입력");
        let cover = field_spec(0, 0, 2, "성명값", "성명");
        let outcome = apply_cells(&mut doc, &[orphan, cover]).unwrap();
        assert_eq!(outcome.stamped.len(), 2);
        assert_eq!(outcome.stamped[0].name, "머리빈칸");
        assert!(outcome.stamped[0].label.is_none());
    }

    #[test]
    fn reserved_names_from_text_specs_collide() {
        let mut doc = doc_with_tables(vec![table(vec![vec![text_cell("성명"), text_cell("")]])]);
        let reserved = HashSet::from(["겹침".to_string()]);
        let err =
            preflight_cells(&mut doc, &[field_spec(0, 0, 1, "겹침", "h")], &reserved).unwrap_err();
        assert!(matches!(err, CellStampError::DuplicateName { .. }));
    }
}
