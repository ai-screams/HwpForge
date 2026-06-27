//! Reference enums: bookmarks, fields, cross-references, and drop-cap styles.

use crate::error::FoundationError;
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// BookmarkType
// ---------------------------------------------------------------------------

/// Type of bookmark in an HWPX document.
///
/// Bookmarks can mark a single point or span a range of content
/// (start/end pair using `fieldBegin`/`fieldEnd`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum BookmarkType {
    /// A point bookmark at a single location (direct serde in `<hp:ctrl>`).
    #[default]
    Point = 0,
    /// Start of a span bookmark (`fieldBegin type="BOOKMARK"`).
    SpanStart = 1,
    /// End of a span bookmark (`fieldEnd beginIDRef`).
    SpanEnd = 2,
}

impl fmt::Display for BookmarkType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Point => f.write_str("Point"),
            Self::SpanStart => f.write_str("SpanStart"),
            Self::SpanEnd => f.write_str("SpanEnd"),
        }
    }
}

impl std::str::FromStr for BookmarkType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Point" | "point" => Ok(Self::Point),
            "SpanStart" | "span_start" => Ok(Self::SpanStart),
            "SpanEnd" | "span_end" => Ok(Self::SpanEnd),
            _ => Err(FoundationError::ParseError {
                type_name: "BookmarkType".to_string(),
                value: s.to_string(),
                valid_values: "Point, SpanStart, SpanEnd".to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for BookmarkType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Point),
            1 => Ok(Self::SpanStart),
            2 => Ok(Self::SpanEnd),
            _ => Err(FoundationError::ParseError {
                type_name: "BookmarkType".to_string(),
                value: value.to_string(),
                valid_values: "0 (Point), 1 (SpanStart), 2 (SpanEnd)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for BookmarkType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("BookmarkType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// FieldType
// ---------------------------------------------------------------------------

/// Type of a press-field (누름틀) or SUMMERY auto-field carried in
/// [`Control::Field`].
///
/// `ClickHere` is the press-field (user-fillable form). The rest are
/// SUMMERY-family auto-fields (HWPX `<hp:fieldBegin type="SUMMERY">`,
/// HWP5 `%smr` ctrl_id) that resolve to document metadata at render time.
///
/// Wave 12n (2026-06-02) breaking: legacy `Date`/`Time`/`DocSummary`/`UserInfo`
/// variants were renamed to match the actual HWPX `Command` token and 한컴
/// menu item, and `PageNum` was moved out to [`Control::InlinePageNumber`]
/// because the wire family is `atno`, not SUMMERY/fieldBegin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
#[repr(u8)]
pub enum FieldType {
    /// Click-here placeholder press-field (default). 한컴 누름틀. HWP5 `%clk`.
    #[default]
    ClickHere = 0,
    /// SUMMERY `$author` — 만든 사람 (was [`FieldType::DocSummary`]).
    Author = 1,
    /// SUMMERY `$lastsaveby` — 마지막 저장한 사람 (was [`FieldType::UserInfo`]).
    LastSavedBy = 2,
    /// SUMMERY `$createtime` — 만든 날짜 (was [`FieldType::Time`]).
    CreatedTime = 3,
    /// SUMMERY `$modifiedtime` — 마지막 저장한 날짜 (was [`FieldType::Date`]).
    ModifiedTime = 4,
    /// SUMMERY `$title` — 문서 제목 (NEW in Wave 12n).
    Title = 5,
}

impl FieldType {
    /// Returns the HWPX `Command` `$token` for SUMMERY variants, or `None`
    /// for non-SUMMERY (`ClickHere`).
    #[must_use]
    pub fn summary_token(self) -> Option<&'static str> {
        match self {
            Self::ClickHere => None,
            Self::Author => Some("$author"),
            Self::LastSavedBy => Some("$lastsaveby"),
            Self::CreatedTime => Some("$createtime"),
            Self::ModifiedTime => Some("$modifiedtime"),
            Self::Title => Some("$title"),
        }
    }

    /// Parses a SUMMERY `$token` (`$author`, `$createtime`, …) into a
    /// matching [`FieldType`]. Returns `None` for unknown tokens; callers
    /// should carry the raw token via [`Control::UnknownSummary`] instead.
    #[must_use]
    pub fn from_summary_token(token: &str) -> Option<Self> {
        match token {
            "$author" => Some(Self::Author),
            "$lastsaveby" => Some(Self::LastSavedBy),
            "$createtime" => Some(Self::CreatedTime),
            "$modifiedtime" => Some(Self::ModifiedTime),
            "$title" => Some(Self::Title),
            _ => None,
        }
    }

    /// Returns the HWPX `<hp:fieldBegin editable="…">` value for this
    /// field type (Wave 12p task #124).
    ///
    /// Empirically derived from the Hancom-native
    /// `sample-field-docsummary.hwpx`:
    /// - `Author`, `Title` → `false` (stable user metadata; Hancom keeps
    ///   the authored value)
    /// - `LastSavedBy`, `CreatedTime`, `ModifiedTime` → `true` (Hancom
    ///   recomputes from the document metadata on save)
    /// - `ClickHere` → `true` (placeholder press-fields are user-editable)
    ///
    /// Encoders use this method to populate the `editable` attribute
    /// instead of the prior hardcoded `"1"` (which previously caused
    /// `$author`/`$title` to appear edited when re-saved by Hancom).
    #[must_use]
    pub fn hwpx_editable(self) -> bool {
        match self {
            Self::ClickHere => true,
            Self::Author | Self::Title => false,
            Self::LastSavedBy | Self::CreatedTime | Self::ModifiedTime => true,
        }
    }
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClickHere => f.write_str("CLICK_HERE"),
            Self::Author => f.write_str("AUTHOR"),
            Self::LastSavedBy => f.write_str("LAST_SAVED_BY"),
            Self::CreatedTime => f.write_str("CREATED_TIME"),
            Self::ModifiedTime => f.write_str("MODIFIED_TIME"),
            Self::Title => f.write_str("TITLE"),
        }
    }
}

impl std::str::FromStr for FieldType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "CLICK_HERE" | "ClickHere" | "click_here" => Ok(Self::ClickHere),
            "AUTHOR" | "Author" | "author"
            // legacy Wave 12l alias: DocSummary used to emit $author
            | "DOC_SUMMARY" | "DocSummary" | "doc_summary" => Ok(Self::Author),
            "LAST_SAVED_BY" | "LastSavedBy" | "last_saved_by"
            // legacy alias: UserInfo used to emit $lastsaveby
            | "USER_INFO" | "UserInfo" | "user_info" => Ok(Self::LastSavedBy),
            "CREATED_TIME" | "CreatedTime" | "created_time"
            // legacy alias: Time used to emit $createtime
            | "TIME" | "Time" | "time" => Ok(Self::CreatedTime),
            "MODIFIED_TIME" | "ModifiedTime" | "modified_time"
            // legacy alias: Date used to emit $modifiedtime
            | "DATE" | "Date" | "date" => Ok(Self::ModifiedTime),
            "TITLE" | "Title" | "title" => Ok(Self::Title),
            _ => Err(FoundationError::ParseError {
                type_name: "FieldType".to_string(),
                value: s.to_string(),
                valid_values:
                    "CLICK_HERE, AUTHOR, LAST_SAVED_BY, CREATED_TIME, MODIFIED_TIME, TITLE"
                        .to_string(),
            }),
        }
    }
}

impl TryFrom<u8> for FieldType {
    type Error = FoundationError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::ClickHere),
            1 => Ok(Self::Author),
            2 => Ok(Self::LastSavedBy),
            3 => Ok(Self::CreatedTime),
            4 => Ok(Self::ModifiedTime),
            5 => Ok(Self::Title),
            _ => Err(FoundationError::ParseError {
                type_name: "FieldType".to_string(),
                value: value.to_string(),
                valid_values: "0..5 (ClickHere..Title)".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for FieldType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("FieldType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// RefType
// ---------------------------------------------------------------------------

/// Target type of a cross-reference (상호참조).
///
/// Wave 12m Phase 2 (Codex(architect+critic) review 반영):
/// - `#[repr(u8)]` 제거 — HWP5 wire 코드는 boundary 에서 매핑 (Core/foundation
///   은 wire-agnostic 의미 모델만 유지).
/// - `Footnote` / `Endnote` / `Outline` 추가 (한컴 native dropdown 7 종 완비).
/// - `Unknown(u8)` tuple variant 추가 — silent fallback 위조 방지 (Codex HIGH).
///
/// HWP5 `%xrf` Command N1 코드 → `RefType` 변환은
/// `smithy-hwp5/src/projection.rs::decode_hwp5_ref_type` 에서 수행.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub enum RefType {
    /// Reference to a bookmark target. 한컴: "책갈피"
    #[default]
    Bookmark,
    /// Reference to a table caption number. 한컴: "표"
    Table,
    /// Reference to a figure/image caption number. 한컴: "그림"
    Figure,
    /// Reference to an equation caption number. 한컴: "수식"
    Equation,
    /// Reference to a footnote (각주).
    Footnote,
    /// Reference to an endnote (미주).
    Endnote,
    /// Reference to an outline heading (개요).
    Outline,
    /// Unrecognized RefType code preserved from wire for forward
    /// compatibility. Carriers MUST NOT silently fallback to a known
    /// variant — the unknown code is preserved so future Hancom
    /// extensions don't get silently lost.
    Unknown(u8),
}

impl fmt::Display for RefType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bookmark => f.write_str("TARGET_BOOKMARK"),
            Self::Table => f.write_str("TARGET_TABLE"),
            Self::Figure => f.write_str("TARGET_FIGURE"),
            Self::Equation => f.write_str("TARGET_EQUATION"),
            Self::Footnote => f.write_str("TARGET_FOOTNOTE"),
            Self::Endnote => f.write_str("TARGET_ENDNOTE"),
            Self::Outline => f.write_str("TARGET_OUTLINE"),
            Self::Unknown(code) => write!(f, "TARGET_UNKNOWN({code})"),
        }
    }
}

impl std::str::FromStr for RefType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "TARGET_BOOKMARK" | "Bookmark" | "bookmark" => Ok(Self::Bookmark),
            "TARGET_TABLE" | "Table" | "table" => Ok(Self::Table),
            "TARGET_FIGURE" | "Figure" | "figure" => Ok(Self::Figure),
            "TARGET_EQUATION" | "Equation" | "equation" => Ok(Self::Equation),
            "TARGET_FOOTNOTE" | "Footnote" | "footnote" => Ok(Self::Footnote),
            "TARGET_ENDNOTE" | "Endnote" | "endnote" => Ok(Self::Endnote),
            "TARGET_OUTLINE" | "Outline" | "outline" => Ok(Self::Outline),
            _ => Err(FoundationError::ParseError {
                type_name: "RefType".to_string(),
                value: s.to_string(),
                valid_values: "TARGET_BOOKMARK, TARGET_TABLE, TARGET_FIGURE, TARGET_EQUATION, \
                    TARGET_FOOTNOTE, TARGET_ENDNOTE, TARGET_OUTLINE"
                    .to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for RefType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("RefType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

// ---------------------------------------------------------------------------
// RefContentType
// ---------------------------------------------------------------------------

/// Content display type for a cross-reference (what to show at the reference site).
///
/// Wave 12m Phase 2 (Codex(architect+critic) review 반영):
/// - `#[repr(u8)]` 제거 — N2 wire 코드는 boundary 에서 매핑.
/// - `Unknown(u8)` 추가 — silent fallback 위조 방지.
///
/// Wave 12p pre-fix (Wave 12m fixup regression revert + 한컴 native
/// wire 일치 보정, 2026-06-08):
///
/// Wave 12m fixup 에서 `BookmarkName` 폐기 + Bookmark+Contents 를 N2=2
/// 로 통일했으나, native wire 분석 결과 정반대였음:
///
/// | HWP5 N2 (Bookmark) | 한컴 의미     | HWPX RefContentType 문자열 |
/// |--------------------|---------------|---------------------------|
/// | 0                  | 책갈피 위치 페이지 | `OBJECT_TYPE_PAGE`        |
/// | 1                  | 책갈피 본문 / 번호 | `OBJECT_TYPE_NUMBER` (!!) |
/// | 2                  | 책갈피 이름       | `OBJECT_TYPE_CONTENTS`    |
/// | 3                  | 위/아래          | `OBJECT_TYPE_UPDOWNPOS`   |
///
/// (책갈피의 wire 는 OWPML spec 표 156 의 직관과 어긋남 — `CONTENTS`
/// 가 "책갈피 이름", `NUMBER` 가 "책갈피 내용" 의미.) `BookmarkName`
/// variant 부활 + Display 는 `OBJECT_TYPE_CONTENTS` 동일 emit (한컴
/// wire 일치) 하되 boundary 의 wire code 매핑은 분리.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub enum RefContentType {
    /// Show page number where the target appears. 한컴: "쪽 번호" (모든 RefType 공통)
    #[default]
    Page,
    /// Show the target's numbering. 의미는 RefType-상대적:
    /// - Footnote/Endnote/Caption/Outline: "주 번호" / "표 번호" / "그림 번호" / "개요 번호" (N2=1)
    /// - Bookmark: **책갈피 본문 / 번호 텍스트** (N2=1, 한컴 wire 특이성)
    Number,
    /// Show the target's content. 의미는 RefType-상대적:
    /// - Figure/Table/Equation: "캡션 내용" (N2=2)
    /// - Outline: "개요 내용" (N2=2)
    /// - Bookmark 에서는 *사용 안 함* — 책갈피의 "내용" 의미는
    ///   `RefContentType::Number` 가 carry (N2=1).
    Contents,
    /// Show the bookmark's NAME. 한컴: "책갈피 이름" (Bookmark+N2=2 전용).
    /// HWPX wire 의 `OBJECT_TYPE_CONTENTS` 와 동일 문자열로 emit
    /// (RefType=TARGET_BOOKMARK 컨텍스트에서 한컴이 의미 결정).
    BookmarkName,
    /// Show relative position ("위" / "아래"). 한컴: "위/아래" (N2=3).
    UpDownPos,
    /// Unrecognized ContentType code preserved from wire for forward
    /// compatibility.
    Unknown(u8),
}

impl fmt::Display for RefContentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Page => f.write_str("OBJECT_TYPE_PAGE"),
            Self::Number => f.write_str("OBJECT_TYPE_NUMBER"),
            // Wave 12p pre-fix: `Contents` 와 `BookmarkName` 모두 한컴
            // wire 의 `OBJECT_TYPE_CONTENTS` 로 emit. 의미 구분은
            // RefType + Command 의 N2 wire code 에서 (Bookmark N2=2 =
            // 책갈피 이름, Figure/Table/Eq/Outline N2=2 = 캡션 내용).
            Self::Contents | Self::BookmarkName => f.write_str("OBJECT_TYPE_CONTENTS"),
            Self::UpDownPos => f.write_str("OBJECT_TYPE_UPDOWNPOS"),
            Self::Unknown(code) => write!(f, "OBJECT_TYPE_UNKNOWN({code})"),
        }
    }
}

impl std::str::FromStr for RefContentType {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "OBJECT_TYPE_PAGE" | "Page" | "page" => Ok(Self::Page),
            "OBJECT_TYPE_NUMBER" | "Number" | "number" => Ok(Self::Number),
            // Wave 12p pre-fix: `OBJECT_TYPE_CONTENTS` 는 caller 의 추가
            // 컨텍스트 (RefType + N2 wire code) 없이는 `Contents` 와
            // `BookmarkName` 을 구분할 수 없으므로 default 로 `Contents`
            // 를 반환. boundary 함수 `decode_hwp5_crossref_content_type`
            // 가 Bookmark N2=2 인 경우만 `BookmarkName` 으로 재매핑.
            "OBJECT_TYPE_CONTENTS" | "Contents" | "contents" => Ok(Self::Contents),
            "BookmarkName" | "bookmark_name" => Ok(Self::BookmarkName),
            "OBJECT_TYPE_UPDOWNPOS" | "UpDownPos" | "updownpos" => Ok(Self::UpDownPos),
            _ => Err(FoundationError::ParseError {
                type_name: "RefContentType".to_string(),
                value: s.to_string(),
                valid_values: "OBJECT_TYPE_PAGE, OBJECT_TYPE_NUMBER, OBJECT_TYPE_CONTENTS, \
                    OBJECT_TYPE_UPDOWNPOS"
                    .to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for RefContentType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("RefContentType")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

/// Drop cap style for floating shape objects (HWPX `dropcapstyle` attribute).
///
/// Controls whether a shape (text box, image, table, etc.) is formatted as a
/// drop capital that occupies multiple lines at the start of a paragraph.
///
/// # HWPX Values
///
/// | Variant      | HWPX string     |
/// |--------------|-----------------|
/// | `None`       | `"None"`        |
/// | `DoubleLine` | `"DoubleLine"`  |
/// | `TripleLine` | `"TripleLine"`  |
/// | `Margin`     | `"Margin"`      |
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DropCapStyle {
    /// No drop cap (default).
    #[default]
    None = 0,
    /// Drop cap spanning 2 lines.
    DoubleLine = 1,
    /// Drop cap spanning 3 lines.
    TripleLine = 2,
    /// Drop cap positioned in the margin.
    Margin = 3,
}

impl fmt::Display for DropCapStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::DoubleLine => f.write_str("DoubleLine"),
            Self::TripleLine => f.write_str("TripleLine"),
            Self::Margin => f.write_str("Margin"),
        }
    }
}

impl DropCapStyle {
    /// Parses an HWPX `dropcapstyle` attribute value (PascalCase).
    ///
    /// Unknown values fall back to `None` (default) for forward compatibility.
    pub fn from_hwpx_str(s: &str) -> Self {
        match s {
            "DoubleLine" => Self::DoubleLine,
            "TripleLine" => Self::TripleLine,
            "Margin" => Self::Margin,
            _ => Self::None,
        }
    }
}

impl std::str::FromStr for DropCapStyle {
    type Err = FoundationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" | "NONE" | "none" => Ok(Self::None),
            "DoubleLine" | "DOUBLE_LINE" => Ok(Self::DoubleLine),
            "TripleLine" | "TRIPLE_LINE" => Ok(Self::TripleLine),
            "Margin" | "MARGIN" => Ok(Self::Margin),
            _ => Err(FoundationError::ParseError {
                type_name: "DropCapStyle".to_string(),
                value: s.to_string(),
                valid_values: "None, DoubleLine, TripleLine, Margin".to_string(),
            }),
        }
    }
}

impl schemars::JsonSchema for DropCapStyle {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("DropCapStyle")
    }

    fn json_schema(gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
        gen.subschema_for::<String>()
    }
}

impl serde::Serialize for DropCapStyle {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for DropCapStyle {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}
