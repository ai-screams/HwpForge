//! Table types: [`Table`], [`TableRow`], [`TableCell`].
//!
//! Tables in HWP documents are structural containers. Each cell holds
//! its own paragraphs (rich content, not just text). Cells can span
//! multiple columns or rows via `col_span` / `row_span`.
//!
//! # Validation
//!
//! Table validation is performed at the Document level (not by Table
//! constructors) so that tables can be built incrementally. The
//! validation rules are:
//!
//! - At least 1 row
//! - Each row has at least 1 cell
//! - Each cell has at least 1 paragraph
//! - `col_span >= 1`, `row_span >= 1`
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::table::{Table, TableRow, TableCell};
//! use hwpforge_core::paragraph::Paragraph;
//! use hwpforge_foundation::{HwpUnit, ParaShapeIndex, CharShapeIndex};
//! use hwpforge_core::run::Run;
//!
//! let cell = TableCell::new(
//!     vec![Paragraph::with_runs(
//!         vec![Run::text("Hello", CharShapeIndex::new(0))],
//!         ParaShapeIndex::new(0),
//!     )],
//!     HwpUnit::from_mm(50.0).unwrap(),
//! );
//! let row = TableRow { cells: vec![cell], height: None };
//! let table = Table::new(vec![row]);
//! assert_eq!(table.row_count(), 1);
//! ```

use hwpforge_foundation::{Color, HwpUnit};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::caption::Caption;
use crate::paragraph::Paragraph;

/// A table: a sequence of rows, with optional width and caption.
///
/// # Design Decision
///
/// No `border: Option<BorderStyle>` in Phase 1. Border styling is a
/// Blueprint concern (Phase 2). Core tables are purely structural.
///
/// # Examples
///
/// ```
/// use hwpforge_core::table::{Table, TableRow, TableCell};
/// use hwpforge_core::paragraph::Paragraph;
/// use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
///
/// let table = Table {
///     rows: vec![TableRow {
///         cells: vec![TableCell::new(
///             vec![Paragraph::new(ParaShapeIndex::new(0))],
///             HwpUnit::from_mm(100.0).unwrap(),
///         )],
///         height: None,
///     }],
///     width: None,
///     caption: None,
/// };
/// assert_eq!(table.row_count(), 1);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Table {
    /// Rows of the table.
    pub rows: Vec<TableRow>,
    /// Optional explicit table width. `None` means auto-width.
    pub width: Option<HwpUnit>,
    /// Optional table caption.
    pub caption: Option<Caption>,
}

impl Table {
    /// Creates a table from rows.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::table::{Table, TableRow};
    ///
    /// let table = Table::new(vec![TableRow { cells: vec![], height: None }]);
    /// assert_eq!(table.row_count(), 1);
    /// ```
    pub fn new(rows: Vec<TableRow>) -> Self {
        Self { rows, width: None, caption: None }
    }

    /// Returns the number of rows.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Returns the number of columns (from the first row).
    ///
    /// Returns 0 if the table has no rows.
    pub fn col_count(&self) -> usize {
        self.rows.first().map_or(0, |r| r.cells.len())
    }

    /// Returns `true` if the table has no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

impl std::fmt::Display for Table {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Table({}x{})", self.row_count(), self.col_count())
    }
}

/// A single row of a table.
///
/// # Examples
///
/// ```
/// use hwpforge_core::table::{TableRow, TableCell};
/// use hwpforge_core::paragraph::Paragraph;
/// use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
///
/// let row = TableRow {
///     cells: vec![
///         TableCell::new(vec![Paragraph::new(ParaShapeIndex::new(0))], HwpUnit::from_mm(50.0).unwrap()),
///         TableCell::new(vec![Paragraph::new(ParaShapeIndex::new(0))], HwpUnit::from_mm(50.0).unwrap()),
///     ],
///     height: None,
/// };
/// assert_eq!(row.cells.len(), 2);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TableRow {
    /// Cells in this row.
    pub cells: Vec<TableCell>,
    /// Optional fixed row height. `None` means auto-height.
    pub height: Option<HwpUnit>,
}

impl TableRow {
    /// Creates a new table row with the given cells and auto-calculated height.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::table::{TableRow, TableCell};
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
    ///
    /// let cell = TableCell::new(
    ///     vec![Paragraph::new(ParaShapeIndex::new(0))],
    ///     HwpUnit::from_mm(40.0).unwrap(),
    /// );
    /// let row = TableRow::new(vec![cell]);
    /// assert!(row.height.is_none());
    /// ```
    pub fn new(cells: Vec<TableCell>) -> Self {
        Self { cells, height: None }
    }

    /// Creates a new table row with an explicit fixed height.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::table::{TableRow, TableCell};
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
    ///
    /// let cell = TableCell::new(
    ///     vec![Paragraph::new(ParaShapeIndex::new(0))],
    ///     HwpUnit::from_mm(40.0).unwrap(),
    /// );
    /// let row = TableRow::with_height(vec![cell], HwpUnit::from_mm(20.0).unwrap());
    /// assert!(row.height.is_some());
    /// ```
    pub fn with_height(cells: Vec<TableCell>, height: HwpUnit) -> Self {
        Self { cells, height: Some(height) }
    }
}

/// A single cell within a table row.
///
/// Each cell contains its own paragraphs (rich content). Spans
/// default to 1 (no spanning).
///
/// # Examples
///
/// ```
/// use hwpforge_core::table::TableCell;
/// use hwpforge_core::paragraph::Paragraph;
/// use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
///
/// let cell = TableCell::new(
///     vec![Paragraph::new(ParaShapeIndex::new(0))],
///     HwpUnit::from_mm(40.0).unwrap(),
/// );
/// assert_eq!(cell.col_span, 1);
/// assert_eq!(cell.row_span, 1);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TableCell {
    /// Rich content within the cell.
    pub paragraphs: Vec<Paragraph>,
    /// Number of columns this cell spans. Must be >= 1.
    pub col_span: u16,
    /// Number of rows this cell spans. Must be >= 1.
    pub row_span: u16,
    /// Cell width.
    pub width: HwpUnit,
    /// Optional cell background color.
    pub background: Option<Color>,
}

impl TableCell {
    /// Creates a cell with default spans (1x1) and no background.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::table::TableCell;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
    ///
    /// let cell = TableCell::new(
    ///     vec![Paragraph::new(ParaShapeIndex::new(0))],
    ///     HwpUnit::from_mm(50.0).unwrap(),
    /// );
    /// assert_eq!(cell.col_span, 1);
    /// assert_eq!(cell.row_span, 1);
    /// assert!(cell.background.is_none());
    /// ```
    pub fn new(paragraphs: Vec<Paragraph>, width: HwpUnit) -> Self {
        Self { paragraphs, col_span: 1, row_span: 1, width, background: None }
    }

    /// Creates a cell with explicit span values.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::table::TableCell;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
    ///
    /// let merged = TableCell::with_span(
    ///     vec![Paragraph::new(ParaShapeIndex::new(0))],
    ///     HwpUnit::from_mm(100.0).unwrap(),
    ///     2, // col_span
    ///     3, // row_span
    /// );
    /// assert_eq!(merged.col_span, 2);
    /// assert_eq!(merged.row_span, 3);
    /// ```
    pub fn with_span(
        paragraphs: Vec<Paragraph>,
        width: HwpUnit,
        col_span: u16,
        row_span: u16,
    ) -> Self {
        Self { paragraphs, col_span, row_span, width, background: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::Run;
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

    fn simple_paragraph() -> Paragraph {
        Paragraph::with_runs(
            vec![Run::text("cell", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )
    }

    fn simple_cell() -> TableCell {
        TableCell::new(vec![simple_paragraph()], HwpUnit::from_mm(50.0).unwrap())
    }

    fn simple_row() -> TableRow {
        TableRow { cells: vec![simple_cell(), simple_cell()], height: None }
    }

    fn simple_table() -> Table {
        Table::new(vec![simple_row(), simple_row()])
    }

    #[test]
    fn table_new() {
        let t = simple_table();
        assert_eq!(t.row_count(), 2);
        assert_eq!(t.col_count(), 2);
        assert!(!t.is_empty());
        assert!(t.width.is_none());
        assert!(t.caption.is_none());
    }

    #[test]
    fn empty_table() {
        let t = Table::new(vec![]);
        assert_eq!(t.row_count(), 0);
        assert_eq!(t.col_count(), 0);
        assert!(t.is_empty());
    }

    #[test]
    fn table_with_caption() {
        let mut t = simple_table();
        t.caption = Some(crate::caption::Caption::default());
        assert!(t.caption.is_some());
    }

    #[test]
    fn table_with_width() {
        let mut t = simple_table();
        t.width = Some(HwpUnit::from_mm(150.0).unwrap());
        assert!(t.width.is_some());
    }

    #[test]
    fn cell_new_defaults() {
        let cell = simple_cell();
        assert_eq!(cell.col_span, 1);
        assert_eq!(cell.row_span, 1);
        assert!(cell.background.is_none());
        assert_eq!(cell.paragraphs.len(), 1);
    }

    #[test]
    fn cell_with_span() {
        let cell =
            TableCell::with_span(vec![simple_paragraph()], HwpUnit::from_mm(100.0).unwrap(), 3, 2);
        assert_eq!(cell.col_span, 3);
        assert_eq!(cell.row_span, 2);
    }

    #[test]
    fn cell_with_background() {
        let mut cell = simple_cell();
        cell.background = Some(Color::from_rgb(200, 200, 200));
        assert!(cell.background.is_some());
    }

    #[test]
    fn table_display() {
        let t = simple_table();
        assert_eq!(t.to_string(), "Table(2x2)");
    }

    #[test]
    fn single_cell_table() {
        let table = Table::new(vec![TableRow {
            cells: vec![simple_cell()],
            height: Some(HwpUnit::from_mm(10.0).unwrap()),
        }]);
        assert_eq!(table.row_count(), 1);
        assert_eq!(table.col_count(), 1);
    }

    #[test]
    fn row_with_fixed_height() {
        let row =
            TableRow { cells: vec![simple_cell()], height: Some(HwpUnit::from_mm(25.0).unwrap()) };
        assert!(row.height.is_some());
    }

    #[test]
    fn row_new_auto_height() {
        let row = TableRow::new(vec![simple_cell(), simple_cell()]);
        assert_eq!(row.cells.len(), 2);
        assert!(row.height.is_none());
    }

    #[test]
    fn row_new_empty_cells() {
        let row = TableRow::new(vec![]);
        assert!(row.cells.is_empty());
        assert!(row.height.is_none());
    }

    #[test]
    fn row_with_height_constructor() {
        let h = HwpUnit::from_mm(20.0).unwrap();
        let row = TableRow::with_height(vec![simple_cell()], h);
        assert_eq!(row.cells.len(), 1);
        assert_eq!(row.height, Some(h));
    }

    #[test]
    fn equality() {
        let a = simple_table();
        let b = simple_table();
        assert_eq!(a, b);
    }

    #[test]
    fn clone_independence() {
        let t = simple_table();
        let mut cloned = t.clone();
        cloned.caption = Some(crate::caption::Caption::default());
        assert!(t.caption.is_none());
    }

    #[test]
    fn serde_roundtrip() {
        let t = simple_table();
        let json = serde_json::to_string(&t).unwrap();
        let back: Table = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn serde_with_all_optional_fields() {
        let mut t = simple_table();
        t.width = Some(HwpUnit::from_mm(150.0).unwrap());
        t.caption = Some(crate::caption::Caption::default());
        t.rows[0].height = Some(HwpUnit::from_mm(20.0).unwrap());
        t.rows[0].cells[0].background = Some(Color::from_rgb(255, 0, 0));

        let json = serde_json::to_string(&t).unwrap();
        let back: Table = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn cell_zero_span_allowed_at_construction() {
        // Zero spans are allowed during construction; validation catches them
        let cell = TableCell::with_span(
            vec![simple_paragraph()],
            HwpUnit::from_mm(50.0).unwrap(),
            0, // invalid, but construction doesn't prevent it
            0,
        );
        assert_eq!(cell.col_span, 0);
        assert_eq!(cell.row_span, 0);
    }

    #[test]
    fn row_new_equals_struct_literal() {
        let cells = vec![simple_cell()];
        let from_new = TableRow::new(cells.clone());
        let from_literal = TableRow { cells, height: None };
        assert_eq!(from_new, from_literal);
    }
}
