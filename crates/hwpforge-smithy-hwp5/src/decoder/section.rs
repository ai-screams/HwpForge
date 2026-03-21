//! `BodyText/Section{N}` stream decoder for HWP5.
//!
//! Reads binary paragraph and run records from each section stream,
//! producing an intermediate representation that the projection layer
//! converts into Core's `Section` and `Paragraph` types.

use std::io::Cursor;

use crate::decoder::Hwp5Warning;
use crate::error::Hwp5Result;
use crate::schema::header::HwpVersion;
use crate::schema::record::{Record, TagId};
use crate::schema::section::{
    Hwp5CharShapeRun, Hwp5PageDef, Hwp5ParaHeader, Hwp5ParaText, Hwp5ShapeComponentGeometry,
    Hwp5ShapeComponentLine, Hwp5ShapeComponentOle, Hwp5ShapeComponentPolygon, Hwp5ShapePicture,
    Hwp5ShapePoint, TextSegment,
};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A decoded paragraph from a BodyText section.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct Hwp5Paragraph {
    /// The paragraph's text content (all Text segments concatenated, with
    /// tab/space/newline substituted for control codes).
    pub text: String,
    /// Paragraph shape ID (index into DocInfo para_shapes).
    pub para_shape_id: u16,
    /// Style ID (index into DocInfo styles).
    pub style_id: u8,
    /// Character shape runs: (position, char_shape_id) pairs.
    pub char_shape_runs: Vec<Hwp5CharShapeRun>,
    /// Inline control objects found in this paragraph (table refs, footnote refs, etc.).
    pub controls: Vec<Hwp5Control>,
}

/// A control object reference found inline in paragraph text.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum Hwp5Control {
    /// Table reference with nested cell paragraphs parsed from child records.
    Table(Hwp5Table),
    /// Image evidence resolved from `gso ` + `ShapeComponent` + `ShapePicture`.
    Image(Hwp5ImageControl),
    /// Line evidence resolved from `gso ` + `ShapeComponent` + `ShapeComponentLine`.
    Line(Hwp5LineControl),
    /// Pure rectangle evidence resolved from `gso ` + `ShapeComponent` + `ShapeComponentRect`.
    Rect(Hwp5RectControl),
    /// Polygon evidence resolved from `gso ` + `ShapeComponent` + `ShapeComponentPolygon`.
    Polygon(Hwp5PolygonControl),
    /// Header control with nested subtree paragraphs.
    Header(Hwp5NestedSubtree),
    /// Footer control with nested subtree paragraphs.
    Footer(Hwp5NestedSubtree),
    /// Textbox-like shape with nested subtree paragraphs.
    TextBox(Hwp5TextBoxControl),
    /// Embedded OLE object evidence resolved from `gso ` + `ShapeComponent` + `ShapeComponentOle`.
    OleObject(Hwp5OleObjectControl),
    /// Generic/unsupported control — preserve the ctrl_id for future expansion.
    Unknown {
        /// Four-byte control ID (big-endian ASCII, e.g. 0x74626C20 = 'tbl ').
        ctrl_id: u32,
    },
}

/// Parsed image evidence from a `gso ` scope.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5ImageControl {
    /// Owning control identifier, currently always `gso `.
    pub ctrl_id: u32,
    /// Minimal recovered geometry.
    pub geometry: Hwp5ShapeComponentGeometry,
    /// `DocInfo/BinData` item identifier referenced by `ShapePicture`.
    pub binary_data_id: u16,
}

/// Parsed line evidence from a `gso ` scope.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5LineControl {
    /// Owning control identifier, currently always `gso `.
    #[allow(dead_code)] // reserved for semantic/control-audit slices
    pub ctrl_id: u32,
    /// Minimal recovered geometry.
    pub geometry: Hwp5ShapeComponentGeometry,
    /// Line start point in local object coordinates.
    pub start: Hwp5ShapePoint,
    /// Line end point in local object coordinates.
    pub end: Hwp5ShapePoint,
}

/// Parsed pure rectangle evidence from a `gso ` scope.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5RectControl {
    /// Owning control identifier, currently always `gso `.
    #[allow(dead_code)] // reserved for semantic/control-audit slices
    pub ctrl_id: u32,
    /// Minimal recovered geometry.
    pub geometry: Hwp5ShapeComponentGeometry,
}

/// Parsed polygon evidence from a `gso ` scope.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5PolygonControl {
    /// Owning control identifier, currently always `gso `.
    #[allow(dead_code)] // reserved for semantic/control-audit slices
    pub ctrl_id: u32,
    /// Minimal recovered geometry.
    pub geometry: Hwp5ShapeComponentGeometry,
    /// Ordered polygon vertices in local object coordinates.
    pub points: Vec<Hwp5ShapePoint>,
}

/// Parsed textbox evidence from a `gso ` scope carrying `drawText/subList`.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5TextBoxControl {
    /// Owning control identifier, currently always `gso `.
    pub ctrl_id: u32,
    /// Minimal recovered geometry from the owning `CtrlHeader`.
    pub geometry: Hwp5ShapeComponentGeometry,
    /// Nested paragraphs captured from the textbox subtree.
    pub paragraphs: Vec<Hwp5Paragraph>,
}

/// Parsed OLE-backed object evidence from a `gso ` scope.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5OleObjectControl {
    /// Owning control identifier, currently always `gso `.
    pub ctrl_id: u32,
    /// Minimal recovered geometry.
    pub geometry: Hwp5ShapeComponentGeometry,
    /// `DocInfo/BinData` item identifier referenced by `ShapeComponentOle`.
    pub binary_data_id: u16,
    /// Embedded object extent width in HWPUNIT.
    pub extent_width: i32,
    /// Embedded object extent height in HWPUNIT.
    pub extent_height: i32,
}

/// Parsed table control content.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5Table {
    /// Number of rows declared by the table body record.
    pub rows: u16,
    /// Number of columns declared by the table body record.
    pub cols: u16,
    /// Page break behavior declared by the table body record.
    pub page_break: Hwp5TablePageBreak,
    /// Whether the table repeats its header row across page breaks.
    pub repeat_header: bool,
    /// Cell spacing declared by the table body record in HWPUNIT16.
    pub cell_spacing: i16,
    /// Optional table-level border/fill reference.
    pub border_fill_id: Option<u16>,
    /// Parsed cell records in source order.
    pub cells: Vec<Hwp5TableCell>,
}

/// HWP5 table page break policy recovered from the table body record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hwp5TablePageBreak {
    /// Do not split the table across pages.
    None,
    /// Split at cell boundaries.
    Cell,
    /// Split at table boundaries.
    Table,
    /// Unknown raw value preserved for audit.
    Unknown(u8),
}

/// Vertical alignment recovered from a table cell `ListHeader`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hwp5TableCellVerticalAlign {
    /// Align cell content to the top edge.
    Top,
    /// Center cell content vertically.
    Center,
    /// Align cell content to the bottom edge.
    Bottom,
    /// Unknown raw value preserved for audit.
    Unknown(u8),
}

/// Explicit cell-local margin recovered from a table cell `ListHeader`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hwp5TableCellMargin {
    /// Left margin in HWPUNIT16.
    pub left: i16,
    /// Right margin in HWPUNIT16.
    pub right: i16,
    /// Top margin in HWPUNIT16.
    pub top: i16,
    /// Bottom margin in HWPUNIT16.
    pub bottom: i16,
}

/// Parsed table cell from a `ListHeader` after a `Table` record.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5TableCell {
    /// Zero-based column index.
    pub column: u16,
    /// Zero-based row index.
    pub row: u16,
    /// Horizontal span. Minimum 1.
    pub col_span: u16,
    /// Vertical span. Minimum 1.
    pub row_span: u16,
    /// Cell width in HWPUNIT.
    pub width: i32,
    /// Cell height in HWPUNIT.
    pub height: i32,
    /// Cell-local inner margin in HWPUNIT16.
    pub margin: Hwp5TableCellMargin,
    /// Cell content vertical alignment.
    pub vertical_align: Hwp5TableCellVerticalAlign,
    /// Whether this cell is marked as belonging to a title/header row.
    pub is_header: bool,
    /// Optional border/fill reference.
    #[allow(dead_code)] // reserved for later border/fill projection work
    pub border_fill_id: Option<u16>,
    /// Cell paragraphs.
    pub paragraphs: Vec<Hwp5Paragraph>,
}

const TABLE_CELL_HEADER_FLAG: u32 = 0x0004_0000;

/// Parsed nested subtree carried by a non-table control.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5NestedSubtree {
    /// Original control identifier that owns the subtree.
    pub ctrl_id: u32,
    /// Nested paragraphs captured under the subtree.
    pub paragraphs: Vec<Hwp5Paragraph>,
}

/// Result of decoding one BodyText/Section{N} stream.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct SectionResult {
    /// Decoded paragraphs in document order.
    pub paragraphs: Vec<Hwp5Paragraph>,
    /// Page definition, if a PageDef record was found.
    pub page_def: Option<Hwp5PageDef>,
    /// Non-fatal warnings.
    pub warnings: Vec<Hwp5Warning>,
}

// ---------------------------------------------------------------------------
// ctrl_id constants
// ---------------------------------------------------------------------------

/// ctrl_id for a table control: ASCII 'tbl ' as big-endian u32.
const CTRL_ID_TABLE: u32 = 0x7462_6C20;
/// ctrl_id for header control: ASCII `head` as big-endian u32.
const CTRL_ID_HEADER: u32 = 0x6865_6164;
/// ctrl_id for footer control: ASCII `foot` as big-endian u32.
const CTRL_ID_FOOTER: u32 = 0x666F_6F74;
/// ctrl_id for generic shape object control: ASCII `gso ` as big-endian u32.
const CTRL_ID_GSO: u32 = 0x6773_6F20;

// ---------------------------------------------------------------------------
// Parser state
// ---------------------------------------------------------------------------

/// Mutable accumulator for a paragraph being assembled.
struct ParaBuf {
    header: Hwp5ParaHeader,
    text: Option<Hwp5ParaText>,
    char_shape_runs: Vec<Hwp5CharShapeRun>,
    controls: Vec<Hwp5Control>,
}

impl ParaBuf {
    fn new(header: Hwp5ParaHeader) -> Self {
        Self { header, text: None, char_shape_runs: Vec::new(), controls: Vec::new() }
    }

    /// Build the final `Hwp5Paragraph`, consuming this buffer.
    fn finish(self) -> Hwp5Paragraph {
        let text = match self.text {
            Some(pt) => segments_to_string(&pt.segments),
            None => String::new(),
        };
        Hwp5Paragraph {
            text,
            para_shape_id: self.header.para_shape_id,
            style_id: self.header.style_id,
            char_shape_runs: self.char_shape_runs,
            controls: self.controls,
        }
    }
}

/// Active table control while walking nested child records.
struct TableContext {
    ctrl_depth: u16,
    table: Hwp5Table,
    seen_table_body: bool,
    current_cell: Option<ActiveTableCell>,
    current_cell_para: Option<ParaBuf>,
    inline_cell_gso_ctx: Option<InlineGsoContext>,
}

impl TableContext {
    fn new(ctrl_depth: u16) -> Self {
        Self {
            ctrl_depth,
            table: Hwp5Table {
                rows: 0,
                cols: 0,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: Vec::new(),
            },
            seen_table_body: false,
            current_cell: None,
            current_cell_para: None,
            inline_cell_gso_ctx: None,
        }
    }

    fn flush_inline_gso(&mut self) {
        attach_inline_gso_control(&mut self.current_cell_para, self.inline_cell_gso_ctx.take());
    }

    fn flush_current_cell_paragraph(&mut self) {
        let Some(buf) = self.current_cell_para.take() else {
            return;
        };
        if let Some(cell) = self.current_cell.as_mut() {
            cell.cell.paragraphs.push(buf.finish());
        }
    }

    fn finish_active_cell_if_ready(&mut self) {
        let should_finish = self.current_cell.as_ref().is_some_and(|cell| {
            cell.expected_paragraphs == 0 || cell.cell.paragraphs.len() >= cell.expected_paragraphs
        });
        if should_finish {
            if let Some(cell) = self.current_cell.take() {
                self.table.cells.push(cell.cell);
            }
        }
    }

    fn finalize(mut self) -> Hwp5Control {
        self.flush_inline_gso();
        self.flush_current_cell_paragraph();
        self.finish_active_cell_if_ready();
        Hwp5Control::Table(self.table)
    }
}

/// Cell currently receiving nested paragraph records.
struct ActiveTableCell {
    expected_paragraphs: usize,
    cell: Hwp5TableCell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NestedSubtreeKind {
    Header,
    Footer,
    TextBox,
}

/// Active non-table control while collecting a nested paragraph subtree.
struct NestedSubtreeContext {
    ctrl_depth: u16,
    ctrl_id: u32,
    saw_list_header: bool,
    saw_shape_rectangle: bool,
    saw_shape_component: bool,
    geometry: Option<Hwp5ShapeComponentGeometry>,
    picture: Option<Hwp5ShapePicture>,
    ole: Option<Hwp5ShapeComponentOle>,
    line: Option<Hwp5ShapeComponentLine>,
    polygon: Option<Hwp5ShapeComponentPolygon>,
    paragraphs: Vec<Hwp5Paragraph>,
}

impl NestedSubtreeContext {
    fn new(ctrl_depth: u16, ctrl_id: u32, geometry: Option<Hwp5ShapeComponentGeometry>) -> Self {
        Self {
            ctrl_depth,
            ctrl_id,
            saw_list_header: false,
            saw_shape_rectangle: false,
            saw_shape_component: false,
            geometry,
            picture: None,
            ole: None,
            line: None,
            polygon: None,
            paragraphs: Vec::new(),
        }
    }

    fn note_list_header(&mut self) {
        self.saw_list_header = true;
    }

    fn note_shape_rectangle(&mut self) {
        self.saw_shape_rectangle = true;
    }

    fn note_shape_component(&mut self) {
        self.saw_shape_component = true;
    }

    fn note_shape_picture(&mut self, picture: Hwp5ShapePicture) {
        self.picture = Some(picture);
    }

    fn note_shape_ole(&mut self, ole: Hwp5ShapeComponentOle) {
        self.ole = Some(ole);
    }

    fn note_shape_line(&mut self, line: Hwp5ShapeComponentLine) {
        self.line = Some(line);
    }

    fn note_shape_polygon(&mut self, polygon: Hwp5ShapeComponentPolygon) {
        self.polygon = Some(polygon);
    }

    fn allows_nested_paragraphs(&self) -> bool {
        match self.ctrl_id {
            CTRL_ID_HEADER | CTRL_ID_FOOTER | CTRL_ID_GSO => self.saw_list_header,
            _ => false,
        }
    }

    fn kind(&self) -> Option<NestedSubtreeKind> {
        match self.ctrl_id {
            CTRL_ID_HEADER => Some(NestedSubtreeKind::Header),
            CTRL_ID_FOOTER => Some(NestedSubtreeKind::Footer),
            CTRL_ID_GSO if self.saw_shape_rectangle => Some(NestedSubtreeKind::TextBox),
            _ => None,
        }
    }

    fn into_control(self) -> Hwp5Control {
        match self.kind() {
            Some(NestedSubtreeKind::Header) if self.saw_list_header => {
                Hwp5Control::Header(Hwp5NestedSubtree {
                    ctrl_id: self.ctrl_id,
                    paragraphs: self.paragraphs,
                })
            }
            Some(NestedSubtreeKind::Footer) if self.saw_list_header => {
                Hwp5Control::Footer(Hwp5NestedSubtree {
                    ctrl_id: self.ctrl_id,
                    paragraphs: self.paragraphs,
                })
            }
            Some(NestedSubtreeKind::TextBox) if self.saw_list_header => match self.geometry {
                Some(geometry) => Hwp5Control::TextBox(Hwp5TextBoxControl {
                    ctrl_id: self.ctrl_id,
                    geometry,
                    paragraphs: self.paragraphs,
                }),
                None => Hwp5Control::Unknown { ctrl_id: self.ctrl_id },
            },
            _ => classify_gso_control(GsoClassificationInput {
                ctrl_id: self.ctrl_id,
                saw_shape_component: self.saw_shape_component,
                saw_shape_rectangle: self.saw_shape_rectangle && !self.saw_list_header,
                geometry: self.geometry,
                picture: self.picture,
                ole: self.ole,
                line: self.line,
                polygon: self.polygon,
            }),
        }
    }
}

/// Active paragraph-local `gso ` scope while collecting image evidence.
struct InlineGsoContext {
    ctrl_depth: u16,
    ctrl_id: u32,
    saw_shape_component: bool,
    saw_shape_rectangle: bool,
    geometry: Option<Hwp5ShapeComponentGeometry>,
    picture: Option<Hwp5ShapePicture>,
    ole: Option<Hwp5ShapeComponentOle>,
    line: Option<Hwp5ShapeComponentLine>,
    polygon: Option<Hwp5ShapeComponentPolygon>,
}

struct GsoClassificationInput {
    ctrl_id: u32,
    saw_shape_component: bool,
    saw_shape_rectangle: bool,
    geometry: Option<Hwp5ShapeComponentGeometry>,
    picture: Option<Hwp5ShapePicture>,
    ole: Option<Hwp5ShapeComponentOle>,
    line: Option<Hwp5ShapeComponentLine>,
    polygon: Option<Hwp5ShapeComponentPolygon>,
}

impl InlineGsoContext {
    fn new(ctrl_depth: u16, ctrl_id: u32, geometry: Option<Hwp5ShapeComponentGeometry>) -> Self {
        Self {
            ctrl_depth,
            ctrl_id,
            saw_shape_component: false,
            saw_shape_rectangle: false,
            geometry,
            picture: None,
            ole: None,
            line: None,
            polygon: None,
        }
    }

    fn note_shape_component(&mut self) {
        self.saw_shape_component = true;
    }

    fn note_shape_rectangle(&mut self) {
        self.saw_shape_rectangle = true;
    }

    fn note_shape_picture(&mut self, picture: Hwp5ShapePicture) {
        self.picture = Some(picture);
    }

    fn note_shape_ole(&mut self, ole: Hwp5ShapeComponentOle) {
        self.ole = Some(ole);
    }

    fn note_shape_line(&mut self, line: Hwp5ShapeComponentLine) {
        self.line = Some(line);
    }

    fn note_shape_polygon(&mut self, polygon: Hwp5ShapeComponentPolygon) {
        self.polygon = Some(polygon);
    }

    fn into_control(self) -> Hwp5Control {
        classify_gso_control(GsoClassificationInput {
            ctrl_id: self.ctrl_id,
            saw_shape_component: self.saw_shape_component,
            saw_shape_rectangle: self.saw_shape_rectangle,
            geometry: self.geometry,
            picture: self.picture,
            ole: self.ole,
            line: self.line,
            polygon: self.polygon,
        })
    }
}

fn classify_gso_control(input: GsoClassificationInput) -> Hwp5Control {
    if input.ctrl_id != CTRL_ID_GSO || !input.saw_shape_component {
        return Hwp5Control::Unknown { ctrl_id: input.ctrl_id };
    }

    let payload_count = usize::from(input.picture.is_some())
        + usize::from(input.ole.is_some())
        + usize::from(input.saw_shape_rectangle)
        + usize::from(input.line.is_some())
        + usize::from(input.polygon.is_some());
    if payload_count != 1 {
        return Hwp5Control::Unknown { ctrl_id: input.ctrl_id };
    }

    match (input.geometry, input.picture, input.ole, input.line, input.polygon) {
        (Some(geometry), Some(picture), None, None, None) => Hwp5Control::Image(Hwp5ImageControl {
            ctrl_id: input.ctrl_id,
            geometry,
            binary_data_id: picture.binary_data_id,
        }),
        (Some(geometry), None, Some(ole), None, None) => {
            Hwp5Control::OleObject(Hwp5OleObjectControl {
                ctrl_id: input.ctrl_id,
                geometry,
                binary_data_id: ole.binary_data_id,
                extent_width: ole.extent_width,
                extent_height: ole.extent_height,
            })
        }
        (Some(geometry), None, None, None, None) if input.saw_shape_rectangle => {
            Hwp5Control::Rect(Hwp5RectControl { ctrl_id: input.ctrl_id, geometry })
        }
        (Some(geometry), None, None, Some(line), None) => Hwp5Control::Line(Hwp5LineControl {
            ctrl_id: input.ctrl_id,
            geometry,
            start: line.start,
            end: line.end,
        }),
        (Some(geometry), None, None, None, Some(polygon)) if polygon.points.len() >= 3 => {
            Hwp5Control::Polygon(Hwp5PolygonControl {
                ctrl_id: input.ctrl_id,
                geometry,
                points: polygon.points,
            })
        }
        _ => Hwp5Control::Unknown { ctrl_id: input.ctrl_id },
    }
}

// ---------------------------------------------------------------------------
// Text rendering
// ---------------------------------------------------------------------------

/// Convert a slice of `TextSegment`s into a plain string.
///
/// - `Text(s)` — appended verbatim
/// - `Tab` — replaced with `\t`
/// - `LineBreak` — replaced with `\n`
/// - `NonBreakingSpace` — replaced with a regular space
/// - `ControlRef` / `ExtendedControlRef` — replaced with `\u{FFFC}` (object replacement)
/// - All other segments (ParaBreak, FieldBegin, FieldEnd, SectionColumnDef) — ignored
fn segments_to_string(segments: &[TextSegment]) -> String {
    let mut out = String::new();
    for seg in segments {
        match seg {
            TextSegment::Text(s) => out.push_str(s),
            TextSegment::Tab => out.push('\t'),
            TextSegment::LineBreak => out.push('\n'),
            TextSegment::NonBreakingSpace => out.push(' '),
            TextSegment::ControlRef { .. } | TextSegment::ExtendedControlRef { .. } => {
                out.push('\u{FFFC}');
            }
            TextSegment::ParaBreak
            | TextSegment::FieldBegin { .. }
            | TextSegment::FieldEnd
            | TextSegment::SectionColumnDef { .. } => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Parse a BodyText section stream into paragraphs.
///
/// Accepts the raw (already-decompressed) bytes of a `BodyText/Section{N}`
/// OLE2 stream. The function is best-effort: unknown or malformed records
/// are collected as [`Hwp5Warning`]s rather than hard errors.
///
/// # Errors
///
/// Returns [`crate::error::Hwp5Error::RecordParse`] only if the stream is
/// too corrupt to be parsed as a sequence of HWP5 records.
pub(crate) fn parse_body_text(data: &[u8], _version: &HwpVersion) -> Hwp5Result<SectionResult> {
    let records = Record::parse_stream(&mut Cursor::new(data))?;
    let mut parser = BodyTextParserState::default();
    for record in &records {
        parser.handle_record(record);
    }
    Ok(parser.finish())
}

#[derive(Default)]
struct BodyTextParserState {
    paragraphs: Vec<Hwp5Paragraph>,
    page_def: Option<Hwp5PageDef>,
    warnings: Vec<Hwp5Warning>,
    current: Option<ParaBuf>,
    table_stack: Vec<TableContext>,
    subtree_ctx: Option<NestedSubtreeContext>,
    current_subtree_para: Option<ParaBuf>,
    inline_subtree_gso_ctx: Option<InlineGsoContext>,
}

impl BodyTextParserState {
    fn handle_record(&mut self, record: &Record) {
        let tag = TagId::from(record.header.tag_id);
        let level = record.header.level;

        self.prepare_for_record(level);

        if self.handle_active_table_record(record, tag, level) {
            return;
        }
        if self.handle_active_subtree_record(record, tag, level) {
            return;
        }
        self.handle_top_level_record(record, tag, level);
    }

    fn prepare_for_record(&mut self, level: u16) {
        if self
            .table_stack
            .last()
            .and_then(|ctx| ctx.inline_cell_gso_ctx.as_ref())
            .is_some_and(|ctx| level <= ctx.ctrl_depth)
        {
            if let Some(ctx) = self.table_stack.last_mut() {
                ctx.flush_inline_gso();
            }
        }

        if self.inline_subtree_gso_ctx.as_ref().is_some_and(|ctx| level <= ctx.ctrl_depth) {
            attach_inline_gso_control(
                &mut self.current_subtree_para,
                self.inline_subtree_gso_ctx.take(),
            );
        }

        while self.table_stack.last().is_some_and(|ctx| level <= ctx.ctrl_depth) {
            let finished = self
                .table_stack
                .pop()
                .expect("table_stack.last().is_some() implies pop succeeds")
                .finalize();
            attach_finished_table(
                &mut self.current,
                &mut self.table_stack,
                finished,
                &mut self.warnings,
            );
        }

        if self.subtree_ctx.as_ref().is_some_and(|ctx| level <= ctx.ctrl_depth) {
            attach_inline_gso_control(
                &mut self.current_subtree_para,
                self.inline_subtree_gso_ctx.take(),
            );
            flush_subtree_paragraph(&mut self.current_subtree_para, self.subtree_ctx.as_mut());
            attach_finished_subtree(&mut self.current, self.subtree_ctx.take());
        }
    }

    fn handle_active_table_record(&mut self, record: &Record, tag: TagId, level: u16) -> bool {
        if self.table_stack.last().is_none_or(|ctx| level <= ctx.ctrl_depth) {
            return false;
        }

        if Self::handle_inline_gso_record(
            record,
            tag,
            &mut self.warnings,
            self.table_stack.last_mut().and_then(|table| table.inline_cell_gso_ctx.as_mut()),
        ) {
            return true;
        }

        match tag {
            TagId::Table => {
                let table_header = parse_table_header(&record.data);
                if let Some(ctx) = self.table_stack.last_mut() {
                    ctx.table.rows = table_header.rows;
                    ctx.table.cols = table_header.cols;
                    ctx.table.page_break = table_header.page_break;
                    ctx.table.repeat_header = table_header.repeat_header;
                    ctx.table.cell_spacing = table_header.cell_spacing;
                    ctx.table.border_fill_id = table_header.border_fill_id;
                    ctx.seen_table_body = true;
                }
            }
            TagId::ListHeader => {
                if let Some(ctx) = self.table_stack.last_mut() {
                    if ctx.seen_table_body && level == ctx.ctrl_depth.saturating_add(1) {
                        ctx.flush_current_cell_paragraph();
                        ctx.finish_active_cell_if_ready();
                        match parse_table_cell(&record.data) {
                            Ok((paragraph_count, cell)) => {
                                ctx.current_cell = Some(ActiveTableCell {
                                    expected_paragraphs: paragraph_count,
                                    cell,
                                });
                                ctx.finish_active_cell_if_ready();
                            }
                            Err(_) => self.push_unsupported_tag(record.header.tag_id),
                        }
                    }
                }
            }
            TagId::ParaHeader => {
                if self.table_stack.last().and_then(|ctx| ctx.current_cell.as_ref()).is_some() {
                    if let Some(ctx) = self.table_stack.last_mut() {
                        ctx.flush_current_cell_paragraph();
                    }
                    if let Some(buf) = Self::parse_para_header_buf(
                        record.header.tag_id,
                        &record.data,
                        &mut self.warnings,
                    ) {
                        if let Some(ctx) = self.table_stack.last_mut() {
                            ctx.current_cell_para = Some(buf);
                        }
                    }
                }
            }
            TagId::ParaText => {
                if let Some(buf) =
                    self.table_stack.last_mut().and_then(|ctx| ctx.current_cell_para.as_mut())
                {
                    if let Some(text) = Self::parse_para_text_value(
                        record.header.tag_id,
                        &record.data,
                        &mut self.warnings,
                    ) {
                        buf.text = Some(text);
                    }
                }
            }
            TagId::ParaCharShape => {
                if let Some(buf) =
                    self.table_stack.last_mut().and_then(|ctx| ctx.current_cell_para.as_mut())
                {
                    if let Some(runs) = Self::parse_para_char_shape_runs(
                        record.header.tag_id,
                        &record.data,
                        &mut self.warnings,
                    ) {
                        buf.char_shape_runs = runs;
                    }
                }
            }
            TagId::ParaLineSeg => {}
            TagId::CtrlHeader => {
                if let Some(ctx) = self.table_stack.last_mut() {
                    let ctrl_id = parse_ctrl_id(&record.data);
                    if ctrl_id == CTRL_ID_TABLE {
                        self.table_stack.push(TableContext::new(level));
                    } else if ctrl_id == CTRL_ID_GSO {
                        ctx.inline_cell_gso_ctx = Some(InlineGsoContext::new(
                            level,
                            ctrl_id,
                            Hwp5ShapeComponentGeometry::parse_from_ctrl_header(&record.data).ok(),
                        ));
                    } else if let Some(buf) = ctx.current_cell_para.as_mut() {
                        buf.controls.push(Hwp5Control::Unknown { ctrl_id });
                    }
                }
            }
            TagId::Unknown(id) => {
                self.warnings.push(Hwp5Warning::UnsupportedTag { tag_id: id, offset: 0 });
            }
            _ => {}
        }

        true
    }

    fn handle_active_subtree_record(&mut self, record: &Record, tag: TagId, level: u16) -> bool {
        if self.subtree_ctx.as_ref().is_none_or(|ctx| level <= ctx.ctrl_depth) {
            return false;
        }

        if Self::handle_inline_gso_record(
            record,
            tag,
            &mut self.warnings,
            self.inline_subtree_gso_ctx.as_mut(),
        ) {
            return true;
        }

        match tag {
            TagId::ListHeader => {
                if let Some(ctx) = self.subtree_ctx.as_mut() {
                    ctx.note_list_header();
                }
            }
            TagId::ShapeComponent => {
                if let Some(ctx) = self.subtree_ctx.as_mut() {
                    ctx.note_shape_component();
                }
            }
            TagId::ShapeComponentLine => match Hwp5ShapeComponentLine::parse(&record.data) {
                Ok(line) => {
                    if let Some(ctx) = self.subtree_ctx.as_mut() {
                        ctx.note_shape_line(line);
                    }
                }
                Err(_) => self.push_unsupported_tag(record.header.tag_id),
            },
            TagId::ShapeComponentRect => {
                if let Some(ctx) = self.subtree_ctx.as_mut() {
                    ctx.note_shape_rectangle();
                }
            }
            TagId::ShapeComponentPolygon => match Hwp5ShapeComponentPolygon::parse(&record.data) {
                Ok(polygon) => {
                    if let Some(ctx) = self.subtree_ctx.as_mut() {
                        ctx.note_shape_polygon(polygon);
                    }
                }
                Err(_) => self.push_unsupported_tag(record.header.tag_id),
            },
            TagId::ShapePicture => match Hwp5ShapePicture::parse(&record.data) {
                Ok(picture) => {
                    if let Some(ctx) = self.subtree_ctx.as_mut() {
                        ctx.note_shape_picture(picture);
                    }
                }
                Err(_) => self.push_unsupported_tag(record.header.tag_id),
            },
            TagId::ShapeComponentOle => match Hwp5ShapeComponentOle::parse(&record.data) {
                Ok(ole) => {
                    if let Some(ctx) = self.subtree_ctx.as_mut() {
                        ctx.note_shape_ole(ole);
                    }
                }
                Err(_) => self.push_unsupported_tag(record.header.tag_id),
            },
            TagId::ParaHeader => {
                if self
                    .subtree_ctx
                    .as_ref()
                    .is_some_and(NestedSubtreeContext::allows_nested_paragraphs)
                {
                    flush_subtree_paragraph(
                        &mut self.current_subtree_para,
                        self.subtree_ctx.as_mut(),
                    );
                    self.current_subtree_para = Self::parse_para_header_buf(
                        record.header.tag_id,
                        &record.data,
                        &mut self.warnings,
                    );
                }
            }
            TagId::ParaText => {
                if let Some(buf) = self.current_subtree_para.as_mut() {
                    if let Some(text) = Self::parse_para_text_value(
                        record.header.tag_id,
                        &record.data,
                        &mut self.warnings,
                    ) {
                        buf.text = Some(text);
                    }
                }
            }
            TagId::ParaCharShape => {
                if let Some(buf) = self.current_subtree_para.as_mut() {
                    if let Some(runs) = Self::parse_para_char_shape_runs(
                        record.header.tag_id,
                        &record.data,
                        &mut self.warnings,
                    ) {
                        buf.char_shape_runs = runs;
                    }
                }
            }
            TagId::ParaLineSeg => {}
            TagId::CtrlHeader => {
                if let Some(buf) = self.current_subtree_para.as_mut() {
                    let ctrl_id = parse_ctrl_id(&record.data);
                    if ctrl_id == CTRL_ID_GSO {
                        self.inline_subtree_gso_ctx = Some(InlineGsoContext::new(
                            level,
                            ctrl_id,
                            Hwp5ShapeComponentGeometry::parse_from_ctrl_header(&record.data).ok(),
                        ));
                    } else {
                        buf.controls.push(Hwp5Control::Unknown { ctrl_id });
                    }
                }
            }
            TagId::Unknown(id) => {
                self.warnings.push(Hwp5Warning::UnsupportedTag { tag_id: id, offset: 0 });
            }
            _ => {}
        }

        true
    }

    fn handle_top_level_record(&mut self, record: &Record, tag: TagId, level: u16) {
        match tag {
            TagId::ParaHeader if level == 0 => {
                if let Some(buf) = self.current.take() {
                    self.paragraphs.push(buf.finish());
                }
                self.current = Self::parse_para_header_buf(
                    record.header.tag_id,
                    &record.data,
                    &mut self.warnings,
                );
            }
            TagId::ParaText => {
                if let Some(buf) = self.current.as_mut() {
                    if let Some(text) = Self::parse_para_text_value(
                        record.header.tag_id,
                        &record.data,
                        &mut self.warnings,
                    ) {
                        buf.text = Some(text);
                    }
                }
            }
            TagId::ParaCharShape => {
                if let Some(buf) = self.current.as_mut() {
                    if let Some(runs) = Self::parse_para_char_shape_runs(
                        record.header.tag_id,
                        &record.data,
                        &mut self.warnings,
                    ) {
                        buf.char_shape_runs = runs;
                    }
                }
            }
            TagId::ParaLineSeg => {}
            TagId::PageDef => match Hwp5PageDef::parse(&record.data) {
                Ok(pd) => self.page_def = Some(pd),
                Err(_) => self.push_unsupported_tag(record.header.tag_id),
            },
            TagId::CtrlHeader => {
                let ctrl_id = parse_ctrl_id(&record.data);
                if ctrl_id == CTRL_ID_TABLE {
                    self.table_stack.push(TableContext::new(level));
                } else if matches!(ctrl_id, CTRL_ID_HEADER | CTRL_ID_FOOTER | CTRL_ID_GSO) {
                    let geometry = if ctrl_id == CTRL_ID_GSO {
                        Hwp5ShapeComponentGeometry::parse_from_ctrl_header(&record.data).ok()
                    } else {
                        None
                    };
                    self.subtree_ctx = Some(NestedSubtreeContext::new(level, ctrl_id, geometry));
                } else if let Some(buf) = self.current.as_mut() {
                    buf.controls.push(Hwp5Control::Unknown { ctrl_id });
                }
            }
            TagId::ListHeader => {}
            TagId::Unknown(id) => {
                self.warnings.push(Hwp5Warning::UnsupportedTag { tag_id: id, offset: 0 });
            }
            _ => {}
        }
    }

    fn finish(mut self) -> SectionResult {
        while let Some(ctx) = self.table_stack.pop() {
            let finished = ctx.finalize();
            attach_finished_table(
                &mut self.current,
                &mut self.table_stack,
                finished,
                &mut self.warnings,
            );
        }
        attach_inline_gso_control(
            &mut self.current_subtree_para,
            self.inline_subtree_gso_ctx.take(),
        );
        flush_subtree_paragraph(&mut self.current_subtree_para, self.subtree_ctx.as_mut());
        attach_finished_subtree(&mut self.current, self.subtree_ctx.take());

        if let Some(buf) = self.current {
            self.paragraphs.push(buf.finish());
        }

        SectionResult {
            paragraphs: self.paragraphs,
            page_def: self.page_def,
            warnings: self.warnings,
        }
    }

    fn push_unsupported_tag(&mut self, tag_id: u16) {
        self.warnings.push(Hwp5Warning::UnsupportedTag { tag_id, offset: 0 });
    }

    fn parse_para_header_buf(
        tag_id: u16,
        data: &[u8],
        warnings: &mut Vec<Hwp5Warning>,
    ) -> Option<ParaBuf> {
        match Hwp5ParaHeader::parse(data) {
            Ok(header) => Some(ParaBuf::new(header)),
            Err(_) => {
                warnings.push(Hwp5Warning::UnsupportedTag { tag_id, offset: 0 });
                None
            }
        }
    }

    fn parse_para_text_value(
        tag_id: u16,
        data: &[u8],
        warnings: &mut Vec<Hwp5Warning>,
    ) -> Option<Hwp5ParaText> {
        match Hwp5ParaText::parse(data) {
            Ok(text) => Some(text),
            Err(_) => {
                warnings.push(Hwp5Warning::UnsupportedTag { tag_id, offset: 0 });
                None
            }
        }
    }

    fn parse_para_char_shape_runs(
        tag_id: u16,
        data: &[u8],
        warnings: &mut Vec<Hwp5Warning>,
    ) -> Option<Vec<Hwp5CharShapeRun>> {
        match Hwp5CharShapeRun::parse_all(data) {
            Ok(runs) => Some(runs),
            Err(_) => {
                warnings.push(Hwp5Warning::UnsupportedTag { tag_id, offset: 0 });
                None
            }
        }
    }

    fn handle_inline_gso_record(
        record: &Record,
        tag: TagId,
        warnings: &mut Vec<Hwp5Warning>,
        inline_gso_ctx: Option<&mut InlineGsoContext>,
    ) -> bool {
        let Some(ctx) = inline_gso_ctx else {
            return false;
        };

        match tag {
            TagId::ShapeComponent => ctx.note_shape_component(),
            TagId::ShapeComponentRect => ctx.note_shape_rectangle(),
            TagId::ShapeComponentLine => match Hwp5ShapeComponentLine::parse(&record.data) {
                Ok(line) => ctx.note_shape_line(line),
                Err(_) => warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::ShapeComponentPolygon => match Hwp5ShapeComponentPolygon::parse(&record.data) {
                Ok(polygon) => ctx.note_shape_polygon(polygon),
                Err(_) => warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::ShapePicture => match Hwp5ShapePicture::parse(&record.data) {
                Ok(picture) => ctx.note_shape_picture(picture),
                Err(_) => warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::ShapeComponentOle => match Hwp5ShapeComponentOle::parse(&record.data) {
                Ok(ole) => ctx.note_shape_ole(ole),
                Err(_) => warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::Unknown(id) => {
                warnings.push(Hwp5Warning::UnsupportedTag { tag_id: id, offset: 0 });
            }
            _ => {}
        }

        true
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the ctrl_id from the first 4 bytes of a `CtrlHeader` data payload.
///
/// The stored bytes are little-endian in the record payload, so the raw
/// sequence `[0x20, 0x6C, 0x62, 0x74]` decodes to `0x74626C20` (`"tbl "`).
/// Returns 0 on short data.
fn parse_ctrl_id(data: &[u8]) -> u32 {
    if data.len() < 4 {
        return 0;
    }
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

/// Parsed table-level fields recovered from a `Table` record payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParsedTableHeader {
    rows: u16,
    cols: u16,
    page_break: Hwp5TablePageBreak,
    repeat_header: bool,
    cell_spacing: i16,
    border_fill_id: Option<u16>,
}

/// Extract minimal table-level fields from a `Table` record data payload.
///
/// Layout (little-endian):
/// - `[0..4]`  u32 property bitfield
/// - `[4..6]`  u16 row_count
/// - `[6..8]`  u16 col_count
/// - `[8..10]` i16 cell_spacing
/// - `[10..18]` padding (ignored for now)
/// - `[18..18+rows*2]` row-local metadata (shape meaning differs across references,
///   but the size is stable and sufficient to recover later fields)
/// - `next..next+2` optional table border/fill id
///
/// Returns zeroed/default fields if the data is too short.
fn parse_table_header(data: &[u8]) -> ParsedTableHeader {
    if data.len() < 8 {
        return ParsedTableHeader {
            rows: 0,
            cols: 0,
            page_break: Hwp5TablePageBreak::None,
            repeat_header: false,
            cell_spacing: 0,
            border_fill_id: None,
        };
    }
    let properties = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let rows = u16::from_le_bytes([data[4], data[5]]);
    let cols = u16::from_le_bytes([data[6], data[7]]);
    let raw_page_break = (properties & 0b11) as u8;
    let page_break = match raw_page_break {
        0 => Hwp5TablePageBreak::None,
        1 => Hwp5TablePageBreak::Table,
        2 => Hwp5TablePageBreak::Cell,
        raw => Hwp5TablePageBreak::Unknown(raw),
    };
    let repeat_header = (properties & 0b100) != 0;
    let cell_spacing = if data.len() >= 10 { i16::from_le_bytes([data[8], data[9]]) } else { 0 };
    let row_metadata_len = usize::from(rows).saturating_mul(2);
    let border_fill_offset = 18usize.saturating_add(row_metadata_len);
    let border_fill_id = data
        .get(border_fill_offset..border_fill_offset + 2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .filter(|&id| id > 0);

    ParsedTableHeader { rows, cols, page_break, repeat_header, cell_spacing, border_fill_id }
}

/// Parse a table cell `ListHeader` payload.
///
/// Real files usually store `paragraph_count` as `u32`; a legacy `size == 30`
/// variant uses `u16` + `u32 properties` ahead of the 24-byte cell payload.
fn parse_table_cell(data: &[u8]) -> Hwp5Result<(usize, Hwp5TableCell)> {
    if data.len() < 30 {
        return Err(crate::error::Hwp5Error::RecordParse {
            offset: 0,
            detail: format!("Table cell ListHeader too short: {} bytes", data.len()),
        });
    }

    let (paragraph_count, properties, base): (usize, u32, &[u8]) = if data.len() == 30 {
        (
            u16::from_le_bytes([data[0], data[1]]) as usize,
            u32::from_le_bytes([data[2], data[3], data[4], data[5]]),
            &data[6..],
        )
    } else {
        (
            u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize,
            u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            &data[8..],
        )
    };
    let column = u16::from_le_bytes([base[0], base[1]]);
    let row = u16::from_le_bytes([base[2], base[3]]);
    let col_span = u16::from_le_bytes([base[4], base[5]]).max(1);
    let row_span = u16::from_le_bytes([base[6], base[7]]).max(1);
    let width = i32::from_le_bytes([base[8], base[9], base[10], base[11]]);
    let height = i32::from_le_bytes([base[12], base[13], base[14], base[15]]);
    let margin = Hwp5TableCellMargin {
        left: i16::from_le_bytes([base[16], base[17]]),
        right: i16::from_le_bytes([base[18], base[19]]),
        top: i16::from_le_bytes([base[20], base[21]]),
        bottom: i16::from_le_bytes([base[22], base[23]]),
    };
    let vertical_align = match ((properties >> 5) & 0x03) as u8 {
        0 => Hwp5TableCellVerticalAlign::Top,
        1 => Hwp5TableCellVerticalAlign::Center,
        2 => Hwp5TableCellVerticalAlign::Bottom,
        raw => Hwp5TableCellVerticalAlign::Unknown(raw),
    };
    let is_header = (properties & TABLE_CELL_HEADER_FLAG) != 0;
    let border_fill_id =
        (base.len() >= 26).then(|| u16::from_le_bytes([base[24], base[25]])).filter(|&id| id > 0);

    Ok((
        paragraph_count,
        Hwp5TableCell {
            column,
            row,
            col_span,
            row_span,
            width,
            height,
            margin,
            vertical_align,
            is_header,
            border_fill_id,
            paragraphs: Vec::new(),
        },
    ))
}

fn attach_finished_table(
    current: &mut Option<ParaBuf>,
    table_stack: &mut [TableContext],
    control: Hwp5Control,
    warnings: &mut Vec<Hwp5Warning>,
) {
    if let Some(parent) = table_stack.last_mut() {
        if let Some(buf) = parent.current_cell_para.as_mut() {
            buf.controls.push(control);
            return;
        }
        warnings.push(Hwp5Warning::ParserFallback {
            subject: "table.nested_attach",
            reason: "orphaned_nested_table_without_parent_paragraph".to_string(),
        });
    } else if let Some(buf) = current.as_mut() {
        buf.controls.push(control);
    } else {
        warnings.push(Hwp5Warning::ParserFallback {
            subject: "table.attach",
            reason: "table_control_without_host_paragraph".to_string(),
        });
    }
}

fn flush_subtree_paragraph(
    current_subtree_para: &mut Option<ParaBuf>,
    subtree_ctx: Option<&mut NestedSubtreeContext>,
) {
    let Some(buf) = current_subtree_para.take() else {
        return;
    };
    let Some(ctx) = subtree_ctx else {
        return;
    };
    ctx.paragraphs.push(buf.finish());
}

fn attach_finished_subtree(
    current: &mut Option<ParaBuf>,
    subtree_ctx: Option<NestedSubtreeContext>,
) {
    let Some(ctx) = subtree_ctx else {
        return;
    };
    if let Some(buf) = current.as_mut() {
        buf.controls.push(ctx.into_control());
    }
}

fn attach_inline_gso_control(
    current_para: &mut Option<ParaBuf>,
    inline_gso_ctx: Option<InlineGsoContext>,
) {
    let Some(ctx) = inline_gso_ctx else {
        return;
    };
    if let Some(buf) = current_para.as_mut() {
        buf.controls.push(ctx.into_control());
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::package::PackageReader;
    use crate::schema::header::HwpVersion;
    use crate::schema::record::{Record, TagId as RawTagId};
    use std::path::PathBuf;

    // ── Helper: build a single record's bytes ────────────────────────────

    fn make_record(tag: TagId, level: u16, data: &[u8]) -> Vec<u8> {
        let tag_val = u16::from(tag) as u32;
        let size = data.len() as u32;
        let mut buf;
        if size > 0xFFE {
            // Use 0xFFF extended-size marker.
            let word = tag_val | ((level as u32) << 10) | (0xFFF << 20);
            buf = word.to_le_bytes().to_vec();
            buf.extend_from_slice(&size.to_le_bytes());
        } else {
            let word = tag_val | ((level as u32) << 10) | (size << 20);
            buf = word.to_le_bytes().to_vec();
        }
        buf.extend_from_slice(data);
        buf
    }

    // ── Helper: 22-byte ParaHeader payload ──────────────────────────────

    fn para_header_data(para_shape_id: u16, style_id: u8) -> Vec<u8> {
        let mut buf = vec![0u8; 22];
        // char_count = 10
        buf[0..4].copy_from_slice(&10u32.to_le_bytes());
        buf[8..10].copy_from_slice(&para_shape_id.to_le_bytes());
        buf[10] = style_id;
        buf
    }

    // ── Helper: ParaText payload (UTF-16LE plain text) ───────────────────

    fn para_text_data(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect()
    }

    fn para_text_with_control_ref(prefix: &str, suffix: &str) -> Vec<u8> {
        let mut units: Vec<u16> = prefix.encode_utf16().collect();
        units.push(0x000B);
        units.extend([0u16; 7]);
        units.extend(suffix.encode_utf16());
        units.into_iter().flat_map(|code_unit| code_unit.to_le_bytes()).collect()
    }

    // ── Helper: CharShapeRun payload ─────────────────────────────────────

    fn char_shape_run_data(position: u32, char_shape_id: u32) -> Vec<u8> {
        let mut buf = Vec::with_capacity(8);
        buf.extend_from_slice(&position.to_le_bytes());
        buf.extend_from_slice(&char_shape_id.to_le_bytes());
        buf
    }

    // ── Helper: 40-byte PageDef payload ──────────────────────────────────

    fn page_def_data() -> Vec<u8> {
        let mut buf = vec![0u8; 40];
        // width = 59535, height = 84180 (A4)
        buf[0..4].copy_from_slice(&59535u32.to_le_bytes());
        buf[4..8].copy_from_slice(&84180u32.to_le_bytes());
        buf
    }

    // ── Helper: CtrlHeader payload with given ctrl_id (little-endian bytes) ─

    fn ctrl_header_data(ctrl_id: u32) -> Vec<u8> {
        ctrl_id.to_le_bytes().to_vec()
    }

    fn gso_ctrl_header_data(x: i32, y: i32, width: u32, height: u32) -> Vec<u8> {
        let mut buf = vec![0u8; 24];
        buf[0..4].copy_from_slice(&CTRL_ID_GSO.to_le_bytes());
        buf[8..12].copy_from_slice(&y.to_le_bytes());
        buf[12..16].copy_from_slice(&x.to_le_bytes());
        buf[16..20].copy_from_slice(&width.to_le_bytes());
        buf[20..24].copy_from_slice(&height.to_le_bytes());
        buf
    }

    // ── Helper: Table record payload ──────────────────────────────────────

    struct TestTableSpec {
        rows: u16,
        cols: u16,
        page_break_bits: u8,
        repeat_header: bool,
        cell_spacing: i16,
        row_metadata: Vec<u16>,
        border_fill_id: Option<u16>,
    }

    fn table_data(spec: TestTableSpec) -> Vec<u8> {
        let mut buf = vec![0u8; 18];
        let mut properties = u32::from(spec.page_break_bits & 0b11);
        if spec.repeat_header {
            properties |= 0b100;
        }
        buf[0..4].copy_from_slice(&properties.to_le_bytes());
        buf[4..6].copy_from_slice(&spec.rows.to_le_bytes());
        buf[6..8].copy_from_slice(&spec.cols.to_le_bytes());
        buf[8..10].copy_from_slice(&spec.cell_spacing.to_le_bytes());
        for value in spec.row_metadata {
            buf.extend_from_slice(&value.to_le_bytes());
        }
        if let Some(border_fill_id) = spec.border_fill_id {
            buf.extend_from_slice(&border_fill_id.to_le_bytes());
        }
        buf
    }

    fn basic_table_data(rows: u16, cols: u16) -> Vec<u8> {
        table_data(TestTableSpec {
            rows,
            cols,
            page_break_bits: 0,
            repeat_header: false,
            cell_spacing: 0,
            row_metadata: vec![0; usize::from(rows)],
            border_fill_id: None,
        })
    }

    fn shape_picture_data(binary_data_id: u16) -> Vec<u8> {
        let mut data = vec![0u8; 73];
        data[71..73].copy_from_slice(&binary_data_id.to_le_bytes());
        data
    }

    fn shape_component_ole_data(
        binary_data_id: u16,
        extent_width: i32,
        extent_height: i32,
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&0x0000_0003u32.to_le_bytes());
        data.extend_from_slice(&extent_width.to_le_bytes());
        data.extend_from_slice(&extent_height.to_le_bytes());
        data.extend_from_slice(&binary_data_id.to_le_bytes());
        data.extend_from_slice(&[0u8; 12]);
        data
    }

    fn shape_component_line_data(start_x: i32, start_y: i32, end_x: i32, end_y: i32) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&start_x.to_le_bytes());
        data.extend_from_slice(&start_y.to_le_bytes());
        data.extend_from_slice(&end_x.to_le_bytes());
        data.extend_from_slice(&end_y.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data
    }

    fn shape_component_polygon_data(points: &[(i32, i32)]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&(points.len() as u32).to_le_bytes());
        for (x, y) in points {
            data.extend_from_slice(&x.to_le_bytes());
            data.extend_from_slice(&y.to_le_bytes());
        }
        data
    }

    struct TestCellSpec {
        paragraph_count: u32,
        legacy_u16_count: bool,
        properties: u32,
        column: u16,
        row: u16,
        col_span: u16,
        row_span: u16,
        width: i32,
        height: i32,
        margin: Hwp5TableCellMargin,
        border_fill_id: Option<u16>,
    }

    fn list_header_table_cell_data(spec: TestCellSpec) -> Vec<u8> {
        let mut buf = Vec::new();
        if spec.legacy_u16_count {
            buf.extend_from_slice(&(spec.paragraph_count as u16).to_le_bytes());
            buf.extend_from_slice(&spec.properties.to_le_bytes());
        } else {
            buf.extend_from_slice(&spec.paragraph_count.to_le_bytes());
            buf.extend_from_slice(&spec.properties.to_le_bytes());
        }
        buf.extend_from_slice(&spec.column.to_le_bytes());
        buf.extend_from_slice(&spec.row.to_le_bytes());
        buf.extend_from_slice(&spec.col_span.to_le_bytes());
        buf.extend_from_slice(&spec.row_span.to_le_bytes());
        buf.extend_from_slice(&spec.width.to_le_bytes());
        buf.extend_from_slice(&spec.height.to_le_bytes());
        buf.extend_from_slice(&spec.margin.left.to_le_bytes());
        buf.extend_from_slice(&spec.margin.right.to_le_bytes());
        buf.extend_from_slice(&spec.margin.top.to_le_bytes());
        buf.extend_from_slice(&spec.margin.bottom.to_le_bytes());
        if let Some(border_fill_id) = spec.border_fill_id {
            buf.extend_from_slice(&border_fill_id.to_le_bytes());
        }
        buf
    }

    fn version() -> HwpVersion {
        HwpVersion::new(5, 0, 2, 5)
    }

    fn fixture_path(name: &str) -> PathBuf {
        crate::test_support::workspace_fixture_path(name)
    }

    fn table_cell_list_header_properties_from_fixture(name: &str) -> Vec<u32> {
        let bytes = std::fs::read(fixture_path(name)).expect("fixture bytes");
        let pkg = PackageReader::open(&bytes).expect("fixture package");
        let mut cursor = std::io::Cursor::new(pkg.sections_data()[0].clone());
        let records = Record::parse_stream(&mut cursor).expect("fixture section records");

        let mut saw_table_body = false;
        let mut properties = Vec::new();
        for record in records {
            match RawTagId::from(record.header.tag_id) {
                RawTagId::Table => saw_table_body = true,
                RawTagId::ListHeader if saw_table_body => {
                    let data = &record.data;
                    let properties_word = if data.len() == 30 {
                        u32::from_le_bytes([data[2], data[3], data[4], data[5]])
                    } else {
                        u32::from_le_bytes([data[4], data[5], data[6], data[7]])
                    };
                    properties.push(properties_word);
                }
                RawTagId::CtrlHeader | RawTagId::ParaHeader => {}
                _ => {}
            }
        }
        properties
    }

    // ── Tests ────────────────────────────────────────────────────────────

    #[test]
    fn empty_stream_returns_empty_result() {
        let result = parse_body_text(&[], &version()).unwrap();
        assert!(result.paragraphs.is_empty());
        assert!(result.page_def.is_none());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn single_paragraph_with_text_and_runs() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(2, 1)));
        stream.extend(make_record(TagId::ParaText, 0, &para_text_data("안녕")));
        stream.extend(make_record(TagId::ParaCharShape, 0, &char_shape_run_data(0, 3)));

        let result = parse_body_text(&stream, &version()).unwrap();
        assert_eq!(result.paragraphs.len(), 1);

        let para = &result.paragraphs[0];
        assert_eq!(para.text, "안녕");
        assert_eq!(para.para_shape_id, 2);
        assert_eq!(para.style_id, 1);
        assert_eq!(para.char_shape_runs.len(), 1);
        assert_eq!(para.char_shape_runs[0].position, 0);
        assert_eq!(para.char_shape_runs[0].char_shape_id, 3);
    }

    #[test]
    fn multiple_paragraphs_correct_count() {
        let mut stream = Vec::new();
        for i in 0u16..3 {
            stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(i, 0)));
            stream.extend(make_record(TagId::ParaText, 0, &para_text_data("text")));
        }

        let result = parse_body_text(&stream, &version()).unwrap();
        assert_eq!(result.paragraphs.len(), 3);
    }

    #[test]
    fn page_def_record_is_captured() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::PageDef, 0, &page_def_data()));
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));

        let result = parse_body_text(&stream, &version()).unwrap();
        assert!(result.page_def.is_some());
        let pd = result.page_def.unwrap();
        assert_eq!(pd.width, 59535);
        assert_eq!(pd.height, 84180);
    }

    #[test]
    fn unknown_tag_produces_warning_no_error() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::Unknown(0xAB), 0, &[0x01, 0x02]));

        let result = parse_body_text(&stream, &version()).unwrap();
        assert_eq!(result.warnings.len(), 1);
        match &result.warnings[0] {
            Hwp5Warning::UnsupportedTag { tag_id, .. } => assert_eq!(*tag_id, 0xAB),
            _ => panic!("expected UnsupportedTag"),
        }
    }

    #[test]
    fn para_header_without_para_text_gives_empty_text() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        // No ParaText record follows.

        let result = parse_body_text(&stream, &version()).unwrap();
        assert_eq!(result.paragraphs.len(), 1);
        assert_eq!(result.paragraphs[0].text, "");
        assert!(result.paragraphs[0].char_shape_runs.is_empty());
    }

    #[test]
    fn ctrl_header_table_ctrl_id_produces_table_control() {
        // ctrl_id = 0x74626C20 = 'tbl '
        let ctrl_id: u32 = CTRL_ID_TABLE;

        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        // CtrlHeader at level 0
        stream.extend(make_record(TagId::CtrlHeader, 0, &ctrl_header_data(ctrl_id)));
        // Table record as child (level 1)
        stream.extend(make_record(TagId::Table, 1, &basic_table_data(3, 4)));

        let result = parse_body_text(&stream, &version()).unwrap();
        assert_eq!(result.paragraphs.len(), 1);

        let para = &result.paragraphs[0];
        assert_eq!(para.controls.len(), 1);
        match &para.controls[0] {
            Hwp5Control::Table(table) => {
                assert_eq!(table.rows, 3);
                assert_eq!(table.cols, 4);
                assert_eq!(table.page_break, Hwp5TablePageBreak::None);
                assert!(!table.repeat_header);
                assert_eq!(table.cell_spacing, 0);
                assert_eq!(table.border_fill_id, None);
                assert!(table.cells.is_empty());
            }
            other => panic!("expected Table, got {:?}", other),
        }
    }

    #[test]
    fn ctrl_header_unknown_ctrl_id_produces_unknown_control() {
        let ctrl_id: u32 = 0x666F_6F20; // 'foo '

        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &ctrl_header_data(ctrl_id)));

        let result = parse_body_text(&stream, &version()).unwrap();
        assert_eq!(result.paragraphs.len(), 1);

        let para = &result.paragraphs[0];
        assert_eq!(para.controls.len(), 1);
        match &para.controls[0] {
            Hwp5Control::Unknown { ctrl_id: id } => assert_eq!(*id, ctrl_id),
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn header_control_captures_nested_paragraphs_after_list_header() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &ctrl_header_data(CTRL_ID_HEADER)));
        stream.extend(make_record(TagId::ListHeader, 1, &[0u8; 4]));
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_data("header text")));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        match &para.controls[0] {
            Hwp5Control::Header(subtree) => {
                assert_eq!(subtree.ctrl_id, CTRL_ID_HEADER);
                assert_eq!(subtree.paragraphs.len(), 1);
                assert_eq!(subtree.paragraphs[0].text, "header text");
            }
            other => panic!("expected Header, got {:?}", other),
        }
    }

    #[test]
    fn footer_control_captures_nested_paragraphs_after_list_header() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &ctrl_header_data(CTRL_ID_FOOTER)));
        stream.extend(make_record(TagId::ListHeader, 1, &[0u8; 4]));
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_data("footer text")));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        match &para.controls[0] {
            Hwp5Control::Footer(subtree) => {
                assert_eq!(subtree.ctrl_id, CTRL_ID_FOOTER);
                assert_eq!(subtree.paragraphs.len(), 1);
                assert_eq!(subtree.paragraphs[0].text, "footer text");
            }
            other => panic!("expected Footer, got {:?}", other),
        }
    }

    #[test]
    fn textbox_control_requires_shape_rectangle_and_list_header() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &gso_ctrl_header_data(15, 25, 7000, 5000)));
        stream.extend(make_record(TagId::ShapeComponentRect, 1, &[0u8; 4]));
        stream.extend(make_record(TagId::ListHeader, 1, &[0u8; 4]));
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_data("textbox text")));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        match &para.controls[0] {
            Hwp5Control::TextBox(textbox) => {
                assert_eq!(textbox.ctrl_id, CTRL_ID_GSO);
                assert_eq!(textbox.geometry.x, 15);
                assert_eq!(textbox.geometry.y, 25);
                assert_eq!(textbox.geometry.width, 7000);
                assert_eq!(textbox.geometry.height, 5000);
                assert_eq!(textbox.paragraphs.len(), 1);
                assert_eq!(textbox.paragraphs[0].text, "textbox text");
            }
            other => panic!("expected TextBox, got {:?}", other),
        }
    }

    #[test]
    fn gso_without_shape_rectangle_stays_unknown() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &ctrl_header_data(CTRL_ID_GSO)));
        stream.extend(make_record(TagId::ListHeader, 1, &[0u8; 4]));
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_data("not textbox")));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        match &para.controls[0] {
            Hwp5Control::Unknown { ctrl_id } => assert_eq!(*ctrl_id, CTRL_ID_GSO),
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn multiple_paragraphs_independent_controls() {
        let mut stream = Vec::new();
        // Para 0: has a table
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &ctrl_header_data(CTRL_ID_TABLE)));
        stream.extend(make_record(TagId::Table, 1, &basic_table_data(2, 2)));
        // Para 1: plain text
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(1, 0)));
        stream.extend(make_record(TagId::ParaText, 0, &para_text_data("hello")));

        let result = parse_body_text(&stream, &version()).unwrap();
        assert_eq!(result.paragraphs.len(), 2);
        assert_eq!(result.paragraphs[0].controls.len(), 1);
        assert_eq!(result.paragraphs[1].controls.len(), 0);
        assert_eq!(result.paragraphs[1].text, "hello");
    }

    #[test]
    fn table_cell_paragraphs_are_nested_under_table_control() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 0, &para_text_with_control_ref("", "")));
        stream.extend(make_record(TagId::CtrlHeader, 0, &ctrl_header_data(CTRL_ID_TABLE)));
        stream.extend(make_record(TagId::Table, 1, &basic_table_data(1, 1)));
        stream.extend(make_record(
            TagId::ListHeader,
            1,
            &list_header_table_cell_data(TestCellSpec {
                paragraph_count: 1,
                legacy_u16_count: false,
                properties: 0x20,
                column: 0,
                row: 0,
                col_span: 1,
                row_span: 1,
                width: 4000,
                height: 1000,
                margin: Hwp5TableCellMargin { left: 0, right: 0, top: 0, bottom: 0 },
                border_fill_id: Some(7),
            }),
        ));
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_data("cell text")));
        stream.extend(make_record(TagId::ParaCharShape, 2, &char_shape_run_data(0, 3)));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        match &para.controls[0] {
            Hwp5Control::Table(table) => {
                assert_eq!(table.rows, 1);
                assert_eq!(table.cols, 1);
                assert_eq!(table.cells.len(), 1);
                assert_eq!(table.cells[0].column, 0);
                assert_eq!(table.cells[0].row, 0);
                assert_eq!(table.cells[0].border_fill_id, Some(7));
                assert_eq!(table.cells[0].paragraphs.len(), 1);
                assert_eq!(table.cells[0].paragraphs[0].text, "cell text");
            }
            other => panic!("expected Table, got {:?}", other),
        }
    }

    #[test]
    fn orphaned_nested_table_emits_parser_fallback_warning() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 0, &para_text_with_control_ref("", "")));
        stream.extend(make_record(TagId::CtrlHeader, 0, &ctrl_header_data(CTRL_ID_TABLE)));
        stream.extend(make_record(TagId::Table, 1, &basic_table_data(1, 1)));
        stream.extend(make_record(
            TagId::ListHeader,
            1,
            &list_header_table_cell_data(TestCellSpec {
                paragraph_count: 1,
                legacy_u16_count: false,
                properties: 0x20,
                column: 0,
                row: 0,
                col_span: 1,
                row_span: 1,
                width: 4000,
                height: 1000,
                margin: Hwp5TableCellMargin { left: 0, right: 0, top: 0, bottom: 0 },
                border_fill_id: Some(7),
            }),
        ));
        // Malformed ordering: nested table opens before the parent cell paragraph starts.
        stream.extend(make_record(TagId::CtrlHeader, 1, &ctrl_header_data(CTRL_ID_TABLE)));
        stream.extend(make_record(TagId::Table, 2, &basic_table_data(1, 1)));
        // Force both tables to close.
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(1, 0)));

        let result = parse_body_text(&stream, &version()).unwrap();
        assert!(result.warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::ParserFallback { subject, reason }
                if *subject == "table.nested_attach"
                    && reason == "orphaned_nested_table_without_parent_paragraph"
        )));
    }

    #[test]
    fn parse_table_cell_recovers_margin_and_vertical_align_from_standard_payload() {
        let (paragraph_count, cell) =
            parse_table_cell(&list_header_table_cell_data(TestCellSpec {
                paragraph_count: 2,
                legacy_u16_count: false,
                properties: 0x20, // bits 5..6 = 1 => center
                column: 1,
                row: 2,
                col_span: 1,
                row_span: 1,
                width: 5000,
                height: 2400,
                margin: Hwp5TableCellMargin { left: 15, right: 20, top: 10, bottom: 5 },
                border_fill_id: Some(7),
            }))
            .expect("standard table cell should parse");

        assert_eq!(paragraph_count, 2);
        assert_eq!(cell.margin, Hwp5TableCellMargin { left: 15, right: 20, top: 10, bottom: 5 });
        assert_eq!(cell.vertical_align, Hwp5TableCellVerticalAlign::Center);
        assert_eq!(cell.border_fill_id, Some(7));
    }

    #[test]
    fn parse_table_cell_accepts_legacy_30_byte_variant() {
        let (paragraph_count, cell) =
            parse_table_cell(&list_header_table_cell_data(TestCellSpec {
                paragraph_count: 1,
                legacy_u16_count: true,
                properties: 0x40, // bits 5..6 = 2 => bottom
                column: 0,
                row: 0,
                col_span: 1,
                row_span: 1,
                width: 1000,
                height: 900,
                margin: Hwp5TableCellMargin { left: 15, right: 20, top: 10, bottom: 5 },
                border_fill_id: None,
            }))
            .expect("legacy table cell should parse");

        assert_eq!(paragraph_count, 1);
        assert_eq!(cell.margin, Hwp5TableCellMargin { left: 15, right: 20, top: 10, bottom: 5 });
        assert_eq!(cell.vertical_align, Hwp5TableCellVerticalAlign::Bottom);
        assert_eq!(cell.border_fill_id, None);
    }

    #[test]
    fn para_line_seg_is_silently_skipped() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 0, &para_text_data("ok")));
        // ParaLineSeg should be silently skipped.
        stream.extend(make_record(TagId::ParaLineSeg, 0, &[0u8; 16]));

        let result = parse_body_text(&stream, &version()).unwrap();
        assert_eq!(result.paragraphs.len(), 1);
        assert_eq!(result.paragraphs[0].text, "ok");
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn text_segments_rendering() {
        // Tab → \t, NonBreakingSpace → ' ', LineBreak → \n
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(
            &"A".encode_utf16().flat_map(|c| c.to_le_bytes()).collect::<Vec<_>>(),
        );
        data.extend_from_slice(&0x09u16.to_le_bytes()); // Tab control
        for extra in [1u16, 2, 3, 4, 5, 6, 7] {
            data.extend_from_slice(&extra.to_le_bytes());
        }
        data.extend_from_slice(
            &"B".encode_utf16().flat_map(|c| c.to_le_bytes()).collect::<Vec<_>>(),
        );
        data.extend_from_slice(&0x1Eu16.to_le_bytes()); // NonBreakingSpace
        data.extend_from_slice(
            &"C".encode_utf16().flat_map(|c| c.to_le_bytes()).collect::<Vec<_>>(),
        );

        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 0, &data));

        let result = parse_body_text(&stream, &version()).unwrap();
        assert_eq!(result.paragraphs[0].text, "A\tB C");
    }

    #[test]
    fn parse_ctrl_id_helper_little_endian() {
        let data = CTRL_ID_TABLE.to_le_bytes();
        assert_eq!(parse_ctrl_id(&data), CTRL_ID_TABLE);
    }

    #[test]
    fn parse_ctrl_id_helper_short_data_returns_zero() {
        assert_eq!(parse_ctrl_id(&[0x01, 0x02]), 0);
    }

    #[test]
    fn parse_table_counts_helper() {
        let data = basic_table_data(5, 7);
        let header = parse_table_header(&data);
        assert_eq!(header.rows, 5);
        assert_eq!(header.cols, 7);
    }

    #[test]
    fn parse_table_counts_short_data_returns_zero() {
        let header = parse_table_header(&[0u8; 3]);
        assert_eq!(header.rows, 0);
        assert_eq!(header.cols, 0);
    }

    #[test]
    fn parse_table_header_recovers_page_break_repeat_header_spacing_and_border_fill() {
        let data = table_data(TestTableSpec {
            rows: 4,
            cols: 3,
            page_break_bits: 2,
            repeat_header: true,
            cell_spacing: 120,
            row_metadata: vec![1, 1, 1, 1],
            border_fill_id: Some(9),
        });

        let header = parse_table_header(&data);
        assert_eq!(header.rows, 4);
        assert_eq!(header.cols, 3);
        assert_eq!(header.page_break, Hwp5TablePageBreak::Cell);
        assert!(header.repeat_header);
        assert_eq!(header.cell_spacing, 120);
        assert_eq!(header.border_fill_id, Some(9));
    }

    #[test]
    fn fixture_repeat_header_multi_page_cell_properties_preserve_header_flag() {
        let single_on =
            table_cell_list_header_properties_from_fixture("table_06_repeat_header_row.hwp");
        let single_off =
            table_cell_list_header_properties_from_fixture("table_06b_no_repeat_header_row.hwp");
        let on = table_cell_list_header_properties_from_fixture(
            "table_06c_repeat_header_multi_page.hwp",
        );
        let off = table_cell_list_header_properties_from_fixture(
            "table_06d_no_repeat_header_multi_page.hwp",
        );
        assert!(single_on.iter().all(|properties| (properties & TABLE_CELL_HEADER_FLAG) == 0));
        assert!(single_off.iter().all(|properties| (properties & TABLE_CELL_HEADER_FLAG) == 0));
        assert!(on.iter().take(3).all(|properties| (properties & TABLE_CELL_HEADER_FLAG) != 0));
        assert!(off.iter().take(3).all(|properties| (properties & TABLE_CELL_HEADER_FLAG) != 0));
        assert!(on.iter().skip(3).all(|properties| (properties & TABLE_CELL_HEADER_FLAG) == 0));
        assert!(off.iter().skip(3).all(|properties| (properties & TABLE_CELL_HEADER_FLAG) == 0));
    }

    #[test]
    fn gso_shape_picture_becomes_image_control() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 0, &para_text_with_control_ref("", "")));
        stream.extend(make_record(
            TagId::CtrlHeader,
            0,
            &gso_ctrl_header_data(-120, 240, 6400, 3200),
        ));
        stream.extend(make_record(TagId::ShapeComponent, 1, &[0u8; 4]));
        stream.extend(make_record(TagId::ShapePicture, 1, &shape_picture_data(7)));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        assert_eq!(para.controls.len(), 1);
        match &para.controls[0] {
            Hwp5Control::Image(image) => {
                assert_eq!(image.ctrl_id, CTRL_ID_GSO);
                assert_eq!(image.geometry.x, -120);
                assert_eq!(image.geometry.y, 240);
                assert_eq!(image.geometry.width, 6400);
                assert_eq!(image.geometry.height, 3200);
                assert_eq!(image.binary_data_id, 7);
            }
            other => panic!("expected Image, got {:?}", other),
        }
    }

    #[test]
    fn textbox_subtree_keeps_nested_image_control() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &gso_ctrl_header_data(0, 0, 8000, 6000)));
        stream.extend(make_record(TagId::ShapeComponentRect, 1, &[0u8; 4]));
        stream.extend(make_record(TagId::ListHeader, 1, &[0u8; 4]));
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_with_control_ref("앞", "뒤")));
        stream.extend(make_record(TagId::CtrlHeader, 2, &gso_ctrl_header_data(10, 20, 3000, 4000)));
        stream.extend(make_record(TagId::ShapeComponent, 3, &[0u8; 4]));
        stream.extend(make_record(TagId::ShapePicture, 3, &shape_picture_data(2)));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        match &para.controls[0] {
            Hwp5Control::TextBox(textbox) => {
                assert_eq!(textbox.geometry.width, 8000);
                assert_eq!(textbox.geometry.height, 6000);
                assert_eq!(textbox.paragraphs.len(), 1);
                assert_eq!(textbox.paragraphs[0].text, "앞\u{FFFC}뒤");
                assert_eq!(textbox.paragraphs[0].controls.len(), 1);
                match &textbox.paragraphs[0].controls[0] {
                    Hwp5Control::Image(image) => {
                        assert_eq!(image.binary_data_id, 2);
                        assert_eq!(image.geometry.x, 10);
                        assert_eq!(image.geometry.y, 20);
                    }
                    other => panic!("expected nested Image, got {:?}", other),
                }
            }
            other => panic!("expected TextBox, got {:?}", other),
        }
    }

    #[test]
    fn table_cell_keeps_image_control_inside_cell_paragraph() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &ctrl_header_data(CTRL_ID_TABLE)));
        stream.extend(make_record(TagId::Table, 1, &basic_table_data(1, 1)));
        stream.extend(make_record(
            TagId::ListHeader,
            1,
            &list_header_table_cell_data(TestCellSpec {
                paragraph_count: 1,
                legacy_u16_count: false,
                properties: 0x20,
                column: 0,
                row: 0,
                col_span: 1,
                row_span: 1,
                width: 4000,
                height: 1000,
                margin: Hwp5TableCellMargin { left: 0, right: 0, top: 0, bottom: 0 },
                border_fill_id: None,
            }),
        ));
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_with_control_ref("", "")));
        stream.extend(make_record(TagId::CtrlHeader, 2, &gso_ctrl_header_data(1, 2, 300, 400)));
        stream.extend(make_record(TagId::ShapeComponent, 3, &[0u8; 4]));
        stream.extend(make_record(TagId::ShapePicture, 3, &shape_picture_data(9)));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        match &para.controls[0] {
            Hwp5Control::Table(table) => {
                assert_eq!(table.cells.len(), 1);
                let cell_para = &table.cells[0].paragraphs[0];
                assert_eq!(cell_para.controls.len(), 1);
                match &cell_para.controls[0] {
                    Hwp5Control::Image(image) => {
                        assert_eq!(image.binary_data_id, 9);
                        assert_eq!(image.geometry.width, 300);
                        assert_eq!(image.geometry.height, 400);
                    }
                    other => panic!("expected cell Image, got {:?}", other),
                }
            }
            other => panic!("expected Table, got {:?}", other),
        }
    }

    #[test]
    fn gso_shape_component_ole_becomes_ole_object_control() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 0, &para_text_with_control_ref("", "")));
        stream.extend(make_record(TagId::CtrlHeader, 0, &gso_ctrl_header_data(30, 40, 5000, 6000)));
        stream.extend(make_record(TagId::ShapeComponent, 1, &[0u8; 4]));
        stream.extend(make_record(
            TagId::ShapeComponentOle,
            1,
            &shape_component_ole_data(1, 9100, 8200),
        ));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        assert_eq!(para.controls.len(), 1);
        match &para.controls[0] {
            Hwp5Control::OleObject(ole) => {
                assert_eq!(ole.ctrl_id, CTRL_ID_GSO);
                assert_eq!(ole.geometry.x, 30);
                assert_eq!(ole.geometry.y, 40);
                assert_eq!(ole.geometry.width, 5000);
                assert_eq!(ole.geometry.height, 6000);
                assert_eq!(ole.binary_data_id, 1);
                assert_eq!(ole.extent_width, 9100);
                assert_eq!(ole.extent_height, 8200);
            }
            other => panic!("expected OleObject, got {:?}", other),
        }
    }

    #[test]
    fn gso_shape_component_line_becomes_line_control() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 0, &para_text_with_control_ref("", "")));
        stream.extend(make_record(
            TagId::CtrlHeader,
            0,
            &gso_ctrl_header_data(9_884, 11_980, 29_360, 0),
        ));
        stream.extend(make_record(TagId::ShapeComponent, 1, &[0u8; 4]));
        stream.extend(make_record(
            TagId::ShapeComponentLine,
            1,
            &shape_component_line_data(0, 0, 100, 100),
        ));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        assert_eq!(para.controls.len(), 1);
        match &para.controls[0] {
            Hwp5Control::Line(line) => {
                assert_eq!(line.ctrl_id, CTRL_ID_GSO);
                assert_eq!(line.geometry.x, 9_884);
                assert_eq!(line.geometry.y, 11_980);
                assert_eq!(line.geometry.width, 29_360);
                assert_eq!(line.geometry.height, 0);
                assert_eq!(line.start, Hwp5ShapePoint { x: 0, y: 0 });
                assert_eq!(line.end, Hwp5ShapePoint { x: 100, y: 100 });
            }
            other => panic!("expected Line, got {:?}", other),
        }
    }

    #[test]
    fn gso_shape_component_polygon_becomes_polygon_control() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 0, &para_text_with_control_ref("", "")));
        stream.extend(make_record(
            TagId::CtrlHeader,
            0,
            &gso_ctrl_header_data(17_804, 13_900, 12_560, 13_040),
        ));
        stream.extend(make_record(TagId::ShapeComponent, 1, &[0u8; 4]));
        stream.extend(make_record(
            TagId::ShapeComponentPolygon,
            1,
            &shape_component_polygon_data(&[
                (1_882, 0),
                (0, 1_405),
                (732, 3_675),
                (3_032, 3_675),
                (3_765, 1_405),
                (1_882, 0),
            ]),
        ));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        assert_eq!(para.controls.len(), 1);
        match &para.controls[0] {
            Hwp5Control::Polygon(polygon) => {
                assert_eq!(polygon.ctrl_id, CTRL_ID_GSO);
                assert_eq!(polygon.geometry.x, 17_804);
                assert_eq!(polygon.geometry.y, 13_900);
                assert_eq!(polygon.geometry.width, 12_560);
                assert_eq!(polygon.geometry.height, 13_040);
                assert_eq!(polygon.points.len(), 6);
                assert_eq!(polygon.points[0], Hwp5ShapePoint { x: 1_882, y: 0 });
                assert_eq!(polygon.points[5], Hwp5ShapePoint { x: 1_882, y: 0 });
            }
            other => panic!("expected Polygon, got {:?}", other),
        }
    }

    #[test]
    fn pure_rect_gso_is_preserved_as_rect_evidence_and_not_treated_as_textbox() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 0, &para_text_with_control_ref("", "")));
        stream.extend(make_record(
            TagId::CtrlHeader,
            0,
            &gso_ctrl_header_data(10_764, 11_020, 10_240, 10_640),
        ));
        stream.extend(make_record(TagId::ShapeComponent, 1, &[0u8; 4]));
        stream.extend(make_record(TagId::ShapeComponentRect, 1, &[0u8; 4]));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        assert_eq!(para.controls.len(), 1);
        match &para.controls[0] {
            Hwp5Control::Rect(rect) => {
                assert_eq!(rect.ctrl_id, CTRL_ID_GSO);
                assert_eq!(rect.geometry.x, 10_764);
                assert_eq!(rect.geometry.y, 11_020);
                assert_eq!(rect.geometry.width, 10_240);
                assert_eq!(rect.geometry.height, 10_640);
            }
            other => panic!("expected Rect evidence for pure rect gso, got {:?}", other),
        }
    }

    #[test]
    fn inline_gso_with_picture_and_ole_stays_unknown() {
        let geometry = crate::schema::section::Hwp5ShapeComponentGeometry {
            x: 10,
            y: 20,
            width: 5_000,
            height: 6_000,
        };
        let mut ctx = InlineGsoContext::new(0, CTRL_ID_GSO, Some(geometry));
        ctx.note_shape_component();
        ctx.note_shape_picture(Hwp5ShapePicture::parse(&shape_picture_data(1)).unwrap());
        ctx.note_shape_ole(
            Hwp5ShapeComponentOle::parse(&shape_component_ole_data(1, 9000, 8000)).unwrap(),
        );

        match ctx.into_control() {
            Hwp5Control::Unknown { ctrl_id } => assert_eq!(ctrl_id, CTRL_ID_GSO),
            other => panic!("expected Unknown for ambiguous gso payload, got {:?}", other),
        }
    }

    #[test]
    fn nested_subtree_gso_with_picture_and_ole_stays_unknown() {
        let geometry = crate::schema::section::Hwp5ShapeComponentGeometry {
            x: 10,
            y: 20,
            width: 5_000,
            height: 6_000,
        };
        let mut ctx = NestedSubtreeContext::new(0, CTRL_ID_GSO, Some(geometry));
        ctx.note_shape_component();
        ctx.note_shape_picture(Hwp5ShapePicture::parse(&shape_picture_data(1)).unwrap());
        ctx.note_shape_ole(
            Hwp5ShapeComponentOle::parse(&shape_component_ole_data(1, 9000, 8000)).unwrap(),
        );

        match ctx.into_control() {
            Hwp5Control::Unknown { ctrl_id } => assert_eq!(ctrl_id, CTRL_ID_GSO),
            other => panic!("expected Unknown for ambiguous subtree gso payload, got {:?}", other),
        }
    }
}
