//! `DocInfo` stream decoder for HWP5.
//!
//! Parses the `DocInfo` binary stream into style definitions:
//! font tables, character property arrays, paragraph property arrays,
//! and named paragraph styles.

use crate::decoder::Hwp5Warning;
use crate::error::Hwp5Result;
use crate::schema::border_fill::Hwp5RawBorderFill;
use crate::schema::header::{
    Hwp5RawBulletDef, Hwp5RawCharShape, Hwp5RawFaceName, Hwp5RawIdMappings, Hwp5RawNumberingDef,
    Hwp5RawParaShape, Hwp5RawStyle, Hwp5RawTabDef, Hwp5TabDefSlot, HwpVersion,
};
use crate::schema::record::{Record, TagId};

/// Result of parsing a border-fill definition slot from DocInfo.
#[derive(Debug, Clone)]
pub struct Hwp5DocInfoBorderFillSlot {
    /// 1-based border fill ID preserved from DocInfo record order.
    pub id: u32,
    /// Parsed border fill payload. `None` means the slot existed but parse failed.
    pub fill: Option<Hwp5RawBorderFill>,
}

/// Result of parsing a numbering definition slot from DocInfo.
#[derive(Debug, Clone)]
pub struct Hwp5DocInfoNumberingSlot {
    /// 1-based numbering ID preserved from DocInfo record order.
    pub id: u32,
    /// Parsed numbering payload. `None` means the slot existed but parse failed.
    pub numbering: Option<Hwp5RawNumberingDef>,
}

/// Result of parsing a bullet definition slot from DocInfo.
#[derive(Debug, Clone)]
pub struct Hwp5DocInfoBulletSlot {
    /// 1-based bullet ID preserved from DocInfo record order.
    pub id: u32,
    /// Raw bullet payload. `None` means the slot existed but the parser had to
    /// drop the record.
    pub bullet: Option<Hwp5RawBulletDef>,
}

/// Result of parsing the DocInfo stream.
///
/// This is the HWP5-native style snapshot. Output-format orchestrators consume
/// it (via [`crate::style_store::Hwp5StyleStore::from_doc_info`]) when mapping
/// HWP5 styles onto a target format.
#[derive(Debug)]
pub struct DocInfoResult {
    /// Parsed `IdMappings` record, if present.
    pub id_mappings: Option<Hwp5RawIdMappings>,
    /// Font face name records.
    pub fonts: Vec<Hwp5RawFaceName>,
    /// Character shape records.
    pub char_shapes: Vec<Hwp5RawCharShape>,
    /// Paragraph shape records.
    pub para_shapes: Vec<Hwp5RawParaShape>,
    /// Numbering definition slots preserved in DocInfo order.
    pub numberings: Vec<Hwp5DocInfoNumberingSlot>,
    /// Bullet definition slots preserved in DocInfo order.
    pub bullets: Vec<Hwp5DocInfoBulletSlot>,
    /// Tab definition slots preserved in DocInfo order.
    pub tab_defs: Vec<Hwp5TabDefSlot>,
    /// Named style records.
    pub styles: Vec<Hwp5RawStyle>,
    /// Border/fill record slots preserved in DocInfo order.
    pub border_fills: Vec<Hwp5DocInfoBorderFillSlot>,
    /// Non-fatal warnings encountered during parsing.
    pub warnings: Vec<Hwp5Warning>,
}

/// Parse the decompressed `DocInfo` stream bytes into style definitions.
///
/// Iterates every record in the stream and dispatches by tag ID. Unknown tags
/// become [`Hwp5Warning::UnsupportedTag`], while malformed-but-known records
/// degrade into more specific fallback warnings so partially-supported files
/// can still be read without silently reindexing raw slots.
///
/// # Errors
///
/// Returns an error if the record stream or an IdMappings declaration is
/// malformed, or if multiple IdMappings declarations are present.
pub(crate) fn parse_doc_info(data: &[u8], _version: &HwpVersion) -> Hwp5Result<DocInfoResult> {
    let records = Record::parse_stream(&mut std::io::Cursor::new(data))?;

    let mut result = DocInfoResult {
        id_mappings: None,
        fonts: Vec::new(),
        char_shapes: Vec::new(),
        para_shapes: Vec::new(),
        numberings: Vec::new(),
        bullets: Vec::new(),
        tab_defs: Vec::new(),
        styles: Vec::new(),
        border_fills: Vec::new(),
        warnings: Vec::new(),
    };

    for record in &records {
        let tag = TagId::from(record.header.tag_id);
        match tag {
            TagId::IdMappings => {
                if result.id_mappings.is_some() {
                    return Err(crate::error::Hwp5Error::RecordParse {
                        offset: 0,
                        detail: "duplicate IdMappings declaration".into(),
                    });
                }
                result.id_mappings = Some(Hwp5RawIdMappings::parse(&record.data)?);
            }
            TagId::FaceName => {
                let slot = result.fonts.len();
                let value = Hwp5RawFaceName::parse(&record.data).unwrap_or_else(|err| {
                    // 이것을 실패시키는 것: 실패한 정의를 건너뛰어 뒤 슬롯 당기기.
                    result.warnings.push(Hwp5Warning::ParserFallback {
                        subject: "fonts.slot",
                        reason: format!("FaceName slot {slot} failed to parse: {err}; references to this slot use an empty placeholder, later IDs are unchanged"),
                    });
                    Hwp5RawFaceName::default()
                });
                result.fonts.push(value);
            }
            TagId::CharShape => {
                let slot = result.char_shapes.len();
                let value = Hwp5RawCharShape::parse(&record.data).unwrap_or_else(|err| {
                    // 이것을 실패시키는 것: 실패한 정의를 건너뛰어 뒤 슬롯 당기기.
                    result.warnings.push(Hwp5Warning::ParserFallback {
                        subject: "char_shapes.slot",
                        reason: format!("CharShape slot {slot} failed to parse: {err}; references to this slot use an empty placeholder, later IDs are unchanged"),
                    });
                    Hwp5RawCharShape::default()
                });
                result.char_shapes.push(value);
            }
            TagId::ParaShape => {
                let slot = result.para_shapes.len();
                let value = Hwp5RawParaShape::parse(&record.data).unwrap_or_else(|err| {
                    // 이것을 실패시키는 것: 실패한 정의를 건너뛰어 뒤 슬롯 당기기.
                    result.warnings.push(Hwp5Warning::ParserFallback {
                        subject: "para_shapes.slot",
                        reason: format!("ParaShape slot {slot} failed to parse: {err}; references to this slot use an empty placeholder, later IDs are unchanged"),
                    });
                    Hwp5RawParaShape::default()
                });
                result.para_shapes.push(value);
            }
            TagId::NumberingDef => match Hwp5RawNumberingDef::parse(&record.data, _version) {
                Ok(numbering) => {
                    let id = (result.numberings.len() as u32) + 1;
                    result
                        .numberings
                        .push(Hwp5DocInfoNumberingSlot { id, numbering: Some(numbering) });
                }
                Err(err) => {
                    let id = (result.numberings.len() as u32) + 1;
                    result.numberings.push(Hwp5DocInfoNumberingSlot { id, numbering: None });
                    result.warnings.push(Hwp5Warning::ParserFallback {
                        subject: "numbering.parse",
                        reason: format!("numbering slot {id} could not be parsed: {err}"),
                    });
                }
            },
            TagId::Bullet => {
                let id = (result.bullets.len() as u32) + 1;
                match Hwp5RawBulletDef::parse(&record.data) {
                    Ok(bullet) => {
                        result.bullets.push(Hwp5DocInfoBulletSlot { id, bullet: Some(bullet) });
                    }
                    Err(err) => {
                        result.bullets.push(Hwp5DocInfoBulletSlot { id, bullet: None });
                        result.warnings.push(Hwp5Warning::ParserFallback {
                            subject: "bullet.parse",
                            reason: format!("bullet slot {id} could not be parsed: {err}"),
                        });
                    }
                }
            }
            TagId::TabDef => match Hwp5RawTabDef::parse(&record.data) {
                Ok(tab_def) => {
                    let raw_id = result.tab_defs.len() as u32;
                    result.tab_defs.push(Hwp5TabDefSlot::parsed(raw_id, tab_def));
                }
                Err(err) => {
                    let raw_id = result.tab_defs.len() as u32;
                    result.tab_defs.push(Hwp5TabDefSlot::invalid(raw_id));
                    result.warnings.push(Hwp5Warning::ParserFallback {
                        subject: "tab_def.parse",
                        reason: format!("tab definition slot {raw_id} could not be parsed: {err}"),
                    });
                }
            },
            TagId::Style => {
                let slot = result.styles.len();
                let value = Hwp5RawStyle::parse(&record.data).unwrap_or_else(|err| {
                    // 이것을 실패시키는 것: 실패한 정의를 건너뛰어 뒤 슬롯 당기기.
                    result.warnings.push(Hwp5Warning::ParserFallback {
                        subject: "styles.slot",
                        reason: format!("Style slot {slot} failed to parse: {err}; references to this slot use an empty placeholder, later IDs are unchanged"),
                    });
                    Hwp5RawStyle::default()
                });
                result.styles.push(value);
            }
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
            TagId::MemoShape | TagId::TrackChangeContent | TagId::TrackChangeAuthor => {
                result.warnings.push(Hwp5Warning::ParserFallback {
                    subject: "header.unsupported_definition",
                    reason: format!("{tag:?} definition and its references are not projected; original record counted for IdMappings"),
                });
            }
            TagId::Unknown(id) => {
                result.warnings.push(Hwp5Warning::UnsupportedTag { tag_id: id, offset: 0 });
            }
            // Other known tags (DocumentProperties, BinData, etc.) — silently skip.
            _ => {}
        }
    }

    if let Some(m) = &result.id_mappings {
        // Count raw records, including malformed slots and unsupported definitions.
        // A mismatch never turns a present declaration into an absent one.
        for (kind, declared, tag) in [
            ("BinData", Some(i64::from(m.bin_data_count)), TagId::BinData),
            (
                "FaceName",
                Some(
                    i64::from(m.hangul_font_count)
                        + i64::from(m.english_font_count)
                        + i64::from(m.hanja_font_count)
                        + i64::from(m.japanese_font_count)
                        + i64::from(m.other_font_count)
                        + i64::from(m.symbol_font_count)
                        + i64::from(m.user_font_count),
                ),
                TagId::FaceName,
            ),
            ("borderFill", Some(i64::from(m.border_fill_count)), TagId::BorderFill),
            ("charPr", Some(i64::from(m.char_shape_count)), TagId::CharShape),
            ("tabPr", Some(i64::from(m.tab_def_count)), TagId::TabDef),
            ("numbering", Some(i64::from(m.numbering_def_count)), TagId::NumberingDef),
            ("bullet", Some(i64::from(m.bullet_def_count)), TagId::Bullet),
            ("paraPr", Some(i64::from(m.para_shape_count)), TagId::ParaShape),
            ("style", Some(i64::from(m.style_count)), TagId::Style),
            ("memoShape", m.memo_shape_count.map(i64::from), TagId::MemoShape),
            ("trackChange", m.change_tracking_count.map(i64::from), TagId::TrackChangeContent),
            (
                "trackChangeAuthor",
                m.change_tracking_author_count.map(i64::from),
                TagId::TrackChangeAuthor,
            ),
        ] {
            if let Some(declared) = declared {
                let actual = records.iter().filter(|r| TagId::from(r.header.tag_id) == tag).count();
                if usize::try_from(declared).ok() != Some(actual) {
                    result.warnings.push(Hwp5Warning::ParserFallback {
                        subject: "IdMappings.count",
                        reason: format!("IdMappings {kind}: declared {declared}, actual {actual}; preserving raw definition slots and declaration"),
                    });
                }
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fix4_id_mappings_damage_and_every_count_are_visible() {
        // 이것을 실패시키는 것: 손상 선언을 None으로 대체, optional 꼬리 절단 허용,
        // 중복 선언 덮어쓰기, 어느 정의 종류든 카운터 불일치 경고 생략.
        let version = HwpVersion::new(5, 0, 3, 2);
        for len in [59, 61, 62, 63, 65, 66, 67, 69, 70, 71] {
            let bytes = make_record_bytes(0x11, &vec![0; len]);
            assert!(parse_doc_info(&bytes, &version).is_err(), "len {len}");
        }
        let declaration = make_record_bytes(0x11, &[0; 72]);
        assert!(parse_doc_info(&[declaration.clone(), declaration].concat(), &version).is_err());
        assert!(parse_doc_info(&[], &version).unwrap().id_mappings.is_none());
        for slot in 0..18 {
            for declared in [-1i32, 1] {
                let mut data = [0; 72];
                data[slot * 4..slot * 4 + 4].copy_from_slice(&declared.to_le_bytes());
                let result = parse_doc_info(&make_record_bytes(0x11, &data), &version).unwrap();
                assert!(result.id_mappings.is_some());
                assert!(
                    result.warnings.iter().any(|w| matches!(w,
                        Hwp5Warning::ParserFallback { subject: "IdMappings.count", reason }
                        if reason.contains(&format!("declared {declared}, actual 0"))
                    )),
                    "slot {slot}: {:?}",
                    result.warnings
                );
            }
        }
        // A failed definition still occupies its original slot.
        let mut data = [0; 72];
        data[36..40].copy_from_slice(&1i32.to_le_bytes());
        let result = parse_doc_info(
            &[make_record_bytes(0x11, &data), make_record_bytes(0x15, &[])].concat(),
            &version,
        )
        .unwrap();
        assert!(!result
            .warnings
            .iter()
            .any(|w| matches!(w, Hwp5Warning::ParserFallback { subject: "IdMappings.count", .. })));
        for tag in [0x5c, 0x60, 0x61] {
            let result = parse_doc_info(&make_record_bytes(tag, &[0; 8]), &version).unwrap();
            assert!(result.warnings.iter().any(|w| matches!(
                w,
                Hwp5Warning::ParserFallback { subject: "header.unsupported_definition", .. }
            )));
        }
    }

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

    fn make_tab_def_data(tab_count: i32) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&tab_count.to_le_bytes());
        for idx in 0..tab_count.max(0) {
            let position = 4000u32 + (idx as u32 * 4000);
            data.extend_from_slice(&position.to_le_bytes());
            data.push(0);
            data.push(3);
            data.extend_from_slice(&0u16.to_le_bytes());
        }
        data
    }

    #[test]
    fn fix2_failed_definitions_keep_slots_and_warn() {
        // 이것을 실패시키는 것: 실패 슬롯 압축 / 경고 삭제 / 뒤 정의를 빈 자리에 복사.
        let mut stream = Vec::new();
        let mut para = make_para_shape_data();
        para[4..8].copy_from_slice(&2468i32.to_le_bytes());
        for (tag, payload) in [
            (0x13, make_face_name_data("C")),
            (0x15, make_char_shape_data()),
            (0x19, para),
            (0x1a, make_style_data("C", "C")),
        ] {
            stream.extend(make_record_bytes(tag, &payload));
            stream.extend(make_record_bytes(tag, &[]));
            stream.extend(make_record_bytes(tag, &payload));
        }
        let result = parse_doc_info(&stream, &HwpVersion::new(5, 0, 2, 5)).unwrap();
        assert_eq!(
            [
                result.fonts.len(),
                result.char_shapes.len(),
                result.para_shapes.len(),
                result.styles.len()
            ],
            [3; 4]
        );
        assert_eq!(result.fonts[1].face_name, "");
        assert_eq!(result.fonts[2].face_name, "C");
        assert_eq!(result.char_shapes[1].height, 0);
        assert_eq!(result.char_shapes[2].height, 1000);
        assert_eq!(result.para_shapes[1].left_margin, 0);
        assert_eq!(result.para_shapes[2].left_margin, 2468);
        assert_eq!(result.styles[1].name, "");
        assert_eq!(result.styles[2].name, "C");
        for subject in ["fonts.slot", "char_shapes.slot", "para_shapes.slot", "styles.slot"] {
            assert!(result.warnings.iter().any(|w| matches!(w,
                Hwp5Warning::ParserFallback { subject: s, reason } if *s == subject && reason.contains("slot 1") && reason.contains("references to this slot"))), "{subject}: {:?}", result.warnings);
        }
        let store = crate::style_store::Hwp5StyleStore::from_doc_info(&result);
        assert_eq!(store.char_shapes[2].height, 1000);
        assert_eq!(store.para_shapes[2].left_margin, 2468);
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
    fn parse_doc_info_preserves_tab_def_slots_when_middle_record_fails() {
        let valid = make_tab_def_data(1);
        let mut invalid = make_tab_def_data(1);
        invalid.push(0xAA);

        let mut stream = Vec::new();
        stream.extend(make_record_bytes(0x16, &valid));
        stream.extend(make_record_bytes(0x16, &invalid));
        stream.extend(make_record_bytes(0x16, &valid));

        let result = parse_doc_info(&stream, &HwpVersion::new(5, 0, 2, 5)).unwrap();
        assert_eq!(result.tab_defs.len(), 3);
        assert_eq!(result.tab_defs[0].raw_id, 0);
        assert!(result.tab_defs[0].tab_def.is_some());
        assert_eq!(result.tab_defs[1].raw_id, 1);
        assert!(result.tab_defs[1].tab_def.is_none());
        assert_eq!(result.tab_defs[2].raw_id, 2);
        assert!(result.tab_defs[2].tab_def.is_some());
        assert!(result.warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::ParserFallback { subject, reason }
                if *subject == "tab_def.parse"
                    && reason.contains("slot 1")
        )));
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
