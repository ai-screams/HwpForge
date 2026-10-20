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
/// `Some(ArrowStyle { arrow_type: ArrowType::None, .. })` draws no arrowhead,
/// but preserves size and fill values Hancom uses for subsequent editing.
/// An absent endpoint with the Hancom defaults (Medium, filled) needs no style.
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

impl ArrowStyle {
    /// Omit only an absent arrowhead with Hancom's default size and fill.
    ///
    /// Decoders must validate wire values before calling this function. A
    /// non-default size or fill is retained even when `arrow_type` is `None`,
    /// because Hancom uses the stored size when an arrowhead is later selected.
    pub fn into_nondefault(self) -> Option<Self> {
        if self.arrow_type == ArrowType::None && self.size == ArrowSize::Medium && self.filled {
            None
        } else {
            Some(self)
        }
    }
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
        /// Horizontal centre in percent of the shape (HWPX `centerX`).
        ///
        /// `None` = the value encoders wrote before this field existed: 0 for
        /// [`GradientType::Linear`], 50 for the other kinds. Decoders store
        /// the wire value as `Some` even when it equals that default, so a
        /// decoded gradient round-trips exactly.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        center_x: Option<i32>,
        /// Vertical centre in percent of the shape (HWPX `centerY`). `None`
        /// as for [`Fill::Gradient::center_x`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        center_y: Option<i32>,
        /// Number of colour steps (HWPX `step`, Hancom writes 1–255). `None`
        /// = 255.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step: Option<u32>,
        /// Where the middle step sits, in percent (HWPX `stepCenter`). `None`
        /// = 50.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step_center: Option<u32>,
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
    /// No fill: the shape's interior is left transparent (HWPX: the shape has
    /// no `<hc:fillBrush>`).
    ///
    /// Distinct from [`ShapeStyle::fill`] being `None`, which leaves the fill
    /// to the encoder's default (white).
    None,
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
    /// Positive turns clockwise on the page, about the box centre, after
    /// the flip (full definition: `hwpforge_core::control::geometry`).
    pub rotation: Option<f32>,
    /// Flip/mirror state. `None` means no flip. Applied about the box
    /// centre, before the rotation.
    pub flip: Option<Flip>,
    /// Arrowhead at the start of a line. Only meaningful for `Control::Line`.
    pub head_arrow: Option<ArrowStyle>,
    /// Arrowhead at the end of a line. Only meaningful for `Control::Line`.
    pub tail_arrow: Option<ArrowStyle>,
    /// Advanced fill (gradient, pattern, image) or explicitly no fill
    /// ([`Fill::None`]). Overrides `fill_color` when present.
    pub fill: Option<Fill>,
    /// Drop cap style for the shape (HWPX `dropcapstyle` attribute).
    /// Controls whether the shape participates in a drop-cap layout.
    #[serde(default)]
    pub drop_cap_style: DropCapStyle,
    /// Corner rounding of a rectangle or text box, in percent of the shorter
    /// side (HWPX `<hp:rect ratio>`, HWP5 rectangle record byte 0): 0 =
    /// square corners, 50 = semicircular ends. Only meaningful for
    /// `Control::Rect` and `Control::TextBox`. `None` = 0.
    /// [`Document::validate`](crate::Document::validate) rejects a value
    /// above 50.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(max = 50))]
    pub corner_ratio: Option<u8>,
}

#[cfg(test)]
mod gradient_json_tests {
    use super::*;

    // S13: 필드 없는 옛 JSON 은 `None` 으로 읽고, `None` 은 쓰지 않는다.
    // 이것을 실패시키는 것: `skip_serializing_if` 를 빼기(옛 그러데이션 JSON 에
    // `null` 넷이 생김).
    #[test]
    fn gradient_without_center_fields_round_trips_as_old_json() {
        let old = r#"{"Gradient":{"gradient_type":"Radial","angle":0,"colors":[]}}"#;
        let fill: Fill = serde_json::from_str(old).unwrap();
        let Fill::Gradient { center_x, center_y, step, step_center, .. } = &fill else {
            panic!("{fill:?}")
        };
        assert_eq!((center_x, center_y, step, step_center), (&None, &None, &None, &None));
        assert_eq!(serde_json::to_string(&fill).unwrap(), old);
    }
}
