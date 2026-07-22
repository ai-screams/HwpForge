//! Class-B (cell-position) stamping primitives — Wave 2, P0-ⓒ.
//!
//! Two definitions everything downstream (detect/plan/apply) builds on:
//!
//! - **Stampable empty**: a cell is a class-B target only when every one of
//!   its paragraphs is exactly one whitespace-only `Text` run — the shapes
//!   native Hancom forms use for unfilled value cells (single empty run,
//!   multi-empty-paragraph row-height padding, `U+3000` full-width-space
//!   padding; blank-HPC exhibits all three across its 63 label→empty
//!   pairs). Any authored text, extra run, or non-text content disqualifies
//!   the cell — the delta gate must be able to restore the original
//!   losslessly, and apply only ever replaces the first paragraph's run.
//! - **Shared-boundary adjacency**: a label annotates a target when their
//!   merged regions share a full grid edge, not when a single probed
//!   coordinate matches — 44.8% of corpus label→empty pairs involve merged
//!   cells, and a merged target can touch several labels along one edge.
//!
//! Guards are evaluated **per label reference** (a guarded left label must
//! not demote a clean above label), and `duplicate_count` carries the
//! document-wide normalized-label tally so callers can see when a suggested
//! name is ambiguous (94% of corpus documents repeat at least one label).

use std::collections::HashMap;

use hwpforge_core::document::{Document, Draft};
use hwpforge_core::run::RunContent;
use hwpforge_core::table::grid::{GridCell, GridCoord, TableGrid};
use hwpforge_core::table::{Table, TableCell};

use super::detect::{paragraph_guard, GuardReason};
use crate::cell_edit::normalize_label;
use crate::table_inventory::{render_path, tables_in_document, TableEntry};

/// Direction from a label cell to the class-B target it annotates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LabelDirection {
    /// The label's region ends exactly at the target's left edge.
    Left,
    /// The label's region ends exactly at the target's top edge.
    Above,
}

/// One label cell adjacent to a class-B target along a shared grid boundary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct LabelRef {
    /// Which edge of the target this label touches.
    pub direction: LabelDirection,
    /// Anchor coordinate of the label cell.
    pub at: GridCoord,
    /// Raw label text (paragraphs joined with `\n`, un-normalized).
    pub raw: String,
    /// Normalized label text (NFC + whitespace collapse) — the identity used
    /// for duplicate counting and drift re-verification.
    pub normalized: String,
    /// Instruction-context guard for THIS label reference only.
    pub guard: Option<GuardReason>,
    /// Document-wide occurrence count of `normalized` among label cells.
    pub duplicate_count: usize,
}

/// Returns `true` iff `cell` is a stampable empty class-B target: at least
/// one paragraph, and EVERY paragraph holds exactly one
/// [`RunContent::Text`] run whose text is empty or Unicode whitespace.
///
/// Native Hancom forms pad value cells with multiple empty paragraphs (row
/// height) and full-width spaces (`U+3000`) — blank-HPC has 17 multi-empty-
/// paragraph and 3 `U+3000` value cells among its 63 label→empty pairs, so
/// the stricter "exactly one `Text(\"\")` run" predicate covers only 70% of
/// the acceptance fixture. Whitespace padding is not authored content: the
/// apply phase replaces only the FIRST paragraph's run (geometry preserved)
/// and the delta records the original run text verbatim, so reversal stays
/// exact. Multi-run paragraphs and non-text content remain excluded.
pub(crate) fn is_stampable_empty(cell: &TableCell) -> bool {
    !cell.paragraphs.is_empty()
        && cell.paragraphs.iter().all(|paragraph| {
            matches!(paragraph.runs.as_slice(),
                [run] if matches!(&run.content, RunContent::Text(text) if text.trim().is_empty()))
        })
}

/// Raw cell text: all paragraph text joined with `\n` (no normalization).
fn raw_cell_text(table: &Table, cell: &GridCell) -> String {
    table.rows[cell.row_idx].cells[cell.cell_idx]
        .paragraphs
        .iter()
        .map(|p| p.text_content())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Collects every label cell adjacent to `target` along a shared grid edge.
///
/// A neighbor qualifies as a label when its normalized text is non-empty.
/// `Left` labels are regions whose column range ends exactly at the target's
/// left edge with overlapping row ranges; `Above` labels end exactly at the
/// target's top edge with overlapping column ranges. Results are ordered
/// `Left` before `Above`, each group by ascending `(row, col)`.
///
/// `duplicates` is the document-wide normalized-label tally (see
/// [`tally_normalized_labels`]); unknown labels get `duplicate_count = 1`.
pub(crate) fn adjacent_labels(
    table: &Table,
    grid: &TableGrid,
    target: &GridCell,
    duplicates: &HashMap<String, usize>,
) -> Vec<LabelRef> {
    let t_row = target.anchor.row;
    let t_col = target.anchor.col;
    let t_row_end = t_row + u32::from(target.row_span);
    let t_col_end = t_col + u32::from(target.col_span);

    let mut refs = Vec::new();
    for label in grid.iter_anchors() {
        let l_row_end = label.anchor.row + u32::from(label.row_span);
        let l_col_end = label.anchor.col + u32::from(label.col_span);

        let direction = if l_col_end == t_col && label.anchor.row < t_row_end && t_row < l_row_end {
            LabelDirection::Left
        } else if l_row_end == t_row && label.anchor.col < t_col_end && t_col < l_col_end {
            LabelDirection::Above
        } else {
            continue;
        };

        let raw = raw_cell_text(table, label);
        let normalized = normalize_label(&raw);
        if normalized.is_empty() {
            continue;
        }
        let duplicate_count = duplicates.get(&normalized).copied().unwrap_or(1);
        let guard = paragraph_guard(&raw);
        refs.push(LabelRef {
            direction,
            at: label.anchor,
            raw,
            normalized,
            guard,
            duplicate_count,
        });
    }
    refs.sort_by_key(|r| (r.direction == LabelDirection::Above, r.at.row, r.at.col));
    refs
}

/// Adds every non-empty normalized label text of `table` into `tally`
/// (document-wide aggregation happens across tables at plan time).
pub(crate) fn tally_normalized_labels(
    table: &Table,
    grid: &TableGrid,
    tally: &mut HashMap<String, usize>,
) {
    for anchor in grid.iter_anchors() {
        let normalized = normalize_label(&raw_cell_text(table, anchor));
        if !normalized.is_empty() {
            *tally.entry(normalized).or_insert(0) += 1;
        }
    }
}

/// One detected class-B candidate: a stampable empty cell with at least one
/// adjacent label.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CellStampCandidate {
    /// Table ordinal (shared inventory DFS pre-order, across sections).
    pub table: usize,
    /// Section index the table lives in.
    pub section: usize,
    /// Canonical anchor coordinate of the empty target cell.
    pub at: GridCoord,
    /// Every adjacent label reference (`Left` before `Above`).
    pub labels: Vec<LabelRef>,
    /// True when every adjacent label is guarded: the candidate is never
    /// auto-required in preflight and needs an explicit spec to be stamped
    /// (mirrors class-A guarded semantics).
    pub guarded: bool,
    /// Normalized text of the preferred clean label (first `Left`, else
    /// first `Above`), present only when unique in the document — a
    /// grounded name suggestion, never applied automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_name: Option<String>,
    /// Raw text of the preferred clean label — a grounded hint suggestion,
    /// never applied automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_hint: Option<String>,
}

/// A table whose logical grid could not be built during [`plan_cells`] —
/// reported as an explicit incomplete-coverage diagnostic, never silently
/// skipped.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SkippedTable {
    /// Table ordinal (shared inventory DFS pre-order).
    pub table: usize,
    /// Serde-shaped path of the table (from the document root).
    pub path: String,
    /// Why the grid failed (first violation, human-readable).
    pub error: String,
}

/// Result of class-B detection over a whole document.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CellPlan {
    /// Detected candidates in document order (table ordinal, then row-major
    /// anchor order).
    pub candidates: Vec<CellStampCandidate>,
    /// Tables excluded from detection because their grid is invalid.
    pub skipped_tables: Vec<SkippedTable>,
}

/// Enumerates class-B cell candidates across all tables of the document.
///
/// Read-only. A candidate is a [stampable empty](is_stampable_empty) anchor
/// cell with at least one [adjacent label](adjacent_labels); orphan empty
/// cells (53% in the government-form corpus — matrix interiors and spacers)
/// are NOT candidates and can only be targeted by explicit specs. Tables
/// whose grid cannot be built are reported in
/// [`skipped_tables`](CellPlan::skipped_tables).
pub fn plan_cells(document: &Document<Draft>) -> CellPlan {
    let entries = tables_in_document(document);

    // Pass 1: per-table grids + the document-wide normalized-label tally.
    let mut grids: Vec<Option<TableGrid>> = Vec::with_capacity(entries.len());
    let mut skipped_tables = Vec::new();
    let mut tally = HashMap::new();
    for entry in &entries {
        match TableGrid::from_table(entry.table) {
            Ok(grid) => {
                tally_normalized_labels(entry.table, &grid, &mut tally);
                grids.push(Some(grid));
            }
            Err(error) => {
                skipped_tables.push(SkippedTable {
                    table: entry.ordinal,
                    path: table_path(entry),
                    error: error.to_string(),
                });
                grids.push(None);
            }
        }
    }

    // Pass 2: canonical empty anchors with adjacent labels become candidates.
    let mut candidates = Vec::new();
    for (entry, grid) in entries.iter().zip(&grids) {
        let Some(grid) = grid else {
            continue;
        };
        for anchor in grid.iter_anchors() {
            let cell = &entry.table.rows[anchor.row_idx].cells[anchor.cell_idx];
            if !is_stampable_empty(cell) {
                continue;
            }
            let labels = adjacent_labels(entry.table, grid, anchor, &tally);
            if labels.is_empty() {
                continue;
            }
            let guarded = labels.iter().all(|label| label.guard.is_some());
            let preferred = labels.iter().find(|label| label.guard.is_none());
            let suggested_name = preferred
                .filter(|label| label.duplicate_count == 1)
                .map(|label| label.normalized.clone());
            let suggested_hint = preferred.map(|label| label.raw.clone());
            candidates.push(CellStampCandidate {
                table: entry.ordinal,
                section: entry.section,
                at: anchor.anchor,
                labels,
                guarded,
                suggested_name,
                suggested_hint,
            });
        }
    }

    CellPlan { candidates, skipped_tables }
}

fn table_path(entry: &TableEntry<'_>) -> String {
    format!("sections[{}].{}", entry.section, render_path("", &entry.path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::Run;
    use hwpforge_core::table::TableRow;
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    fn text_cell(text: &str) -> TableCell {
        TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text(text, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::ZERO,
        )
    }

    fn spanned_cell(text: &str, col_span: u16, row_span: u16) -> TableCell {
        TableCell::with_span(
            vec![Paragraph::with_runs(
                vec![Run::text(text, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::ZERO,
            col_span,
            row_span,
        )
    }

    fn table(rows: Vec<Vec<TableCell>>) -> Table {
        Table::new(rows.into_iter().map(TableRow::new).collect())
    }

    fn anchor_at(grid: &TableGrid, row: u32, col: u32) -> GridCell {
        *grid.resolve(GridCoord::new(row, col)).expect("anchor")
    }

    // ── stampable empty ─────────────────────────────────────────

    #[test]
    fn stampable_empty_accepts_single_empty_text_run() {
        assert!(is_stampable_empty(&text_cell("")));
    }

    #[test]
    fn stampable_empty_accepts_whitespace_padding_but_not_text() {
        // 네이티브 한컴 양식은 값 칸을 전각 공백(U+3000)으로 패딩한다.
        assert!(is_stampable_empty(&text_cell("  ")));
        assert!(is_stampable_empty(&text_cell("\u{3000}")));
        assert!(!is_stampable_empty(&text_cell("성명")));
    }

    #[test]
    fn stampable_empty_accepts_multi_empty_paragraph_cell() {
        // blank-HPC 쌍 63개 중 17개가 이 모양 (빈 문단 여러 개 = 행 높이).
        let paragraph = |text: &str| {
            Paragraph::with_runs(
                vec![Run::text(text, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )
        };
        let cell = TableCell::new(
            vec![paragraph(""), paragraph("\u{3000}"), paragraph("")],
            HwpUnit::ZERO,
        );
        assert!(is_stampable_empty(&cell));

        let with_text = TableCell::new(vec![paragraph(""), paragraph("메모")], HwpUnit::ZERO);
        assert!(!is_stampable_empty(&with_text));
    }

    #[test]
    fn stampable_empty_rejects_zero_paragraphs() {
        let cell = TableCell::new(vec![], HwpUnit::ZERO);
        assert!(!is_stampable_empty(&cell));
    }

    #[test]
    fn stampable_empty_rejects_multiple_runs() {
        let cell = TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("", CharShapeIndex::new(0)), Run::text("", CharShapeIndex::new(1))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::ZERO,
        );
        assert!(!is_stampable_empty(&cell));
    }

    #[test]
    fn stampable_empty_rejects_non_text_content() {
        let nested = Run {
            content: RunContent::Table(Box::new(table(vec![vec![text_cell("")]]))),
            char_shape_id: CharShapeIndex::new(0),
        };
        let cell = TableCell::new(
            vec![Paragraph::with_runs(vec![nested], ParaShapeIndex::new(0))],
            HwpUnit::ZERO,
        );
        assert!(!is_stampable_empty(&cell));
    }

    // ── shared-boundary adjacency ───────────────────────────────

    #[test]
    fn left_and_above_labels_found() {
        // ┌────────┬────────┐
        // │ 제목    │ 성명    │
        // ├────────┼────────┤
        // │ 주소    │ (빈칸)  │
        // └────────┴────────┘
        let t = table(vec![
            vec![text_cell("제목"), text_cell("성명")],
            vec![text_cell("주소"), text_cell("")],
        ]);
        let grid = TableGrid::from_table(&t).unwrap();
        let target = anchor_at(&grid, 1, 1);
        let refs = adjacent_labels(&t, &grid, &target, &HashMap::new());
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].direction, LabelDirection::Left);
        assert_eq!(refs[0].normalized, "주소");
        assert_eq!(refs[0].at, GridCoord::new(1, 0));
        assert_eq!(refs[1].direction, LabelDirection::Above);
        assert_eq!(refs[1].normalized, "성명");
        assert_eq!(refs[1].at, GridCoord::new(0, 1));
    }

    #[test]
    fn merged_label_reaches_every_row_it_borders() {
        // ┌────────┬────────┐
        // │ 비고    │ (빈칸A) │
        // │ (r2)   ├────────┤
        // │        │ (빈칸B) │
        // └────────┴────────┘
        let t = table(vec![vec![spanned_cell("비고", 1, 2), text_cell("")], vec![text_cell("")]]);
        let grid = TableGrid::from_table(&t).unwrap();
        for row in 0..2 {
            let target = anchor_at(&grid, row, 1);
            let refs = adjacent_labels(&t, &grid, &target, &HashMap::new());
            assert_eq!(refs.len(), 1, "row {row}");
            assert_eq!(refs[0].direction, LabelDirection::Left);
            assert_eq!(refs[0].normalized, "비고");
        }
    }

    #[test]
    fn merged_target_collects_multiple_left_labels() {
        // ┌────────┬────────┐
        // │ 성명    │ (빈칸   │
        // ├────────┤  r2)   │
        // │ 주소    │        │
        // └────────┴────────┘
        let t =
            table(vec![vec![text_cell("성명"), spanned_cell("", 1, 2)], vec![text_cell("주소")]]);
        let grid = TableGrid::from_table(&t).unwrap();
        let target = anchor_at(&grid, 0, 1);
        let refs = adjacent_labels(&t, &grid, &target, &HashMap::new());
        assert_eq!(refs.len(), 2);
        assert_eq!(
            refs.iter().map(|r| r.normalized.as_str()).collect::<Vec<_>>(),
            vec!["성명", "주소"],
        );
        assert!(refs.iter().all(|r| r.direction == LabelDirection::Left));
    }

    #[test]
    fn non_touching_and_empty_neighbors_are_not_labels() {
        // 라벨과 target 사이에 빈 셀이 끼면 인접이 아니다.
        let t = table(vec![vec![text_cell("성명"), text_cell(""), text_cell("")]]);
        let grid = TableGrid::from_table(&t).unwrap();
        let target = anchor_at(&grid, 0, 2);
        assert!(adjacent_labels(&t, &grid, &target, &HashMap::new()).is_empty());
    }

    #[test]
    fn guard_is_evaluated_per_label_reference() {
        // left = 안내문 라벨(guarded), above = 정상 라벨(clean).
        let t = table(vec![
            vec![text_cell("제목"), text_cell("성명")],
            vec![text_cell("※ 아래 빈칸은 기재하지 마십시오"), text_cell("")],
        ]);
        let grid = TableGrid::from_table(&t).unwrap();
        let target = anchor_at(&grid, 1, 1);
        let refs = adjacent_labels(&t, &grid, &target, &HashMap::new());
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].direction, LabelDirection::Left);
        assert_eq!(refs[0].guard, Some(GuardReason::InstructionContext));
        assert_eq!(refs[1].direction, LabelDirection::Above);
        assert_eq!(refs[1].guard, None);
    }

    #[test]
    fn duplicate_count_comes_from_document_tally() {
        let t = table(vec![vec![text_cell("과제명"), text_cell("")]]);
        let grid = TableGrid::from_table(&t).unwrap();
        let target = anchor_at(&grid, 0, 1);
        let tally = HashMap::from([("과제명".to_string(), 3)]);
        let refs = adjacent_labels(&t, &grid, &target, &tally);
        assert_eq!(refs[0].duplicate_count, 3);

        let refs = adjacent_labels(&t, &grid, &target, &HashMap::new());
        assert_eq!(refs[0].duplicate_count, 1);
    }

    // ── plan_cells ──────────────────────────────────────────────

    use hwpforge_core::page::PageSettings;
    use hwpforge_core::{Control, Section};
    use hwpforge_foundation::FieldType;

    fn table_run(t: Table) -> Run {
        Run { content: RunContent::Table(Box::new(t)), char_shape_id: CharShapeIndex::new(0) }
    }

    fn doc_with_tables(tables: Vec<Table>) -> hwpforge_core::Document {
        let paras = tables
            .into_iter()
            .map(|t| Paragraph::with_runs(vec![table_run(t)], ParaShapeIndex::new(0)))
            .collect();
        let mut doc = hwpforge_core::Document::new();
        doc.add_section(Section::with_paragraphs(paras, PageSettings::default()));
        doc
    }

    #[test]
    fn plan_cells_empty_document_is_empty() {
        let plan = plan_cells(&hwpforge_core::Document::new());
        assert!(plan.candidates.is_empty());
        assert!(plan.skipped_tables.is_empty());
    }

    #[test]
    fn plan_cells_detects_left_labeled_empty_cell() {
        let doc = doc_with_tables(vec![table(vec![vec![text_cell("성명"), text_cell("")]])]);
        let plan = plan_cells(&doc);
        assert!(plan.skipped_tables.is_empty());
        assert_eq!(plan.candidates.len(), 1);
        let c = &plan.candidates[0];
        assert_eq!((c.table, c.section), (0, 0));
        assert_eq!(c.at, GridCoord::new(0, 1));
        assert_eq!(c.labels.len(), 1);
        assert!(!c.guarded);
        assert_eq!(c.suggested_name.as_deref(), Some("성명"));
        assert_eq!(c.suggested_hint.as_deref(), Some("성명"));
    }

    #[test]
    fn plan_cells_duplicate_label_suppresses_suggested_name_only() {
        let make = || table(vec![vec![text_cell("성명"), text_cell("")]]);
        let doc = doc_with_tables(vec![make(), make()]);
        let plan = plan_cells(&doc);
        assert_eq!(plan.candidates.len(), 2);
        for c in &plan.candidates {
            assert_eq!(c.labels[0].duplicate_count, 2);
            assert_eq!(c.suggested_name, None);
            assert_eq!(c.suggested_hint.as_deref(), Some("성명"));
        }
    }

    #[test]
    fn plan_cells_fully_guarded_candidate_has_no_suggestions() {
        let doc = doc_with_tables(vec![table(vec![vec![
            text_cell("※ 이 난은 기재하지 마십시오"),
            text_cell(""),
        ]])]);
        let plan = plan_cells(&doc);
        assert_eq!(plan.candidates.len(), 1);
        let c = &plan.candidates[0];
        assert!(c.guarded);
        assert_eq!(c.suggested_name, None);
        assert_eq!(c.suggested_hint, None);
    }

    #[test]
    fn plan_cells_mixed_guard_prefers_clean_label() {
        // left = guarded 안내문, above = clean 라벨 → guarded=false 이고
        // 제안은 clean 라벨에서 나온다.
        let doc = doc_with_tables(vec![table(vec![
            vec![text_cell("제목"), text_cell("성명")],
            vec![text_cell("※ 안내"), text_cell("")],
        ])]);
        let plan = plan_cells(&doc);
        assert_eq!(plan.candidates.len(), 1);
        let c = &plan.candidates[0];
        assert!(!c.guarded);
        assert_eq!(c.labels.len(), 2);
        assert_eq!(c.suggested_name.as_deref(), Some("성명"));
        assert_eq!(c.suggested_hint.as_deref(), Some("성명"));
    }

    #[test]
    fn plan_cells_invalid_grid_is_reported_not_silently_skipped() {
        // 표 A = ragged (2행이 1셀뿐, hole → NotTiled), 표 B = 정상.
        let ragged = table(vec![vec![text_cell("성명"), text_cell("")], vec![text_cell("주소")]]);
        let ok = table(vec![vec![text_cell("기관명"), text_cell("")]]);
        let doc = doc_with_tables(vec![ragged, ok]);
        let plan = plan_cells(&doc);
        assert_eq!(plan.skipped_tables.len(), 1);
        assert_eq!(plan.skipped_tables[0].table, 0);
        assert!(plan.skipped_tables[0].path.contains("sections[0]"));
        assert!(!plan.skipped_tables[0].error.is_empty());
        assert_eq!(plan.candidates.len(), 1);
        assert_eq!(plan.candidates[0].table, 1);
    }

    #[test]
    fn plan_cells_orphan_and_authored_cells_are_not_candidates() {
        // (0,0) 빈 셀은 라벨이 오른쪽에만 있음(orphan) · (1,1) 은 authored 텍스트.
        let doc = doc_with_tables(vec![table(vec![
            vec![text_cell(""), text_cell("성명")],
            vec![text_cell("주소"), text_cell("기입됨")],
        ])]);
        assert!(plan_cells(&doc).candidates.is_empty());
    }

    #[test]
    fn plan_cells_whitespace_padded_cell_is_a_candidate() {
        // 전각 공백 패딩 칸(blank-HPC t3 실물 패턴)도 자동 후보다.
        let doc =
            doc_with_tables(vec![table(vec![vec![text_cell("성명"), text_cell("\u{3000}")]])]);
        let plan = plan_cells(&doc);
        assert_eq!(plan.candidates.len(), 1);
        assert_eq!(plan.candidates[0].at, GridCoord::new(0, 1));
    }

    #[test]
    fn plan_cells_nested_table_gets_its_own_ordinal() {
        let inner = table(vec![vec![text_cell("항목"), text_cell("")]]);
        let outer_cell = TableCell::new(
            vec![Paragraph::with_runs(vec![table_run(inner)], ParaShapeIndex::new(0))],
            HwpUnit::ZERO,
        );
        let outer = Table::new(vec![TableRow::new(vec![outer_cell])]);
        let doc = doc_with_tables(vec![outer]);
        let plan = plan_cells(&doc);
        assert_eq!(plan.candidates.len(), 1);
        assert_eq!(plan.candidates[0].table, 1); // DFS pre-order: outer=0, inner=1
        assert_eq!(plan.candidates[0].at, GridCoord::new(0, 1));
    }

    #[test]
    fn plan_cells_existing_clickhere_cell_is_not_a_candidate() {
        let mut para = Paragraph::new(ParaShapeIndex::new(0));
        para.add_run(Run::control(
            Control::Field {
                field_type: FieldType::ClickHere,
                hint_text: Some("힌트".to_string()),
                help_text: None,
                name: Some("이미승격".to_string()),
                display_text: "힌트".to_string(),
            },
            CharShapeIndex::new(0),
        ));
        let field_cell = TableCell::new(vec![para], HwpUnit::ZERO);
        let t = Table::new(vec![TableRow::new(vec![text_cell("성명"), field_cell])]);
        let doc = doc_with_tables(vec![t]);
        assert!(plan_cells(&doc).candidates.is_empty());
    }

    #[test]
    fn tally_counts_normalized_labels_per_table() {
        let t = table(vec![
            vec![text_cell(" 성명 "), text_cell("성명")],
            vec![text_cell("주소"), text_cell("")],
        ]);
        let grid = TableGrid::from_table(&t).unwrap();
        let mut tally = HashMap::new();
        tally_normalized_labels(&t, &grid, &mut tally);
        assert_eq!(tally.get("성명"), Some(&2));
        assert_eq!(tally.get("주소"), Some(&1));
        assert_eq!(tally.len(), 2);
    }
}
