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
//! let row = TableRow::new(vec![cell]);
//! let table = Table::new(vec![row]);
//! assert_eq!(table.row_count(), 1);
//! ```

use hwpforge_foundation::{Color, HwpUnit};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::caption::Caption;
use crate::paragraph::Paragraph;

/// Page-break policy for a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TablePageBreak {
    /// Split the table at cell boundaries.
    #[default]
    Cell,
    /// Split the table as a whole unit.
    Table,
    /// Do not split the table across pages.
    None,
}

/// Vertical alignment for content inside a table cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TableVerticalAlign {
    /// Align cell content to the top edge.
    Top,
    /// Center cell content vertically.
    #[default]
    Center,
    /// Align cell content to the bottom edge.
    Bottom,
}

/// Explicit margins inside a table cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct TableMargin {
    /// Left margin in HWP units.
    pub left: HwpUnit,
    /// Right margin in HWP units.
    pub right: HwpUnit,
    /// Top margin in HWP units.
    pub top: HwpUnit,
    /// Bottom margin in HWP units.
    pub bottom: HwpUnit,
}

fn default_repeat_header() -> bool {
    true
}

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
/// use hwpforge_core::table::{Table, TableCell, TablePageBreak, TableRow};
/// use hwpforge_core::paragraph::Paragraph;
/// use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
///
/// let table = Table::new(vec![TableRow::new(vec![TableCell::new(
///     vec![Paragraph::new(ParaShapeIndex::new(0))],
///     HwpUnit::from_mm(100.0).unwrap(),
/// )])])
/// .with_page_break(TablePageBreak::Cell);
/// assert_eq!(table.row_count(), 1);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct Table {
    /// Rows of the table.
    pub rows: Vec<TableRow>,
    /// Optional explicit table width. `None` means auto-width.
    pub width: Option<HwpUnit>,
    /// Optional table caption.
    pub caption: Option<Caption>,
    /// Page-break policy for this table.
    #[serde(default)]
    pub page_break: TablePageBreak,
    /// Whether the first row repeats across page breaks.
    #[serde(default = "default_repeat_header")]
    pub repeat_header: bool,
    /// Optional explicit spacing between table cells.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_spacing: Option<HwpUnit>,
    /// Optional table-level border/fill reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_fill_id: Option<u32>,
}

impl Table {
    /// Creates a table from rows.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::table::{Table, TableRow};
    ///
    /// let table = Table::new(vec![TableRow::new(vec![])]);
    /// assert_eq!(table.row_count(), 1);
    /// ```
    #[must_use]
    pub fn new(rows: Vec<TableRow>) -> Self {
        Self {
            rows,
            width: None,
            caption: None,
            page_break: TablePageBreak::Cell,
            repeat_header: true,
            cell_spacing: None,
            border_fill_id: None,
        }
    }

    /// Sets an explicit table width.
    #[must_use]
    pub fn with_width(mut self, width: HwpUnit) -> Self {
        self.width = Some(width);
        self
    }

    /// Attaches a table caption.
    #[must_use]
    pub fn with_caption(mut self, caption: Caption) -> Self {
        self.caption = Some(caption);
        self
    }

    /// Sets the page-break policy for this table.
    #[must_use]
    pub fn with_page_break(mut self, page_break: TablePageBreak) -> Self {
        self.page_break = page_break;
        self
    }

    /// Controls whether the leading header block repeats across page breaks.
    #[must_use]
    pub fn with_repeat_header(mut self, repeat_header: bool) -> Self {
        self.repeat_header = repeat_header;
        self
    }

    /// Sets the explicit spacing between cells.
    #[must_use]
    pub fn with_cell_spacing(mut self, cell_spacing: HwpUnit) -> Self {
        self.cell_spacing = Some(cell_spacing);
        self
    }

    /// Sets the table-level border/fill reference.
    #[must_use]
    pub fn with_border_fill_id(mut self, border_fill_id: u32) -> Self {
        self.border_fill_id = Some(border_fill_id);
        self
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
/// let row = TableRow::new(vec![
///     TableCell::new(vec![Paragraph::new(ParaShapeIndex::new(0))], HwpUnit::from_mm(50.0).unwrap()),
///     TableCell::new(vec![Paragraph::new(ParaShapeIndex::new(0))], HwpUnit::from_mm(50.0).unwrap()),
/// ]);
/// assert_eq!(row.cells.len(), 2);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct TableRow {
    /// Cells in this row.
    pub cells: Vec<TableCell>,
    /// Optional fixed row height. `None` means auto-height.
    pub height: Option<HwpUnit>,
    /// Whether this row is part of the table's leading header-row block.
    #[serde(default)]
    pub is_header: bool,
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
    #[must_use]
    pub fn new(cells: Vec<TableCell>) -> Self {
        Self { cells, height: None, is_header: false }
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
    #[must_use]
    pub fn with_height(cells: Vec<TableCell>, height: HwpUnit) -> Self {
        Self { cells, height: Some(height), is_header: false }
    }

    /// Marks whether this row belongs to the table's leading header-row block.
    #[must_use]
    pub fn with_header(mut self, is_header: bool) -> Self {
        self.is_header = is_header;
        self
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
#[non_exhaustive]
pub struct TableCell {
    /// Rich content within the cell.
    pub paragraphs: Vec<Paragraph>,
    /// Number of columns this cell spans. Must be >= 1.
    pub col_span: u16,
    /// Number of rows this cell spans. Must be >= 1.
    pub row_span: u16,
    /// Cell width.
    pub width: HwpUnit,
    /// Optional explicit cell height.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<HwpUnit>,
    /// Optional cell background color.
    pub background: Option<Color>,
    /// Optional border/fill reference for this cell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_fill_id: Option<u32>,
    /// Optional cell-local margin override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin: Option<TableMargin>,
    /// Optional vertical alignment override for the cell content box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_align: Option<TableVerticalAlign>,
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
    #[must_use]
    pub fn new(paragraphs: Vec<Paragraph>, width: HwpUnit) -> Self {
        Self {
            paragraphs,
            col_span: 1,
            row_span: 1,
            width,
            height: None,
            background: None,
            border_fill_id: None,
            margin: None,
            vertical_align: None,
        }
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
    #[must_use]
    pub fn with_span(
        paragraphs: Vec<Paragraph>,
        width: HwpUnit,
        col_span: u16,
        row_span: u16,
    ) -> Self {
        Self {
            paragraphs,
            col_span,
            row_span,
            width,
            height: None,
            background: None,
            border_fill_id: None,
            margin: None,
            vertical_align: None,
        }
    }

    /// Sets an explicit cell height.
    #[must_use]
    pub fn with_height(mut self, height: HwpUnit) -> Self {
        self.height = Some(height);
        self
    }

    /// Sets the cell background color.
    #[must_use]
    pub fn with_background(mut self, background: Color) -> Self {
        self.background = Some(background);
        self
    }

    /// Sets the cell border/fill reference.
    #[must_use]
    pub fn with_border_fill_id(mut self, border_fill_id: u32) -> Self {
        self.border_fill_id = Some(border_fill_id);
        self
    }

    /// Sets the cell-local margin override.
    #[must_use]
    pub fn with_margin(mut self, margin: TableMargin) -> Self {
        self.margin = Some(margin);
        self
    }

    /// Sets the vertical alignment override for the cell content box.
    #[must_use]
    pub fn with_vertical_align(mut self, vertical_align: TableVerticalAlign) -> Self {
        self.vertical_align = Some(vertical_align);
        self
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
        TableRow::new(vec![simple_cell(), simple_cell()])
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
        assert_eq!(t.page_break, TablePageBreak::Cell);
        assert!(t.repeat_header);
        assert!(t.cell_spacing.is_none());
        assert!(t.border_fill_id.is_none());
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
        let t = simple_table().with_caption(crate::caption::Caption::default());
        assert!(t.caption.is_some());
    }

    #[test]
    fn table_with_width() {
        let t = simple_table().with_width(HwpUnit::from_mm(150.0).unwrap());
        assert!(t.width.is_some());
    }

    #[test]
    fn table_with_page_break() {
        let t = simple_table().with_page_break(TablePageBreak::Table);
        assert_eq!(t.page_break, TablePageBreak::Table);
    }

    #[test]
    fn table_with_repeat_header_disabled() {
        let t = simple_table().with_repeat_header(false);
        assert!(!t.repeat_header);
    }

    #[test]
    fn cell_new_defaults() {
        let cell = simple_cell();
        assert_eq!(cell.col_span, 1);
        assert_eq!(cell.row_span, 1);
        assert!(cell.height.is_none());
        assert!(cell.background.is_none());
        assert!(cell.border_fill_id.is_none());
        assert!(cell.margin.is_none());
        assert!(cell.vertical_align.is_none());
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
        let cell = simple_cell().with_background(Color::from_rgb(200, 200, 200));
        assert!(cell.background.is_some());
    }

    #[test]
    fn table_display() {
        let t = simple_table();
        assert_eq!(t.to_string(), "Table(2x2)");
    }

    #[test]
    fn single_cell_table() {
        let table = Table::new(vec![TableRow::with_height(
            vec![simple_cell()],
            HwpUnit::from_mm(10.0).unwrap(),
        )]);
        assert_eq!(table.row_count(), 1);
        assert_eq!(table.col_count(), 1);
    }

    #[test]
    fn row_with_fixed_height() {
        let row = TableRow::with_height(vec![simple_cell()], HwpUnit::from_mm(25.0).unwrap());
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
        let mut t = simple_table()
            .with_width(HwpUnit::from_mm(150.0).unwrap())
            .with_caption(crate::caption::Caption::default())
            .with_page_break(TablePageBreak::None)
            .with_repeat_header(false)
            .with_cell_spacing(HwpUnit::from_mm(2.0).unwrap())
            .with_border_fill_id(7);
        t.rows[0].height = Some(HwpUnit::from_mm(20.0).unwrap());
        t.rows[0].cells[0] = t.rows[0].cells[0]
            .clone()
            .with_background(Color::from_rgb(255, 0, 0))
            .with_height(HwpUnit::from_mm(8.0).unwrap())
            .with_border_fill_id(9)
            .with_margin(TableMargin {
                left: HwpUnit::from_mm(1.0).unwrap(),
                right: HwpUnit::from_mm(2.0).unwrap(),
                top: HwpUnit::from_mm(0.5).unwrap(),
                bottom: HwpUnit::from_mm(0.25).unwrap(),
            })
            .with_vertical_align(TableVerticalAlign::Bottom);

        let json = serde_json::to_string(&t).unwrap();
        let back: Table = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn serde_defaults_missing_new_fields() {
        let json = r#"{"rows":[],"width":null,"caption":null}"#;
        let back: Table = serde_json::from_str(json).unwrap();
        assert_eq!(back.page_break, TablePageBreak::Cell);
        assert!(back.repeat_header);
        assert!(back.cell_spacing.is_none());
        assert!(back.border_fill_id.is_none());
    }

    #[test]
    fn table_margin_defaults_to_zero() {
        let margin = TableMargin::default();
        assert_eq!(margin.left, HwpUnit::ZERO);
        assert_eq!(margin.right, HwpUnit::ZERO);
        assert_eq!(margin.top, HwpUnit::ZERO);
        assert_eq!(margin.bottom, HwpUnit::ZERO);
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
    fn row_new_sets_expected_defaults() {
        let cells = vec![simple_cell()];
        let row = TableRow::new(cells.clone());
        assert_eq!(row.cells, cells);
        assert!(row.height.is_none());
        assert!(!row.is_header);
    }
}
