//! Property-based tests for the Blueprint crate.
//!
//! Uses proptest to verify invariants over random inputs.

use hwpforge_blueprint::error::BlueprintError;
use hwpforge_blueprint::style::{CharShape, ParaShape, PartialCharShape, PartialParaShape};
use hwpforge_foundation::{
    Alignment, Color, EmbossType, EmphasisType, EngraveType, HwpUnit, LineSpacingType, OutlineType,
    ShadowType, StrikeoutShape, UnderlineType, VerticalPosition,
};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Generates a valid HwpUnit within a reasonable range.
fn arb_hwpunit() -> impl Strategy<Value = HwpUnit> {
    // pt range: 1pt to 200pt (100 to 20000 raw)
    (100i32..20_000).prop_map(|raw| HwpUnit::new(raw).unwrap())
}

/// Generates an RGB color.
fn arb_color() -> impl Strategy<Value = Color> {
    (0u8..=255, 0u8..=255, 0u8..=255).prop_map(|(r, g, b)| Color::from_rgb(r, g, b))
}

/// Generates a font name (non-empty ASCII for simplicity).
fn arb_font() -> impl Strategy<Value = String> {
    prop::string::string_regex("[A-Za-z가-힣]{1,20}").unwrap()
}

/// Generates an Alignment variant.
fn arb_alignment() -> impl Strategy<Value = Alignment> {
    prop_oneof![
        Just(Alignment::Left),
        Just(Alignment::Center),
        Just(Alignment::Right),
        Just(Alignment::Justify),
    ]
}

/// Generates a LineSpacingType variant.
fn arb_line_spacing_type() -> impl Strategy<Value = LineSpacingType> {
    prop_oneof![
        Just(LineSpacingType::Percentage),
        Just(LineSpacingType::Fixed),
        Just(LineSpacingType::BetweenLines),
    ]
}

// ---------------------------------------------------------------------------
// Dimension roundtrip
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn dimension_roundtrip_pt(pt_raw in 0i32..50_000) {
        if let Ok(unit) = HwpUnit::new(pt_raw) {
            let s = hwpforge_blueprint::serde_helpers::format_dimension_pt(unit);
            let back = hwpforge_blueprint::serde_helpers::parse_dimension(&s).unwrap();
            // Roundtrip within 1 unit tolerance (0.01pt) due to
            // format_dimension_pt using 2 decimal places (1 raw = 0.01pt)
            prop_assert!((unit.as_i32() - back.as_i32()).abs() <= 1,
                "roundtrip failed: {} -> '{}' -> {}", unit.as_i32(), s, back.as_i32());
        }
    }
}

// ---------------------------------------------------------------------------
// Color roundtrip
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn color_roundtrip_rgb((r, g, b) in (0u8..=255, 0u8..=255, 0u8..=255)) {
        let color = Color::from_rgb(r, g, b);
        let s = hwpforge_blueprint::serde_helpers::format_color(color);
        let back = hwpforge_blueprint::serde_helpers::parse_color(&s).unwrap();
        let (br, bg, bb) = back.to_rgb();
        prop_assert_eq!((r, g, b), (br, bg, bb));
    }
}

// ---------------------------------------------------------------------------
// Percentage roundtrip
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn percentage_roundtrip(val in 0.0f64..1000.0) {
        let s = hwpforge_blueprint::serde_helpers::format_percentage(val);
        let back = hwpforge_blueprint::serde_helpers::parse_percentage(&s).unwrap();
        prop_assert!((val - back).abs() < 1.0,
            "roundtrip failed: {} -> '{}' -> {}", val, s, back);
    }
}

// ---------------------------------------------------------------------------
// CharShape resolve invariants
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn partial_char_shape_with_required_fields_resolves(
        font in arb_font(),
        size in arb_hwpunit(),
        bold in any::<bool>(),
        italic in any::<bool>(),
        color in arb_color(),
    ) {
        let partial = PartialCharShape {
            font: Some(font.clone()),
            size: Some(size),
            bold: Some(bold),
            italic: Some(italic),
            color: Some(color),
            ..Default::default()
        };
        let resolved = partial.resolve("test").unwrap();
        prop_assert_eq!(resolved.font, font);
        prop_assert_eq!(resolved.size, size);
        prop_assert_eq!(resolved.bold, bold);
        prop_assert_eq!(resolved.italic, italic);
        prop_assert_eq!(resolved.color, color);
    }

    #[test]
    fn partial_char_shape_missing_font_fails(
        size in arb_hwpunit(),
    ) {
        let partial = PartialCharShape {
            font: None,
            size: Some(size),
            ..Default::default()
        };
        let err = partial.resolve("test").unwrap_err();
        match err {
            BlueprintError::StyleResolution { field, .. } => {
                prop_assert_eq!(field, "font");
            }
            _ => prop_assert!(false, "Expected StyleResolution error"),
        }
    }

    #[test]
    fn partial_char_shape_missing_size_fails(
        font in arb_font(),
    ) {
        let partial = PartialCharShape {
            font: Some(font),
            size: None,
            ..Default::default()
        };
        let err = partial.resolve("test").unwrap_err();
        match err {
            BlueprintError::StyleResolution { field, .. } => {
                prop_assert_eq!(field, "size");
            }
            _ => prop_assert!(false, "Expected StyleResolution error"),
        }
    }
}

// ---------------------------------------------------------------------------
// Merge is idempotent (merging same twice = merging once)
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn partial_char_shape_merge_idempotent(
        font in proptest::option::of(arb_font()),
        size in proptest::option::of(arb_hwpunit()),
        bold in proptest::option::of(any::<bool>()),
    ) {
        let child = PartialCharShape {
            font,
            size,
            bold,
            ..Default::default()
        };

        let mut base1 = PartialCharShape::default();
        base1.merge(&child);

        let mut base2 = base1.clone();
        base2.merge(&child);

        prop_assert_eq!(base1, base2, "Merge should be idempotent");
    }
}

// ---------------------------------------------------------------------------
// ParaShape resolve always succeeds (all fields have defaults)
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn partial_para_shape_always_resolves(
        alignment in proptest::option::of(arb_alignment()),
        spacing_type in proptest::option::of(arb_line_spacing_type()),
        spacing_val in proptest::option::of(50.0f64..300.0),
    ) {
        let partial = PartialParaShape {
            alignment,
            line_spacing: spacing_type.map(|st| {
                hwpforge_blueprint::style::LineSpacing {
                    spacing_type: Some(st),
                    value: spacing_val,
                }
            }),
            ..Default::default()
        };

        let resolved = partial.resolve();
        // Should always succeed since ParaShape has defaults for all fields
        if let Some(a) = alignment {
            prop_assert_eq!(resolved.alignment, a);
        } else {
            prop_assert_eq!(resolved.alignment, Alignment::Left); // Default
        }
    }
}

// ---------------------------------------------------------------------------
// CharShape serde roundtrip
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn char_shape_serde_yaml_roundtrip(
        font in arb_font(),
        size in arb_hwpunit(),
        bold in any::<bool>(),
        italic in any::<bool>(),
        color in arb_color(),
    ) {
        let original = CharShape {
            font: font.clone(),
            size,
            bold,
            italic,
            color,
            underline_type: UnderlineType::None,
            underline_color: None,
            strikeout_shape: StrikeoutShape::None,
            strikeout_color: None,
            outline: OutlineType::None,
            shadow: ShadowType::None,
            emboss: EmbossType::None,
            engrave: EngraveType::None,
            vertical_position: VerticalPosition::Normal,
            shade_color: None,
            emphasis: EmphasisType::None,
            ratio: 100,
            spacing: 0,
            rel_sz: 100,
            offset: 0,
            use_kerning: false,
            use_font_space: false,
            char_border_fill_id: None,
        };
        let yaml = serde_yaml::to_string(&original).unwrap();
        let back: CharShape = serde_yaml::from_str(&yaml).unwrap();
        prop_assert_eq!(original.font, back.font);
        prop_assert_eq!(original.bold, back.bold);
        // Size may differ by ±1 (0.01pt) due to 2-decimal formatting
        prop_assert!((original.size.as_i32() - back.size.as_i32()).abs() <= 1);
        prop_assert_eq!(original.color, back.color);
    }
}

// ---------------------------------------------------------------------------
// ParaShape serde roundtrip
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn para_shape_serde_yaml_roundtrip(
        alignment in arb_alignment(),
        spacing_type in arb_line_spacing_type(),
        spacing_val in 50.0f64..300.0,
    ) {
        let original = ParaShape {
            alignment,
            line_spacing_type: spacing_type,
            line_spacing_value: spacing_val,
            space_before: HwpUnit::ZERO,
            space_after: HwpUnit::ZERO,
            indent_left: HwpUnit::ZERO,
            indent_right: HwpUnit::ZERO,
            indent_first_line: HwpUnit::ZERO,
            break_type: hwpforge_foundation::BreakType::None,
            keep_with_next: false,
            keep_lines_together: false,
            widow_orphan: true,
            border_fill_id: None,
            heading_type: hwpforge_foundation::HeadingType::None,
        };
        let yaml = serde_yaml::to_string(&original).unwrap();
        let back: ParaShape = serde_yaml::from_str(&yaml).unwrap();
        prop_assert_eq!(original.alignment, back.alignment);
        prop_assert_eq!(original.line_spacing_type, back.line_spacing_type);
        // Float rounding tolerance
        prop_assert!((original.line_spacing_value - back.line_spacing_value).abs() < 1.0);
    }
}
