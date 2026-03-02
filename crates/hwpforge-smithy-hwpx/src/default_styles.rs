//! Version-aware default style definitions for 한글 (Hangul Word Processor).
//!
//! Different versions of 한글 include different numbers of built-in styles.
//! When encoding an HWPX file, the encoder must inject ALL default styles
//! in the correct order so that style IDs match what 한글 expects.
//!
//! # Critical Ordering Note
//!
//! In 한글 Modern (2022+), 개요 8–10 are **inserted** at IDs 9–11, pushing
//! Classic styles like 쪽 번호 from ID 9 to ID 12. This is NOT an append
//! operation — the arrays must be stored as complete, ordered, version-specific
//! lists.
//!
//! # Usage
//!
//! ```
//! use hwpforge_smithy_hwpx::default_styles::{HancomStyleSet, DefaultStyleEntry};
//!
//! let styles: &[DefaultStyleEntry] = HancomStyleSet::Modern.default_styles();
//! assert_eq!(styles[0].name, "바탕글");
//! assert_eq!(styles.len(), 22);
//! ```

/// A single entry in a 한글 default style list.
///
/// Each entry corresponds to one `<hh:style>` element that 한글 expects
/// to find in `header.xml` at a specific positional ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultStyleEntry {
    /// Korean style name (e.g. `"바탕글"`, `"개요 1"`).
    pub name: &'static str,
    /// English style name (e.g. `"Normal"`, `"Outline 1"`).
    pub eng_name: &'static str,
    /// Style type: `"PARA"` for paragraph styles, `"CHAR"` for character styles.
    pub style_type: &'static str,
}

impl DefaultStyleEntry {
    /// Returns `true` if this is a character style (`"CHAR"`).
    ///
    /// Character styles use `nextStyleIDRef=0` (바탕글) instead of
    /// self-referencing like paragraph styles.
    pub fn is_char_style(&self) -> bool {
        self.style_type == "CHAR"
    }
}

/// The set of default styles to inject when building an HWPX file.
///
/// Different versions of 한글 ship with different built-in style tables.
/// Choosing the wrong set causes style ID mismatches, which can break
/// automatic numbering, table-of-contents generation, and other features.
///
/// # Variant ordering
///
/// Use [`HancomStyleSet::Modern`] (the default) unless you are targeting
/// files for 한글 2020 or earlier ([`Classic`][HancomStyleSet::Classic])
/// or 한글 2025+ ([`Latest`][HancomStyleSet::Latest]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum HancomStyleSet {
    /// 18 built-in styles — 한글 2014 through 2020.
    ///
    /// 개요 8–10 are absent; 쪽 번호 is at ID 9.
    Classic,
    /// 22 built-in styles — 한글 2022 and later.
    ///
    /// 개요 8–10 are **inserted** at IDs 9–11, shifting 쪽 번호 to ID 12.
    /// 캡션 (Caption) style is added at ID 21.
    ///
    /// This is the default variant because 한글 2022+ is now the most
    /// widely deployed version.
    #[default]
    Modern,
    /// 23 built-in styles — 한글 2025 and later.
    ///
    /// Same as [`Modern`][HancomStyleSet::Modern] with the addition of
    /// 줄 번호 (Line Number) at ID 22.
    Latest,
}

impl HancomStyleSet {
    /// Returns the complete ordered default-style array for this version.
    ///
    /// The slice index corresponds directly to the `id` attribute that
    /// must be written to `<hh:style id="…">` in `header.xml`.
    pub fn default_styles(&self) -> &'static [DefaultStyleEntry] {
        match self {
            Self::Classic => &CLASSIC_STYLES,
            Self::Modern => &MODERN_STYLES,
            Self::Latest => &LATEST_STYLES,
        }
    }

    /// Returns the number of default styles for this version.
    ///
    /// Equivalent to `self.default_styles().len()`.
    pub fn count(&self) -> usize {
        self.default_styles().len()
    }
}

// ── Helper macro ───────────────────────────────────────────────────────────

macro_rules! entry {
    ($name:expr, $eng:expr, $ty:expr) => {
        DefaultStyleEntry { name: $name, eng_name: $eng, style_type: $ty }
    };
}

// ── Classic (18 styles, 한글 2014–2020) ───────────────────────────────────

/// Default styles shipped with 한글 2014 through 2020.
///
/// The 개요 8–10 (Outline 8–10) styles are absent; 쪽 번호 sits at ID 9.
const CLASSIC_STYLES: [DefaultStyleEntry; 18] = [
    entry!("바탕글", "Normal", "PARA"),         //  0
    entry!("본문", "Body", "PARA"),             //  1
    entry!("개요 1", "Outline 1", "PARA"),      //  2
    entry!("개요 2", "Outline 2", "PARA"),      //  3
    entry!("개요 3", "Outline 3", "PARA"),      //  4
    entry!("개요 4", "Outline 4", "PARA"),      //  5
    entry!("개요 5", "Outline 5", "PARA"),      //  6
    entry!("개요 6", "Outline 6", "PARA"),      //  7
    entry!("개요 7", "Outline 7", "PARA"),      //  8
    entry!("쪽 번호", "Page Number", "CHAR"),   //  9
    entry!("머리말", "Header", "PARA"),         // 10
    entry!("각주", "Footnote", "PARA"),         // 11
    entry!("미주", "Endnote", "PARA"),          // 12
    entry!("메모", "Memo", "PARA"),             // 13
    entry!("차례 제목", "TOC Heading", "PARA"), // 14
    entry!("차례 1", "TOC 1", "PARA"),          // 15
    entry!("차례 2", "TOC 2", "PARA"),          // 16
    entry!("차례 3", "TOC 3", "PARA"),          // 17
];

// ── Modern (22 styles, 한글 2022+) ─────────────────────────────────────────

/// Default styles shipped with 한글 2022 and later.
///
/// 개요 8–10 are **inserted** at IDs 9–11 (not appended), which shifts
/// all subsequent IDs up by 3 compared to Classic. 쪽 번호 moves from
/// Classic ID 9 to Modern ID 12.
///
/// Verified against golden fixture `tests/fixtures/textbox.hwpx`.
const MODERN_STYLES: [DefaultStyleEntry; 22] = [
    entry!("바탕글", "Normal", "PARA"),         //  0
    entry!("본문", "Body", "PARA"),             //  1
    entry!("개요 1", "Outline 1", "PARA"),      //  2
    entry!("개요 2", "Outline 2", "PARA"),      //  3
    entry!("개요 3", "Outline 3", "PARA"),      //  4
    entry!("개요 4", "Outline 4", "PARA"),      //  5
    entry!("개요 5", "Outline 5", "PARA"),      //  6
    entry!("개요 6", "Outline 6", "PARA"),      //  7
    entry!("개요 7", "Outline 7", "PARA"),      //  8
    entry!("개요 8", "Outline 8", "PARA"),      //  9  ← inserted (not Classic ID 9)
    entry!("개요 9", "Outline 9", "PARA"),      // 10
    entry!("개요 10", "Outline 10", "PARA"),    // 11
    entry!("쪽 번호", "Page Number", "CHAR"),   // 12  ← shifted from Classic ID 9
    entry!("머리말", "Header", "PARA"),         // 13
    entry!("각주", "Footnote", "PARA"),         // 14
    entry!("미주", "Endnote", "PARA"),          // 15
    entry!("메모", "Memo", "PARA"),             // 16
    entry!("차례 제목", "TOC Heading", "PARA"), // 17
    entry!("차례 1", "TOC 1", "PARA"),          // 18
    entry!("차례 2", "TOC 2", "PARA"),          // 19
    entry!("차례 3", "TOC 3", "PARA"),          // 20
    entry!("캡션", "Caption", "PARA"),          // 21
];

// ── Latest (23 styles, 한글 2025+) ─────────────────────────────────────────

/// Default styles shipped with 한글 2025 and later.
///
/// Identical to [`MODERN_STYLES`] with the addition of 줄 번호 (Line Number)
/// as a character style at ID 22.
const LATEST_STYLES: [DefaultStyleEntry; 23] = [
    entry!("바탕글", "Normal", "PARA"),         //  0
    entry!("본문", "Body", "PARA"),             //  1
    entry!("개요 1", "Outline 1", "PARA"),      //  2
    entry!("개요 2", "Outline 2", "PARA"),      //  3
    entry!("개요 3", "Outline 3", "PARA"),      //  4
    entry!("개요 4", "Outline 4", "PARA"),      //  5
    entry!("개요 5", "Outline 5", "PARA"),      //  6
    entry!("개요 6", "Outline 6", "PARA"),      //  7
    entry!("개요 7", "Outline 7", "PARA"),      //  8
    entry!("개요 8", "Outline 8", "PARA"),      //  9
    entry!("개요 9", "Outline 9", "PARA"),      // 10
    entry!("개요 10", "Outline 10", "PARA"),    // 11
    entry!("쪽 번호", "Page Number", "CHAR"),   // 12
    entry!("머리말", "Header", "PARA"),         // 13
    entry!("각주", "Footnote", "PARA"),         // 14
    entry!("미주", "Endnote", "PARA"),          // 15
    entry!("메모", "Memo", "PARA"),             // 16
    entry!("차례 제목", "TOC Heading", "PARA"), // 17
    entry!("차례 1", "TOC 1", "PARA"),          // 18
    entry!("차례 2", "TOC 2", "PARA"),          // 19
    entry!("차례 3", "TOC 3", "PARA"),          // 20
    entry!("캡션", "Caption", "PARA"),          // 21
    entry!("줄 번호", "Line Number", "CHAR"),   // 22  ← new in 2025
];

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_has_18_styles() {
        assert_eq!(HancomStyleSet::Classic.count(), 18);
        assert_eq!(HancomStyleSet::Classic.default_styles().len(), 18);
    }

    #[test]
    fn modern_has_22_styles() {
        assert_eq!(HancomStyleSet::Modern.count(), 22);
        assert_eq!(HancomStyleSet::Modern.default_styles().len(), 22);
    }

    #[test]
    fn latest_has_23_styles() {
        assert_eq!(HancomStyleSet::Latest.count(), 23);
        assert_eq!(HancomStyleSet::Latest.default_styles().len(), 23);
    }

    #[test]
    fn modern_is_default() {
        assert_eq!(HancomStyleSet::default(), HancomStyleSet::Modern);
    }

    #[test]
    fn all_styles_start_with_batanggeul() {
        for set in [HancomStyleSet::Classic, HancomStyleSet::Modern, HancomStyleSet::Latest] {
            let styles = set.default_styles();
            assert_eq!(styles[0].name, "바탕글");
            assert_eq!(styles[0].eng_name, "Normal");
            assert_eq!(styles[0].style_type, "PARA");
        }
    }

    #[test]
    fn modern_outline_8_at_index_9() {
        // Critical: 한글 inserts 개요 8-10 at positions 9-11, NOT appended
        let styles = HancomStyleSet::Modern.default_styles();
        assert_eq!(styles[9].name, "개요 8");
        assert_eq!(styles[9].style_type, "PARA");
    }

    #[test]
    fn classic_page_number_at_index_9() {
        let styles = HancomStyleSet::Classic.default_styles();
        assert_eq!(styles[9].name, "쪽 번호");
        assert_eq!(styles[9].style_type, "CHAR");
    }

    #[test]
    fn modern_page_number_at_index_12() {
        // 쪽 번호 shifts from Classic id=9 to Modern id=12
        let styles = HancomStyleSet::Modern.default_styles();
        assert_eq!(styles[12].name, "쪽 번호");
        assert_eq!(styles[12].style_type, "CHAR");
    }

    #[test]
    fn latest_extends_modern_with_line_number() {
        let modern = HancomStyleSet::Modern.default_styles();
        let latest = HancomStyleSet::Latest.default_styles();
        // First 22 entries identical
        assert_eq!(&latest[..22], modern);
        // 23rd is 줄 번호
        assert_eq!(latest[22].name, "줄 번호");
        assert_eq!(latest[22].style_type, "CHAR");
    }
}
