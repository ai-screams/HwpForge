//! XML schema types for `header.xml` (hh: namespace).
//!
//! Maps the `<hh:head>` element tree into Rust structs via serde.
//! All types use `#[serde(default)]` on optional sub-elements for
//! forward compatibility with different 한글 versions.
//!
//! Fields are used by serde deserialization even if not directly accessed.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

// ── Root ──────────────────────────────────────────────────────────

/// `<hh:head version="1.4" secCnt="1">`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename = "head")]
pub struct HxHead {
    #[serde(rename = "@version", default)]
    pub version: String,
    #[serde(rename = "@secCnt", default)]
    pub sec_cnt: u32,
    #[serde(
        rename(serialize = "hh:refList", deserialize = "refList"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ref_list: Option<HxRefList>,
}

// ── RefList ───────────────────────────────────────────────────────

/// `<hh:refList>` — container for all shared definitions.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct HxRefList {
    #[serde(
        rename(serialize = "hh:fontfaces", deserialize = "fontfaces"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub fontfaces: Option<HxFontFaces>,
    #[serde(
        rename(serialize = "hh:charProperties", deserialize = "charProperties"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub char_properties: Option<HxCharProperties>,
    #[serde(
        rename(serialize = "hh:paraProperties", deserialize = "paraProperties"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub para_properties: Option<HxParaProperties>,
    #[serde(
        rename(serialize = "hh:styles", deserialize = "styles"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub styles: Option<HxStyles>,
    /// Border fill definitions.
    #[serde(
        rename(serialize = "hh:borderFills", deserialize = "borderFills"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub border_fills: Option<HxBorderFills>,
    // tabProperties, numberings — skipped (Phase 3)
}

// ── Fonts ─────────────────────────────────────────────────────────

/// `<hh:fontfaces itemCnt="7">`.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct HxFontFaces {
    #[serde(rename = "@itemCnt", default)]
    pub item_cnt: u32,
    #[serde(
        rename(serialize = "hh:fontface", deserialize = "fontface"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub groups: Vec<HxFontFaceGroup>,
}

/// `<hh:fontface lang="HANGUL" fontCnt="2">`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxFontFaceGroup {
    #[serde(rename = "@lang", default)]
    pub lang: String,
    #[serde(rename = "@fontCnt", default)]
    pub font_cnt: u32,
    #[serde(
        rename(serialize = "hh:font", deserialize = "font"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub fonts: Vec<HxFont>,
}

/// `<hh:font id="0" face="함초롬돋움" type="TTF" isEmbedded="0">`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxFont {
    #[serde(rename = "@id")]
    pub id: u32,
    #[serde(rename = "@face", default)]
    pub face: String,
    #[serde(rename = "@type", default)]
    pub font_type: String,
    #[serde(rename = "@isEmbedded", default)]
    pub is_embedded: u32,
    /// Font classification metadata.
    #[serde(
        rename(serialize = "hh:typeInfo", deserialize = "typeInfo"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub type_info: Option<HxTypeInfo>,
}

/// `<hh:typeInfo>` — font classification metadata (PANOSE-like).
///
/// Provides font metric hints for substitution when the exact font
/// is unavailable. Values follow the PANOSE classification system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxTypeInfo {
    /// Font family type (e.g. `"FCAT_GOTHIC"`, `"FCAT_MYEONGJO"`).
    #[serde(rename = "@familyType", default)]
    pub family_type: String,
    /// Stroke weight (typically 1-10, 6 = medium).
    #[serde(rename = "@weight", default)]
    pub weight: u32,
    /// Proportion (0 = any).
    #[serde(rename = "@proportion", default)]
    pub proportion: u32,
    /// Contrast (0 = any).
    #[serde(rename = "@contrast", default)]
    pub contrast: u32,
    /// Stroke variation (1 = no variation).
    #[serde(rename = "@strokeVariation", default)]
    pub stroke_variation: u32,
    /// Arm style (1 = straight).
    #[serde(rename = "@armStyle", default)]
    pub arm_style: u32,
    /// Letterform (1 = normal).
    #[serde(rename = "@letterform", default)]
    pub letterform: u32,
    /// Midline (1 = standard).
    #[serde(rename = "@midline", default)]
    pub midline: u32,
    /// x-height (1 = constant).
    #[serde(rename = "@xHeight", default)]
    pub x_height: u32,
}

// ── Character Properties ──────────────────────────────────────────

/// `<hh:charProperties itemCnt="8">`.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct HxCharProperties {
    #[serde(rename = "@itemCnt", default)]
    pub item_cnt: u32,
    #[serde(
        rename(serialize = "hh:charPr", deserialize = "charPr"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub items: Vec<HxCharPr>,
}

/// `<hh:charPr id="0" height="1000" textColor="#000000" ...>`.
///
/// Attributes: id, height, textColor, shadeColor, useFontSpace,
///     useKerning, symMark, borderFillIDRef.
/// Children: fontRef, ratio, spacing, relSz, offset, underline,
///     strikeout, outline, shadow, (optional) bold, italic.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxCharPr {
    #[serde(rename = "@id")]
    pub id: u32,
    #[serde(rename = "@height", default)]
    pub height: u32,
    #[serde(rename = "@textColor", default)]
    pub text_color: String,
    #[serde(rename = "@shadeColor", default)]
    pub shade_color: String,
    #[serde(rename = "@useFontSpace", default)]
    pub use_font_space: u32,
    #[serde(rename = "@useKerning", default)]
    pub use_kerning: u32,
    #[serde(rename = "@symMark", default)]
    pub sym_mark: String,
    #[serde(rename = "@borderFillIDRef", default)]
    pub border_fill_id_ref: u32,

    // ── child elements ──
    #[serde(
        rename(serialize = "hh:fontRef", deserialize = "fontRef"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub font_ref: Option<HxFontRef>,
    #[serde(
        rename(serialize = "hh:ratio", deserialize = "ratio"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ratio: Option<HxLangValues>,
    #[serde(
        rename(serialize = "hh:spacing", deserialize = "spacing"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub spacing: Option<HxLangValues>,
    #[serde(
        rename(serialize = "hh:relSz", deserialize = "relSz"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub rel_sz: Option<HxLangValues>,
    #[serde(
        rename(serialize = "hh:offset", deserialize = "offset"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub offset: Option<HxLangValues>,
    #[serde(
        rename(serialize = "hh:bold", deserialize = "bold"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub bold: Option<HxPresence>,
    #[serde(
        rename(serialize = "hh:italic", deserialize = "italic"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub italic: Option<HxPresence>,
    #[serde(
        rename(serialize = "hh:underline", deserialize = "underline"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub underline: Option<HxUnderline>,
    #[serde(
        rename(serialize = "hh:strikeout", deserialize = "strikeout"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub strikeout: Option<HxStrikeout>,
    #[serde(
        rename(serialize = "hh:outline", deserialize = "outline"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub outline: Option<HxOutline>,
    #[serde(
        rename(serialize = "hh:shadow", deserialize = "shadow"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shadow: Option<HxShadow>,
}

/// Per-language font index references.
/// `<hh:fontRef hangul="1" latin="1" hanja="1" .../>`.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct HxFontRef {
    #[serde(rename = "@hangul", default)]
    pub hangul: u32,
    #[serde(rename = "@latin", default)]
    pub latin: u32,
    #[serde(rename = "@hanja", default)]
    pub hanja: u32,
    #[serde(rename = "@japanese", default)]
    pub japanese: u32,
    #[serde(rename = "@other", default)]
    pub other: u32,
    #[serde(rename = "@symbol", default)]
    pub symbol: u32,
    #[serde(rename = "@user", default)]
    pub user: u32,
}

/// Per-language value fields used by `<hh:ratio>`, `<hh:spacing>`,
/// `<hh:relSz>`, and `<hh:offset>` child elements of `<hh:charPr>`.
///
/// Each field corresponds to one of the 7 language groups.
/// Defaults: ratio=100, spacing=0, relSz=100, offset=0 for all languages.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct HxLangValues {
    #[serde(rename = "@hangul", default)]
    pub hangul: i32,
    #[serde(rename = "@latin", default)]
    pub latin: i32,
    #[serde(rename = "@hanja", default)]
    pub hanja: i32,
    #[serde(rename = "@japanese", default)]
    pub japanese: i32,
    #[serde(rename = "@other", default)]
    pub other: i32,
    #[serde(rename = "@symbol", default)]
    pub symbol: i32,
    #[serde(rename = "@user", default)]
    pub user: i32,
}

/// Marker for presence-based boolean elements (`<hh:bold/>`, `<hh:italic/>`).
///
/// The element's mere presence means `true`; absence means `false`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxPresence;

/// `<hh:underline type="NONE" shape="SOLID" color="#000000"/>`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxUnderline {
    #[serde(rename = "@type", default)]
    pub underline_type: String,
    #[serde(rename = "@shape", default)]
    pub shape: String,
    #[serde(rename = "@color", default)]
    pub color: String,
}

/// `<hh:strikeout shape="NONE" color="#000000"/>`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxStrikeout {
    #[serde(rename = "@shape", default)]
    pub shape: String,
    #[serde(rename = "@color", default)]
    pub color: String,
}

/// `<hh:outline type="NONE"/>`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxOutline {
    #[serde(rename = "@type", default)]
    pub outline_type: String,
}

/// `<hh:shadow type="NONE" color="#B2B2B2" offsetX="10" offsetY="10"/>`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxShadow {
    #[serde(rename = "@type", default)]
    pub shadow_type: String,
    #[serde(rename = "@color", default)]
    pub color: String,
    #[serde(rename = "@offsetX", default)]
    pub offset_x: i32,
    #[serde(rename = "@offsetY", default)]
    pub offset_y: i32,
}

// ── Paragraph Properties ──────────────────────────────────────────

/// `<hh:paraProperties itemCnt="16">`.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct HxParaProperties {
    #[serde(rename = "@itemCnt", default)]
    pub item_cnt: u32,
    #[serde(
        rename(serialize = "hh:paraPr", deserialize = "paraPr"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub items: Vec<HxParaPr>,
}

/// `<hh:paraPr id="0" tabPrIDRef="0" condense="0" fontLineHeight="0" snapToGrid="1" ...>`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxParaPr {
    #[serde(rename = "@id")]
    pub id: u32,
    #[serde(rename = "@tabPrIDRef", default)]
    pub tab_pr_id_ref: u32,
    #[serde(rename = "@condense", default)]
    pub condense: u32,
    /// Font-based line height calculation flag.
    #[serde(rename = "@fontLineHeight", default)]
    pub font_line_height: u32,
    /// Snap paragraph to document grid.
    #[serde(rename = "@snapToGrid", default)]
    pub snap_to_grid: u32,
    /// Whether line numbers are suppressed for this paragraph.
    #[serde(rename = "@suppressLineNumbers", default)]
    pub suppress_line_numbers: u32,
    /// Checkbox state for this paragraph.
    #[serde(rename = "@checked", default)]
    pub checked: u32,

    // ── child elements ──
    #[serde(
        rename(serialize = "hh:align", deserialize = "align"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub align: Option<HxAlign>,
    #[serde(
        rename(serialize = "hh:heading", deserialize = "heading"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub heading: Option<HxHeading>,
    #[serde(
        rename(serialize = "hh:breakSetting", deserialize = "breakSetting"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub break_setting: Option<HxBreakSetting>,
    #[serde(
        rename(serialize = "hh:autoSpacing", deserialize = "autoSpacing"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub auto_spacing: Option<HxAutoSpacing>,
    /// The `<hp:switch>` elements wrapping version-specific rendering data.
    ///
    /// Some `paraPr` elements contain multiple `<hp:switch>` blocks (e.g. one
    /// for heading and one for margin/lineSpacing). Stored as a `Vec` to handle
    /// all cases gracefully.
    #[serde(
        rename(serialize = "hp:switch", deserialize = "switch"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub switches: Vec<HxSwitch>,
    #[serde(
        rename(serialize = "hh:border", deserialize = "border"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub border: Option<HxBorder>,
}

/// `<hh:align horizontal="LEFT" vertical="BASELINE"/>`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxAlign {
    #[serde(rename = "@horizontal", default)]
    pub horizontal: String,
    #[serde(rename = "@vertical", default)]
    pub vertical: String,
}

/// `<hh:heading type="NONE" idRef="0" level="0"/>`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxHeading {
    #[serde(rename = "@type", default)]
    pub heading_type: String,
    #[serde(rename = "@idRef", default)]
    pub id_ref: u32,
    #[serde(rename = "@level", default)]
    pub level: u32,
}

/// `<hh:breakSetting breakLatinWord="KEEP_WORD" breakNonLatinWord="KEEP_WORD" .../>`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxBreakSetting {
    #[serde(rename = "@breakLatinWord", default)]
    pub break_latin_word: String,
    #[serde(rename = "@breakNonLatinWord", default)]
    pub break_non_latin_word: String,
    #[serde(rename = "@widowOrphan", default)]
    pub widow_orphan: u32,
    #[serde(rename = "@keepWithNext", default)]
    pub keep_with_next: u32,
    #[serde(rename = "@keepLines", default)]
    pub keep_lines: u32,
    #[serde(rename = "@pageBreakBefore", default)]
    pub page_break_before: u32,
    /// Line wrapping mode (`"BREAK"` is the standard default).
    #[serde(rename = "@lineWrap", default, skip_serializing_if = "String::is_empty")]
    pub line_wrap: String,
}

/// `<hh:autoSpacing eAsianEng="0" eAsianNum="0"/>`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxAutoSpacing {
    #[serde(rename = "@eAsianEng", default)]
    pub e_asian_eng: u32,
    #[serde(rename = "@eAsianNum", default)]
    pub e_asian_num: u32,
}

/// `<hh:border borderFillIDRef="2" offsetLeft="0" offsetRight="0" offsetTop="0" offsetBottom="0" connect="0" ignoreMargin="0"/>`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxBorder {
    #[serde(rename = "@borderFillIDRef", default)]
    pub border_fill_id_ref: u32,
    #[serde(rename = "@offsetLeft", default)]
    pub offset_left: i32,
    #[serde(rename = "@offsetRight", default)]
    pub offset_right: i32,
    #[serde(rename = "@offsetTop", default)]
    pub offset_top: i32,
    #[serde(rename = "@offsetBottom", default)]
    pub offset_bottom: i32,
    /// Whether this border connects with adjacent paragraph borders.
    #[serde(rename = "@connect", default)]
    pub connect: u32,
    /// Whether to ignore margin when drawing the border.
    #[serde(rename = "@ignoreMargin", default)]
    pub ignore_margin: u32,
}

// ── hp:switch / hp:case / hp:default ──────────────────────────────

/// `<hp:switch>` — version-specific rendering container.
///
/// Phase 3 reads only the `<hp:default>` block for maximum
/// compatibility across 한글 versions.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxSwitch {
    #[serde(
        rename(serialize = "hp:case", deserialize = "case"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub case: Option<HxSwitchCase>,
    #[serde(
        rename(serialize = "hp:default", deserialize = "default"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub default: Option<HxSwitchDefault>,
}

/// `<hp:case hp:required-namespace="...">`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxSwitchCase {
    #[serde(
        rename(serialize = "@hp:required-namespace", deserialize = "@required-namespace"),
        default
    )]
    pub required_namespace: String,
    #[serde(
        rename(serialize = "hh:margin", deserialize = "margin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub margin: Option<HxMargin>,
    #[serde(
        rename(serialize = "hh:lineSpacing", deserialize = "lineSpacing"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub line_spacing: Option<HxLineSpacing>,
}

/// `<hp:default>` — fallback values for older viewers.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxSwitchDefault {
    #[serde(
        rename(serialize = "hh:margin", deserialize = "margin"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub margin: Option<HxMargin>,
    #[serde(
        rename(serialize = "hh:lineSpacing", deserialize = "lineSpacing"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub line_spacing: Option<HxLineSpacing>,
}

/// `<hh:margin>` containing `<hc:intent>`, `<hc:left>`, etc.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct HxMargin {
    /// NB: HWPX uses `<hc:intent>` (typo in the spec; should be "indent").
    #[serde(
        rename(serialize = "hc:intent", deserialize = "intent"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub indent: Option<HxUnitValue>,
    #[serde(
        rename(serialize = "hc:left", deserialize = "left"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub left: Option<HxUnitValue>,
    #[serde(
        rename(serialize = "hc:right", deserialize = "right"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub right: Option<HxUnitValue>,
    #[serde(
        rename(serialize = "hc:prev", deserialize = "prev"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub prev: Option<HxUnitValue>,
    #[serde(
        rename(serialize = "hc:next", deserialize = "next"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub next: Option<HxUnitValue>,
}

/// `<hc:left value="0" unit="HWPUNIT"/>` — generic value+unit pair.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxUnitValue {
    #[serde(rename = "@value", default)]
    pub value: i32,
    #[serde(rename = "@unit", default)]
    pub unit: String,
}

/// `<hh:lineSpacing type="PERCENT" value="130" unit="HWPUNIT"/>`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxLineSpacing {
    #[serde(rename = "@type", default)]
    pub spacing_type: String,
    #[serde(rename = "@value", default)]
    pub value: u32,
    #[serde(rename = "@unit", default)]
    pub unit: String,
}

// ── Styles ────────────────────────────────────────────────────────

/// `<hh:styles itemCnt="18">`.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct HxStyles {
    #[serde(rename = "@itemCnt", default)]
    pub item_cnt: u32,
    #[serde(
        rename(serialize = "hh:style", deserialize = "style"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub items: Vec<HxStyle>,
}

/// `<hh:style id="0" type="PARA" name="바탕글" engName="Normal" ...>`.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HxStyle {
    #[serde(rename = "@id")]
    pub id: u32,
    #[serde(rename = "@type", default)]
    pub style_type: String,
    #[serde(rename = "@name", default)]
    pub name: String,
    #[serde(rename = "@engName", default)]
    pub eng_name: String,
    #[serde(rename = "@paraPrIDRef", default)]
    pub para_pr_id_ref: u32,
    #[serde(rename = "@charPrIDRef", default)]
    pub char_pr_id_ref: u32,
    #[serde(rename = "@nextStyleIDRef", default)]
    pub next_style_id_ref: u32,
    #[serde(rename = "@langID", default)]
    pub lang_id: u32,
    /// Whether the style is locked for form editing.
    #[serde(rename = "@lockForm", default)]
    pub lock_form: u32,
}

// ── BorderFill schema ─────────────────────────────────────────────

/// `<hh:borderFills itemCnt="N">` — collection of border/fill definitions.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct HxBorderFills {
    /// Number of border fill entries.
    #[serde(rename = "@itemCnt")]
    pub item_cnt: u32,
    /// Border fill entries.
    #[serde(
        rename(serialize = "hh:borderFill", deserialize = "borderFill"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub items: Vec<HxBorderFill>,
}

/// `<hh:borderFill>` — border and fill definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxBorderFill {
    /// Border fill ID (1-based).
    #[serde(rename = "@id")]
    pub id: u32,
    /// 3D border effect flag.
    #[serde(rename = "@threeD", default)]
    pub three_d: u32,
    /// Shadow effect flag.
    #[serde(rename = "@shadow", default)]
    pub shadow: u32,
    /// Center line type string.
    #[serde(rename = "@centerLine", default)]
    pub center_line: String,
    /// Whether to break cell separator line.
    #[serde(rename = "@breakCellSeparateLine", default)]
    pub break_cell_separate_line: u32,

    /// Slash diagonal border.
    #[serde(rename(serialize = "hh:slash", deserialize = "slash"))]
    pub slash: HxDiagonalBorder,
    /// Back-slash diagonal border.
    #[serde(rename(serialize = "hh:backSlash", deserialize = "backSlash"))]
    pub back_slash: HxDiagonalBorder,
    /// Left border line.
    #[serde(rename(serialize = "hh:leftBorder", deserialize = "leftBorder"))]
    pub left_border: HxBorderLine,
    /// Right border line.
    #[serde(rename(serialize = "hh:rightBorder", deserialize = "rightBorder"))]
    pub right_border: HxBorderLine,
    /// Top border line.
    #[serde(rename(serialize = "hh:topBorder", deserialize = "topBorder"))]
    pub top_border: HxBorderLine,
    /// Bottom border line.
    #[serde(rename(serialize = "hh:bottomBorder", deserialize = "bottomBorder"))]
    pub bottom_border: HxBorderLine,
    /// Diagonal border line.
    #[serde(rename(serialize = "hh:diagonal", deserialize = "diagonal"))]
    pub diagonal: HxBorderLine,

    /// Fill brush (None = no fill / transparent).
    #[serde(
        rename(serialize = "hc:fillBrush", deserialize = "fillBrush"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub fill_brush: Option<HxFillBrush>,
}

/// `<hh:slash>` or `<hh:backSlash>` — diagonal border line.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxDiagonalBorder {
    /// Border type string (e.g. `"NONE"`, `"SOLID"`).
    #[serde(rename = "@type", default)]
    pub border_type: String,
    /// Crooked flag.
    #[serde(rename = "@Crooked", default)]
    pub crooked: String,
    /// Counter direction flag.
    #[serde(rename = "@isCounter", default)]
    pub is_counter: String,
}

/// `<hh:leftBorder>` etc. — a single border line definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxBorderLine {
    /// Line type string (e.g. `"NONE"`, `"SOLID"`).
    #[serde(rename = "@type", default)]
    pub border_type: String,
    /// Width string (e.g. `"0.1 mm"`).
    #[serde(rename = "@width", default)]
    pub width: String,
    /// Color string (e.g. `"#000000"`).
    #[serde(rename = "@color", default)]
    pub color: String,
}

/// `<hc:fillBrush>` — fill brush definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxFillBrush {
    /// Solid or hatch fill.
    #[serde(
        rename(serialize = "hc:winBrush", deserialize = "winBrush"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub win_brush: Option<HxWinBrush>,
    /// Gradient fill (future use).
    #[serde(
        rename(serialize = "hc:gradation", deserialize = "gradation"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub gradation: Option<HxGradation>,
    /// Image fill (future use).
    #[serde(
        rename(serialize = "hc:imgBrush", deserialize = "imgBrush"),
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub img_brush: Option<HxImgBrush>,
}

/// `<hc:winBrush>` — solid or hatch fill.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxWinBrush {
    /// Face color (e.g. `"none"`, `"#RRGGBB"`).
    #[serde(rename = "@faceColor", default)]
    pub face_color: String,
    /// Hatch pattern color.
    #[serde(rename = "@hatchColor", default)]
    pub hatch_color: String,
    /// Alpha transparency value.
    #[serde(rename = "@alpha", default)]
    pub alpha: String,
}

/// `<hc:gradation>` — gradient fill (placeholder, attributes parsed but not used).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxGradation {
    /// Gradient type string.
    #[serde(rename = "@type", default)]
    pub gradation_type: String,
    /// Gradient angle in degrees.
    #[serde(rename = "@angle", default)]
    pub angle: String,
    /// Gradient center X.
    #[serde(rename = "@centerX", default)]
    pub center_x: String,
    /// Gradient center Y.
    #[serde(rename = "@centerY", default)]
    pub center_y: String,
    /// Number of gradient steps.
    #[serde(rename = "@step", default)]
    pub step: String,
    /// Number of colors.
    #[serde(rename = "@colorNum", default)]
    pub color_num: String,
}

/// `<hc:imgBrush>` — image pattern fill (placeholder).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HxImgBrush {
    /// Image fill mode string.
    #[serde(rename = "@mode", default)]
    pub mode: String,
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_head(xml: &str) -> HxHead {
        quick_xml::de::from_str(xml).expect("failed to parse HxHead")
    }

    #[test]
    fn parse_minimal_head() {
        let xml = r#"<hh:head version="1.4" secCnt="1"></hh:head>"#;
        let head = parse_head(xml);
        assert_eq!(head.version, "1.4");
        assert_eq!(head.sec_cnt, 1);
        assert!(head.ref_list.is_none());
    }

    #[test]
    fn parse_fontfaces() {
        let xml = r#"
        <hh:head version="1.4" secCnt="1">
          <hh:refList>
            <hh:fontfaces itemCnt="1">
              <hh:fontface lang="HANGUL" fontCnt="1">
                <hh:font id="0" face="함초롬돋움" type="TTF" isEmbedded="0"/>
              </hh:fontface>
            </hh:fontfaces>
          </hh:refList>
        </hh:head>"#;
        let head = parse_head(xml);
        let ff = head.ref_list.unwrap().fontfaces.unwrap();
        assert_eq!(ff.groups.len(), 1);
        assert_eq!(ff.groups[0].lang, "HANGUL");
        assert_eq!(ff.groups[0].fonts[0].face, "함초롬돋움");
    }

    #[test]
    fn parse_char_pr_basic() {
        let xml = r##"
        <hh:head version="1.4" secCnt="1">
          <hh:refList>
            <hh:charProperties itemCnt="1">
              <hh:charPr id="0" height="1000" textColor="#000000" shadeColor="none"
                         useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="2">
                <hh:fontRef hangul="1" latin="1" hanja="1" japanese="1" other="1" symbol="1" user="1"/>
                <hh:underline type="NONE" shape="SOLID" color="#000000"/>
                <hh:strikeout shape="NONE" color="#000000"/>
                <hh:outline type="NONE"/>
                <hh:shadow type="NONE" color="#B2B2B2" offsetX="10" offsetY="10"/>
              </hh:charPr>
            </hh:charProperties>
          </hh:refList>
        </hh:head>"##;
        let head = parse_head(xml);
        let cp = &head.ref_list.unwrap().char_properties.unwrap().items[0];
        assert_eq!(cp.id, 0);
        assert_eq!(cp.height, 1000);
        assert_eq!(cp.text_color, "#000000");
        assert_eq!(cp.shade_color, "none");
        let fr = cp.font_ref.as_ref().unwrap();
        assert_eq!(fr.hangul, 1);
        assert_eq!(fr.latin, 1);
        assert!(cp.bold.is_none());
        assert!(cp.italic.is_none());
    }

    #[test]
    fn parse_char_pr_with_italic() {
        let xml = r##"
        <hh:head version="1.4" secCnt="1">
          <hh:refList>
            <hh:charProperties itemCnt="1">
              <hh:charPr id="7" height="1000" textColor="#FF0000" shadeColor="none"
                         useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="2">
                <hh:fontRef hangul="1" latin="1" hanja="1" japanese="1" other="1" symbol="1" user="1"/>
                <hh:italic/>
                <hh:underline type="NONE" shape="SOLID" color="#000000"/>
                <hh:strikeout shape="NONE" color="#000000"/>
                <hh:outline type="NONE"/>
                <hh:shadow type="NONE" color="#B2B2B2" offsetX="10" offsetY="10"/>
              </hh:charPr>
            </hh:charProperties>
          </hh:refList>
        </hh:head>"##;
        let head = parse_head(xml);
        let cp = &head.ref_list.unwrap().char_properties.unwrap().items[0];
        assert_eq!(cp.id, 7);
        assert_eq!(cp.text_color, "#FF0000");
        assert!(cp.italic.is_some(), "italic should be present");
        assert!(cp.bold.is_none());
    }

    #[test]
    fn parse_para_pr_with_switch() {
        let xml = r#"
        <hh:head version="1.4" secCnt="1">
          <hh:refList>
            <hh:paraProperties itemCnt="1">
              <hh:paraPr id="0" tabPrIDRef="0" condense="0">
                <hh:align horizontal="LEFT" vertical="BASELINE"/>
                <hp:switch>
                  <hp:case hp:required-namespace="http://www.hancom.co.kr/hwpml/2016/HwpUnitChar">
                    <hh:margin>
                      <hc:intent value="0" unit="HWPUNIT"/>
                      <hc:left value="0" unit="HWPUNIT"/>
                      <hc:right value="0" unit="HWPUNIT"/>
                      <hc:prev value="0" unit="HWPUNIT"/>
                      <hc:next value="0" unit="HWPUNIT"/>
                    </hh:margin>
                    <hh:lineSpacing type="PERCENT" value="130" unit="HWPUNIT"/>
                  </hp:case>
                  <hp:default>
                    <hh:margin>
                      <hc:intent value="0" unit="HWPUNIT"/>
                      <hc:left value="0" unit="HWPUNIT"/>
                      <hc:right value="0" unit="HWPUNIT"/>
                      <hc:prev value="0" unit="HWPUNIT"/>
                      <hc:next value="0" unit="HWPUNIT"/>
                    </hh:margin>
                    <hh:lineSpacing type="PERCENT" value="130" unit="HWPUNIT"/>
                  </hp:default>
                </hp:switch>
              </hh:paraPr>
            </hh:paraProperties>
          </hh:refList>
        </hh:head>"#;
        let head = parse_head(xml);
        let pp = &head.ref_list.unwrap().para_properties.unwrap().items[0];
        assert_eq!(pp.id, 0);
        let align = pp.align.as_ref().unwrap();
        assert_eq!(align.horizontal, "LEFT");
        let sw = pp.switches.first().unwrap();
        let def = sw.default.as_ref().unwrap();
        let margin = def.margin.as_ref().unwrap();
        assert_eq!(margin.left.as_ref().unwrap().value, 0);
        let ls = def.line_spacing.as_ref().unwrap();
        assert_eq!(ls.spacing_type, "PERCENT");
        assert_eq!(ls.value, 130);
    }

    #[test]
    fn parse_para_pr_with_non_zero_margin() {
        let xml = r#"
        <hh:head version="1.4" secCnt="1">
          <hh:refList>
            <hh:paraProperties itemCnt="1">
              <hh:paraPr id="4" tabPrIDRef="1" condense="20">
                <hh:align horizontal="JUSTIFY" vertical="BASELINE"/>
                <hp:switch>
                  <hp:default>
                    <hh:margin>
                      <hc:intent value="0" unit="HWPUNIT"/>
                      <hc:left value="14000" unit="HWPUNIT"/>
                      <hc:right value="0" unit="HWPUNIT"/>
                      <hc:prev value="0" unit="HWPUNIT"/>
                      <hc:next value="0" unit="HWPUNIT"/>
                    </hh:margin>
                    <hh:lineSpacing type="PERCENT" value="160" unit="HWPUNIT"/>
                  </hp:default>
                </hp:switch>
              </hh:paraPr>
            </hh:paraProperties>
          </hh:refList>
        </hh:head>"#;
        let head = parse_head(xml);
        let pp = &head.ref_list.unwrap().para_properties.unwrap().items[0];
        let def = pp.switches.first().unwrap().default.as_ref().unwrap();
        let margin = def.margin.as_ref().unwrap();
        assert_eq!(margin.left.as_ref().unwrap().value, 14000);
        assert_eq!(def.line_spacing.as_ref().unwrap().value, 160);
    }

    #[test]
    fn parse_styles() {
        let xml = r#"
        <hh:head version="1.4" secCnt="1">
          <hh:refList>
            <hh:styles itemCnt="2">
              <hh:style id="0" type="PARA" name="바탕글" engName="Normal"
                        paraPrIDRef="3" charPrIDRef="0" nextStyleIDRef="0" langID="1042"/>
              <hh:style id="1" type="PARA" name="본문" engName="Body"
                        paraPrIDRef="11" charPrIDRef="0" nextStyleIDRef="1" langID="1042"/>
            </hh:styles>
          </hh:refList>
        </hh:head>"#;
        let head = parse_head(xml);
        let styles = head.ref_list.unwrap().styles.unwrap();
        assert_eq!(styles.items.len(), 2);
        assert_eq!(styles.items[0].name, "바탕글");
        assert_eq!(styles.items[0].eng_name, "Normal");
        assert_eq!(styles.items[0].para_pr_id_ref, 3);
        assert_eq!(styles.items[0].char_pr_id_ref, 0);
        assert_eq!(styles.items[1].name, "본문");
    }

    #[test]
    fn empty_ref_list_no_panic() {
        let xml = r#"<hh:head version="1.4" secCnt="1"><hh:refList/></hh:head>"#;
        let head = parse_head(xml);
        let rl = head.ref_list.unwrap();
        assert!(rl.fontfaces.is_none());
        assert!(rl.char_properties.is_none());
        assert!(rl.para_properties.is_none());
        assert!(rl.styles.is_none());
    }

    #[test]
    fn unknown_elements_are_silently_skipped() {
        let xml = r#"
        <hh:head version="1.4" secCnt="1">
          <hh:refList>
            <hh:borderFills itemCnt="0"/>
            <hh:tabProperties itemCnt="0"/>
            <hh:numberings itemCnt="0"/>
            <hh:charProperties itemCnt="0"/>
            <hh:paraProperties itemCnt="0"/>
          </hh:refList>
          <hh:compatibleDocument targetProgram="HWP201X"/>
          <hh:docOption/>
          <hh:trackchageConfig flags="56"/>
        </hh:head>"#;
        let head = parse_head(xml);
        assert!(head.ref_list.is_some());
    }

    #[test]
    fn font_ref_defaults_to_zero() {
        let fr = HxFontRef::default();
        assert_eq!(fr.hangul, 0);
        assert_eq!(fr.latin, 0);
        assert_eq!(fr.user, 0);
    }
}
