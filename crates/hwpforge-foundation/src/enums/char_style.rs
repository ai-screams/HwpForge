//! Character-style enums: language, underline, strikeout, outline, shadow, and emphasis.

use crate::error::FoundationError;
use serde::{Deserialize, Serialize};
use std::fmt;

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
// UnderlineShape
// ---------------------------------------------------------------------------

/// Underline line family (e.g. SOLID, DASH, WAVE).
///
/// This selects the line *style* used by an underline; the position
/// (Bottom/Center/Top) is carried separately by [`UnderlineType`].
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::UnderlineShape;
///
/// assert_eq!(UnderlineShape::default(), UnderlineShape::Solid);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[non_exhaustive]
#[repr(u8)]
pub enum UnderlineShape {
    /// Solid continuous line (default).
    #[default]
    Solid = 0,
    /// Dashed line.
    Dash = 1,
    /// Dotted line.
    Dot = 2,
    /// Dash-dot pattern.
    DashDot = 3,
    /// Dash-dot-dot pattern.
    DashDotDot = 4,
    /// Long dash pattern.
    LongDash = 5,
    /// Repeating small circles.
    Circle = 6,
    /// Double thin line.
    DoubleSlim = 7,
    /// Thin then thick double line.
    SlimThick = 8,
    /// Thick then thin double line.
    ThickSlim = 9,
    /// Thick-thin-thick triple line.
    ThickSlimThick = 10,
    /// Wavy line.
    Wave = 11,
}

impl fmt::Display for UnderlineShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Solid => f.write_str("SOLID"),
            Self::Dash => f.write_str("DASH"),
            Self::Dot => f.write_str("DOT"),
            Self::DashDot => f.write_str("DASH_DOT"),
            Self::DashDotDot => f.write_str("DASH_DOT_DOT"),
            Self::LongDash => f.write_str("LONG_DASH"),
            Self::Circle => f.write_str("CIRCLE"),
            Self::DoubleSlim => f.write_str("DOUBLE_SLIM"),
            Self::SlimThick => f.write_str("SLIM_THICK"),
            Self::ThickSlim => f.write_str("THICK_SLIM"),
            Self::ThickSlimThick => f.write_str("THICK_SLIM_THICK"),
            Self::Wave => f.write_str("WAVE"),
        }
    }
}

impl std::str::FromStr for UnderlineShape {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "SOLID" | "Solid" | "solid" => Ok(Self::Solid),
            "DASH" | "Dash" | "dash" => Ok(Self::Dash),
            "DOT" | "Dot" | "dot" => Ok(Self::Dot),
            "DASH_DOT" | "DashDot" | "dash_dot" => Ok(Self::DashDot),
            "DASH_DOT_DOT" | "DashDotDot" | "dash_dot_dot" => Ok(Self::DashDotDot),
            "LONG_DASH" | "LongDash" | "long_dash" => Ok(Self::LongDash),
            "CIRCLE" | "Circle" | "circle" => Ok(Self::Circle),
            "DOUBLE_SLIM" | "DoubleSlim" | "double_slim" => Ok(Self::DoubleSlim),
            "SLIM_THICK" | "SlimThick" | "slim_thick" => Ok(Self::SlimThick),
            "THICK_SLIM" | "ThickSlim" | "thick_slim" => Ok(Self::ThickSlim),
            "THICK_SLIM_THICK" | "ThickSlimThick" | "thick_slim_thick" => {
                Ok(Self::ThickSlimThick)
            }
            "WAVE" | "Wave" | "wave" => Ok(Self::Wave),
            _ => Err(FoundationError::ParseError {
                type_name: "UnderlineShape".to_string(),
                value: s.to_string(),
                valid_values: "SOLID, DASH, DOT, DASH_DOT, DASH_DOT_DOT, LONG_DASH, CIRCLE, DOUBLE_SLIM, SLIM_THICK, THICK_SLIM, THICK_SLIM_THICK, WAVE".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for UnderlineShape {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Solid),
            1 => Ok(Self::Dash),
            2 => Ok(Self::Dot),
            3 => Ok(Self::DashDot),
            4 => Ok(Self::DashDotDot),
            5 => Ok(Self::LongDash),
            6 => Ok(Self::Circle),
            7 => Ok(Self::DoubleSlim),
            8 => Ok(Self::SlimThick),
            9 => Ok(Self::ThickSlim),
            10 => Ok(Self::ThickSlimThick),
            11 => Ok(Self::Wave),
            _ => Err(FoundationError::ParseError {
                type_name: "UnderlineShape".to_string(),
                value: value.to_string(),
                valid_values: "0-11 (SOLID, DASH, DOT, DASH_DOT, DASH_DOT_DOT, LONG_DASH, CIRCLE, DOUBLE_SLIM, SLIM_THICK, THICK_SLIM, THICK_SLIM_THICK, WAVE)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for UnderlineShape {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("UnderlineShape")
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
/// This selects the line *family* used by a strikeout. After Wave 1c the
/// shared IR mirrors the full OWPML strike-shape vocabulary so the HWP5
/// projection can carry the entire line family rather than collapsing to
/// `Solid`. The naming aligns with [`UnderlineShape`] so both axes share
/// vocabulary.
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
    /// Solid continuous line (formerly named `Continuous`).
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
    /// Repeating small circles.
    Circle = 7,
    /// Double thin line.
    DoubleSlim = 8,
    /// Thin then thick double line.
    SlimThick = 9,
    /// Thick then thin double line.
    ThickSlim = 10,
    /// Thick-thin-thick triple line.
    ThickSlimThick = 11,
    /// Wavy line.
    Wave = 12,
}

impl fmt::Display for StrikeoutShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("NONE"),
            Self::Solid => f.write_str("SOLID"),
            Self::Dash => f.write_str("DASH"),
            Self::Dot => f.write_str("DOT"),
            Self::DashDot => f.write_str("DASH_DOT"),
            Self::DashDotDot => f.write_str("DASH_DOT_DOT"),
            Self::LongDash => f.write_str("LONG_DASH"),
            Self::Circle => f.write_str("CIRCLE"),
            Self::DoubleSlim => f.write_str("DOUBLE_SLIM"),
            Self::SlimThick => f.write_str("SLIM_THICK"),
            Self::ThickSlim => f.write_str("THICK_SLIM"),
            Self::ThickSlimThick => f.write_str("THICK_SLIM_THICK"),
            Self::Wave => f.write_str("WAVE"),
        }
    }
}

impl std::str::FromStr for StrikeoutShape {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "NONE" | "None" | "none" => Ok(Self::None),
            "SOLID" | "Solid" | "solid" | "Continuous" | "continuous" => Ok(Self::Solid),
            "DASH" | "Dash" | "dash" => Ok(Self::Dash),
            "DOT" | "Dot" | "dot" => Ok(Self::Dot),
            "DASH_DOT" | "DashDot" | "dashdot" | "dash_dot" => Ok(Self::DashDot),
            "DASH_DOT_DOT" | "DashDotDot" | "dashdotdot" | "dash_dot_dot" => Ok(Self::DashDotDot),
            "LONG_DASH" | "LongDash" | "long_dash" => Ok(Self::LongDash),
            "CIRCLE" | "Circle" | "circle" => Ok(Self::Circle),
            "DOUBLE_SLIM" | "DoubleSlim" | "double_slim" => Ok(Self::DoubleSlim),
            "SLIM_THICK" | "SlimThick" | "slim_thick" => Ok(Self::SlimThick),
            "THICK_SLIM" | "ThickSlim" | "thick_slim" => Ok(Self::ThickSlim),
            "THICK_SLIM_THICK" | "ThickSlimThick" | "thick_slim_thick" => Ok(Self::ThickSlimThick),
            "WAVE" | "Wave" | "wave" => Ok(Self::Wave),
            _ => Err(FoundationError::ParseError {
                type_name: "StrikeoutShape".to_string(),
                value: s.to_string(),
                valid_values: "NONE, SOLID, DASH, DOT, DASH_DOT, DASH_DOT_DOT, LONG_DASH, CIRCLE, DOUBLE_SLIM, SLIM_THICK, THICK_SLIM, THICK_SLIM_THICK, WAVE".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for StrikeoutShape {
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
            7 => Ok(Self::Circle),
            8 => Ok(Self::DoubleSlim),
            9 => Ok(Self::SlimThick),
            10 => Ok(Self::ThickSlim),
            11 => Ok(Self::ThickSlimThick),
            12 => Ok(Self::Wave),
            _ => Err(FoundationError::ParseError {
                type_name: "StrikeoutShape".to_string(),
                value: value.to_string(),
                valid_values: "0-12 (NONE, SOLID, DASH, DOT, DASH_DOT, DASH_DOT_DOT, LONG_DASH, CIRCLE, DOUBLE_SLIM, SLIM_THICK, THICK_SLIM, THICK_SLIM_THICK, WAVE)".to_string(),
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
