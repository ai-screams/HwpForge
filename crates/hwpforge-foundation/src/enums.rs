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
    /// Circled Hangul syllable: ㉮, ㉯, ㉰, ... (used for outline level 8).
    CircledHangulSyllable = 9,
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
            Self::CircledHangulSyllable => f.write_str("CircledHangulSyllable"),
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
            "CircledHangulSyllable" | "circledhangulsyllable" | "CIRCLED_HANGUL_SYLLABLE" => {
                Ok(Self::CircledHangulSyllable)
            }
            _ => Err(FoundationError::ParseError {
                type_name: "NumberFormatType".to_string(),
                value: s.to_string(),
                valid_values: "Digit, CircledDigit, RomanCapital, RomanSmall, LatinCapital, LatinSmall, HangulSyllable, HangulJamo, HanjaDigit, CircledHangulSyllable".to_string(),
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
            9 => Ok(Self::CircledHangulSyllable),
            _ => Err(FoundationError::ParseError {
                type_name: "NumberFormatType".to_string(),
                value: value.to_string(),
                valid_values: "0-9 (Digit, CircledDigit, RomanCapital, RomanSmall, LatinCapital, LatinSmall, HangulSyllable, HangulJamo, HanjaDigit, CircledHangulSyllable)".to_string(),
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

// ---------------------------------------------------------------------------
// EmphasisType
// ---------------------------------------------------------------------------

/// Character emphasis mark (symMark attribute in HWPX).
///
/// Controls the emphasis symbol displayed above or below characters.
/// Maps to HWPX `symMark` attribute values.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::EmphasisType;
///
/// assert_eq!(EmphasisType::default(), EmphasisType::None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum EmphasisType {
    /// No emphasis mark (default).
    #[default]
    None = 0,
    /// Dot above character.
    DotAbove = 1,
    /// Ring above character.
    RingAbove = 2,
    /// Tilde above character.
    Tilde = 3,
    /// Caron (hacek) above character.
    Caron = 4,
    /// Side dot.
    Side = 5,
    /// Colon mark.
    Colon = 6,
    /// Grave accent.
    GraveAccent = 7,
    /// Acute accent.
    AcuteAccent = 8,
    /// Circumflex accent.
    Circumflex = 9,
    /// Macron (overline).
    Macron = 10,
    /// Hook above.
    HookAbove = 11,
    /// Dot below character.
    DotBelow = 12,
}

impl fmt::Display for EmphasisType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::DotAbove => f.write_str("DotAbove"),
            Self::RingAbove => f.write_str("RingAbove"),
            Self::Tilde => f.write_str("Tilde"),
            Self::Caron => f.write_str("Caron"),
            Self::Side => f.write_str("Side"),
            Self::Colon => f.write_str("Colon"),
            Self::GraveAccent => f.write_str("GraveAccent"),
            Self::AcuteAccent => f.write_str("AcuteAccent"),
            Self::Circumflex => f.write_str("Circumflex"),
            Self::Macron => f.write_str("Macron"),
            Self::HookAbove => f.write_str("HookAbove"),
            Self::DotBelow => f.write_str("DotBelow"),
        }
    }
}

impl std::str::FromStr for EmphasisType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "NONE" | "None" | "none" => Ok(Self::None),
            "DOT_ABOVE" | "DotAbove" | "dot_above" => Ok(Self::DotAbove),
            "RING_ABOVE" | "RingAbove" | "ring_above" => Ok(Self::RingAbove),
            "TILDE" | "Tilde" | "tilde" => Ok(Self::Tilde),
            "CARON" | "Caron" | "caron" => Ok(Self::Caron),
            "SIDE" | "Side" | "side" => Ok(Self::Side),
            "COLON" | "Colon" | "colon" => Ok(Self::Colon),
            "GRAVE_ACCENT" | "GraveAccent" | "grave_accent" => Ok(Self::GraveAccent),
            "ACUTE_ACCENT" | "AcuteAccent" | "acute_accent" => Ok(Self::AcuteAccent),
            "CIRCUMFLEX" | "Circumflex" | "circumflex" => Ok(Self::Circumflex),
            "MACRON" | "Macron" | "macron" => Ok(Self::Macron),
            "HOOK_ABOVE" | "HookAbove" | "hook_above" => Ok(Self::HookAbove),
            "DOT_BELOW" | "DotBelow" | "dot_below" => Ok(Self::DotBelow),
            _ => Err(FoundationError::ParseError {
                type_name: "EmphasisType".to_string(),
                value: s.to_string(),
                valid_values:
                    "NONE, DOT_ABOVE, RING_ABOVE, TILDE, CARON, SIDE, COLON, GRAVE_ACCENT, ACUTE_ACCENT, CIRCUMFLEX, MACRON, HOOK_ABOVE, DOT_BELOW"
                        .to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for EmphasisType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::DotAbove),
            2 => Ok(Self::RingAbove),
            3 => Ok(Self::Tilde),
            4 => Ok(Self::Caron),
            5 => Ok(Self::Side),
            6 => Ok(Self::Colon),
            7 => Ok(Self::GraveAccent),
            8 => Ok(Self::AcuteAccent),
            9 => Ok(Self::Circumflex),
            10 => Ok(Self::Macron),
            11 => Ok(Self::HookAbove),
            12 => Ok(Self::DotBelow),
            _ => Err(FoundationError::ParseError {
                type_name: "EmphasisType".to_string(),
                value: value.to_string(),
                valid_values: "0-12 (None through DotBelow)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for EmphasisType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("EmphasisType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// HeadingType
// ---------------------------------------------------------------------------

/// Paragraph heading type for outline/numbering classification.
///
/// Controls how a paragraph participates in document outline or numbering.
/// Maps to the HWPX `<hh:heading type="...">` attribute.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::HeadingType;
///
/// assert_eq!(HeadingType::default(), HeadingType::None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum HeadingType {
    /// No heading (body text, default).
    #[default]
    None = 0,
    /// Outline heading (개요).
    Outline = 1,
    /// Number heading.
    Number = 2,
    /// Bullet heading.
    Bullet = 3,
}

impl HeadingType {
    /// Converts to the HWPX XML attribute string.
    pub fn to_hwpx_str(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::Outline => "OUTLINE",
            Self::Number => "NUMBER",
            Self::Bullet => "BULLET",
        }
    }

    /// Parses a HWPX XML attribute string.
    pub fn from_hwpx_str(s: &str) -> Self {
        match s {
            "NONE" => Self::None,
            "OUTLINE" => Self::Outline,
            "NUMBER" => Self::Number,
            "BULLET" => Self::Bullet,
            _ => Self::None,
        }
    }
}

impl fmt::Display for HeadingType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Outline => f.write_str("Outline"),
            Self::Number => f.write_str("Number"),
            Self::Bullet => f.write_str("Bullet"),
        }
    }
}

impl std::str::FromStr for HeadingType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" | "none" | "NONE" => Ok(Self::None),
            "Outline" | "outline" | "OUTLINE" => Ok(Self::Outline),
            "Number" | "number" | "NUMBER" => Ok(Self::Number),
            "Bullet" | "bullet" | "BULLET" => Ok(Self::Bullet),
            _ => Err(FoundationError::ParseError {
                type_name: "HeadingType".to_string(),
                value: s.to_string(),
                valid_values: "None, Outline, Number, Bullet".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for HeadingType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Outline),
            2 => Ok(Self::Number),
            3 => Ok(Self::Bullet),
            _ => Err(FoundationError::ParseError {
                type_name: "HeadingType".to_string(),
                value: value.to_string(),
                valid_values: "0 (None), 1 (Outline), 2 (Number), 3 (Bullet)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for HeadingType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("HeadingType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// GutterType
// ---------------------------------------------------------------------------

/// Gutter position type for page margins.
///
/// Controls where the binding gutter space is placed on the page.
/// Used in `<hp:pagePr gutterType="...">`.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::GutterType;
///
/// assert_eq!(GutterType::default(), GutterType::LeftOnly);
/// assert_eq!(GutterType::LeftOnly.to_string(), "LeftOnly");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum GutterType {
    /// Gutter on the left side only (default).
    #[default]
    LeftOnly = 0,
    /// Gutter on the left and right sides.
    LeftRight = 1,
    /// Gutter on the top side only.
    TopOnly = 2,
    /// Gutter on the top and bottom sides.
    TopBottom = 3,
}

impl fmt::Display for GutterType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LeftOnly => f.write_str("LeftOnly"),
            Self::LeftRight => f.write_str("LeftRight"),
            Self::TopOnly => f.write_str("TopOnly"),
            Self::TopBottom => f.write_str("TopBottom"),
        }
    }
}

impl std::str::FromStr for GutterType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "LeftOnly" | "LEFT_ONLY" | "left_only" => Ok(Self::LeftOnly),
            "LeftRight" | "LEFT_RIGHT" | "left_right" => Ok(Self::LeftRight),
            "TopOnly" | "TOP_ONLY" | "top_only" => Ok(Self::TopOnly),
            "TopBottom" | "TOP_BOTTOM" | "top_bottom" => Ok(Self::TopBottom),
            _ => Err(FoundationError::ParseError {
                type_name: "GutterType".to_string(),
                value: s.to_string(),
                valid_values: "LeftOnly, LeftRight, TopOnly, TopBottom".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for GutterType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::LeftOnly),
            1 => Ok(Self::LeftRight),
            2 => Ok(Self::TopOnly),
            3 => Ok(Self::TopBottom),
            _ => Err(FoundationError::ParseError {
                type_name: "GutterType".to_string(),
                value: value.to_string(),
                valid_values: "0 (LeftOnly), 1 (LeftRight), 2 (TopOnly), 3 (TopBottom)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for GutterType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("GutterType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// ShowMode
// ---------------------------------------------------------------------------

/// Visibility mode for page borders and fills.
///
/// Controls on which pages the border or fill is displayed.
/// Used in `<hp:visibility border="..." fill="...">`.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::ShowMode;
///
/// assert_eq!(ShowMode::default(), ShowMode::ShowAll);
/// assert_eq!(ShowMode::ShowAll.to_string(), "ShowAll");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum ShowMode {
    /// Show on all pages (default).
    #[default]
    ShowAll = 0,
    /// Hide on all pages.
    HideAll = 1,
    /// Show on odd pages only.
    ShowOdd = 2,
    /// Show on even pages only.
    ShowEven = 3,
}

impl fmt::Display for ShowMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShowAll => f.write_str("ShowAll"),
            Self::HideAll => f.write_str("HideAll"),
            Self::ShowOdd => f.write_str("ShowOdd"),
            Self::ShowEven => f.write_str("ShowEven"),
        }
    }
}

impl std::str::FromStr for ShowMode {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ShowAll" | "SHOW_ALL" | "show_all" => Ok(Self::ShowAll),
            "HideAll" | "HIDE_ALL" | "hide_all" => Ok(Self::HideAll),
            "ShowOdd" | "SHOW_ODD" | "show_odd" => Ok(Self::ShowOdd),
            "ShowEven" | "SHOW_EVEN" | "show_even" => Ok(Self::ShowEven),
            _ => Err(FoundationError::ParseError {
                type_name: "ShowMode".to_string(),
                value: s.to_string(),
                valid_values: "ShowAll, HideAll, ShowOdd, ShowEven".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for ShowMode {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::ShowAll),
            1 => Ok(Self::HideAll),
            2 => Ok(Self::ShowOdd),
            3 => Ok(Self::ShowEven),
            _ => Err(FoundationError::ParseError {
                type_name: "ShowMode".to_string(),
                value: value.to_string(),
                valid_values: "0 (ShowAll), 1 (HideAll), 2 (ShowOdd), 3 (ShowEven)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for ShowMode {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ShowMode")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// RestartType
// ---------------------------------------------------------------------------

/// Line number restart type.
///
/// Controls when line numbering restarts to 1.
/// Used in `<hp:lineNumberShape restartType="...">`.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::RestartType;
///
/// assert_eq!(RestartType::default(), RestartType::Continuous);
/// assert_eq!(RestartType::Continuous.to_string(), "Continuous");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum RestartType {
    /// Continuous numbering throughout the document (default).
    #[default]
    Continuous = 0,
    /// Restart numbering at each section.
    Section = 1,
    /// Restart numbering at each page.
    Page = 2,
}

impl fmt::Display for RestartType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Continuous => f.write_str("Continuous"),
            Self::Section => f.write_str("Section"),
            Self::Page => f.write_str("Page"),
        }
    }
}

impl std::str::FromStr for RestartType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Continuous" | "continuous" | "0" => Ok(Self::Continuous),
            "Section" | "section" | "1" => Ok(Self::Section),
            "Page" | "page" | "2" => Ok(Self::Page),
            _ => Err(FoundationError::ParseError {
                type_name: "RestartType".to_string(),
                value: s.to_string(),
                valid_values: "Continuous, Section, Page".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for RestartType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Continuous),
            1 => Ok(Self::Section),
            2 => Ok(Self::Page),
            _ => Err(FoundationError::ParseError {
                type_name: "RestartType".to_string(),
                value: value.to_string(),
                valid_values: "0 (Continuous), 1 (Section), 2 (Page)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for RestartType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("RestartType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// TextBorderType
// ---------------------------------------------------------------------------

/// Reference frame for page border offset measurement.
///
/// Controls whether page border offsets are measured from the paper edge
/// or from the content area.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::TextBorderType;
///
/// assert_eq!(TextBorderType::default(), TextBorderType::Paper);
/// assert_eq!(TextBorderType::Paper.to_string(), "Paper");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum TextBorderType {
    /// Offsets measured from paper edge (default).
    #[default]
    Paper = 0,
    /// Offsets measured from content area.
    Content = 1,
}

impl fmt::Display for TextBorderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Paper => f.write_str("Paper"),
            Self::Content => f.write_str("Content"),
        }
    }
}

impl std::str::FromStr for TextBorderType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Paper" | "PAPER" | "paper" => Ok(Self::Paper),
            "Content" | "CONTENT" | "content" => Ok(Self::Content),
            _ => Err(FoundationError::ParseError {
                type_name: "TextBorderType".to_string(),
                value: s.to_string(),
                valid_values: "Paper, Content".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for TextBorderType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Paper),
            1 => Ok(Self::Content),
            _ => Err(FoundationError::ParseError {
                type_name: "TextBorderType".to_string(),
                value: value.to_string(),
                valid_values: "0 (Paper), 1 (Content)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for TextBorderType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("TextBorderType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// Flip
// ---------------------------------------------------------------------------

/// Flip/mirror state for drawing shapes.
///
/// Controls horizontal and/or vertical mirroring of a shape.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::Flip;
///
/// assert_eq!(Flip::default(), Flip::None);
/// assert_eq!(Flip::Horizontal.to_string(), "Horizontal");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum Flip {
    /// No flip (default).
    #[default]
    None = 0,
    /// Mirrored horizontally.
    Horizontal = 1,
    /// Mirrored vertically.
    Vertical = 2,
    /// Mirrored both horizontally and vertically.
    Both = 3,
}

impl fmt::Display for Flip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Horizontal => f.write_str("Horizontal"),
            Self::Vertical => f.write_str("Vertical"),
            Self::Both => f.write_str("Both"),
        }
    }
}

impl std::str::FromStr for Flip {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" | "NONE" | "none" => Ok(Self::None),
            "Horizontal" | "HORIZONTAL" | "horizontal" => Ok(Self::Horizontal),
            "Vertical" | "VERTICAL" | "vertical" => Ok(Self::Vertical),
            "Both" | "BOTH" | "both" => Ok(Self::Both),
            _ => Err(FoundationError::ParseError {
                type_name: "Flip".to_string(),
                value: s.to_string(),
                valid_values: "None, Horizontal, Vertical, Both".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for Flip {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Horizontal),
            2 => Ok(Self::Vertical),
            3 => Ok(Self::Both),
            _ => Err(FoundationError::ParseError {
                type_name: "Flip".to_string(),
                value: value.to_string(),
                valid_values: "0 (None), 1 (Horizontal), 2 (Vertical), 3 (Both)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for Flip {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("Flip")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// ArcType
// ---------------------------------------------------------------------------

/// Arc drawing type for ellipse-based arc shapes.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::ArcType;
///
/// assert_eq!(ArcType::default(), ArcType::Normal);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum ArcType {
    /// Open arc (just the curved edge).
    #[default]
    Normal = 0,
    /// Pie/sector (arc + two radii closing to center).
    Pie = 1,
    /// Chord (arc + straight line closing endpoints).
    Chord = 2,
}

impl fmt::Display for ArcType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Normal => f.write_str("NORMAL"),
            Self::Pie => f.write_str("PIE"),
            Self::Chord => f.write_str("CHORD"),
        }
    }
}

impl std::str::FromStr for ArcType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "NORMAL" | "Normal" | "normal" => Ok(Self::Normal),
            "PIE" | "Pie" | "pie" => Ok(Self::Pie),
            "CHORD" | "Chord" | "chord" => Ok(Self::Chord),
            _ => Err(FoundationError::ParseError {
                type_name: "ArcType".to_string(),
                value: s.to_string(),
                valid_values: "NORMAL, PIE, CHORD".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for ArcType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Normal),
            1 => Ok(Self::Pie),
            2 => Ok(Self::Chord),
            _ => Err(FoundationError::ParseError {
                type_name: "ArcType".to_string(),
                value: value.to_string(),
                valid_values: "0 (Normal), 1 (Pie), 2 (Chord)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for ArcType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ArcType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// ArrowType
// ---------------------------------------------------------------------------

/// Arrowhead shape for line endpoints.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::ArrowType;
///
/// assert_eq!(ArrowType::default(), ArrowType::None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum ArrowType {
    /// No arrowhead (default).
    #[default]
    None = 0,
    /// Standard filled arrowhead.
    Normal = 1,
    /// Arrow-shaped arrowhead.
    Arrow = 2,
    /// Concave arrowhead.
    Concave = 3,
    /// Diamond arrowhead.
    Diamond = 4,
    /// Oval/circle arrowhead.
    Oval = 5,
    /// Open (unfilled) arrowhead.
    Open = 6,
}

impl fmt::Display for ArrowType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // KS X 6101 ArrowType values.
        // Diamond/Oval/Open default to FILLED_ variants here;
        // the encoder resolves FILLED_ vs EMPTY_ based on ArrowStyle.filled.
        match self {
            Self::None => f.write_str("NORMAL"),
            Self::Normal => f.write_str("ARROW"),
            Self::Arrow => f.write_str("SPEAR"),
            Self::Concave => f.write_str("CONCAVE_ARROW"),
            Self::Diamond => f.write_str("FILLED_DIAMOND"),
            Self::Oval => f.write_str("FILLED_CIRCLE"),
            Self::Open => f.write_str("EMPTY_BOX"),
        }
    }
}

impl std::str::FromStr for ArrowType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // KS X 6101 ArrowType values (primary) + legacy aliases for backward compat.
        match s {
            "NORMAL" => Ok(Self::None),
            "ARROW" => Ok(Self::Normal),
            "SPEAR" => Ok(Self::Arrow),
            "CONCAVE_ARROW" => Ok(Self::Concave),
            "FILLED_DIAMOND" | "EMPTY_DIAMOND" => Ok(Self::Diamond),
            "FILLED_CIRCLE" | "EMPTY_CIRCLE" => Ok(Self::Oval),
            "FILLED_BOX" | "EMPTY_BOX" => Ok(Self::Open),
            _ => Err(FoundationError::ParseError {
                type_name: "ArrowType".to_string(),
                value: s.to_string(),
                valid_values: "NORMAL, ARROW, SPEAR, CONCAVE_ARROW, FILLED_DIAMOND, EMPTY_DIAMOND, FILLED_CIRCLE, EMPTY_CIRCLE, FILLED_BOX, EMPTY_BOX"
                    .to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for ArrowType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Normal),
            2 => Ok(Self::Arrow),
            3 => Ok(Self::Concave),
            4 => Ok(Self::Diamond),
            5 => Ok(Self::Oval),
            6 => Ok(Self::Open),
            _ => Err(FoundationError::ParseError {
                type_name: "ArrowType".to_string(),
                value: value.to_string(),
                valid_values:
                    "0 (None), 1 (Normal), 2 (Arrow), 3 (Concave), 4 (Diamond), 5 (Oval), 6 (Open)"
                        .to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for ArrowType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ArrowType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// ArrowSize
// ---------------------------------------------------------------------------

/// Arrowhead size for line endpoints.
///
/// Encoded as `{HEAD}_{TAIL}` string in HWPX (e.g. `"MEDIUM_MEDIUM"`).
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::ArrowSize;
///
/// assert_eq!(ArrowSize::default(), ArrowSize::Medium);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum ArrowSize {
    /// Small arrowhead.
    Small = 0,
    /// Medium arrowhead (default).
    #[default]
    Medium = 1,
    /// Large arrowhead.
    Large = 2,
}

impl fmt::Display for ArrowSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Small => f.write_str("SMALL_SMALL"),
            Self::Medium => f.write_str("MEDIUM_MEDIUM"),
            Self::Large => f.write_str("LARGE_LARGE"),
        }
    }
}

impl std::str::FromStr for ArrowSize {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "SMALL_SMALL" | "Small" | "small" => Ok(Self::Small),
            "MEDIUM_MEDIUM" | "Medium" | "medium" => Ok(Self::Medium),
            "LARGE_LARGE" | "Large" | "large" => Ok(Self::Large),
            _ => Err(FoundationError::ParseError {
                type_name: "ArrowSize".to_string(),
                value: s.to_string(),
                valid_values: "SMALL_SMALL, MEDIUM_MEDIUM, LARGE_LARGE".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for ArrowSize {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Small),
            1 => Ok(Self::Medium),
            2 => Ok(Self::Large),
            _ => Err(FoundationError::ParseError {
                type_name: "ArrowSize".to_string(),
                value: value.to_string(),
                valid_values: "0 (Small), 1 (Medium), 2 (Large)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for ArrowSize {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ArrowSize")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// GradientType
// ---------------------------------------------------------------------------

/// Gradient fill direction type.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::GradientType;
///
/// assert_eq!(GradientType::default(), GradientType::Linear);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum GradientType {
    /// Linear gradient (default).
    #[default]
    Linear = 0,
    /// Radial gradient (from center outward).
    Radial = 1,
    /// Square/rectangular gradient.
    Square = 2,
    /// Conical gradient (angular sweep).
    Conical = 3,
}

impl fmt::Display for GradientType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Linear => f.write_str("LINEAR"),
            Self::Radial => f.write_str("RADIAL"),
            Self::Square => f.write_str("SQUARE"),
            Self::Conical => f.write_str("CONICAL"),
        }
    }
}

impl std::str::FromStr for GradientType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "LINEAR" | "Linear" | "linear" => Ok(Self::Linear),
            "RADIAL" | "Radial" | "radial" => Ok(Self::Radial),
            "SQUARE" | "Square" | "square" => Ok(Self::Square),
            "CONICAL" | "Conical" | "conical" => Ok(Self::Conical),
            _ => Err(FoundationError::ParseError {
                type_name: "GradientType".to_string(),
                value: s.to_string(),
                valid_values: "LINEAR, RADIAL, SQUARE, CONICAL".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for GradientType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Linear),
            1 => Ok(Self::Radial),
            2 => Ok(Self::Square),
            3 => Ok(Self::Conical),
            _ => Err(FoundationError::ParseError {
                type_name: "GradientType".to_string(),
                value: value.to_string(),
                valid_values: "0 (Linear), 1 (Radial), 2 (Square), 3 (Conical)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for GradientType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("GradientType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// PatternType
// ---------------------------------------------------------------------------

/// Hatch/pattern fill type for shapes.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::PatternType;
///
/// assert_eq!(PatternType::default(), PatternType::Horizontal);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum PatternType {
    /// Horizontal lines (default).
    #[default]
    Horizontal = 0,
    /// Vertical lines.
    Vertical = 1,
    /// Backslash diagonal lines.
    BackSlash = 2,
    /// Forward slash diagonal lines.
    Slash = 3,
    /// Cross-hatch (horizontal + vertical).
    Cross = 4,
    /// Cross-diagonal hatch.
    CrossDiagonal = 5,
}

impl fmt::Display for PatternType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Horizontal => f.write_str("HORIZONTAL"),
            Self::Vertical => f.write_str("VERTICAL"),
            Self::BackSlash => f.write_str("BACK_SLASH"),
            Self::Slash => f.write_str("SLASH"),
            Self::Cross => f.write_str("CROSS"),
            Self::CrossDiagonal => f.write_str("CROSS_DIAGONAL"),
        }
    }
}

impl std::str::FromStr for PatternType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "HORIZONTAL" | "Horizontal" | "horizontal" => Ok(Self::Horizontal),
            "VERTICAL" | "Vertical" | "vertical" => Ok(Self::Vertical),
            "BACK_SLASH" | "BackSlash" | "backslash" => Ok(Self::BackSlash),
            "SLASH" | "Slash" | "slash" => Ok(Self::Slash),
            "CROSS" | "Cross" | "cross" => Ok(Self::Cross),
            "CROSS_DIAGONAL" | "CrossDiagonal" | "crossdiagonal" => Ok(Self::CrossDiagonal),
            _ => Err(FoundationError::ParseError {
                type_name: "PatternType".to_string(),
                value: s.to_string(),
                valid_values: "HORIZONTAL, VERTICAL, BACK_SLASH, SLASH, CROSS, CROSS_DIAGONAL"
                    .to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for PatternType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Horizontal),
            1 => Ok(Self::Vertical),
            2 => Ok(Self::BackSlash),
            3 => Ok(Self::Slash),
            4 => Ok(Self::Cross),
            5 => Ok(Self::CrossDiagonal),
            _ => Err(FoundationError::ParseError {
                type_name: "PatternType".to_string(),
                value: value.to_string(),
                valid_values:
                    "0 (Horizontal), 1 (Vertical), 2 (BackSlash), 3 (Slash), 4 (Cross), 5 (CrossDiagonal)"
                        .to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for PatternType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("PatternType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// ImageFillMode
// ---------------------------------------------------------------------------

/// How an image is fitted within a shape fill area.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::ImageFillMode;
///
/// assert_eq!(ImageFillMode::default(), ImageFillMode::Tile);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum ImageFillMode {
    /// Tile the image to fill the area (default).
    #[default]
    Tile = 0,
    /// Center the image without scaling.
    Center = 1,
    /// Stretch the image to fit exactly.
    Stretch = 2,
    /// Scale proportionally to fit all within the area.
    FitAll = 3,
}

impl fmt::Display for ImageFillMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tile => f.write_str("TILE"),
            Self::Center => f.write_str("CENTER"),
            Self::Stretch => f.write_str("STRETCH"),
            Self::FitAll => f.write_str("FIT_ALL"),
        }
    }
}

impl std::str::FromStr for ImageFillMode {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "TILE" | "Tile" | "tile" => Ok(Self::Tile),
            "CENTER" | "Center" | "center" => Ok(Self::Center),
            "STRETCH" | "Stretch" | "stretch" => Ok(Self::Stretch),
            "FIT_ALL" | "FitAll" | "fit_all" => Ok(Self::FitAll),
            _ => Err(FoundationError::ParseError {
                type_name: "ImageFillMode".to_string(),
                value: s.to_string(),
                valid_values: "TILE, CENTER, STRETCH, FIT_ALL".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for ImageFillMode {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Tile),
            1 => Ok(Self::Center),
            2 => Ok(Self::Stretch),
            3 => Ok(Self::FitAll),
            _ => Err(FoundationError::ParseError {
                type_name: "ImageFillMode".to_string(),
                value: value.to_string(),
                valid_values: "0 (Tile), 1 (Center), 2 (Stretch), 3 (FitAll)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for ImageFillMode {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ImageFillMode")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// CurveSegmentType
// ---------------------------------------------------------------------------

/// Segment type within a curve path.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::CurveSegmentType;
///
/// assert_eq!(CurveSegmentType::default(), CurveSegmentType::Line);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum CurveSegmentType {
    /// Straight line segment (default).
    #[default]
    Line = 0,
    /// Cubic bezier curve segment.
    Curve = 1,
}

impl fmt::Display for CurveSegmentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Line => f.write_str("LINE"),
            Self::Curve => f.write_str("CURVE"),
        }
    }
}

impl std::str::FromStr for CurveSegmentType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "LINE" | "Line" | "line" => Ok(Self::Line),
            "CURVE" | "Curve" | "curve" => Ok(Self::Curve),
            _ => Err(FoundationError::ParseError {
                type_name: "CurveSegmentType".to_string(),
                value: s.to_string(),
                valid_values: "LINE, CURVE".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for CurveSegmentType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Line),
            1 => Ok(Self::Curve),
            _ => Err(FoundationError::ParseError {
                type_name: "CurveSegmentType".to_string(),
                value: value.to_string(),
                valid_values: "0 (Line), 1 (Curve)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for CurveSegmentType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("CurveSegmentType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// BookmarkType
// ---------------------------------------------------------------------------

/// Type of bookmark in an HWPX document.
///
/// Bookmarks can mark a single point or span a range of content
/// (start/end pair using `fieldBegin`/`fieldEnd`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum BookmarkType {
    /// A point bookmark at a single location (direct serde in `<hp:ctrl>`).
    #[default]
    Point = 0,
    /// Start of a span bookmark (`fieldBegin type="BOOKMARK"`).
    SpanStart = 1,
    /// End of a span bookmark (`fieldEnd beginIDRef`).
    SpanEnd = 2,
}

impl fmt::Display for BookmarkType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Point => f.write_str("Point"),
            Self::SpanStart => f.write_str("SpanStart"),
            Self::SpanEnd => f.write_str("SpanEnd"),
        }
    }
}

impl std::str::FromStr for BookmarkType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Point" | "point" => Ok(Self::Point),
            "SpanStart" | "span_start" => Ok(Self::SpanStart),
            "SpanEnd" | "span_end" => Ok(Self::SpanEnd),
            _ => Err(FoundationError::ParseError {
                type_name: "BookmarkType".to_string(),
                value: s.to_string(),
                valid_values: "Point, SpanStart, SpanEnd".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for BookmarkType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Point),
            1 => Ok(Self::SpanStart),
            2 => Ok(Self::SpanEnd),
            _ => Err(FoundationError::ParseError {
                type_name: "BookmarkType".to_string(),
                value: value.to_string(),
                valid_values: "0 (Point), 1 (SpanStart), 2 (SpanEnd)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for BookmarkType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("BookmarkType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// FieldType
// ---------------------------------------------------------------------------

/// Type of a press-field (누름틀) in an HWPX document.
///
/// Press-fields are interactive form fields that users can click to fill in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum FieldType {
    /// Click-here placeholder field (default).
    #[default]
    ClickHere = 0,
    /// Automatic date field.
    Date = 1,
    /// Automatic time field.
    Time = 2,
    /// Page number field.
    PageNum = 3,
    /// Document summary field.
    DocSummary = 4,
    /// User information field.
    UserInfo = 5,
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClickHere => f.write_str("CLICK_HERE"),
            Self::Date => f.write_str("DATE"),
            Self::Time => f.write_str("TIME"),
            Self::PageNum => f.write_str("PAGE_NUM"),
            Self::DocSummary => f.write_str("DOC_SUMMARY"),
            Self::UserInfo => f.write_str("USER_INFO"),
        }
    }
}

impl std::str::FromStr for FieldType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "CLICK_HERE" | "ClickHere" | "click_here" => Ok(Self::ClickHere),
            "DATE" | "Date" | "date" => Ok(Self::Date),
            "TIME" | "Time" | "time" => Ok(Self::Time),
            "PAGE_NUM" | "PageNum" | "page_num" => Ok(Self::PageNum),
            "DOC_SUMMARY" | "DocSummary" | "doc_summary" => Ok(Self::DocSummary),
            "USER_INFO" | "UserInfo" | "user_info" => Ok(Self::UserInfo),
            _ => Err(FoundationError::ParseError {
                type_name: "FieldType".to_string(),
                value: s.to_string(),
                valid_values: "CLICK_HERE, DATE, TIME, PAGE_NUM, DOC_SUMMARY, USER_INFO"
                    .to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for FieldType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::ClickHere),
            1 => Ok(Self::Date),
            2 => Ok(Self::Time),
            3 => Ok(Self::PageNum),
            4 => Ok(Self::DocSummary),
            5 => Ok(Self::UserInfo),
            _ => Err(FoundationError::ParseError {
                type_name: "FieldType".to_string(),
                value: value.to_string(),
                valid_values: "0..5 (ClickHere..UserInfo)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for FieldType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("FieldType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// RefType
// ---------------------------------------------------------------------------

/// Target type of a cross-reference (상호참조).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum RefType {
    /// Reference to a bookmark target.
    #[default]
    Bookmark = 0,
    /// Reference to a table caption number.
    Table = 1,
    /// Reference to a figure/image caption number.
    Figure = 2,
    /// Reference to an equation number.
    Equation = 3,
}

impl fmt::Display for RefType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bookmark => f.write_str("TARGET_BOOKMARK"),
            Self::Table => f.write_str("TARGET_TABLE"),
            Self::Figure => f.write_str("TARGET_FIGURE"),
            Self::Equation => f.write_str("TARGET_EQUATION"),
        }
    }
}

impl std::str::FromStr for RefType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "TARGET_BOOKMARK" | "Bookmark" | "bookmark" => Ok(Self::Bookmark),
            "TARGET_TABLE" | "Table" | "table" => Ok(Self::Table),
            "TARGET_FIGURE" | "Figure" | "figure" => Ok(Self::Figure),
            "TARGET_EQUATION" | "Equation" | "equation" => Ok(Self::Equation),
            _ => Err(FoundationError::ParseError {
                type_name: "RefType".to_string(),
                value: s.to_string(),
                valid_values: "TARGET_BOOKMARK, TARGET_TABLE, TARGET_FIGURE, TARGET_EQUATION"
                    .to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for RefType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Bookmark),
            1 => Ok(Self::Table),
            2 => Ok(Self::Figure),
            3 => Ok(Self::Equation),
            _ => Err(FoundationError::ParseError {
                type_name: "RefType".to_string(),
                value: value.to_string(),
                valid_values: "0..3 (Bookmark..Equation)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for RefType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("RefType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// RefContentType
// ---------------------------------------------------------------------------

/// Content display type for a cross-reference (what to show at the reference site).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum RefContentType {
    /// Show page number where the target appears.
    #[default]
    Page = 0,
    /// Show the target's numbering (e.g. "표 3", "그림 2").
    Number = 1,
    /// Show the target's content text.
    Contents = 2,
    /// Show relative position ("위" / "아래").
    UpDownPos = 3,
}

impl fmt::Display for RefContentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Page => f.write_str("OBJECT_TYPE_PAGE"),
            Self::Number => f.write_str("OBJECT_TYPE_NUMBER"),
            Self::Contents => f.write_str("OBJECT_TYPE_CONTENTS"),
            Self::UpDownPos => f.write_str("OBJECT_TYPE_UPDOWNPOS"),
        }
    }
}

impl std::str::FromStr for RefContentType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "OBJECT_TYPE_PAGE" | "Page" | "page" => Ok(Self::Page),
            "OBJECT_TYPE_NUMBER" | "Number" | "number" => Ok(Self::Number),
            "OBJECT_TYPE_CONTENTS" | "Contents" | "contents" => Ok(Self::Contents),
            "OBJECT_TYPE_UPDOWNPOS" | "UpDownPos" | "updownpos" => Ok(Self::UpDownPos),
            _ => Err(FoundationError::ParseError {
                type_name: "RefContentType".to_string(),
                value: s.to_string(),
                valid_values:
                    "OBJECT_TYPE_PAGE, OBJECT_TYPE_NUMBER, OBJECT_TYPE_CONTENTS, OBJECT_TYPE_UPDOWNPOS"
                        .to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for RefContentType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Page),
            1 => Ok(Self::Number),
            2 => Ok(Self::Contents),
            3 => Ok(Self::UpDownPos),
            _ => Err(FoundationError::ParseError {
                type_name: "RefContentType".to_string(),
                value: value.to_string(),
                valid_values: "0..3 (Page..UpDownPos)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for RefContentType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("RefContentType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

/// Drop cap style for floating shape objects (HWPX `dropcapstyle` attribute).
///
/// Controls whether a shape (text box, image, table, etc.) is formatted as a
/// drop capital that occupies multiple lines at the start of a paragraph.
///
/// # HWPX Values
///
/// | Variant      | HWPX string     |
/// |--------------|-----------------|
/// | `None`       | `"None"`        |
/// | `DoubleLine` | `"DoubleLine"`  |
/// | `TripleLine` | `"TripleLine"`  |
/// | `Margin`     | `"Margin"`      |
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DropCapStyle {
    /// No drop cap (default).
    #[default]
    None = 0,
    /// Drop cap spanning 2 lines.
    DoubleLine = 1,
    /// Drop cap spanning 3 lines.
    TripleLine = 2,
    /// Drop cap positioned in the margin.
    Margin = 3,
}

impl fmt::Display for DropCapStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::DoubleLine => f.write_str("DoubleLine"),
            Self::TripleLine => f.write_str("TripleLine"),
            Self::Margin => f.write_str("Margin"),
        }
    }
}

impl std::str::FromStr for DropCapStyle {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" | "NONE" | "none" => Ok(Self::None),
            "DoubleLine" | "DOUBLE_LINE" => Ok(Self::DoubleLine),
            "TripleLine" | "TRIPLE_LINE" => Ok(Self::TripleLine),
            "Margin" | "MARGIN" => Ok(Self::Margin),
            _ => Err(FoundationError::ParseError {
                type_name: "DropCapStyle".to_string(),
                value: s.to_string(),
                valid_values: "None, DoubleLine, TripleLine, Margin".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for DropCapStyle {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("DropCapStyle")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

impl serde::Serialize for DropCapStyle {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for DropCapStyle {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

// Compile-time size assertions: all enums are 1 byte
const _: () = assert!(std::mem::size_of::<DropCapStyle>() == 1);
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
const _: () = assert!(std::mem::size_of::<BookmarkType>() == 1);
const _: () = assert!(std::mem::size_of::<FieldType>() == 1);
const _: () = assert!(std::mem::size_of::<RefType>() == 1);
const _: () = assert!(std::mem::size_of::<RefContentType>() == 1);

// ---------------------------------------------------------------------------
// TextDirection
// ---------------------------------------------------------------------------

/// Text writing direction for sections and sub-lists.
///
/// Controls whether text flows horizontally (가로쓰기) or vertically (세로쓰기).
/// Used in `<hp:secPr textDirection="...">` and `<hp:subList textDirection="...">`.
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::TextDirection;
///
/// assert_eq!(TextDirection::default(), TextDirection::Horizontal);
/// assert_eq!(TextDirection::Horizontal.to_string(), "HORIZONTAL");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TextDirection {
    /// Horizontal writing (가로쓰기) — default.
    #[default]
    Horizontal,
    /// Vertical writing with Latin chars rotated 90° (세로쓰기 영문 눕힘).
    Vertical,
    /// Vertical writing with Latin chars upright (세로쓰기 영문 세움).
    VerticalAll,
}

impl TextDirection {
    /// Parses a HWPX XML attribute string (e.g. `"VERTICAL"`).
    ///
    /// Unknown values fall back to [`TextDirection::Horizontal`].
    pub fn from_hwpx_str(s: &str) -> Self {
        match s {
            "VERTICAL" => Self::Vertical,
            "VERTICALALL" => Self::VerticalAll,
            _ => Self::Horizontal,
        }
    }
}

impl fmt::Display for TextDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Horizontal => f.write_str("HORIZONTAL"),
            Self::Vertical => f.write_str("VERTICAL"),
            Self::VerticalAll => f.write_str("VERTICALALL"),
        }
    }
}

impl schemars::JsonSchema for TextDirection {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("TextDirection")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

const _: () = assert!(std::mem::size_of::<TextDirection>() == 1);

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
        assert_eq!(
            NumberFormatType::try_from(9u8).unwrap(),
            NumberFormatType::CircledHangulSyllable
        );
        assert!(NumberFormatType::try_from(10u8).is_err());
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
