//! Border and fill types for paragraph and table styling.
//!
//! This module defines border (line style around elements) and fill (background)
//! configurations following the **two-type pattern**:
//!
//! - [`PartialBorderFill`] — all fields `Option`, for YAML and inheritance
//! - [`BorderFill`] — all fields required, after resolution
//!
//! # Examples
//!
//! ```
//! use hwpforge_blueprint::border_fill::{PartialBorderFill, BorderSide, Fill};
//! use hwpforge_foundation::{BorderLineType, FillBrushType, Color, HwpUnit};
//!
//! let mut partial = PartialBorderFill::default();
//! partial.border = Some(hwpforge_blueprint::border_fill::Border {
//!     top: BorderSide {
//!         line_type: BorderLineType::Solid,
//!         width: Some(HwpUnit::from_pt(0.5).unwrap()),
//!         color: Some(Color::BLACK),
//!     },
//!     left: BorderSide::default(),
//!     right: BorderSide::default(),
//!     bottom: BorderSide::default(),
//! });
//!
//! let resolved = partial.resolve();
//! assert_eq!(resolved.border.top.line_type, BorderLineType::Solid);
//! ```

use hwpforge_foundation::{BorderLineType, Color, FillBrushType, HwpUnit};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::serde_helpers::{de_color_opt, de_dim_opt, ser_color_opt, ser_dim_opt};

// ---------------------------------------------------------------------------
// BorderSide
// ---------------------------------------------------------------------------

/// Border configuration for one side (top/left/right/bottom).
///
/// # Examples
///
/// ```
/// use hwpforge_blueprint::border_fill::BorderSide;
/// use hwpforge_foundation::{BorderLineType, Color, HwpUnit};
///
/// let side = BorderSide {
///     line_type: BorderLineType::Solid,
///     width: Some(HwpUnit::from_pt(1.0).unwrap()),
///     color: Some(Color::BLACK),
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BorderSide {
    /// Border line type.
    #[serde(default)]
    pub line_type: BorderLineType,

    /// Border line width.
    #[serde(
        default,
        serialize_with = "ser_dim_opt",
        deserialize_with = "de_dim_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub width: Option<HwpUnit>,

    /// Border color.
    #[serde(
        default,
        serialize_with = "ser_color_opt",
        deserialize_with = "de_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub color: Option<Color>,
}

impl Default for BorderSide {
    fn default() -> Self {
        Self { line_type: BorderLineType::None, width: None, color: None }
    }
}

// ---------------------------------------------------------------------------
// Border
// ---------------------------------------------------------------------------

/// Four-sided border configuration.
///
/// # Examples
///
/// ```
/// use hwpforge_blueprint::border_fill::Border;
///
/// let border = Border::default();
/// assert_eq!(border.top.line_type, hwpforge_foundation::BorderLineType::None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Border {
    /// Top border.
    #[serde(default)]
    pub top: BorderSide,
    /// Left border.
    #[serde(default)]
    pub left: BorderSide,
    /// Right border.
    #[serde(default)]
    pub right: BorderSide,
    /// Bottom border.
    #[serde(default)]
    pub bottom: BorderSide,
}

// ---------------------------------------------------------------------------
// Fill
// ---------------------------------------------------------------------------

/// Background fill configuration.
///
/// # Examples
///
/// ```
/// use hwpforge_blueprint::border_fill::Fill;
/// use hwpforge_foundation::{FillBrushType, Color};
///
/// let fill = Fill {
///     brush_type: FillBrushType::Solid,
///     color: Some(Color::from_rgb(0xF0, 0xF0, 0xF0)),
///     color2: None,
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Fill {
    /// Fill type.
    #[serde(default)]
    pub brush_type: FillBrushType,

    /// Primary fill color.
    #[serde(
        default,
        serialize_with = "ser_color_opt",
        deserialize_with = "de_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub color: Option<Color>,

    /// Secondary color for gradients/patterns.
    #[serde(
        default,
        serialize_with = "ser_color_opt",
        deserialize_with = "de_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub color2: Option<Color>,
}

impl Default for Fill {
    fn default() -> Self {
        Self { brush_type: FillBrushType::None, color: None, color2: None }
    }
}

// ---------------------------------------------------------------------------
// PartialBorderFill (for YAML and inheritance)
// ---------------------------------------------------------------------------

/// Combined border and fill configuration with optional fields for YAML.
///
/// After inheritance resolution, this is converted to [`BorderFill`] where
/// all fields are guaranteed to be present.
///
/// # Examples
///
/// ```
/// use hwpforge_blueprint::border_fill::PartialBorderFill;
///
/// let partial = PartialBorderFill::default();
/// let resolved = partial.resolve();
/// assert_eq!(resolved.border.top.line_type, hwpforge_foundation::BorderLineType::None);
/// ```
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct PartialBorderFill {
    /// Border configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<Border>,

    /// Fill configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Fill>,
}

impl PartialBorderFill {
    /// Merges `other` into `self` (child overrides parent).
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_blueprint::border_fill::{PartialBorderFill, Fill};
    /// use hwpforge_foundation::{FillBrushType, Color};
    ///
    /// let mut base = PartialBorderFill::default();
    /// let child = PartialBorderFill {
    ///     fill: Some(Fill {
    ///         brush_type: FillBrushType::Solid,
    ///         color: Some(Color::WHITE),
    ///         color2: None,
    ///     }),
    ///     ..Default::default()
    /// };
    /// base.merge(&child);
    /// assert!(base.fill.is_some());
    /// ```
    pub fn merge(&mut self, other: &PartialBorderFill) {
        if other.border.is_some() {
            self.border = other.border;
        }
        if other.fill.is_some() {
            self.fill = other.fill;
        }
    }

    /// Resolves into a fully-specified [`BorderFill`] with defaults.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_blueprint::border_fill::PartialBorderFill;
    ///
    /// let partial = PartialBorderFill::default();
    /// let resolved = partial.resolve();
    /// assert_eq!(resolved.fill.brush_type, hwpforge_foundation::FillBrushType::None);
    /// ```
    pub fn resolve(&self) -> BorderFill {
        BorderFill { border: self.border.unwrap_or_default(), fill: self.fill.unwrap_or_default() }
    }
}

// ---------------------------------------------------------------------------
// BorderFill (resolved, fully-specified)
// ---------------------------------------------------------------------------

/// Fully-resolved border and fill configuration (all fields present).
///
/// Created from [`PartialBorderFill`] after inheritance resolution.
///
/// # Examples
///
/// ```
/// use hwpforge_blueprint::border_fill::{BorderFill, Border, Fill};
///
/// let bf = BorderFill {
///     border: Border::default(),
///     fill: Fill::default(),
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BorderFill {
    /// Border configuration.
    pub border: Border,
    /// Fill configuration.
    pub fill: Fill,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // ===================================================================
    // BorderSide
    // ===================================================================

    #[test]
    fn border_side_default_is_none() {
        let side = BorderSide::default();
        assert_eq!(side.line_type, BorderLineType::None);
        assert!(side.width.is_none());
        assert!(side.color.is_none());
    }

    #[test]
    fn border_side_with_values() {
        let side = BorderSide {
            line_type: BorderLineType::Solid,
            width: Some(HwpUnit::from_pt(1.0).unwrap()),
            color: Some(Color::BLACK),
        };
        assert_eq!(side.line_type, BorderLineType::Solid);
        assert_eq!(side.width, Some(HwpUnit::from_pt(1.0).unwrap()));
        assert_eq!(side.color, Some(Color::BLACK));
    }

    #[test]
    fn border_side_serde_roundtrip() {
        let side = BorderSide {
            line_type: BorderLineType::Dash,
            width: Some(HwpUnit::from_pt(0.5).unwrap()),
            color: Some(Color::from_rgb(0x33, 0x66, 0x99)),
        };
        let yaml = serde_yaml::to_string(&side).unwrap();
        let back: BorderSide = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(side, back);
    }

    // ===================================================================
    // Border
    // ===================================================================

    #[test]
    fn border_default_all_sides_none() {
        let border = Border::default();
        assert_eq!(border.top.line_type, BorderLineType::None);
        assert_eq!(border.left.line_type, BorderLineType::None);
        assert_eq!(border.right.line_type, BorderLineType::None);
        assert_eq!(border.bottom.line_type, BorderLineType::None);
    }

    #[test]
    fn border_with_top_only() {
        let border = Border {
            top: BorderSide {
                line_type: BorderLineType::Solid,
                width: Some(HwpUnit::from_pt(1.0).unwrap()),
                color: Some(Color::BLACK),
            },
            ..Default::default()
        };
        assert_eq!(border.top.line_type, BorderLineType::Solid);
        assert_eq!(border.left.line_type, BorderLineType::None);
    }

    #[test]
    fn border_serde_roundtrip() {
        let border = Border {
            top: BorderSide {
                line_type: BorderLineType::Solid,
                width: Some(HwpUnit::from_pt(1.0).unwrap()),
                color: Some(Color::BLACK),
            },
            bottom: BorderSide {
                line_type: BorderLineType::Dash,
                width: Some(HwpUnit::from_pt(0.5).unwrap()),
                color: Some(Color::from_rgb(0xFF, 0x00, 0x00)),
            },
            ..Default::default()
        };
        let yaml = serde_yaml::to_string(&border).unwrap();
        let back: Border = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(border, back);
    }

    // ===================================================================
    // Fill
    // ===================================================================

    #[test]
    fn fill_default_is_none() {
        let fill = Fill::default();
        assert_eq!(fill.brush_type, FillBrushType::None);
        assert!(fill.color.is_none());
        assert!(fill.color2.is_none());
    }

    #[test]
    fn fill_solid_color() {
        let fill = Fill {
            brush_type: FillBrushType::Solid,
            color: Some(Color::from_rgb(0xF0, 0xF0, 0xF0)),
            color2: None,
        };
        assert_eq!(fill.brush_type, FillBrushType::Solid);
        assert_eq!(fill.color, Some(Color::from_rgb(0xF0, 0xF0, 0xF0)));
    }

    #[test]
    fn fill_gradient_two_colors() {
        let fill = Fill {
            brush_type: FillBrushType::Gradient,
            color: Some(Color::WHITE),
            color2: Some(Color::BLACK),
        };
        assert_eq!(fill.brush_type, FillBrushType::Gradient);
        assert_eq!(fill.color, Some(Color::WHITE));
        assert_eq!(fill.color2, Some(Color::BLACK));
    }

    #[test]
    fn fill_serde_roundtrip() {
        let fill = Fill {
            brush_type: FillBrushType::Gradient,
            color: Some(Color::from_rgb(0xFF, 0xFF, 0x00)),
            color2: Some(Color::from_rgb(0x00, 0xFF, 0xFF)),
        };
        let yaml = serde_yaml::to_string(&fill).unwrap();
        let back: Fill = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(fill, back);
    }

    // ===================================================================
    // PartialBorderFill
    // ===================================================================

    #[test]
    fn partial_border_fill_default_is_all_none() {
        let partial = PartialBorderFill::default();
        assert!(partial.border.is_none());
        assert!(partial.fill.is_none());
    }

    #[test]
    fn partial_border_fill_merge_overrides() {
        let mut base = PartialBorderFill { border: Some(Border::default()), fill: None };
        let child = PartialBorderFill {
            border: None,
            fill: Some(Fill {
                brush_type: FillBrushType::Solid,
                color: Some(Color::WHITE),
                color2: None,
            }),
        };
        base.merge(&child);
        assert!(base.border.is_some()); // Preserved from base
        assert!(base.fill.is_some()); // Added from child
    }

    #[test]
    fn partial_border_fill_merge_child_replaces() {
        let mut base = PartialBorderFill {
            border: Some(Border {
                top: BorderSide {
                    line_type: BorderLineType::Solid,
                    width: Some(HwpUnit::from_pt(1.0).unwrap()),
                    color: Some(Color::BLACK),
                },
                ..Default::default()
            }),
            fill: None,
        };
        let child = PartialBorderFill {
            border: Some(Border::default()), // Replace with default
            fill: None,
        };
        base.merge(&child);
        assert_eq!(base.border.unwrap().top.line_type, BorderLineType::None); // Replaced
    }

    #[test]
    fn partial_border_fill_resolve_defaults() {
        let partial = PartialBorderFill::default();
        let resolved = partial.resolve();
        assert_eq!(resolved.border.top.line_type, BorderLineType::None);
        assert_eq!(resolved.fill.brush_type, FillBrushType::None);
    }

    #[test]
    fn partial_border_fill_resolve_with_values() {
        let partial = PartialBorderFill {
            border: Some(Border {
                top: BorderSide {
                    line_type: BorderLineType::Solid,
                    width: Some(HwpUnit::from_pt(1.0).unwrap()),
                    color: Some(Color::BLACK),
                },
                ..Default::default()
            }),
            fill: Some(Fill {
                brush_type: FillBrushType::Solid,
                color: Some(Color::from_rgb(0xF0, 0xF0, 0xF0)),
                color2: None,
            }),
        };
        let resolved = partial.resolve();
        assert_eq!(resolved.border.top.line_type, BorderLineType::Solid);
        assert_eq!(resolved.fill.brush_type, FillBrushType::Solid);
        assert_eq!(resolved.fill.color, Some(Color::from_rgb(0xF0, 0xF0, 0xF0)));
    }

    #[test]
    fn partial_border_fill_serde_roundtrip() {
        let partial = PartialBorderFill {
            border: Some(Border {
                top: BorderSide {
                    line_type: BorderLineType::Solid,
                    width: Some(HwpUnit::from_pt(1.0).unwrap()),
                    color: Some(Color::BLACK),
                },
                ..Default::default()
            }),
            fill: Some(Fill {
                brush_type: FillBrushType::Solid,
                color: Some(Color::WHITE),
                color2: None,
            }),
        };
        let yaml = serde_yaml::to_string(&partial).unwrap();
        let back: PartialBorderFill = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(partial, back);
    }

    // ===================================================================
    // BorderFill
    // ===================================================================

    #[test]
    fn border_fill_construction() {
        let bf = BorderFill { border: Border::default(), fill: Fill::default() };
        assert_eq!(bf.border.top.line_type, BorderLineType::None);
        assert_eq!(bf.fill.brush_type, FillBrushType::None);
    }

    #[test]
    fn border_fill_serde_roundtrip() {
        let bf = BorderFill {
            border: Border {
                top: BorderSide {
                    line_type: BorderLineType::Solid,
                    width: Some(HwpUnit::from_pt(1.0).unwrap()),
                    color: Some(Color::BLACK),
                },
                ..Default::default()
            },
            fill: Fill {
                brush_type: FillBrushType::Gradient,
                color: Some(Color::WHITE),
                color2: Some(Color::BLACK),
            },
        };
        let yaml = serde_yaml::to_string(&bf).unwrap();
        let back: BorderFill = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(bf, back);
    }

    // ===================================================================
    // YAML examples
    // ===================================================================

    #[test]
    fn partial_border_fill_from_yaml() {
        let yaml = r#"
border:
  top:
    line_type: Solid
    width: 1pt
    color: '#000000'
  bottom:
    line_type: Dash
    width: 0.5pt
    color: '#FF0000'
fill:
  brush_type: Solid
  color: '#F0F0F0'
"#;
        let partial: PartialBorderFill = serde_yaml::from_str(yaml).unwrap();
        assert!(partial.border.is_some());
        let border = partial.border.unwrap();
        assert_eq!(border.top.line_type, BorderLineType::Solid);
        assert_eq!(border.bottom.line_type, BorderLineType::Dash);
        assert!(partial.fill.is_some());
        assert_eq!(partial.fill.unwrap().brush_type, FillBrushType::Solid);
    }
}
