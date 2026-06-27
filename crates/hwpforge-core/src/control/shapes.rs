//! Shape geometry and visual style primitives for drawing controls.
//!
//! Houses [`ShapePoint`], [`LineStyle`], [`ArrowStyle`], [`Fill`], and
//! [`ShapeStyle`] — the building blocks shared by the shape variants of
//! [`Control`](super::Control).

use hwpforge_foundation::{
    ArrowSize, ArrowType, Color, DropCapStyle, Flip, GradientType, ImageFillMode, PatternType,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// A 2D point in raw HWPUNIT coordinates for shape geometry.
///
/// Uses `i32` (not `HwpUnit`) because shape geometry points are raw
/// coordinate values within a bounding box, not document-level measurements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ShapePoint {
    /// X coordinate (HWPUNIT).
    pub x: i32,
    /// Y coordinate (HWPUNIT).
    pub y: i32,
}

impl ShapePoint {
    /// Creates a new shape point with the given coordinates.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::ShapePoint;
    ///
    /// let pt = ShapePoint::new(100, 200);
    /// assert_eq!(pt.x, 100);
    /// assert_eq!(pt.y, 200);
    /// ```
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// Line drawing style for shapes.
///
/// Controls how the stroke of a shape is rendered (solid, dashed, etc.).
/// Maps to HWPX `<hc:lineShape>` `dash` attribute values.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::LineStyle;
///
/// let style = LineStyle::Dash;
/// assert_eq!(style.to_string(), "DASH");
/// assert_eq!("DOT".parse::<LineStyle>().unwrap(), LineStyle::Dot);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum LineStyle {
    /// Continuous solid line (default).
    #[default]
    Solid,
    /// Dashed line.
    Dash,
    /// Dotted line.
    Dot,
    /// Alternating dash and dot.
    DashDot,
    /// Alternating dash, dot, dot.
    DashDotDot,
    /// No visible line.
    None,
}

impl std::fmt::Display for LineStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Solid => f.write_str("SOLID"),
            Self::Dash => f.write_str("DASH"),
            Self::Dot => f.write_str("DOT"),
            Self::DashDot => f.write_str("DASH_DOT"),
            Self::DashDotDot => f.write_str("DASH_DOT_DOT"),
            Self::None => f.write_str("NONE"),
        }
    }
}

impl std::str::FromStr for LineStyle {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "SOLID" | "Solid" | "solid" => Ok(Self::Solid),
            "DASH" | "Dash" | "dash" => Ok(Self::Dash),
            "DOT" | "Dot" | "dot" => Ok(Self::Dot),
            "DASH_DOT" | "DashDot" | "dash_dot" => Ok(Self::DashDot),
            "DASH_DOT_DOT" | "DashDotDot" | "dash_dot_dot" => Ok(Self::DashDotDot),
            "NONE" | "None" | "none" => Ok(Self::None),
            _ => Err(CoreError::InvalidStructure {
                context: "LineStyle".to_string(),
                reason: format!(
                    "unknown line style '{s}', valid: SOLID, DASH, DOT, DASH_DOT, DASH_DOT_DOT, NONE"
                ),
            }),
        }
    }
}

/// Arrowhead style for line endpoints.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::ArrowStyle;
/// use hwpforge_foundation::{ArrowType, ArrowSize};
///
/// let arrow = ArrowStyle {
///     arrow_type: ArrowType::Normal,
///     size: ArrowSize::Medium,
///     filled: true,
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ArrowStyle {
    /// Shape of the arrowhead.
    pub arrow_type: ArrowType,
    /// Size of the arrowhead.
    pub size: ArrowSize,
    /// Whether the arrowhead is filled (true) or outlined (false).
    pub filled: bool,
}

/// Fill specification for shapes.
///
/// Replaces simple `fill_color` for shapes that need gradient, pattern, or image fills.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::Fill;
/// use hwpforge_foundation::Color;
///
/// let solid = Fill::Solid { color: Color::from_rgb(255, 0, 0) };
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum Fill {
    /// Solid color fill.
    Solid {
        /// Fill color.
        color: Color,
    },
    /// Gradient fill.
    Gradient {
        /// Gradient direction type.
        gradient_type: GradientType,
        /// Gradient angle in degrees.
        angle: i32,
        /// Color stops: (color, position 0-100).
        colors: Vec<(Color, u32)>,
    },
    /// Hatch pattern fill.
    Pattern {
        /// Pattern type.
        pattern_type: PatternType,
        /// Foreground pattern color.
        fg_color: Color,
        /// Background color.
        bg_color: Color,
    },
    /// Image fill.
    Image {
        /// Image binary data reference ID.
        image_id: String,
        /// Image fill mode (tile, stretch, etc.).
        mode: ImageFillMode,
    },
}

/// Visual style overrides for drawing shapes.
///
/// All fields are `Option`; `None` means "use the encoder's default"
/// (typically black solid border, white fill, 0.12 mm stroke).
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::{ShapeStyle, LineStyle};
/// use hwpforge_foundation::Color;
///
/// let style = ShapeStyle {
///     line_color: Some(Color::from_rgb(255, 0, 0)),
///     fill_color: Some(Color::from_rgb(0, 255, 0)),
///     line_width: Some(100),
///     line_style: Some(LineStyle::Dash),
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ShapeStyle {
    /// Stroke/border color (e.g. `Color::from_rgb(255, 0, 0)` for red).
    pub line_color: Option<Color>,
    /// Fill color (e.g. `Color::from_rgb(0, 255, 0)` for green).
    /// For advanced fills (gradient, pattern, image), use the `fill` field instead.
    pub fill_color: Option<Color>,
    /// Stroke width in HWPUNIT (33 ≈ 0.12mm, 100 ≈ 0.35mm).
    pub line_width: Option<u32>,
    /// Line drawing style (solid, dash, dot, etc.).
    pub line_style: Option<LineStyle>,
    /// Rotation angle in degrees (0-360). `None` means no rotation.
    pub rotation: Option<f32>,
    /// Flip/mirror state. `None` means no flip.
    pub flip: Option<Flip>,
    /// Arrowhead at the start of a line. Only meaningful for `Control::Line`.
    pub head_arrow: Option<ArrowStyle>,
    /// Arrowhead at the end of a line. Only meaningful for `Control::Line`.
    pub tail_arrow: Option<ArrowStyle>,
    /// Advanced fill (gradient, pattern, image). Overrides `fill_color` when present.
    pub fill: Option<Fill>,
    /// Drop cap style for the shape (HWPX `dropcapstyle` attribute).
    /// Controls whether the shape participates in a drop-cap layout.
    #[serde(default)]
    pub drop_cap_style: DropCapStyle,
}
