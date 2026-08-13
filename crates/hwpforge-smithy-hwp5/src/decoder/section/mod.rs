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

    /// Build the final `Hwp5Paragraph`, consuming this buffer.
    fn finish(self) -> Hwp5Paragraph {
        let text_segments =
            self.text.map_or_else(Vec::new, |paragraph_text| paragraph_text.segments);
        let text = segments_to_string(&text_segments);
        Hwp5Paragraph {
            text,
            text_segments,
            para_shape_id: self.header.para_shape_id,
            style_id: self.header.style_id,
            // divide_sort bit2/bit3 = 쪽/단 나누기 (hwp-rs 확증 + F2 실측:
            // 한컴 재저장 HWPX pageBreak="1" 대응 — W3 carry 시작).
            page_break: self.header.divide_sort & 0x04 != 0,
            column_break: self.header.divide_sort & 0x08 != 0,
            char_shape_runs: self.char_shape_runs,
            line_segments: self.line_segments,
            controls: self.controls,
        }
    }
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
    fn into_paragraphs(mut self) -> Vec<Hwp5Paragraph> {
        if let Some(buf) = self.current_para.take() {
            self.paragraphs.push(buf.finish());
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
                rows: 0,
                cols: 0,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: Vec::new(),
                instance_id,
            },
            seen_table_body: false,
            current_cell: None,
            current_cell_para: None,
            inline_cell_gso_ctx: None,
            over_cap,
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
    paragraphs: Vec<Hwp5Paragraph>,
    /// Active group builder, set when this gso scope's first `ShapeComponent`
    /// carries the `"$con"` type tag. When `Some`, all subtree records are
    /// routed to the group instead of the flat single-shape slots above.
    group: Option<GsoGroupBuilder>,
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
            paragraphs: Vec::new(),
            group: None,
        }
    }

    fn note_list_header(&mut self, data: &[u8]) {
        self.saw_list_header = true;
        // 표 65 문단 리스트 헤더: INT16 문단 수 + UINT32 속성. Capture the
        // 속성 word so projection can recover bits 5–6 (text vertical align).
        if let Some(bytes) = data.get(2..6) {
            if let Ok(arr) = <[u8; 4]>::try_from(bytes) {
                self.list_header_properties = Some(u32::from_le_bytes(arr));
            }
        }
    }

    fn note_shape_rectangle(&mut self) {
        self.saw_shape_rectangle = true;
    }

    fn note_shape_component(&mut self, data: &[u8]) {
        self.saw_shape_component = true;
        if let Some(code) = data.get(..4) {
            self.shape_component_kind = Some([code[0], code[1], code[2], code[3]]);
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
                    ctrl_id: self.ctrl_id,
                    geometry,
                    paragraphs: self.paragraphs,
                    list_header_properties: self.list_header_properties,
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
                instance_id: self.instance_id,
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
            Some(geometry) if ellipse.is_arc() => {
                Hwp5Control::Arc(Hwp5ArcControl { ctrl_id, geometry })
            }
            Some(geometry) => Hwp5Control::Ellipse(Hwp5EllipseControl { ctrl_id, geometry }),
            None => Hwp5Control::Unknown { ctrl_id, header_data: Vec::new() },
        };
    }
    if let Some(curve) = input.curve {
        let ctrl_id = input.ctrl_id;
        return match input.geometry {
            Some(geometry) => Hwp5Control::Curve(Hwp5CurveControl {
                ctrl_id,
                geometry,
                points: curve.points,
                segment_types: curve.segment_types,
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
                ctrl_id: input.ctrl_id,
                geometry: geometry.clone(),
                start: line.start,
                end: line.end,
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
        _ => Hwp5Control::Unknown { ctrl_id: input.ctrl_id, header_data: Vec::new() },
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
            let ctx =
                self.table_stack.pop().expect("table_stack.last().is_some() implies pop succeeds");
            let over_cap = ctx.over_cap;
            let finished = ctx.finalize();
            // Over-cap tables are tracked only to keep pop/cell balance; their
            // finalized control is dropped (a `DroppedControl` warning was
            // already emitted at push time) so they cannot corrupt the parent
            // table or leak unbounded nesting into projection (E1 #3).
            if !over_cap {
                attach_finished_table(
                    &mut self.current,
                    &mut self.table_stack,
                    finished,
                    &mut self.warnings,
                );
            }
        }

        if self.subtree_ctx.as_ref().is_some_and(|ctx| level <= ctx.ctrl_depth) {
            attach_inline_gso_control(
                &mut self.current_subtree_para,
                self.inline_subtree_gso_ctx.take(),
            );
            flush_subtree_paragraph(&mut self.current_subtree_para, self.subtree_ctx.as_mut());
            attach_finished_subtree(&mut self.current, self.subtree_ctx.take(), &mut self.warnings);
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
                } else if let Some(ctx) = self.table_stack.last_mut() {
                    if ctrl_id == CTRL_ID_GSO {
                        ctx.inline_cell_gso_ctx = Some(InlineGsoContext::new(
                            level,
                            ctrl_id,
                            extract_ctrl_header_instance_id(&record.data, ctrl_id),
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
            if let Some(ctx) = self.subtree_ctx.as_mut() {
                ctx.group = Some(GsoGroupBuilder::new(level, geometry, instance_id));
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
                if let Some(ctx) = self.subtree_ctx.as_mut() {
                    ctx.note_shape_component(&record.data);
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
                    if ctrl_id == CTRL_ID_GSO {
                        self.inline_subtree_gso_ctx = Some(InlineGsoContext::new(
                            level,
                            ctrl_id,
                            extract_ctrl_header_instance_id(&record.data, ctrl_id),
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
            let properties_raw = if record.data.len() >= 8 {
                u32::from_le_bytes([record.data[4], record.data[5], record.data[6], record.data[7]])
            } else {
                0
            };
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
                .unwrap_or(Hwp5ShapeComponentGeometry { x: 0, y: 0, width: 0, height: 0 });
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
                    capture.paragraphs.push(buf.finish());
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
            let paragraphs = capture.into_paragraphs();
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
            let finished = ctx.finalize();
            // See `prepare_for_record`: over-cap tables are dropped, not
            // attached (E1 #3).
            if !over_cap {
                attach_finished_table(
                    &mut self.current,
                    &mut self.table_stack,
                    finished,
                    &mut self.warnings,
                );
            }
        }
        attach_inline_gso_control(
            &mut self.current_subtree_para,
            self.inline_subtree_gso_ctx.take(),
        );
        flush_subtree_paragraph(&mut self.current_subtree_para, self.subtree_ctx.as_mut());
        attach_finished_subtree(&mut self.current, self.subtree_ctx.take(), &mut self.warnings);

        if let Some(buf) = self.current.take() {
            self.paragraphs.push(buf.finish());
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
        warnings: &mut Vec<Hwp5Warning>,
        inline_gso_ctx: Option<&mut InlineGsoContext>,
    ) -> bool {
        let Some(ctx) = inline_gso_ctx else {
            return false;
        };

        match tag {
            TagId::ShapeComponent => ctx.note_shape_component(&record.data),
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
            TagId::ShapeComponentEllipse => match Hwp5ShapeComponentEllipse::parse(&record.data) {
                Ok(ellipse) => ctx.note_shape_ellipse(ellipse),
                Err(_) => warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::ShapeComponentCurve => match Hwp5ShapeComponentCurve::parse(&record.data) {
                Ok(curve) => ctx.note_shape_curve(curve),
                Err(_) => warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::ShapeTextArt => {
                match crate::schema::section::Hwp5ShapeTextArt::parse(&record.data) {
                    Ok(ta) => ctx.note_shape_text_art(ta),
                    Err(_) => warnings.push(Hwp5Warning::UnsupportedTag {
                        tag_id: record.header.tag_id,
                        offset: 0,
                    }),
                }
            }
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
    fn ctrl_header_nwno_truncated_payload_warns_dropped() {
        // 10바이트 미만 payload → 무음 드롭 금지, DroppedControl 경고.
        let mut nwno = CTRL_ID_NEW_NUMBER.to_le_bytes().to_vec();
        nwno.extend_from_slice(&[0x00, 0x00]);

        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &nwno));
        let result = parse_body_text(&stream, &version()).unwrap();
        assert!(result.paragraphs[0].controls.is_empty());
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
    fn ctrl_header_pghd_truncated_payload_warns_dropped() {
        let mut pghd = CTRL_ID_PAGE_HIDING.to_le_bytes().to_vec();
        pghd.extend_from_slice(&[0x20]);

        let mut stream = Vec::new();
        stream.extend(make_record(TagId::ParaHeader, 0, &para_header_data(0, 0)));
        stream.extend(make_record(TagId::CtrlHeader, 0, &pghd));
        let result = parse_body_text(&stream, &version()).unwrap();
        assert!(result.paragraphs[0].controls.is_empty());
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
        let mut ctx = InlineGsoContext::new(0, CTRL_ID_GSO, 0, Some(geometry));
        ctx.note_shape_component(&[]);
        ctx.note_shape_picture(Hwp5ShapePicture::parse(&shape_picture_data(1)).unwrap());
        ctx.note_shape_ole(
            Hwp5ShapeComponentOle::parse(&shape_component_ole_data(1, 9000, 8000)).unwrap(),
        );

        match ctx.into_control() {
            Hwp5Control::Unknown { ctrl_id, .. } => assert_eq!(ctrl_id, CTRL_ID_GSO),
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
        let mut ctx = NestedSubtreeContext::new(0, CTRL_ID_GSO, 0, 0, Some(geometry));
        ctx.note_shape_component(&[]);
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
            instance_id: 0,
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
