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
    Tab,
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
    /// Field end marker (U+0004).
    FieldEnd,
    /// Non-breaking space (U+001E).
    NonBreakingSpace,
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
        // HWP5 "extended" control characters (0x01-0x03, 0x0B-0x0C, 0x0E-0x17)
        // occupy 8 wchars total: the control code itself plus 7 extra u16 values.
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
                // Reserved — skip silently (single-wchar controls).
                0x00 | 0x05 | 0x06 | 0x07 | 0x08 => {}

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
                // 0x0E-0x17: extended controls (bookmarks, change tracking, etc.)
                // All consume 7 extra u16 values.
                0x0E..=0x17 => {
                    flush_text!();
                    let _extra = read_extra!(i - 1);
                    // No segment emitted — consumed silently.
                }

                // Single-wchar control chars.
                0x04 => {
                    flush_text!();
                    segments.push(TextSegment::FieldEnd);
                }
                0x09 => {
                    flush_text!();
                    segments.push(TextSegment::Tab);
                }
                0x0A => {
                    flush_text!();
                    segments.push(TextSegment::LineBreak);
                }
                0x0D => {
                    flush_text!();
                    segments.push(TextSegment::ParaBreak);
                }
                // 0x18 is "keep" (word-joiner), 0x1F is "optional hyphen" —
                // both are single-char control codes, skip them.
                0x18 | 0x1F => {
                    flush_text!();
                    // No segment emitted — consumed silently.
                }
                0x1E => {
                    flush_text!();
                    segments.push(TextSegment::NonBreakingSpace);
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
        data.extend_from_slice(&cp_bytes(0x09));
        data.extend_from_slice(&utf16le("B"));
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(
            pt.segments,
            vec![TextSegment::Text("A".into()), TextSegment::Tab, TextSegment::Text("B".into()),]
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
        let data = cp_bytes(0x04);
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments, vec![TextSegment::FieldEnd]);
    }

    #[test]
    fn para_text_non_breaking_space() {
        let data = cp_bytes(0x1E);
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(pt.segments, vec![TextSegment::NonBreakingSpace]);
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
        // Codes 0x00, 0x05-0x08 should not produce segments (single-wchar skips).
        let mut data = Vec::new();
        for cp in [0x00u16, 0x05, 0x06, 0x07, 0x08] {
            data.extend_from_slice(&cp.to_le_bytes());
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
        data.extend_from_slice(&cp_bytes(0x09)); // tab
        data.extend_from_slice(&utf16le("there"));
        data.extend_from_slice(&cp_bytes(0x0D)); // para break
        let pt = Hwp5ParaText::parse(&data).unwrap();
        assert_eq!(
            pt.segments,
            vec![
                TextSegment::Text("hi".into()),
                TextSegment::Tab,
                TextSegment::Text("there".into()),
                TextSegment::ParaBreak,
            ]
        );
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
