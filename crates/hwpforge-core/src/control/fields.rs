//! Field-control payload types: path/file-name commands, cross-reference
//! targets, and inline page-number kinds.
//!
//! Houses [`PathFieldCommand`], [`RefTarget`], and [`InlinePageKind`] — the
//! typed payloads carried by the field variants of [`Control`](super::Control).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::object_id::ObjectId;

/// Typed variant of the HWP5 `%pat` Command string (Wave 12n).
///
/// Observed forms are `$F` (file name only), `$P` (path only), and `$P$F`
/// (full path). Anything else is preserved as [`PathFieldCommand::Unknown`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum PathFieldCommand {
    /// `$F` — file name only.
    FileName,
    /// `$P` — folder path only (no file name).
    Path,
    /// `$P$F` — full path including file name.
    PathAndFileName,
    /// Any other Command form, preserved verbatim.
    Unknown(String),
}

impl PathFieldCommand {
    /// Returns the canonical wire Command string for typed variants.
    /// For [`Self::Unknown`], returns the carried raw string.
    pub fn wire_command(&self) -> &str {
        match self {
            Self::FileName => "$F",
            Self::Path => "$P",
            Self::PathAndFileName => "$P$F",
            Self::Unknown(s) => s.as_str(),
        }
    }

    /// Parses a wire Command string into a typed variant. Unknown forms
    /// are preserved as [`Self::Unknown`].
    pub fn from_wire(cmd: &str) -> Self {
        match cmd {
            "$F" => Self::FileName,
            "$P" => Self::Path,
            "$P$F" => Self::PathAndFileName,
            other => Self::Unknown(other.to_string()),
        }
    }
}

/// Cross-reference target identifier (Wave 12m Phase 2).
///
/// Replaces the previous `target_name: String` field on
/// [`Control::CrossRef`] with a type-safe enum that distinguishes the
/// three observed wire forms in HWP5 `%xrf` Command strings:
///
/// - **`Name(String)`** — 사용자 지정 책갈피 이름 (Bookmark 전용).
///   한컴 fixture 의 `?target1;6;0;0;0;` 형식에서 `target1` 부분.
/// - **`Object(ObjectId)`** — 한컴 자동 생성 정수 ID (Footnote / Endnote /
///   Caption / Outline). 한컴 fixture 의 `?#1108165575;3;0;0;0;` 형식에서
///   `#` 접두사를 떼고 정수로 파싱한 값. 가리키는 타깃(Image/Table/…)의
///   `inst_id: Option<ObjectId>` 와 **같은 [`ObjectId`]** 를 공유해 링크를
///   타입 수준에서 보장한다 (ADR-010).
/// - **`Raw(String)`** — 위 두 형식 어느쪽으로도 파싱이 안 된 wire 값.
///   silent fallback 위조 금지 원칙 (Codex(architect)) — 의미를
///   모르면 raw 보존.
///
/// 변환은 `smithy-hwp5/src/projection.rs::decode_hwp5_target` 의
/// boundary 함수에서 수행 (Core 는 wire-agnostic).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum RefTarget {
    /// 사용자 지정 책갈피 이름 (Bookmark 전용).
    Name(String),
    /// 타깃 객체의 [`ObjectId`] (Footnote / Endnote / Caption / Outline).
    /// 가리키는 타깃의 `inst_id` 와 같은 id 를 공유한다 (ADR-010).
    Object(ObjectId),
    /// 위 형식 어느쪽으로도 정규화되지 않은 raw 값 — fallback 위조 금지.
    Raw(String),
}

impl RefTarget {
    /// Returns the displayable name for this target.
    ///
    /// - `Name(s)` → `s`
    /// - `Object(id)` → `"#{id}"` 형식 (한컴 wire 와 동일)
    /// - `Raw(s)` → `s`
    pub fn as_display(&self) -> String {
        match self {
            Self::Name(s) => s.clone(),
            Self::Object(id) => format!("#{id}"),
            Self::Raw(s) => s.clone(),
        }
    }
}

/// Typed variant of the HWP5 `atno` inline page-number `flag` byte
/// (Wave 12n).
///
/// Observed values: `0x00` = current page, `0x06` = total page count.
/// Other values surface as [`InlinePageKind::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum InlinePageKind {
    /// Flag `0x00` — current page number.
    CurrentPage,
    /// Flag `0x06` — total page count.
    TotalPages,
    /// Any other flag value, surfaced when the HWP5 wire flag is neither
    /// `0x00` nor `0x06`.
    Unknown,
}
