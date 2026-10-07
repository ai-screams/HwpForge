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
    Hwp5CharShapeRun, Hwp5DutmalControl, Hwp5EqEdit, Hwp5MemoCommand, Hwp5PageDef, Hwp5ParaHeader,
    Hwp5ParaLineSeg, Hwp5ParaText, Hwp5ShapeComponentCurve, Hwp5ShapeComponentEllipse,
    Hwp5ShapeComponentGeometry, Hwp5ShapeComponentLine, Hwp5ShapeComponentOle,
    Hwp5ShapeComponentPolygon, Hwp5ShapePicture, Hwp5ShapeTextArt, TextSegment,
};

mod gso;
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

/// Maximum nesting depth of `tbl ` contexts on [`BodyTextParserState::table_stack`].
///
/// `table_stack` grows on every `tbl ` CtrlHeader (top-level or re-entered
/// inside a cell). A malicious or corrupt stream can nest tables arbitrarily,
/// growing the stack — and the eventual recursive projection — without bound.
/// This cap mirrors [`GSO_GROUP_MAX_DEPTH`]: once the stack is this deep, an
/// additional `tbl ` still pushes a context (so the level-driven pop/cell
/// finalize machinery stays balanced) but the context is flagged over-cap and
/// dropped at finalize with a `DroppedControl` warning, rather than attached.
const MAX_TABLE_NESTING: usize = 32;

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
    saw_list_header: bool,
    current_para: Option<ParaBuf>,
    paragraphs: Vec<Hwp5Paragraph>,
}

impl MemoContentCapture {
    fn into_paragraphs(mut self, warnings: &mut Vec<Hwp5Warning>) -> Vec<Hwp5Paragraph> {
        if let Some(buf) = self.current_para.take() {
            self.paragraphs.push(buf.finish(warnings));
        }
        self.paragraphs
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
    /// A `head`/`foot` control opened in the current cell paragraph whose
    /// own paragraphs are being collected (§19.7 F). Without it those
    /// paragraphs were read as more cell paragraphs and the control itself
    /// was dropped as unknown.
    cell_header_footer: Option<CellHeaderFooter>,
    /// True when this table was opened past [`MAX_TABLE_NESTING`]. The context
    /// is still tracked (to keep the level-driven pop/cell machinery balanced
    /// with the surrounding records) but its finalized control is dropped
    /// rather than attached to the parent (E1 #3).
    over_cap: bool,
}

impl TableContext {
    fn new(ctrl_depth: u16, instance_id: u32) -> Self {
        Self::new_with_cap(ctrl_depth, instance_id, false)
    }

    /// Construct a context flagged as over the nesting cap; behaves identically
    /// while parsing but is dropped (not attached) at finalize.
    fn new_over_cap(ctrl_depth: u16, instance_id: u32) -> Self {
        Self::new_with_cap(ctrl_depth, instance_id, true)
    }

    fn new_with_cap(ctrl_depth: u16, instance_id: u32, over_cap: bool) -> Self {
        Self {
            ctrl_depth,
            table: Hwp5Table {
                inner_margin: None,
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
            inline_cell_gso_ctx: None,
            cell_header_footer: None,
            over_cap,
        }
    }

    fn flush_inline_gso(&mut self, warnings: &mut Vec<Hwp5Warning>) {
        attach_inline_gso_control(
            &mut self.current_cell_para,
            self.inline_cell_gso_ctx.take(),
            warnings,
        );
    }

    /// Closes the open cell header/footer and attaches it to the cell
    /// paragraph that holds it.
    fn close_cell_header_footer(&mut self, warnings: &mut Vec<Hwp5Warning>) {
        let Some(mut capture) = self.cell_header_footer.take() else {
            return;
        };
        attach_inline_gso_control(&mut capture.current_para, capture.gso.take(), warnings);
        if let Some(buf) = capture.current_para.take() {
            capture.ctx.paragraphs.push(buf.finish(warnings));
        }
        let control = capture.ctx.into_control(warnings);
        if let Some(buf) = self.current_cell_para.as_mut() {
            buf.controls.push(control);
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
            if let Some(cell) = self.current_cell.take() {
                self.table.cells.push(cell.cell);
            }
        }
    }

    fn finalize(mut self, warnings: &mut Vec<Hwp5Warning>) -> Hwp5Control {
        self.close_cell_header_footer(warnings);
        self.flush_inline_gso(warnings);
        self.flush_current_cell_paragraph(warnings);
        self.finish_active_cell_if_ready();
        Hwp5Control::Table(self.table)
    }
}

/// A header/footer control inside a table cell and the paragraphs read so
/// far (§19.7 F). Records deeper than the control belong to it.
struct CellHeaderFooter {
    ctx: NestedSubtreeContext,
    current_para: Option<ParaBuf>,
    /// A shape (`gso`) opened in the current header/footer paragraph.
    gso: Option<InlineGsoContext>,
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
/// 취급 등 family 별 의미). Shared by the top-level `handle_ctrl_header`
/// and the two nested `gso ` `InlineGsoContext` entry points (table cell
/// / subtree), which need the DWORD before a `NestedSubtreeContext`
/// exists to carry it (W2p — #134 후속).
fn ctrl_header_properties_raw(data: &[u8]) -> u32 {
    if data.len() >= 8 {
        u32::from_le_bytes([data[4], data[5], data[6], data[7]])
    } else {
        0
    }
}

/// Active non-table control while collecting a nested paragraph subtree.
struct NestedSubtreeContext {
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

    fn note_shape_component(&mut self, data: &[u8], level: u16) {
        self.saw_shape_component = true;
        if self.comp_level.is_none() {
            self.look = crate::schema::section::Hwp5ShapeRender::parse_look(data, true);
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
                    ctrl_id: self.ctrl_id,
                    properties_raw: self.properties_raw,
                    instance_id: self.instance_id,
                    paragraphs: self.paragraphs,
                })
            }
            Some(NestedSubtreeKind::Footer) if self.saw_list_header => {
                Hwp5Control::Footer(Hwp5NestedSubtree {
                    ctrl_id: self.ctrl_id,
                    properties_raw: self.properties_raw,
                    instance_id: self.instance_id,
                    paragraphs: self.paragraphs,
                })
            }
            Some(NestedSubtreeKind::Footnote) if self.saw_list_header => {
                Hwp5Control::Footnote(Hwp5NestedSubtree {
                    ctrl_id: self.ctrl_id,
                    properties_raw: self.properties_raw,
                    instance_id: self.instance_id,
                    paragraphs: self.paragraphs,
                })
            }
            Some(NestedSubtreeKind::Endnote) if self.saw_list_header => {
                Hwp5Control::Endnote(Hwp5NestedSubtree {
                    ctrl_id: self.ctrl_id,
                    properties_raw: self.properties_raw,
                    instance_id: self.instance_id,
                    paragraphs: self.paragraphs,
                })
            }
            Some(NestedSubtreeKind::TextBox) if self.saw_list_header => match self.geometry {
                Some(geometry) => Hwp5Control::TextBox(Hwp5TextBoxControl {
                    look: self.look,
                    ctrl_id: self.ctrl_id,
                    geometry,
                    paragraphs: self.paragraphs,
                    list_header_properties: self.list_header_properties,
                    text_margin: self.text_margin,
                    ctrl_properties: self.properties_raw,
                }),
                None => Hwp5Control::Unknown { ctrl_id: self.ctrl_id, header_data: Vec::new() },
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
                look: input.look.clone(),
                ctrl_id,
                geometry,
                ctrl_properties: input.ctrl_properties,
                shape: ellipse,
            }),
            Some(geometry) => Hwp5Control::Ellipse(Hwp5EllipseControl {
                paragraphs: input.paragraphs.clone(),
                list_header_properties: input.list_header_properties,
                text_margin: input.text_margin,
                look: input.look.clone(),
                ctrl_id,
                geometry,
                ctrl_properties: input.ctrl_properties,
                shape: ellipse,
            }),
            None => Hwp5Control::Unknown { ctrl_id, header_data: Vec::new() },
        };
    }
    if let Some(curve) = input.curve {
        let ctrl_id = input.ctrl_id;
        return match input.geometry {
            Some(geometry) => Hwp5Control::Curve(Hwp5CurveControl {
                look: input.look.clone(),
                ctrl_id,
                geometry,
                points: curve.points,
                segment_types: curve.segment_types,
                ctrl_properties: input.ctrl_properties,
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
                look: input.look.clone(),
                ctrl_id: input.ctrl_id,
                geometry: geometry.clone(),
                start: line.start,
                end: line.end,
                ctrl_properties: input.ctrl_properties,
            });
        }
    }

    let gso_instance_id = input.instance_id;
    match (input.geometry, input.picture, input.ole, input.line, input.polygon) {
        (Some(geometry), Some(picture), None, None, None) => Hwp5Control::Image(Hwp5ImageControl {
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
                look: input.look.clone(),
                ctrl_id: input.ctrl_id,
                geometry,
                ctrl_properties: input.ctrl_properties,
            })
        }
        (Some(geometry), None, None, Some(line), None) => Hwp5Control::Line(Hwp5LineControl {
            look: input.look.clone(),
            ctrl_id: input.ctrl_id,
            geometry,
            start: line.start,
            end: line.end,
            ctrl_properties: input.ctrl_properties,
        }),
        (Some(geometry), None, None, None, Some(polygon)) if polygon.points.len() >= 3 => {
            Hwp5Control::Polygon(Hwp5PolygonControl {
                paragraphs: input.paragraphs.clone(),
                list_header_properties: input.list_header_properties,
                text_margin: input.text_margin,
                look: input.look.clone(),
                ctrl_id: input.ctrl_id,
                geometry,
                points: polygon.points,
                ctrl_properties: input.ctrl_properties,
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

#[derive(Default)]
struct BodyTextParserState {
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
    inline_subtree_gso_ctx: Option<InlineGsoContext>,
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
    /// Active capture state for the memo-content cluster currently being
    /// absorbed. `Some` while inside a `HWPTAG_MEMO_LIST` region.
    memo_content_capture: Option<MemoContentCapture>,
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

    /// Push a new `tbl ` context onto [`Self::table_stack`], enforcing the
    /// [`MAX_TABLE_NESTING`] depth cap (E1 #3).
    ///
    /// At/over the cap the context is still pushed (flagged `over_cap`) so the
    /// level-driven pop/cell/finalize machinery stays balanced with the inner
    /// table's body records — but a `DroppedControl` warning is emitted and the
    /// finalized table is dropped instead of attached, preventing unbounded
    /// nesting from reaching projection.
    fn push_table_context(&mut self, level: u16, instance_id: u32) {
        if self.table_stack.len() >= MAX_TABLE_NESTING {
            self.warnings.push(Hwp5Warning::DroppedControl {
                control: "table",
                reason: format!(
                    "table nesting exceeds depth cap {MAX_TABLE_NESTING}; \
                     dropping deepest table"
                ),
            });
            self.table_stack.push(TableContext::new_over_cap(level, instance_id));
        } else {
            self.table_stack.push(TableContext::new(level, instance_id));
        }
    }

    /// Keeps the just-opened table's `CtrlHeader` placement facts.
    fn note_table_ctrl(&mut self, data: &[u8]) {
        if let Some(ctx) = self.table_stack.last_mut() {
            ctx.table.ctrl = Hwp5TableCtrl::parse(data);
        }
    }

    fn prepare_for_record(&mut self, level: u16) {
        if self
            .table_stack
            .last()
            .and_then(|ctx| ctx.inline_cell_gso_ctx.as_ref())
            .is_some_and(|ctx| level <= ctx.ctrl_depth)
        {
            if let Some(ctx) = self.table_stack.last_mut() {
                ctx.flush_inline_gso(&mut self.warnings);
            }
        }

        if self.inline_subtree_gso_ctx.as_ref().is_some_and(|ctx| level <= ctx.ctrl_depth) {
            attach_inline_gso_control(
                &mut self.current_subtree_para,
                self.inline_subtree_gso_ctx.take(),
                &mut self.warnings,
            );
        }

        while self.table_stack.last().is_some_and(|ctx| level <= ctx.ctrl_depth) {
            let ctx =
                self.table_stack.pop().expect("table_stack.last().is_some() implies pop succeeds");
            let over_cap = ctx.over_cap;
            let finished = ctx.finalize(&mut self.warnings);
            // Over-cap tables are tracked only to keep pop/cell balance; their
            // finalized control is dropped (a `DroppedControl` warning was
            // already emitted at push time) so they cannot corrupt the parent
            // table or leak unbounded nesting into projection (E1 #3).
            if !over_cap {
                // An outermost table opened inside a header/footer/note
                // belongs to that subtree's paragraph.
                let host = if self.table_stack.is_empty() && self.subtree_ctx.is_some() {
                    &mut self.current_subtree_para
                } else {
                    &mut self.current
                };
                attach_finished_table(host, &mut self.table_stack, finished, &mut self.warnings);
            }
        }

        if self.subtree_ctx.as_ref().is_some_and(|ctx| level <= ctx.ctrl_depth) {
            attach_inline_gso_control(
                &mut self.current_subtree_para,
                self.inline_subtree_gso_ctx.take(),
                &mut self.warnings,
            );
            flush_subtree_paragraph(
                &mut self.current_subtree_para,
                self.subtree_ctx.as_mut(),
                &mut self.warnings,
            );
            attach_finished_subtree(&mut self.current, self.subtree_ctx.take(), &mut self.warnings);
        }
    }

    /// Routes a record into the open cell header/footer
    /// ([`TableContext::cell_header_footer`]). A record at or above the
    /// control's level closes it first and is handled as usual (`false`).
    ///
    /// Inside, the records are read the way a top-level header's are
    /// ([`Self::handle_active_subtree_record`]): its list header, its
    /// paragraphs, and inline controls (page numbers etc.; others are kept
    /// as unknown).
    fn route_cell_header_footer_record(&mut self, record: &Record, tag: TagId, level: u16) -> bool {
        let Some(table) = self.table_stack.last_mut() else {
            return false;
        };
        let Some(capture) = table.cell_header_footer.as_mut() else {
            return false;
        };
        if level <= capture.ctx.ctrl_depth {
            table.close_cell_header_footer(&mut self.warnings);
            return false;
        }
        // A shape in the header/footer paragraph owns the records below it;
        // a record back at its level closes it into that paragraph.
        if capture.gso.as_ref().is_some_and(|g| level <= g.ctrl_depth) {
            attach_inline_gso_control(
                &mut capture.current_para,
                capture.gso.take(),
                &mut self.warnings,
            );
        }
        if Self::handle_inline_gso_record(
            record,
            tag,
            level,
            &mut self.warnings,
            capture.gso.as_mut(),
        ) {
            return true;
        }
        match tag {
            TagId::ListHeader => capture.ctx.note_list_header(&record.data),
            TagId::ParaHeader => {
                if let Some(buf) = capture.current_para.take() {
                    capture.ctx.paragraphs.push(buf.finish(&mut self.warnings));
                }
                capture.current_para = Self::parse_para_header_buf(
                    record.header.tag_id,
                    &record.data,
                    &mut self.warnings,
                );
            }
            TagId::ParaText => {
                if let Some(buf) = capture.current_para.as_mut() {
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
                if let Some(buf) = capture.current_para.as_mut() {
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
                if let Some(buf) = capture.current_para.as_mut() {
                    if let Some(segments) = Self::parse_para_line_segments(
                        record.header.tag_id,
                        &record.data,
                        &mut self.warnings,
                    ) {
                        buf.line_segments = segments;
                    }
                }
            }
            TagId::CtrlHeader => {
                let ctrl_id = parse_ctrl_id(&record.data);
                if capture.current_para.is_none() {
                    return true;
                }
                // §19.7 F (review r1): a table or shape in a cell's
                // header/footer is decoded like one in a top-level paragraph
                // — the table on the table stack (it attaches back into this
                // paragraph, [`attach_finished_table`]), the shape in its own
                // context — not flattened into the header's text.
                // 이것을 실패시키는 것: 이 두 갈래를 지우기
                // (`cell_footer_keeps_its_table_and_picture`).
                if ctrl_id == CTRL_ID_TABLE {
                    self.push_table_context(
                        level,
                        extract_ctrl_header_instance_id(&record.data, ctrl_id),
                    );
                    self.note_table_ctrl(&record.data);
                } else if ctrl_id == CTRL_ID_GSO {
                    capture.gso = Some(InlineGsoContext::new(
                        level,
                        ctrl_id,
                        extract_ctrl_header_instance_id(&record.data, ctrl_id),
                        ctrl_header_properties_raw(&record.data),
                        Hwp5ShapeComponentGeometry::parse_from_ctrl_header(&record.data).ok(),
                    ));
                } else if let Some(buf) = capture.current_para.as_mut() {
                    buf.controls.push(
                        Self::typed_inline_family_control(ctrl_id, &record.data).unwrap_or_else(
                            || Hwp5Control::Unknown { ctrl_id, header_data: record.data.clone() },
                        ),
                    );
                }
            }
            _ => {}
        }
        true
    }

    fn handle_active_table_record(&mut self, record: &Record, tag: TagId, level: u16) -> bool {
        if self.table_stack.last().is_none_or(|ctx| level <= ctx.ctrl_depth) {
            return false;
        }

        if self.route_cell_header_footer_record(record, tag, level) {
            return true;
        }

        if Self::handle_inline_gso_record(
            record,
            tag,
            level,
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
                    ctx.table.inner_margin = table_header.inner_margin;
                    ctx.seen_table_body = true;
                }
            }
            TagId::ListHeader => {
                if let Some(ctx) = self.table_stack.last_mut() {
                    if ctx.seen_table_body && level == ctx.ctrl_depth.saturating_add(1) {
                        ctx.flush_current_cell_paragraph(&mut self.warnings);
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
            TagId::CtrlHeader => {
                let ctrl_id = parse_ctrl_id(&record.data);
                // A re-entered `tbl ` inside a cell opens a nested table. Handle
                // it before borrowing the current context so the depth-cap check
                // and `DroppedControl` warning don't fight the `last_mut()`
                // borrow (E1 #3).
                if ctrl_id == CTRL_ID_TABLE {
                    self.push_table_context(
                        level,
                        extract_ctrl_header_instance_id(&record.data, ctrl_id),
                    );
                    self.note_table_ctrl(&record.data);
                } else if let Some(ctx) = self.table_stack.last_mut() {
                    // 이것을 실패시키는 것: 이 갈래를 지우기 — 칸 안 꼬리말
                    // 문단이 칸 문단으로 새고 제어는 unknown 으로 버려진다
                    // (convert `cell_footer_converts_with_and_without_the_layout_cache`).
                    if matches!(ctrl_id, CTRL_ID_HEADER | CTRL_ID_FOOTER)
                        && ctx.current_cell_para.is_some()
                    {
                        ctx.cell_header_footer = Some(CellHeaderFooter {
                            ctx: NestedSubtreeContext::new(
                                level,
                                ctrl_id,
                                ctrl_header_properties_raw(&record.data),
                                extract_ctrl_header_instance_id(&record.data, ctrl_id),
                                None,
                            ),
                            current_para: None,
                            gso: None,
                        });
                    } else if ctrl_id == CTRL_ID_GSO {
                        ctx.inline_cell_gso_ctx = Some(InlineGsoContext::new(
                            level,
                            ctrl_id,
                            extract_ctrl_header_instance_id(&record.data, ctrl_id),
                            ctrl_header_properties_raw(&record.data),
                            Hwp5ShapeComponentGeometry::parse_from_ctrl_header(&record.data).ok(),
                        ));
                    } else if let Some(buf) = ctx.current_cell_para.as_mut() {
                        buf.controls.push(
                            Self::typed_inline_family_control(ctrl_id, &record.data)
                                .unwrap_or_else(|| Hwp5Control::Unknown {
                                    ctrl_id,
                                    header_data: record.data.clone(),
                                }),
                        );
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
        let Some((is_gso, ctrl_depth, geometry, instance_id, group_active)) =
            self.subtree_ctx.as_ref().map(|ctx| {
                (
                    ctx.ctrl_id == CTRL_ID_GSO,
                    ctx.ctrl_depth,
                    ctx.geometry.clone(),
                    ctx.instance_id,
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
            if let Some(ctx) = self.subtree_ctx.as_mut() {
                ctx.group = Some(GsoGroupBuilder::new(level, geometry, instance_id, render));
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

        if Self::handle_inline_gso_record(
            record,
            tag,
            level,
            &mut self.warnings,
            self.inline_subtree_gso_ctx.as_mut(),
        ) {
            return true;
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
                    ctx.note_shape_component(&record.data, level);
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
            TagId::ShapeComponentEllipse => match Hwp5ShapeComponentEllipse::parse(&record.data) {
                Ok(ellipse) => {
                    if let Some(ctx) = self.subtree_ctx.as_mut() {
                        ctx.note_shape_ellipse(ellipse);
                    }
                }
                Err(_) => self.push_unsupported_tag(record.header.tag_id),
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
            TagId::CtrlHeader => {
                if let Some(buf) = self.current_subtree_para.as_mut() {
                    let ctrl_id = parse_ctrl_id(&record.data);
                    // §19.7 F (review r1): a table in a header/footer/note is
                    // read on the table stack and attaches back into this
                    // paragraph ([`attach_finished_table`]) instead of its
                    // cells being flattened into the subtree's text.
                    // 이것을 실패시키는 것: 이 갈래를 지우기
                    // (`header_keeps_its_table`).
                    if ctrl_id == CTRL_ID_TABLE {
                        self.push_table_context(
                            level,
                            extract_ctrl_header_instance_id(&record.data, ctrl_id),
                        );
                        self.note_table_ctrl(&record.data);
                        return true;
                    }
                    if ctrl_id == CTRL_ID_GSO {
                        self.inline_subtree_gso_ctx = Some(InlineGsoContext::new(
                            level,
                            ctrl_id,
                            extract_ctrl_header_instance_id(&record.data, ctrl_id),
                            ctrl_header_properties_raw(&record.data),
                            Hwp5ShapeComponentGeometry::parse_from_ctrl_header(&record.data).ok(),
                        ));
                    } else {
                        buf.controls.push(
                            Self::typed_inline_family_control(ctrl_id, &record.data)
                                .unwrap_or_else(|| Hwp5Control::Unknown {
                                    ctrl_id,
                                    header_data: record.data.clone(),
                                }),
                        );
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

    /// 중첩 컨텍스트(표 셀·subtree)의 CtrlHeader 를 top-level 과 동일하게
    /// 타입화한다 — **inline marker 가족(atno/nwno/pghd)만**. 나머지는
    /// Unknown round-trip 보존. (W4: 집계 경고가 각주 subtree 안 atno 의
    /// 기존 무음 드롭을 적발 — 중첩에서도 같은 wire 이므로 같은 parse 재사용.)
    fn typed_inline_family_control(ctrl_id: u32, data: &[u8]) -> Option<Hwp5Control> {
        match ctrl_id {
            CTRL_ID_ATNO => {
                crate::schema::section::Hwp5InlinePageNumberControl::parse(ctrl_id, data)
                    .map(Hwp5Control::InlinePageNumber)
            }
            CTRL_ID_NEW_NUMBER => {
                crate::schema::section::Hwp5NewNumberControl::parse(ctrl_id, data)
                    .map(Hwp5Control::NewNumber)
            }
            CTRL_ID_PAGE_HIDING => {
                crate::schema::section::Hwp5PageHidingControl::parse(ctrl_id, data)
                    .map(Hwp5Control::PageHiding)
            }
            _ => None,
        }
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
            TagId::ParaHeader if level == 0 => {
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
        if matches!(tag, TagId::MemoList) && level == 1 {
            if let Some(memo_id) = parse_memo_list_id(&record.data) {
                self.flush_memo_content_capture();
                self.memo_content_capture = Some(MemoContentCapture {
                    memo_id,
                    saw_list_header: false,
                    current_para: None,
                    paragraphs: Vec::new(),
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
        if matches!(tag, TagId::CtrlData) && level == 2 {
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

    /// Dispatches a top-level `CtrlHeader` (`0x47`) record by its
    /// `ctrl_id` (task #90 — extracted from `handle_top_level_record`).
    ///
    /// Families that open nested regions (`tbl `, `head`/`foot`/`fn`/`en`/
    /// `gso `) push parser state; one-shot field/annotation controls parse
    /// and attach to the current paragraph; unrecognized ids fall through
    /// to `Hwp5Control::Unknown` for round-trip preservation.
    fn handle_ctrl_header(&mut self, record: &Record, level: u16) {
        let ctrl_id = parse_ctrl_id(&record.data);
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
        } else if ctrl_id == CTRL_ID_DUTMAL {
            // `tdut` ctrl carries the dutmal (덧말) main/sub text
            // strings + posType. The paired inline `0x17` marker
            // in the body's `ParaText` decides the visible
            // position; this push keeps the projected Control in
            // doc order via `current.controls`.
            if let Some(dutmal) = Hwp5DutmalControl::parse(ctrl_id, &record.data) {
                if let Some(buf) = self.current.as_mut() {
                    buf.controls.push(Hwp5Control::Dutmal(dutmal));
                }
            } else {
                self.push_unsupported_tag(record.header.tag_id);
            }
        } else if ctrl_id == CTRL_ID_COMPOSE {
            // `tcps` ctrl carries the compose (글자겹침) text,
            // circleType/composeType enums, and 10 charPr refs.
            // Layout assumes `charPrCnt == 10` per HWPX schema;
            // malformed payloads fall through to Unknown so the
            // surrounding section keeps round-tripping.
            if let Some(compose) =
                crate::schema::section::Hwp5ComposeControl::parse(ctrl_id, &record.data)
            {
                if let Some(buf) = self.current.as_mut() {
                    buf.controls.push(Hwp5Control::Compose(compose));
                }
            } else {
                self.push_unsupported_tag(record.header.tag_id);
            }
        } else if ctrl_id == CTRL_ID_CLICK_HERE {
            // `%clk` ctrl carries the CLICK_HERE (누름틀) press-field
            // hint/help text. The form-mode `name` lives in the
            // immediately following `0x57 lvl=2` sub-record. Flush
            // any orphaned pending first (defensive), then store
            // the parsed control so the next `0x57` can attach the
            // name. (Wave 12l.)
            self.flush_pending_clickhere();
            match crate::schema::section::Hwp5ClickHereControl::parse(ctrl_id, &record.data) {
                Ok(clickhere) => self.pending_clickhere = Some(clickhere),
                Err(err) => {
                    self.warnings.push(Hwp5Warning::DroppedControl {
                        control: "clickhere",
                        reason: format!(
                            "malformed %clk CtrlHeader payload ({}); \
                                     dropping press-field metadata",
                            err.as_str()
                        ),
                    });
                }
            }
        } else if ctrl_id == CTRL_ID_INDEXMARK {
            // `idxm` ctrl carries the IndexMark (찾아보기 표시)
            // `primary` text and optional `secondary` text. A
            // malformed payload emits a targeted
            // `DroppedControl` warning (per Codex review) so
            // audit baselines can attribute the loss to the
            // IndexMark code path instead of the generic
            // `UnsupportedTag(0x47)` bucket. (Wave 12k.)
            if let Some(indexmark) =
                crate::schema::section::Hwp5IndexMarkControl::parse(ctrl_id, &record.data)
            {
                if let Some(buf) = self.current.as_mut() {
                    buf.controls.push(Hwp5Control::IndexMark(indexmark));
                }
            } else {
                self.warnings.push(Hwp5Warning::DroppedControl {
                    control: "indexmark",
                    reason: "malformed idxm CtrlHeader payload; dropping index mark".to_string(),
                });
            }
        } else if ctrl_id == CTRL_ID_FIELD_SUMMERY {
            // `%smr` ctrl carries a SUMMERY auto-field Command
            // `$token` (e.g. `$author`, `$modifiedtime`). The
            // projection layer dispatches the token to a typed
            // `FieldType` or `Control::UnknownSummary`. No
            // follow-up sub-record (Wave 12n).
            if let Some(summary) =
                crate::schema::section::Hwp5SummaryControl::parse(ctrl_id, &record.data)
            {
                if let Some(buf) = self.current.as_mut() {
                    buf.controls.push(Hwp5Control::SummaryField(summary));
                }
            } else {
                self.warnings.push(Hwp5Warning::DroppedControl {
                    control: "summary_field",
                    reason: "malformed %smr CtrlHeader payload; dropping auto-field".to_string(),
                });
            }
        } else if ctrl_id == CTRL_ID_FIELD_DATE_CODE {
            // `%dte` ctrl carries a raw date/time format-code
            // (e.g. `"\:1년 2월 3일 (6);0;"`, `"T\:;0;"`). The
            // projection layer wraps it in `Control::DateCodeField`
            // with `is_time_mode` derived from the `T` prefix.
            // (Wave 12n.)
            if let Some(date_code) =
                crate::schema::section::Hwp5DateCodeControl::parse(ctrl_id, &record.data)
            {
                if let Some(buf) = self.current.as_mut() {
                    buf.controls.push(Hwp5Control::DateCodeField(date_code));
                }
            } else {
                self.warnings.push(Hwp5Warning::DroppedControl {
                    control: "date_code_field",
                    reason: "malformed %dte CtrlHeader payload; dropping date-code field"
                        .to_string(),
                });
            }
        } else if ctrl_id == CTRL_ID_FIELD_PATH {
            // `%pat` ctrl carries a path/file-name format-code
            // Command (`"$P"`, `"$F"`, `"$P$F"`). Wave 12n.
            if let Some(pat) =
                crate::schema::section::Hwp5PathFieldControl::parse(ctrl_id, &record.data)
            {
                if let Some(buf) = self.current.as_mut() {
                    buf.controls.push(Hwp5Control::PathField(pat));
                }
            } else {
                self.warnings.push(Hwp5Warning::DroppedControl {
                    control: "path_field",
                    reason: "malformed %pat CtrlHeader payload; dropping path field".to_string(),
                });
            }
        } else if ctrl_id == CTRL_ID_FIELD_CROSSREF {
            // `%xrf` ctrl carries a structured cross-reference Command
            // `?<target>;N1;N2;N3;N4;` + 8-byte trailer. Wave 12m
            // Phase 2. Schema preserves raw N1/N2/N3 codes;
            // projection boundary maps them to typed RefType /
            // RefContentType / RefTarget.
            if let Some(xrf) =
                crate::schema::section::Hwp5CrossRefControl::parse(ctrl_id, &record.data)
            {
                if let Some(buf) = self.current.as_mut() {
                    buf.controls.push(Hwp5Control::CrossRef(xrf));
                }
            } else {
                self.warnings.push(Hwp5Warning::DroppedControl {
                    control: "crossref",
                    reason: "malformed %xrf CtrlHeader payload; dropping cross-reference"
                        .to_string(),
                });
            }
        } else if ctrl_id == CTRL_ID_NEW_NUMBER {
            // `nwno` 새 번호 지정 — 10바이트 payload (W2, F1 실측 §1.2).
            // malformed 여도 **Unknown placeholder 를 push** 해 marker↔control
            // 큐 정렬을 보존한다 (독립 리뷰 High #1: 승격된 0x15 marker 가
            // 뒤따르는 GSO 를 오소비하는 queue poisoning 방지).
            if let Some(nwno) =
                crate::schema::section::Hwp5NewNumberControl::parse(ctrl_id, &record.data)
            {
                if let Some(buf) = self.current.as_mut() {
                    buf.controls.push(Hwp5Control::NewNumber(nwno));
                }
            } else {
                self.warnings.push(Hwp5Warning::DroppedControl {
                    control: "new_number",
                    reason: "malformed nwno CtrlHeader payload; dropping number restart"
                        .to_string(),
                });
                if let Some(buf) = self.current.as_mut() {
                    buf.controls
                        .push(Hwp5Control::Unknown { ctrl_id, header_data: record.data.clone() });
                }
            }
        } else if ctrl_id == CTRL_ID_PAGE_HIDING {
            // `pghd` 감추기 — 8바이트 payload (W3, F2 실측 §1.2).
            if let Some(pghd) =
                crate::schema::section::Hwp5PageHidingControl::parse(ctrl_id, &record.data)
            {
                if let Some(buf) = self.current.as_mut() {
                    buf.controls.push(Hwp5Control::PageHiding(pghd));
                }
            } else {
                self.warnings.push(Hwp5Warning::DroppedControl {
                    control: "page_hiding",
                    reason: "malformed pghd CtrlHeader payload; dropping page hiding".to_string(),
                });
                // 큐 정렬 보존 placeholder (독립 리뷰 High #1).
                if let Some(buf) = self.current.as_mut() {
                    buf.controls
                        .push(Hwp5Control::Unknown { ctrl_id, header_data: record.data.clone() });
                }
            }
        } else if ctrl_id == CTRL_ID_ATNO {
            // `atno` ctrl carries a single 4-byte kind flag
            // (`0x00`/`0x06`). No Command/trailer. Wave 12n.
            if let Some(atno) =
                crate::schema::section::Hwp5InlinePageNumberControl::parse(ctrl_id, &record.data)
            {
                if let Some(buf) = self.current.as_mut() {
                    buf.controls.push(Hwp5Control::InlinePageNumber(atno));
                }
            } else {
                self.warnings.push(Hwp5Warning::DroppedControl {
                    control: "inline_page_number",
                    reason: "malformed atno CtrlHeader payload; dropping inline page".to_string(),
                });
                // 큐 정렬 보존 placeholder — 0x12 marker 승격과 동계열의
                // 선행 결함도 함께 수리 (독립 리뷰 High #1).
                if let Some(buf) = self.current.as_mut() {
                    buf.controls
                        .push(Hwp5Control::Unknown { ctrl_id, header_data: record.data.clone() });
                }
            }
        } else if let Some(command) =
            (ctrl_id == CTRL_ID_MEMO).then(|| Hwp5MemoCommand::parse(&record.data)).flatten()
        {
            // Push a placeholder `Hwp5Control::Memo` now; its content
            // paragraphs get filled in `finish()` by matching this
            // `memo_id` against the captured `HWPTAG_MEMO_LIST`
            // clusters. The `%unk` ctrl_id is shared with other
            // user-defined controls, so we only treat it as a memo
            // when the command string starts with `"MEMO/"` (the
            // `parse_memo_command_id` discriminator); non-memo
            // `%unk` payloads keep falling through to the Unknown
            // arm below for round-trip preservation.
            if let Some(buf) = self.current.as_mut() {
                buf.controls.push(Hwp5Control::Memo(Hwp5MemoControl {
                    ctrl_id: CTRL_ID_MEMO,
                    command,
                    paragraphs: Vec::new(),
                }));
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
        match (tag, level) {
            (TagId::ListHeader, 1) => {
                capture.saw_list_header = true;
                true
            }
            (TagId::ParaHeader, 1) if capture.saw_list_header => {
                if let Some(buf) = capture.current_para.take() {
                    capture.paragraphs.push(buf.finish(&mut self.warnings));
                }
                capture.current_para = Self::parse_para_header_buf(
                    record.header.tag_id,
                    &record.data,
                    &mut self.warnings,
                );
                true
            }
            (TagId::ParaText, 2) => {
                if let Some(buf) = capture.current_para.as_mut() {
                    if let Some(text) = Self::parse_para_text_value(
                        record.header.tag_id,
                        &record.data,
                        &mut self.warnings,
                    ) {
                        buf.text = Some(text);
                    }
                }
                true
            }
            (TagId::ParaCharShape, 2) => {
                if let Some(buf) = capture.current_para.as_mut() {
                    if let Some(runs) = Self::parse_para_char_shape_runs(
                        record.header.tag_id,
                        &record.data,
                        &mut self.warnings,
                    ) {
                        buf.char_shape_runs = runs;
                    }
                }
                true
            }
            (TagId::ParaLineSeg, 2) => {
                if let Some(buf) = capture.current_para.as_mut() {
                    if let Some(segments) = Self::parse_para_line_segments(
                        record.header.tag_id,
                        &record.data,
                        &mut self.warnings,
                    ) {
                        buf.line_segments = segments;
                    }
                }
                true
            }
            _ => {
                // Unknown record inside the cluster region: swallow rather
                // than letting the normal arms touch body-paragraph state.
                true
            }
        }
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
        if let Some(capture) = self.memo_content_capture.take() {
            let memo_id = capture.memo_id;
            let paragraphs = capture.into_paragraphs(&mut self.warnings);
            match self.memo_contents.entry(memo_id) {
                Entry::Vacant(slot) => {
                    slot.insert(paragraphs);
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
        if !self.memo_contents.is_empty() {
            fill_memo_placeholders(
                &mut self.paragraphs,
                &mut self.memo_contents,
                &mut self.warnings,
            );
            for orphan_id in self.memo_contents.keys() {
                self.warnings.push(Hwp5Warning::DroppedControl {
                    control: "memo_content_cluster",
                    reason: format!(
                        "orphan content cluster for memo_id={orphan_id}; no matching MEMO ctrl"
                    ),
                });
            }
            self.memo_contents.clear();
        } else {
            warn_unfilled_memo_placeholders(&self.paragraphs, &mut self.warnings);
        }
    }

    fn finish(mut self) -> SectionResult {
        // Drain any orphan `%clk` whose `0x57` name sub-record was
        // missing or replaced by another top-level record (Wave 12l).
        self.flush_pending_clickhere();
        while let Some(ctx) = self.table_stack.pop() {
            let over_cap = ctx.over_cap;
            let finished = ctx.finalize(&mut self.warnings);
            // See `prepare_for_record`: over-cap tables are dropped, not
            // attached (E1 #3).
            if !over_cap {
                // An outermost table opened inside a header/footer/note
                // belongs to that subtree's paragraph.
                let host = if self.table_stack.is_empty() && self.subtree_ctx.is_some() {
                    &mut self.current_subtree_para
                } else {
                    &mut self.current
                };
                attach_finished_table(host, &mut self.table_stack, finished, &mut self.warnings);
            }
        }
        attach_inline_gso_control(
            &mut self.current_subtree_para,
            self.inline_subtree_gso_ctx.take(),
            &mut self.warnings,
        );
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
        self.attach_memo_contents_to_placeholders();

        SectionResult {
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

    fn handle_inline_gso_record(
        record: &Record,
        tag: TagId,
        level: u16,
        warnings: &mut Vec<Hwp5Warning>,
        inline_gso_ctx: Option<&mut InlineGsoContext>,
    ) -> bool {
        let Some(ctx) = inline_gso_ctx else {
            return false;
        };
        // A group (`$con`) in a table cell or another object's text takes the
        // same group path as one in a body paragraph.
        if ctx.route_group_record(record, tag, level, warnings) {
            return true;
        }
        // The shape's text and any shape inside it (§19.5 1a).
        if ctx.route_text_record(record, tag, level, warnings) {
            return true;
        }
        ctx.note_shape_record(record, tag, warnings);
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
) {
    for para in paragraphs {
        for control in &mut para.controls {
            match control {
                Hwp5Control::Memo(memo) if memo.paragraphs.is_empty() => {
                    if let Some(content) = memo_contents.remove(&memo.command.memo_id) {
                        memo.paragraphs = content;
                    } else {
                        warnings.push(Hwp5Warning::DroppedControl {
                            control: "memo",
                            reason: format!(
                                "no content cluster for memo_id={}",
                                memo.command.memo_id
                            ),
                        });
                    }
                }
                Hwp5Control::Table(table) => {
                    for cell in &mut table.cells {
                        fill_memo_placeholders(&mut cell.paragraphs, memo_contents, warnings);
                    }
                }
                Hwp5Control::Header(subtree)
                | Hwp5Control::Footer(subtree)
                | Hwp5Control::Footnote(subtree)
                | Hwp5Control::Endnote(subtree) => {
                    fill_memo_placeholders(&mut subtree.paragraphs, memo_contents, warnings);
                }
                Hwp5Control::TextBox(textbox) => {
                    fill_memo_placeholders(&mut textbox.paragraphs, memo_contents, warnings);
                }
                _ => {}
            }
        }
    }
}

/// Walks paragraphs and surfaces a warning for every empty `Memo`
/// placeholder. Used in the no-clusters fast path.
fn warn_unfilled_memo_placeholders(paragraphs: &[Hwp5Paragraph], warnings: &mut Vec<Hwp5Warning>) {
    for para in paragraphs {
        for control in &para.controls {
            match control {
                Hwp5Control::Memo(memo) if memo.paragraphs.is_empty() => {
                    warnings.push(Hwp5Warning::DroppedControl {
                        control: "memo",
                        reason: format!("no content cluster for memo_id={}", memo.command.memo_id),
                    });
                }
                Hwp5Control::Table(table) => {
                    for cell in &table.cells {
                        warn_unfilled_memo_placeholders(&cell.paragraphs, warnings);
                    }
                }
                Hwp5Control::Header(subtree)
                | Hwp5Control::Footer(subtree)
                | Hwp5Control::Footnote(subtree)
                | Hwp5Control::Endnote(subtree) => {
                    warn_unfilled_memo_placeholders(&subtree.paragraphs, warnings);
                }
                Hwp5Control::TextBox(textbox) => {
                    warn_unfilled_memo_placeholders(&textbox.paragraphs, warnings);
                }
                _ => {}
            }
        }
    }
}

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
    inner_margin: Option<Hwp5TableCellMargin>,
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
    ParsedTableHeader {
        rows,
        cols,
        page_break,
        repeat_header,
        cell_spacing,
        border_fill_id,
        inner_margin,
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
        // A table read inside a cell's header/footer belongs to that
        // header/footer's paragraph (the capture is still open: it closes
        // only on a record at or above its own level, after this pop).
        if let Some(buf) =
            parent.cell_header_footer.as_mut().and_then(|capture| capture.current_para.as_mut())
        {
            buf.controls.push(control);
            return;
        }
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

fn attach_inline_gso_control(
    current_para: &mut Option<ParaBuf>,
    inline_gso_ctx: Option<InlineGsoContext>,
    warnings: &mut Vec<Hwp5Warning>,
) {
    let Some(ctx) = inline_gso_ctx else {
        return;
    };
    let control = ctx.into_control(warnings);
    if let Some(buf) = current_para.as_mut() {
        buf.controls.push(control);
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
    use crate::schema::section::Hwp5ShapePoint;
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

    fn one_cell_table(stream: &mut Vec<u8>, paragraphs: u32) {
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
        let hints = crate::layout_hint_patch::capture_layout_hints(std::slice::from_ref(&result));
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
        let hwpforge_core::control::Control::Group { children, .. } = control(&paras[1]).unwrap()
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
        // MAX_TABLE_NESTING + 1 deep: must NOT panic/OOM, must emit at least one
        // DroppedControl{control:"table"} warning, must keep decoding the outer
        // content, and the attached tree must be capped (no deeper than the
        // cap) — proving push/pop stayed balanced.
        let depth = MAX_TABLE_NESTING + 1;
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
            attached_depth <= MAX_TABLE_NESTING,
            "attached nesting depth {attached_depth} must not exceed cap {MAX_TABLE_NESTING}",
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
        // W2p: 표 셀 안 `gso ` 는 `InlineGsoContext`(cell 경로,
        // `ctx.inline_cell_gso_ctx`) 를 통해 같은 DWORD 를 carry 해야 한다 —
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
        // W2p: 글상자 subtree 안 `gso ` 는 `InlineGsoContext`(subtree 경로,
        // `self.inline_subtree_gso_ctx`) 를 통해 같은 DWORD 를 carry 해야
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
        let mut ctx = InlineGsoContext::new(0, CTRL_ID_GSO, 0, 0, Some(geometry));
        ctx.note_shape_component(&[], 0);
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
        ctx.note_shape_component(&[], 0);
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
}
