use super::*;
use crate::decoder::header::parse_doc_info;
use crate::decoder::package::PackageReader;
use crate::schema::border_fill::{
    Hwp5BorderLineKind, Hwp5FillImageEffect, Hwp5FillImageMode, Hwp5FillPatternKind,
    Hwp5GradationType, Hwp5RawBorderFill, Hwp5RawBorderFillFill, Hwp5RawBorderLine,
    Hwp5RawColorFill, Hwp5RawGradationFill, Hwp5RawImageFill,
};
use crate::style_store_convert::{
    bgr_colorref_to_color, hwp5_char_shape_to_hwpx, hwp5_para_shape_to_hwpx, hwp5_tab_def_to_hwpx,
};
use hwpforge_foundation::{Color, GradientType, ParaShapeIndex, TabAlign};
use hwpforge_smithy_hwpx::style_store::HwpxFill;
use std::path::PathBuf;

fn border_fill_slot(id: u32, fill: Hwp5RawBorderFill) -> Hwp5DocInfoBorderFillSlot {
    Hwp5DocInfoBorderFillSlot { id, fill: Some(fill) }
}

fn parsed_tab_slot(raw_id: u32, tab_def: Hwp5RawTabDef) -> Hwp5TabDefSlot {
    Hwp5TabDefSlot::parsed(raw_id, tab_def)
}

fn invalid_tab_slot(raw_id: u32) -> Hwp5TabDefSlot {
    Hwp5TabDefSlot::invalid(raw_id)
}

fn fixture_doc_info(name: &str) -> DocInfoResult {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name);
    assert!(path.exists(), "fixture must exist at {:?}", path);
    let bytes = std::fs::read(&path).expect("read hwp fixture");
    let package = PackageReader::open(&bytes).expect("open hwp package");
    parse_doc_info(package.doc_info_data(), &package.file_header().version)
        .expect("parse fixture docinfo")
}

fn fixture_image_fill(name: &str) -> Hwp5RawImageFill {
    fixture_doc_info(name)
        .border_fills
        .iter()
        .find_map(|slot| match slot.fill.as_ref()?.fill {
            Hwp5RawBorderFillFill::Image(ref fill) => Some(fill.clone()),
            _ => None,
        })
        .expect("fixture must contain image border fill")
}

fn none_border_line() -> Hwp5RawBorderLine {
    Hwp5RawBorderLine { kind: Hwp5BorderLineKind::None, width: 0, color: 0x00000000 }
}

impl Hwp5RawCharShape {
    /// Convenience constructor for tests — all zeros / safe defaults.
    pub(crate) fn default_for_test() -> Self {
        Self {
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
            shade_color: 0xFFFF_FFFF, // "none"
            shadow_color: 0x000000,
            border_fill_id: None,
            strike_color: None,
        }
    }
}

impl Hwp5RawParaShape {
    /// Convenience constructor for tests — all zeros / safe defaults.
    pub(crate) fn default_for_test() -> Self {
        Self {
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
}

fn empty_doc_info() -> DocInfoResult {
    DocInfoResult {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
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
        char_shapes: vec![Hwp5RawCharShape::default_for_test()],
        para_shapes: vec![Hwp5RawParaShape::default_for_test()],
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
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };
    let hwpx_store = store.to_hwpx_style_store();
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
        char_shapes: vec![Hwp5RawCharShape::default_for_test()],
        para_shapes: vec![Hwp5RawParaShape::default_for_test()],
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
    let hwpx_store = store.to_hwpx_style_store();
    assert_eq!(hwpx_store.font_count(), 1);
    assert_eq!(hwpx_store.char_shape_count(), 1);
    assert_eq!(hwpx_store.para_shape_count(), 1);
    assert_eq!(hwpx_store.style_count(), 1);
    assert_eq!(hwpx_store.style(0).unwrap().name, "본문");
}

#[test]
fn to_hwpx_style_store_uses_id_mappings_font_buckets() {
    let mut raw = Hwp5RawCharShape::default_for_test();
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
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    let hwpx_store = store.to_hwpx_style_store();
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
    let mut raw = Hwp5RawCharShape::default_for_test();
    raw.property = 0b11; // bold + italic
    let hwpx = hwp5_char_shape_to_hwpx(&raw);
    assert!(hwpx.bold);
    assert!(hwpx.italic);
}

#[test]
fn hwp5_char_shape_not_bold_not_italic() {
    let raw = Hwp5RawCharShape::default_for_test();
    let hwpx = hwp5_char_shape_to_hwpx(&raw);
    assert!(!hwpx.bold);
    assert!(!hwpx.italic);
}

#[test]
fn hwp5_para_shape_alignment_justify() {
    let raw = Hwp5RawParaShape::default_for_test(); // property1 bits 2-4 = 0 => Justify
    let hwpx = hwp5_para_shape_to_hwpx(&raw);
    assert_eq!(hwpx.alignment, hwpforge_foundation::Alignment::Justify);
}

#[test]
fn hwp5_para_shape_alignment_left() {
    let mut raw = Hwp5RawParaShape::default_for_test();
    raw.property1 = 1 << 2; // bits 2-4 = 1 => Left
    let hwpx = hwp5_para_shape_to_hwpx(&raw);
    assert_eq!(hwpx.alignment, hwpforge_foundation::Alignment::Left);
}

#[test]
fn hwp5_para_shape_alignment_center() {
    let mut raw = Hwp5RawParaShape::default_for_test();
    raw.property1 = 3 << 2; // bits 2-4 = 3 => Center
    let hwpx = hwp5_para_shape_to_hwpx(&raw);
    assert_eq!(hwpx.alignment, hwpforge_foundation::Alignment::Center);
}

#[test]
fn hwp5_para_shape_keeps_builtin_tab_ids() {
    for tab_def_id in 0..=2 {
        let mut raw = Hwp5RawParaShape::default_for_test();
        raw.tab_def_id = tab_def_id;
        let hwpx = hwp5_para_shape_to_hwpx(&raw);
        assert_eq!(hwpx.tab_pr_id_ref, tab_def_id as u32);
    }
}

#[test]
fn hwp5_para_shape_preserves_custom_tab_ids() {
    let mut raw = Hwp5RawParaShape::default_for_test();
    raw.tab_def_id = 3;
    let hwpx = hwp5_para_shape_to_hwpx(&raw);
    assert_eq!(hwpx.tab_pr_id_ref, 3);
}

#[test]
fn hwp5_tab_def_maps_stops_and_auto_flags() {
    let raw = Hwp5RawTabDef {
        property: 0b11,
        tab_stops: vec![
            crate::schema::header::Hwp5RawTabStop { position: 4000, tab_type: 0, fill_type: 2 },
            crate::schema::header::Hwp5RawTabStop { position: 8000, tab_type: 3, fill_type: 5 },
        ],
    };

    let hwpx = hwp5_tab_def_to_hwpx(3, &raw);
    assert_eq!(hwpx.id, 3);
    assert!(hwpx.auto_tab_left);
    assert!(hwpx.auto_tab_right);
    assert_eq!(hwpx.stops.len(), 2);
    assert_eq!(hwpx.stops[0].position.as_i32(), 4000);
    assert_eq!(hwpx.stops[0].align, TabAlign::Left);
    assert_eq!(hwpx.stops[0].leader.as_hwpx_str(), "DOT");
    assert_eq!(hwpx.stops[1].align, TabAlign::Decimal);
    assert_eq!(hwpx.stops[1].leader.as_hwpx_str(), "LONG_DASH");
}

#[test]
fn to_hwpx_style_store_carries_tab_defs() {
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        tab_defs: vec![parsed_tab_slot(
            0,
            Hwp5RawTabDef {
                property: 0b01,
                tab_stops: vec![crate::schema::header::Hwp5RawTabStop {
                    position: 12000,
                    tab_type: 1,
                    fill_type: 1,
                }],
            },
        )],
        styles: vec![],
        border_fills: vec![],
    };

    let hwpx_store = store.to_hwpx_style_store();
    let tabs: Vec<_> = hwpx_store.iter_tabs().cloned().collect();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].id, 0);
    assert!(tabs[0].auto_tab_left);
    assert_eq!(tabs[0].stops.len(), 1);
    assert_eq!(tabs[0].stops[0].align, TabAlign::Right);
    assert_eq!(tabs[0].stops[0].leader.as_hwpx_str(), "DASH");
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
        tab_defs: vec![parsed_tab_slot(0, Hwp5RawTabDef { property: 0, tab_stops: vec![] })],
        styles: vec![],
        border_fills: vec![],
    };

    let (_, warnings) = store.to_hwpx_style_store_with_warnings();
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
        tab_defs: vec![parsed_tab_slot(
            0,
            Hwp5RawTabDef {
                property: 0,
                tab_stops: vec![crate::schema::header::Hwp5RawTabStop {
                    position: 12000,
                    tab_type: 9,
                    fill_type: 99,
                }],
            },
        )],
        styles: vec![],
        border_fills: vec![],
    };

    let (_, warnings) = store.to_hwpx_style_store_with_warnings();
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
fn fixture_sample_tab_hwp_has_expected_raw_custom_tab_def() {
    let doc_info = fixture_doc_info("user_samples/sample-tab.hwp");
    assert_eq!(doc_info.tab_defs.len(), 4);

    let custom = doc_info.tab_defs[3].tab_def.as_ref().expect("custom slot should parse");
    assert_eq!(custom.property, 0);
    assert_eq!(custom.tab_stops.len(), 1);
    assert_eq!(custom.tab_stops[0].position, 30000);
    assert_eq!(custom.tab_stops[0].tab_type, 0);
    assert_eq!(custom.tab_stops[0].fill_type, 3);
}

#[test]
fn to_hwpx_style_store_warns_when_para_shape_references_missing_custom_tab_def() {
    let mut para = Hwp5RawParaShape::default_for_test();
    para.tab_def_id = 9;

    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![para],
        tab_defs: vec![parsed_tab_slot(0, Hwp5RawTabDef { property: 0, tab_stops: vec![] })],
        styles: vec![],
        border_fills: vec![],
    };

    let (hwpx_store, warnings) = store.to_hwpx_style_store_with_warnings();
    assert_eq!(hwpx_store.para_shape(ParaShapeIndex::new(0)).unwrap().tab_pr_id_ref, 0);
    assert!(warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, reason }
            if *subject == "tab_def.ref"
                && reason.contains("missing tab definition id 9")
    )));
}

#[test]
fn to_hwpx_style_store_emits_placeholder_for_invalid_tab_slot() {
    let mut para = Hwp5RawParaShape::default_for_test();
    para.tab_def_id = 3;

    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![para],
        tab_defs: vec![invalid_tab_slot(3)],
        styles: vec![],
        border_fills: vec![],
    };

    let (hwpx_store, warnings) = store.to_hwpx_style_store_with_warnings();
    assert_eq!(hwpx_store.para_shape(ParaShapeIndex::new(0)).unwrap().tab_pr_id_ref, 3);
    let tabs: Vec<_> = hwpx_store.iter_tabs().cloned().collect();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].id, 3);
    assert!(tabs[0].stops.is_empty());
    assert!(warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ParserFallback { subject, reason }
            if *subject == "tab_def.slot"
                && reason.contains("slot 3")
    )));
}

#[test]
fn to_hwpx_style_store_warns_and_clamps_out_of_range_tab_position() {
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
        tab_defs: vec![parsed_tab_slot(
            3,
            Hwp5RawTabDef {
                property: 0,
                tab_stops: vec![crate::schema::header::Hwp5RawTabStop {
                    position: (hwpforge_foundation::HwpUnit::MAX_VALUE as u32) + 1,
                    tab_type: 0,
                    fill_type: 0,
                }],
            },
        )],
        styles: vec![],
        border_fills: vec![],
    };

    let (hwpx_store, warnings) = store.to_hwpx_style_store_with_warnings();
    let tabs: Vec<_> = hwpx_store.iter_tabs().cloned().collect();
    assert_eq!(tabs[0].stops[0].position.as_i32(), hwpforge_foundation::HwpUnit::MAX_VALUE);
    assert!(warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, reason }
            if *subject == "tab_def.position"
                && reason.contains("out-of-range position")
    )));
}

#[test]
fn to_hwpx_style_store_preserves_builtin_para_shape_tab_refs_without_warning() {
    let mut para = Hwp5RawParaShape::default_for_test();
    para.tab_def_id = 2;

    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![para],
        tab_defs: vec![],
        styles: vec![],
        border_fills: vec![],
    };

    let (hwpx_store, warnings) = store.to_hwpx_style_store_with_warnings();
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

    let hwpx_store = store.to_hwpx_style_store();
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

    let hwpx_store = store.to_hwpx_style_store();
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

    let hwpx_store = store.to_hwpx_style_store();
    assert!(matches!(hwpx_store.border_fill(1).unwrap().fill, Some(HwpxFill::WinBrush { .. })));
    assert_eq!(hwpx_store.border_fill(1).unwrap().fill_hatch_style.as_deref(), Some("HORIZONTAL"));
}

#[test]
fn to_hwpx_style_store_materializes_gradient_fill() {
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
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

    let hwpx_store = store.to_hwpx_style_store();
    assert!(matches!(
        hwpx_store.border_fill(4).unwrap().gradient_fill,
        Some(ref fill)
            if fill.gradient_type == GradientType::Linear
                && fill.angle == 90
                && fill.center_x == 0
                && fill.center_y == 0
                && fill.step == 255
                && fill.step_center == 50
                && fill.alpha == 0
                && fill.colors == vec![Color::from_raw(0x00FF0000), Color::from_raw(0x0000FF00)]
    ));
    assert!(hwpx_store.border_fill(4).unwrap().fill.is_none());
}

#[test]
fn to_hwpx_style_store_materializes_image_fill() {
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
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

    let hwpx_store = store.to_hwpx_style_store();
    assert!(matches!(
        hwpx_store.border_fill(4).unwrap().image_fill,
        Some(ref fill)
            if fill.mode == "TILE"
                && fill.binary_item_id_ref == "BIN0001"
                && fill.bright == 0
                && fill.contrast == 0
                && fill.effect == "REAL_PIC"
                && fill.alpha == 0
    ));
    assert!(hwpx_store.border_fill(4).unwrap().fill.is_none());
    assert_eq!(store.border_fill_image_binary_ids().into_iter().collect::<Vec<_>>(), vec![1]);
}

#[test]
fn fixture_table_16_image_fill_reports_raw_image_fill_mode() {
    let image_fill = fixture_image_fill("table_16_image_fill.hwp");
    assert_eq!(image_fill.bindata_id, 1);
    assert_eq!(image_fill.mode, Hwp5FillImageMode::Resize);
}

#[test]
fn fixture_table_16b_image_fill_center_reports_raw_image_fill_mode() {
    let image_fill = fixture_image_fill("table_16b_image_fill_center.hwp");
    assert_eq!(image_fill.bindata_id, 1);
    assert_eq!(image_fill.mode, Hwp5FillImageMode::Center);
}

#[test]
fn fixture_table_16c_image_fill_tile_reports_raw_image_fill_mode() {
    let image_fill = fixture_image_fill("table_16c_image_fill_tile.hwp");
    assert_eq!(image_fill.bindata_id, 1);
    assert_eq!(image_fill.mode, Hwp5FillImageMode::TileAll);
}

#[test]
fn fixture_table_18_image_fill_zoom_reports_raw_image_fill_mode() {
    let image_fill = fixture_image_fill("table_18_public_document_composite.hwp");
    assert_eq!(image_fill.bindata_id, 1);
    assert_eq!(image_fill.mode, Hwp5FillImageMode::Zoom);
}

#[test]
fn fixture_table_17_diagonal_border_reports_raw_diagonal_shapes() {
    let doc_info = fixture_doc_info("table_17_diagonal_border.hwp");
    let custom = doc_info
        .border_fills
        .iter()
        .filter_map(|slot| slot.fill.as_ref().map(|fill| (slot.id, fill)))
        .find(|(_, fill)| fill.slash_diagonal_shape != 0 || fill.back_slash_diagonal_shape != 0)
        .expect("fixture must contain a custom diagonal border fill");
    assert_eq!(custom.0, 4);
    assert_eq!(custom.1.back_slash_diagonal_shape, 2);
}

#[test]
fn to_hwpx_style_store_unsupported_image_fill_mode_emits_warning_and_drops_fill() {
    let store = Hwp5StyleStore {
        id_mappings: None,
        fonts: vec![],
        char_shapes: vec![],
        para_shapes: vec![],
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
                    mode: Hwp5FillImageMode::CenterTop,
                    brightness: 0,
                    contrast: 0,
                    effect: Hwp5FillImageEffect::RealPic,
                    bindata_id: 1,
                    extra_data: Vec::new(),
                }),
            },
        )],
    };

    let (hwpx_store, warnings) = store.to_hwpx_style_store_with_warnings();
    assert!(hwpx_store.border_fill(4).unwrap().fill.is_none());
    assert!(warnings.iter().any(|warning| matches!(
        warning,
        Hwp5Warning::ProjectionFallback { subject, reason }
            if *subject == "style.border_fill.image_fill_mode"
                && reason.contains("border_fill_id=4")
                && reason.contains("CenterTop")
    )));
}
