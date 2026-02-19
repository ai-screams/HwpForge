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

use hwpforge_foundation::HwpUnit;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::caption::Caption;
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

/// Visual style overrides for drawing shapes.
///
/// All fields are `Option`; `None` means "use the encoder's default"
/// (typically black solid border, white fill, 0.12 mm stroke).
///
/// Colors are `#RRGGBB` hex strings matching the HWPX XML format directly.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::ShapeStyle;
///
/// let style = ShapeStyle {
///     line_color: Some("#FF0000".to_string()),
///     fill_color: Some("#00FF00".to_string()),
///     line_width: Some(100),
///     line_style: Some("DASH".to_string()),
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ShapeStyle {
    /// Stroke/border color as `#RRGGBB` (e.g. `"#FF0000"` for red).
    pub line_color: Option<String>,
    /// Fill color as `#RRGGBB` (e.g. `"#00FF00"` for green).
    pub fill_color: Option<String>,
    /// Stroke width in HWPUNIT (33 ≈ 0.12mm, 100 ≈ 0.35mm).
    pub line_width: Option<i32>,
    /// Line style: `"SOLID"`, `"DASH"`, `"DOT"`, `"DASH_DOT"`, etc.
    pub line_style: Option<String>,
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
    // TODO(phase9): Add horz_offset/vert_offset for non-inline positioning
    Line {
        /// Start point (x, y in HWPUNIT).
        start: ShapePoint,
        /// End point (x, y in HWPUNIT).
        end: ShapePoint,
        /// Bounding box width (HWPUNIT).
        width: HwpUnit,
        /// Bounding box height (HWPUNIT).
        height: HwpUnit,
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
    // TODO(phase9): Add horz_offset/vert_offset for non-inline positioning
    Polygon {
        /// Ordered list of vertices (minimum 3).
        vertices: Vec<ShapePoint>,
        /// Bounding box width (HWPUNIT).
        width: HwpUnit,
        /// Bounding box height (HWPUNIT).
        height: HwpUnit,
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
        /// Text color as `#RRGGBB`.
        text_color: String,
        /// Font name (typically `"HancomEQN"`).
        font: String,
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

    /// Returns `true` if this is a [`Control::Unknown`].
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }
}

impl std::fmt::Display for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TextBox { paragraphs, .. } => {
                write!(f, "TextBox({} paragraphs)", paragraphs.len())
            }
            Self::Hyperlink { text, url } => {
                let preview: String =
                    if text.len() > 30 { text.chars().take(30).collect() } else { text.clone() };
                write!(f, "Hyperlink(\"{preview}\" -> {url})")
            }
            Self::Footnote { paragraphs, .. } => {
                write!(f, "Footnote({} paragraphs)", paragraphs.len())
            }
            Self::Endnote { paragraphs, .. } => {
                write!(f, "Endnote({} paragraphs)", paragraphs.len())
            }
            Self::Line { .. } => {
                write!(f, "Line")
            }
            Self::Ellipse { paragraphs, .. } => {
                write!(f, "Ellipse({} paragraphs)", paragraphs.len())
            }
            Self::Polygon { vertices, paragraphs, .. } => {
                write!(f, "Polygon({} vertices, {} paragraphs)", vertices.len(), paragraphs.len())
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
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

    fn simple_paragraph() -> Paragraph {
        Paragraph::with_runs(
            vec![Run::text("footnote text", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )
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
        assert_eq!(ctrl.to_string(), "Footnote(1 paragraphs)");
    }

    #[test]
    fn display_endnote() {
        let ctrl = Control::Endnote { inst_id: Some(999), paragraphs: vec![simple_paragraph()] };
        assert_eq!(ctrl.to_string(), "Endnote(1 paragraphs)");
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
        assert_eq!(ctrl.to_string(), "Ellipse(1 paragraphs)");
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
}
