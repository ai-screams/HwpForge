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

use crate::paragraph::Paragraph;

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
        let ctrl =
            Control::Endnote { inst_id: Some(999), paragraphs: vec![simple_paragraph()] };
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
        let ctrl =
            Control::Footnote { inst_id: Some(12345), paragraphs: vec![simple_paragraph()] };
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
}
