//! Caption types for shape objects (tables, images, textboxes, etc.).
//!
//! A [`Caption`] attaches descriptive text (typically numbered) below, above,
//! or beside a shape object. In HWPX, this maps to the `<hp:caption>` element
//! nested inside drawing objects like `<hp:tbl>`, `<hp:pic>`, `<hp:rect>`, etc.
//!
//! # Design
//!
//! Caption is a Core-level structural type. It holds position, gap, optional
//! width, and paragraph content. HWPX-specific attributes (`fullSz`, `lastWidth`)
//! belong in the Schema layer, not here.
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::caption::{Caption, CaptionSide};
//! use hwpforge_core::paragraph::Paragraph;
//! use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
//!
//! let caption = Caption {
//!     side: CaptionSide::Bottom,
//!     width: None,
//!     gap: HwpUnit::new(850).unwrap(),
//!     paragraphs: vec![Paragraph::new(ParaShapeIndex::new(0))],
//! };
//! assert_eq!(caption.side, CaptionSide::Bottom);
//! ```

use hwpforge_foundation::HwpUnit;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::paragraph::Paragraph;

/// Caption attached to a shape object (table, image, textbox, etc.).
///
/// Contains position, gap distance, optional explicit width, and the
/// caption's paragraph content. Empty paragraphs are valid (한글 allows it).
///
/// # Default
///
/// Default caption: side = Bottom, width = None, gap = 850 HWPUNIT (~3mm),
/// paragraphs = empty.
///
/// # Examples
///
/// ```
/// use hwpforge_core::caption::{Caption, CaptionSide};
/// use hwpforge_foundation::HwpUnit;
///
/// let cap = Caption::default();
/// assert_eq!(cap.side, CaptionSide::Bottom);
/// assert_eq!(cap.gap.as_i32(), 850);
/// assert!(cap.width.is_none());
/// assert!(cap.paragraphs.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Caption {
    /// Position of the caption relative to the object.
    pub side: CaptionSide,
    /// Caption width in HwpUnit. `None` = auto (same as object width).
    pub width: Option<HwpUnit>,
    /// Gap between caption and object. Default: `HwpUnit(850)` (~3mm).
    pub gap: HwpUnit,
    /// Caption content paragraphs.
    pub paragraphs: Vec<Paragraph>,
}

impl Default for Caption {
    fn default() -> Self {
        Self {
            side: CaptionSide::default(),
            width: None,
            // 850 HWPUNIT ≈ 3mm. unwrap is safe: 850 is well within valid range.
            gap: HwpUnit::new(850).unwrap(),
            paragraphs: Vec::new(),
        }
    }
}

/// Position of caption relative to its parent object.
///
/// # Default
///
/// Defaults to [`CaptionSide::Bottom`], the most common position in
/// Korean government documents.
///
/// # Examples
///
/// ```
/// use hwpforge_core::caption::CaptionSide;
///
/// assert_eq!(CaptionSide::default(), CaptionSide::Bottom);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum CaptionSide {
    /// Caption appears to the left of the object.
    Left,
    /// Caption appears to the right of the object.
    Right,
    /// Caption appears above the object.
    Top,
    /// Caption appears below the object (most common).
    #[default]
    Bottom,
}

impl std::fmt::Display for CaptionSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Left => write!(f, "Left"),
            Self::Right => write!(f, "Right"),
            Self::Top => write!(f, "Top"),
            Self::Bottom => write!(f, "Bottom"),
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
            vec![Run::text("Figure 1: Example", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )
    }

    #[test]
    fn caption_default_values() {
        let cap = Caption::default();
        assert_eq!(cap.side, CaptionSide::Bottom);
        assert!(cap.width.is_none());
        assert_eq!(cap.gap.as_i32(), 850);
        assert!(cap.paragraphs.is_empty());
    }

    #[test]
    fn caption_side_default_is_bottom() {
        assert_eq!(CaptionSide::default(), CaptionSide::Bottom);
    }

    #[test]
    fn caption_side_all_variants() {
        let sides = [CaptionSide::Left, CaptionSide::Right, CaptionSide::Top, CaptionSide::Bottom];
        assert_eq!(sides.len(), 4);

        // Display
        assert_eq!(CaptionSide::Left.to_string(), "Left");
        assert_eq!(CaptionSide::Right.to_string(), "Right");
        assert_eq!(CaptionSide::Top.to_string(), "Top");
        assert_eq!(CaptionSide::Bottom.to_string(), "Bottom");
    }

    #[test]
    fn caption_serde_roundtrip() {
        let cap = Caption {
            side: CaptionSide::Top,
            width: Some(HwpUnit::from_mm(80.0).unwrap()),
            gap: HwpUnit::new(1000).unwrap(),
            paragraphs: vec![simple_paragraph()],
        };
        let json = serde_json::to_string(&cap).unwrap();
        let back: Caption = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, back);
    }

    #[test]
    fn caption_serde_roundtrip_default() {
        let cap = Caption::default();
        let json = serde_json::to_string(&cap).unwrap();
        let back: Caption = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, back);
    }

    #[test]
    fn caption_side_serde_roundtrip() {
        for side in [CaptionSide::Left, CaptionSide::Right, CaptionSide::Top, CaptionSide::Bottom] {
            let json = serde_json::to_string(&side).unwrap();
            let back: CaptionSide = serde_json::from_str(&json).unwrap();
            assert_eq!(side, back);
        }
    }

    #[test]
    fn caption_with_paragraphs() {
        let cap = Caption {
            side: CaptionSide::Bottom,
            width: None,
            gap: HwpUnit::new(850).unwrap(),
            paragraphs: vec![simple_paragraph(), simple_paragraph()],
        };
        assert_eq!(cap.paragraphs.len(), 2);
    }

    #[test]
    fn caption_empty_paragraphs() {
        // Empty paragraphs are valid (한글 allows it)
        let cap = Caption { paragraphs: vec![], ..Caption::default() };
        assert!(cap.paragraphs.is_empty());
        // Should still serialize/deserialize fine
        let json = serde_json::to_string(&cap).unwrap();
        let back: Caption = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, back);
    }

    #[test]
    fn caption_clone_independence() {
        let cap = Caption {
            side: CaptionSide::Left,
            width: Some(HwpUnit::from_mm(50.0).unwrap()),
            gap: HwpUnit::new(500).unwrap(),
            paragraphs: vec![simple_paragraph()],
        };
        let mut cloned = cap.clone();
        cloned.side = CaptionSide::Right;
        assert_eq!(cap.side, CaptionSide::Left);
    }

    #[test]
    fn caption_equality() {
        let a = Caption::default();
        let b = Caption::default();
        assert_eq!(a, b);

        let c = Caption { side: CaptionSide::Top, ..Caption::default() };
        assert_ne!(a, c);
    }

    #[test]
    fn caption_side_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(CaptionSide::Left);
        set.insert(CaptionSide::Right);
        set.insert(CaptionSide::Left);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn caption_side_copy() {
        let side = CaptionSide::Top;
        let copied = side;
        assert_eq!(side, copied);
    }

    #[test]
    fn caption_custom_gap() {
        let cap = Caption { gap: HwpUnit::from_mm(5.0).unwrap(), ..Caption::default() };
        assert!(cap.gap.as_i32() > 850);
    }
}
