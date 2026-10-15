//! The attribute codec of the lossless view's object elements (`<group>`,
//! `<img>` …): attribute names, the value spelling of each enum, attribute
//! escaping and the color reader. The encoder writes and the decoder reads
//! through these same tables, so the two cannot drift apart.

use hwpforge_core::placement::{
    HorzAlign, ObjectRelativeTo, ObjectTextFlow, ObjectTextWrap, VertAlign,
};
use hwpforge_foundation::Color;

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
    v.extend([SRC, ALT, FORMAT, FORMAT_RAW, HYPERLINK]);
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
