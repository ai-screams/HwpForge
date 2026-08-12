//! Centralised HWP5 `ctrl_id` magic constants.
//!
//! HWP5's `ctrl_id` is a 4-byte BE-ASCII identifier (e.g. `"tbl "`,
//! `"head"`, `"%clk"`, `"%smr"`). It appears in two places on the wire:
//!
//! 1. **CtrlHeader (`0x47`) payload** — first 4 bytes (LE-stored, but we
//!    read it as BE-ASCII for naming consistency).
//! 2. **Inline marker `extra[0..4]`** — inside a 14-byte block following
//!    a control code-point in `ParaText`. Same byte ordering convention
//!    via `ctrl_id_from_inline_extra_bytes`.
//!
//! # History
//!
//! Pre-#94 these constants lived inline across `decoder/section.rs`,
//! `projection.rs`, `schema/section.rs`, and `semantic_adapter.rs` —
//! sometimes with the *same* value defined twice under different names
//! (drift hazard). This module is the single source of truth.
//!
//! # Naming (#94 Step B2 canonicalised)
//!
//! Wire names (`SECD`, `FIELD_CROSSREF`, `ATNO`, …) are the canonical
//! identifiers. Step B1 aliases (`SECTION_DEF`, `CROSSREF`,
//! `FIELD_INLINE_PAGE`, `INDEXMARK_INLINE`, `INLINE_AUTONUM`) were
//! removed in Step B2 — see HWP5_WIRE_SPEC.md §9 for naming rationale.
//!
//! All values are `pub(crate)` — HWP5 wire detail, not part of the
//! external API surface.
//!
//! Layout reference: `HWP5_WIRE_SPEC.md` §9 (CTRL_ID Magic Constants).

// ---------------------------------------------------------------------------
// §1 Structural controls (CtrlHeader only)
// ---------------------------------------------------------------------------

/// ctrl_id for a table control: ASCII `"tbl "` as big-endian u32.
pub(crate) const CTRL_ID_TABLE: u32 = 0x7462_6C20;

/// ctrl_id for header control: ASCII `"head"` as big-endian u32.
pub(crate) const CTRL_ID_HEADER: u32 = 0x6865_6164;

/// ctrl_id for footer control: ASCII `"foot"` as big-endian u32.
pub(crate) const CTRL_ID_FOOTER: u32 = 0x666F_6F74;

/// ctrl_id for section definition control: ASCII `"secd"` as big-endian u32.
/// Holds page-level visibility / column-spacing / paper-spec metadata
/// (HWP 5.0 spec §4.3.10.1 표 129·130).
pub(crate) const CTRL_ID_SECD: u32 = 0x7365_6364;

/// ctrl_id for footnote control: ASCII `"fn  "` as big-endian u32.
pub(crate) const CTRL_ID_FOOTNOTE: u32 = 0x666E_2020;

/// ctrl_id for endnote control: ASCII `"en  "` as big-endian u32.
pub(crate) const CTRL_ID_ENDNOTE: u32 = 0x656E_2020;

/// ctrl_id for generic shape object control: ASCII `"gso "` as big-endian u32.
pub(crate) const CTRL_ID_GSO: u32 = 0x6773_6F20;

/// ctrl_id for the equation editor control: ASCII `"eqed"` as big-endian u32.
pub(crate) const CTRL_ID_EQED: u32 = 0x6571_6564;

/// ctrl_id for the column definition control: ASCII `"cold"` as big-endian u32.
pub(crate) const CTRL_ID_COLUMN_DEF: u32 = 0x636F_6C64;

/// ctrl_id for the page-number control: ASCII `"pgnp"` as big-endian u32.
/// Flows through `Hwp5Control::Unknown`; the semantic model recognises
/// this id to keep audit page-number counts accurate.
pub(crate) const CTRL_ID_PAGE_NUMBER: u32 = 0x7067_6E70;

/// ctrl_id for the 새 번호 지정 (new number) control: ASCII `"nwno"` as
/// big-endian u32. 번호 카운터를 컨트롤 위치부터 재시작한다 — F1 native
/// fixture 실측 (2026-08-12): 10바이트 payload + `0x15` inline 앵커.
pub(crate) const CTRL_ID_NEW_NUMBER: u32 = 0x6E77_6E6F;

/// ctrl_id for the 감추기 (page hiding) control: ASCII `"pghd"` as
/// big-endian u32. 컨트롤이 놓인 쪽의 머리말/꼬리말/바탕쪽/테두리/배경/
/// 쪽번호를 감춘다 — F2 native fixture 실측 (2026-08-12): 8바이트 payload
/// (속성 u32 bits 0-5, secd word 와 동일 배열) + `0x15` inline 앵커.
pub(crate) const CTRL_ID_PAGE_HIDING: u32 = 0x7067_6864;

// ---------------------------------------------------------------------------
// §2 Annotation controls (CtrlHeader only)
// ---------------------------------------------------------------------------

/// ctrl_id for memo placeholder controls: ASCII `"%unk"` as big-endian u32.
///
/// 한컴 stores both memo annotations (with command `"MEMO/.../.../..."`) and
/// other user-unknown controls under this id; we recognize memos by the
/// `"MEMO/"` command prefix. Other `%unk` payloads continue to flow through
/// the `Hwp5Control::Unknown` fallback.
///
/// Distinct from [`CTRL_ID_MEMO_INLINE`] (`"%%me"`, `0x2525_6D65`) — that
/// is the inline `FieldBegin` ctrl_id used to position the memo anchor.
pub(crate) const CTRL_ID_MEMO: u32 = 0x2575_6E6B;

/// ctrl_id for the dutmal (덧말) control: ASCII `"tdut"` as big-endian u32.
///
/// Paired wire artifacts: an inline `0x17` marker in the body's
/// `ParaText` stream (carries the LE-stored ctrl_id as `"tudt"` in
/// `extra[0..4]`) plus this CtrlHeader carrying the actual
/// `mainText` / `subText` strings.
pub(crate) const CTRL_ID_DUTMAL: u32 = 0x7464_7574;

/// ctrl_id for the compose (글자겹침) control: ASCII `"tcps"` as
/// big-endian u32. Paired with an inline `0x17` marker whose
/// `extra[0..4]` carries the LE-stored ctrl_id `"spct"`. Payload
/// layout lives on `crate::schema::section::Hwp5ComposeControl`.
pub(crate) const CTRL_ID_COMPOSE: u32 = 0x7463_7073;

/// ctrl_id for the IndexMark (찾아보기 표시) control: ASCII `"idxm"`
/// as big-endian u32. Paired with an inline `0x16` marker whose
/// `extra[0..4]` carries the LE-stored ctrl_id `"mxdi"`
/// (`6D 78 64 69`). Payload layout lives on
/// `crate::schema::section::Hwp5IndexMarkControl`.
///
/// Used by both:
/// - `decoder/section.rs` for `CtrlHeader` (`0x47`) dispatch
/// - `schema/section.rs` for inline `0x16` marker discrimination
///   in `Hwp5ParaText::parse` (`ctrl_id_from_inline_extra_bytes` reverses
///   the LE bytes to BE-ascii, matching this value)
pub(crate) const CTRL_ID_INDEXMARK: u32 = 0x6964_786D;

// ---------------------------------------------------------------------------
// §3 `%`-class field controls (CtrlHeader + FieldBegin pair)
// ---------------------------------------------------------------------------

/// ctrl_id for the ClickHere (누름틀) press-field: ASCII `"%clk"` as
/// big-endian u32. Wave 12l.
///
/// Wire: inline `FieldBegin` carries this id; the CtrlHeader carries
/// the `Hwp5ClickHereControl` payload (hint/help BSTRs). A `0x57 lvl=2`
/// (`TagId::CtrlData`) sub-record follows with the form-mode `name`.
pub(crate) const CTRL_ID_CLICK_HERE: u32 = 0x2563_6C6B;

/// ctrl_id for the SUMMERY auto-field family (`$author`, `$lastsaveby`,
/// `$createtime`, `$modifiedtime`, `$title`, …): ASCII `"%smr"` as
/// big-endian u32. Wave 12n. Payload layout lives on
/// `crate::schema::section::Hwp5SummaryControl`.
pub(crate) const CTRL_ID_FIELD_SUMMERY: u32 = 0x2573_6D72;

/// ctrl_id for the `%dte` date/time format-code field: ASCII `"%dte"` as
/// big-endian u32. Wave 12n. Payload layout lives on
/// `crate::schema::section::Hwp5DateCodeControl`.
pub(crate) const CTRL_ID_FIELD_DATE_CODE: u32 = 0x2564_7465;

/// ctrl_id for the `%pat` path/file-name field: ASCII `"%pat"` as
/// big-endian u32. Wave 12n. Payload layout lives on
/// `crate::schema::section::Hwp5PathFieldControl`.
pub(crate) const CTRL_ID_FIELD_PATH: u32 = 0x2570_6174;

/// ctrl_id for the `%xrf` cross-reference field: ASCII `"%xrf"` as
/// big-endian bytes (`0x25 0x78 0x72 0x66`). See
/// `crate::schema::section::Hwp5CrossRefControl`. (Wave 12m Phase 2.)
pub(crate) const CTRL_ID_FIELD_CROSSREF: u32 = 0x2578_7266;

/// ctrl_id for the `%bmk` bookmark span field: ASCII `"%bmk"` as
/// big-endian u32. Inline `FieldBegin` / `FieldEnd` mark span endpoints.
pub(crate) const CTRL_ID_BOOKMARK_SPAN: u32 = 0x2562_6D6B;

/// ctrl_id for the `%hlk` hyperlink field: ASCII `"%hlk"` as
/// big-endian u32. Inline `FieldBegin` / `FieldEnd` mark span endpoints.
pub(crate) const CTRL_ID_HYPERLINK: u32 = 0x2568_6C6B;

/// ctrl_id for the `"bokm"` bookmark POINT control (singular, not a span):
/// ASCII `"bokm"` as big-endian u32. Distinct from [`CTRL_ID_BOOKMARK_SPAN`]
/// (`"%bmk"`) — `bokm` is a CtrlHeader-attached point bookmark, `%bmk` is
/// the span-style FieldBegin/FieldEnd pair.
pub(crate) const CTRL_ID_BOOKMARK_POINT: u32 = 0x626F_6B6D;

// ---------------------------------------------------------------------------
// §4 Inline-marker controls (extra[0..4] discrimination)
// ---------------------------------------------------------------------------

/// ctrl_id for the `atno` inline page-number control: ASCII `"atno"` as
/// big-endian u32. Wave 12n. Payload layout lives on
/// `crate::schema::section::Hwp5InlinePageNumberControl`.
///
/// `atno` reaches the projection layer through a `0x12` inline marker
/// (`TextSegment::ControlRef`), not a `0x03` `FieldBegin` path — so
/// despite the Wave 12n family naming, this is **not** in the `%`-class
/// `FIELD_*` family. The constant lives in §4 (inline-marker controls)
/// for that reason.
pub(crate) const CTRL_ID_ATNO: u32 = 0x6174_6E6F;

// ---------------------------------------------------------------------------
// §5 Projection-only inline markers
// ---------------------------------------------------------------------------

/// Inline `FieldBegin` ctrl_id for memo anchors (`"%%me"` BE-ascii,
/// `0x2525_6D65`).
///
/// In the HWP5 body text stream, memos are embedded as `FieldBegin` /
/// `FieldEnd` markers whose `extra[0..4]` raw bytes are
/// `65 6D 25 25` (ASCII `e m % %` — same "LE-stored u32 of BE-ascii name"
/// convention as `%bmk` / `%hlk` / `%xrf`). After
/// `ctrl_id_from_inline_extra` reverses + reads BE, that yields
/// `0x2525_6D65`.
///
/// **Not the same as [`CTRL_ID_MEMO`]** (`"%unk"`, `0x2575_6E6B`) — that
/// one is the CtrlHeader ctrl_id for memo placeholders. HWP5 uses one
/// ID for the inline anchor and another for the `CtrlHeader` placeholder.
pub(crate) const CTRL_ID_MEMO_INLINE: u32 = 0x2525_6D65;
