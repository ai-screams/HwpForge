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
        Ok(Self {
            char_count,
            control_mask,
            para_shape_id,
            style_id,
            line_seg_count,
            char_shape_count,
        })
    }
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
                // 0x0E-0x16: extended controls (bookmarks, change tracking, etc.)
                // All consume 7 extra u16 values. Still silently consumed
                // until a future slice promotes them to a typed variant.
                0x0E..=0x16 => {
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
/// | tail remainder | option / sz_ratio / align / styleIDRef | reserved for fidelity work |
///
/// The first `main_text` char is folded into the same 32-bit word as the
/// length on the wire — packing them together saves a u16 over the more
/// natural "header (len) → body (chars)" layout. Other dutmal options
/// (`align`, `sz_ratio`, `option`, `styleIDRef`) are observed but not
/// promoted to fields yet: every 한컴 fixture we've inspected leaves
/// them at default values, so the encoder writes defaults until a
/// future fixture forces fidelity work. See
/// `.docs/algorithms/2026-06-01_memo_anchor_serialization.md` (general
/// "carry wire metadata only when the source actually populates it"
/// rule).
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
    /// Raw `option` word from the wire. Mirrored verbatim into the
    /// HWPX `<hp:dutmal option=…>` attribute — the precise meaning is
    /// not pinned down (see
    /// `.docs/algorithms/2026-06-01_dutmal_carry.md`), but mirroring it
    /// preserves fidelity round-trip without needing to know.
    pub option_raw: u32,
}

impl Hwp5DutmalControl {
    /// Decodes a `tdut` CtrlHeader payload into a `Hwp5DutmalControl`.
    /// Returns `None` on malformed or truncated payloads — the decoder
    /// falls back to `Hwp5Control::Unknown` so the rest of the section
    /// keeps round-tripping.
    pub(crate) fn parse(ctrl_id: u32, data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        let main_len = u16::from_le_bytes([data[4], data[5]]) as usize;
        if main_len == 0 {
            return None;
        }
        let main_first = u16::from_le_bytes([data[6], data[7]]);

        let mut main_units: Vec<u16> = Vec::with_capacity(main_len);
        main_units.push(main_first);
        let main_tail_bytes = (main_len - 1).checked_mul(2)?;
        let main_tail_end = 8usize.checked_add(main_tail_bytes)?;
        if data.len() < main_tail_end {
            return None;
        }
        for chunk in data[8..main_tail_end].chunks_exact(2) {
            main_units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        let main_text = String::from_utf16(&main_units).ok()?;

        let sub_len_off = main_tail_end;
        if data.len() < sub_len_off + 2 {
            return None;
        }
        let sub_len = u16::from_le_bytes([data[sub_len_off], data[sub_len_off + 1]]) as usize;
        let sub_bytes = sub_len.checked_mul(2)?;
        let sub_off = sub_len_off + 2;
        let sub_end = sub_off.checked_add(sub_bytes)?;
        if data.len() < sub_end {
            return None;
        }
        let sub_units: Vec<u16> = data[sub_off..sub_end]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let sub_text = String::from_utf16(&sub_units).ok()?;

        let pos_type_raw = if data.len() >= sub_end + 4 {
            u32::from_le_bytes([
                data[sub_end],
                data[sub_end + 1],
                data[sub_end + 2],
                data[sub_end + 3],
            ])
        } else {
            0
        };
        // `option_raw` sits at tail offset [8..12] (see
        // `.docs/algorithms/2026-06-01_dutmal_carry.md` for the full tail
        // table). Missing/truncated tails default to 0 — round-trip
        // accuracy degrades to "no option" but the body still carries.
        let option_off = sub_end + 8;
        let option_raw = if data.len() >= option_off + 4 {
            u32::from_le_bytes([
                data[option_off],
                data[option_off + 1],
                data[option_off + 2],
                data[option_off + 3],
            ])
        } else {
            0
        };

        Some(Self { ctrl_id, main_text, sub_text, pos_type_raw, option_raw })
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
}
