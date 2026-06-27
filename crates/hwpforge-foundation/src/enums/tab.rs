//! Tab-stop enums: tab alignment and tab leader characters.

use crate::error::FoundationError;
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// TabAlign
// ---------------------------------------------------------------------------

/// Tab stop alignment.
///
/// Maps to HWPX `<hh:tabItem type="...">`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum TabAlign {
    /// Left-aligned tab.
    #[default]
    Left = 0,
    /// Right-aligned tab.
    Right = 1,
    /// Center-aligned tab.
    Center = 2,
    /// Decimal-aligned tab.
    Decimal = 3,
}

impl TabAlign {
    /// Converts to the HWPX XML attribute string.
    pub fn to_hwpx_str(self) -> &'static str {
        match self {
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
            Self::Center => "CENTER",
            Self::Decimal => "DECIMAL",
        }
    }

    /// Parses a HWPX XML attribute string.
    pub fn from_hwpx_str(s: &str) -> Self {
        match s {
            "RIGHT" => Self::Right,
            "CENTER" => Self::Center,
            "DECIMAL" => Self::Decimal,
            _ => Self::Left,
        }
    }
}

impl fmt::Display for TabAlign {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Left => f.write_str("Left"),
            Self::Right => f.write_str("Right"),
            Self::Center => f.write_str("Center"),
            Self::Decimal => f.write_str("Decimal"),
        }
    }
}

impl std::str::FromStr for TabAlign {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Left" | "LEFT" | "left" => Ok(Self::Left),
            "Right" | "RIGHT" | "right" => Ok(Self::Right),
            "Center" | "CENTER" | "center" => Ok(Self::Center),
            "Decimal" | "DECIMAL" | "decimal" => Ok(Self::Decimal),
            _ => Err(FoundationError::ParseError {
                type_name: "TabAlign".to_string(),
                value: s.to_string(),
                valid_values: "Left, Right, Center, Decimal".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for TabAlign {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("TabAlign")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// TabLeader
// ---------------------------------------------------------------------------

/// Tab leader line style.
///
/// Stored as an uppercase HWPX-compatible string so unknown vendor values
/// survive roundtrip instead of being silently flattened.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct TabLeader(String);

impl TabLeader {
    /// Creates a leader from a HWPX line type string.
    pub fn from_hwpx_str(s: &str) -> Self {
        Self(s.to_ascii_uppercase())
    }

    /// Returns the canonical HWPX string.
    pub fn as_hwpx_str(&self) -> &str {
        &self.0
    }

    /// No leader.
    pub fn none() -> Self {
        Self::from_hwpx_str("NONE")
    }

    /// Dotted leader.
    pub fn dot() -> Self {
        Self::from_hwpx_str("DOT")
    }
}

impl fmt::Display for TabLeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_hwpx_str())
    }
}

impl std::str::FromStr for TabLeader {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_hwpx_str(s))
    }
}

impl schemars::JsonSchema for TabLeader {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("TabLeader")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}
