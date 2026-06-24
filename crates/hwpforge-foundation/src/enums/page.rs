//! Page-level enums: page application scope, numbering, gutters, and section restart.

use crate::error::FoundationError;
use serde::{Deserialize, Serialize};
use std::fmt;

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
    /// Circled Latin lowercase: ⓐ, ⓑ, ⓒ, ...
    CircledLatinSmall = 10,
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
            Self::CircledLatinSmall => f.write_str("CircledLatinSmall"),
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
            "CircledLatinSmall" | "circledlatinsmall" | "CIRCLED_LATIN_SMALL" => {
                Ok(Self::CircledLatinSmall)
            }
            _ => Err(FoundationError::ParseError {
                type_name: "NumberFormatType".to_string(),
                value: s.to_string(),
                valid_values: "Digit, CircledDigit, RomanCapital, RomanSmall, LatinCapital, LatinSmall, HangulSyllable, HangulJamo, HanjaDigit, CircledHangulSyllable, CircledLatinSmall".to_string(),
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
            10 => Ok(Self::CircledLatinSmall),
            _ => Err(FoundationError::ParseError {
                type_name: "NumberFormatType".to_string(),
                value: value.to_string(),
                valid_values: "0-10 (Digit, CircledDigit, RomanCapital, RomanSmall, LatinCapital, LatinSmall, HangulSyllable, HangulJamo, HanjaDigit, CircledHangulSyllable, CircledLatinSmall)".to_string(),
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
