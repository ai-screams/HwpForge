//! HWP5 binary record header and tag ID definitions.
//!
//! Every HWP5 binary stream is a sequence of tag-length-value (TLV) records.
//! Each record begins with a 4-byte header encoding the tag ID, level, and
//! data size. This module defines the types for parsing those headers.

use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Read;

use crate::error::{Hwp5Error, Hwp5Result};

/// Parsed HWP5 record header (4 bytes, little-endian packed).
///
/// Bit layout of the 32-bit word:
/// - bits  0–9:  tag ID (10 bits)
/// - bits 10–19: level  (10 bits, nesting depth)
/// - bits 20–31: size   (12 bits; `0xFFF` means extended size follows as next u32)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordHeader {
    /// Raw 10-bit tag identifier.
    pub tag_id: u16,
    /// Nesting depth (0 = top-level).
    pub level: u16,
    /// Byte length of the record's data payload.
    pub size: u32,
}

impl RecordHeader {
    /// Parse a `RecordHeader` from any [`Read`] source.
    ///
    /// Reads 4 bytes (the packed word) and, if `size == 0xFFF`, reads an
    /// additional 4-byte extended size.
    pub fn parse(reader: &mut impl Read) -> Hwp5Result<Self> {
        let word = reader.read_u32::<LittleEndian>().map_err(|e| Hwp5Error::RecordParse {
            offset: 0,
            detail: format!("failed to read record header word: {e}"),
        })?;

        let tag_id = (word & 0x3FF) as u16;
        let level = ((word >> 10) & 0x3FF) as u16;
        let size_field = (word >> 20) & 0xFFF;

        let size = if size_field == 0xFFF {
            reader.read_u32::<LittleEndian>().map_err(|e| Hwp5Error::RecordParse {
                offset: 4,
                detail: format!("failed to read extended size: {e}"),
            })?
        } else {
            size_field
        };

        Ok(Self { tag_id, level, size })
    }
}

/// A complete HWP5 record: header metadata plus the raw data bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Parsed record header.
    pub header: RecordHeader,
    /// Raw data payload (`header.size` bytes).
    pub data: Vec<u8>,
}

impl Record {
    /// Parse a single record (header + data) from a [`Read`] source.
    pub fn parse(reader: &mut impl Read) -> Hwp5Result<Self> {
        let header = RecordHeader::parse(reader)?;
        let mut data = vec![0u8; header.size as usize];
        reader.read_exact(&mut data).map_err(|e| Hwp5Error::RecordParse {
            offset: 0,
            detail: format!("failed to read record data ({} bytes): {e}", header.size),
        })?;
        Ok(Self { header, data })
    }

    /// Parse all records from a [`Read`] source until EOF.
    ///
    /// Returns an empty `Vec` for an empty stream. Any partial record at the
    /// end of the stream is treated as a parse error.
    pub fn parse_stream(reader: &mut impl Read) -> Hwp5Result<Vec<Record>> {
        let mut records = Vec::new();
        // Buffer the entire stream so we can detect EOF cleanly.
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).map_err(|e| Hwp5Error::RecordParse {
            offset: 0,
            detail: format!("failed to read stream: {e}"),
        })?;

        let mut cursor = std::io::Cursor::new(buf);
        loop {
            // Attempt to read the first byte of the next header to detect EOF.
            let pos = cursor.position() as usize;
            let remaining = cursor.get_ref().len() - pos;
            if remaining == 0 {
                break;
            }
            let record = Record::parse(&mut cursor)?;
            records.push(record);
        }
        Ok(records)
    }
}

/// HWP5 tag identifiers for both DocInfo and BodyText record streams.
///
/// # BodyText offset clarification
///
/// The spec documents BodyText tags as `HWPTAG_BEGIN + offset`, where
/// `HWPTAG_BEGIN = 0x10`. For example, `PARA_HEADER = 0x10 + 50 = 0x42`.
/// The values in this enum reflect the actual byte values found in files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TagId {
    // ── DocInfo tags (0x10 – 0x3F) ───────────────────────────────────────
    /// Document-wide properties. `0x10`
    DocumentProperties,
    /// ID mapping table. `0x11`
    IdMappings,
    /// Embedded binary data. `0x12`
    BinData,
    /// Font face name. `0x13`
    FaceName,
    /// Border/fill definition. `0x14`
    BorderFill,
    /// Character shape. `0x15`
    CharShape,
    /// Tab stop definition. `0x16`
    TabDef,
    /// Numbering definition. `0x17`
    NumberingDef,
    /// Bullet definition. `0x18`
    Bullet,
    /// Paragraph shape. `0x19`
    ParaShape,
    /// Named style. `0x1A`
    Style,
    /// Document-level data. `0x1B`
    DocData,
    /// Distribution-document data. `0x1C`
    DistributeDocData,
    /// Compatible document settings. `0x1E`
    CompatibleDocument,
    /// Layout compatibility settings. `0x1F`
    LayoutCompatibility,
    /// Track-change metadata. `0x20`
    TrackChange,
    /// Track-change content. `0x60`
    TrackChangeContent,
    /// Track-change author. `0x61`
    TrackChangeAuthor,

    // ── BodyText tags (0x42 – 0x73, +16 from documented offsets) ─────────
    /// Paragraph header. `0x42`
    ParaHeader,
    /// Paragraph text. `0x43`
    ParaText,
    /// Paragraph character shape indices. `0x44`
    ParaCharShape,
    /// Paragraph line segment info. `0x45`
    ParaLineSeg,
    /// Paragraph range tag. `0x46`
    ParaRangeTag,
    /// Control object header. `0x47`
    CtrlHeader,
    /// List header. `0x48`
    ListHeader,
    /// Page definition. `0x49`
    PageDef,
    /// Footnote/endnote shape. `0x4A`
    FootnoteShape,
    /// Page border/fill. `0x4B`
    PageBorderFill,
    /// Shape component. `0x4C`
    ShapeComponent,
    /// Table object. `0x4D`
    Table,
    /// Line shape component. `0x4E`
    ShapeComponentLine,
    /// Rectangle shape component. `0x4F`
    ShapeComponentRect,
    /// Ellipse shape component. `0x50`
    ShapeComponentEllipse,
    /// Arc shape component. `0x51`
    ShapeComponentArc,
    /// Polygon shape component. `0x52`
    ShapeComponentPolygon,
    /// Curve shape component. `0x53`
    ShapeComponentCurve,
    /// OLE shape component. `0x54`
    ShapeComponentOle,
    /// Picture shape component. `0x55`
    ShapePicture,
    /// Group/container shape component. `0x56`
    ShapeContainer,
    /// Control data. `0x57`
    CtrlData,
    /// Equation editor data. `0x58`
    EqEdit,
    /// TextArt shape. `0x5A`
    ShapeTextArt,
    /// Form object. `0x5B`
    FormObject,
    /// Memo shape. `0x5C`
    MemoShape,
    /// Memo list. `0x5D`
    MemoList,
    /// Chart data. `0x5F`
    ChartData,
    /// Forbidden-character set. `0x5E`
    ForbiddenChar,
    /// Video data. `0x62`
    VideoData,
    /// Unknown shape fallback. `0x73`
    ShapeUnknown,

    /// A tag value not covered by this enum.
    Unknown(u16),
}

impl From<u16> for TagId {
    fn from(value: u16) -> Self {
        match value {
            0x10 => Self::DocumentProperties,
            0x11 => Self::IdMappings,
            0x12 => Self::BinData,
            0x13 => Self::FaceName,
            0x14 => Self::BorderFill,
            0x15 => Self::CharShape,
            0x16 => Self::TabDef,
            0x17 => Self::NumberingDef,
            0x18 => Self::Bullet,
            0x19 => Self::ParaShape,
            0x1A => Self::Style,
            0x1B => Self::DocData,
            0x1C => Self::DistributeDocData,
            0x1E => Self::CompatibleDocument,
            0x1F => Self::LayoutCompatibility,
            0x20 => Self::TrackChange,
            0x5C => Self::MemoShape,
            0x5E => Self::ForbiddenChar,
            0x60 => Self::TrackChangeContent,
            0x61 => Self::TrackChangeAuthor,

            0x42 => Self::ParaHeader,
            0x43 => Self::ParaText,
            0x44 => Self::ParaCharShape,
            0x45 => Self::ParaLineSeg,
            0x46 => Self::ParaRangeTag,
            0x47 => Self::CtrlHeader,
            0x48 => Self::ListHeader,
            0x49 => Self::PageDef,
            0x4A => Self::FootnoteShape,
            0x4B => Self::PageBorderFill,
            0x4C => Self::ShapeComponent,
            0x4D => Self::Table,
            0x4E => Self::ShapeComponentLine,
            0x4F => Self::ShapeComponentRect,
            0x50 => Self::ShapeComponentEllipse,
            0x51 => Self::ShapeComponentArc,
            0x52 => Self::ShapeComponentPolygon,
            0x53 => Self::ShapeComponentCurve,
            0x54 => Self::ShapeComponentOle,
            0x55 => Self::ShapePicture,
            0x56 => Self::ShapeContainer,
            0x57 => Self::CtrlData,
            0x58 => Self::EqEdit,
            0x5A => Self::ShapeTextArt,
            0x5B => Self::FormObject,
            0x5D => Self::MemoList,
            0x5F => Self::ChartData,
            0x62 => Self::VideoData,
            0x73 => Self::ShapeUnknown,

            other => Self::Unknown(other),
        }
    }
}

impl From<TagId> for u16 {
    fn from(tag: TagId) -> Self {
        match tag {
            TagId::DocumentProperties => 0x10,
            TagId::IdMappings => 0x11,
            TagId::BinData => 0x12,
            TagId::FaceName => 0x13,
            TagId::BorderFill => 0x14,
            TagId::CharShape => 0x15,
            TagId::TabDef => 0x16,
            TagId::NumberingDef => 0x17,
            TagId::Bullet => 0x18,
            TagId::ParaShape => 0x19,
            TagId::Style => 0x1A,
            TagId::DocData => 0x1B,
            TagId::DistributeDocData => 0x1C,
            TagId::CompatibleDocument => 0x1E,
            TagId::LayoutCompatibility => 0x1F,
            TagId::TrackChange => 0x20,
            TagId::MemoShape => 0x5C,
            TagId::ForbiddenChar => 0x5E,
            TagId::TrackChangeContent => 0x60,
            TagId::TrackChangeAuthor => 0x61,

            TagId::ParaHeader => 0x42,
            TagId::ParaText => 0x43,
            TagId::ParaCharShape => 0x44,
            TagId::ParaLineSeg => 0x45,
            TagId::ParaRangeTag => 0x46,
            TagId::CtrlHeader => 0x47,
            TagId::ListHeader => 0x48,
            TagId::PageDef => 0x49,
            TagId::FootnoteShape => 0x4A,
            TagId::PageBorderFill => 0x4B,
            TagId::ShapeComponent => 0x4C,
            TagId::Table => 0x4D,
            TagId::ShapeComponentLine => 0x4E,
            TagId::ShapeComponentRect => 0x4F,
            TagId::ShapeComponentEllipse => 0x50,
            TagId::ShapeComponentArc => 0x51,
            TagId::ShapeComponentPolygon => 0x52,
            TagId::ShapeComponentCurve => 0x53,
            TagId::ShapeComponentOle => 0x54,
            TagId::ShapePicture => 0x55,
            TagId::ShapeContainer => 0x56,
            TagId::CtrlData => 0x57,
            TagId::EqEdit => 0x58,
            TagId::ShapeTextArt => 0x5A,
            TagId::FormObject => 0x5B,
            TagId::MemoList => 0x5D,
            TagId::ChartData => 0x5F,
            TagId::VideoData => 0x62,
            TagId::ShapeUnknown => 0x73,

            TagId::Unknown(v) => v,
        }
    }
}

impl TagId {
    /// Returns `true` if this tag belongs to the DocInfo stream (0x10–0x3F).
    pub fn is_doc_info(self) -> bool {
        let v = u16::from(self);
        (0x10..=0x3F).contains(&v)
    }

    /// Returns `true` if this tag belongs to a BodyText stream (0x42–0x73).
    pub fn is_body_text(self) -> bool {
        let v = u16::from(self);
        (0x42..=0x73).contains(&v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parse_record_header_basic() {
        // tag=0x10(16), level=0, size=100
        let word: u32 = 16 | (100 << 20);
        let bytes = word.to_le_bytes();
        let mut cursor = Cursor::new(&bytes[..]);
        let header = RecordHeader::parse(&mut cursor).unwrap();
        assert_eq!(header.tag_id, 16);
        assert_eq!(header.level, 0);
        assert_eq!(header.size, 100);
    }

    #[test]
    fn parse_record_header_with_level() {
        let word: u32 = 0x42 | (3 << 10) | (50 << 20);
        let bytes = word.to_le_bytes();
        let mut cursor = Cursor::new(&bytes[..]);
        let header = RecordHeader::parse(&mut cursor).unwrap();
        assert_eq!(header.tag_id, 0x42);
        assert_eq!(header.level, 3);
        assert_eq!(header.size, 50);
    }

    #[test]
    fn parse_record_header_extended_size() {
        let word: u32 = 16 | (0xFFF << 20);
        let extended_size: u32 = 50_000;
        let mut buf = Vec::new();
        buf.extend_from_slice(&word.to_le_bytes());
        buf.extend_from_slice(&extended_size.to_le_bytes());
        let mut cursor = Cursor::new(&buf[..]);
        let header = RecordHeader::parse(&mut cursor).unwrap();
        assert_eq!(header.size, 50_000);
    }

    #[test]
    fn parse_record_header_max_values() {
        let word: u32 = 0x3FF | (0x3FF << 10) | (4094 << 20);
        let bytes = word.to_le_bytes();
        let mut cursor = Cursor::new(&bytes[..]);
        let header = RecordHeader::parse(&mut cursor).unwrap();
        assert_eq!(header.tag_id, 0x3FF);
        assert_eq!(header.level, 0x3FF);
        assert_eq!(header.size, 4094);
    }

    #[test]
    fn parse_record_with_data() {
        let word: u32 = 0x10 | (5 << 20);
        let mut buf = Vec::new();
        buf.extend_from_slice(&word.to_le_bytes());
        buf.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE]);
        let mut cursor = Cursor::new(&buf[..]);
        let record = Record::parse(&mut cursor).unwrap();
        assert_eq!(record.header.tag_id, 0x10);
        assert_eq!(record.data, vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE]);
    }

    #[test]
    fn parse_record_stream_multiple() {
        let mut buf = Vec::new();
        // Record 1: tag=0x10, level=0, size=2, data=[0x01, 0x02]
        let word1: u32 = 0x10 | (2 << 20);
        buf.extend_from_slice(&word1.to_le_bytes());
        buf.extend_from_slice(&[0x01, 0x02]);
        // Record 2: tag=0x13, level=0, size=3, data=[0x03, 0x04, 0x05]
        let word2: u32 = 0x13 | (3 << 20);
        buf.extend_from_slice(&word2.to_le_bytes());
        buf.extend_from_slice(&[0x03, 0x04, 0x05]);
        let mut cursor = Cursor::new(&buf[..]);
        let records = Record::parse_stream(&mut cursor).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].header.tag_id, 0x10);
        assert_eq!(records[1].header.tag_id, 0x13);
    }

    #[test]
    fn tag_id_from_u16_doc_info() {
        assert_eq!(TagId::from(0x10), TagId::DocumentProperties);
        assert_eq!(TagId::from(0x11), TagId::IdMappings);
        assert_eq!(TagId::from(0x13), TagId::FaceName);
        assert_eq!(TagId::from(0x15), TagId::CharShape);
        assert_eq!(TagId::from(0x18), TagId::Bullet);
        assert_eq!(TagId::from(0x19), TagId::ParaShape);
        assert_eq!(TagId::from(0x1A), TagId::Style);
        assert_eq!(TagId::from(0x1E), TagId::CompatibleDocument);
    }

    #[test]
    fn tag_id_from_u16_body_text() {
        assert_eq!(TagId::from(0x42), TagId::ParaHeader);
        assert_eq!(TagId::from(0x43), TagId::ParaText);
        assert_eq!(TagId::from(0x44), TagId::ParaCharShape);
        assert_eq!(TagId::from(0x47), TagId::CtrlHeader);
        assert_eq!(TagId::from(0x4B), TagId::PageBorderFill);
        assert_eq!(TagId::from(0x4D), TagId::Table);
        assert_eq!(TagId::from(0x55), TagId::ShapePicture);
    }

    #[test]
    fn tag_id_unknown() {
        assert_eq!(TagId::from(0xFF), TagId::Unknown(0xFF));
        assert_eq!(TagId::from(0x00), TagId::Unknown(0x00));
    }

    #[test]
    fn tag_id_roundtrip() {
        for raw in [0x10u16, 0x13, 0x15, 0x19, 0x1E, 0x42, 0x43, 0x4D, 0x55, 0x73] {
            let tag = TagId::from(raw);
            assert_eq!(u16::from(tag), raw);
        }
    }

    #[test]
    fn tag_id_is_doc_info() {
        assert!(TagId::DocumentProperties.is_doc_info());
        assert!(TagId::CharShape.is_doc_info());
        assert!(!TagId::ParaHeader.is_doc_info());
        assert!(!TagId::Unknown(0xFF).is_doc_info());
    }

    #[test]
    fn tag_id_is_body_text() {
        assert!(TagId::ParaHeader.is_body_text());
        assert!(TagId::Table.is_body_text());
        assert!(TagId::ShapeUnknown.is_body_text());
        assert!(!TagId::DocumentProperties.is_body_text());
    }
}
