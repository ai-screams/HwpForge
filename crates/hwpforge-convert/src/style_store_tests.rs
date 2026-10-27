use std::collections::BTreeMap;

use crate::style_store_convert::{
    bgr_colorref_to_color, hwp5_char_shape_to_hwpx, hwp5_para_shape_to_hwpx, hwp5_tab_def_to_hwpx,
};
use hwpforge_foundation::{
    Alignment, BorderFillIndex, BreakType, CharShadow, CharShapeIndex, Color, EmbossType,
    EmphasisType, EngraveType, GradientType, HeadingType, LineSpacingType, OutlineType,
    ParaShapeIndex, ShadowType, StrikeoutShape, TabAlign, UnderlineType, VerticalPosition,
    WordBreakType,
};
use hwpforge_smithy_hwp5::schema::border_fill::{
    Hwp5BorderLineKind, Hwp5FillImageEffect, Hwp5FillImageMode, Hwp5FillPatternKind,
    Hwp5GradationType, Hwp5RawBorderFill, Hwp5RawBorderFillFill, Hwp5RawBorderLine,
    Hwp5RawColorFill, Hwp5RawGradationFill, Hwp5RawImageFill,
};
use hwpforge_smithy_hwp5::schema::header::{
    Hwp5RawBulletDef, Hwp5RawCharShape, Hwp5RawFaceName, Hwp5RawIdMappings, Hwp5RawNumberingDef,
    Hwp5RawNumberingParaHead, Hwp5RawParaShape, Hwp5RawStyle, Hwp5RawTabDef, Hwp5TabDefSlot,
    HwpVersion,
};
use hwpforge_smithy_hwp5::style_store::Hwp5StyleStore;
use hwpforge_smithy_hwp5::{
    DocInfoResult, Hwp5DocInfoBorderFillSlot, Hwp5DocInfoBulletSlot, Hwp5DocInfoNumberingSlot,
    Hwp5Warning,
};
use hwpforge_smithy_hwpx::style_store::HwpxFill;

fn border_fill_slot(id: u32, fill: Hwp5RawBorderFill) -> Hwp5DocInfoBorderFillSlot {
    Hwp5DocInfoBorderFillSlot { id, fill: Some(fill) }
}

fn parsed_tab_slot(raw_id: u32, tab_def: Hwp5RawTabDef) -> Hwp5TabDefSlot {
    Hwp5TabDefSlot::parsed(raw_id, tab_def)
}

fn invalid_tab_slot(raw_id: u32) -> Hwp5TabDefSlot {
    Hwp5TabDefSlot::invalid(raw_id)
}

fn numbering_slot(id: u32, numbering: Hwp5RawNumberingDef) -> Hwp5DocInfoNumberingSlot {
    Hwp5DocInfoNumberingSlot { id, numbering: Some(numbering) }
}

fn bullet_slot(id: u32, bullet: Hwp5RawBulletDef) -> Hwp5DocInfoBulletSlot {
    Hwp5DocInfoBulletSlot { id, bullet: Some(bullet) }
}

fn none_border_line() -> Hwp5RawBorderLine {
    Hwp5RawBorderLine { kind: Hwp5BorderLineKind::None, width: 0, color: 0x00000000 }
}

fn utf16le_string_bytes(text: &str) -> Vec<u8> {
    let u16s: Vec<u16> = text.encode_utf16().collect();
    let mut data = Vec::with_capacity(2 + u16s.len() * 2);
    data.extend_from_slice(&(u16s.len() as u16).to_le_bytes());
    for ch in u16s {
        data.extend_from_slice(&ch.to_le_bytes());
    }
    data
}

fn numbering_attr_for_format(num_format: &str) -> u32 {
    match num_format {
        "DIGIT" => 0x0C,
        "CIRCLED_DIGIT" => 0x2C,
        "ROMAN_CAPITAL" => 0x4C,
        "ROMAN_SMALL" => 0x6C,
        "LATIN_CAPITAL" => 0x8C,
        "LATIN_SMALL" => 0xAC,
        "CIRCLED_LATIN_SMALL" => 0xEC,
        "HANGUL_SYLLABLE" => 0x10C,
        "CIRCLED_HANGUL_SYLLABLE" => 0x12C,
        "HANGUL_JAMO" => 0x14C,
        "HANJA_DIGIT" => 0x16C,
        _ => 0x0C,
    }
}

fn make_numbering_para_head_bytes(num_format: &str, text: &str) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&numbering_attr_for_format(num_format).to_le_bytes());
    data.extend_from_slice(&0i16.to_le_bytes());
    data.extend_from_slice(&50i16.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&utf16le_string_bytes(text));
    data
}

fn make_numbering_def_bytes(version: HwpVersion, levels: &[(&str, &str)], start: u16) -> Vec<u8> {
    let mut data = Vec::new();
    for idx in 0..7 {
        let (num_format, text) = levels.get(idx).copied().unwrap_or(("DIGIT", ""));
        data.extend_from_slice(&make_numbering_para_head_bytes(num_format, text));
    }
    data.extend_from_slice(&start.to_le_bytes());
    if version >= HwpVersion::new(5, 0, 2, 5) {
        for _ in 0..7 {
            data.extend_from_slice(&1u32.to_le_bytes());
        }
    }
    if version >= HwpVersion::new(5, 1, 0, 0) {
        for idx in 7..10 {
            let (num_format, text) = levels.get(idx).copied().unwrap_or(("DIGIT", ""));
            data.extend_from_slice(&make_numbering_para_head_bytes(num_format, text));
        }
        for _ in 7..10 {
            data.extend_from_slice(&1u32.to_le_bytes());
        }
    }
    data
}

/// Build a 25-byte `HWPTAG_BULLET` payload matching the real Hancom layout:
/// 12-byte paragraph head + bullet glyph (2) + image flag (4) + fixed 5-byte
/// image block + check glyph (2).
fn make_bullet_def_bytes(use_image: bool) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&0u32.to_le_bytes()); // paragraph head properties
    data.extend_from_slice(&0i16.to_le_bytes()); // width adjust
    data.extend_from_slice(&0i16.to_le_bytes()); // text offset
    data.extend_from_slice(&u32::MAX.to_le_bytes()); // inherit character shape
    data.extend_from_slice(&(0x25CFu16).to_le_bytes()); // bullet char: ●
    data.extend_from_slice(&(if use_image { 1i32 } else { 0i32 }).to_le_bytes());
    data.extend_from_slice(&[0u8; 5]); // image block: always 5 bytes
    data.extend_from_slice(&(0x2611u16).to_le_bytes()); // check bullet char: ☑
    data
}

fn default_char_shape() -> Hwp5RawCharShape {
    Hwp5RawCharShape {
        font_ids: [0; 7],
        font_ratios: [100; 7],
        font_spacings: [0; 7],
        font_rel_sizes: [100; 7],
        font_offsets: [0; 7],
        height: 1000,
        property: 0,
        shadow_gap_x: 0,
        shadow_gap_y: 0,
        text_color: 0x000000,
        underline_color: 0x000000,
        shade_color: 0xFFFF_FFFF,
        shadow_color: 0x000000,
        border_fill_id: None,
        strike_color: None,
    }
}

fn default_para_shape() -> Hwp5RawParaShape {
    Hwp5RawParaShape {
        property1: 0,
        left_margin: 0,
        right_margin: 0,
        indent: 0,
        space_before: 0,
        space_after: 0,
        line_spacing: 160,
        tab_def_id: 0,
        numbering_bullet_id: 0,
        border_fill_id: 0,
        border_offset_left: 0,
        border_offset_right: 0,
        border_offset_top: 0,
        border_offset_bottom: 0,
        property2: None,
        property3: None,
        line_spacing2: None,
    }
}

fn empty_doc_info() -> DocInfoResult {
    DocInfoResult {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
        warnings: vec![],
    }
}

#[test]
fn from_doc_info_empty() {
    let doc_info = empty_doc_info();
    let store = Hwp5StyleStore::from_doc_info(&doc_info);
    assert!(store.fonts.is_empty());
    assert!(store.char_shapes.is_empty());
    assert!(store.para_shapes.is_empty());
    assert!(store.styles.is_empty());
}

#[test]
fn from_doc_info_with_data() {
    let doc_info = DocInfoResult {
        id_mappings: None,
        fonts: vec![
            Hwp5RawFaceName {
                property: 0,
                face_name: "바탕".into(),
                alternate_font_type: None,
                alternate_font_name: None,
                panose1: None,
                default_font_name: None,
            },
            Hwp5RawFaceName {
                property: 0,
                face_name: "돋움".into(),
                alternate_font_type: None,
                alternate_font_name: None,
                panose1: None,
                default_font_name: None,
            },
        ],
        char_shapes: vec![default_char_shape()],
        para_shapes: vec![default_para_shape()],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![border_fill_slot(
            1,
            Hwp5RawBorderFill {
                property: 0,
                three_d: false,
                shadow: false,
                slash_diagonal_shape: 0,
                back_slash_diagonal_shape: 0,
                center_line: false,
                left: Hwp5RawBorderLine {
                    kind: Hwp5BorderLineKind::Solid,
                    width: 1,
                    color: 0x00000000,
                },
                right: Hwp5RawBorderLine {
                    kind: Hwp5BorderLineKind::Solid,
                    width: 1,
                    color: 0x00000000,
                },
                top: Hwp5RawBorderLine {
                    kind: Hwp5BorderLineKind::Solid,
                    width: 1,
                    color: 0x00000000,
                },
                bottom: Hwp5RawBorderLine {
                    kind: Hwp5BorderLineKind::Solid,
                    width: 1,
                    color: 0x00000000,
                },
                diagonal: Hwp5RawBorderLine {
                    kind: Hwp5BorderLineKind::Solid,
                    width: 0,
                    color: 0x00000000,
                },
                fill: Hwp5RawBorderFillFill::None,
            },
        )],
        warnings: vec![],
    };
    let store = Hwp5StyleStore::from_doc_info(&doc_info);
    assert!(store.id_mappings.is_none());
    assert_eq!(store.fonts.len(), 2);
    assert_eq!(store.char_shapes.len(), 1);
    assert_eq!(store.para_shapes.len(), 1);
    assert!(store.tab_defs.is_empty());
    assert_eq!(store.border_fills.len(), 1);
}

#[test]
fn to_hwpx_style_store_empty_fonts_returns_preset() {
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };
    let hwpx_store = crate::hwp5_style_store_to_hwpx(&store).unwrap().0;
    assert_eq!(hwpx_store.font_count(), 7);
    assert_eq!(hwpx_store.char_shape_count(), 0);
    assert_eq!(hwpx_store.para_shape_count(), 0);
    assert_eq!(hwpx_store.style_count(), 0);
}

#[test]
fn to_hwpx_style_store_preserves_hwp5_indices() {
    let store = Hwp5StyleStore {
        id_mappings: Some(Hwp5RawIdMappings {
            bin_data_count: 0,
            hangul_font_count: 1,
            english_font_count: 0,
            hanja_font_count: 0,
            japanese_font_count: 0,
            other_font_count: 0,
            symbol_font_count: 0,
            user_font_count: 0,
            border_fill_count: 0,
            char_shape_count: 1,
            tab_def_count: 3,
            numbering_def_count: 0,
            bullet_def_count: 0,
            para_shape_count: 1,
            style_count: 1,
            memo_shape_count: None,
            change_tracking_count: None,
            change_tracking_author_count: None,
        }),
        fonts: vec![Hwp5RawFaceName {
            property: 0,
            face_name: "바탕".into(),
            alternate_font_type: None,
            alternate_font_name: None,
            panose1: None,
            default_font_name: None,
        }],
        char_shapes: vec![default_char_shape()],
        para_shapes: vec![default_para_shape()],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![Hwp5RawStyle {
            name: "본문".into(),
            english_name: "Body".into(),
            kind: 0,
            next_style_id: 0,
            lang_id: 1042,
            para_shape_id: 0,
            char_shape_id: 0,
            lock_form: 0,
        }],
        border_fills: vec![],
    };
    let hwpx_store = crate::hwp5_style_store_to_hwpx(&store).unwrap().0;
    // 이것을 실패시키는 것: 선언된 빈 언어 그룹의 font 0 생략.
    assert_eq!(hwpx_store.font_count(), 7);
    assert_eq!(hwpx_store.char_shape_count(), 1);
    assert_eq!(hwpx_store.para_shape_count(), 1);
    assert_eq!(hwpx_store.style_count(), 1);
    assert_eq!(hwpx_store.style(0).unwrap().name, "본문");
}

#[test]
fn to_hwpx_style_store_projects_numberings_and_warns_on_bullets() {
    let numbering = Hwp5RawNumberingDef {
        start: 1,
        paragraph_heads: vec![Hwp5RawNumberingParaHead {
            attributes: 12,
            width_adjust: 0,
            text_offset: 50,
            char_shape_id: u32::MAX,
            start_number: 1,
            level: 0,
            num_format: "DIGIT".into(),
            text: String::new(),
            checkable: false,
        }],
    };

    let store = Hwp5StyleStore {
        id_mappings: Some(Hwp5RawIdMappings {
            bin_data_count: 0,
            hangul_font_count: 0,
            english_font_count: 0,
            hanja_font_count: 0,
            japanese_font_count: 0,
            other_font_count: 0,
            symbol_font_count: 0,
            user_font_count: 0,
            border_fill_count: 0,
            char_shape_count: 0,
            tab_def_count: 0,
            numbering_def_count: 1,
            bullet_def_count: 1,
            para_shape_count: 0,
            style_count: 0,
            memo_shape_count: None,
            change_tracking_count: None,
            change_tracking_author_count: None,
        }),
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![numbering_slot(1, numbering)],
        bullets: vec![bullet_slot(
            1,
            Hwp5RawBulletDef::parse(&make_bullet_def_bytes(false)).unwrap(),
        )],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    let (hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    assert_eq!(hwpx_store.numbering_count(), 1);
    assert_eq!(hwpx_store.bullet_count(), 1);
    let bullet = hwpx_store.iter_bullets().next().unwrap();
    assert_eq!(bullet.id, 1);
    assert_eq!(bullet.bullet_char, "●");
    assert_eq!(bullet.checked_char.as_deref(), Some("☑"));
    assert!(!bullet.use_image);
    assert!(!warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, .. } if *subject == "bullet.projection"
    )));
}

/// A picture bullet keeps `use_image`, but the image it names is not
/// carried — said once per such bullet, and not for a glyph bullet.
///
/// 이것을 실패시키는 것: 그림 글머리표 경고 갈래를 지우는 것, 조건을
/// 거꾸로 하는 것(글자 글머리표에 경고).
#[test]
fn picture_bullet_reports_that_its_image_is_not_carried() {
    let mut store = image_fill_store();
    store.border_fills.clear();
    store.bullets = vec![
        bullet_slot(2, Hwp5RawBulletDef::parse(&make_bullet_def_bytes(true)).unwrap()),
        bullet_slot(3, Hwp5RawBulletDef::parse(&make_bullet_def_bytes(false)).unwrap()),
    ];

    let (hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();

    assert_eq!(hwpx_store.bullet_count(), 2);
    let reasons: Vec<&String> = warnings
        .iter()
        .filter_map(|w| match w {
            Hwp5Warning::ProjectionFallback { subject: "bullet.image", reason } => Some(reason),
            _ => None,
        })
        .collect();
    assert_eq!(reasons, ["picture bullet id=2: its image reference is not carried"]);
}

#[test]
fn to_hwpx_style_store_uses_id_mappings_font_buckets() {
    let mut raw = default_char_shape();
    raw.font_ids = [1, 0, 0, 0, 0, 0, 0];

    let store = Hwp5StyleStore {
        id_mappings: Some(Hwp5RawIdMappings {
            bin_data_count: 0,
            hangul_font_count: 2,
            english_font_count: 1,
            hanja_font_count: 0,
            japanese_font_count: 0,
            other_font_count: 0,
            symbol_font_count: 0,
            user_font_count: 0,
            border_fill_count: 0,
            char_shape_count: 1,
            tab_def_count: 0,
            numbering_def_count: 0,
            bullet_def_count: 0,
            para_shape_count: 0,
            style_count: 0,
            memo_shape_count: None,
            change_tracking_count: None,
            change_tracking_author_count: None,
        }),
        fonts: vec![
            Hwp5RawFaceName {
                property: 0,
                face_name: "바탕".into(),
                alternate_font_type: None,
                alternate_font_name: None,
                panose1: None,
                default_font_name: None,
            },
            Hwp5RawFaceName {
                property: 0,
                face_name: "돋움".into(),
                alternate_font_type: None,
                alternate_font_name: None,
                panose1: None,
                default_font_name: None,
            },
            Hwp5RawFaceName {
                property: 0,
                face_name: "Arial".into(),
                alternate_font_type: None,
                alternate_font_name: None,
                panose1: None,
                default_font_name: None,
            },
        ],
        char_shapes: vec![raw],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    let hwpx_store = crate::hwp5_style_store_to_hwpx(&store).unwrap().0;
    let fonts: Vec<(u32, String, String)> = hwpx_store
        .iter_fonts()
        .map(|font| (font.id, font.lang.clone(), font.face_name.clone()))
        .collect();
    assert_eq!(
        fonts,
        vec![
            (0, "HANGUL".into(), "바탕".into()),
            (1, "HANGUL".into(), "돋움".into()),
            (0, "LATIN".into(), "Arial".into()),
            (0, "HANJA".into(), "바탕".into()),
            (0, "JAPANESE".into(), "바탕".into()),
            (0, "OTHER".into(), "바탕".into()),
            (0, "SYMBOL".into(), "바탕".into()),
            (0, "USER".into(), "바탕".into()),
        ]
    );

    let cs = hwpx_store.char_shape(hwpforge_foundation::CharShapeIndex::new(0)).unwrap();
    assert_eq!(cs.font_ref.hangul.get(), 1);
    assert_eq!(cs.font_ref.latin.get(), 0);
}

#[test]
fn bgr_colorref_black_roundtrip() {
    let color = bgr_colorref_to_color(0x000000);
    assert_eq!(color, Color::BLACK);
}

#[test]
fn hwp5_char_shape_bold_italic() {
    let mut raw = default_char_shape();
    raw.property = 0b11; // bold + italic
    let hwpx = hwp5_char_shape_to_hwpx(&raw);
    assert!(hwpx.bold);
    assert!(hwpx.italic);
}

#[test]
fn hwp5_char_shape_not_bold_not_italic() {
    let raw = default_char_shape();
    let hwpx = hwp5_char_shape_to_hwpx(&raw);
    assert!(!hwpx.bold);
    assert!(!hwpx.italic);
}

#[test]
fn hwp5_char_shape_single_bit_bold_italic_order() {
    let mut italic_only = default_char_shape();
    italic_only.property = 1 << 0;
    let italic_hwpx = hwp5_char_shape_to_hwpx(&italic_only);
    assert!(!italic_hwpx.bold);
    assert!(italic_hwpx.italic);

    let mut bold_only = default_char_shape();
    bold_only.property = 1 << 1;
    let bold_hwpx = hwp5_char_shape_to_hwpx(&bold_only);
    assert!(bold_hwpx.bold);
    assert!(!bold_hwpx.italic);
}

#[test]
fn hwp5_char_shape_maps_supported_style_surface() {
    let mut raw = default_char_shape();
    raw.property = (1 << 1)
        | (1 << 2)
        | (1 << 8)
        | (1 << 11)
        | (1 << 13)
        | (1 << 15)
        | (1 << 18)
        | (1 << 21)
        | (1 << 25)
        | (2 << 26)
        | (1 << 30);
    raw.underline_color = 0x00112233;
    raw.strike_color = Some(0x00332211);
    raw.border_fill_id = Some(7);
    raw.font_ratios[0] = 80;
    raw.font_spacings[0] = 10;
    raw.font_rel_sizes[0] = 90;
    raw.font_offsets[0] = 15;

    let hwpx = hwp5_char_shape_to_hwpx(&raw);

    assert!(hwpx.bold);
    assert!(!hwpx.italic);
    assert_eq!(hwpx.underline_type, UnderlineType::Bottom);
    assert_eq!(hwpx.underline_color, Some(bgr_colorref_to_color(0x00112233)));
    assert_eq!(hwpx.strikeout_shape, StrikeoutShape::Dot);
    assert_eq!(hwpx.strikeout_color, Some(bgr_colorref_to_color(0x00332211)));
    assert_eq!(hwpx.vertical_position, VerticalPosition::Superscript);
    assert_eq!(hwpx.outline_type, OutlineType::Solid);
    assert_eq!(hwpx.shadow.kind(), ShadowType::Drop);
    assert_eq!(hwpx.emboss_type, EmbossType::Emboss);
    assert_eq!(hwpx.engrave_type, EngraveType::None);
    assert_eq!(hwpx.emphasis, EmphasisType::DotAbove);
    assert_eq!(hwpx.ratio, 80);
    assert_eq!(hwpx.spacing, 10);
    assert_eq!(hwpx.rel_sz, 90);
    assert_eq!(hwpx.char_offset, 15);
    assert!(hwpx.use_font_space);
    assert!(hwpx.use_kerning);
    assert_eq!(hwpx.border_fill_id, Some(7));
}

#[test]
fn hwp5_char_shape_maps_engrave_and_subscript() {
    let mut raw = default_char_shape();
    raw.property = (1 << 14) | (1 << 16);

    let hwpx = hwp5_char_shape_to_hwpx(&raw);

    assert_eq!(hwpx.vertical_position, VerticalPosition::Subscript);
    assert_eq!(hwpx.emboss_type, EmbossType::None);
    assert_eq!(hwpx.engrave_type, EngraveType::Engrave);
}

#[test]
fn hwp5_char_shape_preserves_emboss_and_engrave_together() {
    let mut raw = default_char_shape();
    raw.property = (1 << 13) | (1 << 14);

    let hwpx = hwp5_char_shape_to_hwpx(&raw);

    assert_eq!(hwpx.emboss_type, EmbossType::Emboss);
    assert_eq!(hwpx.engrave_type, EngraveType::Engrave);
}

#[test]
fn hwp5_char_shape_warns_on_conflicting_vertical_position_bits() {
    let mut raw = default_char_shape();
    raw.property = (1 << 15) | (1 << 16);

    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![Hwp5RawFaceName {
            property: 0,
            face_name: "함초롬바탕".into(),
            alternate_font_type: None,
            alternate_font_name: None,
            panose1: None,
            default_font_name: None,
        }],
        char_shapes: vec![raw],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    let (hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();

    assert_eq!(
        hwpx_store.char_shape(CharShapeIndex::new(0)).unwrap().vertical_position,
        VerticalPosition::Superscript
    );
    assert!(warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, reason }
            if *subject == "style.char_shape.vertical_position"
                && reason.contains("both superscript and subscript")
    )));
}

#[test]
fn hwp5_char_shape_warns_on_projection_collapses() {
    let mut raw = default_char_shape();
    raw.property = (1 << 2) | (1 << 4) | (2 << 11) | (1 << 13) | (1 << 15) | (1 << 18) | (7 << 26);
    raw.font_ratios[1] = 90;
    raw.font_spacings[2] = 5;

    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![Hwp5RawFaceName {
            property: 0,
            face_name: "함초롬바탕".into(),
            alternate_font_type: None,
            alternate_font_name: None,
            panose1: None,
            default_font_name: None,
        }],
        char_shapes: vec![raw],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    let (_, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();

    assert!(!warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, .. }
            if *subject == "style.char_shape.underline_shape"
    )));
    // Kind 1 is a discrete shadow, carried as Drop: nothing to report.
    assert!(!warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, .. }
            if *subject == "style.char_shape.shadow_kind"
    )));
    // the strike line family is now carried, so the fallback warning
    // must not fire.
    assert!(!warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, .. }
            if *subject == "style.char_shape.strike_shape"
    )));
    assert!(!warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, .. }
            if *subject == "style.char_shape.emboss"
    )));
    assert!(!warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, .. }
            if *subject == "style.char_shape.engrave"
    )));
    assert!(!warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, .. }
            if *subject == "style.char_shape.vertical_position"
    )));
    assert!(warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, reason }
            if *subject == "style.char_shape.script_scalars" && reason.contains("ratio") && reason.contains("spacing")
    )));
}

/// One char shape through the store conversion: its shadow and the shadow
/// warnings.
fn shadow_of(property: u32, color: u32, gap: (i8, i8)) -> (CharShadow, Vec<&'static str>) {
    let mut raw = default_char_shape();
    raw.property = property;
    raw.shadow_color = color;
    (raw.shadow_gap_x, raw.shadow_gap_y) = gap;
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![Hwp5RawFaceName {
            property: 0,
            face_name: "함초롬바탕".into(),
            alternate_font_type: None,
            alternate_font_name: None,
            panose1: None,
            default_font_name: None,
        }],
        char_shapes: vec![raw.clone()],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };
    let (_, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    let subjects = warnings
        .iter()
        .filter_map(|w| match w {
            Hwp5Warning::ProjectionFallback { subject, .. } if subject.contains("shadow") => {
                Some(*subject)
            }
            _ => None,
        })
        .collect();
    (hwp5_char_shape_to_hwpx(&raw).shadow, subjects)
}

fn tuple(s: CharShadow) -> (ShadowType, Option<String>, i8, i8) {
    (s.kind(), s.color().map(|c| c.to_hex_rgb()), s.offset_x().get(), s.offset_y().get())
}

fn some(hex: &str) -> Option<String> {
    Some(hex.to_string())
}

/// 법무부 charPr 114 (kind 1, 0x00C0C0C0, gaps 5·5) — the shadow Hancom drew
/// under 붙임 on page 3.
/// 이것을 실패시키는 것: 색·간격 하드코딩(`#B2B2B2` 10 10) 복귀.
#[test]
fn hwp5_discrete_shadow_keeps_color_and_offsets() {
    let (s, w) = shadow_of(1 << 11, 0x00C0_C0C0, (5, 5));
    assert_eq!(tuple(s), (ShadowType::Drop, some("#C0C0C0"), 5, 5));
    assert!(w.is_empty(), "{w:?}");
}

/// 이것을 실패시키는 것: 종류 2 를 Drop 으로, BGR 바이트 순서 생략(`#563412`),
/// 간격 x·y 뒤바꿈.
#[test]
fn hwp5_continuous_shadow_keeps_kind_and_rgb_color() {
    let (s, w) = shadow_of(2 << 11, 0x0056_3412, (-3, 4));
    assert_eq!(tuple(s), (ShadowType::Continuous, some("#123456"), -3, 4));
    assert!(w.is_empty(), "{w:?}");
}

/// 0xFFFFFFFF is "no color" (Hancom writes `color="none"`).
/// 이것을 실패시키는 것: 마스크로 `#FFFFFF` 를 만드는 것.
#[test]
fn hwp5_shadow_color_all_ones_is_no_color() {
    let (s, w) = shadow_of(0, 0xFFFF_FFFF, (0, 0));
    assert_eq!(tuple(s), (ShadowType::None, None, 0, 0));
    assert!(w.is_empty(), "{w:?}");
}

/// 이것을 실패시키는 것: 상위 바이트 경고 누락, `0xFFFFFFFF` 판정을 상위 바이트만으로
/// 하는 것(0x01C0C0C0 을 색 없음으로).
#[test]
fn hwp5_shadow_color_upper_byte_is_dropped_and_reported() {
    let (s, w) = shadow_of(1 << 11, 0x01C0_C0C0, (5, 5));
    assert_eq!(tuple(s), (ShadowType::Drop, some("#C0C0C0"), 5, 5));
    assert_eq!(w, ["style.char_shape.shadow_color"]);
}

/// 이것을 실패시키는 것: 자르지 않는 것, 경계(±100)를 범위 밖으로 보는 것, 경고 누락.
#[test]
fn hwp5_shadow_gaps_outside_100_clamp_and_are_reported() {
    let (s, w) = shadow_of(1 << 11, 0x00C0_C0C0, (127, -128));
    assert_eq!(tuple(s), (ShadowType::Drop, some("#C0C0C0"), 100, -100));
    assert_eq!(w, ["style.char_shape.shadow_offset"]);
    let (s, w) = shadow_of(1 << 11, 0x00C0_C0C0, (100, -100));
    assert_eq!(tuple(s), (ShadowType::Drop, some("#C0C0C0"), 100, -100));
    assert!(w.is_empty(), "{w:?}");
}

/// Kind 3 is outside the spec: NONE, keeping color and gaps, reported.
/// 이것을 실패시키는 것: 3 을 Drop 으로, 경고 삭제.
#[test]
fn hwp5_shadow_kind_3_is_none_and_reported() {
    let (s, w) = shadow_of(3 << 11, 0x0056_3412, (5, 5));
    assert_eq!(tuple(s), (ShadowType::None, some("#123456"), 5, 5));
    assert_eq!(w, ["style.char_shape.shadow_kind"]);
}

/// Kind 0 still carries its color and gaps (Hancom writes them on NONE too).
/// 이것을 실패시키는 것: 종류 0 에서 색·간격을 버리는 것.
#[test]
fn hwp5_shadow_kind_0_keeps_color_and_gaps_quietly() {
    let (s, w) = shadow_of(0, 0x00FF_0000, (10, 7));
    assert_eq!(tuple(s), (ShadowType::None, some("#0000FF"), 10, 7));
    assert!(w.is_empty(), "{w:?}");
}

#[test]
fn hwp5_para_shape_alignment_justify() {
    let raw = default_para_shape(); // property1 bits 2-4 = 0 => Justify
    let hwpx = hwp5_para_shape_to_hwpx(&raw).unwrap();
    assert_eq!(hwpx.alignment, Alignment::Justify);
}

#[test]
fn hwp5_para_shape_alignment_left() {
    let mut raw = default_para_shape();
    raw.property1 = 1 << 2; // bits 2-4 = 1 => Left
    let hwpx = hwp5_para_shape_to_hwpx(&raw).unwrap();
    assert_eq!(hwpx.alignment, Alignment::Left);
}

#[test]
fn hwp5_para_shape_alignment_center() {
    let mut raw = default_para_shape();
    raw.property1 = 3 << 2; // bits 2-4 = 3 => Center
    let hwpx = hwp5_para_shape_to_hwpx(&raw).unwrap();
    assert_eq!(hwpx.alignment, Alignment::Center);
}

#[test]
fn hwp5_para_shape_alignment_distribute_and_flush() {
    let mut distribute = default_para_shape();
    distribute.property1 = 4 << 2;
    let distribute_hwpx = hwp5_para_shape_to_hwpx(&distribute).unwrap();
    assert_eq!(distribute_hwpx.alignment, Alignment::Distribute);

    let mut distribute_flush = default_para_shape();
    distribute_flush.property1 = 5 << 2;
    let distribute_flush_hwpx = hwp5_para_shape_to_hwpx(&distribute_flush).unwrap();
    assert_eq!(distribute_flush_hwpx.alignment, Alignment::DistributeFlush);
}

#[test]
fn hwp5_para_shape_maps_line_spacing_types_and_values() {
    let mut fixed_old = default_para_shape();
    fixed_old.property1 = 1;
    fixed_old.line_spacing = 240;
    let fixed_old_hwpx = hwp5_para_shape_to_hwpx(&fixed_old).unwrap();
    assert_eq!(fixed_old_hwpx.line_spacing_type, LineSpacingType::Fixed);
    assert_eq!(fixed_old_hwpx.line_spacing, 240);

    let mut between_old = default_para_shape();
    between_old.property1 = 2;
    between_old.line_spacing = 333;
    let between_old_hwpx = hwp5_para_shape_to_hwpx(&between_old).unwrap();
    assert_eq!(between_old_hwpx.line_spacing_type, LineSpacingType::BetweenLines);
    assert_eq!(between_old_hwpx.line_spacing, 333);

    let mut fixed_new = default_para_shape();
    fixed_new.property3 = Some(1);
    fixed_new.line_spacing2 = Some(2500);
    fixed_new.line_spacing = 160;
    let fixed_new_hwpx = hwp5_para_shape_to_hwpx(&fixed_new).unwrap();
    assert_eq!(fixed_new_hwpx.line_spacing_type, LineSpacingType::Fixed);
    assert_eq!(fixed_new_hwpx.line_spacing, 2500);
}

#[test]
fn hwp5_para_shape_maps_break_flags_condense_and_border_fill() {
    let mut raw = default_para_shape();
    raw.property1 =
        (2 << 5) | (1 << 7) | (1 << 8) | (20 << 9) | (1 << 16) | (1 << 17) | (1 << 18) | (1 << 19);
    raw.border_fill_id = 5;

    let hwpx = hwp5_para_shape_to_hwpx(&raw).unwrap();

    assert_eq!(hwpx.break_latin_word, WordBreakType::BreakWord);
    assert_eq!(hwpx.break_non_latin_word, WordBreakType::BreakWord);
    assert!(hwpx.snap_to_grid);
    assert_eq!(hwpx.condense, 20);
    assert!(hwpx.widow_orphan);
    assert!(hwpx.keep_with_next);
    assert!(hwpx.keep_lines_together);
    assert_eq!(hwpx.break_type, BreakType::Page);
    assert_eq!(hwpx.border_fill_id, Some(BorderFillIndex::new(5)));
}

#[test]
fn hwp5_para_shape_rejects_unsupported_line_spacing_and_carries_latin_hyphenation() {
    let mut raw = default_para_shape();
    // bits 5-6 = 1 → HYPHENATION (carry).
    // property3 = 4 → an unknown line-spacing kind (raw > 3) so the
    // projection fallback warning still fires (raw=3 was moved
    // out of "unsupported" into AtLeast — see the AtLeast carry test
    // below).
    raw.property1 = 1 << 5;
    raw.property3 = Some(4);

    let mut store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![raw],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    assert!(crate::hwp5_style_store_to_hwpx(&store).is_err());
    store.para_shapes[0].property3 = Some(0);
    let (hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    // the HYPHENATION case is carried, not warned.
    assert!(
        !warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::ProjectionFallback { subject, .. }
                if *subject == "style.para_shape.break_latin_word"
        )),
        "break_latin_word projection fallback must not fire for raw=1 (HYPHENATION)"
    );
    let hwpx = hwpx_store
        .para_shape(hwpforge_foundation::ParaShapeIndex::new(0))
        .expect("projected para shape 0 must exist");
    assert_eq!(hwpx.break_latin_word, WordBreakType::Hyphenation);
}

#[test]
fn hwp5_para_shape_carries_at_least_line_spacing_without_warning() {
    use hwpforge_foundation::LineSpacingType;

    let mut raw = default_para_shape();
    // property3 = 3 → AT_LEAST (carry, was previously
    // collapsed to Percentage with a ProjectionFallback warning).
    raw.property3 = Some(3);
    raw.line_spacing = 2400;
    raw.line_spacing2 = Some(2400);

    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![raw],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    let (hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();

    assert!(
        !warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::ProjectionFallback { subject, .. }
                if *subject == "style.para_shape.line_spacing"
        )),
        "line_spacing projection fallback must not fire for raw=3 (AT_LEAST)"
    );

    let hwpx = hwpx_store
        .para_shape(hwpforge_foundation::ParaShapeIndex::new(0))
        .expect("projected para shape 0 must exist");
    assert_eq!(hwpx.line_spacing_type, LineSpacingType::AtLeast);
    assert_eq!(hwpx.line_spacing, 2400);
}

#[test]
fn hwp5_para_shape_warns_on_unknown_latin_break_mode_3() {
    let mut raw = default_para_shape();
    // bits 5-6 = 3 → unspecified, still warns + collapses to KEEP_WORD
    raw.property1 = 3 << 5;

    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![raw],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    let (hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();

    assert!(warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, reason }
            if *subject == "style.para_shape.break_latin_word" && reason.contains("mode 3")
    )));
    let hwpx = hwpx_store
        .para_shape(hwpforge_foundation::ParaShapeIndex::new(0))
        .expect("projected para shape 0 must exist");
    assert_eq!(hwpx.break_latin_word, WordBreakType::KeepWord);
}

/// Vertical alignment (bits 20–21), border offsets (bytes 34–42) and the
/// border connect / ignore-margin flags (bits 28–29) are carried into the
/// HWPX paragraph shape and no longer reported; the font
/// line height and auto-spacing flags still are.
///
/// 이것을 실패시키는 것: 세로 정렬 비트 대응을 바꾸는 것(2 → CENTER) / 오프셋
/// 순서를 바꾸거나 하나를 버리는 것 / 두 플래그 중 하나를 버리는 것.
#[test]
fn hwp5_para_shape_carries_vertical_align_and_border_offsets() {
    let mut raw = default_para_shape();
    raw.property1 = (2 << 20) | (1 << 22) | (1 << 28) | (1 << 29);
    raw.property2 = Some((1 << 4) | (1 << 5));
    raw.border_offset_left = 10;
    raw.border_offset_right = -20;
    raw.border_offset_top = 30;
    raw.border_offset_bottom = -40;

    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![raw.clone()],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    let (hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    let subjects: Vec<&str> = warnings
        .iter()
        .filter_map(|w| match w {
            Hwp5Warning::ProjectionFallback { subject, .. } => Some(*subject),
            _ => None,
        })
        .collect();
    assert_eq!(subjects, ["style.para_shape.auto_spacing"], "{warnings:?}");
    let shape = hwpx_store
        .para_shape(hwpforge_foundation::ParaShapeIndex::new(0))
        .expect("projected para shape 0 must exist");
    assert_eq!(shape.vertical_align, "CENTER");
    let unit = |v: i32| hwpforge_foundation::HwpUnit::new(v).unwrap();
    assert_eq!(shape.border_offset, [unit(10), unit(-20), unit(30), unit(-40)]);
    assert!(shape.border_connect && shape.border_ignore_margin);
    for (bits, name) in [(0, "BASELINE"), (1, "TOP"), (3, "BOTTOM")] {
        raw.property1 = bits << 20;
        assert_eq!(hwp5_para_shape_to_hwpx(&raw).unwrap().vertical_align, name);
    }
}

/// `property2` bits 0-1 "한 줄로 입력" → `lineWrap`. 1 is SQUEEZE, 0 is
/// BREAK, 2 and 3 are undefined — BREAK with one warning per definition.
/// Reserved bits 2-3 alone and a record without `property2` are BREAK and
/// quiet.
///
/// 이것을 실패시키는 것: SQUEEZE 대입 삭제 · 값 1 과 0 뒤바꿈 · 값 2 를 KEEP
/// 등 지원값으로 쓰기 · 값 2·3 경고 삭제 · 비트 2–3 에 경고 · `property2`
/// 없음에 경고.
#[test]
fn hwp5_para_shape_single_line_input_maps_to_line_wrap() {
    let mut raws = Vec::new();
    for property2 in [Some(1u32), Some(0), Some(2), Some(3), Some(0b1100), None] {
        let mut raw = default_para_shape();
        raw.property2 = property2;
        raws.push(raw);
    }
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: raws,
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    let (hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();

    let wraps: Vec<String> = (0..6)
        .map(|i| {
            hwpx_store
                .para_shape(hwpforge_foundation::ParaShapeIndex::new(i))
                .expect("projected para shape")
                .line_wrap
                .clone()
        })
        .collect();
    assert_eq!(wraps, ["SQUEEZE", "BREAK", "BREAK", "BREAK", "BREAK", "BREAK"]);
    let reasons: Vec<&str> = warnings
        .iter()
        .filter_map(|w| match w {
            Hwp5Warning::ProjectionFallback { subject, reason }
                if *subject == "style.para_shape.line_wrap" =>
            {
                Some(reason.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(reasons.len(), 2, "{warnings:?}");
    assert!(reasons[0].contains("para shape 2") && reasons[0].contains("mode 2"), "{reasons:?}");
    assert!(reasons[1].contains("para shape 3") && reasons[1].contains("mode 3"), "{reasons:?}");
}

#[test]
fn hwp5_para_shape_heading_bits_map_to_kind_level_and_ref() {
    // Task #121: HWP5 wire bit 25-27 is a 3-bit zero-based
    // ordinal (`0..=7`) — the previous `saturating_sub(1)` assumption was
    // wrong: the first outline level is 0, the second is 1, and so on.
    let mut raw = default_para_shape();
    raw.property1 = (1 << 23) | (5 << 25);
    raw.numbering_bullet_id = 7;
    assert_eq!(raw.heading_kind(), HeadingType::Outline);
    assert_eq!(raw.heading_level(), 5);
    assert_eq!(raw.list_ref_id(), 7);
    let hwpx = hwp5_para_shape_to_hwpx(&raw).unwrap();
    assert_eq!(hwpx.heading_type, HeadingType::Outline);
    assert_eq!(hwpx.heading_id_ref, 0);
    assert_eq!(hwpx.heading_level, 5);

    raw.property1 = (2 << 23) | (3 << 25);
    assert_eq!(raw.heading_kind(), HeadingType::Number);
    assert_eq!(raw.heading_level(), 3);

    raw.property1 = (3 << 23) | (1 << 25);
    assert_eq!(raw.heading_kind(), HeadingType::Bullet);
    assert_eq!(raw.heading_level(), 1);

    raw.property1 = 0;
    assert_eq!(raw.heading_kind(), HeadingType::None);
    assert_eq!(raw.heading_level(), 0);

    let mut raw = default_para_shape();
    raw.property1 = (3 << 23) | (2 << 25);
    raw.numbering_bullet_id = 4;
    let hwpx = hwp5_para_shape_to_hwpx(&raw).unwrap();
    assert_eq!(hwpx.heading_type, HeadingType::Bullet);
    assert_eq!(hwpx.heading_id_ref, 4);
    assert_eq!(hwpx.heading_level, 2);
}

#[test]
fn hwp5_numbering_def_parse_preserves_core_list_semantics() {
    let data = make_numbering_def_bytes(
        HwpVersion::new(5, 1, 0, 0),
        &[
            ("DIGIT", "^1."),
            ("HANGUL_SYLLABLE", "^2."),
            ("DIGIT", "^3)"),
            ("DIGIT", "(^4)"),
            ("DIGIT", "(^5)"),
            ("DIGIT", "^6"),
            ("DIGIT", ""),
            ("CIRCLED_DIGIT", "^8"),
            ("HANGUL_JAMO", ""),
            ("ROMAN_SMALL", ""),
        ],
        2,
    );
    let numbering = Hwp5RawNumberingDef::parse(&data, &HwpVersion::new(5, 1, 0, 0)).unwrap();
    assert_eq!(numbering.start, 2);
    assert_eq!(numbering.paragraph_heads.len(), 10);
    assert_eq!(numbering.paragraph_heads[0].level, 0);
    assert_eq!(numbering.paragraph_heads[0].start_number, 1);
    assert_eq!(numbering.paragraph_heads[1].num_format, "HANGUL_SYLLABLE");
    assert_eq!(numbering.paragraph_heads[0].text, "^1.");
    assert_eq!(numbering.paragraph_heads[7].num_format, "CIRCLED_DIGIT");
    assert_eq!(numbering.paragraph_heads[7].text, "^8");
    let core = numbering.to_core_numbering_def(1);
    assert_eq!(core.id, 1);
    assert_eq!(core.start, 2);
    assert_eq!(core.levels.len(), 10);
    assert_eq!(core.levels[0].level, 1);
    assert_eq!(core.levels[0].start, 1);
    assert_eq!(core.levels[0].text, "^1.");
    assert_eq!(core.levels[1].num_format, hwpforge_foundation::NumberFormatType::HangulSyllable);
}

#[test]
fn hwp5_numbering_def_tolerates_trailing_bytes() {
    // A valid NumberingDef payload followed by extra trailing bytes (e.g. a
    // future 5.x sub-version field we don't yet decode) must parse OK rather
    // than hard-erroring the whole record — matching CharShape/ParaShape
    // sibling parsers.
    let version = HwpVersion::new(5, 1, 0, 0);
    let mut data = make_numbering_def_bytes(
        version,
        &[
            ("DIGIT", "^1."),
            ("DIGIT", "^2."),
            ("DIGIT", "^3."),
            ("DIGIT", "^4."),
            ("DIGIT", "^5."),
            ("DIGIT", "^6."),
            ("DIGIT", "^7."),
            ("DIGIT", "^8."),
            ("DIGIT", "^9."),
            ("DIGIT", "^10."),
        ],
        3,
    );
    // No-trailing case: still Ok (regression).
    let no_trailing = Hwp5RawNumberingDef::parse(&data, &version).unwrap();
    assert_eq!(no_trailing.start, 3);
    assert_eq!(no_trailing.paragraph_heads.len(), 10);

    // Append unexpected trailing bytes.
    data.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02]);
    let with_trailing = Hwp5RawNumberingDef::parse(&data, &version)
        .expect("trailing bytes must be tolerated, not hard-error");
    // The leading fields must decode identically despite the trailing bytes.
    assert_eq!(with_trailing.start, 3);
    assert_eq!(with_trailing.paragraph_heads.len(), 10);
    assert_eq!(with_trailing.paragraph_heads[0].text, "^1.");
    assert_eq!(with_trailing.paragraph_heads[9].text, "^10.");
}

#[test]
fn hwp5_para_shape_keeps_builtin_tab_ids() {
    for tab_def_id in 0..=2 {
        let mut raw = default_para_shape();
        raw.tab_def_id = tab_def_id;
        let hwpx = hwp5_para_shape_to_hwpx(&raw).unwrap();
        assert_eq!(hwpx.tab_pr_id_ref, tab_def_id as u32);
    }
}

#[test]
fn hwp5_para_shape_preserves_custom_tab_ids() {
    let mut raw = default_para_shape();
    raw.tab_def_id = 3;
    let hwpx = hwp5_para_shape_to_hwpx(&raw).unwrap();
    assert_eq!(hwpx.tab_pr_id_ref, 3);
}

#[test]
fn hwp5_tab_def_maps_stops_and_auto_flags() {
    // Coverage matrix below mirrors what 한컴 actually writes (
    // tab-fidelity hotfix). `position` is halved on the way through
    // `hwp5_tab_position_to_hwp_unit` because the HWPX encoder treats
    // `TabStop.position` as HwpUnitChar (= HWP5 raw HwpUnit / 2). The
    // leader mapping preserves each supported wire style.
    let raw = Hwp5RawTabDef {
        property: 0b11,
        tab_stops: vec![
            // fill_type=2 → openhwp IR LongDash → HWPX "DASH_DOT_DOT"
            hwpforge_smithy_hwp5::schema::header::Hwp5RawTabStop {
                position: 4000,
                tab_type: 0,
                fill_type: 2,
            },
            // fill_type=5 is undefined in the empirical mapping → falls
            // back to "NONE" (was "LONG_DASH" before the fix; that was
            // incorrect — the wire value means DOT)
            hwpforge_smithy_hwp5::schema::header::Hwp5RawTabStop {
                position: 8000,
                tab_type: 3,
                fill_type: 5,
            },
        ],
    };

    let hwpx = hwp5_tab_def_to_hwpx(3, &raw).unwrap();
    assert_eq!(hwpx.id, 3);
    assert!(hwpx.auto_tab_left);
    assert!(hwpx.auto_tab_right);
    assert_eq!(hwpx.stops.len(), 2);
    assert_eq!(
        hwpx.stops[0].position.as_hwp_unit().unwrap().as_i32(),
        2000,
        "raw 4000 HwpUnit → 2000 HwpUnitChar"
    );
    assert_eq!(hwpx.stops[0].align, TabAlign::Left);
    assert_eq!(hwpx.stops[0].leader.as_hwpx_str(), "DASH_DOT_DOT");
    assert_eq!(
        hwpx.stops[1].position.as_hwp_unit().unwrap().as_i32(),
        4000,
        "raw 8000 HwpUnit → 4000 HwpUnitChar"
    );
    assert_eq!(hwpx.stops[1].align, TabAlign::Decimal);
    assert_eq!(hwpx.stops[1].leader.as_hwpx_str(), "NONE");
}

#[test]
fn to_hwpx_style_store_carries_tab_defs() {
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![parsed_tab_slot(
            0,
            Hwp5RawTabDef {
                property: 0b01,
                tab_stops: vec![hwpforge_smithy_hwp5::schema::header::Hwp5RawTabStop {
                    position: 12000,
                    tab_type: 1,
                    fill_type: 1,
                }],
            },
        )],
        styles: vec![],
        border_fills: vec![],
    };

    let hwpx_store = crate::hwp5_style_store_to_hwpx(&store).unwrap().0;
    let tabs: Vec<_> = hwpx_store.iter_tabs().cloned().collect();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].id, 0);
    assert!(tabs[0].auto_tab_left);
    assert_eq!(tabs[0].stops.len(), 1);
    assert_eq!(tabs[0].stops[0].align, TabAlign::Right);
    // HWP5 fill_type=1 → openhwp IR Dot → HWPX "DOT".
    assert_eq!(tabs[0].stops[0].leader.as_hwpx_str(), "DOT");
    // HWP5 raw position 12000 HwpUnit halves to 6000 HwpUnitChar.
    assert_eq!(tabs[0].stops[0].position.as_hwp_unit().unwrap().as_i32(), 6000);
}

#[test]
fn to_hwpx_style_store_warns_when_id_mappings_tab_count_disagrees() {
    let store = Hwp5StyleStore {
        id_mappings: Some(Hwp5RawIdMappings {
            bin_data_count: 0,
            hangul_font_count: 0,
            english_font_count: 0,
            hanja_font_count: 0,
            japanese_font_count: 0,
            other_font_count: 0,
            symbol_font_count: 0,
            user_font_count: 0,
            border_fill_count: 0,
            char_shape_count: 0,
            tab_def_count: 4,
            numbering_def_count: 0,
            bullet_def_count: 0,
            para_shape_count: 0,
            style_count: 0,
            memo_shape_count: None,
            change_tracking_count: None,
            change_tracking_author_count: None,
        }),
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![parsed_tab_slot(0, Hwp5RawTabDef { property: 0, tab_stops: vec![] })],
        styles: vec![],
        border_fills: vec![],
    };

    let (_, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    assert!(warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, reason }
            if *subject == "tab_def.count"
                && reason.contains("declares 4")
                && reason.contains("parsed 1")
    )));
}

#[test]
fn to_hwpx_style_store_warns_on_unknown_tab_codes() {
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![parsed_tab_slot(
            0,
            Hwp5RawTabDef {
                property: 0,
                tab_stops: vec![hwpforge_smithy_hwp5::schema::header::Hwp5RawTabStop {
                    position: 12000,
                    tab_type: 9,
                    fill_type: 99,
                }],
            },
        )],
        styles: vec![],
        border_fills: vec![],
    };

    let (_, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    assert!(warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, reason }
            if *subject == "tab_def.align"
                && reason.contains("tab_type 9")
    )));
    assert!(warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, reason }
            if *subject == "tab_def.leader"
                && reason.contains("fill_type 99")
    )));
}

#[test]
fn to_hwpx_style_store_rejects_missing_custom_tab_def() {
    let mut para = default_para_shape();
    para.tab_def_id = 9;

    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![para],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![parsed_tab_slot(0, Hwp5RawTabDef { property: 0, tab_stops: vec![] })],
        styles: vec![],
        border_fills: vec![],
    };

    // 이것을 실패시키는 것: 없는 탭 참조를 built-in 0으로 대체.
    let error = crate::hwp5_style_store_to_hwpx(&store).unwrap_err().to_string();
    assert!(error.contains("paraPr 0"), "{error}");
    assert!(error.contains("tabPr reference ID 9"), "{error}");
}

#[test]
fn to_hwpx_style_store_emits_placeholder_for_invalid_tab_slot() {
    let mut para = default_para_shape();
    para.tab_def_id = 3;

    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![para],
        numberings: vec![],
        bullets: vec![],
        // a native slot 3 has preceding slots 0..2; IDs cannot stand in for holes.
        tab_defs: vec![
            parsed_tab_slot(0, Hwp5RawTabDef { property: 0, tab_stops: vec![] }),
            parsed_tab_slot(1, Hwp5RawTabDef { property: 0, tab_stops: vec![] }),
            parsed_tab_slot(2, Hwp5RawTabDef { property: 0, tab_stops: vec![] }),
            invalid_tab_slot(3),
        ],
        styles: vec![],
        border_fills: vec![],
    };

    let (hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    assert_eq!(hwpx_store.para_shape(ParaShapeIndex::new(0)).unwrap().tab_pr_id_ref, 3);
    let tabs: Vec<_> = hwpx_store.iter_tabs().cloned().collect();
    assert_eq!(tabs.len(), 4);
    assert_eq!(tabs[3].id, 3);
    assert!(tabs[3].stops.is_empty());
    assert!(warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ParserFallback { subject, reason }
            if *subject == "tab_def.slot"
                && reason.contains("slot 3")
    )));
}

#[test]
fn to_hwpx_style_store_rejects_out_of_range_tab_position() {
    // Even raw values carry physical lengths: unpack before checking MAX_VALUE.
    // A value above the semantic range must be rejected, never clamped.
    let oversize =
        (hwpforge_foundation::HwpUnit::MAX_VALUE as u32).saturating_mul(2).saturating_add(2);
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![parsed_tab_slot(
            3,
            Hwp5RawTabDef {
                property: 0,
                tab_stops: vec![hwpforge_smithy_hwp5::schema::header::Hwp5RawTabStop {
                    position: oversize,
                    tab_type: 0,
                    fill_type: 0,
                }],
            },
        )],
        styles: vec![],
        border_fills: vec![],
    };

    // 이것을 실패시키는 것: 변이 12 — 범위 오류를 clamp로 치환.
    assert!(crate::hwp5_style_store_to_hwpx(&store).is_err());
}

#[test]
fn to_hwpx_style_store_preserves_builtin_para_shape_tab_refs_without_warning() {
    let mut para = default_para_shape();
    para.tab_def_id = 2;

    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![para],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    let (hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    assert_eq!(hwpx_store.para_shape(ParaShapeIndex::new(0)).unwrap().tab_pr_id_ref, 2);
    assert!(!warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, .. } if *subject == "tab_def.ref"
    )));
}

#[test]
fn to_hwpx_style_store_materializes_custom_border_fills() {
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![
            border_fill_slot(
                1,
                Hwp5RawBorderFill {
                    property: 0,
                    three_d: false,
                    shadow: false,
                    slash_diagonal_shape: 0,
                    back_slash_diagonal_shape: 0,
                    center_line: false,
                    left: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 1,
                        color: 0x00000000,
                    },
                    right: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 1,
                        color: 0x00000000,
                    },
                    top: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 1,
                        color: 0x00000000,
                    },
                    bottom: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 10,
                        color: 0x00000000,
                    },
                    diagonal: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 0,
                        color: 0x00000000,
                    },
                    fill: Hwp5RawBorderFillFill::None,
                },
            ),
            border_fill_slot(
                2,
                Hwp5RawBorderFill {
                    property: 0,
                    three_d: false,
                    shadow: false,
                    slash_diagonal_shape: 0,
                    back_slash_diagonal_shape: 0,
                    center_line: false,
                    left: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 13,
                        color: 0x00000000,
                    },
                    right: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 13,
                        color: 0x00000000,
                    },
                    top: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 13,
                        color: 0x00000000,
                    },
                    bottom: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 13,
                        color: 0x00000000,
                    },
                    diagonal: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 0,
                        color: 0x00000000,
                    },
                    fill: Hwp5RawBorderFillFill::Color(Hwp5RawColorFill {
                        background_color: 0x004CBF85,
                        pattern_color: 0xC0FF_FFFF,
                        pattern_kind: Hwp5FillPatternKind::None,
                        alpha: 0,
                        extra_data: Vec::new(),
                    }),
                },
            ),
        ],
    };

    let hwpx_store = crate::hwp5_style_store_to_hwpx(&store).unwrap().0;
    assert_eq!(hwpx_store.border_fill_count(), 2);
    let fourth = hwpx_store.border_fill(1).unwrap();
    assert_eq!(fourth.bottom.width, "1.0 mm");
    assert_eq!(fourth.fill, None);
    let fifth = hwpx_store.border_fill(2).unwrap();
    assert_eq!(fifth.left.width, "3.0 mm");
    assert!(matches!(
        fifth.fill,
        Some(HwpxFill::WinBrush {
            ref face_color,
            ref hatch_color,
            ref alpha,
        })
            if face_color == "#85BF4C"
                && hatch_color == "#C0FFFFFF"
                && alpha == "0"
    ));
    assert!(fifth.fill_hatch_style.is_none());
}

#[test]
fn to_hwpx_style_store_preserves_border_fill_ids_when_middle_slot_is_missing() {
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![
            border_fill_slot(
                1,
                Hwp5RawBorderFill {
                    property: 0,
                    three_d: false,
                    shadow: false,
                    slash_diagonal_shape: 0,
                    back_slash_diagonal_shape: 0,
                    center_line: false,
                    left: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 1,
                        color: 0x00000000,
                    },
                    right: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 1,
                        color: 0x00000000,
                    },
                    top: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 1,
                        color: 0x00000000,
                    },
                    bottom: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 1,
                        color: 0x00000000,
                    },
                    diagonal: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::None,
                        width: 0,
                        color: 0x00000000,
                    },
                    fill: Hwp5RawBorderFillFill::None,
                },
            ),
            Hwp5DocInfoBorderFillSlot { id: 2, fill: None },
            border_fill_slot(
                3,
                Hwp5RawBorderFill {
                    property: 0,
                    three_d: false,
                    shadow: false,
                    slash_diagonal_shape: 0,
                    back_slash_diagonal_shape: 0,
                    center_line: false,
                    left: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 13,
                        color: 0x00000000,
                    },
                    right: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 13,
                        color: 0x00000000,
                    },
                    top: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 13,
                        color: 0x00000000,
                    },
                    bottom: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::Solid,
                        width: 13,
                        color: 0x00000000,
                    },
                    diagonal: Hwp5RawBorderLine {
                        kind: Hwp5BorderLineKind::None,
                        width: 0,
                        color: 0x00000000,
                    },
                    fill: Hwp5RawBorderFillFill::None,
                },
            ),
        ],
    };

    let hwpx_store = crate::hwp5_style_store_to_hwpx(&store).unwrap().0;
    assert_eq!(hwpx_store.border_fill_count(), 3);
    assert_eq!(hwpx_store.border_fill(1).unwrap().id, 1);
    assert_eq!(hwpx_store.border_fill(2).unwrap().id, 2);
    assert_eq!(hwpx_store.border_fill(3).unwrap().id, 3);
    assert_eq!(hwpx_store.border_fill(2).unwrap().fill, None);
    assert_eq!(hwpx_store.border_fill(2).unwrap().diagonal, None);
    assert_eq!(hwpx_store.border_fill(2).unwrap().slash.border_type, "NONE");
    assert_eq!(hwpx_store.border_fill(2).unwrap().back_slash.border_type, "NONE");
    assert_eq!(hwpx_store.border_fill(3).unwrap().left.width, "3.0 mm");
}

#[test]
fn to_hwpx_style_store_materializes_pattern_fill_hatch_style() {
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![border_fill_slot(
            1,
            Hwp5RawBorderFill {
                property: 0,
                three_d: false,
                shadow: false,
                slash_diagonal_shape: 0,
                back_slash_diagonal_shape: 0,
                center_line: false,
                left: Hwp5RawBorderLine {
                    kind: Hwp5BorderLineKind::Solid,
                    width: 1,
                    color: 0x00000000,
                },
                right: Hwp5RawBorderLine {
                    kind: Hwp5BorderLineKind::Solid,
                    width: 1,
                    color: 0x00000000,
                },
                top: Hwp5RawBorderLine {
                    kind: Hwp5BorderLineKind::Solid,
                    width: 1,
                    color: 0x00000000,
                },
                bottom: Hwp5RawBorderLine {
                    kind: Hwp5BorderLineKind::Solid,
                    width: 1,
                    color: 0x00000000,
                },
                diagonal: Hwp5RawBorderLine {
                    kind: Hwp5BorderLineKind::None,
                    width: 0,
                    color: 0x00000000,
                },
                fill: Hwp5RawBorderFillFill::Color(Hwp5RawColorFill {
                    background_color: 0x00FFD700,
                    pattern_color: 0x00000000,
                    pattern_kind: Hwp5FillPatternKind::Horizontal,
                    alpha: 0,
                    extra_data: Vec::new(),
                }),
            },
        )],
    };

    let hwpx_store = crate::hwp5_style_store_to_hwpx(&store).unwrap().0;
    assert!(matches!(hwpx_store.border_fill(1).unwrap().fill, Some(HwpxFill::WinBrush { .. })));
    assert_eq!(hwpx_store.border_fill(1).unwrap().fill_hatch_style.as_deref(), Some("HORIZONTAL"));
}

#[test]
fn to_hwpx_style_store_materializes_gradient_fill() {
    // 이것을 실패시키는 것: 공개 border 조회를 위치 대신 label로 수행.
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![border_fill_slot(
            4,
            Hwp5RawBorderFill {
                property: 0,
                three_d: false,
                shadow: false,
                slash_diagonal_shape: 0,
                back_slash_diagonal_shape: 0,
                center_line: false,
                left: none_border_line(),
                right: none_border_line(),
                top: none_border_line(),
                bottom: none_border_line(),
                diagonal: none_border_line(),
                fill: Hwp5RawBorderFillFill::Gradation(Hwp5RawGradationFill {
                    gradation_type: Hwp5GradationType::Linear,
                    angle: 90,
                    center_x: 0,
                    center_y: 0,
                    blur: 0,
                    colors: vec![0x00FF0000, 0x0000FF00],
                    shape: None,
                    blur_center: Some(50),
                    extra_data: Vec::new(),
                }),
            },
        )],
    };

    let hwpx_store = crate::hwp5_style_store_to_hwpx(&store).unwrap().0;
    assert!(matches!(
        hwpx_store.border_fill(1).unwrap().gradient_fill,
        Some(ref fill)
            if fill.gradient_type == GradientType::Linear
                && fill.angle == 90
                && fill.center_x == 0
                && fill.center_y == 0
                && fill.step == 0
                && fill.step_center == 50
                && fill.alpha == 0
                && fill.colors == vec![Color::from_raw(0x00FF0000), Color::from_raw(0x0000FF00)]
    ));
    assert!(hwpx_store.border_fill(1).unwrap().fill.is_none());
}

/// G: the gradation's `blur` is HWPX `step` — 0-255 carried, above 255
/// clamped to 255 with one warning.
///
/// 이것을 실패시키는 것: 255 고정 복귀 · 자르기 없이 300 · 자르기 경고 삭제 ·
/// 255 에도 경고.
#[test]
fn gradient_blur_becomes_step() {
    let gradation = |blur: u32| Hwp5RawBorderFill {
        property: 0,
        three_d: false,
        shadow: false,
        slash_diagonal_shape: 0,
        back_slash_diagonal_shape: 0,
        center_line: false,
        left: none_border_line(),
        right: none_border_line(),
        top: none_border_line(),
        bottom: none_border_line(),
        diagonal: none_border_line(),
        fill: Hwp5RawBorderFillFill::Gradation(Hwp5RawGradationFill {
            gradation_type: Hwp5GradationType::Linear,
            angle: 0,
            center_x: 0,
            center_y: 0,
            blur,
            colors: vec![0x00FF0000, 0x0000FF00],
            shape: None,
            blur_center: Some(50),
            extra_data: Vec::new(),
        }),
    };
    let blurs = [50u32, 100, 255, 300];
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: blurs
            .iter()
            .enumerate()
            .map(|(i, &blur)| border_fill_slot(i as u32 + 1, gradation(blur)))
            .collect(),
    };

    let (hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    let steps: Vec<i32> = (1..=4)
        .map(|id| hwpx_store.border_fill(id).unwrap().gradient_fill.as_ref().unwrap().step)
        .collect();
    assert_eq!(steps, [50, 100, 255, 255]);
    let step_warnings: Vec<_> = warnings
        .iter()
        .filter(|w| {
            matches!(w, Hwp5Warning::ProjectionFallback { subject, .. }
            if *subject == "style.border_fill.gradation_step")
        })
        .collect();
    assert_eq!(step_warnings.len(), 1, "{warnings:?}");
    assert!(format!("{:?}", step_warnings[0]).contains("300"));
}

#[test]
fn to_hwpx_style_store_image_fill_round_trips_correctly() {
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![border_fill_slot(
            4,
            Hwp5RawBorderFill {
                property: 0,
                three_d: false,
                shadow: false,
                slash_diagonal_shape: 0,
                back_slash_diagonal_shape: 0,
                center_line: false,
                left: none_border_line(),
                right: none_border_line(),
                top: none_border_line(),
                bottom: none_border_line(),
                diagonal: none_border_line(),
                fill: Hwp5RawBorderFillFill::Image(Hwp5RawImageFill {
                    mode: Hwp5FillImageMode::TileAll,
                    brightness: 0,
                    contrast: 0,
                    effect: Hwp5FillImageEffect::RealPic,
                    bindata_id: 1,
                    extra_data: Vec::new(),
                }),
            },
        )],
    };

    let hwpx_store = crate::hwp5_style_store_to_hwpx(&store).unwrap().0;
    assert!(matches!(
        hwpx_store.border_fill(1).unwrap().image_fill,
        Some(ref fill)
            if fill.mode == "TILE"
                && fill.binary_item_id_ref == "BIN0001"
                && fill.bright == 0
                && fill.contrast == 0
                && fill.effect == "REAL_PIC"
                && fill.alpha == 0
    ));
    assert!(hwpx_store.border_fill(1).unwrap().fill.is_none());
    // NOTE: border_fill_image_binary_ids() is pub(crate) in hwpforge-smithy-hwp5
    // and cannot be called from hwpforge-convert tests without adding a new pub item.
    // The assertion is preserved here as a comment for documentation:
    // assert_eq!(store.border_fill_image_binary_ids().into_iter().collect::<Vec<_>>(), vec![1]);
}

/// A store with one border fill (label 4, position 1) whose image fill names `BIN0001`.
/// fix8b: public accessors select position 1 even when the descriptive label is 4.
fn image_fill_store() -> Hwp5StyleStore {
    Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![border_fill_slot(
            4,
            Hwp5RawBorderFill {
                property: 0,
                three_d: false,
                shadow: false,
                slash_diagonal_shape: 0,
                back_slash_diagonal_shape: 0,
                center_line: false,
                left: none_border_line(),
                right: none_border_line(),
                top: none_border_line(),
                bottom: none_border_line(),
                diagonal: none_border_line(),
                fill: Hwp5RawBorderFillFill::Image(Hwp5RawImageFill {
                    mode: Hwp5FillImageMode::TileAll,
                    brightness: 0,
                    contrast: 0,
                    effect: Hwp5FillImageEffect::RealPic,
                    bindata_id: 1,
                    extra_data: Vec::new(),
                }),
            },
        )],
    }
}

/// An image fill names the image of its reference's `DocInfo`
/// position. One whose reference has no image the package will carry is
/// left out rather than written as a reference to nothing (the decoder has
/// already warned that the asset is missing); one that has an image is
/// written with that image's manifest id — `BIN0006` here, where the record
/// at position 1 stores id 6.
///
/// 이것을 실패시키는 것: 자산이 없는 이미지 채우기를 빼는 갈래를 지우는 것
/// (첫 단언), 해석 표를 무시하고 `BIN{참조}` 로 쓰는 것 (둘째 단언).
#[test]
fn image_fill_without_its_asset_is_left_out() {
    let store = image_fill_store();

    let (without, _) =
        crate::hwp5_style_store_to_hwpx_with_image_ids(&store, Some(&BTreeMap::new())).unwrap();
    let fill = without.border_fill(1).unwrap();
    assert!(fill.image_fill.is_none(), "{fill:?}");
    assert!(fill.fill.is_none() && fill.gradient_fill.is_none(), "{fill:?}");

    let ids: BTreeMap<u16, String> = [(1, "BIN0006".to_string())].into();
    let (with, _) = crate::hwp5_style_store_to_hwpx_with_image_ids(&store, Some(&ids)).unwrap();
    assert_eq!(
        with.border_fill(1).unwrap().image_fill.as_ref().map(|f| f.binary_item_id_ref.as_str()),
        Some("BIN0006")
    );
}

/// The old public mapping has no `DocInfo` order: it keeps the reference
/// as `BIN{N:04X}`, as before, and now says the name may be wrong.
///
/// 이것을 실패시키는 것: 공개 함수가 빈 집합을 넘기는 것 (채우기가 빠진다),
/// 해석 없이 쓸 때의 경고를 지우는 것.
#[test]
fn public_style_mapping_keeps_every_image_fill() {
    let (store, warnings) = crate::hwp5_style_store_to_hwpx(&image_fill_store()).unwrap();
    assert_eq!(
        store.border_fill(1).unwrap().image_fill.as_ref().map(|f| f.binary_item_id_ref.as_str()),
        Some("BIN0001")
    );
    let reference_warnings: Vec<&String> = warnings
        .iter()
        .filter_map(|w| match w {
            Hwp5Warning::ProjectionFallback {
                subject: "style.border_fill.image_reference",
                reason,
            } => Some(reason),
            _ => None,
        })
        .collect();
    assert_eq!(
        reference_warnings,
        ["reference 1 written as BIN0001 without a DocInfo order resolver"],
        "{warnings:?}"
    );
}

/// the sibling resolves the reference through the storage names the
/// decoder reports, writes the manifest id (the name's stem), and has
/// nothing to warn about. A reference with no name is left out.
///
/// 이것을 실패시키는 것: 형제 함수가 해석 표 대신 `None` 을 넘기는 것
/// (`BIN0001` + 경고), 저장 이름의 확장자를 떼지 않는 것 (`BIN0006.png`),
/// 이름 없는 참조를 빼면서 경고하지 않는 것, 이름 있는 참조에도 경고하는 것.
#[test]
fn bin_data_style_mapping_writes_the_resolved_image() {
    let names: BTreeMap<u16, String> = [(1, "BIN0006.png".to_string())].into();
    let (store, warnings) =
        crate::hwp5_style_store_to_hwpx_with_bin_data(&image_fill_store(), &names).unwrap();
    assert_eq!(
        store.border_fill(1).unwrap().image_fill.as_ref().map(|f| f.binary_item_id_ref.as_str()),
        Some("BIN0006")
    );
    assert!(
        !warnings.iter().any(|w| matches!(
            w,
            Hwp5Warning::ProjectionFallback { subject: "style.border_fill.image_reference", .. }
        )),
        "{warnings:?}"
    );

    let (unnamed, warnings) =
        crate::hwp5_style_store_to_hwpx_with_bin_data(&image_fill_store(), &BTreeMap::new())
            .unwrap();
    assert!(unnamed.border_fill(1).unwrap().image_fill.is_none());
    let reasons: Vec<&String> = warnings
        .iter()
        .filter_map(|w| match w {
            Hwp5Warning::ProjectionFallback {
                subject: "style.border_fill.image_reference",
                reason,
            } => Some(reason),
            _ => None,
        })
        .collect();
    assert_eq!(
        reasons,
        ["border_fill_id=4: reference 1 has no BinData storage name; image fill left out"],
        "a fill left out by this function is reported here"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// parse_outline_style_name
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn parse_outline_style_name_returns_level_for_korean_prefix_with_space() {
    assert_eq!(crate::parse_outline_style_name("개요 1"), Some(1));
    assert_eq!(crate::parse_outline_style_name("개요 7"), Some(7));
    assert_eq!(crate::parse_outline_style_name("개요 10"), Some(10));
}

#[test]
fn parse_outline_style_name_returns_level_for_korean_prefix_without_space() {
    assert_eq!(crate::parse_outline_style_name("개요1"), Some(1));
    assert_eq!(crate::parse_outline_style_name("개요9"), Some(9));
}

#[test]
fn parse_outline_style_name_returns_level_for_english_prefix() {
    assert_eq!(crate::parse_outline_style_name("Outline 3"), Some(3));
    assert_eq!(crate::parse_outline_style_name("Outline3"), Some(3));
    assert_eq!(crate::parse_outline_style_name("Outline 10"), Some(10));
}

#[test]
fn parse_outline_style_name_returns_none_for_unrelated_names() {
    assert_eq!(crate::parse_outline_style_name("본문"), None);
    assert_eq!(crate::parse_outline_style_name("Body"), None);
    assert_eq!(crate::parse_outline_style_name("개요"), None);
    assert_eq!(crate::parse_outline_style_name("Outline"), None);
    assert_eq!(crate::parse_outline_style_name(""), None);
}

#[test]
fn parse_outline_style_name_trims_leading_trailing_whitespace() {
    assert_eq!(crate::parse_outline_style_name("  개요 2  "), Some(2));
}

// ──────────────────────────────────────────────────────────────────────────
// apply_outline_style_level_overrides (via hwp5_style_store_to_hwpx)
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn apply_outline_style_level_overrides_bumps_level_from_style_name() {
    // "개요 7" → level_one_based=7 → level_zero_based=6.
    // The para shape has Outline kind (bits 23-24=1) with wire-capped level=6
    // (bits 25-27=6). After override heading_level must equal 6.
    let mut para = default_para_shape();
    // bits 23-24 = 0b01 → Outline; bits 25-27 = 0b110 → level 6
    para.property1 = (1u32 << 23) | (6u32 << 25);

    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![default_char_shape()],
        para_shapes: vec![para],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![Hwp5RawStyle {
            name: "개요 7".into(),
            english_name: "Outline7".into(),
            kind: 0,
            next_style_id: 0,
            lang_id: 1042,
            para_shape_id: 0,
            char_shape_id: 0,
            lock_form: 0,
        }],
        border_fills: vec![],
    };

    let (hwpx_store, _warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    let ps = hwpx_store.para_shape(ParaShapeIndex::new(0)).unwrap();
    assert_eq!(ps.heading_level, 6, "override from '개요 7' must set heading_level=6");
}

#[test]
fn apply_outline_style_level_overrides_skips_non_outline_heading_type() {
    // A Number-type paraPr pointing at an "개요 N" style must not be overridden.
    let mut para = default_para_shape();
    // bits 23-24 = 0b10 → Number; bits 25-27 = 2 → level 2
    para.property1 = (2u32 << 23) | (2u32 << 25);

    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![default_char_shape()],
        para_shapes: vec![para],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![Hwp5RawStyle {
            name: "개요 7".into(),
            english_name: "Outline7".into(),
            kind: 0,
            next_style_id: 0,
            lang_id: 1042,
            para_shape_id: 0,
            char_shape_id: 0,
            lock_form: 0,
        }],
        border_fills: vec![],
    };

    let (hwpx_store, _warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    let ps = hwpx_store.para_shape(ParaShapeIndex::new(0)).unwrap();
    // heading_level must remain 2 — Number kind is skipped
    assert_eq!(ps.heading_level, 2, "Number-kind paraPr must not be overridden");
}

#[test]
fn fix3_outline_style_rejects_out_of_bounds_para_shape_id() {
    // 이것을 실패시키는 것: 이름 붙은 스타일의 없는 paraPr 참조를 무시.
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![default_char_shape()],
        para_shapes: vec![], // empty
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![Hwp5RawStyle {
            name: "개요 3".into(),
            english_name: "Outline3".into(),
            kind: 0,
            next_style_id: 0,
            lang_id: 1042,
            para_shape_id: 99, // out of bounds
            char_shape_id: 0,
            lock_form: 0,
        }],
        border_fills: vec![],
    };

    let error = crate::hwp5_style_store_to_hwpx(&store).unwrap_err().to_string();
    assert!(error.contains("paraPr reference ID 99"), "{error}");
}

#[test]
fn apply_outline_style_level_overrides_skips_char_kind_styles() {
    // kind=1 is a character style — must be skipped entirely.
    let mut para = default_para_shape();
    para.property1 = (1u32 << 23) | (1u32 << 25); // Outline, level 1

    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![default_char_shape()],
        para_shapes: vec![para],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![Hwp5RawStyle {
            name: "개요 9".into(),
            english_name: "Outline9".into(),
            kind: 1, // character style — skip
            next_style_id: 0,
            lang_id: 1042,
            para_shape_id: 0,
            char_shape_id: 0,
            lock_form: 0,
        }],
        border_fills: vec![],
    };

    let (hwpx_store, _) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    let ps = hwpx_store.para_shape(ParaShapeIndex::new(0)).unwrap();
    // heading_level stays 1 (decoded from property1 bits 25-27=1)
    assert_eq!(ps.heading_level, 1, "char-style entry must not trigger outline override");
}

// ──────────────────────────────────────────────────────────────────────────
// None-slot paths (lib.rs L137-163)
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn numbering_slot_none_emits_parser_fallback_warning() {
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![Hwp5DocInfoNumberingSlot { id: 3, numbering: None }],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    let (hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    assert_eq!(hwpx_store.numbering_count(), 0, "failed slot must not produce a numbering entry");
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            Hwp5Warning::ParserFallback { subject, reason }
                if *subject == "numbering.slot" && reason.contains("slot 3")
        )),
        "must emit ParserFallback for failed numbering slot: {warnings:?}"
    );
}

#[test]
fn bullet_slot_none_emits_parser_fallback_warning() {
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![Hwp5DocInfoBulletSlot { id: 5, bullet: None }],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    let (hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    assert_eq!(hwpx_store.bullet_count(), 0, "failed bullet slot must not produce an entry");
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            Hwp5Warning::ParserFallback { subject, reason }
                if *subject == "bullet.slot" && reason.contains("slot 5")
        )),
        "must emit ParserFallback for failed bullet slot: {warnings:?}"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// Count-mismatch integrity warnings (lib.rs L261-299)
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn numbering_count_mismatch_emits_projection_fallback_warning() {
    // id_mappings declares 3 but only 1 slot is present.
    let numbering = Hwp5RawNumberingDef {
        start: 1,
        paragraph_heads: vec![Hwp5RawNumberingParaHead {
            attributes: 12,
            width_adjust: 0,
            text_offset: 50,
            char_shape_id: u32::MAX,
            start_number: 1,
            level: 0,
            num_format: "DIGIT".into(),
            text: String::new(),
            checkable: false,
        }],
    };

    let store = Hwp5StyleStore {
        id_mappings: Some(Hwp5RawIdMappings {
            bin_data_count: 0,
            hangul_font_count: 0,
            english_font_count: 0,
            hanja_font_count: 0,
            japanese_font_count: 0,
            other_font_count: 0,
            symbol_font_count: 0,
            user_font_count: 0,
            border_fill_count: 0,
            char_shape_count: 0,
            tab_def_count: 0,
            numbering_def_count: 3,
            bullet_def_count: 0,
            para_shape_count: 0,
            style_count: 0,
            memo_shape_count: None,
            change_tracking_count: None,
            change_tracking_author_count: None,
        }),
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![numbering_slot(1, numbering)],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    let (_, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            Hwp5Warning::ProjectionFallback { subject, reason }
                if *subject == "numbering.count"
                    && reason.contains("declares 3")
                    && reason.contains("parsed 1")
        )),
        "must warn on numbering count mismatch: {warnings:?}"
    );
}

#[test]
fn bullet_count_mismatch_emits_projection_fallback_warning() {
    // id_mappings declares 2 but 0 bullet slots are present.
    let store = Hwp5StyleStore {
        id_mappings: Some(Hwp5RawIdMappings {
            bin_data_count: 0,
            hangul_font_count: 0,
            english_font_count: 0,
            hanja_font_count: 0,
            japanese_font_count: 0,
            other_font_count: 0,
            symbol_font_count: 0,
            user_font_count: 0,
            border_fill_count: 0,
            char_shape_count: 0,
            tab_def_count: 0,
            numbering_def_count: 0,
            bullet_def_count: 2,
            para_shape_count: 0,
            style_count: 0,
            memo_shape_count: None,
            change_tracking_count: None,
            change_tracking_author_count: None,
        }),
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![], // 0 actual
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    let (_, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            Hwp5Warning::ProjectionFallback { subject, reason }
                if *subject == "bullet.count"
                    && reason.contains("declares 2")
                    && reason.contains("parsed 0")
        )),
        "must warn on bullet count mismatch: {warnings:?}"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// shade_color non-null path (style_store_convert.rs)
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn hwp5_char_shape_shade_color_non_null_carries_color() {
    // shade_color != 0xFFFFFFFF sentinel → Some(Color)
    let mut raw = default_char_shape();
    raw.shade_color = 0x00_80_40_20; // non-null BGR color
    let hwpx = hwp5_char_shape_to_hwpx(&raw);
    assert!(hwpx.shade_color.is_some(), "non-null shade_color must produce Some(Color)");
}

#[test]
fn hwp5_char_shape_shade_color_null_sentinel_maps_to_none() {
    let mut raw = default_char_shape();
    raw.shade_color = 0xFFFF_FFFF; // sentinel = "no shade"
    let hwpx = hwp5_char_shape_to_hwpx(&raw);
    assert!(hwpx.shade_color.is_none(), "sentinel 0xFFFFFFFF must map to None");
}

// ──────────────────────────────────────────────────────────────────────────
// underline_shape variants (style_store_convert.rs)
// Bit layout: underline_type = bits 2-3 ((property >> 2) & 0b11), value 1 = Bottom
//             underline_shape_raw = bits 4-7 ((property >> 4) & 0b1111)
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn hwp5_char_shape_underline_shape_variants_all_map_correctly() {
    use hwpforge_foundation::UnderlineShape;

    // Bottom underline is active when bits 2-3 = 0b01 → (1 << 2)
    let underline_active = 1u32 << 2;

    let cases: &[(u32, UnderlineShape)] = &[
        (0 << 4, UnderlineShape::Solid),
        (1 << 4, UnderlineShape::Dash),
        (2 << 4, UnderlineShape::Dot),
        (3 << 4, UnderlineShape::DashDot),
        (4 << 4, UnderlineShape::DashDotDot),
        (5 << 4, UnderlineShape::LongDash),
        (6 << 4, UnderlineShape::Circle),
        (7 << 4, UnderlineShape::DoubleSlim),
        (8 << 4, UnderlineShape::SlimThick),
        (9 << 4, UnderlineShape::ThickSlim),
        (10 << 4, UnderlineShape::ThickSlimThick),
        (11 << 4, UnderlineShape::Wave),
        (12 << 4, UnderlineShape::Solid), // fallback: raw 12 → Solid
    ];

    for &(shape_bits, expected) in cases {
        let mut raw = default_char_shape();
        raw.property = underline_active | shape_bits;
        let hwpx = hwp5_char_shape_to_hwpx(&raw);
        assert_eq!(
            hwpx.underline_shape, expected,
            "shape_bits=0x{shape_bits:04X}: expected {expected:?}"
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────
// outline_kind warning (style_store_convert.rs L285-291)
// outline_kind_raw = bits 8-10 ((property >> 8) & 0b111)
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn hwp5_char_shape_warns_on_outline_kind_when_nonzero() {
    // Non-zero outline kind collapses to Solid and emits a ProjectionFallback.
    // We exercise the warning path by calling through the full store conversion.
    let mut raw = default_char_shape();
    raw.property = 3u32 << 8; // outline_kind_raw = 3 (non-zero)

    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![raw],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    let (hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    // The char shape must be produced (store is non-empty)
    assert!(hwpx_store.char_shape(CharShapeIndex::new(0)).is_ok());
    // A ProjectionFallback must be emitted for the outline kind
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            Hwp5Warning::ProjectionFallback { subject, .. }
                if *subject == "style.char_shape.outline_kind"
        )),
        "non-zero outline_kind must emit ProjectionFallback: {warnings:?}"
    );
}

/// A store with two border fills — slot 1 without lines, slot 2 a solid box
/// — and the given char shape border ids.
fn store_with_char_borders(borders: &[Option<u16>]) -> Hwp5StyleStore {
    let line = |kind| Hwp5RawBorderLine { kind, width: 1, color: 0 };
    let fill = |kind| Hwp5RawBorderFill {
        property: 0,
        three_d: false,
        shadow: false,
        slash_diagonal_shape: 0,
        back_slash_diagonal_shape: 0,
        center_line: false,
        left: line(kind),
        right: line(kind),
        top: line(kind),
        bottom: line(kind),
        diagonal: none_border_line(),
        fill: Hwp5RawBorderFillFill::None,
    };
    let mut doc_info = empty_doc_info();
    doc_info.char_shapes = borders
        .iter()
        .map(|b| Hwp5RawCharShape { border_fill_id: *b, ..default_char_shape() })
        .collect();
    doc_info.border_fills = vec![
        border_fill_slot(1, fill(Hwp5BorderLineKind::None)),
        border_fill_slot(2, fill(Hwp5BorderLineKind::Solid)),
    ];
    Hwp5StyleStore::from_doc_info(&doc_info)
}

/// A char shape whose border is 0 or absent points at an empty border fill
/// appended after the document's own; one pointing at slot 2 keeps slot 2
/// (slot 2 is the document's solid box here, not "no border").
///
/// 이것을 실패시키는 것: `!= 2` 필터 복귀(2 가 `None` 이 되어 실패) / 덧붙이기를
/// 지우기 / 덧붙인 것에 선 넣기.
#[test]
fn char_shape_without_a_border_points_at_an_empty_border_fill() {
    let (out, _) =
        crate::hwp5_style_store_to_hwpx(&store_with_char_borders(&[Some(0), None, Some(2)]))
            .unwrap();
    assert_eq!(out.border_fill_count(), 3);
    let zero = out.char_shape(CharShapeIndex::new(0)).unwrap().border_fill_id;
    assert_eq!(zero, Some(3));
    assert_eq!(out.char_shape(CharShapeIndex::new(1)).unwrap().border_fill_id, Some(3));
    assert_eq!(out.char_shape(CharShapeIndex::new(2)).unwrap().border_fill_id, Some(2));
    let empty = out.border_fill(3).unwrap();
    for side in [&empty.left, &empty.right, &empty.top, &empty.bottom] {
        assert_eq!(side.line_type, "NONE");
    }
    assert!(empty.fill.is_none() && empty.gradient_fill.is_none() && empty.image_fill.is_none());
}

/// Without a borderless char shape nothing is appended.
///
/// 이것을 실패시키는 것: 빈 테두리를 무조건 덧붙이기.
#[test]
fn no_empty_border_fill_without_a_borderless_char_shape() {
    let (out, _) =
        crate::hwp5_style_store_to_hwpx(&store_with_char_borders(&[Some(1), Some(2)])).unwrap();
    assert_eq!(out.border_fill_count(), 2);
}

fn face(property: u8, alternate: Option<u8>, panose: Option<[u8; 10]>) -> Hwp5RawFaceName {
    Hwp5RawFaceName {
        property,
        face_name: format!("F{property:02X}"),
        alternate_font_type: alternate,
        alternate_font_name: alternate.map(|_| "대체".to_string()),
        panose1: panose,
        default_font_name: None,
    }
}

fn faces_to_hwpx(
    faces: Vec<Hwp5RawFaceName>,
) -> (Vec<hwpforge_smithy_hwpx::HwpxFont>, Vec<Hwp5Warning>) {
    let count = faces.len();
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: faces,
        char_shapes: vec![],
        para_shapes: vec![],
        numberings: vec![],
        bullets: vec![],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };
    let (hwpx, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    let fonts = (0..count)
        .map(|i| hwpx.font(hwpforge_foundation::FontIndex::new(i)).unwrap().clone())
        .collect();
    (fonts, warnings)
}

fn font_warnings(warnings: &[Hwp5Warning]) -> Vec<&'static str> {
    warnings
        .iter()
        .filter_map(|w| match w {
            Hwp5Warning::ProjectionFallback { subject, .. }
                if subject.starts_with("style.font.") =>
            {
                Some(*subject)
            }
            _ => None,
        })
        .collect()
}

/// C: a face's kind (property bits 0-1), substitute (0x80, type 0/1/2) and
/// classification (PANOSE byte 0 → family type, bytes 2-9 → the eight
/// numbers) reach the HWPX font with no warning.
/// 이것을 실패시키는 것: 하위 비트 대신 0x80 비트 읽기 · 대체 유형 1·2
/// 뒤바꿈 · PANOSE 바이트 하나 밀림 · 계열 표 한 칸 밀림(3 → BRUSHSCRIPT) ·
/// 지원값에 경고.
#[test]
fn hwp5_face_metadata_maps() {
    use hwpforge_smithy_hwpx::{HwpxFontType, HwpxSubstFont};
    let panose = |family: u8| [family, 9, 10, 11, 12, 13, 14, 15, 16, 17];
    let mut faces = vec![
        face(0x61, None, Some(panose(2))),
        face(0x62, None, None),
        face(0x60, None, None),
        face(0x81, Some(0), None),
        face(0x81, Some(1), None),
        face(0x82, Some(2), None),
    ];
    for family in 0..8 {
        faces.push(face(0x41, None, Some(panose(family))));
    }
    let (fonts, warnings) = faces_to_hwpx(faces);
    assert!(font_warnings(&warnings).is_empty(), "{warnings:?}");
    let kinds: Vec<_> = fonts[..6].iter().map(|f| f.font_type).collect();
    assert_eq!(
        kinds,
        [
            Some(HwpxFontType::Ttf),
            Some(HwpxFontType::Hft),
            None,
            Some(HwpxFontType::Ttf),
            Some(HwpxFontType::Ttf),
            Some(HwpxFontType::Hft)
        ]
    );
    let info = fonts[0].type_info.as_ref().unwrap();
    assert_eq!(
        (info.family_type.as_str(), info.values),
        ("FCAT_GOTHIC", [10, 11, 12, 13, 14, 15, 16, 17])
    );
    assert_eq!(fonts[1].type_info, None);
    let subs: Vec<_> = fonts[2..6].iter().map(|f| f.substitute.clone()).collect();
    assert_eq!(
        subs,
        [
            None,
            Some(HwpxSubstFont::new(None, "대체")),
            Some(HwpxSubstFont::new(Some(HwpxFontType::Ttf), "대체")),
            Some(HwpxSubstFont::new(Some(HwpxFontType::Hft), "대체")),
        ]
    );
    let families: Vec<String> =
        fonts[6..].iter().map(|f| f.type_info.as_ref().unwrap().family_type.clone()).collect();
    assert_eq!(
        families,
        [
            "FCAT_UNKNOWN",
            "FCAT_MYUNGJO",
            "FCAT_GOTHIC",
            "FCAT_SSERIF",
            "FCAT_BRUSHSCRIPT",
            "FCAT_DECORATIVE",
            "FCAT_NONRECTMJ",
            "FCAT_NONRECTGT"
        ]
    );
}

/// C: values HWPX cannot carry — kind 3, substitute type 3, PANOSE family
/// type 8 — each give one warning; the kind is unknown, the substitute is
/// dropped, the family type is `FCAT_UNKNOWN`. Faces repeat across the seven
/// language groups but warn once.
/// 이것을 실패시키는 것: 세 경고 중 하나 삭제 · 언어 무리마다 경고.
#[test]
fn hwp5_face_metadata_unsupported_values_warn() {
    let (fonts, warnings) = faces_to_hwpx(vec![
        face(0x43, None, Some([8, 0, 1, 1, 1, 1, 1, 1, 1, 1])),
        face(0x81, Some(3), None),
    ]);
    assert_eq!(
        font_warnings(&warnings),
        ["style.font.font_type", "style.font.type_info", "style.font.substitute"],
        "{warnings:?}"
    );
    assert_eq!(fonts[0].font_type, None);
    assert_eq!(fonts[0].type_info.as_ref().unwrap().family_type, "FCAT_UNKNOWN");
    assert_eq!(fonts[1].substitute, None);
}

#[test]
fn w1_hwp5_packed_margins_and_tabs_keep_units_and_reject_ranges() {
    // 이것을 실패시키는 것: 변이 1·2·3·12 — 음수 /2 절삭, CHAR 삭제, clamp.
    for value in [-1499, -1, 1, 2001] {
        let mut raw = default_para_shape();
        raw.left_margin = value;
        let p = hwp5_para_shape_to_hwpx(&raw).unwrap();
        assert_eq!(p.margin_left.as_char_count().unwrap().ticks(), value >> 1);
    }
    let mut raw = default_para_shape();
    raw.left_margin = 200_000_002;
    assert!(hwp5_para_shape_to_hwpx(&raw).is_err());
    let tab = |value| Hwp5RawTabDef {
        property: 0,
        tab_stops: vec![hwpforge_smithy_hwp5::schema::header::Hwp5RawTabStop {
            position: value,
            tab_type: 0,
            fill_type: 0,
        }],
    };
    assert_eq!(
        hwp5_tab_def_to_hwpx(3, &tab(u32::MAX)).unwrap().stops[0]
            .position
            .as_char_count()
            .unwrap()
            .ticks(),
        i32::MAX
    );
    assert!(hwp5_tab_def_to_hwpx(3, &tab(200_000_002)).is_err());
    let mut raw = default_para_shape();
    raw.property1 = 1;
    raw.line_spacing = 2001;
    assert_eq!(hwp5_para_shape_to_hwpx(&raw).unwrap().line_spacing, 2001);
    raw.line_spacing2 = Some(u32::MAX);
    assert!(hwp5_para_shape_to_hwpx(&raw).is_err());
}

#[test]
fn fix2_missing_font_reference_is_not_clamped() {
    // 이것을 실패시키는 것: 없는 참조의 마지막 글꼴/0 대체, 언어 검사 생략,
    // 빈 언어 그룹에 font 0 외에 다른 언어 그룹의 모든 글꼴 복제.
    for fonts in [vec![], vec![Hwp5RawFaceName::parse(&[0, 1, 0, b'A', 0]).unwrap()]] {
        let mut store = Hwp5StyleStore::from_doc_info(&empty_doc_info());
        store.fonts = fonts;
        store.char_shapes = vec![default_char_shape(); 18];
        assert!(crate::hwp5_style_store_to_hwpx(&store).is_ok());
        for lang in 0..7 {
            for missing in [1, 9] {
                store.char_shapes[17].font_ids[lang] = missing;
                let err = crate::hwp5_style_store_to_hwpx(&store).unwrap_err().to_string();
                assert!(
                    err.contains("charPr 17") && err.contains(&format!("ID {missing}")),
                    "{err}"
                );
                store.char_shapes[17].font_ids[lang] = 0;
            }
        }
    }
}

#[test]
fn fix3_all_header_reference_kinds_use_strict_lookup() {
    // 이것을 실패시키는 것: 공통 검증 호출 삭제 또는 종류별 조회를 0/기존 정의로 대체.
    let mut valid = Hwp5StyleStore::from_doc_info(&empty_doc_info());
    valid.char_shapes.push(default_char_shape());
    valid.para_shapes.push(default_para_shape());
    valid.styles.push(Hwp5RawStyle {
        name: "base".into(),
        english_name: "base".into(),
        kind: 0,
        next_style_id: 0,
        lang_id: 1042,
        para_shape_id: 0,
        char_shape_id: 0,
        lock_form: 0,
    });
    assert!(crate::hwp5_style_store_to_hwpx(&valid).is_ok());
    for kind in [
        "font",
        "charPr",
        "paraPr",
        "tabPr",
        "bullet",
        "numbering",
        "char borderFill",
        "para borderFill",
    ] {
        let mut store = valid.clone();
        match kind {
            "font" => store.char_shapes[0].font_ids[0] = 99,
            "charPr" => store.styles[0].char_shape_id = 99,
            "paraPr" => store.styles[0].para_shape_id = 99,
            "tabPr" => store.para_shapes[0].tab_def_id = 99,
            "bullet" | "numbering" => {
                store.para_shapes[0].property1 = (if kind == "bullet" { 3 } else { 2 }) << 23;
                store.para_shapes[0].numbering_bullet_id = 99;
            }
            "char borderFill" => store.char_shapes[0].border_fill_id = Some(99),
            "para borderFill" => store.para_shapes[0].border_fill_id = 99,
            _ => unreachable!(),
        }
        let error = crate::hwp5_style_store_to_hwpx(&store).unwrap_err().to_string();
        assert!(
            error.contains(kind.split_whitespace().last().unwrap()) && error.contains("ID 99"),
            "{kind}: {error}"
        );
    }
    // Inactive CHAR-style paraPr and zero list/border sentinels are not lookups.
    valid.styles[0].kind = 1;
    valid.styles[0].para_shape_id = 99;
    assert!(crate::hwp5_style_store_to_hwpx(&valid).is_ok());
}

#[test]
fn fix3_output_only_border_cannot_satisfy_source_reference() {
    // 이것을 실패시키는 것: 원본 참조 검증 전에 기본/빈 테두리 생성 또는 output=true 검증.
    for missing in [1, 2, 3, 4] {
        let mut store = Hwp5StyleStore::from_doc_info(&empty_doc_info());
        store.char_shapes = vec![default_char_shape(), default_char_shape()];
        store.char_shapes[1].border_fill_id = Some(missing);
        let error = crate::hwp5_style_store_to_hwpx(&store).unwrap_err().to_string();
        assert!(
            error.contains("borderFill") && error.contains(&format!("ID {missing}")),
            "{error}"
        );
    }
}

#[test]
fn fix4_list_head_char_reference_survives_parsing_until_lookup() {
    // 이것을 실패시키는 것: 머리의 charShape ID 폐기/0 치환,
    // numbering 또는 bullet의 공통 조회 생략, 보존된 참조를 다시 버리기.
    for kind in ["numbering", "bullet"] {
        for id in [0u32, 99, u32::MAX] {
            let mut bytes = make_bullet_def_bytes(false);
            bytes[8..12].copy_from_slice(&id.to_le_bytes());
            let parsed = Hwp5RawBulletDef::parse(&bytes).unwrap();
            assert_eq!(parsed.paragraph_head.char_shape_id, id);
            let mut store = image_fill_store();
            store.border_fills.clear();
            store.char_shapes.push(default_char_shape());
            if kind == "numbering" {
                store.numberings.push(numbering_slot(
                    1,
                    Hwp5RawNumberingDef { start: 1, paragraph_heads: vec![parsed.paragraph_head] },
                ));
            } else {
                store.bullets.push(bullet_slot(1, parsed));
            }
            let result = crate::hwp5_style_store_to_hwpx(&store);
            if id == 99 {
                assert!(result.unwrap_err().to_string().contains("charPr"));
            } else {
                let (converted, warnings) = result.unwrap();
                assert!(!warnings.iter().any(|w| matches!(
                    w,
                    Hwp5Warning::ParserFallback { subject: "list_head.charPr", .. }
                )));
                let value = serde_json::to_value(&converted).unwrap();
                assert_eq!(
                    value["header_fidelity"][kind]["1"]
                        [if kind == "numbering" { "1" } else { "0" }]["char_pr_id_ref"],
                    id
                );
            }
        }
    }
}

#[test]
fn fix8b_failed_list_slots_rebind_later_references_and_reject_failed_slot() {
    // 이것을 실패시키는 것: 실패 슬롯 압축 후 참조 유지, 실패 슬롯을 뒤 정의로 대체,
    // 0 sentinel 재결합, NUMBER/BULLET 재결합 누락 또는 OUTLINE 상속 계약 변경.
    for heading in [HeadingType::Number, HeadingType::Outline, HeadingType::Bullet] {
        let mut store = Hwp5StyleStore::from_doc_info(&empty_doc_info());
        store.char_shapes.push(default_char_shape());
        store.para_shapes.push(default_para_shape());
        let mut raw = Hwp5RawBulletDef::parse(&make_bullet_def_bytes(false)).unwrap();
        raw.paragraph_head.text = "survivor".into();
        store.numberings = vec![
            Hwp5DocInfoNumberingSlot { id: 1, numbering: None },
            numbering_slot(
                2,
                Hwp5RawNumberingDef { start: 7, paragraph_heads: vec![raw.paragraph_head.clone()] },
            ),
            Hwp5DocInfoNumberingSlot { id: 3, numbering: None },
            numbering_slot(
                4,
                Hwp5RawNumberingDef { start: 9, paragraph_heads: vec![raw.paragraph_head.clone()] },
            ),
        ];
        store.bullets = vec![
            Hwp5DocInfoBulletSlot { id: 1, bullet: None },
            bullet_slot(2, raw.clone()),
            Hwp5DocInfoBulletSlot { id: 3, bullet: None },
            bullet_slot(4, raw),
        ];
        store.para_shapes[0].property1 = match heading {
            HeadingType::Outline => 1,
            HeadingType::Number => 2,
            _ => 3,
        } << 23;
        for (source, expected) in [(0, 0), (2, 1), (4, 2)] {
            store.para_shapes[0].numbering_bullet_id = source;
            let (out, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
            let expected = if heading == HeadingType::Outline { 0 } else { expected };
            assert_eq!(out.para_shape(ParaShapeIndex::new(0)).unwrap().heading_id_ref, expected);
            assert!(warnings.iter().any(|w| matches!(
                w,
                Hwp5Warning::ParserFallback { subject: "numbering.slot", .. }
            )));
            assert!(warnings
                .iter()
                .any(|w| matches!(w, Hwp5Warning::ParserFallback { subject: "bullet.slot", .. })));
        }
        for source in [1, 3, 5] {
            store.para_shapes[0].numbering_bullet_id = source;
            // Existing HWP5 OUTLINE projection always inherits section numbering (0).
            if heading == HeadingType::Outline {
                let (out, _) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
                assert_eq!(out.para_shape(ParaShapeIndex::new(0)).unwrap().heading_id_ref, 0);
                continue;
            }
            let error = crate::hwp5_style_store_to_hwpx(&store).unwrap_err().to_string();
            assert!(error.contains(&format!("ID {source}")), "{error}");
        }
    }
}

#[test]
fn legacy_bullet_record_projects_and_encodes_glyph() {
    // 이것을 실패시키는 것: 실제 14바이트 레코드 거부, glyph/slot/heading 손실.
    // HWP 5.0.1.7, 1111 대기업참여하한개정 행정예고: BULLET payload only.
    const BULLET: [u8; 14] = [0x08, 0, 0, 0, 0, 0, 0x32, 0, 0xff, 0xff, 0xff, 0xff, 0xa1, 0xf0];
    let raw = Hwp5RawBulletDef::parse(&BULLET).unwrap();
    assert_eq!(raw.bullet_char, "\u{f0a1}");
    assert_eq!(raw.paragraph_head.char_shape_id, u32::MAX);
    assert!(!raw.use_image);
    assert_eq!(raw.image_id, None);
    assert_eq!(raw.check_bullet_char, None);
    let mut source = Hwp5StyleStore::from_doc_info(&empty_doc_info());
    source.char_shapes.push(default_char_shape());
    source.para_shapes.push(default_para_shape());
    source.para_shapes[0].property1 = 3 << 23;
    source.para_shapes[0].numbering_bullet_id = 1;
    source.bullets.push(bullet_slot(1, raw));
    let (store, warnings) = crate::hwp5_style_store_to_hwpx(&source).unwrap();
    assert!(!warnings.iter().any(|w| matches!(w, Hwp5Warning::ParserFallback { subject, .. } if subject.starts_with("bullet"))));
    assert_eq!(store.iter_bullets().next().unwrap().bullet_char, "\u{f0a1}");
    let bytes = encode_test_header(&store);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("Contents/header.xml").unwrap(), &mut xml)
        .unwrap();
    assert!(xml.contains("<hh:bullet id=\"1\" char=\"\u{f0a1}\""), "{xml}");
    assert!(xml.contains("type=\"BULLET\" idRef=\"1\""), "{xml}");
    // A genuinely absent slot must still be rejected.
    source.bullets.clear();
    assert!(crate::hwp5_style_store_to_hwpx(&source)
        .unwrap_err()
        .to_string()
        .contains("broken bullet reference ID 1"));
}

fn encode_test_header(store: &hwpforge_smithy_hwpx::HwpxStyleStore) -> Vec<u8> {
    use hwpforge_core::{Document, ImageStore, PageSettings, Paragraph, Run, Section};
    let mut document = Document::new();
    document.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("body", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    hwpforge_smithy_hwpx::HwpxEncoder::encode(
        &document.validate().unwrap(),
        store,
        &ImageStore::new(),
    )
    .unwrap()
}

#[test]
fn next_style_recovery_matches_hwpx_and_preserves_valid_boundaries() {
    // 이것을 실패시키는 것: HWP5 보정 미호출, self 대신 0, 경고 종류/문구 분기, >= 를 > 로 변경.
    use hwpforge_smithy_hwpx::{DecodeWarning, HwpxDecoder};
    for next in [0, 18, 19, 53] {
        let mut source = Hwp5StyleStore::from_doc_info(&empty_doc_info());
        source.char_shapes.push(default_char_shape());
        source.para_shapes.push(default_para_shape());
        source.styles = (0..19)
            .map(|id| Hwp5RawStyle {
                name: format!("style{id}"),
                english_name: format!("style{id}"),
                kind: 1,
                next_style_id: if id == 17 { next } else { id },
                lang_id: 1042,
                para_shape_id: 99,
                char_shape_id: 0,
                lock_form: 0,
            })
            .collect();
        let (store, warnings) = crate::hwp5_style_store_to_hwpx(&source).unwrap();
        let expected = if next >= 19 { 17 } else { u32::from(next) };
        assert_eq!(store.iter_styles().nth(17).unwrap().next_style_id_ref, expected);
        let losses: Vec<_> = warnings
            .iter()
            .filter_map(|w| match w {
                Hwp5Warning::ContentLost { kind, reason, .. } => {
                    Some((kind.as_str(), reason.as_str()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(losses.len(), usize::from(next >= 19));
        let bytes = encode_test_header(&store);
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        assert_eq!(decoded.style_store.iter_styles().nth(17).unwrap().next_style_id_ref, expected);
        if next >= 19 {
            // Feed the same invalid next position through the HWPX decoder.
            let mut input = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
            let mut output = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for i in 0..input.len() {
                let mut entry = input.by_index(i).unwrap();
                let mut data = Vec::new();
                std::io::Read::read_to_end(&mut entry, &mut data).unwrap();
                if entry.name().unwrap() == "Contents/header.xml" {
                    let xml = String::from_utf8(data).unwrap();
                    data = xml
                        .replace("nextStyleIDRef=\"17\"", &format!("nextStyleIDRef=\"{next}\""))
                        .into_bytes();
                }
                output
                    .start_file(entry.name().unwrap(), zip::write::SimpleFileOptions::default())
                    .unwrap();
                std::io::Write::write_all(&mut output, &data).unwrap();
            }
            let changed = output.finish().unwrap().into_inner();
            let decoded = HwpxDecoder::decode(&changed).unwrap();
            let hwpx_losses: Vec<_> = decoded
                .warnings
                .iter()
                .filter_map(|w| match w {
                    DecodeWarning::ContentLost { kind, reason, .. }
                        if kind == "style.nextStyleIDRef" =>
                    {
                        Some((kind.as_str(), reason.as_str()))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(losses, hwpx_losses);
            assert_eq!(losses[0].0, "style.nextStyleIDRef");
            assert_eq!(losses[0].1, format!("style position 17: nextStyleIDRef {next} out of range 19; replaced with self 17"));
            assert_eq!(decoded.style_store.iter_styles().nth(17).unwrap().next_style_id_ref, 17);
        }
    }
}

// 이것을 실패시키는 것: signed widthAdjust/textOffset 또는 같은 레코드 속성을 버리기,
// source slot ID로 metrics를 연결하기, JSON에서 보존값을 잃기.
#[test]
fn paragraph_head_metrics_survive_hwp5_conversion_json_and_hwpx() {
    use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxStyleStore, PackageReader};
    for kind in ["numbering", "bullet"] {
        for attributes in [8u32, 2 | 4 | 16, 3 | 8] {
            let mut bytes = make_bullet_def_bytes(false);
            bytes[..4].copy_from_slice(&attributes.to_le_bytes());
            bytes[4..6].copy_from_slice(&(-700i16).to_le_bytes());
            bytes[6..8].copy_from_slice(&(-25i16).to_le_bytes());
            bytes[8..12].copy_from_slice(&0u32.to_le_bytes());
            let raw = Hwp5RawBulletDef::parse(&bytes).unwrap();
            assert_eq!(raw.paragraph_head.width_adjust, -700);
            assert_eq!(raw.paragraph_head.text_offset, -25);
            assert_eq!(raw.paragraph_head.attributes, attributes);
            let mut source = Hwp5StyleStore::from_doc_info(&empty_doc_info());
            source.char_shapes.push(default_char_shape());
            source.para_shapes.push(default_para_shape());
            if kind == "numbering" {
                source.numberings = vec![
                    Hwp5DocInfoNumberingSlot { id: 1, numbering: None },
                    numbering_slot(
                        2,
                        Hwp5RawNumberingDef { start: 1, paragraph_heads: vec![raw.paragraph_head] },
                    ),
                ];
            } else {
                source.bullets =
                    vec![Hwp5DocInfoBulletSlot { id: 1, bullet: None }, bullet_slot(2, raw)];
            }
            let (store, warnings) = crate::hwp5_style_store_to_hwpx(&source).unwrap();
            assert_eq!(
                warnings.iter().any(|w| matches!(
                    w,
                    Hwp5Warning::ProjectionFallback { subject: "list_head.align", .. }
                )),
                attributes & 3 == 3
            );
            let stored: HwpxStyleStore =
                serde_json::from_str(&serde_json::to_string(&store).unwrap()).unwrap();
            let bytes = encode_test_header(&stored);
            let header = PackageReader::new(&bytes).unwrap().read_header_xml().unwrap();
            for attr in [
                r#"widthAdjust="-700""#,
                r#"textOffset="-25""#,
                r#"charPrIDRef="0""#,
                if attributes & 16 != 0 {
                    r#"textOffsetType="HWPUNIT""#
                } else {
                    r#"textOffsetType="PERCENT""#
                },
                if attributes & 4 != 0 { r#"useInstWidth="1""# } else { r#"useInstWidth="0""# },
                if attributes & 8 != 0 { r#"autoIndent="1""# } else { r#"autoIndent="0""# },
                if attributes & 3 == 2 { r#"align="RIGHT""# } else { r#"align="LEFT""# },
            ] {
                assert!(header.contains(attr), "{kind}/{attr}: {header}");
            }
            let reread = HwpxDecoder::decode(&bytes).unwrap();
            let output = encode_test_header(&reread.style_store);
            let output_header = PackageReader::new(&output).unwrap().read_header_xml().unwrap();
            assert_eq!(header, output_header);
        }
    }
}
