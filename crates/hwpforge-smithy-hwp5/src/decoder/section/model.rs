//! Decoded data model for the HWP5 `BodyText/Section{N}` decoder.
//!
//! Pure data types (paragraphs, controls, tables, section results) produced by
//! the section parser and consumed by the projection layer. Split out of
//! `decoder/section.rs` verbatim (E7 file split); no behavior change.

use crate::decoder::Hwp5Warning;
use crate::schema::section::{
    Hwp5CharShapeRun, Hwp5DutmalControl, Hwp5MemoCommand, Hwp5PageDef, Hwp5ParaLineSeg,
    Hwp5ShapeComponentGeometry, Hwp5ShapePoint, Hwp5ShapeTextArt, TextSegment,
};

/// A decoded paragraph from a BodyText section.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct Hwp5Paragraph {
    /// The paragraph's text content (all Text segments concatenated, with
    /// tab/space/newline substituted for control codes).
    pub text: String,
    /// The raw decoded text segments in paragraph order before flattening.
    pub text_segments: Vec<TextSegment>,
    /// Paragraph shape ID (index into DocInfo para_shapes).
    pub para_shape_id: u16,
    /// Style ID (index into DocInfo styles).
    pub style_id: u8,
    /// Character shape runs: (position, char_shape_id) pairs.
    pub char_shape_runs: Vec<Hwp5CharShapeRun>,
    /// Format-local line layout cache entries from `ParaLineSeg`.
    pub line_segments: Vec<Hwp5ParaLineSeg>,
    /// Inline control objects found in this paragraph (table refs, footnote refs, etc.).
    pub controls: Vec<Hwp5Control>,
}

/// A control object reference found inline in paragraph text.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum Hwp5Control {
    /// Table reference with nested cell paragraphs parsed from child records.
    Table(Hwp5Table),
    /// Image evidence resolved from `gso ` + `ShapeComponent` + `ShapePicture`.
    Image(Hwp5ImageControl),
    /// Line evidence resolved from `gso ` + `ShapeComponent` + `ShapeComponentLine`.
    Line(Hwp5LineControl),
    /// Pure rectangle evidence resolved from `gso ` + `ShapeComponent` + `ShapeComponentRect`.
    Rect(Hwp5RectControl),
    /// Polygon evidence resolved from `gso ` + `ShapeComponent` + `ShapeComponentPolygon`.
    Polygon(Hwp5PolygonControl),
    /// Ellipse evidence resolved from `gso ` + `ShapeComponent` + `ShapeComponentEllipse`.
    Ellipse(Hwp5EllipseControl),
    /// Arc evidence resolved from the same `ShapeComponentEllipse` (`0x50`) record
    /// with arc fields populated — 한컴 does not emit a separate arc sub-record.
    Arc(Hwp5ArcControl),
    /// Curve evidence resolved from `gso ` + `ShapeComponent` + `ShapeComponentCurve`.
    Curve(Hwp5CurveControl),
    /// Connect-line evidence. 한컴 stores connectors in the same
    /// `ShapeComponentLine` (`0x4E`) sub-record as a plain line, distinguished
    /// only by the `ShapeComponent` type tag `"$col"`.
    ConnectLine(Hwp5ConnectLineControl),
    /// Equation editor control (`eqed`) carrying a HancomEQN script.
    Equation(Hwp5EquationControl),
    /// Memo (메모) annotation control. The HWP5 ctrl carries only a
    /// placeholder (`%unk` ctrl with command `"MEMO/{shapeId}/{memo_id}/{instId}"`);
    /// the memo body content lives in a separate `HWPTAG_MEMO_LIST` (0x5D)
    /// cluster at the section's last body paragraph and is joined back by
    /// `memo_id` during `BodyTextParserState::finish`.
    Memo(Hwp5MemoControl),
    /// Dutmal (덧말) — small ruby-style annotation that prints
    /// `main_text` in the body run and `sub_text` above/below it. Wire
    /// pairs an inline `0x17` marker in the body's `ParaText` stream
    /// with a `tdut` CtrlHeader carrying the actual strings — see
    /// `schema::section::Hwp5DutmalControl` for the payload layout.
    Dutmal(Hwp5DutmalControl),
    /// Compose (글자겹침) — overlaid/combined characters in one cell
    /// position. Wire pairs an inline `0x17` marker in the body's
    /// `ParaText` stream with a `tcps` CtrlHeader carrying the
    /// composed text and 10 charPr references — see
    /// `schema::section::Hwp5ComposeControl` for the payload layout.
    Compose(crate::schema::section::Hwp5ComposeControl),
    /// IndexMark (찾아보기 표시) — inline marker that names a
    /// document-level index entry. Wire pairs an inline `0x16`
    /// marker (extra carries the LE-stored ctrl_id `idxm`) in the
    /// body's `ParaText` with an `idxm` CtrlHeader carrying the
    /// `primary` text and an optional `secondary` text — see
    /// `schema::section::Hwp5IndexMarkControl` for the payload
    /// layout. (Wave 12k.)
    IndexMark(crate::schema::section::Hwp5IndexMarkControl),
    /// ClickHere (누름틀, CLICK_HERE press-field) — interactive form
    /// placeholder. Wire pairs an inline `0x03` (FIELD_BEGIN) marker in
    /// the body's `ParaText` stream with a `%clk` CtrlHeader carrying
    /// hint/help text + a following `0x57 lvl=2` sub-record carrying the
    /// form-mode name — see `schema::section::Hwp5ClickHereControl` for
    /// the payload layout. (Wave 12l.)
    ClickHere(crate::schema::section::Hwp5ClickHereControl),
    /// SUMMERY auto-field — `%smr` ctrl_id. Carries the Command `$token`
    /// (e.g. `$author`, `$modifiedtime`) that the projection layer maps
    /// to a typed `FieldType` or, for unknown tokens, to
    /// `Control::UnknownSummary`. See
    /// `schema::section::Hwp5SummaryControl`. (Wave 12n.)
    SummaryField(crate::schema::section::Hwp5SummaryControl),
    /// `%dte` date/time format-code field — carries a raw format pattern
    /// (e.g. `"\:1년 2월 3일 (6);0;"` or `"T\:;0;"`). The projection
    /// layer surfaces it as `Control::DateCodeField` with only the derived
    /// `is_time_mode` (from the `T` prefix); the raw wire pattern and trailer
    /// are smithy-internal and not carried into the core IR (E6 slice C).
    /// See `schema::section::Hwp5DateCodeControl`. (Wave 12n.)
    DateCodeField(crate::schema::section::Hwp5DateCodeControl),
    /// `%pat` path/file-name field — carries a path format-code
    /// Command (`"$P"`, `"$F"`, `"$P$F"`). See
    /// `schema::section::Hwp5PathFieldControl`. (Wave 12n.)
    PathField(crate::schema::section::Hwp5PathFieldControl),
    /// `atno` inline page-number control — carries a 4-byte kind flag
    /// (`0x00` current page, `0x06` total pages, other values forward
    /// preserved). See `schema::section::Hwp5InlinePageNumberControl`.
    /// (Wave 12n.)
    InlinePageNumber(crate::schema::section::Hwp5InlinePageNumberControl),
    /// `nwno` 새 번호 지정 control — 번호 카운터를 컨트롤 위치부터
    /// 재시작한다. `0x15` inline 앵커로 위치 보존 (W2 — F1 실측).
    /// See `schema::section::Hwp5NewNumberControl`.
    NewNumber(crate::schema::section::Hwp5NewNumberControl),
    /// `%xrf` cross-reference control — carries the structured
    /// `?<target>;N1;N2;N3;N4;` Command with raw RefType / ContentType /
    /// hyperlink codes. The projection layer maps these to typed
    /// `Control::CrossRef` via boundary functions in
    /// `smithy-hwp5/src/projection.rs`. See
    /// `schema::section::Hwp5CrossRefControl`. (Wave 12m Phase 2.)
    CrossRef(crate::schema::section::Hwp5CrossRefControl),
    /// Header control with nested subtree paragraphs.
    Header(Hwp5NestedSubtree),
    /// Footer control with nested subtree paragraphs.
    Footer(Hwp5NestedSubtree),
    /// Footnote control with nested subtree paragraphs.
    ///
    /// HWP5 encodes footnotes as an inline `0x06` control-char marker in the
    /// paragraph text stream followed by a `CtrlHeader` carrying ctrl_id
    /// `"fn  "` (`0x666E2020`). The subtree mirrors the header/footer layout
    /// (`ListHeader` → nested `ParaHeader`s).
    Footnote(Hwp5NestedSubtree),
    /// Endnote control with nested subtree paragraphs.
    ///
    /// Same structure as [`Hwp5Control::Footnote`] but with ctrl_id `"en  "`
    /// (`0x656E2020`).
    Endnote(Hwp5NestedSubtree),
    /// Textbox-like shape with nested subtree paragraphs.
    TextBox(Hwp5TextBoxControl),
    /// Embedded OLE object evidence resolved from `gso ` + `ShapeComponent` + `ShapeComponentOle`.
    OleObject(Hwp5OleObjectControl),
    /// Group (묶음 객체) evidence resolved from `gso ` + a `ShapeComponent`
    /// (`0x4C`) carrying the `"$con"` type tag wrapping child
    /// `ShapeComponent`s. Wave A carries FLAT children only; a nested
    /// `$con`-in-`$con` is degraded to `Unknown` with a warning (see
    /// [`GsoGroupBuilder`]). Depth-capped at [`GSO_GROUP_MAX_DEPTH`].
    Group(Hwp5GroupControl),
    /// TextArt (글맵시) evidence: gso ShapeComponent "$tat" + a 0x5A ShapeTextArt sub-record.
    TextArt(Hwp5TextArtControl),
    /// Generic/unsupported control — preserve the ctrl_id for future expansion.
    Unknown {
        /// Four-byte control ID (big-endian ASCII, e.g. 0x74626C20 = 'tbl ').
        ctrl_id: u32,
        /// Raw `CtrlHeader` payload bytes for fixture-driven future decoding.
        header_data: Vec<u8>,
    },
}

/// Parsed image evidence from a `gso ` scope.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5ImageControl {
    /// Owning control identifier, currently always `gso `.
    pub ctrl_id: u32,
    /// Minimal recovered geometry.
    pub geometry: Hwp5ShapeComponentGeometry,
    /// `DocInfo/BinData` item identifier referenced by `ShapePicture`.
    pub binary_data_id: u16,
    /// Wave 12p Step 1c-3: `gso ` CtrlHeader trailer 의 instance ID.
    /// HWPX cross-ref Command `?#1108165583;1;0;0;0;` (TARGET_PICTURE)
    /// 의 target ID 가 한컴 native `<hp:pic id="1108165583">` 와 매칭.
    #[allow(dead_code)]
    pub instance_id: u32,
}

/// Parsed line evidence from a `gso ` scope.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5LineControl {
    /// Owning control identifier, currently always `gso `.
    #[allow(dead_code)] // reserved for semantic/control-audit slices
    pub ctrl_id: u32,
    /// Minimal recovered geometry.
    pub geometry: Hwp5ShapeComponentGeometry,
    /// Line start point in local object coordinates.
    pub start: Hwp5ShapePoint,
    /// Line end point in local object coordinates.
    pub end: Hwp5ShapePoint,
}

/// Parsed pure rectangle evidence from a `gso ` scope.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5RectControl {
    /// Owning control identifier, currently always `gso `.
    #[allow(dead_code)] // reserved for semantic/control-audit slices
    pub ctrl_id: u32,
    /// Minimal recovered geometry.
    pub geometry: Hwp5ShapeComponentGeometry,
}

/// Parsed polygon evidence from a `gso ` scope.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5PolygonControl {
    /// Owning control identifier, currently always `gso `.
    #[allow(dead_code)] // reserved for semantic/control-audit slices
    pub ctrl_id: u32,
    /// Minimal recovered geometry.
    pub geometry: Hwp5ShapeComponentGeometry,
    /// Ordered polygon vertices in local object coordinates.
    pub points: Vec<Hwp5ShapePoint>,
}

/// Parsed plain-ellipse evidence from a `gso ` scope.
///
/// Only geometry (placement + extent) is carried: projection derives the
/// ellipse center/axes from the bounding box via `Control::ellipse`. The
/// precise center/axis points decoded in [`Hwp5ShapeComponentEllipse`] are
/// not needed downstream yet (they would only matter for exact-geometry
/// fidelity, a future refinement).
#[derive(Debug, Clone)]
pub(crate) struct Hwp5EllipseControl {
    /// Owning control identifier, currently always `gso `.
    #[allow(dead_code)] // reserved for semantic/control-audit slices
    pub ctrl_id: u32,
    /// Minimal recovered geometry (placement + extent).
    pub geometry: Hwp5ShapeComponentGeometry,
}

/// Parsed TextArt (글맵시) evidence from a `gso ` scope whose `ShapeComponent`
/// (`0x4C`) carries the `"$tat"` type tag wrapping a `ShapeTextArt` (`0x5A`)
/// sub-record.
///
/// Geometry (placement + extent) comes from the owning `gso ` `CtrlHeader`,
/// identical to how an ellipse gets its geometry; the warped-text payload
/// (text, font, shape enum, spacing, alignment) comes from the `0x5A` record.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5TextArtControl {
    /// Owning control identifier, currently always `gso `.
    #[allow(dead_code)] // reserved for semantic/control-audit slices
    pub ctrl_id: u32,
    /// Minimal recovered geometry (placement + extent).
    pub geometry: Hwp5ShapeComponentGeometry,
    /// Parsed warped-text payload from the `0x5A` `ShapeTextArt` sub-record.
    pub text_art: Hwp5ShapeTextArt,
    /// gso CtrlHeader trailer instance ID (mirror `Hwp5ImageControl.instance_id`).
    pub instance_id: u32,
}

/// Parsed arc evidence from a `gso ` scope (a `ShapeComponentEllipse` with arc fields).
///
/// Carries geometry only for now. We emit a `Normal` arc sized from the
/// bounding box; exact arc-sweep endpoints and pie/chord arc types are a
/// future refinement that needs dedicated fixtures, so the decoded arc points
/// are intentionally not propagated here.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5ArcControl {
    /// Owning control identifier, currently always `gso `.
    #[allow(dead_code)] // reserved for semantic/control-audit slices
    pub ctrl_id: u32,
    /// Minimal recovered geometry (placement + extent).
    pub geometry: Hwp5ShapeComponentGeometry,
}

/// Parsed connect-line evidence from a `gso ` scope.
///
/// Shares the `ShapeComponentLine` (`0x4E`) sub-record with a plain line; the
/// distinction comes from the `ShapeComponent` type tag. Only the endpoints and
/// geometry are carried — the connector's object-link references map to no HWPX
/// `<hp:connectLine>` attribute, so they are intentionally dropped.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5ConnectLineControl {
    /// Owning control identifier, currently always `gso `.
    #[allow(dead_code)] // reserved for semantic/control-audit slices
    pub ctrl_id: u32,
    /// Minimal recovered geometry (placement + extent).
    pub geometry: Hwp5ShapeComponentGeometry,
    /// Connector start point in local object coordinates.
    pub start: Hwp5ShapePoint,
    /// Connector end point in local object coordinates.
    pub end: Hwp5ShapePoint,
}

/// Parsed equation evidence from an `eqed` ctrl + `HWPTAG_EQEDIT` child record.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5EquationControl {
    /// Owning control identifier, always `eqed`.
    #[allow(dead_code)] // reserved for semantic/control-audit slices
    pub ctrl_id: u32,
    /// Minimal recovered geometry (equation box extent) from the ctrl header.
    pub geometry: Hwp5ShapeComponentGeometry,
    /// HancomEQN script text recovered from the `HWPTAG_EQEDIT` record.
    pub script: String,
    /// Wave 12p Step 1c-2: `eqed` CtrlHeader trailer 의 instance ID
    /// (last 8 bytes 의 first 4, u32 LE). HWPX cross-ref Command
    /// `?#1108165599;2;0;0;0;` (TARGET_EQUATION) 의 target ID 가
    /// 한컴 native `<hp:equation id="1108165599">` 와 매칭.
    #[allow(dead_code)]
    pub instance_id: u32,
}

/// Parsed memo placeholder from a `%unk` ctrl with command `"MEMO/.../.../..."`.
///
/// Wire metadata (shape_id / hancom_inst_* / author / terminator) lives in
/// `command`. The decoder pushes this placeholder with `paragraphs: vec![]`
/// when it sees the inline ctrl, then fills `paragraphs` from
/// [`BodyTextParserState::memo_contents`] during `finish()` once the matching
/// `HWPTAG_MEMO_LIST` cluster at the end of the section's last body paragraph
/// has been captured. Cluster matching is keyed by `command.memo_id`, not by
/// document position — see `phase12e_memo_design.md` for the wire layout.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5MemoControl {
    /// Owning control identifier, always `%unk` (0x2575_6E6B BE-ascii).
    #[allow(dead_code)] // reserved for semantic/control-audit slices
    pub ctrl_id: u32,
    /// Full wire command — shape_id / memo_id / author / etc. The
    /// downstream HWPX encoder uses these fields to emit the seven
    /// `<hp:parameters>` 한컴 needs for correct memo classification (see
    /// `.docs/algorithms/2026-06-01_memo_anchor_serialization.md`).
    pub command: Hwp5MemoCommand,
    /// Memo body paragraphs. Empty after the inline-ctrl push; filled by
    /// `attach_memo_contents_to_placeholders` during `finish()`. Stays empty
    /// when the matching cluster is missing (warning surfaced).
    pub paragraphs: Vec<Hwp5Paragraph>,
}

/// Parsed curve evidence from a `gso ` scope.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5CurveControl {
    /// Owning control identifier, currently always `gso `.
    #[allow(dead_code)] // reserved for semantic/control-audit slices
    pub ctrl_id: u32,
    /// Minimal recovered geometry (placement + extent).
    pub geometry: Hwp5ShapeComponentGeometry,
    /// Ordered curve control points in local object coordinates.
    pub points: Vec<Hwp5ShapePoint>,
    /// Per-gap segment type bytes (`0` = line, `1` = curve).
    pub segment_types: Vec<u8>,
}

/// Parsed textbox evidence from a `gso ` scope carrying `drawText/subList`.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5TextBoxControl {
    /// Owning control identifier, currently always `gso `.
    pub ctrl_id: u32,
    /// Minimal recovered geometry from the owning `CtrlHeader`.
    pub geometry: Hwp5ShapeComponentGeometry,
    /// Nested paragraphs captured from the textbox subtree.
    pub paragraphs: Vec<Hwp5Paragraph>,
    /// `속성` (UINT32) word of the textbox `HWPTAG_LIST_HEADER` record (표 65),
    /// when present. Bits 5–6 carry the text vertical alignment; projection
    /// maps `(props >> 5) & 0x03` → Core `VerticalAlign`. `None` when the
    /// ListHeader was absent or too short to carry the 속성 word.
    pub list_header_properties: Option<u32>,
}

/// Parsed OLE-backed object evidence from a `gso ` scope.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5OleObjectControl {
    /// Owning control identifier, currently always `gso `.
    pub ctrl_id: u32,
    /// Minimal recovered geometry.
    pub geometry: Hwp5ShapeComponentGeometry,
    /// `DocInfo/BinData` item identifier referenced by `ShapeComponentOle`.
    pub binary_data_id: u16,
    /// Embedded object extent width in HWPUNIT.
    pub extent_width: i32,
    /// Embedded object extent height in HWPUNIT.
    pub extent_height: i32,
}

/// Parsed group (묶음 객체) evidence from a `gso ` scope whose first
/// `ShapeComponent` (`0x4C`) carries the `"$con"` type tag.
///
/// `children` holds the flat shape controls decoded from the deeper-level
/// `ShapeComponent`s nested under the `$con` record (Wave A). Each child is
/// produced by the same per-record decoders the single-shape path uses, so
/// the group adds no new shape-parsing logic — only scope bookkeeping.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5GroupControl {
    /// Owning control identifier, always `gso `.
    #[allow(dead_code)] // reserved for semantic/control-audit slices
    pub ctrl_id: u32,
    /// Group bounding-box geometry from the owning `gso ` `CtrlHeader`.
    pub geometry: Hwp5ShapeComponentGeometry,
    /// Child drawing objects in document (z-) order.
    pub children: Vec<Hwp5GroupChild>,
    /// `gso ` CtrlHeader trailer instance ID (mirrored to HWPX
    /// `<hp:container instid>`). `0` when not recoverable.
    pub instance_id: u32,
}

/// One child of a [`Hwp5GroupControl`]: a shape control plus any `drawText`
/// paragraphs it carried.
///
/// The shape `control` is the typed evidence (`Rect`/`Ellipse`/`Line`/...);
/// `paragraphs` is non-empty only for text-bearing shapes (the native group
/// fixture's rect and ellipse both hold a single text paragraph). Carrying
/// paragraphs separately keeps the existing typed `Hwp5Control` shape
/// variants unchanged while letting the projection layer attach the text to
/// the Core shape (`Control::TextBox` for rects, `ellipse_with_text` for
/// ellipses).
#[derive(Debug, Clone)]
pub(crate) struct Hwp5GroupChild {
    /// The child shape control.
    pub control: Hwp5Control,
    /// `drawText` paragraphs carried by the child, if any.
    pub paragraphs: Vec<Hwp5Paragraph>,
    /// `속성` (UINT32) word of the child's `HWPTAG_LIST_HEADER` record (표 65),
    /// when present. Bits 5–6 carry text vertical alignment; projection maps
    /// `(props >> 5) & 0x03` → Core `VerticalAlign` for text-bearing children.
    pub list_header_properties: Option<u32>,
}

/// Parsed table control content.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5Table {
    /// Number of rows declared by the table body record.
    pub rows: u16,
    /// Number of columns declared by the table body record.
    pub cols: u16,
    /// Page break behavior declared by the table body record.
    pub page_break: Hwp5TablePageBreak,
    /// Whether the table repeats its header row across page breaks.
    pub repeat_header: bool,
    /// Cell spacing declared by the table body record in HWPUNIT16.
    pub cell_spacing: i16,
    /// Optional table-level border/fill reference.
    pub border_fill_id: Option<u16>,
    /// Parsed cell records in source order.
    pub cells: Vec<Hwp5TableCell>,
    /// Wave 12p Step 1c-1: Table CtrlHeader trailer 의 instance ID
    /// (last 8 bytes 의 first 4, u32 LE). HWPX cross-ref Command
    /// `?#<id>` 의 target ID 와 매칭, 한컴 native `<hp:tbl id="..."`
    /// attribute 로 emit. 추출 불가하면 0.
    #[allow(dead_code)]
    pub instance_id: u32,
}

/// HWP5 table page break policy recovered from the table body record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hwp5TablePageBreak {
    /// Do not split the table across pages.
    None,
    /// Split at cell boundaries.
    Cell,
    /// Split at table boundaries.
    Table,
    /// Unknown raw value preserved for audit.
    Unknown(u8),
}

/// Vertical alignment recovered from a table cell `ListHeader`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hwp5TableCellVerticalAlign {
    /// Align cell content to the top edge.
    Top,
    /// Center cell content vertically.
    Center,
    /// Align cell content to the bottom edge.
    Bottom,
    /// Unknown raw value preserved for audit.
    Unknown(u8),
}

/// Explicit cell-local margin recovered from a table cell `ListHeader`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hwp5TableCellMargin {
    /// Left margin in HWPUNIT16.
    pub left: i16,
    /// Right margin in HWPUNIT16.
    pub right: i16,
    /// Top margin in HWPUNIT16.
    pub top: i16,
    /// Bottom margin in HWPUNIT16.
    pub bottom: i16,
}

/// Parsed table cell from a `ListHeader` after a `Table` record.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5TableCell {
    /// Zero-based column index.
    pub column: u16,
    /// Zero-based row index.
    pub row: u16,
    /// Horizontal span. Minimum 1.
    pub col_span: u16,
    /// Vertical span. Minimum 1.
    pub row_span: u16,
    /// Cell width in HWPUNIT.
    pub width: i32,
    /// Cell height in HWPUNIT.
    pub height: i32,
    /// Cell-local inner margin in HWPUNIT16.
    pub margin: Hwp5TableCellMargin,
    /// Cell content vertical alignment.
    pub vertical_align: Hwp5TableCellVerticalAlign,
    /// Whether this cell is marked as belonging to a title/header row.
    pub is_header: bool,
    /// Optional border/fill reference.
    #[allow(dead_code)] // reserved for later border/fill projection work
    pub border_fill_id: Option<u16>,
    /// Cell paragraphs.
    pub paragraphs: Vec<Hwp5Paragraph>,
}

/// Parsed nested subtree carried by a non-table control.
#[derive(Debug, Clone)]
pub(crate) struct Hwp5NestedSubtree {
    /// Original control identifier that owns the subtree.
    pub ctrl_id: u32,
    /// Raw 4-byte property field that follows `ctrl_id` in the
    /// `CtrlHeader` payload (HWP 5.0 spec §4.3.10.3 표 140·141 for
    /// header/footer ctrls). Projection decodes per-ctrl semantics
    /// from this value (e.g. bit 0~1 → header `applyPageType` —
    /// 0=BOTH, 1=EVEN, 2=ODD).
    ///
    /// Decoder stores the raw bytes verbatim and lets projection do
    /// the semantic split so the decoder stays format-agnostic
    /// (single source of payload, easy to extend for new bits).
    pub properties_raw: u32,
    /// Wave 12p Step 1b: CtrlHeader trailer 의 first 4 bytes (u32 LE).
    /// HWPX cross-ref Command `?#<id>` 의 target ID 와 매칭되는
    /// instance ID. Footnote/Endnote 이 cross-ref 대상이 될 때 한컴이
    /// 이 값을 `<hp:footNote instId="...">` / `<hp:endNote instId="...">`
    /// attribute 로 emit. 추출 불가하면 0.
    /// Wave 12p Step 4 (projection consumer) 가 land 하기 전까지는
    /// 미사용 — `#[allow(dead_code)]` 로 step 별 atomic commit 분리.
    #[allow(dead_code)]
    pub instance_id: u32,
    /// Nested paragraphs captured under the subtree.
    pub paragraphs: Vec<Hwp5Paragraph>,
}

/// Result of decoding one BodyText/Section{N} stream.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct SectionResult {
    /// Decoded paragraphs in document order.
    pub paragraphs: Vec<Hwp5Paragraph>,
    /// Page definition, if a PageDef record was found.
    pub page_def: Option<Hwp5PageDef>,
    /// Raw 4-byte property word from the `secd` ctrl (HWP 5.0 spec
    /// §4.3.10.1 표 130). `None` when no `secd` ctrl was encountered
    /// (e.g. truncated fixtures); bits 0~5 + 8/9 + 19 are decoded by
    /// projection into `Section.visibility` (gap B).
    pub section_def_properties: Option<u32>,
    /// `secd` payload `[20..28]` 시작번호 (쪽/그림/표/수식 각 u16 — F1 실측,
    /// 계획 §1.2). payload 가 28바이트 미만이면 부분 캡처 대신 `None`
    /// (all-or-none — 읽지 않은 값을 기본값으로 날조하지 않는다).
    pub section_def_start_numbers: Option<Hwp5SectionStartNumbers>,
    /// Page border/fill records (`HWPTAG_PAGE_BORDER_FILL`, 0x4B) that
    /// follow the `secd` ctrl. 한글 emits them in `[BOTH, EVEN, ODD]`
    /// order; projection maps each to a `PageBorderFillEntry`.
    pub page_border_fills: Vec<Hwp5PageBorderFill>,
    /// Column (다단) definition from the `cold` ctrl, if present and
    /// multi-column. `None` for single-column sections; projection maps a
    /// `col_count >= 2` value to `Section.column_settings`.
    pub column_def: Option<Hwp5ColumnDef>,
    /// Non-fatal warnings.
    pub warnings: Vec<Hwp5Warning>,
}

/// `secd` ctrl 의 시작번호 묶음 (payload `[20..28]`, 각 u16 LE).
///
/// F1 실측 (2026-08-12 rules-newnum): 한컴은 이 필드가 `1` 이고 속성
/// bits 20-21 이 0 인 구역을 `<hp:startNum page="0">`(이어서) 로 변환한다 —
/// "필드값 = 재시작" 이 아니다. bits ≠ 0 의 의미는 레퍼런스 3사 충돌로
/// 미확정 (corpus 0.12%) — projection 이 경고로 표면화한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hwp5SectionStartNumbers {
    /// 쪽 시작번호 (`[20..22]`).
    pub page: u16,
    /// 그림 시작번호 (`[22..24]`).
    pub pic: u16,
    /// 표 시작번호 (`[24..26]`).
    pub tbl: u16,
    /// 수식 시작번호 (`[26..28]`).
    pub equation: u16,
}

/// Decoded `cold` (column definition / 다단) ctrl payload.
///
/// Wire (after the 4-byte `cold` ctrl_id): `[4..6]` u16 property word
/// (bits 0-1 = column type, bits 2-9 = column count, bits 10-11 =
/// direction), `[6..8]` u16 column gap in HWPUNIT. Equal-width columns
/// (`sameSz`) store no per-column widths — 한글 computes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hwp5ColumnDef {
    /// Number of columns (bits 2-9 of the property word).
    pub col_count: u8,
    /// Gap between columns in HWPUNIT.
    pub gap: u16,
    /// Raw column-separator line (`<hp:colLine>`), if present. Projection
    /// maps it to `ColumnSettings.col_line`.
    pub border: Option<Hwp5ColumnBorder>,
}

/// Raw column-separator line bytes from the `cold` ctrl Border block.
///
/// Wire (6 bytes): `[0]` u8 kind ([`Hwp5BorderLineKind`] code), `[1]` u8 width
/// (HWP5 border-width index), `[2..6]` u32 color (COLORREF). Projection
/// converts these to a typed `ColumnLine`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hwp5ColumnBorder {
    /// Border-line kind code.
    pub kind: u8,
    /// Border-width index.
    pub width: u8,
    /// Line color as a raw `COLORREF` (`0x00BBGGRR`).
    pub color: u32,
}

/// A decoded `HWPTAG_PAGE_BORDER_FILL` (0x4B) record.
///
/// 14-byte layout (empirically verified against 한글 fixtures; note the
/// `borderFillId` trails the offsets, unlike some references):
///
/// | bytes | field |
/// |-------|-------|
/// | 0..4  | `properties` (UINT32) |
/// | 4..6  | `offset_left` (UINT16, HWPUNIT16) |
/// | 6..8  | `offset_right` |
/// | 8..10 | `offset_top` |
/// | 10..12| `offset_bottom` |
/// | 12..14| `border_fill_id` (UINT16, 1-based DocInfo index) |
///
/// `properties` bit 0 selects the border base (1 → paper edge, 0 → text
/// content), bit 1/2 include header/footer in the border area, bit 3
/// fills behind the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hwp5PageBorderFill {
    /// Raw property word.
    pub properties: u32,
    /// Offsets from the page edge, `[left, right, top, bottom]` in HWPUNIT16.
    pub offsets: [u16; 4],
    /// Referenced DocInfo border-fill id (1-based).
    pub border_fill_id: u16,
}

impl Hwp5PageBorderFill {
    /// Parses a 14-byte `HWPTAG_PAGE_BORDER_FILL` record body. Returns
    /// `None` when the record is shorter than the fixed layout.
    pub(crate) fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 14 {
            return None;
        }
        Some(Self {
            properties: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            offsets: [
                u16::from_le_bytes([data[4], data[5]]),
                u16::from_le_bytes([data[6], data[7]]),
                u16::from_le_bytes([data[8], data[9]]),
                u16::from_le_bytes([data[10], data[11]]),
            ],
            border_fill_id: u16::from_le_bytes([data[12], data[13]]),
        })
    }
}
