//! `DocInfo` stream decoder for HWP5.
//!
//! Parses the `DocInfo` binary stream into style definitions:
//! font tables, character property arrays, paragraph property arrays,
//! and named paragraph styles.

use crate::decoder::Hwp5Warning;
use crate::error::Hwp5Result;
use crate::schema::border_fill::Hwp5RawBorderFill;
use crate::schema::header::{
    Hwp5RawCharShape, Hwp5RawFaceName, Hwp5RawIdMappings, Hwp5RawParaShape, Hwp5RawStyle,
    HwpVersion,
};
use crate::schema::record::{Record, TagId};

/// Result of parsing the DocInfo stream.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5DocInfoBorderFillSlot {
    /// 1-based border fill ID preserved from DocInfo record order.
    pub id: u32,
    /// Parsed border fill payload. `None` means the slot existed but parse failed.
    pub fill: Option<Hwp5RawBorderFill>,
}

/// Result of parsing the DocInfo stream.
#[derive(Debug)]
pub(crate) struct DocInfoResult {
    /// Parsed `IdMappings` record, if present.
    pub id_mappings: Option<Hwp5RawIdMappings>,
    /// Font face name records.
    pub fonts: Vec<Hwp5RawFaceName>,
    /// Character shape records.
    pub char_shapes: Vec<Hwp5RawCharShape>,
    /// Paragraph shape records.
    pub para_shapes: Vec<Hwp5RawParaShape>,
    /// Named style records.
    pub styles: Vec<Hwp5RawStyle>,
    /// Border/fill record slots preserved in DocInfo order.
    pub border_fills: Vec<Hwp5DocInfoBorderFillSlot>,
    /// Non-fatal warnings encountered during parsing.
    pub warnings: Vec<Hwp5Warning>,
}

/// Parse the decompressed `DocInfo` stream bytes into style definitions.
///
/// Iterates every record in the stream and dispatches by tag ID. Unknown or
/// unprocessable records are recorded as [`Hwp5Warning::UnsupportedTag`]
/// warnings rather than hard errors so that partially-supported files can
/// still be read.
///
/// # Errors
///
/// Returns an error only if the raw byte stream cannot be parsed as a valid
/// sequence of HWP5 records (i.e., the byte layout is corrupt).
pub(crate) fn parse_doc_info(data: &[u8], _version: &HwpVersion) -> Hwp5Result<DocInfoResult> {
    let records = Record::parse_stream(&mut std::io::Cursor::new(data))?;

    let mut result = DocInfoResult {
        id_mappings: None,
        fonts: Vec::new(),
        char_shapes: Vec::new(),
        para_shapes: Vec::new(),
        styles: Vec::new(),
        border_fills: Vec::new(),
        warnings: Vec::new(),
    };

    for record in &records {
        let tag = TagId::from(record.header.tag_id);
        match tag {
            TagId::IdMappings => match Hwp5RawIdMappings::parse(&record.data) {
                Ok(m) => result.id_mappings = Some(m),
                Err(_) => result
                    .warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::FaceName => match Hwp5RawFaceName::parse(&record.data) {
                Ok(f) => result.fonts.push(f),
                Err(_) => result
                    .warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::CharShape => match Hwp5RawCharShape::parse(&record.data) {
                Ok(cs) => result.char_shapes.push(cs),
                Err(_) => result
                    .warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::ParaShape => match Hwp5RawParaShape::parse(&record.data) {
                Ok(ps) => result.para_shapes.push(ps),
                Err(_) => result
                    .warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::Style => match Hwp5RawStyle::parse(&record.data) {
                Ok(s) => result.styles.push(s),
                Err(_) => result
                    .warnings
                    .push(Hwp5Warning::UnsupportedTag { tag_id: record.header.tag_id, offset: 0 }),
            },
            TagId::BorderFill => {
                let id = (result.border_fills.len() as u32) + 1;
                match Hwp5RawBorderFill::parse(&record.data) {
                    Ok(fill) => {
                        result.border_fills.push(Hwp5DocInfoBorderFillSlot { id, fill: Some(fill) })
                    }
                    Err(_) => {
                        result.border_fills.push(Hwp5DocInfoBorderFillSlot { id, fill: None });
                        result.warnings.push(Hwp5Warning::UnsupportedTag {
                            tag_id: record.header.tag_id,
                            offset: 0,
                        });
                    }
                }
            }
            TagId::Unknown(id) => {
                result.warnings.push(Hwp5Warning::UnsupportedTag { tag_id: id, offset: 0 });
            }
            // Other known tags (DocumentProperties, BinData, TabDef, etc.) — silently skip.
            _ => {}
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build raw bytes for a single record (4-byte header + data).
    fn make_record_bytes(tag_id: u16, data: &[u8]) -> Vec<u8> {
        let size = data.len() as u32;
        let word = if size >= 0xFFF {
            tag_id as u32 | (0xFFF << 20)
        } else {
            tag_id as u32 | (size << 20)
        };
        let mut buf = Vec::new();
        buf.extend_from_slice(&word.to_le_bytes());
        if size >= 0xFFF {
            buf.extend_from_slice(&size.to_le_bytes());
        }
        buf.extend_from_slice(data);
        buf
    }

    fn make_id_mappings_data() -> Vec<u8> {
        let mut data = vec![0u8; 60];
        // hangul_font_count = 1 at offset 4
        data[4..8].copy_from_slice(&1i32.to_le_bytes());
        // char_shape_count = 1 at offset 36
        data[36..40].copy_from_slice(&1i32.to_le_bytes());
        // para_shape_count = 1 at offset 52
        data[52..56].copy_from_slice(&1i32.to_le_bytes());
        // style_count = 1 at offset 56
        data[56..60].copy_from_slice(&1i32.to_le_bytes());
        data
    }

    fn make_face_name_data(name: &str) -> Vec<u8> {
        let mut data = vec![0u8]; // property byte
        let utf16: Vec<u16> = name.encode_utf16().collect();
        data.extend_from_slice(&(utf16.len() as u16).to_le_bytes());
        for &ch in &utf16 {
            data.extend_from_slice(&ch.to_le_bytes());
        }
        data
    }

    fn make_char_shape_data() -> Vec<u8> {
        let mut data = vec![0u8; 68];
        // height = 1000 (10pt) at offset 42
        data[42..46].copy_from_slice(&1000i32.to_le_bytes());
        data
    }

    fn make_para_shape_data() -> Vec<u8> {
        vec![0u8; 42]
    }

    fn make_style_data(name: &str, eng_name: &str) -> Vec<u8> {
        let mut data = Vec::new();
        let name_u16: Vec<u16> = name.encode_utf16().collect();
        data.extend_from_slice(&(name_u16.len() as u16).to_le_bytes());
        for &ch in &name_u16 {
            data.extend_from_slice(&ch.to_le_bytes());
        }
        let eng_u16: Vec<u16> = eng_name.encode_utf16().collect();
        data.extend_from_slice(&(eng_u16.len() as u16).to_le_bytes());
        for &ch in &eng_u16 {
            data.extend_from_slice(&ch.to_le_bytes());
        }
        data.push(0); // kind
        data.push(0); // next_style_id
        data.extend_from_slice(&0i16.to_le_bytes()); // lang_id
        data.extend_from_slice(&0u16.to_le_bytes()); // para_shape_id
        data.extend_from_slice(&0u16.to_le_bytes()); // char_shape_id
        data.extend_from_slice(&0u16.to_le_bytes()); // lock_form
        data
    }

    #[test]
    fn parse_doc_info_empty() {
        let result = parse_doc_info(&[], &HwpVersion::new(5, 0, 2, 5)).unwrap();
        assert!(result.id_mappings.is_none());
        assert!(result.fonts.is_empty());
    }

    #[test]
    fn parse_doc_info_with_id_mappings() {
        let mut stream = Vec::new();
        stream.extend(make_record_bytes(0x11, &make_id_mappings_data()));
        let result = parse_doc_info(&stream, &HwpVersion::new(5, 0, 2, 5)).unwrap();
        assert!(result.id_mappings.is_some());
        let m = result.id_mappings.unwrap();
        assert_eq!(m.hangul_font_count, 1);
        assert_eq!(m.char_shape_count, 1);
    }

    #[test]
    fn parse_doc_info_with_font() {
        let mut stream = Vec::new();
        stream.extend(make_record_bytes(0x13, &make_face_name_data("바탕")));
        let result = parse_doc_info(&stream, &HwpVersion::new(5, 0, 2, 5)).unwrap();
        assert_eq!(result.fonts.len(), 1);
        assert_eq!(result.fonts[0].face_name, "바탕");
    }

    #[test]
    fn parse_doc_info_preserves_border_fill_slots_when_middle_record_fails() {
        let mut valid_fill = Vec::new();
        valid_fill.extend_from_slice(&0u16.to_le_bytes());
        for _ in 0..5 {
            valid_fill.push(1);
            valid_fill.push(1);
            valid_fill.extend_from_slice(&0u32.to_le_bytes());
        }
        valid_fill.extend_from_slice(&0u32.to_le_bytes());
        valid_fill.extend_from_slice(&0u32.to_le_bytes());

        let invalid_fill = vec![0u8; 8];

        let mut stream = Vec::new();
        stream.extend(make_record_bytes(0x14, &valid_fill));
        stream.extend(make_record_bytes(0x14, &invalid_fill));
        stream.extend(make_record_bytes(0x14, &valid_fill));

        let result = parse_doc_info(&stream, &HwpVersion::new(5, 0, 2, 5)).unwrap();
        assert_eq!(result.border_fills.len(), 3);
        assert_eq!(result.border_fills[0].id, 1);
        assert!(result.border_fills[0].fill.is_some());
        assert_eq!(result.border_fills[1].id, 2);
        assert!(result.border_fills[1].fill.is_none());
        assert_eq!(result.border_fills[2].id, 3);
        assert!(result.border_fills[2].fill.is_some());
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn parse_doc_info_full() {
        let mut stream = Vec::new();
        stream.extend(make_record_bytes(0x11, &make_id_mappings_data()));
        stream.extend(make_record_bytes(0x13, &make_face_name_data("바탕")));
        stream.extend(make_record_bytes(0x15, &make_char_shape_data()));
        stream.extend(make_record_bytes(0x19, &make_para_shape_data()));
        stream.extend(make_record_bytes(0x1A, &make_style_data("본문", "Body")));
        let result = parse_doc_info(&stream, &HwpVersion::new(5, 0, 2, 5)).unwrap();
        assert!(result.id_mappings.is_some());
        assert_eq!(result.fonts.len(), 1);
        assert_eq!(result.char_shapes.len(), 1);
        assert_eq!(result.para_shapes.len(), 1);
        assert_eq!(result.styles.len(), 1);
        assert_eq!(result.styles[0].name, "본문");
    }

    #[test]
    fn parse_doc_info_unknown_tags_produce_warnings() {
        let mut stream = Vec::new();
        stream.extend(make_record_bytes(0xFF, &[0x01, 0x02])); // unknown tag
        let result = parse_doc_info(&stream, &HwpVersion::new(5, 0, 2, 5)).unwrap();
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn parse_doc_info_multiple_fonts() {
        let mut stream = Vec::new();
        stream.extend(make_record_bytes(0x13, &make_face_name_data("바탕")));
        stream.extend(make_record_bytes(0x13, &make_face_name_data("돋움")));
        stream.extend(make_record_bytes(0x13, &make_face_name_data("굴림")));
        let result = parse_doc_info(&stream, &HwpVersion::new(5, 0, 2, 5)).unwrap();
        assert_eq!(result.fonts.len(), 3);
        assert_eq!(result.fonts[0].face_name, "바탕");
        assert_eq!(result.fonts[1].face_name, "돋움");
        assert_eq!(result.fonts[2].face_name, "굴림");
    }
}
