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
