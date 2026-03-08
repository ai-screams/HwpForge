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
///
/// # Shape index references
///
/// `char_pr_group` and `para_pr_group` are indices into the default charPr
/// and paraPr arrays injected at the front of the store by `from_registry_with()`.
/// Extracted from golden fixture `tests/fixtures/textbox.hwpx` `Contents/header.xml`.
///
/// Modern (22 styles) mapping:
///
/// ```text
/// charPr groups (7 total, id 0-6):
///   0: 함초롬바탕 10pt #000000  (바탕글/본문/개요1-7/캡션)
///   1: 함초롬돋움 10pt #000000  (쪽 번호)
///   2: 함초롬돋움  9pt #000000  (머리말)
///   3: 함초롬바탕  9pt #000000  (각주/미주)
///   4: 함초롬돋움  9pt #000000  (메모)
///   5: 함초롬돋움 16pt #2E74B5  (차례 제목)
///   6: 함초롬돋움 11pt #000000  (차례 1-3)
///
/// paraPr groups (20 total, id 0-19):
///   0:  JUSTIFY left=0      개요8-10 use non-sequential ids (see below)
///   1:  JUSTIFY left=1500   본문
///   2:  JUSTIFY left=1000 OUTLINE lv=1  개요 1
///   3:  JUSTIFY left=2000 OUTLINE lv=2  개요 2
///   4:  JUSTIFY left=3000 OUTLINE lv=3  개요 3
///   5:  JUSTIFY left=4000 OUTLINE lv=4  개요 4
///   6:  JUSTIFY left=5000 OUTLINE lv=5  개요 5
///   7:  JUSTIFY left=6000 OUTLINE lv=6  개요 6
///   8:  JUSTIFY left=7000 OUTLINE lv=7  개요 7
///   9:  JUSTIFY left=0   150% spacing   머리말
///  10:  JUSTIFY indent=-1310 130%       각주/미주
///  11:  LEFT    left=0   130%           메모
///  12:  LEFT    left=0   prev=1200 next=300  차례 제목
///  13:  LEFT    left=0   next=700            차례 1
///  14:  LEFT    left=1100 next=700           차례 2
///  15:  LEFT    left=2200 next=700           차례 3
///  16:  JUSTIFY left=9000 OUTLINE lv=9  개요 8  (style 9 → paraPr 16)
///  17:  JUSTIFY left=10000 OUTLINE lv=10 개요 9  (style 10 → paraPr 17)  NOTE: lv=10 maps to OUTLINE lv=9 in XML but stored as id=17
///  18:  JUSTIFY left=8000 OUTLINE lv=8  개요 10 (style 11 → paraPr 18)  NOTE: lv=8 in XML (level field 7→8)
///  19:  JUSTIFY left=0   150% next=800  캡션
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultStyleEntry {
    /// Korean style name (e.g. `"바탕글"`, `"개요 1"`).
    pub name: &'static str,
    /// English style name (e.g. `"Normal"`, `"Outline 1"`).
    pub eng_name: &'static str,
    /// Style type: `"PARA"` for paragraph styles, `"CHAR"` for character styles.
    pub style_type: &'static str,
    /// Index into the default charPr array (0–6 for Modern).
    ///
    /// References the `charPrIDRef` attribute in `<hh:style>` elements.
    pub char_pr_group: u8,
    /// Index into the default paraPr array (0–19 for Modern).
    ///
    /// References the `paraPrIDRef` attribute in `<hh:style>` elements.
    pub para_pr_group: u8,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
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

    /// Looks up the style index for a given Korean or English style name.
    ///
    /// Returns `None` if no matching default style is found in this version's
    /// style table. The returned index corresponds directly to the `styleIDRef`
    /// attribute in HWPX `<hp:p>` elements.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_smithy_hwpx::default_styles::HancomStyleSet;
    ///
    /// assert_eq!(HancomStyleSet::Modern.style_id_for_name("개요 1"), Some(2));
    /// assert_eq!(HancomStyleSet::Modern.style_id_for_name("Outline 1"), Some(2));
    /// assert_eq!(HancomStyleSet::Modern.style_id_for_name("바탕글"), Some(0));
    /// assert_eq!(HancomStyleSet::Modern.style_id_for_name("unknown"), None);
    /// ```
    pub fn style_id_for_name(&self, name: &str) -> Option<usize> {
        self.default_styles().iter().position(|entry| entry.name == name || entry.eng_name == name)
    }
}

// ── Helper macro ───────────────────────────────────────────────────────────

macro_rules! entry {
    ($name:expr, $eng:expr, $ty:expr, $cp:expr, $pp:expr) => {
        DefaultStyleEntry {
            name: $name,
            eng_name: $eng,
            style_type: $ty,
            char_pr_group: $cp,
            para_pr_group: $pp,
        }
    };
}

// ── Classic (18 styles, 한글 2014–2020) ───────────────────────────────────

/// Default styles shipped with 한글 2014 through 2020.
///
/// The 개요 8–10 (Outline 8–10) styles are absent; 쪽 번호 sits at ID 9.
const CLASSIC_STYLES: [DefaultStyleEntry; 18] = [
    entry!("바탕글", "Normal", "PARA", 0, 0),          //  0
    entry!("본문", "Body", "PARA", 0, 1),              //  1
    entry!("개요 1", "Outline 1", "PARA", 0, 2),       //  2
    entry!("개요 2", "Outline 2", "PARA", 0, 3),       //  3
    entry!("개요 3", "Outline 3", "PARA", 0, 4),       //  4
    entry!("개요 4", "Outline 4", "PARA", 0, 5),       //  5
    entry!("개요 5", "Outline 5", "PARA", 0, 6),       //  6
    entry!("개요 6", "Outline 6", "PARA", 0, 7),       //  7
    entry!("개요 7", "Outline 7", "PARA", 0, 8),       //  8
    entry!("쪽 번호", "Page Number", "CHAR", 1, 0),    //  9  (CHAR: paraPr=0 unused)
    entry!("머리말", "Header", "PARA", 2, 9),          // 10
    entry!("각주", "Footnote", "PARA", 3, 10),         // 11
    entry!("미주", "Endnote", "PARA", 3, 10),          // 12
    entry!("메모", "Memo", "PARA", 4, 11),             // 13
    entry!("차례 제목", "TOC Heading", "PARA", 5, 12), // 14
    entry!("차례 1", "TOC 1", "PARA", 6, 13),          // 15
    entry!("차례 2", "TOC 2", "PARA", 6, 14),          // 16
    entry!("차례 3", "TOC 3", "PARA", 6, 15),          // 17
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
    entry!("바탕글", "Normal", "PARA", 0, 0), //  0  charPr=0 paraPr=0
    entry!("본문", "Body", "PARA", 0, 1),     //  1  charPr=0 paraPr=1
    entry!("개요 1", "Outline 1", "PARA", 0, 2), //  2  charPr=0 paraPr=2
    entry!("개요 2", "Outline 2", "PARA", 0, 3), //  3  charPr=0 paraPr=3
    entry!("개요 3", "Outline 3", "PARA", 0, 4), //  4  charPr=0 paraPr=4
    entry!("개요 4", "Outline 4", "PARA", 0, 5), //  5  charPr=0 paraPr=5
    entry!("개요 5", "Outline 5", "PARA", 0, 6), //  6  charPr=0 paraPr=6
    entry!("개요 6", "Outline 6", "PARA", 0, 7), //  7  charPr=0 paraPr=7
    entry!("개요 7", "Outline 7", "PARA", 0, 8), //  8  charPr=0 paraPr=8
    entry!("개요 8", "Outline 8", "PARA", 0, 18), //  9  charPr=0 paraPr=18 ← non-sequential!
    entry!("개요 9", "Outline 9", "PARA", 0, 16), // 10  charPr=0 paraPr=16
    entry!("개요 10", "Outline 10", "PARA", 0, 17), // 11  charPr=0 paraPr=17
    entry!("쪽 번호", "Page Number", "CHAR", 1, 0), // 12  charPr=1 paraPr=0 (CHAR: paraPr unused)
    entry!("머리말", "Header", "PARA", 2, 9), // 13  charPr=2 paraPr=9
    entry!("각주", "Footnote", "PARA", 3, 10), // 14  charPr=3 paraPr=10
    entry!("미주", "Endnote", "PARA", 3, 10), // 15  charPr=3 paraPr=10
    entry!("메모", "Memo", "PARA", 4, 11),    // 16  charPr=4 paraPr=11
    entry!("차례 제목", "TOC Heading", "PARA", 5, 12), // 17  charPr=5 paraPr=12
    entry!("차례 1", "TOC 1", "PARA", 6, 13), // 18  charPr=6 paraPr=13
    entry!("차례 2", "TOC 2", "PARA", 6, 14), // 19  charPr=6 paraPr=14
    entry!("차례 3", "TOC 3", "PARA", 6, 15), // 20  charPr=6 paraPr=15
    entry!("캡션", "Caption", "PARA", 0, 19), // 21  charPr=0 paraPr=19
];

// ── Latest (23 styles, 한글 2025+) ─────────────────────────────────────────

/// Default styles shipped with 한글 2025 and later.
///
/// Identical to [`MODERN_STYLES`] with the addition of 줄 번호 (Line Number)
/// as a character style at ID 22.
const LATEST_STYLES: [DefaultStyleEntry; 23] = [
    entry!("바탕글", "Normal", "PARA", 0, 0),          //  0
    entry!("본문", "Body", "PARA", 0, 1),              //  1
    entry!("개요 1", "Outline 1", "PARA", 0, 2),       //  2
    entry!("개요 2", "Outline 2", "PARA", 0, 3),       //  3
    entry!("개요 3", "Outline 3", "PARA", 0, 4),       //  4
    entry!("개요 4", "Outline 4", "PARA", 0, 5),       //  5
    entry!("개요 5", "Outline 5", "PARA", 0, 6),       //  6
    entry!("개요 6", "Outline 6", "PARA", 0, 7),       //  7
    entry!("개요 7", "Outline 7", "PARA", 0, 8),       //  8
    entry!("개요 8", "Outline 8", "PARA", 0, 18),      //  9
    entry!("개요 9", "Outline 9", "PARA", 0, 16),      // 10
    entry!("개요 10", "Outline 10", "PARA", 0, 17),    // 11
    entry!("쪽 번호", "Page Number", "CHAR", 1, 0),    // 12
    entry!("머리말", "Header", "PARA", 2, 9),          // 13
    entry!("각주", "Footnote", "PARA", 3, 10),         // 14
    entry!("미주", "Endnote", "PARA", 3, 10),          // 15
    entry!("메모", "Memo", "PARA", 4, 11),             // 16
    entry!("차례 제목", "TOC Heading", "PARA", 5, 12), // 17
    entry!("차례 1", "TOC 1", "PARA", 6, 13),          // 18
    entry!("차례 2", "TOC 2", "PARA", 6, 14),          // 19
    entry!("차례 3", "TOC 3", "PARA", 6, 15),          // 20
    entry!("캡션", "Caption", "PARA", 0, 19),          // 21
    entry!("줄 번호", "Line Number", "CHAR", 1, 0),    // 22  ← new in 2025 (same charPr as 쪽 번호)
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

    #[test]
    fn style_id_for_name_korean_outline1() {
        assert_eq!(HancomStyleSet::Modern.style_id_for_name("개요 1"), Some(2));
    }

    #[test]
    fn style_id_for_name_english_outline1() {
        assert_eq!(HancomStyleSet::Modern.style_id_for_name("Outline 1"), Some(2));
    }

    #[test]
    fn style_id_for_name_batanggeul_is_0() {
        assert_eq!(HancomStyleSet::Modern.style_id_for_name("바탕글"), Some(0));
        assert_eq!(HancomStyleSet::Modern.style_id_for_name("Normal"), Some(0));
    }

    #[test]
    fn style_id_for_name_unknown_returns_none() {
        assert_eq!(HancomStyleSet::Modern.style_id_for_name("unknown"), None);
        assert_eq!(HancomStyleSet::Modern.style_id_for_name(""), None);
    }

    #[test]
    fn style_id_for_name_classic_vs_modern_differ() {
        // 쪽 번호 is at 9 in Classic, 12 in Modern
        assert_eq!(HancomStyleSet::Classic.style_id_for_name("쪽 번호"), Some(9));
        assert_eq!(HancomStyleSet::Modern.style_id_for_name("쪽 번호"), Some(12));
    }

    #[test]
    fn modern_char_pr_groups_in_range() {
        // All charPr group indices must be within 0..7
        for entry in HancomStyleSet::Modern.default_styles() {
            assert!(
                entry.char_pr_group < 7,
                "charPr group {} out of range for {}",
                entry.char_pr_group,
                entry.name
            );
        }
    }

    #[test]
    fn modern_para_pr_groups_in_range() {
        // All paraPr group indices must be within 0..20
        for entry in HancomStyleSet::Modern.default_styles() {
            assert!(
                entry.para_pr_group < 20,
                "paraPr group {} out of range for {}",
                entry.para_pr_group,
                entry.name
            );
        }
    }

    #[test]
    fn modern_batanggeul_uses_group_0() {
        let styles = HancomStyleSet::Modern.default_styles();
        assert_eq!(styles[0].char_pr_group, 0); // 바탕글: charPr=0
        assert_eq!(styles[0].para_pr_group, 0); // 바탕글: paraPr=0
    }

    #[test]
    fn modern_outline8_uses_nonconsecutive_para_pr() {
        // 개요 8 (id=9) uses paraPr=18, NOT paraPr=9 — non-sequential!
        let styles = HancomStyleSet::Modern.default_styles();
        assert_eq!(styles[9].name, "개요 8");
        assert_eq!(styles[9].para_pr_group, 18);
        assert_eq!(styles[10].name, "개요 9");
        assert_eq!(styles[10].para_pr_group, 16);
        assert_eq!(styles[11].name, "개요 10");
        assert_eq!(styles[11].para_pr_group, 17);
    }

    #[test]
    fn modern_footnote_endnote_share_para_pr() {
        let styles = HancomStyleSet::Modern.default_styles();
        let footnote = styles.iter().find(|e| e.name == "각주").unwrap();
        let endnote = styles.iter().find(|e| e.name == "미주").unwrap();
        assert_eq!(footnote.para_pr_group, endnote.para_pr_group);
        assert_eq!(footnote.char_pr_group, endnote.char_pr_group);
    }

    #[test]
    fn modern_toc_entries_share_char_pr() {
        let styles = HancomStyleSet::Modern.default_styles();
        let toc1 = styles.iter().find(|e| e.name == "차례 1").unwrap();
        let toc2 = styles.iter().find(|e| e.name == "차례 2").unwrap();
        let toc3 = styles.iter().find(|e| e.name == "차례 3").unwrap();
        assert_eq!(toc1.char_pr_group, 6);
        assert_eq!(toc2.char_pr_group, 6);
        assert_eq!(toc3.char_pr_group, 6);
    }
}
