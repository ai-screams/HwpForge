//! Caption types for shape objects (tables, images, textboxes, etc.).
//!
//! A [`Caption`] attaches descriptive text (typically numbered) below, above,
//! or beside a shape object. In HWPX, this maps to the `<hp:caption>` element
//! nested inside drawing objects like `<hp:tbl>`, `<hp:pic>`, `<hp:rect>`, etc.
//!
//! # Design
//!
//! Caption is a Core-level structural type. It holds position, gap, optional
//! width, whether it spans the object's outer margins (`full_size`, HWPX
//! `fullSz`), the box width Hancom saved when it differs from the object's
//! (`last_width`, HWPX `lastWidth`), and paragraph content. Both HWP 5.0 and
//! HWPX carry the last two, and the PDF renderer places the caption box with
//! them.
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::caption::{Caption, CaptionSide};
//! use hwpforge_core::paragraph::Paragraph;
//! use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
//!
//! let caption = Caption::new(vec![Paragraph::new(ParaShapeIndex::new(0))], CaptionSide::Bottom)
//!     .with_gap(HwpUnit::new(850).unwrap());
//! assert_eq!(caption.side, CaptionSide::Bottom);
//! assert!(!caption.full_size && caption.last_width.is_none());
//! ```

use hwpforge_foundation::HwpUnit;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::paragraph::Paragraph;

/// Default caption gap in HWPUNIT (~3mm). Used by [`Caption::default`] and [`Caption::new`].
pub const DEFAULT_CAPTION_GAP: i32 = 850;

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
#[non_exhaustive]
pub struct Caption {
    /// Position of the caption relative to the object.
    pub side: CaptionSide,
    /// Caption width in HwpUnit. `None` = auto (same as object width).
    pub width: Option<HwpUnit>,
    /// Gap between caption and object. Default: `HwpUnit(850)` (~3mm).
    pub gap: HwpUnit,
    /// Caption content paragraphs.
    pub paragraphs: Vec<Paragraph>,
    /// Whether the caption box spans the object's outer margins too
    /// (HWPX `fullSz`, HWP 5.0 caption property bit 2). Left out of JSON
    /// when false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub full_size: bool,
    /// The caption box width the file saved (HWPX `lastWidth`), kept only
    /// when it differs from the object's width; `None` means the object's
    /// width, which encoders write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_width: Option<HwpUnit>,
}

impl Default for Caption {
    fn default() -> Self {
        Self {
            side: CaptionSide::default(),
            width: None,
            gap: HwpUnit::new(DEFAULT_CAPTION_GAP).unwrap(),
            paragraphs: Vec::new(),
            full_size: false,
            last_width: None,
        }
    }
}

impl Caption {
    /// Creates a caption with the given paragraphs and side placement.
    ///
    /// Uses default gap (850 HWPUNIT ≈ 3mm) and auto width (`None`).
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::caption::{Caption, CaptionSide};
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::ParaShapeIndex;
    ///
    /// let cap = Caption::new(
    ///     vec![Paragraph::new(ParaShapeIndex::new(0))],
    ///     CaptionSide::Bottom,
    /// );
    /// assert_eq!(cap.side, CaptionSide::Bottom);
    /// assert_eq!(cap.gap.as_i32(), 850);
    /// assert!(cap.width.is_none());
    /// assert_eq!(cap.paragraphs.len(), 1);
    /// ```
    /// Creates a caption with the given paragraphs and side.
    pub fn new(paragraphs: Vec<Paragraph>, side: CaptionSide) -> Self {
        Self {
            side,
            width: None,
            gap: HwpUnit::new(DEFAULT_CAPTION_GAP).expect("DEFAULT_CAPTION_GAP is valid"),
            paragraphs,
            full_size: false,
            last_width: None,
        }
    }

    /// 캡션 문단 전부를 재귀 방문한다 (문단 안 중첩 포함).
    #[cfg(test)]
    pub(crate) fn walk_paragraphs_mut(&mut self, f: &mut dyn FnMut(&mut Paragraph)) {
        crate::walk::paragraphs_mut(crate::walk::NodeMut::Paragraphs(&mut self.paragraphs), f);
    }

    /// [`Self::walk_paragraphs_mut`] 의 불변 쌍둥이 — 방문 순서 동일.
    #[cfg(test)]
    pub(crate) fn walk_paragraphs(&self, f: &mut dyn FnMut(&Paragraph)) {
        crate::walk::paragraphs(crate::walk::Node::Paragraphs(&self.paragraphs), f);
    }

    /// Sets an explicit caption width.
    #[must_use]
    pub fn with_width(mut self, width: HwpUnit) -> Self {
        self.width = Some(width);
        self
    }

    /// Sets the gap between the caption and the object.
    #[must_use]
    pub fn with_gap(mut self, gap: HwpUnit) -> Self {
        self.gap = gap;
        self
    }

    /// Sets whether the caption spans the object's outer margins
    /// (see [`Caption::full_size`]).
    #[must_use]
    pub fn with_full_size(mut self, full_size: bool) -> Self {
        self.full_size = full_size;
        self
    }

    /// Sets the saved caption box width (see [`Caption::last_width`]).
    #[must_use]
    pub fn with_last_width(mut self, last_width: Option<HwpUnit>) -> Self {
        self.last_width = last_width;
        self
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
    fn caption_new_bottom() {
        let cap = Caption::new(vec![simple_paragraph()], CaptionSide::Bottom);
        assert_eq!(cap.side, CaptionSide::Bottom);
        assert_eq!(cap.gap.as_i32(), 850);
        assert!(cap.width.is_none());
        assert_eq!(cap.paragraphs.len(), 1);
    }

    #[test]
    fn caption_new_top() {
        let cap = Caption::new(vec![simple_paragraph(), simple_paragraph()], CaptionSide::Top);
        assert_eq!(cap.side, CaptionSide::Top);
        assert_eq!(cap.paragraphs.len(), 2);
    }

    #[test]
    fn caption_new_empty_paragraphs() {
        let cap = Caption::new(vec![], CaptionSide::Left);
        assert!(cap.paragraphs.is_empty());
        assert_eq!(cap.side, CaptionSide::Left);
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
            full_size: false,
            last_width: None,
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
            full_size: false,
            last_width: None,
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
            full_size: false,
            last_width: None,
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

    /// D: `full_size` and `last_width` are left out of JSON at their
    /// defaults, read as defaults when missing (JSON written before them),
    /// and kept otherwise.
    /// 이것을 실패시키는 것: `serde(default)` 삭제 · 생략 조건 삭제 · 값 버리기.
    #[test]
    fn caption_full_size_and_last_width_json() {
        let cap = Caption::default();
        let json = serde_json::to_value(&cap).unwrap();
        assert!(json.get("full_size").is_none() && json.get("last_width").is_none(), "{json}");
        assert_eq!(serde_json::from_value::<Caption>(json).unwrap(), cap);
        let set = Caption::default()
            .with_full_size(true)
            .with_last_width(Some(HwpUnit::new(48190).unwrap()));
        let json = serde_json::to_value(&set).unwrap();
        assert_eq!(
            (json["full_size"].clone(), json["last_width"].clone()),
            (true.into(), 48190.into())
        );
        assert_eq!(serde_json::from_value::<Caption>(json).unwrap(), set);
    }

    #[test]
    fn caption_new_empty_bottom_equals_default() {
        let from_new = Caption::new(vec![], CaptionSide::Bottom);
        let from_default = Caption::default();
        assert_eq!(from_new, from_default);
    }

    #[test]
    fn default_caption_gap_constant() {
        assert_eq!(super::DEFAULT_CAPTION_GAP, 850);
        let cap = Caption::default();
        assert_eq!(cap.gap.as_i32(), super::DEFAULT_CAPTION_GAP);
    }
}
