//! XML schema types for `section*.xml` (hp:, hs: namespaces).
//!
//! Maps the `<hs:sec>` element tree into Rust structs via serde.
//! Unknown elements (shapes, controls, line segments) are silently
//! ignored for Phase 3 — we extract text, tables, images only.
//!
//! Fields are used by serde deserialization even if not directly accessed.
#![allow(dead_code)]

use hwpforge_core::inline::{InlineSegment, InlineTabAttr, InlineText};
use hwpforge_core::run::RunContent;
use hwpforge_foundation::HwpUnit;
use serde::{Deserialize, Serialize};

use super::deser_i32_or_u32;

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

fn default_table_page_break() -> String {
    "CELL".to_string()
}

fn default_table_repeat_header() -> u32 {
    1
}

// ── Paragraph ─────────────────────────────────────────────────────

/// `<hp:p id="..." paraPrIDRef="3" styleIDRef="0" ...>`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
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
/// W1a (이미지/글상자 에픽): 역직렬화는 수동 구현 — `$value` 혼합 파싱으로
/// 종류별 Vec 을 채우면서 **문서 순서 사이드카** [`HxRun::child_order`] 를
/// 함께 기록한다 (한컴 인터리브 `<t>a</t><pic/><t>b</t>` 의 순서 보존 —
/// 스파이크 `tests/hxrun_read_spike.rs` 로 quick-xml 0.41 지원 확증).
/// 직렬화는 derive 유지 (인코더는 1 Core Run → 1 HxRun 정규형 — mixed
/// 직렬화 불요, Codex 리뷰 #2 채택).
#[derive(Debug, Clone, Serialize, PartialEq)]
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

    /// All `<hp:container>` elements in this run (group / 묶음 객체).
    ///
    /// Serialization is NOT driven by this field — the encoder builds the
    /// container fragment as raw XML (heterogeneous, z-ordered children that
    /// serde cannot express) and injects it via marker substitution. This
    /// field exists for the decode side so a round-tripped `<hp:container>`
    /// deserializes back into a `Control::Group`.
    #[serde(rename(deserialize = "container"), default, skip_serializing)]
    pub containers: Vec<HxContainer>,

    /// All `<hp:textart>` elements in this run (글맵시 / TextArt objects).
    ///
    /// Like `containers`, serialization is NOT driven by this field — the
    /// encoder builds the `<hp:textart>` fragment as raw XML (derived
    /// `scaMatrix`, fixed corner-point block that serde cannot express) and
    /// injects it via marker substitution. This field exists for the decode
    /// side so a round-tripped `<hp:textart>` deserializes back into a
    /// `Control::TextArt`.
    #[serde(rename(deserialize = "textart"), default, skip_serializing)]
    pub textarts: Vec<HxTextArt>,

    /// 자식의 **문서 순서** 사이드카: `(종류, 해당 종류 Vec 내 인덱스)` 를
    /// 파싱 순서대로 기록한다 (W1a). 종류별 Vec 은 기존 소비자 호환을 위해
    /// 유지되고, 순서가 필요한 소비자(디코더 평탄화·patch raw walker)만
    /// 이 사이드카를 걷는다. 인코더 생성 run 은 빈 Vec (직렬화 제외).
    #[serde(skip)]
    pub child_order: Vec<(HxRunChildKind, usize)>,
}

/// [`HxRun::child_order`] 의 자식 종류 태그 (W1a).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HxRunChildKind {
    /// `<hp:secPr>` (Option 필드 — 인덱스는 항상 0).
    SecPr,
    /// `<hp:t>`.
    Text,
    /// `<hp:tbl>`.
    Table,
    /// `<hp:pic>`.
    Picture,
    /// `<hp:ctrl>`.
    Ctrl,
    /// `<hp:rect>`.
    Rect,
    /// `<hp:line>`.
    Line,
    /// `<hp:ellipse>`.
    Ellipse,
    /// `<hp:polygon>`.
    Polygon,
    /// `<hp:curve>`.
    Curve,
    /// `<hp:connectLine>`.
    ConnectLine,
    /// `<hp:equation>`.
    Equation,
    /// `<hp:switch>`.
    Switch,
    /// `<hp:titleMark>` (Option 필드 — 인덱스는 항상 0).
    TitleMark,
    /// `<hp:dutmal>`.
    Dutmal,
    /// `<hp:compose>`.
    Compose,
    /// `<hp:container>`.
    Container,
    /// `<hp:textart>`.
    TextArt,
    /// 미지 요소 (자리 보존용 — 종류별 Vec 에는 미수록).
    Other,
}

/// 종전 by-kind 고정 순서를 재현하는 폴백 (child_order 사이드카가 비어 있는
/// 수동 생성 `HxRun` 전용 — 파싱 경로는 항상 사이드카를 갖는다).
pub(crate) fn legacy_child_order(hx: &HxRun) -> Vec<(HxRunChildKind, usize)> {
    use HxRunChildKind as K;
    let mut order = Vec::new();
    order.extend((0..hx.texts.len()).map(|i| (K::Text, i)));
    order.extend((0..hx.tables.len()).map(|i| (K::Table, i)));
    order.extend((0..hx.pictures.len()).map(|i| (K::Picture, i)));
    order.extend((0..hx.ctrls.len()).map(|i| (K::Ctrl, i)));
    order.extend((0..hx.rects.len()).map(|i| (K::Rect, i)));
    order.extend((0..hx.lines.len()).map(|i| (K::Line, i)));
    order.extend((0..hx.ellipses.len()).map(|i| (K::Ellipse, i)));
    order.extend((0..hx.polygons.len()).map(|i| (K::Polygon, i)));
    order.extend((0..hx.curves.len()).map(|i| (K::Curve, i)));
    order.extend((0..hx.textarts.len()).map(|i| (K::TextArt, i)));
    order.extend((0..hx.connect_lines.len()).map(|i| (K::ConnectLine, i)));
    order.extend((0..hx.containers.len()).map(|i| (K::Container, i)));
    order.extend((0..hx.equations.len()).map(|i| (K::Equation, i)));
    order.extend((0..hx.dutmals.len()).map(|i| (K::Dutmal, i)));
    order.extend((0..hx.composes.len()).map(|i| (K::Compose, i)));
    order
}

/// `$value` 혼합 파싱용 중간 enum (W1a — 역직렬화 전용).
#[derive(Deserialize)]
enum HxRunChildDe {
    #[serde(rename = "secPr")]
    SecPr(Box<HxSecPr>),
    #[serde(rename = "t")]
    Text(HxText),
    #[serde(rename = "tbl")]
    Table(Box<HxTable>),
    #[serde(rename = "pic")]
    Picture(Box<HxPic>),
    #[serde(rename = "ctrl")]
    Ctrl(Box<HxCtrl>),
    #[serde(rename = "rect")]
    Rect(Box<HxRect>),
    #[serde(rename = "line")]
    Line(Box<HxLine>),
    #[serde(rename = "ellipse")]
    Ellipse(Box<HxEllipse>),
    #[serde(rename = "polygon")]
    Polygon(Box<HxPolygon>),
    #[serde(rename = "curve")]
    Curve(Box<HxCurve>),
    #[serde(rename = "connectLine")]
    ConnectLine(Box<HxConnectLine>),
    #[serde(rename = "equation")]
    Equation(Box<HxEquation>),
    #[serde(rename = "switch")]
    Switch(Box<HxRunSwitch>),
    #[serde(rename = "titleMark")]
    TitleMark(Box<HxTitleMark>),
    #[serde(rename = "dutmal")]
    Dutmal(Box<HxDutmal>),
    #[serde(rename = "compose")]
    Compose(Box<HxCompose>),
    #[serde(rename = "container")]
    Container(Box<HxContainer>),
    #[serde(rename = "textart")]
    TextArt(Box<HxTextArt>),
    /// run 직속 텍스트 노드 (정상 문서엔 없음 — 방어적 흡수).
    #[serde(rename = "$text")]
    StrayText(String),
    /// 미지 요소 — 파싱 실패 대신 자리만 기록.
    #[serde(other)]
    Other,
}

impl<'de> serde::Deserialize<'de> for HxRun {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct HxRunDe {
            #[serde(rename = "@charPrIDRef", default)]
            char_pr_id_ref: u32,
            #[serde(rename = "$value", default)]
            children: Vec<HxRunChildDe>,
        }
        let de = HxRunDe::deserialize(deserializer)?;
        let mut run = HxRun {
            char_pr_id_ref: de.char_pr_id_ref,
            sec_pr: None,
            texts: Vec::new(),
            tables: Vec::new(),
            pictures: Vec::new(),
            ctrls: Vec::new(),
            rects: Vec::new(),
            lines: Vec::new(),
            ellipses: Vec::new(),
            polygons: Vec::new(),
            curves: Vec::new(),
            connect_lines: Vec::new(),
            equations: Vec::new(),
            switches: Vec::new(),
            title_mark: None,
            dutmals: Vec::new(),
            composes: Vec::new(),
            containers: Vec::new(),
            textarts: Vec::new(),
            child_order: Vec::new(),
        };
        use HxRunChildKind as K;
        for child in de.children {
            match child {
                HxRunChildDe::SecPr(x) => {
                    run.child_order.push((K::SecPr, 0));
                    run.sec_pr = Some(*x);
                }
                HxRunChildDe::Text(x) => {
                    run.child_order.push((K::Text, run.texts.len()));
                    run.texts.push(x);
                }
                HxRunChildDe::Table(x) => {
                    run.child_order.push((K::Table, run.tables.len()));
                    run.tables.push(*x);
                }
                HxRunChildDe::Picture(x) => {
                    run.child_order.push((K::Picture, run.pictures.len()));
                    run.pictures.push(*x);
                }
                HxRunChildDe::Ctrl(x) => {
                    run.child_order.push((K::Ctrl, run.ctrls.len()));
                    run.ctrls.push(*x);
                }
                HxRunChildDe::Rect(x) => {
                    run.child_order.push((K::Rect, run.rects.len()));
                    run.rects.push(*x);
                }
                HxRunChildDe::Line(x) => {
                    run.child_order.push((K::Line, run.lines.len()));
                    run.lines.push(*x);
                }
                HxRunChildDe::Ellipse(x) => {
                    run.child_order.push((K::Ellipse, run.ellipses.len()));
                    run.ellipses.push(*x);
                }
                HxRunChildDe::Polygon(x) => {
                    run.child_order.push((K::Polygon, run.polygons.len()));
                    run.polygons.push(*x);
                }
                HxRunChildDe::Curve(x) => {
                    run.child_order.push((K::Curve, run.curves.len()));
                    run.curves.push(*x);
                }
                HxRunChildDe::ConnectLine(x) => {
                    run.child_order.push((K::ConnectLine, run.connect_lines.len()));
                    run.connect_lines.push(*x);
                }
                HxRunChildDe::Equation(x) => {
                    run.child_order.push((K::Equation, run.equations.len()));
                    run.equations.push(*x);
                }
                HxRunChildDe::Switch(x) => {
                    run.child_order.push((K::Switch, run.switches.len()));
                    run.switches.push(*x);
                }
                HxRunChildDe::TitleMark(x) => {
                    run.child_order.push((K::TitleMark, 0));
                    run.title_mark = Some(*x);
                }
                HxRunChildDe::Dutmal(x) => {
                    run.child_order.push((K::Dutmal, run.dutmals.len()));
                    run.dutmals.push(*x);
                }
                HxRunChildDe::Compose(x) => {
                    run.child_order.push((K::Compose, run.composes.len()));
                    run.composes.push(*x);
                }
                HxRunChildDe::Container(x) => {
                    run.child_order.push((K::Container, run.containers.len()));
                    run.containers.push(*x);
                }
                HxRunChildDe::TextArt(x) => {
                    run.child_order.push((K::TextArt, run.textarts.len()));
                    run.textarts.push(*x);
                }
                HxRunChildDe::StrayText(_) => {
                    // run 직속 텍스트는 스키마상 없음 — 자리 기록 없이 무시.
                }
                HxRunChildDe::Other => {
                    run.child_order.push((K::Other, 0));
                }
            }
        }
        Ok(run)
    }
}

/// `<hp:textart>` — TextArt (글맵시) decorative warped-text object.
///
/// Decode-only mirror of [`crate::encoder::shapes::encode_text_art_to_xml`].
/// Captures only the attributes needed to reconstruct `Control::TextArt`:
/// the displayed `text`, `instid`, placement (`offset`), size (`sz`), and the
/// `<hp:textartPr>` typography sub-element.
#[derive(Debug, Default, Clone, Deserialize, PartialEq)]
pub struct HxTextArt {
    /// Displayed text content.
    #[serde(rename = "@text", default)]
    pub text: String,
    /// Instance identifier (mirrors HWP5 / Core `inst_id`).
    #[serde(rename = "@instid", default)]
    pub instid: String,
    /// Placement offset (`<hp:offset>`) — carries the anchor x/y.
    #[serde(rename(deserialize = "offset"), default)]
    pub offset: Option<HxOffset>,
    /// Display size (`<hp:sz>`) — carries width/height.
    #[serde(rename(deserialize = "sz"), default)]
    pub sz: Option<HxTableSz>,
    /// Typography properties (`<hp:textartPr>`).
    #[serde(rename(deserialize = "textartPr"), default)]
    pub textart_pr: Option<HxTextArtPr>,
}

/// `<hp:textartPr>` — TextArt typography (font / shape / spacing / align).
#[derive(Debug, Default, Clone, Deserialize, PartialEq)]
pub struct HxTextArtPr {
    /// Font family name.
    #[serde(rename = "@fontName", default)]
    pub font_name: String,
    /// Font style label.
    #[serde(rename = "@fontStyle", default)]
    pub font_style: String,
    /// HWPX `textShape` name (e.g. `"WAVE2"`).
    #[serde(rename = "@textShape", default)]
    pub text_shape: String,
    /// Line spacing (percent).
    #[serde(rename = "@lineSpacing", default, deserialize_with = "deser_i32_or_u32")]
    pub line_spacing: i32,
    /// Character spacing (percent).
    #[serde(rename = "@charSpacing", default, deserialize_with = "deser_i32_or_u32")]
    pub char_spacing: i32,
    /// Text alignment (e.g. `"LEFT"`).
    #[serde(rename = "@align", default)]
    pub align: String,
}

/// `<hp:container>` — group (묶음 객체 / 개체 묶기) wrapping child shapes.
///
/// Decodes flat children (rect/ellipse/line/polygon/curve/connectLine) plus
/// nested `<hp:container>` children (Wave B). The shape-common block
/// (offset/orgSz/…) is parsed for geometry; children reuse the per-shape
/// decoders, and nested containers recurse through `decode_container`.
#[derive(Debug, Default, Clone, Deserialize, PartialEq)]
pub struct HxContainer {
    /// Group nesting level (`0` = outermost).
    #[serde(rename = "@groupLevel", default)]
    pub group_level: u32,
    /// Instance identifier (mirrors HWP5 / Core `inst_id`).
    #[serde(rename = "@instid", default)]
    pub instid: String,

    /// Group bounding-box original size (`<hp:orgSz>`).
    #[serde(rename(deserialize = "orgSz"), default)]
    pub org_sz: Option<HxSizeAttr>,
    /// Group placement (`<hp:pos>`) — carries the anchor offsets.
    #[serde(rename(deserialize = "pos"), default)]
    pub pos: Option<HxTablePos>,
    /// Group display size (`<hp:sz>`).
    #[serde(rename(deserialize = "sz"), default)]
    pub sz: Option<HxTableSz>,

    // ── Children (flat shapes for Wave A) ──
    /// `<hp:rect>` children (textboxes / pure rects).
    #[serde(rename(deserialize = "rect"), default)]
    pub rects: Vec<HxRect>,
    /// `<hp:line>` children.
    #[serde(rename(deserialize = "line"), default)]
    pub lines: Vec<HxLine>,
    /// `<hp:ellipse>` children (ellipse / arc).
    #[serde(rename(deserialize = "ellipse"), default)]
    pub ellipses: Vec<HxEllipse>,
    /// `<hp:polygon>` children.
    #[serde(rename(deserialize = "polygon"), default)]
    pub polygons: Vec<HxPolygon>,
    /// `<hp:curve>` children.
    #[serde(rename(deserialize = "curve"), default)]
    pub curves: Vec<HxCurve>,
    /// `<hp:connectLine>` children.
    #[serde(rename(deserialize = "connectLine"), default)]
    pub connect_lines: Vec<HxConnectLine>,
    /// Nested `<hp:container>` children (group-in-group, Wave B). `Vec` is
    /// heap-indirected so the recursive type needs no explicit `Box`.
    #[serde(rename(deserialize = "container"), default)]
    pub containers: Vec<HxContainer>,
}

// ── Text ──────────────────────────────────────────────────────────

/// `<hp:t>수학</hp:t>` or `<hp:t/>` (empty).
///
/// Supports mixed content: `<hp:t>text<hp:lineBreak/>more</hp:t>`.
/// Use [`HxText::text()`] to get the combined text with `\n` for line breaks.
///
/// Deserialization uses `$value` to capture mixed text + element content.
/// Serialization outputs plain concatenated text (line breaks become `\n`),
/// which is fine because the encoder builds XML manually.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct HxText {
    /// Mixed content parts (text nodes and line breaks).
    #[serde(rename = "$value", default)]
    pub parts: Vec<HxTextPart>,
}

/// Serializes `HxText` as simple text content (`$text`), avoiding the
/// quick-xml limitation where `$value` with mixed enum content fails.
impl Serialize for HxText {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Helper {
            #[serde(rename = "$text")]
            text: String,
        }
        Helper { text: self.text() }.serialize(serializer)
    }
}

impl HxText {
    /// Creates a new `HxText` from a plain string.
    pub fn new(text: impl Into<String>) -> Self {
        let s = text.into();
        if s.is_empty() {
            Self { parts: Vec::new() }
        } else {
            Self { parts: vec![HxTextPart::Text(s)] }
        }
    }

    /// Returns the combined text content, with `\n` for line breaks.
    ///
    /// Inline markup elements that Core carries through `RunContent::Text`
    /// are projected onto sentinel code-points so the round-trip through
    /// the encoder (`inline_text::encode_inline_text_xml`) is lossless:
    ///
    /// - `<hp:lineBreak/>` → `\n`
    /// - `<hp:tab/>` → `\t`
    /// - `<hp:nbSpace/>` → `U+00A0`
    /// - `<hp:fwSpace/>` → `U+001F` (mirrors the HWP5 wire control byte)
    pub fn text(&self) -> String {
        self.parts
            .iter()
            .map(|p| match p {
                HxTextPart::Text(s) => s.as_str(),
                HxTextPart::LineBreak {} => "\n",
                HxTextPart::Tab { .. } => "\t",
                HxTextPart::FwSpace {} => "\u{001F}",
                HxTextPart::NbSpace {} => "\u{00a0}",
                HxTextPart::MarkpenBegin {} | HxTextPart::MarkpenEnd {} => "",
                HxTextPart::Other => "",
            })
            .collect()
    }

    /// Returns a `RunContent` that preserves inline tab attributes
    /// (`width` / `leader` / `tab_type`) when present.
    ///
    /// Falls back to `RunContent::Text(String)` — preserving the
    /// existing surface for the 18+ consumer sites that match
    /// `RunContent::Text` directly — when every `<hp:tab>` in the
    /// part list either:
    ///
    /// - has no attributes (`<hp:tab/>`), or
    /// - has all-zero attribute values (semantically equivalent to a
    ///   bare tab).
    ///
    /// Otherwise returns `RunContent::InlineText(InlineText)` with one
    /// `InlineSegment::Tab(InlineTabAttr { .. })` per attribute-rich
    /// tab and `InlineSegment::Plain` for everything else. Other
    /// inline parts (`<hp:lineBreak/>`, `<hp:nbSpace/>`,
    /// `<hp:fwSpace/>`) flatten into `Plain` with the same sentinel
    /// characters `text()` uses, so the HWPX encoder restores them
    /// via `encode_inline_text_xml` without loss.
    ///
    /// See `.docs/debug/2026-05-27_hwpx_decoder_inline_tab_attrs_lost.md`
    /// for the algorithm and side-effect analysis.
    pub fn to_run_content(&self) -> RunContent {
        let needs_inline = self.parts.iter().any(|p| {
            matches!(
                p,
                HxTextPart::Tab { width, leader, tab_type }
                if width.unwrap_or(0) != 0
                    || leader.unwrap_or(0) != 0
                    || tab_type.unwrap_or(0) != 0
            )
        });
        if !needs_inline {
            return RunContent::Text(self.text());
        }

        let segments = self.parts.iter().filter_map(|p| match p {
            HxTextPart::Text(s) if s.is_empty() => None,
            HxTextPart::Text(s) => Some(InlineSegment::Plain(s.clone())),
            HxTextPart::Tab { width, leader, tab_type } => {
                Some(InlineSegment::Tab(InlineTabAttr {
                    width: HwpUnit::new(width.unwrap_or(0)).unwrap_or(HwpUnit::ZERO),
                    leader: leader.unwrap_or(0),
                    tab_type: tab_type.unwrap_or(0),
                }))
            }
            HxTextPart::LineBreak {} => Some(InlineSegment::Plain("\n".into())),
            HxTextPart::FwSpace {} => Some(InlineSegment::Plain("\u{001F}".into())),
            HxTextPart::NbSpace {} => Some(InlineSegment::Plain("\u{00A0}".into())),
            HxTextPart::MarkpenBegin {} | HxTextPart::MarkpenEnd {} | HxTextPart::Other => None,
        });

        RunContent::InlineText(InlineText::from_segments(segments))
    }
}

/// A part of mixed text content inside `<hp:t>`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub enum HxTextPart {
    /// Plain text content.
    #[serde(rename = "$text")]
    Text(String),
    /// `<hp:lineBreak/>` — line break within a text run.
    #[serde(rename(serialize = "hp:lineBreak", deserialize = "lineBreak"))]
    LineBreak {},
    /// `<hp:tab/>` — tab character within a text run.
    ///
    /// Carries the optional per-occurrence attributes Hancom emits for
    /// rich tabs (`width` in HwpUnit, `leader` as raw HWP5 fill_type,
    /// `tab_type` as raw HWP5 tab type). Bare `<hp:tab/>` (the common
    /// case from HWP5 default tabs) parses with all three as `None`
    /// and round-trips through `HxText::to_run_content` as
    /// `RunContent::Text("\t")` — keeping the existing
    /// `Text(String)` surface in place for the common path.
    ///
    /// See `.docs/debug/2026-05-27_hwpx_decoder_inline_tab_attrs_lost.md`
    /// for the algorithm and side-effect analysis.
    #[serde(rename(serialize = "hp:tab", deserialize = "tab"))]
    Tab {
        /// Inline tab stop position (HwpUnit). `None` when the
        /// element was emitted without the `width` attribute.
        #[serde(rename = "@width", default, skip_serializing_if = "Option::is_none")]
        width: Option<i32>,
        /// Raw HWP5 fill_type byte (0..=4 known: 0=None, 1=Dot,
        /// 2=LongDash, 3=Dash, 4=Underscore).
        #[serde(rename = "@leader", default, skip_serializing_if = "Option::is_none")]
        leader: Option<u8>,
        /// Raw HWP5 tab_type byte (0=Left, 1=Right, 2=Center,
        /// 3=Decimal).
        #[serde(rename = "@type", default, skip_serializing_if = "Option::is_none")]
        tab_type: Option<u8>,
    },
    /// `<hp:fwSpace/>` — fixed-width space.
    #[serde(rename(serialize = "hp:fwSpace", deserialize = "fwSpace"))]
    FwSpace {},
    /// `<hp:nbSpace/>` — non-breaking space.
    #[serde(rename(serialize = "hp:nbSpace", deserialize = "nbSpace"))]
    NbSpace {},
    /// `<hp:markpenBegin/>` — highlight marker begin (no text output).
    #[serde(rename(serialize = "hp:markpenBegin", deserialize = "markpenBegin"))]
    MarkpenBegin {},
    /// `<hp:markpenEnd/>` — highlight marker end (no text output).
    #[serde(rename(serialize = "hp:markpenEnd", deserialize = "markpenEnd"))]
    MarkpenEnd {},
    /// Catch-all for unknown inline elements (titleMark, hyphen, etc.)
    #[serde(other)]
    Other,
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
    #[serde(rename = "@charSz", default, deserialize_with = "deser_i32_or_u32")]
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

// ── Field control types ──────────────────────────────────────────

/// `<hp:stringParam name="..." xml:space="preserve">value</hp:stringParam>`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxStringParam {
    /// Parameter name (e.g. `"Path"`, `"Command"`).
    #[serde(rename = "@name", default)]
    pub name: String,
    /// XML space preservation attribute.
    #[serde(rename = "@xml:space", default, skip_serializing_if = "String::is_empty")]
    pub xml_space: String,
    /// Parameter value (text content).
    #[serde(rename = "$text", default)]
    pub value: String,
}

/// `<hp:integerParam name="...">value</hp:integerParam>`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxIntegerParam {
    /// Parameter name (e.g. `"Prop"`).
    #[serde(rename = "@name", default)]
    pub name: String,
    /// Parameter value as string (text content).
    #[serde(rename = "$text", default)]
    pub value: String,
}

/// `<hp:booleanParam name="...">value</hp:booleanParam>`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxBooleanParam {
    /// Parameter name (e.g. `"RefHyperLink"`).
    #[serde(rename = "@name", default)]
    pub name: String,
    /// Parameter value as string (text content).
    #[serde(rename = "$text", default)]
    pub value: String,
}

/// `<hp:parameters cnt="..." name="...">` — field parameter container.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxFieldParameters {
    /// Number of parameters declared.
    #[serde(rename = "@cnt", default)]
    pub cnt: u32,
    /// Parameter group name (usually empty).
    #[serde(rename = "@name", default)]
    pub name: String,
    /// String-typed parameters.
    #[serde(
        rename(serialize = "hp:stringParam", deserialize = "stringParam"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub string_params: Vec<HxStringParam>,
    /// Integer-typed parameters.
    #[serde(
        rename(serialize = "hp:integerParam", deserialize = "integerParam"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub integer_params: Vec<HxIntegerParam>,
    /// Boolean-typed parameters.
    #[serde(
        rename(serialize = "hp:booleanParam", deserialize = "booleanParam"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub boolean_params: Vec<HxBooleanParam>,
}

/// `<hp:fieldBegin>` — start of a field control pair (hyperlink, bookmark, etc.).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxFieldBegin {
    /// Element ID.
    #[serde(rename = "@id", default)]
    pub id: String,
    /// Field type (e.g. `"HYPERLINK"`, `"BOOKMARK"`, `"SUMMERY"`, `"CROSSREF"`, `"MEMO"`).
    #[serde(rename = "@type", default)]
    pub field_type: String,
    /// Field name (used by bookmarks).
    #[serde(rename = "@name", default)]
    pub name: String,
    /// Whether the field content is editable.
    #[serde(rename = "@editable", default)]
    pub editable: String,
    /// Dirty flag.
    #[serde(rename = "@dirty", default)]
    pub dirty: String,
    /// Z-order for stacking.
    #[serde(rename = "@zorder", default)]
    pub zorder: String,
    /// Field identifier for begin/end pairing.
    #[serde(rename = "@fieldid", default)]
    pub fieldid: String,
    /// Meta tag for field categorization.
    #[serde(rename = "@metaTag", default)]
    pub meta_tag: String,
    /// Optional parameters (Path, Command, RefPath, etc.).
    #[serde(
        rename(serialize = "hp:parameters", deserialize = "parameters"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub parameters: Option<HxFieldParameters>,
    /// Optional subList (used by MEMO fields for memo body content).
    #[serde(
        rename(serialize = "hp:subList", deserialize = "subList"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sub_list: Option<HxSubList>,
}

/// `<hp:fieldEnd>` — end of a field control pair.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxFieldEnd {
    /// Reference to the matching fieldBegin's ID.
    #[serde(rename = "@beginIDRef", default)]
    pub begin_id_ref: String,
    /// Field identifier for begin/end pairing.
    #[serde(rename = "@fieldid", default)]
    pub fieldid: String,
}

/// `<hp:autoNumFormat>` — formatting for auto-numbering.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxAutoNumFormat {
    /// Format type (e.g. `"DIGIT"`).
    #[serde(rename = "@type", default)]
    pub format_type: String,
    /// User-defined character for custom numbering.
    #[serde(rename = "@userChar", default)]
    pub user_char: String,
    /// Prefix character before the number.
    #[serde(rename = "@prefixChar", default)]
    pub prefix_char: String,
    /// Suffix character after the number.
    #[serde(rename = "@suffixChar", default)]
    pub suffix_char: String,
    /// Superscript flag.
    #[serde(rename = "@supscript", default)]
    pub supscript: String,
}

/// `<hp:autoNum>` — inline auto-numbering (page number, etc.).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxAutoNum {
    /// Current number value.
    #[serde(rename = "@num", default)]
    pub num: u32,
    /// Numbering type (e.g. `"PAGE"`, `"FOOTNOTE"`).
    #[serde(rename = "@numType", default)]
    pub num_type: String,
    /// Optional formatting specification.
    #[serde(
        rename(serialize = "hp:autoNumFormat", deserialize = "autoNumFormat"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub auto_num_format: Option<HxAutoNumFormat>,
}

// ── Control wrapper ──────────────────────────────────────────────

/// `<hp:ctrl>` — wrapper for header, footer, colPr, pageNum, footnote, endnote,
/// fieldBegin, fieldEnd, autoNum.
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
    /// Optional fieldBegin element (hyperlink, bookmark, field, crossref, memo).
    #[serde(
        rename(serialize = "hp:fieldBegin", deserialize = "fieldBegin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub field_begin: Option<HxFieldBegin>,
    /// Optional fieldEnd element (closes a fieldBegin pair).
    #[serde(
        rename(serialize = "hp:fieldEnd", deserialize = "fieldEnd"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub field_end: Option<HxFieldEnd>,
    /// Optional autoNum element (inline page number).
    #[serde(
        rename(serialize = "hp:autoNum", deserialize = "autoNum"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub auto_num: Option<HxAutoNum>,
    /// Optional newNum element (새 번호 지정 — 번호 재시작).
    #[serde(
        rename(serialize = "hp:newNum", deserialize = "newNum"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub new_num: Option<HxNewNum>,
    /// Optional pageHiding element (감추기 — 해당 쪽 요소 숨김).
    #[serde(
        rename(serialize = "hp:pageHiding", deserialize = "pageHiding"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub page_hiding: Option<HxPageHiding>,
}

/// `<hp:pageHiding hideHeader="0" … hidePageNum="1"/>` — 감추기 (표 177).
///
/// 한컴 F2 실측 (2026-08-12): run 안 `<hp:ctrl>` 자식, 자기닫힘, 6속성이
/// 항상 `"0"`/`"1"` 로 병기된다.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxPageHiding {
    /// 머리말 감춤: 0 or 1.
    #[serde(rename = "@hideHeader", default)]
    pub hide_header: u8,
    /// 꼬리말 감춤: 0 or 1.
    #[serde(rename = "@hideFooter", default)]
    pub hide_footer: u8,
    /// 바탕쪽 감춤: 0 or 1.
    #[serde(rename = "@hideMasterPage", default)]
    pub hide_master_page: u8,
    /// 테두리 감춤: 0 or 1.
    #[serde(rename = "@hideBorder", default)]
    pub hide_border: u8,
    /// 배경 감춤: 0 or 1.
    #[serde(rename = "@hideFill", default)]
    pub hide_fill: u8,
    /// 쪽번호 감춤: 0 or 1.
    #[serde(rename = "@hidePageNum", default)]
    pub hide_page_num: u8,
}

/// `<hp:newNum num="7" numType="PAGE"/>` — 새 번호 지정 (번호 재시작).
///
/// 한컴 F1b 실측 (2026-08-12): run 안 `<hp:ctrl>` 자식, 자기닫힘, 텍스트
/// 위치를 소비하지 않는다 (linesegarray textpos 무영향).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxNewNum {
    /// New counter value (`xs:positiveInteger`).
    #[serde(rename = "@num", default)]
    pub num: u32,
    /// Counter type (`"PAGE"` / `"FOOTNOTE"` / `"ENDNOTE"` / `"PICTURE"` /
    /// `"TABLE"` / `"EQUATION"`).
    #[serde(rename = "@numType", default)]
    pub num_type: String,
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

/// `<hp:colLine>` — separator line between columns (child of `<hp:colPr>`).
///
/// Mirrors `HxBorderLine`: all attributes are wire strings (`type`, `width`
/// in millimetres e.g. `"0.7 mm"`, `color` e.g. `"#3A3C84"`).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxColLine {
    /// Line type string (e.g. `"SOLID"`, `"DOUBLE_SLIM"`).
    #[serde(rename = "@type", default)]
    pub line_type: String,
    /// Width string in millimetres (e.g. `"0.7 mm"`).
    #[serde(rename = "@width", default)]
    pub width: String,
    /// Color string (e.g. `"#3A3C84"`).
    #[serde(rename = "@color", default)]
    pub color: String,
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
    #[serde(rename = "@sameGap", default, deserialize_with = "deser_i32_or_u32")]
    pub same_gap: i32,

    /// Optional separator line drawn between columns (`<hp:colLine>`).
    /// OWPML orders `colLine` before `colSz`/`<hp:col>` children.
    #[serde(
        rename(serialize = "hp:colLine", deserialize = "colLine"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub col_line: Option<HxColLine>,

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
    #[serde(rename = "@width", default, deserialize_with = "deser_i32_or_u32")]
    pub width: i32,
    /// Gap after this column in HWPUNIT (0 for last column).
    #[serde(rename = "@gap", default, deserialize_with = "deser_i32_or_u32")]
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
    ///
    /// Widened to `u64` in E6/M2 to mirror Core's `Option<ObjectId>` without
    /// truncation (footnote/endnote ids share the unified id space). In-range
    /// values serialize byte-identically to the former `u32`.
    #[serde(rename = "@instId", default, skip_serializing_if = "Option::is_none")]
    pub inst_id: Option<u64>,

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
    #[serde(rename = "@width", default, deserialize_with = "deser_i32_or_u32")]
    pub width: i32,
    /// Gap between caption and object (default: 850 HWPUNIT ~= 3mm).
    #[serde(
        rename = "@gap",
        default = "default_caption_gap",
        deserialize_with = "deser_i32_or_u32"
    )]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
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

    /// `<hp:startNum>` — starting numbers for page/pic/tbl/equation.
    /// Deserialized from secPr for round-trip fidelity; encoding is handled
    /// by `enrich_sec_pr()` via raw XML injection (hence `skip_serializing`).
    #[serde(rename = "startNum", default, skip_serializing)]
    pub start_num: Option<HxStartNum>,
    // footNotePr, endNotePr, grid — still skipped by serde
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
    #[serde(rename = "@distance", default, deserialize_with = "deser_i32_or_u32")]
    pub distance: i32,
    /// Starting line number.
    #[serde(rename = "@startNumber", default)]
    pub start_number: u32,
}

/// `<hp:startNum>` — starting numbers for auto-numbering within a section.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HxStartNum {
    /// Which page to start on: BOTH, ODD, EVEN.
    #[serde(rename = "@pageStartsOn", default)]
    pub page_starts_on: String,
    /// Starting page number.
    #[serde(rename = "@page", default)]
    pub page: u32,
    /// Starting picture number.
    #[serde(rename = "@pic", default)]
    pub pic: u32,
    /// Starting table number.
    #[serde(rename = "@tbl", default)]
    pub tbl: u32,
    /// Starting equation number.
    #[serde(rename = "@equation", default)]
    pub equation: u32,
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
    #[serde(rename = "@left", default, deserialize_with = "deser_i32_or_u32")]
    pub left: i32,
    /// Right offset in HwpUnit.
    #[serde(rename = "@right", default, deserialize_with = "deser_i32_or_u32")]
    pub right: i32,
    /// Top offset in HwpUnit.
    #[serde(rename = "@top", default, deserialize_with = "deser_i32_or_u32")]
    pub top: i32,
    /// Bottom offset in HwpUnit.
    #[serde(rename = "@bottom", default, deserialize_with = "deser_i32_or_u32")]
    pub bottom: i32,
}

/// `<hp:pagePr landscape="WIDELY" width="59528" height="84188">`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxPagePr {
    #[serde(rename = "@landscape", default)]
    pub landscape: String,
    #[serde(rename = "@width", default, deserialize_with = "deser_i32_or_u32")]
    pub width: i32,
    #[serde(rename = "@height", default, deserialize_with = "deser_i32_or_u32")]
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
    #[serde(rename = "@header", default, deserialize_with = "deser_i32_or_u32")]
    pub header: i32,
    #[serde(rename = "@footer", default, deserialize_with = "deser_i32_or_u32")]
    pub footer: i32,
    #[serde(rename = "@gutter", default, deserialize_with = "deser_i32_or_u32")]
    pub gutter: i32,
    #[serde(rename = "@left", default, deserialize_with = "deser_i32_or_u32")]
    pub left: i32,
    #[serde(rename = "@right", default, deserialize_with = "deser_i32_or_u32")]
    pub right: i32,
    #[serde(rename = "@top", default, deserialize_with = "deser_i32_or_u32")]
    pub top: i32,
    #[serde(rename = "@bottom", default, deserialize_with = "deser_i32_or_u32")]
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
    #[serde(rename = "@vertpos", default, deserialize_with = "deser_i32_or_u32")]
    pub vertpos: i32,
    /// Vertical size of the line (HWPUNIT).
    #[serde(rename = "@vertsize", default, deserialize_with = "deser_i32_or_u32")]
    pub vertsize: i32,
    /// Text height within the line (HWPUNIT).
    #[serde(rename = "@textheight", default, deserialize_with = "deser_i32_or_u32")]
    pub textheight: i32,
    /// Baseline position from the top of the line (HWPUNIT).
    #[serde(rename = "@baseline", default, deserialize_with = "deser_i32_or_u32")]
    pub baseline: i32,
    /// Line spacing value (HWPUNIT).
    #[serde(rename = "@spacing", default, deserialize_with = "deser_i32_or_u32")]
    pub spacing: i32,
    /// Horizontal position of the line start (HWPUNIT).
    #[serde(rename = "@horzpos", default, deserialize_with = "deser_i32_or_u32")]
    pub horzpos: i32,
    /// Horizontal size available for text (HWPUNIT).
    #[serde(rename = "@horzsize", default, deserialize_with = "deser_i32_or_u32")]
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
    #[serde(rename = "@pageBreak", default = "default_table_page_break")]
    pub page_break: String,
    #[serde(rename = "@repeatHeader", default = "default_table_repeat_header")]
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
    #[serde(rename = "@width", default, deserialize_with = "deser_i32_or_u32")]
    pub width: i32,
    #[serde(rename = "@widthRelTo", default)]
    pub width_rel_to: String,
    #[serde(rename = "@height", default, deserialize_with = "deser_i32_or_u32")]
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
    #[serde(rename = "@vertOffset", default, deserialize_with = "deser_i32_or_u32")]
    pub vert_offset: i32,
    #[serde(rename = "@horzOffset", default, deserialize_with = "deser_i32_or_u32")]
    pub horz_offset: i32,
}

/// `<hp:outMargin>` / `<hp:inMargin>` / `<hp:cellMargin>` — margin specification.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxTableMargin {
    #[serde(rename = "@left", default, deserialize_with = "deser_i32_or_u32")]
    pub left: i32,
    #[serde(rename = "@right", default, deserialize_with = "deser_i32_or_u32")]
    pub right: i32,
    #[serde(rename = "@top", default, deserialize_with = "deser_i32_or_u32")]
    pub top: i32,
    #[serde(rename = "@bottom", default, deserialize_with = "deser_i32_or_u32")]
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
    #[serde(rename = "@width", default, deserialize_with = "deser_i32_or_u32")]
    pub width: i32,
    #[serde(rename = "@height", default, deserialize_with = "deser_i32_or_u32")]
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
    #[serde(rename = "@bright", default, deserialize_with = "deser_i32_or_u32")]
    pub bright: i32,
    #[serde(rename = "@contrast", default, deserialize_with = "deser_i32_or_u32")]
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
    #[serde(rename = "@width", default, deserialize_with = "deser_i32_or_u32")]
    pub width: i32,
    #[serde(rename = "@height", default, deserialize_with = "deser_i32_or_u32")]
    pub height: i32,
}

// ── Picture-specific sub-elements ────────────────────────────────

/// `<hp:offset x="0" y="0"/>` — position offset for shapes.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxOffset {
    #[serde(rename = "@x", default, deserialize_with = "deser_i32_or_u32")]
    pub x: i32,
    #[serde(rename = "@y", default, deserialize_with = "deser_i32_or_u32")]
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
    #[serde(rename = "@angle", default, deserialize_with = "deser_i32_or_u32")]
    pub angle: i32,
    #[serde(rename = "@centerX", default, deserialize_with = "deser_i32_or_u32")]
    pub center_x: i32,
    #[serde(rename = "@centerY", default, deserialize_with = "deser_i32_or_u32")]
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
    #[serde(rename = "@left", default, deserialize_with = "deser_i32_or_u32")]
    pub left: i32,
    #[serde(rename = "@right", default, deserialize_with = "deser_i32_or_u32")]
    pub right: i32,
    #[serde(rename = "@top", default, deserialize_with = "deser_i32_or_u32")]
    pub top: i32,
    #[serde(rename = "@bottom", default, deserialize_with = "deser_i32_or_u32")]
    pub bottom: i32,
}

/// `<hp:imgDim dimwidth="..." dimheight="..."/>` — original pixel dimensions.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxImgDim {
    #[serde(rename = "@dimwidth", default, deserialize_with = "deser_i32_or_u32")]
    pub dim_width: i32,
    #[serde(rename = "@dimheight", default, deserialize_with = "deser_i32_or_u32")]
    pub dim_height: i32,
}

// Shape types (HxRect, HxDrawText) are defined in `super::shapes`.

/// 2D point for shape geometry (e.g., rectangle corners).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxPoint {
    /// X coordinate (HWPUNIT).
    #[serde(rename = "@x", default, deserialize_with = "deser_i32_or_u32")]
    pub x: i32,
    /// Y coordinate (HWPUNIT).
    #[serde(rename = "@y", default, deserialize_with = "deser_i32_or_u32")]
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
        assert_eq!(p.runs[0].texts[0].text(), "안녕하세요");
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
        assert_eq!(p.runs[0].texts[0].text(), "Hello");
        assert_eq!(p.runs[1].char_pr_id_ref, 7);
        assert_eq!(p.runs[1].texts[0].text(), "World");
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
        assert_eq!(sec.paragraphs[0].runs[0].texts[0].text(), "");
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
        let text = cell0.sub_list.as_ref().unwrap().paragraphs[0].runs[0].texts[0].text();
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
        assert_eq!(run.texts[0].text(), "text after ctrl");
    }

    #[test]
    fn linesegarray_is_captured() {
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
        assert_eq!(sec.paragraphs[0].runs[0].texts[0].text(), "text");
        let array = sec.paragraphs[0].linesegarray.as_ref().expect("captured");
        assert_eq!(array.items.len(), 1);
        assert_eq!(array.items[0].vertsize, 1000);
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
        assert_eq!(sec.paragraphs[0].runs[0].texts[0].text(), "First");
        assert_eq!(sec.paragraphs[1].runs[0].texts[0].text(), "Second");
        assert_eq!(sec.paragraphs[2].runs[0].texts[0].text(), "Third");
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
        assert_eq!(cap.sub_list.paragraphs[0].runs[0].texts[0].text(), "Figure 1. Sample");

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
        assert_eq!(cap.sub_list.paragraphs[0].runs[0].texts[0].text(), "Table 1. Data");
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
