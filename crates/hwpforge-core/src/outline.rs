//! Shared paragraph classification for outline/list semantics.
//!
//! Single truth source for "what kind of paragraph is this" across format
//! encoders and read surfaces. The precedence chain was historically embedded
//! in the Markdown styled encoder; it is lifted here so every consumer
//! (Markdown export, document outline, read projections) agrees:
//!
//! 1. paragraph-shape outline level ([`StyleLookup::para_heading_level`]) —
//!    the format-agnostic truth source for headings
//! 2. paragraph-shape list semantics ([`StyleLookup::para_list_type`] plus
//!    [`para_list_level`](StyleLookup::para_list_level) /
//!    [`para_checked_state`](StyleLookup::para_checked_state))
//! 3. style heading level ([`StyleLookup::style_heading_level`])
//! 4. style-name list heuristic — Korean style names containing `글머리` or
//!    `개조` imply a bullet list, `번호` a numbered list. This is a documented
//!    heuristic inherited from the Markdown encoder and kept intentionally
//!    narrow; new name patterns need explicit justification.
//!
//! [`Paragraph::heading_level`] is **not** consulted: that field carries
//! titleMark / TOC-marker semantics, which is a different axis from
//! paragraph-shape outline lists.
//!
//! Classification is pure paragraph-shape/style semantics: paragraph text is
//! never consulted. Renderers keep their own empty-text guards (an empty
//! heading or list item renders as nothing, but that is a rendering concern).

use crate::{Paragraph, StyleLookup};

/// Which axis produced a [`ParaKind::Heading`] classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadingSource {
    /// Paragraph-shape outline metadata
    /// ([`StyleLookup::para_heading_level`]) — highest priority.
    ParaShape,
    /// Style registry heading level
    /// ([`StyleLookup::style_heading_level`]) — third priority.
    Style,
}

/// Which axis produced a [`ParaKind::ListItem`] classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListSource {
    /// Paragraph-shape list heading ([`StyleLookup::para_list_type`]) —
    /// second priority.
    ParaShape,
    /// Style-name heuristic (`글머리`/`개조`/`번호`) — lowest priority.
    StyleName,
}

/// List marker family of a [`ParaKind::ListItem`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListItemKind {
    /// Bullet marker. Checkable bullets stay in this family (gotcha #8 —
    /// checkable is still `BULLET`); see the `checked` field.
    Bullet,
    /// Ordered/numbered marker.
    Number,
}

/// Semantic kind of a paragraph as resolved through the shared precedence
/// chain.
///
/// This enum is deliberately exhaustive (no `#[non_exhaustive]`): consumers
/// match on it to render or project paragraphs, and a new classification kind
/// must force every consumer to decide its handling at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParaKind {
    /// Outline or style heading.
    Heading {
        /// Heading depth, clamped to `1..=6`.
        level: u8,
        /// Axis that produced the classification.
        source: HeadingSource,
    },
    /// Ordered / bullet (optionally checkable) list item.
    ListItem {
        /// Marker family.
        kind: ListItemKind,
        /// Zero-based nesting depth. The style-name fallback carries no
        /// depth information and always reports `0`.
        level: u8,
        /// Checkbox state for checkable bullets; `None` = not checkable.
        checked: Option<bool>,
        /// Axis that produced the classification.
        source: ListSource,
    },
    /// Plain body paragraph (no outline/list semantics found).
    Body,
}

/// Classifies `paragraph` through the shared outline/list precedence chain.
///
/// Unknown [`StyleLookup::para_list_type`] strings degrade to
/// [`ListItemKind::Bullet`], mirroring the historical Markdown renderer
/// (only `"NUMBER"` selects the numbered family).
#[must_use]
pub fn classify_paragraph(paragraph: &Paragraph, styles: &dyn StyleLookup) -> ParaKind {
    if let Some(level) = styles.para_heading_level(paragraph.para_shape_id) {
        return ParaKind::Heading { level: level.clamp(1, 6), source: HeadingSource::ParaShape };
    }

    if let Some(list_type) = styles.para_list_type(paragraph.para_shape_id) {
        let kind = if list_type == "NUMBER" { ListItemKind::Number } else { ListItemKind::Bullet };
        return ParaKind::ListItem {
            kind,
            level: styles.para_list_level(paragraph.para_shape_id).unwrap_or(0),
            checked: styles.para_checked_state(paragraph.para_shape_id),
            source: ListSource::ParaShape,
        };
    }

    if let Some(style_id) = paragraph.style_id {
        if let Some(level) = styles.style_heading_level(style_id) {
            return ParaKind::Heading { level: level.clamp(1, 6), source: HeadingSource::Style };
        }
        if let Some(name) = styles.style_name(style_id) {
            if name.contains("글머리") || name.contains("개조") {
                return ParaKind::ListItem {
                    kind: ListItemKind::Bullet,
                    level: 0,
                    checked: None,
                    source: ListSource::StyleName,
                };
            }
            if name.contains("번호") {
                return ParaKind::ListItem {
                    kind: ListItemKind::Number,
                    level: 0,
                    checked: None,
                    source: ListSource::StyleName,
                };
            }
        }
    }

    ParaKind::Body
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_foundation::{ParaShapeIndex, StyleIndex};
    use std::collections::HashMap;

    #[derive(Default)]
    struct MockStyles {
        para_headings: HashMap<usize, u8>,
        para_list_types: HashMap<usize, &'static str>,
        para_list_levels: HashMap<usize, u8>,
        para_checked: HashMap<usize, bool>,
        style_headings: HashMap<usize, u8>,
        style_names: HashMap<usize, &'static str>,
    }

    impl StyleLookup for MockStyles {
        fn para_heading_level(&self, id: ParaShapeIndex) -> Option<u8> {
            self.para_headings.get(&id.get()).copied()
        }
        fn para_list_type(&self, id: ParaShapeIndex) -> Option<&str> {
            self.para_list_types.get(&id.get()).copied()
        }
        fn para_list_level(&self, id: ParaShapeIndex) -> Option<u8> {
            self.para_list_levels.get(&id.get()).copied()
        }
        fn para_checked_state(&self, id: ParaShapeIndex) -> Option<bool> {
            self.para_checked.get(&id.get()).copied()
        }
        fn style_heading_level(&self, id: StyleIndex) -> Option<u8> {
            self.style_headings.get(&id.get()).copied()
        }
        fn style_name(&self, id: StyleIndex) -> Option<&str> {
            self.style_names.get(&id.get()).copied()
        }
    }

    fn para(shape: usize) -> Paragraph {
        Paragraph::new(ParaShapeIndex::new(shape))
    }

    fn styled_para(shape: usize, style: usize) -> Paragraph {
        let mut p = para(shape);
        p.style_id = Some(StyleIndex::new(style));
        p
    }

    // -- edge cases -------------------------------------------------------

    #[test]
    fn noop_styles_and_no_style_id_is_body() {
        struct Noop;
        impl StyleLookup for Noop {}
        assert_eq!(classify_paragraph(&para(0), &Noop), ParaKind::Body);
    }

    #[test]
    fn para_heading_level_zero_clamps_to_one() {
        let styles = MockStyles { para_headings: [(0, 0)].into(), ..Default::default() };
        assert_eq!(
            classify_paragraph(&para(0), &styles),
            ParaKind::Heading { level: 1, source: HeadingSource::ParaShape }
        );
    }

    #[test]
    fn para_heading_level_255_clamps_to_six() {
        let styles = MockStyles { para_headings: [(0, 255)].into(), ..Default::default() };
        assert_eq!(
            classify_paragraph(&para(0), &styles),
            ParaKind::Heading { level: 6, source: HeadingSource::ParaShape }
        );
    }

    #[test]
    fn style_heading_level_clamps_like_para_heading() {
        let styles = MockStyles { style_headings: [(3, 9)].into(), ..Default::default() };
        assert_eq!(
            classify_paragraph(&styled_para(0, 3), &styles),
            ParaKind::Heading { level: 6, source: HeadingSource::Style }
        );
    }

    #[test]
    fn unknown_list_type_string_degrades_to_bullet() {
        let styles = MockStyles { para_list_types: [(0, "WEIRD")].into(), ..Default::default() };
        assert_eq!(
            classify_paragraph(&para(0), &styles),
            ParaKind::ListItem {
                kind: ListItemKind::Bullet,
                level: 0,
                checked: None,
                source: ListSource::ParaShape,
            }
        );
    }

    #[test]
    fn missing_list_level_defaults_to_zero() {
        let styles = MockStyles { para_list_types: [(0, "BULLET")].into(), ..Default::default() };
        let ParaKind::ListItem { level, .. } = classify_paragraph(&para(0), &styles) else {
            panic!("expected list item");
        };
        assert_eq!(level, 0);
    }

    // -- precedence chain -------------------------------------------------

    #[test]
    fn para_heading_beats_para_list() {
        let styles = MockStyles {
            para_headings: [(0, 2)].into(),
            para_list_types: [(0, "NUMBER")].into(),
            ..Default::default()
        };
        assert_eq!(
            classify_paragraph(&para(0), &styles),
            ParaKind::Heading { level: 2, source: HeadingSource::ParaShape }
        );
    }

    #[test]
    fn para_list_beats_style_heading() {
        let styles = MockStyles {
            para_list_types: [(0, "BULLET")].into(),
            style_headings: [(1, 1)].into(),
            ..Default::default()
        };
        let kind = classify_paragraph(&styled_para(0, 1), &styles);
        assert!(matches!(kind, ParaKind::ListItem { source: ListSource::ParaShape, .. }));
    }

    #[test]
    fn style_heading_beats_style_name_list() {
        let styles = MockStyles {
            style_headings: [(1, 3)].into(),
            style_names: [(1, "번호 개요")].into(),
            ..Default::default()
        };
        assert_eq!(
            classify_paragraph(&styled_para(0, 1), &styles),
            ParaKind::Heading { level: 3, source: HeadingSource::Style }
        );
    }

    // -- normal cases -----------------------------------------------------

    #[test]
    fn number_list_type_maps_to_number_kind() {
        let styles = MockStyles {
            para_list_types: [(0, "NUMBER")].into(),
            para_list_levels: [(0, 2)].into(),
            ..Default::default()
        };
        assert_eq!(
            classify_paragraph(&para(0), &styles),
            ParaKind::ListItem {
                kind: ListItemKind::Number,
                level: 2,
                checked: None,
                source: ListSource::ParaShape,
            }
        );
    }

    #[test]
    fn checkable_bullet_carries_checked_state() {
        let styles = MockStyles {
            para_list_types: [(0, "BULLET")].into(),
            para_checked: [(0, true)].into(),
            ..Default::default()
        };
        assert_eq!(
            classify_paragraph(&para(0), &styles),
            ParaKind::ListItem {
                kind: ListItemKind::Bullet,
                level: 0,
                checked: Some(true),
                source: ListSource::ParaShape,
            }
        );
    }

    #[test]
    fn style_name_bullet_patterns_classify_as_bullet() {
        for name in ["글머리표", "개조식 본문"] {
            let styles = MockStyles { style_names: [(2, name)].into(), ..Default::default() };
            assert_eq!(
                classify_paragraph(&styled_para(0, 2), &styles),
                ParaKind::ListItem {
                    kind: ListItemKind::Bullet,
                    level: 0,
                    checked: None,
                    source: ListSource::StyleName,
                },
                "style name {name:?} should imply a bullet list",
            );
        }
    }

    #[test]
    fn style_name_number_pattern_classifies_as_number() {
        let styles =
            MockStyles { style_names: [(2, "번호 목록")].into(), ..Default::default() };
        assert_eq!(
            classify_paragraph(&styled_para(0, 2), &styles),
            ParaKind::ListItem {
                kind: ListItemKind::Number,
                level: 0,
                checked: None,
                source: ListSource::StyleName,
            }
        );
    }

    #[test]
    fn unrecognized_style_name_is_body() {
        let styles = MockStyles { style_names: [(2, "바탕글")].into(), ..Default::default() };
        assert_eq!(classify_paragraph(&styled_para(0, 2), &styles), ParaKind::Body);
    }

    #[test]
    fn core_heading_level_field_is_ignored() {
        // Paragraph.heading_level is the titleMark axis, not outline truth.
        let mut p = para(0);
        p.heading_level = Some(3);
        struct Noop;
        impl StyleLookup for Noop {}
        assert_eq!(classify_paragraph(&p, &Noop), ParaKind::Body);
    }
}
