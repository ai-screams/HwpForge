//! Object-placement metadata shared by every floating drawing object.
//!
//! [`ObjectPlacement`] models the HWPX `<hp:pos>` block plus the owning
//! shape's `textWrap`/`textFlow` attributes: where an object anchors, how
//! surrounding text wraps around it, and whether it behaves like an inline
//! character. It is carried by [`Image`](crate::image::Image) and by every
//! [`Control`](crate::control::Control) drawing-object variant (text boxes,
//! rectangles, lines, ellipses, polygons, arcs, curves, connect lines,
//! groups, textart, embedded charts).
//!
//! Historically these types lived in `image.rs` under `Image*` names and only
//! images carried them; the shape variants stored two loose `i32` offsets and
//! silently dropped the rest of `<hp:pos>`. Promoting the type to a shared
//! `Object*` vocabulary lets the shape encoders/decoders carry the full
//! placement instead.
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::placement::{ObjectPlacement, ObjectTextWrap};
//!
//! let inline = ObjectPlacement::legacy_inline_defaults();
//! assert!(inline.treat_as_char);
//! assert_eq!(inline.text_wrap, ObjectTextWrap::TopAndBottom);
//! ```

use std::borrow::Cow;

use hwpforge_foundation::HwpUnit;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Optional object-placement metadata for a floating drawing object.
///
/// Mirrors the HWPX `<hp:pos>` block (anchor references + offsets +
/// treat-as-char / overlap / flow flags) together with the owning shape's
/// `textWrap`/`textFlow` attributes.
///
/// [`ObjectPlacement::legacy_inline_defaults`] is the canonical "plain inline
/// object" value: an image or shape that behaves like a character with no
/// floating offset. Decoders collapse a placement equal to this default back
/// to `None`, so the encoder's untouched legacy path emits the historical
/// inline bytes (mirrors the `Option<ShapeStyle>` collapse pattern in the
/// HWPX shape-style decoder).
///
/// Offsets are signed: Hancom persists negative offsets in HWPX `hp:pos`
/// as **u32 wrap-around decimal strings** (e.g. `horzOffset="4294965029"`
/// = −2267, dialog-authored fixture measurement), and in HWP5 as plain
/// signed `i32`. Codecs must round-trip both encodings losslessly.
///
/// The struct is `#[non_exhaustive]`: build one with [`ObjectPlacement::new`]
/// (or [`ObjectPlacement::legacy_inline_defaults`]) and the `with_*`
/// builders; fields stay public for reading and in-place edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct ObjectPlacement {
    /// Text wrapping mode around the object.
    pub text_wrap: ObjectTextWrap,
    /// Side flow policy around the wrapped object.
    pub text_flow: ObjectTextFlow,
    /// Whether the object behaves like an inline character.
    pub treat_as_char: bool,
    /// Whether surrounding text should flow with the object.
    pub flow_with_text: bool,
    /// Whether overlapping other objects is allowed.
    pub allow_overlap: bool,
    /// Vertical anchor reference for `vert_offset`.
    pub vert_rel_to: ObjectRelativeTo,
    /// Horizontal anchor reference for `horz_offset`.
    pub horz_rel_to: ObjectRelativeTo,
    /// Vertical offset from `vert_rel_to`.
    pub vert_offset: HwpUnit,
    /// Horizontal offset from `horz_rel_to`.
    pub horz_offset: HwpUnit,
    /// Vertical alignment against `vert_rel_to` (HWPX `<hp:pos vertAlign>`);
    /// the offset is measured from the aligned edge.
    #[serde(default)]
    pub vert_align: VertAlign,
    /// Horizontal alignment against `horz_rel_to` (HWPX `<hp:pos horzAlign>`).
    #[serde(default)]
    pub horz_align: HorzAlign,
    /// Whether the object affects the line spacing of the line it sits on
    /// (HWPX `<hp:pos affectLSpacing>`).
    #[serde(default)]
    pub affect_line_spacing: bool,
    /// Whether the object keeps its anchor when other objects overlap it
    /// (HWPX `<hp:pos holdAnchorAndSO>`).
    #[serde(default)]
    pub hold_anchor_and_so: bool,
    /// The object's outer margin (HWPX `<hp:outMargin>`, HWP5 object common
    /// header bytes 28–36): the space kept between the object and the text
    /// around it. A treat-as-char object with a bottom margin sits that much
    /// higher on its line. `None` = 0 on every side, what the encoders wrote
    /// before this field existed; decoders write `Some` only for a margin
    /// that is not 0 everywhere, so placements that only differed by it keep
    /// collapsing to `None`. JSON omits it when `None`. A table's outer
    /// margin is [`Table::out_margin`](crate::table::Table::out_margin), not
    /// this field: decoders leave it `None` on tables and the table encoder
    /// does not read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_margin: Option<crate::table::TableMargin>,
}

impl ObjectPlacement {
    /// The canonical inline-object placement: treat-as-char, paragraph-anchored,
    /// zero offset, no overlap, top-and-bottom wrap.
    ///
    /// Used by the pre-placement HWPX image path and by shape decoders as the
    /// sentinel that collapses to `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::placement::{ObjectPlacement, ObjectRelativeTo, ObjectTextFlow};
    /// use hwpforge_foundation::HwpUnit;
    ///
    /// let p = ObjectPlacement::legacy_inline_defaults();
    /// assert!(p.treat_as_char);
    /// assert!(!p.allow_overlap);
    /// assert_eq!(p.text_flow, ObjectTextFlow::BothSides);
    /// assert_eq!(p.vert_rel_to, ObjectRelativeTo::Para);
    /// assert_eq!(p.horz_offset, HwpUnit::ZERO);
    /// ```
    #[must_use]
    pub fn legacy_inline_defaults() -> Self {
        Self {
            text_wrap: ObjectTextWrap::TopAndBottom,
            text_flow: ObjectTextFlow::BothSides,
            treat_as_char: true,
            flow_with_text: false,
            allow_overlap: false,
            vert_rel_to: ObjectRelativeTo::Para,
            horz_rel_to: ObjectRelativeTo::Para,
            vert_offset: HwpUnit::ZERO,
            horz_offset: HwpUnit::ZERO,
            vert_align: VertAlign::Top,
            horz_align: HorzAlign::Left,
            affect_line_spacing: false,
            hold_anchor_and_so: false,
            out_margin: None,
        }
    }

    /// A placement with the given wrap mode and anchors. The other fields
    /// start as: text flow [`ObjectTextFlow::BothSides`], not treated as a
    /// character, no flow with text, no overlap, zero offsets, aligned
    /// [`VertAlign::Top`]/[`HorzAlign::Left`], not affecting line spacing,
    /// anchor not held. Change them with the `with_*` builders.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::placement::{HorzAlign, ObjectPlacement, ObjectRelativeTo, ObjectTextWrap};
    /// use hwpforge_foundation::HwpUnit;
    ///
    /// let p = ObjectPlacement::new(ObjectTextWrap::Square, ObjectRelativeTo::Para, ObjectRelativeTo::Column)
    ///     .with_horz_align(HorzAlign::Right)
    ///     .with_vert_offset(HwpUnit::new(1200).unwrap());
    /// assert!(!p.treat_as_char);
    /// assert_eq!(p.horz_align, HorzAlign::Right);
    /// ```
    #[must_use]
    pub fn new(
        text_wrap: ObjectTextWrap,
        vert_rel_to: ObjectRelativeTo,
        horz_rel_to: ObjectRelativeTo,
    ) -> Self {
        Self {
            text_wrap,
            text_flow: ObjectTextFlow::BothSides,
            treat_as_char: false,
            flow_with_text: false,
            allow_overlap: false,
            vert_rel_to,
            horz_rel_to,
            vert_offset: HwpUnit::ZERO,
            horz_offset: HwpUnit::ZERO,
            vert_align: VertAlign::Top,
            horz_align: HorzAlign::Left,
            affect_line_spacing: false,
            hold_anchor_and_so: false,
            out_margin: None,
        }
    }

    /// Sets the side flow policy.
    #[must_use]
    pub fn with_text_flow(mut self, text_flow: ObjectTextFlow) -> Self {
        self.text_flow = text_flow;
        self
    }

    /// Sets whether the object behaves like an inline character.
    #[must_use]
    pub fn with_treat_as_char(mut self, treat_as_char: bool) -> Self {
        self.treat_as_char = treat_as_char;
        self
    }

    /// Sets whether surrounding text flows with the object.
    #[must_use]
    pub fn with_flow_with_text(mut self, flow_with_text: bool) -> Self {
        self.flow_with_text = flow_with_text;
        self
    }

    /// Sets whether overlapping other objects is allowed.
    #[must_use]
    pub fn with_allow_overlap(mut self, allow_overlap: bool) -> Self {
        self.allow_overlap = allow_overlap;
        self
    }

    /// Sets the vertical offset from `vert_rel_to`.
    #[must_use]
    pub fn with_vert_offset(mut self, vert_offset: HwpUnit) -> Self {
        self.vert_offset = vert_offset;
        self
    }

    /// Sets the horizontal offset from `horz_rel_to`.
    #[must_use]
    pub fn with_horz_offset(mut self, horz_offset: HwpUnit) -> Self {
        self.horz_offset = horz_offset;
        self
    }

    /// Sets the vertical alignment.
    #[must_use]
    pub fn with_vert_align(mut self, vert_align: VertAlign) -> Self {
        self.vert_align = vert_align;
        self
    }

    /// Sets the horizontal alignment.
    #[must_use]
    pub fn with_horz_align(mut self, horz_align: HorzAlign) -> Self {
        self.horz_align = horz_align;
        self
    }

    /// Sets whether the object affects line spacing.
    #[must_use]
    pub fn with_affect_line_spacing(mut self, affect_line_spacing: bool) -> Self {
        self.affect_line_spacing = affect_line_spacing;
        self
    }

    /// Sets whether the object holds its anchor against overlapping objects.
    #[must_use]
    pub fn with_hold_anchor_and_so(mut self, hold_anchor_and_so: bool) -> Self {
        self.hold_anchor_and_so = hold_anchor_and_so;
        self
    }
}

/// Vertical alignment of a placed object against its vertical anchor.
///
/// Serialized as the HWPX attribute value (`"TOP"` …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum VertAlign {
    /// Top edge (default).
    #[default]
    #[serde(rename = "TOP")]
    Top,
    /// Centre.
    #[serde(rename = "CENTER")]
    Center,
    /// Bottom edge.
    #[serde(rename = "BOTTOM")]
    Bottom,
    /// Inside edge (towards the binding).
    #[serde(rename = "INSIDE")]
    Inside,
    /// Outside edge (away from the binding).
    #[serde(rename = "OUTSIDE")]
    Outside,
}

impl VertAlign {
    /// The value for an HWPX `vertAlign` string, or `None` for a string this
    /// type does not model.
    pub fn from_hwpx(value: &str) -> Option<Self> {
        match value {
            "TOP" => Some(Self::Top),
            "CENTER" => Some(Self::Center),
            "BOTTOM" => Some(Self::Bottom),
            "INSIDE" => Some(Self::Inside),
            "OUTSIDE" => Some(Self::Outside),
            _ => None,
        }
    }

    /// The HWPX `vertAlign` string.
    pub fn as_hwpx_str(&self) -> &'static str {
        match self {
            Self::Top => "TOP",
            Self::Center => "CENTER",
            Self::Bottom => "BOTTOM",
            Self::Inside => "INSIDE",
            Self::Outside => "OUTSIDE",
        }
    }
}

/// Horizontal alignment of a placed object against its horizontal anchor.
///
/// Serialized as the HWPX attribute value (`"LEFT"` …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum HorzAlign {
    /// Left edge (default).
    #[default]
    #[serde(rename = "LEFT")]
    Left,
    /// Centre.
    #[serde(rename = "CENTER")]
    Center,
    /// Right edge.
    #[serde(rename = "RIGHT")]
    Right,
    /// Inside edge (towards the binding).
    #[serde(rename = "INSIDE")]
    Inside,
    /// Outside edge (away from the binding).
    #[serde(rename = "OUTSIDE")]
    Outside,
}

impl HorzAlign {
    /// The value for an HWPX `horzAlign` string, or `None` for a string this
    /// type does not model.
    pub fn from_hwpx(value: &str) -> Option<Self> {
        match value {
            "LEFT" => Some(Self::Left),
            "CENTER" => Some(Self::Center),
            "RIGHT" => Some(Self::Right),
            "INSIDE" => Some(Self::Inside),
            "OUTSIDE" => Some(Self::Outside),
            _ => None,
        }
    }

    /// The HWPX `horzAlign` string.
    pub fn as_hwpx_str(&self) -> &'static str {
        match self {
            Self::Left => "LEFT",
            Self::Center => "CENTER",
            Self::Right => "RIGHT",
            Self::Inside => "INSIDE",
            Self::Outside => "OUTSIDE",
        }
    }
}

/// Text wrapping mode for placed objects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum ObjectTextWrap {
    /// Place text above and below the object.
    TopAndBottom,
    /// Wrap text on the object's sides.
    Square,
    /// Place the object behind text.
    BehindText,
    /// Place the object in front of text.
    InFrontOfText,
    /// Tight text wrapping around the object.
    Tight,
    /// Through-style wrapping.
    Through,
    /// Any wrap value not modeled explicitly.
    Other(String),
}

impl ObjectTextWrap {
    /// Converts a raw HWPX wrap string into a typed value.
    pub fn from_hwpx(value: &str) -> Self {
        match value {
            "TOP_AND_BOTTOM" => Self::TopAndBottom,
            "SQUARE" => Self::Square,
            "BEHIND_TEXT" => Self::BehindText,
            "IN_FRONT_OF_TEXT" => Self::InFrontOfText,
            "TIGHT" => Self::Tight,
            "THROUGH" => Self::Through,
            other => Self::Other(other.to_string()),
        }
    }

    /// Returns the HWPX serialization string for this wrap mode.
    pub fn as_hwpx_str(&self) -> Cow<'_, str> {
        match self {
            Self::TopAndBottom => Cow::Borrowed("TOP_AND_BOTTOM"),
            Self::Square => Cow::Borrowed("SQUARE"),
            Self::BehindText => Cow::Borrowed("BEHIND_TEXT"),
            Self::InFrontOfText => Cow::Borrowed("IN_FRONT_OF_TEXT"),
            Self::Tight => Cow::Borrowed("TIGHT"),
            Self::Through => Cow::Borrowed("THROUGH"),
            Self::Other(value) => Cow::Borrowed(value.as_str()),
        }
    }
}

/// Text flow mode for placed objects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum ObjectTextFlow {
    /// Text can flow on both sides.
    BothSides,
    /// Text can flow only on the left side.
    LeftOnly,
    /// Text can flow only on the right side.
    RightOnly,
    /// Use the side with the larger available space.
    LargestOnly,
    /// Any flow value not modeled explicitly.
    Other(String),
}

impl ObjectTextFlow {
    /// Converts a raw HWPX flow string into a typed value.
    pub fn from_hwpx(value: &str) -> Self {
        match value {
            "BOTH_SIDES" => Self::BothSides,
            "LEFT_ONLY" => Self::LeftOnly,
            "RIGHT_ONLY" => Self::RightOnly,
            "LARGEST_ONLY" => Self::LargestOnly,
            other => Self::Other(other.to_string()),
        }
    }

    /// Returns the HWPX serialization string for this flow mode.
    pub fn as_hwpx_str(&self) -> Cow<'_, str> {
        match self {
            Self::BothSides => Cow::Borrowed("BOTH_SIDES"),
            Self::LeftOnly => Cow::Borrowed("LEFT_ONLY"),
            Self::RightOnly => Cow::Borrowed("RIGHT_ONLY"),
            Self::LargestOnly => Cow::Borrowed("LARGEST_ONLY"),
            Self::Other(value) => Cow::Borrowed(value.as_str()),
        }
    }
}

/// Anchor target for object placement offsets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum ObjectRelativeTo {
    /// Anchor offsets to the paper.
    Paper,
    /// Anchor offsets to the page.
    Page,
    /// Anchor offsets to the paragraph.
    Para,
    /// Anchor offsets to the column.
    Column,
    /// Anchor offsets to the character box.
    Character,
    /// Anchor offsets to the line box.
    Line,
    /// Any anchor value not modeled explicitly.
    Other(String),
}

impl ObjectRelativeTo {
    /// Converts a raw HWPX anchor string into a typed value.
    pub fn from_hwpx(value: &str) -> Self {
        match value {
            "PAPER" => Self::Paper,
            "PAGE" => Self::Page,
            "PARA" => Self::Para,
            "COLUMN" => Self::Column,
            "CHAR" => Self::Character,
            "LINE" => Self::Line,
            other => Self::Other(other.to_string()),
        }
    }

    /// Returns the HWPX serialization string for this anchor mode.
    pub fn as_hwpx_str(&self) -> Cow<'_, str> {
        match self {
            Self::Paper => Cow::Borrowed("PAPER"),
            Self::Page => Cow::Borrowed("PAGE"),
            Self::Para => Cow::Borrowed("PARA"),
            Self::Column => Cow::Borrowed("COLUMN"),
            Self::Character => Cow::Borrowed("CHAR"),
            Self::Line => Cow::Borrowed("LINE"),
            Self::Other(value) => Cow::Borrowed(value.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// S18: no outer margin writes no `out_margin` (old JSON and its output
    /// unchanged); a margin round trips.
    ///
    /// 이것을 실패시키는 것: `skip_serializing_if` 삭제.
    #[test]
    fn out_margin_json() {
        let p = ObjectPlacement::legacy_inline_defaults();
        let json = serde_json::to_value(&p).unwrap();
        assert!(json.get("out_margin").is_none(), "{json}");
        assert_eq!(serde_json::from_value::<ObjectPlacement>(json).unwrap(), p);
        let mut with = p;
        let u = HwpUnit::new(140).unwrap();
        with.out_margin = Some(crate::table::TableMargin {
            left: HwpUnit::ZERO,
            right: HwpUnit::ZERO,
            top: HwpUnit::ZERO,
            bottom: u,
        });
        let json = serde_json::to_value(&with).unwrap();
        assert_eq!(json["out_margin"]["bottom"], 140);
        assert_eq!(serde_json::from_value::<ObjectPlacement>(json).unwrap(), with);
    }

    #[test]
    fn legacy_inline_defaults_are_inline() {
        let p = ObjectPlacement::legacy_inline_defaults();
        assert!(p.treat_as_char);
        assert!(!p.flow_with_text);
        assert!(!p.allow_overlap);
        assert_eq!(p.text_wrap, ObjectTextWrap::TopAndBottom);
        assert_eq!(p.text_flow, ObjectTextFlow::BothSides);
        assert_eq!(p.vert_rel_to, ObjectRelativeTo::Para);
        assert_eq!(p.horz_rel_to, ObjectRelativeTo::Para);
        assert_eq!(p.vert_offset, HwpUnit::ZERO);
        assert_eq!(p.horz_offset, HwpUnit::ZERO);
    }

    #[test]
    fn placement_serde_roundtrip() {
        let p = ObjectPlacement::new(
            ObjectTextWrap::Square,
            ObjectRelativeTo::Paper,
            ObjectRelativeTo::Page,
        )
        .with_text_flow(ObjectTextFlow::RightOnly)
        .with_flow_with_text(true)
        .with_allow_overlap(true)
        .with_vert_offset(HwpUnit::new(1200).unwrap())
        .with_horz_offset(HwpUnit::new(3400).unwrap())
        .with_vert_align(VertAlign::Bottom)
        .with_horz_align(HorzAlign::Outside)
        .with_affect_line_spacing(true)
        .with_hold_anchor_and_so(true);
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""vert_align":"BOTTOM""#), "{json}");
        assert!(json.contains(r#""horz_align":"OUTSIDE""#), "{json}");
        let back: ObjectPlacement = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    // S1: 네 필드 없는 옛 JSON 은 Top·Left·false·false 로 읽힌다.
    // 이것을 실패시키는 것: 네 필드 중 하나에서 `#[serde(default)]` 를 빼기.
    #[test]
    fn placement_old_json_reads_the_new_fields_as_defaults() {
        let old = r#"{"text_wrap":"Square","text_flow":"BothSides","treat_as_char":false,
            "flow_with_text":false,"allow_overlap":false,"vert_rel_to":"Para",
            "horz_rel_to":"Column","vert_offset":0,"horz_offset":0}"#;
        let p: ObjectPlacement = serde_json::from_str(old).unwrap();
        assert_eq!(
            p,
            ObjectPlacement::new(
                ObjectTextWrap::Square,
                ObjectRelativeTo::Para,
                ObjectRelativeTo::Column
            )
        );
    }

    #[test]
    fn align_hwpx_roundtrip() {
        for v in [
            VertAlign::Top,
            VertAlign::Center,
            VertAlign::Bottom,
            VertAlign::Inside,
            VertAlign::Outside,
        ] {
            assert_eq!(VertAlign::from_hwpx(v.as_hwpx_str()), Some(v));
            assert_eq!(serde_json::to_string(&v).unwrap(), format!("\"{}\"", v.as_hwpx_str()));
        }
        for v in [
            HorzAlign::Left,
            HorzAlign::Center,
            HorzAlign::Right,
            HorzAlign::Inside,
            HorzAlign::Outside,
        ] {
            assert_eq!(HorzAlign::from_hwpx(v.as_hwpx_str()), Some(v));
            assert_eq!(serde_json::to_string(&v).unwrap(), format!("\"{}\"", v.as_hwpx_str()));
        }
        assert_eq!(VertAlign::from_hwpx("MIDDLE"), None);
        assert_eq!(HorzAlign::from_hwpx("left"), None);
    }

    #[test]
    fn text_wrap_hwpx_roundtrip() {
        for (s, v) in [
            ("TOP_AND_BOTTOM", ObjectTextWrap::TopAndBottom),
            ("SQUARE", ObjectTextWrap::Square),
            ("BEHIND_TEXT", ObjectTextWrap::BehindText),
            ("IN_FRONT_OF_TEXT", ObjectTextWrap::InFrontOfText),
            ("TIGHT", ObjectTextWrap::Tight),
            ("THROUGH", ObjectTextWrap::Through),
        ] {
            assert_eq!(ObjectTextWrap::from_hwpx(s), v);
            assert_eq!(v.as_hwpx_str(), s);
        }
        assert_eq!(ObjectTextWrap::from_hwpx("WEIRD"), ObjectTextWrap::Other("WEIRD".to_string()));
        assert_eq!(ObjectTextWrap::Other("WEIRD".to_string()).as_hwpx_str(), "WEIRD");
    }

    #[test]
    fn text_flow_hwpx_roundtrip() {
        for (s, v) in [
            ("BOTH_SIDES", ObjectTextFlow::BothSides),
            ("LEFT_ONLY", ObjectTextFlow::LeftOnly),
            ("RIGHT_ONLY", ObjectTextFlow::RightOnly),
            ("LARGEST_ONLY", ObjectTextFlow::LargestOnly),
        ] {
            assert_eq!(ObjectTextFlow::from_hwpx(s), v);
            assert_eq!(v.as_hwpx_str(), s);
        }
        assert_eq!(ObjectTextFlow::from_hwpx("X"), ObjectTextFlow::Other("X".to_string()));
        assert_eq!(ObjectTextFlow::Other("X".to_string()).as_hwpx_str(), "X");
    }

    #[test]
    fn relative_to_hwpx_roundtrip() {
        for (s, v) in [
            ("PAPER", ObjectRelativeTo::Paper),
            ("PAGE", ObjectRelativeTo::Page),
            ("PARA", ObjectRelativeTo::Para),
            ("COLUMN", ObjectRelativeTo::Column),
            ("CHAR", ObjectRelativeTo::Character),
            ("LINE", ObjectRelativeTo::Line),
        ] {
            assert_eq!(ObjectRelativeTo::from_hwpx(s), v);
            assert_eq!(v.as_hwpx_str(), s);
        }
        assert_eq!(ObjectRelativeTo::from_hwpx("Z"), ObjectRelativeTo::Other("Z".to_string()));
        assert_eq!(ObjectRelativeTo::Other("Z".to_string()).as_hwpx_str(), "Z");
    }
}
