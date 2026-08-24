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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
        let p = ObjectPlacement {
            text_wrap: ObjectTextWrap::Square,
            text_flow: ObjectTextFlow::RightOnly,
            treat_as_char: false,
            flow_with_text: true,
            allow_overlap: true,
            vert_rel_to: ObjectRelativeTo::Paper,
            horz_rel_to: ObjectRelativeTo::Page,
            vert_offset: HwpUnit::new(1200).unwrap(),
            horz_offset: HwpUnit::new(3400).unwrap(),
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: ObjectPlacement = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
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
