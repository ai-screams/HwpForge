//! Border and fill enums: line types, brush/gradient/pattern fills, and image fill modes.

use crate::error::FoundationError;
use serde::{Deserialize, Serialize};
use std::fmt;

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
        // 한글 renders BACK_SLASH as `/` and SLASH as `\` — opposite to KS X 6101 XSD docs.
        // We swap the mapping so our semantic enum values match actual rendering.
        match self {
            Self::Horizontal => f.write_str("HORIZONTAL"),
            Self::Vertical => f.write_str("VERTICAL"),
            Self::BackSlash => f.write_str("SLASH"),
            Self::Slash => f.write_str("BACK_SLASH"),
            Self::Cross => f.write_str("CROSS"),
            Self::CrossDiagonal => f.write_str("CROSS_DIAGONAL"),
        }
    }
}

impl std::str::FromStr for PatternType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Swapped BACK_SLASH/SLASH to match Display (한글 renders them opposite to spec).
        // Only SCREAMING_CASE forms used here — PascalCase comes through serde derive.
        match s {
            "HORIZONTAL" | "horizontal" => Ok(Self::Horizontal),
            "VERTICAL" | "vertical" => Ok(Self::Vertical),
            "BACK_SLASH" | "backslash" => Ok(Self::Slash),
            "SLASH" | "slash" => Ok(Self::BackSlash),
            "CROSS" | "cross" => Ok(Self::Cross),
            "CROSS_DIAGONAL" | "crossdiagonal" => Ok(Self::CrossDiagonal),
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
