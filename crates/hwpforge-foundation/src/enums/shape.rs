//! Shape geometry enums: flip, arc, arrow, and curve-segment types.

use crate::error::FoundationError;
use serde::{Deserialize, Serialize};
use std::fmt;

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
// VerticalAlign
// ---------------------------------------------------------------------------

/// Vertical alignment of text inside a drawing shape (글상자/타원/다각형).
///
/// Maps to the HWPX `<hp:drawText><hp:subList vertAlign="...">` attribute and
/// the HWP5 문단 리스트 헤더 (`HWPTAG_LIST_HEADER`) 속성 bits 5–6 (표 65):
/// `0 = top`, `1 = center`, `2 = bottom`. The default is [`VerticalAlign::Top`],
/// matching 한컴's default rendering (text pinned to the shape's top edge).
///
/// This is a neutral enum shared by shape text. Table cells keep their own
/// [`crate`]-external `TableVerticalAlign` for now; unifying the two is a
/// follow-up concern (see ADR-008).
///
/// `Display`/`FromStr` use the HWPX wire tokens (`TOP`/`CENTER`/`BOTTOM`).
///
/// # Examples
///
/// ```
/// use hwpforge_foundation::VerticalAlign;
/// use std::str::FromStr;
///
/// assert_eq!(VerticalAlign::default(), VerticalAlign::Top);
/// assert_eq!(VerticalAlign::Center.to_string(), "CENTER");
/// assert_eq!(VerticalAlign::from_str("BOTTOM").unwrap(), VerticalAlign::Bottom);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum VerticalAlign {
    /// Align text to the top edge of the shape (default).
    #[default]
    Top = 0,
    /// Center text vertically within the shape.
    Center = 1,
    /// Align text to the bottom edge of the shape.
    Bottom = 2,
}

impl fmt::Display for VerticalAlign {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Top => f.write_str("TOP"),
            Self::Center => f.write_str("CENTER"),
            Self::Bottom => f.write_str("BOTTOM"),
        }
    }
}

impl std::str::FromStr for VerticalAlign {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Top" | "top" | "TOP" => Ok(Self::Top),
            "Center" | "center" | "CENTER" => Ok(Self::Center),
            "Bottom" | "bottom" | "BOTTOM" => Ok(Self::Bottom),
            _ => Err(FoundationError::ParseError {
                type_name: "VerticalAlign".to_string(),
                value: s.to_string(),
                valid_values: "TOP, CENTER, BOTTOM".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for VerticalAlign {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Top),
            1 => Ok(Self::Center),
            2 => Ok(Self::Bottom),
            _ => Err(FoundationError::ParseError {
                type_name: "VerticalAlign".to_string(),
                value: value.to_string(),
                valid_values: "0 (Top), 1 (Center), 2 (Bottom)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for VerticalAlign {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("VerticalAlign")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

#[cfg(test)]
mod vertical_align_tests {
    use super::VerticalAlign;
    use std::str::FromStr;

    #[test]
    fn default_is_top() {
        assert_eq!(VerticalAlign::default(), VerticalAlign::Top);
    }

    #[test]
    fn display_uses_hwpx_tokens() {
        assert_eq!(VerticalAlign::Top.to_string(), "TOP");
        assert_eq!(VerticalAlign::Center.to_string(), "CENTER");
        assert_eq!(VerticalAlign::Bottom.to_string(), "BOTTOM");
    }

    #[test]
    fn from_str_accepts_hwpx_and_pascal_and_lower() {
        assert_eq!(VerticalAlign::from_str("TOP").unwrap(), VerticalAlign::Top);
        assert_eq!(VerticalAlign::from_str("Center").unwrap(), VerticalAlign::Center);
        assert_eq!(VerticalAlign::from_str("bottom").unwrap(), VerticalAlign::Bottom);
    }

    #[test]
    fn from_str_round_trips_display() {
        for v in [VerticalAlign::Top, VerticalAlign::Center, VerticalAlign::Bottom] {
            assert_eq!(VerticalAlign::from_str(&v.to_string()).unwrap(), v);
        }
    }

    #[test]
    fn from_str_rejects_invalid() {
        assert!(VerticalAlign::from_str("").is_err());
        assert!(VerticalAlign::from_str("MIDDLE").is_err());
        assert!(VerticalAlign::from_str("baseline").is_err());
    }

    #[test]
    fn try_from_u8_boundaries() {
        assert_eq!(VerticalAlign::try_from(0u8).unwrap(), VerticalAlign::Top);
        assert_eq!(VerticalAlign::try_from(1u8).unwrap(), VerticalAlign::Center);
        assert_eq!(VerticalAlign::try_from(2u8).unwrap(), VerticalAlign::Bottom);
        assert!(VerticalAlign::try_from(3u8).is_err());
        assert!(VerticalAlign::try_from(u8::MAX).is_err());
    }

    #[test]
    fn is_one_byte() {
        assert_eq!(std::mem::size_of::<VerticalAlign>(), 1);
    }
}
