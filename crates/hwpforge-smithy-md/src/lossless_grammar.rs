//! The attribute codec of the lossless view's object elements (`<group>`,
//! `<img>` …): attribute names, the value spelling of each enum, attribute
//! escaping and the color reader. The encoder writes and the decoder reads
//! through these same tables, so the two cannot drift apart.

use hwpforge_core::caption::CaptionSide;
use hwpforge_core::control::LineStyle;
use hwpforge_core::placement::{
    HorzAlign, ObjectRelativeTo, ObjectTextFlow, ObjectTextWrap, VertAlign,
};
use hwpforge_core::table::CellLineWrap;
use hwpforge_foundation::{
    ArcType, ArrowSize, ArrowType, Color, CurveSegmentType, DropCapStyle, Flip, GradientType,
    PatternType, VerticalAlign,
};

use crate::error::{MdError, MdResult};

/// PLACE (`ObjectPlacement` without its outer margin): written all together
/// or not at all. Enum values use the Core serde variant names.
pub(crate) const TEXT_WRAP: &str = "data-text-wrap";
pub(crate) const TEXT_FLOW: &str = "data-text-flow";
pub(crate) const TREAT_AS_CHAR: &str = "data-treat-as-char";
pub(crate) const FLOW_WITH_TEXT: &str = "data-flow-with-text";
pub(crate) const ALLOW_OVERLAP: &str = "data-allow-overlap";
pub(crate) const VERT_REL_TO: &str = "data-vert-rel-to";
pub(crate) const HORZ_REL_TO: &str = "data-horz-rel-to";
pub(crate) const VERT_OFFSET: &str = "data-vert-offset-unit";
pub(crate) const HORZ_OFFSET: &str = "data-horz-offset-unit";
pub(crate) const VERT_ALIGN: &str = "data-vert-align";
pub(crate) const HORZ_ALIGN: &str = "data-horz-align";
pub(crate) const AFFECT_LINE_SPACING: &str = "data-affect-line-spacing";
pub(crate) const HOLD_ANCHOR: &str = "data-hold-anchor";
/// The PLACE attributes that must all be present (or all absent).
pub(crate) const PLACE: [&str; 13] = [
    TEXT_WRAP,
    TEXT_FLOW,
    TREAT_AS_CHAR,
    FLOW_WITH_TEXT,
    ALLOW_OVERLAP,
    VERT_REL_TO,
    HORZ_REL_TO,
    VERT_OFFSET,
    HORZ_OFFSET,
    VERT_ALIGN,
    HORZ_ALIGN,
    AFFECT_LINE_SPACING,
    HOLD_ANCHOR,
];
/// Readable (ignored) companions of the two offsets.
pub(crate) const PLACE_MM: [&str; 2] = ["data-vert-offset-mm", "data-horz-offset-mm"];
/// OUTMARGIN (the placement's outer margin): all four or none, only with
/// PLACE. `Some` with four zeros is written too, so it stays apart from `None`.
pub(crate) const OUT_MARGIN: [&str; 4] = [
    "data-out-margin-left-unit",
    "data-out-margin-right-unit",
    "data-out-margin-top-unit",
    "data-out-margin-bottom-unit",
];
/// BOX: the displayed size.
pub(crate) const WIDTH: &str = "data-width-unit";
pub(crate) const HEIGHT: &str = "data-height-unit";
pub(crate) const BOX_MM: [&str; 2] = ["data-width-mm", "data-height-mm"];
/// FRAME (S20): the original box and the size lock.
pub(crate) const FRAME: [&str; 5] = [
    "data-original-width-unit",
    "data-original-height-unit",
    "data-original-width-mm",
    "data-original-height-mm",
    "data-size-protect",
];
/// CS: the run's char shape — required under `<p>`, refused elsewhere.
pub(crate) const CHAR_SHAPE: &str = "data-char-shape";
/// ID: the object's instance id.
pub(crate) const INST_ID: &str = "data-inst-id";
/// The picture's own attributes.
pub(crate) const SRC: &str = "src";
pub(crate) const ALT: &str = "alt";
pub(crate) const FORMAT: &str = "data-format";
pub(crate) const FORMAT_RAW: &str = "data-format-raw";
pub(crate) const HYPERLINK: &str = "data-hyperlink";
/// CROP (`Image::crop`): all four or none.
pub(crate) const CROP: [&str; 4] =
    ["data-crop-left-unit", "data-crop-top-unit", "data-crop-right-unit", "data-crop-bottom-unit"];
/// EXTENT (`Image::image_extent`): both or none.
pub(crate) const EXTENT: [&str; 2] =
    ["data-image-extent-width-unit", "data-image-extent-height-unit"];

/// The caption element (`<caption>`): side and gap required, the width
/// (BOX's `data-width-unit`) optional, then the full-size flag and saved
/// width when set.
pub(crate) const CAPTION_SIDE: &str = "data-side";
pub(crate) const CAPTION_GAP: &str = "data-gap-unit";
pub(crate) const CAPTION_GAP_MM: &str = "data-gap-mm";
pub(crate) const CAPTION_FULL_SIZE: &str = "data-full-size";
pub(crate) const CAPTION_LAST_WIDTH: &str = "data-last-width-unit";

/// Every attribute `<caption>` may carry.
pub(crate) fn caption_attributes() -> Vec<&'static str> {
    vec![
        CAPTION_SIDE,
        CAPTION_GAP,
        CAPTION_GAP_MM,
        WIDTH,
        BOX_MM[0],
        CAPTION_FULL_SIZE,
        CAPTION_LAST_WIDTH,
    ]
}

pub(crate) fn caption_side_str(v: CaptionSide) -> MdResult<&'static str> {
    Ok(match v {
        CaptionSide::Left => "Left",
        CaptionSide::Right => "Right",
        CaptionSide::Top => "Top",
        CaptionSide::Bottom => "Bottom",
        #[allow(unreachable_patterns)]
        _ => return Err(unsupported("caption side")),
    })
}

pub(crate) fn caption_side_from(s: &str) -> Option<CaptionSide> {
    Some(match s {
        "Left" => CaptionSide::Left,
        "Right" => CaptionSide::Right,
        "Top" => CaptionSide::Top,
        "Bottom" => CaptionSide::Bottom,
        _ => return None,
    })
}

/// STYLE (`ShapeStyle`): the marker `data-style="true"` stands for
/// `Some(style)`; every other STYLE attribute needs it. Each `Option` field
/// is written only when `Some`.
pub(crate) const STYLE: &str = "data-style";
pub(crate) const LINE_COLOR: &str = "data-line-color";
pub(crate) const LINE_WIDTH: &str = "data-line-width-unit";
pub(crate) const LINE_STYLE: &str = "data-line-style";
pub(crate) const FILL_COLOR: &str = "data-fill-color";
pub(crate) const IN_MARGIN: [&str; 4] =
    ["data-in-margin-left", "data-in-margin-right", "data-in-margin-top", "data-in-margin-bottom"];
pub(crate) const ROTATION: &str = "data-rotation";
pub(crate) const FLIP: &str = "data-flip";
/// Head and tail arrows: type, size, filled — all three or none, each.
pub(crate) const HEAD_ARROW: [&str; 3] =
    ["data-head-arrow-type", "data-head-arrow-size", "data-head-arrow-filled"];
pub(crate) const TAIL_ARROW: [&str; 3] =
    ["data-tail-arrow-type", "data-tail-arrow-size", "data-tail-arrow-filled"];
/// The fill (`ShapeStyle::fill`): its kind, then that kind's attributes only.
pub(crate) const FILL_KIND: &str = "data-fill-kind";
pub(crate) const FILL_SOLID_COLOR: &str = "data-fill-solid-color";
/// Gradient: type, angle and stops required; the centre and steps each
/// written when `Some`.
pub(crate) const GRADIENT: [&str; 3] =
    ["data-fill-gradient-type", "data-fill-gradient-angle", "data-fill-gradient-stops"];
pub(crate) const GRADIENT_OPTIONAL: [&str; 4] = [
    "data-fill-gradient-center-x",
    "data-fill-gradient-center-y",
    "data-fill-gradient-step",
    "data-fill-gradient-step-center",
];
/// Pattern: type and the two colors.
pub(crate) const PATTERN: [&str; 3] =
    ["data-fill-pattern-type", "data-fill-pattern-fg-color", "data-fill-pattern-bg-color"];
pub(crate) const DROP_CAP: &str = "data-drop-cap";
pub(crate) const CORNER_RATIO: &str = "data-corner-ratio";
/// VALIGN: the text's vertical alignment inside a shape, when not `Top`.
pub(crate) const TEXT_VALIGN: &str = "data-text-valign";
/// TEXTMARGIN (`TextBox::text_margin`): all four or none.
pub(crate) const TEXT_MARGIN: [&str; 4] = [
    "data-text-margin-left-unit",
    "data-text-margin-right-unit",
    "data-text-margin-top-unit",
    "data-text-margin-bottom-unit",
];
/// Defining points.
pub(crate) const START: [&str; 2] = ["data-start-x", "data-start-y"];
pub(crate) const END: [&str; 2] = ["data-end-x", "data-end-y"];
pub(crate) const CENTER: [&str; 2] = ["data-cx", "data-cy"];
pub(crate) const AXIS1: [&str; 2] = ["data-ax1-x", "data-ax1-y"];
pub(crate) const AXIS2: [&str; 2] = ["data-ax2-x", "data-ax2-y"];
pub(crate) const POINTS: &str = "data-points";
pub(crate) const START1: [&str; 2] = ["data-start1-x", "data-start1-y"];
pub(crate) const END1: [&str; 2] = ["data-end1-x", "data-end1-y"];
pub(crate) const START2: [&str; 2] = ["data-start2-x", "data-start2-y"];
pub(crate) const END2: [&str; 2] = ["data-end2-x", "data-end2-y"];
pub(crate) const ARC_DEFINITION: &str = "data-arc-definition";
pub(crate) const ARC_TYPE: &str = "data-arc-type";
/// Curve segment kinds, `Line`/`Curve` joined by `,` (empty = none).
pub(crate) const SEGMENTS: &str = "data-segments";
/// Connect line routing points, `x,y;x,y` (empty = none).
pub(crate) const CONTROL_POINTS: &str = "data-control-points";
/// Connect line type, verbatim (escaped as an attribute).
pub(crate) const CONNECT_TYPE: &str = "data-connect-type";

/// Every STYLE attribute.
pub(crate) fn style_attributes() -> Vec<&'static str> {
    let mut v = vec![
        STYLE,
        LINE_COLOR,
        LINE_WIDTH,
        LINE_STYLE,
        FILL_COLOR,
        ROTATION,
        FLIP,
        FILL_KIND,
        FILL_SOLID_COLOR,
        DROP_CAP,
        CORNER_RATIO,
    ];
    v.extend(HEAD_ARROW);
    v.extend(TAIL_ARROW);
    v.extend(GRADIENT);
    v.extend(GRADIENT_OPTIONAL);
    v.extend(PATTERN);
    v
}

/// What a drawing shape carries besides its own defining points: CS, BOX,
/// FRAME, PLACE, OUTMARGIN and STYLE.
pub(crate) fn shape_attributes() -> Vec<&'static str> {
    let mut v = vec![CHAR_SHAPE, WIDTH, HEIGHT];
    v.extend(BOX_MM);
    v.extend(FRAME);
    v.extend(PLACE);
    v.extend(PLACE_MM);
    v.extend(OUT_MARGIN);
    v.extend(style_attributes());
    v
}

/// A color STYLE writes: `#RRGGBB`; a color with its reserved high byte set
/// has no spelling and is refused.
pub(crate) fn color_str(c: Color) -> MdResult<String> {
    if c.to_raw() >> 24 != 0 {
        return Err(unsupported("color with reserved bits"));
    }
    Ok(c.to_string())
}

pub(crate) fn line_style_str(v: LineStyle) -> MdResult<&'static str> {
    Ok(match v {
        LineStyle::Solid => "Solid",
        LineStyle::Dash => "Dash",
        LineStyle::Dot => "Dot",
        LineStyle::DashDot => "DashDot",
        LineStyle::DashDotDot => "DashDotDot",
        LineStyle::None => "None",
        _ => return Err(unsupported("line style")),
    })
}

pub(crate) fn line_style_from(s: &str) -> Option<LineStyle> {
    Some(match s {
        "Solid" => LineStyle::Solid,
        "Dash" => LineStyle::Dash,
        "Dot" => LineStyle::Dot,
        "DashDot" => LineStyle::DashDot,
        "DashDotDot" => LineStyle::DashDotDot,
        "None" => LineStyle::None,
        _ => return None,
    })
}

pub(crate) fn flip_str(v: Flip) -> MdResult<&'static str> {
    Ok(match v {
        Flip::None => "None",
        Flip::Horizontal => "Horizontal",
        Flip::Vertical => "Vertical",
        Flip::Both => "Both",
        _ => return Err(unsupported("flip")),
    })
}

pub(crate) fn flip_from(s: &str) -> Option<Flip> {
    Some(match s {
        "None" => Flip::None,
        "Horizontal" => Flip::Horizontal,
        "Vertical" => Flip::Vertical,
        "Both" => Flip::Both,
        _ => return None,
    })
}

pub(crate) fn arrow_type_str(v: ArrowType) -> MdResult<&'static str> {
    Ok(match v {
        ArrowType::None => "None",
        ArrowType::Normal => "Normal",
        ArrowType::Arrow => "Arrow",
        ArrowType::Concave => "Concave",
        ArrowType::Diamond => "Diamond",
        ArrowType::Oval => "Oval",
        ArrowType::Open => "Open",
        _ => return Err(unsupported("arrow type")),
    })
}

pub(crate) fn arrow_type_from(s: &str) -> Option<ArrowType> {
    Some(match s {
        "None" => ArrowType::None,
        "Normal" => ArrowType::Normal,
        "Arrow" => ArrowType::Arrow,
        "Concave" => ArrowType::Concave,
        "Diamond" => ArrowType::Diamond,
        "Oval" => ArrowType::Oval,
        "Open" => ArrowType::Open,
        _ => return None,
    })
}

pub(crate) fn arrow_size_str(v: ArrowSize) -> MdResult<&'static str> {
    Ok(match v {
        ArrowSize::Small => "Small",
        ArrowSize::Medium => "Medium",
        ArrowSize::Large => "Large",
        ArrowSize::SmallMedium => "SmallMedium",
        ArrowSize::SmallLarge => "SmallLarge",
        ArrowSize::MediumSmall => "MediumSmall",
        ArrowSize::MediumLarge => "MediumLarge",
        ArrowSize::LargeSmall => "LargeSmall",
        ArrowSize::LargeMedium => "LargeMedium",

        _ => return Err(unsupported("arrow size")),
    })
}

pub(crate) fn arrow_size_from(s: &str) -> Option<ArrowSize> {
    Some(match s {
        "Small" => ArrowSize::Small,
        "Medium" => ArrowSize::Medium,
        "Large" => ArrowSize::Large,
        "SmallMedium" => ArrowSize::SmallMedium,
        "SmallLarge" => ArrowSize::SmallLarge,
        "MediumSmall" => ArrowSize::MediumSmall,
        "MediumLarge" => ArrowSize::MediumLarge,
        "LargeSmall" => ArrowSize::LargeSmall,
        "LargeMedium" => ArrowSize::LargeMedium,

        _ => return None,
    })
}

pub(crate) fn gradient_type_str(v: GradientType) -> MdResult<&'static str> {
    Ok(match v {
        GradientType::Linear => "Linear",
        GradientType::Radial => "Radial",
        GradientType::Square => "Square",
        GradientType::Conical => "Conical",
        _ => return Err(unsupported("gradient type")),
    })
}

pub(crate) fn gradient_type_from(s: &str) -> Option<GradientType> {
    Some(match s {
        "Linear" => GradientType::Linear,
        "Radial" => GradientType::Radial,
        "Square" => GradientType::Square,
        "Conical" => GradientType::Conical,
        _ => return None,
    })
}

pub(crate) fn pattern_type_str(v: PatternType) -> MdResult<&'static str> {
    Ok(match v {
        PatternType::Horizontal => "Horizontal",
        PatternType::Vertical => "Vertical",
        PatternType::BackSlash => "BackSlash",
        PatternType::Slash => "Slash",
        PatternType::Cross => "Cross",
        PatternType::CrossDiagonal => "CrossDiagonal",
        _ => return Err(unsupported("pattern type")),
    })
}

pub(crate) fn pattern_type_from(s: &str) -> Option<PatternType> {
    Some(match s {
        "Horizontal" => PatternType::Horizontal,
        "Vertical" => PatternType::Vertical,
        "BackSlash" => PatternType::BackSlash,
        "Slash" => PatternType::Slash,
        "Cross" => PatternType::Cross,
        "CrossDiagonal" => PatternType::CrossDiagonal,
        _ => return None,
    })
}

/// A drop cap other than `None` (which is not written).
pub(crate) fn drop_cap_str(v: DropCapStyle) -> MdResult<Option<&'static str>> {
    Ok(match v {
        DropCapStyle::None => None,
        DropCapStyle::DoubleLine => Some("DoubleLine"),
        DropCapStyle::TripleLine => Some("TripleLine"),
        DropCapStyle::Margin => Some("Margin"),
        _ => return Err(unsupported("drop cap")),
    })
}

pub(crate) fn drop_cap_from(s: &str) -> Option<DropCapStyle> {
    Some(match s {
        "DoubleLine" => DropCapStyle::DoubleLine,
        "TripleLine" => DropCapStyle::TripleLine,
        "Margin" => DropCapStyle::Margin,
        _ => return None,
    })
}

pub(crate) fn arc_type_str(v: ArcType) -> MdResult<&'static str> {
    Ok(match v {
        ArcType::Normal => "Normal",
        ArcType::Pie => "Pie",
        ArcType::Chord => "Chord",
        _ => return Err(unsupported("arc type")),
    })
}

pub(crate) fn arc_type_from(s: &str) -> Option<ArcType> {
    Some(match s {
        "Normal" => ArcType::Normal,
        "Pie" => ArcType::Pie,
        "Chord" => ArcType::Chord,
        _ => return None,
    })
}

pub(crate) fn segment_str(v: CurveSegmentType) -> MdResult<&'static str> {
    Ok(match v {
        CurveSegmentType::Line => "Line",
        CurveSegmentType::Curve => "Curve",
        _ => return Err(unsupported("curve segment")),
    })
}

pub(crate) fn segment_from(s: &str) -> Option<CurveSegmentType> {
    Some(match s {
        "Line" => CurveSegmentType::Line,
        "Curve" => CurveSegmentType::Curve,
        _ => return None,
    })
}

/// A shape's text vertical alignment other than `Top` (not written).
pub(crate) fn valign_str(v: VerticalAlign) -> MdResult<Option<&'static str>> {
    Ok(match v {
        VerticalAlign::Top => None,
        VerticalAlign::Center => Some("Center"),
        VerticalAlign::Bottom => Some("Bottom"),
        _ => return Err(unsupported("text vertical align")),
    })
}

pub(crate) fn valign_from(s: &str) -> Option<VerticalAlign> {
    Some(match s {
        "Center" => VerticalAlign::Center,
        "Bottom" => VerticalAlign::Bottom,
        _ => return None,
    })
}

/// A table cell's line wrap (`<td>`), written only when it is not the
/// default break.
pub(crate) const LINE_WRAP: &str = "data-line-wrap";

// Title markers: a missing cell attribute retains row inheritance.
pub(crate) const CELL_HEADER: &str = "data-header";
pub(crate) const ROW_HEADER: &str = "data-is-header";

/// How many containers (tables, groups, pictures, captions, shapes, notes)
/// may enclose one another while parsing. The writer accepts validated Core
/// documents, whose semantic nesting depth is checked by Core validation
/// before any recursive encoding walk. This parser budget bounds input
/// nesting before building the Core tree.
// Shares Core's public MAX_NESTING_DEPTH; the parser counts different containers.
pub(crate) const MAX_NESTING_DEPTH: usize = hwpforge_core::MAX_NESTING_DEPTH;

/// The elements that count toward [`MAX_NESTING_DEPTH`].
pub(crate) const CONTAINER_TAGS: [&str; 15] = [
    "table",
    "group",
    "img",
    "caption",
    "textbox",
    "line",
    "ellipse",
    "polygon",
    "rect",
    "arc",
    "curve",
    "connectline",
    "footnote",
    "endnote",
    "field-start",
];

/// Every attribute `<group>` may carry.
pub(crate) fn group_attributes() -> Vec<&'static str> {
    let mut v = vec![CHAR_SHAPE, WIDTH, HEIGHT, INST_ID];
    v.extend(BOX_MM);
    v.extend(FRAME);
    v.extend(PLACE);
    v.extend(PLACE_MM);
    v.extend(OUT_MARGIN);
    v
}

/// Every attribute `<img>` may carry.
pub(crate) fn picture_attributes() -> Vec<&'static str> {
    let mut v = group_attributes();
    v.extend([SRC, ALT, FORMAT, FORMAT_RAW, HYPERLINK, ROTATION, FLIP]);
    v.extend(IN_MARGIN);
    v.extend(CROP);
    v.extend(EXTENT);
    v
}

/// Escapes an attribute value: the five markup characters as before, and
/// every control character U+0001–U+001F (tab, CR and LF included) as a
/// character reference, which attribute-value normalization leaves alone
/// (a raw tab or line break would come back as a space). U+0000 has no XML
/// form and is refused.
pub(crate) fn escape_attr(input: &str) -> MdResult<String> {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            '\0' => {
                return Err(MdError::UnsupportedStructure {
                    detail: "attribute value with U+0000 has no XML form".to_string(),
                })
            }
            c if (c as u32) < 0x20 => escaped.push_str(&format!("&#{};", c as u32)),
            c => escaped.push(c),
        }
    }
    Ok(escaped)
}

/// Reads `#RRGGBB` (the `#` is optional): exactly six ASCII hex digits, or
/// the value is invalid — checked byte by byte before slicing, so a
/// multi-byte value cannot split a character.
pub(crate) fn parse_hex_color(
    element: &'static str,
    attribute: &'static str,
    value: &str,
) -> MdResult<Color> {
    let invalid =
        || MdError::LosslessInvalidAttribute { element, attribute, value: value.to_string() };
    let hex = value.strip_prefix('#').unwrap_or(value);
    let bytes = hex.as_bytes();
    if bytes.len() != 6 || !bytes.iter().all(u8::is_ascii_hexdigit) {
        return Err(invalid());
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| invalid());
    Ok(Color::from_rgb(byte(0)?, byte(2)?, byte(4)?))
}

/// An enum value the grammar has no spelling for (a future Core variant or
/// an `Other(..)` raw value): refused by the encoder.
fn unsupported(what: &str) -> MdError {
    MdError::UnsupportedStructure {
        detail: format!("{what} value is not carried by the lossless encoder"),
    }
}

pub(crate) fn text_wrap_str(v: &ObjectTextWrap) -> MdResult<&'static str> {
    Ok(match v {
        ObjectTextWrap::TopAndBottom => "TopAndBottom",
        ObjectTextWrap::Square => "Square",
        ObjectTextWrap::BehindText => "BehindText",
        ObjectTextWrap::InFrontOfText => "InFrontOfText",
        ObjectTextWrap::Tight => "Tight",
        ObjectTextWrap::Through => "Through",
        _ => return Err(unsupported("object text wrap")),
    })
}

pub(crate) fn text_wrap_from(s: &str) -> Option<ObjectTextWrap> {
    Some(match s {
        "TopAndBottom" => ObjectTextWrap::TopAndBottom,
        "Square" => ObjectTextWrap::Square,
        "BehindText" => ObjectTextWrap::BehindText,
        "InFrontOfText" => ObjectTextWrap::InFrontOfText,
        "Tight" => ObjectTextWrap::Tight,
        "Through" => ObjectTextWrap::Through,
        _ => return None,
    })
}

pub(crate) fn text_flow_str(v: &ObjectTextFlow) -> MdResult<&'static str> {
    Ok(match v {
        ObjectTextFlow::BothSides => "BothSides",
        ObjectTextFlow::LeftOnly => "LeftOnly",
        ObjectTextFlow::RightOnly => "RightOnly",
        ObjectTextFlow::LargestOnly => "LargestOnly",
        _ => return Err(unsupported("object text flow")),
    })
}

pub(crate) fn text_flow_from(s: &str) -> Option<ObjectTextFlow> {
    Some(match s {
        "BothSides" => ObjectTextFlow::BothSides,
        "LeftOnly" => ObjectTextFlow::LeftOnly,
        "RightOnly" => ObjectTextFlow::RightOnly,
        "LargestOnly" => ObjectTextFlow::LargestOnly,
        _ => return None,
    })
}

pub(crate) fn rel_to_str(v: &ObjectRelativeTo) -> MdResult<&'static str> {
    Ok(match v {
        ObjectRelativeTo::Paper => "Paper",
        ObjectRelativeTo::Page => "Page",
        ObjectRelativeTo::Para => "Para",
        ObjectRelativeTo::Column => "Column",
        ObjectRelativeTo::Character => "Character",
        ObjectRelativeTo::Line => "Line",
        _ => return Err(unsupported("object relative-to")),
    })
}

pub(crate) fn rel_to_from(s: &str) -> Option<ObjectRelativeTo> {
    Some(match s {
        "Paper" => ObjectRelativeTo::Paper,
        "Page" => ObjectRelativeTo::Page,
        "Para" => ObjectRelativeTo::Para,
        "Column" => ObjectRelativeTo::Column,
        "Character" => ObjectRelativeTo::Character,
        "Line" => ObjectRelativeTo::Line,
        _ => return None,
    })
}

pub(crate) fn vert_align_str(v: &VertAlign) -> MdResult<&'static str> {
    Ok(match v {
        VertAlign::Top => "TOP",
        VertAlign::Center => "CENTER",
        VertAlign::Bottom => "BOTTOM",
        VertAlign::Inside => "INSIDE",
        VertAlign::Outside => "OUTSIDE",
        _ => return Err(unsupported("object vertical align")),
    })
}

pub(crate) fn vert_align_from(s: &str) -> Option<VertAlign> {
    Some(match s {
        "TOP" => VertAlign::Top,
        "CENTER" => VertAlign::Center,
        "BOTTOM" => VertAlign::Bottom,
        "INSIDE" => VertAlign::Inside,
        "OUTSIDE" => VertAlign::Outside,
        _ => return None,
    })
}

pub(crate) fn horz_align_str(v: &HorzAlign) -> MdResult<&'static str> {
    Ok(match v {
        HorzAlign::Left => "LEFT",
        HorzAlign::Center => "CENTER",
        HorzAlign::Right => "RIGHT",
        HorzAlign::Inside => "INSIDE",
        HorzAlign::Outside => "OUTSIDE",
        _ => return Err(unsupported("object horizontal align")),
    })
}

pub(crate) fn horz_align_from(s: &str) -> Option<HorzAlign> {
    Some(match s {
        "LEFT" => HorzAlign::Left,
        "CENTER" => HorzAlign::Center,
        "RIGHT" => HorzAlign::Right,
        "INSIDE" => HorzAlign::Inside,
        "OUTSIDE" => HorzAlign::Outside,
        _ => return None,
    })
}

/// The spelling of a cell line wrap; `None` for the default break (not
/// written).
pub(crate) fn line_wrap_str(v: CellLineWrap) -> MdResult<Option<&'static str>> {
    Ok(match v {
        CellLineWrap::Break => None,
        CellLineWrap::Squeeze => Some("SQUEEZE"),
        CellLineWrap::Keep => Some("KEEP"),
        _ => return Err(unsupported("cell line wrap")),
    })
}

pub(crate) fn line_wrap_from(s: &str) -> Option<CellLineWrap> {
    Some(match s {
        "SQUEEZE" => CellLineWrap::Squeeze,
        "KEEP" => CellLineWrap::Keep,
        _ => return None,
    })
}

/// A `"true"`/`"false"` value.
pub(crate) fn bool_str(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

/// The readable millimetre companion of a length (never read back).
pub(crate) fn mm(v: hwpforge_foundation::HwpUnit) -> String {
    format!("{:.2}", v.to_mm())
}

/// Field markers are siblings of their visible body runs, never wrappers.
pub(crate) const FIELD_START: &[&str] = &[
    CHAR_SHAPE,
    "data-id",
    "data-kind",
    "data-name",
    "data-editable",
    "data-dirty",
    "data-z-order",
    "data-field-id",
    "data-meta-tag",
    "data-parameter-name",
    "data-unclosed",
];
pub(crate) const FIELD_END: &[&str] = &[CHAR_SHAPE, "data-begin-id", "data-field-id"];
pub(crate) const FIELD_PARAM: &[&str] = &["data-kind", "data-name", "data-value", "data-xml-space"];

pub(crate) fn field_parameter_kind(
    p: &hwpforge_core::control::FieldParameter,
) -> MdResult<&'static str> {
    use hwpforge_core::control::FieldParameter;
    Ok(match p {
        FieldParameter::String { .. } => "String",
        FieldParameter::Integer { .. } => "Integer",
        FieldParameter::Boolean { .. } => "Boolean",
        _ => return Err(unsupported("field parameter")),
    })
}

/// Reads the same scalar-kind vocabulary the writer uses. Values stay strings;
/// source spelling is meaningful even for numeric and boolean parameters.
pub(crate) fn field_parameter_from(
    kind: &str,
    name: String,
    value: String,
    xml_space: Option<&str>,
) -> MdResult<hwpforge_core::control::FieldParameter> {
    use hwpforge_core::control::FieldParameter;
    Ok(match (kind, xml_space) {
        ("String", Some(space)) => FieldParameter::String { name, value, xml_space: space.into() },
        ("Integer", None) => FieldParameter::Integer { name, value },
        ("Boolean", None) => FieldParameter::Boolean { name, value },
        ("String", None) => {
            return Err(MdError::LosslessMissingAttribute {
                element: "field-param",
                attribute: "data-xml-space",
            })
        }
        _ => {
            return Err(MdError::LosslessInvalidAttribute {
                element: "field-param",
                attribute: "data-kind",
                value: kind.into(),
            })
        }
    })
}

pub(crate) fn arc_definition_from(s: &str) -> Option<hwpforge_core::control::ArcDefinition> {
    use hwpforge_core::control::ArcDefinition;
    match s {
        "EllipseSweep" => Some(ArcDefinition::EllipseSweep),
        "AxisEndpoints" => Some(ArcDefinition::AxisEndpoints),
        _ => None,
    }
}
