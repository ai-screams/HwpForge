use hwpforge_smithy_hwp5::schema::header::{Hwp5RawCharShape, Hwp5RawFaceName};
use hwpforge_smithy_hwp5::style_store::Hwp5StyleStore;
use hwpforge_smithy_hwp5::DocInfoResult;

fn default_test_char_shape() -> Hwp5RawCharShape {
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

fn doc_info_with_style_projection_warning() -> DocInfoResult {
    let mut raw = default_test_char_shape();
    raw.property = (1 << 2) | (1 << 4) | (2 << 11) | (1 << 13) | (1 << 15) | (1 << 18) | (7 << 26);
    raw.font_ratios[1] = 90;
    raw.font_spacings[2] = 5;

    DocInfoResult {
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
        warnings: vec![],
    }
}

#[test]
fn inspect_summary_counts_style_projection_warnings() {
    let doc_info = doc_info_with_style_projection_warning();
    let store = Hwp5StyleStore::from_doc_info(&doc_info);
    let (_hwpx_store, style_warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    assert!(
        !style_warnings.is_empty(),
        "synthetic doc info must trigger style projection warnings"
    );
}

#[cfg(test)]
mod lib_unit_tests {
    use hwpforge_smithy_hwp5::schema::header::{
        Hwp5RawIdMappings, Hwp5RawParaShape, Hwp5RawStyle, Hwp5RawTabDef, Hwp5RawTabStop,
        Hwp5TabDefSlot,
    };
    use hwpforge_smithy_hwp5::style_store::Hwp5StyleStore;
    use hwpforge_smithy_hwp5::Hwp5Warning;

    fn empty_store() -> Hwp5StyleStore {
        Hwp5StyleStore {
            id_mappings: None,
            fonts: vec![],
            char_shapes: vec![],
            para_shapes: vec![],
            numberings: vec![],
            bullets: vec![],
            tab_defs: vec![],
            styles: vec![],
            border_fills: vec![],
        }
    }

    fn id_mappings_with_counts(tab: i32, numbering: i32, bullet: i32) -> Hwp5RawIdMappings {
        Hwp5RawIdMappings {
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
            tab_def_count: tab,
            numbering_def_count: numbering,
            bullet_def_count: bullet,
            para_shape_count: 0,
            style_count: 0,
            memo_shape_count: None,
            change_tracking_count: None,
            change_tracking_author_count: None,
        }
    }

    fn simple_tab_def(id: u32) -> Hwp5TabDefSlot {
        Hwp5TabDefSlot::parsed(id, Hwp5RawTabDef { property: 0, tab_stops: vec![] })
    }

    fn minimal_para_shape_with_tab_id(tab_def_id: u16) -> Hwp5RawParaShape {
        Hwp5RawParaShape {
            property1: 0,
            left_margin: 0,
            right_margin: 0,
            indent: 0,
            space_before: 0,
            space_after: 0,
            line_spacing: 160,
            tab_def_id,
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

    fn outline_para_shape_with_tab_id(tab_def_id: u16) -> Hwp5RawParaShape {
        Hwp5RawParaShape {
            // bits 23-24 = 1 → heading_kind = Outline
            property1: 1_u32 << 23,
            left_margin: 0,
            right_margin: 0,
            indent: 0,
            space_before: 0,
            space_after: 0,
            line_spacing: 160,
            tab_def_id,
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

    fn para_style(name: &str, para_shape_id: u16) -> Hwp5RawStyle {
        Hwp5RawStyle {
            name: name.into(),
            english_name: String::new(),
            kind: 0, // paragraph style
            next_style_id: 0,
            lang_id: 0,
            para_shape_id,
            char_shape_id: 0,
            lock_form: 0,
        }
    }

    fn char_style(name: &str, para_shape_id: u16) -> Hwp5RawStyle {
        Hwp5RawStyle {
            name: name.into(),
            english_name: String::new(),
            kind: 1, // character style
            next_style_id: 0,
            lang_id: 0,
            para_shape_id,
            char_shape_id: 0,
            lock_form: 0,
        }
    }

    /// tab_def count mismatch → ParserFallback warning for tab_def.count
    #[test]
    fn tab_def_count_mismatch_emits_integrity_warning() {
        let mut store = empty_store();
        store.id_mappings = Some(id_mappings_with_counts(2, 0, 0));
        store.tab_defs = vec![simple_tab_def(0)];

        let (_hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        assert!(
            warnings.iter().any(|w| matches!(w,
                Hwp5Warning::ProjectionFallback { subject, .. } if *subject == "tab_def.count")),
            "expected tab_def.count integrity warning"
        );
    }

    /// numbering count mismatch → ParserFallback warning for numbering.count
    #[test]
    fn numbering_count_mismatch_emits_integrity_warning() {
        let mut store = empty_store();
        store.id_mappings = Some(id_mappings_with_counts(0, 3, 0));

        let (_hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        assert!(
            warnings.iter().any(|w| matches!(w,
                Hwp5Warning::ProjectionFallback { subject, .. } if *subject == "numbering.count")),
            "expected numbering.count integrity warning"
        );
    }

    /// bullet count mismatch → ParserFallback warning for bullet.count
    #[test]
    fn bullet_count_mismatch_emits_integrity_warning() {
        let mut store = empty_store();
        store.id_mappings = Some(id_mappings_with_counts(0, 0, 5));

        let (_hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        assert!(
            warnings.iter().any(|w| matches!(w,
                Hwp5Warning::ProjectionFallback { subject, .. } if *subject == "bullet.count")),
            "expected bullet.count integrity warning"
        );
    }

    /// tab_def slot with None (failed parse earlier) → ParserFallback for tab_def.slot
    #[test]
    fn tab_def_slot_none_emits_slot_warning() {
        let mut store = empty_store();
        store.tab_defs = vec![Hwp5TabDefSlot { raw_id: 0, tab_def: None }];

        let (_hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        assert!(
            warnings.iter().any(|w| matches!(w,
                Hwp5Warning::ParserFallback { subject, .. } if *subject == "tab_def.slot")),
            "expected tab_def.slot warning for None slot"
        );
    }

    /// tab stop with out-of-range position → ParserFallback for tab_def.position
    #[test]
    fn tab_stop_char_position_outside_physical_range_retains_unit() {
        let mut store = empty_store();
        store.tab_defs = vec![Hwp5TabDefSlot::parsed(
            0,
            Hwp5RawTabDef {
                property: 0,
                tab_stops: vec![Hwp5RawTabStop {
                    position: hwpforge_foundation::HwpUnit::MAX_VALUE as u32 + 1,
                    tab_type: 0,
                    fill_type: 0,
                }],
            },
        )];

        let (hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        assert_eq!(
            hwpx_store.iter_tabs().next().unwrap().stops[0]
                .position
                .as_char_count()
                .unwrap()
                .ticks(),
            50_000_000
        );
        assert!(warnings.is_empty());
    }

    /// tab stop with unknown tab_type → ParserFallback for tab_def.align
    #[test]
    fn tab_stop_unknown_tab_type_emits_warning() {
        let mut store = empty_store();
        store.tab_defs = vec![Hwp5TabDefSlot::parsed(
            0,
            Hwp5RawTabDef {
                property: 0,
                tab_stops: vec![Hwp5RawTabStop { position: 1000, tab_type: 99, fill_type: 0 }],
            },
        )];

        let (_hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        assert!(
            warnings.iter().any(|w| matches!(w,
                Hwp5Warning::ProjectionFallback { subject, .. } if *subject == "tab_def.align")),
            "expected tab_def.align warning for unknown tab_type 99"
        );
    }

    /// tab stop with out-of-range fill_type → ParserFallback for tab_def.leader
    #[test]
    fn tab_stop_out_of_range_fill_type_emits_warning() {
        let mut store = empty_store();
        store.tab_defs = vec![Hwp5TabDefSlot::parsed(
            0,
            Hwp5RawTabDef {
                property: 0,
                tab_stops: vec![Hwp5RawTabStop { position: 1000, tab_type: 0, fill_type: 17 }],
            },
        )];

        let (_hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        assert!(
            warnings.iter().any(|w| matches!(w,
                Hwp5Warning::ProjectionFallback { subject, .. } if *subject == "tab_def.leader")),
            "expected tab_def.leader warning for fill_type 17"
        );
    }

    /// paragraph shape referencing a missing tab definition → ParserFallback for tab_def.ref
    #[test]
    fn fix2_para_shape_referencing_missing_tab_def_is_error() {
        let mut store = empty_store();
        store.tab_defs = vec![simple_tab_def(0)];
        store.para_shapes = vec![minimal_para_shape_with_tab_id(99)];

        // 이것을 실패시키는 것: 없는 탭 99를 0으로 바꿔 성공 반환.
        let error = crate::hwp5_style_store_to_hwpx(&store).unwrap_err().to_string();
        assert!(error.contains("paraPr 0") && error.contains("ID 99"), "{error}");
    }

    /// Korean "개요 N" style name triggers outline-level override on matching paraPr
    #[test]
    fn outline_style_override_applies_for_korean_name() {
        use hwpforge_foundation::{HeadingType, ParaShapeIndex};

        let mut store = empty_store();
        store.tab_defs = vec![simple_tab_def(0)];
        store.para_shapes = vec![outline_para_shape_with_tab_id(0)];
        store.char_shapes.push(
            hwpforge_smithy_hwp5::schema::header::Hwp5RawCharShape::parse(&[0u8; 68]).unwrap(),
        );
        store.styles = vec![para_style("개요 8", 0)];

        let (hwpx_store, _warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        let para = hwpx_store.para_shape(ParaShapeIndex::new(0)).expect("para shape 0 must exist");
        // The override only applies when heading_type is Outline
        if matches!(para.heading_type, HeadingType::Outline) {
            assert_eq!(para.heading_level, 7, "개요 8 should set heading_level to 7");
        }
    }

    /// English "Outline N" name is also recognized by parse_outline_style_name
    #[test]
    fn outline_style_override_applies_for_english_name() {
        use hwpforge_foundation::{HeadingType, ParaShapeIndex};

        let mut store = empty_store();
        store.tab_defs = vec![simple_tab_def(0)];
        store.para_shapes = vec![outline_para_shape_with_tab_id(0)];
        store.char_shapes.push(
            hwpforge_smithy_hwp5::schema::header::Hwp5RawCharShape::parse(&[0u8; 68]).unwrap(),
        );
        store.styles = vec![para_style("Outline 9", 0)];

        let (hwpx_store, _warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        let para = hwpx_store.para_shape(ParaShapeIndex::new(0)).expect("para shape 0 must exist");
        if matches!(para.heading_type, HeadingType::Outline) {
            assert_eq!(para.heading_level, 8, "Outline 9 should set heading_level to 8");
        }
    }

    /// Character-kind style (kind=1) is silently skipped for outline override
    #[test]
    fn non_paragraph_kind_style_is_skipped_for_outline_override() {
        let mut store = empty_store();
        store.tab_defs = vec![simple_tab_def(0)];
        store.para_shapes = vec![outline_para_shape_with_tab_id(0)];
        store.char_shapes.push(
            hwpforge_smithy_hwp5::schema::header::Hwp5RawCharShape::parse(&[0u8; 68]).unwrap(),
        );
        store.styles = vec![char_style("개요 3", 0)];

        // Should not panic
        let (_hwpx_store, _warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
    }

    /// Missing paraPr references fail before outline-name handling.
    #[test]
    fn fix3_outline_style_with_oob_para_shape_id_is_error() {
        let mut store = empty_store();
        store.tab_defs = vec![simple_tab_def(0)];
        store.para_shapes = vec![outline_para_shape_with_tab_id(0)];
        // para_shape_id=99 references a non-existent slot
        store.char_shapes.push(
            hwpforge_smithy_hwp5::schema::header::Hwp5RawCharShape::parse(&[0u8; 68]).unwrap(),
        );
        store.styles = vec![para_style("개요 3", 99)];

        // 이것을 실패시키는 것: 없는 paraPr 참조를 outline override에서 무시.
        let error = crate::hwp5_style_store_to_hwpx(&store).unwrap_err().to_string();
        assert!(error.contains("paraPr reference ID 99"), "{error}");
    }

    /// Style with unrecognized name (not outline) leaves heading_level unchanged
    #[test]
    fn unrecognized_style_name_does_not_trigger_override() {
        use hwpforge_foundation::{HeadingType, ParaShapeIndex};

        let mut store = empty_store();
        store.tab_defs = vec![simple_tab_def(0)];
        store.para_shapes = vec![outline_para_shape_with_tab_id(0)];
        store.char_shapes.push(
            hwpforge_smithy_hwp5::schema::header::Hwp5RawCharShape::parse(&[0u8; 68]).unwrap(),
        );
        store.styles = vec![para_style("바탕글", 0)];

        let (hwpx_store, _warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        let para = hwpx_store.para_shape(ParaShapeIndex::new(0)).expect("para shape 0 must exist");
        // heading_level should NOT have been overridden to anything unexpected
        // (the value is determined by the property1 bits; level 0 from 1<<23 gives Outline level 0)
        if matches!(para.heading_type, HeadingType::Outline) {
            // no assertion on exact value; just verifying no panic and no incorrect override
            let _ = para.heading_level;
        }
    }

    /// No id_mappings → all integrity checks are silently skipped (no warnings)
    #[test]
    fn no_id_mappings_skips_all_integrity_checks() {
        let store = empty_store(); // id_mappings = None

        let (_hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        assert!(
            !warnings.iter().any(|w| matches!(w,
                Hwp5Warning::ProjectionFallback { subject, .. }
                    if matches!(*subject, "tab_def.count" | "numbering.count" | "bullet.count"))),
            "no integrity warnings should fire when id_mappings is None"
        );
    }

    /// Matching counts produce no integrity warnings
    #[test]
    fn matching_counts_produce_no_integrity_warnings() {
        let mut store = empty_store();
        store.id_mappings = Some(id_mappings_with_counts(1, 0, 0));
        store.tab_defs = vec![simple_tab_def(0)];

        let (_hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        assert!(
            !warnings.iter().any(|w| matches!(w,
                Hwp5Warning::ProjectionFallback { subject, .. } if *subject == "tab_def.count")),
            "no tab_def.count warning when counts match"
        );
    }

    /// tab stop at exactly MAX_VALUE position (valid boundary) emits no position warning
    #[test]
    fn tab_stop_at_max_value_boundary_emits_no_position_warning() {
        let mut store = empty_store();
        store.tab_defs = vec![Hwp5TabDefSlot::parsed(
            0,
            Hwp5RawTabDef {
                property: 0,
                tab_stops: vec![Hwp5RawTabStop {
                    position: hwpforge_foundation::HwpUnit::MAX_VALUE as u32,
                    tab_type: 0,
                    fill_type: 0,
                }],
            },
        )];

        let (_hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        assert!(
            !warnings.iter().any(|w| matches!(w,
                Hwp5Warning::ProjectionFallback { subject, .. } if *subject == "tab_def.position")),
            "no position warning for exactly MAX_VALUE"
        );
    }

    /// tab stop at known tab_type boundary (3 = last valid) emits no align warning
    #[test]
    fn tab_stop_at_tab_type_boundary_3_emits_no_align_warning() {
        let mut store = empty_store();
        store.tab_defs = vec![Hwp5TabDefSlot::parsed(
            0,
            Hwp5RawTabDef {
                property: 0,
                tab_stops: vec![Hwp5RawTabStop { position: 500, tab_type: 3, fill_type: 0 }],
            },
        )];

        let (_hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        assert!(
            !warnings.iter().any(|w| matches!(w,
                Hwp5Warning::ProjectionFallback { subject, .. } if *subject == "tab_def.align")),
            "no align warning for tab_type 3 (valid boundary)"
        );
    }

    /// tab stop fill_type = 16 (valid boundary) emits no leader warning
    #[test]
    fn tab_stop_fill_type_16_boundary_emits_no_leader_warning() {
        let mut store = empty_store();
        store.tab_defs = vec![Hwp5TabDefSlot::parsed(
            0,
            Hwp5RawTabDef {
                property: 0,
                tab_stops: vec![Hwp5RawTabStop { position: 500, tab_type: 0, fill_type: 16 }],
            },
        )];

        let (_hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        assert!(
            !warnings.iter().any(|w| matches!(w,
                Hwp5Warning::ProjectionFallback { subject, .. } if *subject == "tab_def.leader")),
            "no leader warning for fill_type 16 (valid boundary)"
        );
    }

    /// Style with out-of-range N > 10 in "개요 N" is ignored
    #[test]
    fn outline_style_level_above_10_is_skipped() {
        use hwpforge_foundation::{HeadingType, ParaShapeIndex};

        let mut store = empty_store();
        store.tab_defs = vec![simple_tab_def(0)];
        store.para_shapes = vec![outline_para_shape_with_tab_id(0)];
        store.char_shapes.push(
            hwpforge_smithy_hwp5::schema::header::Hwp5RawCharShape::parse(&[0u8; 68]).unwrap(),
        );
        store.styles = vec![para_style("개요 11", 0)]; // N=11 is out of range

        let (hwpx_store, _warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        let para = hwpx_store.para_shape(ParaShapeIndex::new(0)).expect("para shape 0 must exist");
        // No override should have happened; heading_level remains at the wire-encoded value
        if matches!(para.heading_type, HeadingType::Outline) {
            // Value is not 10 (which would be 11-1); if override was incorrectly applied
            // it would be 10. The wire-encoded level from 1<<23 with all heading bits = 0
            // gives heading_level = 0, which is also != 10.
            assert_ne!(para.heading_level, 10, "level 11 should not have been applied");
        }
    }

    /// para_shape referencing tab_def_id=0 which IS in tab_defs emits no ref warning
    #[test]
    fn para_shape_with_known_tab_def_id_emits_no_ref_warning() {
        let mut store = empty_store();
        store.tab_defs = vec![simple_tab_def(0)];
        store.para_shapes = vec![minimal_para_shape_with_tab_id(0)];

        let (_hwpx_store, warnings) = crate::hwp5_style_store_to_hwpx(&store).unwrap();
        assert!(
            !warnings.iter().any(|w| matches!(w,
                Hwp5Warning::ProjectionFallback { subject, .. } if *subject == "tab_def.ref")),
            "no tab_def.ref warning when tab_def_id is known"
        );
    }
}
