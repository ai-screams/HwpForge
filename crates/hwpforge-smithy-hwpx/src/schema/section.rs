//! XML schema types for `section*.xml` (hp:, hs: namespaces).
//!
//! Maps the `<hs:sec>` element tree into Rust structs via serde.
//! Unknown elements (shapes, controls, line segments) are silently
//! ignored for Phase 3 — we extract text, tables, images only.
//!
//! Fields are used by serde deserialization even if not directly accessed.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

// ── Section root ──────────────────────────────────────────────────

/// `<hs:sec>` — root element of section*.xml.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename = "sec")]
pub struct HxSection {
    #[serde(
        rename(serialize = "hp:p", deserialize = "p"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub paragraphs: Vec<HxParagraph>,
}

// ── Paragraph ─────────────────────────────────────────────────────

/// `<hp:p id="..." paraPrIDRef="3" styleIDRef="0" ...>`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxParagraph {
    #[serde(rename = "@id", default)]
    pub id: String,
    #[serde(rename = "@paraPrIDRef", default)]
    pub para_pr_id_ref: u32,
    #[serde(rename = "@styleIDRef", default)]
    pub style_id_ref: u32,
    #[serde(rename = "@pageBreak", default)]
    pub page_break: u32,
    #[serde(rename = "@columnBreak", default)]
    pub column_break: u32,
    #[serde(rename = "@merged", default)]
    pub merged: u32,

    #[serde(
        rename(serialize = "hp:run", deserialize = "run"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub runs: Vec<HxRun>,
    /// Line segment array for layout hints.
    #[serde(
        rename(serialize = "hp:linesegarray", deserialize = "linesegarray"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub linesegarray: Option<HxLineSegArray>,
}

// ── Run ───────────────────────────────────────────────────────────

/// `<hp:run charPrIDRef="0">`.
///
/// A run can contain multiple mixed children:
/// `<hp:secPr>`, `<hp:ctrl>`, `<hp:t>`, `<hp:tbl>`, `<hp:pic>`,
/// `<hp:rect>`, `<hp:ellipse>`, etc.
///
/// Phase 3 extracts text, tables, images, and secPr; everything else
/// is silently skipped by serde (no `deny_unknown_fields`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxRun {
    #[serde(rename = "@charPrIDRef", default)]
    pub char_pr_id_ref: u32,

    /// Section properties (typically in the first run of the first paragraph).
    #[serde(
        rename(serialize = "hp:secPr", deserialize = "secPr"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sec_pr: Option<HxSecPr>,

    /// All `<hp:t>` elements in this run (may be multiple).
    #[serde(
        rename(serialize = "hp:t", deserialize = "t"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub texts: Vec<HxText>,

    /// All `<hp:tbl>` elements in this run.
    #[serde(
        rename(serialize = "hp:tbl", deserialize = "tbl"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub tables: Vec<HxTable>,

    /// All `<hp:pic>` elements in this run.
    #[serde(
        rename(serialize = "hp:pic", deserialize = "pic"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub pictures: Vec<HxPic>,

    /// All `<hp:ctrl>` elements in this run (header, footer, colPr, pageNum, footnote, endnote).
    #[serde(
        rename(serialize = "hp:ctrl", deserialize = "ctrl"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub ctrls: Vec<HxCtrl>,

    /// All `<hp:rect>` elements in this run (textboxes with optional text content).
    #[serde(
        rename(serialize = "hp:rect", deserialize = "rect"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub rects: Vec<HxRect>,

    /// All `<hp:line>` elements in this run (line drawing objects).
    #[serde(
        rename(serialize = "hp:line", deserialize = "line"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub lines: Vec<HxLine>,

    /// All `<hp:ellipse>` elements in this run (ellipse/circle drawing objects).
    #[serde(
        rename(serialize = "hp:ellipse", deserialize = "ellipse"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub ellipses: Vec<HxEllipse>,

    /// All `<hp:polygon>` elements in this run (polygon drawing objects).
    #[serde(
        rename(serialize = "hp:polygon", deserialize = "polygon"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub polygons: Vec<HxPolygon>,
}

// ── Text ──────────────────────────────────────────────────────────

/// `<hp:t>수학</hp:t>` or `<hp:t/>` (empty).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxText {
    #[serde(rename = "$text", default)]
    pub text: String,
}

// ── Control wrapper ──────────────────────────────────────────────

/// `<hp:ctrl>` — wrapper for header, footer, colPr, pageNum, footnote, endnote.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxCtrl {
    /// Optional column properties element.
    #[serde(
        rename(serialize = "hp:colPr", deserialize = "colPr"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub col_pr: Option<HxColPr>,
    /// Optional header element.
    #[serde(
        rename(serialize = "hp:header", deserialize = "header"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub header: Option<HxHeaderFooter>,
    /// Optional footer element.
    #[serde(
        rename(serialize = "hp:footer", deserialize = "footer"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub footer: Option<HxHeaderFooter>,
    /// Optional page number element.
    #[serde(
        rename(serialize = "hp:pageNum", deserialize = "pageNum"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub page_num: Option<HxPageNum>,
    /// Optional footnote element.
    #[serde(
        rename(serialize = "hp:footNote", deserialize = "footNote"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub foot_note: Option<HxFootNote>,
    /// Optional endnote element.
    #[serde(
        rename(serialize = "hp:endNote", deserialize = "endNote"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub end_note: Option<HxEndNote>,
}

/// `<hp:colPr>` — column properties element.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxColPr {
    /// Element ID (usually empty).
    #[serde(rename = "@id", default)]
    pub id: String,
    /// Column flow type: NEWSPAPER or PARALLEL.
    #[serde(rename = "@type", default)]
    pub col_type: String,
    /// Column balance strategy: LEFT, RIGHT, or MIRROR.
    #[serde(rename = "@layout", default)]
    pub layout: String,
    /// Number of columns.
    #[serde(rename = "@colCount", default)]
    pub col_count: u32,
    /// Whether all columns have the same width (0 or 1).
    #[serde(rename = "@sameSz", default)]
    pub same_sz: u32,
    /// Gap between columns in HWPUNIT (only when sameSz=1).
    #[serde(rename = "@sameGap", default)]
    pub same_gap: i32,

    /// Individual column definitions (only when sameSz=0).
    #[serde(
        rename(serialize = "hp:col", deserialize = "col"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub columns: Vec<HxCol>,
}

/// `<hp:col>` — individual column width/gap.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxCol {
    /// Column width in HWPUNIT.
    #[serde(rename = "@width", default)]
    pub width: i32,
    /// Gap after this column in HWPUNIT (0 for last column).
    #[serde(rename = "@gap", default)]
    pub gap: i32,
}

/// `<hp:header>` or `<hp:footer>` — header/footer region with sub-list paragraphs.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxHeaderFooter {
    /// Element ID.
    #[serde(rename = "@id", default)]
    pub id: String,
    /// Page type: BOTH, EVEN, ODD.
    #[serde(rename = "@applyPageType", default)]
    pub apply_page_type: String,
    /// Sub-list containing paragraphs.
    #[serde(
        rename(serialize = "hp:subList", deserialize = "subList"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sub_list: Option<HxSubList>,
}

/// `<hp:pageNum>` — page number control element.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HxPageNum {
    /// Position: BOTTOM_CENTER, TOP_LEFT, etc.
    #[serde(rename = "@pos", default)]
    pub pos: String,
    /// Format type: DIGIT, ROMAN_CAPITAL, etc.
    #[serde(rename = "@formatType", default)]
    pub format_type: String,
    /// Side character (e.g. "-").
    #[serde(rename = "@sideChar", default)]
    pub side_char: String,
}

// ── Footnote / Endnote ───────────────────────────────────────────

/// `<hp:footNote>` — footnote element (NoteType in XSD).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxFootNote {
    /// Instance identifier (optional, for linking references).
    #[serde(rename = "@instId", default, skip_serializing_if = "Option::is_none")]
    pub inst_id: Option<u32>,

    /// Paragraph content container (required).
    #[serde(rename(serialize = "hp:subList", deserialize = "subList"))]
    pub sub_list: HxSubList,
}

/// `<hp:endNote>` — endnote element (NoteType in XSD, same structure as footnote).
pub type HxEndNote = HxFootNote;

/// `<hp:footNotePr>` — section-level footnote formatting (decoder-only for Phase 4.5).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HxFootNotePr {
    /// Raw XML content preserved for roundtrip fidelity.
    #[serde(rename = "$value", default)]
    pub raw_xml: String,
}

/// `<hp:endNotePr>` — section-level endnote formatting (decoder-only for Phase 4.5).
pub type HxEndNotePr = HxFootNotePr;

// ── Caption ──────────────────────────────────────────────────────

/// `<hp:caption>` — caption element attached to shapes (tables, images, rects, etc.).
///
/// Captions contain paragraph content via a sub-list and are positioned
/// relative to their parent object (LEFT, RIGHT, TOP, BOTTOM).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxCaption {
    /// Caption side: LEFT, RIGHT, TOP, BOTTOM.
    #[serde(rename = "@side", default = "default_caption_side")]
    pub side: String,
    /// Include outer margin in caption width (0=false, 1=true).
    #[serde(rename = "@fullSz", default)]
    pub full_sz: u32,
    /// Caption width in HWPUNIT.
    #[serde(rename = "@width", default)]
    pub width: i32,
    /// Gap between caption and object (default: 850 HWPUNIT ~= 3mm).
    #[serde(rename = "@gap", default = "default_caption_gap")]
    pub gap: i32,
    /// Max text width = parent object width (HWPUNIT).
    #[serde(rename = "@lastWidth", default)]
    pub last_width: u32,
    /// Caption paragraph content.
    #[serde(rename(serialize = "hp:subList", deserialize = "subList"))]
    pub sub_list: HxSubList,
}

/// XSD default is LEFT; Core `CaptionSide::default()` uses Bottom for Korean doc convenience.
fn default_caption_side() -> String {
    "LEFT".to_string()
}

fn default_caption_gap() -> i32 {
    850
}

// ── Section Properties ────────────────────────────────────────────

/// `<hp:secPr>` — section settings, embedded in the first paragraph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxSecPr {
    #[serde(rename = "@textDirection", default)]
    pub text_direction: String,

    #[serde(
        rename(serialize = "hp:pagePr", deserialize = "pagePr"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub page_pr: Option<HxPagePr>,
    // footNotePr, endNotePr, grid, startNum, visibility, lineNumberShape,
    // pageBorderFill — all skipped by serde (no deny_unknown_fields).
    // The encoder injects these as raw XML strings via enrich_sec_pr().
}

/// `<hp:pagePr landscape="WIDELY" width="59528" height="84188">`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxPagePr {
    #[serde(rename = "@landscape", default)]
    pub landscape: String,
    #[serde(rename = "@width", default)]
    pub width: i32,
    #[serde(rename = "@height", default)]
    pub height: i32,
    #[serde(rename = "@gutterType", default)]
    pub gutter_type: String,

    #[serde(
        rename(serialize = "hp:margin", deserialize = "margin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub margin: Option<HxPageMargin>,
}

/// `<hp:margin header="4252" footer="4252" gutter="0" left="8504" ...>`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct HxPageMargin {
    #[serde(rename = "@header", default)]
    pub header: i32,
    #[serde(rename = "@footer", default)]
    pub footer: i32,
    #[serde(rename = "@gutter", default)]
    pub gutter: i32,
    #[serde(rename = "@left", default)]
    pub left: i32,
    #[serde(rename = "@right", default)]
    pub right: i32,
    #[serde(rename = "@top", default)]
    pub top: i32,
    #[serde(rename = "@bottom", default)]
    pub bottom: i32,
}

// ── Line Segment Array ────────────────────────────────────────────

/// `<hp:linesegarray>` — container for line layout segments.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct HxLineSegArray {
    /// Individual line segments.
    #[serde(
        rename(serialize = "hp:lineseg", deserialize = "lineseg"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub items: Vec<HxLineSeg>,
}

/// `<hp:lineseg>` — a single line layout segment with position/size hints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxLineSeg {
    /// Character position in the paragraph where this line starts.
    #[serde(rename = "@textpos", default)]
    pub textpos: u32,
    /// Vertical position from the top of the paragraph (HWPUNIT).
    #[serde(rename = "@vertpos", default)]
    pub vertpos: i32,
    /// Vertical size of the line (HWPUNIT).
    #[serde(rename = "@vertsize", default)]
    pub vertsize: i32,
    /// Text height within the line (HWPUNIT).
    #[serde(rename = "@textheight", default)]
    pub textheight: i32,
    /// Baseline position from the top of the line (HWPUNIT).
    #[serde(rename = "@baseline", default)]
    pub baseline: i32,
    /// Line spacing value (HWPUNIT).
    #[serde(rename = "@spacing", default)]
    pub spacing: i32,
    /// Horizontal position of the line start (HWPUNIT).
    #[serde(rename = "@horzpos", default)]
    pub horzpos: i32,
    /// Horizontal size available for text (HWPUNIT).
    #[serde(rename = "@horzsize", default)]
    pub horzsize: i32,
    /// Layout flags (393216 = standard value).
    #[serde(rename = "@flags", default)]
    pub flags: u32,
}

// ── Table ─────────────────────────────────────────────────────────

/// `<hp:tbl>` — full table element with all attributes required by 한글.
///
/// Field order matters for serialization: attributes first, then
/// `hp:sz`, `hp:pos`, `hp:outMargin`, `hp:inMargin`, then `hp:tr` rows.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxTable {
    // ── Attributes ──
    #[serde(rename = "@id", default)]
    pub id: String,
    #[serde(rename = "@zOrder", default)]
    pub z_order: u32,
    #[serde(rename = "@numberingType", default)]
    pub numbering_type: String,
    #[serde(rename = "@textWrap", default)]
    pub text_wrap: String,
    #[serde(rename = "@textFlow", default)]
    pub text_flow: String,
    #[serde(rename = "@lock", default)]
    pub lock: u32,
    #[serde(rename = "@dropcapstyle", default)]
    pub dropcap_style: String,
    #[serde(rename = "@pageBreak", default)]
    pub page_break: String,
    #[serde(rename = "@repeatHeader", default)]
    pub repeat_header: u32,
    #[serde(rename = "@rowCnt", default)]
    pub row_cnt: u32,
    #[serde(rename = "@colCnt", default)]
    pub col_cnt: u32,
    #[serde(rename = "@cellSpacing", default)]
    pub cell_spacing: u32,
    #[serde(rename = "@borderFillIDRef", default)]
    pub border_fill_id_ref: u32,
    #[serde(rename = "@noAdjust", default)]
    pub no_adjust: u32,

    // ── Sub-elements (order: sz → pos → outMargin → inMargin → rows) ──
    #[serde(
        rename(serialize = "hp:sz", deserialize = "sz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sz: Option<HxTableSz>,
    #[serde(
        rename(serialize = "hp:pos", deserialize = "pos"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pos: Option<HxTablePos>,
    #[serde(
        rename(serialize = "hp:outMargin", deserialize = "outMargin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub out_margin: Option<HxTableMargin>,
    #[serde(
        rename(serialize = "hp:caption", deserialize = "caption"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub caption: Option<HxCaption>,
    #[serde(
        rename(serialize = "hp:inMargin", deserialize = "inMargin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub in_margin: Option<HxTableMargin>,
    #[serde(
        rename(serialize = "hp:tr", deserialize = "tr"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub rows: Vec<HxTableRow>,
}

/// `<hp:sz>` — table size specification.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxTableSz {
    #[serde(rename = "@width", default)]
    pub width: i32,
    #[serde(rename = "@widthRelTo", default)]
    pub width_rel_to: String,
    #[serde(rename = "@height", default)]
    pub height: i32,
    #[serde(rename = "@heightRelTo", default)]
    pub height_rel_to: String,
    #[serde(rename = "@protect", default)]
    pub protect: u32,
}

/// `<hp:pos>` — table position specification.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxTablePos {
    #[serde(rename = "@treatAsChar", default)]
    pub treat_as_char: u32,
    #[serde(rename = "@affectLSpacing", default)]
    pub affect_l_spacing: u32,
    #[serde(rename = "@flowWithText", default)]
    pub flow_with_text: u32,
    #[serde(rename = "@allowOverlap", default)]
    pub allow_overlap: u32,
    #[serde(rename = "@holdAnchorAndSO", default)]
    pub hold_anchor_and_so: u32,
    #[serde(rename = "@vertRelTo", default)]
    pub vert_rel_to: String,
    #[serde(rename = "@horzRelTo", default)]
    pub horz_rel_to: String,
    #[serde(rename = "@vertAlign", default)]
    pub vert_align: String,
    #[serde(rename = "@horzAlign", default)]
    pub horz_align: String,
    #[serde(rename = "@vertOffset", default)]
    pub vert_offset: i32,
    #[serde(rename = "@horzOffset", default)]
    pub horz_offset: i32,
}

/// `<hp:outMargin>` / `<hp:inMargin>` / `<hp:cellMargin>` — margin specification.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxTableMargin {
    #[serde(rename = "@left", default)]
    pub left: i32,
    #[serde(rename = "@right", default)]
    pub right: i32,
    #[serde(rename = "@top", default)]
    pub top: i32,
    #[serde(rename = "@bottom", default)]
    pub bottom: i32,
}

/// `<hp:tr>`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxTableRow {
    #[serde(
        rename(serialize = "hp:tc", deserialize = "tc"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub cells: Vec<HxTableCell>,
}

/// `<hp:tc>` — table cell with all attributes required by 한글.
///
/// Field order: attributes, then `hp:subList`, `hp:cellAddr`,
/// `hp:cellSpan`, `hp:cellSz`, `hp:cellMargin`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxTableCell {
    // ── Attributes ──
    #[serde(rename = "@name", default)]
    pub name: String,
    #[serde(rename = "@header", default)]
    pub header: u32,
    #[serde(rename = "@hasMargin", default)]
    pub has_margin: u32,
    #[serde(rename = "@protect", default)]
    pub protect: u32,
    #[serde(rename = "@editable", default)]
    pub editable: u32,
    #[serde(rename = "@dirty", default)]
    pub dirty: u32,
    #[serde(rename = "@borderFillIDRef", default)]
    pub border_fill_id_ref: u32,

    // ── Sub-elements (order: subList → cellAddr → cellSpan → cellSz → cellMargin) ──
    #[serde(
        rename(serialize = "hp:subList", deserialize = "subList"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sub_list: Option<HxSubList>,
    #[serde(
        rename(serialize = "hp:cellAddr", deserialize = "cellAddr"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cell_addr: Option<HxCellAddr>,
    #[serde(
        rename(serialize = "hp:cellSpan", deserialize = "cellSpan"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cell_span: Option<HxCellSpan>,
    #[serde(
        rename(serialize = "hp:cellSz", deserialize = "cellSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cell_sz: Option<HxCellSz>,
    #[serde(
        rename(serialize = "hp:cellMargin", deserialize = "cellMargin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cell_margin: Option<HxTableMargin>,
}

/// `<hp:cellAddr colAddr="0" rowAddr="0"/>`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxCellAddr {
    #[serde(rename = "@colAddr", default)]
    pub col_addr: u32,
    #[serde(rename = "@rowAddr", default)]
    pub row_addr: u32,
}

/// `<hp:cellSpan rowSpan="1" colSpan="1"/>`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxCellSpan {
    #[serde(rename = "@rowSpan", default = "default_one")]
    pub row_span: u32,
    #[serde(rename = "@colSpan", default = "default_one")]
    pub col_span: u32,
}

fn default_one() -> u32 {
    1
}

/// `<hp:cellSz width="..." height="..."/>`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxCellSz {
    #[serde(rename = "@width", default)]
    pub width: i32,
    #[serde(rename = "@height", default)]
    pub height: i32,
}

/// `<hp:subList>` — container for paragraphs inside a table cell.
///
/// Includes layout attributes required by 한글 (textDirection, lineWrap, etc.).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxSubList {
    #[serde(rename = "@id", default)]
    pub id: String,
    #[serde(rename = "@textDirection", default)]
    pub text_direction: String,
    #[serde(rename = "@lineWrap", default)]
    pub line_wrap: String,
    #[serde(rename = "@vertAlign", default)]
    pub vert_align: String,
    #[serde(rename = "@linkListIDRef", default)]
    pub link_list_id_ref: u32,
    #[serde(rename = "@linkListNextIDRef", default)]
    pub link_list_next_id_ref: u32,
    #[serde(rename = "@textWidth", default)]
    pub text_width: u32,
    #[serde(rename = "@textHeight", default)]
    pub text_height: u32,
    #[serde(rename = "@hasTextRef", default)]
    pub has_text_ref: u32,
    #[serde(rename = "@hasNumRef", default)]
    pub has_num_ref: u32,

    #[serde(
        rename(serialize = "hp:p", deserialize = "p"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub paragraphs: Vec<HxParagraph>,
}

// ── Picture / Image ───────────────────────────────────────────────

/// `<hp:pic>` — image container.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxPic {
    #[serde(rename = "@id", default)]
    pub id: String,

    #[serde(
        rename(serialize = "hp:img", deserialize = "img"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub img: Option<HxImg>,
    #[serde(
        rename(serialize = "hp:orgSz", deserialize = "orgSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub org_sz: Option<HxSizeAttr>,
    #[serde(
        rename(serialize = "hp:curSz", deserialize = "curSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cur_sz: Option<HxSizeAttr>,
    /// Optional caption attached to this image.
    #[serde(
        rename(serialize = "hp:caption", deserialize = "caption"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub caption: Option<HxCaption>,
    // lineShape, fillBrush, shadow, pos, sz — ignored
}

/// `<hp:img binaryItemIDRef="image1" bright="0" contrast="0" .../>` or
/// `<hc:img binaryItemIDRef="..."/>`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxImg {
    #[serde(rename = "@binaryItemIDRef", default)]
    pub binary_item_id_ref: String,
    #[serde(rename = "@bright", default)]
    pub bright: i32,
    #[serde(rename = "@contrast", default)]
    pub contrast: i32,
}

/// Generic width/height attribute pair used in `<hp:orgSz>`, `<hp:curSz>`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxSizeAttr {
    #[serde(rename = "@width", default)]
    pub width: i32,
    #[serde(rename = "@height", default)]
    pub height: i32,
}

// ── Rectangle / TextBox ──────────────────────────────────────────

/// `<hp:rect>` — rectangle drawing object (can contain textbox content via `<hp:drawText>`).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxRect {
    // ── AbstractShapeObjectType attributes ──
    /// Element ID.
    #[serde(rename = "@id", default)]
    pub id: String,
    /// Z-order for overlapping objects.
    #[serde(rename = "@zOrder", default)]
    pub z_order: u32,
    /// Numbering type: NONE, TABLE, FIGURE, EQUATION.
    #[serde(rename = "@numberingType", default)]
    pub numbering_type: String,
    /// Text wrapping mode: TOP_AND_BOTTOM, SQUARE, TIGHT, etc.
    #[serde(rename = "@textWrap", default)]
    pub text_wrap: String,
    /// Text flow mode: BOTH_SIDES, LEFT_ONLY, RIGHT_ONLY, etc.
    #[serde(rename = "@textFlow", default)]
    pub text_flow: String,
    /// Lock flag (0 = unlocked).
    #[serde(rename = "@lock", default)]
    pub lock: u32,
    /// Drop cap style: None, Normal, etc.
    #[serde(rename = "@dropcapstyle", default)]
    pub dropcap_style: String,

    // ── AbstractShapeComponentType attributes ──
    /// Hyperlink reference.
    #[serde(rename = "@href", default)]
    pub href: String,
    /// Group nesting level.
    #[serde(rename = "@groupLevel", default)]
    pub group_level: u32,
    /// Instance identifier (unique within document).
    #[serde(rename = "@instid", default)]
    pub instid: String,

    // ── Rectangle-specific ──
    /// Corner rounding ratio (0 = sharp, 50 = max rounding).
    #[serde(rename = "@ratio", default)]
    pub ratio: u8,

    // ── Children (ORDER MATTERS for serialization!) ──
    /// Size specification (width, height).
    #[serde(
        rename(serialize = "hp:sz", deserialize = "sz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sz: Option<HxTableSz>,

    /// Position specification.
    #[serde(
        rename(serialize = "hp:pos", deserialize = "pos"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pos: Option<HxTablePos>,

    /// Outer margin.
    #[serde(
        rename(serialize = "hp:outMargin", deserialize = "outMargin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub out_margin: Option<HxTableMargin>,

    /// Optional caption attached to this rectangle.
    #[serde(
        rename(serialize = "hp:caption", deserialize = "caption"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub caption: Option<HxCaption>,

    /// Textbox content (if present, this rect is a textbox).
    #[serde(
        rename(serialize = "hp:drawText", deserialize = "drawText"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub draw_text: Option<HxDrawText>,

    /// Rectangle corner point 0 (top-left).
    #[serde(
        rename(serialize = "hp:pt0", deserialize = "pt0"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pt0: Option<HxPoint>,
    /// Rectangle corner point 1 (top-right).
    #[serde(
        rename(serialize = "hp:pt1", deserialize = "pt1"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pt1: Option<HxPoint>,
    /// Rectangle corner point 2 (bottom-right).
    #[serde(
        rename(serialize = "hp:pt2", deserialize = "pt2"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pt2: Option<HxPoint>,
    /// Rectangle corner point 3 (bottom-left).
    #[serde(
        rename(serialize = "hp:pt3", deserialize = "pt3"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pt3: Option<HxPoint>,
}

/// `<hp:drawText>` — textbox content container (paragraphs + text margin).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxDrawText {
    /// Maximum text width in HWPUNIT (typically width - left_margin - right_margin).
    #[serde(rename = "@lastWidth", default)]
    pub last_width: u32,

    /// Textbox name (usually empty).
    #[serde(rename = "@name", default)]
    pub name: String,

    /// Whether textbox is editable (0 = readonly, 1 = editable).
    #[serde(rename = "@editable", default)]
    pub editable: u32,

    /// Paragraph content (required).
    #[serde(rename(serialize = "hp:subList", deserialize = "subList"))]
    pub sub_list: HxSubList,

    /// Inner text padding (optional, default ~1mm on all sides).
    #[serde(
        rename(serialize = "hp:textMargin", deserialize = "textMargin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub text_margin: Option<HxTableMargin>,
}

/// 2D point for shape geometry (e.g., rectangle corners).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxPoint {
    /// X coordinate (HWPUNIT).
    #[serde(rename = "@x", default)]
    pub x: i32,
    /// Y coordinate (HWPUNIT).
    #[serde(rename = "@y", default)]
    pub y: i32,
}

// ── Line / Ellipse / Polygon shapes ─────────────────────────────

/// `<hp:line>` — line drawing object (2 endpoints).
///
/// Flat struct (independent of HxRect) per Wave 3 API design decision.
/// Common attributes duplicated from AbstractShapeObjectType / AbstractShapeComponentType.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxLine {
    // ── AbstractShapeObjectType attrs ──
    /// Element ID.
    #[serde(rename = "@id", default)]
    pub id: String,
    /// Z-order for overlapping objects.
    #[serde(rename = "@zOrder", default)]
    pub z_order: u32,
    /// Numbering type: NONE, TABLE, FIGURE, EQUATION.
    #[serde(rename = "@numberingType", default)]
    pub numbering_type: String,
    /// Text wrapping mode.
    #[serde(rename = "@textWrap", default)]
    pub text_wrap: String,
    /// Text flow mode.
    #[serde(rename = "@textFlow", default)]
    pub text_flow: String,
    /// Lock flag (0 = unlocked).
    #[serde(rename = "@lock", default)]
    pub lock: u32,
    /// Drop cap style.
    #[serde(rename = "@dropcapstyle", default)]
    pub dropcap_style: String,

    // ── AbstractShapeComponentType attrs ──
    /// Hyperlink reference.
    #[serde(rename = "@href", default)]
    pub href: String,
    /// Group nesting level.
    #[serde(rename = "@groupLevel", default)]
    pub group_level: u32,
    /// Instance identifier.
    #[serde(rename = "@instid", default)]
    pub instid: String,

    // ── Line-specific attr ──
    /// Whether to reverse horizontal/vertical orientation.
    #[serde(rename = "@isReverseHV", default)]
    pub is_reverse_hv: u32,

    // ── Children (ORDER MATTERS!) ──
    /// Size specification.
    #[serde(
        rename(serialize = "hp:sz", deserialize = "sz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sz: Option<HxTableSz>,
    /// Position specification.
    #[serde(
        rename(serialize = "hp:pos", deserialize = "pos"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pos: Option<HxTablePos>,
    /// Outer margin.
    #[serde(
        rename(serialize = "hp:outMargin", deserialize = "outMargin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub out_margin: Option<HxTableMargin>,

    /// Optional caption attached to this line.
    #[serde(
        rename(serialize = "hp:caption", deserialize = "caption"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub caption: Option<HxCaption>,

    // ── Line-specific children ──
    /// Start point of the line.
    #[serde(
        rename(serialize = "hp:startPt", deserialize = "startPt"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub start_pt: Option<HxPoint>,
    /// End point of the line.
    #[serde(
        rename(serialize = "hp:endPt", deserialize = "endPt"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub end_pt: Option<HxPoint>,
}

/// `<hp:ellipse>` — ellipse/circle drawing object.
///
/// Flat struct with common attrs duplicated from AbstractShapeObjectType.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxEllipse {
    // ── AbstractShapeObjectType attrs ──
    /// Element ID.
    #[serde(rename = "@id", default)]
    pub id: String,
    /// Z-order.
    #[serde(rename = "@zOrder", default)]
    pub z_order: u32,
    /// Numbering type.
    #[serde(rename = "@numberingType", default)]
    pub numbering_type: String,
    /// Text wrapping mode.
    #[serde(rename = "@textWrap", default)]
    pub text_wrap: String,
    /// Text flow mode.
    #[serde(rename = "@textFlow", default)]
    pub text_flow: String,
    /// Lock flag.
    #[serde(rename = "@lock", default)]
    pub lock: u32,
    /// Drop cap style.
    #[serde(rename = "@dropcapstyle", default)]
    pub dropcap_style: String,

    // ── AbstractShapeComponentType attrs ──
    /// Hyperlink reference.
    #[serde(rename = "@href", default)]
    pub href: String,
    /// Group nesting level.
    #[serde(rename = "@groupLevel", default)]
    pub group_level: u32,
    /// Instance identifier.
    #[serde(rename = "@instid", default)]
    pub instid: String,

    // ── Ellipse-specific attrs ──
    /// Interval dirty flag.
    #[serde(rename = "@intervalDirty", default)]
    pub interval_dirty: u32,
    /// Whether this ellipse has arc properties.
    #[serde(rename = "@hasArcPr", default)]
    pub has_arc_pr: u32,
    /// Arc type (NORMAL for full ellipse).
    #[serde(rename = "@arcType", default)]
    pub arc_type: String,

    // ── Common children (ORDER MATTERS!) ──
    /// Size specification.
    #[serde(
        rename(serialize = "hp:sz", deserialize = "sz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sz: Option<HxTableSz>,
    /// Position specification.
    #[serde(
        rename(serialize = "hp:pos", deserialize = "pos"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pos: Option<HxTablePos>,
    /// Outer margin.
    #[serde(
        rename(serialize = "hp:outMargin", deserialize = "outMargin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub out_margin: Option<HxTableMargin>,

    /// Optional caption attached to this ellipse.
    #[serde(
        rename(serialize = "hp:caption", deserialize = "caption"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub caption: Option<HxCaption>,

    // ── drawText (textbox content, optional) ──
    /// Optional textbox content inside the ellipse.
    #[serde(
        rename(serialize = "hp:drawText", deserialize = "drawText"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub draw_text: Option<HxDrawText>,

    // ── Ellipse-specific children ──
    /// Center point of the ellipse.
    #[serde(
        rename(serialize = "hp:center", deserialize = "center"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub center: Option<HxPoint>,
    /// Axis 1 endpoint (semi-major axis direction).
    #[serde(
        rename(serialize = "hp:ax1", deserialize = "ax1"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ax1: Option<HxPoint>,
    /// Axis 2 endpoint (semi-minor axis direction).
    #[serde(
        rename(serialize = "hp:ax2", deserialize = "ax2"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ax2: Option<HxPoint>,
}

/// `<hp:polygon>` — polygon drawing object (3+ vertices).
///
/// Flat struct with common attrs duplicated from AbstractShapeObjectType.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxPolygon {
    // ── AbstractShapeObjectType attrs ──
    /// Element ID.
    #[serde(rename = "@id", default)]
    pub id: String,
    /// Z-order.
    #[serde(rename = "@zOrder", default)]
    pub z_order: u32,
    /// Numbering type.
    #[serde(rename = "@numberingType", default)]
    pub numbering_type: String,
    /// Text wrapping mode.
    #[serde(rename = "@textWrap", default)]
    pub text_wrap: String,
    /// Text flow mode.
    #[serde(rename = "@textFlow", default)]
    pub text_flow: String,
    /// Lock flag.
    #[serde(rename = "@lock", default)]
    pub lock: u32,
    /// Drop cap style.
    #[serde(rename = "@dropcapstyle", default)]
    pub dropcap_style: String,

    // ── AbstractShapeComponentType attrs ──
    /// Hyperlink reference.
    #[serde(rename = "@href", default)]
    pub href: String,
    /// Group nesting level.
    #[serde(rename = "@groupLevel", default)]
    pub group_level: u32,
    /// Instance identifier.
    #[serde(rename = "@instid", default)]
    pub instid: String,

    // ── Common children (ORDER MATTERS!) ──
    /// Size specification.
    #[serde(
        rename(serialize = "hp:sz", deserialize = "sz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sz: Option<HxTableSz>,
    /// Position specification.
    #[serde(
        rename(serialize = "hp:pos", deserialize = "pos"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub pos: Option<HxTablePos>,
    /// Outer margin.
    #[serde(
        rename(serialize = "hp:outMargin", deserialize = "outMargin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub out_margin: Option<HxTableMargin>,

    /// Optional caption attached to this polygon.
    #[serde(
        rename(serialize = "hp:caption", deserialize = "caption"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub caption: Option<HxCaption>,

    // ── drawText (textbox content, optional) ──
    /// Optional textbox content inside the polygon.
    #[serde(
        rename(serialize = "hp:drawText", deserialize = "drawText"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub draw_text: Option<HxDrawText>,

    // ── Polygon-specific children ──
    /// Ordered list of polygon vertices.
    #[serde(
        rename(serialize = "hp:pt", deserialize = "pt"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub points: Vec<HxPoint>,
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_section(xml: &str) -> HxSection {
        quick_xml::de::from_str(xml).expect("failed to parse HxSection")
    }

    #[test]
    fn parse_minimal_section() {
        let xml = r#"<hs:sec></hs:sec>"#;
        let sec = parse_section(xml);
        assert!(sec.paragraphs.is_empty());
    }

    #[test]
    fn parse_single_text_paragraph() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="3" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0">
            <hp:run charPrIDRef="0">
              <hp:t>안녕하세요</hp:t>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        assert_eq!(sec.paragraphs.len(), 1);
        let p = &sec.paragraphs[0];
        assert_eq!(p.para_pr_id_ref, 3);
        assert_eq!(p.style_id_ref, 0);
        assert_eq!(p.runs.len(), 1);
        assert_eq!(p.runs[0].char_pr_id_ref, 0);
        assert_eq!(p.runs[0].texts.len(), 1);
        assert_eq!(p.runs[0].texts[0].text, "안녕하세요");
    }

    #[test]
    fn parse_multiple_text_runs() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="3" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:t>Hello</hp:t>
            </hp:run>
            <hp:run charPrIDRef="7">
              <hp:t>World</hp:t>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        let p = &sec.paragraphs[0];
        assert_eq!(p.runs.len(), 2);
        assert_eq!(p.runs[0].texts[0].text, "Hello");
        assert_eq!(p.runs[1].char_pr_id_ref, 7);
        assert_eq!(p.runs[1].texts[0].text, "World");
    }

    #[test]
    fn parse_empty_text_element() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:t/>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        assert_eq!(sec.paragraphs[0].runs[0].texts[0].text, "");
    }

    #[test]
    fn parse_sec_pr_with_page_settings() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="3" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:secPr textDirection="HORIZONTAL">
                <hp:pagePr landscape="WIDELY" width="59528" height="84188" gutterType="LEFT_ONLY">
                  <hp:margin header="4252" footer="4252" gutter="0" left="8504" right="8504" top="5668" bottom="4252"/>
                </hp:pagePr>
              </hp:secPr>
              <hp:t>text</hp:t>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        let run = &sec.paragraphs[0].runs[0];
        let sec_pr = run.sec_pr.as_ref().unwrap();
        let page_pr = sec_pr.page_pr.as_ref().unwrap();
        assert_eq!(page_pr.width, 59528);
        assert_eq!(page_pr.height, 84188);
        assert_eq!(page_pr.landscape, "WIDELY");
        let margin = page_pr.margin.as_ref().unwrap();
        assert_eq!(margin.left, 8504);
        assert_eq!(margin.right, 8504);
        assert_eq!(margin.top, 5668);
        assert_eq!(margin.bottom, 4252);
        assert_eq!(margin.header, 4252);
        assert_eq!(margin.footer, 4252);
    }

    #[test]
    fn parse_table_basic() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:tbl rowCnt="2" colCnt="2">
                <hp:tr>
                  <hp:tc name="A1">
                    <hp:cellSpan rowSpan="1" colSpan="1"/>
                    <hp:cellSz width="1000" height="500"/>
                    <hp:subList>
                      <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
                        <hp:run charPrIDRef="0">
                          <hp:t>Cell 1</hp:t>
                        </hp:run>
                      </hp:p>
                    </hp:subList>
                  </hp:tc>
                  <hp:tc name="B1">
                    <hp:cellSpan rowSpan="1" colSpan="1"/>
                    <hp:cellSz width="1000" height="500"/>
                    <hp:subList>
                      <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
                        <hp:run charPrIDRef="0">
                          <hp:t>Cell 2</hp:t>
                        </hp:run>
                      </hp:p>
                    </hp:subList>
                  </hp:tc>
                </hp:tr>
                <hp:tr>
                  <hp:tc name="A2">
                    <hp:cellSpan rowSpan="1" colSpan="1"/>
                    <hp:subList>
                      <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
                        <hp:run charPrIDRef="0">
                          <hp:t>Cell 3</hp:t>
                        </hp:run>
                      </hp:p>
                    </hp:subList>
                  </hp:tc>
                  <hp:tc name="B2">
                    <hp:cellSpan rowSpan="1" colSpan="1"/>
                    <hp:subList>
                      <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
                        <hp:run charPrIDRef="0">
                          <hp:t>Cell 4</hp:t>
                        </hp:run>
                      </hp:p>
                    </hp:subList>
                  </hp:tc>
                </hp:tr>
              </hp:tbl>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        let tbl = &sec.paragraphs[0].runs[0].tables[0];
        assert_eq!(tbl.row_cnt, 2);
        assert_eq!(tbl.col_cnt, 2);
        assert_eq!(tbl.rows.len(), 2);
        assert_eq!(tbl.rows[0].cells.len(), 2);
        let cell0 = &tbl.rows[0].cells[0];
        assert_eq!(cell0.name, "A1");
        let text = &cell0.sub_list.as_ref().unwrap().paragraphs[0].runs[0].texts[0].text;
        assert_eq!(text, "Cell 1");
    }

    #[test]
    fn parse_picture() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:pic id="123">
                <hp:img binaryItemIDRef="image1.jpg"/>
                <hp:orgSz width="5000" height="3000"/>
              </hp:pic>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        let pic = &sec.paragraphs[0].runs[0].pictures[0];
        let img = pic.img.as_ref().unwrap();
        assert_eq!(img.binary_item_id_ref, "image1.jpg");
        let org = pic.org_sz.as_ref().unwrap();
        assert_eq!(org.width, 5000);
        assert_eq!(org.height, 3000);
    }

    #[test]
    fn unknown_elements_in_run_are_skipped() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:ctrl>
                <hp:colPr id="" type="NEWSPAPER" layout="LEFT" colCount="1"/>
              </hp:ctrl>
              <hp:t>text after ctrl</hp:t>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        let run = &sec.paragraphs[0].runs[0];
        assert_eq!(run.texts[0].text, "text after ctrl");
    }

    #[test]
    fn linesegarray_is_ignored() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:t>text</hp:t>
            </hp:run>
            <hp:linesegarray>
              <hp:lineseg textpos="0" vertpos="0" vertsize="1000"/>
            </hp:linesegarray>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        assert_eq!(sec.paragraphs[0].runs[0].texts[0].text, "text");
    }

    #[test]
    fn multiple_paragraphs() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
            <hp:run charPrIDRef="0"><hp:t>First</hp:t></hp:run>
          </hp:p>
          <hp:p id="1" paraPrIDRef="1" styleIDRef="0">
            <hp:run charPrIDRef="1"><hp:t>Second</hp:t></hp:run>
          </hp:p>
          <hp:p id="2" paraPrIDRef="2" styleIDRef="0">
            <hp:run charPrIDRef="0"><hp:t>Third</hp:t></hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        assert_eq!(sec.paragraphs.len(), 3);
        assert_eq!(sec.paragraphs[0].runs[0].texts[0].text, "First");
        assert_eq!(sec.paragraphs[1].runs[0].texts[0].text, "Second");
        assert_eq!(sec.paragraphs[2].runs[0].texts[0].text, "Third");
    }

    // ── Caption tests ──

    #[test]
    fn parse_caption_standalone_roundtrip() {
        let xml = r#"<caption side="BOTTOM" fullSz="0" width="42520" gap="850" lastWidth="42520"><subList id="" textDirection="" lineWrap="" vertAlign="" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0"><p id="0" paraPrIDRef="0" styleIDRef="0"><run charPrIDRef="0"><t>Figure 1. Sample</t></run></p></subList></caption>"#;
        let cap: HxCaption = quick_xml::de::from_str(xml).expect("parse HxCaption");
        assert_eq!(cap.side, "BOTTOM");
        assert_eq!(cap.full_sz, 0);
        assert_eq!(cap.width, 42520);
        assert_eq!(cap.gap, 850);
        assert_eq!(cap.last_width, 42520);
        assert_eq!(cap.sub_list.paragraphs.len(), 1);
        assert_eq!(cap.sub_list.paragraphs[0].runs[0].texts[0].text, "Figure 1. Sample");

        // Roundtrip: serialize and deserialize
        let serialized = quick_xml::se::to_string(&cap).expect("serialize HxCaption");
        let cap2: HxCaption = quick_xml::de::from_str(&serialized).expect("re-parse HxCaption");
        assert_eq!(cap.side, cap2.side);
        assert_eq!(cap.width, cap2.width);
        assert_eq!(cap.gap, cap2.gap);
    }

    #[test]
    fn caption_defaults() {
        let xml = r#"<caption><subList><p id="0" paraPrIDRef="0" styleIDRef="0"><run charPrIDRef="0"><t>cap</t></run></p></subList></caption>"#;
        let cap: HxCaption = quick_xml::de::from_str(xml).expect("parse");
        assert_eq!(cap.side, "LEFT");
        assert_eq!(cap.gap, 850);
        assert_eq!(cap.full_sz, 0);
        assert_eq!(cap.width, 0);
        assert_eq!(cap.last_width, 0);
    }

    #[test]
    fn parse_table_with_caption() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:tbl rowCnt="1" colCnt="1">
                <hp:sz width="42520" height="5000"/>
                <hp:outMargin left="0" right="0" top="0" bottom="0"/>
                <hp:caption side="BOTTOM" fullSz="0" width="42520" gap="850" lastWidth="42520">
                  <hp:subList id="" textDirection="" lineWrap="" vertAlign="">
                    <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
                      <hp:run charPrIDRef="0"><hp:t>Table 1. Data</hp:t></hp:run>
                    </hp:p>
                  </hp:subList>
                </hp:caption>
                <hp:inMargin left="0" right="0" top="0" bottom="0"/>
                <hp:tr>
                  <hp:tc name="A1">
                    <hp:cellSpan rowSpan="1" colSpan="1"/>
                    <hp:cellSz width="42520" height="5000"/>
                    <hp:subList>
                      <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
                        <hp:run charPrIDRef="0"><hp:t>cell</hp:t></hp:run>
                      </hp:p>
                    </hp:subList>
                  </hp:tc>
                </hp:tr>
              </hp:tbl>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        let tbl = &sec.paragraphs[0].runs[0].tables[0];
        let cap = tbl.caption.as_ref().expect("table should have caption");
        assert_eq!(cap.side, "BOTTOM");
        assert_eq!(cap.width, 42520);
        assert_eq!(cap.sub_list.paragraphs[0].runs[0].texts[0].text, "Table 1. Data");
        // Table data should still parse correctly
        assert_eq!(tbl.rows.len(), 1);
    }

    #[test]
    fn table_without_caption_roundtrip() {
        // Ensure existing tables without caption still work
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:tbl rowCnt="1" colCnt="1">
                <hp:tr>
                  <hp:tc name="A1">
                    <hp:cellSpan rowSpan="1" colSpan="1"/>
                    <hp:subList>
                      <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
                        <hp:run charPrIDRef="0"><hp:t>ok</hp:t></hp:run>
                      </hp:p>
                    </hp:subList>
                  </hp:tc>
                </hp:tr>
              </hp:tbl>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        let tbl = &sec.paragraphs[0].runs[0].tables[0];
        assert!(tbl.caption.is_none());
    }

    #[test]
    fn parse_rect_with_caption() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:rect id="1" zOrder="0" numberingType="FIGURE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0">
                <hp:sz width="20000" height="10000"/>
                <hp:outMargin left="0" right="0" top="0" bottom="0"/>
                <hp:caption side="TOP" width="20000" gap="500" lastWidth="20000">
                  <hp:subList>
                    <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
                      <hp:run charPrIDRef="0"><hp:t>Fig caption</hp:t></hp:run>
                    </hp:p>
                  </hp:subList>
                </hp:caption>
                <hp:drawText lastWidth="18000">
                  <hp:subList>
                    <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
                      <hp:run charPrIDRef="0"><hp:t>box text</hp:t></hp:run>
                    </hp:p>
                  </hp:subList>
                </hp:drawText>
              </hp:rect>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        let rect = &sec.paragraphs[0].runs[0].rects[0];
        let cap = rect.caption.as_ref().expect("rect should have caption");
        assert_eq!(cap.side, "TOP");
        assert_eq!(cap.gap, 500);
        assert!(rect.draw_text.is_some());
    }
}
