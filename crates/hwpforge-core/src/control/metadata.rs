//! Annotation metadata types for memo and dutmal controls.
//!
//! Houses [`MemoMetadata`], [`DutmalMetadata`], [`DutmalPosition`], and
//! [`DutmalAlign`] — the wire-mirrored metadata carried by the memo and
//! dutmal variants of [`Control`](super::Control).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Metadata associated with a memo annotation (HWPX `<hp:parameters>`).
///
/// Carries the seven HWPX memo parameters (`Prop`, `Command`, `ID`, `Number`,
/// `Author`, `MemoShapeIDRef`, `CreateDateTime`). Empty / default values are
/// rendered by encoders as sensible defaults: `Prop` is always `0` on
/// 한컴-authored fixtures so the field is omitted on the Core side, and
/// `CreateDateTime` is auto-generated at encode time if blank (a 한컴-native
/// memo always has a timestamp).
///
/// Other format encoders (Markdown, future ODT) can ignore the
/// HWPX-specific fields and just use `author` if useful.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct MemoMetadata {
    /// Reference to the `MemoShape` table entry. 한컴 default is `65535`.
    pub shape_id_ref: u32,
    /// Memo number (`<hp:integerParam name="Number">`). Equals the HWP5
    /// `memo_id`. Encoders normally also derive the HWPX `ID` parameter
    /// from this value (`"memo{number}"`).
    pub number: u32,
    /// HWPX `ID` parameter (typically `"memo{number}"`). Encoders may
    /// auto-derive from `number` when blank.
    pub id: String,
    /// Memo author (`Author` parameter). Empty when unknown.
    pub author: String,
    /// HWPX `CreateDateTime` parameter (ISO 8601 UTC). Empty triggers
    /// encoder-side auto-generation at write time.
    pub create_datetime: String,
    /// Raw wire `Command` parameter (`"MEMO/{shape}/{number}/…"` from HWP5).
    /// Empty for HwpForge-authored memos — encoders synthesise a minimum
    /// `MEMO/{shape}/{number}/` form in that case.
    pub command: String,
}

impl Default for MemoMetadata {
    fn default() -> Self {
        Self {
            shape_id_ref: 65535,
            number: 0,
            id: String::new(),
            author: String::new(),
            create_datetime: String::new(),
            command: String::new(),
        }
    }
}

impl MemoMetadata {
    /// Returns the HWPX `ID` parameter, deriving `"memo{number}"` when the
    /// explicit field is blank.
    pub fn hwpx_id(&self) -> String {
        if self.id.is_empty() {
            format!("memo{}", self.number)
        } else {
            self.id.clone()
        }
    }
}

/// Wire-mirrored metadata attached to a `Control::Dutmal`.
///
/// Carries HWPX `<hp:dutmal>` attributes that HwpForge does not yet
/// model as typed fields — currently just `option`. Field is mirrored
/// verbatim from HWP5 wire / HWPX `option=` attribute and emitted
/// verbatim on encode so round-trips preserve the value even when the
/// semantics aren't pinned down (see
/// `.docs/algorithms/2026-06-01_dutmal_carry.md`).
///
/// `#[non_exhaustive]` — additions like `style_id_ref` or the two
/// reserved tail words are additive and don't break existing
/// destructure / pattern-match sites.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct DutmalMetadata {
    /// `<hp:dutmal option=…>` value mirrored verbatim. Likely a
    /// bit-field or enum on the HWPX side; HwpForge treats it as an
    /// opaque u32 until the spec is confirmed.
    pub option: u32,
}

/// Position of dutmal annotation text relative to the main text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum DutmalPosition {
    /// Annotation above main text (default).
    #[default]
    Top,
    /// Annotation below main text.
    Bottom,
    /// Annotation to the right.
    Right,
    /// Annotation to the left.
    Left,
}

/// Alignment of dutmal annotation text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum DutmalAlign {
    /// Center-aligned (default).
    #[default]
    Center,
    /// Left-aligned.
    Left,
    /// Right-aligned.
    Right,
}
