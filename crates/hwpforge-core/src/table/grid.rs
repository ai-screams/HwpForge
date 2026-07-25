//! Format-neutral logical grid derivation for tables.
//!
//! A table's cells carry only spans (`col_span`/`row_span`); their absolute
//! grid positions are implicit in row/cell order. This module derives the
//! **pre-merge logical grid** — the same coordinate system used by document
//! formats on the wire — from Core data alone, using a row-major greedy
//! placement scan (cursor per row, skipping positions occupied by spans from
//! previous rows).
//!
//! Two surfaces share one placement algorithm:
//!
//! - [`TableGrid::from_table`] — **strict**: fails on any tiling violation
//!   (overlap, hole, bottom overhang, oversized grid). Addressing surfaces
//!   (export, cell editing) use this so addresses are only ever derived from
//!   well-formed grids.
//! - [`grid_placements`] — **lenient**: mirrors the historical encoder
//!   behaviour exactly, performing no validation. Format encoders use this so
//!   byte output for existing documents (including malformed tables) never
//!   changes.
//!
//! The grid validates only what Core can see. Wire-level addresses that a
//! source format may have carried are not compared here (decoders normalize
//! into Core before this module runs).

use super::Table;

/// Maximum number of logical grid positions (`rows × cols`) a grid may have.
///
/// Real-world documents are far below this (the largest table observed in a
/// 3,999-file government corpus has 414 logical positions); the cap exists to
/// stop pathological spans (e.g. `65535×65535`) from exhausting memory.
pub const MAX_GRID_POSITIONS: u64 = 1_048_576;

/// Absolute position on the pre-merge logical grid, zero-based.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
pub struct GridCoord {
    /// Zero-based logical row.
    pub row: u32,
    /// Zero-based logical column.
    pub col: u32,
}

impl GridCoord {
    /// Creates a coordinate from a row and column.
    #[must_use]
    pub const fn new(row: u32, col: u32) -> Self {
        Self { row, col }
    }
}

/// An anchor cell placed on the logical grid.
///
/// Only merge anchors (the top-left cell of a merged region) exist as cells;
/// positions covered by a span resolve back to their anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridCell {
    /// Logical position of the anchor (top-left of the merged region).
    pub anchor: GridCoord,
    /// Index into [`Table::rows`] where this cell lives.
    pub row_idx: usize,
    /// Index into the row's `cells` where this cell lives.
    pub cell_idx: usize,
    /// Number of logical rows this cell covers (≥ 1).
    pub row_span: u16,
    /// Number of logical columns this cell covers (≥ 1).
    pub col_span: u16,
}

/// Why a table's cells do not tile a well-formed logical grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum GridError {
    /// The grid would exceed [`MAX_GRID_POSITIONS`].
    TooLarge {
        /// Derived (or partially derived) row count.
        rows: u64,
        /// Derived (or partially derived) column count.
        cols: u64,
    },
    /// A cell's span covers a position already covered by another cell.
    Overlap {
        /// First doubly-covered position encountered.
        at: GridCoord,
    },
    /// A cell's `row_span` extends past the table's last row.
    OverhangsBottom {
        /// First covered position outside the grid.
        at: GridCoord,
    },
    /// A position inside the grid rectangle is covered by no cell.
    NotTiled {
        /// First uncovered position (row-major scan order).
        at: GridCoord,
    },
}

impl core::fmt::Display for GridError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooLarge { rows, cols } => write!(
                f,
                "table grid {rows}x{cols} exceeds the {MAX_GRID_POSITIONS}-position limit"
            ),
            Self::Overlap { at } => {
                write!(f, "cell spans overlap at logical position ({}, {})", at.row, at.col)
            }
            Self::OverhangsBottom { at } => write!(
                f,
                "cell row span extends past the last table row at ({}, {})",
                at.row, at.col
            ),
            Self::NotTiled { at } => {
                write!(f, "no cell covers logical position ({}, {})", at.row, at.col)
            }
        }
    }
}

impl std::error::Error for GridError {}

/// One placement produced by the lenient scan: where a cell landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacedCell {
    /// Logical position the cursor assigned to this cell.
    pub at: GridCoord,
    /// Index into [`Table::rows`].
    pub row_idx: usize,
    /// Index into the row's `cells`.
    pub cell_idx: usize,
}

/// Result of the lenient placement scan (see [`grid_placements`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridPlacements {
    /// Every cell's assigned position, in row-major table order.
    pub cells: Vec<PlacedCell>,
    /// Column count as the historical encoder derives it: the maximum
    /// row-cursor end position (equal to the true grid width for well-formed
    /// tables; may differ for malformed ones).
    pub cols: u32,
}

/// Places every cell on the logical grid without validating the result.
///
/// This mirrors the historical HWPX encoder scan exactly — per-row cursor,
/// skip positions occupied by earlier spans, mark this cell's span occupied —
/// including its silent tolerance of overlaps and ragged rows. Encoders and
/// analysis passes use this to stay byte/behaviour-identical for existing
/// documents; addressing surfaces must use [`TableGrid::from_table`] instead.
#[must_use]
pub fn grid_placements(table: &Table) -> GridPlacements {
    let mut occupied = std::collections::HashSet::<(u32, u32)>::new();
    let mut cells = Vec::new();
    let mut cols: u32 = 0;

    for (row_idx, row) in table.rows.iter().enumerate() {
        let mut cursor: u32 = 0;
        for (cell_idx, cell) in row.cells.iter().enumerate() {
            while occupied.contains(&(row_idx as u32, cursor)) {
                cursor += 1;
            }
            cells.push(PlacedCell {
                at: GridCoord::new(row_idx as u32, cursor),
                row_idx,
                cell_idx,
            });
            let col_span = u32::from(cell.col_span).max(1);
            let row_span = u32::from(cell.row_span).max(1);
            for dr in 0..row_span {
                for dc in 0..col_span {
                    occupied.insert((row_idx as u32 + dr, cursor + dc));
                }
            }
            cursor += col_span;
        }
        cols = cols.max(cursor);
    }

    GridPlacements { cells, cols }
}

/// Sums the grid area covered by every cell's span (`col_span × row_span`,
/// each floored at 1), saturating at `u64::MAX`.
///
/// This is the O(cells) pre-check that strict [`TableGrid::from_table`]
/// performs before scanning; lenient call sites compare the result against
/// [`MAX_GRID_POSITIONS`] to refuse or degrade **before** [`grid_placements`]
/// allocates per-position state. For overlapping spans the sum over-counts
/// actual coverage, which is the conservative direction for a cap guard.
#[must_use]
pub fn covered_area(table: &Table) -> u64 {
    let mut covered: u64 = 0;
    for row in &table.rows {
        for cell in &row.cells {
            let area = u64::from(cell.col_span.max(1)) * u64::from(cell.row_span.max(1));
            covered = covered.saturating_add(area);
        }
    }
    covered
}

/// Interval of covered columns within one logical row: `[start, end)` maps to
/// `anchors[idx]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RowInterval {
    start: u32,
    end: u32,
    idx: usize,
}

/// The derived logical grid of a table (strict; see module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableGrid {
    rows: u32,
    cols: u32,
    anchors: Vec<GridCell>,
    /// Per logical row, covered column intervals sorted by `start`.
    coverage: Vec<Vec<RowInterval>>,
}

impl TableGrid {
    /// Derives the logical grid, failing on any tiling violation.
    ///
    /// # Errors
    ///
    /// [`GridError::TooLarge`] when the grid would exceed
    /// [`MAX_GRID_POSITIONS`]; [`GridError::Overlap`] /
    /// [`GridError::OverhangsBottom`] / [`GridError::NotTiled`] when the
    /// cells do not tile the grid rectangle exactly.
    pub fn from_table(table: &Table) -> Result<Self, GridError> {
        let rows = table.rows.len() as u32;

        // Pre-check total covered area before any per-position allocation so
        // pathological spans cannot exhaust memory while scanning.
        let area = covered_area(table);
        if area > MAX_GRID_POSITIONS {
            return Err(GridError::TooLarge { rows: u64::from(rows), cols: area });
        }

        let mut anchors: Vec<GridCell> = Vec::new();
        let mut coverage: Vec<Vec<RowInterval>> = vec![Vec::new(); table.rows.len()];
        let mut cols: u32 = 0;

        for (row_idx, row) in table.rows.iter().enumerate() {
            let mut cursor: u32 = 0;
            for (cell_idx, cell) in row.cells.iter().enumerate() {
                while covered(&coverage[row_idx], cursor) {
                    cursor += 1;
                }
                let col_span = cell.col_span.max(1);
                let row_span = cell.row_span.max(1);
                let idx = anchors.len();
                let anchor = GridCoord::new(row_idx as u32, cursor);

                let end_row = row_idx as u64 + u64::from(row_span);
                if end_row > u64::from(rows) {
                    return Err(GridError::OverhangsBottom {
                        at: GridCoord::new(rows, anchor.col),
                    });
                }
                let end_col = u64::from(cursor) + u64::from(col_span);
                if u64::from(rows) * end_col > MAX_GRID_POSITIONS {
                    return Err(GridError::TooLarge { rows: u64::from(rows), cols: end_col });
                }

                for dr in 0..u32::from(row_span) {
                    let target = &mut coverage[row_idx + dr as usize];
                    let start = cursor;
                    let end = cursor + u32::from(col_span);
                    if let Some(col) = first_covered_in(target, start, end) {
                        return Err(GridError::Overlap {
                            at: GridCoord::new(row_idx as u32 + dr, col),
                        });
                    }
                    let pos = target.partition_point(|iv| iv.start < start);
                    target.insert(pos, RowInterval { start, end, idx });
                }

                anchors.push(GridCell { anchor, row_idx, cell_idx, row_span, col_span });
                cols = cols.max(cursor + u32::from(col_span));
                cursor += u32::from(col_span);
            }
        }

        // Every position inside rows × cols must be covered exactly once.
        // Overlaps were rejected above, so contiguity per row is sufficient.
        for (row_idx, intervals) in coverage.iter().enumerate() {
            let mut expected: u32 = 0;
            for iv in intervals {
                if iv.start != expected {
                    return Err(GridError::NotTiled {
                        at: GridCoord::new(row_idx as u32, expected),
                    });
                }
                expected = iv.end;
            }
            if expected != cols {
                return Err(GridError::NotTiled { at: GridCoord::new(row_idx as u32, expected) });
            }
        }

        Ok(Self { rows, cols, anchors, coverage })
    }

    /// Grid dimensions as `(rows, cols)` in logical positions.
    #[must_use]
    pub fn dimensions(&self) -> (u32, u32) {
        (self.rows, self.cols)
    }

    /// Resolves a logical position to the cell covering it.
    ///
    /// Positions inside a merged region resolve to the region's anchor.
    /// Returns `None` when the position lies outside the grid.
    #[must_use]
    pub fn resolve(&self, at: GridCoord) -> Option<&GridCell> {
        let intervals = self.coverage.get(at.row as usize)?;
        let idx = interval_at(intervals, at.col)?;
        Some(&self.anchors[idx])
    }

    /// Iterates over anchor cells in row-major placement order.
    pub fn iter_anchors(&self) -> impl Iterator<Item = &GridCell> {
        self.anchors.iter()
    }
}

/// Whether `col` is covered by any interval in a row.
fn covered(intervals: &[RowInterval], col: u32) -> bool {
    interval_at(intervals, col).is_some()
}

/// Index of the anchor covering `col`, if any.
fn interval_at(intervals: &[RowInterval], col: u32) -> Option<usize> {
    let pos = intervals.partition_point(|iv| iv.end <= col);
    let iv = intervals.get(pos)?;
    (iv.start <= col).then_some(iv.idx)
}

/// First covered column in `[start, end)`, if any.
fn first_covered_in(intervals: &[RowInterval], start: u32, end: u32) -> Option<u32> {
    let pos = intervals.partition_point(|iv| iv.end <= start);
    let iv = intervals.get(pos)?;
    (iv.start < end).then_some(iv.start.max(start))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paragraph::Paragraph;
    use crate::table::{TableCell, TableRow};
    use hwpforge_foundation::{HwpUnit, ParaShapeIndex};

    fn cell(row_span: u16, col_span: u16) -> TableCell {
        TableCell::with_span(
            vec![Paragraph::new(ParaShapeIndex::new(0))],
            HwpUnit::from_mm(10.0).unwrap(),
            col_span,
            row_span,
        )
    }

    fn table(rows: Vec<Vec<TableCell>>) -> Table {
        Table::new(rows.into_iter().map(TableRow::new).collect())
    }

    // === Edge cases first (TDD) ===

    #[test]
    fn empty_table_yields_zero_dimensions() {
        let grid = TableGrid::from_table(&table(vec![])).unwrap();
        assert_eq!(grid.dimensions(), (0, 0));
        assert_eq!(grid.iter_anchors().count(), 0);
        assert_eq!(grid.resolve(GridCoord::new(0, 0)), None);
    }

    #[test]
    fn all_empty_rows_yield_degenerate_zero_width_grid() {
        // A table with rows but no cells derives as rows×0 — vacuously tiled.
        // Callers deciding document validity must check width themselves
        // (Core validation keeps rejecting such tables).
        let grid = TableGrid::from_table(&table(vec![vec![], vec![]])).unwrap();
        assert_eq!(grid.dimensions(), (2, 0));
        assert_eq!(grid.iter_anchors().count(), 0);
    }

    #[test]
    fn pathological_span_rejected_as_too_large() {
        let t = table(vec![vec![cell(u16::MAX, u16::MAX)]]);
        assert!(matches!(TableGrid::from_table(&t), Err(GridError::TooLarge { .. })));
    }

    #[test]
    fn zero_span_normalized_to_one() {
        let t = table(vec![vec![cell(0, 0)]]);
        let grid = TableGrid::from_table(&t).unwrap();
        assert_eq!(grid.dimensions(), (1, 1));
    }

    #[test]
    fn row_span_overhang_rejected() {
        let t = table(vec![vec![cell(2, 1)]]);
        assert_eq!(
            TableGrid::from_table(&t),
            Err(GridError::OverhangsBottom { at: GridCoord::new(1, 0) })
        );
    }

    #[test]
    fn ragged_rows_rejected_as_not_tiled() {
        let t = table(vec![vec![cell(1, 1), cell(1, 1)], vec![cell(1, 1)]]);
        assert_eq!(
            TableGrid::from_table(&t),
            Err(GridError::NotTiled { at: GridCoord::new(1, 1) })
        );
    }

    #[test]
    fn uncovered_empty_row_rejected_as_not_tiled() {
        let t = table(vec![vec![cell(1, 1)], vec![]]);
        assert_eq!(
            TableGrid::from_table(&t),
            Err(GridError::NotTiled { at: GridCoord::new(1, 0) })
        );
    }

    #[test]
    fn overlapping_spans_rejected() {
        // Row 0: A(rs2), B, C(rs2) → 3 cols. Row 1: X(cs2) placed at col 1,
        // covering (1,1)+(1,2) — (1,2) is already covered by C.
        let t = table(vec![vec![cell(2, 1), cell(1, 1), cell(2, 1)], vec![cell(1, 2)]]);
        assert_eq!(TableGrid::from_table(&t), Err(GridError::Overlap { at: GridCoord::new(1, 2) }));
    }

    // === Well-formed grids ===

    #[test]
    fn fully_covered_empty_row_accepted() {
        let t = table(vec![vec![cell(2, 1)], vec![]]);
        let grid = TableGrid::from_table(&t).unwrap();
        assert_eq!(grid.dimensions(), (2, 1));
        let anchor = grid.resolve(GridCoord::new(1, 0)).unwrap();
        assert_eq!(anchor.anchor, GridCoord::new(0, 0));
        assert_eq!((anchor.row_idx, anchor.cell_idx), (0, 0));
    }

    #[test]
    fn hpc_form_layout_resolves_covered_positions_to_anchors() {
        // Real layout from a native government form (blank-HPC table #11):
        // 4×3 grid, 8 anchors, 4 covered positions.
        //   row 0: (0,0,rs2) (0,1,cs2)
        //   row 1: cells land at col 1, 2 (col 0 covered from above)
        //   row 2: three 1×1 cells
        //   row 3: (3,0,cs3)
        let t = table(vec![
            vec![cell(2, 1), cell(1, 2)],
            vec![cell(1, 1), cell(1, 1)],
            vec![cell(1, 1), cell(1, 1), cell(1, 1)],
            vec![cell(1, 3)],
        ]);
        let grid = TableGrid::from_table(&t).unwrap();
        assert_eq!(grid.dimensions(), (4, 3));
        assert_eq!(grid.iter_anchors().count(), 8);

        // Covered → anchor.
        let a = grid.resolve(GridCoord::new(1, 0)).unwrap();
        assert_eq!(a.anchor, GridCoord::new(0, 0));
        let b = grid.resolve(GridCoord::new(0, 2)).unwrap();
        assert_eq!(b.anchor, GridCoord::new(0, 1));
        let c = grid.resolve(GridCoord::new(3, 2)).unwrap();
        assert_eq!(c.anchor, GridCoord::new(3, 0));
        assert_eq!((c.row_idx, c.cell_idx), (3, 0));

        // Exact anchors resolve to themselves; row-1 cells landed at col 1, 2.
        let x = grid.resolve(GridCoord::new(1, 1)).unwrap();
        assert_eq!((x.row_idx, x.cell_idx), (1, 0));
        assert_eq!(x.anchor, GridCoord::new(1, 1));

        // Out of bounds.
        assert_eq!(grid.resolve(GridCoord::new(4, 0)), None);
        assert_eq!(grid.resolve(GridCoord::new(0, 3)), None);
    }

    // === Lenient placement mirrors the historical encoder ===

    #[test]
    fn lenient_placement_matches_strict_for_well_formed_tables() {
        let t = table(vec![
            vec![cell(2, 1), cell(1, 2)],
            vec![cell(1, 1), cell(1, 1)],
            vec![cell(1, 1), cell(1, 1), cell(1, 1)],
            vec![cell(1, 3)],
        ]);
        let placements = grid_placements(&t);
        let grid = TableGrid::from_table(&t).unwrap();
        assert_eq!(placements.cols, grid.dimensions().1);
        let strict: Vec<_> =
            grid.iter_anchors().map(|a| (a.anchor, a.row_idx, a.cell_idx)).collect();
        let lenient: Vec<_> =
            placements.cells.iter().map(|p| (p.at, p.row_idx, p.cell_idx)).collect();
        assert_eq!(strict, lenient);
    }

    #[test]
    fn lenient_placement_tolerates_malformed_tables() {
        // Overlap case from `overlapping_spans_rejected` — lenient must not
        // fail and must keep the historical cursor result.
        let t = table(vec![vec![cell(2, 1), cell(1, 1), cell(2, 1)], vec![cell(1, 2)]]);
        let placements = grid_placements(&t);
        assert_eq!(placements.cells.len(), 4);
        assert_eq!(placements.cells[3].at, GridCoord::new(1, 1));
        assert_eq!(placements.cols, 3);
    }

    // === covered_area pre-check primitive ===

    #[test]
    fn covered_area_sums_span_areas() {
        // rs2×cs1 + rs1×cs2 + two 1×1 = 2 + 2 + 1 + 1 = 6.
        let t = table(vec![vec![cell(2, 1), cell(1, 2)], vec![cell(1, 1), cell(1, 1)]]);
        assert_eq!(covered_area(&t), 6);
        // Empty table covers nothing.
        assert_eq!(covered_area(&table(vec![])), 0);
    }

    #[test]
    fn covered_area_floors_zero_spans_at_one() {
        // Mirrors the placement scan's `.max(1)` so the guard and the scan
        // agree on what a degenerate span occupies.
        let t = table(vec![vec![cell(0, 0), cell(0, 3)]]);
        assert_eq!(covered_area(&t), 1 + 3);
    }

    #[test]
    fn covered_area_boundary_sits_exactly_at_cap() {
        // 1024×1024 = MAX_GRID_POSITIONS exactly; guards use strict `>` so
        // this table is still allowed.
        let t = table(vec![vec![cell(1024, 1024)]]);
        assert_eq!(covered_area(&t), MAX_GRID_POSITIONS);
    }

    #[test]
    fn covered_area_saturates_instead_of_overflowing() {
        let row: Vec<TableCell> = (0..8).map(|_| cell(u16::MAX, u16::MAX)).collect();
        let t = table(vec![row]);
        assert_eq!(covered_area(&t), 8 * 4_294_836_225u64);
        assert!(covered_area(&t) > MAX_GRID_POSITIONS);
    }
}
