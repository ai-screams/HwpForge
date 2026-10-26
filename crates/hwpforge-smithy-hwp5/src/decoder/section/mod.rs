//! `BodyText/Section{N}` stream decoder for HWP5.
//!
//! Reads binary paragraph and run records from each section stream,
//! producing an intermediate representation that the projection layer
//! converts into Core's `Section` and `Paragraph` types.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io::Cursor;

use crate::decoder::Hwp5Warning;
use crate::error::Hwp5Result;
use crate::schema::header::HwpVersion;
use crate::schema::record::{Record, TagId};
use crate::schema::section::{
    Hwp5CharShapeRun, Hwp5ComposeControl, Hwp5EqEdit, Hwp5PageDef, Hwp5ParaHeader, Hwp5ParaLineSeg,
    Hwp5ParaText, Hwp5ShapeComponentCurve, Hwp5ShapeComponentEllipse, Hwp5ShapeComponentGeometry,
    Hwp5ShapeComponentLine, Hwp5ShapeComponentOle, Hwp5ShapeComponentPolygon, Hwp5ShapePicture,
    Hwp5ShapeTextArt, TextSegment,
};

mod gso;
mod master;
mod model;

use gso::*;
pub(crate) use model::*;
// ---------------------------------------------------------------------------
// ctrl_id constants — consolidated in `crate::ctrl_ids` (#94 Step B1)
// ---------------------------------------------------------------------------

use crate::ctrl_ids::{
    CTRL_ID_ATNO, CTRL_ID_CLICK_HERE, CTRL_ID_COLUMN_DEF, CTRL_ID_COMPOSE, CTRL_ID_DUTMAL,
    CTRL_ID_ENDNOTE, CTRL_ID_EQED, CTRL_ID_FIELD_CROSSREF, CTRL_ID_FIELD_DATE_CODE,
    CTRL_ID_FIELD_PATH, CTRL_ID_FIELD_SUMMERY, CTRL_ID_FOOTER, CTRL_ID_FOOTNOTE, CTRL_ID_GSO,
    CTRL_ID_HEADER, CTRL_ID_INDEXMARK, CTRL_ID_MEMO, CTRL_ID_NEW_NUMBER, CTRL_ID_PAGE_HIDING,
    CTRL_ID_SECD, CTRL_ID_TABLE,
};

/// `ShapeComponent` (`0x4C`) type tag identifying a connect line, stored as the
/// little-endian bytes for `"$col"`. 한컴 reuses the `ShapeComponentLine`
/// (`0x4E`) sub-record for both plain lines and connectors, so this 4-byte tag
/// in the `ShapeComponent` header is the only discriminator (confirmed against
/// `$rec`/`$ell`/`$cur` for rect/ellipse/curve from 한컴 truth fixtures).
///
/// Not a `ctrl_id` — `[u8; 4]` shape-component type tag (different wire role),
/// so stays here rather than moving to `crate::ctrl_ids`.
const SHAPE_COMPONENT_TYPE_CONNECT_LINE: [u8; 4] = [0x6C, 0x6F, 0x63, 0x24];

/// `ShapeComponent` (`0x4C`) type tag identifying a group container (묶음
/// 객체 / 개체 묶기), stored as the little-endian bytes for `"$con"`. Probed
/// from the native fixture `sample-gso-group.hwp` (the leading 4 bytes of the
/// `$con` `ShapeComponent` decode to `[0x6E, 0x6F, 0x63, 0x24]`). Shares the
/// same discriminator mechanism as `$col` (connect line) and `$rec`/`$ell`/
/// `$cur` (rect/ellipse/curve).
///
/// Not a `ctrl_id` — a `[u8; 4]` shape-component type tag — so it stays here
/// rather than in `crate::ctrl_ids`.
const SHAPE_COMPONENT_TYPE_GROUP: [u8; 4] = [0x6E, 0x6F, 0x63, 0x24];

/// `ShapeComponent` (`0x4C`) type tag identifying a TextArt (글맵시) object,
/// stored as the little-endian bytes for `"$tat"`. Probed from native 한컴
/// fixtures. Shares the same discriminator mechanism as `$con` (group) and
/// `$col` (connect line).
///
/// Not a `ctrl_id` — a `[u8; 4]` shape-component type tag — so it stays here
/// rather than in `crate::ctrl_ids`.
const SHAPE_COMPONENT_TYPE_TEXTART: [u8; 4] = [0x74, 0x61, 0x74, 0x24]; // "$tat"

/// Maximum group nesting depth the decoder will descend before degrading.
///
/// Wave A only carries FLAT groups (one `$con` level), but the cap is wired
/// now so Wave B (recursive `$con`-in-`$con`) is a small lift. Mirrors the
/// defense-in-depth caps in `schema/summary_info.rs` — on exceed we emit a
/// warning and degrade rather than recurse or panic.
const GSO_GROUP_MAX_DEPTH: u16 = 16;

/// Maximum nesting of shapes in shapes' text (a text box in a text box …),
/// the same budget as [`GSO_GROUP_MAX_DEPTH`]: the collection recurses once
/// per level, so a stream nesting them without bound would recurse without
/// bound. A deeper shape is skipped and reported (`shape_text_depth`).
pub(super) const SHAPE_TEXT_MAX_DEPTH: u16 = GSO_GROUP_MAX_DEPTH;

/// Global container path limit, shared by tables, notes, shapes, groups and
/// group child text. The original table limit remains the overall ceiling.
const MAX_CONTROL_NESTING: usize = hwpforge_core::MAX_NESTING_DEPTH;

/// One path budget inherited across *all* container kinds, including group
/// children. No nested parser creates a fresh budget. Family caps remain for
/// compatibility, but changing kinds cannot reset the total depth.
#[derive(Clone, Copy, Default)]
struct NestingBudget {
    total: usize,
    shapes: u16,
    groups: u16,
}

impl NestingBudget {
    fn enter(self, shape: bool, group: bool) -> Option<Self> {
        let next = Self {
            total: self.total + 1,
            shapes: self.shapes + u16::from(shape),
            groups: self.groups + u16::from(group),
        };
        (next.total <= MAX_CONTROL_NESTING
            && next.shapes <= SHAPE_TEXT_MAX_DEPTH + 2
            && next.groups <= GSO_GROUP_MAX_DEPTH)
            .then_some(next)
    }
}

#[derive(Clone, Copy)]
enum ControlHost {
    Cell,
    Subtree,
}

struct NestedParagraphControl {
    level: u16,
    host: ControlHost,
    parser: Box<BodyTextParserState>,
}

const TABLE_CELL_HEADER_FLAG: u32 = 0x0004_0000;

// The `0x57 lvl=2` sub-record that follows every `%clk` CtrlHeader
// carries the form-mode field name and HWP5 names it `CtrlData`. The
// dispatch arm matches `TagId::CtrlData` directly (no numeric constant
// needed) — keeping a parallel constant would only invite drift.

// Memo wire command parsing now lives on `Hwp5MemoCommand::parse` in
// `crate::schema::section` — shared with the slash-command util that future
// `%hlk` / `%xrf` / `%bmk` parsers can reuse.

// ---------------------------------------------------------------------------
// Parser state
// ---------------------------------------------------------------------------

/// Mutable accumulator for a paragraph being assembled.
struct ParaBuf {
    header: Hwp5ParaHeader,
    text: Option<Hwp5ParaText>,
    char_shape_runs: Vec<Hwp5CharShapeRun>,
    line_segments: Vec<Hwp5ParaLineSeg>,
    controls: Vec<Hwp5Control>,
}

impl ParaBuf {
    fn new(header: Hwp5ParaHeader) -> Self {
        Self {
            header,
            text: None,
            char_shape_runs: Vec::new(),
            line_segments: Vec::new(),
            controls: Vec::new(),
        }
    }

    /// Build the final `Hwp5Paragraph`, consuming this buffer. Its char
    /// shape boundaries move from wire positions to positions in `text`.
    fn finish(self, warnings: &mut Vec<Hwp5Warning>) -> Hwp5Paragraph {
        let (text_segments, silent_wires) = self.text.map_or_else(
            || (Vec::new(), Vec::new()),
            |paragraph_text| (paragraph_text.segments, paragraph_text.silent_wires),
        );
        let text = segments_to_string(&text_segments);
        let (body_runs, end_char_shape) =
            split_end_mark_shape(&text_segments, &silent_wires, self.char_shape_runs);
        let char_shape_runs =
            char_shape_runs_to_text_positions(&text_segments, &silent_wires, &body_runs, warnings);
        Hwp5Paragraph {
            silent_wires,
            text,
            text_segments,
            para_shape_id: self.header.para_shape_id,
            style_id: self.header.style_id,
            // divide_sort bit2/bit3 = 쪽/단 나누기 (hwp-rs 확증 + F2 실측:
            // 한컴 재저장 HWPX pageBreak="1" 대응 — W3 carry 시작).
            page_break: self.header.divide_sort & 0x04 != 0,
            column_break: self.header.divide_sort & 0x08 != 0,
            char_shape_runs,
            line_segments: self.line_segments,
            controls: self.controls,
            end_char_shape,
        }
    }
}

/// The wire length and text length of one text segment.
fn segment_lengths(segment: &TextSegment) -> (u32, u32) {
    match segment {
        TextSegment::Text(text) => {
            let units = text.encode_utf16().count() as u32;
            (units, units)
        }
        TextSegment::LineBreak | TextSegment::NonBreakingSpace | TextSegment::FwSpace => (1, 1),
        TextSegment::Tab { .. }
        | TextSegment::ControlRef { .. }
        | TextSegment::ExtendedControlRef { .. } => (8, 1),
        TextSegment::SectionColumnDef { .. }
        | TextSegment::FieldBegin { .. }
        | TextSegment::FieldEnd => (8, 0),
        TextSegment::ParaBreak => (1, 0),
    }
}

/// Takes the paragraph-end mark's char shape out of the wire boundaries
/// (§19.22 D5, S17), before they move to text positions: the entry placed
/// exactly at the wire position of the paragraph-end character (the last
/// segment, `ParaBreak`) — the last one there when several share it.
/// Whether it differs from the last character is decided at projection,
/// against the last run actually written (review r1: the last wire boundary
/// can belong to something that writes no run, such as a field end). Entries
/// past the end character are left to the conversion. An empty paragraph
/// (no segment but the end character, or none) keeps its boundary: its one
/// run takes the shape at position 0 (§19.22 B).
///
/// 이것을 실패시키는 것: 변환 뒤 위치로 판정 / 끝 경계를 본문 목록에 남기기 /
/// 빈 문단에서도 판정 / 문자 수로 셈(`end_mark_shape_is_read_from_the_wire`).
fn split_end_mark_shape(
    segments: &[TextSegment],
    silent_wires: &[crate::schema::section::SilentWire],
    runs: Vec<Hwp5CharShapeRun>,
) -> (Vec<Hwp5CharShapeRun>, Option<u32>) {
    let Some((TextSegment::ParaBreak, body)) = segments.split_last() else {
        return (runs, None);
    };
    if body.is_empty() {
        return (runs, None);
    }
    let mut wire = 0u32;
    let mut silent = silent_wires.iter().peekable();
    for segment in body {
        while let Some(skipped) = silent.next_if(|skipped| skipped.start <= wire) {
            wire = wire.saturating_add(skipped.len);
        }
        wire = wire.saturating_add(segment_lengths(segment).0);
    }
    while let Some(skipped) = silent.next_if(|skipped| skipped.start <= wire) {
        wire = wire.saturating_add(skipped.len);
    }
    let end = wire;
    let Some(end_shape) = runs.iter().rev().find(|r| r.position == end).map(|r| r.char_shape_id)
    else {
        return (runs, None);
    };
    let body_runs: Vec<_> = runs.into_iter().filter(|r| r.position != end).collect();
    (body_runs, Some(end_shape))
}

/// In-progress capture state for one `HWPTAG_MEMO_LIST` (0x5D) cluster.
///
/// The decoder enters capture mode when it sees `MemoList` at level 1 and
/// stays in it until either the next `MemoList` arrives (start of the next
/// memo) or a body `ParaHeader` at level 0 arrives (start of the next body
/// paragraph), at which point the captured paragraphs are flushed to
/// [`BodyTextParserState::memo_contents`].
///
/// The cluster wire is (records at level 1 are siblings of the body
/// paragraph's `ParaText`):
///
/// ```text
/// MemoList    lvl=1   (4-byte LE memo_id payload)
/// ListHeader  lvl=1
/// ParaHeader  lvl=1   (memo body content para — *not* a body paragraph)
/// ParaText    lvl=2
/// ParaCharShape lvl=2
/// ...
/// ```
struct MemoContentCapture {
    memo_id: u32,
    note_sources: Vec<master::NoteNumberingSource>,
    saw_list_header: bool,
    parser: Box<BodyTextParserState>,
}

impl MemoContentCapture {
    fn into_paragraphs(self, warnings: &mut Vec<Hwp5Warning>) -> Vec<Hwp5Paragraph> {
        let result = self.parser.finish();
        warnings.extend(result.warnings);
        result.paragraphs
    }
}

/// Active table control while walking nested child records.
struct TableContext {
    ctrl_depth: u16,
    table: Hwp5Table,
    seen_table_body: bool,
    current_cell: Option<ActiveTableCell>,
    current_cell_para: Option<ParaBuf>,
}

impl TableContext {
    fn new(ctrl_depth: u16, instance_id: u32) -> Self {
        Self {
            ctrl_depth,
            table: Hwp5Table {
                inner_margin: None,
                no_adjust: false,
                cell_zones: Vec::new(),
                caption: None,
                rows: 0,
                cols: 0,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: Vec::new(),
                instance_id,
                ctrl: None,
            },
            seen_table_body: false,
            current_cell: None,
            current_cell_para: None,
        }
    }

    fn flush_current_cell_paragraph(&mut self, warnings: &mut Vec<Hwp5Warning>) {
        let Some(buf) = self.current_cell_para.take() else {
            return;
        };
        if let Some(cell) = self.current_cell.as_mut() {
            cell.cell.paragraphs.push(buf.finish(warnings));
        }
    }

    fn finish_active_cell_if_ready(&mut self) {
        let should_finish = self.current_cell.as_ref().is_some_and(|cell| {
            cell.expected_paragraphs == 0 || cell.cell.paragraphs.len() >= cell.expected_paragraphs
        });
        if should_finish {
            self.finish_active_cell();
        }
    }

    /// Closes the open cell (or caption holder) whatever its count says.
    fn finish_active_cell(&mut self) {
        if let Some(cell) = self.current_cell.take() {
            match cell.caption {
                Some(mut caption) => {
                    caption.paragraphs = cell.cell.paragraphs;
                    self.table.caption = Some(caption);
                }
                None => self.table.cells.push(cell.cell),
            }
        }
    }

    fn finalize(mut self, warnings: &mut Vec<Hwp5Warning>) -> Hwp5Control {
        self.flush_current_cell_paragraph(warnings);
        self.finish_active_cell_if_ready();
        Hwp5Control::Table(self.table)
    }
}

/// Cell currently receiving nested paragraph records.
struct ActiveTableCell {
    expected_paragraphs: usize,
    cell: Hwp5TableCell,
    /// D: this "cell" collects the table caption's paragraphs (the caption
    /// list header comes before the `TABLE` record); finishing it fills
    /// [`Hwp5Table::caption`] instead of adding a cell.
    caption: Option<Hwp5TableCaption>,
}

impl ActiveTableCell {
    /// A holder for the caption's paragraphs, reusing the cell paragraph
    /// machinery (text, char shapes, line cache, inline controls).
    fn caption(expected_paragraphs: usize, caption: Hwp5TableCaption) -> Self {
        let margin = Hwp5TableCellMargin { left: 0, right: 0, top: 0, bottom: 0 };
        Self {
            expected_paragraphs,
            cell: Hwp5TableCell {
                column: 0,
                row: 0,
                col_span: 1,
                row_span: 1,
                width: 0,
                height: 0,
                margin,
                vertical_align: Hwp5TableCellVerticalAlign::Top,
                is_header: false,
                has_margin: false,
                line_wrap: 0,
                border_fill_id: None,
                paragraphs: Vec::new(),
            },
            caption: Some(caption),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NestedSubtreeKind {
    Header,
    Footer,
    Footnote,
    Endnote,
    TextBox,
}

/// Wave 12p Step 5 (#134): extract the CtrlHeader's `instance_id` field
/// using ctrl_id-family-aware offsets.
///
/// Earlier (Wave 12p Step 1b) we tried `data[n-8..n-4]` as a uniform
/// trailer, but wire audits (2026-06-09) on Hancom-native fixtures
/// (`sample-field-crossref-footnote/endnote/table-caption/eq-caption.hwp`)
/// proved the trailer offset only matches by coincidence. The real
/// `instance_id` lives at a fixed offset relative to ctrl_id (record-data
/// prefix), and the offset depends on the control family because
/// different ctrl families pack different sized header blocks before the
/// id. Verified offsets:
///
/// | ctrl_id family       | data.len | instance_id offset |
/// |----------------------|----------|--------------------|
/// | `fn  ` (footnote)    | 20       | `data[16..20]`     |
/// | `en  ` (endnote)     | 20       | `data[16..20]`     |
/// | `tbl ` (table)       | 46       | `data[36..40]`     |
/// | `gso ` (shape)       | (varies) | `data[36..40]`     |
/// | `eqed` (equation)    | 58       | `data[36..40]`     |
///
/// Other ctrl families (`head`, `foot`, `secd`, …) currently return 0 —
/// they do not appear as cross-ref targets in the 12-fixture matrix.
///
/// Returns `0` when the payload is too short to carry the field at the
/// expected offset, or when the family is unrecognized.
fn extract_ctrl_header_instance_id(data: &[u8], ctrl_id: u32) -> u32 {
    let offset = match ctrl_id {
        CTRL_ID_FOOTNOTE | CTRL_ID_ENDNOTE => 16,
        CTRL_ID_GSO | CTRL_ID_TABLE | CTRL_ID_EQED => 36,
        _ => return 0,
    };
    data.get(offset..offset + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
        .unwrap_or(0)
}

/// `CtrlHeader` bytes `[4..8]` — HWP 5.0 spec §4.3.10.3 표 140/70 공통
/// 개체 속성 DWORD (header/footer 는 applyPageType, gso 는 bit0=글자처럼
/// 취급 등 family 별 의미). The common control interpreter reads this word
/// identically in body paragraphs and every nested host.
fn ctrl_header_properties_raw(data: &[u8]) -> u32 {
    if data.len() >= 8 {
        u32::from_le_bytes([data[4], data[5], data[6], data[7]])
    } else {
        0
    }
}

/// Active non-table control while collecting a nested paragraph subtree.
struct NestedSubtreeContext {
    wire_order: usize,
    ctrl_depth: u16,
    ctrl_id: u32,
    /// Bytes [4..8] of the `CtrlHeader` payload (see
    /// [`Hwp5NestedSubtree::properties_raw`]).
    properties_raw: u32,
    /// CtrlHeader trailer instance ID (Wave 12p Step 1b). See
    /// [`Hwp5NestedSubtree::instance_id`].
    instance_id: u32,
    saw_list_header: bool,
    /// `속성` (UINT32) field of the gso/textbox `HWPTAG_LIST_HEADER` record
    /// (표 65). `None` until a ListHeader is seen. Bits 5–6 carry the text
    /// vertical alignment (`0=top`, `1=center`, `2=bottom`); projection
    /// extracts them for text-bearing shapes. Mirrors how `parse_table_cell`
    /// recovers the same field for table cells.
    list_header_properties: Option<u32>,
    text_margin: Option<crate::decoder::section::Hwp5TableCellMargin>,
    saw_shape_rectangle: bool,
    saw_shape_component: bool,
    geometry: Option<Hwp5ShapeComponentGeometry>,
    picture: Option<Hwp5ShapePicture>,
    ole: Option<Hwp5ShapeComponentOle>,
    line: Option<Hwp5ShapeComponentLine>,
    polygon: Option<Hwp5ShapeComponentPolygon>,
    ellipse: Option<Hwp5ShapeComponentEllipse>,
    curve: Option<Hwp5ShapeComponentCurve>,
    text_art: Option<Hwp5ShapeTextArt>,
    /// Leading 4-byte type tag from the `ShapeComponent` (`0x4C`) record, used
    /// to tell a connect line apart from a plain line (both use `0x4E`).
    shape_component_kind: Option<[u8; 4]>,
    /// Level of the scope's first `ShapeComponent`: the shape's own records
    /// sit one level below it.
    comp_level: Option<u16>,
    /// The shape's hyperlink (`CtrlData` one level below its component).
    link: Option<Result<String, String>>,
    paragraphs: Vec<Hwp5Paragraph>,
    /// Active group builder, set when this gso scope's first `ShapeComponent`
    /// carries the `"$con"` type tag. When `Some`, all subtree records are
    /// routed to the group instead of the flat single-shape slots above.
    group: Option<GsoGroupBuilder>,
    /// Outline and fill from the scope's first `ShapeComponent` (§19.5 1b).
    look: Option<crate::schema::section::Hwp5ShapeLook>,
    /// Caption paragraphs read before the shape's component (not carried).
    caption_paragraphs: usize,
    /// A rectangle's corner ratio read without a look to carry it.
    unread_corner_ratio: u8,
    /// Initial box of the scope's first `ShapeComponent` (HWPX `orgSz`).
    original_size: Option<(u32, u32)>,
    standalone_render: Option<Result<crate::schema::section::Hwp5ShapeRender, String>>,
    /// Length of the last list header seen (a shape's text list when the
    /// scope is a shape).
    list_header_len: Option<usize>,
}

impl NestedSubtreeContext {
    fn new(
        ctrl_depth: u16,
        ctrl_id: u32,
        properties_raw: u32,
        instance_id: u32,
        geometry: Option<Hwp5ShapeComponentGeometry>,
    ) -> Self {
        Self {
            wire_order: 0,
            ctrl_depth,
            ctrl_id,
            properties_raw,
            instance_id,
            saw_list_header: false,
            list_header_properties: None,
            text_margin: None,
            saw_shape_rectangle: false,
            saw_shape_component: false,
            geometry,
            picture: None,
            ole: None,
            line: None,
            polygon: None,
            ellipse: None,
            curve: None,
            text_art: None,
            shape_component_kind: None,
            comp_level: None,
            link: None,
            paragraphs: Vec::new(),
            group: None,
            look: None,
            caption_paragraphs: 0,
            unread_corner_ratio: 0,
            original_size: None,
            standalone_render: None,
            list_header_len: None,
        }
    }

    fn note_list_header(&mut self, data: &[u8]) {
        self.saw_list_header = true;
        // A shape's text list (§19.22 D4): its own layout. A header's or
        // footer's list header passes here too; its word is not used.
        self.list_header_properties = gso::shape_text_list_properties(data);
        self.text_margin = gso::shape_text_list_margin(data);
        self.list_header_len = Some(data.len());
    }

    /// A rectangle record: its byte 0 is the corner ratio (§19.5 1c).
    ///
    /// 이것을 실패시키는 것: 비율 읽기를 지우기(`rect_corner_ratio_is_read`).
    fn note_shape_rectangle(&mut self, data: &[u8]) {
        self.saw_shape_rectangle = true;
        match (self.look.as_mut(), data.first()) {
            (Some(look), Some(&ratio)) => look.corner_ratio = ratio,
            // No outline/fill block to carry it with (review r1): reported
            // when the shape is classified, not dropped.
            (None, Some(&ratio)) => self.unread_corner_ratio = ratio,
            _ => {}
        }
    }

    fn note_shape_component(&mut self, data: &[u8], level: u16, warnings: &mut Vec<Hwp5Warning>) {
        self.saw_shape_component = true;
        if self.comp_level.is_none() {
            self.standalone_render = Some(
                crate::schema::section::Hwp5ShapeRender::parse(data, true)
                    .map(|mut r| {
                        r.source_id = self.instance_id;
                        r
                    })
                    .map_err(|e| e.to_string()),
            );
            self.look = gso::parse_shape_look(data, true, warnings);
            // A shape's caption is a list one level below its control
            // header, before the component; its text list comes after. The
            // caption is not carried — its paragraphs must not become the
            // shape's text (review r1 of §19.5 1a: a polygon showed its
            // "[그림 3]" caption as its own text).
            // 이것을 실패시키는 것: 이 분리를 지우기
            // (`ellipse_and_polygon_keep_their_text`).
            self.caption_paragraphs += self.paragraphs.len();
            self.paragraphs.clear();
            self.saw_list_header = false;
            self.list_header_properties = None;
            self.text_margin = None;
            self.list_header_len = None;
            self.original_size = crate::schema::section::Hwp5ShapeRender::initial_size(data, true);
        }
        self.comp_level.get_or_insert(level);
        if let Some(code) = data.get(..4) {
            self.shape_component_kind = Some([code[0], code[1], code[2], code[3]]);
        }
    }

    /// A `CtrlData` record: the shape's hyperlink when it sits one level
    /// below the shape's component (deeper ones belong to its text).
    fn note_ctrl_data(&mut self, data: &[u8], level: u16) {
        if self.link.is_none() && self.comp_level.is_some_and(|c| level == c.saturating_add(1)) {
            self.link = Some(crate::schema::section::parse_object_link(data));
        }
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

    fn note_shape_ellipse(&mut self, ellipse: Hwp5ShapeComponentEllipse) {
        self.ellipse = Some(ellipse);
    }

    fn note_shape_curve(&mut self, curve: Hwp5ShapeComponentCurve) {
        self.curve = Some(curve);
    }

    fn note_shape_text_art(&mut self, ta: Hwp5ShapeTextArt) {
        self.text_art = Some(ta);
    }

    fn allows_nested_paragraphs(&self) -> bool {
        match self.ctrl_id {
            CTRL_ID_HEADER | CTRL_ID_FOOTER | CTRL_ID_FOOTNOTE | CTRL_ID_ENDNOTE | CTRL_ID_GSO => {
                self.saw_list_header
            }
            _ => false,
        }
    }

    fn kind(&self) -> Option<NestedSubtreeKind> {
        match self.ctrl_id {
            CTRL_ID_HEADER => Some(NestedSubtreeKind::Header),
            CTRL_ID_FOOTER => Some(NestedSubtreeKind::Footer),
            CTRL_ID_FOOTNOTE => Some(NestedSubtreeKind::Footnote),
            CTRL_ID_ENDNOTE => Some(NestedSubtreeKind::Endnote),
            CTRL_ID_GSO if self.saw_shape_rectangle => Some(NestedSubtreeKind::TextBox),
            _ => None,
        }
    }

    fn into_control(self, warnings: &mut Vec<Hwp5Warning>) -> Hwp5Control {
        // A `$con` group takes precedence over the flat single-shape path:
        // the gso scope wrapped child shapes rather than a single shape.
        if let Some(group) = self.group {
            return group.into_control(warnings);
        }
        // 이것을 실패시키는 것: 이 보고를 지우기
        // (`corner_ratio_without_a_look_is_reported`).
        if self.unread_corner_ratio > 0 {
            warnings.push(Hwp5Warning::ProjectionFallback {
                subject: "shape.corner_ratio",
                reason: format!(
                    "corner ratio {} not carried: the shape's outline and fill block is unreadable",
                    self.unread_corner_ratio
                ),
            });
        }
        // A shape's text list too short for its property word (§19.22 D4).
        // 이것을 실패시키는 것: 이 보고를 지우기
        // (`shape_text_list_header_reads_its_own_layout`).
        if let Some(len) = self.list_header_len.filter(|_| self.comp_level.is_some()) {
            if self.text_margin.is_none() {
                warnings.push(gso::short_list_header_warning(len));
            }
        }
        if self.caption_paragraphs > 0 {
            warnings.push(Hwp5Warning::DroppedControl {
                control: "caption",
                reason: format!(
                    "shape caption ({} paragraphs) is not carried",
                    self.caption_paragraphs
                ),
            });
        }
        match self.kind() {
            Some(NestedSubtreeKind::Header) if self.saw_list_header => {
                Hwp5Control::Header(Hwp5NestedSubtree {
                    wire_order: self.wire_order,
                    ctrl_id: self.ctrl_id,
                    properties_raw: self.properties_raw,
                    instance_id: self.instance_id,
                    paragraphs: self.paragraphs,
                })
            }
            Some(NestedSubtreeKind::Footer) if self.saw_list_header => {
                Hwp5Control::Footer(Hwp5NestedSubtree {
                    wire_order: self.wire_order,
                    ctrl_id: self.ctrl_id,
                    properties_raw: self.properties_raw,
                    instance_id: self.instance_id,
                    paragraphs: self.paragraphs,
                })
            }
            Some(NestedSubtreeKind::Footnote) if self.saw_list_header => {
                Hwp5Control::Footnote(Hwp5NestedSubtree {
                    wire_order: self.wire_order,
                    ctrl_id: self.ctrl_id,
                    properties_raw: self.properties_raw,
                    instance_id: self.instance_id,
                    paragraphs: self.paragraphs,
                })
            }
            Some(NestedSubtreeKind::Endnote) if self.saw_list_header => {
                Hwp5Control::Endnote(Hwp5NestedSubtree {
                    wire_order: self.wire_order,
                    ctrl_id: self.ctrl_id,
                    properties_raw: self.properties_raw,
                    instance_id: self.instance_id,
                    paragraphs: self.paragraphs,
                })
            }
            Some(NestedSubtreeKind::TextBox) if self.saw_list_header => match self.geometry {
                Some(geometry) => Hwp5Control::TextBox(Hwp5TextBoxControl {
                    standalone_render: self.standalone_render,
                    look: self.look,
                    ctrl_id: self.ctrl_id,
                    geometry,
                    paragraphs: self.paragraphs,
                    list_header_properties: self.list_header_properties,
                    text_margin: self.text_margin,
                    ctrl_properties: self.properties_raw,
                    original_size: self.original_size,
                }),
                None => Hwp5Control::Unknown { ctrl_id: self.ctrl_id, header_data: Vec::new() },
            },
            _ => classify_gso_control(GsoClassificationInput {
                standalone_render: self.standalone_render,
                ctrl_id: self.ctrl_id,
                saw_shape_component: self.saw_shape_component,
                saw_shape_rectangle: self.saw_shape_rectangle && !self.saw_list_header,
                geometry: self.geometry,
                picture: self.picture,
                ole: self.ole,
                line: self.line,
                polygon: self.polygon,
                ellipse: self.ellipse,
                curve: self.curve,
                text_art: self.text_art,
                shape_component_kind: self.shape_component_kind,
                link: self.link,
                instance_id: self.instance_id,
                ctrl_properties: self.properties_raw,
                look: self.look,
                paragraphs: self.paragraphs,
                list_header_properties: self.list_header_properties,
                text_margin: self.text_margin,
                original_size: self.original_size,
            }),
        }
    }
}

fn classify_gso_control(input: GsoClassificationInput) -> Hwp5Control {
    if input.ctrl_id != CTRL_ID_GSO || !input.saw_shape_component {
        return Hwp5Control::Unknown { ctrl_id: input.ctrl_id, header_data: Vec::new() };
    }

    // TextArt (글맵시) carries a `0x5A` `ShapeTextArt` sub-record but no
    // single-shape payload (`payload_count == 0`), so it must be handled
    // before the `payload_count != 1` guard below.
    if input.shape_component_kind == Some(SHAPE_COMPONENT_TYPE_TEXTART) {
        if let (Some(geometry), Some(text_art)) = (input.geometry, input.text_art) {
            return Hwp5Control::TextArt(Hwp5TextArtControl {
                ctrl_id: input.ctrl_id,
                geometry,
                text_art,
                instance_id: input.instance_id,
                original_size: input.original_size,
                ctrl_properties: input.ctrl_properties,
            });
        }
        return Hwp5Control::Unknown { ctrl_id: input.ctrl_id, header_data: Vec::new() };
    }

    let payload_count = usize::from(input.picture.is_some())
        + usize::from(input.ole.is_some())
        + usize::from(input.saw_shape_rectangle)
        + usize::from(input.line.is_some())
        + usize::from(input.polygon.is_some())
        + usize::from(input.ellipse.is_some())
        + usize::from(input.curve.is_some());
    if payload_count != 1 {
        return Hwp5Control::Unknown { ctrl_id: input.ctrl_id, header_data: Vec::new() };
    }

    // Ellipse (0x50) doubles as the arc carrier — split on its arc fields.
    // Both ellipse/arc/curve need geometry for placement; without it we drop
    // to Unknown rather than fabricate a zero-size shape.
    if let Some(ellipse) = input.ellipse {
        let ctrl_id = input.ctrl_id;
        return match input.geometry {
            Some(geometry) if ellipse.is_arc() => Hwp5Control::Arc(Hwp5ArcControl {
                standalone_render: input.standalone_render,
                look: input.look.clone(),
                ctrl_id,
                geometry,
                ctrl_properties: input.ctrl_properties,
                shape: ellipse,
                original_size: input.original_size,
            }),
            Some(geometry) => Hwp5Control::Ellipse(Hwp5EllipseControl {
                standalone_render: input.standalone_render,
                paragraphs: input.paragraphs.clone(),
                list_header_properties: input.list_header_properties,
                text_margin: input.text_margin,
                look: input.look.clone(),
                ctrl_id,
                geometry,
                ctrl_properties: input.ctrl_properties,
                shape: ellipse,
                original_size: input.original_size,
            }),
            None => Hwp5Control::Unknown { ctrl_id, header_data: Vec::new() },
        };
    }
    if let Some(curve) = input.curve {
        let ctrl_id = input.ctrl_id;
        return match input.geometry {
            Some(geometry) => Hwp5Control::Curve(Hwp5CurveControl {
                standalone_render: input.standalone_render,
                look: input.look.clone(),
                ctrl_id,
                geometry,
                points: curve.points,
                segment_types: curve.segment_types,
                ctrl_properties: input.ctrl_properties,
                original_size: input.original_size,
            }),
            None => Hwp5Control::Unknown { ctrl_id, header_data: Vec::new() },
        };
    }

    // Connect line shares the 0x4E ShapeComponentLine sub-record with a plain
    // line; the only discriminator is the ShapeComponent "$col" type tag.
    // Conservative: only an exact "$col" match upgrades to a connect line, so a
    // plain line is never reclassified (uses `as_ref` so the fall-through tuple
    // match below still owns geometry/line for the plain-line case).
    if input.shape_component_kind == Some(SHAPE_COMPONENT_TYPE_CONNECT_LINE) {
        if let (Some(geometry), Some(line)) = (input.geometry.as_ref(), input.line.as_ref()) {
            return Hwp5Control::ConnectLine(Hwp5ConnectLineControl {
                standalone_render: input.standalone_render,
                look: input.look.clone(),
                ctrl_id: input.ctrl_id,
                geometry: geometry.clone(),
                start: line.start,
                end: line.end,
                ctrl_properties: input.ctrl_properties,
                original_size: input.original_size,
            });
        }
    }

    let gso_instance_id = input.instance_id;
    match (input.geometry, input.picture, input.ole, input.line, input.polygon) {
        (Some(geometry), Some(picture), None, None, None) => Hwp5Control::Image(Hwp5ImageControl {
            standalone_render: input.standalone_render,
            ctrl_id: input.ctrl_id,
            geometry,
            binary_data_id: picture.binary_data_id,
            instance_id: gso_instance_id,
            ctrl_properties: input.ctrl_properties,
            picture,
            original_size: input.original_size,
            link: input.link,
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
            Hwp5Control::Rect(Hwp5RectControl {
                standalone_render: input.standalone_render,
                look: input.look.clone(),
                ctrl_id: input.ctrl_id,
                geometry,
                ctrl_properties: input.ctrl_properties,
                original_size: input.original_size,
            })
        }
        (Some(geometry), None, None, Some(line), None) => Hwp5Control::Line(Hwp5LineControl {
            standalone_render: input.standalone_render,
            look: input.look.clone(),
            ctrl_id: input.ctrl_id,
            geometry,
            start: line.start,
            end: line.end,
            ctrl_properties: input.ctrl_properties,
            original_size: input.original_size,
        }),
        (Some(geometry), None, None, None, Some(polygon)) if polygon.points.len() >= 3 => {
            Hwp5Control::Polygon(Hwp5PolygonControl {
                standalone_render: input.standalone_render,
                paragraphs: input.paragraphs.clone(),
                list_header_properties: input.list_header_properties,
                text_margin: input.text_margin,
                look: input.look.clone(),
                ctrl_id: input.ctrl_id,
                geometry,
                points: polygon.points,
                ctrl_properties: input.ctrl_properties,
                original_size: input.original_size,
            })
        }
        _ => Hwp5Control::Unknown { ctrl_id: input.ctrl_id, header_data: Vec::new() },
    }
}

// ---------------------------------------------------------------------------
// Text rendering
// ---------------------------------------------------------------------------

/// Moves `PARA_CHAR_SHAPE` boundaries from wire positions (UTF-16 units of
/// the `PARA_TEXT` stream, where a tab or an inline/extended control takes
/// 8) to positions in [`Hwp5Paragraph::text`] (UTF-16), the axis projection
/// splits runs on: text 1:1, a tab or an object 8 → 1, a section/column
/// definition or a field marker 8 → 0, the paragraph end 1 → 0, a silently
/// consumed control (`silent_wires`) n → 0.
///
/// Boundaries that land on the same text position (zero-width controls in
/// between) keep the last one, which is the shape the characters there
/// take. A boundary inside a control's wire units (no file in the corpus
/// has one) moves to the control's end with a
/// `char_shape.boundary_inside_control` warning; one past the end of the
/// text moves to its end.
fn char_shape_runs_to_text_positions(
    segments: &[TextSegment],
    silent_wires: &[crate::schema::section::SilentWire],
    runs: &[Hwp5CharShapeRun],
    warnings: &mut Vec<Hwp5Warning>,
) -> Vec<Hwp5CharShapeRun> {
    let mut builder = crate::wire_text_map::WireMapBuilder::new();
    let mut wire = 0u32;
    let mut silent = silent_wires.iter().peekable();
    for segment in segments {
        while let Some(skipped) = silent.next_if(|skipped| skipped.start <= wire) {
            builder.push_span(skipped.len, 0);
            wire = wire.saturating_add(skipped.len);
        }
        let (wire_len, text_len) = segment_lengths(segment);
        if wire_len == text_len {
            builder.advance_identity(wire_len);
        } else {
            builder.push_span(wire_len, text_len);
        }
        wire = wire.saturating_add(wire_len);
    }
    for skipped in silent {
        builder.push_span(skipped.len, 0);
    }
    let Ok(map) = builder.finish() else {
        warnings.push(Hwp5Warning::ProjectionFallback {
            subject: "char_shape.positions",
            reason: "paragraph wire length overflows; char shape boundaries kept at wire \
                     positions"
                .to_string(),
        });
        return runs.to_vec();
    };

    // Boundaries come in ascending order, so one cursor walks the spans once
    // for all of them (`O(boundaries + spans)`).
    let mut cursor = map.cursor();
    let mut out: Vec<Hwp5CharShapeRun> = Vec::with_capacity(runs.len());
    for run in runs {
        // 이것을 실패시키는 것: 개체 안쪽 경계를 커서로 개체 끝까지 전진시켜
        // 찾기(같은 개체 안의 다음 경계가 커서를 되감아 `O(S²)`,
        // `char_shape_boundaries_inside_controls_stay_linear`).
        let position = match cursor.core_at_or_after(run.position) {
            Ok((position, false)) => position,
            Ok((position, true)) => {
                warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "char_shape.boundary_inside_control",
                    reason: format!(
                        "char shape {} starts at wire {}, inside a control; moved to \
                         the control's end",
                        run.char_shape_id, run.position
                    ),
                });
                position
            }
            Err(_) => map.core_end(),
        };
        match out.last_mut() {
            Some(last) if last.position == position => last.char_shape_id = run.char_shape_id,
            _ => out.push(Hwp5CharShapeRun { position, char_shape_id: run.char_shape_id }),
        }
    }
    out
}

/// Convert a slice of `TextSegment`s into a plain string.
///
/// - `Text(s)` — appended verbatim
/// - `Tab` — replaced with `\t`
/// - `LineBreak` — replaced with `\n`
/// - `NonBreakingSpace` — replaced with `\u{00A0}` (canonical NBSP sentinel)
/// - `FwSpace` — replaced with `\u{001F}` (fixed-width space sentinel; mirrors
///   the HWP5 wire control byte so the round-trip through Core is lossless)
/// - `ControlRef` / `ExtendedControlRef` — replaced with `\u{FFFC}` (object replacement)
/// - All other segments (ParaBreak, FieldBegin, FieldEnd, SectionColumnDef) — ignored
fn segments_to_string(segments: &[TextSegment]) -> String {
    let mut out = String::new();
    for seg in segments {
        match seg {
            TextSegment::Text(s) => out.push_str(s),
            TextSegment::Tab { .. } => out.push('\t'),
            TextSegment::LineBreak => out.push('\n'),
            TextSegment::NonBreakingSpace => out.push('\u{00A0}'),
            TextSegment::FwSpace => out.push('\u{001F}'),
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

/// Validate original wire references before projection can omit any container or style 0.
/// Record levels do not restrict this check: nested tables, captions, notes, shapes,
/// groups and memo lists all use the same paragraph records.
/// 이것을 실패시키는 것: 종류별 lookup 생략/인덱스 무시/중첩 깊이 필터링
/// (`body_style_references_check_original_definitions_in_every_container`).
pub(super) fn validate_body_style_references(
    data: &[u8],
    definitions: &super::header::DocInfoResult,
) -> Hwp5Result<()> {
    use hwpforge_core::style_lookup::lookup;
    for record in Record::parse_stream(&mut Cursor::new(data))? {
        match TagId::from(record.header.tag_id) {
            TagId::ParaHeader => {
                // Preserve the parser's warning/fallback contract for malformed payloads.
                // 이것을 실패시키는 것: 잘린 payload를 새 hard error로 바꾸기(같은 회귀 시험).
                if let Ok(header) = Hwp5ParaHeader::parse(&record.data) {
                    lookup(
                        definitions.para_shapes.get(usize::from(header.para_shape_id)),
                        "paraPr",
                        header.para_shape_id,
                    )?;
                    lookup(
                        definitions.styles.get(usize::from(header.style_id)),
                        "style",
                        header.style_id,
                    )?;
                }
            }
            TagId::ParaCharShape => {
                if let Ok(runs) = Hwp5CharShapeRun::parse_all(&record.data) {
                    for run in runs {
                        lookup(
                            definitions.char_shapes.get(run.char_shape_id as usize),
                            "charPr",
                            run.char_shape_id,
                        )?;
                    }
                }
            }
            TagId::CtrlHeader if parse_ctrl_id(&record.data) == CTRL_ID_COMPOSE => {
                // 이것을 실패시키는 것: compose charPr 검사 생략 또는 MAX 상속값 거부.
                if let Some(compose) = Hwp5ComposeControl::parse(CTRL_ID_COMPOSE, &record.data) {
                    for id in compose.char_pr_ids {
                        if id != u32::MAX {
                            lookup(definitions.char_shapes.get(id as usize), "compose charPr", id)?;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Default)]
struct BodyTextParserState {
    master_pages: Vec<Hwp5MasterPage>,
    master_capture: Option<master::MasterCapture>,
    secd_level: Option<u16>,
    secd_declaration: Option<(usize, u16, Vec<u8>)>,
    extended_master_count: u16,
    ordinary_master_count: usize,
    record_index: usize,
    wire_sequence: std::rc::Rc<std::cell::Cell<usize>>,
    /// Absolute paragraph level; records and their payloads stay borrowed.
    base_level: u16,
    budget: NestingBudget,
    active_budget: NestingBudget,
    nested_control: Option<NestedParagraphControl>,
    skipped_control: Option<u16>,
    pending_clickhere_level: u16,
    /// (record depth, owning paragraph's control index). Only an immediate
    /// CtrlData child can name that bookmark; a sibling cancels this binding.
    pending_bookmark: Option<(u16, usize)>,
    /// Only the section owner resolves MEMO anchors after collecting all lists.
    nested: bool,
    #[cfg(test)]
    work: std::rc::Rc<std::cell::Cell<usize>>,
    #[cfg(test)]
    record_identity: std::rc::Rc<std::cell::Cell<*const Record>>,

    paragraphs: Vec<Hwp5Paragraph>,
    page_def: Option<Hwp5PageDef>,
    /// Captured `secd` ctrl property word (HWP 5.0 spec §4.3.10.1 표 130).
    section_def_properties: Option<u32>,
    /// Captured `secd` 시작번호 (`[20..28]`, all-or-none — W1).
    section_def_start_numbers: Option<Hwp5SectionStartNumbers>,
    /// Page border/fill records collected in document order.
    page_border_fills: Vec<Hwp5PageBorderFill>,
    /// Captured `cold` (column definition) ctrl, if multi-column.
    column_def: Option<Hwp5ColumnDef>,
    warnings: Vec<Hwp5Warning>,
    current: Option<ParaBuf>,
    table_stack: Vec<TableContext>,
    subtree_ctx: Option<NestedSubtreeContext>,
    current_subtree_para: Option<ParaBuf>,
    /// Geometry of an `eqed` ctrl awaiting its `HWPTAG_EQEDIT` script child.
    /// Wave 12p Step 1c-2: stash (geometry, instance_id) from the
    /// `eqed` CtrlHeader; finalized by the paired `HWPTAG_EQEDIT` record.
    eqed_pending: Option<(Hwp5ShapeComponentGeometry, u32)>,
    /// `%clk` CtrlHeader awaiting its trailing `0x57 lvl=2` sub-record
    /// for the form-mode field name (Wave 12l). If the next top-level
    /// record is not the expected sub-record, the press-field is
    /// finalized with `name=None` and a targeted warning so the rest
    /// of the section keeps round-tripping.
    pending_clickhere: Option<crate::schema::section::Hwp5ClickHereControl>,
    /// `HWPTAG_MEMO_LIST` clusters collected from the section, keyed by
    /// memo_id. Filled while parsing the cluster region at the section end;
    /// consumed in `finish` to merge into the matching `Hwp5Control::Memo`
    /// placeholders.
    memo_contents: HashMap<u32, Vec<Hwp5Paragraph>>,
    memo_note_sources: std::collections::BTreeMap<u32, Vec<master::NoteNumberingSource>>,
    /// Active capture state for the memo-content cluster currently being
    /// absorbed. `Some` while inside a `HWPTAG_MEMO_LIST` region.
    memo_content_capture: Option<MemoContentCapture>,
}

impl BodyTextParserState {
    fn handle_record(&mut self, record: &Record) {
        let tag = TagId::from(record.header.tag_id);
        let level = record.header.level;
        self.record_index += 1;

        #[cfg(test)]
        {
            self.work.set(self.work.get() + 1);
            let expected = self.record_identity.get();
            assert!(expected.is_null() || std::ptr::eq(expected, record), "record was cloned");
        }
        if self.dispatch_master_record(record, tag, level) {
            return;
        }
        if let Some(skipped) = self.skipped_control {
            if level > skipped {
                return;
            }
            self.skipped_control = None;
        }
        if self.nested_control.as_ref().is_some_and(|c| level <= c.level) {
            self.close_nested_control();
        }
        if let Some(capture) = self.nested_control.as_mut() {
            capture.parser.handle_record(record);
            return;
        }
        if self.try_attach_bookmark_name(record) {
            return;
        }
        self.prepare_for_record(level);
        if self.intercept_master_record(record, tag, level) {
            return;
        }

        if self.handle_active_table_record(record, tag, level) {
            return;
        }
        if self.handle_active_subtree_record(record, tag, level) {
            return;
        }
        self.handle_top_level_record(record, tag, level);
    }

    fn try_attach_bookmark_name(&mut self, record: &Record) -> bool {
        let Some((level, index)) = self.pending_bookmark.take() else {
            return false;
        };
        if TagId::from(record.header.tag_id) != TagId::CtrlData
            || record.header.level != level.saturating_add(1)
        {
            return false;
        }
        let Some(Hwp5Control::Bookmark { name, .. }) =
            self.current.as_mut().and_then(|p| p.controls.get_mut(index))
        else {
            return false;
        };
        // Named parameter set: set 0x21b, one item, item 0, PIT_BSTR=0x4000.
        // Reuse the bounded UTF-16 reader, but require the bookmark parameter
        // identity as well as the owning record scope.
        if record.data.starts_with(&[0x1b, 2, 1, 0, 0, 0, 0, 0x40, 1, 0]) {
            if let crate::schema::section::ClickHereNameSubrecord::Named(value) =
                crate::schema::section::Hwp5ClickHereControl::parse_name_subrecord(&record.data)
            {
                *name = Some(value);
            }
        }
        true
    }

    fn warn_nested_section_state(&mut self) {
        if self.nested
            && (self.page_def.is_some()
                || self.section_def_properties.is_some()
                || self.section_def_start_numbers.is_some()
                || self.column_def.is_some()
                || !self.page_border_fills.is_empty())
        {
            self.warnings.push(Hwp5Warning::DroppedControl {
                control: "nested_section_settings",
                reason: "page/section/column/border settings inside a nested paragraph host have no independent Core section scope; omitted".into(),
            });
        }
    }

    fn nested_parser(&self, base_level: u16) -> Self {
        Self {
            base_level,
            budget: self.active_budget,
            active_budget: self.active_budget,
            nested: true,
            wire_sequence: self.wire_sequence.clone(),
            #[cfg(test)]
            work: self.work.clone(),
            #[cfg(test)]
            record_identity: self.record_identity.clone(),
            ..Self::default()
        }
    }

    /// All nested CtrlHeaders call the very same interpreter as body controls.
    /// Move the host buffer so pending equations/fields attach to its exact run
    /// queue. The buffer is returned before the enclosing paragraph can close.
    fn open_nested_control(&mut self, record: &Record, level: u16, host: ControlHost) {
        let mut parser = self.nested_parser(self.base_level);
        parser.current = match host {
            ControlHost::Cell => {
                self.table_stack.last_mut().and_then(|t| t.current_cell_para.take())
            }
            ControlHost::Subtree => self.current_subtree_para.take(),
        };
        if parser.current.is_none() {
            if parse_ctrl_id(&record.data) == CTRL_ID_TABLE {
                self.warnings.push(Hwp5Warning::ParserFallback {
                    subject: "table.nested_attach",
                    reason: "orphaned_nested_table_without_parent_paragraph".to_string(),
                });
            }
            self.skipped_control = Some(level);
            return;
        }
        parser.handle_ctrl_header(record, level);
        self.nested_control =
            Some(NestedParagraphControl { level, host, parser: Box::new(parser) });
    }

    fn close_nested_control(&mut self) {
        let Some(mut capture) = self.nested_control.take() else {
            return;
        };
        capture.parser.close_nested_control();
        capture.parser.prepare_for_record(0);
        capture.parser.flush_pending_clickhere();
        match capture.host {
            ControlHost::Cell => {
                if let Some(table) = self.table_stack.last_mut() {
                    table.current_cell_para = capture.parser.current.take();
                }
            }
            ControlHost::Subtree => self.current_subtree_para = capture.parser.current.take(),
        }
        capture.parser.warn_nested_section_state();
        self.warnings.append(&mut capture.parser.warnings);
    }

    /// The common interpreter already charged the global container budget.
    fn push_table_context(&mut self, level: u16, instance_id: u32) {
        self.table_stack.push(TableContext::new(level, instance_id));
    }

    /// Keeps the just-opened table's `CtrlHeader` placement facts.
    fn note_table_ctrl(&mut self, data: &[u8]) {
        if let Some(ctx) = self.table_stack.last_mut() {
            ctx.table.ctrl = Hwp5TableCtrl::parse(data);
        }
    }

    fn prepare_for_record(&mut self, level: u16) {
        while self.table_stack.last().is_some_and(|ctx| level <= ctx.ctrl_depth) {
            let ctx =
                self.table_stack.pop().expect("table_stack.last().is_some() implies pop succeeds");
            let finished = ctx.finalize(&mut self.warnings);
            attach_finished_table(
                &mut self.current,
                &mut self.table_stack,
                finished,
                &mut self.warnings,
            );
        }

        if self.subtree_ctx.as_ref().is_some_and(|ctx| level <= ctx.ctrl_depth) {
            flush_subtree_paragraph(
                &mut self.current_subtree_para,
                self.subtree_ctx.as_mut(),
                &mut self.warnings,
            );
            attach_finished_subtree(&mut self.current, self.subtree_ctx.take(), &mut self.warnings);
        }
    }

    fn handle_active_table_record(&mut self, record: &Record, tag: TagId, level: u16) -> bool {
        if self.table_stack.last().is_none_or(|ctx| level <= ctx.ctrl_depth) {
            return false;
        }

        match tag {
            TagId::Table => {
                let table_header = parse_table_header(&record.data);
                if let Some(ctx) = self.table_stack.last_mut() {
                    // D: the caption's paragraphs end where the body starts.
                    // Fewer than the list header declared is a cut caption
                    // (D review r1 MEDIUM-5). 이것을 실패시키는 것: 이 경고 삭제
                    // (`table_caption_is_read` 의 2 문단 선언·1 문단 도착) · `<` 를 `<=` 로
                    // (선언대로 온 캡션도 경고).
                    if ctx.current_cell.as_ref().is_some_and(|c| c.caption.is_some()) {
                        ctx.flush_current_cell_paragraph(&mut self.warnings);
                        if let Some(cell) = &ctx.current_cell {
                            let got = cell.cell.paragraphs.len();
                            if got < cell.expected_paragraphs {
                                self.warnings.push(Hwp5Warning::ParserFallback {
                                    subject: "table.caption",
                                    reason: format!(
                                        "caption declares {} paragraphs but {got} arrived before the table body",
                                        cell.expected_paragraphs
                                    ),
                                });
                            }
                        }
                        ctx.finish_active_cell();
                    }
                    ctx.table.rows = table_header.rows;
                    ctx.table.cols = table_header.cols;
                    ctx.table.page_break = table_header.page_break;
                    ctx.table.repeat_header = table_header.repeat_header;
                    ctx.table.cell_spacing = table_header.cell_spacing;
                    ctx.table.border_fill_id = table_header.border_fill_id;
                    ctx.table.inner_margin = table_header.inner_margin;
                    ctx.table.no_adjust = table_header.no_adjust;
                    let damage = if table_header.cell_zone_count_cut {
                        Some("table record ends one byte into its cell zone count; no zones are read".to_string())
                    } else {
                        table_header.cell_zones_declared.map(|declared| {
                            format!(
                                "table record declares {declared} cell zones but holds {}; the rest are dropped",
                                table_header.cell_zones.len()
                            )
                        })
                    };
                    if let Some(reason) = damage {
                        crate::warning_utils::push_projection_fallback(
                            &mut self.warnings,
                            "table.cell_zone",
                            reason,
                        );
                    }
                    ctx.table.cell_zones = table_header.cell_zones;
                    ctx.seen_table_body = true;
                }
            }
            TagId::ListHeader => {
                if let Some(ctx) = self.table_stack.last_mut() {
                    // D: a list header directly under the table control before
                    // its body is the caption. 이것을 실패시키는 것:
                    // `seen_table_body` 조건 반전(칸이 캡션으로) · 오프셋 6 으로 읽기 ·
                    // 짧은 레코드 경고 삭제(`table_caption_is_read`).
                    if !ctx.seen_table_body
                        && level == ctx.ctrl_depth.saturating_add(1)
                        && ctx.current_cell.is_none()
                        && ctx.table.caption.is_none()
                    {
                        match Hwp5TableCaption::parse_list_header(&record.data) {
                            Some((paragraph_count, caption)) => {
                                ctx.current_cell =
                                    Some(ActiveTableCell::caption(paragraph_count, caption));
                            }
                            None => self.warnings.push(Hwp5Warning::DroppedControl {
                                control: "caption",
                                reason: format!(
                                    "table caption list header is {} bytes (needs 22)",
                                    record.data.len()
                                ),
                            }),
                        }
                    } else if ctx.seen_table_body && level == ctx.ctrl_depth.saturating_add(1) {
                        ctx.flush_current_cell_paragraph(&mut self.warnings);
                        ctx.finish_active_cell_if_ready();
                        match parse_table_cell(&record.data) {
                            Ok((paragraph_count, cell)) => {
                                ctx.current_cell = Some(ActiveTableCell {
                                    expected_paragraphs: paragraph_count,
                                    cell,
                                    caption: None,
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
                        ctx.flush_current_cell_paragraph(&mut self.warnings);
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
            TagId::ParaLineSeg => {
                if let Some(buf) =
                    self.table_stack.last_mut().and_then(|ctx| ctx.current_cell_para.as_mut())
                {
                    if let Some(segments) = Self::parse_para_line_segments(
                        record.header.tag_id,
                        &record.data,
                        &mut self.warnings,
                    ) {
                        buf.line_segments = segments;
                    }
                }
            }
            TagId::CtrlHeader => self.open_nested_control(record, level, ControlHost::Cell),
            TagId::Unknown(id) => {
                self.warnings.push(Hwp5Warning::UnsupportedTag { tag_id: id, offset: 0 });
            }
            _ => {}
        }

        true
    }

    /// Routes a subtree record into the active gso `$con` group, activating
    /// the group on the first `$con` `ShapeComponent` (`0x4C`) seen at the
    /// gso scope's top child level.
    ///
    /// Returns `true` when the record was consumed by group handling so the
    /// caller skips the flat single-shape dispatch. Wave A keeps flat groups
    /// only; nested `$con` children degrade to `Unknown` inside
    /// [`GsoGroupBuilder`].
    fn maybe_route_subtree_group_record(
        &mut self,
        record: &Record,
        tag: TagId,
        level: u16,
    ) -> bool {
        // Read the gso scope shape (ctrl_id / depth / geometry / group flag)
        // without holding a mutable borrow across the `self.warnings` writes.
        let Some((is_gso, ctrl_depth, geometry, instance_id, properties, group_active)) =
            self.subtree_ctx.as_ref().map(|ctx| {
                (
                    ctx.ctrl_id == CTRL_ID_GSO,
                    ctx.ctrl_depth,
                    ctx.geometry.clone(),
                    ctx.instance_id,
                    ctx.properties_raw,
                    ctx.group.is_some(),
                )
            })
        else {
            return false;
        };
        if !is_gso {
            return false;
        }

        // Activate the group on the first `$con` ShapeComponent at the gso
        // scope's top child level (ctrl_depth + 1). Requires a recovered
        // group bounding box; without it we cannot place the container, so
        // fall through to the (degrading) flat path.
        if !group_active
            && matches!(tag, TagId::ShapeComponent)
            && level == ctrl_depth.saturating_add(1)
            && record.data.get(..4) == Some(&SHAPE_COMPONENT_TYPE_GROUP)
        {
            let Some(geometry) = geometry else {
                self.warnings.push(Hwp5Warning::DroppedControl {
                    control: "gso_group",
                    reason: "group ($con) without recoverable bounding box; \
                             cannot place <hp:container>"
                        .to_string(),
                });
                return false;
            };
            // Directly under the `gso` control header the component writes
            // its type tag twice.
            let render = crate::schema::section::Hwp5ShapeRender::parse(&record.data, true)
                .map_err(|e| e.to_string());
            if self.active_budget.groups >= GSO_GROUP_MAX_DEPTH {
                self.warnings.push(Hwp5Warning::DroppedControl {
                    control: "gso_group",
                    reason: "shared group depth cap exceeded".to_string(),
                });
                self.skipped_control = Some(ctrl_depth);
                return true;
            }
            if let Some(ctx) = self.subtree_ctx.as_mut() {
                ctx.group = Some(GsoGroupBuilder::new(
                    level,
                    geometry,
                    instance_id,
                    properties,
                    render,
                    NestingBudget { groups: self.active_budget.groups + 1, ..self.active_budget },
                ));
                if let Some(group) = &mut ctx.group {
                    group.wire_sequence = self.wire_sequence.clone();
                }
                #[cfg(test)]
                if let Some(group) = &mut ctx.group {
                    group.work = self.work.clone();
                    group.record_identity = self.record_identity.clone();
                }
            }
            // The `$con` record itself opens the group scope; it carries no
            // child payload, so nothing further to route for this record.
            return true;
        }

        if group_active {
            // Take the group out so its mutator can borrow `self.warnings`
            // disjointly, then store it back.
            let mut group =
                self.subtree_ctx.as_mut().and_then(|ctx| ctx.group.take()).expect("group active");
            group.handle_record(record, tag, level, &mut self.warnings);
            if let Some(ctx) = self.subtree_ctx.as_mut() {
                ctx.group = Some(group);
            }
            return true;
        }

        false
    }

    fn handle_active_subtree_record(&mut self, record: &Record, tag: TagId, level: u16) -> bool {
        if self.subtree_ctx.as_ref().is_none_or(|ctx| level <= ctx.ctrl_depth) {
            return false;
        }

        // Group (묶음 객체) routing. A gso scope whose first `ShapeComponent`
        // (`0x4C`) carries the `"$con"` type tag is a group, not a single
        // shape: every following subtree record belongs to the group's child
        // collector rather than the flat single-shape slots. Once the group
        // is active it owns all deeper records until the scope closes.
        if self.maybe_route_subtree_group_record(record, tag, level) {
            return true;
        }

        match tag {
            TagId::ListHeader => {
                if let Some(ctx) = self.subtree_ctx.as_mut() {
                    ctx.note_list_header(&record.data);
                }
            }
            TagId::ShapeComponent => {
                if self.subtree_ctx.as_ref().is_some_and(|ctx| ctx.comp_level.is_none()) {
                    flush_subtree_paragraph(
                        &mut self.current_subtree_para,
                        self.subtree_ctx.as_mut(),
                        &mut self.warnings,
                    );
                }
                if let Some(ctx) = self.subtree_ctx.as_mut() {
                    ctx.note_shape_component(&record.data, level, &mut self.warnings);
                }
            }
            TagId::CtrlData => {
                if let Some(ctx) = self.subtree_ctx.as_mut() {
                    ctx.note_ctrl_data(&record.data, level);
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
                    ctx.note_shape_rectangle(&record.data);
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
            // Native arcs exist independently of rotation and arrowheads.
            TagId::ShapeComponentArc => match Hwp5ShapeComponentEllipse::parse_arc(&record.data) {
                Ok(arc) => {
                    if let Some(ctx) = self.subtree_ctx.as_mut() {
                        ctx.note_shape_ellipse(arc);
                    }
                }
                Err(e) => self.warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "shape.arc_record",
                    reason: format!(
                        "native arc record ({} bytes, expected at least 25): {e}",
                        record.data.len()
                    ),
                }),
            },
            TagId::ShapeComponentEllipse => match Hwp5ShapeComponentEllipse::parse(&record.data) {
                Ok(ellipse) => {
                    if let Some(ctx) = self.subtree_ctx.as_mut() {
                        ctx.note_shape_ellipse(ellipse);
                    }
                }
                Err(e) => self.warnings.push(Hwp5Warning::ProjectionFallback {
                    subject: "shape.ellipse_record",
                    reason: format!("ellipse/arc record: {e}"),
                }),
            },
            TagId::ShapeComponentCurve => match Hwp5ShapeComponentCurve::parse(&record.data) {
                Ok(curve) => {
                    if let Some(ctx) = self.subtree_ctx.as_mut() {
                        ctx.note_shape_curve(curve);
                    }
                }
                Err(_) => self.push_unsupported_tag(record.header.tag_id),
            },
            TagId::ShapeTextArt => {
                match crate::schema::section::Hwp5ShapeTextArt::parse(&record.data) {
                    Ok(ta) => {
                        if let Some(ctx) = self.subtree_ctx.as_mut() {
                            ctx.note_shape_text_art(ta);
                        }
                    }
                    Err(_) => self.push_unsupported_tag(record.header.tag_id),
                }
            }
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
                        &mut self.warnings,
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
            TagId::ParaLineSeg => {
                if let Some(buf) = self.current_subtree_para.as_mut() {
                    if let Some(segments) = Self::parse_para_line_segments(
                        record.header.tag_id,
                        &record.data,
                        &mut self.warnings,
                    ) {
                        buf.line_segments = segments;
                    }
                }
            }
            TagId::CtrlHeader => self.open_nested_control(record, level, ControlHost::Subtree),
            TagId::Unknown(id) => {
                self.warnings.push(Hwp5Warning::UnsupportedTag { tag_id: id, offset: 0 });
            }
            _ => {}
        }

        true
    }

    /// One-shot controls use the same typing and malformed-payload policy in
    /// body paragraphs, cells, notes, headers and shape text. A placeholder
    /// retains marker/control queue alignment when a known payload is invalid.
    fn typed_paragraph_control(
        ctrl_id: u32,
        data: &[u8],
        warnings: &mut Vec<Hwp5Warning>,
    ) -> Option<Hwp5Control> {
        use crate::schema::section::*;
        if ctrl_id == CTRL_ID_MEMO {
            return Hwp5MemoCommand::parse(data).map(|command| {
                Hwp5Control::Memo(Hwp5MemoControl { ctrl_id, command, paragraphs: Vec::new() })
            });
        }
        let (parsed, name) = match ctrl_id {
            CTRL_ID_DUTMAL => {
                (Hwp5DutmalControl::parse(ctrl_id, data).map(Hwp5Control::Dutmal), "dutmal")
            }
            CTRL_ID_COMPOSE => {
                (Hwp5ComposeControl::parse(ctrl_id, data).map(Hwp5Control::Compose), "compose")
            }
            CTRL_ID_INDEXMARK => (
                Hwp5IndexMarkControl::parse(ctrl_id, data).map(Hwp5Control::IndexMark),
                "indexmark",
            ),
            CTRL_ID_FIELD_SUMMERY => (
                Hwp5SummaryControl::parse(ctrl_id, data).map(Hwp5Control::SummaryField),
                "summary_field",
            ),
            CTRL_ID_FIELD_DATE_CODE => (
                Hwp5DateCodeControl::parse(ctrl_id, data).map(Hwp5Control::DateCodeField),
                "date_code_field",
            ),
            CTRL_ID_FIELD_PATH => (
                Hwp5PathFieldControl::parse(ctrl_id, data).map(Hwp5Control::PathField),
                "path_field",
            ),
            CTRL_ID_FIELD_CROSSREF => {
                (Hwp5CrossRefControl::parse(ctrl_id, data).map(Hwp5Control::CrossRef), "crossref")
            }
            CTRL_ID_NEW_NUMBER => (
                Hwp5NewNumberControl::parse(ctrl_id, data).map(Hwp5Control::NewNumber),
                "new_number",
            ),
            CTRL_ID_PAGE_HIDING => (
                Hwp5PageHidingControl::parse(ctrl_id, data).map(Hwp5Control::PageHiding),
                "page_hiding",
            ),
            CTRL_ID_ATNO => (
                Hwp5InlinePageNumberControl::parse(ctrl_id, data)
                    .map(Hwp5Control::InlinePageNumber),
                "inline_page_number",
            ),
            CTRL_ID_CLICK_HERE => {
                return Some(match Hwp5ClickHereControl::parse(ctrl_id, data) {
                    Ok(control) => Hwp5Control::ClickHere(control),
                    Err(err) => {
                        warnings.push(Hwp5Warning::DroppedControl { control: "clickhere", reason: format!("malformed %clk CtrlHeader payload ({}); preserving queue placeholder", err.as_str()) });
                        Hwp5Control::Unknown { ctrl_id, header_data: data.to_vec() }
                    }
                })
            }
            _ => return None,
        };
        Some(parsed.unwrap_or_else(|| {
            warnings.push(Hwp5Warning::DroppedControl {
                control: name,
                reason: format!(
                    "malformed {name} CtrlHeader payload; preserving queue placeholder"
                ),
            });
            Hwp5Control::Unknown { ctrl_id, header_data: data.to_vec() }
        }))
    }

    fn handle_top_level_record(&mut self, record: &Record, tag: TagId, level: u16) {
        // Pending-state guards run before the normal arms (task #90 —
        // extracted helpers; see each method for the wave rationale).
        if self.try_intercept_memo_cluster(record, tag, level) {
            return;
        }
        if self.try_attach_clickhere_name(record, tag, level) {
            return;
        }

        match tag {
            TagId::ParaHeader if level == self.base_level => {
                if let Some(buf) = self.current.take() {
                    self.paragraphs.push(buf.finish(&mut self.warnings));
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
            TagId::ParaLineSeg => {
                if let Some(buf) = self.current.as_mut() {
                    if let Some(segments) = Self::parse_para_line_segments(
                        record.header.tag_id,
                        &record.data,
                        &mut self.warnings,
                    ) {
                        buf.line_segments = segments;
                    }
                }
            }
            TagId::PageDef => match Hwp5PageDef::parse(&record.data) {
                Ok(pd) => self.page_def = Some(pd),
                Err(_) => self.push_unsupported_tag(record.header.tag_id),
            },
            TagId::PageBorderFill => match Hwp5PageBorderFill::parse(&record.data) {
                Some(pbf) => self.page_border_fills.push(pbf),
                None => self.push_unsupported_tag(record.header.tag_id),
            },
            TagId::CtrlHeader => self.handle_ctrl_header(record, level),
            TagId::EqEdit => self.handle_eqedit_record(record),
            TagId::ListHeader => {}
            TagId::Unknown(id) => {
                self.warnings.push(Hwp5Warning::UnsupportedTag { tag_id: id, offset: 0 });
            }
            _ => {}
        }
    }

    /// Memo content-cluster state machine (Wave 12e — task #90 extract).
    ///
    /// `HWPTAG_MEMO_LIST` clusters appear at the end of the section's
    /// last body paragraph as a sequence of (MemoList, ListHeader,
    /// ParaHeader, ParaText, CharShape...) records at lvl=1/2. They are
    /// intercepted *before* the normal arms so the body paragraph's
    /// `self.current` text/char_shape stay untouched — without this,
    /// the cluster's lvl=2 ParaText would fall into the normal
    /// `ParaText` arm and overwrite the body text (Wave 12e-Memo
    /// corruption root cause).
    ///
    /// Returns `true` when the record was consumed by the cluster
    /// region (caller must not dispatch it further). A body
    /// `ParaHeader` at lvl=0 closes the capture and falls through so
    /// the next body paragraph opens cleanly.
    fn try_intercept_memo_cluster(&mut self, record: &Record, tag: TagId, level: u16) -> bool {
        if self.nested {
            return false;
        }
        if matches!(tag, TagId::MemoList) && level == 1 {
            if let Some(memo_id) = parse_memo_list_id(&record.data) {
                self.flush_memo_content_capture();
                self.memo_content_capture = Some(MemoContentCapture {
                    memo_id,
                    note_sources: Vec::new(),
                    saw_list_header: false,
                    parser: Box::new(Self {
                        base_level: level,
                        budget: self.budget.enter(false, false).expect("section memo budget"),
                        nested: true,
                        wire_sequence: self.wire_sequence.clone(),
                        #[cfg(test)]
                        work: self.work.clone(),
                        #[cfg(test)]
                        record_identity: self.record_identity.clone(),
                        ..Self::default()
                    }),
                });
            } else {
                self.push_unsupported_tag(record.header.tag_id);
            }
            return true;
        }
        if self.memo_content_capture.is_some() {
            if matches!(tag, TagId::ParaHeader) && level == 0 {
                // Body paragraph boundary closes the cluster region; fall
                // through to the normal `ParaHeader` arm so the next body
                // para is opened cleanly.
                self.flush_memo_content_capture();
            } else if self.try_capture_memo_record(record, tag, level) {
                return true;
            }
        }
        false
    }

    /// ClickHere name sub-record pairing (Wave 12l — task #90 extract).
    ///
    /// A `%clk` CtrlHeader is always followed by a `0x57 lvl=2`
    /// sub-record carrying the form-mode `name`. The sub-record is
    /// intercepted here so it does not emit a generic
    /// `UnsupportedTag(0x57)` warning; the parsed name merges into the
    /// pending press-field which is then pushed as
    /// `Hwp5Control::ClickHere` on the current paragraph.
    ///
    /// Returns `true` when the record was the expected sub-record. If
    /// the next top-level record is anything else, the pending
    /// press-field is finalized with `name=None` and `false` lets the
    /// record fall through to the normal dispatch — this keeps the rest
    /// of the section round-tripping (Codex review: grace-degrade
    /// rather than drop).
    fn try_attach_clickhere_name(&mut self, record: &Record, tag: TagId, level: u16) -> bool {
        if self.pending_clickhere.is_none() {
            return false;
        }
        if matches!(tag, TagId::CtrlData) && level == self.pending_clickhere_level.saturating_add(1)
        {
            let mut clickhere = self
                .pending_clickhere
                .take()
                .expect("pending_clickhere checked Some at the start of this method");
            use crate::schema::section::ClickHereNameSubrecord;
            match crate::schema::section::Hwp5ClickHereControl::parse_name_subrecord(&record.data) {
                ClickHereNameSubrecord::Named(name) => clickhere.name = Some(name),
                // Construction starts with `name = None`, so a
                // nameless-but-valid sub-record needs no write.
                ClickHereNameSubrecord::Unnamed => {}
                ClickHereNameSubrecord::Malformed => {
                    self.warnings.push(Hwp5Warning::ProjectionFallback {
                        subject: "field.clickhere",
                        reason: "malformed %clk name sub-record (0x57); \
                                 keeping the press-field with name=None"
                            .to_string(),
                    });
                }
            }
            if let Some(buf) = self.current.as_mut() {
                buf.controls.push(Hwp5Control::ClickHere(clickhere));
            } else if let Some(buf) = self.current_subtree_para.as_mut() {
                buf.controls.push(Hwp5Control::ClickHere(clickhere));
            }
            return true;
        }
        // Anything else: finalize with name=None and fall through to
        // the normal dispatch so the current record can be processed.
        self.flush_pending_clickhere();
        false
    }

    /// The sole CtrlHeader interpreter, used by body paragraphs and every
    /// nested host. Host routing never selects supported control families.
    ///
    /// Families that open nested regions (`tbl `, `head`/`foot`/`fn`/`en`/
    /// `gso `) push parser state; one-shot field/annotation controls parse
    /// and attach to the current paragraph; unrecognized ids fall through
    /// to `Hwp5Control::Unknown` for round-trip preservation.
    fn handle_ctrl_header(&mut self, record: &Record, level: u16) {
        let ctrl_id = parse_ctrl_id(&record.data);
        if matches!(
            ctrl_id,
            crate::ctrl_ids::CTRL_ID_BOOKMARK_POINT | crate::ctrl_ids::CTRL_ID_BOOKMARK_SPAN
        ) {
            if let Some(p) = self.current.as_mut() {
                let index = p.controls.len();
                p.controls.push(Hwp5Control::Bookmark {
                    ctrl_id,
                    header_data: record.data.clone(),
                    name: None,
                });
                self.pending_bookmark = Some((level, index));
            } else {
                self.warnings.push(Hwp5Warning::DroppedControl {
                    control: "bookmark",
                    reason: "no owning paragraph for bookmark".into(),
                });
            }
            return;
        }
        if matches!(
            ctrl_id,
            CTRL_ID_TABLE
                | CTRL_ID_GSO
                | CTRL_ID_HEADER
                | CTRL_ID_FOOTER
                | CTRL_ID_FOOTNOTE
                | CTRL_ID_ENDNOTE
        ) {
            let Some(budget) = self.budget.enter(ctrl_id == CTRL_ID_GSO, false) else {
                self.warnings.push(Hwp5Warning::DroppedControl {
                    control: if ctrl_id == CTRL_ID_TABLE { "table" } else { "nested_control" },
                    reason: "shared container depth cap exceeded".to_string(),
                });
                if ctrl_id == CTRL_ID_GSO {
                    self.warnings.push(Hwp5Warning::ContentLost {
                        kind: "shape_text_depth".to_string(),
                        reason: "shared container depth cap exceeded".to_string(),
                        location: format!("record level {level}"),
                    });
                }
                self.skipped_control = Some(level);
                return;
            };
            self.active_budget = budget;
        }
        if ctrl_id == CTRL_ID_TABLE {
            self.push_table_context(level, extract_ctrl_header_instance_id(&record.data, ctrl_id));
            self.note_table_ctrl(&record.data);
        } else if matches!(
            ctrl_id,
            CTRL_ID_HEADER | CTRL_ID_FOOTER | CTRL_ID_FOOTNOTE | CTRL_ID_ENDNOTE | CTRL_ID_GSO
        ) {
            let geometry = if ctrl_id == CTRL_ID_GSO {
                Hwp5ShapeComponentGeometry::parse_from_ctrl_header(&record.data).ok()
            } else {
                None
            };
            // HWP 5.0 spec §4.3.10.3 표 140: bytes [4..8] of
            // the ctrl_header payload are the property field
            // (e.g. header/footer applyPageType in bits 0~1).
            // Header is 4 bytes, so falling back to 0 keeps
            // pre-spec defaults consistent.
            let properties_raw = ctrl_header_properties_raw(&record.data);
            // Wave 12p Step 5 (#134): CtrlHeader instance_id lives at
            // a family-specific offset. fn/en use data[16..20], gso
            // uses data[36..40]. See `extract_ctrl_header_instance_id`
            // for offset table. HWPX cross-ref Command 의 target ID
            // 와 매칭. 추출 불가 시 0.
            let instance_id = extract_ctrl_header_instance_id(&record.data, ctrl_id);
            self.subtree_ctx = Some(NestedSubtreeContext::new(
                level,
                ctrl_id,
                properties_raw,
                instance_id,
                geometry,
            ));
            let wire_order = self.wire_sequence.get();
            self.wire_sequence.set(wire_order + 1);
            self.subtree_ctx.as_mut().expect("just opened subtree").wire_order = wire_order;
        } else if ctrl_id == CTRL_ID_EQED {
            // The `eqed` ctrl carries only geometry; its HancomEQN
            // script lives in the child `HWPTAG_EQEDIT` (0x58) record.
            // Stash the geometry + instance_id and finalize when
            // that child arrives (Wave 12p Step 1c-2). Wave 12p
            // Step 5: eqed instance_id at data[36..40] (audited).
            let geometry = Hwp5ShapeComponentGeometry::parse_from_ctrl_header(&record.data)
                .unwrap_or(Hwp5ShapeComponentGeometry {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                    outer_margin: [0; 4],
                });
            let instance_id = extract_ctrl_header_instance_id(&record.data, ctrl_id);
            self.eqed_pending = Some((geometry, instance_id));
        } else if ctrl_id == CTRL_ID_CLICK_HERE {
            // `%clk` ctrl carries the CLICK_HERE (누름틀) press-field
            // hint/help text. The form-mode `name` lives in the
            // immediately following `0x57 lvl=2` sub-record. Flush
            // any orphaned pending first (defensive), then store
            // the parsed control so the next `0x57` can attach the
            // name. (Wave 12l.)
            self.flush_pending_clickhere();
            self.pending_clickhere_level = level;
            match Self::typed_paragraph_control(ctrl_id, &record.data, &mut self.warnings) {
                Some(Hwp5Control::ClickHere(control)) => self.pending_clickhere = Some(control),
                Some(control) => {
                    if let Some(buf) = self.current.as_mut() {
                        buf.controls.push(control);
                    }
                }
                None => {}
            }
        } else if let Some(control) =
            Self::typed_paragraph_control(ctrl_id, &record.data, &mut self.warnings)
        {
            if let Some(buf) = self.current.as_mut() {
                buf.controls.push(control);
            }
        } else if let Some(buf) = self.current.as_mut() {
            // Snapshot the `secd` ctrl property word for
            // projection-level Visibility decoding (gap B,
            // HWP 5.0 spec §4.3.10.1 표 130). The ctrl
            // continues to flow through the Unknown path so
            // downstream semantic adapter still emits the
            // SectionColumnDef inline item from the inline
            // 0x02 control byte; the property word is just an
            // additional sidecar capture.
            if ctrl_id == CTRL_ID_SECD
                && self.section_def_properties.is_none()
                && record.data.len() >= 8
            {
                self.section_def_properties = Some(u32::from_le_bytes([
                    record.data[4],
                    record.data[5],
                    record.data[6],
                    record.data[7],
                ]));
                if let Some(bytes) = record.data.get(18..20) {
                    let id = u16::from_le_bytes([bytes[0], bytes[1]]);
                    if id > 1 {
                        self.warnings.push(Hwp5Warning::ParserFallback {
                            subject: "section.outlineShapeIDRef",
                            reason: format!("original numbering {id}: section outline selection is not carried; HWPX output selects 1"),
                        });
                    }
                }
                // 시작번호 `[20..28]` (쪽/그림/표/수식 u16 — F1 실측 §1.2).
                // all-or-none: 28바이트 미만이면 캡처하지 않는다 (부분값을
                // 기본값과 섞어 날조 금지 — 계획 W1 fail-safe).
                if let Some(bytes) = record.data.get(20..28) {
                    let word = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
                    self.section_def_start_numbers = Some(Hwp5SectionStartNumbers {
                        page: word(0),
                        pic: word(2),
                        tbl: word(4),
                        equation: word(6),
                    });
                }
            }
            // Snapshot the `cold` (column definition) ctrl, mirroring the
            // `secd` sidecar capture. Payload after the 4-byte ctrl_id:
            // `[4..6]` u16 property (bits 0-1 = type, bits 2-9 = column
            // count, bits 10-11 = direction), `[6..8]` u16 column gap in
            // HWPUNIT. The ctrl still flows through the Unknown path so the
            // inline `0x02` SectionColumnDef marker is unaffected; this is
            // an additional capture for projection-level `ColumnSettings`.
            if ctrl_id == CTRL_ID_COLUMN_DEF && self.column_def.is_none() && record.data.len() >= 8
            {
                let data = &record.data;
                let property = u16::from_le_bytes([data[4], data[5]]);
                let gap = u16::from_le_bytes([data[6], data[7]]);
                let col_count = ((property >> 2) & 0xFF) as u8;
                let same_width = (property >> 12) & 1 == 1;
                // Layout after gap: per-column widths (count×u16, only when
                // !same_width) → u16 reserved → Border(6B): kind u8, width u8,
                // color u32 (COLORREF). See HWP5_WIRE_SPEC §6.3.
                let mut off = 8usize;
                if !same_width {
                    off += col_count as usize * 2;
                }
                off += 2; // reserved word
                let border = if data.len() >= off + 6 {
                    Some(Hwp5ColumnBorder {
                        kind: data[off],
                        width: data[off + 1],
                        color: u32::from_le_bytes([
                            data[off + 2],
                            data[off + 3],
                            data[off + 4],
                            data[off + 5],
                        ]),
                    })
                } else {
                    None
                };
                self.column_def = Some(Hwp5ColumnDef { col_count, gap, border });
            }
            buf.controls.push(Hwp5Control::Unknown { ctrl_id, header_data: record.data.clone() });
        }
    }

    /// Finalizes the equation started by the preceding `eqed` ctrl
    /// using its paired `HWPTAG_EQEDIT` (`0x58`) script record
    /// (task #90 — extracted from `handle_top_level_record`).
    fn handle_eqedit_record(&mut self, record: &Record) {
        // Finalize the equation started by the preceding `eqed` ctrl.
        if let Some((geometry, instance_id)) = self.eqed_pending.take() {
            match Hwp5EqEdit::parse(&record.data) {
                Ok(eqedit) => {
                    if let Some(buf) = self.current.as_mut() {
                        buf.controls.push(Hwp5Control::Equation(Hwp5EquationControl {
                            ctrl_id: CTRL_ID_EQED,
                            geometry,
                            script: eqedit.script,
                            instance_id,
                        }));
                    }
                }
                Err(_) => self.push_unsupported_tag(record.header.tag_id),
            }
        }
    }

    /// Absorbs a record into the active memo-content cluster capture.
    ///
    /// Returns `true` if the record belongs to the cluster (regardless of
    /// whether it was structurally usable), so the caller can short-circuit
    /// the normal record dispatch. The cluster region carries the same
    /// `ParaText`/`ParaCharShape`/`ParaLineSeg` records as a normal body
    /// paragraph; isolating them here keeps body-paragraph fields untouched.
    fn try_capture_memo_record(&mut self, record: &Record, tag: TagId, level: u16) -> bool {
        let Some(capture) = self.memo_content_capture.as_mut() else {
            return false;
        };
        if tag == TagId::ListHeader && level == 1 {
            capture.saw_list_header = true;
        } else if capture.saw_list_header {
            if let Some(source) =
                master::NoteNumberingSource::from_record(record, self.record_index - 1)
            {
                capture.note_sources.push(source);
            }
            capture.parser.handle_record(record);
        }
        true
    }

    /// Finalizes a pending `%clk` ClickHere control whose trailing
    /// `0x57` name sub-record never arrived (Wave 12l). Pushes it onto
    /// the current paragraph with `name=None` and emits a warning so
    /// the audit baseline can attribute the form-mode identifier loss
    /// to the ClickHere code path instead of a generic
    /// `UnsupportedTag(0x47)` bucket.
    fn flush_pending_clickhere(&mut self) {
        let Some(clickhere) = self.pending_clickhere.take() else {
            return;
        };
        // Exactly one warning per orphan: ProjectionFallback when we
        // *can* still attach (partial loss), DroppedControl when we
        // cannot (full loss). Emitting both would double-count the same
        // event in audit baselines.
        if let Some(buf) = self.current.as_mut() {
            buf.controls.push(Hwp5Control::ClickHere(clickhere));
            self.warnings.push(Hwp5Warning::ProjectionFallback {
                subject: "field.clickhere",
                reason: "%clk press-field missing its trailing 0x57 name sub-record; \
                         keeping the field with name=None"
                    .to_string(),
            });
        } else if let Some(buf) = self.current_subtree_para.as_mut() {
            buf.controls.push(Hwp5Control::ClickHere(clickhere));
            self.warnings.push(Hwp5Warning::ProjectionFallback {
                subject: "field.clickhere",
                reason: "%clk press-field missing its trailing 0x57 name sub-record; \
                         keeping the field with name=None"
                    .to_string(),
            });
        } else {
            // No paragraph buffer to attach to (e.g. malformed
            // ParaHeader caused `parse_para_header_buf` to return None
            // earlier). Surface the silent data loss with a targeted
            // DroppedControl warning instead of a silent drop — per
            // Wave 12l quality review (HIGH).
            self.warnings.push(Hwp5Warning::DroppedControl {
                control: "clickhere",
                reason: "no active paragraph buffer to attach orphan %clk press-field".to_string(),
            });
        }
    }

    /// Commits the active memo-content capture (if any) into
    /// `memo_contents`. First cluster wins on duplicate `memo_id` (warning
    /// surfaced); zero-content clusters insert an empty entry so the
    /// matching placeholder still resolves cleanly.
    fn flush_memo_content_capture(&mut self) {
        if let Some(mut capture) = self.memo_content_capture.take() {
            let memo_id = capture.memo_id;
            let note_sources = std::mem::take(&mut capture.note_sources);
            let paragraphs = capture.into_paragraphs(&mut self.warnings);
            match self.memo_contents.entry(memo_id) {
                Entry::Vacant(slot) => {
                    slot.insert(paragraphs);
                    if !note_sources.is_empty() {
                        self.memo_note_sources.insert(memo_id, note_sources);
                    }
                }
                Entry::Occupied(_) => {
                    self.warnings.push(Hwp5Warning::DroppedControl {
                        control: "memo_content_cluster",
                        reason: format!("duplicate cluster for memo_id={memo_id}; keeping first"),
                    });
                }
            }
        }
    }

    /// Joins captured memo-content clusters to their inline `Memo`
    /// placeholders by `memo_id`. Called once from `finish()` after the
    /// last body paragraph is committed.
    fn attach_memo_contents_to_placeholders(&mut self) {
        fill_memo_placeholders(
            &mut self.paragraphs,
            &mut self.memo_contents,
            &mut self.warnings,
            self.budget,
        );
        // The join removes each cluster exactly once. Discard body-owned
        // sources first, then diagnose only clusters consumed by each master.
        self.memo_note_sources.retain(|id, _| self.memo_contents.contains_key(id));
        for master in &mut self.master_pages {
            fill_memo_placeholders(
                &mut master.paragraphs,
                &mut self.memo_contents,
                &mut self.warnings,
                self.budget.enter(false, false).expect("section master budget"),
            );
            self.memo_note_sources.retain(|id, sources| {
                if self.memo_contents.contains_key(id) {
                    return true;
                }
                for source in sources {
                    self.warnings.push(source.warning(&master.location(), Some(*id)));
                }
                false
            });
        }
        for orphan_id in self.memo_contents.keys() {
            self.warnings.push(Hwp5Warning::DroppedControl {
                control: "memo_content_cluster",
                reason: format!(
                    "orphan content cluster for memo_id={orphan_id}; no matching MEMO ctrl"
                ),
            });
        }
        self.memo_contents.clear();
    }

    fn finish(mut self) -> SectionResult {
        self.close_master();
        if !self.nested && self.secd_declaration.is_some() {
            self.check_master_counts();
        }
        self.close_nested_control();
        // Drain any orphan `%clk` whose `0x57` name sub-record was
        // missing or replaced by another top-level record (Wave 12l).
        self.flush_pending_clickhere();
        while let Some(ctx) = self.table_stack.pop() {
            let finished = ctx.finalize(&mut self.warnings);
            attach_finished_table(
                &mut self.current,
                &mut self.table_stack,
                finished,
                &mut self.warnings,
            );
        }
        flush_subtree_paragraph(
            &mut self.current_subtree_para,
            self.subtree_ctx.as_mut(),
            &mut self.warnings,
        );
        attach_finished_subtree(&mut self.current, self.subtree_ctx.take(), &mut self.warnings);

        if let Some(buf) = self.current.take() {
            self.paragraphs.push(buf.finish(&mut self.warnings));
        }

        // Flush any in-flight memo-content capture (cluster at end-of-stream)
        // then join captured clusters to inline `Memo` placeholders by
        // `memo_id`. See `phase12e_memo_design.md` for the wire layout.
        self.flush_memo_content_capture();
        if !self.nested {
            self.attach_memo_contents_to_placeholders();
        }

        self.warn_nested_section_state();
        SectionResult {
            master_pages: self.master_pages,
            paragraphs: self.paragraphs,
            page_def: self.page_def,
            section_def_properties: self.section_def_properties,
            section_def_start_numbers: self.section_def_start_numbers,
            page_border_fills: self.page_border_fills,
            column_def: self.column_def,
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

    fn parse_para_line_segments(
        tag_id: u16,
        data: &[u8],
        warnings: &mut Vec<Hwp5Warning>,
    ) -> Option<Vec<Hwp5ParaLineSeg>> {
        match Hwp5ParaLineSeg::parse_all(data) {
            Ok(segments) => Some(segments),
            Err(_) => {
                warnings.push(Hwp5Warning::UnsupportedTag { tag_id, offset: 0 });
                None
            }
        }
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
/// Parses the 4-byte payload of a `HWPTAG_MEMO_LIST` (0x5D) record as a
/// little-endian u32 memo identifier. The same id appears at slash index 2
/// of the matching `%unk MEMO/{shapeId}/{memo_id}/{instId}` command string.
fn parse_memo_list_id(data: &[u8]) -> Option<u32> {
    if data.len() >= 4 {
        Some(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
    } else {
        None
    }
}

/// Walks paragraphs (recursively into table cells / nested subtrees) and
/// fills every empty `Hwp5Control::Memo` placeholder whose `memo_id`
/// matches an entry in `memo_contents`. Entries are removed as they are
/// consumed so the caller can surface remaining orphans as warnings.
fn fill_memo_placeholders(
    paragraphs: &mut [Hwp5Paragraph],
    memo_contents: &mut HashMap<u32, Vec<Hwp5Paragraph>>,
    warnings: &mut Vec<Hwp5Warning>,
    budget: NestingBudget,
) {
    for para in paragraphs {
        para.controls
            .retain_mut(|control| fill_control_memos(control, memo_contents, warnings, budget));
    }
}

fn fill_control_memos(
    control: &mut Hwp5Control,
    memo_contents: &mut HashMap<u32, Vec<Hwp5Paragraph>>,
    warnings: &mut Vec<Hwp5Warning>,
    budget: NestingBudget,
) -> bool {
    // Only containers consume nesting. A compose/equation at depth 32 is
    // still a valid leaf, just like plain text in the same paragraph.
    if !matches!(
        control,
        Hwp5Control::Memo(_)
            | Hwp5Control::Group(_)
            | Hwp5Control::Table(_)
            | Hwp5Control::Header(_)
            | Hwp5Control::Footer(_)
            | Hwp5Control::Footnote(_)
            | Hwp5Control::Endnote(_)
            | Hwp5Control::TextBox(_)
            | Hwp5Control::Ellipse(_)
            | Hwp5Control::Polygon(_)
    ) {
        return true;
    }
    // MEMO_LIST bodies are joined after parsing, so references can otherwise
    // create a new, unbounded nesting chain across individually shallow lists.
    let Some(budget) = budget.enter(false, false) else {
        warnings.push(Hwp5Warning::DroppedControl {
            control: "memo_depth",
            reason:
                "shared container depth cap exceeded while joining memo bodies; subtree removed"
                    .to_string(),
        });
        return false;
    };
    match control {
        Hwp5Control::Memo(memo) => {
            if memo.paragraphs.is_empty() {
                if let Some(content) = memo_contents.remove(&memo.command.memo_id) {
                    memo.paragraphs = content;
                } else {
                    warnings.push(Hwp5Warning::DroppedControl {
                        control: "memo",
                        reason: format!("no content cluster for memo_id={}", memo.command.memo_id),
                    });
                }
            }
            fill_memo_placeholders(&mut memo.paragraphs, memo_contents, warnings, budget);
        }
        Hwp5Control::Group(group) => {
            group.children.retain_mut(|child| {
                if !fill_control_memos(&mut child.control, memo_contents, warnings, budget) {
                    return false;
                }
                if let Some(text_budget) = budget.enter(false, false) {
                    fill_memo_placeholders(
                        &mut child.paragraphs,
                        memo_contents,
                        warnings,
                        text_budget,
                    );
                } else if !child.paragraphs.is_empty() {
                    warnings.push(Hwp5Warning::DroppedControl {
                        control: "memo_depth",
                        reason: "shared container depth cap exceeded at group child text; subtree removed"
                            .to_string(),
                    });
                    child.paragraphs.clear();
                }
                true
            });
        }
        Hwp5Control::Table(table) => {
            if let Some(caption) = &mut table.caption {
                fill_memo_placeholders(&mut caption.paragraphs, memo_contents, warnings, budget);
            }
            for cell in &mut table.cells {
                fill_memo_placeholders(&mut cell.paragraphs, memo_contents, warnings, budget);
            }
        }
        Hwp5Control::Header(subtree)
        | Hwp5Control::Footer(subtree)
        | Hwp5Control::Footnote(subtree)
        | Hwp5Control::Endnote(subtree) => {
            fill_memo_placeholders(&mut subtree.paragraphs, memo_contents, warnings, budget);
        }
        Hwp5Control::TextBox(shape) => {
            fill_memo_placeholders(&mut shape.paragraphs, memo_contents, warnings, budget)
        }
        Hwp5Control::Ellipse(shape) => {
            fill_memo_placeholders(&mut shape.paragraphs, memo_contents, warnings, budget)
        }
        Hwp5Control::Polygon(shape) => {
            fill_memo_placeholders(&mut shape.paragraphs, memo_contents, warnings, budget)
        }
        _ => {}
    }
    true
}

fn parse_ctrl_id(data: &[u8]) -> u32 {
    if data.len() < 4 {
        return 0;
    }
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

/// Parsed table-level fields recovered from a `Table` record payload.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedTableHeader {
    rows: u16,
    cols: u16,
    page_break: Hwp5TablePageBreak,
    repeat_header: bool,
    cell_spacing: i16,
    border_fill_id: Option<u16>,
    inner_margin: Option<Hwp5TableCellMargin>,
    no_adjust: bool,
    cell_zones: Vec<Hwp5TableCellZone>,
    /// The zone count the record declares when it holds fewer complete
    /// entries (a damaged list); `None` when the list is whole or absent.
    cell_zones_declared: Option<u16>,
    /// The record ends one byte into the zone count (damage).
    cell_zone_count_cut: bool,
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
            inner_margin: None,
            no_adjust: false,
            cell_zones: Vec::new(),
            cell_zones_declared: None,
            cell_zone_count_cut: false,
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

    // Bytes 10..18: the default cell padding (left, right, top, bottom).
    // 이것을 실패시키는 것: 오프셋 ±2 / 안쪽·바깥 맞바꿈
    // (`table_margins_and_cell_margin_flag_are_read`).
    let inner_margin = Hwp5TableCellMargin::read(data, 10);
    // Property bit 3 is HWPX `noAdjust` (Hancom twins 6,061/6,061; 표 76
    // lists bits 0-2 only). 이것을 실패시키는 것: bit 2 읽기
    // (`table_property_bit_3_is_no_adjust`).
    let no_adjust = properties & 0b1000 != 0;
    // F: after the border fill id, a zone count (`u16`, 5.0.1.0 and later)
    // and 10-byte zones. No count = no zones (an older record, not damage);
    // a count the record cannot hold keeps the complete entries and reports
    // the rest. 이것을 실패시키는 것: 수 오프셋 오류 · 행·열 뒤바꿈 · 잘림
    // 보고 삭제(`table_cell_zones_are_read`).
    let zone_count_at = border_fill_offset.saturating_add(2);
    // Review r1 MEDIUM-5: one byte left is half a count — damage, not an
    // absent field. 이것을 실패시키는 것: `left == 1` 판정 삭제(`table_cell_zones_are_read`).
    let left = data.len().saturating_sub(zone_count_at);
    let (cell_zones, cell_zones_declared) = match data.get(zone_count_at..zone_count_at + 2) {
        None => (Vec::new(), None),
        Some(bytes) => {
            let declared = u16::from_le_bytes([bytes[0], bytes[1]]);
            let zones: Vec<Hwp5TableCellZone> = data[zone_count_at + 2..]
                .chunks_exact(10)
                .take(usize::from(declared))
                .map(|z| {
                    let v = |i: usize| u16::from_le_bytes([z[i], z[i + 1]]);
                    Hwp5TableCellZone {
                        start_row: v(0),
                        start_col: v(2),
                        end_row: v(4),
                        end_col: v(6),
                        border_fill_id: v(8),
                    }
                })
                .collect();
            let short = zones.len() < usize::from(declared);
            (zones, short.then_some(declared))
        }
    };
    ParsedTableHeader {
        rows,
        cols,
        page_break,
        repeat_header,
        cell_spacing,
        border_fill_id,
        inner_margin,
        no_adjust,
        cell_zones,
        cell_zones_declared,
        cell_zone_count_cut: left == 1,
    }
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
    let has_margin = properties & (1 << 16) != 0;
    let line_wrap = ((properties >> 3) & 0b11) as u8;
    let border_fill_id =
        (base.len() >= 26).then(|| u16::from_le_bytes([base[24], base[25]])).filter(|&id| id > 0);

    Ok((
        paragraph_count,
        Hwp5TableCell {
            has_margin,
            line_wrap,
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
    warnings: &mut Vec<Hwp5Warning>,
) {
    let Some(buf) = current_subtree_para.take() else {
        return;
    };
    let Some(ctx) = subtree_ctx else {
        return;
    };
    ctx.paragraphs.push(buf.finish(warnings));
}

fn attach_finished_subtree(
    current: &mut Option<ParaBuf>,
    subtree_ctx: Option<NestedSubtreeContext>,
    warnings: &mut Vec<Hwp5Warning>,
) {
    let Some(ctx) = subtree_ctx else {
        return;
    };
    let control = ctx.into_control(warnings);
    if let Some(buf) = current.as_mut() {
        buf.controls.push(control);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::schema::header::HwpVersion;
    use crate::schema::record::Record;
    use crate::schema::section::Hwp5ShapePoint;

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

    fn para_line_seg_data(segments: &[(u32, i32, i32)]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(segments.len() * 36);
        for &(textpos, vertpos, vertsize) in segments {
            buf.extend_from_slice(&textpos.to_le_bytes());
            buf.extend_from_slice(&vertpos.to_le_bytes());
            buf.extend_from_slice(&vertsize.to_le_bytes());
            buf.extend_from_slice(&1000i32.to_le_bytes());
            buf.extend_from_slice(&850i32.to_le_bytes());
            buf.extend_from_slice(&600i32.to_le_bytes());
            buf.extend_from_slice(&0i32.to_le_bytes());
            buf.extend_from_slice(&20272i32.to_le_bytes());
            buf.extend_from_slice(&393216u32.to_le_bytes());
        }
        buf
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

    /// Same as [`gso_ctrl_header_data`] but also stamps bytes `[4..8]` with a
    /// known object-property DWORD (표 70; bit0 = 글자처럼 취급) — used by
    /// W2p carry tests to prove the raw DWORD reaches `Hwp5ImageControl`.
    fn gso_ctrl_header_data_with_properties(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        properties: u32,
    ) -> Vec<u8> {
        let mut buf = gso_ctrl_header_data(x, y, width, height);
        buf[4..8].copy_from_slice(&properties.to_le_bytes());
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

    /// `secd` payload — F1 실측 레이아웃 (계획 §1.2): 속성 u32 + 열간격/세로/
    /// 가로 정렬(각 u16) + 기본 탭(u32) + 번호 문단 모양 id(u16) + 시작번호
    /// 4×u16. `starts = None` 이면 20바이트에서 자른다 (truncation 케이스).
    fn secd_ctrl_data(properties: u32, starts: Option<(u16, u16, u16, u16)>) -> Vec<u8> {
        let mut buf = CTRL_ID_SECD.to_le_bytes().to_vec();
        buf.extend_from_slice(&properties.to_le_bytes());
        buf.extend_from_slice(&[0u8; 12]); // [8..20] 열간격·정렬·탭·번호모양
        if let Some((page, pic, tbl, equation)) = starts {
            for v in [page, pic, tbl, equation] {
                buf.extend_from_slice(&v.to_le_bytes());
            }
        }
        buf
    }

    #[test]
    fn fix4_section_outline_selection_loss_is_explicit() {
        // 이것을 실패시키는 것: 미지원 구역 numbering 선택을 경고 없이 1로 대체.
        for id in [0u16, 1, 7, 99] {
            let mut ctrl = secd_ctrl_data(0, Some((1, 1, 1, 1)));
            ctrl[18..20].copy_from_slice(&id.to_le_bytes());
            let stream = [
                make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)),
                make_record(TagId::CtrlHeader, 0, &ctrl),
            ]
            .concat();
            let result = parse_body_text(&stream, &version()).unwrap();
            assert_eq!(result.warnings.iter().any(|w| matches!(w,
                Hwp5Warning::ParserFallback { subject: "section.outlineShapeIDRef", reason } if reason.contains(&id.to_string()))), id > 1);
        }
    }

    #[test]
    fn ctrl_header_secd_captures_start_numbers_when_payload_full() {
        // F1 실측 (rules-newnum-base.hwp): 한컴 5.1 은 [20..28] 에 쪽/그림/표/
        // 수식 시작번호를 쓴다 — sidecar 가 네 값을 그대로 캡처해야 한다.
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &secd_ctrl_data(0, Some((7, 2, 3, 4)))));
        let result = parse_body_text(&stream, &version()).unwrap();
        assert_eq!(
            result.section_def_start_numbers,
            Some(Hwp5SectionStartNumbers { page: 7, pic: 2, tbl: 3, equation: 4 })
        );
        assert_eq!(result.section_def_properties, Some(0));
    }

    #[test]
    fn ctrl_header_secd_truncated_payload_captures_no_start_numbers() {
        // all-or-none (Codex 결함 8): [20..28] 이 없으면 부분 캡처 대신 None —
        // 속성 word 캡처(기존 gap B 경로)는 그대로 살아 있어야 한다.
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &secd_ctrl_data(0x20, None)));
        let result = parse_body_text(&stream, &version()).unwrap();
        assert_eq!(result.section_def_start_numbers, None);
        assert_eq!(result.section_def_properties, Some(0x20));
    }

    #[test]
    fn ctrl_header_nwno_produces_typed_new_number() {
        // F1 실측 (rules-newnum-base.hwp): nwno = ctrl_id + 속성 u32(bits0-3
        // = kind) + 번호 u16 — `00 00 00 00 07 00` = 쪽 번호 7.
        let mut nwno = CTRL_ID_NEW_NUMBER.to_le_bytes().to_vec();
        nwno.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x07, 0x00]);

        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &nwno));
        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        assert!(
            para.controls.iter().any(|c| matches!(
                c,
                Hwp5Control::NewNumber(n) if n.kind_raw == 0 && n.number == 7
            )),
            "typed NewNumber expected: {:?}",
            para.controls
        );
    }

    #[test]
    fn ctrl_header_nwno_truncated_payload_warns_and_keeps_placeholder() {
        // 10바이트 미만 payload → DroppedControl 경고 + **Unknown placeholder**
        // 로 marker↔control 큐 정렬 보존 (독립 리뷰 High #1 — placeholder 가
        // 없으면 승격된 0x15 marker 가 뒤따르는 객체를 오소비한다).
        let mut nwno = CTRL_ID_NEW_NUMBER.to_le_bytes().to_vec();
        nwno.extend_from_slice(&[0x00, 0x00]);

        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &nwno));
        let result = parse_body_text(&stream, &version()).unwrap();
        assert!(
            matches!(
                result.paragraphs[0].controls.as_slice(),
                [Hwp5Control::Unknown { ctrl_id, .. }] if *ctrl_id == CTRL_ID_NEW_NUMBER
            ),
            "placeholder 로 큐 정렬 보존: {:?}",
            result.paragraphs[0].controls
        );
        assert!(result
            .warnings
            .iter()
            .any(|w| matches!(w, Hwp5Warning::DroppedControl { control: "new_number", .. })));
    }

    #[test]
    fn ctrl_header_pghd_produces_typed_page_hiding() {
        // F2-① 실측 (rules-pagehide-base.hwp): pghd = ctrl_id + 속성 u32 —
        // `20 00 00 00` = bit5 = 쪽번호만 감춤.
        let mut pghd = CTRL_ID_PAGE_HIDING.to_le_bytes().to_vec();
        pghd.extend_from_slice(&[0x20, 0x00, 0x00, 0x00]);

        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &pghd));
        let result = parse_body_text(&stream, &version()).unwrap();
        assert!(
            result.paragraphs[0].controls.iter().any(|c| matches!(
                c,
                Hwp5Control::PageHiding(p) if p.mask == 0x20
            )),
            "typed PageHiding expected: {:?}",
            result.paragraphs[0].controls
        );
    }

    #[test]
    fn ctrl_header_pghd_truncated_payload_warns_and_keeps_placeholder() {
        let mut pghd = CTRL_ID_PAGE_HIDING.to_le_bytes().to_vec();
        pghd.extend_from_slice(&[0x20]);

        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &pghd));
        let result = parse_body_text(&stream, &version()).unwrap();
        assert!(
            matches!(
                result.paragraphs[0].controls.as_slice(),
                [Hwp5Control::Unknown { ctrl_id, .. }] if *ctrl_id == CTRL_ID_PAGE_HIDING
            ),
            "placeholder 로 큐 정렬 보존 (독립 리뷰 High #1): {:?}",
            result.paragraphs[0].controls
        );
        assert!(result
            .warnings
            .iter()
            .any(|w| matches!(w, Hwp5Warning::DroppedControl { control: "page_hiding", .. })));
    }

    #[test]
    fn ctrl_header_cold_captures_column_def() {
        // `cold` ctrl: ctrl_id(4) + property u16 (bits 2-9 = colCount=2)
        // + gap u16 (2268) + 8 zero bytes — mirrors the native 2-column wire.
        let mut cold = CTRL_ID_COLUMN_DEF.to_le_bytes().to_vec();
        cold.extend_from_slice(&0x0008u16.to_le_bytes()); // property: colCount=2 at bits 2-9
        cold.extend_from_slice(&2268u16.to_le_bytes()); // gap
        cold.extend_from_slice(&[0u8; 8]);

        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &cold));

        let result = parse_body_text(&stream, &version()).unwrap();
        let col = result.column_def.expect("cold ctrl should be captured");
        assert_eq!(col.col_count, 2);
        assert_eq!(col.gap, 2268);
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
            Hwp5Control::Unknown { ctrl_id: id, .. } => assert_eq!(*id, ctrl_id),
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
    fn footnote_control_captures_nested_paragraphs_after_list_header() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &ctrl_header_data(CTRL_ID_FOOTNOTE)));
        stream.extend(make_record(TagId::ListHeader, 1, &[0u8; 4]));
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_data("footnote body")));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        match &para.controls[0] {
            Hwp5Control::Footnote(subtree) => {
                assert_eq!(subtree.ctrl_id, CTRL_ID_FOOTNOTE);
                assert_eq!(subtree.paragraphs.len(), 1);
                assert_eq!(subtree.paragraphs[0].text, "footnote body");
            }
            other => panic!("expected Footnote, got {:?}", other),
        }
    }

    #[test]
    fn endnote_control_captures_nested_paragraphs_after_list_header() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &ctrl_header_data(CTRL_ID_ENDNOTE)));
        stream.extend(make_record(TagId::ListHeader, 1, &[0u8; 4]));
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_data("endnote body")));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        match &para.controls[0] {
            Hwp5Control::Endnote(subtree) => {
                assert_eq!(subtree.ctrl_id, CTRL_ID_ENDNOTE);
                assert_eq!(subtree.paragraphs.len(), 1);
                assert_eq!(subtree.paragraphs[0].text, "endnote body");
            }
            other => panic!("expected Endnote, got {:?}", other),
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
            Hwp5Control::Unknown { ctrl_id, .. } => assert_eq!(*ctrl_id, CTRL_ID_GSO),
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

    pub(super) fn one_cell_table(stream: &mut Vec<u8>, paragraphs: u32) {
        stream.extend(make_record(TagId::CtrlHeader, 0, &ctrl_header_data(CTRL_ID_TABLE)));
        stream.extend(make_record(TagId::Table, 1, &basic_table_data(1, 1)));
        stream.extend(make_record(
            TagId::ListHeader,
            1,
            &list_header_table_cell_data(TestCellSpec {
                paragraph_count: paragraphs,
                legacy_u16_count: false,
                properties: 0,
                column: 0,
                row: 0,
                col_span: 1,
                row_span: 1,
                width: 9000,
                height: 3000,
                margin: Hwp5TableCellMargin { left: 0, right: 0, top: 0, bottom: 0 },
                border_fill_id: Some(1),
            }),
        ));
    }

    /// `gso` rect with a text list at `level`: component, list header and
    /// one paragraph per entry of `texts` (an entry of `None` holds an inner
    /// rect with text "inner" instead), then the rect body record.
    fn text_rect(stream: &mut Vec<u8>, level: u16, texts: &[Option<&str>]) {
        stream.extend(make_record(
            TagId::CtrlHeader,
            level,
            &gso_ctrl_header_data(0, 0, 9000, 3000),
        ));
        stream.extend(make_record(TagId::ShapeComponent, level + 1, b"cer$cer$"));
        stream.extend(make_record(
            TagId::ListHeader,
            level + 2,
            &[texts.len() as u8, 0, 0, 0, 0, 0],
        ));
        for text in texts {
            stream.extend(make_record(TagId::ParaHeader, level + 2, &para_header_data(0, 0)));
            match text {
                Some(t) => {
                    stream.extend(make_record(TagId::ParaText, level + 3, &para_text_data(t)));
                }
                None => {
                    stream.extend(make_record(
                        TagId::ParaText,
                        level + 3,
                        &para_text_with_control_ref("", ""),
                    ));
                    text_rect(stream, level + 3, &[Some("inner")]);
                }
            }
        }
        stream.extend(make_record(TagId::ShapeComponentRect, level + 2, &[0u8; 9]));
    }

    /// A chain of `depth` text boxes, each holding the next in its text, the
    /// last holding "leaf".
    fn nested_text_rects(stream: &mut Vec<u8>, level: u16, depth: usize) {
        stream.extend(make_record(
            TagId::CtrlHeader,
            level,
            &gso_ctrl_header_data(0, 0, 9000, 3000),
        ));
        stream.extend(make_record(TagId::ShapeComponent, level + 1, b"cer$cer$"));
        stream.extend(make_record(TagId::ListHeader, level + 2, &[1, 0, 0, 0, 0, 0]));
        stream.extend(make_record(TagId::ParaHeader, level + 2, &para_header_data(0, 0)));
        if depth > 1 {
            stream.extend(make_record(
                TagId::ParaText,
                level + 3,
                &para_text_with_control_ref("", ""),
            ));
            nested_text_rects(stream, level + 3, depth - 1);
        } else {
            stream.extend(make_record(TagId::ParaText, level + 3, &para_text_data("leaf")));
        }
        stream.extend(make_record(TagId::ShapeComponentRect, level + 2, &[0u8; 9]));
    }

    /// Branch review B-M3: text boxes nested in text boxes are read up to
    /// the budget; a deeper one is skipped and reported.
    /// 이것을 실패시키는 것: 상한 삭제(전부 읽힘, 보고 없음) · 상한을 하나 낮추기.
    #[test]
    fn shape_text_nesting_is_capped() {
        let read = |depth: usize| {
            let mut stream = Vec::new();
            stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::ParaText, 0, &para_text_with_control_ref("", "")));
            nested_text_rects(&mut stream, 0, depth);
            let result = parse_body_text(&stream, &version()).unwrap();
            let mut boxes = 0;
            let mut at = result.paragraphs[0].controls.first();
            while let Some(Hwp5Control::TextBox(tb)) = at {
                boxes += 1;
                at = tb.paragraphs.first().and_then(|p| p.controls.first());
            }
            let capped = result.warnings.iter().any(
                |w| matches!(w, Hwp5Warning::ContentLost { kind, .. } if kind == "shape_text_depth"),
            );
            (boxes, capped)
        };
        // The body box's text context is depth 0 and the first shape in its
        // text opens depth 1, so `SHAPE_TEXT_MAX_DEPTH` + 2 boxes are read.
        let max = usize::from(SHAPE_TEXT_MAX_DEPTH) + 2;
        assert_eq!(read(max), (max, false));
        assert_eq!(read(max + 5), (max, true));
    }

    fn text_box_texts(control: &Hwp5Control) -> Vec<String> {
        let Hwp5Control::TextBox(tb) = control else { panic!("text box expected: {control:?}") };
        tb.paragraphs.iter().map(|p| p.text.clone()).collect()
    }

    /// 1a-T1/T2: a text box in a table cell keeps its paragraphs, and a text
    /// box inside it (in the cell and in a body text box) keeps its own — the
    /// body path's collection one level down. Before, both were plain rects
    /// without text.
    ///
    /// 이것을 실패시키는 것: 칸 gso 의 글 수집 삭제(글 없는 사각형) / 안쪽 개체로
    /// 보내지 않기(안쪽 글이 바깥 문단으로 섞이거나 사라짐) / 사각형+글 목록을
    /// 글상자로 만들지 않기.
    #[test]
    fn shape_text_in_a_cell_and_in_a_text_box() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        one_cell_table(&mut stream, 1);
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_with_control_ref("", "")));
        text_rect(&mut stream, 2, &[None, Some("first"), Some("second")]);
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 0, &para_text_with_control_ref("", "")));
        text_rect(&mut stream, 0, &[None]);

        let result = parse_body_text(&stream, &version()).unwrap();
        let Hwp5Control::Table(table) = &result.paragraphs[0].controls[0] else { panic!() };
        let cell = &table.cells[0].paragraphs[0].controls;
        assert_eq!(cell.len(), 1, "{cell:?}");
        let Hwp5Control::TextBox(outer) = &cell[0] else { panic!("{cell:?}") };
        assert_eq!(text_box_texts(&cell[0]).len(), 3);
        assert_eq!(&text_box_texts(&cell[0])[1..], ["first", "second"]);
        assert_eq!(text_box_texts(&outer.paragraphs[0].controls[0]), ["inner"]);

        let body = &result.paragraphs[1].controls;
        let Hwp5Control::TextBox(body_box) = &body[0] else { panic!("{body:?}") };
        assert_eq!(text_box_texts(&body_box.paragraphs[0].controls[0]), ["inner"]);
    }

    /// A record deeper than a shape's text (a table inside its text) is not
    /// the shape's own: a rectangle record under that table does not turn
    /// the ellipse holding it into a text box.
    ///
    /// 이것을 실패시키는 것: 글 수준보다 깊은 기록을 자기 기록으로 받기
    /// (`_ => false`) — 타원이 글상자로 분류된다.
    #[test]
    fn records_under_an_object_in_shape_text_are_not_the_shape_s_own() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        one_cell_table(&mut stream, 1);
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_with_control_ref("", "")));
        stream.extend(make_record(TagId::CtrlHeader, 2, &gso_ctrl_header_data(0, 0, 9000, 3000)));
        stream.extend(make_record(TagId::ShapeComponent, 3, b"lle$lle$"));
        stream.extend(make_record(TagId::ListHeader, 4, &[1, 0, 0, 0, 0, 0]));
        stream.extend(make_record(TagId::ParaHeader, 4, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 5, &para_text_with_control_ref("", "")));
        stream.extend(make_record(TagId::CtrlHeader, 5, &ctrl_header_data(CTRL_ID_TABLE)));
        stream.extend(make_record(TagId::ShapeComponentRect, 7, &[0u8; 9]));
        stream.extend(make_record(TagId::ShapeComponentEllipse, 4, &[0u8; 60]));

        let result = parse_body_text(&stream, &version()).unwrap();
        let Hwp5Control::Table(table) = &result.paragraphs[0].controls[0] else { panic!() };
        let cell = &table.cells[0].paragraphs[0].controls;
        assert!(matches!(cell[..], [Hwp5Control::Ellipse(_)]), "{cell:?}");
    }

    /// A rectangle record's byte 0 is its corner ratio, kept on the shape's
    /// look — for a body shape and for one in a table cell.
    /// 이것을 실패시키는 것: 두 문맥의 비율 읽기 중 하나를 지우기.
    #[test]
    fn rect_corner_ratio_is_read() {
        let rect = |stream: &mut Vec<u8>, level: u16| {
            stream.extend(make_record(
                TagId::CtrlHeader,
                level,
                &gso_ctrl_header_data(0, 0, 3000, 1000),
            ));
            stream.extend(make_record(
                TagId::ShapeComponent,
                level + 1,
                &shape_component_render_data(b"cer$", true, 0, (3000, 1000), 0.0),
            ));
            let mut body = vec![20u8];
            body.extend([0u8; 32]);
            stream.extend(make_record(TagId::ShapeComponentRect, level + 2, &body));
        };
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 0, &para_text_with_control_ref("", "")));
        rect(&mut stream, 0);
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        one_cell_table(&mut stream, 1);
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_with_control_ref("", "")));
        rect(&mut stream, 2);
        let result = parse_body_text(&stream, &version()).unwrap();
        let ratio = |c: &Hwp5Control| match c {
            Hwp5Control::Rect(r) => r.look.as_ref().map(|l| l.corner_ratio),
            other => panic!("{other:?}"),
        };
        assert_eq!(ratio(&result.paragraphs[0].controls[0]), Some(20));
        let Hwp5Control::Table(table) = &result.paragraphs[1].controls[0] else { panic!() };
        assert_eq!(ratio(&table.cells[0].paragraphs[0].controls[0]), Some(20));
    }

    /// A rectangle whose component has no readable outline/fill block (here
    /// the bare type tags) cannot carry its corner ratio: the ratio is
    /// reported — for a body shape and for one in a table cell — not dropped
    /// (review r1). A ratio of 0 is not reported.
    #[test]
    fn corner_ratio_without_a_look_is_reported() {
        let rect = |stream: &mut Vec<u8>, level: u16, ratio: u8| {
            stream.extend(make_record(
                TagId::CtrlHeader,
                level,
                &gso_ctrl_header_data(0, 0, 3000, 1000),
            ));
            stream.extend(make_record(TagId::ShapeComponent, level + 1, b"cer$cer$"));
            let mut body = vec![ratio];
            body.extend([0u8; 32]);
            stream.extend(make_record(TagId::ShapeComponentRect, level + 2, &body));
        };
        let reported = |ratio: u8| {
            let mut stream = Vec::new();
            stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::ParaText, 0, &para_text_with_control_ref("", "")));
            rect(&mut stream, 0, ratio);
            stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
            one_cell_table(&mut stream, 1);
            stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::ParaText, 2, &para_text_with_control_ref("", "")));
            rect(&mut stream, 2, ratio);
            let result = parse_body_text(&stream, &version()).unwrap();
            result
                .warnings
                .iter()
                .filter(|w| {
                    matches!(
                        w,
                        Hwp5Warning::ProjectionFallback { subject: "shape.corner_ratio", .. }
                    )
                })
                .count()
        };
        assert_eq!(reported(20), 2);
        assert_eq!(reported(0), 0);
    }

    /// A footer in a cell paragraph: its paragraph becomes the footer's, not
    /// the cell's; a control after it at the footer's own level is the cell
    /// paragraph's again; a footer that ends the table is still attached.
    ///
    /// 이것을 실패시키는 것: 닫는 조건을 `level < depth` 로(같은 수준의 다음
    /// 제어가 꼬리말 문단으로 들어감) / `finalize` 의 닫기 삭제(표 끝 꼬리말
    /// 소실) / 칸 머리말·꼬리말 갈래 삭제(꼬리말 문단이 칸 문단으로).
    #[test]
    fn cell_footer_paragraphs_belong_to_the_footer() {
        let footer = |stream: &mut Vec<u8>, text: &str| {
            stream.extend(make_record(TagId::CtrlHeader, 2, &ctrl_header_data(CTRL_ID_FOOTER)));
            stream.extend(make_record(TagId::ListHeader, 3, &[1, 0, 0, 0, 0, 0]));
            stream.extend(make_record(TagId::ParaHeader, 3, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::ParaText, 4, &para_text_data(text)));
        };
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        one_cell_table(&mut stream, 1);
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_data("cell")));
        footer(&mut stream, "F1");
        let mut nwno = ctrl_header_data(CTRL_ID_NEW_NUMBER);
        nwno.extend_from_slice(&[0, 0, 0, 0, 1, 0]);
        stream.extend(make_record(TagId::CtrlHeader, 2, &nwno));
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        one_cell_table(&mut stream, 1);
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        footer(&mut stream, "F2");

        let result = parse_body_text(&stream, &version()).unwrap();
        let cell_controls = |para: usize| {
            let Hwp5Control::Table(table) = &result.paragraphs[para].controls[0] else {
                panic!("table expected");
            };
            assert_eq!(table.cells[0].paragraphs.len(), 1, "footer text leaked into the cell");
            table.cells[0].paragraphs[0].controls.clone()
        };
        let first = cell_controls(0);
        let [Hwp5Control::Footer(f1), Hwp5Control::NewNumber(_)] = &first[..] else {
            panic!("{first:?}");
        };
        assert_eq!(f1.paragraphs.len(), 1);
        assert_eq!(f1.paragraphs[0].text, "F1");
        let second = cell_controls(1);
        let [Hwp5Control::Footer(f2)] = &second[..] else { panic!("{second:?}") };
        assert_eq!(f2.paragraphs[0].text, "F2");
    }

    /// A table `CtrlHeader` with a placement word and a geometry
    /// (`y`, `x`, width, height at bytes 8–24).
    fn table_ctrl_data(properties: u32, x: i32, y: i32, width: u32, height: u32) -> Vec<u8> {
        let mut data = CTRL_ID_TABLE.to_le_bytes().to_vec();
        data.extend_from_slice(&properties.to_le_bytes());
        data.extend_from_slice(&y.to_le_bytes());
        data.extend_from_slice(&x.to_le_bytes());
        data.extend_from_slice(&width.to_le_bytes());
        data.extend_from_slice(&height.to_le_bytes());
        data
    }

    /// A-T3/A-T4 from the wire (review r1): a body table's, a nested
    /// table's and a header table's `CtrlHeader` placement word and height
    /// reach `Hwp5Table::ctrl` — before, the tests built `ctrl` by hand and
    /// dropping the decoder's note passed them.
    ///
    /// 이것을 실패시키는 것: 표를 여는 네 곳 중 하나에서 `note_table_ctrl` 을
    /// 지우기(그 표의 `ctrl` 이 `None`).
    #[test]
    fn table_ctrl_is_read_from_the_wire_everywhere() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(
            TagId::CtrlHeader,
            0,
            &table_ctrl_data(0x082a_4310, 13, 27_415, 50_737, 43_643),
        ));
        stream.extend(make_record(TagId::Table, 1, &basic_table_data(1, 1)));
        stream.extend(make_record(
            TagId::ListHeader,
            1,
            &list_header_table_cell_data(TestCellSpec {
                paragraph_count: 1,
                legacy_u16_count: false,
                properties: 0,
                column: 0,
                row: 0,
                col_span: 1,
                row_span: 1,
                width: 9000,
                height: 3000,
                margin: Hwp5TableCellMargin { left: 0, right: 0, top: 0, bottom: 0 },
                border_fill_id: Some(1),
            }),
        ));
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(
            TagId::CtrlHeader,
            2,
            &table_ctrl_data(0x082a_2311, 0, 0, 8000, 1200),
        ));
        stream.extend(make_record(TagId::Table, 3, &basic_table_data(1, 1)));
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 1, &ctrl_header_data(CTRL_ID_HEADER)));
        stream.extend(make_record(TagId::ListHeader, 2, &[1, 0, 0, 0, 0, 0]));
        stream.extend(make_record(TagId::ParaHeader, 2, &para_header_data(0, 0)));
        stream.extend(make_record(
            TagId::CtrlHeader,
            3,
            &table_ctrl_data(0x082a_2311, 0, 0, 7000, 900),
        ));
        stream.extend(make_record(TagId::Table, 4, &basic_table_data(1, 1)));
        // A table in a cell's footer.
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        one_cell_table(&mut stream, 1);
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 2, &ctrl_header_data(CTRL_ID_FOOTER)));
        stream.extend(make_record(TagId::ListHeader, 3, &[1, 0, 0, 0, 0, 0]));
        stream.extend(make_record(TagId::ParaHeader, 3, &para_header_data(0, 0)));
        stream.extend(make_record(
            TagId::CtrlHeader,
            4,
            &table_ctrl_data(0x082a_2311, 0, 0, 6000, 700),
        ));
        stream.extend(make_record(TagId::Table, 5, &basic_table_data(1, 1)));

        let result = parse_body_text(&stream, &version()).unwrap();
        let ctrl = |t: &Hwp5Table| t.ctrl.as_ref().map(|c| (c.properties, c.geometry.height));
        let Hwp5Control::Table(body) = &result.paragraphs[0].controls[0] else { panic!() };
        assert_eq!(ctrl(body), Some((0x082a_4310, 43_643)));
        let Hwp5Control::Table(nested) = &body.cells[0].paragraphs[0].controls[0] else {
            panic!("{:?}", body.cells[0].paragraphs[0].controls)
        };
        assert_eq!(ctrl(nested), Some((0x082a_2311, 1200)));
        let Hwp5Control::Header(header) = &result.paragraphs[1].controls[0] else { panic!() };
        let Hwp5Control::Table(in_header) = &header.paragraphs[0].controls[0] else { panic!() };
        assert_eq!(ctrl(in_header), Some((0x082a_2311, 900)));
        let Hwp5Control::Table(host) = &result.paragraphs[2].controls[0] else { panic!() };
        let Hwp5Control::Footer(footer) = &host.cells[0].paragraphs[0].controls[0] else {
            panic!("{:?}", host.cells[0].paragraphs[0].controls)
        };
        let Hwp5Control::Table(in_footer) = &footer.paragraphs[0].controls[0] else { panic!() };
        assert_eq!(ctrl(in_footer), Some((0x082a_2311, 700)));
        // The output height: each table's hint is its `CtrlHeader` height
        // (body, then the header's — the layout-hint queue's order).
        let hints = crate::layout_hint_patch::capture_layout_hints(
            std::slice::from_ref(&result),
            &mut Vec::new(),
        );
        let heights: Vec<_> = hints[0].tables.iter().map(|t| t.height).collect();
        // Body tables (the body's, its nested one, the cell-footer host),
        // then the header's, then the footer's.
        assert_eq!(heights[..2], [Some(43_643), Some(1200)]);
        assert_eq!(heights[3..], [Some(900), Some(700)]);
    }

    /// §19.22 D1·D2: the table body record's bytes 10..18 are the default
    /// cell padding, the table `CtrlHeader`'s bytes 28..36 the outer margin
    /// (all four sides different, so a swapped side or offset shows), and a
    /// cell's property bit 16 says whether it has its own margin — a cell
    /// without it uses the table padding (`margin: None`).
    ///
    /// 이것을 실패시키는 것: 두 오프셋 ±2 / 안쪽·바깥 맞바꿈 / 칸 비트 번호
    /// 15·17 / 판정 반전.
    #[test]
    fn table_margins_and_cell_margin_flag_are_read() {
        let mut ctrl = table_ctrl_data(0, 0, 0, 9000, 3000);
        ctrl.resize(28, 0);
        for v in [101i16, 102, 103, 104] {
            ctrl.extend_from_slice(&v.to_le_bytes());
        }
        ctrl.extend_from_slice(&[0u8; 8]);
        let mut body = basic_table_data(1, 2);
        for (i, v) in [510i16, 520, 566, 576].iter().enumerate() {
            body[10 + 2 * i..12 + 2 * i].copy_from_slice(&v.to_le_bytes());
        }
        let cell = |column: u16, properties: u32| {
            list_header_table_cell_data(TestCellSpec {
                paragraph_count: 1,
                legacy_u16_count: false,
                properties,
                column,
                row: 0,
                col_span: 1,
                row_span: 1,
                width: 4500,
                height: 3000,
                margin: Hwp5TableCellMargin { left: 11, right: 12, top: 13, bottom: 14 },
                border_fill_id: Some(1),
            })
        };
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 1, &ctrl));
        stream.extend(make_record(TagId::Table, 2, &body));
        // Bits 15 and 17 set but not 16: no own margin.
        stream.extend(make_record(TagId::ListHeader, 2, &cell(0, (1 << 15) | (1 << 17))));
        stream.extend(make_record(TagId::ParaHeader, 2, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ListHeader, 2, &cell(1, 1 << 16)));
        stream.extend(make_record(TagId::ParaHeader, 2, &para_header_data(0, 0)));
        let result = parse_body_text(&stream, &version()).unwrap();
        let (doc, _) = crate::projection::project_to_core(vec![result]).expect("project");
        let hwpforge_core::run::RunContent::Table(table) =
            &doc.sections()[0].paragraphs[0].runs[0].content
        else {
            panic!("{:?}", doc.sections()[0].paragraphs[0].runs)
        };
        let m = |l, r, t, b| hwpforge_core::table::TableMargin {
            left: hwpforge_foundation::HwpUnit::new(l).unwrap(),
            right: hwpforge_foundation::HwpUnit::new(r).unwrap(),
            top: hwpforge_foundation::HwpUnit::new(t).unwrap(),
            bottom: hwpforge_foundation::HwpUnit::new(b).unwrap(),
        };
        assert_eq!(table.in_margin, Some(m(510, 520, 566, 576)));
        assert_eq!(table.out_margin, Some(m(101, 102, 103, 104)));
        assert_eq!(table.rows[0].cells[0].margin, None);
        assert_eq!(table.rows[0].cells[1].margin, Some(m(11, 12, 13, 14)));
    }

    /// S19: a cell without its own margin keeps its own values as a dormant
    /// margin unless they are the table's padding.
    ///
    /// 이것을 실패시키는 것: 휴면 값을 싣지 않기 / 표 여백과 같은 값을 지우지 않기 /
    /// 안쪽 여백이 없는 표의 기본값을 바꾸기.
    #[test]
    fn dormant_cell_margin_is_kept() {
        let mut ctrl = table_ctrl_data(0, 0, 0, 9000, 3000);
        ctrl.resize(28, 0);
        for v in [101i16, 102, 103, 104] {
            ctrl.extend_from_slice(&v.to_le_bytes());
        }
        ctrl.extend_from_slice(&[0u8; 8]);
        let mut body = basic_table_data(1, 3);
        for (i, v) in [510i16, 520, 566, 576].iter().enumerate() {
            body[10 + 2 * i..12 + 2 * i].copy_from_slice(&v.to_le_bytes());
        }
        let cell_with = |column: u16, properties: u32, m: [i16; 4]| {
            list_header_table_cell_data(TestCellSpec {
                paragraph_count: 1,
                legacy_u16_count: false,
                properties,
                column,
                row: 0,
                col_span: 1,
                row_span: 1,
                width: 4500,
                height: 3000,
                margin: Hwp5TableCellMargin { left: m[0], right: m[1], top: m[2], bottom: m[3] },
                border_fill_id: Some(1),
            })
        };
        let cell = |column: u16, properties: u32| cell_with(column, properties, [11, 12, 13, 14]);
        let dormant_of = |body: &[u8], padding: [i16; 4]| {
            let mut stream = Vec::new();
            stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::CtrlHeader, 1, &ctrl));
            stream.extend(make_record(TagId::Table, 2, body));
            // Bits 15 and 17 set but not 16: no own margin.
            stream.extend(make_record(TagId::ListHeader, 2, &cell(0, (1 << 15) | (1 << 17))));
            stream.extend(make_record(TagId::ParaHeader, 2, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::ListHeader, 2, &cell(1, 1 << 16)));
            stream.extend(make_record(TagId::ParaHeader, 2, &para_header_data(0, 0)));
            // No own margin, values equal to the table's padding.
            stream.extend(make_record(TagId::ListHeader, 2, &cell_with(2, 0, padding)));
            stream.extend(make_record(TagId::ParaHeader, 2, &para_header_data(0, 0)));
            let result = parse_body_text(&stream, &version()).unwrap();
            let (doc, _) = crate::projection::project_to_core(vec![result]).expect("project");
            let hwpforge_core::run::RunContent::Table(table) =
                &doc.sections()[0].paragraphs[0].runs[0].content
            else {
                panic!("{:?}", doc.sections()[0].paragraphs[0].runs)
            };
            table.rows[0].cells.iter().map(|c| c.dormant_margin).collect::<Vec<_>>()
        };
        let m = |l, r, t, b| hwpforge_core::table::TableMargin {
            left: hwpforge_foundation::HwpUnit::new(l).unwrap(),
            right: hwpforge_foundation::HwpUnit::new(r).unwrap(),
            top: hwpforge_foundation::HwpUnit::new(t).unwrap(),
            bottom: hwpforge_foundation::HwpUnit::new(b).unwrap(),
        };
        // S19: the first cell (no own margin) keeps its values; the third,
        // whose values are the table's padding, carries none; the second
        // has its own margin and no dormant one.
        assert_eq!(dormant_of(&body, [510, 520, 566, 576]), [Some(m(11, 12, 13, 14)), None, None]);
        // A table record too short for its padding: the padding is the
        // 510/510/141/141 default.
        assert_eq!(
            dormant_of(&body[..10], [510, 510, 141, 141]),
            [Some(m(11, 12, 13, 14)), None, None]
        );
    }

    /// §19.22 D3: cell property bits 3–4 — 0 break, 1 squeeze, 2 keep (no
    /// Hancom twin, reported), 3 no meaning (reported, read as break). Bits
    /// 5–6 (vertical align) set in every cell so a shifted read shows.
    ///
    /// 이것을 실패시키는 것: 비트 3–4 를 5–6 으로 / 2 의 경고 삭제 / 3 을
    /// squeeze 로.
    #[test]
    fn cell_line_wrap_is_read_from_bits_3_and_4() {
        use hwpforge_core::table::CellLineWrap;
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 1, &ctrl_header_data(CTRL_ID_TABLE)));
        stream.extend(make_record(TagId::Table, 2, &basic_table_data(1, 4)));
        for column in 0..4u16 {
            let properties = (u32::from(column) << 3) | (1 << 5) | (1 << 16);
            stream.extend(make_record(
                TagId::ListHeader,
                2,
                &list_header_table_cell_data(TestCellSpec {
                    paragraph_count: 1,
                    legacy_u16_count: false,
                    properties,
                    column,
                    row: 0,
                    col_span: 1,
                    row_span: 1,
                    width: 2000,
                    height: 1000,
                    margin: Hwp5TableCellMargin { left: 0, right: 0, top: 0, bottom: 0 },
                    border_fill_id: Some(1),
                }),
            ));
            stream.extend(make_record(TagId::ParaHeader, 2, &para_header_data(0, 0)));
        }
        let result = parse_body_text(&stream, &version()).unwrap();
        let (doc, warnings) = crate::projection::project_to_core(vec![result]).expect("project");
        let hwpforge_core::run::RunContent::Table(table) =
            &doc.sections()[0].paragraphs[0].runs[0].content
        else {
            panic!()
        };
        let wraps: Vec<_> = table.rows[0].cells.iter().map(|c| c.line_wrap).collect();
        assert_eq!(
            wraps,
            [CellLineWrap::Break, CellLineWrap::Squeeze, CellLineWrap::Keep, CellLineWrap::Break]
        );
        let subjects: Vec<&str> = warnings
            .iter()
            .filter_map(|w| match w {
                Hwp5Warning::ProjectionFallback { subject, .. }
                    if subject.starts_with("cell.line_wrap") =>
                {
                    Some(*subject)
                }
                _ => None,
            })
            .collect();
        assert_eq!(subjects, ["cell.line_wrap_unverified", "cell.line_wrap"]);
    }

    /// A shape text `LIST_HEADER` of `len` bytes: u32 paragraph count 1,
    /// then `properties`, then zeros.
    fn shape_text_list_header(len: usize, properties: u32) -> Vec<u8> {
        let mut data = 1u32.to_le_bytes().to_vec();
        data.extend_from_slice(&properties.to_le_bytes());
        data.resize(len, 0);
        data
    }

    /// §19.22 D4: a shape's text list header carries a u32 paragraph count
    /// and its property word at bytes 4..8 (vertical align bits 5–6), in all
    /// three readers — a body text box (33 bytes), a group child (45 bytes)
    /// and a text box in a table cell (33 bytes). A header shorter than 8
    /// bytes is reported, in each reader.
    ///
    /// 이것을 실패시키는 것: 세 읽기 중 하나를 옛 오프셋(2..6)으로 / 짧은 길이
    /// 보고를 지우기(세 곳 각각).
    #[test]
    fn shape_text_list_header_reads_its_own_layout() {
        let build = |len: usize| {
            let lh = shape_text_list_header(len.min(33), 0x20);
            let lh45 = shape_text_list_header(len.min(45), 0x20);
            let mut stream = Vec::new();
            // Body text box.
            stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
            stream.extend(make_record(
                TagId::CtrlHeader,
                0,
                &gso_ctrl_header_data(0, 0, 7000, 5000),
            ));
            stream.extend(make_record(TagId::ShapeComponent, 1, b"cer$cer$"));
            stream.extend(make_record(TagId::ListHeader, 2, &lh));
            stream.extend(make_record(TagId::ParaHeader, 2, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::ParaText, 3, &para_text_data("body")));
            stream.extend(make_record(TagId::ShapeComponentRect, 2, &[0u8; 9]));
            // Group with a text-box child.
            stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::ParaText, 1, &para_text_with_control_ref("", "")));
            stream.extend(make_record(
                TagId::CtrlHeader,
                1,
                &gso_ctrl_header_data(0, 0, 3000, 1000),
            ));
            stream.extend(make_record(
                TagId::ShapeComponent,
                2,
                &shape_component_render_data(
                    &SHAPE_COMPONENT_TYPE_GROUP,
                    true,
                    0,
                    (3000, 1000),
                    0.0,
                ),
            ));
            for x in [0.0, 2000.0] {
                stream.extend(make_record(
                    TagId::ShapeComponent,
                    3,
                    &shape_component_render_data(b"cer$", false, 1, (1000, 1000), x),
                ));
                stream.extend(make_record(TagId::ShapeComponentRect, 4, &[0u8; 9]));
            }
            stream.extend(make_record(TagId::ListHeader, 4, &lh45));
            stream.extend(make_record(TagId::ParaHeader, 4, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::ParaText, 5, &para_text_data("child")));
            // A text box in a table cell.
            stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
            one_cell_table(&mut stream, 1);
            stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::ParaText, 2, &para_text_with_control_ref("", "")));
            stream.extend(make_record(
                TagId::CtrlHeader,
                2,
                &gso_ctrl_header_data(0, 0, 9000, 3000),
            ));
            stream.extend(make_record(TagId::ShapeComponent, 3, b"cer$cer$"));
            stream.extend(make_record(TagId::ListHeader, 4, &lh));
            stream.extend(make_record(TagId::ParaHeader, 4, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::ParaText, 5, &para_text_data("cell")));
            stream.extend(make_record(TagId::ShapeComponentRect, 4, &[0u8; 9]));
            parse_body_text(&stream, &version()).unwrap()
        };
        let props = |result: &SectionResult| {
            let Hwp5Control::TextBox(body) = &result.paragraphs[0].controls[0] else {
                panic!("{:?}", result.paragraphs[0].controls)
            };
            let Hwp5Control::Group(group) = &result.paragraphs[1].controls[0] else {
                panic!("{:?}", result.paragraphs[1].controls)
            };
            let Hwp5Control::Table(table) = &result.paragraphs[2].controls[0] else { panic!() };
            let Hwp5Control::TextBox(in_cell) = &table.cells[0].paragraphs[0].controls[0] else {
                panic!("{:?}", table.cells[0].paragraphs[0].controls)
            };
            [
                body.list_header_properties,
                group.children.last().unwrap().list_header_properties,
                in_cell.list_header_properties,
            ]
        };
        let shorts = |result: &SectionResult| {
            result
                .warnings
                .iter()
                .filter(|w| {
                    matches!(
                        w,
                        Hwp5Warning::ProjectionFallback {
                            subject: "textbox.list_header_short",
                            ..
                        }
                    )
                })
                .count()
        };
        let full = build(usize::MAX);
        assert_eq!(props(&full), [Some(0x20); 3]);
        assert_eq!(shorts(&full), 0);
        let short = build(6);
        assert_eq!(props(&short), [None; 3]);
        assert_eq!(shorts(&short), 3, "{:?}", short.warnings);
    }

    /// A 33-byte shape text list header with margins `m` (left, right, top,
    /// bottom) at bytes 8..16.
    fn margined_list_header(len: usize, m: [i16; 4]) -> Vec<u8> {
        let mut data = shape_text_list_header(16, 0);
        for (i, v) in m.iter().enumerate() {
            data[8 + 2 * i..10 + 2 * i].copy_from_slice(&v.to_le_bytes());
        }
        data.resize(len, 0);
        data
    }

    /// §19.22 D4: a text box's margin is bytes 8..16 of its text list
    /// header, read in all three readers (body, group child, in a cell) and
    /// carried to Core — all four sides different, so a swapped pair shows;
    /// an ellipse's or polygon's margin other than 283 is reported; a header
    /// of 12 bytes is reported and leaves the margin `None`.
    ///
    /// 이것을 실패시키는 것: 세 읽기 중 하나의 여백 읽기 / 오프셋 / 왼·오, 위·아래
    /// 맞바꿈 / 타원·다각형 보고 / 12 바이트 보고.
    /// S20 review r1: a top-level TextArt's initial size (7000 x 8000) and
    /// size lock reach its model and its Core frame.
    /// 이것을 실패시키는 것: 글맵시 모델에 초기 크기·속성을 넘기지 않기.
    #[test]
    fn text_art_initial_size_and_lock_are_read() {
        let mut art = Vec::new();
        for (x, y) in [(0i32, 0i32), (14173, 0), (14173, 14173), (0, 14173)] {
            art.extend_from_slice(&x.to_le_bytes());
            art.extend_from_slice(&y.to_le_bytes());
        }
        for text in ["A", "F", "S"] {
            art.extend_from_slice(&1u16.to_le_bytes());
            art.extend_from_slice(&u16::from(text.as_bytes()[0]).to_le_bytes());
        }
        for v in [1u32, 17, 120, 100, 0] {
            art.extend_from_slice(&v.to_le_bytes());
        }
        art.extend_from_slice(&[0u8; 20]);
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(
            TagId::CtrlHeader,
            0,
            &gso_ctrl_header_data_with_properties(0, 0, 5000, 3000, 1 << 20),
        ));
        stream.extend(make_record(
            TagId::ShapeComponent,
            1,
            &shape_component_render_data(&SHAPE_COMPONENT_TYPE_TEXTART, true, 0, (7000, 8000), 0.0),
        ));
        stream.extend(make_record(TagId::ShapeTextArt, 2, &art));
        let result = parse_body_text(&stream, &version()).unwrap();
        let Some(Hwp5Control::TextArt(model)) = result.paragraphs[0].controls.first() else {
            panic!("{:?}", result.paragraphs[0].controls);
        };
        assert_eq!(
            (model.original_size, model.ctrl_properties & (1 << 20)),
            (Some((7000, 8000)), 1 << 20)
        );
        let (doc, _) = crate::projection::project_to_core(vec![result]).expect("project");
        let frame = doc.sections()[0].paragraphs[0]
            .runs
            .iter()
            .find_map(|r| r.content.as_control())
            .and_then(|c| c.frame().copied())
            .unwrap();
        let u = |v| hwpforge_foundation::HwpUnit::new(v).unwrap();
        assert_eq!(frame.original_size, Some(hwpforge_foundation::Size::new(u(7000), u(8000))));
        assert!(frame.size_protect);
    }

    /// S20 review r1: a group's `CtrlHeader` size lock (bit 20) reaches the
    /// group model and the Core group — a control in the paragraph's text, a
    /// bare control header, and a group in a table cell (the inline context).
    /// 이것을 실패시키는 것: 묶음 모델에 속성을 넘기지 않기(세 경로 중 하나라도).
    #[test]
    fn group_size_lock_is_read() {
        let group = |stream: &mut Vec<u8>, level: u16, properties: u32| {
            stream.extend(make_record(
                TagId::CtrlHeader,
                level,
                &gso_ctrl_header_data_with_properties(0, 0, 3000, 1000, properties),
            ));
            stream.extend(make_record(
                TagId::ShapeComponent,
                level + 1,
                &shape_component_render_data(
                    &SHAPE_COMPONENT_TYPE_GROUP,
                    true,
                    0,
                    (3000, 1000),
                    0.0,
                ),
            ));
            for x in [0.0, 2000.0] {
                stream.extend(make_record(
                    TagId::ShapeComponent,
                    level + 2,
                    &shape_component_render_data(b"cer$", false, 1, (1000, 1000), x),
                ));
                stream.extend(make_record(TagId::ShapeComponentRect, level + 3, &[0u8; 9]));
            }
        };
        let mut stream = Vec::new();
        for properties in [1 << 20, 0] {
            stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::ParaText, 1, &para_text_with_control_ref("", "")));
            group(&mut stream, 1, properties);
            stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
            group(&mut stream, 0, properties);
        }
        let result = parse_body_text(&stream, &version()).unwrap();
        let (doc, _) = crate::projection::project_to_core(vec![result]).expect("project");
        let locks: Vec<bool> = doc.sections()[0]
            .paragraphs
            .iter()
            .flat_map(|p| &p.runs)
            .filter_map(|r| r.content.as_control())
            .filter(|c| matches!(c, hwpforge_core::control::Control::Group { .. }))
            .map(|c| c.frame().unwrap().size_protect)
            .collect();
        assert_eq!(locks, [true, true, false, false]);

        // A group in a table cell (the inline gso context).
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
                width: 9000,
                height: 3000,
                margin: Hwp5TableCellMargin { left: 0, right: 0, top: 0, bottom: 0 },
                border_fill_id: Some(7),
            }),
        ));
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_with_control_ref("", "")));
        group(&mut stream, 2, 1 << 20);
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(1, 0)));
        let result = parse_body_text(&stream, &version()).unwrap();
        let Hwp5Control::Table(table) = &result.paragraphs[0].controls[0] else { panic!() };
        let [Hwp5Control::Group(cell_group)] = &table.cells[0].paragraphs[0].controls[..] else {
            panic!()
        };
        assert_eq!(cell_group.ctrl_properties, 1 << 20);
    }

    #[test]
    fn text_margins_are_read() {
        let odd = [850i16, 700, 566, 400];
        let lh = margined_list_header(33, odd);
        let mut stream = Vec::new();
        // Body text box.
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &gso_ctrl_header_data(0, 0, 7000, 5000)));
        stream.extend(make_record(TagId::ShapeComponent, 1, b"cer$cer$"));
        stream.extend(make_record(TagId::ListHeader, 2, &lh));
        stream.extend(make_record(TagId::ParaHeader, 2, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 3, &para_text_data("body")));
        stream.extend(make_record(TagId::ShapeComponentRect, 2, &[0u8; 9]));
        // Group with a text-box child.
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 1, &para_text_with_control_ref("", "")));
        stream.extend(make_record(TagId::CtrlHeader, 1, &gso_ctrl_header_data(0, 0, 3000, 1000)));
        stream.extend(make_record(
            TagId::ShapeComponent,
            2,
            &shape_component_render_data(&SHAPE_COMPONENT_TYPE_GROUP, true, 0, (3000, 1000), 0.0),
        ));
        for x in [0.0, 2000.0] {
            stream.extend(make_record(
                TagId::ShapeComponent,
                3,
                &shape_component_render_data(b"cer$", false, 1, (1000, 1000), x),
            ));
            stream.extend(make_record(TagId::ShapeComponentRect, 4, &[0u8; 9]));
        }
        stream.extend(make_record(TagId::ListHeader, 4, &margined_list_header(45, odd)));
        stream.extend(make_record(TagId::ParaHeader, 4, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 5, &para_text_data("child")));
        // A text box in a table cell.
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        one_cell_table(&mut stream, 1);
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_with_control_ref("", "")));
        stream.extend(make_record(TagId::CtrlHeader, 2, &gso_ctrl_header_data(0, 0, 9000, 3000)));
        stream.extend(make_record(TagId::ShapeComponent, 3, b"cer$cer$"));
        stream.extend(make_record(TagId::ListHeader, 4, &lh));
        stream.extend(make_record(TagId::ParaHeader, 4, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 5, &para_text_data("cell")));
        stream.extend(make_record(TagId::ShapeComponentRect, 4, &[0u8; 9]));
        // An ellipse and a polygon with text and a margin.
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &gso_ctrl_header_data(0, 0, 7000, 5000)));
        stream.extend(make_record(TagId::ShapeComponent, 1, b"lle$lle$"));
        stream.extend(make_record(TagId::ListHeader, 2, &lh));
        stream.extend(make_record(TagId::ParaHeader, 2, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 3, &para_text_data("ellipse")));
        stream.extend(make_record(TagId::ShapeComponentEllipse, 2, &[0u8; 60]));
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &gso_ctrl_header_data(0, 0, 7000, 5000)));
        stream.extend(make_record(TagId::ShapeComponent, 1, b"lop$lop$"));
        stream.extend(make_record(TagId::ListHeader, 2, &lh));
        stream.extend(make_record(TagId::ParaHeader, 2, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 3, &para_text_data("polygon")));
        stream.extend(make_record(
            TagId::ShapeComponentPolygon,
            2,
            &shape_component_polygon_data(&[(0, 0), (100, 0), (50, 80)]),
        ));
        // A group with an ellipse child that has text and a margin.
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 1, &para_text_with_control_ref("", "")));
        stream.extend(make_record(TagId::CtrlHeader, 1, &gso_ctrl_header_data(0, 0, 3000, 1000)));
        stream.extend(make_record(
            TagId::ShapeComponent,
            2,
            &shape_component_render_data(&SHAPE_COMPONENT_TYPE_GROUP, true, 0, (3000, 1000), 0.0),
        ));
        for (x, tag) in [(0.0, b"cer$"), (2000.0, b"lle$")] {
            stream.extend(make_record(
                TagId::ShapeComponent,
                3,
                &shape_component_render_data(tag, false, 1, (1000, 1000), x),
            ));
            if tag == b"cer$" {
                stream.extend(make_record(TagId::ShapeComponentRect, 4, &[0u8; 9]));
            } else {
                stream.extend(make_record(TagId::ShapeComponentEllipse, 4, &[0u8; 60]));
            }
        }
        stream.extend(make_record(TagId::ListHeader, 4, &lh));
        stream.extend(make_record(TagId::ParaHeader, 4, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 5, &para_text_data("group ellipse")));
        // A text box with a 12-byte list header.
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &gso_ctrl_header_data(0, 0, 7000, 5000)));
        stream.extend(make_record(TagId::ShapeComponent, 1, b"cer$cer$"));
        stream.extend(make_record(TagId::ListHeader, 2, &margined_list_header(12, odd)));
        stream.extend(make_record(TagId::ParaHeader, 2, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 3, &para_text_data("short")));
        stream.extend(make_record(TagId::ShapeComponentRect, 2, &[0u8; 9]));

        let result = parse_body_text(&stream, &version()).unwrap();
        let section_warnings = result.warnings.clone();
        let (doc, warnings) = crate::projection::project_to_core(vec![result]).expect("project");
        let margin = |c: &hwpforge_core::control::Control| match c {
            hwpforge_core::control::Control::TextBox { text_margin, .. } => text_margin
                .map(|m| [m.left.as_i32(), m.right.as_i32(), m.top.as_i32(), m.bottom.as_i32()]),
            other => panic!("{other:?}"),
        };
        let paras = &doc.sections()[0].paragraphs;
        let control = |p: &hwpforge_core::Paragraph| {
            p.runs.iter().find_map(|r| match &r.content {
                hwpforge_core::run::RunContent::Control(c) => Some(c.as_ref().clone()),
                _ => None,
            })
        };
        let want = Some([850, 700, 566, 400]);
        assert_eq!(margin(&control(&paras[0]).unwrap()), want, "body");
        let hwpforge_core::control::Control::Group { ref children, .. } =
            control(&paras[1]).unwrap()
        else {
            panic!()
        };
        let hwpforge_core::control::GroupChild::Shape(child) = children.last().unwrap() else {
            panic!()
        };
        assert_eq!(margin(child), want, "group child");
        let hwpforge_core::run::RunContent::Table(table) = &paras[2].runs[0].content else {
            panic!("{:?}", paras[2].runs)
        };
        assert_eq!(margin(&control(&table.rows[0].cells[0].paragraphs[0]).unwrap()), want, "cell");
        assert_eq!(margin(&control(&paras[6]).unwrap()), None, "short");
        let count = |subject: &str, ws: &[Hwp5Warning]| {
            ws.iter()
                .filter(|w| matches!(w, Hwp5Warning::ProjectionFallback { subject: s, .. } if *s == subject))
                .count()
        };
        // The body ellipse and polygon, and the group's ellipse child.
        assert_eq!(count("shape.text_margin", &warnings), 3, "{warnings:?}");
        assert_eq!(
            count("textbox.list_header_short", &section_warnings),
            1,
            "{section_warnings:?}"
        );
    }

    /// S18: an object's outer margin (`CtrlHeader` bytes 28–36) reaches its
    /// Core placement — a body ellipse (treat-as-char, otherwise the inline
    /// default) and a group — and a negative side is reported and dropped.
    ///
    /// 이것을 실패시키는 것: 바이트 28–36 을 읽지 않기 / 도형·묶음 갈래의 여백
    /// 싣기를 각각 지우기 / 음수 보고를 지우기.
    #[test]
    fn object_outer_margin_is_read() {
        let header = |margin: [i16; 4]| {
            let mut data = gso_ctrl_header_data_with_properties(0, 0, 850, 850, 0x1);
            data.resize(28, 0);
            for v in margin {
                data.extend_from_slice(&v.to_le_bytes());
            }
            data
        };
        let ellipse = |stream: &mut Vec<u8>, margin: [i16; 4]| {
            stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::CtrlHeader, 0, &header(margin)));
            stream.extend(make_record(TagId::ShapeComponent, 1, b"lle$lle$"));
            stream.extend(make_record(TagId::ShapeComponentEllipse, 2, &[0u8; 60]));
        };
        let mut stream = Vec::new();
        ellipse(&mut stream, [0, 0, 0, 140]);
        ellipse(&mut stream, [-1, -1, -1, -1]);
        ellipse(&mut stream, [0; 4]);
        // A group with two rect children.
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 1, &para_text_with_control_ref("", "")));
        stream.extend(make_record(TagId::CtrlHeader, 1, &header([283, 283, 283, 283])));
        stream.extend(make_record(
            TagId::ShapeComponent,
            2,
            &shape_component_render_data(&SHAPE_COMPONENT_TYPE_GROUP, true, 0, (3000, 1000), 0.0),
        ));
        for x in [0.0, 2000.0] {
            stream.extend(make_record(
                TagId::ShapeComponent,
                3,
                &shape_component_render_data(b"cer$", false, 1, (1000, 1000), x),
            ));
            stream.extend(make_record(TagId::ShapeComponentRect, 4, &[0u8; 9]));
        }
        let result = parse_body_text(&stream, &version()).unwrap();
        let (doc, warnings) = crate::projection::project_to_core(vec![result]).expect("project");
        let margin = |i: usize| {
            let paragraphs = &doc.sections()[0].paragraphs;
            let hwpforge_core::run::RunContent::Control(c) = &paragraphs[i].runs[0].content else {
                panic!("{:?}", paragraphs[i].runs)
            };
            let placement = match c.as_ref() {
                hwpforge_core::control::Control::Ellipse { placement, .. }
                | hwpforge_core::control::Control::Group { placement, .. } => placement.clone(),
                other => panic!("{other:?}"),
            };
            placement
                .and_then(|p| p.out_margin)
                .map(|m| [m.left.as_i32(), m.right.as_i32(), m.top.as_i32(), m.bottom.as_i32()])
        };
        assert_eq!(margin(0), Some([0, 0, 0, 140]));
        assert_eq!(margin(1), None);
        assert_eq!(margin(2), None);
        assert_eq!(margin(3), Some([283; 4]));
        let negative = warnings
            .iter()
            .filter(|w| {
                matches!(w, Hwp5Warning::ProjectionFallback { subject: "object.out_margin", .. })
            })
            .count();
        assert_eq!(negative, 1, "{warnings:?}");
    }

    /// [`one_cell_table`] whose control header sits at `base`.
    fn one_cell_table_at(stream: &mut Vec<u8>, base: u16, text: &str) {
        stream.extend(make_record(TagId::CtrlHeader, base, &ctrl_header_data(CTRL_ID_TABLE)));
        stream.extend(make_record(TagId::Table, base + 1, &basic_table_data(1, 1)));
        stream.extend(make_record(
            TagId::ListHeader,
            base + 1,
            &list_header_table_cell_data(TestCellSpec {
                paragraph_count: 1,
                legacy_u16_count: false,
                properties: 0,
                column: 0,
                row: 0,
                col_span: 1,
                row_span: 1,
                width: 9000,
                height: 3000,
                margin: Hwp5TableCellMargin { left: 0, right: 0, top: 0, bottom: 0 },
                border_fill_id: Some(1),
            }),
        ));
        stream.extend(make_record(TagId::ParaHeader, base + 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, base + 2, &para_text_data(text)));
    }

    /// A table and a picture in a cell's footer stay objects of the footer
    /// paragraph (review r1 of §19.7 F): before, the table's cell paragraph
    /// was read as one more footer paragraph and the picture was dropped as
    /// an unknown control.
    ///
    /// 이것을 실패시키는 것: 칸 머리말·꼬리말 갈래의 `tbl`·`gso` 처리를 지우기 /
    /// 끝난 표를 꼬리말 문단 대신 칸 문단에 붙이기.
    #[test]
    fn cell_footer_keeps_its_table_and_picture() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        one_cell_table(&mut stream, 1);
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_data("cell")));
        stream.extend(make_record(TagId::CtrlHeader, 2, &ctrl_header_data(CTRL_ID_FOOTER)));
        stream.extend(make_record(TagId::ListHeader, 3, &[1, 0, 0, 0, 0, 0]));
        stream.extend(make_record(TagId::ParaHeader, 3, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 4, &para_text_data("F")));
        one_cell_table_at(&mut stream, 4, "inner");
        stream.extend(make_record(TagId::CtrlHeader, 4, &gso_ctrl_header_data(0, 0, 6400, 3200)));
        stream.extend(make_record(TagId::ShapeComponent, 5, &[0u8; 4]));
        stream.extend(make_record(TagId::ShapePicture, 5, &shape_picture_data(7)));
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 1, &para_text_data("after")));

        let result = parse_body_text(&stream, &version()).unwrap();
        assert_eq!(result.paragraphs.len(), 2);
        let Hwp5Control::Table(table) = &result.paragraphs[0].controls[0] else {
            panic!("{:?}", result.paragraphs[0].controls);
        };
        assert_eq!(table.cells[0].paragraphs.len(), 1, "footer text leaked into the cell");
        let [Hwp5Control::Footer(footer)] = &table.cells[0].paragraphs[0].controls[..] else {
            panic!("{:?}", table.cells[0].paragraphs[0].controls);
        };
        assert_eq!(footer.paragraphs.len(), 1, "inner cell text leaked into the footer");
        assert_eq!(footer.paragraphs[0].text, "F");
        let [Hwp5Control::Table(inner), Hwp5Control::Image(_)] = &footer.paragraphs[0].controls[..]
        else {
            panic!("{:?}", footer.paragraphs[0].controls);
        };
        assert_eq!(inner.cells[0].paragraphs[0].text, "inner");
        assert_eq!(result.paragraphs[1].text, "after");
    }

    /// A table in a top-level header stays an object of the header
    /// paragraph, not flattened into the header's text (review r1 of §19.7
    /// F, the same path as a cell's header).
    ///
    /// 이것을 실패시키는 것: subtree 갈래의 `tbl` 처리를 지우기 / 끝난 표를 본문
    /// 문단에 붙이기.
    #[test]
    fn header_keeps_its_table() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 1, &para_text_data("body")));
        stream.extend(make_record(TagId::CtrlHeader, 1, &ctrl_header_data(CTRL_ID_HEADER)));
        stream.extend(make_record(TagId::ListHeader, 2, &[1, 0, 0, 0, 0, 0]));
        stream.extend(make_record(TagId::ParaHeader, 2, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 3, &para_text_data("H")));
        one_cell_table_at(&mut stream, 3, "inner");
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 1, &para_text_data("after")));

        let result = parse_body_text(&stream, &version()).unwrap();
        assert_eq!(result.paragraphs.len(), 2);
        let [Hwp5Control::Header(header)] = &result.paragraphs[0].controls[..] else {
            panic!("{:?}", result.paragraphs[0].controls);
        };
        assert_eq!(header.paragraphs.len(), 1, "inner cell text leaked into the header");
        let [Hwp5Control::Table(inner)] = &header.paragraphs[0].controls[..] else {
            panic!("{:?}", header.paragraphs[0].controls);
        };
        assert_eq!(inner.cells[0].paragraphs[0].text, "inner");
    }

    /// A `ShapeComponent` payload with the rendering block: `repeated` writes
    /// the type tag twice (a component right under a `gso` control header).
    fn shape_component_render_data(
        tag: &[u8; 4],
        repeated: bool,
        group_level: u16,
        initial: (u32, u32),
        x: f64,
    ) -> Vec<u8> {
        let mut d = Vec::new();
        if repeated {
            d.extend_from_slice(tag);
        }
        d.extend_from_slice(tag);
        d.extend_from_slice(&[0u8; 8]);
        d.extend_from_slice(&group_level.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&initial.0.to_le_bytes());
        d.extend_from_slice(&initial.1.to_le_bytes());
        d.extend_from_slice(&[0u8; 22]);
        d.extend_from_slice(&(group_level + 1).to_le_bytes());
        let id = [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0];
        let trans = [1.0f64, 0.0, x, 0.0, 1.0, 0.0];
        for m in
            std::iter::once(trans).chain(std::iter::repeat_n(id, 2 * usize::from(group_level + 1)))
        {
            for v in m {
                d.extend_from_slice(&v.to_le_bytes());
            }
        }
        // line information + fill kind 0 (no fill)
        d.extend_from_slice(&[0u8; 17]);
        d
    }

    /// A group in a table cell paragraph takes the group path, as one in a
    /// body paragraph does: its two rects become group children. On the
    /// single-shape path the scope's two rect records made one rect the size
    /// of the whole group.
    ///
    /// 이것을 실패시키는 것: 표 칸 gso 문맥에서 묶음 경로를 타지 않는 것(평면 경로가
    /// 가짜 사각형 하나를 낸다).
    #[test]
    fn group_in_a_table_cell_takes_the_group_path() {
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
                width: 9000,
                height: 3000,
                margin: Hwp5TableCellMargin { left: 0, right: 0, top: 0, bottom: 0 },
                border_fill_id: Some(7),
            }),
        ));
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_with_control_ref("", "")));
        stream.extend(make_record(TagId::CtrlHeader, 2, &gso_ctrl_header_data(0, 0, 3000, 1000)));
        stream.extend(make_record(
            TagId::ShapeComponent,
            3,
            &shape_component_render_data(&SHAPE_COMPONENT_TYPE_GROUP, true, 0, (3000, 1000), 0.0),
        ));
        for x in [0.0, 2000.0] {
            stream.extend(make_record(
                TagId::ShapeComponent,
                4,
                &shape_component_render_data(b"cer$", false, 1, (1000, 1000), x),
            ));
            stream.extend(make_record(TagId::ShapeComponentRect, 5, &[0u8; 9]));
        }
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(1, 0)));

        let result = parse_body_text(&stream, &version()).unwrap();
        let Hwp5Control::Table(table) = &result.paragraphs[0].controls[0] else {
            panic!("expected a table, got {:?}", result.paragraphs[0].controls);
        };
        let controls = &table.cells[0].paragraphs[0].controls;
        let [Hwp5Control::Group(group)] = &controls[..] else {
            panic!("expected one group in the cell, got {controls:?}");
        };
        assert!(group.render.is_ok(), "{:?}", group.render);
        assert_eq!(group.children.len(), 2);
        for child in &group.children {
            assert!(matches!(child.control, Hwp5Control::Rect(_)), "{:?}", child.control);
            assert!(child.render.is_ok());
        }
    }

    /// A group in a text box's text (the other object-text context) takes the
    /// group path too.
    ///
    /// 이것을 실패시키는 것: 글상자 문단(하위 트리) gso 문맥에서 묶음 경로를 타지
    /// 않는 것.
    #[test]
    fn group_in_a_text_box_takes_the_group_path() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &gso_ctrl_header_data(0, 0, 8000, 6000)));
        stream.extend(make_record(TagId::ShapeComponentRect, 1, &[0u8; 4]));
        stream.extend(make_record(TagId::ListHeader, 1, &[0u8; 4]));
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_with_control_ref("", "")));
        stream.extend(make_record(TagId::CtrlHeader, 2, &gso_ctrl_header_data(0, 0, 3000, 1000)));
        stream.extend(make_record(
            TagId::ShapeComponent,
            3,
            &shape_component_render_data(&SHAPE_COMPONENT_TYPE_GROUP, true, 0, (3000, 1000), 0.0),
        ));
        for x in [0.0, 2000.0] {
            stream.extend(make_record(
                TagId::ShapeComponent,
                4,
                &shape_component_render_data(b"cer$", false, 1, (1000, 1000), x),
            ));
            stream.extend(make_record(TagId::ShapeComponentRect, 5, &[0u8; 9]));
        }

        let result = parse_body_text(&stream, &version()).unwrap();
        let Hwp5Control::TextBox(textbox) = &result.paragraphs[0].controls[0] else {
            panic!("expected a text box, got {:?}", result.paragraphs[0].controls);
        };
        let controls = &textbox.paragraphs[0].controls;
        let [Hwp5Control::Group(group)] = &controls[..] else {
            panic!("expected one group in the text box, got {controls:?}");
        };
        assert_eq!(group.children.len(), 2);
    }

    // ── §19 D: char shape boundaries are wire positions ──────────────────

    /// A `ParaText` payload from code units; `ctrl(code)` gives the eight
    /// units of an inline or extended control.
    fn units_data(units: &[u16]) -> Vec<u8> {
        units.iter().flat_map(|unit| unit.to_le_bytes()).collect()
    }

    fn utf16(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn ctrl(code: u16) -> Vec<u16> {
        let mut out = vec![code];
        out.extend([0u16; 7]);
        out
    }

    fn char_shape_runs_data(runs: &[(u32, u32)]) -> Vec<u8> {
        runs.iter().flat_map(|&(position, id)| char_shape_run_data(position, id)).collect()
    }

    /// One body paragraph from `units` and wire-position `runs`.
    fn decode_paragraph(units: &[u16], runs: &[(u32, u32)]) -> SectionResult {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 1, &units_data(units)));
        stream.extend(make_record(TagId::ParaCharShape, 1, &char_shape_runs_data(runs)));
        parse_body_text(&stream, &version()).unwrap()
    }

    /// The text each char shape covers: `(shape, text)` per run, cut from
    /// `Hwp5Paragraph::text` at the run positions (UTF-16), as projection
    /// cuts it.
    fn run_texts(para: &Hwp5Paragraph) -> Vec<(u32, String)> {
        let text: Vec<u16> = para.text.encode_utf16().collect();
        let runs = &para.char_shape_runs;
        runs.iter()
            .enumerate()
            .map(|(i, run)| {
                let end = runs.get(i + 1).map_or(text.len(), |next| next.position as usize);
                let slice = &text[run.position as usize..end];
                (run.char_shape_id, String::from_utf16(slice).unwrap())
            })
            .collect()
    }

    fn boundary_warnings(result: &SectionResult) -> usize {
        result
            .warnings
            .iter()
            .filter(|w| {
                matches!(
                    w,
                    Hwp5Warning::ProjectionFallback {
                        subject: "char_shape.boundary_inside_control",
                        ..
                    }
                )
            })
            .count()
    }

    /// D-T1: a tab is eight wire units and one character. The boundary at
    /// wire 10 is after "나".
    ///
    /// 이것을 실패시키는 것: 변환을 지워 wire 위치를 그대로 쓰는 것.
    #[test]
    fn char_shape_boundary_after_a_tab_is_moved_to_text_position() {
        let units = [utf16("가"), ctrl(0x09), utf16("나다라")].concat();
        let result = decode_paragraph(&units, &[(0, 1), (10, 2)]);
        let para = &result.paragraphs[0];
        assert_eq!(run_texts(para), [(1, "가\t나".to_string()), (2, "다라".to_string())]);
    }

    /// D-T2: the shape of an underlined phrase after an inline object (the
    /// 법무부 rec260 form) covers exactly that phrase.
    ///
    /// 이것을 실패시키는 것: 개체를 wire 1 칸으로 세는 것.
    #[test]
    fn char_shape_after_an_inline_object_covers_exactly_its_phrase() {
        let after = " 가상상자란 ";
        let phrase = "가상 사업을 위한 맡김";
        let units =
            [utf16(" "), ctrl(0x0B), utf16(after), utf16(phrase), utf16("을 말한다")].concat();
        let start = (1 + 8 + utf16(after).len()) as u32;
        let end = start + utf16(phrase).len() as u32;
        let result = decode_paragraph(&units, &[(0, 1), (start, 2), (end, 1)]);
        assert_eq!(
            run_texts(&result.paragraphs[0]),
            [
                (1, " \u{FFFC} 가상상자란 ".to_string()),
                (2, phrase.to_string()),
                (1, "을 말한다".to_string()),
            ]
        );
    }

    /// D-T3: a field begin at the head of the paragraph takes no text, so a
    /// boundary right after it starts at the first character — and replaces
    /// the boundary at 0.
    ///
    /// 이것을 실패시키는 것: 필드 begin 을 글자 1 칸으로 세는 것.
    #[test]
    fn char_shape_boundary_after_a_leading_field_begin_starts_at_the_first_char() {
        let units = [ctrl(0x03), utf16("본문")].concat();
        let result = decode_paragraph(&units, &[(0, 1), (8, 2)]);
        assert_eq!(run_texts(&result.paragraphs[0]), [(2, "본문".to_string())]);
    }

    /// D-T4: a silently consumed extended control (`0x01`) takes wire units
    /// but no text.
    ///
    /// 이것을 실패시키는 것: `silent_wires` 를 무시하는 것.
    #[test]
    fn char_shape_boundary_after_a_silent_control_skips_it() {
        let units = [utf16("가나"), ctrl(0x01), utf16("다라")].concat();
        let result = decode_paragraph(&units, &[(0, 1), (10, 2)]);
        assert_eq!(
            run_texts(&result.paragraphs[0]),
            [(1, "가나".to_string()), (2, "다라".to_string())]
        );
    }

    /// D-T5: a group child's text box paragraph goes through the gso path,
    /// which writes its text and runs after the paragraph is finished. Its
    /// boundaries move the same way, silent controls included.
    ///
    /// 이것을 실패시키는 것: gso 갈래의 변환 호출을 지우는 것, gso 갈래에서
    /// `silent_wires` 를 옮기지 않는 것.
    #[test]
    fn char_shape_boundaries_of_a_group_child_text_move_too() {
        let units = [utf16("가"), ctrl(0x09), utf16("나"), ctrl(0x01), utf16("다라")].concat();
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 1, &para_text_with_control_ref("", "")));
        stream.extend(make_record(TagId::CtrlHeader, 1, &gso_ctrl_header_data(0, 0, 3000, 1000)));
        stream.extend(make_record(
            TagId::ShapeComponent,
            2,
            &shape_component_render_data(&SHAPE_COMPONENT_TYPE_GROUP, true, 0, (3000, 1000), 0.0),
        ));
        for x in [0.0, 2000.0] {
            stream.extend(make_record(
                TagId::ShapeComponent,
                3,
                &shape_component_render_data(b"cer$", false, 1, (1000, 1000), x),
            ));
            stream.extend(make_record(TagId::ShapeComponentRect, 4, &[0u8; 9]));
        }
        stream.extend(make_record(TagId::ListHeader, 4, &[0u8; 8]));
        stream.extend(make_record(TagId::ParaHeader, 4, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 5, &units_data(&units)));
        stream.extend(make_record(
            TagId::ParaCharShape,
            5,
            &char_shape_runs_data(&[(0, 1), (18, 2)]),
        ));

        let result = parse_body_text(&stream, &version()).unwrap();
        let [Hwp5Control::Group(group)] = &result.paragraphs[0].controls[..] else {
            panic!("expected one group, got {:?}", result.paragraphs[0].controls);
        };
        let child = group.children.last().expect("children");
        assert_eq!(
            run_texts(&child.paragraphs[0]),
            [(1, "가\t나".to_string()), (2, "다라".to_string())]
        );
    }

    /// §19.22 D5 (S17): the char shape entry at the wire position of the
    /// paragraph-end character is the end mark's, taken out of the body
    /// boundaries before they move to text positions; it is kept only when it
    /// differs from the last character's.
    ///
    /// 이것을 실패시키는 것: 변환 뒤 위치로 판정(필드 끝 뒤 끝 표시가 필드 끝
    /// 경계를 덮음) / 끝 경계를 본문에 남기기 / 빈 문단에서도 판정 / 글자 수로
    /// 셈(비-BMP) / 같은 모양도 `Some` / 범위 밖 경계를 끝 표시로.
    #[test]
    fn end_mark_shape_is_read_from_the_wire() {
        let end = |units: &[u16], runs: &[(u32, u32)]| {
            let result = decode_paragraph(units, runs);
            let p = &result.paragraphs[0];
            (
                p.end_char_shape,
                p.char_shape_runs.iter().map(|r| (r.position, r.char_shape_id)).collect::<Vec<_>>(),
            )
        };
        let text = |t: &str| [utf16(t), vec![0x0D]].concat();
        // Differs from the last character: carried, and not a body boundary.
        assert_eq!(end(&text("가나"), &[(0, 1), (2, 5)]), (Some(5), vec![(0, 1)]));
        // Same as the last character: kept here, folded at projection.
        assert_eq!(end(&text("가나"), &[(0, 1), (2, 1)]).0, Some(1));
        // Past the end character: not the end mark.
        assert_eq!(end(&text("가나"), &[(0, 1), (3, 5)]).0, None);
        // A field end just before the end character (8 wire units, no text):
        // the field end's boundary stays the last character's shape.
        let field = [utf16("가"), ctrl(0x04), vec![0x0D]].concat();
        let (shape, runs) = end(&field, &[(0, 1), (1, 2), (9, 5)]);
        assert_eq!(shape, Some(5));
        assert_eq!(runs, [(0, 1), (1, 2)]);
        // An object-only paragraph (a table and the end mark).
        let object = [ctrl(0x0B), vec![0x0D]].concat();
        assert_eq!(end(&object, &[(0, 1), (8, 5)]).0, Some(5));
        // A character outside the BMP: two code units.
        assert_eq!(end(&text("𝒜"), &[(0, 1), (2, 5)]).0, Some(5));
        // An empty paragraph keeps its boundary for its one run (§19.22 B),
        // with or without a ParaText.
        assert_eq!(end(&[0x0D], &[(0, 5)]), (None, vec![(0, 5)]));
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaCharShape, 1, &char_shape_runs_data(&[(0, 5)])));
        let no_text = parse_body_text(&stream, &version()).unwrap();
        assert_eq!(no_text.paragraphs[0].end_char_shape, None);
        assert_eq!(no_text.paragraphs[0].char_shape_runs.len(), 1);
    }

    /// Review r1 counterexample: body shape 1, a field end with shape 2 just
    /// before the end mark, end mark 2. The field end writes no run, so the
    /// last written run has shape 1 and the end mark (2) must be kept.
    ///
    /// 이것을 실패시키는 것: 마지막 wire 경계(필드 끝)와 비교해 접기.
    #[test]
    fn end_shape_after_a_field_end_is_kept() {
        let field = [utf16("가"), ctrl(0x04), vec![0x0D]].concat();
        let result = decode_paragraph(&field, &[(0, 1), (1, 2), (9, 2)]);
        let (doc, _) = crate::projection::project_to_core(vec![result]).expect("project");
        let p = &doc.sections()[0].paragraphs[0];
        assert_eq!(
            p.runs.last().unwrap().char_shape_id,
            hwpforge_foundation::CharShapeIndex::new(1)
        );
        assert_eq!(p.end_char_shape, Some(hwpforge_foundation::CharShapeIndex::new(2)));
    }

    /// The same reading in a group child's text (§19.22 D5).
    ///
    /// 이것을 실패시키는 것: 묶음 자식 글에서 끝 표시를 빼지 않기.
    #[test]
    fn end_mark_shape_of_a_group_child_text() {
        let units = [utf16("가나"), vec![0x0D]].concat();
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 1, &para_text_with_control_ref("", "")));
        stream.extend(make_record(TagId::CtrlHeader, 1, &gso_ctrl_header_data(0, 0, 3000, 1000)));
        stream.extend(make_record(
            TagId::ShapeComponent,
            2,
            &shape_component_render_data(&SHAPE_COMPONENT_TYPE_GROUP, true, 0, (3000, 1000), 0.0),
        ));
        for x in [0.0, 2000.0] {
            stream.extend(make_record(
                TagId::ShapeComponent,
                3,
                &shape_component_render_data(b"cer$", false, 1, (1000, 1000), x),
            ));
            stream.extend(make_record(TagId::ShapeComponentRect, 4, &[0u8; 9]));
        }
        stream.extend(make_record(TagId::ListHeader, 4, &[0u8; 8]));
        stream.extend(make_record(TagId::ParaHeader, 4, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 5, &units_data(&units)));
        stream.extend(make_record(
            TagId::ParaCharShape,
            5,
            &char_shape_runs_data(&[(0, 1), (2, 5)]),
        ));
        let result = parse_body_text(&stream, &version()).unwrap();
        let [Hwp5Control::Group(group)] = &result.paragraphs[0].controls[..] else { panic!() };
        let p = &group.children.last().unwrap().paragraphs[0];
        assert_eq!(p.end_char_shape, Some(5));
        assert_eq!(p.char_shape_runs.len(), 1);
    }

    /// D-T6: a boundary inside an object's eight wire units (no file has
    /// one) moves to the object's end and is reported.
    ///
    /// 이것을 실패시키는 것: 경고 갈래를 지우는 것, 개체 시작으로 붙이는 것.
    #[test]
    fn char_shape_boundary_inside_a_control_moves_to_its_end_with_a_warning() {
        let units = [utf16(" "), ctrl(0x0B), utf16("ab")].concat();
        let result = decode_paragraph(&units, &[(0, 1), (3, 2)]);
        assert_eq!(
            run_texts(&result.paragraphs[0]),
            [(1, " \u{FFFC}".to_string()), (2, "ab".to_string())]
        );
        assert_eq!(boundary_warnings(&result), 1, "{:?}", result.warnings);
        assert_eq!(boundary_warnings(&decode_paragraph(&units, &[(0, 1), (9, 2)])), 0);
    }

    /// D-T8: inside a field, the character after an object takes the shape
    /// that starts after the object — in the decoded boundaries and in the
    /// Core runs projection makes of the field's text.
    ///
    /// 이것을 실패시키는 것: 필드 begin 을 글자 1 칸으로 세는 것, 개체를 wire
    /// 1 칸으로 세는 것.
    #[test]
    fn char_shape_boundary_after_an_object_inside_a_field() {
        let units = [ctrl(0x03), utf16("가"), ctrl(0x0B), utf16("나"), ctrl(0x04)].concat();
        let result = decode_paragraph(&units, &[(0, 1), (17, 2)]);
        assert_eq!(
            run_texts(&result.paragraphs[0]),
            [(1, "가\u{FFFC}".to_string()), (2, "나".to_string())]
        );
        let (document, _) = crate::projection::project_to_core(vec![result]).unwrap();
        let text_runs: Vec<(String, usize)> = document.sections()[0].paragraphs[0]
            .runs
            .iter()
            .filter_map(|run| {
                run.content.as_text().map(|t| (t.to_string(), run.char_shape_id.get()))
            })
            .collect();
        assert!(text_runs.contains(&("나".to_string(), 2)), "{text_runs:?}");
        assert!(!text_runs.iter().any(|(t, id)| t.contains('나') && *id != 2), "{text_runs:?}");
    }

    /// D-T9: a surrogate pair is two UTF-16 units on both axes, so a silent
    /// control after it (and the tab before that) is found at the right
    /// wire position.
    ///
    /// 이것을 실패시키는 것: 글자를 UTF-16 단위가 아니라 문자 수로 세는 것
    /// (무음 구간이 뒤로 밀려 경계가 그 안에 떨어진다).
    #[test]
    fn char_shape_boundary_after_a_surrogate_pair_and_a_tab() {
        let units = [utf16("𝒳"), ctrl(0x09), ctrl(0x01), utf16("가나")].concat();
        let result = decode_paragraph(&units, &[(0, 1), (18, 2)]);
        assert_eq!(
            run_texts(&result.paragraphs[0]),
            [(1, "𝒳\t".to_string()), (2, "가나".to_string())]
        );
        assert_eq!(boundary_warnings(&result), 0, "{:?}", result.warnings);
    }

    /// A paragraph of 50,000 tab-separated characters with a boundary at
    /// every character: one cursor walks the spans once for all boundaries,
    /// so the spans checked stay within boundaries + spans — counted, not
    /// timed. Rescanning the spans for each boundary checks about
    /// boundaries × spans / 2 (over a billion here).
    ///
    /// 이것을 실패시키는 것: 경계마다 `WireTextMap::to_core` 로 처음부터 다시
    /// 훑는 것(검사한 span 수가 곱으로 는다).
    #[test]
    fn char_shape_conversion_is_linear_in_boundaries_and_spans() {
        let count = 50_000u32;
        let mut units = Vec::new();
        let mut runs = Vec::new();
        for i in 0..count {
            runs.push((i * 9, i % 7));
            units.extend(utf16("가"));
            units.extend(ctrl(0x09));
        }
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 1, &units_data(&units)));
        stream.extend(make_record(TagId::ParaCharShape, 1, &char_shape_runs_data(&runs)));
        let paragraph = {
            let result = parse_body_text(&stream, &version()).unwrap();
            result.paragraphs.into_iter().next().unwrap()
        };
        let segments = paragraph.text_segments.clone();
        let wire_runs: Vec<Hwp5CharShapeRun> = runs
            .iter()
            .map(|&(position, char_shape_id)| Hwp5CharShapeRun { position, char_shape_id })
            .collect();
        crate::wire_text_map::SPAN_VISITS.with(|visits| visits.set(0));
        let converted =
            char_shape_runs_to_text_positions(&segments, &[], &wire_runs, &mut Vec::new());
        let visits = crate::wire_text_map::SPAN_VISITS.with(std::cell::Cell::get);
        assert_eq!(converted.len(), wire_runs.len());
        assert_eq!(converted.last().map(|run| run.position), Some((count - 1) * 2));
        assert_eq!(paragraph.char_shape_runs, converted);
        // 50,000 tab spans and 50,000 boundaries.
        let bound = 2 * (u64::from(count) + u64::from(count));
        assert!(visits <= bound, "{visits} span checks for {count} boundaries and spans");
    }

    /// Boundaries inside controls, ascending (`가`+tab repeated, boundaries
    /// 9i+2 and 9i+3 both inside the i-th tab): each is moved to its tab's
    /// end without the cursor passing the next boundary, so the spans checked
    /// stay linear. Advancing the cursor to the tab's end made the next
    /// boundary (smaller than the cursor) rescan from the start — quadratic.
    ///
    /// 이것을 실패시키는 것: 안쪽 경계를 `(wire..).find_map(core_at)` 처럼 커서를
    /// 개체 끝까지 전진시켜 찾기.
    #[test]
    fn char_shape_boundaries_inside_controls_stay_linear() {
        let count = 20_000u32;
        let mut units = Vec::new();
        let mut runs = Vec::new();
        for i in 0..count {
            runs.push((i * 9 + 2, 1));
            runs.push((i * 9 + 3, 2));
            units.extend(utf16("가"));
            units.extend(ctrl(0x09));
        }
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 1, &units_data(&units)));
        let paragraph = {
            let result = parse_body_text(&stream, &version()).unwrap();
            result.paragraphs.into_iter().next().unwrap()
        };
        let wire_runs: Vec<Hwp5CharShapeRun> = runs
            .iter()
            .map(|&(position, char_shape_id)| Hwp5CharShapeRun { position, char_shape_id })
            .collect();
        crate::wire_text_map::SPAN_VISITS.with(|visits| visits.set(0));
        let mut warnings = Vec::new();
        let converted = char_shape_runs_to_text_positions(
            &paragraph.text_segments,
            &[],
            &wire_runs,
            &mut warnings,
        );
        let visits = crate::wire_text_map::SPAN_VISITS.with(std::cell::Cell::get);
        // Both boundaries of tab i land at its end (Core 2i+2) and the later
        // one wins.
        assert_eq!(converted.len(), count as usize);
        assert_eq!(converted[0], Hwp5CharShapeRun { position: 2, char_shape_id: 2 });
        assert_eq!(warnings.len(), 2 * count as usize);
        let bound = 2 * (u64::from(2 * count) + u64::from(count));
        assert!(visits <= bound, "{visits} span checks for {} boundaries", 2 * count);
    }

    /// A boundary past the paragraph's wire end (it covers no character)
    /// moves to the end of the text.
    ///
    /// 이것을 실패시키는 것: 범위 밖 경계를 wire 위치 그대로 두는 것.
    #[test]
    fn char_shape_boundary_past_the_end_moves_to_the_text_end() {
        let result = decode_paragraph(&utf16("ab"), &[(0, 1), (1, 2), (10, 3)]);
        assert_eq!(
            run_texts(&result.paragraphs[0]),
            [(1, "a".to_string()), (2, "b".to_string()), (3, String::new())]
        );
    }

    /// D-T10: boundaries on a run of zero-width controls (section definition,
    /// field begin) all land on the same character: the last one wins.
    ///
    /// 이것을 실패시키는 것: 같은 위치에 겹친 경계에서 앞 경계를 남기는 것.
    #[test]
    fn overlapping_char_shape_boundaries_on_zero_width_controls_keep_the_last() {
        let units = [ctrl(0x02), ctrl(0x03), utf16("본문"), ctrl(0x04)].concat();
        let result = decode_paragraph(&units, &[(0, 1), (8, 2), (16, 3)]);
        assert_eq!(run_texts(&result.paragraphs[0]), [(3, "본문".to_string())]);
    }

    /// Build a stream of `depth` tables nested table-in-cell-in-table.
    ///
    /// Level layout per nesting `d` (0-based), opening table `d` inside the
    /// cell paragraph of table `d-1`:
    ///   CtrlHeader(level = 2*d,   TABLE)   → table ctrl_depth = 2*d
    ///   Table     (level = 2*d+1)          → table body
    ///   ListHeader(level = 2*d+1)          → cell (1 paragraph)
    ///   ParaHeader(level = 2*d+1)          → cell paragraph (control-ref host)
    ///   ParaText  (level = 2*d+2, ctrl-ref)→ so the cell paragraph can host the
    ///                                        next nested table CtrlHeader
    /// After the deepest table, a `ParaHeader(level=0)` forces every table to
    /// close (the level-driven pop loop).
    fn nested_tables_stream(depth: usize) -> Vec<u8> {
        let mut stream = Vec::new();
        // Host paragraph for the outermost table.
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 0, &para_text_with_control_ref("", "")));
        for d in 0..depth {
            let base = (2 * d) as u16;
            stream.extend(make_record(TagId::CtrlHeader, base, &ctrl_header_data(CTRL_ID_TABLE)));
            stream.extend(make_record(TagId::Table, base + 1, &basic_table_data(1, 1)));
            stream.extend(make_record(
                TagId::ListHeader,
                base + 1,
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
            // Cell paragraph with a control reference so it can host the next
            // nested table.
            stream.extend(make_record(TagId::ParaHeader, base + 1, &para_header_data(0, 0)));
            stream.extend(make_record(
                TagId::ParaText,
                base + 2,
                &para_text_with_control_ref("", ""),
            ));
        }
        // Force every open table to close.
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(1, 0)));
        stream
    }

    fn count_nested_table_depth(table: &Hwp5Table) -> usize {
        let mut max_child = 0;
        for cell in &table.cells {
            for para in &cell.paragraphs {
                for control in &para.controls {
                    if let Hwp5Control::Table(inner) = control {
                        max_child = max_child.max(count_nested_table_depth(inner));
                    }
                }
            }
        }
        1 + max_child
    }

    #[test]
    fn normal_nested_tables_still_decode() {
        // Regression: shallow 2- and 3-deep nesting (well under the cap) must
        // still produce fully attached nested tables, no DroppedControl warning.
        for depth in [2usize, 3] {
            let stream = nested_tables_stream(depth);
            let result = parse_body_text(&stream, &version()).unwrap();
            let outer = result
                .paragraphs
                .iter()
                .flat_map(|p| &p.controls)
                .find_map(|c| match c {
                    Hwp5Control::Table(t) => Some(t),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("depth {depth}: expected an outer table"));
            assert_eq!(
                count_nested_table_depth(outer),
                depth,
                "depth {depth}: all nested tables must attach",
            );
            assert!(
                !result
                    .warnings
                    .iter()
                    .any(|w| matches!(w, Hwp5Warning::DroppedControl { control: "table", .. })),
                "depth {depth}: normal nesting must not emit a table DroppedControl warning",
            );
        }
    }

    #[test]
    fn over_cap_table_nesting_is_dropped_without_panic() {
        // MAX_CONTROL_NESTING + 1 deep: must NOT panic/OOM, must emit at least one
        // DroppedControl{control:"table"} warning, must keep decoding the outer
        // content, and the attached tree must be capped (no deeper than the
        // cap) — proving push/pop stayed balanced.
        let depth = MAX_CONTROL_NESTING + 1;
        let stream = nested_tables_stream(depth);

        let result = parse_body_text(&stream, &version()).expect("deep nesting must not error");

        // (b) the over-cap warning fired.
        let dropped = result
            .warnings
            .iter()
            .filter(|w| matches!(w, Hwp5Warning::DroppedControl { control: "table", .. }))
            .count();
        assert!(
            dropped >= 1,
            "expected a table DroppedControl warning, got warnings: {:?}",
            result.warnings
        );

        // (c) the outer table is still decoded and attached.
        let outer = result
            .paragraphs
            .iter()
            .flat_map(|p| &p.controls)
            .find_map(|c| match c {
                Hwp5Control::Table(t) => Some(t),
                _ => None,
            })
            .expect("outer table must still decode");

        // (d) the attached nesting is capped — the deepest (over-cap) table was
        // dropped rather than attached, so the visible depth never exceeds the
        // cap. This also proves the pop/finalize machinery stayed balanced
        // (otherwise the outer table would be malformed or missing).
        let attached_depth = count_nested_table_depth(outer);
        assert!(
            attached_depth <= MAX_CONTROL_NESTING,
            "attached nesting depth {attached_depth} must not exceed cap {MAX_CONTROL_NESTING}",
        );

        // The closing paragraph (para_shape_id 1) must still be decoded after
        // all the tables unwound — i.e. outer body parsing resumed normally.
        assert!(
            result.paragraphs.len() >= 2,
            "outer content after the nested tables must still parse",
        );
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
    fn para_line_seg_is_parsed_into_paragraph_layout_cache() {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 0, &para_text_data("ok")));
        stream.extend(make_record(
            TagId::ParaLineSeg,
            0,
            &para_line_seg_data(&[(0, 0, 1000), (2, 1600, 1000)]),
        ));

        let result = parse_body_text(&stream, &version()).unwrap();
        assert_eq!(result.paragraphs.len(), 1);
        assert_eq!(result.paragraphs[0].text, "ok");
        assert_eq!(result.paragraphs[0].line_segments.len(), 2);
        assert_eq!(result.paragraphs[0].line_segments[1].text_start_position, 2);
        assert_eq!(result.paragraphs[0].line_segments[1].vertical_position, 1600);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn text_segments_rendering() {
        // Tab → \t, NonBreakingSpace (0x18) → \u{00A0}, FwSpace (0x1F) → \u{001F}
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
        data.extend_from_slice(&0x18u16.to_le_bytes()); // NonBreakingSpace (per HWP5 spec)
        data.extend_from_slice(
            &"C".encode_utf16().flat_map(|c| c.to_le_bytes()).collect::<Vec<_>>(),
        );
        data.extend_from_slice(&0x1Fu16.to_le_bytes()); // FwSpace (per HWP5 spec)
        data.extend_from_slice(
            &"D".encode_utf16().flat_map(|c| c.to_le_bytes()).collect::<Vec<_>>(),
        );

        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 0, &data));

        let result = parse_body_text(&stream, &version()).unwrap();
        assert_eq!(result.paragraphs[0].text, "A\tB\u{00A0}C\u{001F}D");
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

    fn zone_table_data(declared: Option<u16>, zones: &[[u16; 5]]) -> Vec<u8> {
        let mut data = table_data(TestTableSpec {
            rows: 40,
            cols: 2,
            page_break_bits: 0,
            repeat_header: false,
            cell_spacing: 0,
            row_metadata: vec![2; 40],
            border_fill_id: Some(1),
        });
        if let Some(n) = declared {
            data.extend_from_slice(&n.to_le_bytes());
            for zone in zones {
                for v in zone {
                    data.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        data
    }

    /// F: the zone list after the border fill id — none when the field is
    /// absent, the entries in wire order, and only the complete entries of a
    /// list shorter than its count (which is then reported).
    /// 이것을 실패시키는 것: 수 오프셋 ±2 · 행·열 뒤바꿈(비대칭 (12,0,31,1)) ·
    /// 선언 수 무시(남는 바이트를 읽기) · 잘림 표시 삭제.
    #[test]
    fn table_cell_zones_are_read() {
        let zone = |z: [u16; 5]| Hwp5TableCellZone {
            start_row: z[0],
            start_col: z[1],
            end_row: z[2],
            end_col: z[3],
            border_fill_id: z[4],
        };
        let absent = parse_table_header(&zone_table_data(None, &[]));
        assert!(absent.cell_zones.is_empty() && absent.cell_zones_declared.is_none());
        let empty = parse_table_header(&zone_table_data(Some(0), &[]));
        assert!(empty.cell_zones.is_empty() && empty.cell_zones_declared.is_none());
        let two =
            parse_table_header(&zone_table_data(Some(2), &[[12, 0, 31, 1, 7], [0, 1, 0, 1, 3]]));
        assert_eq!(two.cell_zones, [zone([12, 0, 31, 1, 7]), zone([0, 1, 0, 1, 3])]);
        assert_eq!(two.cell_zones_declared, None);
        let mut extra = zone_table_data(Some(1), &[[12, 0, 31, 1, 7]]);
        extra.extend_from_slice(&[9u8; 10]);
        assert_eq!(parse_table_header(&extra).cell_zones, [zone([12, 0, 31, 1, 7])]);
        let mut half = zone_table_data(None, &[]);
        half.push(2);
        let half = parse_table_header(&half);
        assert!(half.cell_zones.is_empty());
        assert!(half.cell_zone_count_cut, "one byte of a count is damage");
        assert!(!two.cell_zone_count_cut && !absent.cell_zone_count_cut);
        let short = parse_table_header(&zone_table_data(Some(2), &[[12, 0, 31, 1, 7]]));
        assert_eq!(short.cell_zones, [zone([12, 0, 31, 1, 7])]);
        assert_eq!(short.cell_zones_declared, Some(2));

        let warnings = |declared, zones: &[[u16; 5]]| {
            let mut stream = Vec::new();
            stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::CtrlHeader, 0, &ctrl_header_data(CTRL_ID_TABLE)));
            stream.extend(make_record(TagId::Table, 1, &zone_table_data(Some(declared), zones)));
            parse_body_text(&stream, &version())
                .unwrap()
                .warnings
                .iter()
                .filter(|w| format!("{w:?}").contains("table.cell_zone"))
                .count()
        };
        assert_eq!(warnings(2, &[[12, 0, 31, 1, 7]]), 1);
        assert_eq!(warnings(1, &[[12, 0, 31, 1, 7]]), 0);
        let mut cut = zone_table_data(None, &[]);
        cut.push(2);
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &ctrl_header_data(CTRL_ID_TABLE)));
        stream.extend(make_record(TagId::Table, 1, &cut));
        let cut_warnings = parse_body_text(&stream, &version()).unwrap().warnings;
        assert!(
            cut_warnings
                .iter()
                .any(|w| format!("{w:?}").contains("one byte into its cell zone count")),
            "{cut_warnings:?}"
        );
    }

    fn caption_list_header(
        paragraphs: u16,
        property: u32,
        width: u32,
        gap: i16,
        last: u32,
    ) -> Vec<u8> {
        let mut d = paragraphs.to_le_bytes().to_vec();
        d.extend_from_slice(&[0u8; 6]);
        d.extend_from_slice(&property.to_le_bytes());
        d.extend_from_slice(&width.to_le_bytes());
        d.extend_from_slice(&gap.to_le_bytes());
        d.extend_from_slice(&last.to_le_bytes());
        d
    }

    /// One table whose control holds `caption` (if any) before the body and
    /// one 1×1 cell.
    fn captioned_table_stream(caption: Option<Vec<u8>>) -> Vec<u8> {
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &ctrl_header_data(CTRL_ID_TABLE)));
        if let Some(caption) = caption {
            stream.extend(make_record(TagId::ListHeader, 1, &caption));
            stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::ParaText, 2, &para_text_data("캡션")));
        }
        stream.extend(make_record(TagId::Table, 1, &basic_table_data(1, 1)));
        stream.extend(make_record(
            TagId::ListHeader,
            1,
            &list_header_table_cell_data(TestCellSpec {
                paragraph_count: 1,
                legacy_u16_count: false,
                properties: 0,
                column: 0,
                row: 0,
                col_span: 1,
                row_span: 1,
                width: 8000,
                height: 1000,
                margin: Hwp5TableCellMargin { left: 0, right: 0, top: 0, bottom: 0 },
                border_fill_id: None,
            }),
        ));
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_data("칸")));
        stream
    }

    fn only_table(result: &SectionResult) -> &Hwp5Table {
        match &result.paragraphs[0].controls[0] {
            Hwp5Control::Table(table) => table,
            other => panic!("expected Table, got {other:?}"),
        }
    }

    /// D: the list header under the table control before the body record is
    /// the caption — its block at bytes 8..22 (side, `fullSz`, width, gap,
    /// last width) and its paragraphs, which stay out of the cells. A
    /// shorter header is reported and drops the caption.
    /// 이것을 실패시키는 것: `seen_table_body` 조건 반전(칸이 캡션으로) · 오프셋
    /// 6 으로 읽기 · 측 마스크 오류 · `fullSz` 비트 오류 · 짧은 레코드 경고 삭제 ·
    /// 본문에서 캡션 닫기 생략(칸 문단이 캡션으로).
    #[test]
    fn table_caption_is_read() {
        let result = parse_body_text(
            &captioned_table_stream(Some(caption_list_header(1, 0b111, 8504, 850, 45978))),
            &version(),
        )
        .unwrap();
        let table = only_table(&result);
        let caption = table.caption.as_ref().expect("caption");
        assert_eq!(
            (caption.side, caption.full_size, caption.width, caption.gap, caption.last_width),
            (3, true, 8504, 850, 45978)
        );
        assert_eq!(caption.paragraphs.len(), 1);
        assert_eq!(caption.paragraphs[0].text, "캡션");
        assert_eq!(table.cells.len(), 1);
        assert_eq!(table.cells[0].paragraphs[0].text, "칸");
        for (property, side) in [(0u32, 0u8), (1, 1), (2, 2)] {
            let result = parse_body_text(
                &captioned_table_stream(Some(caption_list_header(1, property, 0, 0, 0))),
                &version(),
            )
            .unwrap();
            let caption = only_table(&result).caption.as_ref().unwrap();
            assert_eq!((caption.side, caption.full_size), (side, false));
        }

        let short = parse_body_text(
            &captioned_table_stream(Some(vec![1, 0, 0, 0, 0, 0, 0, 0])),
            &version(),
        )
        .unwrap();
        assert!(only_table(&short).caption.is_none());
        assert_eq!(only_table(&short).cells[0].paragraphs[0].text, "칸");
        assert!(short
            .warnings
            .iter()
            .any(|w| matches!(w, Hwp5Warning::DroppedControl { control: "caption", .. })));
        let plain = parse_body_text(&captioned_table_stream(None), &version()).unwrap();
        assert!(only_table(&plain).caption.is_none());
        // A caption that declares more paragraphs than it holds ends at the
        // body record, not at the first cell.
        let open = parse_body_text(
            &captioned_table_stream(Some(caption_list_header(2, 3, 0, 0, 0))),
            &version(),
        )
        .unwrap();
        assert_eq!(only_table(&open).caption.as_ref().map(|c| c.paragraphs.len()), Some(1));
        assert_eq!(only_table(&open).cells[0].paragraphs[0].text, "칸");
        let cut = |r: &SectionResult| {
            r.warnings
                .iter()
                .filter(|w| {
                    matches!(w, Hwp5Warning::ParserFallback { subject: "table.caption", .. })
                })
                .count()
        };
        assert_eq!(cut(&open), 1);
        assert!(format!("{:?}", open.warnings).contains("declares 2 paragraphs but 1"));
        assert_eq!(cut(&result), 0);
    }

    /// E: table property bit 3 is `noAdjust`; bits 0-2 (page break, repeat
    /// header) do not set it.
    ///
    /// 이것을 실패시키는 것: bit 2 읽기(`0x4` 가 참) · bit 3 무시(`0x8` 가 거짓).
    #[test]
    fn table_property_bit_3_is_no_adjust() {
        for (properties, expected) in [(0x8u32, true), (0x7, false), (0xF, true), (0x0, false)] {
            let mut data = basic_table_data(1, 1);
            data[0..4].copy_from_slice(&properties.to_le_bytes());
            assert_eq!(parse_table_header(&data).no_adjust, expected, "{properties:#x}");
        }
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

    /// Hancom's object-link record (`CtrlData`, as saved under a logo in a
    /// government press release) one level below a picture's component.
    const LINK_RECORD: &str = "1b02010000006f0200806f0201000000650201001b0068007400740070005c003a002f002f007700770077002e006b006f007200650061002e006b0072003b0031003b0030003b0030003b00";

    fn link_record() -> Vec<u8> {
        (0..LINK_RECORD.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&LINK_RECORD[i..i + 2], 16).unwrap())
            .collect()
    }

    /// A picture's link is its own `CtrlData` (one level below its
    /// component); one deeper belongs to something inside it and is not
    /// taken. Both the paragraph-level and the text-box-level paths.
    ///
    /// 이것을 실패시키는 것: 두 경로의 `CtrlData` 갈래를 각각 지우는 것 / 수준
    /// 검사(`comp_level + 1`)를 지우는 것(깊은 기록이 링크가 된다).
    #[test]
    fn picture_link_record_is_read_at_its_own_level() {
        let link = |deeper: bool| {
            let mut stream = Vec::new();
            stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::ParaText, 0, &para_text_with_control_ref("", "")));
            stream.extend(make_record(TagId::CtrlHeader, 0, &gso_ctrl_header_data(0, 0, 640, 320)));
            stream.extend(make_record(TagId::ShapeComponent, 1, &[0u8; 4]));
            stream.extend(make_record(TagId::CtrlData, if deeper { 3 } else { 2 }, &link_record()));
            stream.extend(make_record(TagId::ShapePicture, 2, &shape_picture_data(7)));
            let result = parse_body_text(&stream, &version()).unwrap();
            match &result.paragraphs[0].controls[0] {
                Hwp5Control::Image(image) => image.link.clone(),
                other => panic!("expected Image, got {other:?}"),
            }
        };
        assert_eq!(link(false), Some(Ok("http://www.korea.kr;1;0;0;".to_string())));
        assert_eq!(link(true), None);

        let boxed_link = |deeper: bool| {
            let mut stream = Vec::new();
            stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
            stream.extend(make_record(
                TagId::CtrlHeader,
                0,
                &gso_ctrl_header_data(0, 0, 8000, 6000),
            ));
            stream.extend(make_record(TagId::ShapeComponentRect, 1, &[0u8; 4]));
            stream.extend(make_record(TagId::ListHeader, 1, &[0u8; 4]));
            stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
            stream.extend(make_record(TagId::ParaText, 2, &para_text_with_control_ref("앞", "뒤")));
            stream.extend(make_record(
                TagId::CtrlHeader,
                2,
                &gso_ctrl_header_data(10, 20, 3000, 4000),
            ));
            stream.extend(make_record(TagId::ShapeComponent, 3, &[0u8; 4]));
            stream.extend(make_record(TagId::CtrlData, if deeper { 5 } else { 4 }, &link_record()));
            stream.extend(make_record(TagId::ShapePicture, 4, &shape_picture_data(2)));
            let result = parse_body_text(&stream, &version()).unwrap();
            let Hwp5Control::TextBox(textbox) = &result.paragraphs[0].controls[0] else { panic!() };
            let Hwp5Control::Image(image) = &textbox.paragraphs[0].controls[0] else { panic!() };
            image.link.clone()
        };
        assert_eq!(boxed_link(false), Some(Ok("http://www.korea.kr;1;0;0;".to_string())));
        assert_eq!(boxed_link(true), None);
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
    fn gso_shape_picture_carries_ctrl_header_object_properties_top_level() {
        // W2p: CtrlHeader `data[4..8]` (표 70 공통 개체 속성) 은 top-level
        // `handle_ctrl_header` → `NestedSubtreeContext` 경로를 통해
        // `Hwp5ImageControl.ctrl_properties` 까지 그대로 carry 돼야 한다.
        // KNOWN_PROPERTIES = bit0(글자처럼 취급) + 노이즈 비트 — bit 추출이
        // 아니라 raw DWORD 전체가 그대로 전달되는지 검증.
        const KNOWN_PROPERTIES: u32 = 0x0000_1235;
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 0, &para_text_with_control_ref("", "")));
        stream.extend(make_record(
            TagId::CtrlHeader,
            0,
            &gso_ctrl_header_data_with_properties(-120, 240, 6400, 3200, KNOWN_PROPERTIES),
        ));
        stream.extend(make_record(TagId::ShapeComponent, 1, &[0u8; 4]));
        stream.extend(make_record(TagId::ShapePicture, 1, &shape_picture_data(7)));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        match &para.controls[0] {
            Hwp5Control::Image(image) => {
                assert_eq!(image.ctrl_properties, KNOWN_PROPERTIES);
            }
            other => panic!("expected Image, got {:?}", other),
        }
    }

    #[test]
    fn gso_shape_picture_carries_ctrl_header_object_properties_inside_table_cell() {
        // W2p: 표 셀 안 `gso ` 도 공통 컨트롤 해석기로 같은 DWORD 를 carry 한다 —
        // top-level 과 별개 코드 경로.
        const KNOWN_PROPERTIES: u32 = 0x0000_0001;
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
        stream.extend(make_record(
            TagId::CtrlHeader,
            2,
            &gso_ctrl_header_data_with_properties(1, 2, 300, 400, KNOWN_PROPERTIES),
        ));
        stream.extend(make_record(TagId::ShapeComponent, 3, &[0u8; 4]));
        stream.extend(make_record(TagId::ShapePicture, 3, &shape_picture_data(9)));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        match &para.controls[0] {
            Hwp5Control::Table(table) => {
                let cell_para = &table.cells[0].paragraphs[0];
                match &cell_para.controls[0] {
                    Hwp5Control::Image(image) => {
                        assert_eq!(image.ctrl_properties, KNOWN_PROPERTIES);
                    }
                    other => panic!("expected cell Image, got {:?}", other),
                }
            }
            other => panic!("expected Table, got {:?}", other),
        }
    }

    #[test]
    fn gso_shape_picture_carries_ctrl_header_object_properties_inside_textbox_subtree() {
        // W2p: 글상자 subtree 안 `gso ` 도 공통 컨트롤 해석기로 같은 DWORD 를 carry 해야
        // 한다 — table-cell 경로와도 별개.
        const KNOWN_PROPERTIES: u32 = 0x0000_0001;
        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &gso_ctrl_header_data(0, 0, 8000, 6000)));
        stream.extend(make_record(TagId::ShapeComponentRect, 1, &[0u8; 4]));
        stream.extend(make_record(TagId::ListHeader, 1, &[0u8; 4]));
        stream.extend(make_record(TagId::ParaHeader, 1, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::ParaText, 2, &para_text_with_control_ref("앞", "뒤")));
        stream.extend(make_record(
            TagId::CtrlHeader,
            2,
            &gso_ctrl_header_data_with_properties(10, 20, 3000, 4000, KNOWN_PROPERTIES),
        ));
        stream.extend(make_record(TagId::ShapeComponent, 3, &[0u8; 4]));
        stream.extend(make_record(TagId::ShapePicture, 3, &shape_picture_data(2)));

        let result = parse_body_text(&stream, &version()).unwrap();
        let para = &result.paragraphs[0];
        match &para.controls[0] {
            Hwp5Control::TextBox(textbox) => match &textbox.paragraphs[0].controls[0] {
                Hwp5Control::Image(image) => {
                    assert_eq!(image.ctrl_properties, KNOWN_PROPERTIES);
                }
                other => panic!("expected nested Image, got {:?}", other),
            },
            other => panic!("expected TextBox, got {:?}", other),
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
            outer_margin: [0; 4],
            x: 10,
            y: 20,
            width: 5_000,
            height: 6_000,
        };
        let mut ctx = NestedSubtreeContext::new(0, CTRL_ID_GSO, 0, 0, Some(geometry));
        ctx.note_shape_component(&[], 0, &mut Vec::new());
        ctx.note_shape_picture(Hwp5ShapePicture::parse(&shape_picture_data(1)).unwrap());
        ctx.note_shape_ole(
            Hwp5ShapeComponentOle::parse(&shape_component_ole_data(1, 9000, 8000)).unwrap(),
        );

        match ctx.into_control(&mut Vec::new()) {
            Hwp5Control::Unknown { ctrl_id, .. } => assert_eq!(ctrl_id, CTRL_ID_GSO),
            other => panic!("expected Unknown for ambiguous gso payload, got {:?}", other),
        }
    }

    #[test]
    fn nested_subtree_gso_with_picture_and_ole_stays_unknown() {
        let geometry = crate::schema::section::Hwp5ShapeComponentGeometry {
            outer_margin: [0; 4],
            x: 10,
            y: 20,
            width: 5_000,
            height: 6_000,
        };
        let mut ctx = NestedSubtreeContext::new(0, CTRL_ID_GSO, 0, 0, Some(geometry));
        ctx.note_shape_component(&[], 0, &mut Vec::new());
        ctx.note_shape_picture(Hwp5ShapePicture::parse(&shape_picture_data(1)).unwrap());
        ctx.note_shape_ole(
            Hwp5ShapeComponentOle::parse(&shape_component_ole_data(1, 9000, 8000)).unwrap(),
        );

        let mut warnings = Vec::new();
        match ctx.into_control(&mut warnings) {
            Hwp5Control::Unknown { ctrl_id, .. } => assert_eq!(ctrl_id, CTRL_ID_GSO),
            other => panic!("expected Unknown for ambiguous subtree gso payload, got {:?}", other),
        }
    }

    fn line_gso_input(shape_component_kind: Option<[u8; 4]>) -> GsoClassificationInput {
        GsoClassificationInput {
            standalone_render: None,
            ctrl_id: CTRL_ID_GSO,
            saw_shape_component: true,
            saw_shape_rectangle: false,
            geometry: Some(crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 1,
                y: 2,
                width: 100,
                height: 50,
            }),
            picture: None,
            ole: None,
            line: Some(crate::schema::section::Hwp5ShapeComponentLine {
                start: Hwp5ShapePoint { x: 0, y: 0 },
                end: Hwp5ShapePoint { x: 100, y: 0 },
            }),
            polygon: None,
            ellipse: None,
            curve: None,
            text_art: None,
            shape_component_kind,
            link: None,
            look: None,
            paragraphs: Vec::new(),
            list_header_properties: None,
            text_margin: None,
            original_size: None,
            instance_id: 0,
            ctrl_properties: 0,
        }
    }

    #[test]
    fn gso_line_with_connect_line_tag_classifies_as_connect_line() {
        let input = line_gso_input(Some(SHAPE_COMPONENT_TYPE_CONNECT_LINE));
        match classify_gso_control(input) {
            Hwp5Control::ConnectLine(connect_line) => {
                assert_eq!(connect_line.end.x, 100);
                assert_eq!(connect_line.geometry.width, 100);
            }
            other => panic!("expected ConnectLine for the $col tag, got {other:?}"),
        }
    }

    #[test]
    fn gso_line_without_connect_line_tag_stays_line() {
        // Conservative guard: a plain line (no tag, or any non-$col tag) must
        // never be reclassified as a connect line.
        for kind in [None, Some(*b"$rec"), Some(*b"$lin")] {
            match classify_gso_control(line_gso_input(kind)) {
                Hwp5Control::Line(_) => {}
                other => panic!("expected Line for shape_component_kind={kind:?}, got {other:?}"),
            }
        }
    }

    /// 이것을 실패시키는 것: 잘못된 payload의 큐 자리나 진단을 버리기.
    #[test]
    fn nested_malformed_controls_keep_a_warned_queue_slot() {
        for id in [
            CTRL_ID_COMPOSE,
            CTRL_ID_DUTMAL,
            CTRL_ID_INDEXMARK,
            CTRL_ID_FIELD_SUMMERY,
            CTRL_ID_FIELD_DATE_CODE,
            CTRL_ID_FIELD_PATH,
            CTRL_ID_FIELD_CROSSREF,
            CTRL_ID_CLICK_HERE,
            CTRL_ID_ATNO,
            CTRL_ID_NEW_NUMBER,
            CTRL_ID_PAGE_HIDING,
        ] {
            let mut warnings = Vec::new();
            let control =
                BodyTextParserState::typed_paragraph_control(id, &id.to_le_bytes(), &mut warnings);
            assert!(matches!(control,Some(Hwp5Control::Unknown {ctrl_id,..}) if ctrl_id==id));
            assert_eq!(warnings.len(), 1);
        }
    }
    include!("review_tests.rs");
}
