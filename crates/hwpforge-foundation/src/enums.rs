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

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::FoundationError;

// ---------------------------------------------------------------------------
// Alignment
// ---------------------------------------------------------------------------

/// Horizontal text alignment within a paragraph.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::Alignment;
///
/// assert_eq!(Alignment::default(), Alignment::Left);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum Alignment {
    /// Left-aligned (default).
    #[default]
    Left = 0,
    /// Centered.
    Center = 1,
    /// Right-aligned.
    Right = 2,
    /// Justified (both edges flush).
    Justify = 3,
}

impl fmt::Display for Alignment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Left => f.write_str("Left"),
            Self::Center => f.write_str("Center"),
            Self::Right => f.write_str("Right"),
            Self::Justify => f.write_str("Justify"),
        }
    }
}

impl std::str::FromStr for Alignment {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Left" | "left" => Ok(Self::Left),
            "Center" | "center" => Ok(Self::Center),
            "Right" | "right" => Ok(Self::Right),
            "Justify" | "justify" => Ok(Self::Justify),
            _ => Err(FoundationError::ParseError {
                type_name: "Alignment".to_string(),
                value: s.to_string(),
                valid_values: "Left, Center, Right, Justify".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for Alignment {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Left),
            1 => Ok(Self::Center),
            2 => Ok(Self::Right),
            3 => Ok(Self::Justify),
            _ => Err(FoundationError::ParseError {
                type_name: "Alignment".to_string(),
                value: value.to_string(),
                valid_values: "0 (Left), 1 (Center), 2 (Right), 3 (Justify)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for Alignment {
    fn schema_name() -> String {
        "Alignment".to_string()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// LineSpacingType
// ---------------------------------------------------------------------------

/// How line spacing is calculated.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::LineSpacingType;
///
/// assert_eq!(LineSpacingType::default(), LineSpacingType::Percentage);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum LineSpacingType {
    /// Spacing as a percentage of the font size (default: 160%).
    #[default]
    Percentage = 0,
    /// Fixed spacing in HwpUnit, regardless of font size.
    Fixed = 1,
    /// Space between the bottom of one line and top of the next.
    BetweenLines = 2,
}

impl fmt::Display for LineSpacingType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Percentage => f.write_str("Percentage"),
            Self::Fixed => f.write_str("Fixed"),
            Self::BetweenLines => f.write_str("BetweenLines"),
        }
    }
}

impl std::str::FromStr for LineSpacingType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Percentage" | "percentage" => Ok(Self::Percentage),
            "Fixed" | "fixed" => Ok(Self::Fixed),
            "BetweenLines" | "betweenlines" | "between_lines" => Ok(Self::BetweenLines),
            _ => Err(FoundationError::ParseError {
                type_name: "LineSpacingType".to_string(),
                value: s.to_string(),
                valid_values: "Percentage, Fixed, BetweenLines".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for LineSpacingType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Percentage),
            1 => Ok(Self::Fixed),
            2 => Ok(Self::BetweenLines),
            _ => Err(FoundationError::ParseError {
                type_name: "LineSpacingType".to_string(),
                value: value.to_string(),
                valid_values: "0 (Percentage), 1 (Fixed), 2 (BetweenLines)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for LineSpacingType {
    fn schema_name() -> String {
        "LineSpacingType".to_string()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// BreakType
// ---------------------------------------------------------------------------

/// Page/column break type before a paragraph.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::BreakType;
///
/// assert_eq!(BreakType::default(), BreakType::None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum BreakType {
    /// No break.
    #[default]
    None = 0,
    /// Column break.
    Column = 1,
    /// Page break.
    Page = 2,
}

impl fmt::Display for BreakType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Column => f.write_str("Column"),
            Self::Page => f.write_str("Page"),
        }
    }
}

impl std::str::FromStr for BreakType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" | "none" => Ok(Self::None),
            "Column" | "column" => Ok(Self::Column),
            "Page" | "page" => Ok(Self::Page),
            _ => Err(FoundationError::ParseError {
                type_name: "BreakType".to_string(),
                value: s.to_string(),
                valid_values: "None, Column, Page".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for BreakType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Column),
            2 => Ok(Self::Page),
            _ => Err(FoundationError::ParseError {
                type_name: "BreakType".to_string(),
                value: value.to_string(),
                valid_values: "0 (None), 1 (Column), 2 (Page)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for BreakType {
    fn schema_name() -> String {
        "BreakType".to_string()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// Language
// ---------------------------------------------------------------------------

/// HWP5 language slots for font assignment.
///
/// Each character shape stores a font per language slot.
/// The discriminant values match the HWP5 specification exactly.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::Language;
///
/// assert_eq!(Language::COUNT, 7);
/// assert_eq!(Language::Korean as u8, 0);
/// ```
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
#[repr(u8)]
pub enum Language {
    /// Korean (slot 0).
    #[default]
    Korean = 0,
    /// English (slot 1).
    English = 1,
    /// Chinese characters / Hanja (slot 2).
    Hanja = 2,
    /// Japanese (slot 3).
    Japanese = 3,
    /// Other languages (slot 4).
    Other = 4,
    /// Symbol characters (slot 5).
    Symbol = 5,
    /// User-defined (slot 6).
    User = 6,
}

impl Language {
    /// Total number of language slots (7), matching the HWP5 spec.
    pub const COUNT: usize = 7;

    /// All language variants in slot order.
    pub const ALL: [Self; 7] = [
        Self::Korean,
        Self::English,
        Self::Hanja,
        Self::Japanese,
        Self::Other,
        Self::Symbol,
        Self::User,
    ];
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Korean => f.write_str("Korean"),
            Self::English => f.write_str("English"),
            Self::Hanja => f.write_str("Hanja"),
            Self::Japanese => f.write_str("Japanese"),
            Self::Other => f.write_str("Other"),
            Self::Symbol => f.write_str("Symbol"),
            Self::User => f.write_str("User"),
        }
    }
}

impl std::str::FromStr for Language {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Korean" | "korean" => Ok(Self::Korean),
            "English" | "english" => Ok(Self::English),
            "Hanja" | "hanja" => Ok(Self::Hanja),
            "Japanese" | "japanese" => Ok(Self::Japanese),
            "Other" | "other" => Ok(Self::Other),
            "Symbol" | "symbol" => Ok(Self::Symbol),
            "User" | "user" => Ok(Self::User),
            _ => Err(FoundationError::ParseError {
                type_name: "Language".to_string(),
                value: s.to_string(),
                valid_values: "Korean, English, Hanja, Japanese, Other, Symbol, User".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for Language {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Korean),
            1 => Ok(Self::English),
            2 => Ok(Self::Hanja),
            3 => Ok(Self::Japanese),
            4 => Ok(Self::Other),
            5 => Ok(Self::Symbol),
            6 => Ok(Self::User),
            _ => Err(FoundationError::ParseError {
                type_name: "Language".to_string(),
                value: value.to_string(),
                valid_values: "0-6 (Korean, English, Hanja, Japanese, Other, Symbol, User)"
                    .to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for Language {
    fn schema_name() -> String {
        "Language".to_string()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        gen.subschema_for::<String>()
    }
}

// Compile-time size assertions: all enums are 1 byte
const _: () = assert!(std::mem::size_of::<Alignment>() == 1);
const _: () = assert!(std::mem::size_of::<LineSpacingType>() == 1);
const _: () = assert!(std::mem::size_of::<BreakType>() == 1);
const _: () = assert!(std::mem::size_of::<Language>() == 1);

#[cfg(test)]
mod tests {
    use super::*;
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
    }

    #[test]
    fn alignment_from_str_pascal_case() {
        assert_eq!(Alignment::from_str("Left").unwrap(), Alignment::Left);
        assert_eq!(Alignment::from_str("Center").unwrap(), Alignment::Center);
        assert_eq!(Alignment::from_str("Right").unwrap(), Alignment::Right);
        assert_eq!(Alignment::from_str("Justify").unwrap(), Alignment::Justify);
    }

    #[test]
    fn alignment_from_str_lower_case() {
        assert_eq!(Alignment::from_str("left").unwrap(), Alignment::Left);
        assert_eq!(Alignment::from_str("center").unwrap(), Alignment::Center);
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
        assert!(Alignment::try_from(4u8).is_err());
        assert!(Alignment::try_from(255u8).is_err());
    }

    #[test]
    fn alignment_repr_values() {
        assert_eq!(Alignment::Left as u8, 0);
        assert_eq!(Alignment::Center as u8, 1);
        assert_eq!(Alignment::Right as u8, 2);
        assert_eq!(Alignment::Justify as u8, 3);
    }

    #[test]
    fn alignment_serde_roundtrip() {
        for variant in &[Alignment::Left, Alignment::Center, Alignment::Right, Alignment::Justify] {
            let json = serde_json::to_string(variant).unwrap();
            let back: Alignment = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, variant);
        }
    }

    #[test]
    fn alignment_str_roundtrip() {
        for variant in &[Alignment::Left, Alignment::Center, Alignment::Right, Alignment::Justify] {
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
    }

    #[test]
    fn line_spacing_from_str() {
        assert_eq!(LineSpacingType::from_str("Percentage").unwrap(), LineSpacingType::Percentage);
        assert_eq!(LineSpacingType::from_str("Fixed").unwrap(), LineSpacingType::Fixed);
        assert_eq!(
            LineSpacingType::from_str("BetweenLines").unwrap(),
            LineSpacingType::BetweenLines
        );
        assert!(LineSpacingType::from_str("invalid").is_err());
    }

    #[test]
    fn line_spacing_try_from_u8() {
        assert_eq!(LineSpacingType::try_from(0u8).unwrap(), LineSpacingType::Percentage);
        assert_eq!(LineSpacingType::try_from(1u8).unwrap(), LineSpacingType::Fixed);
        assert_eq!(LineSpacingType::try_from(2u8).unwrap(), LineSpacingType::BetweenLines);
        assert!(LineSpacingType::try_from(3u8).is_err());
    }

    #[test]
    fn line_spacing_str_roundtrip() {
        for v in
            &[LineSpacingType::Percentage, LineSpacingType::Fixed, LineSpacingType::BetweenLines]
        {
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
    // Cross-enum size assertions (compile-time already, but test at runtime too)
    // ===================================================================

    #[test]
    fn all_enums_are_one_byte() {
        assert_eq!(std::mem::size_of::<Alignment>(), 1);
        assert_eq!(std::mem::size_of::<LineSpacingType>(), 1);
        assert_eq!(std::mem::size_of::<BreakType>(), 1);
        assert_eq!(std::mem::size_of::<Language>(), 1);
    }
}
