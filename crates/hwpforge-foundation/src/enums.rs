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
    /// Distribute spacing evenly between characters.
    Distribute = 4,
    /// Distribute spacing evenly between characters, last line flush.
    DistributeFlush = 5,
}

impl fmt::Display for Alignment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Left => f.write_str("Left"),
            Self::Center => f.write_str("Center"),
            Self::Right => f.write_str("Right"),
            Self::Justify => f.write_str("Justify"),
            Self::Distribute => f.write_str("Distribute"),
            Self::DistributeFlush => f.write_str("DistributeFlush"),
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
            "Distribute" | "distribute" => Ok(Self::Distribute),
            "DistributeFlush" | "distributeflush" | "distribute_flush" => Ok(Self::DistributeFlush),
            _ => Err(FoundationError::ParseError {
                type_name: "Alignment".to_string(),
                value: s.to_string(),
                valid_values: "Left, Center, Right, Justify, Distribute, DistributeFlush"
                    .to_string(),
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
            4 => Ok(Self::Distribute),
            5 => Ok(Self::DistributeFlush),
            _ => Err(FoundationError::ParseError {
                type_name: "Alignment".to_string(),
                value: value.to_string(),
                valid_values:
                    "0 (Left), 1 (Center), 2 (Right), 3 (Justify), 4 (Distribute), 5 (DistributeFlush)"
                        .to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for Alignment {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("Alignment")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
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
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("LineSpacingType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
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
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("BreakType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
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
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("Language")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// UnderlineType
// ---------------------------------------------------------------------------

/// Underline decoration type.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::UnderlineType;
///
/// assert_eq!(UnderlineType::default(), UnderlineType::None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum UnderlineType {
    /// No underline (default).
    #[default]
    None = 0,
    /// Single straight line below text.
    Bottom = 1,
    /// Single line centered on text.
    Center = 2,
    /// Single line above text.
    Top = 3,
}

impl fmt::Display for UnderlineType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Bottom => f.write_str("Bottom"),
            Self::Center => f.write_str("Center"),
            Self::Top => f.write_str("Top"),
        }
    }
}

impl std::str::FromStr for UnderlineType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" | "none" => Ok(Self::None),
            "Bottom" | "bottom" => Ok(Self::Bottom),
            "Center" | "center" => Ok(Self::Center),
            "Top" | "top" => Ok(Self::Top),
            _ => Err(FoundationError::ParseError {
                type_name: "UnderlineType".to_string(),
                value: s.to_string(),
                valid_values: "None, Bottom, Center, Top".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for UnderlineType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Bottom),
            2 => Ok(Self::Center),
            3 => Ok(Self::Top),
            _ => Err(FoundationError::ParseError {
                type_name: "UnderlineType".to_string(),
                value: value.to_string(),
                valid_values: "0 (None), 1 (Bottom), 2 (Center), 3 (Top)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for UnderlineType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("UnderlineType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// StrikeoutShape
// ---------------------------------------------------------------------------

/// Strikeout line shape.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::StrikeoutShape;
///
/// assert_eq!(StrikeoutShape::default(), StrikeoutShape::None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum StrikeoutShape {
    /// No strikeout (default).
    #[default]
    None = 0,
    /// Continuous straight line.
    Continuous = 1,
    /// Dashed line.
    Dash = 2,
    /// Dotted line.
    Dot = 3,
    /// Dash-dot pattern.
    DashDot = 4,
    /// Dash-dot-dot pattern.
    DashDotDot = 5,
}

impl fmt::Display for StrikeoutShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Continuous => f.write_str("Continuous"),
            Self::Dash => f.write_str("Dash"),
            Self::Dot => f.write_str("Dot"),
            Self::DashDot => f.write_str("DashDot"),
            Self::DashDotDot => f.write_str("DashDotDot"),
        }
    }
}

impl std::str::FromStr for StrikeoutShape {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" | "none" => Ok(Self::None),
            "Continuous" | "continuous" => Ok(Self::Continuous),
            "Dash" | "dash" => Ok(Self::Dash),
            "Dot" | "dot" => Ok(Self::Dot),
            "DashDot" | "dashdot" | "dash_dot" => Ok(Self::DashDot),
            "DashDotDot" | "dashdotdot" | "dash_dot_dot" => Ok(Self::DashDotDot),
            _ => Err(FoundationError::ParseError {
                type_name: "StrikeoutShape".to_string(),
                value: s.to_string(),
                valid_values: "None, Continuous, Dash, Dot, DashDot, DashDotDot".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for StrikeoutShape {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Continuous),
            2 => Ok(Self::Dash),
            3 => Ok(Self::Dot),
            4 => Ok(Self::DashDot),
            5 => Ok(Self::DashDotDot),
            _ => Err(FoundationError::ParseError {
                type_name: "StrikeoutShape".to_string(),
                value: value.to_string(),
                valid_values: "0-5 (None, Continuous, Dash, Dot, DashDot, DashDotDot)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for StrikeoutShape {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("StrikeoutShape")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// OutlineType
// ---------------------------------------------------------------------------

/// Text outline type (1pt border around glyphs).
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::OutlineType;
///
/// assert_eq!(OutlineType::default(), OutlineType::None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum OutlineType {
    /// No outline (default).
    #[default]
    None = 0,
    /// Solid 1pt outline.
    Solid = 1,
}

impl fmt::Display for OutlineType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Solid => f.write_str("Solid"),
        }
    }
}

impl std::str::FromStr for OutlineType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" | "none" => Ok(Self::None),
            "Solid" | "solid" => Ok(Self::Solid),
            _ => Err(FoundationError::ParseError {
                type_name: "OutlineType".to_string(),
                value: s.to_string(),
                valid_values: "None, Solid".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for OutlineType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Solid),
            _ => Err(FoundationError::ParseError {
                type_name: "OutlineType".to_string(),
                value: value.to_string(),
                valid_values: "0 (None), 1 (Solid)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for OutlineType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("OutlineType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// ShadowType
// ---------------------------------------------------------------------------

/// Text shadow type.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::ShadowType;
///
/// assert_eq!(ShadowType::default(), ShadowType::None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum ShadowType {
    /// No shadow (default).
    #[default]
    None = 0,
    /// Drop shadow.
    Drop = 1,
}

impl fmt::Display for ShadowType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Drop => f.write_str("Drop"),
        }
    }
}

impl std::str::FromStr for ShadowType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" | "none" => Ok(Self::None),
            "Drop" | "drop" => Ok(Self::Drop),
            _ => Err(FoundationError::ParseError {
                type_name: "ShadowType".to_string(),
                value: s.to_string(),
                valid_values: "None, Drop".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for ShadowType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Drop),
            _ => Err(FoundationError::ParseError {
                type_name: "ShadowType".to_string(),
                value: value.to_string(),
                valid_values: "0 (None), 1 (Drop)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for ShadowType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ShadowType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// EmbossType
// ---------------------------------------------------------------------------

/// Text embossing (raised appearance).
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::EmbossType;
///
/// assert_eq!(EmbossType::default(), EmbossType::None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum EmbossType {
    /// No emboss (default).
    #[default]
    None = 0,
    /// Raised emboss effect.
    Emboss = 1,
}

impl fmt::Display for EmbossType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Emboss => f.write_str("Emboss"),
        }
    }
}

impl std::str::FromStr for EmbossType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" | "none" => Ok(Self::None),
            "Emboss" | "emboss" => Ok(Self::Emboss),
            _ => Err(FoundationError::ParseError {
                type_name: "EmbossType".to_string(),
                value: s.to_string(),
                valid_values: "None, Emboss".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for EmbossType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Emboss),
            _ => Err(FoundationError::ParseError {
                type_name: "EmbossType".to_string(),
                value: value.to_string(),
                valid_values: "0 (None), 1 (Emboss)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for EmbossType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("EmbossType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// EngraveType
// ---------------------------------------------------------------------------

/// Text engraving (sunken appearance).
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::EngraveType;
///
/// assert_eq!(EngraveType::default(), EngraveType::None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum EngraveType {
    /// No engrave (default).
    #[default]
    None = 0,
    /// Sunken engrave effect.
    Engrave = 1,
}

impl fmt::Display for EngraveType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Engrave => f.write_str("Engrave"),
        }
    }
}

impl std::str::FromStr for EngraveType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" | "none" => Ok(Self::None),
            "Engrave" | "engrave" => Ok(Self::Engrave),
            _ => Err(FoundationError::ParseError {
                type_name: "EngraveType".to_string(),
                value: s.to_string(),
                valid_values: "None, Engrave".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for EngraveType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Engrave),
            _ => Err(FoundationError::ParseError {
                type_name: "EngraveType".to_string(),
                value: value.to_string(),
                valid_values: "0 (None), 1 (Engrave)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for EngraveType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("EngraveType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// VerticalPosition
// ---------------------------------------------------------------------------

/// Superscript/subscript position type.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::VerticalPosition;
///
/// assert_eq!(VerticalPosition::default(), VerticalPosition::Normal);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum VerticalPosition {
    /// Normal baseline (default).
    #[default]
    Normal = 0,
    /// Superscript.
    Superscript = 1,
    /// Subscript.
    Subscript = 2,
}

impl fmt::Display for VerticalPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Normal => f.write_str("Normal"),
            Self::Superscript => f.write_str("Superscript"),
            Self::Subscript => f.write_str("Subscript"),
        }
    }
}

impl std::str::FromStr for VerticalPosition {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Normal" | "normal" => Ok(Self::Normal),
            "Superscript" | "superscript" | "super" => Ok(Self::Superscript),
            "Subscript" | "subscript" | "sub" => Ok(Self::Subscript),
            _ => Err(FoundationError::ParseError {
                type_name: "VerticalPosition".to_string(),
                value: s.to_string(),
                valid_values: "Normal, Superscript, Subscript".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for VerticalPosition {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Normal),
            1 => Ok(Self::Superscript),
            2 => Ok(Self::Subscript),
            _ => Err(FoundationError::ParseError {
                type_name: "VerticalPosition".to_string(),
                value: value.to_string(),
                valid_values: "0 (Normal), 1 (Superscript), 2 (Subscript)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for VerticalPosition {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("VerticalPosition")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// BorderLineType
// ---------------------------------------------------------------------------

/// Border line type.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::BorderLineType;
///
/// assert_eq!(BorderLineType::default(), BorderLineType::None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum BorderLineType {
    /// No border.
    #[default]
    None = 0,
    /// Solid line.
    Solid = 1,
    /// Dashed line.
    Dash = 2,
    /// Dotted line.
    Dot = 3,
    /// Dash-dot pattern.
    DashDot = 4,
    /// Dash-dot-dot pattern.
    DashDotDot = 5,
    /// Long dash pattern.
    LongDash = 6,
    /// Triple dot pattern.
    TripleDot = 7,
    /// Double line.
    Double = 8,
    /// Thin-thick double.
    DoubleSlim = 9,
    /// Thick-thin double.
    ThickBetweenSlim = 10,
}

impl fmt::Display for BorderLineType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Solid => f.write_str("Solid"),
            Self::Dash => f.write_str("Dash"),
            Self::Dot => f.write_str("Dot"),
            Self::DashDot => f.write_str("DashDot"),
            Self::DashDotDot => f.write_str("DashDotDot"),
            Self::LongDash => f.write_str("LongDash"),
            Self::TripleDot => f.write_str("TripleDot"),
            Self::Double => f.write_str("Double"),
            Self::DoubleSlim => f.write_str("DoubleSlim"),
            Self::ThickBetweenSlim => f.write_str("ThickBetweenSlim"),
        }
    }
}

impl std::str::FromStr for BorderLineType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" | "none" => Ok(Self::None),
            "Solid" | "solid" => Ok(Self::Solid),
            "Dash" | "dash" => Ok(Self::Dash),
            "Dot" | "dot" => Ok(Self::Dot),
            "DashDot" | "dashdot" | "dash_dot" => Ok(Self::DashDot),
            "DashDotDot" | "dashdotdot" | "dash_dot_dot" => Ok(Self::DashDotDot),
            "LongDash" | "longdash" | "long_dash" => Ok(Self::LongDash),
            "TripleDot" | "tripledot" | "triple_dot" => Ok(Self::TripleDot),
            "Double" | "double" => Ok(Self::Double),
            "DoubleSlim" | "doubleslim" | "double_slim" => Ok(Self::DoubleSlim),
            "ThickBetweenSlim" | "thickbetweenslim" | "thick_between_slim" => {
                Ok(Self::ThickBetweenSlim)
            }
            _ => Err(FoundationError::ParseError {
                type_name: "BorderLineType".to_string(),
                value: s.to_string(),
                valid_values: "None, Solid, Dash, Dot, DashDot, DashDotDot, LongDash, TripleDot, Double, DoubleSlim, ThickBetweenSlim".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for BorderLineType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Solid),
            2 => Ok(Self::Dash),
            3 => Ok(Self::Dot),
            4 => Ok(Self::DashDot),
            5 => Ok(Self::DashDotDot),
            6 => Ok(Self::LongDash),
            7 => Ok(Self::TripleDot),
            8 => Ok(Self::Double),
            9 => Ok(Self::DoubleSlim),
            10 => Ok(Self::ThickBetweenSlim),
            _ => Err(FoundationError::ParseError {
                type_name: "BorderLineType".to_string(),
                value: value.to_string(),
                valid_values: "0-10 (None, Solid, Dash, Dot, DashDot, DashDotDot, LongDash, TripleDot, Double, DoubleSlim, ThickBetweenSlim)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for BorderLineType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("BorderLineType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// FillBrushType
// ---------------------------------------------------------------------------

/// Fill brush type for backgrounds.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::FillBrushType;
///
/// assert_eq!(FillBrushType::default(), FillBrushType::None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum FillBrushType {
    /// No fill (transparent, default).
    #[default]
    None = 0,
    /// Solid color fill.
    Solid = 1,
    /// Gradient fill (linear or radial).
    Gradient = 2,
    /// Pattern fill (hatch, dots, etc.).
    Pattern = 3,
}

impl fmt::Display for FillBrushType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Solid => f.write_str("Solid"),
            Self::Gradient => f.write_str("Gradient"),
            Self::Pattern => f.write_str("Pattern"),
        }
    }
}

impl std::str::FromStr for FillBrushType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" | "none" => Ok(Self::None),
            "Solid" | "solid" => Ok(Self::Solid),
            "Gradient" | "gradient" => Ok(Self::Gradient),
            "Pattern" | "pattern" => Ok(Self::Pattern),
            _ => Err(FoundationError::ParseError {
                type_name: "FillBrushType".to_string(),
                value: s.to_string(),
                valid_values: "None, Solid, Gradient, Pattern".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for FillBrushType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Solid),
            2 => Ok(Self::Gradient),
            3 => Ok(Self::Pattern),
            _ => Err(FoundationError::ParseError {
                type_name: "FillBrushType".to_string(),
                value: value.to_string(),
                valid_values: "0 (None), 1 (Solid), 2 (Gradient), 3 (Pattern)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for FillBrushType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("FillBrushType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// ApplyPageType
// ---------------------------------------------------------------------------

/// Which pages a header/footer applies to.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::ApplyPageType;
///
/// assert_eq!(ApplyPageType::default(), ApplyPageType::Both);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum ApplyPageType {
    /// Both even and odd pages (default).
    #[default]
    Both = 0,
    /// Even pages only.
    Even = 1,
    /// Odd pages only.
    Odd = 2,
}

impl fmt::Display for ApplyPageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Both => f.write_str("Both"),
            Self::Even => f.write_str("Even"),
            Self::Odd => f.write_str("Odd"),
        }
    }
}

impl std::str::FromStr for ApplyPageType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Both" | "both" | "BOTH" => Ok(Self::Both),
            "Even" | "even" | "EVEN" => Ok(Self::Even),
            "Odd" | "odd" | "ODD" => Ok(Self::Odd),
            _ => Err(FoundationError::ParseError {
                type_name: "ApplyPageType".to_string(),
                value: s.to_string(),
                valid_values: "Both, Even, Odd".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for ApplyPageType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Both),
            1 => Ok(Self::Even),
            2 => Ok(Self::Odd),
            _ => Err(FoundationError::ParseError {
                type_name: "ApplyPageType".to_string(),
                value: value.to_string(),
                valid_values: "0 (Both), 1 (Even), 2 (Odd)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for ApplyPageType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ApplyPageType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// NumberFormatType
// ---------------------------------------------------------------------------

/// Number format for page numbering.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::NumberFormatType;
///
/// assert_eq!(NumberFormatType::default(), NumberFormatType::Digit);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum NumberFormatType {
    /// Arabic digits: 1, 2, 3, ... (default).
    #[default]
    Digit = 0,
    /// Circled digits: ①, ②, ③, ...
    CircledDigit = 1,
    /// Roman capitals: I, II, III, ...
    RomanCapital = 2,
    /// Roman lowercase: i, ii, iii, ...
    RomanSmall = 3,
    /// Latin capitals: A, B, C, ...
    LatinCapital = 4,
    /// Latin lowercase: a, b, c, ...
    LatinSmall = 5,
    /// Hangul syllable: 가, 나, 다, ...
    HangulSyllable = 6,
    /// Hangul jamo: ㄱ, ㄴ, ㄷ, ...
    HangulJamo = 7,
    /// Hanja digits: 一, 二, 三, ...
    HanjaDigit = 8,
}

impl fmt::Display for NumberFormatType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Digit => f.write_str("Digit"),
            Self::CircledDigit => f.write_str("CircledDigit"),
            Self::RomanCapital => f.write_str("RomanCapital"),
            Self::RomanSmall => f.write_str("RomanSmall"),
            Self::LatinCapital => f.write_str("LatinCapital"),
            Self::LatinSmall => f.write_str("LatinSmall"),
            Self::HangulSyllable => f.write_str("HangulSyllable"),
            Self::HangulJamo => f.write_str("HangulJamo"),
            Self::HanjaDigit => f.write_str("HanjaDigit"),
        }
    }
}

impl std::str::FromStr for NumberFormatType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Digit" | "digit" | "DIGIT" => Ok(Self::Digit),
            "CircledDigit" | "circleddigit" | "CIRCLED_DIGIT" => Ok(Self::CircledDigit),
            "RomanCapital" | "romancapital" | "ROMAN_CAPITAL" => Ok(Self::RomanCapital),
            "RomanSmall" | "romansmall" | "ROMAN_SMALL" => Ok(Self::RomanSmall),
            "LatinCapital" | "latincapital" | "LATIN_CAPITAL" => Ok(Self::LatinCapital),
            "LatinSmall" | "latinsmall" | "LATIN_SMALL" => Ok(Self::LatinSmall),
            "HangulSyllable" | "hangulsyllable" | "HANGUL_SYLLABLE" => Ok(Self::HangulSyllable),
            "HangulJamo" | "hanguljamo" | "HANGUL_JAMO" => Ok(Self::HangulJamo),
            "HanjaDigit" | "hanjadigit" | "HANJA_DIGIT" => Ok(Self::HanjaDigit),
            _ => Err(FoundationError::ParseError {
                type_name: "NumberFormatType".to_string(),
                value: s.to_string(),
                valid_values: "Digit, CircledDigit, RomanCapital, RomanSmall, LatinCapital, LatinSmall, HangulSyllable, HangulJamo, HanjaDigit".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for NumberFormatType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Digit),
            1 => Ok(Self::CircledDigit),
            2 => Ok(Self::RomanCapital),
            3 => Ok(Self::RomanSmall),
            4 => Ok(Self::LatinCapital),
            5 => Ok(Self::LatinSmall),
            6 => Ok(Self::HangulSyllable),
            7 => Ok(Self::HangulJamo),
            8 => Ok(Self::HanjaDigit),
            _ => Err(FoundationError::ParseError {
                type_name: "NumberFormatType".to_string(),
                value: value.to_string(),
                valid_values: "0-8 (Digit, CircledDigit, RomanCapital, RomanSmall, LatinCapital, LatinSmall, HangulSyllable, HangulJamo, HanjaDigit)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for NumberFormatType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("NumberFormatType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// PageNumberPosition
// ---------------------------------------------------------------------------

/// Position of page numbers on the page.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::PageNumberPosition;
///
/// assert_eq!(PageNumberPosition::default(), PageNumberPosition::TopCenter);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum PageNumberPosition {
    /// No page number.
    None = 0,
    /// Top left.
    TopLeft = 1,
    /// Top center (default).
    #[default]
    TopCenter = 2,
    /// Top right.
    TopRight = 3,
    /// Bottom left.
    BottomLeft = 4,
    /// Bottom center.
    BottomCenter = 5,
    /// Bottom right.
    BottomRight = 6,
    /// Outside top.
    OutsideTop = 7,
    /// Outside bottom.
    OutsideBottom = 8,
    /// Inside top.
    InsideTop = 9,
    /// Inside bottom.
    InsideBottom = 10,
}

impl fmt::Display for PageNumberPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::TopLeft => f.write_str("TopLeft"),
            Self::TopCenter => f.write_str("TopCenter"),
            Self::TopRight => f.write_str("TopRight"),
            Self::BottomLeft => f.write_str("BottomLeft"),
            Self::BottomCenter => f.write_str("BottomCenter"),
            Self::BottomRight => f.write_str("BottomRight"),
            Self::OutsideTop => f.write_str("OutsideTop"),
            Self::OutsideBottom => f.write_str("OutsideBottom"),
            Self::InsideTop => f.write_str("InsideTop"),
            Self::InsideBottom => f.write_str("InsideBottom"),
        }
    }
}

impl std::str::FromStr for PageNumberPosition {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" | "none" | "NONE" => Ok(Self::None),
            "TopLeft" | "topleft" | "TOP_LEFT" | "top-left" => Ok(Self::TopLeft),
            "TopCenter" | "topcenter" | "TOP_CENTER" | "top-center" => Ok(Self::TopCenter),
            "TopRight" | "topright" | "TOP_RIGHT" | "top-right" => Ok(Self::TopRight),
            "BottomLeft" | "bottomleft" | "BOTTOM_LEFT" | "bottom-left" => Ok(Self::BottomLeft),
            "BottomCenter" | "bottomcenter" | "BOTTOM_CENTER" | "bottom-center" => {
                Ok(Self::BottomCenter)
            }
            "BottomRight" | "bottomright" | "BOTTOM_RIGHT" | "bottom-right" => {
                Ok(Self::BottomRight)
            }
            "OutsideTop" | "outsidetop" | "OUTSIDE_TOP" | "outside-top" => Ok(Self::OutsideTop),
            "OutsideBottom" | "outsidebottom" | "OUTSIDE_BOTTOM" | "outside-bottom" => {
                Ok(Self::OutsideBottom)
            }
            "InsideTop" | "insidetop" | "INSIDE_TOP" | "inside-top" => Ok(Self::InsideTop),
            "InsideBottom" | "insidebottom" | "INSIDE_BOTTOM" | "inside-bottom" => {
                Ok(Self::InsideBottom)
            }
            _ => Err(FoundationError::ParseError {
                type_name: "PageNumberPosition".to_string(),
                value: s.to_string(),
                valid_values: "None, TopLeft, TopCenter, TopRight, BottomLeft, BottomCenter, BottomRight, OutsideTop, OutsideBottom, InsideTop, InsideBottom".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for PageNumberPosition {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::TopLeft),
            2 => Ok(Self::TopCenter),
            3 => Ok(Self::TopRight),
            4 => Ok(Self::BottomLeft),
            5 => Ok(Self::BottomCenter),
            6 => Ok(Self::BottomRight),
            7 => Ok(Self::OutsideTop),
            8 => Ok(Self::OutsideBottom),
            9 => Ok(Self::InsideTop),
            10 => Ok(Self::InsideBottom),
            _ => Err(FoundationError::ParseError {
                type_name: "PageNumberPosition".to_string(),
                value: value.to_string(),
                valid_values: "0-10 (None, TopLeft, TopCenter, TopRight, BottomLeft, BottomCenter, BottomRight, OutsideTop, OutsideBottom, InsideTop, InsideBottom)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for PageNumberPosition {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("PageNumberPosition")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// WordBreakType
// ---------------------------------------------------------------------------

/// Word-breaking behavior for paragraph text justification.
///
/// Controls how 한글 distributes extra space in justified text.
/// `KeepWord` preserves word boundaries (natural spacing),
/// `BreakWord` allows breaking at any character (stretched spacing).
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::WordBreakType;
///
/// assert_eq!(WordBreakType::default(), WordBreakType::KeepWord);
/// assert_eq!(WordBreakType::KeepWord.to_string(), "KEEP_WORD");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum WordBreakType {
    /// Keep words intact — distribute space between words only (한글 default).
    #[default]
    KeepWord = 0,
    /// Allow breaking at any character — distribute space between all characters.
    BreakWord = 1,
}

impl fmt::Display for WordBreakType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeepWord => f.write_str("KEEP_WORD"),
            Self::BreakWord => f.write_str("BREAK_WORD"),
        }
    }
}

impl std::str::FromStr for WordBreakType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "KEEP_WORD" | "KeepWord" | "keep_word" => Ok(Self::KeepWord),
            "BREAK_WORD" | "BreakWord" | "break_word" => Ok(Self::BreakWord),
            _ => Err(FoundationError::ParseError {
                type_name: "WordBreakType".to_string(),
                value: s.to_string(),
                valid_values: "KEEP_WORD, BREAK_WORD".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for WordBreakType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::KeepWord),
            1 => Ok(Self::BreakWord),
            _ => Err(FoundationError::ParseError {
                type_name: "WordBreakType".to_string(),
                value: value.to_string(),
                valid_values: "0 (KeepWord), 1 (BreakWord)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for WordBreakType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("WordBreakType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// Compile-time size assertions: all enums are 1 byte
const _: () = assert!(std::mem::size_of::<Alignment>() == 1);
const _: () = assert!(std::mem::size_of::<LineSpacingType>() == 1);
const _: () = assert!(std::mem::size_of::<BreakType>() == 1);
const _: () = assert!(std::mem::size_of::<Language>() == 1);
const _: () = assert!(std::mem::size_of::<UnderlineType>() == 1);
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
    // StrikeoutShape
    // ===================================================================

    #[test]
    fn strikeout_shape_default_is_none() {
        assert_eq!(StrikeoutShape::default(), StrikeoutShape::None);
    }

    #[test]
    fn strikeout_shape_display() {
        assert_eq!(StrikeoutShape::None.to_string(), "None");
        assert_eq!(StrikeoutShape::Continuous.to_string(), "Continuous");
        assert_eq!(StrikeoutShape::Dash.to_string(), "Dash");
        assert_eq!(StrikeoutShape::DashDotDot.to_string(), "DashDotDot");
    }

    #[test]
    fn strikeout_shape_from_str() {
        assert_eq!(StrikeoutShape::from_str("None").unwrap(), StrikeoutShape::None);
        assert_eq!(StrikeoutShape::from_str("continuous").unwrap(), StrikeoutShape::Continuous);
        assert_eq!(StrikeoutShape::from_str("dash_dot").unwrap(), StrikeoutShape::DashDot);
        assert!(StrikeoutShape::from_str("invalid").is_err());
    }

    #[test]
    fn strikeout_shape_try_from_u8() {
        assert_eq!(StrikeoutShape::try_from(0u8).unwrap(), StrikeoutShape::None);
        assert_eq!(StrikeoutShape::try_from(1u8).unwrap(), StrikeoutShape::Continuous);
        assert_eq!(StrikeoutShape::try_from(5u8).unwrap(), StrikeoutShape::DashDotDot);
        assert!(StrikeoutShape::try_from(6u8).is_err());
    }

    #[test]
    fn strikeout_shape_str_roundtrip() {
        for v in &[
            StrikeoutShape::None,
            StrikeoutShape::Continuous,
            StrikeoutShape::Dash,
            StrikeoutShape::Dot,
            StrikeoutShape::DashDot,
            StrikeoutShape::DashDotDot,
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
        assert!(NumberFormatType::try_from(9u8).is_err());
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
    }

    #[test]
    fn word_break_type_from_str() {
        assert_eq!(WordBreakType::from_str("KEEP_WORD").unwrap(), WordBreakType::KeepWord);
        assert_eq!(WordBreakType::from_str("KeepWord").unwrap(), WordBreakType::KeepWord);
        assert_eq!(WordBreakType::from_str("keep_word").unwrap(), WordBreakType::KeepWord);
        assert_eq!(WordBreakType::from_str("BREAK_WORD").unwrap(), WordBreakType::BreakWord);
        assert_eq!(WordBreakType::from_str("BreakWord").unwrap(), WordBreakType::BreakWord);
        assert_eq!(WordBreakType::from_str("break_word").unwrap(), WordBreakType::BreakWord);
        assert!(WordBreakType::from_str("invalid").is_err());
    }

    #[test]
    fn word_break_type_try_from_u8() {
        assert_eq!(WordBreakType::try_from(0u8).unwrap(), WordBreakType::KeepWord);
        assert_eq!(WordBreakType::try_from(1u8).unwrap(), WordBreakType::BreakWord);
        assert!(WordBreakType::try_from(2u8).is_err());
    }

    #[test]
    fn word_break_type_serde_roundtrip() {
        for v in &[WordBreakType::KeepWord, WordBreakType::BreakWord] {
            let json = serde_json::to_string(v).unwrap();
            let back: WordBreakType = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, v);
        }
    }

    #[test]
    fn word_break_type_str_roundtrip() {
        for v in &[WordBreakType::KeepWord, WordBreakType::BreakWord] {
            let s = v.to_string();
            let back = WordBreakType::from_str(&s).unwrap();
            assert_eq!(&back, v);
        }
    }
}
