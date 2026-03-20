//! Numbering definitions for outline and list numbering.
//!
//! A [`NumberingDef`] contains up to 10 levels of [`ParaHead`] entries,
//! each defining the number format, prefix/suffix, and display template
//! for that outline level.

use hwpforge_foundation::{BulletIndex, HeadingType, NumberFormatType, NumberingIndex};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A single level definition within a numbering scheme.
///
/// Maps to HWPX `<hh:paraHead>` inside `<hh:numbering>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ParaHead {
    /// Starting number for this level.
    pub start: u32,
    /// Outline level (1-10).
    pub level: u32,
    /// Number format (DIGIT, HANGUL_SYLLABLE, etc.).
    pub num_format: NumberFormatType,
    /// Display template with `^N` placeholder (e.g. `"^1."`, `"(^5)"`).
    /// Empty string for levels 9 and 10 (self-closing in HWPX).
    pub text: String,
    /// Whether this level is checkable.
    pub checkable: bool,
}

/// A complete numbering definition.
///
/// Maps to HWPX `<hh:numbering>` inside `<hh:numberings>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NumberingDef {
    /// Numbering ID (1-based).
    pub id: u32,
    /// Starting number offset.
    pub start: u32,
    /// Level definitions (up to 10).
    pub levels: Vec<ParaHead>,
}

/// A bullet list definition.
///
/// Maps to HWPX `<hh:bullet>` inside `<hh:bullets>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BulletDef {
    /// Bullet definition ID (1-based on the wire).
    pub id: u32,
    /// Bullet glyph string.
    pub bullet_char: String,
    /// Whether this bullet uses an image marker.
    pub use_image: bool,
    /// Bullet paragraph-head metadata.
    pub para_head: ParaHead,
}

/// Shared paragraph list semantics.
///
/// This is the format-independent IR carried by paragraph styles. It stores the
/// resolved list kind plus the branded definition index when a shared numbering
/// or bullet definition is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParagraphListRef {
    /// Outline heading semantics.
    Outline {
        /// Zero-based outline level (`0..=9`).
        level: u8,
    },
    /// Numbered list semantics.
    Number {
        /// Branded index into the shared numbering definition table.
        numbering_id: NumberingIndex,
        /// Zero-based paragraph list level (`0..=9`).
        level: u8,
    },
    /// Bullet list semantics.
    Bullet {
        /// Branded index into the shared bullet definition table.
        bullet_id: BulletIndex,
        /// Zero-based paragraph list level (`0..=9`).
        level: u8,
    },
}

impl ParagraphListRef {
    /// Highest supported shared paragraph list level.
    pub const MAX_LEVEL: u8 = 9;

    /// Returns the shared list level.
    pub const fn level(self) -> u8 {
        match self {
            Self::Outline { level } | Self::Number { level, .. } | Self::Bullet { level, .. } => {
                level
            }
        }
    }

    /// Returns the corresponding heading type for HWP-family wire formats.
    pub const fn heading_type(self) -> HeadingType {
        match self {
            Self::Outline { .. } => HeadingType::Outline,
            Self::Number { .. } => HeadingType::Number,
            Self::Bullet { .. } => HeadingType::Bullet,
        }
    }
}

impl NumberingDef {
    /// Creates the default 10-level outline numbering (한글 Modern default).
    ///
    /// Matches golden fixture `tests/fixtures/textbox.hwpx`:
    ///
    /// - Level 1: DIGIT `^1.` checkable=false
    /// - Level 2: HANGUL_SYLLABLE `^2.` checkable=false
    /// - Level 3: DIGIT `^3)` checkable=false
    /// - Level 4: HANGUL_SYLLABLE `^4)` checkable=false
    /// - Level 5: DIGIT `(^5)` checkable=false
    /// - Level 6: HANGUL_SYLLABLE `(^6)` checkable=false
    /// - Level 7: CIRCLED_DIGIT `^7` checkable=true
    /// - Level 8: CIRCLED_HANGUL_SYLLABLE `^8` checkable=true
    /// - Level 9: HANGUL_JAMO `` (empty) checkable=false
    /// - Level 10: ROMAN_SMALL `` (empty) checkable=true
    pub fn default_outline() -> Self {
        Self {
            id: 1,
            start: 0,
            levels: vec![
                ParaHead {
                    start: 1,
                    level: 1,
                    num_format: NumberFormatType::Digit,
                    text: "^1.".into(),
                    checkable: false,
                },
                ParaHead {
                    start: 1,
                    level: 2,
                    num_format: NumberFormatType::HangulSyllable,
                    text: "^2.".into(),
                    checkable: false,
                },
                ParaHead {
                    start: 1,
                    level: 3,
                    num_format: NumberFormatType::Digit,
                    text: "^3)".into(),
                    checkable: false,
                },
                ParaHead {
                    start: 1,
                    level: 4,
                    num_format: NumberFormatType::HangulSyllable,
                    text: "^4)".into(),
                    checkable: false,
                },
                ParaHead {
                    start: 1,
                    level: 5,
                    num_format: NumberFormatType::Digit,
                    text: "(^5)".into(),
                    checkable: false,
                },
                ParaHead {
                    start: 1,
                    level: 6,
                    num_format: NumberFormatType::HangulSyllable,
                    text: "(^6)".into(),
                    checkable: false,
                },
                ParaHead {
                    start: 1,
                    level: 7,
                    num_format: NumberFormatType::CircledDigit,
                    text: "^7".into(),
                    checkable: true,
                },
                ParaHead {
                    start: 1,
                    level: 8,
                    num_format: NumberFormatType::CircledHangulSyllable,
                    text: "^8".into(),
                    checkable: true,
                },
                ParaHead {
                    start: 1,
                    level: 9,
                    num_format: NumberFormatType::HangulJamo,
                    text: String::new(),
                    checkable: false,
                },
                ParaHead {
                    start: 1,
                    level: 10,
                    num_format: NumberFormatType::RomanSmall,
                    text: String::new(),
                    checkable: true,
                },
            ],
        }
    }

    /// Returns the paragraph-head definition for a zero-based shared list level.
    pub fn para_head(&self, level: u8) -> Option<&ParaHead> {
        self.levels.get(level as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_outline_has_10_levels() {
        let def = NumberingDef::default_outline();
        assert_eq!(def.levels.len(), 10);
        assert_eq!(def.id, 1);
        assert_eq!(def.start, 0);
    }

    #[test]
    fn default_outline_level_formats() {
        let def = NumberingDef::default_outline();
        assert_eq!(def.levels[0].num_format, NumberFormatType::Digit);
        assert_eq!(def.levels[1].num_format, NumberFormatType::HangulSyllable);
        assert_eq!(def.levels[6].num_format, NumberFormatType::CircledDigit);
        assert_eq!(def.levels[7].num_format, NumberFormatType::CircledHangulSyllable);
        assert_eq!(def.levels[8].num_format, NumberFormatType::HangulJamo);
        assert_eq!(def.levels[9].num_format, NumberFormatType::RomanSmall);
    }

    #[test]
    fn default_outline_level_texts() {
        let def = NumberingDef::default_outline();
        assert_eq!(def.levels[0].text, "^1.");
        assert_eq!(def.levels[1].text, "^2.");
        assert_eq!(def.levels[2].text, "^3)");
        assert_eq!(def.levels[3].text, "^4)");
        assert_eq!(def.levels[4].text, "(^5)");
        assert_eq!(def.levels[5].text, "(^6)");
        assert_eq!(def.levels[6].text, "^7");
        assert_eq!(def.levels[7].text, "^8");
        assert_eq!(def.levels[8].text, ""); // self-closing
        assert_eq!(def.levels[9].text, ""); // self-closing
    }

    #[test]
    fn default_outline_checkable_flags() {
        let def = NumberingDef::default_outline();
        // Levels 1-6: not checkable
        for i in 0..6 {
            assert!(!def.levels[i].checkable, "level {} should not be checkable", i + 1);
        }
        // Level 7: checkable
        assert!(def.levels[6].checkable);
        // Level 8: checkable
        assert!(def.levels[7].checkable);
        // Level 9: NOT checkable
        assert!(!def.levels[8].checkable);
        // Level 10: checkable
        assert!(def.levels[9].checkable);
    }

    #[test]
    fn default_outline_levels_are_sequential() {
        let def = NumberingDef::default_outline();
        for (i, lvl) in def.levels.iter().enumerate() {
            assert_eq!(lvl.level, (i + 1) as u32);
        }
    }
}
