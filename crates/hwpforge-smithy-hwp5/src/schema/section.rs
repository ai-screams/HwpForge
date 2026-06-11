//! HWP5 `BodyText` section record schema types.
//!
//! Defines typed Rust structs for paragraph header records, run records,
//! and control object records found in `BodyText/Section{N}` streams.

use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;

use crate::error::{Hwp5Error, Hwp5Result};

// ---------------------------------------------------------------------------
// Hwp5ParaHeader
// ---------------------------------------------------------------------------

/// Parsed from a `ParaHeader` (tag `0x42`) record in a BodyText section.
///
/// Contains metadata describing a single paragraph: how many characters it
/// has, which style and shape IDs apply, and how many child run records to
/// expect.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct Hwp5ParaHeader {
    /// Number of characters in this paragraph (in UTF-16 code units).
    pub char_count: u32,
    /// Control mask — bitfield indicating which child records are present.
    pub control_mask: u32,
    /// Paragraph shape ID (index into the DocInfo `ParaShape` table).
    pub para_shape_id: u16,
    /// Style ID (index into the DocInfo `Style` table).
    pub style_id: u8,
    /// Number of line segment entries in the companion `ParaLineSeg` record.
    pub line_seg_count: u16,
    /// Number of character-shape run entries in the companion `ParaCharShape` record.
    pub char_shape_count: u16,
    /// Paragraph instance ID — 한컴 wire 의 unique per-paragraph
    /// identifier carried in HWP5 `ParaHeader[18..22]` (u32 LE). HWPX
    /// cross-ref Command 의 `?#<id>` target lookup (Outline / 다른
    /// paragraph 대상 참조) 이 이 값을 매칭. Wave 12p Step 1: 이전엔
    /// `parse()` 가 skip 했던 필드를 carry 시작.
    pub instance_id: u32,
}

impl Hwp5ParaHeader {
    /// Minimum byte length for a `ParaHeader` payload.
    ///
    /// The base layout (without version-gated trailing fields) is 22 bytes.
    /// Real files typically have 22 bytes (v5.0.x base) or 24 bytes
    /// (v5.0.3.2+ with `is_merged_by_track`).
    const MIN_SIZE: usize = 22;

    /// Parse a `ParaHeader` record from its raw payload bytes.
    ///
    /// Layout (packed, no padding):
    /// - `[0..4]`   char_count (u32 LE)
    /// - `[4..8]`   control_mask (u32 LE)
    /// - `[8..10]`  para_shape_id (u16 LE)
    /// - `[10]`     style_id (u8)
    /// - `[11]`     page_break / divide_sort (u8)
    /// - `[12..14]` char_shape_count (u16 LE)
    /// - `[14..16]` range_tag_count (u16 LE)
    /// - `[16..18]` line_seg_count (u16 LE)
    /// - `[18..22]` instance_id (u32 LE)
    /// - `[22..24]` is_merged_by_track (u16 LE) — v5.0.3.2+ only
    ///
    /// # Errors
    ///
    /// Returns [`Hwp5Error::RecordParse`] if `data` is shorter than 22 bytes.
    pub(crate) fn parse(data: &[u8]) -> Hwp5Result<Self> {
        if data.len() < Self::MIN_SIZE {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ParaHeader too short: {} bytes (expected >= {})",
                    data.len(),
                    Self::MIN_SIZE
                ),
            });
        }
        let mut cur = Cursor::new(data);
        let char_count = cur.read_u32::<LittleEndian>()?;
        let control_mask = cur.read_u32::<LittleEndian>()?;
        let para_shape_id = cur.read_u16::<LittleEndian>()?;
        let style_id = cur.read_u8()?;
        // [11] page_break / divide_sort — skip
        cur.set_position(12);
        let char_shape_count = cur.read_u16::<LittleEndian>()?;
        // [14..16] range_tag_count — skip
        cur.set_position(16);
        let line_seg_count = cur.read_u16::<LittleEndian>()?;
        // [18..22] instance_id (u32 LE) — Wave 12p Step 1: HWPX cross-ref
        // target ID for outline-style references. ParaHeader MIN_SIZE
        // already guarantees >= 22 bytes so the read is bounds-safe.
        let instance_id = cur.read_u32::<LittleEndian>()?;
        Ok(Self {
            char_count,
            control_mask,
            para_shape_id,
            style_id,
            line_seg_count,
            char_shape_count,
            instance_id,
        })
    }
}

// ---------------------------------------------------------------------------
// IndexMark inline-marker discriminator (Wave 12k)
// ---------------------------------------------------------------------------

use crate::ctrl_ids::{CTRL_ID_ATNO, CTRL_ID_INDEXMARK};

/// Reads the LE-stored ctrl_id from the first four bytes of an
/// inline-marker's `extra` block and returns it as the BE-ascii u32
/// HWP5 uses for CtrlHeader matching. Wave 12k's `0x16` arm calls
/// this to decide whether to promote a marker to `TextSegment::
/// ControlRef`.
fn ctrl_id_from_inline_extra_bytes(extra: &[u8; 14]) -> u32 {
    u32::from_be_bytes([extra[3], extra[2], extra[1], extra[0]])
}

// ---------------------------------------------------------------------------
// Hwp5ParaText / TextSegment
// ---------------------------------------------------------------------------

/// A logical segment extracted from a `ParaText` (tag `0x43`) record.
///
/// HWP5 paragraph text is stored as a flat UTF-16LE stream where certain
/// code-point values carry special meaning. This enum represents one
/// decoded segment of that stream.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TextSegment {
    /// Normal Unicode text content.
    Text(String),
    /// Horizontal tab character (U+0009).
    ///
    /// The 7 u16 (14 bytes) of inline payload carry the tab's
    /// `width` (HwpUnit, u32 LE), `leader` (u8), and `tab_type` (u8)
    /// followed by 8 reserved bytes. Core's inline text model cannot
    /// currently carry these per-tab attributes, so the projection
    /// stage records the payload here and emits a warning when any of
    /// them are non-zero (see Bug A in
    /// `.docs/research/2026-05-26_tab_fidelity_bugs.md`).
    Tab {
        /// Fourteen bytes of inline tab metadata. All-zero for the
        /// "default" tab — non-zero values are silently lost on emit
        /// until the Core inline-tab carry slice (Phase 2 in the issue
        /// doc) lands.
        extra: [u8; 14],
    },
    /// Soft line break (U+000A).
    LineBreak,
    /// Drawing/table/control object embedded in the text stream (U+000B).
    /// The 14 bytes that follow in the stream are opaque metadata (7 u16 values).
    ControlRef {
        /// Fourteen bytes of opaque object metadata.
        extra: [u8; 14],
    },
    /// Extended control reference (U+000C).
    /// Semantics mirror [`TextSegment::ControlRef`].
    ExtendedControlRef {
        /// Fourteen bytes of opaque object metadata.
        extra: [u8; 14],
    },
    /// Paragraph end / break marker (U+000D).
    ParaBreak,
    /// Section or column definition boundary (U+0002).
    SectionColumnDef {
        /// Fourteen bytes of opaque section metadata (7 u16 values).
        extra: [u8; 14],
    },
    /// Field begin marker (U+0003).
    FieldBegin {
        /// Fourteen bytes of opaque field metadata (7 u16 values).
        extra: [u8; 14],
    },
    // (insert order: FieldBegin/FieldEnd remain stable for downstream pattern
    // matches; do not reorder TextSegment variants without an audit of the
    // decoder/projection match arms.)
    /// Field end marker (U+0004).
    ///
    /// HWP5 stores this as an inline control: one control code unit plus
    /// seven extra UTF-16 code units of payload. The payload is currently
    /// consumed and discarded because Core does not model field-end inline
    /// metadata yet.
    FieldEnd,
    /// Non-breaking space (HWP5 control code `0x18`, emitted as
    /// `<hp:nbSpace/>` in HWPX).
    NonBreakingSpace,
    /// Fixed-width space (HWP5 control code `0x1F`, emitted as
    /// `<hp:fwSpace/>` in HWPX).
    FwSpace,
}

/// Parsed from a `ParaText` (tag `0x43`) record in a BodyText section.
///
/// Contains the decoded text of one paragraph as a sequence of typed
/// [`TextSegment`] values. Each segment is either a run of normal Unicode
/// text or a single control code (tab, line break, object reference, etc.).
#[derive(Debug, Clone)]
pub(crate) struct Hwp5ParaText {
    /// Decoded text segments in paragraph order.
    pub segments: Vec<TextSegment>,
}

impl Hwp5ParaText {
    /// Parse a `ParaText` record from its raw UTF-16LE payload bytes.
    ///
    /// The byte slice is interpreted as a sequence of little-endian `u16`
    /// code-point values. Control code-points trigger special segment types;
    /// everything else is accumulated into [`TextSegment::Text`] runs.
    ///
    /// # Errors
    ///
    /// Returns [`Hwp5Error::RecordParse`] if the data contains an odd number
    /// of bytes or a control character that is followed by insufficient extra
    /// data. Returns [`Hwp5Error::Encoding`] if a collected code-unit sequence
    /// cannot be decoded as UTF-16.
    pub(crate) fn parse(data: &[u8]) -> Hwp5Result<Self> {
        if !data.len().is_multiple_of(2) {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!("ParaText data has odd byte count: {}", data.len()),
            });
        }

        // Convert bytes to u16 code units.
        let code_units: Vec<u16> =
            data.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();

        let mut segments: Vec<TextSegment> = Vec::new();
        let mut text_buf: Vec<u16> = Vec::new();
        let mut i = 0usize;

        // Helper: flush accumulated text buffer as a Text segment.
        macro_rules! flush_text {
            () => {
                if !text_buf.is_empty() {
                    let s = String::from_utf16(&text_buf).map_err(|_| Hwp5Error::Encoding {
                        detail: "invalid UTF-16 sequence in ParaText".into(),
                    })?;
                    segments.push(TextSegment::Text(s));
                    text_buf.clear();
                }
            };
        }

        // Helper: read 7 more u16 values (14 bytes) as extra data.
        //
        // HWP5 "inline" and "extended" control characters occupy 8 UTF-16
        // code units total: the control code itself plus 7 extra u16 values.
        macro_rules! read_extra {
            ($offset:expr) => {{
                if i + 7 > code_units.len() {
                    return Err(Hwp5Error::RecordParse {
                        offset: $offset * 2,
                        detail: format!(
                            "ParaText control char at position {} requires 7 more code units but only {} remain",
                            $offset, code_units.len() - i
                        ),
                    });
                }
                let mut extra = [0u8; 14];
                for k in 0..7usize {
                    let le = code_units[i + k].to_le_bytes();
                    extra[k * 2] = le[0];
                    extra[k * 2 + 1] = le[1];
                }
                i += 7;
                extra
            }};
        }

        while i < code_units.len() {
            let cp = code_units[i];
            i += 1;

            match cp {
                // Reserved single-wchar control.
                0x00 => {}

                // Extended controls: 8 wchars total (1 control + 7 extra u16).
                // 0x01 = reserved extended control.
                0x01 => {
                    flush_text!();
                    let _extra = read_extra!(i - 1);
                    // No segment emitted — consumed silently.
                }
                0x02 => {
                    flush_text!();
                    let extra = read_extra!(i - 1);
                    segments.push(TextSegment::SectionColumnDef { extra });
                }
                0x03 => {
                    flush_text!();
                    let extra = read_extra!(i - 1);
                    segments.push(TextSegment::FieldBegin { extra });
                }
                0x0B => {
                    flush_text!();
                    let extra = read_extra!(i - 1);
                    segments.push(TextSegment::ControlRef { extra });
                }
                0x0C => {
                    flush_text!();
                    let extra = read_extra!(i - 1);
                    segments.push(TextSegment::ExtendedControlRef { extra });
                }
                0x13 | 0x14 => {
                    flush_text!();
                    let _extra = read_extra!(i - 1);
                    // Unsupported inline control — consumed silently.
                }
                // 0x17: dutmal (덧말) inline marker.
                // `extra[0..4]` carries the BE-ascii ctrl_id `tdut` for the
                // corresponding `Hwp5Control::Dutmal`; the projection layer
                // pops it from the paragraph's control queue the same way
                // it handles a `0x0B` ControlRef.
                0x17 => {
                    flush_text!();
                    let extra = read_extra!(i - 1);
                    segments.push(TextSegment::ControlRef { extra });
                }
                // 0x16: IndexMark (찾아보기) inline marker (Wave 12k).
                // `extra[0..4]` carries the LE-stored ctrl_id `idxm`.
                // Only `idxm`-tagged `0x16` markers are promoted to a
                // `ControlRef`; other 0x16 owners (unknown extended
                // controls) keep falling through to the silent-consume
                // arm below so we don't accidentally promote a control
                // family whose CtrlHeader we don't yet decode.
                0x16 => {
                    flush_text!();
                    let extra = read_extra!(i - 1);
                    if ctrl_id_from_inline_extra_bytes(&extra) == CTRL_ID_INDEXMARK {
                        segments.push(TextSegment::ControlRef { extra });
                    }
                    // Else: consumed silently, same as 0x0E..=0x15 below.
                }
                // 0x12: extended control — Wave 12n discovered that
                // `atno` inline page-number markers ride this control
                // code (extra[0..4] = LE-stored `atno`). Promote only
                // `atno`-tagged 0x12 to a `ControlRef`; other 0x12 owners
                // keep falling through to silent-consume to avoid
                // accidentally promoting an unknown control family.
                0x12 => {
                    flush_text!();
                    let extra = read_extra!(i - 1);
                    if ctrl_id_from_inline_extra_bytes(&extra) == CTRL_ID_ATNO {
                        segments.push(TextSegment::ControlRef { extra });
                    }
                    // Else: consumed silently, same as 0x0E..=0x11 / 0x13..=0x15 below.
                }
                // 0x0E-0x15 (except 0x12): extended controls (bookmarks,
                // change tracking, etc.). All consume 7 extra u16 values.
                // Still silently consumed until a future slice promotes
                // them to a typed variant.
                0x0E..=0x11 | 0x13..=0x15 => {
                    flush_text!();
                    let _extra = read_extra!(i - 1);
                    // No segment emitted — consumed silently.
                }

                // Inline controls: 8 wchars total (1 control + 7 extra u16).
                0x04 => {
                    flush_text!();
                    let _extra = read_extra!(i - 1);
                    segments.push(TextSegment::FieldEnd);
                }
                0x05..=0x07 => {
                    flush_text!();
                    let _extra = read_extra!(i - 1);
                    // Unsupported inline control — consumed silently.
                }
                0x08 => {
                    flush_text!();
                    let _extra = read_extra!(i - 1);
                    // Title mark is not modeled in Core IR yet.
                }
                0x09 => {
                    flush_text!();
                    let extra = read_extra!(i - 1);
                    segments.push(TextSegment::Tab { extra });
                }

                // Single-wchar control chars.
                0x0A => {
                    flush_text!();
                    segments.push(TextSegment::LineBreak);
                }
                0x0D => {
                    flush_text!();
                    segments.push(TextSegment::ParaBreak);
                }
                // Per HWP 5.0 spec (rev 1.3) and the openhwp reference
                // (`paragraph.rs::to_plain_text`):
                //   0x18 → non-breaking space (`<hp:nbSpace/>`)
                //   0x1E → non-breaking / hard hyphen (not modeled in Core yet)
                //   0x1F → fixed-width space (`<hp:fwSpace/>`)
                0x18 => {
                    flush_text!();
                    segments.push(TextSegment::NonBreakingSpace);
                }
                0x1F => {
                    flush_text!();
                    segments.push(TextSegment::FwSpace);
                }
                0x1E => {
                    flush_text!();
                    // Hard hyphen — not modeled in Core yet; consumed silently.
                }

                // Everything else: normal character.
                _ => {
                    text_buf.push(cp);
                }
            }
        }

        // Flush any trailing text.
        flush_text!();

        Ok(Self { segments })
    }
}

// ---------------------------------------------------------------------------
// Hwp5CharShapeRun
// ---------------------------------------------------------------------------

/// A single character-shape run entry from a `ParaCharShape` (tag `0x44`) record.
///
/// Each run says: "from character position `position` onward, use char-shape
/// `char_shape_id`." Runs are listed in ascending position order and cover the
/// paragraph up to the next run's position (or to the end).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hwp5CharShapeRun {
    /// Starting character position within the paragraph (UTF-16 code units from
    /// the paragraph start).
    pub position: u32,
    /// Index into the DocInfo `CharShape` table.
    pub char_shape_id: u32,
}

impl Hwp5CharShapeRun {
    /// Byte size of a single run entry.
    const RUN_SIZE: usize = 8;

    /// Parse all `CharShapeRun` entries from a `ParaCharShape` record payload.
    ///
    /// The payload is a tightly packed array of 8-byte entries
    /// `(position: u32, char_shape_id: u32)`. An empty payload yields an empty
    /// `Vec`.
    ///
    /// # Errors
    ///
    /// Returns [`Hwp5Error::RecordParse`] if `data.len()` is not a multiple of 8.
    pub(crate) fn parse_all(data: &[u8]) -> Hwp5Result<Vec<Self>> {
        if !data.len().is_multiple_of(Self::RUN_SIZE) {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ParaCharShape data length {} is not a multiple of {}",
                    data.len(),
                    Self::RUN_SIZE
                ),
            });
        }
        let count = data.len() / Self::RUN_SIZE;
        let mut cur = Cursor::new(data);
        let mut runs = Vec::with_capacity(count);
        for _ in 0..count {
            let position = cur.read_u32::<LittleEndian>()?;
            let char_shape_id = cur.read_u32::<LittleEndian>()?;
            runs.push(Self { position, char_shape_id });
        }
        Ok(runs)
    }
}

// ---------------------------------------------------------------------------
// Hwp5ParaLineSeg
// ---------------------------------------------------------------------------

/// A single line layout segment from a `ParaLineSeg` (tag `0x45`) record.
///
/// This is format-local layout cache data. It must not leak into Core, but
/// HWP5 → HWPX conversion can preserve it as a fidelity hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hwp5ParaLineSeg {
    /// Character position where the line starts.
    pub text_start_position: u32,
    /// Vertical offset from the paragraph top in HWPUNIT.
    pub vertical_position: i32,
    /// Full line box height in HWPUNIT.
    pub line_height: i32,
    /// Text glyph box height in HWPUNIT.
    pub text_height: i32,
    /// Baseline position in HWPUNIT.
    pub baseline_distance: i32,
    /// Line spacing in HWPUNIT.
    pub line_spacing: i32,
    /// Horizontal start offset in HWPUNIT.
    pub column_start_position: i32,
    /// Available horizontal width in HWPUNIT.
    pub segment_width: i32,
    /// Layout flags bitfield.
    pub tag: u32,
}

impl Hwp5ParaLineSeg {
    /// Byte size of a single line segment entry.
    const SEGMENT_SIZE: usize = 36;

    /// Parse all line segment entries from a `ParaLineSeg` record payload.
    pub(crate) fn parse_all(data: &[u8]) -> Hwp5Result<Vec<Self>> {
        if !data.len().is_multiple_of(Self::SEGMENT_SIZE) {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ParaLineSeg data length {} is not a multiple of {}",
                    data.len(),
                    Self::SEGMENT_SIZE
                ),
            });
        }

        let count = data.len() / Self::SEGMENT_SIZE;
        let mut cur = Cursor::new(data);
        let mut segments = Vec::with_capacity(count);
        for _ in 0..count {
            segments.push(Self {
                text_start_position: cur.read_u32::<LittleEndian>()?,
                vertical_position: cur.read_i32::<LittleEndian>()?,
                line_height: cur.read_i32::<LittleEndian>()?,
                text_height: cur.read_i32::<LittleEndian>()?,
                baseline_distance: cur.read_i32::<LittleEndian>()?,
                line_spacing: cur.read_i32::<LittleEndian>()?,
                column_start_position: cur.read_i32::<LittleEndian>()?,
                segment_width: cur.read_i32::<LittleEndian>()?,
                tag: cur.read_u32::<LittleEndian>()?,
            });
        }
        Ok(segments)
    }
}

/// Minimal common geometry recovered from a `gso ` common-control payload.
///
/// The signed offsets and size fields live inside the owning `CtrlHeader`
/// payload immediately after the 4-byte `ctrl_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hwp5ShapeComponentGeometry {
    /// Horizontal offset in signed HWPUNIT.
    pub x: i32,
    /// Vertical offset in signed HWPUNIT.
    pub y: i32,
    /// Object width in HWPUNIT.
    pub width: u32,
    /// Object height in HWPUNIT.
    pub height: u32,
}

impl Hwp5ShapeComponentGeometry {
    /// Minimum `CtrlHeader` payload size needed to recover common geometry.
    const MIN_CTRL_HEADER_SIZE: usize = 24;

    /// Parse common geometry from a `gso ` / `tbl ` `CtrlHeader` payload.
    pub(crate) fn parse_from_ctrl_header(data: &[u8]) -> Hwp5Result<Self> {
        if data.len() < Self::MIN_CTRL_HEADER_SIZE {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "common control geometry too short: {} bytes (expected >= {})",
                    data.len(),
                    Self::MIN_CTRL_HEADER_SIZE
                ),
            });
        }

        let mut cur = Cursor::new(&data[8..24]);
        let y = cur.read_i32::<LittleEndian>()?;
        let x = cur.read_i32::<LittleEndian>()?;
        let width = cur.read_u32::<LittleEndian>()?;
        let height = cur.read_u32::<LittleEndian>()?;
        Ok(Self { x, y, width, height })
    }
}

// ---------------------------------------------------------------------------
// Hwp5ShapePoint / Hwp5ShapeComponentLine / Hwp5ShapeComponentPolygon
// ---------------------------------------------------------------------------

/// Minimal point used by non-image GSO shape components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hwp5ShapePoint {
    /// Horizontal coordinate in HWPUNIT.
    pub x: i32,
    /// Vertical coordinate in HWPUNIT.
    pub y: i32,
}

/// Minimal `ShapeComponentLine` payload needed to emit a visible line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hwp5ShapeComponentLine {
    /// Line start point in local object coordinates.
    pub start: Hwp5ShapePoint,
    /// Line end point in local object coordinates.
    pub end: Hwp5ShapePoint,
}

impl Hwp5ShapeComponentLine {
    /// Minimum payload size required to recover the two endpoints.
    const MIN_SIZE: usize = 16;

    /// Parse the stable line endpoint prefix from a `ShapeComponentLine` payload.
    pub(crate) fn parse(data: &[u8]) -> Hwp5Result<Self> {
        if data.len() < Self::MIN_SIZE {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ShapeComponentLine too short: {} bytes (expected >= {})",
                    data.len(),
                    Self::MIN_SIZE
                ),
            });
        }

        let mut cur = Cursor::new(data);
        let start_x = cur.read_i32::<LittleEndian>()?;
        let start_y = cur.read_i32::<LittleEndian>()?;
        let end_x = cur.read_i32::<LittleEndian>()?;
        let end_y = cur.read_i32::<LittleEndian>()?;
        Ok(Self {
            start: Hwp5ShapePoint { x: start_x, y: start_y },
            end: Hwp5ShapePoint { x: end_x, y: end_y },
        })
    }
}

/// Minimal `ShapeComponentPolygon` payload needed to emit a visible polygon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hwp5ShapeComponentPolygon {
    /// Ordered polygon vertices in local object coordinates.
    pub points: Vec<Hwp5ShapePoint>,
}

impl Hwp5ShapeComponentPolygon {
    /// Minimum payload size required to recover the point count.
    const MIN_SIZE: usize = 4;
    /// Serialized size of one point pair.
    const POINT_SIZE: usize = 8;

    /// Parse the stable polygon point list from a `ShapeComponentPolygon` payload.
    pub(crate) fn parse(data: &[u8]) -> Hwp5Result<Self> {
        if data.len() < Self::MIN_SIZE {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ShapeComponentPolygon too short: {} bytes (expected >= {})",
                    data.len(),
                    Self::MIN_SIZE
                ),
            });
        }

        let mut cur = Cursor::new(data);
        let point_count_u32 = cur.read_u32::<LittleEndian>()?;
        let point_count: usize =
            usize::try_from(point_count_u32).map_err(|_| Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ShapeComponentPolygon point count does not fit usize: {point_count_u32}"
                ),
            })?;
        let required_size = Self::MIN_SIZE
            .checked_add(point_count.checked_mul(Self::POINT_SIZE).ok_or_else(|| {
                Hwp5Error::RecordParse {
                    offset: 0,
                    detail: format!(
                        "ShapeComponentPolygon point count overflows payload size: {point_count_u32}"
                    ),
                }
            })?)
            .ok_or_else(|| Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ShapeComponentPolygon payload size overflows for point count: {point_count_u32}"
                ),
            })?;
        if data.len() < required_size {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ShapeComponentPolygon too short for {} points: {} bytes (expected >= {})",
                    point_count_u32,
                    data.len(),
                    required_size
                ),
            });
        }

        let mut points = Vec::with_capacity(point_count);
        for _ in 0..point_count {
            let x = cur.read_i32::<LittleEndian>()?;
            let y = cur.read_i32::<LittleEndian>()?;
            points.push(Hwp5ShapePoint { x, y });
        }
        Ok(Self { points })
    }
}

/// Minimal `ShapeComponentEllipse` (tag `0x50`) payload.
///
/// 한컴 stores **both** plain ellipses and arcs in this 60-byte record — it does
/// not emit a separate `ShapeComponentArc` (`0x51`) for arcs (empirically
/// confirmed from 한컴 output). The discriminator is content: a plain ellipse
/// leaves `property` at zero and all four arc endpoints at the origin, while an
/// arc carries a non-zero `property` and real arc endpoints.
///
/// Layout (15 little-endian words; the first read as `u32`, the rest as `i32`):
///
/// ```text
/// property | center(x,y) | axis1(x,y) | axis2(x,y)
///          | start1(x,y) | end1(x,y) | start2(x,y) | end2(x,y)
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hwp5ShapeComponentEllipse {
    /// Raw property bitfield (`0` for a plain ellipse; non-zero marks an arc).
    pub property: u32,
    /// Ellipse center in local object coordinates.
    pub center: Hwp5ShapePoint,
    /// First-axis reference point.
    pub axis1: Hwp5ShapePoint,
    /// Second-axis reference point.
    pub axis2: Hwp5ShapePoint,
    /// Arc start point 1 (origin for a plain ellipse).
    pub start1: Hwp5ShapePoint,
    /// Arc end point 1 (origin for a plain ellipse).
    pub end1: Hwp5ShapePoint,
    /// Arc start point 2 (origin for a plain ellipse).
    pub start2: Hwp5ShapePoint,
    /// Arc end point 2 (origin for a plain ellipse).
    pub end2: Hwp5ShapePoint,
}

impl Hwp5ShapeComponentEllipse {
    /// Exact payload size: `property` (4) + 7 point pairs (56) = 60 bytes.
    const MIN_SIZE: usize = 60;

    /// Parse the ellipse/arc geometry from a `ShapeComponentEllipse` payload.
    pub(crate) fn parse(data: &[u8]) -> Hwp5Result<Self> {
        if data.len() < Self::MIN_SIZE {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ShapeComponentEllipse too short: {} bytes (expected >= {})",
                    data.len(),
                    Self::MIN_SIZE
                ),
            });
        }

        let mut cur = Cursor::new(data);
        let property = cur.read_u32::<LittleEndian>()?;
        let center = read_point(&mut cur)?;
        let axis1 = read_point(&mut cur)?;
        let axis2 = read_point(&mut cur)?;
        let start1 = read_point(&mut cur)?;
        let end1 = read_point(&mut cur)?;
        let start2 = read_point(&mut cur)?;
        let end2 = read_point(&mut cur)?;
        Ok(Self { property, center, axis1, axis2, start1, end1, start2, end2 })
    }

    /// Whether this record describes an arc rather than a plain ellipse.
    ///
    /// True when the property bitfield is set or any arc endpoint is non-origin.
    /// This content-based check is robust against unknown `property` bits.
    pub(crate) fn is_arc(&self) -> bool {
        let arc_points_present =
            [self.start1, self.end1, self.start2, self.end2].iter().any(|p| p.x != 0 || p.y != 0);
        self.property != 0 || arc_points_present
    }
}

/// Minimal `ShapeComponentCurve` (tag `0x53`) payload.
///
/// Layout: `count` (`u32`) followed by `count` × (`i32` x, `i32` y) control
/// points, then `count - 1` `UINT8` segment-type bytes (`0` = straight line,
/// `1` = curve) plus trailing reserved bytes we ignore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hwp5ShapeComponentCurve {
    /// Ordered curve control points in local object coordinates.
    pub points: Vec<Hwp5ShapePoint>,
    /// Per-segment type bytes (`0` = line, `1` = curve); one per point gap.
    pub segment_types: Vec<u8>,
}

impl Hwp5ShapeComponentCurve {
    /// Minimum payload size required to recover the point count.
    const MIN_SIZE: usize = 4;
    /// Serialized size of one point pair.
    const POINT_SIZE: usize = 8;

    /// Parse the point list and segment types from a `ShapeComponentCurve` payload.
    pub(crate) fn parse(data: &[u8]) -> Hwp5Result<Self> {
        if data.len() < Self::MIN_SIZE {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ShapeComponentCurve too short: {} bytes (expected >= {})",
                    data.len(),
                    Self::MIN_SIZE
                ),
            });
        }

        let mut cur = Cursor::new(data);
        let point_count_u32 = cur.read_u32::<LittleEndian>()?;
        let point_count: usize =
            usize::try_from(point_count_u32).map_err(|_| Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ShapeComponentCurve point count does not fit usize: {point_count_u32}"
                ),
            })?;
        let points_size =
            point_count.checked_mul(Self::POINT_SIZE).ok_or_else(|| Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ShapeComponentCurve point count overflows payload size: {point_count_u32}"
                ),
            })?;
        let required_size =
            Self::MIN_SIZE.checked_add(points_size).ok_or_else(|| Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ShapeComponentCurve payload size overflows for point count: {point_count_u32}"
                ),
            })?;
        if data.len() < required_size {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ShapeComponentCurve too short for {} points: {} bytes (expected >= {})",
                    point_count_u32,
                    data.len(),
                    required_size
                ),
            });
        }

        let mut points = Vec::with_capacity(point_count);
        for _ in 0..point_count {
            points.push(read_point(&mut cur)?);
        }
        // Segment types: one per gap between points; best-effort if truncated.
        let segment_count = point_count.saturating_sub(1);
        let mut segment_types = Vec::with_capacity(segment_count);
        for _ in 0..segment_count {
            match cur.read_u8() {
                Ok(byte) => segment_types.push(byte),
                Err(_) => break,
            }
        }
        Ok(Self { points, segment_types })
    }
}

/// Read one little-endian `(i32 x, i32 y)` point pair from a cursor.
fn read_point(cur: &mut Cursor<&[u8]>) -> Hwp5Result<Hwp5ShapePoint> {
    let x = cur.read_i32::<LittleEndian>()?;
    let y = cur.read_i32::<LittleEndian>()?;
    Ok(Hwp5ShapePoint { x, y })
}

/// Minimal `HWPTAG_EQEDIT` (`0x58`) payload: the equation script.
///
/// Layout (confirmed from 한컴 output): `UINT32` property, then the script as a
/// `UINT16` WCHAR-count length prefix followed by that many little-endian
/// UTF-16 code units. Trailing fields (version string, font name) are ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hwp5EqEdit {
    /// HancomEQN script text, e.g. `"{a + b} over {c + d}"`.
    pub script: String,
}

// ---------------------------------------------------------------------------
// CtrlHeader command-string utilities
// ---------------------------------------------------------------------------

/// Decodes the UTF-16 BE command string embedded in a `%`-class CtrlHeader
/// payload (`%hlk` hyperlink, `%bmk` bookmark, `%xrf` cross-reference,
/// `%unk` memo, …).
///
/// HWP 5.0 spec §4.3.10.3 표 140 layout for the `extended-ctrl` family:
///
/// | byte range | meaning |
/// |-----------:|---------|
/// | `[0..4]`   | ctrl_id (little-endian u32 of BE-ascii name) |
/// | `[4..8]`   | properties (u32 LE) |
/// | `[8..10]`  | command char count (UINT16 **big-endian**) |
/// | `[10..]`   | UTF-16 **big-endian** code units (`char_count` of them) |
///
/// The BE encoding inside the LE wire is observed on 한컴-authored
/// fixtures; the format-spec wording is ambiguous so we keep one BE
/// fallback for record robustness. Returns `None` for payloads that fail
/// either length or UTF-16 decode.
pub(crate) fn parse_ctrl_header_command_string(header_data: &[u8]) -> Option<String> {
    if header_data.len() < 10 {
        return None;
    }
    // BE char_count + BE u16 chars (한컴 default).
    let be_char_len = u16::from_be_bytes([header_data[8], header_data[9]]) as usize;
    if let Some(s) = decode_command(header_data, be_char_len, /*be_chars=*/ true) {
        return Some(s);
    }
    // LE fallback — older or fuzzed payloads.
    let le_char_len = u16::from_le_bytes([header_data[8], header_data[9]]) as usize;
    decode_command(header_data, le_char_len, /*be_chars=*/ false)
}

fn decode_command(header_data: &[u8], char_count: usize, be_chars: bool) -> Option<String> {
    if char_count == 0 {
        return None;
    }
    let byte_len = char_count.checked_mul(2)?;
    let end = 10usize.checked_add(byte_len)?;
    if end > header_data.len() {
        return None;
    }
    let units: Vec<u16> = header_data[10..end]
        .chunks_exact(2)
        .map(|c| {
            if be_chars {
                u16::from_be_bytes([c[0], c[1]])
            } else {
                u16::from_le_bytes([c[0], c[1]])
            }
        })
        .collect();
    String::from_utf16(&units).ok()
}

/// Splits a slash-delimited HWP5 wire command into its fields and verifies
/// the leading element matches `expected_prefix` (e.g. `"MEMO"` for memos,
/// `"%hlk"` is *not* part of the slash payload — it's the ctrl_id).
///
/// Returns `None` if the prefix doesn't match. Callers index the remaining
/// fields positionally — the slash layout is part of HWP5 spec for each
/// command family.
pub(crate) fn split_slash_command<'a>(
    command: &'a str,
    expected_prefix: &str,
) -> Option<Vec<&'a str>> {
    let parts: Vec<&'a str> = command.split('/').collect();
    if parts.first().copied()? != expected_prefix {
        return None;
    }
    Some(parts)
}

/// Memo wire command parsed from a `%unk` CtrlHeader.
///
/// Format observed on 한컴-authored fixtures: `"MEMO/{shape_id}/{memo_id}/{hancom_inst_a}/{hancom_inst_b}/{author}/{terminator}"`.
///
/// `hancom_inst_a` and `hancom_inst_b` are 한컴-internal instance
/// identifiers that 한컴 re-derives on HWPX save (the HWPX
/// `<hp:fieldBegin id>` and `fieldid` attributes do *not* equal these); we
/// carry them so the round-tripped HWPX can mirror 한컴's Command
/// parameter verbatim.
///
/// `terminator` is the trailing slash segment (usually `"\;;"`); we keep it
/// for the same Command-parameter mirroring.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5MemoCommand {
    /// Raw command string ("MEMO/65535/1/.../.../hanyul/\;;"). Mirrored
    /// verbatim into the HWPX `Command` parameter.
    pub raw: String,
    /// Memo-shape table reference (slash[1]); 한컴 default is `65535`.
    pub shape_id: u32,
    /// Memo identifier (slash[2]); equals the `HWPTAG_MEMO_LIST` payload.
    pub memo_id: u32,
    /// 한컴-internal instance id A (slash[3]).
    ///
    /// Not consumed directly today — the HWPX encoder mirrors the whole
    /// `raw` command verbatim into the `Command` parameter, so these
    /// 한컴-internal ids ride along inside that string. Kept as
    /// dedicated fields so audit / round-trip code can read them without
    /// re-splitting `raw`.
    #[allow(dead_code)]
    pub hancom_inst_a: u32,
    /// 한컴-internal instance id B (slash[4]). See `hancom_inst_a`.
    #[allow(dead_code)]
    pub hancom_inst_b: u32,
    /// Author name (slash[5]).
    pub author: String,
    /// Trailing terminator segment (slash[6], typically `"\;;"`). Same
    /// rationale as `hancom_inst_a` — carried inside `raw` verbatim.
    #[allow(dead_code)]
    pub terminator: String,
}

// ---------------------------------------------------------------------------
// UTF-16 BSTR helpers (Wave 12i/12k shared — task #95)
// ---------------------------------------------------------------------------

/// Decodes a "split-leader" UTF-16 string: the first code unit lives in
/// the high half of the CtrlHeader's `properties` word (caller passes
/// it as `packed_first`), and `total_units - 1` further code units sit
/// at `data[start..]` as plain LE u16s.
///
/// Returns `(text, end_offset)`, where `end_offset` points just past
/// the tail bytes — callers chain that into the next field's offset.
///
/// `None` on:
/// - `total_units == 0` (caller-validated invariant; both Wave 12i/12k
///   parsers reject zero up-front)
/// - `total_units > max_units` (defence-in-depth allocation cap)
/// - tail truncation
/// - UTF-16 validation failure
///
/// Wire users (Wave 12i/12k):
/// - `Hwp5DutmalControl::parse` — `main_text`
/// - `Hwp5IndexMarkControl::parse` — `primary`
///
/// **Not used by Compose (Wave 12j)** — `Hwp5ComposeControl` has no
/// length prefix (text region size is inferred from `body_len -
/// FIXED_TRAILER`), so its packed-first-char path stays inline.
fn parse_split_leader_utf16(
    data: &[u8],
    start: usize,
    total_units: usize,
    packed_first: u16,
    max_units: usize,
) -> Option<(String, usize)> {
    if total_units == 0 || total_units > max_units {
        return None;
    }
    let tail_bytes = total_units.checked_sub(1)?.checked_mul(2)?;
    let tail_end = start.checked_add(tail_bytes)?;
    if data.len() < tail_end {
        return None;
    }
    let mut units: Vec<u16> = Vec::with_capacity(total_units);
    units.push(packed_first);
    for chunk in data[start..tail_end].chunks_exact(2) {
        units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }
    let text = String::from_utf16(&units).ok()?;
    Some((text, tail_end))
}

/// Decodes a "plain" length-prefixed UTF-16 BSTR:
/// `data[off..off+2]` carries the unit count `N` (LE u16), followed by
/// `2 * N` payload bytes.
///
/// Returns `(text, end_offset)`. An empty string is returned when
/// `N == 0` (no payload) — callers can wrap in `Option` if their wire
/// uses zero as a "no value" sentinel (e.g. IndexMark secondary).
///
/// `None` on length-prefix truncation, allocation cap overrun, payload
/// truncation, or UTF-16 validation failure.
///
/// Wire users (Wave 12i/12k):
/// - `Hwp5DutmalControl::parse` — `sub_text` (empty when wire `sub_len == 0`)
/// - `Hwp5IndexMarkControl::parse` — `secondary` (wrapper maps empty
///   to `None` per Hancom's "no secondary" semantic)
fn parse_length_prefixed_utf16(
    data: &[u8],
    off: usize,
    max_units: usize,
) -> Option<(String, usize)> {
    let len_end = off.checked_add(2)?;
    if data.len() < len_end {
        return None;
    }
    let units_len = u16::from_le_bytes([data[off], data[off + 1]]) as usize;
    if units_len > max_units {
        return None;
    }
    let body_bytes = units_len.checked_mul(2)?;
    let body_end = len_end.checked_add(body_bytes)?;
    if data.len() < body_end {
        return None;
    }
    let units: Vec<u16> =
        data[len_end..body_end].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let text = String::from_utf16(&units).ok()?;
    Some((text, body_end))
}

/// Dutmal (덧말) control parsed from a `tdut` CtrlHeader (`0x74647574`
/// BE-ascii).
///
/// Wire layout observed on 한컴-authored fixtures:
///
/// | offset | field | encoding |
/// |---:|---|---|
/// | `[4..6]` | `main_len` | LE u16 (number of chars in `main_text`) |
/// | `[6..8]` | `main_text[0]` | LE u16 (first char — wire packs it into the high half of the `properties` word) |
/// | `[8..8 + 2*(main_len-1)]` | `main_text[1..]` | LE UTF-16 |
/// | next 2 bytes | `sub_len` | LE u16 |
/// | next `2 * sub_len` bytes | `sub_text` | LE UTF-16 |
/// | tail `[0..4]` | `pos_type_raw` | LE u32 (0 = TOP, 1 = BOTTOM, …) |
/// | tail `[4..8]` | `sz_ratio` | LE u32 percent, 0 = auto (task #73) |
/// | tail `[8..12]` | `option_raw` | LE u32, mirrored verbatim |
/// | tail `[12..16]` | reserved | constant 0 on every observed fixture (styleIDRef candidate) |
/// | tail `[16..20]` | `align_raw` | LE u32: 1 = LEFT, 2 = RIGHT, 3 = CENTER (task #73) |
///
/// The first `main_text` char is folded into the same 32-bit word as the
/// length on the wire — packing them together saves a u16 over the more
/// natural "header (len) → body (chars)" layout. `sz_ratio` / `align_raw`
/// offsets were pinned by the task #73 one-knob-per-paragraph fixture
/// (`sample-dutmal-variants.hwp`; `probe_dutmal_tail` diff vs baseline:
/// szRatio=50/75 flips tail `[4]` to `0x32`/`0x4B`, align=LEFT/RIGHT
/// flips tail `[16]` to `01`/`02`). `styleIDRef` remains unattributed —
/// the fixture could not vary it (Core does not model it yet), so the
/// reserved word stays un-promoted per the "carry wire metadata only
/// when the source actually populates it" rule.
/// Defence-in-depth allocation cap for the `tdut` dutmal main/sub text
/// payloads (task #86 hardening). The dutmal feature is decorative
/// (위/아래 작은 글자) and realistic Korean strings rarely exceed
/// dozens of code units; 1024 leaves ample headroom while limiting a
/// single hostile record to ≤ 4 KB per slot.
const MAX_DUTMAL_TEXT_UNITS: usize = 1024;

#[derive(Debug, Clone)]
pub(crate) struct Hwp5DutmalControl {
    /// Owning control identifier, always `tdut` (`0x7464_7574` BE-ascii).
    #[allow(dead_code)]
    pub ctrl_id: u32,
    /// Visible body text (`<hp:mainText>`).
    pub main_text: String,
    /// Annotation text (`<hp:subText>`).
    pub sub_text: String,
    /// Raw `pos_type` word from the wire — meaning is mapped on the
    /// projection side (`0 = Top`, `1 = Bottom`, others reserved).
    pub pos_type_raw: u32,
    /// Annotation size as a percentage of the main text (tail `[4..8]`,
    /// task #73). `0` = auto (한컴 renders auto at ≈50%). Carried into
    /// HWPX `<hp:dutmal szRatio=…>`.
    pub sz_ratio: u32,
    /// Raw `option` word from the wire. Mirrored verbatim into the
    /// HWPX `<hp:dutmal option=…>` attribute — the precise meaning is
    /// not pinned down (see
    /// `.docs/algorithms/2026-06-01_dutmal_carry.md`), but mirroring it
    /// preserves fidelity round-trip without needing to know.
    pub option_raw: u32,
    /// Raw `align` word from the wire (tail `[16..20]`, task #73):
    /// `1` = LEFT, `2` = RIGHT, `3` = CENTER. Mapped to a typed
    /// `DutmalAlign` on the projection side; unknown codes fall back
    /// to CENTER with a `ProjectionFallback` warning. Truncated tails
    /// default to the CENTER wire code so legacy/minimal payloads stay
    /// warning-free.
    pub align_raw: u32,
}

/// Wire code for dutmal CENTER alignment (tail `[16..20]` — note the
/// wire treats `3`, not `0`, as the default/center value; task #73).
pub(crate) const DUTMAL_ALIGN_WIRE_CENTER: u32 = 3;

impl Hwp5DutmalControl {
    /// Decodes a `tdut` CtrlHeader payload into a `Hwp5DutmalControl`.
    /// Returns `None` on malformed or truncated payloads — the decoder
    /// falls back to `Hwp5Control::Unknown` so the rest of the section
    /// keeps round-tripping.
    pub(crate) fn parse(ctrl_id: u32, data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        // `main_text` is a split-leader BSTR: `properties.low` (data[4..6])
        // carries the unit count; `properties.high` (data[6..8]) carries
        // text[0]; data[8..] carries text[1..]. Defence-in-depth cap
        // (task #86) is `MAX_DUTMAL_TEXT_UNITS`. Helper sits at the top
        // of this file (task #95 — shared with IndexMark `primary`).
        let main_len = u16::from_le_bytes([data[4], data[5]]) as usize;
        let main_first = u16::from_le_bytes([data[6], data[7]]);
        let (main_text, main_tail_end) =
            parse_split_leader_utf16(data, 8, main_len, main_first, MAX_DUTMAL_TEXT_UNITS)?;

        // `sub_text` is a plain length-prefixed BSTR (task #86 cap shared
        // with main). Empty wire (`sub_len == 0`) yields `String::new()`.
        let (sub_text, sub_end) =
            parse_length_prefixed_utf16(data, main_tail_end, MAX_DUTMAL_TEXT_UNITS)?;

        // Tail words (see the struct doc offset table; task #73 pinned
        // sz_ratio/align via the variants fixture). Missing or truncated
        // tails fall back to the per-field default so legacy/minimal
        // payloads still carry their body text.
        let tail_word = |off: usize, default: u32| -> u32 {
            data.get(off..off + 4)
                .and_then(|slice| slice.try_into().ok())
                .map(u32::from_le_bytes)
                .unwrap_or(default)
        };
        let pos_type_raw = tail_word(sub_end, 0);
        let sz_ratio = tail_word(sub_end + 4, 0);
        let option_raw = tail_word(sub_end + 8, 0);
        let align_raw = tail_word(sub_end + 16, DUTMAL_ALIGN_WIRE_CENTER);

        Some(Self { ctrl_id, main_text, sub_text, pos_type_raw, sz_ratio, option_raw, align_raw })
    }
}

/// Compose (글자겹침) control parsed from a `tcps` CtrlHeader
/// (`0x74637073` BE-ascii).
///
/// Wire layout observed on a 한컴-authored fixture
/// (`sample-compose-basic` — composeText = `"한韓"`):
///
/// | offset | bytes | field | encoding |
/// |---:|---|---|---|
/// | `[0..N]` | `5C D5 D3 97` | `compose_text` | `N/2` LE UTF-16 code units |
/// | `[N..N+1]` | `01` | `circle_type_raw` | u8 — `0=CHAR`, `1=SHAPE_CIRCLE`, … (OWPML `SHAPECIRCLETYPE`) |
/// | `[N+1..N+2]` | `FD` | `char_sz` | i8 — `-3` in the fixture |
/// | `[N+2..N+3]` | `00` | `compose_type_raw` | u8 — `0=SPREAD`, `1=OVERLAP` (OWPML `COMPOSETYPE`) |
/// | `[N+3..N+4]` | `0A` | `char_pr_cnt` | u8 — fixed `10` per HWPX schema |
/// | `[N+4..N+4+40]` | `07 00 00 00 …` | `char_pr_ids[0..10]` | 10 × LE u32 (`u32::MAX` = no-override sentinel) |
///
/// `compose_text` has no length prefix on the wire. We infer
/// `N = payload_len - 44` (4 metadata bytes + 40 charPr bytes) given the
/// `charPrCnt = 10` invariant. If the result is negative, odd, or
/// `char_pr_cnt` is not exactly `10`, the payload is treated as
/// malformed and the decoder falls back to `Hwp5Control::Unknown`.
///
/// `circle_type` and `compose_type` are kept as raw `u8` values so the
/// projection layer can map them to the OWPML enum strings — same
/// strategy `Hwp5DutmalControl` uses for `pos_type_raw`.
///
/// See `.docs/algorithms/2026-06-01_compose_carry.md` for the longer
/// rationale (including why the layout is inferred from total payload
/// size rather than a length prefix, and which fields we deliberately
/// do not promote to typed enums yet).
#[derive(Debug, Clone)]
pub(crate) struct Hwp5ComposeControl {
    /// Owning control identifier, always `tcps` (`0x7463_7073` BE-ascii).
    #[allow(dead_code)]
    pub ctrl_id: u32,
    /// Overlaid characters (`<hp:compose composeText="…">`).
    pub compose_text: String,
    /// Raw circleType byte (mapped to OWPML enum on the projection side).
    pub circle_type_raw: u8,
    /// charSz adjustment as signed i8 (HWPX `<hp:compose charSz="…">`).
    pub char_sz: i8,
    /// Raw composeType byte (mapped to OWPML enum on the projection side).
    pub compose_type_raw: u8,
    /// 10 charPr `prIDRef` values (`u32::MAX` = no override).
    pub char_pr_ids: Vec<u32>,
}

impl Hwp5ComposeControl {
    /// HWPX schema fixes `charPrCnt` at 10; the wire is rejected when
    /// this byte holds anything else (see struct doc for full rationale).
    const CHAR_PR_CNT: usize = 10;
    /// Bytes after the inferred `compose_text` region: 4 metadata
    /// (circleType + charSz + composeType + charPrCnt) + 10 × u32.
    const FIXED_TRAILER: usize = 4 + Self::CHAR_PR_CNT * 4;

    /// Decodes a `tcps` CtrlHeader payload into a `Hwp5ComposeControl`.
    /// Returns `None` on malformed or truncated payloads — the decoder
    /// falls back to `Hwp5Control::Unknown` so the rest of the section
    /// keeps round-tripping.
    ///
    /// The wire has two observed layouts discriminated by the
    /// `properties` low half (`data[4..6]` as LE u16):
    ///
    /// - **`0x0003` (unpacked)** — `composeText` is fully in the body
    ///   (`data[8..]`). `properties[2..4]` is a shape glyph (e.g.
    ///   `U+25EF` ◯ for `SHAPE_CIRCLE`) that the decoder ignores —
    ///   the actual `circleType` enum is in the body trailer. This is
    ///   what 한컴 emits natively and for almost every HWPX→HWP5
    ///   round-tripped variant.
    /// - **`0x0002` (packed)** — `composeText[0]` is in
    ///   `properties[2..4]` (LE u16), the rest in the body, and the
    ///   low half (`0x0002`) doubles as `composeText.len()`. Observed
    ///   exclusively on the `CHAR + OVERLAP` variant when 한컴 saved
    ///   an HWPX → HWP5 — presumably because `CHAR` has no decoration
    ///   glyph to put in `properties[2..4]`, so 한컴 packs the first
    ///   text char there instead. The body trailer layout is
    ///   unchanged; only the leading char-region shrinks by one
    ///   `u16`.
    ///
    /// Any other `properties.low` value falls through to `None`
    /// (treated as malformed) — clamp-style guessing risks silently
    /// inventing characters from unrelated bits.
    pub(crate) fn parse(ctrl_id: u32, data: &[u8]) -> Option<Self> {
        // CtrlHeader payload is `[0..8] = ctrl_id + properties`, then
        // the compose-specific data lives in `[8..]`. We mirror the
        // calling convention used by `Hwp5DutmalControl::parse`.
        if data.len() < 8 {
            return None;
        }
        let props_low = u16::from_le_bytes([data[4], data[5]]);
        let props_high = u16::from_le_bytes([data[6], data[7]]);
        let body = &data[8..];
        if body.len() < Self::FIXED_TRAILER {
            return None;
        }
        let text_bytes_end = body.len() - Self::FIXED_TRAILER;
        if !text_bytes_end.is_multiple_of(2) {
            return None;
        }

        // Layout discriminator: `0x0003` = unpacked (default), `0x0002`
        // = packed (one char carried in properties.high; len encoded
        // in properties.low). See struct doc for the empirical
        // rationale and which fixture surfaced the variant.
        let packed_first_char: Option<u16> = match props_low {
            0x0003 => None,
            0x0002 => Some(props_high),
            _ => return None,
        };

        let body_chars = text_bytes_end / 2;
        let total_chars = body_chars + packed_first_char.map_or(0, |_| 1);
        let mut compose_units: Vec<u16> = Vec::with_capacity(total_chars);
        if let Some(first) = packed_first_char {
            compose_units.push(first);
        }
        for chunk in body[..text_bytes_end].chunks_exact(2) {
            compose_units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        let compose_text = String::from_utf16(&compose_units).ok()?;

        let meta_off = text_bytes_end;
        let circle_type_raw = body[meta_off];
        let char_sz = body[meta_off + 1] as i8;
        let compose_type_raw = body[meta_off + 2];
        let char_pr_cnt = body[meta_off + 3] as usize;
        if char_pr_cnt != Self::CHAR_PR_CNT {
            // 한컴이 HWPX에서 항상 10을 emit한다는 invariant가 깨지면
            // 알려진 layout이 더 이상 안전하지 않으니 Unknown으로 후퇴.
            return None;
        }

        let mut char_pr_ids = Vec::with_capacity(Self::CHAR_PR_CNT);
        let charpr_off = meta_off + 4;
        for i in 0..Self::CHAR_PR_CNT {
            let off = charpr_off + i * 4;
            char_pr_ids.push(u32::from_le_bytes([
                body[off],
                body[off + 1],
                body[off + 2],
                body[off + 3],
            ]));
        }

        Some(Self {
            ctrl_id,
            compose_text,
            circle_type_raw,
            char_sz,
            compose_type_raw,
            char_pr_ids,
        })
    }
}

/// IndexMark (찾아보기 표시) control parsed from an `idxm`
/// CtrlHeader (`0x6964_786D` BE-ascii).
///
/// Wire layout observed across 10 한컴-authored entries (2 native +
/// 8 from an HWPX → HWP5 round-trip). The layout mirrors Wave 12i's
/// `Hwp5DutmalControl` — the first `primary` char is packed into
/// the `properties` word, then the rest of the body follows.
///
/// Word/offset table (`primary="컴퓨터"`, `secondary="하드웨어"`
/// shown as a worked example):
///
/// | word/offset | bytes | field | encoding |
/// |---|---|---|---|
/// | `properties[0..2]` | `03 00` | `primary_units_len` | LE u16 — UTF-16 code-unit count for primary |
/// | `properties[2..4]` | `F4 CE` | `primary[0]` | LE u16 (first char, folded into the high half of `properties`) |
/// | `payload[0..(primary_units_len-1)*2]` | `E8 D4 30 D1` | `primary[1..N]` | LE UTF-16 (may be empty when primary is one unit) |
/// | next 2 bytes | `04 00` | `secondary_units_len` | LE u16 — 0 means "no secondary" |
/// | next `2 * secondary_units_len` bytes | `58 D5 DC B4 E8 C6 B4 C5` | `secondary` | LE UTF-16 |
/// | trailing 4 bytes | `00 00 00 00` (round-trip) / `FF FF FF FF` (native) | trailer | observed but discarded |
///
/// `secondary_units_len == 0` is the only "no secondary" signal on
/// the wire; 한컴 normalizes a source `Some("")` to no secondary
/// when it saves an HWPX as HWP5. The decoder reflects that by
/// returning `Option<String>` and treating `0` as `None`.
///
/// The trailing 4 bytes are deliberately discarded — HWPX has no
/// corresponding `<hp:indexmark>` field and Wave 12k is HWP5 →
/// HWPX carry only. Their presence is still required (a truncated
/// trailer means we no longer know the record boundary). See
/// `.docs/algorithms/2026-06-02_indexmark_carry.md` for the
/// Codex-reviewed rationale.
///
/// Defence-in-depth allocation cap (task #86 hardening). Index keys
/// are short search strings; 1024 units accommodates the longest
/// realistic content while keeping a hostile record under 4 KB.
const MAX_INDEXMARK_KEY_UNITS: usize = 1024;

#[derive(Debug, Clone)]
pub(crate) struct Hwp5IndexMarkControl {
    /// Owning control identifier, always `idxm` (`0x6964_786D`
    /// BE-ascii).
    #[allow(dead_code)]
    pub ctrl_id: u32,
    /// Primary index key (`<hp:firstKey>`).
    pub primary: String,
    /// Secondary index key (`<hp:secondKey>`). `None` when the
    /// wire has `secondary_units_len == 0` — Hancom-saved HWP5
    /// cannot distinguish `Some("")` from `None`.
    pub secondary: Option<String>,
}

impl Hwp5IndexMarkControl {
    /// Decodes an `idxm` CtrlHeader payload into a
    /// `Hwp5IndexMarkControl`. Returns `None` on malformed or
    /// truncated payloads — the decoder converts that into a
    /// targeted `Hwp5Warning::DroppedControl { control: "indexmark", … }`
    /// rather than the generic `UnsupportedTag` so audit baselines
    /// can attribute the loss to the IndexMark codepath.
    pub(crate) fn parse(ctrl_id: u32, data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        // `primary` is a split-leader BSTR — same wire shape as Dutmal
        // `main_text`. `properties.low` (data[4..6]) is the UTF-16
        // code-unit count (not Unicode scalar count); the high half
        // (data[6..8]) carries `primary[0]`. Helper shared via task #95.
        let primary_units_len = usize::from(u16::from_le_bytes([data[4], data[5]]));
        let primary_first = u16::from_le_bytes([data[6], data[7]]);
        let (primary, primary_end) = parse_split_leader_utf16(
            data,
            8,
            primary_units_len,
            primary_first,
            MAX_INDEXMARK_KEY_UNITS,
        )?;

        // `secondary` is a plain length-prefixed BSTR. Hancom normalises
        // a source `Some("")` to wire `len == 0`, so we map an empty
        // returned string back to `None` (the only "no secondary"
        // signal on the wire). Allocation cap shared with `primary`.
        let (secondary_text, secondary_end) =
            parse_length_prefixed_utf16(data, primary_end, MAX_INDEXMARK_KEY_UNITS)?;
        // Require the trailing 4 bytes to be present — a truncated
        // record means the boundary is no longer trustworthy.
        let _trailer_end = secondary_end.checked_add(4)?;
        if data.len() < secondary_end + 4 {
            return None;
        }

        let secondary = if secondary_text.is_empty() { None } else { Some(secondary_text) };

        // Trailer u32 observed = 0x0000_0000 (round-trip) or
        // 0xFFFF_FFFF (native). HWPX has no field that carries it,
        // so it is intentionally discarded at the HWP5 boundary.
        Some(Self { ctrl_id, primary, secondary })
    }
}

// ---------------------------------------------------------------------------
// Hwp5ClickHereControl (Wave 12l)
// ---------------------------------------------------------------------------

/// HWP5 representation of the `%clk` (CLICK_HERE / 누름틀) press-field.
///
/// Wire layout (CtrlHeader, ctrl_id=`0x2563_6C6B`, properties=`0x0000_0001`):
///
/// | offset | bytes | field | encoding |
/// |---|---|---|---|
/// | `body[0]` | `09` | flag (`Prop` integer param value) | u8 constant |
/// | `body[1..3]` | LE u16 | command UTF-16 code unit count `N` | u16 LE |
/// | `body[3..3+2N]` | UTF-16LE | command string (`Clickhere:set:N:Direction:wstring:H:<hint> HelpState:wstring:E:<help>  `) | UTF-16LE |
/// | `body[3+2N..3+2N+4]` | LE u32 | field unique id (smithy-local — discarded by Core) | u32 LE |
/// | `body[3+2N+4..3+2N+8]` | `00 00 00 00` | trailer padding | u32 LE |
///
/// `name` (양식 모드 식별자) is **not** carried in the CtrlHeader payload;
/// it lives in the immediately following `0x57 lvl=2` sub-record parsed
/// by `Hwp5ClickHereControl::parse_name_subrecord` and merged at the
/// decoder boundary.
///
/// Command parser is length-driven (not delimiter split) so embedded
/// colons in hint/help do not break decoding. Lengths are UTF-16 code
/// unit counts — Codex review explicitly required this so surrogate
/// pairs (Wave 12k learning) decode correctly.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5ClickHereControl {
    /// Owning control identifier, always `0x2563_6C6B` (`"%clk"` BE-ascii).
    #[allow(dead_code)]
    pub ctrl_id: u32,
    /// Hint text shown as visible placeholder (`<hp:stringParam name="Direction">`).
    /// `None` when the wire encoded an empty hint (Hancom collapses
    /// `Some("")` and `None` to the same wire form).
    pub hint_text: Option<String>,
    /// Help text shown as tooltip (`<hp:stringParam name="HelpState">`).
    /// `None` when the wire encoded an empty help string.
    pub help_text: Option<String>,
    /// Form-mode identifier filled in by `merge_name_subrecord` after
    /// decoding the trailing `0x57 lvl=2` sub-record. Construction
    /// always starts as `None`.
    pub name: Option<String>,
    /// Field unique id pulled from the trailer u32; smithy-local fidelity
    /// only — Core never sees this value (the encoder reallocates ids).
    #[allow(dead_code)]
    pub field_unique_id: u32,
}

/// Defence-in-depth allocation cap for the `%clk` Command UTF-16
/// payload (Wave 12l security review MEDIUM). The largest observed
/// command across the five Wave 12l fixtures is 107 UTF-16 units; 32 K
/// is roughly two orders of magnitude over the realistic ceiling
/// while keeping a single hostile record under 64 KB. The cap is
/// applied **before** any `Vec::with_capacity`, so the decoder cannot
/// be coerced into allocating a 130 KB intermediate per malformed
/// record on top of the bytes already present in the input.
const MAX_CLICKHERE_COMMAND_UNITS: usize = 32 * 1024;

/// Same cap applied to the trailing `0x57` name sub-record. Realistic
/// form-field identifiers are short; 2 K UTF-16 units (≈ 4 KB) is
/// already absurd for an HTML-input-name-style identifier.
const MAX_CLICKHERE_NAME_UNITS: usize = 2 * 1024;

/// Why a `%clk` CtrlHeader payload was rejected by
/// [`Hwp5ClickHereControl::parse`] (task #89 — observability).
///
/// Pre-#89 every reject collapsed into a single `None`, so the decoder
/// could only emit one generic "malformed %clk" warning. Distinct
/// variants let `audit-hwp5` baselines attribute a dropped press-field
/// to a concrete wire defect instead of a catch-all bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClickHereParseError {
    /// Payload shorter than the 19-byte fixed minimum
    /// (ctrl_id + properties + flag + count + trailer).
    TruncatedHeader,
    /// Declared command length exceeds `MAX_CLICKHERE_COMMAND_UNITS`
    /// (allocation-cap guard, task #86).
    CommandTooLong,
    /// Declared command length + 8-byte trailer overruns the payload
    /// (including arithmetic overflow on hostile lengths).
    TruncatedCommand,
    /// The command decoded as UTF-16 but did not match the
    /// `Clickhere:set:N:…` grammar.
    CommandSyntax,
}

impl ClickHereParseError {
    /// Short static description for embedding in warning messages.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::TruncatedHeader => "payload shorter than fixed 19-byte minimum",
            Self::CommandTooLong => "command length exceeds allocation cap",
            Self::TruncatedCommand => "declared command length overruns payload",
            Self::CommandSyntax => "command does not match Clickhere:set grammar",
        }
    }
}

impl Hwp5ClickHereControl {
    /// Decodes a `%clk` CtrlHeader payload (the raw record `data`
    /// excluding the 4-byte CtrlHeader prefix the dispatcher already
    /// matched). Returns a [`ClickHereParseError`] naming the wire
    /// defect on truncation or malformed command string — the decoder
    /// embeds it in a targeted
    /// `Hwp5Warning::DroppedControl { control: "clickhere", … }` so
    /// the lossy path is auditable per-defect (task #89).
    ///
    /// `name` is left `None`; the dispatcher merges it from the
    /// following `0x57` sub-record once parsed.
    pub(crate) fn parse(ctrl_id: u32, data: &[u8]) -> Result<Self, ClickHereParseError> {
        // Need: 4 (ctrl_id) + 4 (properties) + 1 (flag) + 2 (count)
        // + 0 (command) + 8 (trailer) = 19 bytes minimum.
        if data.len() < 19 {
            return Err(ClickHereParseError::TruncatedHeader);
        }
        // Skip the 8-byte CtrlHeader prefix (ctrl_id + properties).
        let body = &data[8..];
        // Command char count at body[1..3]; body[0] is the 0x09 flag
        // (validated softly — codex review: warning on mismatch but
        // continue, so we accept any value here and trust the count).
        let command_units = usize::from(u16::from_le_bytes([body[1], body[2]]));
        if command_units > MAX_CLICKHERE_COMMAND_UNITS {
            return Err(ClickHereParseError::CommandTooLong);
        }
        let command_bytes =
            command_units.checked_mul(2).ok_or(ClickHereParseError::TruncatedCommand)?;
        let command_end =
            3usize.checked_add(command_bytes).ok_or(ClickHereParseError::TruncatedCommand)?;
        // Trailer is 8 bytes (u32 field id + u32 padding).
        let trailer_end =
            command_end.checked_add(8).ok_or(ClickHereParseError::TruncatedCommand)?;
        if body.len() < trailer_end {
            return Err(ClickHereParseError::TruncatedCommand);
        }

        let mut command_units_vec: Vec<u16> = Vec::with_capacity(command_units);
        for chunk in body[3..command_end].chunks_exact(2) {
            command_units_vec.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }

        let (hint_text, help_text) = parse_clickhere_command(&command_units_vec)
            .ok_or(ClickHereParseError::CommandSyntax)?;

        let field_unique_id = u32::from_le_bytes([
            body[command_end],
            body[command_end + 1],
            body[command_end + 2],
            body[command_end + 3],
        ]);
        // body[command_end+4..command_end+8] is the 4-byte zero pad —
        // intentionally not validated; Codex review (medium severity)
        // recommended warning-not-error on mismatch which the decoder
        // does at the dispatch layer.

        Ok(Self { ctrl_id, hint_text, help_text, name: None, field_unique_id })
    }

    /// Decodes a `0x57 lvl=2` sub-record's `data` into the field
    /// `name`. Returns [`ClickHereNameSubrecord::Malformed`] on
    /// truncation or impossible length; callers should treat that as
    /// "name not recoverable, keep the rest of the press-field"
    /// (codex review medium: don't drop the entire clickhere just
    /// because the name sub-record is bad).
    pub(crate) fn parse_name_subrecord(data: &[u8]) -> ClickHereNameSubrecord {
        // 12-byte constant header observed: 1B 02 01 00 00 00 00 40 01 00 LL LL
        // We tolerate constant-prefix mismatch (codex: "부분 strict"):
        // the only field we strictly need is the u16 LE name length at
        // [10..12]. Lengths above that bound the body.
        if data.len() < 12 {
            return ClickHereNameSubrecord::Malformed;
        }
        let name_units = usize::from(u16::from_le_bytes([data[10], data[11]]));
        if name_units > MAX_CLICKHERE_NAME_UNITS {
            return ClickHereNameSubrecord::Malformed;
        }
        let Some(body_bytes) = name_units.checked_mul(2) else {
            return ClickHereNameSubrecord::Malformed;
        };
        let Some(body_end) = 12usize.checked_add(body_bytes) else {
            return ClickHereNameSubrecord::Malformed;
        };
        if data.len() < body_end {
            return ClickHereNameSubrecord::Malformed;
        }
        if name_units == 0 {
            // Some("") and None are indistinguishable on the wire —
            // length 0 normalizes to Unnamed (never `Named("")`).
            return ClickHereNameSubrecord::Unnamed;
        }
        let mut units: Vec<u16> = Vec::with_capacity(name_units);
        for chunk in data[12..body_end].chunks_exact(2) {
            units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        match String::from_utf16(&units) {
            Ok(name) => ClickHereNameSubrecord::Named(name),
            Err(_) => ClickHereNameSubrecord::Malformed,
        }
    }
}

/// Outcome of parsing the `%clk` trailing `0x57 lvl=2` name sub-record
/// (task #88 — replaces the earlier `Option<Option<String>>` shape so
/// the three-way semantics carry their own names).
///
/// `Some("")` and `None` are wire-indistinguishable (length 0), so there
/// is no `Named("")` — an empty name normalizes to [`Self::Unnamed`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClickHereNameSubrecord {
    /// Structurally valid, carries a non-empty form-mode name.
    Named(String),
    /// Structurally valid, wire length 0 — 한컴 saved a nameless field.
    Unnamed,
    /// Truncated / impossible length / invalid UTF-16 — the caller
    /// warns and keeps the press-field with `name = None`
    /// (grace-degrade rather than drop, per the Wave 12l codex review).
    Malformed,
}

/// Parses the `Clickhere:set:N:Direction:wstring:H:<hint> HelpState:wstring:E:<help>  `
/// command string into `(hint, help)` Options. Returns `None` if the
/// structure cannot be matched.
///
/// Length-driven (not delimiter split) so embedded `:` characters in
/// hint/help do not break decoding. UTF-16 code-unit aware throughout —
/// surrogate-pair safe per the Codex review.
fn parse_clickhere_command(units: &[u16]) -> Option<(Option<String>, Option<String>)> {
    let mut cursor = 0usize;
    // Helper: try to match an ASCII literal at the current cursor.
    let match_literal = |units: &[u16], cursor: &mut usize, lit: &str| -> bool {
        let lit_units: Vec<u16> = lit.encode_utf16().collect();
        if cursor.checked_add(lit_units.len()).is_none_or(|end| end > units.len()) {
            return false;
        }
        if units[*cursor..*cursor + lit_units.len()] != lit_units[..] {
            return false;
        }
        *cursor += lit_units.len();
        true
    };
    // Helper: parse decimal digits up to a terminator ':'.
    let parse_decimal_until_colon = |units: &[u16], cursor: &mut usize| -> Option<usize> {
        let mut n: usize = 0;
        let mut consumed = 0usize;
        while *cursor < units.len() {
            let u = units[*cursor];
            if u == u16::from(b':') {
                *cursor += 1;
                if consumed == 0 {
                    return None;
                }
                return Some(n);
            }
            if !(u16::from(b'0')..=u16::from(b'9')).contains(&u) {
                return None;
            }
            n = n.checked_mul(10)?.checked_add(usize::from(u - u16::from(b'0')))?;
            consumed += 1;
            *cursor += 1;
        }
        None
    };

    if !match_literal(units, &mut cursor, "Clickhere:set:") {
        return None;
    }
    let _command_n = parse_decimal_until_colon(units, &mut cursor)?;
    if !match_literal(units, &mut cursor, "Direction:wstring:") {
        return None;
    }
    let hint_len = parse_decimal_until_colon(units, &mut cursor)?;
    if cursor.checked_add(hint_len).is_none_or(|end| end > units.len()) {
        return None;
    }
    let hint_units = &units[cursor..cursor + hint_len];
    let hint = String::from_utf16(hint_units).ok()?;
    cursor += hint_len;
    if !match_literal(units, &mut cursor, " HelpState:wstring:") {
        return None;
    }
    let help_len = parse_decimal_until_colon(units, &mut cursor)?;
    if cursor.checked_add(help_len).is_none_or(|end| end > units.len()) {
        return None;
    }
    let help_units = &units[cursor..cursor + help_len];
    let help = String::from_utf16(help_units).ok()?;

    Some((
        if hint.is_empty() { None } else { Some(hint) },
        if help.is_empty() { None } else { Some(help) },
    ))
}

// ---------------------------------------------------------------------------
// Hwp5SummeryControl (Wave 12n)
// ---------------------------------------------------------------------------

/// Defence-in-depth allocation cap for the `%smr` Command UTF-16 payload
/// (Wave 12n architect review). The longest observed SUMMERY token across
/// the Wave 12n native fixtures is 13 UTF-16 units (`$modifiedtime`). A
/// 1024-unit cap is roughly two orders of magnitude over the realistic
/// ceiling while keeping a single hostile record well under 4 KB.
const MAX_SUMMERY_COMMAND_UNITS: usize = 1024;

/// HWP5 representation of a `%smr` SUMMERY auto-field control.
///
/// Wire layout (CtrlHeader, ctrl_id=`0x2573_6D72`, observed properties
/// `01 00 00 00` on native fixtures):
///
/// | offset | bytes | field | encoding |
/// |---|---|---|---|
/// | `body[0]` | `08` | flag (`Prop` integer param value) | u8 constant |
/// | `body[1..3]` | LE u16 | command UTF-16 code unit count `N` | u16 LE |
/// | `body[3..3+2N]` | UTF-16LE | command string (e.g. `"$author"`, `"$modifiedtime"`) | UTF-16LE |
/// | `body[3+2N..3+2N+4]` | LE u32 | field unique id (smithy-local, discarded by Core) | u32 LE |
/// | `body[3+2N+4..3+2N+8]` | `00 00 00 00` | trailer padding | u32 LE |
///
/// Token meanings (Wave 12n native fixture analysis,
/// `.docs/research/2026-06-02_auto_field_wire_dump.md`):
/// - `$author` → 만든 사람 → `FieldType::Author`
/// - `$lastsaveby` → 마지막 저장한 사람 → `FieldType::LastSavedBy`
/// - `$createtime` → 만든 날짜 → `FieldType::CreatedTime`
/// - `$modifiedtime` → 마지막 저장한 날짜 → `FieldType::ModifiedTime`
/// - `$title` → 문서 제목 → `FieldType::Title`
/// - 기타 `$X` → `Control::UnknownSummery { token }` carry
#[derive(Debug, Clone)]
pub(crate) struct Hwp5SummeryControl {
    /// Owning control identifier, always `0x2573_6D72` (`"%smr"` BE-ascii).
    #[allow(dead_code)]
    pub ctrl_id: u32,
    /// Decoded Command token (e.g. `"$author"`). Forwarded verbatim to
    /// the projection layer for typed vs unknown dispatch.
    pub command_token: String,
    /// Field unique id pulled from the trailer u32; smithy-local fidelity
    /// only — Core never sees this value (the encoder reallocates ids).
    #[allow(dead_code)]
    pub field_unique_id: u32,
}

/// Defence-in-depth allocation cap for the `%dte` Command UTF-16 payload
/// (Wave 12n). The longest observed `%dte` Command across the Wave 12n
/// native fixtures is 17 code units (`"\:1년 2월 3일 (6);0;"`). A 1024-unit
/// cap matches `MAX_SUMMERY_COMMAND_UNITS` (same family of CtrlHeader).
const MAX_DATECODE_COMMAND_UNITS: usize = 1024;

/// HWP5 representation of a `%dte` date/time format-code field
/// (Wave 12n).
///
/// Wire envelope is identical to [`Hwp5SummeryControl`] / [`Hwp5ClickHereControl`]:
///
/// | offset | bytes | field |
/// |---|---|---|
/// | `body[0]` | `08` | flag |
/// | `body[1..3]` | LE u16 | command UTF-16 code unit count `N` |
/// | `body[3..3+2N]` | UTF-16LE | format-code Command (e.g. `"\:1년 2월 3일 (6);0;"`, `"T\:;0;"`) |
/// | `body[3+2N..3+2N+8]` | 8 bytes | trailer (`[instance_id u32, padding u32]`) |
///
/// Unlike SUMMERY, the Command is a raw format pattern — the grammar
/// (`\:` header, positional codes `1`/`2`/`3`/`6`, `;0;` options,
/// optional `T` prefix for time-only) is not parsed into structured
/// fields. The trailer is preserved verbatim so a future encoder can
/// round-trip the instance id if needed.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5DateCodeControl {
    /// Owning control identifier, always `0x2564_7465` (`"%dte"` BE-ascii).
    #[allow(dead_code)]
    pub ctrl_id: u32,
    /// Raw Command string as recovered from the wire.
    pub raw_command: String,
    /// 8-byte trailer carried verbatim for round-trip fidelity.
    pub raw_trailer: [u8; 8],
}

impl Hwp5DateCodeControl {
    /// Decodes a `%dte` CtrlHeader payload. Returns `None` on truncation
    /// or malformed UTF-16; the decoder converts that into a targeted
    /// `Hwp5Warning::DroppedControl { control: "date_code_field", … }`.
    pub(crate) fn parse(ctrl_id: u32, data: &[u8]) -> Option<Self> {
        if data.len() < 19 {
            return None;
        }
        let body = &data[8..];
        let command_units = usize::from(u16::from_le_bytes([body[1], body[2]]));
        if command_units > MAX_DATECODE_COMMAND_UNITS {
            return None;
        }
        let command_bytes = command_units.checked_mul(2)?;
        let command_end = 3usize.checked_add(command_bytes)?;
        let trailer_end = command_end.checked_add(8)?;
        if body.len() < trailer_end {
            return None;
        }
        let mut units: Vec<u16> = Vec::with_capacity(command_units);
        for chunk in body[3..command_end].chunks_exact(2) {
            units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        let raw_command = String::from_utf16(&units).ok()?;
        let mut raw_trailer = [0u8; 8];
        raw_trailer.copy_from_slice(&body[command_end..trailer_end]);
        Some(Self { ctrl_id, raw_command, raw_trailer })
    }
}

/// Defence-in-depth allocation cap for the `%pat` Command UTF-16
/// payload (Wave 12n). Observed Command strings are `$P`, `$F`, `$P$F` —
/// well under the cap. 256 units leaves ample headroom for future path
/// format codes without inviting allocator abuse.
const MAX_PATHFIELD_COMMAND_UNITS: usize = 256;

/// HWP5 representation of a `%pat` path / file-name field (Wave 12n).
///
/// Wire envelope is identical to [`Hwp5SummeryControl`] / [`Hwp5DateCodeControl`]:
/// the 1-byte flag + u16 LE command count + UTF-16LE Command + 8-byte
/// trailer. The Command is a path-format-code string (`$P` = path,
/// `$F` = file name, `$P$F` = both).
#[derive(Debug, Clone)]
pub(crate) struct Hwp5PathFieldControl {
    /// Owning control identifier, always `0x2570_6174` (`"%pat"` BE-ascii).
    #[allow(dead_code)]
    pub ctrl_id: u32,
    /// Raw Command string (`"$P"`, `"$F"`, or `"$P$F"`; unknown forms
    /// preserved verbatim).
    pub raw_command: String,
}

impl Hwp5PathFieldControl {
    /// Decodes a `%pat` CtrlHeader payload. Returns `None` on truncation
    /// or malformed UTF-16; the decoder emits a targeted
    /// `Hwp5Warning::DroppedControl { control: "path_field", … }`.
    pub(crate) fn parse(ctrl_id: u32, data: &[u8]) -> Option<Self> {
        if data.len() < 19 {
            return None;
        }
        let body = &data[8..];
        let command_units = usize::from(u16::from_le_bytes([body[1], body[2]]));
        if command_units > MAX_PATHFIELD_COMMAND_UNITS {
            return None;
        }
        let command_bytes = command_units.checked_mul(2)?;
        let command_end = 3usize.checked_add(command_bytes)?;
        let trailer_end = command_end.checked_add(8)?;
        if body.len() < trailer_end {
            return None;
        }
        let mut units: Vec<u16> = Vec::with_capacity(command_units);
        for chunk in body[3..command_end].chunks_exact(2) {
            units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        let raw_command = String::from_utf16(&units).ok()?;
        Some(Self { ctrl_id, raw_command })
    }
}

/// HWP5 representation of an `atno` inline page-number control
/// (Wave 12n).
///
/// Unlike the SUMMERY-family controls, `atno` is a fixed 16-byte record
/// with no Command string and no 8-byte trailer. The single 4-byte flag
/// at `body[0..4]` distinguishes current-page (`0x00`) from total-page
/// (`0x06`); other values are preserved verbatim via
/// [`Control::InlinePageNumber::raw_flag`].
///
/// Wire layout:
///
/// | offset | bytes | field |
/// |---|---|---|
/// | `body[0..4]` | LE u32 | kind flag (`0x00` current / `0x06` total) |
/// | `body[4..12]` | `01 00 00 00 00 00 00 00` | constant tail observed across fixtures |
#[derive(Debug, Clone)]
pub(crate) struct Hwp5InlinePageNumberControl {
    /// Owning control identifier, always `0x6174_6E6F` (`"atno"`).
    #[allow(dead_code)]
    pub ctrl_id: u32,
    /// Raw kind flag (`0x00` = current page, `0x06` = total pages,
    /// other values preserved for forward compatibility).
    pub raw_flag: u32,
}

impl Hwp5InlinePageNumberControl {
    /// Decodes an `atno` CtrlHeader payload. Returns `None` if the
    /// envelope is truncated.
    ///
    /// Wire layout (from Wave 12n native fixture analysis):
    ///
    /// | offset | bytes | meaning |
    /// |---|---|---|
    /// | `[0..4]` | ctrl_id | `"atno"` (LE bytes) |
    /// | `[4..8]` | LE u32 flag | `0x00` current page / `0x06` total pages |
    /// | `[8..16]` | constant tail | `01 00 00 00 00 00 00 00` |
    ///
    /// Unlike the `%clk`/`%smr`/`%dte`/`%pat` family this control does
    /// not have a separate 4-byte properties word — the flag sits in
    /// the slot that the other families use for properties.
    pub(crate) fn parse(ctrl_id: u32, data: &[u8]) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }
        let raw_flag = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        Some(Self { ctrl_id, raw_flag })
    }
}

impl Hwp5SummeryControl {
    /// Decodes a `%smr` CtrlHeader payload (the raw record `data`
    /// excluding the 4-byte CtrlHeader prefix the dispatcher already
    /// matched). Returns `None` on truncation or malformed UTF-16 — the
    /// decoder reports those as a targeted
    /// `Hwp5Warning::DroppedControl { control: "summery_field", … }` so
    /// the lossy path is auditable.
    pub(crate) fn parse(ctrl_id: u32, data: &[u8]) -> Option<Self> {
        // Need: 4 (ctrl_id) + 4 (properties) + 1 (flag) + 2 (count)
        // + 0 (command) + 8 (trailer) = 19 bytes minimum.
        if data.len() < 19 {
            return None;
        }
        // Skip the 8-byte CtrlHeader prefix (ctrl_id + properties).
        let body = &data[8..];
        // body[0] is the 0x08 flag (validated softly — Wave 12l pattern).
        let command_units = usize::from(u16::from_le_bytes([body[1], body[2]]));
        if command_units > MAX_SUMMERY_COMMAND_UNITS {
            return None;
        }
        let command_bytes = command_units.checked_mul(2)?;
        let command_end = 3usize.checked_add(command_bytes)?;
        // Trailer is 8 bytes (u32 field id + u32 padding).
        let trailer_end = command_end.checked_add(8)?;
        if body.len() < trailer_end {
            return None;
        }

        let mut units: Vec<u16> = Vec::with_capacity(command_units);
        for chunk in body[3..command_end].chunks_exact(2) {
            units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        let command_token = String::from_utf16(&units).ok()?;

        let field_unique_id = u32::from_le_bytes([
            body[command_end],
            body[command_end + 1],
            body[command_end + 2],
            body[command_end + 3],
        ]);
        // body[command_end+4..command_end+8] is the 4-byte zero pad —
        // intentionally not validated (Wave 12l convention).

        Some(Self { ctrl_id, command_token, field_unique_id })
    }
}

// ---------------------------------------------------------------------------
// Hwp5CrossRefControl (Wave 12m Phase 2)
// ---------------------------------------------------------------------------

/// Defence-in-depth allocation cap for the `%xrf` cross-reference Command
/// UTF-16 payload (Wave 12m). Observed Command strings range from 17 to 21
/// units (`?<target>;N1;N2;N3;N4;`); 1024 leaves ample headroom for
/// pathological inputs without inviting allocator abuse.
///
/// Cap is applied **immediately after reading the length-prefix**, before
/// any allocation occurs (Codex(architect) Wave 12m §UTF-16 cap timing).
#[allow(dead_code)] // Step 4 (decoder/projection wire-up) will reference this.
const MAX_CROSSREF_COMMAND_UNITS: usize = 1024;

/// HWP5 representation of a `%xrf` cross-reference control (Wave 12m
/// Phase 2).
///
/// Wire envelope (verified against 12 한컴 native fixtures in
/// `tests/fixtures/hwp5/crossref/`):
///
/// | offset | bytes | meaning |
/// |---|---|---|
/// | `data[0..4]` | ctrl_id | `"%xrf"` (LE bytes `66 72 78 25`) |
/// | `data[4..8]` | LE u32 | properties bitfield (always `0x0000_0002`) |
/// | `data[8]` | flag byte | always `0x00` |
/// | `data[9..11]` | LE u16 | UTF-16 command-unit count |
/// | `data[11..N]` | UTF-16LE | Command string `?<target>;N1;N2;N3;N4;` |
/// | `data[N..N+8]` | 8 bytes | trailer (begin_id u32 + field_id u32) |
///
/// The Command's semicolon-separated suffix encodes:
/// - `N1` = RefType (Table=0, Figure=1, Equation=2, Footnote=3,
///   Endnote=4, Outline=5, Bookmark=6)
/// - `N2` = ContentType (RefType-relative; Page=0, Number/Contents=1,
///   Contents/BookmarkName=2, UpDownPos=3)
/// - `N3` = as_hyperlink (0 / 1)
/// - `N4` = currently unidentified (all observed = 0)
///
/// Schema preserves **all wire fidelity** — semantic interpretation
/// (RefType / RefContentType enums, RefTarget normalization) happens at
/// the projection boundary in `smithy-hwp5/src/projection.rs`.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // Step 4 (decoder + projection) will construct + read these.
pub(crate) struct Hwp5CrossRefControl {
    /// Owning control identifier, always `0x2578_7266` (`"%xrf"` LE-ascii).
    #[allow(dead_code)]
    pub ctrl_id: u32,
    /// Full raw Command UTF-16LE-decoded string
    /// (`?<target>;N1;N2;N3;N4;`).
    pub command_raw: String,
    /// `?<target>` 의 target 부분만 — `#<system_id>` 의 `#` 도 포함된 raw.
    /// Bookmark refs 는 사용자 지정 string, 그 외 RefType 은 `#<id>` 형식.
    pub target_raw: String,
    /// N1 raw byte — RefType code.
    pub ref_type_code: u8,
    /// N2 raw byte — ContentType code (RefType-relative).
    pub content_type_code: u8,
    /// N3 raw byte — as_hyperlink code (0/1 observed; raw u8 preserved).
    pub hyperlink_code: u8,
    /// N4 raw byte — unidentified (all observed fixtures = 0; preserved
    /// for forward compatibility).
    pub param4_raw: u8,
    /// `data[8]` flag byte (always 0x00 observed).
    pub header_flag_raw: u8,
    /// Trailer 8-byte `begin_id` (first u32).
    pub trailer_begin_id: u32,
    /// Trailer 8-byte `field_id` (second u32).
    pub trailer_field_id: u32,
}

impl Hwp5CrossRefControl {
    #[allow(dead_code)] // Step 4 (decoder dispatch) will call this.
    /// Decodes a `%xrf` CtrlHeader payload. Returns `None` on:
    /// - truncated envelope (< 19 bytes minimum)
    /// - oversized command (> `MAX_CROSSREF_COMMAND_UNITS`)
    /// - malformed UTF-16
    /// - missing trailing semicolon on `?<target>;N1;N2;N3;N4;`
    /// - command lacks 5 semicolon-separated fields
    /// - N1/N2/N3/N4 fail to parse as `u8`
    ///
    /// All allocation caps are applied **before** any `Vec`/`String`
    /// allocation (Codex(architect) Wave 12m §UTF-16 cap timing CRITICAL).
    pub(crate) fn parse(ctrl_id: u32, data: &[u8]) -> Option<Self> {
        if data.len() < 19 {
            return None;
        }
        let body = &data[8..];
        let header_flag_raw = body[0];
        let command_units = usize::from(u16::from_le_bytes([body[1], body[2]]));
        // CRITICAL: cap check BEFORE allocation
        if command_units > MAX_CROSSREF_COMMAND_UNITS {
            return None;
        }
        let command_bytes = command_units.checked_mul(2)?;
        let command_end = 3usize.checked_add(command_bytes)?;
        let trailer_end = command_end.checked_add(8)?;
        if body.len() < trailer_end {
            return None;
        }

        let mut units: Vec<u16> = Vec::with_capacity(command_units);
        for chunk in body[3..command_end].chunks_exact(2) {
            units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        let command_raw = String::from_utf16(&units).ok()?;

        // Parse "?<target>;N1;N2;N3;N4;" — require leading `?`, trailing `;`,
        // and exactly 5 semicolon-separated fields after the `?` prefix.
        let stripped = command_raw.strip_prefix('?')?;
        if !stripped.ends_with(';') {
            return None;
        }
        // strip_suffix the trailing `;` so split doesn't yield empty final
        // segment, then expect target + 4 numeric fields = 5 fields.
        let inner = stripped.strip_suffix(';')?;
        let parts: Vec<&str> = inner.split(';').collect();
        if parts.len() != 5 {
            return None;
        }
        let target_raw = parts[0].to_string();
        let ref_type_code: u8 = parts[1].parse().ok()?;
        let content_type_code: u8 = parts[2].parse().ok()?;
        let hyperlink_code: u8 = parts[3].parse().ok()?;
        let param4_raw: u8 = parts[4].parse().ok()?;

        let trailer_begin_id = u32::from_le_bytes([
            body[command_end],
            body[command_end + 1],
            body[command_end + 2],
            body[command_end + 3],
        ]);
        let trailer_field_id = u32::from_le_bytes([
            body[command_end + 4],
            body[command_end + 5],
            body[command_end + 6],
            body[command_end + 7],
        ]);

        Some(Self {
            ctrl_id,
            command_raw,
            target_raw,
            ref_type_code,
            content_type_code,
            hyperlink_code,
            param4_raw,
            header_flag_raw,
            trailer_begin_id,
            trailer_field_id,
        })
    }
}

impl Hwp5MemoCommand {
    /// Decodes a `%unk` CtrlHeader payload into a `Hwp5MemoCommand`.
    ///
    /// Returns `None` if the payload doesn't carry a `"MEMO/…"` command —
    /// `%unk` is the catch-all ctrl_id and only commands with the `MEMO/`
    /// prefix are memo placeholders. Numeric fields that don't parse as
    /// `u32` default to `0` (HWP5 spec doesn't promise their numeric
    /// range; the wire is `String` from 한컴's side).
    pub(crate) fn parse(header_data: &[u8]) -> Option<Self> {
        let raw = parse_ctrl_header_command_string(header_data)?;
        let parts = split_slash_command(&raw, "MEMO")?;
        if parts.len() < 7 {
            // Spec allows trailing segments; require at least 7 (MEMO + 6
            // value slots). Shorter strings are malformed and dropped.
            return None;
        }
        Some(Self {
            raw: raw.clone(),
            shape_id: parts[1].parse().unwrap_or(0),
            memo_id: parts[2].parse().unwrap_or(0),
            hancom_inst_a: parts[3].parse().unwrap_or(0),
            hancom_inst_b: parts[4].parse().unwrap_or(0),
            author: parts[5].to_string(),
            terminator: parts[6].to_string(),
        })
    }
}

impl Hwp5EqEdit {
    /// Byte offset of the script length word: after the 4-byte property field.
    const SCRIPT_LEN_OFFSET: usize = 4;

    /// Parse the equation script from a `HWPTAG_EQEDIT` payload.
    pub(crate) fn parse(data: &[u8]) -> Hwp5Result<Self> {
        let len_at = Self::SCRIPT_LEN_OFFSET;
        if data.len() < len_at + 2 {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!("EQEDIT too short for script length: {} bytes", data.len()),
            });
        }
        let char_count = usize::from(u16::from_le_bytes([data[len_at], data[len_at + 1]]));
        let start = len_at + 2;
        let byte_len = char_count.checked_mul(2).ok_or_else(|| Hwp5Error::RecordParse {
            offset: 0,
            detail: format!("EQEDIT script length overflows: {char_count}"),
        })?;
        let end = start.checked_add(byte_len).ok_or_else(|| Hwp5Error::RecordParse {
            offset: 0,
            detail: format!("EQEDIT script range overflows: {char_count}"),
        })?;
        if data.len() < end {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "EQEDIT too short for {char_count}-char script: {} bytes (expected >= {end})",
                    data.len()
                ),
            });
        }
        let units: Vec<u16> =
            data[start..end].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Ok(Self { script: String::from_utf16_lossy(&units) })
    }
}

// ---------------------------------------------------------------------------
// Hwp5ShapePicture
// ---------------------------------------------------------------------------

/// Minimal `ShapePicture` payload needed to resolve a `DocInfo/BinData` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hwp5ShapePicture {
    /// 1-based binary item identifier.
    pub binary_data_id: u16,
}

impl Hwp5ShapePicture {
    /// Prefix bytes preceding the binary item identifier in a picture record.
    const BINARY_DATA_ID_OFFSET: usize = 71;
    /// Minimum payload size required to recover the binary item identifier.
    const MIN_SIZE: usize = Self::BINARY_DATA_ID_OFFSET + 2;

    /// Parse a `ShapePicture` payload.
    pub(crate) fn parse(data: &[u8]) -> Hwp5Result<Self> {
        if data.len() < Self::MIN_SIZE {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ShapePicture too short: {} bytes (expected >= {})",
                    data.len(),
                    Self::MIN_SIZE
                ),
            });
        }

        let start = Self::BINARY_DATA_ID_OFFSET;
        let binary_data_id = u16::from_le_bytes([data[start], data[start + 1]]);
        Ok(Self { binary_data_id })
    }
}

// ---------------------------------------------------------------------------
// Hwp5ShapeComponentOle
// ---------------------------------------------------------------------------

/// Minimal `ShapeComponentOle` payload needed to preserve embedded-object evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hwp5ShapeComponentOle {
    /// Raw OLE/object property bitfield.
    pub property: u32,
    /// Embedded object extent width in HWPUNIT.
    pub extent_width: i32,
    /// Embedded object extent height in HWPUNIT.
    pub extent_height: i32,
    /// 1-based binary item identifier backing the embedded object.
    pub binary_data_id: u16,
}

impl Hwp5ShapeComponentOle {
    /// Minimum payload size required to recover property, extents, and storage reference.
    const MIN_SIZE: usize = 14;

    /// Parse the stable OLE evidence prefix from a `ShapeComponentOle` payload.
    pub(crate) fn parse(data: &[u8]) -> Hwp5Result<Self> {
        if data.len() < Self::MIN_SIZE {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "ShapeComponentOle too short: {} bytes (expected >= {})",
                    data.len(),
                    Self::MIN_SIZE
                ),
            });
        }

        let mut cur = Cursor::new(data);
        let property = cur.read_u32::<LittleEndian>()?;
        let extent_width = cur.read_i32::<LittleEndian>()?;
        let extent_height = cur.read_i32::<LittleEndian>()?;
        let binary_data_id = cur.read_u16::<LittleEndian>()?;

        Ok(Self { property, extent_width, extent_height, binary_data_id })
    }
}

// ---------------------------------------------------------------------------
// Hwp5PageDef
// ---------------------------------------------------------------------------

/// Parsed from a `PageDef` (tag `0x49`) record in a BodyText section.
///
/// Describes the page dimensions, margins, and orientation for the section
/// that follows this record.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5PageDef {
    /// Page width in HwpUnit (portrait width regardless of orientation).
    pub width: u32,
    /// Page height in HwpUnit (portrait height regardless of orientation).
    pub height: u32,
    /// Left margin in HwpUnit.
    pub margin_left: u32,
    /// Right margin in HwpUnit.
    pub margin_right: u32,
    /// Top margin in HwpUnit.
    pub margin_top: u32,
    /// Bottom margin in HwpUnit.
    pub margin_bottom: u32,
    /// Header area height in HwpUnit.
    pub header_margin: u32,
    /// Footer area height in HwpUnit.
    pub footer_margin: u32,
    /// Gutter (binding margin) in HwpUnit.
    pub gutter: u32,
    /// `true` if the page uses landscape orientation (property bit 0 is set).
    pub landscape: bool,
}

impl Hwp5PageDef {
    /// Minimum byte length for a `PageDef` payload.
    const MIN_SIZE: usize = 40;

    /// Parse a `PageDef` record from its raw payload bytes.
    ///
    /// Layout:
    /// - `[0..4]`   width (u32 LE)
    /// - `[4..8]`   height (u32 LE)
    /// - `[8..12]`  margin_left (u32 LE)
    /// - `[12..16]` margin_right (u32 LE)
    /// - `[16..20]` margin_top (u32 LE)
    /// - `[20..24]` margin_bottom (u32 LE)
    /// - `[24..28]` header_margin (u32 LE)
    /// - `[28..32]` footer_margin (u32 LE)
    /// - `[32..36]` gutter (u32 LE)
    /// - `[36..40]` property bitfield (u32 LE); bit 0 = landscape
    ///
    /// # Errors
    ///
    /// Returns [`Hwp5Error::RecordParse`] if `data` is shorter than 40 bytes.
    pub(crate) fn parse(data: &[u8]) -> Hwp5Result<Self> {
        if data.len() < Self::MIN_SIZE {
            return Err(Hwp5Error::RecordParse {
                offset: 0,
                detail: format!(
                    "PageDef too short: {} bytes (expected >= {})",
                    data.len(),
                    Self::MIN_SIZE
                ),
            });
        }
        let mut cur = Cursor::new(data);
        let width = cur.read_u32::<LittleEndian>()?;
        let height = cur.read_u32::<LittleEndian>()?;
        let margin_left = cur.read_u32::<LittleEndian>()?;
        let margin_right = cur.read_u32::<LittleEndian>()?;
        let margin_top = cur.read_u32::<LittleEndian>()?;
        let margin_bottom = cur.read_u32::<LittleEndian>()?;
        let header_margin = cur.read_u32::<LittleEndian>()?;
        let footer_margin = cur.read_u32::<LittleEndian>()?;
        let gutter = cur.read_u32::<LittleEndian>()?;
        let property = cur.read_u32::<LittleEndian>()?;
        let landscape = (property & 0x01) != 0;
        Ok(Self {
            width,
            height,
            margin_left,
            margin_right,
            margin_top,
            margin_bottom,
            header_margin,
            footer_margin,
            gutter,
            landscape,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Hwp5ParaHeader
    // -----------------------------------------------------------------------

    fn make_para_header(
        char_count: u32,
        control_mask: u32,
        para_shape_id: u16,
        style_id: u8,
        line_seg_count: u16,
        char_shape_count: u16,
    ) -> Vec<u8> {
        let mut buf = vec![0u8; 22];
        buf[0..4].copy_from_slice(&char_count.to_le_bytes());
        buf[4..8].copy_from_slice(&control_mask.to_le_bytes());
        buf[8..10].copy_from_slice(&para_shape_id.to_le_bytes());
        buf[10] = style_id;
        // [11] page_break = 0
        buf[12..14].copy_from_slice(&char_shape_count.to_le_bytes());
        // [14..16] range_tag_count = 0
        buf[16..18].copy_from_slice(&line_seg_count.to_le_bytes());
        // [18..22] instance_id = 0
        buf
    }

    #[test]
    fn para_header_parse_basic() {
        let data = make_para_header(100, 0x0003, 5, 2, 3, 4);
        let h = Hwp5ParaHeader::parse(&data).unwrap();
        assert_eq!(h.char_count, 100);
        assert_eq!(h.control_mask, 0x0003);
        assert_eq!(h.para_shape_id, 5);
        assert_eq!(h.style_id, 2);
        assert_eq!(h.line_seg_count, 3);
        assert_eq!(h.char_shape_count, 4);
    }

    #[test]
    fn para_header_parse_zero_counts() {
        let data = make_para_header(0, 0, 0, 0, 0, 0);
        let h = Hwp5ParaHeader::parse(&data).unwrap();
        assert_eq!(h.char_count, 0);
        assert_eq!(h.char_shape_count, 0);
        assert_eq!(h.line_seg_count, 0);
    }

    #[test]
    fn para_header_parse_max_values() {
        let data = make_para_header(u32::MAX, u32::MAX, u16::MAX, u8::MAX, u16::MAX, u16::MAX);
        let h = Hwp5ParaHeader::parse(&data).unwrap();
        assert_eq!(h.char_count, u32::MAX);
        assert_eq!(h.para_shape_id, u16::MAX);
        assert_eq!(h.style_id, u8::MAX);
        assert_eq!(h.char_shape_count, u16::MAX);
    }

    #[test]
    fn para_header_too_short() {
        let data = vec![0u8; 21];
        assert!(matches!(Hwp5ParaHeader::parse(&data).unwrap_err(), Hwp5Error::RecordParse { .. }));
    }

    #[test]
    fn para_header_empty() {
        assert!(matches!(Hwp5ParaHeader::parse(&[]).unwrap_err(), Hwp5Error::RecordParse { .. }));
    }

    #[test]
    fn para_header_larger_data_ok() {
        // Extra bytes beyond 34 should be ignored.
        let mut data = make_para_header(10, 0, 1, 0, 2, 1);
        data.extend_from_slice(&[0xFF; 20]);
        let h = Hwp5ParaHeader::parse(&data).unwrap();
        assert_eq!(h.char_count, 10);
    }

    // -----------------------------------------------------------------------
    // Hwp5ParaText
    // -----------------------------------------------------------------------

    fn utf16le(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect()
    }

    fn cp_bytes(cp: u16) -> Vec<u8> {
        cp.to_le_bytes().to_vec()
    }

    fn inline_control_bytes(cp: u16, extra_words: [u16; 7]) -> Vec<u8> {
        let mut data = cp_bytes(cp);
        for word in extra_words {
            data.extend_from_slice(&word.to_le_bytes());
        }
        data
    }

    #[test]
    fn para_text_empty_data() {
        let pt = Hwp5ParaText::parse(&[]).unwrap();
        assert!(pt.segments.is_empty());
    }

    #[test]
    fn para_text_plain_text() {
        let data = utf16le("안녕");
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments.len(), 1);
        assert_eq!(pt.segments[0], TextSegment::Text("안녕".into()));
    }

    #[test]
    fn para_text_ascii() {
        let data = utf16le("Hello");
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments, vec![TextSegment::Text("Hello".into())]);
    }

    #[test]
    fn para_text_tab() {
        let mut data = utf16le("A");
        data.extend_from_slice(&inline_control_bytes(0x09, [1, 2, 3, 4, 5, 6, 7]));
        data.extend_from_slice(&utf16le("B"));
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(
            pt.segments,
            vec![
                TextSegment::Text("A".into()),
                // `inline_control_bytes(0x09, [1, 2, 3, 4, 5, 6, 7])`
                // emits each u16 as little-endian bytes.
                TextSegment::Tab { extra: [1, 0, 2, 0, 3, 0, 4, 0, 5, 0, 6, 0, 7, 0] },
                TextSegment::Text("B".into()),
            ]
        );
    }

    #[test]
    fn para_text_line_break() {
        let mut data = cp_bytes(0x0A);
        data.extend_from_slice(&utf16le("X"));
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments, vec![TextSegment::LineBreak, TextSegment::Text("X".into()),]);
    }

    #[test]
    fn para_text_para_break() {
        let data = cp_bytes(0x0D);
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments, vec![TextSegment::ParaBreak]);
    }

    #[test]
    fn para_text_field_end() {
        let data = inline_control_bytes(0x04, [1, 2, 3, 4, 5, 6, 7]);
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments, vec![TextSegment::FieldEnd]);
    }

    #[test]
    fn para_text_non_breaking_space() {
        // HWP5 control code 0x18 = non-breaking space (per openhwp reference).
        let data = cp_bytes(0x18);
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments, vec![TextSegment::NonBreakingSpace]);
    }

    #[test]
    fn para_text_fixed_width_space() {
        // HWP5 control code 0x1F = fixed-width space (per openhwp reference).
        let data = cp_bytes(0x1F);
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments, vec![TextSegment::FwSpace]);
    }

    #[test]
    fn para_text_fwspace_between_text_runs() {
        // Mirrors the Hancom-authored `sample-fwspace-fixed.hwp` wire shape:
        // FWLEFT<U+001F>FWRIGHT
        let mut data = utf16le("FWLEFT");
        data.extend_from_slice(&cp_bytes(0x1F));
        data.extend_from_slice(&utf16le("FWRIGHT"));
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(
            pt.segments,
            vec![
                TextSegment::Text("FWLEFT".into()),
                TextSegment::FwSpace,
                TextSegment::Text("FWRIGHT".into()),
            ]
        );
    }

    #[test]
    fn para_text_hard_hyphen_is_consumed_silently_without_breaking_surrounding_text() {
        // 0x1E (hard-hyphen) is not modeled in Core yet — must not be confused
        // with non-breaking space and must not get appended to surrounding text.
        let mut data = utf16le("A");
        data.extend_from_slice(&cp_bytes(0x1E));
        data.extend_from_slice(&utf16le("B"));
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments, vec![TextSegment::Text("A".into()), TextSegment::Text("B".into())]);
    }

    #[test]
    fn shape_component_ole_parse_minimal_prefix() {
        let mut data = Vec::new();
        data.extend_from_slice(&0x0000_0003u32.to_le_bytes());
        data.extend_from_slice(&1200i32.to_le_bytes());
        data.extend_from_slice(&3400i32.to_le_bytes());
        data.extend_from_slice(&7u16.to_le_bytes());
        data.extend_from_slice(&[0xFF; 12]);

        let ole = Hwp5ShapeComponentOle::parse(&data).unwrap();
        assert_eq!(ole.property, 0x0000_0003);
        assert_eq!(ole.extent_width, 1200);
        assert_eq!(ole.extent_height, 3400);
        assert_eq!(ole.binary_data_id, 7);
    }

    #[test]
    fn shape_component_ole_too_short() {
        let data = vec![0u8; 13];
        assert!(matches!(
            Hwp5ShapeComponentOle::parse(&data).unwrap_err(),
            Hwp5Error::RecordParse { .. }
        ));
    }

    #[test]
    fn shape_component_line_parse_minimal_prefix() {
        let mut data = Vec::new();
        data.extend_from_slice(&10i32.to_le_bytes());
        data.extend_from_slice(&20i32.to_le_bytes());
        data.extend_from_slice(&30i32.to_le_bytes());
        data.extend_from_slice(&40i32.to_le_bytes());
        data.extend_from_slice(&[0xFF; 8]);

        let line = Hwp5ShapeComponentLine::parse(&data).unwrap();
        assert_eq!(line.start, Hwp5ShapePoint { x: 10, y: 20 });
        assert_eq!(line.end, Hwp5ShapePoint { x: 30, y: 40 });
    }

    #[test]
    fn shape_component_line_too_short() {
        let data = vec![0u8; 15];
        assert!(matches!(
            Hwp5ShapeComponentLine::parse(&data).unwrap_err(),
            Hwp5Error::RecordParse { .. }
        ));
    }

    #[test]
    fn shape_component_polygon_parse_points() {
        let mut data = Vec::new();
        data.extend_from_slice(&3u32.to_le_bytes());
        for (x, y) in [(0i32, 0i32), (100i32, 200i32), (300i32, 400i32)] {
            data.extend_from_slice(&x.to_le_bytes());
            data.extend_from_slice(&y.to_le_bytes());
        }

        let polygon = Hwp5ShapeComponentPolygon::parse(&data).unwrap();
        assert_eq!(
            polygon.points,
            vec![
                Hwp5ShapePoint { x: 0, y: 0 },
                Hwp5ShapePoint { x: 100, y: 200 },
                Hwp5ShapePoint { x: 300, y: 400 },
            ]
        );
    }

    #[test]
    fn shape_component_polygon_too_short_for_points() {
        let mut data = Vec::new();
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&10i32.to_le_bytes());
        data.extend_from_slice(&20i32.to_le_bytes());

        assert!(matches!(
            Hwp5ShapeComponentPolygon::parse(&data).unwrap_err(),
            Hwp5Error::RecordParse { .. }
        ));
    }

    /// Build a 60-byte `ShapeComponentEllipse` payload from a property word and
    /// the seven `(x, y)` point pairs in record order.
    fn ellipse_bytes(property: u32, points: [(i32, i32); 7]) -> Vec<u8> {
        let mut data = Vec::with_capacity(Hwp5ShapeComponentEllipse::MIN_SIZE);
        data.extend_from_slice(&property.to_le_bytes());
        for (x, y) in points {
            data.extend_from_slice(&x.to_le_bytes());
            data.extend_from_slice(&y.to_le_bytes());
        }
        data
    }

    #[test]
    fn shape_component_ellipse_parses_plain_ellipse() {
        // Mirrors the 한컴 plain-ellipse layout: property 0, real center/axes,
        // arc endpoints left at the origin.
        let data = ellipse_bytes(
            0,
            [(7086, 4252), (14173, 4252), (7086, 8504), (0, 0), (0, 0), (0, 0), (0, 0)],
        );
        let ellipse = Hwp5ShapeComponentEllipse::parse(&data).unwrap();
        assert_eq!(ellipse.property, 0);
        assert_eq!(ellipse.center, Hwp5ShapePoint { x: 7086, y: 4252 });
        assert_eq!(ellipse.axis1, Hwp5ShapePoint { x: 14173, y: 4252 });
        assert_eq!(ellipse.axis2, Hwp5ShapePoint { x: 7086, y: 8504 });
        assert!(!ellipse.is_arc(), "zero property + origin arc points is a plain ellipse");
    }

    #[test]
    fn shape_component_ellipse_with_property_is_arc() {
        // 한컴 normal arc: property 2, arc endpoints populated.
        let data = ellipse_bytes(
            2,
            [
                (5669, 4252),
                (11339, 4252),
                (5669, 8504),
                (11339, 4252),
                (5669, 0),
                (11349, 4265),
                (5671, 27),
            ],
        );
        let arc = Hwp5ShapeComponentEllipse::parse(&data).unwrap();
        assert_eq!(arc.property, 2);
        assert_eq!(arc.start1, Hwp5ShapePoint { x: 11339, y: 4252 });
        assert_eq!(arc.end2, Hwp5ShapePoint { x: 5671, y: 27 });
        assert!(arc.is_arc(), "non-zero property marks an arc");
    }

    #[test]
    fn shape_component_ellipse_arc_points_alone_mark_arc() {
        // Even with property 0, a non-origin arc endpoint means an arc.
        let data = ellipse_bytes(0, [(10, 10), (20, 10), (10, 20), (5, 5), (0, 0), (0, 0), (0, 0)]);
        let parsed = Hwp5ShapeComponentEllipse::parse(&data).unwrap();
        assert!(parsed.is_arc(), "content-based arc detection ignores unknown property bits");
    }

    #[test]
    fn shape_component_ellipse_too_short() {
        assert!(matches!(
            Hwp5ShapeComponentEllipse::parse(&[0u8; 59]).unwrap_err(),
            Hwp5Error::RecordParse { .. }
        ));
    }

    #[test]
    fn shape_component_curve_parses_points_and_segments() {
        let mut data = Vec::new();
        data.extend_from_slice(&4u32.to_le_bytes());
        for (x, y) in [(0i32, 5000i32), (3000, 0), (6000, 10000), (9000, 5000)] {
            data.extend_from_slice(&x.to_le_bytes());
            data.extend_from_slice(&y.to_le_bytes());
        }
        // count - 1 = 3 segment bytes, then trailing reserved bytes we ignore.
        data.extend_from_slice(&[1u8, 0u8, 1u8, 0u8, 0u8, 0u8, 0u8]);

        let curve = Hwp5ShapeComponentCurve::parse(&data).unwrap();
        assert_eq!(
            curve.points,
            vec![
                Hwp5ShapePoint { x: 0, y: 5000 },
                Hwp5ShapePoint { x: 3000, y: 0 },
                Hwp5ShapePoint { x: 6000, y: 10000 },
                Hwp5ShapePoint { x: 9000, y: 5000 },
            ]
        );
        assert_eq!(curve.segment_types, vec![1, 0, 1]);
    }

    #[test]
    fn shape_component_curve_tolerates_missing_segment_bytes() {
        // Points present but no trailing segment bytes — best-effort, no panic.
        let mut data = Vec::new();
        data.extend_from_slice(&2u32.to_le_bytes());
        for (x, y) in [(0i32, 0i32), (100, 200)] {
            data.extend_from_slice(&x.to_le_bytes());
            data.extend_from_slice(&y.to_le_bytes());
        }
        let curve = Hwp5ShapeComponentCurve::parse(&data).unwrap();
        assert_eq!(curve.points.len(), 2);
        assert!(curve.segment_types.is_empty());
    }

    #[test]
    fn shape_component_curve_too_short_for_points() {
        let mut data = Vec::new();
        data.extend_from_slice(&5u32.to_le_bytes());
        data.extend_from_slice(&1i32.to_le_bytes());

        assert!(matches!(
            Hwp5ShapeComponentCurve::parse(&data).unwrap_err(),
            Hwp5Error::RecordParse { .. }
        ));
    }

    #[test]
    fn eqedit_parses_script_after_property_and_length() {
        let script = "{a + b} over {c + d}";
        let units: Vec<u16> = script.encode_utf16().collect();
        let mut data = Vec::new();
        data.extend_from_slice(&0u32.to_le_bytes()); // property
        data.extend_from_slice(&(units.len() as u16).to_le_bytes()); // WCHAR count
        for u in &units {
            data.extend_from_slice(&u.to_le_bytes());
        }
        // Trailing version/font fields are present in real records; ignored here.
        data.extend_from_slice(&[0xDE, 0xAD]);

        let parsed = Hwp5EqEdit::parse(&data).unwrap();
        assert_eq!(parsed.script, script);
    }

    #[test]
    fn eqedit_too_short_for_declared_script() {
        let mut data = Vec::new();
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&10u16.to_le_bytes()); // claims 10 chars
        data.extend_from_slice(&[0x41, 0x00]); // but only 1 provided

        assert!(matches!(Hwp5EqEdit::parse(&data).unwrap_err(), Hwp5Error::RecordParse { .. }));
    }

    #[test]
    fn para_text_control_ref_with_extra() {
        // 0x0B followed by 7 u16 extra words.
        let mut data = cp_bytes(0x0B);
        let extra_words: [u16; 7] = [0x1234, 0x5678, 0x9ABC, 0xDEF0, 0x1111, 0x2222, 0x3333];
        for &w in &extra_words {
            data.extend_from_slice(&w.to_le_bytes());
        }
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments.len(), 1);
        if let TextSegment::ControlRef { extra } = &pt.segments[0] {
            assert_eq!(extra[0..2], 0x1234u16.to_le_bytes());
            assert_eq!(extra[2..4], 0x5678u16.to_le_bytes());
        } else {
            panic!("expected ControlRef");
        }
    }

    #[test]
    fn para_text_extended_control_ref_with_extra() {
        let mut data = cp_bytes(0x0C);
        for w in [0xAAAAu16, 0xBBBB, 0xCCCC, 0xDDDD, 0xEEEE, 0xFFFF, 0x1111] {
            data.extend_from_slice(&w.to_le_bytes());
        }
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments.len(), 1);
        assert!(matches!(pt.segments[0], TextSegment::ExtendedControlRef { .. }));
    }

    #[test]
    fn para_text_section_column_def_with_extra() {
        let mut data = cp_bytes(0x02);
        for w in [0x0001u16, 0x0002, 0x0003, 0x0004, 0x0005, 0x0006, 0x0007] {
            data.extend_from_slice(&w.to_le_bytes());
        }
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments.len(), 1);
        assert!(matches!(pt.segments[0], TextSegment::SectionColumnDef { .. }));
    }

    #[test]
    fn para_text_field_begin_with_extra() {
        let mut data = cp_bytes(0x03);
        for w in [0x0011u16, 0x0022, 0x0033, 0x0044, 0x0055, 0x0066, 0x0077] {
            data.extend_from_slice(&w.to_le_bytes());
        }
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments.len(), 1);
        assert!(matches!(pt.segments[0], TextSegment::FieldBegin { .. }));
    }

    #[test]
    fn para_text_control_ref_missing_extra_returns_error() {
        // 0x0B with no following words — should fail.
        let data = cp_bytes(0x0B);
        assert!(matches!(Hwp5ParaText::parse(&data).unwrap_err(), Hwp5Error::RecordParse { .. }));
    }

    #[test]
    fn para_text_odd_byte_count_returns_error() {
        let data = vec![0x41u8, 0x00, 0x42];
        assert!(matches!(Hwp5ParaText::parse(&data).unwrap_err(), Hwp5Error::RecordParse { .. }));
    }

    #[test]
    fn para_text_reserved_chars_skipped() {
        // 0x00 is a single-wchar reserved control and should not produce a segment.
        let mut data = Vec::new();
        data.extend_from_slice(&cp_bytes(0x00));
        data.extend_from_slice(&utf16le("ok"));
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments, vec![TextSegment::Text("ok".into())]);
    }

    #[test]
    fn para_text_inline_controls_with_payload_are_consumed() {
        let mut data = Vec::new();
        for cp in [0x05u16, 0x06, 0x07, 0x08, 0x13, 0x14] {
            data.extend_from_slice(&inline_control_bytes(cp, [0, 0, 0, 0, 0, 0, 0]));
        }
        data.extend_from_slice(&utf16le("ok"));
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments, vec![TextSegment::Text("ok".into())]);
    }

    #[test]
    fn para_text_extended_reserved_0x01_skipped() {
        // 0x01 is an extended control (8 wchars total): consumed with 7 extra.
        let mut data = cp_bytes(0x01);
        for _ in 0..7 {
            data.extend_from_slice(&0x0000u16.to_le_bytes());
        }
        data.extend_from_slice(&utf16le("ok"));
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments, vec![TextSegment::Text("ok".into())]);
    }

    #[test]
    fn para_text_extended_0x0e_through_0x17_skipped() {
        // 0x0E-0x17 are extended controls (8 wchars total): consumed silently.
        let mut data = cp_bytes(0x0E);
        for _ in 0..7 {
            data.extend_from_slice(&0x0000u16.to_le_bytes());
        }
        data.extend_from_slice(&utf16le("ok"));
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments, vec![TextSegment::Text("ok".into())]);
    }

    #[test]
    fn para_text_multiple_segments() {
        let mut data = utf16le("hi");
        data.extend_from_slice(&inline_control_bytes(0x09, [1, 2, 3, 4, 5, 6, 7])); // tab
        data.extend_from_slice(&utf16le("there"));
        data.extend_from_slice(&cp_bytes(0x0D)); // para break
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(
            pt.segments,
            vec![
                TextSegment::Text("hi".into()),
                TextSegment::Tab { extra: [1, 0, 2, 0, 3, 0, 4, 0, 5, 0, 6, 0, 7, 0] },
                TextSegment::Text("there".into()),
                TextSegment::ParaBreak,
            ]
        );
    }

    #[test]
    fn para_text_tab_missing_inline_payload_returns_error() {
        let data = cp_bytes(0x09);
        assert!(matches!(Hwp5ParaText::parse(&data).unwrap_err(), Hwp5Error::RecordParse { .. }));
    }

    #[test]
    fn para_text_field_end_missing_inline_payload_returns_error() {
        let data = cp_bytes(0x04);
        assert!(matches!(Hwp5ParaText::parse(&data).unwrap_err(), Hwp5Error::RecordParse { .. }));
    }

    // -----------------------------------------------------------------------
    // Hwp5CharShapeRun
    // -----------------------------------------------------------------------

    fn make_run_bytes(position: u32, char_shape_id: u32) -> Vec<u8> {
        let mut buf = Vec::with_capacity(8);
        buf.extend_from_slice(&position.to_le_bytes());
        buf.extend_from_slice(&char_shape_id.to_le_bytes());
        buf
    }

    #[test]
    fn char_shape_run_empty_data() {
        let runs = Hwp5CharShapeRun::parse_all(&[]).unwrap();
        assert!(runs.is_empty());
    }

    #[test]
    fn char_shape_run_single() {
        let data = make_run_bytes(0, 3);
        let runs = Hwp5CharShapeRun::parse_all(&data).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].position, 0);
        assert_eq!(runs[0].char_shape_id, 3);
    }

    #[test]
    fn char_shape_run_multiple() {
        let mut data = make_run_bytes(0, 1);
        data.extend_from_slice(&make_run_bytes(10, 2));
        data.extend_from_slice(&make_run_bytes(20, 5));
        let runs = Hwp5CharShapeRun::parse_all(&data).unwrap();
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0], Hwp5CharShapeRun { position: 0, char_shape_id: 1 });
        assert_eq!(runs[1], Hwp5CharShapeRun { position: 10, char_shape_id: 2 });
        assert_eq!(runs[2], Hwp5CharShapeRun { position: 20, char_shape_id: 5 });
    }

    #[test]
    fn char_shape_run_max_values() {
        let data = make_run_bytes(u32::MAX, u32::MAX);
        let runs = Hwp5CharShapeRun::parse_all(&data).unwrap();
        assert_eq!(runs[0].position, u32::MAX);
        assert_eq!(runs[0].char_shape_id, u32::MAX);
    }

    #[test]
    fn char_shape_run_non_multiple_returns_error() {
        let data = vec![0u8; 7]; // not a multiple of 8
        assert!(matches!(
            Hwp5CharShapeRun::parse_all(&data).unwrap_err(),
            Hwp5Error::RecordParse { .. }
        ));
    }

    #[test]
    fn char_shape_run_non_multiple_9_bytes() {
        let data = vec![0u8; 9];
        assert!(matches!(
            Hwp5CharShapeRun::parse_all(&data).unwrap_err(),
            Hwp5Error::RecordParse { .. }
        ));
    }

    // -----------------------------------------------------------------------
    // Hwp5ShapeComponentGeometry
    // -----------------------------------------------------------------------

    #[test]
    fn shape_component_geometry_parses_signed_offsets_and_size() {
        let mut data = vec![0u8; 24];
        data[0..4].copy_from_slice(&0x6773_6F20u32.to_le_bytes());
        data[8..12].copy_from_slice(&(-720i32).to_le_bytes());
        data[12..16].copy_from_slice(&1440i32.to_le_bytes());
        data[16..20].copy_from_slice(&28_800u32.to_le_bytes());
        data[20..24].copy_from_slice(&14_400u32.to_le_bytes());

        let geometry = Hwp5ShapeComponentGeometry::parse_from_ctrl_header(&data).unwrap();
        assert_eq!(
            geometry,
            Hwp5ShapeComponentGeometry { x: 1440, y: -720, width: 28_800, height: 14_400 }
        );
    }

    #[test]
    fn shape_component_geometry_requires_full_ctrl_header_payload() {
        assert!(matches!(
            Hwp5ShapeComponentGeometry::parse_from_ctrl_header(&[0u8; 20]).unwrap_err(),
            Hwp5Error::RecordParse { .. }
        ));
    }

    // -----------------------------------------------------------------------
    // Hwp5ShapePicture
    // -----------------------------------------------------------------------

    #[test]
    fn shape_picture_parses_binary_data_id() {
        let mut data = vec![0u8; 73];
        data[71..73].copy_from_slice(&1u16.to_le_bytes());
        let picture = Hwp5ShapePicture::parse(&data).unwrap();
        assert_eq!(picture, Hwp5ShapePicture { binary_data_id: 1 });
    }

    #[test]
    fn shape_picture_too_short_fails() {
        assert!(matches!(
            Hwp5ShapePicture::parse(&[0u8; 72]).unwrap_err(),
            Hwp5Error::RecordParse { .. }
        ));
    }

    // -----------------------------------------------------------------------
    // Hwp5PageDef
    // -----------------------------------------------------------------------

    fn make_page_def(
        width: u32,
        height: u32,
        margins: [u32; 6],
        gutter: u32,
        property: u32,
    ) -> Vec<u8> {
        let mut buf = Vec::with_capacity(40);
        buf.extend_from_slice(&width.to_le_bytes());
        buf.extend_from_slice(&height.to_le_bytes());
        for m in margins {
            buf.extend_from_slice(&m.to_le_bytes());
        }
        buf.extend_from_slice(&gutter.to_le_bytes());
        buf.extend_from_slice(&property.to_le_bytes());
        buf
    }

    #[test]
    fn page_def_parse_portrait() {
        // A4: 210mm × 297mm ≈ 59535 × 84180 HwpUnit
        let data = make_page_def(59535, 84180, [5670, 5670, 5670, 4252, 4252, 4252], 0, 0x00);
        let pd = Hwp5PageDef::parse(&data).unwrap();
        assert_eq!(pd.width, 59535);
        assert_eq!(pd.height, 84180);
        assert_eq!(pd.margin_left, 5670);
        assert_eq!(pd.margin_top, 5670);
        assert_eq!(pd.gutter, 0);
        assert!(!pd.landscape);
    }

    #[test]
    fn page_def_parse_landscape() {
        let data = make_page_def(84180, 59535, [0; 6], 0, 0x01);
        let pd = Hwp5PageDef::parse(&data).unwrap();
        assert!(pd.landscape);
        assert_eq!(pd.width, 84180);
    }

    #[test]
    fn page_def_parse_property_bit1_not_landscape() {
        // bit 1 set but bit 0 clear — landscape should be false.
        let data = make_page_def(100, 200, [0; 6], 0, 0x02);
        let pd = Hwp5PageDef::parse(&data).unwrap();
        assert!(!pd.landscape);
    }

    #[test]
    fn page_def_parse_all_margins() {
        let margins = [1000u32, 2000, 3000, 4000, 5000, 6000];
        let data = make_page_def(0, 0, margins, 1500, 0);
        let pd = Hwp5PageDef::parse(&data).unwrap();
        assert_eq!(pd.margin_left, 1000);
        assert_eq!(pd.margin_right, 2000);
        assert_eq!(pd.margin_top, 3000);
        assert_eq!(pd.margin_bottom, 4000);
        assert_eq!(pd.header_margin, 5000);
        assert_eq!(pd.footer_margin, 6000);
        assert_eq!(pd.gutter, 1500);
    }

    #[test]
    fn page_def_too_short() {
        let data = vec![0u8; 39];
        assert!(matches!(Hwp5PageDef::parse(&data).unwrap_err(), Hwp5Error::RecordParse { .. }));
    }

    #[test]
    fn page_def_empty() {
        assert!(matches!(Hwp5PageDef::parse(&[]).unwrap_err(), Hwp5Error::RecordParse { .. }));
    }

    #[test]
    fn page_def_larger_data_ok() {
        // Extra bytes beyond 40 should be ignored.
        let mut data = make_page_def(100, 200, [10; 6], 5, 0x01);
        data.extend_from_slice(&[0xFF; 100]);
        let pd = Hwp5PageDef::parse(&data).unwrap();
        assert!(pd.landscape);
        assert_eq!(pd.width, 100);
    }

    // -----------------------------------------------------------------------
    // Hwp5IndexMarkControl (Wave 12k)
    // -----------------------------------------------------------------------

    const IDXM_CTRL_ID: u32 = 0x6964_786D;

    /// Builds a synthetic `idxm` CtrlHeader payload (ctrl_id +
    /// properties + body) for `Hwp5IndexMarkControl::parse`.
    fn make_idxm(primary: &str, secondary: Option<&str>, trailer: u32) -> Vec<u8> {
        let primary_units: Vec<u16> = primary.encode_utf16().collect();
        assert!(!primary_units.is_empty(), "primary must have at least one unit");
        let primary_units_len = u16::try_from(primary_units.len()).expect("primary too long");
        let primary_first = primary_units[0];

        let mut data = Vec::new();
        // ctrl_id (BE bytes mirror the on-wire LE storage for `idxm`).
        data.extend_from_slice(&IDXM_CTRL_ID.to_be_bytes());
        // properties.low = primary_units_len, .high = primary[0].
        data.extend_from_slice(&primary_units_len.to_le_bytes());
        data.extend_from_slice(&primary_first.to_le_bytes());
        // primary[1..].
        for &unit in &primary_units[1..] {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        // secondary_units_len + secondary text (UTF-16LE).
        let secondary_units: Vec<u16> =
            secondary.map(|s| s.encode_utf16().collect()).unwrap_or_default();
        let secondary_units_len = u16::try_from(secondary_units.len()).expect("secondary too long");
        data.extend_from_slice(&secondary_units_len.to_le_bytes());
        for &unit in &secondary_units {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        // 4-byte trailer (observed as 0x0000_0000 / 0xFFFF_FFFF).
        data.extend_from_slice(&trailer.to_le_bytes());
        data
    }

    #[test]
    fn indexmark_parse_primary_units_len_eq_one_edge_case() {
        // Wave 12k: 한컴 단일-char primary는 fixture로 관찰되지 않았지만,
        // 파서 산수는 char[0]을 properties에 두고 body[0..0]을 비우는
        // 형태로 그대로 통과한다. 회귀를 막으려면 명시적 단위 테스트가
        // 필요하다 (Codex 검토 권고).
        let data = make_idxm("A", None, 0x0000_0000);
        let parsed = Hwp5IndexMarkControl::parse(IDXM_CTRL_ID, &data)
            .expect("primary_units_len == 1 must decode");
        assert_eq!(parsed.primary, "A");
        assert_eq!(parsed.secondary, None);
    }

    #[test]
    fn indexmark_parse_native_wire_bytes_match() {
        // Reuses the raw native fixture bytes from `wire-native.txt`
        // (entry [012], primary="테스트", no secondary, trailer
        // 0xFFFF_FFFF). The exact-byte assertion catches offset
        // drift better than synthetic-only coverage.
        let mut data = Vec::new();
        data.extend_from_slice(&IDXM_CTRL_ID.to_be_bytes());
        // properties = 0xD14C_0003 (primary_units_len=3, primary[0]="테").
        data.extend_from_slice(&[0x03, 0x00, 0x4C, 0xD1]);
        // body: "스" + "트" + secondary_len=0 + trailer (native pattern).
        data.extend_from_slice(&[0xA4, 0xC2, 0xB8, 0xD2, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF]);
        let parsed = Hwp5IndexMarkControl::parse(IDXM_CTRL_ID, &data)
            .expect("real native idxm bytes must decode");
        assert_eq!(parsed.primary, "테스트");
        assert_eq!(parsed.secondary, None);
    }

    #[test]
    fn indexmark_parse_multi_with_secondary_match() {
        // Round-trip fixture entry [017]: primary="컴퓨터",
        // secondary="하드웨어", trailer 0x0000_0000.
        let mut data = Vec::new();
        data.extend_from_slice(&IDXM_CTRL_ID.to_be_bytes());
        // properties = 0xCEF4_0003 (primary_units_len=3, primary[0]="컴").
        data.extend_from_slice(&[0x03, 0x00, 0xF4, 0xCE]);
        // body: "퓨터" + secondary_len=4 + "하드웨어" + trailer (round-trip pattern).
        data.extend_from_slice(&[
            0xE8, 0xD4, 0x30, 0xD1, 0x04, 0x00, 0x58, 0xD5, 0xDC, 0xB4, 0xE8, 0xC6, 0xB4, 0xC5,
            0x00, 0x00, 0x00, 0x00,
        ]);
        let parsed = Hwp5IndexMarkControl::parse(IDXM_CTRL_ID, &data)
            .expect("real multi idxm bytes must decode");
        assert_eq!(parsed.primary, "컴퓨터");
        assert_eq!(parsed.secondary.as_deref(), Some("하드웨어"));
    }

    #[test]
    fn indexmark_parse_secondary_len_zero_returns_none() {
        let data = make_idxm("AB", Some(""), 0x0000_0000);
        let parsed = Hwp5IndexMarkControl::parse(IDXM_CTRL_ID, &data).expect("decode");
        // Empty-string secondary in our builder is materially the same as
        // secondary_len=0 on the wire — the decoder normalizes it to None.
        assert_eq!(parsed.secondary, None);
    }

    #[test]
    fn indexmark_parse_rejects_primary_units_len_zero() {
        // properties.low = 0 means "no primary"; HWPX `firstKey` is
        // semantic payload that cannot be empty without breaking the
        // index entry. Reject so projection emits a typed warning
        // rather than fabricating an empty key.
        let mut data = Vec::new();
        data.extend_from_slice(&IDXM_CTRL_ID.to_be_bytes());
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // properties = 0
        data.extend_from_slice(&[0x00, 0x00]); // secondary_len = 0
        data.extend_from_slice(&[0; 4]); // trailer
        assert!(Hwp5IndexMarkControl::parse(IDXM_CTRL_ID, &data).is_none());
    }

    #[test]
    fn indexmark_parse_rejects_truncated_trailer() {
        // The 4-byte trailer must be present even though the decoder
        // discards its value — a truncated record means the boundary
        // is no longer trustworthy.
        let data = make_idxm("A", None, 0);
        // Cut off the trailing trailer bytes.
        let truncated = &data[..data.len() - 4];
        assert!(Hwp5IndexMarkControl::parse(IDXM_CTRL_ID, truncated).is_none());
    }

    // -----------------------------------------------------------------------
    // Hwp5ClickHereControl (Wave 12l)
    // -----------------------------------------------------------------------

    const CLK_CTRL_ID: u32 = 0x2563_6C6B;

    /// Builds a synthetic `%clk` CtrlHeader payload mirroring the
    /// observed 한컴 wire layout — see
    /// `.docs/research/2026-06-02_clickhere_wire_dump.md`.
    fn make_clk(hint: &str, help: &str, field_id: u32) -> Vec<u8> {
        let hint_units = hint.encode_utf16().count();
        let help_units = help.encode_utf16().count();
        let command =
            format!("Clickhere:set:0:Direction:wstring:{hint_units}:{hint} HelpState:wstring:{help_units}:{help}  ");
        let command_units: Vec<u16> = command.encode_utf16().collect();

        let mut data = Vec::new();
        data.extend_from_slice(&CLK_CTRL_ID.to_be_bytes());
        data.extend_from_slice(&0x0000_0001u32.to_le_bytes()); // properties
        data.push(0x09); // flag byte
        data.extend_from_slice(&(command_units.len() as u16).to_le_bytes());
        for unit in &command_units {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        data.extend_from_slice(&field_id.to_le_bytes());
        data.extend_from_slice(&[0, 0, 0, 0]); // pad
        data
    }

    /// Builds a synthetic `0x57` field-name sub-record body.
    fn make_clk_name(name: &str) -> Vec<u8> {
        let mut data = vec![0x1B, 0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x40, 0x01, 0x00];
        let units: Vec<u16> = name.encode_utf16().collect();
        data.extend_from_slice(&(units.len() as u16).to_le_bytes());
        for unit in &units {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        data
    }

    #[test]
    fn clickhere_parse_hint_only_carries_hint_and_drops_help() {
        let data = make_clk("이곳에 입력", "", 0x41A4_AD66);
        let parsed =
            Hwp5ClickHereControl::parse(CLK_CTRL_ID, &data).expect("valid %clk must parse");
        assert_eq!(parsed.hint_text.as_deref(), Some("이곳에 입력"));
        assert_eq!(parsed.help_text, None, "empty help must normalize to None");
        assert_eq!(parsed.field_unique_id, 0x41A4_AD66);
        assert_eq!(parsed.name, None, "name is populated only after merge_name_subrecord");
    }

    #[test]
    fn clickhere_parse_hint_and_help_carries_both() {
        let data = make_clk("이메일 입력", "회사 이메일", 1);
        let parsed = Hwp5ClickHereControl::parse(CLK_CTRL_ID, &data).expect("parse");
        assert_eq!(parsed.hint_text.as_deref(), Some("이메일 입력"));
        assert_eq!(parsed.help_text.as_deref(), Some("회사 이메일"));
    }

    #[test]
    fn clickhere_parse_empty_hint_returns_none() {
        // `Direction:wstring:0:` matches an empty hint — decoder must
        // normalize that to `None`, matching the indexmark/dutmal
        // convention (wire cannot distinguish `Some("")` from `None`).
        let data = make_clk("", "", 0);
        let parsed = Hwp5ClickHereControl::parse(CLK_CTRL_ID, &data).expect("parse");
        assert_eq!(parsed.hint_text, None);
        assert_eq!(parsed.help_text, None);
    }

    #[test]
    fn clickhere_parse_hint_with_embedded_colon() {
        // hint contains ':' — length-driven parser must not split on
        // colon (Codex review HIGH).
        let data = make_clk("scheme://host:port", "tip: read this", 0);
        let parsed = Hwp5ClickHereControl::parse(CLK_CTRL_ID, &data).expect("parse");
        assert_eq!(parsed.hint_text.as_deref(), Some("scheme://host:port"));
        assert_eq!(parsed.help_text.as_deref(), Some("tip: read this"));
    }

    #[test]
    fn clickhere_parse_rejects_truncated_command() {
        let mut data = make_clk("hi", "", 0);
        data.truncate(data.len() - 4);
        assert_eq!(
            Hwp5ClickHereControl::parse(CLK_CTRL_ID, &data).unwrap_err(),
            ClickHereParseError::TruncatedCommand,
        );
    }

    #[test]
    fn clickhere_parse_rejects_short_header() {
        // Below the 19-byte fixed minimum — distinct reason from a
        // declared-length overrun (task #89).
        let data = vec![0u8; 18];
        assert_eq!(
            Hwp5ClickHereControl::parse(CLK_CTRL_ID, &data).unwrap_err(),
            ClickHereParseError::TruncatedHeader,
        );
    }

    #[test]
    fn clickhere_parse_rejects_command_over_allocation_cap() {
        // Declared command length above MAX_CLICKHERE_COMMAND_UNITS
        // must be refused before any allocation (tasks #86/#89).
        let mut data = vec![0u8; 8];
        data.push(0x09);
        data.extend_from_slice(&u16::MAX.to_le_bytes());
        data.extend_from_slice(&[0u8; 8]);
        assert_eq!(
            Hwp5ClickHereControl::parse(CLK_CTRL_ID, &data).unwrap_err(),
            ClickHereParseError::CommandTooLong,
        );
    }

    #[test]
    fn clickhere_parse_rejects_non_clickhere_command_syntax() {
        // Structurally valid envelope whose command is not the
        // `Clickhere:set` grammar.
        let mut data = vec![0u8; 8];
        data.push(0x09);
        let command: Vec<u16> = "NotAClickhereCommand".encode_utf16().collect();
        data.extend_from_slice(&(command.len() as u16).to_le_bytes());
        for unit in &command {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        data.extend_from_slice(&[0u8; 8]);
        assert_eq!(
            Hwp5ClickHereControl::parse(CLK_CTRL_ID, &data).unwrap_err(),
            ClickHereParseError::CommandSyntax,
        );
    }

    #[test]
    fn clickhere_name_subrecord_carries_ascii_name() {
        let data = make_clk_name("user_email");
        assert_eq!(
            Hwp5ClickHereControl::parse_name_subrecord(&data),
            ClickHereNameSubrecord::Named("user_email".to_string()),
        );
    }

    #[test]
    fn clickhere_name_subrecord_carries_korean_name() {
        let data = make_clk_name("입력필드");
        assert_eq!(
            Hwp5ClickHereControl::parse_name_subrecord(&data),
            ClickHereNameSubrecord::Named("입력필드".to_string()),
        );
    }

    #[test]
    fn clickhere_name_subrecord_empty_returns_unnamed() {
        // Some("") and None are wire-indistinguishable; length=0
        // normalizes to Unnamed (never `Named("")`).
        let data = make_clk_name("");
        assert_eq!(
            Hwp5ClickHereControl::parse_name_subrecord(&data),
            ClickHereNameSubrecord::Unnamed,
        );
    }

    #[test]
    fn clickhere_name_subrecord_truncated_returns_malformed() {
        // 12-byte header but LL claims 5 chars (10 bytes), data only
        // has the header — must fail.
        let mut data = vec![0x1B, 0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x40, 0x01, 0x00];
        data.extend_from_slice(&5u16.to_le_bytes());
        // intentionally no body bytes
        assert_eq!(
            Hwp5ClickHereControl::parse_name_subrecord(&data),
            ClickHereNameSubrecord::Malformed,
        );
    }

    // ── Wave 12n — auto-field parsers ────────────────────────────────

    /// Builds a `%smr` / `%dte` / `%pat` CtrlHeader payload — they share
    /// the same outer envelope (8 prefix + flag + u16 cmd-len + UTF-16LE
    /// command + 8 trailer).
    fn make_envelope(ctrl_id: u32, properties: u32, command: &str, trailer_id: u32) -> Vec<u8> {
        let units: Vec<u16> = command.encode_utf16().collect();
        let mut data = Vec::new();
        data.extend_from_slice(&ctrl_id.to_be_bytes());
        data.extend_from_slice(&properties.to_le_bytes());
        data.push(0x08); // flag
        data.extend_from_slice(&(units.len() as u16).to_le_bytes());
        for unit in &units {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        data.extend_from_slice(&trailer_id.to_le_bytes());
        data.extend_from_slice(&[0, 0, 0, 0]);
        data
    }

    const SMR_CTRL_ID: u32 = 0x2573_6D72; // "%smr"
    const DTE_CTRL_ID: u32 = 0x2564_7465; // "%dte"
    const PAT_CTRL_ID: u32 = 0x2570_6174; // "%pat"
    const ATNO_CTRL_ID: u32 = 0x6174_6E6F; // "atno"

    #[test]
    fn summery_parse_known_token_carries_command() {
        for token in ["$author", "$lastsaveby", "$createtime", "$modifiedtime", "$title"] {
            let data = make_envelope(SMR_CTRL_ID, 0x0000_0001, token, 0x41A4_AD76);
            let parsed = Hwp5SummeryControl::parse(SMR_CTRL_ID, &data)
                .unwrap_or_else(|| panic!("token {token} must parse"));
            assert_eq!(parsed.command_token, token);
            assert_eq!(parsed.field_unique_id, 0x41A4_AD76);
        }
    }

    #[test]
    fn summery_parse_unknown_token_still_carried() {
        // Forward-compat: parser does not gate on known $X — projection layer
        // routes unknown tokens to Control::UnknownSummery.
        let data = make_envelope(SMR_CTRL_ID, 0x0000_0001, "$company", 0);
        let parsed = Hwp5SummeryControl::parse(SMR_CTRL_ID, &data).expect("parse");
        assert_eq!(parsed.command_token, "$company");
    }

    #[test]
    fn summery_parse_rejects_oversized_command() {
        // command_units > MAX_SUMMERY_COMMAND_UNITS (1024) must return None
        // without attempting allocation.
        let mut data = Vec::new();
        data.extend_from_slice(&SMR_CTRL_ID.to_be_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.push(0x08);
        data.extend_from_slice(&(MAX_SUMMERY_COMMAND_UNITS as u16 + 1).to_le_bytes());
        // intentionally no further bytes
        assert!(Hwp5SummeryControl::parse(SMR_CTRL_ID, &data).is_none());
    }

    #[test]
    fn summery_parse_rejects_truncated() {
        let mut data = make_envelope(SMR_CTRL_ID, 0, "$author", 0);
        data.truncate(data.len() - 4);
        assert!(Hwp5SummeryControl::parse(SMR_CTRL_ID, &data).is_none());
    }

    #[test]
    fn datecode_parse_date_pattern_preserves_raw() {
        let data = make_envelope(DTE_CTRL_ID, 0, "\\:1년 2월 3일 (6);0;", 0x41A4_AD6F);
        let parsed = Hwp5DateCodeControl::parse(DTE_CTRL_ID, &data).expect("parse");
        assert_eq!(parsed.raw_command, "\\:1년 2월 3일 (6);0;");
    }

    #[test]
    fn datecode_parse_time_pattern_starts_with_t() {
        let data = make_envelope(DTE_CTRL_ID, 0, "T\\:;0;", 0);
        let parsed = Hwp5DateCodeControl::parse(DTE_CTRL_ID, &data).expect("parse");
        assert!(parsed.raw_command.starts_with('T'), "time mode wire begins with T");
    }

    #[test]
    fn datecode_parse_carries_trailer_verbatim() {
        let data = make_envelope(DTE_CTRL_ID, 0, "\\:;0;", 0xDEAD_BEEF);
        let parsed = Hwp5DateCodeControl::parse(DTE_CTRL_ID, &data).expect("parse");
        assert_eq!(&parsed.raw_trailer[0..4], &0xDEAD_BEEFu32.to_le_bytes());
        assert_eq!(&parsed.raw_trailer[4..8], &[0, 0, 0, 0]);
    }

    #[test]
    fn datecode_parse_rejects_truncated() {
        let mut data = make_envelope(DTE_CTRL_ID, 0, "\\:;0;", 0);
        data.truncate(data.len() - 2);
        assert!(Hwp5DateCodeControl::parse(DTE_CTRL_ID, &data).is_none());
    }

    // ── Wave 12m Phase 2 Step 2 — Hwp5CrossRefControl tests ──────────

    const XRF_CTRL_ID: u32 = 0x2578_7266; // "%xrf"

    /// Builds a synthetic `%xrf` CtrlHeader payload. Wire envelope:
    /// 4-byte ctrl_id + 4-byte properties + 1-byte flag (0x00 for xrf,
    /// distinct from 0x08 used by %smr/%dte/%pat) + 2-byte u16 cmd-len
    /// + UTF-16LE command + 8-byte trailer (begin_id u32 + field_id u32).
    fn make_xrf_envelope(command: &str, begin_id: u32, field_id: u32) -> Vec<u8> {
        let units: Vec<u16> = command.encode_utf16().collect();
        let mut data = Vec::new();
        data.extend_from_slice(&XRF_CTRL_ID.to_be_bytes()); // ctrl_id
        data.extend_from_slice(&0x0000_0002u32.to_le_bytes()); // properties
        data.push(0x00); // xrf flag is 0x00 (not 0x08)
        data.extend_from_slice(&(units.len() as u16).to_le_bytes());
        for unit in &units {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        data.extend_from_slice(&begin_id.to_le_bytes());
        data.extend_from_slice(&field_id.to_le_bytes());
        data
    }

    #[test]
    fn crossref_parse_bookmark_page_baseline() {
        // Wave 12m Phase 1 fixture #1 (sample-bookmark-page.hwp):
        //   "?target1;6;0;0;0;" → Bookmark + Page, no hyperlink
        let data = make_xrf_envelope("?target1;6;0;0;0;", 0x420D_43BE, 0);
        let parsed = Hwp5CrossRefControl::parse(XRF_CTRL_ID, &data).expect("parse");
        assert_eq!(parsed.command_raw, "?target1;6;0;0;0;");
        assert_eq!(parsed.target_raw, "target1");
        assert_eq!(parsed.ref_type_code, 6); // Bookmark
        assert_eq!(parsed.content_type_code, 0); // Page
        assert_eq!(parsed.hyperlink_code, 0);
        assert_eq!(parsed.param4_raw, 0);
        assert_eq!(parsed.header_flag_raw, 0x00);
        assert_eq!(parsed.trailer_begin_id, 0x420D_43BE);
        assert_eq!(parsed.trailer_field_id, 0);
    }

    #[test]
    fn crossref_parse_all_ref_types_carry_n1() {
        // Wave 12m Phase 1 — all 7 RefType codes observed in 한컴 native
        // fixtures. Parser is RefType-agnostic; semantics applied at
        // projection boundary in smithy-hwp5/src/projection.rs.
        for (label, n1) in [
            ("Table", 0u8),
            ("Figure", 1),
            ("Equation", 2),
            ("Footnote", 3),
            ("Endnote", 4),
            ("Outline", 5),
            ("Bookmark", 6),
        ] {
            let target = if n1 == 6 { "target1".to_string() } else { "#1108165575".to_string() };
            let command = format!("?{target};{n1};0;0;0;");
            let data = make_xrf_envelope(&command, 0, 0);
            let parsed = Hwp5CrossRefControl::parse(XRF_CTRL_ID, &data)
                .unwrap_or_else(|| panic!("{label} (N1={n1}) must parse"));
            assert_eq!(parsed.ref_type_code, n1, "{label} N1 carry");
            assert_eq!(parsed.target_raw, target, "{label} target");
        }
    }

    #[test]
    fn crossref_parse_all_content_types_carry_n2() {
        // ContentType is RefType-relative (Wave 12m Phase 1). Parser
        // only carries the raw u8 — projection interprets.
        for n2 in 0..=3u8 {
            let command = format!("?target1;6;{n2};0;0;");
            let data = make_xrf_envelope(&command, 0, 0);
            let parsed = Hwp5CrossRefControl::parse(XRF_CTRL_ID, &data)
                .unwrap_or_else(|| panic!("N2={n2} must parse"));
            assert_eq!(parsed.content_type_code, n2);
        }
    }

    #[test]
    fn crossref_parse_hyperlink_toggle_carries_n3() {
        // N3 = 0 (no hyperlink) and 1 (hyperlink) — observed in
        // sample-bookmark-page.hwp (N3=0) and
        // sample-bookmark-page-hyperlink.hwp (N3=1).
        for n3 in 0..=1u8 {
            let command = format!("?target1;6;0;{n3};0;");
            let data = make_xrf_envelope(&command, 0, 0);
            let parsed = Hwp5CrossRefControl::parse(XRF_CTRL_ID, &data)
                .unwrap_or_else(|| panic!("N3={n3} must parse"));
            assert_eq!(parsed.hyperlink_code, n3);
        }
    }

    #[test]
    fn crossref_parse_carries_n4_raw() {
        // N4 is currently always 0 in observed fixtures, but parser must
        // preserve any u8 value for forward compatibility.
        let data = make_xrf_envelope("?target1;6;0;0;42;", 0, 0);
        let parsed = Hwp5CrossRefControl::parse(XRF_CTRL_ID, &data).expect("parse");
        assert_eq!(parsed.param4_raw, 42);
    }

    #[test]
    fn crossref_parse_carries_hash_prefix_target() {
        // Footnote/Endnote/Caption/Outline targets use `#<system_id>`
        // form (한컴 자동 생성 ID). Raw form preserved including `#`.
        let data = make_xrf_envelope("?#1108165575;3;0;0;0;", 0, 0);
        let parsed = Hwp5CrossRefControl::parse(XRF_CTRL_ID, &data).expect("parse");
        assert_eq!(parsed.target_raw, "#1108165575");
    }

    #[test]
    fn crossref_parse_carries_trailer_ids() {
        let data = make_xrf_envelope("?target1;6;0;0;0;", 0xDEAD_BEEF, 0xCAFE_F00D);
        let parsed = Hwp5CrossRefControl::parse(XRF_CTRL_ID, &data).expect("parse");
        assert_eq!(parsed.trailer_begin_id, 0xDEAD_BEEF);
        assert_eq!(parsed.trailer_field_id, 0xCAFE_F00D);
    }

    #[test]
    fn crossref_parse_rejects_oversized_command() {
        // CRITICAL: cap (MAX_CROSSREF_COMMAND_UNITS = 1024) is applied
        // BEFORE allocation (Codex(architect) §UTF-16 cap timing).
        let mut data = Vec::new();
        data.extend_from_slice(&XRF_CTRL_ID.to_be_bytes());
        data.extend_from_slice(&0x0000_0002u32.to_le_bytes());
        data.push(0x00);
        data.extend_from_slice(&(MAX_CROSSREF_COMMAND_UNITS as u16 + 1).to_le_bytes());
        // intentionally no further bytes
        assert!(Hwp5CrossRefControl::parse(XRF_CTRL_ID, &data).is_none());
    }

    #[test]
    fn crossref_parse_rejects_truncated() {
        let mut data = make_xrf_envelope("?target1;6;0;0;0;", 0, 0);
        data.truncate(data.len() - 4);
        assert!(Hwp5CrossRefControl::parse(XRF_CTRL_ID, &data).is_none());
    }

    #[test]
    fn crossref_parse_rejects_missing_question_mark() {
        // Command must start with `?`.
        let data = make_xrf_envelope("target1;6;0;0;0;", 0, 0);
        assert!(Hwp5CrossRefControl::parse(XRF_CTRL_ID, &data).is_none());
    }

    #[test]
    fn crossref_parse_rejects_missing_trailing_semicolon() {
        // Command must end with `;` after the last numeric field.
        let data = make_xrf_envelope("?target1;6;0;0;0", 0, 0);
        assert!(Hwp5CrossRefControl::parse(XRF_CTRL_ID, &data).is_none());
    }

    #[test]
    fn crossref_parse_rejects_wrong_field_count() {
        // Only 3 numeric fields instead of expected 4 (target + 4).
        let data = make_xrf_envelope("?target1;6;0;0;", 0, 0);
        assert!(Hwp5CrossRefControl::parse(XRF_CTRL_ID, &data).is_none());
    }

    #[test]
    fn crossref_parse_rejects_non_numeric_field() {
        let data = make_xrf_envelope("?target1;6;abc;0;0;", 0, 0);
        assert!(Hwp5CrossRefControl::parse(XRF_CTRL_ID, &data).is_none());
    }

    #[test]
    fn pathfield_parse_pf_command() {
        let data = make_envelope(PAT_CTRL_ID, 0, "$P$F", 0x41A4_AD7E);
        let parsed = Hwp5PathFieldControl::parse(PAT_CTRL_ID, &data).expect("parse");
        assert_eq!(parsed.raw_command, "$P$F");
    }

    #[test]
    fn pathfield_parse_rejects_oversized() {
        let mut data = Vec::new();
        data.extend_from_slice(&PAT_CTRL_ID.to_be_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.push(0x08);
        data.extend_from_slice(&(MAX_PATHFIELD_COMMAND_UNITS as u16 + 1).to_le_bytes());
        assert!(Hwp5PathFieldControl::parse(PAT_CTRL_ID, &data).is_none());
    }

    #[test]
    fn pathfield_parse_rejects_truncated() {
        let mut data = make_envelope(PAT_CTRL_ID, 0, "$F", 0);
        data.truncate(data.len() - 4);
        assert!(Hwp5PathFieldControl::parse(PAT_CTRL_ID, &data).is_none());
    }

    /// Builds a synthetic `atno` 16-byte CtrlHeader payload.
    fn make_atno(flag: u32) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&ATNO_CTRL_ID.to_be_bytes());
        data.extend_from_slice(&flag.to_le_bytes());
        data.extend_from_slice(&[0x01, 0, 0, 0, 0, 0, 0, 0]);
        data
    }

    #[test]
    fn atno_parse_current_page_flag() {
        let data = make_atno(0x00);
        let parsed = Hwp5InlinePageNumberControl::parse(ATNO_CTRL_ID, &data).expect("parse");
        assert_eq!(parsed.raw_flag, 0x00);
    }

    #[test]
    fn atno_parse_total_pages_flag() {
        let data = make_atno(0x06);
        let parsed = Hwp5InlinePageNumberControl::parse(ATNO_CTRL_ID, &data).expect("parse");
        assert_eq!(parsed.raw_flag, 0x06);
    }

    #[test]
    fn atno_parse_unknown_flag_still_carries_raw() {
        // Forward-compat: unknown flags must surface so the projection
        // layer can preserve them via InlinePageKind::Unknown + raw_flag.
        let data = make_atno(0xABCD_1234);
        let parsed = Hwp5InlinePageNumberControl::parse(ATNO_CTRL_ID, &data).expect("parse");
        assert_eq!(parsed.raw_flag, 0xABCD_1234);
    }

    #[test]
    fn atno_parse_rejects_truncated() {
        let mut data = make_atno(0);
        data.truncate(8);
        assert!(Hwp5InlinePageNumberControl::parse(ATNO_CTRL_ID, &data).is_none());
    }
}
