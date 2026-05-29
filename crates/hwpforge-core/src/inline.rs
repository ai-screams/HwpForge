//! Rich inline content carried inside a single [`Run`](crate::run::Run).
//!
//! Most runs hold plain text (modeled as `RunContent::Text(String)`),
//! but a handful of HWPX inline elements carry per-occurrence attributes
//! that cannot survive a `String` round-trip:
//!
//! - `<hp:tab width="..." leader="..." type="..."/>` — explicit tab stop
//!   position, leader glyph, and alignment, emitted inside `<hp:t>` mixed
//!   content
//!
//! For runs that need to carry any such attribute payload, projection
//! emits `RunContent::InlineText(InlineText)` (a non-exhaustive enum
//! variant added alongside `Text(String)` for backward compatibility).
//! Plain text runs continue to use `Text(String)` so the 18-file
//! `RunContent::Text` surface is undisturbed.
//!
//! See `.docs/research/2026-05-26_tab_fidelity_bugs.md` (Bug A / Phase 2)
//! for the underlying investigation.

use hwpforge_foundation::HwpUnit;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A sequence of inline segments that compose a single `<hp:t>` element
/// with mixed content.
///
/// `segments` is normalized so adjacent [`InlineSegment::Plain`] entries
/// are merged. Callers that only need the plain-text equivalent (no tab
/// attributes) can use [`InlineText::plain_text`] which renders each
/// [`InlineSegment::Tab`] as a `\t` character.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct InlineText {
    /// Ordered inline segments. Always non-empty for a meaningful run;
    /// projection that produces an empty `InlineText` should emit
    /// `RunContent::Text(String::new())` instead.
    pub segments: Vec<InlineSegment>,
}

/// One typed segment inside an [`InlineText`].
///
/// Marked `#[non_exhaustive]` so future inline elements (ruby, hyphen
/// hints, etc.) can be added without breaking existing matches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum InlineSegment {
    /// Plain text. May contain `\n`, `U+00A0` (NBSP), `U+001F` (fwSpace)
    /// — those are still handled as character-level sentinels by the
    /// HWPX encoder, matching the `Text(String)` behavior.
    Plain(String),
    /// An inline tab with explicit per-occurrence attributes.
    Tab(InlineTabAttr),
}

/// Per-occurrence attributes for an inline `<hp:tab>` element.
///
/// Mirrors the HWP5 0x09 control char's 14-byte inline payload (only
/// the first 6 bytes are meaningful per HWP 5.0 spec §1.5):
///
/// | offset | width    | meaning                                |
/// |-------:|----------|----------------------------------------|
/// | 0..4   | `u32 LE` | `width` — HwpUnit, distance to the stop |
/// | 4      | `u8`     | `leader` — fill glyph (raw HWP5 enum)   |
/// | 5      | `u8`     | `tab_type` — 0=Left, 1=Right, 2=Center, 3=Decimal |
///
/// The raw `leader` / `tab_type` integers are preserved verbatim and
/// emitted into HWPX as `<hp:tab leader="3" type="1"/>` — Hancom uses
/// the HWP5 numeric encoding directly for inline tabs (unlike the
/// header-level `<hh:tabItem>` which uses enum strings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InlineTabAttr {
    /// Position (HwpUnit) at which the tab stops. Truth-pair fixtures
    /// show this is the original HWP5 raw value, NOT the halved
    /// HwpUnitChar form used by `<hh:tabPr>` stops.
    pub width: HwpUnit,
    /// Raw HWP5 fill_type byte (0..=4 known; openhwp: 0=None, 1=Dot,
    /// 2=LongDash, 3=Dash, 4=Underscore).
    pub leader: u8,
    /// Raw HWP5 tab_type byte (0=Left, 1=Right, 2=Center, 3=Decimal).
    pub tab_type: u8,
}

impl InlineTabAttr {
    /// Returns `true` when all attributes are zero — i.e. a default tab
    /// that is semantically identical to a bare `<hp:tab/>` and does
    /// not need the rich [`InlineSegment::Tab`] representation.
    pub fn is_default(&self) -> bool {
        self.width.as_i32() == 0 && self.leader == 0 && self.tab_type == 0
    }
}

impl InlineText {
    /// Constructs an [`InlineText`] from raw segments, dropping empty
    /// `Plain` runs and merging adjacent ones to keep the canonical
    /// form predictable for equality comparisons and HWPX emit.
    pub fn from_segments(segments: impl IntoIterator<Item = InlineSegment>) -> Self {
        let mut out: Vec<InlineSegment> = Vec::new();
        for seg in segments {
            match seg {
                InlineSegment::Plain(s) if s.is_empty() => continue,
                InlineSegment::Plain(s) => match out.last_mut() {
                    Some(InlineSegment::Plain(prev)) => prev.push_str(&s),
                    _ => out.push(InlineSegment::Plain(s)),
                },
                other => out.push(other),
            }
        }
        Self { segments: out }
    }

    /// Returns the plain-text equivalent: each [`InlineSegment::Tab`]
    /// becomes a `\t` character. Useful for callers (Markdown bridge,
    /// CLI search, etc.) that cannot represent tab attributes.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for seg in &self.segments {
            match seg {
                InlineSegment::Plain(s) => out.push_str(s),
                InlineSegment::Tab(_) => out.push('\t'),
            }
        }
        out
    }

    /// Returns `true` when the inline text carries no information that
    /// `RunContent::Text(String)` could not also represent — i.e. every
    /// segment is `Plain` or a default tab. Projection can downgrade
    /// such [`InlineText`] back to `Text(String)` to keep the audit
    /// baseline simple.
    pub fn is_downgradable(&self) -> bool {
        self.segments.iter().all(|seg| match seg {
            InlineSegment::Plain(_) => true,
            InlineSegment::Tab(attr) => attr.is_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(width: i32, leader: u8, tab_type: u8) -> InlineTabAttr {
        InlineTabAttr { width: HwpUnit::new(width).unwrap(), leader, tab_type }
    }

    #[test]
    fn from_segments_merges_adjacent_plain_runs() {
        let it = InlineText::from_segments([
            InlineSegment::Plain("LE".into()),
            InlineSegment::Plain("FT".into()),
            InlineSegment::Tab(attr(12488, 3, 1)),
            InlineSegment::Plain("RI".into()),
            InlineSegment::Plain("GHT".into()),
        ]);
        assert_eq!(
            it.segments,
            vec![
                InlineSegment::Plain("LEFT".into()),
                InlineSegment::Tab(attr(12488, 3, 1)),
                InlineSegment::Plain("RIGHT".into()),
            ]
        );
    }

    #[test]
    fn from_segments_drops_empty_plain_entries() {
        let it = InlineText::from_segments([
            InlineSegment::Plain(String::new()),
            InlineSegment::Tab(attr(0, 0, 0)),
            InlineSegment::Plain(String::new()),
        ]);
        assert_eq!(it.segments, vec![InlineSegment::Tab(attr(0, 0, 0))]);
    }

    #[test]
    fn plain_text_renders_tabs_as_horizontal_tab_chars() {
        let it = InlineText::from_segments([
            InlineSegment::Plain("a".into()),
            InlineSegment::Tab(attr(12488, 3, 1)),
            InlineSegment::Plain("b".into()),
            InlineSegment::Tab(attr(0, 0, 0)),
            InlineSegment::Plain("c".into()),
        ]);
        assert_eq!(it.plain_text(), "a\tb\tc");
    }

    #[test]
    fn is_default_attr() {
        assert!(attr(0, 0, 0).is_default());
        assert!(!attr(1, 0, 0).is_default());
        assert!(!attr(0, 1, 0).is_default());
        assert!(!attr(0, 0, 1).is_default());
    }

    #[test]
    fn is_downgradable_true_for_plain_and_default_tab_only() {
        assert!(InlineText::from_segments([InlineSegment::Plain("hi".into())]).is_downgradable());
        assert!(InlineText::from_segments([
            InlineSegment::Plain("a".into()),
            InlineSegment::Tab(attr(0, 0, 0)),
            InlineSegment::Plain("b".into()),
        ])
        .is_downgradable());
        assert!(
            !InlineText::from_segments([InlineSegment::Tab(attr(12488, 3, 1))]).is_downgradable()
        );
    }
}
