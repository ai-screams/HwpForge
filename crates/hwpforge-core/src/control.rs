//! Control elements: text boxes, hyperlinks, footnotes, endnotes, etc.
//!
//! [`Control`] represents non-text inline elements within a document.
//! The enum is `#[non_exhaustive]` so new control types can be added
//! in future phases without a breaking change.
//!
//! TextBox, Footnote, and Endnote contain `Vec<Paragraph>` (recursive
//! reference through the document tree). This is how HWP models inline
//! frames and annotations.
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::control::Control;
//! use hwpforge_core::paragraph::Paragraph;
//! use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
//!
//! let link = Control::Hyperlink {
//!     text: "Click here".to_string(),
//!     url: "https://example.com".to_string(),
//! };
//! assert!(link.is_hyperlink());
//! ```

use hwpforge_foundation::{Color, HwpUnit};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::caption::Caption;
use crate::chart::{
    BarShape, ChartData, ChartGrouping, ChartType, LegendPosition, OfPieType, RadarStyle,
    ScatterStyle, StockVariant,
};
use crate::error::{CoreError, CoreResult};
use crate::paragraph::Paragraph;

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
/// };
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ShapeStyle {
    /// Stroke/border color (e.g. `Color::from_rgb(255, 0, 0)` for red).
    pub line_color: Option<Color>,
    /// Fill color (e.g. `Color::from_rgb(0, 255, 0)` for green).
    pub fill_color: Option<Color>,
    /// Stroke width in HWPUNIT (33 ≈ 0.12mm, 100 ≈ 0.35mm).
    pub line_width: Option<u32>,
    /// Line drawing style (solid, dash, dot, etc.).
    pub line_style: Option<LineStyle>,
}

/// An inline control element.
///
/// Controls are non-text elements that appear within a Run.
/// Each variant carries its own data; the enum is `#[non_exhaustive]`
/// for forward compatibility.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::Control;
/// use hwpforge_core::paragraph::Paragraph;
/// use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
///
/// let text_box = Control::TextBox {
///     paragraphs: vec![Paragraph::new(ParaShapeIndex::new(0))],
///     width: HwpUnit::from_mm(80.0).unwrap(),
///     height: HwpUnit::from_mm(40.0).unwrap(),
///     horz_offset: 0,
///     vert_offset: 0,
///     caption: None,
///     style: None,
/// };
/// assert!(text_box.is_text_box());
/// assert!(!text_box.is_hyperlink());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum Control {
    /// An inline text box with its own paragraph content.
    /// Maps to HWPX `<hp:rect>` + `<hp:drawText>` (drawing object, not control).
    TextBox {
        /// Paragraphs inside the text box.
        paragraphs: Vec<Paragraph>,
        /// Box width (HWPUNIT).
        width: HwpUnit,
        /// Box height (HWPUNIT).
        height: HwpUnit,
        /// Horizontal offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        horz_offset: i32,
        /// Vertical offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        vert_offset: i32,
        /// Optional caption attached to this text box.
        caption: Option<Caption>,
        /// Optional visual style overrides (border color, fill, line width).
        style: Option<ShapeStyle>,
    },

    /// A hyperlink with display text and URL.
    Hyperlink {
        /// Visible text of the link.
        text: String,
        /// Target URL.
        url: String,
    },

    /// A footnote containing paragraph content.
    /// Maps to HWPX `<hp:ctrl><hp:footNote>`.
    Footnote {
        /// Instance identifier (unique ID for linking, optional).
        inst_id: Option<u32>,
        /// Paragraphs that form the footnote body.
        paragraphs: Vec<Paragraph>,
    },

    /// An endnote containing paragraph content.
    /// Maps to HWPX `<hp:ctrl><hp:endNote>`.
    Endnote {
        /// Instance identifier (unique ID for linking, optional).
        inst_id: Option<u32>,
        /// Paragraphs that form the endnote body.
        paragraphs: Vec<Paragraph>,
    },

    /// A line drawing object (2 endpoints).
    /// Maps to HWPX `<hp:line>`.
    Line {
        /// Start point (x, y in HWPUNIT).
        start: ShapePoint,
        /// End point (x, y in HWPUNIT).
        end: ShapePoint,
        /// Bounding box width (HWPUNIT).
        width: HwpUnit,
        /// Bounding box height (HWPUNIT).
        height: HwpUnit,
        /// Horizontal offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        horz_offset: i32,
        /// Vertical offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        vert_offset: i32,
        /// Optional caption attached to this line.
        caption: Option<Caption>,
        /// Optional visual style overrides (border color, fill, line width).
        style: Option<ShapeStyle>,
    },

    /// An ellipse (or circle) drawing object.
    /// Maps to HWPX `<hp:ellipse>`.
    Ellipse {
        /// Center point (x, y in HWPUNIT).
        center: ShapePoint,
        /// Axis 1 endpoint (defines semi-major axis direction and length).
        axis1: ShapePoint,
        /// Axis 2 endpoint (perpendicular to axis1, defines semi-minor axis).
        axis2: ShapePoint,
        /// Bounding box width (HWPUNIT).
        width: HwpUnit,
        /// Bounding box height (HWPUNIT).
        height: HwpUnit,
        /// Horizontal offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        horz_offset: i32,
        /// Vertical offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        vert_offset: i32,
        /// Optional text content inside the ellipse.
        paragraphs: Vec<Paragraph>,
        /// Optional caption attached to this ellipse.
        caption: Option<Caption>,
        /// Optional visual style overrides (border color, fill, line width).
        style: Option<ShapeStyle>,
    },

    /// A polygon drawing object (3+ vertices).
    /// Maps to HWPX `<hp:polygon>`.
    Polygon {
        /// Ordered list of vertices (minimum 3).
        vertices: Vec<ShapePoint>,
        /// Bounding box width (HWPUNIT).
        width: HwpUnit,
        /// Bounding box height (HWPUNIT).
        height: HwpUnit,
        /// Horizontal offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        horz_offset: i32,
        /// Vertical offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        vert_offset: i32,
        /// Optional text content inside the polygon.
        paragraphs: Vec<Paragraph>,
        /// Optional caption attached to this polygon.
        caption: Option<Caption>,
        /// Optional visual style overrides (border color, fill, line width).
        style: Option<ShapeStyle>,
    },

    /// An inline equation (수식) using HancomEQN script format.
    /// Maps to HWPX `<hp:equation>` with `<hp:script>` child.
    ///
    /// Equations have NO shape common block (no offset, orgSz, curSz, flip,
    /// rotation, lineShape, fillBrush, shadow). Only sz + pos + outMargin + script.
    Equation {
        /// HancomEQN script text (e.g. `"{a+b} over {c+d}"`).
        script: String,
        /// Bounding box width (HWPUNIT).
        width: HwpUnit,
        /// Bounding box height (HWPUNIT).
        height: HwpUnit,
        /// Baseline position (51-90 typical range).
        base_line: u32,
        /// Text color.
        text_color: Color,
        /// Font name (typically `"HancomEQN"`).
        font: String,
    },

    /// An OOXML chart embedded in the document.
    /// Maps to HWPX `<hp:switch><hp:case><hp:chart>` with separate Chart XML file.
    ///
    /// Charts have NO shape common block (like Equation): only sz + pos + outMargin.
    Chart {
        /// Chart type (18 variants covering all OOXML chart types).
        chart_type: ChartType,
        /// Chart data (category-based or XY-based).
        data: ChartData,
        /// Chart width (HWPUNIT, default ~32250 ≈ 114mm).
        width: HwpUnit,
        /// Chart height (HWPUNIT, default ~18750 ≈ 66mm).
        height: HwpUnit,
        /// Optional chart title.
        title: Option<String>,
        /// Legend position.
        legend: LegendPosition,
        /// Series grouping mode.
        grouping: ChartGrouping,
        /// 3D bar/column shape (None = default Box).
        bar_shape: Option<BarShape>,
        /// Exploded pie/doughnut percentage (None = not exploded, Some(25) = 25% explosion).
        explosion: Option<u32>,
        /// Pie-of-pie or bar-of-pie sub-type (None = default pie-of-pie).
        of_pie_type: Option<OfPieType>,
        /// Radar chart rendering style (None = default Standard).
        radar_style: Option<RadarStyle>,
        /// Surface chart wireframe mode (None = default solid).
        wireframe: Option<bool>,
        /// 3D bubble effect (None = default flat).
        bubble_3d: Option<bool>,
        /// Scatter chart style (None = default Dots).
        scatter_style: Option<ScatterStyle>,
        /// Show data point markers on line charts (None = no markers).
        show_markers: Option<bool>,
        /// Stock chart sub-variant (None = default HLC, 3 series).
        ///
        /// VHLC and VOHLC generate a composite `<c:plotArea>` with both
        /// `<c:barChart>` (volume) and `<c:stockChart>` (price) elements.
        stock_variant: Option<StockVariant>,
    },

    /// An unrecognized control element preserved for round-trip fidelity.
    ///
    /// `tag` holds the element's tag name or type identifier.
    /// `data` holds optional serialized content for lossless preservation.
    Unknown {
        /// Tag name or type identifier of the unrecognized element.
        tag: String,
        /// Optional serialized data for round-trip preservation.
        data: Option<String>,
    },
}

impl Control {
    /// Returns `true` if this is a [`Control::TextBox`].
    pub fn is_text_box(&self) -> bool {
        matches!(self, Self::TextBox { .. })
    }

    /// Returns `true` if this is a [`Control::Hyperlink`].
    pub fn is_hyperlink(&self) -> bool {
        matches!(self, Self::Hyperlink { .. })
    }

    /// Returns `true` if this is a [`Control::Footnote`].
    pub fn is_footnote(&self) -> bool {
        matches!(self, Self::Footnote { .. })
    }

    /// Returns `true` if this is a [`Control::Endnote`].
    pub fn is_endnote(&self) -> bool {
        matches!(self, Self::Endnote { .. })
    }

    /// Returns `true` if this is a [`Control::Line`].
    pub fn is_line(&self) -> bool {
        matches!(self, Self::Line { .. })
    }

    /// Returns `true` if this is a [`Control::Ellipse`].
    pub fn is_ellipse(&self) -> bool {
        matches!(self, Self::Ellipse { .. })
    }

    /// Returns `true` if this is a [`Control::Polygon`].
    pub fn is_polygon(&self) -> bool {
        matches!(self, Self::Polygon { .. })
    }

    /// Returns `true` if this is a [`Control::Equation`].
    pub fn is_equation(&self) -> bool {
        matches!(self, Self::Equation { .. })
    }

    /// Returns `true` if this is a [`Control::Chart`].
    pub fn is_chart(&self) -> bool {
        matches!(self, Self::Chart { .. })
    }

    /// Returns `true` if this is a [`Control::Unknown`].
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Creates a chart control with default dimensions and settings.
    ///
    /// Defaults: width ≈ 114mm, height ≈ 66mm, no title, right legend, clustered grouping.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_core::chart::{ChartType, ChartData};
    ///
    /// let data = ChartData::category(&["A", "B"], &[("S1", &[10.0, 20.0])]);
    /// let ctrl = Control::chart(ChartType::Column, data);
    /// assert!(ctrl.is_chart());
    /// ```
    pub fn chart(chart_type: ChartType, data: ChartData) -> Self {
        Self::Chart {
            chart_type,
            data,
            width: HwpUnit::new(32250).expect("32250 is valid"),
            height: HwpUnit::new(18750).expect("18750 is valid"),
            title: None,
            legend: LegendPosition::default(),
            grouping: ChartGrouping::default(),
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        }
    }

    /// Creates an equation control with default dimensions for the given HancomEQN script.
    ///
    /// Defaults: width ≈ 31mm (8779 HWPUNIT), height ≈ 9.2mm (2600 HWPUNIT),
    /// baseline 71%, black text, `HancomEQN` font.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    ///
    /// let ctrl = Control::equation("{a+b} over {c+d}");
    /// assert!(ctrl.is_equation());
    /// ```
    pub fn equation(script: &str) -> Self {
        Self::Equation {
            script: script.to_string(),
            width: HwpUnit::new(8779).expect("8779 is valid"),
            height: HwpUnit::new(2600).expect("2600 is valid"),
            base_line: 71,
            text_color: Color::BLACK,
            font: "HancomEQN".to_string(),
        }
    }

    /// Creates a text box control with the given paragraphs and dimensions.
    ///
    /// Defaults: inline positioning (horz_offset=0, vert_offset=0), no caption, no style override.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
    ///
    /// let para = Paragraph::new(ParaShapeIndex::new(0));
    /// let width = HwpUnit::from_mm(80.0).unwrap();
    /// let height = HwpUnit::from_mm(40.0).unwrap();
    /// let ctrl = Control::text_box(vec![para], width, height);
    /// assert!(ctrl.is_text_box());
    /// ```
    pub fn text_box(paragraphs: Vec<Paragraph>, width: HwpUnit, height: HwpUnit) -> Self {
        Self::TextBox {
            paragraphs,
            width,
            height,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        }
    }

    /// Creates a footnote control with the given paragraph content.
    ///
    /// Defaults: no inst_id.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_core::run::Run;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    ///
    /// let para = Paragraph::with_runs(
    ///     vec![Run::text("Note text", CharShapeIndex::new(0))],
    ///     ParaShapeIndex::new(0),
    /// );
    /// let ctrl = Control::footnote(vec![para]);
    /// assert!(ctrl.is_footnote());
    /// ```
    pub fn footnote(paragraphs: Vec<Paragraph>) -> Self {
        Self::Footnote { inst_id: None, paragraphs }
    }

    /// Creates an endnote control with the given paragraph content.
    ///
    /// Defaults: no inst_id.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_core::run::Run;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    ///
    /// let para = Paragraph::with_runs(
    ///     vec![Run::text("End note", CharShapeIndex::new(0))],
    ///     ParaShapeIndex::new(0),
    /// );
    /// let ctrl = Control::endnote(vec![para]);
    /// assert!(ctrl.is_endnote());
    /// ```
    pub fn endnote(paragraphs: Vec<Paragraph>) -> Self {
        Self::Endnote { inst_id: None, paragraphs }
    }

    /// Creates a footnote with an explicit instance ID for cross-referencing.
    ///
    /// Use this when you need stable `inst_id` references (e.g. matching decoder output).
    /// For simple footnotes without cross-references, prefer [`Control::footnote`].
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::ParaShapeIndex;
    ///
    /// let ctrl = Control::footnote_with_id(1, vec![Paragraph::new(ParaShapeIndex::new(0))]);
    /// assert!(ctrl.is_footnote());
    /// ```
    pub fn footnote_with_id(inst_id: u32, paragraphs: Vec<Paragraph>) -> Self {
        Self::Footnote { inst_id: Some(inst_id), paragraphs }
    }

    /// Creates an endnote with an explicit instance ID for cross-referencing.
    ///
    /// Use this when you need stable `inst_id` references (e.g. matching decoder output).
    /// For simple endnotes without cross-references, prefer [`Control::endnote`].
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::ParaShapeIndex;
    ///
    /// let ctrl = Control::endnote_with_id(2, vec![Paragraph::new(ParaShapeIndex::new(0))]);
    /// assert!(ctrl.is_endnote());
    /// ```
    pub fn endnote_with_id(inst_id: u32, paragraphs: Vec<Paragraph>) -> Self {
        Self::Endnote { inst_id: Some(inst_id), paragraphs }
    }

    /// Creates an ellipse control with the given bounding box dimensions.
    ///
    /// Geometry is auto-derived: center=(w/2, h/2), axis1=(w, h/2), axis2=(w/2, h).
    /// Defaults: inline positioning (horz_offset=0, vert_offset=0), no paragraphs, no caption, no style.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_foundation::HwpUnit;
    ///
    /// let width = HwpUnit::from_mm(40.0).unwrap();
    /// let height = HwpUnit::from_mm(30.0).unwrap();
    /// let ctrl = Control::ellipse(width, height);
    /// assert!(ctrl.is_ellipse());
    /// ```
    pub fn ellipse(width: HwpUnit, height: HwpUnit) -> Self {
        let w = width.as_i32();
        let h = height.as_i32();
        Self::Ellipse {
            center: ShapePoint::new(w / 2, h / 2),
            axis1: ShapePoint::new(w, h / 2),
            axis2: ShapePoint::new(w / 2, h),
            width,
            height,
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        }
    }

    /// Creates an ellipse control with paragraph content inside.
    ///
    /// Same as [`Control::ellipse`] but accepts paragraphs for text drawn inside the ellipse.
    /// Geometry is auto-derived: center=(w/2, h/2), axis1=(w, h/2), axis2=(w/2, h).
    /// Defaults: inline positioning (horz_offset=0, vert_offset=0), no caption, no style.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
    ///
    /// let width = HwpUnit::from_mm(40.0).unwrap();
    /// let height = HwpUnit::from_mm(30.0).unwrap();
    /// let para = Paragraph::new(ParaShapeIndex::new(0));
    /// let ctrl = Control::ellipse_with_text(width, height, vec![para]);
    /// assert!(ctrl.is_ellipse());
    /// ```
    pub fn ellipse_with_text(width: HwpUnit, height: HwpUnit, paragraphs: Vec<Paragraph>) -> Self {
        let w = width.as_i32();
        let h = height.as_i32();
        Self::Ellipse {
            center: ShapePoint::new(w / 2, h / 2),
            axis1: ShapePoint::new(w, h / 2),
            axis2: ShapePoint::new(w / 2, h),
            width,
            height,
            horz_offset: 0,
            vert_offset: 0,
            paragraphs,
            caption: None,
            style: None,
        }
    }

    /// Creates a polygon control from the given vertices.
    ///
    /// The bounding box is auto-derived from the min/max of vertex coordinates.
    /// Defaults: no paragraphs, no caption, no style.
    ///
    /// Returns an error if fewer than 3 vertices are provided.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidStructure`] if `vertices.len() < 3`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::{Control, ShapePoint};
    ///
    /// let vertices = vec![
    ///     ShapePoint::new(0, 1000),
    ///     ShapePoint::new(500, 0),
    ///     ShapePoint::new(1000, 1000),
    /// ];
    /// let ctrl = Control::polygon(vertices).unwrap();
    /// assert!(ctrl.is_polygon());
    /// ```
    pub fn polygon(vertices: Vec<ShapePoint>) -> CoreResult<Self> {
        if vertices.len() < 3 {
            return Err(CoreError::InvalidStructure {
                context: "Control::polygon".to_string(),
                reason: format!("polygon requires at least 3 vertices, got {}", vertices.len()),
            });
        }
        let min_x = vertices.iter().map(|p| p.x as i64).min().unwrap_or(0);
        let max_x = vertices.iter().map(|p| p.x as i64).max().unwrap_or(0);
        let min_y = vertices.iter().map(|p| p.y as i64).min().unwrap_or(0);
        let max_y = vertices.iter().map(|p| p.y as i64).max().unwrap_or(0);
        let bbox_w = (max_x - min_x).max(0) as i32;
        let bbox_h = (max_y - min_y).max(0) as i32;
        let width = HwpUnit::new(bbox_w).map_err(|_| CoreError::InvalidStructure {
            context: "Control::polygon".into(),
            reason: format!("bounding box width {bbox_w} exceeds HwpUnit range"),
        })?;
        let height = HwpUnit::new(bbox_h).map_err(|_| CoreError::InvalidStructure {
            context: "Control::polygon".into(),
            reason: format!("bounding box height {bbox_h} exceeds HwpUnit range"),
        })?;
        Ok(Self::Polygon {
            vertices,
            width,
            height,
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        })
    }

    /// Creates a line control between two endpoints.
    ///
    /// The bounding box width and height are derived from the absolute difference
    /// of the endpoint coordinates: `width = |end.x - start.x|`, `height = |end.y - start.y|`.
    /// Each axis is clamped to a minimum of 100 HwpUnit (~1pt) because 한글 cannot
    /// render lines with a zero-dimension bounding box.
    /// Defaults: no caption, no style.
    ///
    /// Returns an error if start and end are the same point (degenerate line).
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidStructure`] if start equals end.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::{Control, ShapePoint};
    ///
    /// let ctrl = Control::line(ShapePoint::new(0, 0), ShapePoint::new(5000, 0)).unwrap();
    /// assert!(ctrl.is_line());
    /// ```
    pub fn line(start: ShapePoint, end: ShapePoint) -> CoreResult<Self> {
        if start == end {
            return Err(CoreError::InvalidStructure {
                context: "Control::line".to_string(),
                reason: "start and end points are identical (degenerate line)".to_string(),
            });
        }
        let raw_w = ((end.x as i64) - (start.x as i64)).unsigned_abs() as i32;
        let raw_h = ((end.y as i64) - (start.y as i64)).unsigned_abs() as i32;
        // Minimum bounding box of 100 HwpUnit (~1pt) per axis.
        // 한글 cannot render lines with a zero-dimension bounding box.
        let raw_w = raw_w.max(100);
        let raw_h = raw_h.max(100);
        let width = HwpUnit::new(raw_w).unwrap_or_else(|_| HwpUnit::new(100).expect("valid"));
        let height = HwpUnit::new(raw_h).unwrap_or_else(|_| HwpUnit::new(100).expect("valid"));
        Ok(Self::Line {
            start,
            end,
            width,
            height,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        })
    }

    /// Creates a horizontal line of the given width.
    ///
    /// Shortcut for `line(ShapePoint::new(0, 0), ShapePoint::new(width.as_i32(), 0))`.
    /// The bounding box height is clamped to 100 HwpUnit (~1pt minimum) because
    /// 한글 cannot render lines with a zero-dimension bounding box.
    /// Defaults: no caption, no style.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_foundation::HwpUnit;
    ///
    /// let width = HwpUnit::from_mm(100.0).unwrap();
    /// let ctrl = Control::horizontal_line(width);
    /// assert!(ctrl.is_line());
    /// ```
    pub fn horizontal_line(width: HwpUnit) -> Self {
        let w = width.as_i32();
        Self::Line {
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(w, 0),
            width,
            height: HwpUnit::new(100).expect("100 is valid"),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        }
    }

    /// Creates a hyperlink control with the given display text and URL.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    ///
    /// let ctrl = Control::hyperlink("Visit Rust", "https://rust-lang.org");
    /// assert!(ctrl.is_hyperlink());
    /// ```
    pub fn hyperlink(text: &str, url: &str) -> Self {
        Self::Hyperlink { text: text.to_string(), url: url.to_string() }
    }
}

impl std::fmt::Display for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TextBox { paragraphs, .. } => {
                let n = paragraphs.len();
                let word = if n == 1 { "paragraph" } else { "paragraphs" };
                write!(f, "TextBox({n} {word})")
            }
            Self::Hyperlink { text, url } => {
                let preview: String =
                    if text.len() > 30 { text.chars().take(30).collect() } else { text.clone() };
                write!(f, "Hyperlink(\"{preview}\" -> {url})")
            }
            Self::Footnote { paragraphs, .. } => {
                let n = paragraphs.len();
                let word = if n == 1 { "paragraph" } else { "paragraphs" };
                write!(f, "Footnote({n} {word})")
            }
            Self::Endnote { paragraphs, .. } => {
                let n = paragraphs.len();
                let word = if n == 1 { "paragraph" } else { "paragraphs" };
                write!(f, "Endnote({n} {word})")
            }
            Self::Line { .. } => {
                write!(f, "Line")
            }
            Self::Ellipse { paragraphs, .. } => {
                let n = paragraphs.len();
                let word = if n == 1 { "paragraph" } else { "paragraphs" };
                write!(f, "Ellipse({n} {word})")
            }
            Self::Polygon { vertices, paragraphs, .. } => {
                let nv = vertices.len();
                let np = paragraphs.len();
                let vw = if nv == 1 { "vertex" } else { "vertices" };
                let pw = if np == 1 { "paragraph" } else { "paragraphs" };
                write!(f, "Polygon({nv} {vw}, {np} {pw})")
            }
            Self::Chart { chart_type, data, .. } => {
                let series_count = match data {
                    ChartData::Category { series, .. } => series.len(),
                    ChartData::Xy { series } => series.len(),
                };
                write!(f, "Chart({chart_type:?}, {series_count} series)")
            }
            Self::Equation { script, .. } => {
                let preview: String = if script.len() > 30 {
                    script.chars().take(30).collect()
                } else {
                    script.clone()
                };
                write!(f, "Equation(\"{preview}\")")
            }
            Self::Unknown { tag, .. } => {
                write!(f, "Unknown({tag})")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::Run;
    use hwpforge_foundation::{CharShapeIndex, Color, ParaShapeIndex};

    fn simple_paragraph() -> Paragraph {
        Paragraph::with_runs(
            vec![Run::text("footnote text", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )
    }

    #[test]
    fn shape_style_default_all_none() {
        let s = ShapeStyle::default();
        assert!(s.line_color.is_none());
        assert!(s.fill_color.is_none());
        assert!(s.line_width.is_none());
        assert!(s.line_style.is_none());
    }

    #[test]
    fn shape_style_with_typed_fields() {
        let s = ShapeStyle {
            line_color: Some(Color::from_rgb(255, 0, 0)),
            fill_color: Some(Color::from_rgb(0, 255, 0)),
            line_width: Some(100),
            line_style: Some(LineStyle::Dash),
        };
        assert_eq!(s.line_color.unwrap(), Color::from_rgb(255, 0, 0));
        assert_eq!(s.fill_color.unwrap(), Color::from_rgb(0, 255, 0));
        assert_eq!(s.line_width.unwrap(), 100);
        assert_eq!(s.line_style.unwrap(), LineStyle::Dash);
    }

    #[test]
    fn line_style_default() {
        assert_eq!(LineStyle::default(), LineStyle::Solid);
    }

    #[test]
    fn line_style_display() {
        assert_eq!(LineStyle::Solid.to_string(), "SOLID");
        assert_eq!(LineStyle::Dash.to_string(), "DASH");
        assert_eq!(LineStyle::Dot.to_string(), "DOT");
        assert_eq!(LineStyle::DashDot.to_string(), "DASH_DOT");
        assert_eq!(LineStyle::DashDotDot.to_string(), "DASH_DOT_DOT");
        assert_eq!(LineStyle::None.to_string(), "NONE");
    }

    #[test]
    fn line_style_from_str() {
        assert_eq!("SOLID".parse::<LineStyle>().unwrap(), LineStyle::Solid);
        assert_eq!("Dash".parse::<LineStyle>().unwrap(), LineStyle::Dash);
        assert_eq!("dot".parse::<LineStyle>().unwrap(), LineStyle::Dot);
        assert_eq!("DASH_DOT".parse::<LineStyle>().unwrap(), LineStyle::DashDot);
        assert_eq!("DashDotDot".parse::<LineStyle>().unwrap(), LineStyle::DashDotDot);
        assert_eq!("NONE".parse::<LineStyle>().unwrap(), LineStyle::None);
        assert!("INVALID".parse::<LineStyle>().is_err());
    }

    #[test]
    fn line_style_serde_roundtrip() {
        for style in [
            LineStyle::Solid,
            LineStyle::Dash,
            LineStyle::Dot,
            LineStyle::DashDot,
            LineStyle::DashDotDot,
            LineStyle::None,
        ] {
            let json = serde_json::to_string(&style).unwrap();
            let back: LineStyle = serde_json::from_str(&json).unwrap();
            assert_eq!(style, back);
        }
    }

    #[test]
    fn text_box_construction() {
        let ctrl = Control::TextBox {
            paragraphs: vec![simple_paragraph()],
            width: HwpUnit::from_mm(80.0).unwrap(),
            height: HwpUnit::from_mm(40.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        assert!(ctrl.is_text_box());
        assert!(!ctrl.is_hyperlink());
        assert!(!ctrl.is_footnote());
        assert!(!ctrl.is_endnote());
        assert!(!ctrl.is_unknown());
    }

    #[test]
    fn hyperlink_construction() {
        let ctrl = Control::Hyperlink {
            text: "Click".to_string(),
            url: "https://example.com".to_string(),
        };
        assert!(ctrl.is_hyperlink());
        assert!(!ctrl.is_text_box());
    }

    #[test]
    fn footnote_construction() {
        let ctrl = Control::Footnote { inst_id: None, paragraphs: vec![simple_paragraph()] };
        assert!(ctrl.is_footnote());
        assert!(!ctrl.is_text_box());
        assert!(!ctrl.is_endnote());
    }

    #[test]
    fn endnote_construction() {
        let ctrl = Control::Endnote { inst_id: Some(123456), paragraphs: vec![simple_paragraph()] };
        assert!(ctrl.is_endnote());
        assert!(!ctrl.is_footnote());
        assert!(!ctrl.is_text_box());
    }

    #[test]
    fn unknown_construction() {
        let ctrl = Control::Unknown {
            tag: "custom:widget".to_string(),
            data: Some("<data>value</data>".to_string()),
        };
        assert!(ctrl.is_unknown());
    }

    #[test]
    fn unknown_without_data() {
        let ctrl = Control::Unknown { tag: "header".to_string(), data: None };
        assert!(ctrl.is_unknown());
    }

    #[test]
    fn display_text_box() {
        let ctrl = Control::TextBox {
            paragraphs: vec![simple_paragraph(), simple_paragraph()],
            width: HwpUnit::from_mm(80.0).unwrap(),
            height: HwpUnit::from_mm(40.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        assert_eq!(ctrl.to_string(), "TextBox(2 paragraphs)");
    }

    #[test]
    fn display_hyperlink() {
        let ctrl =
            Control::Hyperlink { text: "Short".to_string(), url: "https://x.com".to_string() };
        let s = ctrl.to_string();
        assert!(s.contains("Short"), "display: {s}");
        assert!(s.contains("https://x.com"), "display: {s}");
    }

    #[test]
    fn display_hyperlink_long_text_truncated() {
        let ctrl =
            Control::Hyperlink { text: "A".repeat(100), url: "https://example.com".to_string() };
        let s = ctrl.to_string();
        // Should show first 30 chars
        assert!(s.contains(&"A".repeat(30)), "display: {s}");
        assert!(!s.contains(&"A".repeat(31)), "display: {s}");
    }

    #[test]
    fn display_footnote() {
        let ctrl = Control::Footnote { inst_id: None, paragraphs: vec![simple_paragraph()] };
        assert_eq!(ctrl.to_string(), "Footnote(1 paragraph)");
    }

    #[test]
    fn display_endnote() {
        let ctrl = Control::Endnote { inst_id: Some(999), paragraphs: vec![simple_paragraph()] };
        assert_eq!(ctrl.to_string(), "Endnote(1 paragraph)");
    }

    #[test]
    fn display_unknown() {
        let ctrl = Control::Unknown { tag: "bookmark".to_string(), data: None };
        assert_eq!(ctrl.to_string(), "Unknown(bookmark)");
    }

    #[test]
    fn equality() {
        let a = Control::Hyperlink { text: "A".to_string(), url: "B".to_string() };
        let b = Control::Hyperlink { text: "A".to_string(), url: "B".to_string() };
        let c = Control::Hyperlink { text: "A".to_string(), url: "C".to_string() };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn serde_roundtrip_text_box() {
        let ctrl = Control::TextBox {
            paragraphs: vec![simple_paragraph()],
            width: HwpUnit::from_mm(80.0).unwrap(),
            height: HwpUnit::from_mm(40.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn serde_roundtrip_hyperlink() {
        let ctrl = Control::Hyperlink {
            text: "link text".to_string(),
            url: "https://rust-lang.org".to_string(),
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn serde_roundtrip_footnote() {
        let ctrl = Control::Footnote { inst_id: Some(12345), paragraphs: vec![simple_paragraph()] };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn serde_roundtrip_endnote() {
        let ctrl = Control::Endnote { inst_id: None, paragraphs: vec![simple_paragraph()] };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn serde_roundtrip_unknown() {
        let ctrl = Control::Unknown { tag: "test".to_string(), data: Some("payload".to_string()) };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    // ── Shape variant tests ──────────────────────────────────────

    #[test]
    fn line_construction() {
        let ctrl = Control::Line {
            start: ShapePoint { x: 0, y: 0 },
            end: ShapePoint { x: 1000, y: 500 },
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(25.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        assert!(ctrl.is_line());
        assert!(!ctrl.is_text_box());
        assert!(!ctrl.is_ellipse());
        assert!(!ctrl.is_polygon());
    }

    #[test]
    fn ellipse_construction() {
        let ctrl = Control::Ellipse {
            center: ShapePoint { x: 500, y: 500 },
            axis1: ShapePoint { x: 1000, y: 500 },
            axis2: ShapePoint { x: 500, y: 1000 },
            width: HwpUnit::from_mm(40.0).unwrap(),
            height: HwpUnit::from_mm(30.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        };
        assert!(ctrl.is_ellipse());
        assert!(!ctrl.is_line());
        assert!(!ctrl.is_polygon());
    }

    #[test]
    fn ellipse_with_paragraphs() {
        let ctrl = Control::Ellipse {
            center: ShapePoint { x: 500, y: 500 },
            axis1: ShapePoint { x: 1000, y: 500 },
            axis2: ShapePoint { x: 500, y: 1000 },
            width: HwpUnit::from_mm(40.0).unwrap(),
            height: HwpUnit::from_mm(30.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![simple_paragraph()],
            caption: None,
            style: None,
        };
        assert!(ctrl.is_ellipse());
        assert_eq!(ctrl.to_string(), "Ellipse(1 paragraph)");
    }

    #[test]
    fn polygon_construction() {
        let ctrl = Control::Polygon {
            vertices: vec![
                ShapePoint { x: 0, y: 0 },
                ShapePoint { x: 1000, y: 0 },
                ShapePoint { x: 500, y: 1000 },
            ],
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(50.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        };
        assert!(ctrl.is_polygon());
        assert!(!ctrl.is_line());
        assert!(!ctrl.is_ellipse());
        assert_eq!(ctrl.to_string(), "Polygon(3 vertices, 0 paragraphs)");
    }

    #[test]
    fn display_line() {
        let ctrl = Control::Line {
            start: ShapePoint { x: 0, y: 0 },
            end: ShapePoint { x: 100, y: 200 },
            width: HwpUnit::from_mm(10.0).unwrap(),
            height: HwpUnit::from_mm(5.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        assert_eq!(ctrl.to_string(), "Line");
    }

    #[test]
    fn serde_roundtrip_line() {
        let ctrl = Control::Line {
            start: ShapePoint { x: 100, y: 200 },
            end: ShapePoint { x: 300, y: 400 },
            width: HwpUnit::from_mm(20.0).unwrap(),
            height: HwpUnit::from_mm(10.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn serde_roundtrip_ellipse() {
        let ctrl = Control::Ellipse {
            center: ShapePoint { x: 500, y: 500 },
            axis1: ShapePoint { x: 1000, y: 500 },
            axis2: ShapePoint { x: 500, y: 1000 },
            width: HwpUnit::from_mm(40.0).unwrap(),
            height: HwpUnit::from_mm(30.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![simple_paragraph()],
            caption: None,
            style: None,
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn serde_roundtrip_polygon() {
        let ctrl = Control::Polygon {
            vertices: vec![
                ShapePoint { x: 0, y: 0 },
                ShapePoint { x: 1000, y: 0 },
                ShapePoint { x: 500, y: 1000 },
            ],
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(50.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn shape_point_equality() {
        let a = ShapePoint { x: 10, y: 20 };
        let b = ShapePoint { x: 10, y: 20 };
        let c = ShapePoint { x: 10, y: 30 };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn shape_point_new() {
        let pt = ShapePoint::new(100, 200);
        assert_eq!(pt.x, 100);
        assert_eq!(pt.y, 200);
    }

    #[test]
    fn shape_point_serde_roundtrip() {
        let pt = ShapePoint::new(500, 750);
        let json = serde_json::to_string(&pt).unwrap();
        let back: ShapePoint = serde_json::from_str(&json).unwrap();
        assert_eq!(pt, back);
    }

    // ── Convenience constructor tests ────────────────────────────────────

    #[test]
    fn equation_constructor_defaults() {
        let ctrl = Control::equation("{a+b} over {c+d}");
        assert!(ctrl.is_equation());
        match ctrl {
            Control::Equation { script, width, height, base_line, text_color, ref font } => {
                assert_eq!(script, "{a+b} over {c+d}");
                assert_eq!(width, HwpUnit::new(8779).unwrap());
                assert_eq!(height, HwpUnit::new(2600).unwrap());
                assert_eq!(base_line, 71);
                assert_eq!(text_color, Color::BLACK);
                assert_eq!(font, "HancomEQN");
            }
            _ => panic!("expected Equation"),
        }
    }

    #[test]
    fn equation_constructor_empty_script() {
        let ctrl = Control::equation("");
        assert!(ctrl.is_equation());
    }

    #[test]
    fn text_box_constructor_defaults() {
        let width = HwpUnit::from_mm(80.0).unwrap();
        let height = HwpUnit::from_mm(40.0).unwrap();
        let ctrl = Control::text_box(vec![simple_paragraph()], width, height);
        assert!(ctrl.is_text_box());
        match ctrl {
            Control::TextBox { paragraphs, horz_offset, vert_offset, caption, style, .. } => {
                assert_eq!(paragraphs.len(), 1);
                assert_eq!(horz_offset, 0);
                assert_eq!(vert_offset, 0);
                assert!(caption.is_none());
                assert!(style.is_none());
            }
            _ => panic!("expected TextBox"),
        }
    }

    #[test]
    fn footnote_constructor_defaults() {
        let ctrl = Control::footnote(vec![simple_paragraph()]);
        assert!(ctrl.is_footnote());
        match ctrl {
            Control::Footnote { inst_id, paragraphs } => {
                assert!(inst_id.is_none());
                assert_eq!(paragraphs.len(), 1);
            }
            _ => panic!("expected Footnote"),
        }
    }

    #[test]
    fn endnote_constructor_defaults() {
        let ctrl = Control::endnote(vec![simple_paragraph()]);
        assert!(ctrl.is_endnote());
        match ctrl {
            Control::Endnote { inst_id, paragraphs } => {
                assert!(inst_id.is_none());
                assert_eq!(paragraphs.len(), 1);
            }
            _ => panic!("expected Endnote"),
        }
    }

    #[test]
    fn ellipse_constructor_geometry() {
        let width = HwpUnit::from_mm(40.0).unwrap();
        let height = HwpUnit::from_mm(30.0).unwrap();
        let ctrl = Control::ellipse(width, height);
        assert!(ctrl.is_ellipse());
        match &ctrl {
            Control::Ellipse {
                center,
                axis1,
                axis2,
                horz_offset,
                vert_offset,
                paragraphs,
                caption,
                style,
                ..
            } => {
                let w = width.as_i32();
                let h = height.as_i32();
                assert_eq!(*center, ShapePoint::new(w / 2, h / 2));
                assert_eq!(*axis1, ShapePoint::new(w, h / 2));
                assert_eq!(*axis2, ShapePoint::new(w / 2, h));
                assert_eq!(*horz_offset, 0);
                assert_eq!(*vert_offset, 0);
                assert!(paragraphs.is_empty());
                assert!(caption.is_none());
                assert!(style.is_none());
            }
            _ => panic!("expected Ellipse"),
        }
    }

    #[test]
    fn polygon_constructor_triangle() {
        let vertices =
            vec![ShapePoint::new(0, 1000), ShapePoint::new(500, 0), ShapePoint::new(1000, 1000)];
        let ctrl = Control::polygon(vertices).unwrap();
        assert!(ctrl.is_polygon());
        match &ctrl {
            Control::Polygon {
                vertices,
                width,
                height,
                horz_offset,
                vert_offset,
                paragraphs,
                caption,
                style,
            } => {
                assert_eq!(vertices.len(), 3);
                // bbox: x 0..1000, y 0..1000
                assert_eq!(*width, HwpUnit::new(1000).unwrap());
                assert_eq!(*height, HwpUnit::new(1000).unwrap());
                assert_eq!(*horz_offset, 0);
                assert_eq!(*vert_offset, 0);
                assert!(paragraphs.is_empty());
                assert!(caption.is_none());
                assert!(style.is_none());
            }
            _ => panic!("expected Polygon"),
        }
    }

    #[test]
    fn polygon_constructor_fewer_than_3_vertices_errors() {
        assert!(Control::polygon(vec![]).is_err());
        assert!(Control::polygon(vec![ShapePoint::new(0, 0)]).is_err());
        assert!(Control::polygon(vec![ShapePoint::new(0, 0), ShapePoint::new(1, 1)]).is_err());
    }

    #[test]
    fn polygon_constructor_negative_coordinates() {
        let vertices =
            vec![ShapePoint::new(-500, -500), ShapePoint::new(500, -500), ShapePoint::new(0, 500)];
        let ctrl = Control::polygon(vertices).unwrap();
        assert!(ctrl.is_polygon());
        match ctrl {
            Control::Polygon { width, height, .. } => {
                // bbox: x -500..500 = 1000, y -500..500 = 1000
                assert_eq!(width, HwpUnit::new(1000).unwrap());
                assert_eq!(height, HwpUnit::new(1000).unwrap());
            }
            _ => panic!("expected Polygon"),
        }
    }

    #[test]
    fn polygon_constructor_degenerate_collinear() {
        // 3 collinear points: height = 0 (flat), should succeed
        let vertices =
            vec![ShapePoint::new(0, 0), ShapePoint::new(500, 0), ShapePoint::new(1000, 0)];
        let ctrl = Control::polygon(vertices).unwrap();
        assert!(ctrl.is_polygon());
        match ctrl {
            Control::Polygon { width, height, .. } => {
                assert_eq!(width, HwpUnit::new(1000).unwrap());
                assert_eq!(height, HwpUnit::new(0).unwrap());
            }
            _ => panic!("expected Polygon"),
        }
    }

    #[test]
    fn line_constructor_horizontal() {
        let ctrl = Control::line(ShapePoint::new(0, 0), ShapePoint::new(5000, 0)).unwrap();
        assert!(ctrl.is_line());
        match ctrl {
            Control::Line {
                start,
                end,
                width,
                height,
                horz_offset,
                vert_offset,
                caption,
                style,
            } => {
                assert_eq!(start, ShapePoint::new(0, 0));
                assert_eq!(end, ShapePoint::new(5000, 0));
                assert_eq!(width, HwpUnit::new(5000).unwrap());
                assert_eq!(height, HwpUnit::new(100).unwrap()); // min bounding box
                assert_eq!(horz_offset, 0);
                assert_eq!(vert_offset, 0);
                assert!(caption.is_none());
                assert!(style.is_none());
            }
            _ => panic!("expected Line"),
        }
    }

    #[test]
    fn line_constructor_vertical() {
        let ctrl = Control::line(ShapePoint::new(0, 0), ShapePoint::new(0, 3000)).unwrap();
        assert!(ctrl.is_line());
        match ctrl {
            Control::Line { width, height, .. } => {
                assert_eq!(width, HwpUnit::new(100).unwrap()); // min bounding box
                assert_eq!(height, HwpUnit::new(3000).unwrap());
            }
            _ => panic!("expected Line"),
        }
    }

    #[test]
    fn line_constructor_diagonal_bounding_box() {
        let ctrl = Control::line(ShapePoint::new(100, 200), ShapePoint::new(400, 500)).unwrap();
        match ctrl {
            Control::Line { width, height, .. } => {
                assert_eq!(width, HwpUnit::new(300).unwrap());
                assert_eq!(height, HwpUnit::new(300).unwrap());
            }
            _ => panic!("expected Line"),
        }
    }

    #[test]
    fn line_constructor_same_point_errors() {
        let pt = ShapePoint::new(100, 200);
        assert!(Control::line(pt, pt).is_err());
    }

    #[test]
    fn horizontal_line_constructor() {
        let width = HwpUnit::from_mm(100.0).unwrap();
        let ctrl = Control::horizontal_line(width);
        assert!(ctrl.is_line());
        match ctrl {
            Control::Line {
                start,
                end,
                width: w,
                height,
                horz_offset,
                vert_offset,
                caption,
                style,
            } => {
                assert_eq!(start, ShapePoint::new(0, 0));
                assert_eq!(end.y, 0);
                assert_eq!(end.x, width.as_i32());
                assert_eq!(w, width);
                assert_eq!(height, HwpUnit::new(100).unwrap()); // min bounding box
                assert_eq!(horz_offset, 0);
                assert_eq!(vert_offset, 0);
                assert!(caption.is_none());
                assert!(style.is_none());
            }
            _ => panic!("expected Line"),
        }
    }

    #[test]
    fn hyperlink_constructor() {
        let ctrl = Control::hyperlink("Visit Rust", "https://rust-lang.org");
        assert!(ctrl.is_hyperlink());
        match ctrl {
            Control::Hyperlink { text, url } => {
                assert_eq!(text, "Visit Rust");
                assert_eq!(url, "https://rust-lang.org");
            }
            _ => panic!("expected Hyperlink"),
        }
    }

    #[test]
    fn footnote_with_id_sets_inst_id() {
        let para = Paragraph::new(ParaShapeIndex::new(0));
        let ctrl = Control::footnote_with_id(42, vec![para]);
        assert!(ctrl.is_footnote());
        match ctrl {
            Control::Footnote { inst_id, paragraphs } => {
                assert_eq!(inst_id, Some(42));
                assert_eq!(paragraphs.len(), 1);
            }
            _ => panic!("expected Footnote"),
        }
    }

    #[test]
    fn endnote_with_id_sets_inst_id() {
        let para = Paragraph::new(ParaShapeIndex::new(0));
        let ctrl = Control::endnote_with_id(7, vec![para]);
        assert!(ctrl.is_endnote());
        match ctrl {
            Control::Endnote { inst_id, paragraphs } => {
                assert_eq!(inst_id, Some(7));
                assert_eq!(paragraphs.len(), 1);
            }
            _ => panic!("expected Endnote"),
        }
    }

    #[test]
    fn footnote_with_id_differs_from_plain_footnote() {
        let ctrl_plain = Control::footnote(vec![]);
        let ctrl_id = Control::footnote_with_id(1, vec![]);
        match ctrl_plain {
            Control::Footnote { inst_id, .. } => assert_eq!(inst_id, None),
            _ => panic!("expected Footnote"),
        }
        match ctrl_id {
            Control::Footnote { inst_id, .. } => assert_eq!(inst_id, Some(1)),
            _ => panic!("expected Footnote"),
        }
    }

    #[test]
    fn ellipse_with_text_has_correct_geometry_and_paragraphs() {
        use hwpforge_foundation::HwpUnit;
        let width = HwpUnit::from_mm(40.0).unwrap();
        let height = HwpUnit::from_mm(30.0).unwrap();
        let para = Paragraph::new(ParaShapeIndex::new(0));
        let ctrl = Control::ellipse_with_text(width, height, vec![para]);
        assert!(ctrl.is_ellipse());
        match ctrl {
            Control::Ellipse {
                center,
                axis1,
                axis2,
                width: w,
                height: h,
                horz_offset,
                vert_offset,
                paragraphs,
                caption,
                style,
            } => {
                let wv = w.as_i32();
                let hv = h.as_i32();
                assert_eq!(center, ShapePoint::new(wv / 2, hv / 2));
                assert_eq!(axis1, ShapePoint::new(wv, hv / 2));
                assert_eq!(axis2, ShapePoint::new(wv / 2, hv));
                assert_eq!(horz_offset, 0);
                assert_eq!(vert_offset, 0);
                assert_eq!(paragraphs.len(), 1);
                assert!(caption.is_none());
                assert!(style.is_none());
            }
            _ => panic!("expected Ellipse"),
        }
    }

    #[test]
    fn serde_roundtrip_chart() {
        use crate::chart::{ChartData, ChartGrouping, ChartType, LegendPosition};
        let ctrl = Control::Chart {
            chart_type: ChartType::Column,
            data: ChartData::category(&["A", "B"], &[("S1", &[1.0, 2.0])]),
            title: Some("Test Chart".to_string()),
            legend: LegendPosition::Bottom,
            grouping: ChartGrouping::Stacked,
            width: HwpUnit::from_mm(100.0).unwrap(),
            height: HwpUnit::from_mm(80.0).unwrap(),
            stock_variant: None,
            bar_shape: None,
            scatter_style: None,
            radar_style: None,
            of_pie_type: None,
            explosion: None,
            wireframe: None,
            bubble_3d: None,
            show_markers: None,
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn serde_roundtrip_equation() {
        let ctrl = Control::Equation {
            script: "{a+b} over {c+d}".to_string(),
            width: HwpUnit::new(8779).unwrap(),
            height: HwpUnit::new(2600).unwrap(),
            base_line: 71,
            text_color: Color::BLACK,
            font: "HancomEQN".to_string(),
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn ellipse_with_text_empty_paragraphs_matches_ellipse() {
        use hwpforge_foundation::HwpUnit;
        let width = HwpUnit::from_mm(20.0).unwrap();
        let height = HwpUnit::from_mm(10.0).unwrap();
        let plain = Control::ellipse(width, height);
        let with_text = Control::ellipse_with_text(width, height, vec![]);
        // Both should produce identical shapes when paragraphs are empty
        assert_eq!(plain, with_text);
    }
}
