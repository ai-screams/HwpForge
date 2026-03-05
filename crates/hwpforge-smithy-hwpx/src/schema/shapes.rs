//! XML schema types for shape drawing objects in HWPX section XML.
//!
//! These types are split from `section.rs` to enable parallel development.
//! They map shape-related elements (`<hp:rect>`, `<hp:line>`, `<hp:ellipse>`,
//! `<hp:polygon>`) and their common sub-elements into Rust structs via serde.
//!
//! Fields are used by serde deserialization even if not directly accessed.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

use super::section::{
    HxCaption, HxFlip, HxOffset, HxPoint, HxRenderingInfo, HxRotationInfo, HxSizeAttr, HxSubList,
    HxTableMargin, HxTablePos, HxTableSz,
};

// ── Shape-common sub-elements ────────────────────────────────────

/// `<hp:lineShape>` — stroke style for drawing shapes (ellipse, rect, polygon, line).
///
/// All 12 attributes are required by 한글. Use [`HxLineShape::default_solid`] for
/// a standard thin black border.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxLineShape {
    /// Stroke color as `#RRGGBB`.
    #[serde(rename = "@color", default)]
    pub color: String,
    /// Stroke width in HWPUNIT (33 ≈ 0.12mm, the standard thin border).
    #[serde(rename = "@width", default)]
    pub width: i32,
    /// Line style: SOLID, DASH, DOT, etc.
    #[serde(rename = "@style", default)]
    pub style: String,
    /// End cap style: FLAT, ROUND, SQUARE.
    #[serde(rename = "@endCap", default)]
    pub end_cap: String,
    /// Arrowhead style at start: NORMAL, OPEN, etc.
    #[serde(rename = "@headStyle", default)]
    pub head_style: String,
    /// Arrowhead style at end: NORMAL, OPEN, etc.
    #[serde(rename = "@tailStyle", default)]
    pub tail_style: String,
    /// Whether arrowhead at start is filled (0 or 1).
    #[serde(rename = "@headfill", default)]
    pub head_fill: u32,
    /// Whether arrowhead at end is filled (0 or 1).
    #[serde(rename = "@tailfill", default)]
    pub tail_fill: u32,
    /// Arrowhead size at start: SMALL_SMALL, MEDIUM_MEDIUM, LARGE_LARGE, etc.
    #[serde(rename = "@headSz", default)]
    pub head_sz: String,
    /// Arrowhead size at end.
    #[serde(rename = "@tailSz", default)]
    pub tail_sz: String,
    /// Outline style: NORMAL, OUTER, INNER.
    #[serde(rename = "@outlineStyle", default)]
    pub outline_style: String,
    /// Alpha transparency (0 = opaque).
    #[serde(rename = "@alpha", default)]
    pub alpha: i32,
}

impl HxLineShape {
    /// Creates a standard thin solid black border (matches 한글 default).
    pub fn default_solid() -> Self {
        Self {
            color: "#000000".to_string(),
            width: 33,
            style: "SOLID".to_string(),
            end_cap: "FLAT".to_string(),
            head_style: "NORMAL".to_string(),
            tail_style: "NORMAL".to_string(),
            head_fill: 1,
            tail_fill: 1,
            head_sz: "MEDIUM_MEDIUM".to_string(),
            tail_sz: "MEDIUM_MEDIUM".to_string(),
            outline_style: "NORMAL".to_string(),
            alpha: 0,
        }
    }
}

/// `<hc:winBrush>` — solid or hatch fill brush (core `hc:` namespace).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxWinBrush {
    /// Fill face color as `#RRGGBB`.
    #[serde(rename = "@faceColor", default)]
    pub face_color: String,
    /// Hatch pattern color as `#RRGGBB`.
    #[serde(rename = "@hatchColor", default)]
    pub hatch_color: String,
    /// Alpha transparency (0 = opaque).
    #[serde(rename = "@alpha", default)]
    pub alpha: i32,
}

/// `<hc:fillBrush>` — fill brush container (core `hc:` namespace).
///
/// Contains a `<hc:winBrush>` child. Use [`HxFillBrush::default_white`] for
/// a standard white fill (the default for shapes in 한글).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxFillBrush {
    /// Solid/hatch fill brush.
    #[serde(rename(serialize = "hc:winBrush", deserialize = "winBrush"))]
    pub win_brush: HxWinBrush,
}

impl HxFillBrush {
    /// Creates a standard white fill brush (matches 한글 default for shapes).
    pub fn default_white() -> Self {
        Self {
            win_brush: HxWinBrush {
                face_color: "#FFFFFF".to_string(),
                hatch_color: "#000000".to_string(),
                alpha: 0,
            },
        }
    }
}

/// `<hp:shadow>` — drop shadow properties for drawing shapes.
///
/// Use [`HxShadow::default_none`] for no shadow (the standard default).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxShadow {
    /// Shadow type: NONE, DROP, etc.
    #[serde(rename = "@type", default)]
    pub shadow_type: String,
    /// Shadow color as `#RRGGBB`.
    #[serde(rename = "@color", default)]
    pub color: String,
    /// Horizontal shadow offset in HWPUNIT.
    #[serde(rename = "@offsetX", default)]
    pub offset_x: i32,
    /// Vertical shadow offset in HWPUNIT.
    #[serde(rename = "@offsetY", default)]
    pub offset_y: i32,
    /// Alpha transparency (0 = opaque).
    #[serde(rename = "@alpha", default)]
    pub alpha: i32,
}

impl HxShadow {
    /// Creates a no-shadow default (matches 한글 default for shapes).
    pub fn default_none() -> Self {
        Self {
            shadow_type: "NONE".to_string(),
            color: "#B2B2B2".to_string(),
            offset_x: 0,
            offset_y: 0,
            alpha: 0,
        }
    }
}

/// `<hp:shapeComment>` — optional text description of a shape.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxShapeComment {
    /// The comment text content.
    #[serde(rename = "$text", default)]
    pub text: String,
}

// ── Rectangle / TextBox ──────────────────────────────────────────

/// `<hp:rect>` — rectangle drawing object (can contain textbox content via `<hp:drawText>`).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxRect {
    // ── AbstractShapeObjectType attributes ──
    /// Element ID.
    #[serde(rename = "@id", default)]
    pub id: String,
    /// Z-order for overlapping objects.
    #[serde(rename = "@zOrder", default)]
    pub z_order: u32,
    /// Numbering type: NONE, TABLE, FIGURE, EQUATION.
    #[serde(rename = "@numberingType", default)]
    pub numbering_type: String,
    /// Text wrapping mode: TOP_AND_BOTTOM, SQUARE, TIGHT, etc.
    #[serde(rename = "@textWrap", default)]
    pub text_wrap: String,
    /// Text flow mode: BOTH_SIDES, LEFT_ONLY, RIGHT_ONLY, etc.
    #[serde(rename = "@textFlow", default)]
    pub text_flow: String,
    /// Lock flag (0 = unlocked).
    #[serde(rename = "@lock", default)]
    pub lock: u32,
    /// Drop cap style: None, Normal, etc.
    #[serde(rename = "@dropcapstyle", default)]
    pub dropcap_style: String,

    // ── AbstractShapeComponentType attributes ──
    /// Hyperlink reference.
    #[serde(rename = "@href", default)]
    pub href: String,
    /// Group nesting level.
    #[serde(rename = "@groupLevel", default)]
    pub group_level: u32,
    /// Instance identifier (unique within document).
    #[serde(rename = "@instid", default)]
    pub instid: String,

    // ── Rectangle-specific ──
    /// Corner rounding ratio (0 = sharp, 50 = max rounding).
    #[serde(rename = "@ratio", default)]
    pub ratio: u8,

    // ── Shape-common children (ORDER MATTERS for serialization!) ──
    /// Position offset (required by 한글).
    #[serde(
        rename(serialize = "hp:offset", deserialize = "offset"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub offset: Option<HxOffset>,
    /// Original size before scaling (required by 한글).
    #[serde(
        rename(serialize = "hp:orgSz", deserialize = "orgSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub org_sz: Option<HxSizeAttr>,
    /// Current display size (required by 한글, usually 0×0).
    #[serde(
        rename(serialize = "hp:curSz", deserialize = "curSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cur_sz: Option<HxSizeAttr>,
    /// Flip state (required by 한글).
    #[serde(
        rename(serialize = "hp:flip", deserialize = "flip"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub flip: Option<HxFlip>,
    /// Rotation information (required by 한글).
    #[serde(
        rename(serialize = "hp:rotationInfo", deserialize = "rotationInfo"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rotation_info: Option<HxRotationInfo>,
    /// Rendering transformation matrices (required by 한글).
    #[serde(
        rename(serialize = "hp:renderingInfo", deserialize = "renderingInfo"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rendering_info: Option<HxRenderingInfo>,
    /// Stroke style (required by 한글).
    #[serde(
        rename(serialize = "hp:lineShape", deserialize = "lineShape"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub line_shape: Option<HxLineShape>,
    /// Fill brush (hc: namespace, required by 한글).
    #[serde(
        rename(serialize = "hc:fillBrush", deserialize = "fillBrush"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub fill_brush: Option<HxFillBrush>,
    /// Drop shadow (required by 한글).
    #[serde(
        rename(serialize = "hp:shadow", deserialize = "shadow"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shadow: Option<HxShadow>,

    /// Textbox content (if present, this rect is a textbox).
    #[serde(
        rename(serialize = "hp:drawText", deserialize = "drawText"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub draw_text: Option<HxDrawText>,

    /// Optional caption attached to this rectangle.
    #[serde(
        rename(serialize = "hp:caption", deserialize = "caption"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub caption: Option<HxCaption>,

    // ── Rectangle corner points (hc: namespace per KS X 6101) ──
    /// Rectangle corner point 0 (top-left).
    #[serde(
        rename(serialize = "hc:pt0", deserialize = "pt0"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pt0: Option<HxPoint>,
    /// Rectangle corner point 1 (top-right).
    #[serde(
        rename(serialize = "hc:pt1", deserialize = "pt1"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pt1: Option<HxPoint>,
    /// Rectangle corner point 2 (bottom-right).
    #[serde(
        rename(serialize = "hc:pt2", deserialize = "pt2"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pt2: Option<HxPoint>,
    /// Rectangle corner point 3 (bottom-left).
    #[serde(
        rename(serialize = "hc:pt3", deserialize = "pt3"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pt3: Option<HxPoint>,

    // ── Size / position / margin ──
    /// Size specification (width, height).
    #[serde(
        rename(serialize = "hp:sz", deserialize = "sz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sz: Option<HxTableSz>,

    /// Position specification.
    #[serde(
        rename(serialize = "hp:pos", deserialize = "pos"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pos: Option<HxTablePos>,

    /// Outer margin.
    #[serde(
        rename(serialize = "hp:outMargin", deserialize = "outMargin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub out_margin: Option<HxTableMargin>,

    /// Shape description comment (e.g. "사각형입니다.").
    #[serde(
        rename(serialize = "hp:shapeComment", deserialize = "shapeComment"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shape_comment: Option<HxShapeComment>,
}

/// `<hp:drawText>` — textbox content container (paragraphs + text margin).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxDrawText {
    /// Maximum text width in HWPUNIT (typically width - left_margin - right_margin).
    #[serde(rename = "@lastWidth", default)]
    pub last_width: u32,

    /// Textbox name (usually empty).
    #[serde(rename = "@name", default)]
    pub name: String,

    /// Whether textbox is editable (0 = readonly, 1 = editable).
    #[serde(rename = "@editable", default)]
    pub editable: u32,

    /// Paragraph content (required).
    #[serde(rename(serialize = "hp:subList", deserialize = "subList"))]
    pub sub_list: HxSubList,

    /// Inner text padding (optional, default ~1mm on all sides).
    #[serde(
        rename(serialize = "hp:textMargin", deserialize = "textMargin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub text_margin: Option<HxTableMargin>,
}

// ── Line shape ───────────────────────────────────────────────────

/// `<hp:line>` — line drawing object (2 endpoints).
///
/// Flat struct (independent of HxRect) per Wave 3 API design decision.
/// Common attributes duplicated from AbstractShapeObjectType / AbstractShapeComponentType.
///
/// Element order matches 한글's expected serialization:
/// offset → orgSz → curSz → flip → rotationInfo → renderingInfo →
/// lineShape → fillBrush → shadow →
/// startPt → endPt → sz → pos → outMargin → shapeComment → caption
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxLine {
    // ── AbstractShapeObjectType attrs ──
    /// Element ID.
    #[serde(rename = "@id", default)]
    pub id: String,
    /// Z-order for overlapping objects.
    #[serde(rename = "@zOrder", default)]
    pub z_order: u32,
    /// Numbering type: NONE, TABLE, FIGURE, EQUATION.
    #[serde(rename = "@numberingType", default)]
    pub numbering_type: String,
    /// Text wrapping mode.
    #[serde(rename = "@textWrap", default)]
    pub text_wrap: String,
    /// Text flow mode.
    #[serde(rename = "@textFlow", default)]
    pub text_flow: String,
    /// Lock flag (0 = unlocked).
    #[serde(rename = "@lock", default)]
    pub lock: u32,
    /// Drop cap style.
    #[serde(rename = "@dropcapstyle", default)]
    pub dropcap_style: String,

    // ── AbstractShapeComponentType attrs ──
    /// Hyperlink reference.
    #[serde(rename = "@href", default)]
    pub href: String,
    /// Group nesting level.
    #[serde(rename = "@groupLevel", default)]
    pub group_level: u32,
    /// Instance identifier.
    #[serde(rename = "@instid", default)]
    pub instid: String,

    // ── Line-specific attr ──
    /// Whether to reverse horizontal/vertical orientation.
    #[serde(rename = "@isReverseHV", default)]
    pub is_reverse_hv: u32,

    // ── Shape-common children (ORDER MATTERS!) ──
    /// Position offset (required by 한글).
    #[serde(
        rename(serialize = "hp:offset", deserialize = "offset"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub offset: Option<HxOffset>,
    /// Original size before scaling (required by 한글).
    #[serde(
        rename(serialize = "hp:orgSz", deserialize = "orgSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub org_sz: Option<HxSizeAttr>,
    /// Current display size (required by 한글, usually 0×0).
    #[serde(
        rename(serialize = "hp:curSz", deserialize = "curSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cur_sz: Option<HxSizeAttr>,
    /// Flip state (required by 한글).
    #[serde(
        rename(serialize = "hp:flip", deserialize = "flip"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub flip: Option<HxFlip>,
    /// Rotation information (required by 한글).
    #[serde(
        rename(serialize = "hp:rotationInfo", deserialize = "rotationInfo"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rotation_info: Option<HxRotationInfo>,
    /// Rendering transformation matrices (required by 한글).
    #[serde(
        rename(serialize = "hp:renderingInfo", deserialize = "renderingInfo"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rendering_info: Option<HxRenderingInfo>,
    /// Stroke style (required by 한글).
    #[serde(
        rename(serialize = "hp:lineShape", deserialize = "lineShape"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub line_shape: Option<HxLineShape>,
    /// Fill brush (hc: namespace, required by 한글).
    #[serde(
        rename(serialize = "hc:fillBrush", deserialize = "fillBrush"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub fill_brush: Option<HxFillBrush>,
    /// Drop shadow (required by 한글).
    #[serde(
        rename(serialize = "hp:shadow", deserialize = "shadow"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shadow: Option<HxShadow>,

    // ── Line-specific children (hc: namespace geometry BEFORE sz/pos) ──
    /// Start point of the line.
    #[serde(
        rename(serialize = "hc:startPt", deserialize = "startPt"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub start_pt: Option<HxPoint>,
    /// End point of the line.
    #[serde(
        rename(serialize = "hc:endPt", deserialize = "endPt"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub end_pt: Option<HxPoint>,

    // ── Size / position / margin ──
    /// Size specification.
    #[serde(
        rename(serialize = "hp:sz", deserialize = "sz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sz: Option<HxTableSz>,
    /// Position specification.
    #[serde(
        rename(serialize = "hp:pos", deserialize = "pos"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pos: Option<HxTablePos>,
    /// Outer margin.
    #[serde(
        rename(serialize = "hp:outMargin", deserialize = "outMargin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub out_margin: Option<HxTableMargin>,

    /// Optional shape description text (e.g. "선입니다.").
    #[serde(
        rename(serialize = "hp:shapeComment", deserialize = "shapeComment"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shape_comment: Option<HxShapeComment>,

    /// Optional caption attached to this line.
    #[serde(
        rename(serialize = "hp:caption", deserialize = "caption"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub caption: Option<HxCaption>,
}

// ── Ellipse shape ────────────────────────────────────────────────

/// `<hp:ellipse>` — ellipse/circle drawing object.
///
/// Flat struct with common attrs duplicated from AbstractShapeObjectType.
///
/// Element order matches 한글's expected serialization:
/// offset → orgSz → curSz → flip → rotationInfo → renderingInfo →
/// lineShape → fillBrush → shadow →
/// center → ax1 → ax2 → start1 → end1 → start2 → end2 →
/// sz → pos → outMargin → caption → drawText
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxEllipse {
    // ── AbstractShapeObjectType attrs ──
    /// Element ID.
    #[serde(rename = "@id", default)]
    pub id: String,
    /// Z-order.
    #[serde(rename = "@zOrder", default)]
    pub z_order: u32,
    /// Numbering type.
    #[serde(rename = "@numberingType", default)]
    pub numbering_type: String,
    /// Text wrapping mode.
    #[serde(rename = "@textWrap", default)]
    pub text_wrap: String,
    /// Text flow mode.
    #[serde(rename = "@textFlow", default)]
    pub text_flow: String,
    /// Lock flag.
    #[serde(rename = "@lock", default)]
    pub lock: u32,
    /// Drop cap style.
    #[serde(rename = "@dropcapstyle", default)]
    pub dropcap_style: String,

    // ── AbstractShapeComponentType attrs ──
    /// Hyperlink reference.
    #[serde(rename = "@href", default)]
    pub href: String,
    /// Group nesting level.
    #[serde(rename = "@groupLevel", default)]
    pub group_level: u32,
    /// Instance identifier.
    #[serde(rename = "@instid", default)]
    pub instid: String,

    // ── Ellipse-specific attrs ──
    /// Interval dirty flag.
    #[serde(rename = "@intervalDirty", default)]
    pub interval_dirty: u32,
    /// Whether this ellipse has arc properties.
    #[serde(rename = "@hasArcPr", default)]
    pub has_arc_pr: u32,
    /// Arc type (NORMAL for full ellipse).
    #[serde(rename = "@arcType", default)]
    pub arc_type: String,

    // ── Shape-common children (ORDER MATTERS!) ──
    /// Position offset (required by 한글).
    #[serde(
        rename(serialize = "hp:offset", deserialize = "offset"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub offset: Option<HxOffset>,
    /// Original size before scaling (required by 한글).
    #[serde(
        rename(serialize = "hp:orgSz", deserialize = "orgSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub org_sz: Option<HxSizeAttr>,
    /// Current display size (required by 한글, usually 0×0).
    #[serde(
        rename(serialize = "hp:curSz", deserialize = "curSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cur_sz: Option<HxSizeAttr>,
    /// Flip state (required by 한글).
    #[serde(
        rename(serialize = "hp:flip", deserialize = "flip"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub flip: Option<HxFlip>,
    /// Rotation information (required by 한글).
    #[serde(
        rename(serialize = "hp:rotationInfo", deserialize = "rotationInfo"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rotation_info: Option<HxRotationInfo>,
    /// Rendering transformation matrices (required by 한글).
    #[serde(
        rename(serialize = "hp:renderingInfo", deserialize = "renderingInfo"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rendering_info: Option<HxRenderingInfo>,
    /// Stroke style (required by 한글).
    #[serde(
        rename(serialize = "hp:lineShape", deserialize = "lineShape"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub line_shape: Option<HxLineShape>,
    /// Fill brush (hc: namespace, required by 한글).
    #[serde(
        rename(serialize = "hc:fillBrush", deserialize = "fillBrush"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub fill_brush: Option<HxFillBrush>,
    /// Drop shadow (required by 한글).
    #[serde(
        rename(serialize = "hp:shadow", deserialize = "shadow"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shadow: Option<HxShadow>,

    // ── Ellipse geometry (hc: namespace per KS X 6101 spec) ──
    /// Center point of the ellipse (hc: namespace).
    #[serde(
        rename(serialize = "hc:center", deserialize = "center"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub center: Option<HxPoint>,
    /// Axis 1 endpoint — semi-major axis (hc: namespace).
    #[serde(
        rename(serialize = "hc:ax1", deserialize = "ax1"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ax1: Option<HxPoint>,
    /// Axis 2 endpoint — semi-minor axis (hc: namespace).
    #[serde(
        rename(serialize = "hc:ax2", deserialize = "ax2"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ax2: Option<HxPoint>,
    /// Arc start point 1 (hc: namespace; zero for full ellipse).
    #[serde(
        rename(serialize = "hc:start1", deserialize = "start1"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub start1: Option<HxPoint>,
    /// Arc end point 1 (hc: namespace; zero for full ellipse).
    #[serde(
        rename(serialize = "hc:end1", deserialize = "end1"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub end1: Option<HxPoint>,
    /// Arc start point 2 (hc: namespace; zero for full ellipse).
    #[serde(
        rename(serialize = "hc:start2", deserialize = "start2"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub start2: Option<HxPoint>,
    /// Arc end point 2 (hc: namespace; zero for full ellipse).
    #[serde(
        rename(serialize = "hc:end2", deserialize = "end2"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub end2: Option<HxPoint>,

    // ── Size / position / margin ──
    /// Size specification.
    #[serde(
        rename(serialize = "hp:sz", deserialize = "sz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sz: Option<HxTableSz>,
    /// Position specification.
    #[serde(
        rename(serialize = "hp:pos", deserialize = "pos"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pos: Option<HxTablePos>,
    /// Outer margin.
    #[serde(
        rename(serialize = "hp:outMargin", deserialize = "outMargin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub out_margin: Option<HxTableMargin>,

    /// Shape description comment (e.g. "타원입니다.").
    #[serde(
        rename(serialize = "hp:shapeComment", deserialize = "shapeComment"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shape_comment: Option<HxShapeComment>,

    /// Optional caption attached to this ellipse.
    #[serde(
        rename(serialize = "hp:caption", deserialize = "caption"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub caption: Option<HxCaption>,

    /// Optional textbox content inside the ellipse.
    #[serde(
        rename(serialize = "hp:drawText", deserialize = "drawText"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub draw_text: Option<HxDrawText>,
}

// ── Polygon shape ────────────────────────────────────────────────

/// `<hp:polygon>` — polygon drawing object (3+ vertices).
///
/// Flat struct with common attrs duplicated from AbstractShapeObjectType.
///
/// Element order matches 한글's expected serialization:
/// offset → orgSz → curSz → flip → rotationInfo → renderingInfo →
/// lineShape → fillBrush → shadow →
/// sz → pos → outMargin → caption → drawText → pt[]
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxPolygon {
    // ── AbstractShapeObjectType attrs ──
    /// Element ID.
    #[serde(rename = "@id", default)]
    pub id: String,
    /// Z-order.
    #[serde(rename = "@zOrder", default)]
    pub z_order: u32,
    /// Numbering type.
    #[serde(rename = "@numberingType", default)]
    pub numbering_type: String,
    /// Text wrapping mode.
    #[serde(rename = "@textWrap", default)]
    pub text_wrap: String,
    /// Text flow mode.
    #[serde(rename = "@textFlow", default)]
    pub text_flow: String,
    /// Lock flag.
    #[serde(rename = "@lock", default)]
    pub lock: u32,
    /// Drop cap style.
    #[serde(rename = "@dropcapstyle", default)]
    pub dropcap_style: String,

    // ── AbstractShapeComponentType attrs ──
    /// Hyperlink reference.
    #[serde(rename = "@href", default)]
    pub href: String,
    /// Group nesting level.
    #[serde(rename = "@groupLevel", default)]
    pub group_level: u32,
    /// Instance identifier.
    #[serde(rename = "@instid", default)]
    pub instid: String,

    // ── Shape-common children (ORDER MATTERS!) ──
    /// Position offset (required by 한글).
    #[serde(
        rename(serialize = "hp:offset", deserialize = "offset"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub offset: Option<HxOffset>,
    /// Original size before scaling (required by 한글).
    #[serde(
        rename(serialize = "hp:orgSz", deserialize = "orgSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub org_sz: Option<HxSizeAttr>,
    /// Current display size (required by 한글, usually 0×0).
    #[serde(
        rename(serialize = "hp:curSz", deserialize = "curSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cur_sz: Option<HxSizeAttr>,
    /// Flip state (required by 한글).
    #[serde(
        rename(serialize = "hp:flip", deserialize = "flip"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub flip: Option<HxFlip>,
    /// Rotation information (required by 한글).
    #[serde(
        rename(serialize = "hp:rotationInfo", deserialize = "rotationInfo"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rotation_info: Option<HxRotationInfo>,
    /// Rendering transformation matrices (required by 한글).
    #[serde(
        rename(serialize = "hp:renderingInfo", deserialize = "renderingInfo"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rendering_info: Option<HxRenderingInfo>,
    /// Stroke style (required by 한글).
    #[serde(
        rename(serialize = "hp:lineShape", deserialize = "lineShape"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub line_shape: Option<HxLineShape>,
    /// Fill brush (hc: namespace, required by 한글).
    #[serde(
        rename(serialize = "hc:fillBrush", deserialize = "fillBrush"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub fill_brush: Option<HxFillBrush>,
    /// Drop shadow (required by 한글).
    #[serde(
        rename(serialize = "hp:shadow", deserialize = "shadow"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shadow: Option<HxShadow>,

    // ── Size / position / margin (MUST come before hp:-namespaced geometry) ──
    /// Size specification.
    #[serde(
        rename(serialize = "hp:sz", deserialize = "sz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sz: Option<HxTableSz>,
    /// Position specification.
    #[serde(
        rename(serialize = "hp:pos", deserialize = "pos"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pos: Option<HxTablePos>,
    /// Outer margin.
    #[serde(
        rename(serialize = "hp:outMargin", deserialize = "outMargin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub out_margin: Option<HxTableMargin>,

    /// Shape description comment (e.g. "다각형입니다.").
    #[serde(
        rename(serialize = "hp:shapeComment", deserialize = "shapeComment"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shape_comment: Option<HxShapeComment>,

    /// Optional caption attached to this polygon.
    #[serde(
        rename(serialize = "hp:caption", deserialize = "caption"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub caption: Option<HxCaption>,

    /// Optional textbox content inside the polygon.
    #[serde(
        rename(serialize = "hp:drawText", deserialize = "drawText"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub draw_text: Option<HxDrawText>,

    // ── Polygon-specific children (geometry AFTER sz/pos/outMargin; hc: namespace) ──
    /// Ordered list of polygon vertices (hc: namespace per KS X 6101).
    #[serde(
        rename(serialize = "hc:pt", deserialize = "pt"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub points: Vec<HxPoint>,
}

// ── Curve shape ─────────────────────────────────────────────────

/// `<hp:curve>` — bezier/polyline curve drawing object.
///
/// Element order matches 한글's expected serialization:
/// offset → orgSz → curSz → flip → rotationInfo → renderingInfo →
/// lineShape → fillBrush → shadow →
/// sz → pos → outMargin → shapeComment → caption → pt[]
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxCurve {
    // ── AbstractShapeObjectType attrs ──
    /// Element ID.
    #[serde(rename = "@id", default)]
    pub id: String,
    /// Z-order.
    #[serde(rename = "@zOrder", default)]
    pub z_order: u32,
    /// Numbering type.
    #[serde(rename = "@numberingType", default)]
    pub numbering_type: String,
    /// Text wrapping mode.
    #[serde(rename = "@textWrap", default)]
    pub text_wrap: String,
    /// Text flow mode.
    #[serde(rename = "@textFlow", default)]
    pub text_flow: String,
    /// Lock flag.
    #[serde(rename = "@lock", default)]
    pub lock: u32,
    /// Drop cap style.
    #[serde(rename = "@dropcapstyle", default)]
    pub dropcap_style: String,

    // ── AbstractShapeComponentType attrs ──
    /// Hyperlink reference.
    #[serde(rename = "@href", default)]
    pub href: String,
    /// Group nesting level.
    #[serde(rename = "@groupLevel", default)]
    pub group_level: u32,
    /// Instance identifier.
    #[serde(rename = "@instid", default)]
    pub instid: String,

    // ── Shape-common children (ORDER MATTERS!) ──
    /// Position offset.
    #[serde(
        rename(serialize = "hp:offset", deserialize = "offset"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub offset: Option<HxOffset>,
    /// Original size.
    #[serde(
        rename(serialize = "hp:orgSz", deserialize = "orgSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub org_sz: Option<HxSizeAttr>,
    /// Current display size.
    #[serde(
        rename(serialize = "hp:curSz", deserialize = "curSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cur_sz: Option<HxSizeAttr>,
    /// Flip state.
    #[serde(
        rename(serialize = "hp:flip", deserialize = "flip"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub flip: Option<HxFlip>,
    /// Rotation information.
    #[serde(
        rename(serialize = "hp:rotationInfo", deserialize = "rotationInfo"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rotation_info: Option<HxRotationInfo>,
    /// Rendering transformation matrices.
    #[serde(
        rename(serialize = "hp:renderingInfo", deserialize = "renderingInfo"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rendering_info: Option<HxRenderingInfo>,
    /// Stroke style.
    #[serde(
        rename(serialize = "hp:lineShape", deserialize = "lineShape"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub line_shape: Option<HxLineShape>,
    /// Fill brush.
    #[serde(
        rename(serialize = "hc:fillBrush", deserialize = "fillBrush"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub fill_brush: Option<HxFillBrush>,
    /// Drop shadow.
    #[serde(
        rename(serialize = "hp:shadow", deserialize = "shadow"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shadow: Option<HxShadow>,

    // ── Size / position / margin ──
    /// Size specification.
    #[serde(
        rename(serialize = "hp:sz", deserialize = "sz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sz: Option<HxTableSz>,
    /// Position specification.
    #[serde(
        rename(serialize = "hp:pos", deserialize = "pos"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pos: Option<HxTablePos>,
    /// Outer margin.
    #[serde(
        rename(serialize = "hp:outMargin", deserialize = "outMargin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub out_margin: Option<HxTableMargin>,

    /// Shape description comment (e.g. "곡선입니다.").
    #[serde(
        rename(serialize = "hp:shapeComment", deserialize = "shapeComment"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shape_comment: Option<HxShapeComment>,

    /// Optional caption.
    #[serde(
        rename(serialize = "hp:caption", deserialize = "caption"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub caption: Option<HxCaption>,

    // ── Curve-specific children (hc: namespace geometry) ──
    /// Ordered list of curve control points (hc: namespace per KS X 6101).
    #[serde(
        rename(serialize = "hc:pt", deserialize = "pt"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub points: Vec<HxPoint>,

    /// Segment type flags (one per segment: "LINE" or "CURVE").
    #[serde(
        rename(serialize = "hp:seg", deserialize = "seg"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub segments: Vec<HxCurveSegment>,
}

/// `<hp:seg>` — curve segment type descriptor.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxCurveSegment {
    /// Segment type: "LINE" or "CURVE".
    #[serde(rename = "@type", default)]
    pub seg_type: String,
}

// ── Connect line shape ──────────────────────────────────────────

/// `<hp:connectLine>` — connect line drawing object (line with routing).
///
/// Element order matches 한글's expected serialization:
/// offset → orgSz → curSz → flip → rotationInfo → renderingInfo →
/// lineShape → fillBrush → shadow →
/// startPt → endPt → controlPt[] → sz → pos → outMargin → shapeComment → caption
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxConnectLine {
    // ── AbstractShapeObjectType attrs ──
    /// Element ID.
    #[serde(rename = "@id", default)]
    pub id: String,
    /// Z-order.
    #[serde(rename = "@zOrder", default)]
    pub z_order: u32,
    /// Numbering type.
    #[serde(rename = "@numberingType", default)]
    pub numbering_type: String,
    /// Text wrapping mode.
    #[serde(rename = "@textWrap", default)]
    pub text_wrap: String,
    /// Text flow mode.
    #[serde(rename = "@textFlow", default)]
    pub text_flow: String,
    /// Lock flag.
    #[serde(rename = "@lock", default)]
    pub lock: u32,
    /// Drop cap style.
    #[serde(rename = "@dropcapstyle", default)]
    pub dropcap_style: String,

    // ── AbstractShapeComponentType attrs ──
    /// Hyperlink reference.
    #[serde(rename = "@href", default)]
    pub href: String,
    /// Group nesting level.
    #[serde(rename = "@groupLevel", default)]
    pub group_level: u32,
    /// Instance identifier.
    #[serde(rename = "@instid", default)]
    pub instid: String,

    // ── ConnectLine-specific attrs ──
    /// Connection type (STRAIGHT, BENT, CURVED).
    #[serde(rename = "@type", default)]
    pub connect_type: String,

    // ── Shape-common children (ORDER MATTERS!) ──
    /// Position offset.
    #[serde(
        rename(serialize = "hp:offset", deserialize = "offset"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub offset: Option<HxOffset>,
    /// Original size.
    #[serde(
        rename(serialize = "hp:orgSz", deserialize = "orgSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub org_sz: Option<HxSizeAttr>,
    /// Current display size.
    #[serde(
        rename(serialize = "hp:curSz", deserialize = "curSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cur_sz: Option<HxSizeAttr>,
    /// Flip state.
    #[serde(
        rename(serialize = "hp:flip", deserialize = "flip"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub flip: Option<HxFlip>,
    /// Rotation information.
    #[serde(
        rename(serialize = "hp:rotationInfo", deserialize = "rotationInfo"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rotation_info: Option<HxRotationInfo>,
    /// Rendering transformation matrices.
    #[serde(
        rename(serialize = "hp:renderingInfo", deserialize = "renderingInfo"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rendering_info: Option<HxRenderingInfo>,
    /// Stroke style.
    #[serde(
        rename(serialize = "hp:lineShape", deserialize = "lineShape"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub line_shape: Option<HxLineShape>,
    /// Fill brush.
    #[serde(
        rename(serialize = "hc:fillBrush", deserialize = "fillBrush"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub fill_brush: Option<HxFillBrush>,
    /// Drop shadow.
    #[serde(
        rename(serialize = "hp:shadow", deserialize = "shadow"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shadow: Option<HxShadow>,

    // ── ConnectLine-specific children (hc: namespace geometry) ──
    /// Start point of the connect line.
    #[serde(
        rename(serialize = "hc:startPt", deserialize = "startPt"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub start_pt: Option<HxPoint>,
    /// End point of the connect line.
    #[serde(
        rename(serialize = "hc:endPt", deserialize = "endPt"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub end_pt: Option<HxPoint>,
    /// Intermediate control points for routing.
    #[serde(
        rename(serialize = "hc:controlPt", deserialize = "controlPt"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub control_points: Vec<HxPoint>,

    // ── Size / position / margin ──
    /// Size specification.
    #[serde(
        rename(serialize = "hp:sz", deserialize = "sz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sz: Option<HxTableSz>,
    /// Position specification.
    #[serde(
        rename(serialize = "hp:pos", deserialize = "pos"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pos: Option<HxTablePos>,
    /// Outer margin.
    #[serde(
        rename(serialize = "hp:outMargin", deserialize = "outMargin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub out_margin: Option<HxTableMargin>,

    /// Shape description comment (e.g. "연결선입니다.").
    #[serde(
        rename(serialize = "hp:shapeComment", deserialize = "shapeComment"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shape_comment: Option<HxShapeComment>,

    /// Optional caption.
    #[serde(
        rename(serialize = "hp:caption", deserialize = "caption"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub caption: Option<HxCaption>,
}
