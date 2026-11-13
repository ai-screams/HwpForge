//! Source coordinates are established before continuation cells are removed.
use super::*;
use crate::format::table::{self as properties, TableProperties, TableProperty};
use hwpforge_blueprint::border_fill::{Border, BorderFill, Fill};
use hwpforge_core::table::{
    grid::{TableGrid, MAX_GRID_POSITIONS},
    Table, TableCell, TableMargin, TableRow, TableVerticalAlign,
};
use hwpforge_foundation::{BorderLineType, HwpUnit};
mod widths;
pub(super) use widths::apply_pending;
mod borders;
const MAX_POSITIONS: usize = 4_194_304;
const MAX_TABLE_DEPTH: usize = hwpforge_core::MAX_NESTING_DEPTH;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Merge {
    #[default]
    None,
    Restart,
    Continue,
}
#[derive(Default)]
struct Cell {
    col: usize,
    span: u16,
    merge: Merge,
    active: bool,
    continuation: bool,
    content_minimum: i32,
    content_started: bool,
    content_after: i32,
    vertical_align: Option<TableVerticalAlign>,
    properties: TableProperties,
    paragraphs: Vec<Paragraph>,
    nested: Vec<(usize, WidthPlan)>,
    anchor_boundaries: Vec<usize>,
}
#[derive(Default)]
struct Row {
    height: Option<i32>,
    exact_at: Option<Location>,
    before: usize,
    after: usize,
    end: usize,
    cells: Vec<Cell>,
}
struct Source {
    at: Location,
    grid: Vec<Option<i32>>,
    grid_present: bool,
    properties: TableProperties,
    style: Option<StyleKey>,
    rows: Vec<Row>,
    cell: Option<Cell>,
    area: usize,
    columns: usize,
    saved: Saved,
}
struct Saved {
    paragraphs: Vec<Paragraph>,
    origins: Vec<Origin>,
    pending_shapes: Vec<(usize, Delta)>,
    pending_tables: Vec<(usize, WidthPlan)>,
    layout_known: bool,
}
pub(super) struct WidthPlan {
    at: Location,
    columns: Vec<Option<i32>>,
    width: Option<i32>,
    cells: Vec<CellWidth>,
}
struct CellWidth {
    row: usize,
    cell: usize,
    col: usize,
    span: usize,
    preferred: Option<i32>,
    nested: Vec<(usize, WidthPlan)>,
    anchor_boundaries: Vec<usize>,
}
#[derive(Default)]
pub(super) struct Tables {
    stack: Vec<Source>,
    pub(super) pending: Vec<(usize, WidthPlan)>,
    positions: usize,
}
impl Tables {
    pub(super) fn measures_paragraph(&self) -> bool {
        self.stack.last().and_then(|t| t.cell.as_ref()).is_some()
    }
    pub(super) fn paragraph_minimum(
        &mut self,
        height: i32,
        gap: i32,
        before: i32,
        after: i32,
        part: &str,
    ) -> Result<()> {
        if let Some(cell) = self.stack.last_mut().and_then(|t| t.cell.as_mut()) {
            // Height includes each source paragraph's forced lines in its first
            // page/column fragment and its first-line marker growth once.
            // Width-dependent wrapping is not counted (a line-count lower bound).
            // Before is source spacing, counted only for the first paragraph;
            // collapsed internal gaps supply the remaining source spacing.
            cell.content_minimum = cell
                .content_minimum
                .checked_add(height)
                .and_then(|h| h.checked_add(gap))
                .and_then(|h| h.checked_add(if cell.content_started { 0 } else { before }))
                .filter(|h| *h <= HwpUnit::MAX_VALUE)
                .ok_or_else(|| DocxError::invalid(part, "tc", "cell minimum height overflow"))?;
            cell.content_started = true;
            cell.content_after = after;
        }
        Ok(())
    }
    pub(super) fn inside(&self) -> bool {
        !self.stack.is_empty()
    }
    pub(super) fn discard(&self) -> bool {
        self.stack.last().and_then(|t| t.cell.as_ref()).is_some_and(|c| c.continuation)
    }
    pub(super) fn style(&self) -> Option<StyleKey> {
        self.stack.last().and_then(|t| t.style)
    }
}
impl Body<'_> {
    pub(super) fn begin_table(&mut self, frame: &mut Frame) -> Result<()> {
        if self.paragraph.is_some()
            || (self.tables.inside() && self.tables.stack.last().unwrap().cell.is_none())
        {
            return Err(DocxError::invalid(
                self.part,
                &frame.location,
                "table requires a block container",
            ));
        }
        admit_count(
            self.part,
            &frame.location,
            "table nesting",
            self.tables.stack.len(),
            MAX_TABLE_DEPTH,
        )?;
        self.end_spacing_container()?;
        if let Some(cell) = self.tables.stack.last_mut().and_then(|t| t.cell.as_mut()) {
            // A nested table ends the source-paragraph boundary. Omitting its height
            // and the preceding paragraph's after keeps the parent minimum below Word.
            cell.content_after = 0;
        }
        let (style, _) =
            self.resolver.as_ref().unwrap().table_properties(None, self.part, self.warnings)?;
        let properties = TableProperties::default();
        let saved = Saved {
            paragraphs: std::mem::take(&mut self.paragraphs),
            origins: std::mem::take(&mut self.origins),
            pending_shapes: std::mem::take(&mut self.pending_shapes),
            pending_tables: std::mem::take(&mut self.tables.pending),
            layout_known: self.section_layout_known,
        };
        self.section_layout_known = true;
        self.tables.stack.push(Source {
            at: frame.location.clone(),
            grid: Vec::new(),
            grid_present: false,
            properties,
            style,
            rows: Vec::new(),
            cell: None,
            area: 0,
            columns: 0,
            saved,
        });
        frame.node = Node::Table;
        Ok(())
    }
    fn end_spacing_container(&mut self) -> Result<()> {
        if let Some(pending) = self.trailing_break.take() {
            if pending.first {
                self.warn_split(&pending.split_at)?;
            }
        }
        self.finalize_shapes(0)?;
        self.spacing_neighbor = None;
        Ok(())
    }
    fn structural_parent(stack: &[Frame]) -> Option<&Node> {
        stack.iter().rev().find(|frame| !frame.passes_content).map(|frame| &frame.node)
    }
    pub(super) fn table_element(&mut self, e: &Element, frame: &mut Frame) -> Result<bool> {
        let Some(parent) = Self::structural_parent(&self.stack) else { return Ok(false) };
        // Content wrappers, range markers and unknown content share the body path.
        if matches!(parent, Node::Table) && !matches!(e.name.as_str(), "tblPr" | "tblGrid" | "tr")
            || matches!(parent, Node::TableRow) && !matches!(e.name.as_str(), "trPr" | "tc")
        {
            return Ok(false);
        }
        if !matches!(
            parent,
            Node::Table
                | Node::TableRow
                | Node::TableGrid
                | Node::TableProperties(_)
                | Node::CellProperties(_)
                | Node::RowProperties
        ) {
            return Ok(false);
        }
        let at = frame.location.to_string();
        if e.namespace_id != W_NS.0 {
            return Ok(false);
        }
        if !matches!(parent, Node::TableProperties(_) | Node::CellProperties(_))
            || matches!(e.name.as_str(), "gridSpan" | "vMerge" | "tblStyle")
        {
            properties::attributes(e, self.part, &at, self.warnings)?;
        }
        let source = self
            .tables
            .stack
            .last_mut()
            .ok_or_else(|| DocxError::invalid(self.part, &at, "missing table"))?;
        let mut next = Node::TableLeaf;
        match (parent, e.name.as_str()) {
            (Node::Table, "tblPr") => next = Node::TableProperties(TableProperty::Root),
            (Node::Table, "tblGrid") => {
                if source.grid_present || !source.rows.is_empty() {
                    return Err(DocxError::invalid(self.part, &at, "duplicate or late table grid"));
                }
                source.grid_present = true;
                next = Node::TableGrid;
            }
            (Node::TableGrid, "gridCol") => {
                count(
                    self.part,
                    &at,
                    "table columns",
                    source.grid.len() + 1,
                    MAX_GRID_POSITIONS as usize,
                )?;
                let width = attr(e, "w")
                    .map(|_| {
                        properties::length(
                            e,
                            "w",
                            crate::format::StructuralDimension::Width,
                            self.part,
                            &at,
                        )
                    })
                    .transpose()?;
                source.grid.push(width);
            }
            (Node::Table, "tr") => {
                count(
                    self.part,
                    &at,
                    "table rows",
                    source.rows.len() + 1,
                    MAX_GRID_POSITIONS as usize,
                )?;
                source.rows.push(Row::default());
                next = Node::TableRow;
                self.check_table_budget()?;
            }
            (Node::TableRow, "trPr") => next = Node::RowProperties,
            (Node::TableRow, "tc") => {
                if source.cell.is_some() {
                    return Err(DocxError::invalid(self.part, &at, "nested cell"));
                }
                let row = source.rows.last().unwrap();
                source.cell = Some(Cell { col: row.end, span: 1, ..Cell::default() });
                next = Node::TableCell;
            }
            (Node::RowProperties, "gridBefore" | "gridAfter") => {
                let n = integer(e, 0, self.part, &at)?;
                let row = source.rows.last_mut().unwrap();
                if !row.cells.is_empty() {
                    return Err(DocxError::invalid(self.part, &at, "late row grid properties"));
                }
                if e.name == "gridBefore" {
                    row.before = n;
                    row.end = n;
                } else {
                    row.after = n;
                }
                self.check_table_budget()?;
            }
            (Node::RowProperties, "trHeight") => {
                let row = source.rows.last_mut().unwrap();
                let height = attr(e, "val")
                    .map(|_| {
                        properties::length(
                            e,
                            "val",
                            crate::format::StructuralDimension::RowHeight,
                            self.part,
                            &at,
                        )
                    })
                    .transpose()?;
                let rule = attr(e, "hRule");
                // Auto ignores val for layout, but explicit dimensions still require validation.
                row.height = if rule == Some("auto") { None } else { height };
                row.exact_at = (rule == Some("exact") && height.is_some_and(|h| h > 0))
                    .then(|| frame.location.clone());
                if rule != Some("auto")
                    && row.exact_at.is_none()
                    && (row.height.is_none() || !matches!(rule, None | Some("atLeast")))
                {
                    self.warn(
                        Kind::ContentLost,
                        &frame.location,
                        || "Missing row height value or unsupported rule; using a minimum",
                        "row_height",
                        "use_default",
                    )?;
                }
            }
            (Node::TableProperties(TableProperty::Root), "tblStyle") => {
                let key = attr(e, "val")
                    .map(|v| {
                        self.resolver.as_ref().unwrap().definitions.style_key(v, self.part, &at)
                    })
                    .transpose()?;
                let (style, _) = self.resolver.as_ref().unwrap().table_properties(
                    key,
                    self.part,
                    self.warnings,
                )?;
                source.style = style;
            }
            (Node::TableProperties(state), _) => {
                next = Node::TableProperties(source.properties.read(
                    *state,
                    e,
                    self.part,
                    &at,
                    self.warnings,
                )?)
            }
            (Node::CellProperties(TableProperty::Root), "vAlign") => {
                source.cell.as_mut().unwrap().vertical_align = match attr(e, "val") {
                    Some("top") => Some(TableVerticalAlign::Top),
                    Some("center") => Some(TableVerticalAlign::Center),
                    Some("bottom") => Some(TableVerticalAlign::Bottom),
                    _ => {
                        record(
                            self.warnings,
                            Kind::UnsupportedValue,
                            "cell_vertical_alignment",
                            "use_default",
                            || {
                                (
                                    Some(self.part),
                                    Some(&frame.location),
                                    "Unsupported cell vertical alignment uses the Word top default",
                                )
                            },
                        )?;
                        None
                    }
                };
            }
            (Node::CellProperties(TableProperty::Root), "gridSpan") => {
                source.cell.as_mut().unwrap().span = integer(e, 1, self.part, &at)?
                    .try_into()
                    .map_err(|_| DocxError::invalid(self.part, &at, "column span exceeds u16"))?;
            }
            (Node::CellProperties(TableProperty::Root), "vMerge") => {
                source.cell.as_mut().unwrap().merge = match attr(e, "val") {
                    Some("restart") => Merge::Restart,
                    None | Some("continue") => Merge::Continue,
                    _ => {
                        return Err(DocxError::invalid(
                            self.part,
                            &at,
                            "unknown vertical merge value",
                        ))
                    }
                };
            }
            (Node::CellProperties(TableProperty::Borders), "insideH" | "insideV") => {
                self.warn(
                    Kind::TableBorderApproximated,
                    &frame.location,
                    || "Cell-internal borders have no face in the output cell and are omitted",
                    "cell_internal_border",
                    "omit",
                )?;
            }
            (Node::CellProperties(state), _) => {
                next = Node::CellProperties(source.cell.as_mut().unwrap().properties.read(
                    *state,
                    e,
                    self.part,
                    &at,
                    self.warnings,
                )?)
            }
            _ => {
                self.warn(
                    Kind::UnsupportedAttribute,
                    &frame.location,
                    || "Unsupported table structure property omitted",
                    "table_property",
                    "omit",
                )?;
            }
        }
        frame.node = next;
        frame.blocked = matches!(frame.node, Node::TableLeaf);
        Ok(true)
    }
    pub(super) fn begin_cell_properties(&mut self, frame: &mut Frame) -> Result<()> {
        if !matches!(Self::structural_parent(&self.stack), Some(Node::TableCell)) {
            return Err(DocxError::invalid(
                self.part,
                &frame.location,
                "cell properties require a cell",
            ));
        }
        frame.node = Node::CellProperties(TableProperty::Root);
        Ok(())
    }
    pub(super) fn prepare_cell_content(&mut self) -> Result<()> {
        let Some(source) = self.tables.stack.last_mut() else {
            return Ok(());
        };
        let Some(cell) = source.cell.as_mut() else {
            return Err(DocxError::invalid(self.part, &source.at, "table content requires a cell"));
        };
        cell.active = cell.merge == Merge::Restart;
        if cell.merge == Merge::Continue {
            let previous = source.rows.len().checked_sub(2).map(|r| &source.rows[r]);
            if let Some(previous) = previous {
                let first =
                    previous.cells.partition_point(|c| c.col + usize::from(c.span) <= cell.col);
                for prior in previous.cells[first..]
                    .iter()
                    .take_while(|p| p.col < cell.col + usize::from(cell.span))
                    .filter(|p| p.active)
                {
                    if prior.col != cell.col || prior.span != cell.span {
                        return Err(DocxError::invalid(
                            self.part,
                            &source.at,
                            "vertical merge interval differs from the preceding anchor",
                        ));
                    }
                    cell.continuation = true;
                    cell.active = true;
                }
            }
        }
        self.check_table_budget()
    }
    fn check_table_budget(&mut self) -> Result<()> {
        let source = self.tables.stack.last_mut().unwrap();
        let row = source.rows.last();
        let end =
            source.cell.as_ref().map_or(row.map_or(0, |r| r.end), |c| c.col + usize::from(c.span));
        let width = end
            .checked_add(row.map_or(0, |r| r.after))
            .ok_or_else(|| DocxError::invalid(self.part, &source.at, "grid width overflow"))?;
        if source.grid_present && width > source.grid.len() {
            return Err(DocxError::invalid(
                self.part,
                &source.at,
                "row exceeds declared table grid",
            ));
        }
        let width = width.max(source.grid.len()).max(source.columns);
        source.columns = width;
        let area = source
            .rows
            .len()
            .checked_mul(width.max(1))
            .ok_or_else(|| DocxError::invalid(self.part, &source.at, "grid area overflow"))?;
        let total =
            admit_positions(self.part, &source.at, self.tables.positions, source.area, area)?;
        self.tables.positions = total;
        source.area = area;
        Ok(())
    }
    pub(super) fn finish_cell(&mut self, at: &Location) -> Result<()> {
        self.prepare_cell_content()?;
        self.end_spacing_container()?;
        if self.paragraphs.is_empty() && !self.tables.discard() {
            self.empty_container_paragraph(at, true)?;
            self.end_spacing_container()?;
        }
        let source = self.tables.stack.last_mut().unwrap();
        let mut cell = source.cell.take().unwrap();
        // Word includes the final source paragraph's resolved after in cell height.
        // It is an exact spacing contribution, hence no greater than Word's; Hancom
        // omits it from automatic height, so retain it only in this minimum.
        cell.content_minimum = cell
            .content_minimum
            .checked_add(cell.content_after)
            .filter(|h| *h <= HwpUnit::MAX_VALUE)
            .ok_or_else(|| DocxError::invalid(self.part, at, "cell minimum height overflow"))?;
        cell.paragraphs = std::mem::take(&mut self.paragraphs);
        cell.nested = std::mem::take(&mut self.tables.pending);
        cell.anchor_boundaries = self
            .origins
            .iter()
            .enumerate()
            .filter_map(|(i, o)| o.anchor_boundary.then_some(i))
            .collect();
        self.origins.clear();
        let row = source.rows.last_mut().unwrap();
        row.end = cell.col + usize::from(cell.span);
        row.cells.push(cell);
        Ok(())
    }
    fn empty_container_paragraph(&mut self, at: &Location, measure_first: bool) -> Result<()> {
        self.output_paragraphs =
            admit_count(self.part, at, "paragraphs", self.output_paragraphs, MAX_PARAGRAPHS)?;
        self.source_paragraphs += 1;
        self.direct_para = Resolver::empty_properties();
        self.direct_run = Delta::default();
        self.paragraph_style = None;
        self.run_style = None;
        self.mark = None;
        self.mark_style = None;
        self.breaks.clear();
        self.diagnose_runs.clear();
        self.run_diagnosed = false;
        self.boundary_input = None;
        self.paragraph = Some(Paragraph::new(ParaShapeIndex::new(0)));
        self.ensure_fragment_run(at)?;
        self.finish_paragraph(measure_first)?;
        let (index, delta) = self.pending_shapes.pop().unwrap();
        self.paragraphs[index].para_shape_id =
            self.resolver.as_mut().unwrap().para_shape(&delta)?;
        self.paragraphs[index].style_ref = None;
        Ok(())
    }
    pub(super) fn finish_table(&mut self, at: &Location) -> Result<()> {
        let source = self.tables.stack.pop().unwrap();
        let Saved { paragraphs, origins, pending_shapes, pending_tables, layout_known } =
            source.saved;
        self.paragraphs = paragraphs;
        self.origins = origins;
        self.pending_shapes = pending_shapes;
        self.tables.pending = pending_tables;
        self.section_layout_known = layout_known;
        let (_, mut effective) = self.resolver.as_ref().unwrap().table_properties(
            source.style,
            self.part,
            self.warnings,
        )?;
        effective.overlay(&source.properties);
        let budget = AssemblyBudget::approve(
            &source.rows,
            source.columns,
            self.output_paragraphs,
            self.runs,
            self.part,
            at,
        )?;
        let (mut table, plan) = assemble(
            source.at,
            (source.grid, source.grid_present, &budget),
            effective,
            source.rows,
            &mut self.resolver.as_mut().unwrap().registry,
            self.part,
            self.warnings,
        )?;
        // A real empty paragraph supplies canonical styles to synthesized cells and table holders.
        self.empty_container_paragraph(at, false)?;
        let holder = self.paragraphs.last_mut().unwrap();
        let empty = holder.clone();
        for row in &mut table.rows {
            for cell in &mut row.cells {
                if cell.paragraphs.is_empty() {
                    cell.paragraphs.push(empty.clone());
                }
            }
        }
        budget.consume(&mut self.output_paragraphs, &mut self.runs);
        holder.runs[0].content = RunContent::Table(Box::new(table));
        holder.runs.push(empty.runs[0].clone());
        self.tables.pending.push((self.paragraphs.len() - 1, plan));
        self.spacing_neighbor = None;
        Ok(())
    }
}
fn admit_positions(
    part: &str,
    at: &Location,
    current: usize,
    previous_area: usize,
    area: usize,
) -> Result<usize> {
    count(part, at, "table positions", area, MAX_GRID_POSITIONS as usize)?;
    let total = current.saturating_add(area.saturating_sub(previous_area));
    count(part, at, "document table positions", total, MAX_POSITIONS)?;
    Ok(total)
}

fn count(
    part: &str,
    at: &impl fmt::Display,
    resource: &'static str,
    observed: usize,
    limit: usize,
) -> Result<()> {
    if observed > limit {
        return Err(DocxError::Limit {
            part: quote(part),
            element: quote(at),
            resource,
            observed,
            limit,
        });
    }
    Ok(())
}
fn integer(e: &Element, min: usize, part: &str, at: &str) -> Result<usize> {
    let n = attr(e, "val")
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n >= min)
        .ok_or_else(|| DocxError::invalid(part, at, "invalid grid count"))?;
    count(part, &at, "grid count", n, MAX_GRID_POSITIONS as usize)?;
    Ok(n)
}
/// Approval precedes cell expansion, width plans and border allocation. The holder
/// consumes a table run and an empty text run; each filler consumes one paragraph/run.
struct AssemblyBudget {
    width: usize,
    fillers: usize,
    paragraphs: usize,
    runs: usize,
}
impl AssemblyBudget {
    fn consume(self, paragraphs: &mut usize, runs: &mut usize) {
        *paragraphs = self.paragraphs;
        *runs = self.runs;
    }
    fn approve(
        rows: &[Row],
        width: usize,
        paragraphs: usize,
        runs: usize,
        part: &str,
        at: &Location,
    ) -> Result<Self> {
        let fillers = rows
            .iter()
            .map(|row| width - row.cells.iter().map(|c| usize::from(c.span)).sum::<usize>())
            .sum::<usize>();
        let growth = fillers.saturating_add(1);
        let paragraphs = paragraphs.saturating_add(growth);
        let runs = runs.saturating_add(growth).saturating_add(1);
        count(part, at, "paragraphs", paragraphs, MAX_PARAGRAPHS)?;
        count(part, at, "runs", runs, MAX_RUNS)?;
        Ok(Self { width, fillers, paragraphs, runs })
    }
}
fn assemble(
    at: Location,
    grid: (Vec<Option<i32>>, bool, &AssemblyBudget),
    properties: TableProperties,
    rows: Vec<Row>,
    registry: &mut hwpforge_blueprint::registry::StyleRegistry,
    part: &str,
    w: &mut Vec<DocxWarning>,
) -> Result<(Table, WidthPlan)> {
    let (mut columns, grid_present, budget) = grid;
    let width = budget.width;
    if width == 0 || rows.is_empty() {
        return Err(DocxError::invalid(part, &at, "table has no determinable grid"));
    }
    if !grid_present {
        record(w, Kind::TablePositionApproximated, "table_grid", "infer_grid", || {
            (Some(part), Some(&at), "Column grid inferred from source spans")
        })?;
        columns.resize(width, None);
    }
    let nrows = rows.len();
    // Size protection also clips automatic rows: enable it only when every row
    // has an exact value. Content minima omit width-dependent wrapping.
    let size_protect = rows.iter().all(|row| row.exact_at.is_some());
    for row in &rows {
        if let Some(exact_at) = &row.exact_at {
            record(w, Kind::ContentLost, "row_height", "use_default", || {
                (
                    Some(part),
                    Some(exact_at),
                    if size_protect {
                        "Exact row height reproduced by fixing the table size; Hancom table resizing and automatic row expansion are locked; overflowing text is clipped (as in Word)"
                    } else {
                        "Exact row height approximated as a minimum; Hancom may expand it to fit content"
                    },
                )
            })?;
        }
    }
    let border_heights = borders::row_heights(&rows, width, &properties);
    let mut output: Vec<TableRow> = Vec::with_capacity(nrows);
    let mut expected = Vec::new();
    let mut plans = Vec::new();
    let mut overrides: Vec<[Option<bool>; 6]> = Vec::new();
    let mut previous = vec![None::<(usize, usize, usize)>; width];
    let mut minima = Vec::new();
    let mut row_rules = Vec::new();
    for (ri, row) in rows.into_iter().enumerate() {
        let mut cell_minima = Vec::new();
        let mut current = vec![None; width];
        let mut cells = Vec::new();
        let mut cursor = 0;
        for mut source in row.cells {
            while cursor < source.col {
                push_filler(&mut cells, &mut expected, &mut plans, &mut overrides, ri, cursor);
                cell_minima.push(0);
                cursor += 1;
            }
            let span = usize::from(source.span);
            let end = source.col + span;
            if end > width {
                return Err(DocxError::invalid(part, &at, "cell outside source grid"));
            }
            if source.continuation {
                let (ar, ac, ai) = previous[source.col].ok_or_else(|| {
                    DocxError::invalid(part, &at, "missing preceding merge anchor")
                })?;
                if previous[source.col..end].iter().any(|p| *p != Some((ar, ac, ai))) {
                    return Err(DocxError::invalid(
                        part,
                        &at,
                        "conflicting vertical merge intervals",
                    ));
                }
                let anchor = &mut output[ar].cells[ac];
                anchor.row_span = anchor
                    .row_span
                    .checked_add(1)
                    .ok_or_else(|| DocxError::invalid(part, &at, "row span exceeds u16"))?;
                expected[ai].3 = anchor.row_span;
                // The final continuation owns the merged bottom, including inheritance.
                overrides[ai][3] = source.properties.borders[3];
                for (side, target) in overrides[ai][..2].iter_mut().enumerate() {
                    let inherited = properties.borders[if (side == 0 && source.col == 0)
                        || (side == 1 && end == width)
                    {
                        side
                    } else {
                        5
                    }]
                    .unwrap_or(false);
                    let previous = target.unwrap_or(inherited);
                    let next = source.properties.borders[side].unwrap_or(inherited);
                    if previous != next {
                        record(
                            w,
                            Kind::TableBorderApproximated,
                            "border_conflict",
                            "use_none",
                            || {
                                (
                                    Some(part),
                                    Some(&at),
                                    "Different row segments on a merged side use no border",
                                )
                            },
                        )?;
                    }
                    *target = Some(previous && next);
                }
                current[source.col..end].fill(Some((ar, ac, ai)));
            } else {
                if source.merge == Merge::Continue {
                    record(w, Kind::TableMergeApproximated, "vertical_merge", "keep_cell", || {
                        (Some(part), Some(&at), "Orphan continuation retained as an ordinary cell")
                    })?;
                }
                let ci = cells.len();
                let ai = expected.len();
                let mut cell = TableCell::with_span(
                    std::mem::take(&mut source.paragraphs),
                    HwpUnit::new(1).unwrap(),
                    source.span,
                    1,
                );
                let mut margins = properties.margins;
                for (a, b) in margins.iter_mut().zip(source.properties.margins) {
                    if b.is_some() {
                        *a = b;
                    }
                }
                let margins = margin(margins);
                let height = source
                    .content_minimum
                    .checked_add(margins.top.as_i32())
                    .and_then(|h| h.checked_add(margins.bottom.as_i32()))
                    .ok_or_else(|| DocxError::invalid(part, &at, "row height overflow"))?;
                cell_minima.push(height);
                cell.margin = Some(margins);
                cell.vertical_align =
                    Some(source.vertical_align.unwrap_or(TableVerticalAlign::Top));
                plans.push(CellWidth {
                    row: ri,
                    cell: ci,
                    col: source.col,
                    span,
                    preferred: source.properties.width.flatten(),
                    nested: source.nested,
                    anchor_boundaries: source.anchor_boundaries,
                });
                overrides.push(source.properties.borders);
                expected.push((ri, source.col, source.span, 1));
                cells.push(cell);
                if source.active {
                    current[source.col..end].fill(Some((ri, ci, ai)));
                }
            }
            cursor = end;
        }
        while cursor < width {
            push_filler(&mut cells, &mut expected, &mut plans, &mut overrides, ri, cursor);
            cell_minima.push(0);
            cursor += 1;
        }
        minima.push(cell_minima);
        row_rules.push(row.height);
        output.push(TableRow::new(cells));
        previous = current;
    }
    // Without size protection Hancom expands supplied minima for additional lines.
    // Merge spans are now final. A spanning anchor cannot impose its content's
    // forced lines on its starting row; retain them for the merged region below.
    for (((row, minima), explicit), border) in
        output.iter_mut().zip(&minima).zip(row_rules).zip(border_heights)
    {
        let minimum = row
            .cells
            .iter()
            .zip(minima)
            .filter(|(cell, _)| cell.row_span == 1)
            .map(|(_, h)| *h)
            .max()
            .unwrap_or(0);
        let height = if size_protect {
            i64::from(explicit.unwrap())
        } else {
            (i64::from(minimum) + border).max(i64::from(explicit.unwrap_or(0)))
        };
        let height = i32::try_from(height)
            .map_err(|_| DocxError::invalid(part, &at, "row height overflow"))?;
        row.height =
            Some(HwpUnit::new(height).map_err(|e| DocxError::invalid(part, &at, e.to_string()))?);
    }
    // The encoder passes row heights only to cells starting in that row. Preserve
    // otherwise unrepresented rows and spanning content in the merged minimum.
    let mut heights = vec![0_i64];
    let mut empty_rows = vec![0_usize];
    for row in &output {
        heights.push(heights.last().unwrap() + i64::from(row.height.unwrap().as_i32()));
        empty_rows.push(empty_rows.last().unwrap() + usize::from(row.cells.is_empty()));
    }
    for (ri, (row, minima)) in output.iter_mut().zip(minima).enumerate() {
        // An explicit cell height disables the encoder's fallback for the whole
        // starting row, so every other starting cell needs its own minimum too.
        let explicit_heights = row.cells.iter().zip(&minima).any(|(cell, content_minimum)| {
            let end = ri + usize::from(cell.row_span);
            empty_rows[end] > empty_rows[ri]
                || (!size_protect
                    && cell.row_span > 1
                    && i64::from(*content_minimum) > heights[end] - heights[ri])
        });
        if !explicit_heights {
            continue;
        }
        for (cell, content_minimum) in row.cells.iter_mut().zip(minima) {
            let end = ri + usize::from(cell.row_span);
            if cell.height.is_none() {
                let minimum = heights[end] - heights[ri];
                let minimum = if !size_protect && cell.row_span > 1 {
                    minimum.max(i64::from(content_minimum))
                } else {
                    minimum
                };
                let height =
                    i32::try_from(minimum).ok().and_then(|h| HwpUnit::new(h).ok()).ok_or_else(
                        || DocxError::invalid(part, &at, "merged minimum height overflow"),
                    )?;
                cell.height = Some(height);
            }
        }
    }
    if budget.fillers > 0 {
        record(w, Kind::TablePositionApproximated, "omitted_cells", "fill_empty", || {
            (Some(part), Some(&at), "Omitted source positions filled with empty cells")
        })?;
    }
    let mut table = Table::new(output);
    table.frame.size_protect = size_protect;
    table.in_margin = Some(margin(properties.margins));
    // Word inline tables have no outer margins, including tables inside cells.
    table.out_margin = Some(TableMargin {
        left: HwpUnit::ZERO,
        right: HwpUnit::ZERO,
        top: HwpUnit::ZERO,
        bottom: HwpUnit::ZERO,
    });
    let grid =
        TableGrid::from_table(&table).map_err(|e| DocxError::invalid(part, &at, e.to_string()))?;
    let actual: Vec<_> = grid
        .iter_anchors()
        .map(|c| (c.anchor.row as usize, c.anchor.col as usize, c.col_span, c.row_span))
        .collect();
    if actual != expected || grid.dimensions() != (nrows as u32, width as u32) {
        return Err(DocxError::invalid(
            part,
            &at,
            "Core grid differs from source cell coordinates and spans",
        ));
    }
    // Resolve adjacent faces before interning, with no output-format numbers here.
    borders::apply(&mut table, &grid, &overrides, properties.borders, registry, (part, &at), w)?;
    Ok((table, WidthPlan { at, columns, width: properties.width.flatten(), cells: plans }))
}
fn push_filler(
    cells: &mut Vec<TableCell>,
    expected: &mut Vec<(usize, usize, u16, u16)>,
    plans: &mut Vec<CellWidth>,
    overrides: &mut Vec<[Option<bool>; 6]>,
    row: usize,
    col: usize,
) {
    plans.push(CellWidth {
        row,
        cell: cells.len(),
        col,
        span: 1,
        preferred: None,
        nested: Vec::new(),
        anchor_boundaries: Vec::new(),
    });
    let mut filler = TableCell::new(Vec::new(), HwpUnit::new(1).unwrap());
    filler.vertical_align = Some(TableVerticalAlign::Top);
    cells.push(filler);
    expected.push((row, col, 1, 1));
    overrides.push([None; 6]);
}
/// Word 가 기본 표 스타일에 저장하는 셀 여백 (twip): left, right, top, bottom.
const WORD_DEFAULT_CELL_MARGINS: [i32; 4] = [108, 108, 0, 0];
fn margin(values: [Option<i32>; 4]) -> TableMargin {
    let [left, right, top, bottom] = std::array::from_fn(|i| {
        HwpUnit::new(values[i].unwrap_or(WORD_DEFAULT_CELL_MARGINS[i] * 5)).unwrap()
    });
    TableMargin { left, right, top, bottom }
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    // 이것을 실패시키는 것: 채움/보유 문단 또는 run 예산 누락, span을 셀 개수로 세거나 승인 합계 재계산.
    #[test]
    fn assembly_budget_includes_holder_before_expansion() {
        // 이것을 실패시키는 것: 끝 anchor의 빈 text run을 사전 예산에서 제외.
        let at = Location { name: Rc::from("tbl"), ordinal: 1 };
        let rows = vec![Row {
            end: 3,
            cells: vec![Cell { col: 1, span: 2, ..Cell::default() }],
            ..Row::default()
        }];
        // Two fillers + one holder with two runs; existing source paragraphs/runs already counted.
        let budget = AssemblyBudget::approve(&rows, 4, 999_997, 3_999_996, "body", &at).unwrap();
        assert_eq!(
            (budget.width, budget.fillers, budget.paragraphs, budget.runs),
            (4, 2, 1_000_000, 4_000_000)
        );
        assert!(matches!(
            AssemblyBudget::approve(&rows, 4, 999_998, 0, "body", &at),
            Err(DocxError::Limit { resource: "paragraphs", observed: 1_000_001, .. })
        ));
        assert!(matches!(
            AssemblyBudget::approve(&rows, 4, 0, 3_999_997, "body", &at),
            Err(DocxError::Limit { resource: "runs", observed: 4_000_001, .. })
        ));
        let (table, _) = assemble(
            at,
            (vec![], false, &budget),
            TableProperties::default(),
            rows,
            &mut hwpforge_blueprint::registry::StyleRegistry::with_fonts(vec![]),
            "body",
            &mut vec![],
        )
        .unwrap();
        let (mut paragraphs, mut runs) = (999_998, 3_999_998);
        budget.consume(&mut paragraphs, &mut runs);
        assert_eq!((paragraphs, runs), (1_000_000, 4_000_000));
        assert_eq!(table.rows[0].cells.len(), 3);
        assert_eq!(table.rows[0].cells.iter().map(|c| c.col_span).collect::<Vec<_>>(), [1, 2, 1]);
    }
    // 이것을 실패시키는 것: 문서 합계/표별 예산 상한을 늘리거나 기존 면적을 중복 계산.
    #[test]
    fn position_budget_is_checked_without_materializing_cells() {
        let at = Location { name: Rc::from("tbl"), ordinal: 1 };
        assert_eq!(admit_positions("body", &at, 4_194_303, 0, 1).unwrap(), 4_194_304);
        assert!(admit_positions("body", &at, 4_194_304, 0, 1).is_err());
        assert_eq!(
            admit_positions("body", &at, 1_048_576, 1_048_576, 1_048_576).unwrap(),
            1_048_576
        );
        assert!(admit_positions("body", &at, 0, 0, 1_048_577).is_err());
        assert!(admit_positions("body", &at, usize::MAX, 0, 1).is_err());
    }
}
