//! Core enums used throughout HWP document processing.
//!
//! All enums are `#[non_exhaustive]` to allow future variant additions
//! without breaking downstream code. They use `#[repr(u8)]` for compact
//! storage and provide `TryFrom<u8>` for binary parsing.
//!
//! # Examples
//!
//! ```
//! use hwpforge_foundation::Alignment;
//! use std::str::FromStr;
//!
//! let a = Alignment::from_str("Justify").unwrap();
//! assert_eq!(a, Alignment::Justify);
//! assert_eq!(a.to_string(), "Justify");
//! ```

mod border_fill;
mod char_style;
mod page;
mod paragraph;
mod reference;
mod shape;
mod tab;
pub use border_fill::*;
pub use char_style::*;
pub use page::*;
pub use paragraph::*;
pub use reference::*;
pub use shape::*;
pub use tab::*;

// Compile-time size assertions: all enums are 1 byte
const _: () = assert!(std::mem::size_of::<DropCapStyle>() == 1);
const _: () = assert!(std::mem::size_of::<Alignment>() == 1);
const _: () = assert!(std::mem::size_of::<LineSpacingType>() == 1);
const _: () = assert!(std::mem::size_of::<BreakType>() == 1);
const _: () = assert!(std::mem::size_of::<Language>() == 1);
const _: () = assert!(std::mem::size_of::<UnderlineType>() == 1);
const _: () = assert!(std::mem::size_of::<UnderlineShape>() == 1);
const _: () = assert!(std::mem::size_of::<StrikeoutShape>() == 1);
const _: () = assert!(std::mem::size_of::<OutlineType>() == 1);
const _: () = assert!(std::mem::size_of::<ShadowType>() == 1);
const _: () = assert!(std::mem::size_of::<EmbossType>() == 1);
const _: () = assert!(std::mem::size_of::<EngraveType>() == 1);
const _: () = assert!(std::mem::size_of::<VerticalPosition>() == 1);
const _: () = assert!(std::mem::size_of::<BorderLineType>() == 1);
const _: () = assert!(std::mem::size_of::<FillBrushType>() == 1);
const _: () = assert!(std::mem::size_of::<ApplyPageType>() == 1);
const _: () = assert!(std::mem::size_of::<NumberFormatType>() == 1);
const _: () = assert!(std::mem::size_of::<PageNumberPosition>() == 1);
const _: () = assert!(std::mem::size_of::<WordBreakType>() == 1);
const _: () = assert!(std::mem::size_of::<EmphasisType>() == 1);
const _: () = assert!(std::mem::size_of::<HeadingType>() == 1);
const _: () = assert!(std::mem::size_of::<GutterType>() == 1);
const _: () = assert!(std::mem::size_of::<ShowMode>() == 1);
const _: () = assert!(std::mem::size_of::<RestartType>() == 1);
const _: () = assert!(std::mem::size_of::<TextBorderType>() == 1);
const _: () = assert!(std::mem::size_of::<Flip>() == 1);
const _: () = assert!(std::mem::size_of::<ArcType>() == 1);
const _: () = assert!(std::mem::size_of::<ArrowType>() == 1);
const _: () = assert!(std::mem::size_of::<ArrowSize>() == 1);
const _: () = assert!(std::mem::size_of::<GradientType>() == 1);
const _: () = assert!(std::mem::size_of::<PatternType>() == 1);
const _: () = assert!(std::mem::size_of::<ImageFillMode>() == 1);
const _: () = assert!(std::mem::size_of::<CurveSegmentType>() == 1);
const _: () = assert!(std::mem::size_of::<VerticalAlign>() == 1);
const _: () = assert!(std::mem::size_of::<BookmarkType>() == 1);
const _: () = assert!(std::mem::size_of::<FieldType>() == 1);
// Wave 12m Phase 2: RefType / RefContentType 의 `#[repr(u8)]` 제거 + Unknown(u8)
// tuple variant 추가로 size 가 1 byte 에서 2 bytes (discriminant + u8 payload) 로
// 증가. trade-off: type-safe forward-compat carry > 1 byte memory.
const _: () = assert!(std::mem::size_of::<RefType>() == 2);
const _: () = assert!(std::mem::size_of::<RefContentType>() == 2);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::FoundationError;
    use std::str::FromStr;

    // ===================================================================
    // Alignment (10+ tests)
    // ===================================================================

    #[test]
    fn alignment_default_is_left() {
        assert_eq!(Alignment::default(), Alignment::Left);
    }

    #[test]
    fn alignment_display_all_variants() {
        assert_eq!(Alignment::Left.to_string(), "Left");
        assert_eq!(Alignment::Center.to_string(), "Center");
        assert_eq!(Alignment::Right.to_string(), "Right");
        assert_eq!(Alignment::Justify.to_string(), "Justify");
        assert_eq!(Alignment::Distribute.to_string(), "Distribute");
        assert_eq!(Alignment::DistributeFlush.to_string(), "DistributeFlush");
    }

    #[test]
    fn alignment_from_str_pascal_case() {
        assert_eq!(Alignment::from_str("Left").unwrap(), Alignment::Left);
        assert_eq!(Alignment::from_str("Center").unwrap(), Alignment::Center);
        assert_eq!(Alignment::from_str("Right").unwrap(), Alignment::Right);
        assert_eq!(Alignment::from_str("Justify").unwrap(), Alignment::Justify);
        assert_eq!(Alignment::from_str("Distribute").unwrap(), Alignment::Distribute);
        assert_eq!(Alignment::from_str("DistributeFlush").unwrap(), Alignment::DistributeFlush);
    }

    #[test]
    fn alignment_from_str_lower_case() {
        assert_eq!(Alignment::from_str("left").unwrap(), Alignment::Left);
        assert_eq!(Alignment::from_str("center").unwrap(), Alignment::Center);
        assert_eq!(Alignment::from_str("distribute").unwrap(), Alignment::Distribute);
        assert_eq!(Alignment::from_str("distributeflush").unwrap(), Alignment::DistributeFlush);
        assert_eq!(Alignment::from_str("distribute_flush").unwrap(), Alignment::DistributeFlush);
    }

    #[test]
    fn alignment_from_str_invalid() {
        let err = Alignment::from_str("leftt").unwrap_err();
        match err {
            FoundationError::ParseError { ref type_name, ref value, .. } => {
                assert_eq!(type_name, "Alignment");
                assert_eq!(value, "leftt");
            }
            other => panic!("unexpected: {other}"),
        }
    }

    #[test]
    fn alignment_try_from_u8() {
        assert_eq!(Alignment::try_from(0u8).unwrap(), Alignment::Left);
        assert_eq!(Alignment::try_from(1u8).unwrap(), Alignment::Center);
        assert_eq!(Alignment::try_from(2u8).unwrap(), Alignment::Right);
        assert_eq!(Alignment::try_from(3u8).unwrap(), Alignment::Justify);
        assert_eq!(Alignment::try_from(4u8).unwrap(), Alignment::Distribute);
        assert_eq!(Alignment::try_from(5u8).unwrap(), Alignment::DistributeFlush);
        assert!(Alignment::try_from(6u8).is_err());
        assert!(Alignment::try_from(255u8).is_err());
    }

    #[test]
    fn alignment_repr_values() {
        assert_eq!(Alignment::Left as u8, 0);
        assert_eq!(Alignment::Center as u8, 1);
        assert_eq!(Alignment::Right as u8, 2);
        assert_eq!(Alignment::Justify as u8, 3);
        assert_eq!(Alignment::Distribute as u8, 4);
        assert_eq!(Alignment::DistributeFlush as u8, 5);
    }

    #[test]
    fn alignment_serde_roundtrip() {
        for variant in &[
            Alignment::Left,
            Alignment::Center,
            Alignment::Right,
            Alignment::Justify,
            Alignment::Distribute,
            Alignment::DistributeFlush,
        ] {
            let json = serde_json::to_string(variant).unwrap();
            let back: Alignment = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, variant);
        }
    }

    #[test]
    fn alignment_str_roundtrip() {
        for variant in &[
            Alignment::Left,
            Alignment::Center,
            Alignment::Right,
            Alignment::Justify,
            Alignment::Distribute,
            Alignment::DistributeFlush,
        ] {
            let s = variant.to_string();
            let back = Alignment::from_str(&s).unwrap();
            assert_eq!(&back, variant);
        }
    }

    #[test]
    fn alignment_copy_and_hash() {
        use std::collections::HashSet;
        let a = Alignment::Left;
        let b = a; // Copy
        assert_eq!(a, b);

        let mut set = HashSet::new();
        set.insert(Alignment::Left);
        set.insert(Alignment::Right);
        assert_eq!(set.len(), 2);
    }

    // ===================================================================
    // LineSpacingType
    // ===================================================================

    #[test]
    fn line_spacing_default_is_percentage() {
        assert_eq!(LineSpacingType::default(), LineSpacingType::Percentage);
    }

    #[test]
    fn line_spacing_display() {
        assert_eq!(LineSpacingType::Percentage.to_string(), "Percentage");
        assert_eq!(LineSpacingType::Fixed.to_string(), "Fixed");
        assert_eq!(LineSpacingType::BetweenLines.to_string(), "BetweenLines");
        assert_eq!(LineSpacingType::AtLeast.to_string(), "AtLeast");
    }

    #[test]
    fn line_spacing_from_str() {
        assert_eq!(LineSpacingType::from_str("Percentage").unwrap(), LineSpacingType::Percentage);
        assert_eq!(LineSpacingType::from_str("Fixed").unwrap(), LineSpacingType::Fixed);
        assert_eq!(
            LineSpacingType::from_str("BetweenLines").unwrap(),
            LineSpacingType::BetweenLines
        );
        assert_eq!(LineSpacingType::from_str("AtLeast").unwrap(), LineSpacingType::AtLeast);
        assert_eq!(LineSpacingType::from_str("at_least").unwrap(), LineSpacingType::AtLeast);
        assert!(LineSpacingType::from_str("invalid").is_err());
    }

    #[test]
    fn line_spacing_try_from_u8() {
        assert_eq!(LineSpacingType::try_from(0u8).unwrap(), LineSpacingType::Percentage);
        assert_eq!(LineSpacingType::try_from(1u8).unwrap(), LineSpacingType::Fixed);
        assert_eq!(LineSpacingType::try_from(2u8).unwrap(), LineSpacingType::BetweenLines);
        assert_eq!(LineSpacingType::try_from(3u8).unwrap(), LineSpacingType::AtLeast);
        assert!(LineSpacingType::try_from(4u8).is_err());
    }

    #[test]
    fn line_spacing_str_roundtrip() {
        for v in &[
            LineSpacingType::Percentage,
            LineSpacingType::Fixed,
            LineSpacingType::BetweenLines,
            LineSpacingType::AtLeast,
        ] {
            let s = v.to_string();
            let back = LineSpacingType::from_str(&s).unwrap();
            assert_eq!(&back, v);
        }
    }

    // ===================================================================
    // BreakType
    // ===================================================================

    #[test]
    fn break_type_default_is_none() {
        assert_eq!(BreakType::default(), BreakType::None);
    }

    #[test]
    fn break_type_display() {
        assert_eq!(BreakType::None.to_string(), "None");
        assert_eq!(BreakType::Column.to_string(), "Column");
        assert_eq!(BreakType::Page.to_string(), "Page");
    }

    #[test]
    fn break_type_from_str() {
        assert_eq!(BreakType::from_str("None").unwrap(), BreakType::None);
        assert_eq!(BreakType::from_str("Column").unwrap(), BreakType::Column);
        assert_eq!(BreakType::from_str("Page").unwrap(), BreakType::Page);
        assert!(BreakType::from_str("section").is_err());
    }

    #[test]
    fn break_type_try_from_u8() {
        assert_eq!(BreakType::try_from(0u8).unwrap(), BreakType::None);
        assert_eq!(BreakType::try_from(1u8).unwrap(), BreakType::Column);
        assert_eq!(BreakType::try_from(2u8).unwrap(), BreakType::Page);
        assert!(BreakType::try_from(3u8).is_err());
    }

    #[test]
    fn break_type_str_roundtrip() {
        for v in &[BreakType::None, BreakType::Column, BreakType::Page] {
            let s = v.to_string();
            let back = BreakType::from_str(&s).unwrap();
            assert_eq!(&back, v);
        }
    }

    // ===================================================================
    // Language
    // ===================================================================

    #[test]
    fn language_count_is_7() {
        assert_eq!(Language::COUNT, 7);
        assert_eq!(Language::ALL.len(), 7);
    }

    #[test]
    fn language_default_is_korean() {
        assert_eq!(Language::default(), Language::Korean);
    }

    #[test]
    fn language_discriminants() {
        assert_eq!(Language::Korean as u8, 0);
        assert_eq!(Language::English as u8, 1);
        assert_eq!(Language::Hanja as u8, 2);
        assert_eq!(Language::Japanese as u8, 3);
        assert_eq!(Language::Other as u8, 4);
        assert_eq!(Language::Symbol as u8, 5);
        assert_eq!(Language::User as u8, 6);
    }

    #[test]
    fn language_display() {
        assert_eq!(Language::Korean.to_string(), "Korean");
        assert_eq!(Language::English.to_string(), "English");
        assert_eq!(Language::Japanese.to_string(), "Japanese");
    }

    #[test]
    fn language_from_str() {
        for lang in &Language::ALL {
            let s = lang.to_string();
            let back = Language::from_str(&s).unwrap();
            assert_eq!(&back, lang);
        }
        assert!(Language::from_str("invalid").is_err());
    }

    #[test]
    fn language_try_from_u8() {
        for (i, expected) in Language::ALL.iter().enumerate() {
            let parsed = Language::try_from(i as u8).unwrap();
            assert_eq!(&parsed, expected);
        }
        assert!(Language::try_from(7u8).is_err());
        assert!(Language::try_from(255u8).is_err());
    }

    #[test]
    fn language_all_used_as_index() {
        // Common pattern: fonts[lang as usize]
        let fonts: [&str; Language::COUNT] =
            ["Batang", "Arial", "SimSun", "MS Mincho", "Arial", "Symbol", "Arial"];
        for lang in &Language::ALL {
            let _ = fonts[*lang as usize];
        }
    }

    #[test]
    fn language_serde_roundtrip() {
        for lang in &Language::ALL {
            let json = serde_json::to_string(lang).unwrap();
            let back: Language = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, lang);
        }
    }

    // ===================================================================
    // UnderlineType
    // ===================================================================

    #[test]
    fn underline_type_default_is_none() {
        assert_eq!(UnderlineType::default(), UnderlineType::None);
    }

    #[test]
    fn underline_type_display() {
        assert_eq!(UnderlineType::None.to_string(), "None");
        assert_eq!(UnderlineType::Bottom.to_string(), "Bottom");
        assert_eq!(UnderlineType::Center.to_string(), "Center");
        assert_eq!(UnderlineType::Top.to_string(), "Top");
    }

    #[test]
    fn underline_type_from_str() {
        assert_eq!(UnderlineType::from_str("None").unwrap(), UnderlineType::None);
        assert_eq!(UnderlineType::from_str("Bottom").unwrap(), UnderlineType::Bottom);
        assert_eq!(UnderlineType::from_str("center").unwrap(), UnderlineType::Center);
        assert!(UnderlineType::from_str("invalid").is_err());
    }

    #[test]
    fn underline_type_try_from_u8() {
        assert_eq!(UnderlineType::try_from(0u8).unwrap(), UnderlineType::None);
        assert_eq!(UnderlineType::try_from(1u8).unwrap(), UnderlineType::Bottom);
        assert_eq!(UnderlineType::try_from(2u8).unwrap(), UnderlineType::Center);
        assert_eq!(UnderlineType::try_from(3u8).unwrap(), UnderlineType::Top);
        assert!(UnderlineType::try_from(4u8).is_err());
    }

    #[test]
    fn underline_type_str_roundtrip() {
        for v in
            &[UnderlineType::None, UnderlineType::Bottom, UnderlineType::Center, UnderlineType::Top]
        {
            let s = v.to_string();
            let back = UnderlineType::from_str(&s).unwrap();
            assert_eq!(&back, v);
        }
    }

    // ===================================================================
    // UnderlineShape
    // ===================================================================

    #[test]
    fn underline_shape_default_is_solid() {
        assert_eq!(UnderlineShape::default(), UnderlineShape::Solid);
    }

    #[test]
    fn underline_shape_display_screaming_snake_case() {
        assert_eq!(UnderlineShape::Solid.to_string(), "SOLID");
        assert_eq!(UnderlineShape::Dash.to_string(), "DASH");
        assert_eq!(UnderlineShape::Dot.to_string(), "DOT");
        assert_eq!(UnderlineShape::DashDot.to_string(), "DASH_DOT");
        assert_eq!(UnderlineShape::DashDotDot.to_string(), "DASH_DOT_DOT");
        assert_eq!(UnderlineShape::LongDash.to_string(), "LONG_DASH");
        assert_eq!(UnderlineShape::Circle.to_string(), "CIRCLE");
        assert_eq!(UnderlineShape::DoubleSlim.to_string(), "DOUBLE_SLIM");
        assert_eq!(UnderlineShape::SlimThick.to_string(), "SLIM_THICK");
        assert_eq!(UnderlineShape::ThickSlim.to_string(), "THICK_SLIM");
        assert_eq!(UnderlineShape::ThickSlimThick.to_string(), "THICK_SLIM_THICK");
        assert_eq!(UnderlineShape::Wave.to_string(), "WAVE");
    }

    #[test]
    fn underline_shape_from_str_variants() {
        assert_eq!(UnderlineShape::from_str("SOLID").unwrap(), UnderlineShape::Solid);
        assert_eq!(UnderlineShape::from_str("dash").unwrap(), UnderlineShape::Dash);
        assert_eq!(UnderlineShape::from_str("DOUBLE_SLIM").unwrap(), UnderlineShape::DoubleSlim);
        assert_eq!(UnderlineShape::from_str("WAVE").unwrap(), UnderlineShape::Wave);
        assert!(UnderlineShape::from_str("invalid").is_err());
    }

    #[test]
    fn underline_shape_try_from_u8() {
        assert_eq!(UnderlineShape::try_from(0u8).unwrap(), UnderlineShape::Solid);
        assert_eq!(UnderlineShape::try_from(1u8).unwrap(), UnderlineShape::Dash);
        assert_eq!(UnderlineShape::try_from(7u8).unwrap(), UnderlineShape::DoubleSlim);
        assert_eq!(UnderlineShape::try_from(11u8).unwrap(), UnderlineShape::Wave);
        assert!(UnderlineShape::try_from(12u8).is_err());
    }

    #[test]
    fn underline_shape_str_roundtrip() {
        for v in &[
            UnderlineShape::Solid,
            UnderlineShape::Dash,
            UnderlineShape::Dot,
            UnderlineShape::DashDot,
            UnderlineShape::DashDotDot,
            UnderlineShape::LongDash,
            UnderlineShape::Circle,
            UnderlineShape::DoubleSlim,
            UnderlineShape::SlimThick,
            UnderlineShape::ThickSlim,
            UnderlineShape::ThickSlimThick,
            UnderlineShape::Wave,
        ] {
            let s = v.to_string();
            let back = UnderlineShape::from_str(&s).unwrap();
            assert_eq!(&back, v);
        }
    }

    // ===================================================================
    // StrikeoutShape
    // ===================================================================

    #[test]
    fn strikeout_shape_default_is_none() {
        assert_eq!(StrikeoutShape::default(), StrikeoutShape::None);
    }

    #[test]
    fn strikeout_shape_display() {
        assert_eq!(StrikeoutShape::None.to_string(), "NONE");
        assert_eq!(StrikeoutShape::Solid.to_string(), "SOLID");
        assert_eq!(StrikeoutShape::Dash.to_string(), "DASH");
        assert_eq!(StrikeoutShape::Dot.to_string(), "DOT");
        assert_eq!(StrikeoutShape::DashDot.to_string(), "DASH_DOT");
        assert_eq!(StrikeoutShape::DashDotDot.to_string(), "DASH_DOT_DOT");
        assert_eq!(StrikeoutShape::LongDash.to_string(), "LONG_DASH");
        assert_eq!(StrikeoutShape::Circle.to_string(), "CIRCLE");
        assert_eq!(StrikeoutShape::DoubleSlim.to_string(), "DOUBLE_SLIM");
        assert_eq!(StrikeoutShape::SlimThick.to_string(), "SLIM_THICK");
        assert_eq!(StrikeoutShape::ThickSlim.to_string(), "THICK_SLIM");
        assert_eq!(StrikeoutShape::ThickSlimThick.to_string(), "THICK_SLIM_THICK");
        assert_eq!(StrikeoutShape::Wave.to_string(), "WAVE");
    }

    #[test]
    fn strikeout_shape_from_str() {
        assert_eq!(StrikeoutShape::from_str("NONE").unwrap(), StrikeoutShape::None);
        assert_eq!(StrikeoutShape::from_str("SOLID").unwrap(), StrikeoutShape::Solid);
        // Backward-compatible alias for the pre-Wave-1c name.
        assert_eq!(StrikeoutShape::from_str("Continuous").unwrap(), StrikeoutShape::Solid);
        assert_eq!(StrikeoutShape::from_str("dash_dot").unwrap(), StrikeoutShape::DashDot);
        assert_eq!(StrikeoutShape::from_str("DOUBLE_SLIM").unwrap(), StrikeoutShape::DoubleSlim);
        assert_eq!(StrikeoutShape::from_str("WAVE").unwrap(), StrikeoutShape::Wave);
        assert!(StrikeoutShape::from_str("invalid").is_err());
    }

    #[test]
    fn strikeout_shape_try_from_u8() {
        assert_eq!(StrikeoutShape::try_from(0u8).unwrap(), StrikeoutShape::None);
        assert_eq!(StrikeoutShape::try_from(1u8).unwrap(), StrikeoutShape::Solid);
        assert_eq!(StrikeoutShape::try_from(5u8).unwrap(), StrikeoutShape::DashDotDot);
        assert_eq!(StrikeoutShape::try_from(8u8).unwrap(), StrikeoutShape::DoubleSlim);
        assert_eq!(StrikeoutShape::try_from(12u8).unwrap(), StrikeoutShape::Wave);
        assert!(StrikeoutShape::try_from(13u8).is_err());
    }

    #[test]
    fn strikeout_shape_str_roundtrip() {
        for v in &[
            StrikeoutShape::None,
            StrikeoutShape::Solid,
            StrikeoutShape::Dash,
            StrikeoutShape::Dot,
            StrikeoutShape::DashDot,
            StrikeoutShape::DashDotDot,
            StrikeoutShape::LongDash,
            StrikeoutShape::Circle,
            StrikeoutShape::DoubleSlim,
            StrikeoutShape::SlimThick,
            StrikeoutShape::ThickSlim,
            StrikeoutShape::ThickSlimThick,
            StrikeoutShape::Wave,
        ] {
            let s = v.to_string();
            let back = StrikeoutShape::from_str(&s).unwrap();
            assert_eq!(&back, v);
        }
    }

    // ===================================================================
    // OutlineType
    // ===================================================================

    #[test]
    fn outline_type_default_is_none() {
        assert_eq!(OutlineType::default(), OutlineType::None);
    }

    #[test]
    fn outline_type_display() {
        assert_eq!(OutlineType::None.to_string(), "None");
        assert_eq!(OutlineType::Solid.to_string(), "Solid");
    }

    #[test]
    fn outline_type_from_str() {
        assert_eq!(OutlineType::from_str("None").unwrap(), OutlineType::None);
        assert_eq!(OutlineType::from_str("solid").unwrap(), OutlineType::Solid);
        assert!(OutlineType::from_str("dashed").is_err());
    }

    #[test]
    fn outline_type_try_from_u8() {
        assert_eq!(OutlineType::try_from(0u8).unwrap(), OutlineType::None);
        assert_eq!(OutlineType::try_from(1u8).unwrap(), OutlineType::Solid);
        assert!(OutlineType::try_from(2u8).is_err());
    }

    // ===================================================================
    // ShadowType
    // ===================================================================

    #[test]
    fn shadow_type_default_is_none() {
        assert_eq!(ShadowType::default(), ShadowType::None);
    }

    #[test]
    fn shadow_type_display() {
        assert_eq!(ShadowType::None.to_string(), "None");
        assert_eq!(ShadowType::Drop.to_string(), "Drop");
    }

    #[test]
    fn shadow_type_from_str() {
        assert_eq!(ShadowType::from_str("None").unwrap(), ShadowType::None);
        assert_eq!(ShadowType::from_str("drop").unwrap(), ShadowType::Drop);
        assert!(ShadowType::from_str("shadow").is_err());
    }

    #[test]
    fn shadow_type_try_from_u8() {
        assert_eq!(ShadowType::try_from(0u8).unwrap(), ShadowType::None);
        assert_eq!(ShadowType::try_from(1u8).unwrap(), ShadowType::Drop);
        assert!(ShadowType::try_from(2u8).is_err());
    }

    // ===================================================================
    // EmbossType
    // ===================================================================

    #[test]
    fn emboss_type_default_is_none() {
        assert_eq!(EmbossType::default(), EmbossType::None);
    }

    #[test]
    fn emboss_type_display() {
        assert_eq!(EmbossType::None.to_string(), "None");
        assert_eq!(EmbossType::Emboss.to_string(), "Emboss");
    }

    #[test]
    fn emboss_type_from_str() {
        assert_eq!(EmbossType::from_str("None").unwrap(), EmbossType::None);
        assert_eq!(EmbossType::from_str("emboss").unwrap(), EmbossType::Emboss);
        assert!(EmbossType::from_str("raised").is_err());
    }

    #[test]
    fn emboss_type_try_from_u8() {
        assert_eq!(EmbossType::try_from(0u8).unwrap(), EmbossType::None);
        assert_eq!(EmbossType::try_from(1u8).unwrap(), EmbossType::Emboss);
        assert!(EmbossType::try_from(2u8).is_err());
    }

    // ===================================================================
    // EngraveType
    // ===================================================================

    #[test]
    fn engrave_type_default_is_none() {
        assert_eq!(EngraveType::default(), EngraveType::None);
    }

    #[test]
    fn engrave_type_display() {
        assert_eq!(EngraveType::None.to_string(), "None");
        assert_eq!(EngraveType::Engrave.to_string(), "Engrave");
    }

    #[test]
    fn engrave_type_from_str() {
        assert_eq!(EngraveType::from_str("None").unwrap(), EngraveType::None);
        assert_eq!(EngraveType::from_str("engrave").unwrap(), EngraveType::Engrave);
        assert!(EngraveType::from_str("sunken").is_err());
    }

    #[test]
    fn engrave_type_try_from_u8() {
        assert_eq!(EngraveType::try_from(0u8).unwrap(), EngraveType::None);
        assert_eq!(EngraveType::try_from(1u8).unwrap(), EngraveType::Engrave);
        assert!(EngraveType::try_from(2u8).is_err());
    }

    // ===================================================================
    // VerticalPosition
    // ===================================================================

    #[test]
    fn vertical_position_default_is_normal() {
        assert_eq!(VerticalPosition::default(), VerticalPosition::Normal);
    }

    #[test]
    fn vertical_position_display() {
        assert_eq!(VerticalPosition::Normal.to_string(), "Normal");
        assert_eq!(VerticalPosition::Superscript.to_string(), "Superscript");
        assert_eq!(VerticalPosition::Subscript.to_string(), "Subscript");
    }

    #[test]
    fn vertical_position_from_str() {
        assert_eq!(VerticalPosition::from_str("Normal").unwrap(), VerticalPosition::Normal);
        assert_eq!(
            VerticalPosition::from_str("superscript").unwrap(),
            VerticalPosition::Superscript
        );
        assert_eq!(VerticalPosition::from_str("sub").unwrap(), VerticalPosition::Subscript);
        assert!(VerticalPosition::from_str("middle").is_err());
    }

    #[test]
    fn vertical_position_try_from_u8() {
        assert_eq!(VerticalPosition::try_from(0u8).unwrap(), VerticalPosition::Normal);
        assert_eq!(VerticalPosition::try_from(1u8).unwrap(), VerticalPosition::Superscript);
        assert_eq!(VerticalPosition::try_from(2u8).unwrap(), VerticalPosition::Subscript);
        assert!(VerticalPosition::try_from(3u8).is_err());
    }

    #[test]
    fn vertical_position_str_roundtrip() {
        for v in
            &[VerticalPosition::Normal, VerticalPosition::Superscript, VerticalPosition::Subscript]
        {
            let s = v.to_string();
            let back = VerticalPosition::from_str(&s).unwrap();
            assert_eq!(&back, v);
        }
    }

    // ===================================================================
    // BorderLineType
    // ===================================================================

    #[test]
    fn border_line_type_default_is_none() {
        assert_eq!(BorderLineType::default(), BorderLineType::None);
    }

    #[test]
    fn border_line_type_display() {
        assert_eq!(BorderLineType::None.to_string(), "None");
        assert_eq!(BorderLineType::Solid.to_string(), "Solid");
        assert_eq!(BorderLineType::DashDot.to_string(), "DashDot");
        assert_eq!(BorderLineType::ThickBetweenSlim.to_string(), "ThickBetweenSlim");
    }

    #[test]
    fn border_line_type_from_str() {
        assert_eq!(BorderLineType::from_str("None").unwrap(), BorderLineType::None);
        assert_eq!(BorderLineType::from_str("solid").unwrap(), BorderLineType::Solid);
        assert_eq!(BorderLineType::from_str("dash_dot").unwrap(), BorderLineType::DashDot);
        assert_eq!(BorderLineType::from_str("double").unwrap(), BorderLineType::Double);
        assert!(BorderLineType::from_str("wavy").is_err());
    }

    #[test]
    fn border_line_type_try_from_u8() {
        assert_eq!(BorderLineType::try_from(0u8).unwrap(), BorderLineType::None);
        assert_eq!(BorderLineType::try_from(1u8).unwrap(), BorderLineType::Solid);
        assert_eq!(BorderLineType::try_from(10u8).unwrap(), BorderLineType::ThickBetweenSlim);
        assert!(BorderLineType::try_from(11u8).is_err());
    }

    #[test]
    fn border_line_type_str_roundtrip() {
        for v in &[
            BorderLineType::None,
            BorderLineType::Solid,
            BorderLineType::Dash,
            BorderLineType::Dot,
            BorderLineType::DashDot,
            BorderLineType::DashDotDot,
            BorderLineType::LongDash,
            BorderLineType::TripleDot,
            BorderLineType::Double,
            BorderLineType::DoubleSlim,
            BorderLineType::ThickBetweenSlim,
        ] {
            let s = v.to_string();
            let back = BorderLineType::from_str(&s).unwrap();
            assert_eq!(&back, v);
        }
    }

    // ===================================================================
    // FillBrushType
    // ===================================================================

    #[test]
    fn fill_brush_type_default_is_none() {
        assert_eq!(FillBrushType::default(), FillBrushType::None);
    }

    #[test]
    fn fill_brush_type_display() {
        assert_eq!(FillBrushType::None.to_string(), "None");
        assert_eq!(FillBrushType::Solid.to_string(), "Solid");
        assert_eq!(FillBrushType::Gradient.to_string(), "Gradient");
        assert_eq!(FillBrushType::Pattern.to_string(), "Pattern");
    }

    #[test]
    fn fill_brush_type_from_str() {
        assert_eq!(FillBrushType::from_str("None").unwrap(), FillBrushType::None);
        assert_eq!(FillBrushType::from_str("solid").unwrap(), FillBrushType::Solid);
        assert_eq!(FillBrushType::from_str("gradient").unwrap(), FillBrushType::Gradient);
        assert!(FillBrushType::from_str("texture").is_err());
    }

    #[test]
    fn fill_brush_type_try_from_u8() {
        assert_eq!(FillBrushType::try_from(0u8).unwrap(), FillBrushType::None);
        assert_eq!(FillBrushType::try_from(1u8).unwrap(), FillBrushType::Solid);
        assert_eq!(FillBrushType::try_from(2u8).unwrap(), FillBrushType::Gradient);
        assert_eq!(FillBrushType::try_from(3u8).unwrap(), FillBrushType::Pattern);
        assert!(FillBrushType::try_from(4u8).is_err());
    }

    #[test]
    fn fill_brush_type_str_roundtrip() {
        for v in &[
            FillBrushType::None,
            FillBrushType::Solid,
            FillBrushType::Gradient,
            FillBrushType::Pattern,
        ] {
            let s = v.to_string();
            let back = FillBrushType::from_str(&s).unwrap();
            assert_eq!(&back, v);
        }
    }

    // ===================================================================
    // Cross-enum size assertions (compile-time already, but test at runtime too)
    // ===================================================================

    #[test]
    fn all_enums_are_one_byte() {
        assert_eq!(std::mem::size_of::<Alignment>(), 1);
        assert_eq!(std::mem::size_of::<LineSpacingType>(), 1);
        assert_eq!(std::mem::size_of::<BreakType>(), 1);
        assert_eq!(std::mem::size_of::<Language>(), 1);
        assert_eq!(std::mem::size_of::<UnderlineType>(), 1);
        assert_eq!(std::mem::size_of::<StrikeoutShape>(), 1);
        assert_eq!(std::mem::size_of::<OutlineType>(), 1);
        assert_eq!(std::mem::size_of::<ShadowType>(), 1);
        assert_eq!(std::mem::size_of::<EmbossType>(), 1);
        assert_eq!(std::mem::size_of::<EngraveType>(), 1);
        assert_eq!(std::mem::size_of::<VerticalPosition>(), 1);
        assert_eq!(std::mem::size_of::<BorderLineType>(), 1);
        assert_eq!(std::mem::size_of::<FillBrushType>(), 1);
        assert_eq!(std::mem::size_of::<ApplyPageType>(), 1);
        assert_eq!(std::mem::size_of::<NumberFormatType>(), 1);
        assert_eq!(std::mem::size_of::<PageNumberPosition>(), 1);
    }

    // ===================================================================
    // ApplyPageType
    // ===================================================================

    #[test]
    fn apply_page_type_default_is_both() {
        assert_eq!(ApplyPageType::default(), ApplyPageType::Both);
    }

    #[test]
    fn apply_page_type_display() {
        assert_eq!(ApplyPageType::Both.to_string(), "Both");
        assert_eq!(ApplyPageType::Even.to_string(), "Even");
        assert_eq!(ApplyPageType::Odd.to_string(), "Odd");
    }

    #[test]
    fn apply_page_type_from_str() {
        assert_eq!(ApplyPageType::from_str("Both").unwrap(), ApplyPageType::Both);
        assert_eq!(ApplyPageType::from_str("BOTH").unwrap(), ApplyPageType::Both);
        assert_eq!(ApplyPageType::from_str("even").unwrap(), ApplyPageType::Even);
        assert_eq!(ApplyPageType::from_str("ODD").unwrap(), ApplyPageType::Odd);
        assert!(ApplyPageType::from_str("invalid").is_err());
    }

    #[test]
    fn apply_page_type_try_from_u8() {
        assert_eq!(ApplyPageType::try_from(0u8).unwrap(), ApplyPageType::Both);
        assert_eq!(ApplyPageType::try_from(1u8).unwrap(), ApplyPageType::Even);
        assert_eq!(ApplyPageType::try_from(2u8).unwrap(), ApplyPageType::Odd);
        assert!(ApplyPageType::try_from(3u8).is_err());
    }

    #[test]
    fn apply_page_type_str_roundtrip() {
        for v in &[ApplyPageType::Both, ApplyPageType::Even, ApplyPageType::Odd] {
            let s = v.to_string();
            let back = ApplyPageType::from_str(&s).unwrap();
            assert_eq!(&back, v);
        }
    }

    // ===================================================================
    // NumberFormatType
    // ===================================================================

    #[test]
    fn number_format_type_default_is_digit() {
        assert_eq!(NumberFormatType::default(), NumberFormatType::Digit);
    }

    #[test]
    fn number_format_type_display() {
        assert_eq!(NumberFormatType::Digit.to_string(), "Digit");
        assert_eq!(NumberFormatType::CircledDigit.to_string(), "CircledDigit");
        assert_eq!(NumberFormatType::RomanCapital.to_string(), "RomanCapital");
        assert_eq!(NumberFormatType::HanjaDigit.to_string(), "HanjaDigit");
    }

    #[test]
    fn number_format_type_from_str() {
        assert_eq!(NumberFormatType::from_str("Digit").unwrap(), NumberFormatType::Digit);
        assert_eq!(NumberFormatType::from_str("DIGIT").unwrap(), NumberFormatType::Digit);
        assert_eq!(
            NumberFormatType::from_str("CircledDigit").unwrap(),
            NumberFormatType::CircledDigit
        );
        assert_eq!(
            NumberFormatType::from_str("ROMAN_CAPITAL").unwrap(),
            NumberFormatType::RomanCapital
        );
        assert!(NumberFormatType::from_str("invalid").is_err());
    }

    #[test]
    fn number_format_type_try_from_u8() {
        assert_eq!(NumberFormatType::try_from(0u8).unwrap(), NumberFormatType::Digit);
        assert_eq!(NumberFormatType::try_from(1u8).unwrap(), NumberFormatType::CircledDigit);
        assert_eq!(NumberFormatType::try_from(8u8).unwrap(), NumberFormatType::HanjaDigit);
        assert_eq!(
            NumberFormatType::try_from(9u8).unwrap(),
            NumberFormatType::CircledHangulSyllable
        );
        assert_eq!(NumberFormatType::try_from(10u8).unwrap(), NumberFormatType::CircledLatinSmall);
        assert!(NumberFormatType::try_from(11u8).is_err());
    }

    #[test]
    fn number_format_type_circled_hangul_syllable() {
        assert_eq!(NumberFormatType::CircledHangulSyllable.to_string(), "CircledHangulSyllable");
        assert_eq!(
            NumberFormatType::from_str("CircledHangulSyllable").unwrap(),
            NumberFormatType::CircledHangulSyllable
        );
        assert_eq!(
            NumberFormatType::from_str("CIRCLED_HANGUL_SYLLABLE").unwrap(),
            NumberFormatType::CircledHangulSyllable
        );
    }

    #[test]
    fn number_format_type_str_roundtrip() {
        for v in &[
            NumberFormatType::Digit,
            NumberFormatType::CircledDigit,
            NumberFormatType::RomanCapital,
            NumberFormatType::RomanSmall,
            NumberFormatType::LatinCapital,
            NumberFormatType::LatinSmall,
            NumberFormatType::HangulSyllable,
            NumberFormatType::HangulJamo,
            NumberFormatType::HanjaDigit,
            NumberFormatType::CircledHangulSyllable,
            NumberFormatType::CircledLatinSmall,
        ] {
            let s = v.to_string();
            let back = NumberFormatType::from_str(&s).unwrap();
            assert_eq!(&back, v);
        }
    }

    // ===================================================================
    // PageNumberPosition
    // ===================================================================

    #[test]
    fn page_number_position_default_is_top_center() {
        assert_eq!(PageNumberPosition::default(), PageNumberPosition::TopCenter);
    }

    #[test]
    fn page_number_position_display() {
        assert_eq!(PageNumberPosition::None.to_string(), "None");
        assert_eq!(PageNumberPosition::TopCenter.to_string(), "TopCenter");
        assert_eq!(PageNumberPosition::BottomCenter.to_string(), "BottomCenter");
        assert_eq!(PageNumberPosition::InsideBottom.to_string(), "InsideBottom");
    }

    #[test]
    fn page_number_position_from_str() {
        assert_eq!(PageNumberPosition::from_str("None").unwrap(), PageNumberPosition::None);
        assert_eq!(
            PageNumberPosition::from_str("BOTTOM_CENTER").unwrap(),
            PageNumberPosition::BottomCenter
        );
        assert_eq!(
            PageNumberPosition::from_str("bottom-center").unwrap(),
            PageNumberPosition::BottomCenter
        );
        assert_eq!(PageNumberPosition::from_str("TopLeft").unwrap(), PageNumberPosition::TopLeft);
        assert!(PageNumberPosition::from_str("invalid").is_err());
    }

    #[test]
    fn page_number_position_try_from_u8() {
        assert_eq!(PageNumberPosition::try_from(0u8).unwrap(), PageNumberPosition::None);
        assert_eq!(PageNumberPosition::try_from(2u8).unwrap(), PageNumberPosition::TopCenter);
        assert_eq!(PageNumberPosition::try_from(5u8).unwrap(), PageNumberPosition::BottomCenter);
        assert_eq!(PageNumberPosition::try_from(10u8).unwrap(), PageNumberPosition::InsideBottom);
        assert!(PageNumberPosition::try_from(11u8).is_err());
    }

    #[test]
    fn page_number_position_str_roundtrip() {
        for v in &[
            PageNumberPosition::None,
            PageNumberPosition::TopLeft,
            PageNumberPosition::TopCenter,
            PageNumberPosition::TopRight,
            PageNumberPosition::BottomLeft,
            PageNumberPosition::BottomCenter,
            PageNumberPosition::BottomRight,
            PageNumberPosition::OutsideTop,
            PageNumberPosition::OutsideBottom,
            PageNumberPosition::InsideTop,
            PageNumberPosition::InsideBottom,
        ] {
            let s = v.to_string();
            let back = PageNumberPosition::from_str(&s).unwrap();
            assert_eq!(&back, v);
        }
    }

    // ===================================================================
    // WordBreakType
    // ===================================================================

    #[test]
    fn word_break_type_default_is_keep_word() {
        assert_eq!(WordBreakType::default(), WordBreakType::KeepWord);
    }

    #[test]
    fn word_break_type_display() {
        assert_eq!(WordBreakType::KeepWord.to_string(), "KEEP_WORD");
        assert_eq!(WordBreakType::BreakWord.to_string(), "BREAK_WORD");
        assert_eq!(WordBreakType::Hyphenation.to_string(), "HYPHENATION");
    }

    #[test]
    fn word_break_type_from_str() {
        assert_eq!(WordBreakType::from_str("KEEP_WORD").unwrap(), WordBreakType::KeepWord);
        assert_eq!(WordBreakType::from_str("KeepWord").unwrap(), WordBreakType::KeepWord);
        assert_eq!(WordBreakType::from_str("keep_word").unwrap(), WordBreakType::KeepWord);
        assert_eq!(WordBreakType::from_str("BREAK_WORD").unwrap(), WordBreakType::BreakWord);
        assert_eq!(WordBreakType::from_str("BreakWord").unwrap(), WordBreakType::BreakWord);
        assert_eq!(WordBreakType::from_str("break_word").unwrap(), WordBreakType::BreakWord);
        assert_eq!(WordBreakType::from_str("HYPHENATION").unwrap(), WordBreakType::Hyphenation);
        assert_eq!(WordBreakType::from_str("Hyphenation").unwrap(), WordBreakType::Hyphenation);
        assert_eq!(WordBreakType::from_str("hyphenation").unwrap(), WordBreakType::Hyphenation);
        assert!(WordBreakType::from_str("invalid").is_err());
    }

    #[test]
    fn word_break_type_try_from_u8() {
        assert_eq!(WordBreakType::try_from(0u8).unwrap(), WordBreakType::KeepWord);
        assert_eq!(WordBreakType::try_from(1u8).unwrap(), WordBreakType::BreakWord);
        assert_eq!(WordBreakType::try_from(2u8).unwrap(), WordBreakType::Hyphenation);
        assert!(WordBreakType::try_from(3u8).is_err());
    }

    #[test]
    fn word_break_type_serde_roundtrip() {
        for v in &[WordBreakType::KeepWord, WordBreakType::BreakWord, WordBreakType::Hyphenation] {
            let json = serde_json::to_string(v).unwrap();
            let back: WordBreakType = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, v);
        }
    }

    #[test]
    fn word_break_type_str_roundtrip() {
        for v in &[WordBreakType::KeepWord, WordBreakType::BreakWord, WordBreakType::Hyphenation] {
            let s = v.to_string();
            let back = WordBreakType::from_str(&s).unwrap();
            assert_eq!(&back, v);
        }
    }

    // ===================================================================
    // EmphasisType
    // ===================================================================

    #[test]
    fn emphasis_type_default_is_none() {
        assert_eq!(EmphasisType::default(), EmphasisType::None);
    }

    #[test]
    fn emphasis_type_display_pascal_case() {
        assert_eq!(EmphasisType::None.to_string(), "None");
        assert_eq!(EmphasisType::DotAbove.to_string(), "DotAbove");
        assert_eq!(EmphasisType::RingAbove.to_string(), "RingAbove");
        assert_eq!(EmphasisType::Tilde.to_string(), "Tilde");
        assert_eq!(EmphasisType::Caron.to_string(), "Caron");
        assert_eq!(EmphasisType::Side.to_string(), "Side");
        assert_eq!(EmphasisType::Colon.to_string(), "Colon");
        assert_eq!(EmphasisType::GraveAccent.to_string(), "GraveAccent");
        assert_eq!(EmphasisType::AcuteAccent.to_string(), "AcuteAccent");
        assert_eq!(EmphasisType::Circumflex.to_string(), "Circumflex");
        assert_eq!(EmphasisType::Macron.to_string(), "Macron");
        assert_eq!(EmphasisType::HookAbove.to_string(), "HookAbove");
        assert_eq!(EmphasisType::DotBelow.to_string(), "DotBelow");
    }

    #[test]
    fn emphasis_type_from_str_screaming_snake_case() {
        assert_eq!(EmphasisType::from_str("NONE").unwrap(), EmphasisType::None);
        assert_eq!(EmphasisType::from_str("DOT_ABOVE").unwrap(), EmphasisType::DotAbove);
        assert_eq!(EmphasisType::from_str("RING_ABOVE").unwrap(), EmphasisType::RingAbove);
        assert_eq!(EmphasisType::from_str("GRAVE_ACCENT").unwrap(), EmphasisType::GraveAccent);
        assert_eq!(EmphasisType::from_str("DOT_BELOW").unwrap(), EmphasisType::DotBelow);
    }

    #[test]
    fn emphasis_type_from_str_pascal_case() {
        assert_eq!(EmphasisType::from_str("None").unwrap(), EmphasisType::None);
        assert_eq!(EmphasisType::from_str("DotAbove").unwrap(), EmphasisType::DotAbove);
        assert_eq!(EmphasisType::from_str("HookAbove").unwrap(), EmphasisType::HookAbove);
    }

    #[test]
    fn emphasis_type_from_str_invalid() {
        let err = EmphasisType::from_str("INVALID").unwrap_err();
        match err {
            FoundationError::ParseError { ref type_name, ref value, .. } => {
                assert_eq!(type_name, "EmphasisType");
                assert_eq!(value, "INVALID");
            }
            other => panic!("unexpected: {other}"),
        }
    }

    #[test]
    fn emphasis_type_try_from_u8() {
        assert_eq!(EmphasisType::try_from(0u8).unwrap(), EmphasisType::None);
        assert_eq!(EmphasisType::try_from(1u8).unwrap(), EmphasisType::DotAbove);
        assert_eq!(EmphasisType::try_from(12u8).unwrap(), EmphasisType::DotBelow);
        assert!(EmphasisType::try_from(13u8).is_err());
        assert!(EmphasisType::try_from(255u8).is_err());
    }

    #[test]
    fn emphasis_type_repr_values() {
        assert_eq!(EmphasisType::None as u8, 0);
        assert_eq!(EmphasisType::DotAbove as u8, 1);
        assert_eq!(EmphasisType::DotBelow as u8, 12);
    }

    #[test]
    fn emphasis_type_serde_roundtrip() {
        for variant in &[
            EmphasisType::None,
            EmphasisType::DotAbove,
            EmphasisType::RingAbove,
            EmphasisType::DotBelow,
        ] {
            let json = serde_json::to_string(variant).unwrap();
            let back: EmphasisType = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, variant);
        }
    }

    #[test]
    fn emphasis_type_str_roundtrip() {
        for variant in &[
            EmphasisType::None,
            EmphasisType::DotAbove,
            EmphasisType::GraveAccent,
            EmphasisType::DotBelow,
        ] {
            let s = variant.to_string();
            let back = EmphasisType::from_str(&s).unwrap();
            assert_eq!(&back, variant);
        }
    }
}
