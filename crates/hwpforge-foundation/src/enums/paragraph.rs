//! Paragraph-level enums: alignment, line spacing, breaks, headings, and direction.

use crate::error::FoundationError;
use serde::{Deserialize, Serialize};
use std::fmt;

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
    /// Minimum spacing in HwpUnit; line height expands when content
    /// requires more room (HWPX wire form `AT_LEAST`).
    AtLeast = 3,
}

impl fmt::Display for LineSpacingType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Percentage => f.write_str("Percentage"),
            Self::Fixed => f.write_str("Fixed"),
            Self::BetweenLines => f.write_str("BetweenLines"),
            Self::AtLeast => f.write_str("AtLeast"),
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
            "AtLeast" | "atleast" | "at_least" => Ok(Self::AtLeast),
            _ => Err(FoundationError::ParseError {
                type_name: "LineSpacingType".to_string(),
                value: s.to_string(),
                valid_values: "Percentage, Fixed, BetweenLines, AtLeast".to_string(),
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
            3 => Ok(Self::AtLeast),
            _ => Err(FoundationError::ParseError {
                type_name: "LineSpacingType".to_string(),
                value: value.to_string(),
                valid_values: "0 (Percentage), 1 (Fixed), 2 (BetweenLines), 3 (AtLeast)"
                    .to_string(),
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
    /// Allow hyphenation at line breaks (Latin scripts only).
    Hyphenation = 2,
}

impl fmt::Display for WordBreakType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeepWord => f.write_str("KEEP_WORD"),
            Self::BreakWord => f.write_str("BREAK_WORD"),
            Self::Hyphenation => f.write_str("HYPHENATION"),
        }
    }
}

impl std::str::FromStr for WordBreakType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "KEEP_WORD" | "KeepWord" | "keep_word" => Ok(Self::KeepWord),
            "BREAK_WORD" | "BreakWord" | "break_word" => Ok(Self::BreakWord),
            "HYPHENATION" | "Hyphenation" | "hyphenation" => Ok(Self::Hyphenation),
            _ => Err(FoundationError::ParseError {
                type_name: "WordBreakType".to_string(),
                value: s.to_string(),
                valid_values: "KEEP_WORD, BREAK_WORD, HYPHENATION".to_string(),
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
            2 => Ok(Self::Hyphenation),
            _ => Err(FoundationError::ParseError {
                type_name: "WordBreakType".to_string(),
                value: value.to_string(),
                valid_values: "0 (KeepWord), 1 (BreakWord), 2 (Hyphenation)".to_string(),
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
