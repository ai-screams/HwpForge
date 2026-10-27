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

pub mod grid;

use hwpforge_foundation::{Color, HwpUnit};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::caption::Caption;
use crate::object_id::ObjectId;
use crate::paragraph::Paragraph;

/// Counts the leading nonempty physical rows whose effective cell markers are
/// all true. Empty rows end the block even when covered by a rowspan; spans do
/// not promote rows. Importers can adapt their source cells to this same rule.
/// This count is independent of the table's repeat-header switch.
///
/// 이것을 실패시키는 것: 첫 false/빈 행 건너뛰기 · all 대신 any 사용
/// (`cell_header_effective_values_survive_derivation`).
pub fn header_row_count(rows: impl IntoIterator<Item = impl IntoIterator<Item = bool>>) -> usize {
    let mut count = 0;
    for row in rows {
        let mut cells = row.into_iter();
        if cells.next() != Some(true) || !cells.all(|header| header) {
            break;
        }
        count += 1;
    }
    count
}

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

/// How the text of a table cell breaks into lines (`<hp:subList lineWrap>`,
/// HWP5 cell property bits 3–4).
///
/// # Examples
///
/// ```
/// use hwpforge_core::table::{CellLineWrap, TableCell};
/// use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
/// use hwpforge_core::paragraph::Paragraph;
///
/// let cell = TableCell::new(vec![Paragraph::new(ParaShapeIndex::new(0))], HwpUnit::ZERO)
///     .with_line_wrap(CellLineWrap::Squeeze);
/// assert_eq!(cell.line_wrap, CellLineWrap::Squeeze);
/// assert_eq!(CellLineWrap::default(), CellLineWrap::Break);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Default)]
#[non_exhaustive]
pub enum CellLineWrap {
    /// Lines break at the cell width (Hancom's default).
    #[default]
    #[serde(rename = "BREAK")]
    Break,
    /// A line that would break is squeezed into the cell width instead.
    #[serde(rename = "SQUEEZE")]
    Squeeze,
    /// The line is kept as it is (Hancom's `KEEP`).
    #[serde(rename = "KEEP")]
    Keep,
}

fn is_break(wrap: &CellLineWrap) -> bool {
    *wrap == CellLineWrap::Break
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

/// Decode-only layout cache for a table — Hancom-saved wire values that are
/// replayed or used as checksums, never re-typeset.
///
/// Encoders MUST NOT emit these values back to the wire (the HWPX encoder
/// keeps its own `sz` policy). Edit surfaces drop this cache together with
/// paragraph caches via [`crate::document::Document::strip_layout_caches`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct TableLayoutCache {
    /// Hancom-saved `<hp:sz height>` ([`None`] when the wire stored `0` or
    /// omitted it — natively authored split tables do).
    ///
    /// ⚠️ For a page-spanning (split) table this is the **first fragment's
    /// height**, not the total table height (W0 measurement).
    pub saved_sz_height: Option<HwpUnit>,
    /// Whether the wire `<hp:pos>` was the default inline-flow combination
    /// (`treatAsChar=0`, `flowWithText=1`, PARA/COLUMN relative, zero
    /// offsets). Renderers only support default-flow tables — a `false`
    /// here must fail closed (verified profile admission).
    pub default_flow_pos: bool,
}

impl TableLayoutCache {
    /// Creates a table layout cache from decoded wire facts.
    #[must_use]
    pub fn new(saved_sz_height: Option<HwpUnit>, default_flow_pos: bool) -> Self {
        Self { saved_sz_height, default_flow_pos }
    }
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
    /// Whether the leading header-row block (consecutive rows marked
    /// [`TableRow::is_header`]) repeats across page breaks.
    #[serde(default = "default_repeat_header")]
    pub repeat_header: bool,
    /// Optional explicit spacing between table cells.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_spacing: Option<HwpUnit>,
    /// Optional table-level border/fill reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_fill_id: Option<u32>,
    /// Wave 12p Step 2b: instance ID for cross-ref target lookup. HWP5
    /// 변환 시 Table CtrlHeader trailer 의 instance ID 가 채워지고,
    /// HWPX encoder 가 `<hp:tbl id="...">` attribute 로 emit. `None`
    /// 이면 encoder 가 fallback 값 (예: sequential counter) 사용 허용.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inst_id: Option<ObjectId>,
    /// Outer margins around the table (`<hp:outMargin>`) — the wire source
    /// for table placement (PDF replay rule R5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_margin: Option<TableMargin>,
    /// Table-level default cell padding (`<hp:inMargin>`) — the fallback when
    /// a cell has no explicit [`TableCell::margin`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_margin: Option<TableMargin>,
    /// Decode-only layout cache (saved wire geometry) — see
    /// [`TableLayoutCache`]. Encoders must not emit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_cache: Option<TableLayoutCache>,
    /// Where the table sits and how text wraps it (HWPX `<hp:pos>` plus the
    /// table's `textWrap`/`textFlow`).
    ///
    /// `None` is the placement encoders have always written for a table:
    /// not treated as a character, flowing with the text, no overlap,
    /// anchored to the paragraph vertically and the column horizontally,
    /// zero offsets, aligned top/left, not affecting line spacing, anchor not
    /// held, top-and-bottom wrap, both-sides flow. (A shape's `None`, by
    /// contrast, means an inline object.) Decoders write `None` only when
    /// every one of those matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<crate::placement::ObjectPlacement>,
    /// The table's size lock (HWPX `<hp:sz protect>`, HWP 5.0 object common
    /// property bit 20). A table has no original box: its
    /// [`original_size`](crate::frame::ObjectFrame::original_size) must stay
    /// `None` (`validate` rejects one).
    #[serde(default, skip_serializing_if = "crate::frame::ObjectFrame::is_unset")]
    pub frame: crate::frame::ObjectFrame,
    /// Whether the cell sizes are fixed (HWPX `<hp:tbl noAdjust>`, HWP 5.0
    /// table property bit 3): Hancom keeps the cell sizes when the table's
    /// size changes. `false` is the default and is not written to JSON.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_adjust: bool,
    /// Cell zones (HWPX `<hp:cellzoneList>`, HWP 5.0 table "valid zone
    /// info"): rectangles of cells drawn with one border/fill. Empty for most
    /// tables and left out of JSON when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cell_zones: Vec<CellZone>,
}

/// A rectangle of table cells drawn with one border/fill (HWPX
/// `<hp:cellzone>`): grid rows `start_row..=end_row`, columns
/// `start_col..=end_col`.
///
/// `validate` refuses a zone whose start is past its end or whose end is
/// outside the table.
///
/// # Examples
///
/// ```
/// use hwpforge_core::table::CellZone;
///
/// let zone = CellZone::new(0, 0, 1, 2, 5);
/// assert_eq!((zone.end_row, zone.end_col, zone.border_fill_id), (1, 2, 5));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct CellZone {
    /// First grid row of the zone.
    pub start_row: u16,
    /// First grid column of the zone.
    pub start_col: u16,
    /// Last grid row of the zone (inclusive).
    pub end_row: u16,
    /// Last grid column of the zone (inclusive).
    pub end_col: u16,
    /// The border/fill the zone is drawn with (`borderFillIDRef`).
    pub border_fill_id: u32,
}

impl CellZone {
    /// Creates a zone over rows `start_row..=end_row` and columns
    /// `start_col..=end_col`.
    #[must_use]
    pub fn new(
        start_row: u16,
        start_col: u16,
        end_row: u16,
        end_col: u16,
        border_fill_id: u32,
    ) -> Self {
        Self { start_row, start_col, end_row, end_col, border_fill_id }
    }
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
            inst_id: None,
            out_margin: None,
            in_margin: None,
            layout_cache: None,
            placement: None,
            frame: crate::frame::ObjectFrame::new(),
            no_adjust: false,
            cell_zones: Vec::new(),
        }
    }

    /// 표의 모든 셀 문단과 캡션 문단을 재귀 방문한다 (중첩 표 포함).
    #[cfg(test)]
    pub(crate) fn walk_paragraphs_mut(
        &mut self,
        f: &mut dyn FnMut(&mut crate::paragraph::Paragraph),
    ) {
        crate::walk::paragraphs_mut(crate::walk::NodeMut::Table(self), f);
    }

    /// `walk_paragraphs_mut` 의 불변 쌍둥이 — 방문 순서 동일
    /// (행 → 셀 → 셀 문단, 그다음 캡션).
    pub(crate) fn walk_paragraphs(&self, f: &mut dyn FnMut(&crate::paragraph::Paragraph)) {
        crate::walk::paragraphs(crate::walk::Node::Table(self), f);
    }

    /// Sets where the table sits (see [`Table::placement`]).
    #[must_use]
    pub fn with_placement(mut self, placement: crate::placement::ObjectPlacement) -> Self {
        self.placement = Some(placement);
        self
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

    /// Sets the cell zones (see [`Table::cell_zones`]).
    #[must_use]
    pub fn with_cell_zones(mut self, cell_zones: Vec<CellZone>) -> Self {
        self.cell_zones = cell_zones;
        self
    }

    /// Sets whether the cell sizes are fixed (see [`Table::no_adjust`]).
    #[must_use]
    pub fn with_no_adjust(mut self, no_adjust: bool) -> Self {
        self.no_adjust = no_adjust;
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

    /// Sets the outer margins around the table.
    #[must_use]
    pub fn with_out_margin(mut self, out_margin: TableMargin) -> Self {
        self.out_margin = Some(out_margin);
        self
    }

    /// Sets the table-level default cell padding.
    #[must_use]
    pub fn with_in_margin(mut self, in_margin: TableMargin) -> Self {
        self.in_margin = Some(in_margin);
        self
    }

    /// Attaches the decode-only layout cache.
    #[must_use]
    pub fn with_layout_cache(mut self, layout_cache: TableLayoutCache) -> Self {
        self.layout_cache = Some(layout_cache);
        self
    }

    /// Returns the leading header-row count derived from effective cell values.
    /// See [`header_row_count`]; `repeat_header` does not affect this count.
    pub fn header_row_count(&self) -> usize {
        TableRow::header_row_count(&self.rows)
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
    /// A compatibility fallback for cells with `header: None`, and otherwise a
    /// derived convenience flag. Importers and `Document::validate()` refresh
    /// it with [`TableRow::derive_header_rows`], preserving effective cell values.
    /// Consumers use [`Table::header_row_count`] rather than trusting this flag.
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

    /// Counts header rows using the shared physical-row rule.
    pub fn header_row_count(rows: &[Self]) -> usize {
        header_row_count(
            rows.iter()
                .map(|row| row.cells.iter().map(|cell| cell.effective_header(row.is_header))),
        )
    }

    /// Refreshes the derived leading header flags without changing any cell's
    /// effective value. Before changing a row flag, inherited (`None`) markers
    /// in that row are materialized as `Some(old_flag)`. Unchanged rows retain
    /// `None`, preserving legacy generation and lossless Markdown inheritance.
    /// Call after editing cell markers; document validation also does this.
    ///
    /// 이것을 실패시키는 것: 구체화 생략 · 새 행 값으로 구체화 · Some 덮기
    /// (`cell_header_effective_values_survive_derivation`).
    pub fn derive_header_rows(rows: &mut [Self]) {
        let count = Self::header_row_count(rows);
        for (index, row) in rows.iter_mut().enumerate() {
            let derived = index < count;
            if row.is_header != derived {
                for cell in &mut row.cells {
                    cell.header.get_or_insert(row.is_header);
                }
                row.is_header = derived;
            }
        }
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
    /// Cell-local title marker. `Some` preserves the source value; `None`
    /// inherits [`TableRow::is_header`] for legacy JSON and generated cells.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<bool>,
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
    /// How the cell's text breaks into lines; omitted from JSON when it is
    /// [`CellLineWrap::Break`].
    #[serde(default, skip_serializing_if = "is_break")]
    pub line_wrap: CellLineWrap,
    /// The cell's own margin values while it uses the table's padding
    /// (`margin` is `None`, HWPX `hasMargin="0"`): not used for layout, but
    /// kept, as Hancom keeps them, so turning the cell's own margin back on
    /// finds them again. `None` = the table's padding, what the encoder
    /// writes then; ignored when `margin` is `Some`. JSON omits it when
    /// `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dormant_margin: Option<TableMargin>,
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
            header: None,
            paragraphs,
            col_span: 1,
            row_span: 1,
            width,
            height: None,
            background: None,
            border_fill_id: None,
            margin: None,
            vertical_align: None,
            line_wrap: CellLineWrap::Break,
            dormant_margin: None,
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
            header: None,
            paragraphs,
            col_span,
            row_span,
            width,
            height: None,
            background: None,
            border_fill_id: None,
            margin: None,
            vertical_align: None,
            line_wrap: CellLineWrap::Break,
            dormant_margin: None,
        }
    }

    /// Resolves the title marker, giving an explicit cell value precedence.
    #[must_use]
    pub fn effective_header(&self, row_is_header: bool) -> bool {
        self.header.unwrap_or(row_is_header)
    }

    /// Sets a cell-local title marker, or restores row inheritance with `None`.
    #[must_use]
    pub fn with_header(mut self, header: Option<bool>) -> Self {
        self.header = header;
        self
    }

    /// Sets how the cell's text breaks into lines.
    #[must_use]
    pub fn with_line_wrap(mut self, line_wrap: CellLineWrap) -> Self {
        self.line_wrap = line_wrap;
        self
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

    /// S19: no dormant margin writes no `dormant_margin`; one round trips.
    ///
    /// 이것을 실패시키는 것: `skip_serializing_if` 삭제.
    #[test]
    fn dormant_margin_json() {
        let cell = TableCell::new(vec![simple_paragraph()], HwpUnit::new(1000).unwrap());
        let json = serde_json::to_value(&cell).unwrap();
        assert!(json.get("dormant_margin").is_none(), "{json}");
        let mut with = cell;
        let u = HwpUnit::new(141).unwrap();
        with.dormant_margin = Some(TableMargin { left: u, right: u, top: u, bottom: u });
        let json = serde_json::to_value(&with).unwrap();
        assert_eq!(json["dormant_margin"]["left"], 141);
        assert_eq!(serde_json::from_value::<TableCell>(json).unwrap(), with);
    }

    /// F: `cell_zones` is left out of JSON when empty, read as empty when
    /// missing, and kept field by field.
    ///
    /// 이것을 실패시키는 것: `serde(default)` 삭제 · 빈 목록 생략 삭제 · 칸
    /// 이름 바꾸기.
    #[test]
    fn table_cell_zones_json() {
        let table = Table::new(vec![]);
        let json = serde_json::to_value(&table).unwrap();
        assert!(json.get("cell_zones").is_none(), "{json}");
        assert!(serde_json::from_value::<Table>(json).unwrap().cell_zones.is_empty());
        let zoned = table.with_cell_zones(vec![CellZone::new(12, 0, 31, 1, 7)]);
        let json = serde_json::to_value(&zoned).unwrap();
        assert_eq!(
            json["cell_zones"],
            serde_json::json!([{"start_row": 12, "start_col": 0, "end_row": 31, "end_col": 1, "border_fill_id": 7}])
        );
        assert_eq!(serde_json::from_value::<Table>(json).unwrap().cell_zones, zoned.cell_zones);
    }

    /// E: `no_adjust` is left out of JSON when false, read as false when
    /// missing (JSON written before the field), and kept when true.
    ///
    /// 이것을 실패시키는 것: `serde(default)` 삭제(옛 JSON 실패) ·
    /// `skip_serializing_if` 삭제(기본 표 JSON 이 바뀜) · 직렬화에서 값 버리기.
    #[test]
    fn table_no_adjust_json() {
        let table = Table::new(vec![]);
        let json = serde_json::to_value(&table).unwrap();
        assert!(json.get("no_adjust").is_none(), "{json}");
        let back: Table = serde_json::from_value(json).unwrap();
        assert!(!back.no_adjust);
        let fixed = table.with_no_adjust(true);
        let json = serde_json::to_value(&fixed).unwrap();
        assert_eq!(json["no_adjust"], true);
        assert!(serde_json::from_value::<Table>(json).unwrap().no_adjust);
    }

    /// A breaking cell's JSON has no `line_wrap` (old JSON and
    /// output unchanged), a JSON without it reads as `Break`, and a squeezed
    /// cell writes and reads `"SQUEEZE"`.
    ///
    /// 이것을 실패시키는 것: `skip_serializing_if` 삭제 / serde 이름 바꾸기.
    #[test]
    fn cell_line_wrap_json() {
        let cell = TableCell::new(vec![simple_paragraph()], HwpUnit::new(1000).unwrap());
        let json = serde_json::to_value(&cell).unwrap();
        assert!(json.get("line_wrap").is_none(), "{json}");
        let back: TableCell = serde_json::from_value(json).unwrap();
        assert_eq!(back.line_wrap, CellLineWrap::Break);
        let squeezed = cell.with_line_wrap(CellLineWrap::Squeeze);
        let json = serde_json::to_value(&squeezed).unwrap();
        assert_eq!(json["line_wrap"], "SQUEEZE");
        assert_eq!(serde_json::from_value::<TableCell>(json).unwrap(), squeezed);
    }

    /// 이것을 실패시키는 것: Some(false)를 행 true로 덮기 / None 기본값 변경 /
    /// JSON의 None 생략 삭제 / 빌더가 값을 버리기 / with_span 기본값 변경.
    #[test]
    fn cell_header_json_and_precedence() {
        let cell = simple_cell();
        assert_eq!(cell.header, None);
        let json = serde_json::to_value(&cell).unwrap();
        assert!(json.get("header").is_none());
        assert_eq!(serde_json::from_value::<TableCell>(json.clone()).unwrap().header, None);
        let mut explicit_null = json;
        explicit_null["header"] = serde_json::Value::Null;
        assert_eq!(serde_json::from_value::<TableCell>(explicit_null).unwrap().header, None);
        assert_eq!(TableCell::with_span(vec![simple_paragraph()], cell.width, 2, 1).header, None);
        for marker in [None, Some(false), Some(true)] {
            let cell = cell.clone().with_header(marker);
            assert_eq!(cell.header, marker);
            for row in [false, true] {
                assert_eq!(cell.effective_header(row), marker.unwrap_or(row));
            }
            let json = serde_json::to_value(&cell).unwrap();
            if let Some(value) = marker {
                assert_eq!(json["header"], value);
            }
            assert_eq!(serde_json::from_value::<TableCell>(json).unwrap(), cell);
        }
    }

    /// 이것을 실패시키는 것: all→any / 빈 행 허용 / 선두 제한 제거 /
    /// 기존 행 값 fallback 제거 / 파생하면서 원본 칸 값을 바꾸기.
    #[test]
    fn cell_header_derivation_is_leading_and_non_destructive() {
        let row = |markers: &[Option<bool>], flag| {
            TableRow::new(markers.iter().map(|v| simple_cell().with_header(*v)).collect())
                .with_header(flag)
        };
        for stop in
            [vec![], vec![Some(false), Some(false)], vec![Some(true), Some(false)], vec![None]]
        {
            let mut rows = vec![
                row(&[None, Some(true)], true),
                row(&[Some(true), Some(true)], false),
                row(&stop, false),
                row(&[Some(true), Some(true)], false),
            ];
            let original: Vec<_> = rows.iter().map(|r| r.cells.clone()).collect();
            TableRow::derive_header_rows(&mut rows);
            assert_eq!(
                rows.iter().map(|r| r.is_header).collect::<Vec<_>>(),
                [true, true, false, false]
            );
            assert_eq!(rows.iter().map(|r| r.cells.clone()).collect::<Vec<_>>(), original);
            let once = rows.clone();
            TableRow::derive_header_rows(&mut rows);
            assert_eq!(rows, once);
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(512))]
        /// 이것을 실패시키는 것: None 구체화 생략/반전 · 명시값 덮기 ·
        /// 선두/빈 행/all 규칙 변경 · rowspan으로 제목행을 늘리기.
        #[test]
        fn cell_header_effective_values_survive_derivation(
            input in proptest::collection::vec(
                (proptest::bool::ANY, proptest::collection::vec(
                    (proptest::option::of(proptest::bool::ANY), 1u16..5), 0..6)), 0..8)
        ) {
            let mut rows: Vec<_> = input.iter().map(|(flag, cells)| {
                TableRow::new(cells.iter().map(|(marker, span)| {
                    let mut cell = simple_cell().with_header(*marker);
                    cell.row_span = *span;
                    cell
                }).collect()).with_header(*flag)
            }).collect();
            let values = |rows: &[TableRow]| rows.iter().map(|r| {
                r.cells.iter().map(|c| c.effective_header(r.is_header)).collect::<Vec<_>>()
            }).collect::<Vec<_>>();
            let before = values(&rows);
            // Independent oracle: first row containing no cells or a false cell.
            let expected = before.iter().position(|r| r.is_empty() || r.contains(&false))
                .unwrap_or(before.len());
            proptest::prop_assert_eq!(TableRow::header_row_count(&rows), expected);
            TableRow::derive_header_rows(&mut rows);
            proptest::prop_assert_eq!(values(&rows), before);
            for (index, row) in rows.iter().enumerate() {
                proptest::prop_assert_eq!(row.is_header, index < expected);
                for (cell, (marker, span)) in row.cells.iter().zip(&input[index].1) {
                    proptest::prop_assert_eq!(cell.row_span, *span);
                    if marker.is_some() || input[index].0 == row.is_header {
                        proptest::prop_assert_eq!(cell.header, *marker);
                    } else {
                        proptest::prop_assert_eq!(cell.header, Some(input[index].0));
                    }
                }
            }
            let once = rows.clone();
            TableRow::derive_header_rows(&mut rows);
            proptest::prop_assert_eq!(rows, once);
        }
    }

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
