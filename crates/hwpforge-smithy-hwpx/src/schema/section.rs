//! XML schema types for `section*.xml` (hp:, hs: namespaces).
//!
//! Maps the `<hs:sec>` element tree into Rust structs via serde.
//! Unknown elements (shapes, controls, line segments) are silently
//! ignored for Phase 3 — we extract text, tables, images only.
//!
//! Fields are used by serde deserialization even if not directly accessed.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

// Re-export shape types so `crate::schema::section::HxRect` etc. still resolve.
// Re-export shape types so `crate::schema::section::HxRect` etc. still resolve.
pub use super::shapes::{
    HxConnectLine, HxConnectPoint, HxControlPoint, HxControlPoints, HxCurve, HxCurveSegment,
    HxDrawText, HxEllipse, HxFillBrush, HxLine, HxLineShape, HxPolygon, HxRect, HxShadow,
    HxShapeComment,
};

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

    /// All `<hp:curve>` elements in this run (bezier/polyline curve objects).
    #[serde(
        rename(serialize = "hp:curve", deserialize = "curve"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub curves: Vec<HxCurve>,

    /// All `<hp:connectLine>` elements in this run (connect line objects).
    #[serde(
        rename(serialize = "hp:connectLine", deserialize = "connectLine"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub connect_lines: Vec<HxConnectLine>,

    /// All `<hp:equation>` elements in this run (inline equations).
    #[serde(
        rename(serialize = "hp:equation", deserialize = "equation"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub equations: Vec<HxEquation>,

    /// All `<hp:switch>` elements in this run (chart feature-gate wrappers).
    #[serde(
        rename(serialize = "hp:switch", deserialize = "switch"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub switches: Vec<HxRunSwitch>,

    /// Optional `<hp:titleMark>` element for TOC participation.
    #[serde(
        rename(serialize = "hp:titleMark", deserialize = "titleMark"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub title_mark: Option<HxTitleMark>,

    /// All `<hp:dutmal>` elements in this run (Korean annotation text).
    #[serde(
        rename(serialize = "hp:dutmal", deserialize = "dutmal"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub dutmals: Vec<HxDutmal>,

    /// All `<hp:compose>` elements in this run (Korean overlaid characters).
    #[serde(
        rename(serialize = "hp:compose", deserialize = "compose"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub composes: Vec<HxCompose>,
}

// ── Text ──────────────────────────────────────────────────────────

/// `<hp:t>수학</hp:t>` or `<hp:t/>` (empty).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxText {
    #[serde(rename = "$text", default)]
    pub text: String,
}

// ── Title mark ────────────────────────────────────────────────────

/// `<hp:titleMark ignore="false"/>` — marks a paragraph for TOC participation.
///
/// When present in a run, 한글 includes the paragraph in its auto-generated
/// Table of Contents. `ignore = false` means "include in TOC".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxTitleMark {
    /// Whether to exclude from TOC (`false` = include, `true` = exclude).
    #[serde(rename = "@ignore")]
    pub ignore: bool,
}

// ── Dutmal ────────────────────────────────────────────────────────

/// `<hp:dutmal posType="TOP" szRatio="0" option="0" styleIDRef="0" align="CENTER">`.
///
/// Represents a Korean 덧말 (annotation text above/below main text).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxDutmal {
    /// Position of annotation relative to main text (e.g. `"TOP"`, `"BOTTOM"`).
    #[serde(rename = "@posType", default)]
    pub pos_type: String,
    /// Size ratio of annotation text (0 = auto).
    #[serde(rename = "@szRatio", default)]
    pub sz_ratio: u32,
    /// Additional option flags (typically 0).
    #[serde(rename = "@option", default)]
    pub option: u32,
    /// Style ID reference (0 = default).
    #[serde(rename = "@styleIDRef", default)]
    pub style_id_ref: u32,
    /// Alignment of annotation text (e.g. `"CENTER"`, `"LEFT"`, `"RIGHT"`).
    #[serde(rename = "@align", default)]
    pub align: String,
    /// The main text that receives the annotation.
    #[serde(rename(serialize = "hp:mainText", deserialize = "mainText"), default)]
    pub main_text: String,
    /// The annotation text displayed above/below.
    #[serde(rename(serialize = "hp:subText", deserialize = "subText"), default)]
    pub sub_text: String,
}

// ── Compose ───────────────────────────────────────────────────────

/// `<hp:compose circleType="..." charSz="-3" composeType="SPREAD" ...>`.
///
/// Represents a Korean 글자겹침 (overlaid/combined characters).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxCompose {
    /// Circle/frame type (e.g. `"SHAPE_REVERSAL_TIRANGLE"` — spec typo preserved).
    #[serde(rename = "@circleType", default)]
    pub circle_type: String,
    /// Character size adjustment (typically -3).
    #[serde(rename = "@charSz", default)]
    pub char_sz: i32,
    /// Composition layout type (e.g. `"SPREAD"`).
    #[serde(rename = "@composeType", default)]
    pub compose_type: String,
    /// Number of character property references (always 10).
    #[serde(rename = "@charPrCnt", default)]
    pub char_pr_cnt: u32,
    /// The combined text content.
    #[serde(rename = "@composeText", default)]
    pub compose_text: String,
    /// 10 charPr references (u32::MAX = no override sentinel).
    #[serde(rename(serialize = "hp:charPr", deserialize = "charPr"), default)]
    pub char_prs: Vec<HxComposeCharPr>,
}

/// `<hp:charPr prIDRef="7"/>` inside compose.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxComposeCharPr {
    /// Property ID reference (u32::MAX = no override).
    #[serde(rename = "@prIDRef")]
    pub pr_id_ref: u32,
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
    /// Optional bookmark element (point bookmark).
    #[serde(
        rename(serialize = "hp:bookmark", deserialize = "bookmark"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub bookmark: Option<HxBookmark>,
    /// Optional index mark element.
    #[serde(
        rename(serialize = "hp:indexmark", deserialize = "indexmark"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub indexmark: Option<HxIndexMark>,
}

/// `<hp:bookmark name="..."/>` — point bookmark element inside `<hp:ctrl>`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxBookmark {
    /// Bookmark name.
    #[serde(rename = "@name")]
    pub name: String,
}

/// `<hp:indexmark>` — index mark element inside `<hp:ctrl>`.
///
/// Contains `<hp:firstKey>` (required) and optionally `<hp:secondKey>`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxIndexMark {
    /// Primary index key (`<hp:firstKey>`).
    #[serde(rename(serialize = "hp:firstKey", deserialize = "firstKey"))]
    pub first_key: String,
    /// Optional secondary index key (`<hp:secondKey>`).
    #[serde(
        rename(serialize = "hp:secondKey", deserialize = "secondKey"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub second_key: Option<String>,
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

    /// Master page count attribute.
    #[serde(rename = "@masterPageCnt", default)]
    pub master_page_cnt: u32,

    /// `<hp:visibility>` — page element visibility flags.
    #[serde(
        rename(serialize = "hp:visibility", deserialize = "visibility"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub visibility: Option<HxVisibility>,

    /// `<hp:lineNumberShape>` — line numbering settings.
    #[serde(
        rename(serialize = "hp:lineNumberShape", deserialize = "lineNumberShape"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub line_number_shape: Option<HxLineNumberShape>,

    #[serde(
        rename(serialize = "hp:pagePr", deserialize = "pagePr"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub page_pr: Option<HxPagePr>,

    /// `<hp:pageBorderFill>` — page border/fill entries (typically 3: BOTH/EVEN/ODD).
    #[serde(
        rename(serialize = "hp:pageBorderFill", deserialize = "pageBorderFill"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub page_border_fills: Vec<HxPageBorderFill>,
    // footNotePr, endNotePr, grid, startNum — still skipped by serde
    // (no deny_unknown_fields). The encoder injects these as raw XML strings
    // via enrich_sec_pr().
}

/// `<hp:visibility>` — controls visibility of page elements.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HxVisibility {
    /// Hide header on first page: 0 or 1.
    #[serde(rename = "@hideFirstHeader", default)]
    pub hide_first_header: u8,
    /// Hide footer on first page: 0 or 1.
    #[serde(rename = "@hideFirstFooter", default)]
    pub hide_first_footer: u8,
    /// Hide master page on first page: 0 or 1.
    #[serde(rename = "@hideFirstMasterPage", default)]
    pub hide_first_master_page: u8,
    /// Border visibility mode (SHOW_ALL, HIDE_ALL, SHOW_ODD, SHOW_EVEN).
    #[serde(rename = "@border", default)]
    pub border: String,
    /// Fill visibility mode (SHOW_ALL, HIDE_ALL, SHOW_ODD, SHOW_EVEN).
    #[serde(rename = "@fill", default)]
    pub fill: String,
    /// Hide page number on first page: 0 or 1.
    #[serde(rename = "@hideFirstPageNum", default)]
    pub hide_first_page_num: u8,
    /// Hide empty line on first page: 0 or 1.
    #[serde(rename = "@hideFirstEmptyLine", default)]
    pub hide_first_empty_line: u8,
    /// Show line numbers: 0 or 1.
    #[serde(rename = "@showLineNumber", default)]
    pub show_line_number: u8,
}

/// `<hp:lineNumberShape>` — line numbering configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HxLineNumberShape {
    /// Restart type: CONTINUOUS, PAGE, SECTION.
    #[serde(rename = "@restartType", default)]
    pub restart_type: String,
    /// Show number every N lines.
    #[serde(rename = "@countBy", default)]
    pub count_by: u16,
    /// Distance from text to line number (HwpUnit).
    #[serde(rename = "@distance", default)]
    pub distance: i32,
    /// Starting line number.
    #[serde(rename = "@startNumber", default)]
    pub start_number: u32,
}

/// `<hp:pageBorderFill>` — a single page border/fill entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxPageBorderFill {
    /// Which pages: BOTH, EVEN, ODD.
    #[serde(rename = "@type", default)]
    pub apply_type: String,
    /// Reference to a borderFill definition (1-based).
    #[serde(rename = "@borderFillIDRef", default)]
    pub border_fill_id: u32,
    /// Border relative to text or paper: PAPER, CONTENT.
    #[serde(rename = "@textBorder", default)]
    pub text_border: String,
    /// Header inside border: 0 or 1.
    #[serde(rename = "@headerInside", default)]
    pub header_inside: u8,
    /// Footer inside border: 0 or 1.
    #[serde(rename = "@footerInside", default)]
    pub footer_inside: u8,
    /// Fill area: PAPER or PAGE.
    #[serde(rename = "@fillArea", default)]
    pub fill_area: String,
    /// Offset from page edge.
    #[serde(
        rename(serialize = "hp:offset", deserialize = "offset"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub offset: Option<HxPageBorderFillOffset>,
}

/// `<hp:offset>` inside `<hp:pageBorderFill>`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HxPageBorderFillOffset {
    /// Left offset in HwpUnit.
    #[serde(rename = "@left", default)]
    pub left: i32,
    /// Right offset in HwpUnit.
    #[serde(rename = "@right", default)]
    pub right: i32,
    /// Top offset in HwpUnit.
    #[serde(rename = "@top", default)]
    pub top: i32,
    /// Bottom offset in HwpUnit.
    #[serde(rename = "@bottom", default)]
    pub bottom: i32,
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

/// `<hp:pic>` — image container with full shape properties.
///
/// Element order matches 한글's expected serialization:
/// offset → orgSz → curSz → flip → rotationInfo → renderingInfo →
/// imgRect → imgClip → inMargin → imgDim → img → sz → pos → outMargin → caption
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxPic {
    // ── AbstractShapeObjectType attrs ──
    /// Element ID.
    #[serde(rename = "@id", default)]
    pub id: String,
    /// Z-order for overlapping objects.
    #[serde(rename = "@zOrder", default)]
    pub z_order: u32,
    /// Numbering type: NONE, PICTURE, TABLE, EQUATION.
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
    /// Instance identifier (unique within document).
    #[serde(rename = "@instid", default)]
    pub instid: String,
    /// Reverse flag.
    #[serde(rename = "@reverse", default)]
    pub reverse: u32,

    // ── Children (ORDER MATTERS for serialization!) ──
    /// Position offset.
    #[serde(
        rename(serialize = "hp:offset", deserialize = "offset"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub offset: Option<HxOffset>,
    /// Original image size (before scaling).
    #[serde(
        rename(serialize = "hp:orgSz", deserialize = "orgSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub org_sz: Option<HxSizeAttr>,
    /// Current display size.
    #[serde(
        rename(serialize = "hp:curSz", deserialize = "curSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cur_sz: Option<HxSizeAttr>,
    /// Flip state.
    #[serde(
        rename(serialize = "hp:flip", deserialize = "flip"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub flip: Option<HxFlip>,
    /// Rotation information.
    #[serde(
        rename(serialize = "hp:rotationInfo", deserialize = "rotationInfo"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rotation_info: Option<HxRotationInfo>,
    /// Rendering transformation matrices.
    #[serde(
        rename(serialize = "hp:renderingInfo", deserialize = "renderingInfo"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rendering_info: Option<HxRenderingInfo>,
    /// Image bounding rectangle (4 corner points).
    #[serde(
        rename(serialize = "hp:imgRect", deserialize = "imgRect"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub img_rect: Option<HxImgRect>,
    /// Image clipping region.
    #[serde(
        rename(serialize = "hp:imgClip", deserialize = "imgClip"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub img_clip: Option<HxImgClip>,
    /// Inner margin.
    #[serde(
        rename(serialize = "hp:inMargin", deserialize = "inMargin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub in_margin: Option<HxTableMargin>,
    /// Image pixel dimensions.
    #[serde(
        rename(serialize = "hp:imgDim", deserialize = "imgDim"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub img_dim: Option<HxImgDim>,
    /// Image binary reference (uses `hc:` core namespace).
    #[serde(
        rename(serialize = "hc:img", deserialize = "img"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub img: Option<HxImg>,
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
    /// Optional caption attached to this image.
    #[serde(
        rename(serialize = "hp:caption", deserialize = "caption"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub caption: Option<HxCaption>,
}

/// `<hc:img binaryItemIDRef="image1" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/>`.
/// Uses `hc:` (core namespace) per HWPX spec.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HxImg {
    #[serde(rename = "@binaryItemIDRef", default)]
    pub binary_item_id_ref: String,
    #[serde(rename = "@bright", default)]
    pub bright: i32,
    #[serde(rename = "@contrast", default)]
    pub contrast: i32,
    /// Image effect type: REAL_PIC (original), etc.
    #[serde(rename = "@effect", default, skip_serializing_if = "String::is_empty")]
    pub effect: String,
    /// Alpha transparency (0 = opaque).
    #[serde(rename = "@alpha", default, skip_serializing_if = "String::is_empty")]
    pub alpha: String,
}

/// Generic width/height attribute pair used in `<hp:orgSz>`, `<hp:curSz>`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HxSizeAttr {
    #[serde(rename = "@width", default)]
    pub width: i32,
    #[serde(rename = "@height", default)]
    pub height: i32,
}

// ── Picture-specific sub-elements ────────────────────────────────

/// `<hp:offset x="0" y="0"/>` — position offset for shapes.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxOffset {
    #[serde(rename = "@x", default)]
    pub x: i32,
    #[serde(rename = "@y", default)]
    pub y: i32,
}

/// `<hp:flip horizontal="0" vertical="0"/>` — flip state.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxFlip {
    #[serde(rename = "@horizontal", default)]
    pub horizontal: u32,
    #[serde(rename = "@vertical", default)]
    pub vertical: u32,
}

/// `<hp:rotationInfo angle="0" centerX="..." centerY="..." rotateimage="1"/>`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxRotationInfo {
    #[serde(rename = "@angle", default)]
    pub angle: i32,
    #[serde(rename = "@centerX", default)]
    pub center_x: i32,
    #[serde(rename = "@centerY", default)]
    pub center_y: i32,
    #[serde(rename = "@rotateimage", default)]
    pub rotate_image: u32,
}

/// 2D affine transformation matrix (6 elements: e1-e6).
/// Used in `<hc:transMatrix>`, `<hc:scaMatrix>`, `<hc:rotMatrix>`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxMatrix {
    #[serde(rename = "@e1", default)]
    pub e1: String,
    #[serde(rename = "@e2", default)]
    pub e2: String,
    #[serde(rename = "@e3", default)]
    pub e3: String,
    #[serde(rename = "@e4", default)]
    pub e4: String,
    #[serde(rename = "@e5", default)]
    pub e5: String,
    #[serde(rename = "@e6", default)]
    pub e6: String,
}

impl HxMatrix {
    /// Creates an identity transformation matrix.
    pub fn identity() -> Self {
        Self {
            e1: "1".to_string(),
            e2: "0".to_string(),
            e3: "0".to_string(),
            e4: "0".to_string(),
            e5: "1".to_string(),
            e6: "0".to_string(),
        }
    }
}

impl Default for HxMatrix {
    fn default() -> Self {
        Self::identity()
    }
}

/// `<hp:renderingInfo>` — transformation matrices for rendering.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HxRenderingInfo {
    /// Translation matrix.
    #[serde(rename(serialize = "hc:transMatrix", deserialize = "transMatrix"))]
    pub trans_matrix: HxMatrix,
    /// Scale matrix.
    #[serde(rename(serialize = "hc:scaMatrix", deserialize = "scaMatrix"))]
    pub sca_matrix: HxMatrix,
    /// Rotation matrix.
    #[serde(rename(serialize = "hc:rotMatrix", deserialize = "rotMatrix"))]
    pub rot_matrix: HxMatrix,
}

// Shape-common types (HxLineShape, HxWinBrush, HxFillBrush, HxShadow, HxShapeComment)
// are defined in `super::shapes` and re-exported via `schema/mod.rs`.

// ── Equation ─────────────────────────────────────────────────────

/// `<hp:equation>` — inline equation (수식) with HancomEQN script.
///
/// Unlike other drawing objects, equations have NO shape common block
/// (no offset, orgSz, curSz, flip, rotation, lineShape, fillBrush, shadow).
/// Only sz + pos + outMargin + shapeComment + script.
///
/// Element order matches 한글's expected serialization:
/// attrs → sz → pos → outMargin → shapeComment → script
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxEquation {
    // ── AbstractShapeObjectType attrs ──
    /// Element ID.
    #[serde(rename = "@id", default)]
    pub id: String,
    /// Z-order for overlapping objects.
    #[serde(rename = "@zOrder", default)]
    pub z_order: u32,
    /// Numbering type: NONE, EQUATION, etc.
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

    // ── Equation-specific attrs ──
    /// Equation version string (e.g. "Equation Version 60").
    #[serde(rename = "@version", default)]
    pub version: String,
    /// Baseline position (51-90 typical range).
    #[serde(rename = "@baseLine", default)]
    pub base_line: u32,
    /// Text color as `#RRGGBB`.
    #[serde(rename = "@textColor", default)]
    pub text_color: String,
    /// Base unit for equation rendering (typically 1000).
    #[serde(rename = "@baseUnit", default)]
    pub base_unit: u32,
    /// Line mode: CHAR (inline).
    #[serde(rename = "@lineMode", default)]
    pub line_mode: String,
    /// Font name (typically "HancomEQN").
    #[serde(rename = "@font", default)]
    pub font: String,

    // ── Children (ORDER MATTERS) ──
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
    /// Shape comment (typically "수식입니다.").
    #[serde(
        rename(serialize = "hp:shapeComment", deserialize = "shapeComment"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shape_comment: Option<HxShapeComment>,
    /// Equation script content (HancomEQN format).
    #[serde(
        rename(serialize = "hp:script", deserialize = "script"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub script: Option<HxScript>,
}

/// `<hp:script>` — equation script text content.
///
/// Uses `$text` to capture the raw text content. Serde handles XML entity
/// escaping automatically (`&` → `&amp;`, `<` → `&lt;`).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxScript {
    /// The HancomEQN script text.
    #[serde(rename = "$text", default)]
    pub text: String,
}

// ── Chart (switch/case wrapper) ──────────────────────────────────

/// `<hp:switch>` — chart feature-gate wrapper within a run.
///
/// Charts use `<hp:switch><hp:case required-namespace="..."><hp:chart .../></hp:case></hp:switch>`.
/// The `<hp:default>` child (OLE fallback) is silently skipped.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxRunSwitch {
    /// `<hp:case>` — conditional content (contains chart).
    #[serde(
        rename(serialize = "hp:case", deserialize = "case"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub case: Option<HxRunCase>,
}

/// `<hp:case>` — conditional content block requiring a namespace.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxRunCase {
    /// Required namespace URI for this case to activate.
    #[serde(rename = "@hp:required-namespace", default)]
    pub required_namespace: String,

    /// Optional chart element.
    #[serde(
        rename(serialize = "hp:chart", deserialize = "chart"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub chart: Option<HxChart>,
}

/// `<hp:chart>` — chart reference element (section-level). NO shape common block.
///
/// Only has sz + pos + outMargin (like Equation but with chartIDRef).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxChart {
    // ── Attributes ──
    /// Element ID.
    #[serde(rename = "@id", default)]
    pub id: String,
    /// Z-order for overlapping objects.
    #[serde(rename = "@zOrder", default)]
    pub z_order: u32,
    /// Numbering type (typically "PICTURE" for charts).
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
    /// Drop cap style (typically "None" for charts).
    #[serde(rename = "@dropcapstyle", default)]
    pub dropcap_style: String,
    /// Reference to the chart XML file within the ZIP (e.g. "Chart/chart1.xml").
    #[serde(rename = "@chartIDRef", default)]
    pub chart_id_ref: String,

    // ── Children (ORDER MATTERS) ──
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
}

/// `<hp:imgRect>` — image bounding rectangle (4 corner points, uses `hc:` namespace).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HxImgRect {
    #[serde(rename(serialize = "hc:pt0", deserialize = "pt0"))]
    pub pt0: HxPoint,
    #[serde(rename(serialize = "hc:pt1", deserialize = "pt1"))]
    pub pt1: HxPoint,
    #[serde(rename(serialize = "hc:pt2", deserialize = "pt2"))]
    pub pt2: HxPoint,
    #[serde(rename(serialize = "hc:pt3", deserialize = "pt3"))]
    pub pt3: HxPoint,
}

/// `<hp:imgClip left="0" right="..." top="0" bottom="..."/>` — image clipping region.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxImgClip {
    #[serde(rename = "@left", default)]
    pub left: i32,
    #[serde(rename = "@right", default)]
    pub right: i32,
    #[serde(rename = "@top", default)]
    pub top: i32,
    #[serde(rename = "@bottom", default)]
    pub bottom: i32,
}

/// `<hp:imgDim dimwidth="..." dimheight="..."/>` — original pixel dimensions.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxImgDim {
    #[serde(rename = "@dimwidth", default)]
    pub dim_width: i32,
    #[serde(rename = "@dimheight", default)]
    pub dim_height: i32,
}

// Shape types (HxRect, HxDrawText) are defined in `super::shapes`.

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

// Shape types (HxLine, HxEllipse, HxPolygon) are defined in `super::shapes`.

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
