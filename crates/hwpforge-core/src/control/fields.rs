//! Field-control payload types: path/file-name commands, cross-reference
//! targets, and inline page-number kinds.
//!
//! Houses [`PathFieldCommand`], [`RefTarget`], and [`InlinePageKind`] — the
//! typed payloads carried by the field variants of [`Control`](super::Control).

use hwpforge_foundation::NumberFormatType;
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
/// [`Control::CrossRef`](crate::control::Control::CrossRef) with a type-safe enum that distinguishes the
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

/// Which counter a [`Control::AutoNumber`] shows (HWPX `<hp:autoNum
/// numType>`): a caption's picture, table or equation number.
///
/// [`Control::AutoNumber`]: crate::control::Control::AutoNumber
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum AutoNumberKind {
    /// Picture number (`numType="PICTURE"`).
    Picture,
    /// Table number (`numType="TABLE"`).
    Table,
    /// Equation number (`numType="EQUATION"`).
    Equation,
}

/// How a [`Control::AutoNumber`] writes its number (HWPX
/// `<hp:autoNumFormat>`). The default is a plain digit: `DIGIT`, no user
/// character, no prefix or suffix, not superscript.
///
/// [`Control::AutoNumber`]: crate::control::Control::AutoNumber
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::AutoNumberFormat;
/// use hwpforge_foundation::NumberFormatType;
///
/// let format = AutoNumberFormat::default();
/// assert_eq!(format.number_type, NumberFormatType::Digit);
/// assert_eq!(format.prefix, None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
pub struct AutoNumberFormat {
    /// Number shape (`type`).
    pub number_type: NumberFormatType,
    /// User character (`userChar`); `None` = empty.
    pub user_char: Option<char>,
    /// Character before the number (`prefixChar`); `None` = empty.
    pub prefix: Option<char>,
    /// Character after the number (`suffixChar`); `None` = empty.
    pub suffix: Option<char>,
    /// The number is superscript (`supscript`).
    pub superscript: bool,
}

/// Which counter a [`Control::NewNumber`] restarts.
///
/// [`Control::NewNumber`]: crate::control::Control::NewNumber
///
/// Maps 1:1 to HWPX `<hp:newNum numType>` and HWP5 `nwno` property
/// bits 0-3 (native fixture 실측 2026-08-12: 쪽 번호 = `0`, corpus 613회 중
/// 607회가 `Page`). Unmapped wire values are surfaced as [`Self::Unknown`]
/// — encoders must skip them instead of fabricating a type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum NewNumberKind {
    /// 쪽 번호 재시작 (`numType="PAGE"`, wire `0`).
    Page,
    /// 각주 번호 재시작 (`numType="FOOTNOTE"`, wire `1`).
    Footnote,
    /// 미주 번호 재시작 (`numType="ENDNOTE"`, wire `2`).
    Endnote,
    /// 그림 번호 재시작 (`numType="PICTURE"`, wire `3`).
    Picture,
    /// 표 번호 재시작 (`numType="TABLE"`, wire `4`).
    Table,
    /// 수식 번호 재시작 (`numType="EQUATION"`, wire `5`).
    Equation,
    /// Unmapped wire value (6-15) — preserved as a warning signal only.
    Unknown,
}

/// A field boundary whose body remains in the surrounding ordinary runs.
///
/// Unlike a folded field control, this representation can cross run and
/// paragraph boundaries. Identifiers and parameters are carried verbatim;
/// the body is never a synthesized evaluation of a formula or placeholder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum FieldBoundary {
    /// Start of the range.
    Start(Box<FieldStart>),
    /// End of the range, referring to the start's instance identifier.
    End {
        /// Identifier of the matching start.
        begin_id: String,
        /// Original field tag.
        field_id: String,
    },
}

/// Metadata of a field whose text lives in ordinary runs.
///
/// String-valued flags retain the authored spelling without inventing a
/// value when the source omitted one. Parameters retain their scalar type,
/// name and value; none are evaluated by Core.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct FieldStart {
    /// Instance identifier, shared by the end marker.
    pub id: String,
    /// Field kind, such as `CLICK_HERE`, `BOOKMARK` or `FORMULA`.
    pub kind: String,
    /// Authored field name.
    pub name: String,
    /// Editability flag.
    pub editable: String,
    /// Recalculation flag.
    pub dirty: String,
    /// Stacking order.
    pub z_order: String,
    /// Original field tag.
    pub field_id: String,
    /// Authored metadata tag.
    pub meta_tag: String,
    /// Optional parameter group name; `None` means no parameter group.
    pub parameter_name: Option<String>,
    /// Scalar field parameters in their source order within each type.
    pub parameters: Vec<FieldParameter>,
    /// Annotation paragraphs owned by the start, distinct from the visible
    /// range body in ordinary runs. `None` means no annotation list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotation: Option<Vec<crate::paragraph::Paragraph>>,
}

/// A typed scalar parameter of a range field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum FieldParameter {
    /// String parameter, including its whitespace-preservation attribute.
    String {
        /// Parameter name.
        name: String,
        /// Parameter value.
        value: String,
        /// Original whitespace-preservation attribute.
        xml_space: String,
    },
    /// Integer parameter; spelling is retained without numeric coercion.
    Integer {
        /// Parameter name.
        name: String,
        /// Parameter value.
        value: String,
    },
    /// Boolean parameter; spelling is retained without coercion.
    Boolean {
        /// Parameter name.
        name: String,
        /// Parameter value.
        value: String,
    },
}
