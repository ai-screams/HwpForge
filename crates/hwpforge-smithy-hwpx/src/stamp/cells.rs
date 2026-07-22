//! Class-B (cell-position) stamping primitives — Wave 2, P0-ⓒ.
//!
//! Two definitions everything downstream (detect/plan/apply) builds on:
//!
//! - **Canonical empty**: a cell is a class-B target only when it is exactly
//!   one paragraph holding exactly one empty `Text` run — the shape the HWPX
//!   decoder produces for visually empty form cells (99.6% of empty cells in
//!   the government-form corpus). Whitespace-only or multi-run cells are NOT
//!   canonical; they carry authored content the delta gate cannot restore
//!   losslessly.
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

use hwpforge_core::run::RunContent;
use hwpforge_core::table::grid::{GridCell, GridCoord, TableGrid};
use hwpforge_core::table::{Table, TableCell};

use super::detect::{paragraph_guard, GuardReason};
use crate::cell_edit::normalize_label;

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

/// Returns `true` iff `cell` is a canonical empty class-B target: exactly one
/// paragraph holding exactly one empty [`RunContent::Text`] run.
#[allow(dead_code)] // consumed by the Wave 2A plan slice
pub(crate) fn is_canonical_empty(cell: &TableCell) -> bool {
    let [paragraph] = cell.paragraphs.as_slice() else {
        return false;
    };
    let [run] = paragraph.runs.as_slice() else {
        return false;
    };
    matches!(&run.content, RunContent::Text(text) if text.is_empty())
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
#[allow(dead_code)] // consumed by the Wave 2A plan slice
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
#[allow(dead_code)] // consumed by the Wave 2A plan slice
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

    // ── canonical empty ─────────────────────────────────────────

    #[test]
    fn canonical_empty_accepts_single_empty_text_run() {
        assert!(is_canonical_empty(&text_cell("")));
    }

    #[test]
    fn canonical_empty_rejects_whitespace_text() {
        assert!(!is_canonical_empty(&text_cell("  ")));
        assert!(!is_canonical_empty(&text_cell("성명")));
    }

    #[test]
    fn canonical_empty_rejects_multiple_runs() {
        let cell = TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("", CharShapeIndex::new(0)), Run::text("", CharShapeIndex::new(1))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::ZERO,
        );
        assert!(!is_canonical_empty(&cell));
    }

    #[test]
    fn canonical_empty_rejects_multiple_paragraphs() {
        let paragraph = || {
            Paragraph::with_runs(
                vec![Run::text("", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )
        };
        let cell = TableCell::new(vec![paragraph(), paragraph()], HwpUnit::ZERO);
        assert!(!is_canonical_empty(&cell));
    }

    #[test]
    fn canonical_empty_rejects_non_text_content() {
        let nested = Run {
            content: RunContent::Table(Box::new(table(vec![vec![text_cell("")]]))),
            char_shape_id: CharShapeIndex::new(0),
        };
        let cell = TableCell::new(
            vec![Paragraph::with_runs(vec![nested], ParaShapeIndex::new(0))],
            HwpUnit::ZERO,
        );
        assert!(!is_canonical_empty(&cell));
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
