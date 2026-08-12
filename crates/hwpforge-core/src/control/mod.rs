//! Control elements: text boxes, hyperlinks, footnotes, endnotes, etc.
//!
//! [`Control`] represents non-text inline elements within a document.
//! The enum is `#[non_exhaustive]` so new control types can be added
//! in future phases without a breaking change.
//!
//! TextBox, Footnote, and Endnote contain `Vec<Paragraph>` (recursive
//! reference through the document tree). This is how HWP models inline
//! frames and annotations.
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::control::Control;
//! use hwpforge_core::paragraph::Paragraph;
//! use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
//!
//! let link = Control::Hyperlink {
//!     text: "Click here".to_string(),
//!     url: "https://example.com".to_string(),
//! };
//! assert!(link.is_hyperlink());
//! ```

mod fields;
mod metadata;
mod shapes;

pub use fields::*;
pub use metadata::*;
pub use shapes::*;

use hwpforge_foundation::{
    ArcType, BookmarkType, Color, CurveSegmentType, FieldType, HwpUnit, RefContentType, RefType,
    VerticalAlign,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::caption::Caption;
use crate::chart::{
    BarShape, ChartData, ChartGrouping, ChartType, LegendPosition, OfPieType, RadarStyle,
    ScatterStyle, StockVariant,
};
use crate::error::{CoreError, CoreResult};
use crate::object_id::ObjectId;
use crate::paragraph::Paragraph;
use crate::run::Run;

/// An inline control element.
///
/// Controls are non-text elements that appear within a Run.
/// Each variant carries its own data; the enum is `#[non_exhaustive]`
/// for forward compatibility.
///
/// # Examples
///
/// ```
/// use hwpforge_core::control::Control;
/// use hwpforge_core::paragraph::Paragraph;
/// use hwpforge_foundation::{HwpUnit, ParaShapeIndex, VerticalAlign};
///
/// let text_box = Control::TextBox {
///     paragraphs: vec![Paragraph::new(ParaShapeIndex::new(0))],
///     width: HwpUnit::from_mm(80.0).unwrap(),
///     height: HwpUnit::from_mm(40.0).unwrap(),
///     horz_offset: 0,
///     vert_offset: 0,
///     caption: None,
///     style: None,
///     text_vertical_align: VerticalAlign::Top,
/// };
/// assert!(text_box.is_text_box());
/// assert!(!text_box.is_hyperlink());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum Control {
    /// An inline text box with its own paragraph content.
    /// Maps to HWPX `<hp:rect>` + `<hp:drawText>` (drawing object, not control).
    TextBox {
        /// Paragraphs inside the text box.
        paragraphs: Vec<Paragraph>,
        /// Box width (HWPUNIT).
        width: HwpUnit,
        /// Box height (HWPUNIT).
        height: HwpUnit,
        /// Horizontal offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        horz_offset: i32,
        /// Vertical offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        vert_offset: i32,
        /// Optional caption attached to this text box.
        caption: Option<Caption>,
        /// Optional visual style overrides (border color, fill, line width).
        style: Option<ShapeStyle>,
        /// Vertical alignment of the embedded text within the box.
        /// Maps to HWPX `<hp:drawText><hp:subList vertAlign="...">` and HWP5
        /// 문단 리스트 헤더 속성 bits 5–6. Defaults to [`VerticalAlign::Top`].
        #[serde(default)]
        text_vertical_align: VerticalAlign,
    },

    /// A hyperlink with display text and URL.
    Hyperlink {
        /// Visible text of the link.
        text: String,
        /// Target URL.
        url: String,
    },

    /// A footnote containing paragraph content.
    /// Maps to HWPX `<hp:ctrl><hp:footNote>`.
    Footnote {
        /// Object identity for cross-ref linking (optional). Shares the
        /// [`ObjectId`] space with [`RefTarget::Object`](crate::control::RefTarget::Object).
        inst_id: Option<ObjectId>,
        /// Paragraphs that form the footnote body.
        paragraphs: Vec<Paragraph>,
    },

    /// An endnote containing paragraph content.
    /// Maps to HWPX `<hp:ctrl><hp:endNote>`.
    Endnote {
        /// Object identity for cross-ref linking (optional). Shares the
        /// [`ObjectId`] space with [`RefTarget::Object`](crate::control::RefTarget::Object).
        inst_id: Option<ObjectId>,
        /// Paragraphs that form the endnote body.
        paragraphs: Vec<Paragraph>,
    },

    /// A line drawing object (2 endpoints).
    /// Maps to HWPX `<hp:line>`.
    Line {
        /// Start point (x, y in HWPUNIT).
        start: ShapePoint,
        /// End point (x, y in HWPUNIT).
        end: ShapePoint,
        /// Bounding box width (HWPUNIT).
        width: HwpUnit,
        /// Bounding box height (HWPUNIT).
        height: HwpUnit,
        /// Horizontal offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        horz_offset: i32,
        /// Vertical offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        vert_offset: i32,
        /// Optional caption attached to this line.
        caption: Option<Caption>,
        /// Optional visual style overrides (border color, fill, line width).
        style: Option<ShapeStyle>,
    },

    /// An ellipse (or circle) drawing object.
    /// Maps to HWPX `<hp:ellipse>`.
    Ellipse {
        /// Center point (x, y in HWPUNIT).
        center: ShapePoint,
        /// Axis 1 endpoint (defines semi-major axis direction and length).
        axis1: ShapePoint,
        /// Axis 2 endpoint (perpendicular to axis1, defines semi-minor axis).
        axis2: ShapePoint,
        /// Bounding box width (HWPUNIT).
        width: HwpUnit,
        /// Bounding box height (HWPUNIT).
        height: HwpUnit,
        /// Horizontal offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        horz_offset: i32,
        /// Vertical offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        vert_offset: i32,
        /// Optional text content inside the ellipse.
        paragraphs: Vec<Paragraph>,
        /// Optional caption attached to this ellipse.
        caption: Option<Caption>,
        /// Optional visual style overrides (border color, fill, line width).
        style: Option<ShapeStyle>,
        /// Vertical alignment of the embedded text within the ellipse.
        /// Maps to HWPX `<hp:drawText><hp:subList vertAlign="...">` and HWP5
        /// 문단 리스트 헤더 속성 bits 5–6. Defaults to [`VerticalAlign::Top`].
        #[serde(default)]
        text_vertical_align: VerticalAlign,
    },

    /// A HWP5 chart carried as opaque OOXML + OLE blob passthrough.
    ///
    /// Used when chart data is extracted from a HWP5 BinData OLE container
    /// and emitted to HWPX without round-tripping through the structured
    /// [`Control::Chart`] data model. Renders in 한컴 via the `<hp:switch>`
    /// block with full OOXML chart inside `<hp:case>` and an OLE fallback
    /// inside `<hp:default>`.
    ///
    /// Wave 4c passthrough: the chart XML and OLE bytes are carried as-is
    /// from the source HWP5 file. The encoder writes:
    /// - `Chart/chartN.xml` (NOT registered in manifest — gotcha #5)
    /// - `BinData/oleN.ole` (registered in `content.hpf` as `application/ole`)
    /// - section `<hp:switch>` with `<hp:case>` chart + `<hp:default>` ole
    EmbeddedChart {
        /// Full OOXML chart XML (starts with `<?xml`, contains `<c:chartSpace>`).
        chart_xml: String,
        /// Raw OLE2 compound file bytes for `<hp:ole>` fallback rendering.
        ole_bytes: Vec<u8>,
        /// Chart width (HWPUNIT).
        width: HwpUnit,
        /// Chart height (HWPUNIT).
        height: HwpUnit,
        /// Horizontal offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        horz_offset: i32,
        /// Vertical offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        vert_offset: i32,
    },

    /// A pure rectangle drawing object (no embedded text).
    ///
    /// Distinct from [`Control::TextBox`], which uses `<hp:rect>` with a
    /// `<hp:drawText>` child for inline text. A pure `Rect` carries only the
    /// rectangle geometry and visual style and emits `<hp:rect>` without
    /// `<hp:drawText>`.
    Rect {
        /// Bounding box width (HWPUNIT).
        width: HwpUnit,
        /// Bounding box height (HWPUNIT).
        height: HwpUnit,
        /// Horizontal offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        horz_offset: i32,
        /// Vertical offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        vert_offset: i32,
        /// Optional caption attached to this rectangle.
        caption: Option<Caption>,
        /// Optional visual style overrides (border color, fill, line width).
        style: Option<ShapeStyle>,
    },

    /// A polygon drawing object (3+ vertices).
    /// Maps to HWPX `<hp:polygon>`.
    Polygon {
        /// Ordered list of vertices (minimum 3).
        vertices: Vec<ShapePoint>,
        /// Bounding box width (HWPUNIT).
        width: HwpUnit,
        /// Bounding box height (HWPUNIT).
        height: HwpUnit,
        /// Horizontal offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        horz_offset: i32,
        /// Vertical offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        vert_offset: i32,
        /// Optional text content inside the polygon.
        paragraphs: Vec<Paragraph>,
        /// Optional caption attached to this polygon.
        caption: Option<Caption>,
        /// Optional visual style overrides (border color, fill, line width).
        style: Option<ShapeStyle>,
        /// Vertical alignment of the embedded text within the polygon.
        /// Maps to HWPX `<hp:drawText><hp:subList vertAlign="...">` and HWP5
        /// 문단 리스트 헤더 속성 bits 5–6. Defaults to [`VerticalAlign::Top`].
        #[serde(default)]
        text_vertical_align: VerticalAlign,
    },

    /// An inline equation (수식) using HancomEQN script format.
    /// Maps to HWPX `<hp:equation>` with `<hp:script>` child.
    ///
    /// Equations have NO shape common block (no offset, orgSz, curSz, flip,
    /// rotation, lineShape, fillBrush, shadow). Only sz + pos + outMargin + script.
    Equation {
        /// HancomEQN script text (e.g. `"{a+b} over {c+d}"`).
        script: String,
        /// Bounding box width (HWPUNIT).
        width: HwpUnit,
        /// Bounding box height (HWPUNIT).
        height: HwpUnit,
        /// Baseline position (51-90 typical range).
        base_line: u32,
        /// Text color.
        text_color: Color,
        /// Font name (typically `"HancomEQN"`).
        font: String,
        /// Wave 12p Step 2c: instance ID for cross-ref target lookup.
        /// HWP5 변환 시 `eqed` CtrlHeader trailer 의 instance ID 가
        /// 채워지고, HWPX encoder 가 `<hp:equation id="...">` attribute
        /// 로 emit. `None` 이면 encoder fallback 허용.
        inst_id: Option<ObjectId>,
    },

    /// An OOXML chart embedded in the document.
    /// Maps to HWPX `<hp:switch><hp:case><hp:chart>` with separate Chart XML file.
    ///
    /// Charts have NO shape common block (like Equation): only sz + pos + outMargin.
    Chart {
        /// Chart type (18 variants covering all OOXML chart types).
        chart_type: ChartType,
        /// Chart data (category-based or XY-based).
        data: ChartData,
        /// Chart width (HWPUNIT, default ~32250 ≈ 114mm).
        width: HwpUnit,
        /// Chart height (HWPUNIT, default ~18750 ≈ 66mm).
        height: HwpUnit,
        /// Optional chart title.
        title: Option<String>,
        /// Legend position.
        legend: LegendPosition,
        /// Series grouping mode.
        grouping: ChartGrouping,
        /// 3D bar/column shape (None = default Box).
        bar_shape: Option<BarShape>,
        /// Exploded pie/doughnut percentage (None = not exploded, Some(25) = 25% explosion).
        explosion: Option<u32>,
        /// Pie-of-pie or bar-of-pie sub-type (None = default pie-of-pie).
        of_pie_type: Option<OfPieType>,
        /// Radar chart rendering style (None = default Standard).
        radar_style: Option<RadarStyle>,
        /// Surface chart wireframe mode (None = default solid).
        wireframe: Option<bool>,
        /// 3D bubble effect (None = default flat).
        bubble_3d: Option<bool>,
        /// Scatter chart style (None = default Dots).
        scatter_style: Option<ScatterStyle>,
        /// Show data point markers on line charts (None = no markers).
        show_markers: Option<bool>,
        /// Stock chart sub-variant (None = default HLC, 3 series).
        ///
        /// VHLC and VOHLC generate a composite `<c:plotArea>` with both
        /// `<c:barChart>` (volume) and `<c:stockChart>` (price) elements.
        stock_variant: Option<StockVariant>,
    },

    /// Dutmal (덧말): annotation text displayed above or below main text.
    /// Maps to HWPX `<hp:dutmal>`.
    Dutmal {
        /// Main text that receives the annotation.
        main_text: String,
        /// Annotation text displayed above/below.
        sub_text: String,
        /// Position of the annotation relative to main text.
        position: DutmalPosition,
        /// Size ratio of annotation text relative to main (0 = auto).
        sz_ratio: u32,
        /// Alignment of the annotation text.
        align: DutmalAlign,
        /// Optional metadata that mirrors HWPX `<hp:dutmal>` attributes
        /// HwpForge doesn't promote to typed fields yet — currently
        /// carries `option` verbatim so HWP5↔HWPX round-trips preserve
        /// it. `#[non_exhaustive]` so future fields are additive.
        metadata: DutmalMetadata,
    },

    /// Compose (글자겹침): overlaid/combined characters.
    /// Maps to HWPX `<hp:compose>`.
    Compose {
        /// The combined text (e.g. "12" for two overlaid digits).
        compose_text: String,
        /// Circle/frame type for the composition.
        circle_type: String,
        /// Character size adjustment (-3 = slightly smaller).
        char_sz: i32,
        /// Composition layout type.
        compose_type: String,
        /// 10 `<hp:charPr prIDRef="N"/>` references (HWPX `charPrCnt` is
        /// fixed at 10). `u32::MAX` is the "no override" sentinel —
        /// 한컴 emits it for unused slots. A `Vec` shorter or longer
        /// than 10 is normalized by the HWPX encoder (pad / truncate).
        char_pr_ids: Vec<u32>,
    },

    /// An arc (partial ellipse) drawing object.
    /// Maps to HWPX `<hp:ellipse>` with `hasArcPr="1"`.
    Arc {
        /// Arc type (normal open arc, pie/sector, chord).
        arc_type: ArcType,
        /// Center point of the parent ellipse.
        center: ShapePoint,
        /// Axis 1 endpoint (semi-major axis).
        axis1: ShapePoint,
        /// Axis 2 endpoint (semi-minor axis).
        axis2: ShapePoint,
        /// Arc start point 1.
        start1: ShapePoint,
        /// Arc end point 1.
        end1: ShapePoint,
        /// Arc start point 2.
        start2: ShapePoint,
        /// Arc end point 2.
        end2: ShapePoint,
        /// Bounding box width (HWPUNIT).
        width: HwpUnit,
        /// Bounding box height (HWPUNIT).
        height: HwpUnit,
        /// Horizontal offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        horz_offset: i32,
        /// Vertical offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        vert_offset: i32,
        /// Optional caption attached to this arc.
        caption: Option<Caption>,
        /// Optional visual style overrides.
        style: Option<ShapeStyle>,
    },

    /// A curve drawing object (bezier/polyline).
    /// Maps to HWPX `<hp:curve>`.
    Curve {
        /// Ordered control points for the curve path.
        points: Vec<ShapePoint>,
        /// Segment types (one per segment between points).
        segment_types: Vec<CurveSegmentType>,
        /// Bounding box width (HWPUNIT).
        width: HwpUnit,
        /// Bounding box height (HWPUNIT).
        height: HwpUnit,
        /// Horizontal offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        horz_offset: i32,
        /// Vertical offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        vert_offset: i32,
        /// Optional caption attached to this curve.
        caption: Option<Caption>,
        /// Optional visual style overrides.
        style: Option<ShapeStyle>,
    },

    /// A connect line drawing object (line with control points for routing).
    /// Maps to HWPX `<hp:connectLine>`.
    ConnectLine {
        /// Start point of the connect line.
        start: ShapePoint,
        /// End point of the connect line.
        end: ShapePoint,
        /// Intermediate control points for routing.
        control_points: Vec<ShapePoint>,
        /// Connect line type (e.g. "STRAIGHT", "BENT", "CURVED").
        connect_type: String,
        /// Bounding box width (HWPUNIT).
        width: HwpUnit,
        /// Bounding box height (HWPUNIT).
        height: HwpUnit,
        /// Horizontal offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        horz_offset: i32,
        /// Vertical offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        vert_offset: i32,
        /// Optional caption attached to this connect line.
        caption: Option<Caption>,
        /// Optional visual style overrides.
        style: Option<ShapeStyle>,
    },

    /// A group of drawing objects (묶음 객체 / 개체 묶기).
    /// Maps to HWPX `<hp:container>` and HWP5 `gso` → `ShapeComponent` with
    /// the `"$con"` type tag wrapping child `ShapeComponent`s.
    ///
    /// `children` reuses the shape `Control` variants (`Rect`/`Ellipse`/
    /// `Line`/`Polygon`/`Curve`/`ConnectLine`/`Image`/`EmbeddedChart` and,
    /// recursively, `Group`). Non-shape variants are rejected by
    /// `validate` rather than the type system, matching how every other
    /// recursive container (`TextBox`/`Footnote`/`Memo.content`) carries a
    /// loose `Vec<Paragraph>` / `Vec<Run>`.
    Group {
        /// Child drawing objects, in z-order. May nest further `Group`s.
        children: Vec<Control>,
        /// Bounding box width (HWPUNIT).
        width: HwpUnit,
        /// Bounding box height (HWPUNIT).
        height: HwpUnit,
        /// Horizontal offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        horz_offset: i32,
        /// Vertical offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        vert_offset: i32,
        /// HWP5 ParaHeader / GSO trailer instance ID, mirrored to the
        /// HWPX `<hp:container instid>` attribute. `None` = not carried.
        inst_id: Option<ObjectId>,
    },

    /// A TextArt (글맵시) decorative warped-text object.
    /// Maps to HWPX `<hp:textart>` with `<hp:textartPr>`.
    ///
    /// TextArt warps a short string into a shape (wave, arch, circle, …) and
    /// renders it as a drawing object. The HWP5 wire stores `shape` as an
    /// integer enum (`0..=54`); the HWPX wire stores it as a string name
    /// (e.g. `"WAVE2"`). This carries the HWPX string form directly.
    TextArt {
        /// The displayed text content.
        text: String,
        /// HWPX `textShape` name (e.g. `"WAVE2"`). One of 55 known shapes.
        shape: String,
        /// Font family name (e.g. `"함초롬바탕"`).
        font_name: String,
        /// Font style label (e.g. `"보통"`).
        font_style: String,
        /// HWPX `align` value within the textart (e.g. `"LEFT"`).
        align: String,
        /// Line spacing (percent, HWPX `lineSpacing`).
        line_spacing: u32,
        /// Character spacing (percent, HWPX `charSpacing`).
        char_spacing: u32,
        /// Bounding box width (HWPUNIT).
        width: HwpUnit,
        /// Bounding box height (HWPUNIT).
        height: HwpUnit,
        /// Horizontal offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        horz_offset: i32,
        /// Vertical offset from anchor point (HWPUNIT, 0 = inline/treat-as-char).
        vert_offset: i32,
        /// Fill color (the `<hc:winBrush faceColor>` of the textart glyphs).
        /// `None` = no explicit fill carried.
        fill_color: Option<Color>,
        /// HWP5 GSO trailer instance ID, mirrored to HWPX `<hp:textart instid>`.
        /// `None` = not carried.
        inst_id: Option<ObjectId>,
    },

    /// A bookmark marking a named location in the document.
    /// Maps to HWPX `<hp:ctrl><hp:bookmark>` (point) or `fieldBegin/fieldEnd type="BOOKMARK"` (span).
    Bookmark {
        /// Bookmark name (unique within the document).
        name: String,
        /// Type: point bookmark or span start/end.
        bookmark_type: BookmarkType,
    },

    /// A cross-reference (상호참조) to a bookmark, footnote, endnote,
    /// outline heading, table/figure/equation caption.
    ///
    /// Maps to HWPX `fieldBegin type="CROSSREF"` with parameters.
    ///
    /// Wave 12m Phase 2: `target_name: String` 가 `target: RefTarget` 로
    /// 변경 (breaking). 책갈피 이름과 한컴 자동 ID (#<id>) 가 타입으로
    /// 구분되어 caller 가 의미를 정확히 알 수 있음.
    ///
    /// Wave 12m Phase 2 Step 4 (breaking): `display_text: String` 추가.
    /// HWPX wire 는 `<hp:fieldBegin>` 과 `<hp:fieldEnd>` 사이의 visible run
    /// 으로 display text 를 embedding 한다. HWP5 `%xrf` wire 는 display
    /// text 를 직접 carry 하지 않고 ParaText 본문에 풀어 두지만, projection
    /// 이 FieldBegin..FieldEnd span 을 읽어 이 필드에 채워 넣는다.
    /// 빈 문자열은 "display text 없음" 의미 (Hyperlink::text 와 동일).
    CrossRef {
        /// Reference target — Bookmark name (`Name(String)`) or system
        /// id (`SystemId(u64)`) or unparseable raw (`Raw(String)`).
        target: RefTarget,
        /// What kind of target is being referenced.
        ref_type: RefType,
        /// What content to display at the reference site.
        content_type: RefContentType,
        /// Whether to render the reference as a clickable hyperlink.
        as_hyperlink: bool,
        /// Visible body text shown between `fieldBegin` and `fieldEnd`
        /// in the encoded wire. HWP5 sources this from the FieldBegin
        /// span; native builders may leave this empty.
        display_text: String,
    },

    /// A press-field (누름틀) — an interactive form field.
    /// Maps to HWPX `fieldBegin type="CLICK_HERE"` with parameters and `metaTag`.
    Field {
        /// Field type (ClickHere, Date, Time, etc.).
        field_type: FieldType,
        /// Hint/visible text shown in the field placeholder.
        hint_text: Option<String>,
        /// Help text shown when hovering or clicking the field.
        help_text: Option<String>,
        /// Form-mode identifier used to reference the field programmatically.
        /// Maps to HWPX `fieldBegin name="..."` attribute. `None` represents
        /// the empty string convention (한컴 wire stores it as a 0-length BSTR).
        name: Option<String>,
        /// Cached resolved value rendered between `<hp:fieldBegin>` and
        /// `<hp:fieldEnd>` (e.g. the author name, the locale-formatted date).
        /// HWP5 sources this from the FieldBegin..FieldEnd span; 한컴 native
        /// HWPX carries the same cached render and recomputes it on save.
        /// Empty string = "no cached value" (same convention as
        /// [`Self::CrossRef::display_text`] / [`Self::Hyperlink::text`]).
        /// For `ClickHere` this is the user-filled value; an unfilled field
        /// carries either the same string as [`Self::Field::hint_text`]
        /// (decoded from a native body) or the empty string (constructed).
        /// When empty, the HWPX encoder emits `hint_text` as the body.
        ///
        /// An empty body triggers 한컴's "낮은 보안 수준 복구" warning on
        /// open for SUMMERY fields (#120/#136) — carrying the verbatim
        /// source value avoids it.
        display_text: String,
    },

    /// A memo (메모) annotation attached to text.
    ///
    /// Maps to HWPX `fieldBegin type="MEMO"` + anchor body runs +
    /// `fieldEnd` flat inside one `<hp:run>`. The memo's body lives inside
    /// `fieldBegin`'s `<hp:subList>`; the *anchor* runs sit between
    /// `fieldBegin` and `fieldEnd` so 한컴 can pair the markers and render
    /// the `[메모 시작]…[메모 끝]` UI labels instead of generic
    /// `[메모 시작]…[필드 끝]`.
    ///
    /// Wave 12e: `author`/`date` fields removed (no wire path populated
    /// them).
    ///
    /// Wave 12f: `anchor_runs` added. Without it, the encoder produced an
    /// empty `<hp:t/>` between `fieldBegin` and `fieldEnd`, which 한컴 reads
    /// as an unpaired field — visible bug.
    Memo {
        /// Paragraphs forming the memo body content (rendered in
        /// `<hp:subList>`).
        content: Vec<Paragraph>,
        /// Runs that form the visible *anchor* text — the body span the memo
        /// is attached to. Encoders interleave these between `fieldBegin`
        /// and `fieldEnd` inside one `<hp:run>`. Should normally hold only
        /// `RunContent::Text`; other variants are downgraded by the encoder
        /// with a warning (memos cannot anchor on tables/images/nested
        /// controls in HWPX).
        anchor_runs: Vec<Run>,
        /// HWPX `<hp:parameters>` for the memo. Carrying these as a
        /// dedicated [`MemoMetadata`] (instead of half-empty hard-coded
        /// values) keeps the metadata format-agnostic — encoders for HWPX
        /// (and any future format with similar metadata) consume the same
        /// struct.
        metadata: MemoMetadata,
    },

    /// An index mark for building a document index (찾아보기).
    /// Maps to HWPX `<hp:ctrl><hp:indexmark>`.
    IndexMark {
        /// Primary index key (required).
        primary: String,
        /// Secondary (sub-entry) index key.
        secondary: Option<String>,
    },

    /// An unknown SUMMERY (`%smr`) `$token` carried verbatim for forward
    /// compatibility (Wave 12n).
    ///
    /// Wave 12n only models the five HwpForge-observed tokens (`$author`,
    /// `$lastsaveby`, `$createtime`, `$modifiedtime`, `$title`) as typed
    /// [`FieldType`] variants. Any other `%smr` Command (e.g. additional
    /// 한컴 metadata tokens not yet measured) is preserved here instead of
    /// being silently coerced to `ClickHere`.
    UnknownSummary {
        /// Raw `Command` string after envelope (e.g. `"$company"`).
        token: String,
        /// Cached resolved value rendered between `fieldBegin`/`fieldEnd`.
        /// Same semantics as [`Self::Field::display_text`]; empty = none.
        display_text: String,
    },

    /// A `%dte` date/time **format-pattern** field (Wave 12n).
    ///
    /// HWP5 family `%dte` (ctrl_id `0x2564_7465`) used by 한컴
    /// `입력 → 날짜/시간/파일 이름 → 날짜/시간 코드` menu. Unlike SUMMERY
    /// (which carries semantic tokens like `$createtime`), `%dte` carries
    /// a raw format pattern string (e.g. `"\:1년 2월 3일 (6);0;"` for date,
    /// `"T\:;0;"` for time-only). The HWP5 wire format pattern is
    /// smithy-internal; the format-agnostic core retains only the derived
    /// `is_time_mode` helper (was based on the `T` prefix at projection time).
    DateCodeField {
        /// Helper view: `true` for a time-only (`T`-prefixed) format,
        /// `false` for a date format. Derived at HWP5 projection time.
        is_time_mode: bool,
        /// Cached resolved value rendered between `fieldBegin`/`fieldEnd`
        /// (the locale-formatted date/time string). Same semantics as
        /// [`Self::Field::display_text`]; empty = none.
        display_text: String,
    },

    /// A `%pat` path / file-name field (Wave 12n).
    ///
    /// HWP5 family `%pat` (ctrl_id `0x2570_6174`) emitted by 한컴
    /// `상용구 → 파일 이름 / 파일 이름과 경로`. Uses `$P` (path) and
    /// `$F` (file name) format codes.
    PathField {
        /// Typed variant of the observed `Command` pattern.
        command: PathFieldCommand,
        /// Cached resolved value rendered between `fieldBegin`/`fieldEnd`
        /// (the absolute path/file name 한컴 last evaluated). Same semantics
        /// as [`Self::Field::display_text`]; empty = none. 한컴 recomputes
        /// `$P`/`$F` against the file's on-disk path on save, but an empty
        /// body on open triggers the recovery warning (#120).
        display_text: String,
    },

    /// An `atno` **inline** page number control (Wave 12n).
    ///
    /// HWP5 family `atno` (ctrl_id `0x6174_6E6F`) used by 한컴
    /// `상용구 → 현재 쪽 번호 / 전체 쪽수 / 현재 쪽/전체 쪽수`. Distinct
    /// from `pgnp` (section-level page numbering control already modeled
    /// as `Section.page_number`). Inline `atno` renders to HWPX
    /// `<hp:autoNum>` inside a `<hp:run>`.
    ///
    /// The 16-byte wire envelope carries a single 4-byte flag that
    /// distinguishes current-page from total-pages; the HWP5 projection
    /// maps it to [`InlinePageKind`].
    InlinePageNumber {
        /// Typed variant of the observed `flag` byte.
        kind: InlinePageKind,
    },

    /// A `nwno` **새 번호 지정** control — restarts a numbering counter
    /// from this position (HWPX `<hp:newNum num numType>`).
    ///
    /// HWP5 wire (native fixture 실측 2026-08-12): 10 bytes —
    /// `ctrl_id + 속성 u32(bits 0-3 = kind) + 번호 u16`, anchored in the
    /// paragraph text by a `0x15` inline marker. 한컴 [쪽]→[새 번호로
    /// 시작] 이 쓰는 경로다 (corpus: 407/2,231 문서). The restart applies
    /// from the **physical page containing the control** (PDF 실측:
    /// 1쪽 앵커 = 전문서 재번호, 2쪽 앵커 = `1, 7`).
    NewNumber {
        /// Which counter restarts ([`NewNumberKind::Page`] is the only
        /// renderer-consumed kind; others carry through to HWPX).
        kind: NewNumberKind,
        /// The new counter value (1-based; fixture sentinel `7`). `u32`
        /// covers both wires losslessly — HWP5 `nwno` carries u16, HWPX
        /// `newNum@num` is `xs:positiveInteger`.
        number: u32,
    },

    /// An unrecognized control element preserved for round-trip fidelity.
    ///
    /// `tag` holds the element's tag name or type identifier.
    /// `data` holds optional serialized content for lossless preservation.
    Unknown {
        /// Tag name or type identifier of the unrecognized element.
        tag: String,
        /// Optional serialized data for round-trip preservation.
        data: Option<String>,
    },
}

impl Control {
    /// 이 컨트롤 안에 중첩된 모든 문단을 재귀 방문한다
    /// (글상자/도형 본문·캡션·각주/미주·메모 본문+앵커 run·묶음 자식 포함).
    ///
    /// 새 variant 가 문단을 담게 되면 이 match 가 컴파일 에러로 강제한다
    /// (와일드카드 없음 — 순회 완전성 보장).
    pub(crate) fn walk_paragraphs_mut(
        &mut self,
        f: &mut dyn FnMut(&mut crate::paragraph::Paragraph),
    ) {
        fn walk_vec(
            paragraphs: &mut [crate::paragraph::Paragraph],
            f: &mut dyn FnMut(&mut crate::paragraph::Paragraph),
        ) {
            for p in paragraphs {
                p.walk_paragraphs_mut(f);
            }
        }
        fn walk_caption(
            caption: &mut Option<Caption>,
            f: &mut dyn FnMut(&mut crate::paragraph::Paragraph),
        ) {
            if let Some(c) = caption {
                c.walk_paragraphs_mut(f);
            }
        }
        match self {
            Self::TextBox { paragraphs, caption, .. }
            | Self::Ellipse { paragraphs, caption, .. }
            | Self::Polygon { paragraphs, caption, .. } => {
                walk_vec(paragraphs, f);
                walk_caption(caption, f);
            }
            Self::Footnote { paragraphs, .. } | Self::Endnote { paragraphs, .. } => {
                walk_vec(paragraphs, f);
            }
            Self::Line { caption, .. }
            | Self::Rect { caption, .. }
            | Self::Arc { caption, .. }
            | Self::Curve { caption, .. }
            | Self::ConnectLine { caption, .. } => walk_caption(caption, f),
            Self::Group { children, .. } => {
                for child in children {
                    child.walk_paragraphs_mut(f);
                }
            }
            Self::Memo { content, anchor_runs, .. } => {
                walk_vec(content, f);
                for run in anchor_runs {
                    run.walk_paragraphs_mut(f);
                }
            }
            Self::Hyperlink { .. }
            | Self::EmbeddedChart { .. }
            | Self::Equation { .. }
            | Self::Chart { .. }
            | Self::Dutmal { .. }
            | Self::Compose { .. }
            | Self::TextArt { .. }
            | Self::Bookmark { .. }
            | Self::CrossRef { .. }
            | Self::Field { .. }
            | Self::IndexMark { .. }
            | Self::UnknownSummary { .. }
            | Self::DateCodeField { .. }
            | Self::PathField { .. }
            | Self::InlinePageNumber { .. }
            | Self::NewNumber { .. }
            | Self::Unknown { .. } => {}
        }
    }

    /// Returns the stable snake_case name of this control's kind.
    ///
    /// Read/diff projections use this to label embedded content without
    /// exposing payload details. The match is deliberately exhaustive (no
    /// wildcard) so adding a variant forces a name here at compile time.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::TextBox { .. } => "text_box",
            Self::Hyperlink { .. } => "hyperlink",
            Self::Footnote { .. } => "footnote",
            Self::Endnote { .. } => "endnote",
            Self::Line { .. } => "line",
            Self::Ellipse { .. } => "ellipse",
            Self::EmbeddedChart { .. } => "embedded_chart",
            Self::Rect { .. } => "rect",
            Self::Polygon { .. } => "polygon",
            Self::Equation { .. } => "equation",
            Self::Chart { .. } => "chart",
            Self::Dutmal { .. } => "dutmal",
            Self::Compose { .. } => "compose",
            Self::Arc { .. } => "arc",
            Self::Curve { .. } => "curve",
            Self::ConnectLine { .. } => "connect_line",
            Self::Group { .. } => "group",
            Self::TextArt { .. } => "text_art",
            Self::Bookmark { .. } => "bookmark",
            Self::CrossRef { .. } => "cross_ref",
            Self::Field { .. } => "field",
            Self::Memo { .. } => "memo",
            Self::IndexMark { .. } => "index_mark",
            Self::UnknownSummary { .. } => "unknown_summary",
            Self::DateCodeField { .. } => "date_code_field",
            Self::PathField { .. } => "path_field",
            Self::InlinePageNumber { .. } => "inline_page_number",
            Self::NewNumber { .. } => "new_number",
            Self::Unknown { .. } => "unknown",
        }
    }

    /// Returns `true` if this is a [`Control::TextBox`].
    pub fn is_text_box(&self) -> bool {
        matches!(self, Self::TextBox { .. })
    }

    /// Returns `true` if this is a [`Control::Hyperlink`].
    pub fn is_hyperlink(&self) -> bool {
        matches!(self, Self::Hyperlink { .. })
    }

    /// Returns `true` if this is a [`Control::Footnote`].
    pub fn is_footnote(&self) -> bool {
        matches!(self, Self::Footnote { .. })
    }

    /// Returns `true` if this is a [`Control::Endnote`].
    pub fn is_endnote(&self) -> bool {
        matches!(self, Self::Endnote { .. })
    }

    /// Returns `true` if this is a [`Control::Line`].
    pub fn is_line(&self) -> bool {
        matches!(self, Self::Line { .. })
    }

    /// Returns `true` if this is a [`Control::Ellipse`].
    pub fn is_ellipse(&self) -> bool {
        matches!(self, Self::Ellipse { .. })
    }

    /// Returns `true` if this is a [`Control::Rect`].
    pub fn is_rect(&self) -> bool {
        matches!(self, Self::Rect { .. })
    }

    /// Returns `true` if this is a [`Control::Polygon`].
    pub fn is_polygon(&self) -> bool {
        matches!(self, Self::Polygon { .. })
    }

    /// Returns `true` if this is a [`Control::Equation`].
    pub fn is_equation(&self) -> bool {
        matches!(self, Self::Equation { .. })
    }

    /// Returns `true` if this is a [`Control::Chart`].
    pub fn is_chart(&self) -> bool {
        matches!(self, Self::Chart { .. })
    }

    /// Returns `true` if this is a [`Control::EmbeddedChart`].
    pub fn is_embedded_chart(&self) -> bool {
        matches!(self, Self::EmbeddedChart { .. })
    }

    /// Returns `true` if this is a [`Control::Unknown`].
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns `true` if this is a [`Control::Dutmal`].
    pub fn is_dutmal(&self) -> bool {
        matches!(self, Self::Dutmal { .. })
    }

    /// Returns `true` if this is a [`Control::Compose`].
    pub fn is_compose(&self) -> bool {
        matches!(self, Self::Compose { .. })
    }

    /// Returns `true` if this is a [`Control::Arc`].
    pub fn is_arc(&self) -> bool {
        matches!(self, Self::Arc { .. })
    }

    /// Returns `true` if this is a [`Control::Curve`].
    pub fn is_curve(&self) -> bool {
        matches!(self, Self::Curve { .. })
    }

    /// Returns `true` if this is a [`Control::ConnectLine`].
    pub fn is_connect_line(&self) -> bool {
        matches!(self, Self::ConnectLine { .. })
    }

    /// Returns `true` if this is a [`Control::Group`].
    pub fn is_group(&self) -> bool {
        matches!(self, Self::Group { .. })
    }

    /// Returns `true` if this is a [`Control::Bookmark`].
    pub fn is_bookmark(&self) -> bool {
        matches!(self, Self::Bookmark { .. })
    }

    /// Returns `true` if this is a [`Control::CrossRef`].
    pub fn is_cross_ref(&self) -> bool {
        matches!(self, Self::CrossRef { .. })
    }

    /// Returns `true` if this is a [`Control::Field`].
    pub fn is_field(&self) -> bool {
        matches!(self, Self::Field { .. })
    }

    /// Returns `true` if this is a [`Control::Memo`].
    pub fn is_memo(&self) -> bool {
        matches!(self, Self::Memo { .. })
    }

    /// Returns `true` if this is a [`Control::IndexMark`].
    pub fn is_index_mark(&self) -> bool {
        matches!(self, Self::IndexMark { .. })
    }

    /// Creates a point bookmark at a named location.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    ///
    /// let bm = Control::bookmark("section1");
    /// assert!(bm.is_bookmark());
    /// ```
    pub fn bookmark(name: &str) -> Self {
        Self::Bookmark { name: name.to_string(), bookmark_type: BookmarkType::Point }
    }

    /// Creates a press-field (누름틀) with the given hint text.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    ///
    /// let field = Control::field("이름을 입력하세요");
    /// assert!(field.is_field());
    /// ```
    pub fn field(hint: &str) -> Self {
        Self::Field {
            field_type: FieldType::ClickHere,
            hint_text: Some(hint.to_string()),
            help_text: None,
            name: None,
            display_text: String::new(),
        }
    }

    /// Creates an index mark with a primary key.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    ///
    /// let mark = Control::index_mark("한글");
    /// assert!(mark.is_index_mark());
    /// ```
    pub fn index_mark(primary: &str) -> Self {
        Self::IndexMark { primary: primary.to_string(), secondary: None }
    }

    /// Creates a memo annotation with the given paragraph body.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::ParaShapeIndex;
    ///
    /// let para = Paragraph::new(ParaShapeIndex::new(0));
    /// let memo = Control::memo(vec![para]);
    /// assert!(memo.is_memo());
    /// ```
    pub fn memo(content: Vec<Paragraph>) -> Self {
        Self::Memo { content, anchor_runs: Vec::new(), metadata: MemoMetadata::default() }
    }

    /// Creates a memo annotation with both body content and anchor runs.
    ///
    /// `anchor_runs` are the visible body span the memo is attached to (the
    /// text between HWPX `<hp:fieldBegin type="MEMO">` and `<hp:fieldEnd>`);
    /// `content` is the memo body inside `<hp:subList>`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_core::run::Run;
    /// use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    ///
    /// let body = vec![Paragraph::new(ParaShapeIndex::new(0))];
    /// let anchor = vec![Run::text("hello", CharShapeIndex::new(0))];
    /// let memo = Control::memo_with_anchor(body, anchor);
    /// assert!(memo.is_memo());
    /// ```
    pub fn memo_with_anchor(content: Vec<Paragraph>, anchor_runs: Vec<Run>) -> Self {
        Self::Memo { content, anchor_runs, metadata: MemoMetadata::default() }
    }

    /// Creates a cross-reference to a bookmark target (convenience helper).
    ///
    /// Wave 12m Phase 2: 인자 타입이 `&str` 에서 `RefTarget` 로 변경
    /// (breaking). 책갈피 이름이라면 `RefTarget::Name(...)`, 한컴 시스템
    /// ID 라면 `RefTarget::SystemId(...)` 를 명시.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::{Control, RefTarget};
    /// use hwpforge_foundation::{RefType, RefContentType};
    ///
    /// let xref = Control::cross_ref(
    ///     RefTarget::Name("section1".to_string()),
    ///     RefType::Bookmark,
    ///     RefContentType::Page,
    /// );
    /// assert!(xref.is_cross_ref());
    /// ```
    pub fn cross_ref(target: RefTarget, ref_type: RefType, content_type: RefContentType) -> Self {
        Self::CrossRef {
            target,
            ref_type,
            content_type,
            as_hyperlink: false,
            display_text: String::new(),
        }
    }

    /// Creates a chart control with default dimensions and settings.
    ///
    /// Defaults: width ≈ 114mm, height ≈ 66mm, no title, right legend, clustered grouping.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_core::chart::{ChartType, ChartData};
    ///
    /// let data = ChartData::category(&["A", "B"], &[("S1", &[10.0, 20.0])]);
    /// let ctrl = Control::chart(ChartType::Column, data);
    /// assert!(ctrl.is_chart());
    /// ```
    pub fn chart(chart_type: ChartType, data: ChartData) -> Self {
        Self::Chart {
            chart_type,
            data,
            width: HwpUnit::new(32250).expect("32250 is valid"),
            height: HwpUnit::new(18750).expect("18750 is valid"),
            title: None,
            legend: LegendPosition::default(),
            grouping: ChartGrouping::default(),
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        }
    }

    /// Creates an equation control with default dimensions for the given HancomEQN script.
    ///
    /// Defaults: width ≈ 31mm (8779 HWPUNIT), height ≈ 9.2mm (2600 HWPUNIT),
    /// baseline 71%, black text, `HancomEQN` font.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    ///
    /// let ctrl = Control::equation("{a+b} over {c+d}");
    /// assert!(ctrl.is_equation());
    /// ```
    pub fn equation(script: &str) -> Self {
        Self::Equation {
            script: script.to_string(),
            width: HwpUnit::new(8779).expect("8779 is valid"),
            height: HwpUnit::new(2600).expect("2600 is valid"),
            base_line: 71,
            text_color: Color::BLACK,
            font: "HancomEQN".to_string(),
            inst_id: None,
        }
    }

    /// Creates a text box control with the given paragraphs and dimensions.
    ///
    /// Defaults: inline positioning (horz_offset=0, vert_offset=0), no caption, no style override.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
    ///
    /// let para = Paragraph::new(ParaShapeIndex::new(0));
    /// let width = HwpUnit::from_mm(80.0).unwrap();
    /// let height = HwpUnit::from_mm(40.0).unwrap();
    /// let ctrl = Control::text_box(vec![para], width, height);
    /// assert!(ctrl.is_text_box());
    /// ```
    pub fn text_box(paragraphs: Vec<Paragraph>, width: HwpUnit, height: HwpUnit) -> Self {
        Self::TextBox {
            paragraphs,
            width,
            height,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        }
    }

    /// Creates a footnote control with the given paragraph content.
    ///
    /// Defaults: no inst_id.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_core::run::Run;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    ///
    /// let para = Paragraph::with_runs(
    ///     vec![Run::text("Note text", CharShapeIndex::new(0))],
    ///     ParaShapeIndex::new(0),
    /// );
    /// let ctrl = Control::footnote(vec![para]);
    /// assert!(ctrl.is_footnote());
    /// ```
    pub fn footnote(paragraphs: Vec<Paragraph>) -> Self {
        Self::Footnote { inst_id: None, paragraphs }
    }

    /// Creates an endnote control with the given paragraph content.
    ///
    /// Defaults: no inst_id.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_core::run::Run;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    ///
    /// let para = Paragraph::with_runs(
    ///     vec![Run::text("End note", CharShapeIndex::new(0))],
    ///     ParaShapeIndex::new(0),
    /// );
    /// let ctrl = Control::endnote(vec![para]);
    /// assert!(ctrl.is_endnote());
    /// ```
    pub fn endnote(paragraphs: Vec<Paragraph>) -> Self {
        Self::Endnote { inst_id: None, paragraphs }
    }

    /// Creates a footnote with an explicit instance ID for cross-referencing.
    ///
    /// Use this when you need stable `inst_id` references (e.g. matching decoder output).
    /// For simple footnotes without cross-references, prefer [`Control::footnote`].
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::ParaShapeIndex;
    ///
    /// let ctrl = Control::footnote_with_id(1, vec![Paragraph::new(ParaShapeIndex::new(0))]);
    /// assert!(ctrl.is_footnote());
    /// ```
    pub fn footnote_with_id(inst_id: u64, paragraphs: Vec<Paragraph>) -> Self {
        Self::Footnote { inst_id: Some(ObjectId::new(inst_id)), paragraphs }
    }

    /// Creates an endnote with an explicit instance ID for cross-referencing.
    ///
    /// Use this when you need stable `inst_id` references (e.g. matching decoder output).
    /// For simple endnotes without cross-references, prefer [`Control::endnote`].
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::ParaShapeIndex;
    ///
    /// let ctrl = Control::endnote_with_id(2, vec![Paragraph::new(ParaShapeIndex::new(0))]);
    /// assert!(ctrl.is_endnote());
    /// ```
    pub fn endnote_with_id(inst_id: u64, paragraphs: Vec<Paragraph>) -> Self {
        Self::Endnote { inst_id: Some(ObjectId::new(inst_id)), paragraphs }
    }

    /// Creates an ellipse control with the given bounding box dimensions.
    ///
    /// Geometry is auto-derived: center=(w/2, h/2), axis1=(w, h/2), axis2=(w/2, h).
    /// Defaults: inline positioning (horz_offset=0, vert_offset=0), no paragraphs, no caption, no style.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_foundation::HwpUnit;
    ///
    /// let width = HwpUnit::from_mm(40.0).unwrap();
    /// let height = HwpUnit::from_mm(30.0).unwrap();
    /// let ctrl = Control::ellipse(width, height);
    /// assert!(ctrl.is_ellipse());
    /// ```
    pub fn ellipse(width: HwpUnit, height: HwpUnit) -> Self {
        let w = width.as_i32();
        let h = height.as_i32();
        Self::Ellipse {
            center: ShapePoint::new(w / 2, h / 2),
            axis1: ShapePoint::new(w, h / 2),
            axis2: ShapePoint::new(w / 2, h),
            width,
            height,
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        }
    }

    /// Creates an ellipse control with paragraph content inside.
    ///
    /// Same as [`Control::ellipse`] but accepts paragraphs for text drawn inside the ellipse.
    /// Geometry is auto-derived: center=(w/2, h/2), axis1=(w, h/2), axis2=(w/2, h).
    /// Defaults: inline positioning (horz_offset=0, vert_offset=0), no caption, no style.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_core::paragraph::Paragraph;
    /// use hwpforge_foundation::{HwpUnit, ParaShapeIndex};
    ///
    /// let width = HwpUnit::from_mm(40.0).unwrap();
    /// let height = HwpUnit::from_mm(30.0).unwrap();
    /// let para = Paragraph::new(ParaShapeIndex::new(0));
    /// let ctrl = Control::ellipse_with_text(width, height, vec![para]);
    /// assert!(ctrl.is_ellipse());
    /// ```
    pub fn ellipse_with_text(width: HwpUnit, height: HwpUnit, paragraphs: Vec<Paragraph>) -> Self {
        let w = width.as_i32();
        let h = height.as_i32();
        Self::Ellipse {
            center: ShapePoint::new(w / 2, h / 2),
            axis1: ShapePoint::new(w, h / 2),
            axis2: ShapePoint::new(w / 2, h),
            width,
            height,
            horz_offset: 0,
            vert_offset: 0,
            paragraphs,
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        }
    }

    /// Creates a pure rectangle control with the given bounding box dimensions.
    ///
    /// Pure rectangle means no embedded text content; for a textbox-style rect with
    /// inline paragraphs, use [`Control::text_box`].
    /// Defaults: inline positioning (horz_offset=0, vert_offset=0), no caption, no style.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidStructure`] if either dimension is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_foundation::HwpUnit;
    ///
    /// let width = HwpUnit::from_mm(40.0).unwrap();
    /// let height = HwpUnit::from_mm(20.0).unwrap();
    /// let ctrl = Control::rect(width, height).unwrap();
    /// assert!(ctrl.is_rect());
    /// ```
    pub fn rect(width: HwpUnit, height: HwpUnit) -> CoreResult<Self> {
        if width.as_i32() == 0 || height.as_i32() == 0 {
            return Err(CoreError::InvalidStructure {
                context: "Control::rect".to_string(),
                reason: format!(
                    "rectangle requires non-zero dimensions, got {}x{}",
                    width.as_i32(),
                    height.as_i32()
                ),
            });
        }
        Ok(Self::Rect { width, height, horz_offset: 0, vert_offset: 0, caption: None, style: None })
    }

    /// Creates a polygon control from the given vertices.
    ///
    /// The bounding box is auto-derived from the min/max of vertex coordinates.
    /// Defaults: no paragraphs, no caption, no style.
    ///
    /// Returns an error if fewer than 3 vertices are provided.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidStructure`] if `vertices.len() < 3`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::{Control, ShapePoint};
    ///
    /// let vertices = vec![
    ///     ShapePoint::new(0, 1000),
    ///     ShapePoint::new(500, 0),
    ///     ShapePoint::new(1000, 1000),
    /// ];
    /// let ctrl = Control::polygon(vertices).unwrap();
    /// assert!(ctrl.is_polygon());
    /// ```
    pub fn polygon(vertices: Vec<ShapePoint>) -> CoreResult<Self> {
        if vertices.len() < 3 {
            return Err(CoreError::InvalidStructure {
                context: "Control::polygon".to_string(),
                reason: format!("polygon requires at least 3 vertices, got {}", vertices.len()),
            });
        }
        let min_x = vertices.iter().map(|p| p.x as i64).min().unwrap_or(0);
        let max_x = vertices.iter().map(|p| p.x as i64).max().unwrap_or(0);
        let min_y = vertices.iter().map(|p| p.y as i64).min().unwrap_or(0);
        let max_y = vertices.iter().map(|p| p.y as i64).max().unwrap_or(0);
        let bbox_w = i32::try_from((max_x - min_x).max(0)).unwrap_or(i32::MAX);
        let bbox_h = i32::try_from((max_y - min_y).max(0)).unwrap_or(i32::MAX);
        let width = HwpUnit::new(bbox_w).map_err(|_| CoreError::InvalidStructure {
            context: "Control::polygon".into(),
            reason: format!("bounding box width {bbox_w} exceeds HwpUnit range"),
        })?;
        let height = HwpUnit::new(bbox_h).map_err(|_| CoreError::InvalidStructure {
            context: "Control::polygon".into(),
            reason: format!("bounding box height {bbox_h} exceeds HwpUnit range"),
        })?;
        Ok(Self::Polygon {
            vertices,
            width,
            height,
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        })
    }

    /// Creates a line control between two endpoints.
    ///
    /// The bounding box width and height are derived from the absolute difference
    /// of the endpoint coordinates: `width = |end.x - start.x|`, `height = |end.y - start.y|`.
    /// Each axis is clamped to a minimum of 100 HwpUnit (~1pt) because 한글 cannot
    /// render lines with a zero-dimension bounding box.
    /// Defaults: no caption, no style.
    ///
    /// Returns an error if start and end are the same point (degenerate line).
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidStructure`] if start equals end.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::{Control, ShapePoint};
    ///
    /// let ctrl = Control::line(ShapePoint::new(0, 0), ShapePoint::new(5000, 0)).unwrap();
    /// assert!(ctrl.is_line());
    /// ```
    pub fn line(start: ShapePoint, end: ShapePoint) -> CoreResult<Self> {
        if start == end {
            return Err(CoreError::InvalidStructure {
                context: "Control::line".to_string(),
                reason: "start and end points are identical (degenerate line)".to_string(),
            });
        }
        // Normalize points to bounding-box-relative coordinates.
        // HWPX requires startPt/endPt within the shape's bounding box (0,0)→(w,h).
        let min_x = start.x.min(end.x);
        let min_y = start.y.min(end.y);
        let norm_start =
            ShapePoint::new(start.x.saturating_sub(min_x), start.y.saturating_sub(min_y));
        let norm_end = ShapePoint::new(end.x.saturating_sub(min_x), end.y.saturating_sub(min_y));

        let raw_w =
            i32::try_from(((end.x as i64) - (start.x as i64)).unsigned_abs()).unwrap_or(i32::MAX);
        let raw_h =
            i32::try_from(((end.y as i64) - (start.y as i64)).unsigned_abs()).unwrap_or(i32::MAX);
        // Minimum bounding box of 100 HwpUnit (~1pt) per axis.
        // 한글 cannot render lines with a zero-dimension bounding box.
        let raw_w = raw_w.max(100);
        let raw_h = raw_h.max(100);
        let width = HwpUnit::new(raw_w).unwrap_or_else(|_| HwpUnit::new(100).expect("valid"));
        let height = HwpUnit::new(raw_h).unwrap_or_else(|_| HwpUnit::new(100).expect("valid"));
        Ok(Self::Line {
            start: norm_start,
            end: norm_end,
            width,
            height,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        })
    }

    /// Creates a horizontal line of the given width.
    ///
    /// Shortcut for `line(ShapePoint::new(0, 0), ShapePoint::new(width.as_i32(), 0))`.
    /// The bounding box height is clamped to 100 HwpUnit (~1pt minimum) because
    /// 한글 cannot render lines with a zero-dimension bounding box.
    /// Defaults: no caption, no style.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_foundation::HwpUnit;
    ///
    /// let width = HwpUnit::from_mm(100.0).unwrap();
    /// let ctrl = Control::horizontal_line(width);
    /// assert!(ctrl.is_line());
    /// ```
    pub fn horizontal_line(width: HwpUnit) -> Self {
        let w = width.as_i32();
        Self::Line {
            start: ShapePoint::new(0, 0),
            end: ShapePoint::new(w, 0),
            width,
            height: HwpUnit::new(100).expect("100 is valid"),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        }
    }

    /// Creates a dutmal (annotation text) control with default positioning.
    ///
    /// Defaults: position = Top, sz_ratio = 0 (auto), align = Center.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    ///
    /// let ctrl = Control::dutmal("본문", "주석");
    /// assert!(ctrl.is_dutmal());
    /// ```
    pub fn dutmal(main_text: impl Into<String>, sub_text: impl Into<String>) -> Self {
        Self::Dutmal {
            main_text: main_text.into(),
            sub_text: sub_text.into(),
            position: DutmalPosition::Top,
            sz_ratio: 0,
            align: DutmalAlign::Center,
            metadata: DutmalMetadata::default(),
        }
    }

    /// Creates a compose (글자겹침) control with default settings.
    ///
    /// Defaults: `circle_type = "SHAPE_REVERSAL_TIRANGLE"` (spec typo preserved),
    /// `char_sz = -3`, `compose_type = "SPREAD"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    ///
    /// let ctrl = Control::compose("12");
    /// assert!(ctrl.is_compose());
    /// ```
    pub fn compose(text: impl Into<String>) -> Self {
        Self::Compose {
            compose_text: text.into(),
            circle_type: "SHAPE_REVERSAL_TIRANGLE".to_string(), // official spec typo preserved
            char_sz: -3,
            compose_type: "SPREAD".to_string(),
            // 10 × no-override sentinel (HWPX `charPrCnt` is fixed at 10).
            char_pr_ids: vec![u32::MAX; 10],
        }
    }

    /// Creates an arc control with the given bounding box dimensions.
    ///
    /// Geometry is auto-derived from the bounding box.
    /// Defaults: inline positioning, no caption, no style.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    /// use hwpforge_foundation::{ArcType, HwpUnit};
    ///
    /// let width = HwpUnit::from_mm(40.0).unwrap();
    /// let height = HwpUnit::from_mm(30.0).unwrap();
    /// let ctrl = Control::arc(ArcType::Pie, width, height);
    /// assert!(ctrl.is_arc());
    /// ```
    pub fn arc(arc_type: ArcType, width: HwpUnit, height: HwpUnit) -> Self {
        let w = width.as_i32();
        let h = height.as_i32();
        Self::Arc {
            arc_type,
            center: ShapePoint::new(w / 2, h / 2),
            axis1: ShapePoint::new(w, h / 2),
            axis2: ShapePoint::new(w / 2, h),
            start1: ShapePoint::new(w, h / 2),
            end1: ShapePoint::new(w / 2, 0),
            start2: ShapePoint::new(w, h / 2),
            end2: ShapePoint::new(w / 2, 0),
            width,
            height,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        }
    }

    /// Creates a curve control from the given control points.
    ///
    /// All segments default to [`CurveSegmentType::Curve`].
    /// The bounding box is auto-derived from min/max of point coordinates.
    ///
    /// Returns an error if fewer than 2 points are provided.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidStructure`] if `points.len() < 2`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::{Control, ShapePoint};
    ///
    /// let pts = vec![
    ///     ShapePoint::new(0, 0),
    ///     ShapePoint::new(2500, 5000),
    ///     ShapePoint::new(5000, 0),
    /// ];
    /// let ctrl = Control::curve(pts).unwrap();
    /// assert!(ctrl.is_curve());
    /// ```
    pub fn curve(points: Vec<ShapePoint>) -> CoreResult<Self> {
        if points.len() < 2 {
            return Err(CoreError::InvalidStructure {
                context: "Control::curve".to_string(),
                reason: format!("curve requires at least 2 points, got {}", points.len()),
            });
        }
        let min_x = points.iter().map(|p| p.x as i64).min().unwrap_or(0);
        let max_x = points.iter().map(|p| p.x as i64).max().unwrap_or(0);
        let min_y = points.iter().map(|p| p.y as i64).min().unwrap_or(0);
        let max_y = points.iter().map(|p| p.y as i64).max().unwrap_or(0);
        let bbox_w = i32::try_from((max_x - min_x).max(1)).unwrap_or(i32::MAX);
        let bbox_h = i32::try_from((max_y - min_y).max(1)).unwrap_or(i32::MAX);
        let width = HwpUnit::new(bbox_w).map_err(|_| CoreError::InvalidStructure {
            context: "Control::curve".into(),
            reason: format!("bounding box width {bbox_w} exceeds HwpUnit range"),
        })?;
        let height = HwpUnit::new(bbox_h).map_err(|_| CoreError::InvalidStructure {
            context: "Control::curve".into(),
            reason: format!("bounding box height {bbox_h} exceeds HwpUnit range"),
        })?;
        let seg_count = points.len().saturating_sub(1);
        Ok(Self::Curve {
            points,
            segment_types: vec![CurveSegmentType::Curve; seg_count],
            width,
            height,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        })
    }

    /// Creates a connect line between two endpoints.
    ///
    /// Defaults: no control points, type "STRAIGHT", no caption, no style.
    ///
    /// Returns an error if start equals end.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidStructure`] if start equals end.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::{Control, ShapePoint};
    ///
    /// let ctrl = Control::connect_line(
    ///     ShapePoint::new(0, 0),
    ///     ShapePoint::new(5000, 5000),
    /// ).unwrap();
    /// assert!(ctrl.is_connect_line());
    /// ```
    pub fn connect_line(start: ShapePoint, end: ShapePoint) -> CoreResult<Self> {
        if start == end {
            return Err(CoreError::InvalidStructure {
                context: "Control::connect_line".to_string(),
                reason: "start and end points are identical (degenerate line)".to_string(),
            });
        }
        // Normalize points to bounding-box-relative coordinates.
        // HWPX requires startPt/endPt within the shape's bounding box (0,0)→(w,h).
        let min_x = start.x.min(end.x);
        let min_y = start.y.min(end.y);
        let norm_start =
            ShapePoint::new(start.x.saturating_sub(min_x), start.y.saturating_sub(min_y));
        let norm_end = ShapePoint::new(end.x.saturating_sub(min_x), end.y.saturating_sub(min_y));

        let raw_w =
            i32::try_from(((end.x as i64) - (start.x as i64)).unsigned_abs()).unwrap_or(i32::MAX);
        let raw_h =
            i32::try_from(((end.y as i64) - (start.y as i64)).unsigned_abs()).unwrap_or(i32::MAX);
        let raw_w = raw_w.max(100);
        let raw_h = raw_h.max(100);
        let width = HwpUnit::new(raw_w).unwrap_or_else(|_| HwpUnit::new(100).expect("valid"));
        let height = HwpUnit::new(raw_h).unwrap_or_else(|_| HwpUnit::new(100).expect("valid"));
        Ok(Self::ConnectLine {
            start: norm_start,
            end: norm_end,
            control_points: Vec::new(),
            connect_type: "STRAIGHT".to_string(),
            width,
            height,
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        })
    }

    /// Creates a hyperlink control with the given display text and URL.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::control::Control;
    ///
    /// let ctrl = Control::hyperlink("Visit Rust", "https://rust-lang.org");
    /// assert!(ctrl.is_hyperlink());
    /// ```
    pub fn hyperlink(text: &str, url: &str) -> Self {
        Self::Hyperlink { text: text.to_string(), url: url.to_string() }
    }
}

impl std::fmt::Display for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TextBox { paragraphs, .. } => {
                let n = paragraphs.len();
                let word = if n == 1 { "paragraph" } else { "paragraphs" };
                write!(f, "TextBox({n} {word})")
            }
            Self::Hyperlink { text, url } => {
                let preview: String =
                    if text.len() > 30 { text.chars().take(30).collect() } else { text.clone() };
                write!(f, "Hyperlink(\"{preview}\" -> {url})")
            }
            Self::Footnote { paragraphs, .. } => {
                let n = paragraphs.len();
                let word = if n == 1 { "paragraph" } else { "paragraphs" };
                write!(f, "Footnote({n} {word})")
            }
            Self::Endnote { paragraphs, .. } => {
                let n = paragraphs.len();
                let word = if n == 1 { "paragraph" } else { "paragraphs" };
                write!(f, "Endnote({n} {word})")
            }
            Self::Line { .. } => {
                write!(f, "Line")
            }
            Self::Ellipse { paragraphs, .. } => {
                let n = paragraphs.len();
                let word = if n == 1 { "paragraph" } else { "paragraphs" };
                write!(f, "Ellipse({n} {word})")
            }
            Self::Rect { width, height, .. } => {
                write!(f, "Rect({}x{})", width.as_i32(), height.as_i32())
            }
            Self::Polygon { vertices, paragraphs, .. } => {
                let nv = vertices.len();
                let np = paragraphs.len();
                let vw = if nv == 1 { "vertex" } else { "vertices" };
                let pw = if np == 1 { "paragraph" } else { "paragraphs" };
                write!(f, "Polygon({nv} {vw}, {np} {pw})")
            }
            Self::Chart { chart_type, data, .. } => {
                let series_count = match data {
                    ChartData::Category { series, .. } => series.len(),
                    ChartData::Xy { series } => series.len(),
                };
                write!(f, "Chart({chart_type:?}, {series_count} series)")
            }
            Self::EmbeddedChart { chart_xml, ole_bytes, width, height, .. } => {
                write!(
                    f,
                    "EmbeddedChart(xml={} bytes, ole={} bytes, {}x{})",
                    chart_xml.len(),
                    ole_bytes.len(),
                    width.as_i32(),
                    height.as_i32()
                )
            }
            Self::Equation { script, .. } => {
                let preview: String = if script.len() > 30 {
                    script.chars().take(30).collect()
                } else {
                    script.clone()
                };
                write!(f, "Equation(\"{preview}\")")
            }
            Self::Dutmal { main_text, sub_text, .. } => {
                write!(f, "Dutmal(\"{main_text}\" / \"{sub_text}\")")
            }
            Self::Compose { compose_text, .. } => {
                write!(f, "Compose(\"{compose_text}\")")
            }
            Self::Arc { arc_type, .. } => {
                write!(f, "Arc({arc_type})")
            }
            Self::Curve { points, .. } => {
                write!(f, "Curve({} points)", points.len())
            }
            Self::ConnectLine { .. } => {
                write!(f, "ConnectLine")
            }
            Self::Group { children, .. } => {
                write!(f, "Group({} children)", children.len())
            }
            Self::TextArt { text, shape, .. } => {
                write!(f, "TextArt(\"{text}\", {shape})")
            }
            Self::Bookmark { name, bookmark_type } => {
                write!(f, "Bookmark(\"{name}\", {bookmark_type})")
            }
            Self::CrossRef { target, ref_type, .. } => {
                write!(f, "CrossRef({:?}, {ref_type})", target.as_display())
            }
            Self::Field { field_type, hint_text, name, .. } => {
                let hint = hint_text.as_deref().unwrap_or("");
                match name.as_deref().filter(|s| !s.is_empty()) {
                    Some(n) => write!(f, "Field({field_type}, name=\"{n}\", \"{hint}\")"),
                    None => write!(f, "Field({field_type}, \"{hint}\")"),
                }
            }
            Self::Memo { content, anchor_runs, .. } => {
                let n = content.len();
                let word = if n == 1 { "paragraph" } else { "paragraphs" };
                let anchor_len = anchor_runs.len();
                write!(f, "Memo({n} {word}, anchor={anchor_len} runs)")
            }
            Self::IndexMark { primary, secondary } => {
                if let Some(sec) = secondary {
                    write!(f, "IndexMark(\"{primary}\" / \"{sec}\")")
                } else {
                    write!(f, "IndexMark(\"{primary}\")")
                }
            }
            Self::UnknownSummary { token, .. } => {
                write!(f, "UnknownSummary({token})")
            }
            Self::DateCodeField { is_time_mode, .. } => {
                let mode = if *is_time_mode { "time" } else { "date" };
                write!(f, "DateCodeField({mode})")
            }
            Self::PathField { command, .. } => {
                write!(f, "PathField({})", command.wire_command())
            }
            Self::InlinePageNumber { kind } => match kind {
                InlinePageKind::CurrentPage => write!(f, "InlinePageNumber(current)"),
                InlinePageKind::TotalPages => write!(f, "InlinePageNumber(total)"),
                InlinePageKind::Unknown => write!(f, "InlinePageNumber(unknown)"),
            },
            Self::NewNumber { kind, number } => {
                write!(f, "NewNumber({kind:?}, {number})")
            }
            Self::Unknown { tag, .. } => {
                write!(f, "Unknown({tag})")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::Run;
    use hwpforge_foundation::{CharShapeIndex, Color, ParaShapeIndex, VerticalAlign};

    fn simple_paragraph() -> Paragraph {
        Paragraph::with_runs(
            vec![Run::text("footnote text", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )
    }

    #[test]
    fn shape_style_default_all_none() {
        let s = ShapeStyle::default();
        assert!(s.line_color.is_none());
        assert!(s.fill_color.is_none());
        assert!(s.line_width.is_none());
        assert!(s.line_style.is_none());
    }

    #[test]
    fn shape_style_with_typed_fields() {
        let s = ShapeStyle {
            line_color: Some(Color::from_rgb(255, 0, 0)),
            fill_color: Some(Color::from_rgb(0, 255, 0)),
            line_width: Some(100),
            line_style: Some(LineStyle::Dash),
            ..Default::default()
        };
        assert_eq!(s.line_color.unwrap(), Color::from_rgb(255, 0, 0));
        assert_eq!(s.fill_color.unwrap(), Color::from_rgb(0, 255, 0));
        assert_eq!(s.line_width.unwrap(), 100);
        assert_eq!(s.line_style.unwrap(), LineStyle::Dash);
    }

    #[test]
    fn line_style_default() {
        assert_eq!(LineStyle::default(), LineStyle::Solid);
    }

    #[test]
    fn line_style_display() {
        assert_eq!(LineStyle::Solid.to_string(), "SOLID");
        assert_eq!(LineStyle::Dash.to_string(), "DASH");
        assert_eq!(LineStyle::Dot.to_string(), "DOT");
        assert_eq!(LineStyle::DashDot.to_string(), "DASH_DOT");
        assert_eq!(LineStyle::DashDotDot.to_string(), "DASH_DOT_DOT");
        assert_eq!(LineStyle::None.to_string(), "NONE");
    }

    #[test]
    fn line_style_from_str() {
        assert_eq!("SOLID".parse::<LineStyle>().unwrap(), LineStyle::Solid);
        assert_eq!("Dash".parse::<LineStyle>().unwrap(), LineStyle::Dash);
        assert_eq!("dot".parse::<LineStyle>().unwrap(), LineStyle::Dot);
        assert_eq!("DASH_DOT".parse::<LineStyle>().unwrap(), LineStyle::DashDot);
        assert_eq!("DashDotDot".parse::<LineStyle>().unwrap(), LineStyle::DashDotDot);
        assert_eq!("NONE".parse::<LineStyle>().unwrap(), LineStyle::None);
        assert!("INVALID".parse::<LineStyle>().is_err());
    }

    #[test]
    fn line_style_serde_roundtrip() {
        for style in [
            LineStyle::Solid,
            LineStyle::Dash,
            LineStyle::Dot,
            LineStyle::DashDot,
            LineStyle::DashDotDot,
            LineStyle::None,
        ] {
            let json = serde_json::to_string(&style).unwrap();
            let back: LineStyle = serde_json::from_str(&json).unwrap();
            assert_eq!(style, back);
        }
    }

    #[test]
    fn text_box_construction() {
        let ctrl = Control::TextBox {
            paragraphs: vec![simple_paragraph()],
            width: HwpUnit::from_mm(80.0).unwrap(),
            height: HwpUnit::from_mm(40.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        assert!(ctrl.is_text_box());
        assert!(!ctrl.is_hyperlink());
        assert!(!ctrl.is_footnote());
        assert!(!ctrl.is_endnote());
        assert!(!ctrl.is_unknown());
    }

    #[test]
    fn hyperlink_construction() {
        let ctrl = Control::Hyperlink {
            text: "Click".to_string(),
            url: "https://example.com".to_string(),
        };
        assert!(ctrl.is_hyperlink());
        assert!(!ctrl.is_text_box());
    }

    #[test]
    fn footnote_construction() {
        let ctrl = Control::Footnote { inst_id: None, paragraphs: vec![simple_paragraph()] };
        assert!(ctrl.is_footnote());
        assert!(!ctrl.is_text_box());
        assert!(!ctrl.is_endnote());
    }

    #[test]
    fn endnote_construction() {
        let ctrl = Control::Endnote {
            inst_id: Some(ObjectId::new(123456)),
            paragraphs: vec![simple_paragraph()],
        };
        assert!(ctrl.is_endnote());
        assert!(!ctrl.is_footnote());
        assert!(!ctrl.is_text_box());
    }

    #[test]
    fn unknown_construction() {
        let ctrl = Control::Unknown {
            tag: "custom:widget".to_string(),
            data: Some("<data>value</data>".to_string()),
        };
        assert!(ctrl.is_unknown());
    }

    #[test]
    fn unknown_without_data() {
        let ctrl = Control::Unknown { tag: "header".to_string(), data: None };
        assert!(ctrl.is_unknown());
    }

    #[test]
    fn display_text_box() {
        let ctrl = Control::TextBox {
            paragraphs: vec![simple_paragraph(), simple_paragraph()],
            width: HwpUnit::from_mm(80.0).unwrap(),
            height: HwpUnit::from_mm(40.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        assert_eq!(ctrl.to_string(), "TextBox(2 paragraphs)");
    }

    #[test]
    fn display_hyperlink() {
        let ctrl =
            Control::Hyperlink { text: "Short".to_string(), url: "https://x.com".to_string() };
        let s = ctrl.to_string();
        assert!(s.contains("Short"), "display: {s}");
        assert!(s.contains("https://x.com"), "display: {s}");
    }

    #[test]
    fn display_hyperlink_long_text_truncated() {
        let ctrl =
            Control::Hyperlink { text: "A".repeat(100), url: "https://example.com".to_string() };
        let s = ctrl.to_string();
        // Should show first 30 chars
        assert!(s.contains(&"A".repeat(30)), "display: {s}");
        assert!(!s.contains(&"A".repeat(31)), "display: {s}");
    }

    #[test]
    fn display_footnote() {
        let ctrl = Control::Footnote { inst_id: None, paragraphs: vec![simple_paragraph()] };
        assert_eq!(ctrl.to_string(), "Footnote(1 paragraph)");
    }

    #[test]
    fn display_endnote() {
        let ctrl = Control::Endnote {
            inst_id: Some(ObjectId::new(999)),
            paragraphs: vec![simple_paragraph()],
        };
        assert_eq!(ctrl.to_string(), "Endnote(1 paragraph)");
    }

    #[test]
    fn display_unknown() {
        let ctrl = Control::Unknown { tag: "bookmark".to_string(), data: None };
        assert_eq!(ctrl.to_string(), "Unknown(bookmark)");
    }

    #[test]
    fn equality() {
        let a = Control::Hyperlink { text: "A".to_string(), url: "B".to_string() };
        let b = Control::Hyperlink { text: "A".to_string(), url: "B".to_string() };
        let c = Control::Hyperlink { text: "A".to_string(), url: "C".to_string() };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn serde_roundtrip_text_box() {
        let ctrl = Control::TextBox {
            paragraphs: vec![simple_paragraph()],
            width: HwpUnit::from_mm(80.0).unwrap(),
            height: HwpUnit::from_mm(40.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn serde_roundtrip_hyperlink() {
        let ctrl = Control::Hyperlink {
            text: "link text".to_string(),
            url: "https://rust-lang.org".to_string(),
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn serde_roundtrip_footnote() {
        let ctrl = Control::Footnote {
            inst_id: Some(ObjectId::new(12345)),
            paragraphs: vec![simple_paragraph()],
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn serde_roundtrip_endnote() {
        let ctrl = Control::Endnote { inst_id: None, paragraphs: vec![simple_paragraph()] };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn serde_roundtrip_unknown() {
        let ctrl = Control::Unknown { tag: "test".to_string(), data: Some("payload".to_string()) };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    // ── Shape variant tests ──────────────────────────────────────

    #[test]
    fn line_construction() {
        let ctrl = Control::Line {
            start: ShapePoint { x: 0, y: 0 },
            end: ShapePoint { x: 1000, y: 500 },
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(25.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        assert!(ctrl.is_line());
        assert!(!ctrl.is_text_box());
        assert!(!ctrl.is_ellipse());
        assert!(!ctrl.is_polygon());
    }

    #[test]
    fn ellipse_construction() {
        let ctrl = Control::Ellipse {
            center: ShapePoint { x: 500, y: 500 },
            axis1: ShapePoint { x: 1000, y: 500 },
            axis2: ShapePoint { x: 500, y: 1000 },
            width: HwpUnit::from_mm(40.0).unwrap(),
            height: HwpUnit::from_mm(30.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        assert!(ctrl.is_ellipse());
        assert!(!ctrl.is_line());
        assert!(!ctrl.is_polygon());
    }

    #[test]
    fn ellipse_with_paragraphs() {
        let ctrl = Control::Ellipse {
            center: ShapePoint { x: 500, y: 500 },
            axis1: ShapePoint { x: 1000, y: 500 },
            axis2: ShapePoint { x: 500, y: 1000 },
            width: HwpUnit::from_mm(40.0).unwrap(),
            height: HwpUnit::from_mm(30.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![simple_paragraph()],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        assert!(ctrl.is_ellipse());
        assert_eq!(ctrl.to_string(), "Ellipse(1 paragraph)");
    }

    #[test]
    fn polygon_construction() {
        let ctrl = Control::Polygon {
            vertices: vec![
                ShapePoint { x: 0, y: 0 },
                ShapePoint { x: 1000, y: 0 },
                ShapePoint { x: 500, y: 1000 },
            ],
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(50.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        assert!(ctrl.is_polygon());
        assert!(!ctrl.is_line());
        assert!(!ctrl.is_ellipse());
        assert_eq!(ctrl.to_string(), "Polygon(3 vertices, 0 paragraphs)");
    }

    #[test]
    fn display_line() {
        let ctrl = Control::Line {
            start: ShapePoint { x: 0, y: 0 },
            end: ShapePoint { x: 100, y: 200 },
            width: HwpUnit::from_mm(10.0).unwrap(),
            height: HwpUnit::from_mm(5.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        assert_eq!(ctrl.to_string(), "Line");
    }

    #[test]
    fn serde_roundtrip_line() {
        let ctrl = Control::Line {
            start: ShapePoint { x: 100, y: 200 },
            end: ShapePoint { x: 300, y: 400 },
            width: HwpUnit::from_mm(20.0).unwrap(),
            height: HwpUnit::from_mm(10.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            caption: None,
            style: None,
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn serde_roundtrip_ellipse() {
        let ctrl = Control::Ellipse {
            center: ShapePoint { x: 500, y: 500 },
            axis1: ShapePoint { x: 1000, y: 500 },
            axis2: ShapePoint { x: 500, y: 1000 },
            width: HwpUnit::from_mm(40.0).unwrap(),
            height: HwpUnit::from_mm(30.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![simple_paragraph()],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn serde_roundtrip_polygon() {
        let ctrl = Control::Polygon {
            vertices: vec![
                ShapePoint { x: 0, y: 0 },
                ShapePoint { x: 1000, y: 0 },
                ShapePoint { x: 500, y: 1000 },
            ],
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(50.0).unwrap(),
            horz_offset: 0,
            vert_offset: 0,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn shape_point_equality() {
        let a = ShapePoint { x: 10, y: 20 };
        let b = ShapePoint { x: 10, y: 20 };
        let c = ShapePoint { x: 10, y: 30 };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn shape_point_new() {
        let pt = ShapePoint::new(100, 200);
        assert_eq!(pt.x, 100);
        assert_eq!(pt.y, 200);
    }

    #[test]
    fn shape_point_serde_roundtrip() {
        let pt = ShapePoint::new(500, 750);
        let json = serde_json::to_string(&pt).unwrap();
        let back: ShapePoint = serde_json::from_str(&json).unwrap();
        assert_eq!(pt, back);
    }

    // ── Convenience constructor tests ────────────────────────────────────

    #[test]
    fn equation_constructor_defaults() {
        let ctrl = Control::equation("{a+b} over {c+d}");
        assert!(ctrl.is_equation());
        match ctrl {
            Control::Equation {
                script,
                width,
                height,
                base_line,
                text_color,
                ref font,
                inst_id: _,
            } => {
                assert_eq!(script, "{a+b} over {c+d}");
                assert_eq!(width, HwpUnit::new(8779).unwrap());
                assert_eq!(height, HwpUnit::new(2600).unwrap());
                assert_eq!(base_line, 71);
                assert_eq!(text_color, Color::BLACK);
                assert_eq!(font, "HancomEQN");
            }
            _ => panic!("expected Equation"),
        }
    }

    #[test]
    fn equation_constructor_empty_script() {
        let ctrl = Control::equation("");
        assert!(ctrl.is_equation());
    }

    #[test]
    fn text_box_constructor_defaults() {
        let width = HwpUnit::from_mm(80.0).unwrap();
        let height = HwpUnit::from_mm(40.0).unwrap();
        let ctrl = Control::text_box(vec![simple_paragraph()], width, height);
        assert!(ctrl.is_text_box());
        match ctrl {
            Control::TextBox { paragraphs, horz_offset, vert_offset, caption, style, .. } => {
                assert_eq!(paragraphs.len(), 1);
                assert_eq!(horz_offset, 0);
                assert_eq!(vert_offset, 0);
                assert!(caption.is_none());
                assert!(style.is_none());
            }
            _ => panic!("expected TextBox"),
        }
    }

    #[test]
    fn footnote_constructor_defaults() {
        let ctrl = Control::footnote(vec![simple_paragraph()]);
        assert!(ctrl.is_footnote());
        match ctrl {
            Control::Footnote { inst_id, paragraphs } => {
                assert!(inst_id.is_none());
                assert_eq!(paragraphs.len(), 1);
            }
            _ => panic!("expected Footnote"),
        }
    }

    #[test]
    fn endnote_constructor_defaults() {
        let ctrl = Control::endnote(vec![simple_paragraph()]);
        assert!(ctrl.is_endnote());
        match ctrl {
            Control::Endnote { inst_id, paragraphs } => {
                assert!(inst_id.is_none());
                assert_eq!(paragraphs.len(), 1);
            }
            _ => panic!("expected Endnote"),
        }
    }

    #[test]
    fn ellipse_constructor_geometry() {
        let width = HwpUnit::from_mm(40.0).unwrap();
        let height = HwpUnit::from_mm(30.0).unwrap();
        let ctrl = Control::ellipse(width, height);
        assert!(ctrl.is_ellipse());
        match &ctrl {
            Control::Ellipse {
                center,
                axis1,
                axis2,
                horz_offset,
                vert_offset,
                paragraphs,
                caption,
                style,
                ..
            } => {
                let w = width.as_i32();
                let h = height.as_i32();
                assert_eq!(*center, ShapePoint::new(w / 2, h / 2));
                assert_eq!(*axis1, ShapePoint::new(w, h / 2));
                assert_eq!(*axis2, ShapePoint::new(w / 2, h));
                assert_eq!(*horz_offset, 0);
                assert_eq!(*vert_offset, 0);
                assert!(paragraphs.is_empty());
                assert!(caption.is_none());
                assert!(style.is_none());
            }
            _ => panic!("expected Ellipse"),
        }
    }

    #[test]
    fn rect_constructor_basic_geometry() {
        let width = HwpUnit::from_mm(40.0).unwrap();
        let height = HwpUnit::from_mm(20.0).unwrap();
        let ctrl = Control::rect(width, height).unwrap();
        assert!(ctrl.is_rect());
        match ctrl {
            Control::Rect { width: w, height: h, horz_offset, vert_offset, caption, style } => {
                assert_eq!(w, width);
                assert_eq!(h, height);
                assert_eq!(horz_offset, 0);
                assert_eq!(vert_offset, 0);
                assert!(caption.is_none());
                assert!(style.is_none());
            }
            _ => panic!("expected Rect"),
        }
    }

    #[test]
    fn rect_constructor_zero_dimension_errors() {
        let zero = HwpUnit::new(0).unwrap();
        let nonzero = HwpUnit::from_mm(10.0).unwrap();
        assert!(Control::rect(zero, nonzero).is_err());
        assert!(Control::rect(nonzero, zero).is_err());
    }

    #[test]
    fn polygon_constructor_triangle() {
        let vertices =
            vec![ShapePoint::new(0, 1000), ShapePoint::new(500, 0), ShapePoint::new(1000, 1000)];
        let ctrl = Control::polygon(vertices).unwrap();
        assert!(ctrl.is_polygon());
        match &ctrl {
            Control::Polygon {
                vertices,
                width,
                height,
                horz_offset,
                vert_offset,
                paragraphs,
                caption,
                style,
                ..
            } => {
                assert_eq!(vertices.len(), 3);
                // bbox: x 0..1000, y 0..1000
                assert_eq!(*width, HwpUnit::new(1000).unwrap());
                assert_eq!(*height, HwpUnit::new(1000).unwrap());
                assert_eq!(*horz_offset, 0);
                assert_eq!(*vert_offset, 0);
                assert!(paragraphs.is_empty());
                assert!(caption.is_none());
                assert!(style.is_none());
            }
            _ => panic!("expected Polygon"),
        }
    }

    #[test]
    fn polygon_constructor_fewer_than_3_vertices_errors() {
        assert!(Control::polygon(vec![]).is_err());
        assert!(Control::polygon(vec![ShapePoint::new(0, 0)]).is_err());
        assert!(Control::polygon(vec![ShapePoint::new(0, 0), ShapePoint::new(1, 1)]).is_err());
    }

    #[test]
    fn polygon_constructor_negative_coordinates() {
        let vertices =
            vec![ShapePoint::new(-500, -500), ShapePoint::new(500, -500), ShapePoint::new(0, 500)];
        let ctrl = Control::polygon(vertices).unwrap();
        assert!(ctrl.is_polygon());
        match ctrl {
            Control::Polygon { width, height, .. } => {
                // bbox: x -500..500 = 1000, y -500..500 = 1000
                assert_eq!(width, HwpUnit::new(1000).unwrap());
                assert_eq!(height, HwpUnit::new(1000).unwrap());
            }
            _ => panic!("expected Polygon"),
        }
    }

    #[test]
    fn polygon_constructor_degenerate_collinear() {
        // 3 collinear points: height = 0 (flat), should succeed
        let vertices =
            vec![ShapePoint::new(0, 0), ShapePoint::new(500, 0), ShapePoint::new(1000, 0)];
        let ctrl = Control::polygon(vertices).unwrap();
        assert!(ctrl.is_polygon());
        match ctrl {
            Control::Polygon { width, height, .. } => {
                assert_eq!(width, HwpUnit::new(1000).unwrap());
                assert_eq!(height, HwpUnit::new(0).unwrap());
            }
            _ => panic!("expected Polygon"),
        }
    }

    #[test]
    fn line_constructor_horizontal() {
        let ctrl = Control::line(ShapePoint::new(0, 0), ShapePoint::new(5000, 0)).unwrap();
        assert!(ctrl.is_line());
        match ctrl {
            Control::Line {
                start,
                end,
                width,
                height,
                horz_offset,
                vert_offset,
                caption,
                style,
            } => {
                assert_eq!(start, ShapePoint::new(0, 0));
                assert_eq!(end, ShapePoint::new(5000, 0));
                assert_eq!(width, HwpUnit::new(5000).unwrap());
                assert_eq!(height, HwpUnit::new(100).unwrap()); // min bounding box
                assert_eq!(horz_offset, 0);
                assert_eq!(vert_offset, 0);
                assert!(caption.is_none());
                assert!(style.is_none());
            }
            _ => panic!("expected Line"),
        }
    }

    #[test]
    fn line_constructor_vertical() {
        let ctrl = Control::line(ShapePoint::new(0, 0), ShapePoint::new(0, 3000)).unwrap();
        assert!(ctrl.is_line());
        match ctrl {
            Control::Line { width, height, .. } => {
                assert_eq!(width, HwpUnit::new(100).unwrap()); // min bounding box
                assert_eq!(height, HwpUnit::new(3000).unwrap());
            }
            _ => panic!("expected Line"),
        }
    }

    #[test]
    fn line_constructor_diagonal_bounding_box() {
        let ctrl = Control::line(ShapePoint::new(100, 200), ShapePoint::new(400, 500)).unwrap();
        match ctrl {
            Control::Line { width, height, .. } => {
                assert_eq!(width, HwpUnit::new(300).unwrap());
                assert_eq!(height, HwpUnit::new(300).unwrap());
            }
            _ => panic!("expected Line"),
        }
    }

    #[test]
    fn line_constructor_same_point_errors() {
        let pt = ShapePoint::new(100, 200);
        assert!(Control::line(pt, pt).is_err());
    }

    #[test]
    fn horizontal_line_constructor() {
        let width = HwpUnit::from_mm(100.0).unwrap();
        let ctrl = Control::horizontal_line(width);
        assert!(ctrl.is_line());
        match ctrl {
            Control::Line {
                start,
                end,
                width: w,
                height,
                horz_offset,
                vert_offset,
                caption,
                style,
            } => {
                assert_eq!(start, ShapePoint::new(0, 0));
                assert_eq!(end.y, 0);
                assert_eq!(end.x, width.as_i32());
                assert_eq!(w, width);
                assert_eq!(height, HwpUnit::new(100).unwrap()); // min bounding box
                assert_eq!(horz_offset, 0);
                assert_eq!(vert_offset, 0);
                assert!(caption.is_none());
                assert!(style.is_none());
            }
            _ => panic!("expected Line"),
        }
    }

    #[test]
    fn hyperlink_constructor() {
        let ctrl = Control::hyperlink("Visit Rust", "https://rust-lang.org");
        assert!(ctrl.is_hyperlink());
        match ctrl {
            Control::Hyperlink { text, url } => {
                assert_eq!(text, "Visit Rust");
                assert_eq!(url, "https://rust-lang.org");
            }
            _ => panic!("expected Hyperlink"),
        }
    }

    #[test]
    fn footnote_with_id_sets_inst_id() {
        let para = Paragraph::new(ParaShapeIndex::new(0));
        let ctrl = Control::footnote_with_id(42, vec![para]);
        assert!(ctrl.is_footnote());
        match ctrl {
            Control::Footnote { inst_id, paragraphs } => {
                assert_eq!(inst_id, Some(ObjectId::new(42)));
                assert_eq!(paragraphs.len(), 1);
            }
            _ => panic!("expected Footnote"),
        }
    }

    #[test]
    fn endnote_with_id_sets_inst_id() {
        let para = Paragraph::new(ParaShapeIndex::new(0));
        let ctrl = Control::endnote_with_id(7, vec![para]);
        assert!(ctrl.is_endnote());
        match ctrl {
            Control::Endnote { inst_id, paragraphs } => {
                assert_eq!(inst_id, Some(ObjectId::new(7)));
                assert_eq!(paragraphs.len(), 1);
            }
            _ => panic!("expected Endnote"),
        }
    }

    #[test]
    fn footnote_with_id_differs_from_plain_footnote() {
        let ctrl_plain = Control::footnote(vec![]);
        let ctrl_id = Control::footnote_with_id(1, vec![]);
        match ctrl_plain {
            Control::Footnote { inst_id, .. } => assert_eq!(inst_id, None),
            _ => panic!("expected Footnote"),
        }
        match ctrl_id {
            Control::Footnote { inst_id, .. } => assert_eq!(inst_id, Some(ObjectId::new(1))),
            _ => panic!("expected Footnote"),
        }
    }

    #[test]
    fn ellipse_with_text_has_correct_geometry_and_paragraphs() {
        use hwpforge_foundation::HwpUnit;
        let width = HwpUnit::from_mm(40.0).unwrap();
        let height = HwpUnit::from_mm(30.0).unwrap();
        let para = Paragraph::new(ParaShapeIndex::new(0));
        let ctrl = Control::ellipse_with_text(width, height, vec![para]);
        assert!(ctrl.is_ellipse());
        match ctrl {
            Control::Ellipse {
                center,
                axis1,
                axis2,
                width: w,
                height: h,
                horz_offset,
                vert_offset,
                paragraphs,
                caption,
                style,
                ..
            } => {
                let wv = w.as_i32();
                let hv = h.as_i32();
                assert_eq!(center, ShapePoint::new(wv / 2, hv / 2));
                assert_eq!(axis1, ShapePoint::new(wv, hv / 2));
                assert_eq!(axis2, ShapePoint::new(wv / 2, hv));
                assert_eq!(horz_offset, 0);
                assert_eq!(vert_offset, 0);
                assert_eq!(paragraphs.len(), 1);
                assert!(caption.is_none());
                assert!(style.is_none());
            }
            _ => panic!("expected Ellipse"),
        }
    }

    #[test]
    fn serde_roundtrip_chart() {
        use crate::chart::{ChartData, ChartGrouping, ChartType, LegendPosition};
        let ctrl = Control::Chart {
            chart_type: ChartType::Column,
            data: ChartData::category(&["A", "B"], &[("S1", &[1.0, 2.0])]),
            title: Some("Test Chart".to_string()),
            legend: LegendPosition::Bottom,
            grouping: ChartGrouping::Stacked,
            width: HwpUnit::from_mm(100.0).unwrap(),
            height: HwpUnit::from_mm(80.0).unwrap(),
            stock_variant: None,
            bar_shape: None,
            scatter_style: None,
            radar_style: None,
            of_pie_type: None,
            explosion: None,
            wireframe: None,
            bubble_3d: None,
            show_markers: None,
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn serde_roundtrip_equation() {
        let ctrl = Control::Equation {
            script: "{a+b} over {c+d}".to_string(),
            width: HwpUnit::new(8779).unwrap(),
            height: HwpUnit::new(2600).unwrap(),
            base_line: 71,
            text_color: Color::BLACK,
            font: "HancomEQN".to_string(),
            inst_id: None,
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let back: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, back);
    }

    #[test]
    fn ellipse_with_text_empty_paragraphs_matches_ellipse() {
        use hwpforge_foundation::HwpUnit;
        let width = HwpUnit::from_mm(20.0).unwrap();
        let height = HwpUnit::from_mm(10.0).unwrap();
        let plain = Control::ellipse(width, height);
        let with_text = Control::ellipse_with_text(width, height, vec![]);
        // Both should produce identical shapes when paragraphs are empty
        assert_eq!(plain, with_text);
    }

    // ── Dutmal (덧말) tests ──────────────────────────────────────

    #[test]
    fn dutmal_constructor_defaults() {
        let ctrl = Control::dutmal("본문", "주석");
        assert!(ctrl.is_dutmal());
        match ctrl {
            Control::Dutmal { main_text, sub_text, position, sz_ratio, align, .. } => {
                assert_eq!(main_text, "본문");
                assert_eq!(sub_text, "주석");
                assert_eq!(position, DutmalPosition::Top);
                assert_eq!(sz_ratio, 0);
                assert_eq!(align, DutmalAlign::Center);
            }
            _ => panic!("expected Dutmal"),
        }
    }

    #[test]
    fn dutmal_is_dutmal_true() {
        assert!(Control::dutmal("a", "b").is_dutmal());
    }

    #[test]
    fn dutmal_is_compose_false() {
        assert!(!Control::dutmal("a", "b").is_compose());
    }

    #[test]
    fn dutmal_display() {
        let ctrl = Control::dutmal("hello", "world");
        assert_eq!(ctrl.to_string(), r#"Dutmal("hello" / "world")"#);
    }

    #[test]
    fn dutmal_serde_roundtrip() {
        let ctrl = Control::Dutmal {
            main_text: "테스트".to_string(),
            sub_text: "test".to_string(),
            position: DutmalPosition::Bottom,
            sz_ratio: 50,
            align: DutmalAlign::Right,
            metadata: DutmalMetadata::default(),
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let decoded: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, decoded);
    }

    #[test]
    fn dutmal_position_default_is_top() {
        assert_eq!(DutmalPosition::default(), DutmalPosition::Top);
    }

    #[test]
    fn dutmal_align_default_is_center() {
        assert_eq!(DutmalAlign::default(), DutmalAlign::Center);
    }

    // ── Compose (글자겹침) tests ─────────────────────────────────

    #[test]
    fn compose_constructor_defaults() {
        let ctrl = Control::compose("가");
        assert!(ctrl.is_compose());
        match ctrl {
            Control::Compose { compose_text, circle_type, char_sz, compose_type, char_pr_ids } => {
                assert_eq!(compose_text, "가");
                assert_eq!(circle_type, "SHAPE_REVERSAL_TIRANGLE");
                assert_eq!(char_sz, -3);
                assert_eq!(compose_type, "SPREAD");
                assert_eq!(char_pr_ids, vec![u32::MAX; 10]);
            }
            _ => panic!("expected Compose"),
        }
    }

    #[test]
    fn compose_is_compose_true() {
        assert!(Control::compose("나").is_compose());
    }

    #[test]
    fn compose_is_dutmal_false() {
        assert!(!Control::compose("나").is_dutmal());
    }

    #[test]
    fn compose_display() {
        let ctrl = Control::compose("가나");
        assert_eq!(ctrl.to_string(), r#"Compose("가나")"#);
    }

    #[test]
    fn compose_serde_roundtrip() {
        let ctrl = Control::Compose {
            compose_text: "①".to_string(),
            circle_type: "SHAPE_REVERSAL_TIRANGLE".to_string(),
            char_sz: -3,
            compose_type: "SPREAD".to_string(),
            char_pr_ids: vec![u32::MAX; 10],
        };
        let json = serde_json::to_string(&ctrl).unwrap();
        let decoded: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, decoded);
    }

    #[test]
    fn compose_spec_typo_preserved() {
        // "SHAPE_REVERSAL_TIRANGLE" is an official spec typo — must be preserved exactly
        let ctrl = Control::compose("X");
        match ctrl {
            Control::Compose { circle_type, .. } => {
                assert_eq!(circle_type, "SHAPE_REVERSAL_TIRANGLE");
                assert!(!circle_type.contains("TRIANGLE")); // confirm the typo
            }
            _ => panic!("expected Compose"),
        }
    }

    // ===================================================================
    // H2: saturating i64→i32 conversion in shape constructors
    // ===================================================================

    #[test]
    fn line_extreme_coords_no_panic() {
        // Coordinates near i32 extremes produce a valid line without panicking
        let start = ShapePoint::new(i32::MIN, i32::MIN);
        let end = ShapePoint::new(i32::MAX, i32::MAX);
        let ctrl = Control::line(start, end).unwrap();
        assert!(ctrl.is_line());
    }

    #[test]
    fn connect_line_extreme_coords_no_panic() {
        let start = ShapePoint::new(i32::MIN, 0);
        let end = ShapePoint::new(i32::MAX, 0);
        let ctrl = Control::connect_line(start, end).unwrap();
        assert!(ctrl.is_connect_line());
    }

    #[test]
    fn polygon_extreme_coords_no_panic() {
        // Span exceeds i32::MAX — should error (HwpUnit range exceeded), not panic
        let vertices = vec![
            ShapePoint::new(i32::MIN, 0),
            ShapePoint::new(i32::MAX, 0),
            ShapePoint::new(0, i32::MAX),
        ];
        // Either succeeds (saturated) or returns an error — must not panic
        let _ = Control::polygon(vertices);
    }

    #[test]
    fn curve_extreme_coords_no_panic() {
        let points = vec![ShapePoint::new(i32::MIN, i32::MIN), ShapePoint::new(i32::MAX, i32::MAX)];
        let _ = Control::curve(points);
    }
}
