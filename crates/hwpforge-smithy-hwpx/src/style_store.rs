//! HWPX-specific style storage.
//!
//! [`HwpxStyleStore`] is the **smithy-hwpx** analogue of Blueprint's
//! `StyleRegistry`, but much simpler: it stores only what was actually
//! found in `header.xml`, with zero inheritance logic.
//!
//! All fields use Foundation types (`Color`, `HwpUnit`, `Alignment`)
//! so downstream code never touches raw XML strings.

use serde::{Deserialize, Serialize};

use crate::color::parse_hex_color_or_black;
use crate::list_bridge::{heading_type_to_para_list_type, list_ref_to_wire_parts};
use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_core::{BulletDef, NumberingDef, StyleLookup, TabDef};
use hwpforge_foundation::{
    Alignment, BorderFillIndex, BreakType, CharShapeIndex, Color, EmbossType, EmphasisType,
    EngraveType, FontIndex, GradientType, HeadingType, HwpUnit, LineSpacingType, OutlineType,
    ParaShapeIndex, ShadowType, StrikeoutShape, StyleIndex, UnderlineType, VerticalPosition,
    WordBreakType,
};

use crate::default_styles::HancomStyleSet;
use crate::error::{HwpxError, HwpxResult};

// ── Font ─────────────────────────────────────────────────────────

/// A resolved font from `<hh:fontface>` → `<hh:font>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HwpxFont {
    /// Original `id` attribute from XML.
    pub id: u32,
    /// Face name (e.g. `"함초롬돋움"`, `"Times New Roman"`).
    pub face_name: String,
    /// Language group this font belongs to (e.g. `"HANGUL"`, `"LATIN"`).
    pub lang: String,
}

impl HwpxFont {
    /// Creates a new font entry.
    pub fn new(id: u32, face_name: impl Into<String>, lang: impl Into<String>) -> Self {
        Self { id, face_name: face_name.into(), lang: lang.into() }
    }
}

// ── Per-language font references ─────────────────────────────────

/// Per-language font index references from `<hh:fontRef>`.
///
/// Each field is a [`FontIndex`] pointing into the store's font list
/// for that language group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HwpxFontRef {
    /// Hangul (한글) font index.
    pub hangul: FontIndex,
    /// Latin font index.
    pub latin: FontIndex,
    /// Hanja (한자) font index.
    pub hanja: FontIndex,
    /// Japanese (日本語) font index.
    pub japanese: FontIndex,
    /// Other scripts font index.
    pub other: FontIndex,
    /// Symbol font index.
    pub symbol: FontIndex,
    /// User-defined font index.
    pub user: FontIndex,
}

impl Default for HwpxFontRef {
    fn default() -> Self {
        let zero = FontIndex::new(0);
        Self {
            hangul: zero,
            latin: zero,
            hanja: zero,
            japanese: zero,
            other: zero,
            symbol: zero,
            user: zero,
        }
    }
}

// ── Character Shape ──────────────────────────────────────────────

/// Resolved character properties from `<hh:charPr>`.
///
/// All raw XML strings have been converted to Foundation types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HwpxCharShape {
    /// Per-language font references.
    pub font_ref: HwpxFontRef,
    /// Font height in HwpUnit (height attribute × 1, already HWPUNIT).
    pub height: HwpUnit,
    /// Text color (from `textColor` attribute, e.g. `"#000000"`).
    pub text_color: Color,
    /// Background shade color (from `shadeColor`, `"none"` → None).
    pub shade_color: Option<Color>,
    /// Bold formatting.
    pub bold: bool,
    /// Italic formatting.
    pub italic: bool,
    /// Underline type (e.g. `None`, `Bottom`).
    pub underline_type: UnderlineType,
    /// Underline color (None = inherit text color).
    pub underline_color: Option<Color>,
    /// Strikeout shape (e.g. `None`, `Continuous`).
    pub strikeout_shape: StrikeoutShape,
    /// Strikeout color (None = inherit text color).
    pub strikeout_color: Option<Color>,
    /// Vertical position (Normal/Superscript/Subscript).
    pub vertical_position: VerticalPosition,
    /// Text outline type.
    pub outline_type: OutlineType,
    /// Drop shadow type.
    pub shadow_type: ShadowType,
    /// Emboss effect type.
    pub emboss_type: EmbossType,
    /// Engrave effect type.
    pub engrave_type: EngraveType,
    /// Emphasis mark type (from `symMark` attribute).
    pub emphasis: EmphasisType,
    /// Character width ratio (uniform, from `ratio` child element).
    pub ratio: i32,
    /// Inter-character spacing (uniform, from `spacing` child element).
    pub spacing: i32,
    /// Relative font size (uniform, from `relSz` child element).
    pub rel_sz: i32,
    /// Vertical position offset (uniform, from `offset` child element).
    pub char_offset: i32,
    /// Enable kerning (from `useKerning` attribute, 0/1).
    pub use_kerning: bool,
    /// Use font space (from `useFontSpace` attribute, 0/1).
    pub use_font_space: bool,
    /// Border/fill reference for character border (`borderFillIDRef`).
    ///
    /// `None` means use the default value of `2` (한글 default char background).
    /// Set to `Some(id)` to reference a custom `HwpxBorderFill` entry.
    pub border_fill_id: Option<u32>,
}

impl Default for HwpxCharShape {
    fn default() -> Self {
        Self {
            font_ref: HwpxFontRef::default(),
            height: HwpUnit::new(1000).unwrap(), // 10pt default (한글 compatible)
            text_color: Color::BLACK,
            shade_color: None,
            bold: false,
            italic: false,
            underline_type: UnderlineType::None,
            underline_color: None,
            strikeout_shape: StrikeoutShape::None,
            strikeout_color: None,
            vertical_position: VerticalPosition::Normal,
            outline_type: OutlineType::None,
            shadow_type: ShadowType::None,
            emboss_type: EmbossType::None,
            engrave_type: EngraveType::None,
            emphasis: EmphasisType::None,
            ratio: 100,
            spacing: 0,
            rel_sz: 100,
            char_offset: 0,
            use_kerning: false,
            use_font_space: false,
            border_fill_id: None,
        }
    }
}

// ── Style ────────────────────────────────────────────────────────

/// Resolved style definition from `<hh:style>`.
///
/// Stores style metadata like names and references to character/paragraph
/// properties. This enables full roundtrip of style names like "바탕글",
/// "본문", etc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HwpxStyle {
    /// Style ID (from `id` attribute).
    pub id: u32,
    /// Style type (e.g. `"PARA"`, `"CHAR"`).
    pub style_type: String,
    /// Korean style name (e.g. `"바탕글"`).
    pub name: String,
    /// English style name (e.g. `"Normal"`).
    pub eng_name: String,
    /// Reference to paragraph properties (from `paraPrIDRef`).
    pub para_pr_id_ref: u32,
    /// Reference to character properties (from `charPrIDRef`).
    pub char_pr_id_ref: u32,
    /// Reference to next style (from `nextStyleIDRef`).
    pub next_style_id_ref: u32,
    /// Language ID (from `langID`).
    pub lang_id: u32,
    /// Form lock flag (from `lockForm`).
    pub lock_form: u32,
}

impl HwpxStyle {
    /// Creates a style definition with explicit HWPX ids and references.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: u32,
        style_type: impl Into<String>,
        name: impl Into<String>,
        eng_name: impl Into<String>,
        para_pr_id_ref: u32,
        char_pr_id_ref: u32,
        next_style_id_ref: u32,
        lang_id: u32,
        lock_form: u32,
    ) -> Self {
        Self {
            id,
            style_type: style_type.into(),
            name: name.into(),
            eng_name: eng_name.into(),
            para_pr_id_ref,
            char_pr_id_ref,
            next_style_id_ref,
            lang_id,
            lock_form,
        }
    }
}

// ── Paragraph Shape ──────────────────────────────────────────────

/// Resolved paragraph properties from `<hh:paraPr>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HwpxParaShape {
    /// Horizontal alignment.
    pub alignment: Alignment,
    /// Left indent (from `<hc:left value="..."/>`).
    pub margin_left: HwpUnit,
    /// Right indent.
    pub margin_right: HwpUnit,
    /// Paragraph indent (from `<hc:intent value="..."/>`).
    pub indent: HwpUnit,
    /// Space before paragraph (from `<hc:prev value="..."/>`).
    pub spacing_before: HwpUnit,
    /// Space after paragraph (from `<hc:next value="..."/>`).
    pub spacing_after: HwpUnit,
    /// Line spacing value.
    pub line_spacing: i32,
    /// Line spacing type.
    pub line_spacing_type: LineSpacingType,

    // Advanced paragraph controls (NEW - Phase 6.2)
    /// Page/column break type before paragraph.
    pub break_type: BreakType,
    /// Keep paragraph with next (prevent page break between).
    pub keep_with_next: bool,
    /// Keep lines together (prevent page break within paragraph).
    pub keep_lines_together: bool,
    /// Widow/orphan control (minimum 2 lines at page boundaries).
    pub widow_orphan: bool,
    /// Word-breaking rule for Latin text (default: KeepWord).
    pub break_latin_word: WordBreakType,
    /// Word-breaking rule for non-Latin text including Korean (default: KeepWord).
    pub break_non_latin_word: WordBreakType,
    /// Border/fill reference (None = no border/fill).
    pub border_fill_id: Option<BorderFillIndex>,
    /// Heading type for this paragraph.
    pub heading_type: HeadingType,
    /// Heading numbering reference (idRef in heading element, 0 = none).
    pub heading_id_ref: u32,
    /// Heading outline level (0 = none, 1-10 for outline levels).
    pub heading_level: u32,
    /// Tab property reference (tabPrIDRef, 0 = default).
    pub tab_pr_id_ref: u32,
    /// Condense value for tight outline spacing.
    pub condense: u32,
}

impl Default for HwpxParaShape {
    fn default() -> Self {
        Self {
            alignment: Alignment::Left,
            margin_left: HwpUnit::ZERO,
            margin_right: HwpUnit::ZERO,
            indent: HwpUnit::ZERO,
            spacing_before: HwpUnit::ZERO,
            spacing_after: HwpUnit::ZERO,
            line_spacing: 160,
            line_spacing_type: LineSpacingType::Percentage,
            break_type: BreakType::None,
            keep_with_next: false,
            keep_lines_together: false,
            widow_orphan: true, // Enabled by default in HWPX
            break_latin_word: WordBreakType::KeepWord,
            break_non_latin_word: WordBreakType::KeepWord,
            border_fill_id: None,
            heading_type: HeadingType::None,
            heading_id_ref: 0,
            heading_level: 0,
            tab_pr_id_ref: 0,
            condense: 0,
        }
    }
}

// ── Border Fill ──────────────────────────────────────────────────

/// Resolved border/fill definition from `<hh:borderFill>`.
///
/// Stores border line styles for all 4 sides plus diagonal borders,
/// 3D/shadow flags, and optional fill configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HwpxBorderFill {
    /// Border fill ID (1-based, matching `borderFillIDRef` in charPr/paraPr).
    pub id: u32,
    /// Whether 3D border effect is enabled.
    pub three_d: bool,
    /// Whether shadow effect is enabled.
    pub shadow: bool,
    /// Center line type string (e.g. `"NONE"`).
    pub center_line: String,
    /// Left border line.
    pub left: HwpxBorderLine,
    /// Right border line.
    pub right: HwpxBorderLine,
    /// Top border line.
    pub top: HwpxBorderLine,
    /// Bottom border line.
    pub bottom: HwpxBorderLine,
    /// Diagonal border line (optional — omitted in some HWPX files).
    pub diagonal: Option<HwpxBorderLine>,
    /// Slash diagonal type string.
    pub slash_type: String,
    /// Back-slash diagonal type string.
    pub back_slash_type: String,
    /// Slash diagonal metadata.
    #[serde(default)]
    pub slash: HwpxDiagonalLine,
    /// Back-slash diagonal metadata.
    #[serde(default)]
    pub back_slash: HwpxDiagonalLine,
    /// Legacy `winBrush` fill configuration.
    ///
    /// Gradient and image border fills use `gradient_fill` / `image_fill`.
    pub fill: Option<HwpxFill>,
    /// Hatch pattern kind when `fill` uses `<hc:winBrush>`.
    #[serde(default)]
    pub fill_hatch_style: Option<String>,
    /// Gradient fill payload when `<hc:gradation>` is present.
    #[serde(default)]
    pub gradient_fill: Option<HwpxGradientFill>,
    /// Image fill payload when `<hc:imgBrush>` is present.
    #[serde(default)]
    pub image_fill: Option<HwpxImageFill>,
}

/// Resolved diagonal border metadata from `<hh:slash>` / `<hh:backSlash>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HwpxDiagonalLine {
    /// Border type string (for example, `"NONE"` or `"CENTER"`).
    pub border_type: String,
    /// Crooked diagonal flag.
    pub crooked: bool,
    /// Counter direction flag.
    pub is_counter: bool,
}

impl Default for HwpxDiagonalLine {
    fn default() -> Self {
        Self { border_type: "NONE".into(), crooked: false, is_counter: false }
    }
}

/// A single border line configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HwpxBorderLine {
    /// Border line type (e.g. `"NONE"`, `"SOLID"`).
    pub line_type: String,
    /// Width string (e.g. `"0.1 mm"`).
    pub width: String,
    /// Color string (e.g. `"#000000"`).
    pub color: String,
}

impl Default for HwpxBorderLine {
    fn default() -> Self {
        Self { line_type: "NONE".into(), width: "0.1 mm".into(), color: "#000000".into() }
    }
}

/// Fill brush configuration for a [`HwpxBorderFill`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HwpxFill {
    /// Solid or hatch fill via `<hc:winBrush>`.
    WinBrush {
        /// Face color string (e.g. `"none"`, `"#RRGGBB"`).
        face_color: String,
        /// Hatch pattern color string.
        hatch_color: String,
        /// Alpha transparency string.
        alpha: String,
    },
}

pub(crate) enum ActiveBorderFillBrush<'a> {
    None,
    WinBrush {
        face_color: &'a str,
        hatch_color: &'a str,
        hatch_style: Option<&'a str>,
        alpha: &'a str,
    },
    Gradient(&'a HwpxGradientFill),
    Image(&'a HwpxImageFill),
}

/// Gradient fill payload for table border fills.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HwpxGradientFill {
    /// Gradient type.
    pub gradient_type: GradientType,
    /// Gradient angle in degrees.
    pub angle: i32,
    /// Gradient center X percentage.
    pub center_x: i32,
    /// Gradient center Y percentage.
    pub center_y: i32,
    /// Gradient step count.
    pub step: i32,
    /// Gradient step center percentage.
    pub step_center: i32,
    /// Alpha transparency.
    pub alpha: i32,
    /// Ordered gradient color stops.
    pub colors: Vec<Color>,
}

/// Image fill payload for table border fills.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HwpxImageFill {
    /// HWPX image fill mode string (for example, `TOTAL`).
    pub mode: String,
    /// `binaryItemIDRef` value without extension.
    pub binary_item_id_ref: String,
    /// Brightness adjustment.
    pub bright: i32,
    /// Contrast adjustment.
    pub contrast: i32,
    /// HWPX effect string (for example, `REAL_PIC`).
    pub effect: String,
    /// Alpha transparency.
    pub alpha: i32,
}

impl HwpxBorderFill {
    /// Creates a fully-specified resolved border fill.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: u32,
        three_d: bool,
        shadow: bool,
        center_line: impl Into<String>,
        left: HwpxBorderLine,
        right: HwpxBorderLine,
        top: HwpxBorderLine,
        bottom: HwpxBorderLine,
        diagonal: Option<HwpxBorderLine>,
        slash: HwpxDiagonalLine,
        back_slash: HwpxDiagonalLine,
        fill: Option<HwpxFill>,
    ) -> Self {
        Self {
            id,
            three_d,
            shadow,
            center_line: center_line.into(),
            left,
            right,
            top,
            bottom,
            diagonal,
            slash_type: slash.border_type.clone(),
            back_slash_type: back_slash.border_type.clone(),
            slash,
            back_slash,
            fill,
            fill_hatch_style: None,
            gradient_fill: None,
            image_fill: None,
        }
    }

    /// Updates slash diagonal metadata and keeps the legacy type field in sync.
    pub fn set_slash(&mut self, slash: HwpxDiagonalLine) {
        self.slash_type = slash.border_type.clone();
        self.slash = slash;
    }

    /// Updates back-slash diagonal metadata and keeps the legacy type field in sync.
    pub fn set_back_slash(&mut self, back_slash: HwpxDiagonalLine) {
        self.back_slash_type = back_slash.border_type.clone();
        self.back_slash = back_slash;
    }

    /// Clears every mutually exclusive border-fill brush payload.
    pub fn clear_fill_brush(&mut self) {
        self.fill = None;
        self.fill_hatch_style = None;
        self.gradient_fill = None;
        self.image_fill = None;
    }

    /// Sets a legacy `winBrush` fill and clears gradient/image payloads.
    pub fn set_win_brush_fill(
        &mut self,
        face_color: impl Into<String>,
        hatch_color: impl Into<String>,
        alpha: impl Into<String>,
        hatch_style: Option<String>,
    ) {
        self.clear_fill_brush();
        self.fill = Some(HwpxFill::WinBrush {
            face_color: face_color.into(),
            hatch_color: hatch_color.into(),
            alpha: alpha.into(),
        });
        self.fill_hatch_style = hatch_style;
    }

    /// Sets a gradient fill and clears competing border-fill brush payloads.
    pub fn set_gradient_fill(&mut self, fill: HwpxGradientFill) {
        self.clear_fill_brush();
        self.gradient_fill = Some(fill);
    }

    /// Sets an image fill and clears competing border-fill brush payloads.
    pub fn set_image_fill(&mut self, fill: HwpxImageFill) {
        self.clear_fill_brush();
        self.image_fill = Some(fill);
    }

    /// Returns the slash diagonal type after reconciling legacy and rich fields.
    pub fn effective_slash_type(&self) -> &str {
        self.effective_diagonal_type(&self.slash, &self.slash_type)
    }

    /// Returns the back-slash diagonal type after reconciling legacy and rich fields.
    pub fn effective_back_slash_type(&self) -> &str {
        self.effective_diagonal_type(&self.back_slash, &self.back_slash_type)
    }

    pub(crate) fn active_fill_brush(&self) -> ActiveBorderFillBrush<'_> {
        debug_assert!(
            self.fill_source_count() <= 1,
            "HwpxBorderFill has conflicting fill payloads: fill={}, gradient_fill={}, image_fill={}",
            self.fill.is_some(),
            self.gradient_fill.is_some(),
            self.image_fill.is_some()
        );

        if let Some(fill) = &self.image_fill {
            return ActiveBorderFillBrush::Image(fill);
        }
        if let Some(fill) = &self.gradient_fill {
            return ActiveBorderFillBrush::Gradient(fill);
        }
        match self.fill.as_ref() {
            Some(HwpxFill::WinBrush { face_color, hatch_color, alpha }) => {
                ActiveBorderFillBrush::WinBrush {
                    face_color,
                    hatch_color,
                    hatch_style: self.fill_hatch_style.as_deref(),
                    alpha,
                }
            }
            None => ActiveBorderFillBrush::None,
        }
    }

    pub(crate) fn fill_source_count(&self) -> usize {
        usize::from(self.fill.is_some())
            + usize::from(self.gradient_fill.is_some())
            + usize::from(self.image_fill.is_some())
    }

    fn effective_diagonal_type<'a>(
        &self,
        diagonal: &'a HwpxDiagonalLine,
        legacy_border_type: &'a str,
    ) -> &'a str {
        if diagonal.border_type != "NONE" {
            diagonal.border_type.as_str()
        } else {
            legacy_border_type
        }
    }

    /// Default border fill id=1: empty borders, no fill (used for page borders).
    ///
    /// Matches the first entry of the legacy `BORDER_FILLS_XML` constant.
    pub fn default_page_border() -> Self {
        let none_border = HwpxBorderLine::default(); // NONE, 0.1 mm, #000000
        Self {
            id: 1,
            three_d: false,
            shadow: false,
            center_line: "NONE".into(),
            left: none_border.clone(),
            right: none_border.clone(),
            top: none_border.clone(),
            bottom: none_border.clone(),
            diagonal: Some(HwpxBorderLine {
                line_type: "SOLID".into(),
                ..HwpxBorderLine::default()
            }),
            slash_type: "NONE".into(),
            back_slash_type: "NONE".into(),
            slash: HwpxDiagonalLine::default(),
            back_slash: HwpxDiagonalLine::default(),
            fill: None,
            fill_hatch_style: None,
            gradient_fill: None,
            image_fill: None,
        }
    }

    /// Default border fill id=2: char background with `winBrush` fill.
    ///
    /// This is referenced by every `<hh:charPr borderFillIDRef="2">`.
    /// Matches the second entry of the legacy `BORDER_FILLS_XML` constant.
    pub fn default_char_background() -> Self {
        let none_border = HwpxBorderLine::default();
        Self {
            id: 2,
            three_d: false,
            shadow: false,
            center_line: "NONE".into(),
            left: none_border.clone(),
            right: none_border.clone(),
            top: none_border.clone(),
            bottom: none_border.clone(),
            diagonal: Some(HwpxBorderLine {
                line_type: "SOLID".into(),
                ..HwpxBorderLine::default()
            }),
            slash_type: "NONE".into(),
            back_slash_type: "NONE".into(),
            slash: HwpxDiagonalLine::default(),
            back_slash: HwpxDiagonalLine::default(),
            fill: Some(HwpxFill::WinBrush {
                face_color: "none".into(),
                hatch_color: "#FF000000".into(),
                alpha: "0".into(),
            }),
            fill_hatch_style: None,
            gradient_fill: None,
            image_fill: None,
        }
    }

    /// Default border fill id=3: SOLID borders on all 4 sides (used for table cells).
    ///
    /// Matches the third entry of the legacy `BORDER_FILLS_XML` constant.
    pub fn default_table_border() -> Self {
        let solid_border = HwpxBorderLine {
            line_type: "SOLID".into(),
            width: "0.12 mm".into(),
            color: "#000000".into(),
        };
        Self {
            id: 3,
            three_d: false,
            shadow: false,
            center_line: "NONE".into(),
            left: solid_border.clone(),
            right: solid_border.clone(),
            top: solid_border.clone(),
            bottom: solid_border.clone(),
            diagonal: Some(HwpxBorderLine {
                line_type: "SOLID".into(),
                ..HwpxBorderLine::default()
            }),
            slash_type: "NONE".into(),
            back_slash_type: "NONE".into(),
            slash: HwpxDiagonalLine::default(),
            back_slash: HwpxDiagonalLine::default(),
            fill: None,
            fill_hatch_style: None,
            gradient_fill: None,
            image_fill: None,
        }
    }
}

// ── Default shape definitions ────────────────────────────────────

/// Returns the 7 default character shapes for Modern (한글 2022+).
///
/// Extracted from golden fixture `tests/fixtures/textbox.hwpx` `Contents/header.xml`.
///
/// ```text
/// id=0: 함초롬바탕 10pt #000000  (바탕글/본문/개요1-7/캡션)
/// id=1: 함초롬돋움 10pt #000000  (쪽 번호)
/// id=2: 함초롬돋움  9pt #000000  (머리말)
/// id=3: 함초롬바탕  9pt #000000  (각주/미주)
/// id=4: 함초롬돋움  9pt #000000  (메모)
/// id=5: 함초롬돋움 16pt #2E74B5  (차례 제목)
/// id=6: 함초롬돋움 11pt #000000  (차례 1-3)
/// ```
///
/// Font indices: 0 = 함초롬돋움, 1 = 함초롬바탕 (as in fixture font table).
pub(crate) fn default_char_shapes_modern() -> [HwpxCharShape; 7] {
    let batang = FontIndex::new(1); // 함초롬바탕
    let dotum = FontIndex::new(0); // 함초롬돋움

    let batang_ref = HwpxFontRef {
        hangul: batang,
        latin: batang,
        hanja: batang,
        japanese: batang,
        other: batang,
        symbol: batang,
        user: batang,
    };
    let dotum_ref = HwpxFontRef {
        hangul: dotum,
        latin: dotum,
        hanja: dotum,
        japanese: dotum,
        other: dotum,
        symbol: dotum,
        user: dotum,
    };

    let base = HwpxCharShape {
        font_ref: batang_ref,
        height: HwpUnit::new(1000).unwrap(), // 10pt
        text_color: Color::BLACK,
        shade_color: None,
        bold: false,
        italic: false,
        underline_type: UnderlineType::None,
        underline_color: None,
        strikeout_shape: StrikeoutShape::None,
        strikeout_color: None,
        vertical_position: VerticalPosition::Normal,
        outline_type: OutlineType::None,
        shadow_type: ShadowType::None,
        emboss_type: EmbossType::None,
        engrave_type: EngraveType::None,
        emphasis: EmphasisType::None,
        ratio: 100,
        spacing: 0,
        rel_sz: 100,
        char_offset: 0,
        use_kerning: false,
        use_font_space: false,
        border_fill_id: None,
    };

    [
        // id=0: 함초롬바탕 10pt black (바탕글/본문/개요1-7/캡션)
        base.clone(),
        // id=1: 함초롬돋움 10pt black (쪽 번호)
        HwpxCharShape { font_ref: dotum_ref, ..base.clone() },
        // id=2: 함초롬돋움 9pt black (머리말)
        HwpxCharShape { font_ref: dotum_ref, height: HwpUnit::new(900).unwrap(), ..base.clone() },
        // id=3: 함초롬바탕 9pt black (각주/미주)
        HwpxCharShape { height: HwpUnit::new(900).unwrap(), ..base.clone() },
        // id=4: 함초롬돋움 9pt black (메모)
        HwpxCharShape { font_ref: dotum_ref, height: HwpUnit::new(900).unwrap(), ..base.clone() },
        // id=5: 함초롬돋움 16pt #2E74B5 (차례 제목)
        HwpxCharShape {
            font_ref: dotum_ref,
            height: HwpUnit::new(1600).unwrap(),
            text_color: Color::from_rgb(0x2E, 0x74, 0xB5),
            ..base.clone()
        },
        // id=6: 함초롬돋움 11pt black (차례 1-3)
        HwpxCharShape { font_ref: dotum_ref, height: HwpUnit::new(1100).unwrap(), ..base },
    ]
}

/// Returns the 20 default paragraph shapes for Modern (한글 2022+).
///
/// Extracted from golden fixture `tests/fixtures/textbox.hwpx` `Contents/header.xml`.
///
/// Values are in HWPUNIT (1pt = 100 HWPUNIT).
pub(crate) fn default_para_shapes_modern() -> [HwpxParaShape; 20] {
    let justify = Alignment::Justify;
    let left = Alignment::Left;

    // Base: JUSTIFY, no margins/indent, 160% line spacing, no widow/orphan
    let base = HwpxParaShape {
        alignment: justify,
        margin_left: HwpUnit::ZERO,
        margin_right: HwpUnit::ZERO,
        indent: HwpUnit::ZERO,
        spacing_before: HwpUnit::ZERO,
        spacing_after: HwpUnit::ZERO,
        line_spacing: 160,
        line_spacing_type: LineSpacingType::Percentage,
        break_type: BreakType::None,
        keep_with_next: false,
        keep_lines_together: false,
        widow_orphan: false,
        break_latin_word: WordBreakType::KeepWord,
        break_non_latin_word: WordBreakType::KeepWord,
        border_fill_id: None,
        heading_type: HeadingType::None,
        heading_id_ref: 0,
        heading_level: 0,
        tab_pr_id_ref: 0,
        condense: 0,
    };

    [
        //  0: 바탕글 — JUSTIFY left=0 160%
        base.clone(),
        //  1: 본문 — JUSTIFY left=1500 160%
        HwpxParaShape { margin_left: HwpUnit::new(1500).unwrap(), ..base.clone() },
        //  2: 개요 1 — JUSTIFY left=1000 160% OUTLINE level=1
        HwpxParaShape {
            margin_left: HwpUnit::new(1000).unwrap(),
            heading_type: HeadingType::Outline,
            heading_id_ref: 0,
            heading_level: 1,
            tab_pr_id_ref: 1,
            condense: 20,
            ..base.clone()
        },
        //  3: 개요 2 — JUSTIFY left=2000 160% OUTLINE level=2
        HwpxParaShape {
            margin_left: HwpUnit::new(2000).unwrap(),
            heading_type: HeadingType::Outline,
            heading_id_ref: 0,
            heading_level: 2,
            tab_pr_id_ref: 1,
            condense: 20,
            ..base.clone()
        },
        //  4: 개요 3 — JUSTIFY left=3000 160% OUTLINE level=3
        HwpxParaShape {
            margin_left: HwpUnit::new(3000).unwrap(),
            heading_type: HeadingType::Outline,
            heading_id_ref: 0,
            heading_level: 3,
            tab_pr_id_ref: 1,
            condense: 20,
            ..base.clone()
        },
        //  5: 개요 4 — JUSTIFY left=4000 160% OUTLINE level=4
        HwpxParaShape {
            margin_left: HwpUnit::new(4000).unwrap(),
            heading_type: HeadingType::Outline,
            heading_id_ref: 0,
            heading_level: 4,
            tab_pr_id_ref: 1,
            condense: 20,
            ..base.clone()
        },
        //  6: 개요 5 — JUSTIFY left=5000 160% OUTLINE level=5
        HwpxParaShape {
            margin_left: HwpUnit::new(5000).unwrap(),
            heading_type: HeadingType::Outline,
            heading_id_ref: 0,
            heading_level: 5,
            tab_pr_id_ref: 1,
            condense: 20,
            ..base.clone()
        },
        //  7: 개요 6 — JUSTIFY left=6000 160% OUTLINE level=6
        HwpxParaShape {
            margin_left: HwpUnit::new(6000).unwrap(),
            heading_type: HeadingType::Outline,
            heading_id_ref: 0,
            heading_level: 6,
            tab_pr_id_ref: 1,
            condense: 20,
            ..base.clone()
        },
        //  8: 개요 7 — JUSTIFY left=7000 160% OUTLINE level=7
        HwpxParaShape {
            margin_left: HwpUnit::new(7000).unwrap(),
            heading_type: HeadingType::Outline,
            heading_id_ref: 0,
            heading_level: 7,
            tab_pr_id_ref: 1,
            condense: 20,
            ..base.clone()
        },
        //  9: 머리말 — JUSTIFY left=0 150%
        HwpxParaShape { line_spacing: 150, ..base.clone() },
        // 10: 각주/미주 — JUSTIFY indent=-1310 130%
        HwpxParaShape { indent: HwpUnit::new(-1310).unwrap(), line_spacing: 130, ..base.clone() },
        // 11: 메모 — LEFT left=0 130%
        HwpxParaShape { alignment: left, line_spacing: 130, ..base.clone() },
        // 12: 차례 제목 — LEFT left=0 prev=1200 next=300 160%
        HwpxParaShape {
            alignment: left,
            spacing_before: HwpUnit::new(1200).unwrap(),
            spacing_after: HwpUnit::new(300).unwrap(),
            ..base.clone()
        },
        // 13: 차례 1 — LEFT left=0 next=700 160%
        HwpxParaShape {
            alignment: left,
            spacing_after: HwpUnit::new(700).unwrap(),
            ..base.clone()
        },
        // 14: 차례 2 — LEFT left=1100 next=700 160%
        HwpxParaShape {
            alignment: left,
            margin_left: HwpUnit::new(1100).unwrap(),
            spacing_after: HwpUnit::new(700).unwrap(),
            ..base.clone()
        },
        // 15: 차례 3 — LEFT left=2200 next=700 160%
        HwpxParaShape {
            alignment: left,
            margin_left: HwpUnit::new(2200).unwrap(),
            spacing_after: HwpUnit::new(700).unwrap(),
            ..base.clone()
        },
        // 16: 개요 9 (style 10→paraPr 16) — JUSTIFY left=9000 160% OUTLINE level=9
        HwpxParaShape {
            margin_left: HwpUnit::new(9000).unwrap(),
            heading_type: HeadingType::Outline,
            heading_id_ref: 0,
            heading_level: 9,
            tab_pr_id_ref: 1,
            condense: 20,
            ..base.clone()
        },
        // 17: 개요 10 (style 11→paraPr 17) — JUSTIFY left=10000 160% OUTLINE level=10
        HwpxParaShape {
            margin_left: HwpUnit::new(10000).unwrap(),
            heading_type: HeadingType::Outline,
            heading_id_ref: 0,
            heading_level: 10,
            tab_pr_id_ref: 1,
            condense: 20,
            ..base.clone()
        },
        // 18: 개요 8 (style 9→paraPr 18) — JUSTIFY left=8000 160% OUTLINE level=8
        HwpxParaShape {
            margin_left: HwpUnit::new(8000).unwrap(),
            heading_type: HeadingType::Outline,
            heading_id_ref: 0,
            heading_level: 8,
            tab_pr_id_ref: 1,
            condense: 20,
            ..base.clone()
        },
        // 19: 캡션 — JUSTIFY left=0 next=800 150%
        HwpxParaShape { line_spacing: 150, spacing_after: HwpUnit::new(800).unwrap(), ..base },
    ]
}

// ── Style Store ──────────────────────────────────────────────────

/// HWPX-specific style storage populated from `header.xml`.
///
/// Unlike Blueprint's `StyleRegistry`, this has no inheritance or
/// template merging — it holds exactly what was parsed from the file.
///
/// # Index Safety
///
/// All accessors return `HwpxResult<&T>` to guard against invalid
/// indices from malformed HWPX files.
///
/// # Examples
///
/// ```
/// use hwpforge_smithy_hwpx::HwpxStyleStore;
/// use hwpforge_foundation::CharShapeIndex;
///
/// let store = HwpxStyleStore::new();
/// assert!(store.char_shape(CharShapeIndex::new(0)).is_err());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HwpxStyleStore {
    /// The 한글 version style set used when injecting default styles.
    style_set: HancomStyleSet,
    fonts: Vec<HwpxFont>,
    char_shapes: Vec<HwpxCharShape>,
    para_shapes: Vec<HwpxParaShape>,
    styles: Vec<HwpxStyle>,
    border_fills: Vec<HwpxBorderFill>,
    numberings: Vec<NumberingDef>,
    bullets: Vec<BulletDef>,
    tabs: Vec<TabDef>,
}

impl HwpxStyleStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new style store with the given font registered for all 7 language groups
    /// (HANGUL, LATIN, HANJA, JAPANESE, OTHER, SYMBOL, USER).
    ///
    /// This eliminates the common boilerplate of manually pushing fonts for each language.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_smithy_hwpx::style_store::HwpxStyleStore;
    ///
    /// let store = HwpxStyleStore::with_default_fonts("함초롬돋움");
    /// assert_eq!(store.font_count(), 7);
    /// ```
    pub fn with_default_fonts(font_name: &str) -> Self {
        let mut store: Self = Self::new();
        let langs: [&str; 7] = ["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"];
        for &lang in &langs {
            // `fontRef` indices are group-local in HWPX, so a single default font
            // per language group must always use local id 0.
            store.push_font(HwpxFont::new(0, font_name, lang));
        }
        store
    }

    /// Returns the style set used by this store.
    pub fn style_set(&self) -> HancomStyleSet {
        self.style_set
    }

    /// Creates a store from a Blueprint [`StyleRegistry`] using the default
    /// style set ([`HancomStyleSet::Modern`]).
    ///
    /// This is the **bridge** that lets the MD → Core → HWPX pipeline
    /// carry resolved styles all the way through to the HWPX encoder.
    ///
    /// To target a specific 한글 version, use [`from_registry_with`][Self::from_registry_with].
    pub fn from_registry(registry: &StyleRegistry) -> HwpxResult<Self> {
        Self::from_registry_with(registry, HancomStyleSet::default())
    }

    /// Creates a store from a Blueprint [`StyleRegistry`] with a specific style set.
    ///
    /// The `style_set` controls which default styles are injected:
    /// - [`Classic`][HancomStyleSet::Classic] — 18 styles (한글 2014–2020)
    /// - [`Modern`][HancomStyleSet::Modern] — 22 styles (한글 2022+)
    /// - [`Latest`][HancomStyleSet::Latest] — 23 styles (한글 2025+)
    ///
    /// Mapping:
    /// - `registry.fonts` → [`HwpxFont`] (assigned to HANGUL group)
    /// - `registry.char_shapes` → [`HwpxCharShape`] (font ref mirrors same index for all lang groups)
    /// - `registry.para_shapes` → [`HwpxParaShape`]
    /// - `registry.style_entries` → [`HwpxStyle`] (PARA type, Korean langID)
    pub fn from_registry_with(
        registry: &StyleRegistry,
        style_set: HancomStyleSet,
    ) -> HwpxResult<Self> {
        let mut store = Self { style_set, ..Self::default() };

        // Step 1: Ensure 한글-compatible fonts exist
        // If registry has no fonts, inject default Korean fonts
        let has_fonts = !registry.fonts.is_empty();
        let default_font = if has_fonts {
            registry.fonts[0].as_str()
        } else {
            "함초롬바탕" // Fallback if no fonts in registry
        };

        // Fonts: FontId → HwpxFont (mirrored across all 7 language groups)
        // 한글 expects identical font entries for each language group.
        const FONT_LANGS: &[&str] =
            &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"];

        if has_fonts {
            for &lang in FONT_LANGS {
                for (i, font_id) in registry.fonts.iter().enumerate() {
                    store.push_font(HwpxFont {
                        id: i as u32,
                        face_name: font_id.as_str().to_string(),
                        lang: lang.to_string(),
                    });
                }
            }
        } else {
            // No fonts in registry - inject minimal default
            for &lang in FONT_LANGS {
                store.push_font(HwpxFont {
                    id: 0,
                    face_name: default_font.to_string(),
                    lang: lang.to_string(),
                });
            }
        }

        // Step 2: Inject 7 default charShapes and 20 default paraShapes (Modern).
        //
        // These MUST come first so that default styles can reference them by
        // group index (char_pr_group / para_pr_group from DefaultStyleEntry).
        // User shapes are pushed after and start at offset 7 / 20.
        //
        // Classic and Latest share the same shape definitions (only the style
        // table and its charPr/paraPr references differ).
        for cs in default_char_shapes_modern() {
            store.push_char_shape(cs);
        }
        for ps in default_para_shapes_modern() {
            store.push_para_shape(ps);
        }

        // Offsets for user-defined shapes (placed after the 7+20 defaults).
        let char_shape_offset = store.char_shape_count(); // 7
        let para_shape_offset = store.para_shape_count(); // 20

        // Step 3: Push user charShapes from Blueprint (indices start at offset).
        for cs in &registry.char_shapes {
            let font_idx = registry
                .fonts
                .iter()
                .position(|f| f.as_str() == cs.font)
                .map(FontIndex::new)
                .unwrap_or(FontIndex::new(0));
            let font_ref = HwpxFontRef {
                hangul: font_idx,
                latin: font_idx,
                hanja: font_idx,
                japanese: font_idx,
                other: font_idx,
                symbol: font_idx,
                user: font_idx,
            };
            store.push_char_shape(HwpxCharShape {
                font_ref,
                height: cs.size,
                text_color: cs.color,
                shade_color: cs.shade_color,
                bold: cs.bold,
                italic: cs.italic,
                underline_type: cs.underline_type,
                underline_color: cs.underline_color,
                strikeout_shape: cs.strikeout_shape,
                strikeout_color: cs.strikeout_color,
                vertical_position: cs.vertical_position,
                outline_type: cs.outline,
                shadow_type: cs.shadow,
                emboss_type: cs.emboss,
                engrave_type: cs.engrave,
                emphasis: cs.emphasis,
                ratio: cs.ratio,
                spacing: cs.spacing,
                rel_sz: cs.rel_sz,
                char_offset: cs.offset,
                use_kerning: cs.use_kerning,
                use_font_space: cs.use_font_space,
                border_fill_id: cs.char_border_fill_id,
            });
        }

        // Step 4: Push user paraShapes from Blueprint (indices start at offset).
        for numbering in &registry.numberings {
            store.push_numbering(numbering.clone());
        }
        for bullet in &registry.bullets {
            store.push_bullet(bullet.clone());
        }
        for ps in &registry.para_shapes {
            let (heading_type, heading_id_ref, heading_level) =
                list_ref_to_wire_parts(ps.list, &registry.numberings, &registry.bullets)?;
            store.push_para_shape(HwpxParaShape {
                alignment: ps.alignment,
                margin_left: ps.indent_left,
                margin_right: ps.indent_right,
                indent: ps.indent_first_line,
                spacing_before: ps.space_before,
                spacing_after: ps.space_after,
                line_spacing: ps.line_spacing_value.round() as i32,
                line_spacing_type: ps.line_spacing_type,
                break_type: ps.break_type,
                keep_with_next: ps.keep_with_next,
                keep_lines_together: ps.keep_lines_together,
                widow_orphan: ps.widow_orphan,
                break_latin_word: WordBreakType::KeepWord,
                break_non_latin_word: WordBreakType::KeepWord,
                border_fill_id: ps.border_fill_id,
                heading_type,
                heading_id_ref,
                heading_level,
                tab_pr_id_ref: ps.tab_def_id,
                condense: 0,
            });
        }

        // Step 4.25: Push shared tab definitions from Blueprint.
        //
        // The encoder merges these with the canonical 한글 defaults (id=0..2)
        // so user registries only need to provide explicit custom tab stops.
        for tab in &registry.tabs {
            store.push_tab(tab.clone());
        }

        // Step 4.5: Inject 3 default border fills for backward compatibility.
        // These must always be present; user-defined fills get id=4+.
        store.push_border_fill(HwpxBorderFill::default_page_border()); // id=1
        store.push_border_fill(HwpxBorderFill::default_char_background()); // id=2
        store.push_border_fill(HwpxBorderFill::default_table_border()); // id=3

        // Step 5: Inject default styles with per-style charPr/paraPr group refs.
        // The group indices are verified against golden fixture textbox.hwpx.
        let defaults = store.style_set.default_styles();
        for (idx, entry) in defaults.iter().enumerate() {
            let next_style_id_ref = if entry.is_char_style() { 0 } else { idx as u32 };
            store.push_style(HwpxStyle {
                id: idx as u32,
                style_type: entry.style_type.to_string(),
                name: entry.name.to_string(),
                eng_name: entry.eng_name.to_string(),
                para_pr_id_ref: entry.para_pr_group as u32,
                char_pr_id_ref: entry.char_pr_group as u32,
                next_style_id_ref,
                lang_id: 1042, // Korean
                lock_form: 0,
            });
        }

        // Step 6: Add user's styles from registry (starting after defaults).
        // User charPr/paraPr refs are offset-adjusted so they point at the
        // user shapes in the store (which start after the 7/20 defaults).
        let style_offset = defaults.len();
        for (i, (name, entry)) in registry.style_entries.iter().enumerate() {
            store.push_style(HwpxStyle {
                id: (style_offset + i) as u32,
                style_type: "PARA".to_string(),
                name: name.clone(),
                eng_name: name.clone(),
                para_pr_id_ref: (entry.para_shape_id.get() + para_shape_offset) as u32,
                char_pr_id_ref: (entry.char_shape_id.get() + char_shape_offset) as u32,
                next_style_id_ref: 0,
                lang_id: 1042, // Korean
                lock_form: 0,
            });
        }

        Ok(store)
    }

    // ── Fonts ────────────────────────────────────────────────────

    /// Adds a font and returns its index.
    pub fn push_font(&mut self, font: HwpxFont) -> FontIndex {
        let idx = FontIndex::new(self.fonts.len());
        self.fonts.push(font);
        idx
    }

    /// Returns the font at `index`.
    pub fn font(&self, index: FontIndex) -> HwpxResult<&HwpxFont> {
        self.fonts.get(index.get()).ok_or_else(|| HwpxError::IndexOutOfBounds {
            kind: "font",
            index: index.get() as u32,
            max: self.fonts.len() as u32,
        })
    }

    /// Returns the number of fonts.
    pub fn font_count(&self) -> usize {
        self.fonts.len()
    }

    /// Replaces all font entries matching `old_face` with `new_face`.
    ///
    /// This is used by the restyle tool to change the base font of a decoded
    /// document without destroying char/para shape indices. Font entries are
    /// replicated across 7 language groups, so all matching entries are updated.
    ///
    /// Specialty fonts (e.g., D2Coding for code blocks) are preserved because
    /// they don't match `old_face`.
    pub fn replace_font(&mut self, old_face: &str, new_face: &str) {
        for font in &mut self.fonts {
            if font.face_name == old_face {
                font.face_name = new_face.to_string();
            }
        }
    }

    // ── Character Shapes ─────────────────────────────────────────

    /// Adds a char shape and returns its index.
    pub fn push_char_shape(&mut self, shape: HwpxCharShape) -> CharShapeIndex {
        let idx = CharShapeIndex::new(self.char_shapes.len());
        self.char_shapes.push(shape);
        idx
    }

    /// Returns the char shape at `index`.
    pub fn char_shape(&self, index: CharShapeIndex) -> HwpxResult<&HwpxCharShape> {
        self.char_shapes.get(index.get()).ok_or_else(|| HwpxError::IndexOutOfBounds {
            kind: "char_shape",
            index: index.get() as u32,
            max: self.char_shapes.len() as u32,
        })
    }

    /// Returns the number of char shapes.
    pub fn char_shape_count(&self) -> usize {
        self.char_shapes.len()
    }

    // ── Paragraph Shapes ─────────────────────────────────────────

    /// Adds a para shape and returns its index.
    pub fn push_para_shape(&mut self, shape: HwpxParaShape) -> ParaShapeIndex {
        let idx = ParaShapeIndex::new(self.para_shapes.len());
        self.para_shapes.push(shape);
        idx
    }

    /// Returns the para shape at `index`.
    pub fn para_shape(&self, index: ParaShapeIndex) -> HwpxResult<&HwpxParaShape> {
        self.para_shapes.get(index.get()).ok_or_else(|| HwpxError::IndexOutOfBounds {
            kind: "para_shape",
            index: index.get() as u32,
            max: self.para_shapes.len() as u32,
        })
    }

    /// Returns the number of para shapes.
    pub fn para_shape_count(&self) -> usize {
        self.para_shapes.len()
    }

    // ── Iterators ────────────────────────────────────────────────

    /// Returns an iterator over all fonts in the store.
    pub fn iter_fonts(&self) -> impl Iterator<Item = &HwpxFont> {
        self.fonts.iter()
    }

    /// Returns an iterator over all character shapes in the store.
    pub fn iter_char_shapes(&self) -> impl Iterator<Item = &HwpxCharShape> {
        self.char_shapes.iter()
    }

    /// Returns an iterator over all paragraph shapes in the store.
    pub fn iter_para_shapes(&self) -> impl Iterator<Item = &HwpxParaShape> {
        self.para_shapes.iter()
    }

    // ── Styles ───────────────────────────────────────────────────

    /// Adds a style definition.
    pub fn push_style(&mut self, style: HwpxStyle) {
        self.styles.push(style);
    }

    /// Returns the style at `index`.
    pub fn style(&self, index: usize) -> HwpxResult<&HwpxStyle> {
        self.styles.get(index).ok_or(HwpxError::IndexOutOfBounds {
            kind: "style",
            index: index as u32,
            max: self.styles.len() as u32,
        })
    }

    /// Returns the number of styles.
    pub fn style_count(&self) -> usize {
        self.styles.len()
    }

    /// Returns an iterator over all styles in the store.
    pub fn iter_styles(&self) -> impl Iterator<Item = &HwpxStyle> {
        self.styles.iter()
    }

    // ── Border Fills ─────────────────────────────────────────────

    /// Adds a border fill to the store and returns its 1-based ID.
    ///
    /// Border fill IDs in HWPX are 1-based (unlike other indices which are 0-based).
    pub fn push_border_fill(&mut self, bf: HwpxBorderFill) -> u32 {
        let id = bf.id;
        self.border_fills.push(bf);
        id
    }

    /// Returns the border fill with the given 1-based ID.
    ///
    /// # Errors
    ///
    /// Returns [`HwpxError::IndexOutOfBounds`] if no border fill with that ID exists.
    pub fn border_fill(&self, id: u32) -> HwpxResult<&HwpxBorderFill> {
        self.border_fills.iter().find(|bf| bf.id == id).ok_or(HwpxError::IndexOutOfBounds {
            kind: "border_fill",
            index: id,
            max: self.border_fills.len() as u32,
        })
    }

    /// Returns the number of border fills in the store.
    pub fn border_fill_count(&self) -> usize {
        self.border_fills.len()
    }

    /// Returns an iterator over all border fills in the store.
    pub fn iter_border_fills(&self) -> impl Iterator<Item = &HwpxBorderFill> {
        self.border_fills.iter()
    }

    /// Adds a numbering definition to the store.
    pub fn push_numbering(&mut self, ndef: NumberingDef) {
        self.numberings.push(ndef);
    }

    /// Adds a bullet definition to the store.
    pub fn push_bullet(&mut self, bullet: BulletDef) {
        self.bullets.push(bullet);
    }

    /// Adds a tab property definition to the store.
    pub fn push_tab(&mut self, tab: TabDef) {
        self.tabs.push(tab);
    }

    /// Returns the number of numbering definitions in the store.
    pub fn numbering_count(&self) -> u32 {
        self.numberings.len() as u32
    }

    /// Returns the number of bullet definitions in the store.
    pub fn bullet_count(&self) -> u32 {
        self.bullets.len() as u32
    }

    /// Returns the number of tab property definitions in the store.
    pub fn tab_count(&self) -> u32 {
        self.tabs.len() as u32
    }

    /// Returns an iterator over all numbering definitions in the store.
    pub fn iter_numberings(&self) -> impl Iterator<Item = &NumberingDef> {
        self.numberings.iter()
    }

    /// Returns an iterator over all bullet definitions in the store.
    pub fn iter_bullets(&self) -> impl Iterator<Item = &BulletDef> {
        self.bullets.iter()
    }

    /// Returns an iterator over all tab property definitions in the store.
    pub fn iter_tabs(&self) -> impl Iterator<Item = &TabDef> {
        self.tabs.iter()
    }
}

// ── Thread safety assertions ─────────────────────────────────────

const _: () = {
    fn assert_send_sync<T: Send + Sync>() {}
    let _ = assert_send_sync::<HwpxStyleStore>;
};

// ── StyleLookup implementation ───────────────────────────────────

/// Parses a heading level from a Korean style name.
///
/// Recognizes the following patterns:
/// - `"개요 N"` (Outline N) → level `N` (1–6, clamped)
/// - `"+제목"`, `"타이들"`, `"큰제목"` → level 1
///
/// Returns `None` for non-heading style names.
///
/// # Examples
///
/// ```
/// use hwpforge_smithy_hwpx::style_store::parse_heading_level_from_name;
///
/// assert_eq!(parse_heading_level_from_name("개요 1"), Some(1));
/// assert_eq!(parse_heading_level_from_name("개요 7"), Some(6));
/// assert_eq!(parse_heading_level_from_name("+제목"), Some(1));
/// assert_eq!(parse_heading_level_from_name("바탕글"), None);
/// ```
pub fn parse_heading_level_from_name(name: &str) -> Option<u8> {
    let trimmed = name.trim();

    // "개요 N" pattern
    if let Some(suffix) = trimmed.strip_prefix("개요 ") {
        if let Ok(n) = suffix.trim().parse::<u8>() {
            // Clamp to Markdown's 1–6 range
            return Some(n.clamp(1, 6));
        }
    }

    // Known title-level style names → heading 1
    // "제목" matches only the exact standalone name, not names that merely contain
    // "제목" as a substring (e.g. "제목없음" must remain None).
    match trimmed {
        "+제목" | "제목" | "타이들" | "큰제목" => Some(1),
        _ => None,
    }
}

fn heading_level_from_para_shape(ps: &HwpxParaShape) -> Option<u8> {
    if ps.heading_type == HeadingType::Outline && ps.heading_level > 0 {
        Some((ps.heading_level as u8).clamp(1, 6))
    } else {
        None
    }
}

fn list_level_from_para_shape(ps: &HwpxParaShape) -> Option<u8> {
    match ps.heading_type {
        HeadingType::Number | HeadingType::Bullet => u8::try_from(ps.heading_level).ok(),
        HeadingType::Outline | HeadingType::None => None,
        _ => None,
    }
}

impl StyleLookup for HwpxStyleStore {
    fn char_bold(&self, id: CharShapeIndex) -> Option<bool> {
        self.char_shapes.get(id.get()).map(|cs| cs.bold)
    }

    fn char_italic(&self, id: CharShapeIndex) -> Option<bool> {
        self.char_shapes.get(id.get()).map(|cs| cs.italic)
    }

    fn char_underline(&self, id: CharShapeIndex) -> Option<UnderlineType> {
        self.char_shapes.get(id.get()).map(|cs| cs.underline_type)
    }

    fn char_strikeout(&self, id: CharShapeIndex) -> Option<bool> {
        self.char_shapes.get(id.get()).map(|cs| cs.strikeout_shape != StrikeoutShape::None)
    }

    fn char_superscript(&self, id: CharShapeIndex) -> Option<bool> {
        self.char_shapes
            .get(id.get())
            .map(|cs| cs.vertical_position == VerticalPosition::Superscript)
    }

    fn char_subscript(&self, id: CharShapeIndex) -> Option<bool> {
        self.char_shapes.get(id.get()).map(|cs| cs.vertical_position == VerticalPosition::Subscript)
    }

    fn char_font_name(&self, id: CharShapeIndex) -> Option<&str> {
        let cs = self.char_shapes.get(id.get())?;
        let font = self.fonts.get(cs.font_ref.hangul.get())?;
        Some(font.face_name.as_str())
    }

    fn char_font_size(&self, id: CharShapeIndex) -> Option<HwpUnit> {
        self.char_shapes.get(id.get()).map(|cs| cs.height)
    }

    fn char_text_color(&self, id: CharShapeIndex) -> Option<Color> {
        self.char_shapes.get(id.get()).map(|cs| cs.text_color)
    }

    fn para_alignment(&self, id: ParaShapeIndex) -> Option<Alignment> {
        self.para_shapes.get(id.get()).map(|ps| ps.alignment)
    }

    fn para_list_type(&self, id: ParaShapeIndex) -> Option<&str> {
        let ps = self.para_shapes.get(id.get())?;
        heading_type_to_para_list_type(ps.heading_type)
    }

    fn para_list_level(&self, id: ParaShapeIndex) -> Option<u8> {
        let ps = self.para_shapes.get(id.get())?;
        list_level_from_para_shape(ps)
    }

    fn para_heading_level(&self, id: ParaShapeIndex) -> Option<u8> {
        let ps = self.para_shapes.get(id.get())?;
        heading_level_from_para_shape(ps)
    }

    fn style_name(&self, id: StyleIndex) -> Option<&str> {
        self.styles.get(id.get()).map(|s| s.name.as_str())
    }

    fn style_heading_level(&self, id: StyleIndex) -> Option<u8> {
        let style = self.styles.get(id.get())?;
        if let Some(para_shape) = self.para_shapes.get(style.para_pr_id_ref as usize) {
            if let Some(level) = heading_level_from_para_shape(para_shape) {
                return Some(level);
            }
        }
        parse_heading_level_from_name(&style.name)
    }

    fn image_data(&self, _key: &str) -> Option<&[u8]> {
        // ImageStore is separate from HwpxStyleStore; use HwpxStyleLookup bridge instead.
        None
    }
}

// ── Color parsing helper ─────────────────────────────────────────

/// Parses a HWPX hex color string (`"#RRGGBB"`) into a [`Color`].
///
/// Returns `Color::BLACK` for `"none"`, empty strings, or invalid formats.
/// This is intentionally lenient: real-world HWPX files sometimes contain
/// non-standard color values, and rejecting them would make the decoder
/// unusable for slightly malformed documents.
pub(crate) fn parse_hex_color(s: &str) -> Color {
    parse_hex_color_or_black(s)
}

/// Parses a HWPX alignment string into an [`Alignment`].
///
/// Defaults to `Alignment::Left` for unknown values.
pub(crate) fn parse_alignment(s: &str) -> Alignment {
    if s.eq_ignore_ascii_case("LEFT") {
        Alignment::Left
    } else if s.eq_ignore_ascii_case("BOTH") || s.eq_ignore_ascii_case("JUSTIFY") {
        Alignment::Justify
    } else if s.eq_ignore_ascii_case("CENTER") {
        Alignment::Center
    } else if s.eq_ignore_ascii_case("RIGHT") {
        Alignment::Right
    } else if s.eq_ignore_ascii_case("DISTRIBUTE") {
        Alignment::Distribute
    } else if s.eq_ignore_ascii_case("DISTRIBUTE_FLUSH") {
        Alignment::DistributeFlush
    } else {
        Alignment::Left
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_blueprint::builtins::builtin_default;
    use hwpforge_blueprint::{registry::StyleRegistry, style::ParaShape};
    use hwpforge_core::{ParagraphListRef, TabStop};
    use hwpforge_foundation::{
        Alignment, CharShapeIndex, FontIndex, HeadingType, HwpUnit, LineSpacingType,
        NumberFormatType, ParaShapeIndex, TabAlign, TabLeader,
    };

    // ── HwpxStyleStore basic operations ──────────────────────────

    #[test]
    fn empty_store_returns_errors() {
        let store = HwpxStyleStore::new();
        assert!(store.font(FontIndex::new(0)).is_err());
        assert!(store.char_shape(CharShapeIndex::new(0)).is_err());
        assert!(store.para_shape(ParaShapeIndex::new(0)).is_err());
    }

    #[test]
    fn push_and_get_font() {
        let mut store = HwpxStyleStore::new();
        let idx = store.push_font(HwpxFont {
            id: 0,
            face_name: "함초롬돋움".into(),
            lang: "HANGUL".into(),
        });
        assert_eq!(idx.get(), 0);
        let font = store.font(idx).unwrap();
        assert_eq!(font.face_name, "함초롬돋움");
        assert_eq!(font.lang, "HANGUL");
    }

    #[test]
    fn push_and_get_char_shape() {
        let mut store = HwpxStyleStore::new();
        let shape = HwpxCharShape {
            height: HwpUnit::new(1000).unwrap(),
            text_color: Color::from_rgb(255, 0, 0),
            bold: true,
            ..Default::default()
        };
        let idx = store.push_char_shape(shape);
        let cs = store.char_shape(idx).unwrap();
        assert_eq!(cs.height.as_i32(), 1000);
        assert_eq!(cs.text_color.red(), 255);
        assert!(cs.bold);
        assert!(!cs.italic);
    }

    #[test]
    fn push_and_get_para_shape() {
        let mut store = HwpxStyleStore::new();
        let shape =
            HwpxParaShape { alignment: Alignment::Center, line_spacing: 200, ..Default::default() };
        let idx = store.push_para_shape(shape);
        let ps = store.para_shape(idx).unwrap();
        assert_eq!(ps.alignment, Alignment::Center);
        assert_eq!(ps.line_spacing, 200);
    }

    #[test]
    fn push_and_get_bullet() {
        let mut store = HwpxStyleStore::new();
        let bullet = BulletDef {
            id: 1,
            bullet_char: "".into(),
            use_image: false,
            para_head: hwpforge_core::ParaHead {
                start: 0,
                level: 1,
                num_format: NumberFormatType::Digit,
                text: String::new(),
                checkable: false,
            },
        };
        store.push_bullet(bullet);

        assert_eq!(store.bullet_count(), 1);
        let fetched = store.iter_bullets().next().unwrap();
        assert_eq!(fetched.id, 1);
        assert_eq!(fetched.bullet_char, "");
        assert!(!fetched.use_image);
    }

    #[test]
    fn index_out_of_bounds_error() {
        let store = HwpxStyleStore::new();
        let err = store.char_shape(CharShapeIndex::new(42)).unwrap_err();
        match err {
            HwpxError::IndexOutOfBounds { kind, index, max } => {
                assert_eq!(kind, "char_shape");
                assert_eq!(index, 42);
                assert_eq!(max, 0);
            }
            _ => panic!("expected IndexOutOfBounds"),
        }
    }

    #[test]
    fn multiple_items_sequential_indices() {
        let mut store = HwpxStyleStore::new();
        for i in 0..5 {
            let idx = store.push_font(HwpxFont {
                id: i,
                face_name: format!("Font{i}"),
                lang: "LATIN".into(),
            });
            assert_eq!(idx.get(), i as usize);
        }
        assert_eq!(store.font_count(), 5);
        assert_eq!(store.font(FontIndex::new(3)).unwrap().face_name, "Font3");
    }

    #[test]
    fn count_methods() {
        let mut store = HwpxStyleStore::new();
        assert_eq!(store.font_count(), 0);
        assert_eq!(store.char_shape_count(), 0);
        assert_eq!(store.para_shape_count(), 0);

        store.push_font(HwpxFont { id: 0, face_name: "A".into(), lang: "LATIN".into() });
        store.push_char_shape(HwpxCharShape::default());
        store.push_char_shape(HwpxCharShape::default());
        store.push_para_shape(HwpxParaShape::default());

        assert_eq!(store.font_count(), 1);
        assert_eq!(store.char_shape_count(), 2);
        assert_eq!(store.para_shape_count(), 1);
    }

    // ── Iterator methods ───────────────────────────────────────────

    #[test]
    fn iter_fonts_yields_all() {
        let mut store = HwpxStyleStore::new();
        for i in 0..3 {
            store.push_font(HwpxFont {
                id: i,
                face_name: format!("Font{i}"),
                lang: "LATIN".into(),
            });
        }
        let names: Vec<&str> = store.iter_fonts().map(|f| f.face_name.as_str()).collect();
        assert_eq!(names, vec!["Font0", "Font1", "Font2"]);
    }

    #[test]
    fn iter_char_shapes_yields_all() {
        let mut store = HwpxStyleStore::new();
        store.push_char_shape(HwpxCharShape { bold: true, ..Default::default() });
        store.push_char_shape(HwpxCharShape { italic: true, ..Default::default() });
        let styles: Vec<(bool, bool)> =
            store.iter_char_shapes().map(|c| (c.bold, c.italic)).collect();
        assert_eq!(styles, vec![(true, false), (false, true)]);
    }

    #[test]
    fn iter_para_shapes_yields_all() {
        let mut store = HwpxStyleStore::new();
        store.push_para_shape(HwpxParaShape { line_spacing: 130, ..Default::default() });
        store.push_para_shape(HwpxParaShape { line_spacing: 200, ..Default::default() });
        let spacings: Vec<i32> = store.iter_para_shapes().map(|p| p.line_spacing).collect();
        assert_eq!(spacings, vec![130, 200]);
    }

    #[test]
    fn iter_empty_store() {
        let store = HwpxStyleStore::new();
        assert_eq!(store.iter_fonts().count(), 0);
        assert_eq!(store.iter_char_shapes().count(), 0);
        assert_eq!(store.iter_para_shapes().count(), 0);
    }

    // ── HwpxFontRef default ──────────────────────────────────────

    #[test]
    fn font_ref_default_all_zero() {
        let r = HwpxFontRef::default();
        assert_eq!(r.hangul.get(), 0);
        assert_eq!(r.latin.get(), 0);
        assert_eq!(r.hanja.get(), 0);
        assert_eq!(r.japanese.get(), 0);
        assert_eq!(r.other.get(), 0);
        assert_eq!(r.symbol.get(), 0);
        assert_eq!(r.user.get(), 0);
    }

    // ── HwpxCharShape default ────────────────────────────────────

    #[test]
    fn char_shape_default_values() {
        let cs = HwpxCharShape::default();
        assert_eq!(cs.height, HwpUnit::new(1000).unwrap()); // 10pt default
        assert_eq!(cs.text_color, Color::BLACK);
        assert_eq!(cs.shade_color, None);
        assert!(!cs.bold);
        assert!(!cs.italic);
        assert_eq!(cs.underline_type, UnderlineType::None);
        assert_eq!(cs.underline_color, None);
        assert_eq!(cs.strikeout_shape, StrikeoutShape::None);
        assert_eq!(cs.strikeout_color, None);
    }

    // ── HwpxParaShape default ────────────────────────────────────

    #[test]
    fn para_shape_default_values() {
        let ps = HwpxParaShape::default();
        assert_eq!(ps.alignment, Alignment::Left);
        assert_eq!(ps.margin_left, HwpUnit::ZERO);
        assert_eq!(ps.indent, HwpUnit::ZERO);
        assert_eq!(ps.line_spacing, 160);
        assert_eq!(ps.line_spacing_type, LineSpacingType::Percentage);
    }

    // ── parse_hex_color ──────────────────────────────────────────

    #[test]
    fn parse_hex_color_valid() {
        let c = parse_hex_color("#FF0000");
        assert_eq!(c.red(), 255);
        assert_eq!(c.green(), 0);
        assert_eq!(c.blue(), 0);
    }

    #[test]
    fn parse_hex_color_lowercase() {
        let c = parse_hex_color("#00ff00");
        assert_eq!(c.green(), 255);
    }

    #[test]
    fn parse_hex_color_no_hash() {
        let c = parse_hex_color("0000FF");
        assert_eq!(c.blue(), 255);
    }

    #[test]
    fn parse_hex_color_none_returns_black() {
        assert_eq!(parse_hex_color("none"), Color::BLACK);
        assert_eq!(parse_hex_color("NONE"), Color::BLACK);
    }

    #[test]
    fn parse_hex_color_empty_returns_black() {
        assert_eq!(parse_hex_color(""), Color::BLACK);
    }

    #[test]
    fn parse_hex_color_invalid_returns_black() {
        assert_eq!(parse_hex_color("#GGHHII"), Color::BLACK);
        assert_eq!(parse_hex_color("#FFF"), Color::BLACK); // too short
        assert_eq!(parse_hex_color("garbage"), Color::BLACK);
    }

    #[test]
    fn parse_hex_color_white() {
        let c = parse_hex_color("#FFFFFF");
        assert_eq!(c, Color::WHITE);
    }

    // ── parse_alignment ──────────────────────────────────────────

    #[test]
    fn parse_alignment_standard() {
        assert_eq!(parse_alignment("LEFT"), Alignment::Left);
        assert_eq!(parse_alignment("CENTER"), Alignment::Center);
        assert_eq!(parse_alignment("RIGHT"), Alignment::Right);
        assert_eq!(parse_alignment("JUSTIFY"), Alignment::Justify);
    }

    #[test]
    fn parse_alignment_both_maps_to_justify() {
        // HWPX: "BOTH" means 양쪽 맞춤 (Justify), not Left
        assert_eq!(parse_alignment("BOTH"), Alignment::Justify);
    }

    #[test]
    fn parse_alignment_case_insensitive() {
        assert_eq!(parse_alignment("center"), Alignment::Center);
        assert_eq!(parse_alignment("Right"), Alignment::Right);
    }

    #[test]
    fn parse_alignment_distribute() {
        assert_eq!(parse_alignment("DISTRIBUTE"), Alignment::Distribute);
        assert_eq!(parse_alignment("distribute"), Alignment::Distribute);
        assert_eq!(parse_alignment("DISTRIBUTE_FLUSH"), Alignment::DistributeFlush);
        assert_eq!(parse_alignment("distribute_flush"), Alignment::DistributeFlush);
    }

    #[test]
    fn parse_alignment_unknown_defaults_left() {
        assert_eq!(parse_alignment("DISTRIBUTED"), Alignment::Left);
        assert_eq!(parse_alignment(""), Alignment::Left);
    }

    // ── HwpxStyle operations ────────────────────────────────────

    #[test]
    fn push_and_get_style() {
        let mut store = HwpxStyleStore::new();
        let style = HwpxStyle {
            id: 0,
            style_type: "PARA".into(),
            name: "바탕글".into(),
            eng_name: "Normal".into(),
            para_pr_id_ref: 0,
            char_pr_id_ref: 0,
            next_style_id_ref: 0,
            lang_id: 1042,
            lock_form: 0,
        };
        store.push_style(style);
        assert_eq!(store.style_count(), 1);
        let s = store.style(0).unwrap();
        assert_eq!(s.name, "바탕글");
        assert_eq!(s.eng_name, "Normal");
        assert_eq!(s.style_type, "PARA");
    }

    #[test]
    fn style_index_out_of_bounds() {
        let store = HwpxStyleStore::new();
        let err = store.style(0).unwrap_err();
        match err {
            HwpxError::IndexOutOfBounds { kind, index, max } => {
                assert_eq!(kind, "style");
                assert_eq!(index, 0);
                assert_eq!(max, 0);
            }
            _ => panic!("expected IndexOutOfBounds"),
        }
    }

    #[test]
    fn iter_styles_yields_all() {
        let mut store = HwpxStyleStore::new();
        store.push_style(HwpxStyle {
            id: 0,
            style_type: "PARA".into(),
            name: "바탕글".into(),
            eng_name: "Normal".into(),
            para_pr_id_ref: 0,
            char_pr_id_ref: 0,
            next_style_id_ref: 0,
            lang_id: 1042,
            lock_form: 0,
        });
        store.push_style(HwpxStyle {
            id: 1,
            style_type: "CHAR".into(),
            name: "본문".into(),
            eng_name: "Body".into(),
            para_pr_id_ref: 1,
            char_pr_id_ref: 1,
            next_style_id_ref: 1,
            lang_id: 1042,
            lock_form: 0,
        });
        let names: Vec<&str> = store.iter_styles().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["바탕글", "본문"]);
    }

    // ── from_registry bridge tests ──────────────────────────────

    #[test]
    fn from_registry_empty_produces_empty_store() {
        let registry: StyleRegistry = serde_json::from_str(
            r#"{"fonts":[],"char_shapes":[],"para_shapes":[],"style_entries":{}}"#,
        )
        .unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();

        // Empty registry injects 한글-compatible defaults:
        // 1 font × 7 language groups, 7 default charShapes, 20 default paraShapes,
        // 22 required styles (Modern default set)
        assert_eq!(store.font_count(), 7);
        assert_eq!(store.char_shape_count(), 7); // 7 default charPr groups
        assert_eq!(store.para_shape_count(), 20); // 20 default paraPr groups
        assert_eq!(store.style_count(), 22);
    }

    #[test]
    fn from_registry_preserves_counts() {
        let template = builtin_default().unwrap();
        let registry = StyleRegistry::from_template(&template).unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();

        // Fonts are mirrored across 7 language groups (HANGUL, LATIN, HANJA, JAPANESE, OTHER, SYMBOL, USER)
        assert_eq!(store.font_count(), registry.font_count() * 7);
        // 7 default charShapes + user charShapes; 20 default paraShapes + user paraShapes
        assert_eq!(store.char_shape_count(), 7 + registry.char_shape_count());
        assert_eq!(store.para_shape_count(), 20 + registry.para_shape_count());
        // +22 for injected Modern default styles (the default HancomStyleSet)
        assert_eq!(store.style_count(), registry.style_count() + 22);
    }

    #[test]
    fn from_registry_font_face_names_match() {
        let template = builtin_default().unwrap();
        let registry = StyleRegistry::from_template(&template).unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();

        let font_count = registry.font_count();
        let langs = ["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"];
        // Fonts are stored as: lang0[font0, font1, ...], lang1[font0, font1, ...], ...
        for (lang_idx, &lang) in langs.iter().enumerate() {
            for (font_idx, font_id) in registry.fonts.iter().enumerate() {
                let store_idx = lang_idx * font_count + font_idx;
                let hwpx_font = store.font(FontIndex::new(store_idx)).unwrap();
                assert_eq!(hwpx_font.face_name, font_id.as_str());
                assert_eq!(hwpx_font.lang, lang);
            }
        }
    }

    #[test]
    fn from_registry_char_shape_properties() {
        let template = builtin_default().unwrap();
        let registry = StyleRegistry::from_template(&template).unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();

        // User charShapes start at index 7 (after 7 default charPr groups)
        for (i, bp_cs) in registry.char_shapes.iter().enumerate() {
            let hwpx_cs = store.char_shape(CharShapeIndex::new(7 + i)).unwrap();
            assert_eq!(hwpx_cs.height, bp_cs.size);
            assert_eq!(hwpx_cs.text_color, bp_cs.color);
            assert_eq!(hwpx_cs.shade_color, bp_cs.shade_color);
            assert_eq!(hwpx_cs.bold, bp_cs.bold);
            assert_eq!(hwpx_cs.italic, bp_cs.italic);
            assert_eq!(hwpx_cs.underline_type, bp_cs.underline_type);
            assert_eq!(hwpx_cs.underline_color, bp_cs.underline_color);
            assert_eq!(hwpx_cs.strikeout_shape, bp_cs.strikeout_shape);
            assert_eq!(hwpx_cs.strikeout_color, bp_cs.strikeout_color);
            assert_eq!(hwpx_cs.vertical_position, bp_cs.vertical_position);
            assert_eq!(hwpx_cs.outline_type, bp_cs.outline);
            assert_eq!(hwpx_cs.shadow_type, bp_cs.shadow);
            assert_eq!(hwpx_cs.emboss_type, bp_cs.emboss);
            assert_eq!(hwpx_cs.engrave_type, bp_cs.engrave);
        }
    }

    #[test]
    fn from_registry_para_shape_properties() {
        let template = builtin_default().unwrap();
        let registry = StyleRegistry::from_template(&template).unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();

        // User paraShapes start at index 20 (after 20 default paraPr groups)
        for (i, bp_ps) in registry.para_shapes.iter().enumerate() {
            let hwpx_ps = store.para_shape(ParaShapeIndex::new(20 + i)).unwrap();
            assert_eq!(hwpx_ps.alignment, bp_ps.alignment);
            assert_eq!(hwpx_ps.margin_left, bp_ps.indent_left);
            assert_eq!(hwpx_ps.margin_right, bp_ps.indent_right);
            assert_eq!(hwpx_ps.indent, bp_ps.indent_first_line);
            assert_eq!(hwpx_ps.spacing_before, bp_ps.space_before);
            assert_eq!(hwpx_ps.spacing_after, bp_ps.space_after);
            assert_eq!(hwpx_ps.line_spacing, bp_ps.line_spacing_value.round() as i32);
        }
    }

    #[test]
    fn from_registry_carries_custom_tab_definitions_and_refs() {
        let mut registry = StyleRegistry::with_fonts(vec![]);
        registry.numberings.push(NumberingDef {
            id: 42,
            start: 0,
            levels: vec![hwpforge_core::ParaHead {
                start: 1,
                level: 1,
                num_format: NumberFormatType::Digit,
                text: "^1.".into(),
                checkable: false,
            }],
        });
        registry.para_shapes.push(ParaShape {
            alignment: Alignment::Left,
            line_spacing_type: LineSpacingType::Percentage,
            line_spacing_value: 160.0,
            space_before: HwpUnit::ZERO,
            space_after: HwpUnit::ZERO,
            indent_left: HwpUnit::ZERO,
            indent_right: HwpUnit::ZERO,
            indent_first_line: HwpUnit::ZERO,
            break_type: hwpforge_foundation::BreakType::None,
            keep_with_next: false,
            keep_lines_together: false,
            widow_orphan: true,
            border_fill_id: None,
            tab_def_id: 3,
            list: None,
        });
        registry.tabs.push(TabDef {
            id: 3,
            auto_tab_left: false,
            auto_tab_right: false,
            stops: vec![TabStop {
                position: HwpUnit::new(15000).unwrap(),
                align: TabAlign::Left,
                leader: TabLeader::dot(),
            }],
        });

        let store = HwpxStyleStore::from_registry(&registry).unwrap();

        let hwpx_ps = store.para_shape(ParaShapeIndex::new(20)).unwrap();
        assert_eq!(hwpx_ps.tab_pr_id_ref, 3);

        let tabs: Vec<_> = store.iter_tabs().cloned().collect();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].id, 3);
        assert_eq!(tabs[0].stops.len(), 1);
        assert_eq!(tabs[0].stops[0].position, HwpUnit::new(15000).unwrap());
    }

    #[test]
    fn from_registry_lowers_shared_list_ref_into_wire_triple() {
        let mut registry = StyleRegistry::with_fonts(vec![]);
        registry.numberings.push(NumberingDef {
            id: 42,
            start: 0,
            levels: vec![hwpforge_core::ParaHead {
                start: 1,
                level: 1,
                num_format: NumberFormatType::Digit,
                text: "^1.".into(),
                checkable: false,
            }],
        });
        registry.para_shapes.push(ParaShape {
            alignment: Alignment::Left,
            line_spacing_type: LineSpacingType::Percentage,
            line_spacing_value: 160.0,
            space_before: HwpUnit::ZERO,
            space_after: HwpUnit::ZERO,
            indent_left: HwpUnit::ZERO,
            indent_right: HwpUnit::ZERO,
            indent_first_line: HwpUnit::ZERO,
            break_type: hwpforge_foundation::BreakType::None,
            keep_with_next: false,
            keep_lines_together: false,
            widow_orphan: true,
            border_fill_id: None,
            tab_def_id: 0,
            list: Some(ParagraphListRef::Number {
                numbering_id: hwpforge_foundation::NumberingIndex::new(0),
                level: 2,
            }),
        });

        let store = HwpxStyleStore::from_registry(&registry).unwrap();
        let hwpx_ps = store.para_shape(ParaShapeIndex::new(20)).unwrap();
        assert_eq!(hwpx_ps.heading_type, HeadingType::Number);
        assert_eq!(hwpx_ps.heading_id_ref, 42);
        assert_eq!(hwpx_ps.heading_level, 2);
    }

    #[test]
    fn from_registry_lowers_outline_list_ref_using_one_based_hwpx_level() {
        let mut registry = StyleRegistry::with_fonts(vec![]);
        registry.para_shapes.push(ParaShape {
            alignment: Alignment::Left,
            line_spacing_type: LineSpacingType::Percentage,
            line_spacing_value: 160.0,
            space_before: HwpUnit::ZERO,
            space_after: HwpUnit::ZERO,
            indent_left: HwpUnit::ZERO,
            indent_right: HwpUnit::ZERO,
            indent_first_line: HwpUnit::ZERO,
            break_type: hwpforge_foundation::BreakType::None,
            keep_with_next: false,
            keep_lines_together: false,
            widow_orphan: true,
            border_fill_id: None,
            tab_def_id: 0,
            list: Some(ParagraphListRef::Outline { level: 0 }),
        });

        let store = HwpxStyleStore::from_registry(&registry).unwrap();
        let hwpx_ps = store.para_shape(ParaShapeIndex::new(20)).unwrap();
        assert_eq!(hwpx_ps.heading_type, HeadingType::Outline);
        assert_eq!(hwpx_ps.heading_id_ref, 0);
        assert_eq!(hwpx_ps.heading_level, 1);
    }

    #[test]
    fn from_registry_rejects_invalid_shared_list_definition_indices() {
        let mut registry = StyleRegistry::with_fonts(vec![]);
        registry.para_shapes.push(ParaShape {
            alignment: Alignment::Left,
            line_spacing_type: LineSpacingType::Percentage,
            line_spacing_value: 160.0,
            space_before: HwpUnit::ZERO,
            space_after: HwpUnit::ZERO,
            indent_left: HwpUnit::ZERO,
            indent_right: HwpUnit::ZERO,
            indent_first_line: HwpUnit::ZERO,
            break_type: hwpforge_foundation::BreakType::None,
            keep_with_next: false,
            keep_lines_together: false,
            widow_orphan: true,
            border_fill_id: None,
            tab_def_id: 0,
            list: Some(ParagraphListRef::Number {
                numbering_id: hwpforge_foundation::NumberingIndex::new(99),
                level: 0,
            }),
        });

        let err = HwpxStyleStore::from_registry(&registry).unwrap_err();
        assert!(matches!(err, HwpxError::IndexOutOfBounds { kind: "numbering definition", .. }));
    }

    #[test]
    fn from_registry_style_entries_reference_valid_indices() {
        let template = builtin_default().unwrap();
        let registry = StyleRegistry::from_template(&template).unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();

        for i in 0..store.style_count() {
            let style = store.style(i).unwrap();
            // Style type is either "PARA" or "CHAR" (default styles include both)
            assert!(
                style.style_type == "PARA" || style.style_type == "CHAR",
                "unexpected style_type '{}' for style '{}'",
                style.style_type,
                style.name
            );
            assert!(
                (style.char_pr_id_ref as usize) < store.char_shape_count(),
                "char_pr_id_ref {} out of bounds for style '{}'",
                style.char_pr_id_ref,
                style.name
            );
            assert!(
                (style.para_pr_id_ref as usize) < store.para_shape_count(),
                "para_pr_id_ref {} out of bounds for style '{}'",
                style.para_pr_id_ref,
                style.name
            );
        }
    }

    // ── HancomStyleSet count tests ──────────────────────────────

    #[test]
    fn default_style_set_classic_count() {
        assert_eq!(HancomStyleSet::Classic.count(), 18);
    }

    #[test]
    fn default_style_set_modern_count() {
        assert_eq!(HancomStyleSet::Modern.count(), 22);
    }

    #[test]
    fn default_style_set_latest_count() {
        assert_eq!(HancomStyleSet::Latest.count(), 23);
    }

    #[test]
    fn default_style_set_modern_is_default() {
        assert_eq!(HancomStyleSet::default(), HancomStyleSet::Modern);
    }

    // ── with_default_fonts ───────────────────────────────────────

    #[test]
    fn with_default_fonts_creates_seven_fonts() {
        let store = HwpxStyleStore::with_default_fonts("함초롬돋움");
        assert_eq!(store.font_count(), 7);
    }

    #[test]
    fn with_default_fonts_all_names_match() {
        let font_name = "나눔고딕";
        let store = HwpxStyleStore::with_default_fonts(font_name);
        for font in store.iter_fonts() {
            assert_eq!(font.face_name, font_name);
        }
    }

    #[test]
    fn with_default_fonts_lang_groups_correct() {
        let store = HwpxStyleStore::with_default_fonts("함초롬바탕");
        let langs: Vec<&str> = store.iter_fonts().map(|f| f.lang.as_str()).collect();
        assert_eq!(langs, vec!["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"]);
    }

    #[test]
    fn with_default_fonts_use_group_local_zero_ids() {
        let store = HwpxStyleStore::with_default_fonts("함초롬바탕");
        let ids: Vec<u32> = store.iter_fonts().map(|font| font.id).collect();
        assert_eq!(ids, vec![0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn from_registry_with_classic_style_set() {
        let registry: StyleRegistry = serde_json::from_str(
            r#"{"fonts":[],"char_shapes":[],"para_shapes":[],"style_entries":{}}"#,
        )
        .unwrap();
        let store = HwpxStyleStore::from_registry_with(&registry, HancomStyleSet::Classic).unwrap();
        assert_eq!(store.style_set(), HancomStyleSet::Classic);
        // Classic injects exactly 18 default styles
        assert_eq!(store.style_count(), 18);
        // 쪽 번호 at Classic position (id=9)
        assert_eq!(store.style(9).unwrap().name, "쪽 번호");
    }

    #[test]
    fn modern_styles_match_golden_fixture() {
        // Verified from golden fixture tests/fixtures/textbox.hwpx (한글 2022+)
        let styles = HancomStyleSet::Modern.default_styles();
        // 개요 8-10 inserted at 9-11
        assert_eq!(styles[9].name, "개요 8");
        assert_eq!(styles[10].name, "개요 9");
        assert_eq!(styles[11].name, "개요 10");
        // 쪽 번호 shifted to 12
        assert_eq!(styles[12].name, "쪽 번호");
        assert_eq!(styles[12].style_type, "CHAR");
        // 캡션 at 21
        assert_eq!(styles[21].name, "캡션");
        assert_eq!(styles[21].style_type, "PARA");
    }

    // ── Border Fill tests ─────────────────────────────────────────

    #[test]
    fn default_border_fills_count() {
        use hwpforge_blueprint::{builtins::builtin_default, registry::StyleRegistry};
        let template = builtin_default().unwrap();
        let registry = StyleRegistry::from_template(&template).unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();
        assert_eq!(store.border_fill_count(), 3, "from_registry produces exactly 3 default fills");
    }

    #[test]
    fn default_border_fill_page() {
        // id=1: page border — empty borders, no fill
        let bf = HwpxBorderFill::default_page_border();
        assert_eq!(bf.id, 1);
        assert!(!bf.three_d);
        assert!(!bf.shadow);
        assert_eq!(bf.center_line, "NONE");
        assert_eq!(bf.left.line_type, "NONE");
        assert_eq!(bf.right.line_type, "NONE");
        assert_eq!(bf.top.line_type, "NONE");
        assert_eq!(bf.bottom.line_type, "NONE");
        assert_eq!(bf.diagonal.as_ref().unwrap().line_type, "SOLID");
        assert!(bf.fill.is_none());
    }

    #[test]
    fn default_border_fill_char() {
        // id=2: char background — must have WinBrush fill
        let bf = HwpxBorderFill::default_char_background();
        assert_eq!(bf.id, 2);
        assert!(bf.fill.is_some(), "char background must have a fill brush");
        match bf.fill.as_ref().unwrap() {
            HwpxFill::WinBrush { face_color, hatch_color, alpha } => {
                assert_eq!(face_color, "none");
                assert_eq!(hatch_color, "#FF000000");
                assert_eq!(alpha, "0");
            }
        }
        assert_eq!(bf.fill_hatch_style, None);
    }

    #[test]
    fn default_border_fill_table() {
        // id=3: table border — SOLID on all 4 sides, 0.12 mm
        let bf = HwpxBorderFill::default_table_border();
        assert_eq!(bf.id, 3);
        assert_eq!(bf.left.line_type, "SOLID");
        assert_eq!(bf.left.width, "0.12 mm");
        assert_eq!(bf.right.line_type, "SOLID");
        assert_eq!(bf.top.line_type, "SOLID");
        assert_eq!(bf.bottom.line_type, "SOLID");
        assert_eq!(bf.diagonal.as_ref().unwrap().line_type, "SOLID");
        assert_eq!(bf.diagonal.as_ref().unwrap().width, "0.1 mm");
        assert!(bf.fill.is_none());
    }

    #[test]
    fn push_user_border_fill() {
        let mut store = HwpxStyleStore::new();
        let bf = HwpxBorderFill {
            id: 4,
            three_d: false,
            shadow: false,
            center_line: "NONE".into(),
            left: HwpxBorderLine {
                line_type: "DASH".into(),
                width: "0.2 mm".into(),
                color: "#FF0000".into(),
            },
            right: HwpxBorderLine::default(),
            top: HwpxBorderLine::default(),
            bottom: HwpxBorderLine::default(),
            diagonal: Some(HwpxBorderLine::default()),
            slash_type: "NONE".into(),
            back_slash_type: "NONE".into(),
            slash: HwpxDiagonalLine::default(),
            back_slash: HwpxDiagonalLine::default(),
            fill: None,
            fill_hatch_style: None,
            gradient_fill: None,
            image_fill: None,
        };
        let returned_id = store.push_border_fill(bf);
        assert_eq!(returned_id, 4);
        assert_eq!(store.border_fill_count(), 1);
        let fetched = store.border_fill(4).unwrap();
        assert_eq!(fetched.left.line_type, "DASH");
        assert_eq!(fetched.left.width, "0.2 mm");
    }

    #[test]
    fn set_slash_and_back_slash_keep_legacy_fields_in_sync() {
        let mut bf = HwpxBorderFill::default_page_border();
        bf.set_slash(HwpxDiagonalLine {
            border_type: "CENTER".into(),
            crooked: true,
            is_counter: false,
        });
        bf.set_back_slash(HwpxDiagonalLine {
            border_type: "ALL".into(),
            crooked: false,
            is_counter: true,
        });

        assert_eq!(bf.slash_type, "CENTER");
        assert_eq!(bf.back_slash_type, "ALL");
        assert_eq!(bf.effective_slash_type(), "CENTER");
        assert_eq!(bf.effective_back_slash_type(), "ALL");
    }

    #[test]
    fn set_gradient_fill_clears_legacy_fill_fields() {
        let mut bf = HwpxBorderFill::default_char_background();
        bf.set_gradient_fill(HwpxGradientFill {
            gradient_type: GradientType::Linear,
            angle: 90,
            center_x: 0,
            center_y: 0,
            step: 255,
            step_center: 50,
            alpha: 0,
            colors: vec![Color::from_rgb(255, 0, 0), Color::from_rgb(0, 255, 0)],
        });

        assert!(bf.fill.is_none());
        assert!(bf.fill_hatch_style.is_none());
        assert!(bf.gradient_fill.is_some());
        assert!(bf.image_fill.is_none());
        assert_eq!(bf.fill_source_count(), 1);
    }

    #[test]
    fn set_image_fill_clears_gradient_fill() {
        let mut bf = HwpxBorderFill::default_page_border();
        bf.set_gradient_fill(HwpxGradientFill {
            gradient_type: GradientType::Linear,
            angle: 90,
            center_x: 0,
            center_y: 0,
            step: 255,
            step_center: 50,
            alpha: 0,
            colors: vec![Color::from_rgb(255, 0, 0), Color::from_rgb(0, 255, 0)],
        });
        bf.set_image_fill(HwpxImageFill {
            mode: "TOTAL".into(),
            binary_item_id_ref: "BIN0001".into(),
            bright: 0,
            contrast: 0,
            effect: "REAL_PIC".into(),
            alpha: 0,
        });

        assert!(bf.fill.is_none());
        assert!(bf.gradient_fill.is_none());
        assert!(bf.image_fill.is_some());
        assert_eq!(bf.fill_source_count(), 1);
    }

    #[test]
    fn border_fill_not_found_returns_error() {
        let store = HwpxStyleStore::new();
        assert!(store.border_fill(1).is_err());
    }

    #[test]
    fn from_registry_border_fills_have_correct_ids() {
        use hwpforge_blueprint::{builtins::builtin_default, registry::StyleRegistry};
        let template = builtin_default().unwrap();
        let registry = StyleRegistry::from_template(&template).unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();
        // IDs are 1-based
        assert_eq!(store.border_fill(1).unwrap().id, 1);
        assert_eq!(store.border_fill(2).unwrap().id, 2);
        assert_eq!(store.border_fill(3).unwrap().id, 3);
    }

    // ── 7.3 per-style shape injection tests ──────────────────────

    #[test]
    fn from_registry_injects_7_default_char_shapes() {
        let registry: StyleRegistry = serde_json::from_str(
            r#"{"fonts":[],"char_shapes":[],"para_shapes":[],"style_entries":{}}"#,
        )
        .unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();
        assert_eq!(store.char_shape_count(), 7, "must have exactly 7 default charPr groups");
    }

    #[test]
    fn from_registry_injects_20_default_para_shapes() {
        let registry: StyleRegistry = serde_json::from_str(
            r#"{"fonts":[],"char_shapes":[],"para_shapes":[],"style_entries":{}}"#,
        )
        .unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();
        assert_eq!(store.para_shape_count(), 20, "must have exactly 20 default paraPr groups");
    }

    #[test]
    fn default_char_shape_0_is_batang_10pt_black() {
        // charPr 0 = 함초롬바탕 10pt #000000 (바탕글/본문/개요1-7/캡션)
        let registry: StyleRegistry = serde_json::from_str(
            r#"{"fonts":[],"char_shapes":[],"para_shapes":[],"style_entries":{}}"#,
        )
        .unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();
        let cs = store.char_shape(CharShapeIndex::new(0)).unwrap();
        assert_eq!(cs.height.as_i32(), 1000); // 10pt
        assert_eq!(cs.text_color, Color::BLACK);
        assert!(!cs.bold);
        assert!(!cs.italic);
    }

    #[test]
    fn default_char_shape_5_is_toc_heading() {
        // charPr 5 = 함초롬돋움 16pt #2E74B5 (차례 제목)
        let registry: StyleRegistry = serde_json::from_str(
            r#"{"fonts":[],"char_shapes":[],"para_shapes":[],"style_entries":{}}"#,
        )
        .unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();
        let cs = store.char_shape(CharShapeIndex::new(5)).unwrap();
        assert_eq!(cs.height.as_i32(), 1600); // 16pt
        assert_eq!(cs.text_color, Color::from_rgb(0x2E, 0x74, 0xB5));
    }

    #[test]
    fn from_registry_user_shapes_offset() {
        // User charShapes must start at index 7, user paraShapes at index 20
        let template = builtin_default().unwrap();
        let registry = StyleRegistry::from_template(&template).unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();
        // First user charShape is at index 7
        assert!(store.char_shape(CharShapeIndex::new(7)).is_ok());
        // First user paraShape is at index 20
        assert!(store.para_shape(ParaShapeIndex::new(20)).is_ok());
    }

    #[test]
    fn from_registry_default_style_refs_match_groups() {
        // Default styles must reference the correct charPr/paraPr group indices
        let registry: StyleRegistry = serde_json::from_str(
            r#"{"fonts":[],"char_shapes":[],"para_shapes":[],"style_entries":{}}"#,
        )
        .unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();
        let defaults = HancomStyleSet::Modern.default_styles();
        for (idx, entry) in defaults.iter().enumerate() {
            let style = store.style(idx).unwrap();
            assert_eq!(
                style.char_pr_id_ref, entry.char_pr_group as u32,
                "charPr ref mismatch for style '{}'",
                entry.name
            );
            assert_eq!(
                style.para_pr_id_ref, entry.para_pr_group as u32,
                "paraPr ref mismatch for style '{}'",
                entry.name
            );
        }
    }

    // ── serde round-trip tests ─────────────────────────────────

    #[test]
    fn char_shape_roundtrip_json() {
        let cs = HwpxCharShape::default();
        let json = serde_json::to_string(&cs).unwrap();
        let restored: HwpxCharShape = serde_json::from_str(&json).unwrap();
        assert_eq!(cs, restored);
    }

    #[test]
    fn para_shape_roundtrip_json() {
        let ps = HwpxParaShape::default();
        let json = serde_json::to_string(&ps).unwrap();
        let restored: HwpxParaShape = serde_json::from_str(&json).unwrap();
        assert_eq!(ps, restored);
    }

    #[test]
    fn style_store_roundtrip_json() {
        let mut store = HwpxStyleStore::with_default_fonts("함초롬돋움");
        store.push_char_shape(HwpxCharShape::default());
        store.push_para_shape(HwpxParaShape::default());
        let json = serde_json::to_string_pretty(&store).unwrap();
        let restored: HwpxStyleStore = serde_json::from_str(&json).unwrap();
        assert_eq!(store.font_count(), restored.font_count());
        assert_eq!(store.char_shape_count(), restored.char_shape_count());
        assert_eq!(store.para_shape_count(), restored.para_shape_count());
    }

    #[test]
    fn from_registry_user_style_refs_are_offset_adjusted() {
        // User styles' charPr/paraPr refs must be offset by 7/20
        let template = builtin_default().unwrap();
        let registry = StyleRegistry::from_template(&template).unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();
        let defaults_len = HancomStyleSet::Modern.count();
        for (i, (_, entry)) in registry.style_entries.iter().enumerate() {
            let style = store.style(defaults_len + i).unwrap();
            assert_eq!(
                style.char_pr_id_ref,
                (entry.char_shape_id.get() + 7) as u32,
                "user charPr ref not offset-adjusted for style index {i}"
            );
            assert_eq!(
                style.para_pr_id_ref,
                (entry.para_shape_id.get() + 20) as u32,
                "user paraPr ref not offset-adjusted for style index {i}"
            );
        }
    }

    #[test]
    fn default_para_shape_0_is_batanggeul() {
        // paraPr 0 = JUSTIFY, left=0, 160% line spacing (바탕글)
        let registry: StyleRegistry = serde_json::from_str(
            r#"{"fonts":[],"char_shapes":[],"para_shapes":[],"style_entries":{}}"#,
        )
        .unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();
        let ps = store.para_shape(ParaShapeIndex::new(0)).unwrap();
        assert_eq!(ps.alignment, Alignment::Justify);
        assert_eq!(ps.margin_left.as_i32(), 0);
        assert_eq!(ps.line_spacing, 160);
    }

    #[test]
    fn default_para_shape_2_is_outline1() {
        // paraPr 2 = JUSTIFY, left=1000 (개요 1 with OUTLINE heading)
        let registry: StyleRegistry = serde_json::from_str(
            r#"{"fonts":[],"char_shapes":[],"para_shapes":[],"style_entries":{}}"#,
        )
        .unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();
        let ps = store.para_shape(ParaShapeIndex::new(2)).unwrap();
        assert_eq!(ps.alignment, Alignment::Justify);
        assert_eq!(ps.margin_left.as_i32(), 1000);
        assert_eq!(ps.heading_type, HeadingType::Outline);
        assert_eq!(ps.heading_id_ref, 0);
        assert_eq!(ps.heading_level, 1);
        assert_eq!(ps.line_spacing, 160);
    }

    #[test]
    fn replace_font_swaps_matching_entries_only() {
        let mut store = HwpxStyleStore::with_default_fonts("함초롬돋움");
        // Add a second font (D2Coding) for all 7 lang groups
        for lang in &["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
            store.push_font(HwpxFont {
                id: 1,
                face_name: "D2Coding".to_string(),
                lang: lang.to_string(),
            });
        }
        assert_eq!(store.font_count(), 14); // 7 (함초롬돋움) + 7 (D2Coding)

        store.replace_font("함초롬돋움", "맑은 고딕");

        // 함초롬돋움 should be replaced
        let first = store.iter_fonts().next().unwrap();
        assert_eq!(first.face_name, "맑은 고딕");

        // D2Coding should be preserved
        let d2 = store.iter_fonts().find(|f| f.face_name == "D2Coding");
        assert!(d2.is_some(), "D2Coding should not be replaced");

        // Total count unchanged
        assert_eq!(store.font_count(), 14);
    }

    // ── StyleLookup impl tests ──────────────────────────────────

    /// Helper: build a minimal store with one font, one char shape, one para shape, one style.
    fn style_lookup_test_store() -> HwpxStyleStore {
        let mut store = HwpxStyleStore::new();
        // Font at index 0
        store.push_font(HwpxFont::new(0, "함초롬돋움", "HANGUL"));
        // Char shape at index 0: bold + italic + strikeout + superscript
        store.push_char_shape(HwpxCharShape {
            bold: true,
            italic: true,
            underline_type: UnderlineType::Bottom,
            strikeout_shape: StrikeoutShape::Continuous,
            vertical_position: VerticalPosition::Superscript,
            height: HwpUnit::new(1200).unwrap(), // 12pt
            text_color: Color::from_rgb(255, 0, 0),
            font_ref: HwpxFontRef::default(), // hangul = FontIndex(0)
            ..Default::default()
        });
        // Para shape at index 0: center
        store.push_para_shape(HwpxParaShape { alignment: Alignment::Center, ..Default::default() });
        // Style at index 0: "개요 2"
        store.push_style(HwpxStyle {
            id: 0,
            style_type: "PARA".to_string(),
            name: "개요 2".to_string(),
            eng_name: "Outline 2".to_string(),
            para_pr_id_ref: 0,
            char_pr_id_ref: 0,
            next_style_id_ref: 0,
            lang_id: 1042,
            lock_form: 0,
        });
        store
    }

    #[test]
    fn style_lookup_char_bold() {
        use hwpforge_core::StyleLookup;
        let store = style_lookup_test_store();
        assert_eq!(store.char_bold(CharShapeIndex::new(0)), Some(true));
    }

    #[test]
    fn style_lookup_char_italic() {
        use hwpforge_core::StyleLookup;
        let store = style_lookup_test_store();
        assert_eq!(store.char_italic(CharShapeIndex::new(0)), Some(true));
    }

    #[test]
    fn style_lookup_char_underline() {
        use hwpforge_core::StyleLookup;
        let store = style_lookup_test_store();
        assert_eq!(store.char_underline(CharShapeIndex::new(0)), Some(UnderlineType::Bottom));
    }

    #[test]
    fn style_lookup_char_strikeout() {
        use hwpforge_core::StyleLookup;
        let store = style_lookup_test_store();
        assert_eq!(store.char_strikeout(CharShapeIndex::new(0)), Some(true));
    }

    #[test]
    fn style_lookup_char_superscript() {
        use hwpforge_core::StyleLookup;
        let store = style_lookup_test_store();
        assert_eq!(store.char_superscript(CharShapeIndex::new(0)), Some(true));
        // Superscript is not subscript
        assert_eq!(store.char_subscript(CharShapeIndex::new(0)), Some(false));
    }

    #[test]
    fn style_lookup_char_subscript() {
        use hwpforge_core::StyleLookup;
        let mut store = HwpxStyleStore::new();
        store.push_char_shape(HwpxCharShape {
            vertical_position: VerticalPosition::Subscript,
            ..Default::default()
        });
        assert_eq!(store.char_subscript(CharShapeIndex::new(0)), Some(true));
        assert_eq!(store.char_superscript(CharShapeIndex::new(0)), Some(false));
    }

    #[test]
    fn style_lookup_char_font_name() {
        use hwpforge_core::StyleLookup;
        let store = style_lookup_test_store();
        assert_eq!(store.char_font_name(CharShapeIndex::new(0)), Some("함초롬돋움"));
    }

    #[test]
    fn style_lookup_char_font_name_missing_font() {
        use hwpforge_core::StyleLookup;
        let mut store = HwpxStyleStore::new();
        // Char shape referencing font index 5, but no fonts in store
        store.push_char_shape(HwpxCharShape {
            font_ref: HwpxFontRef { hangul: FontIndex::new(5), ..Default::default() },
            ..Default::default()
        });
        assert!(store.char_font_name(CharShapeIndex::new(0)).is_none());
    }

    #[test]
    fn style_lookup_char_font_size() {
        use hwpforge_core::StyleLookup;
        let store = style_lookup_test_store();
        assert_eq!(store.char_font_size(CharShapeIndex::new(0)), Some(HwpUnit::new(1200).unwrap()));
    }

    #[test]
    fn style_lookup_char_text_color() {
        use hwpforge_core::StyleLookup;
        let store = style_lookup_test_store();
        let color = store.char_text_color(CharShapeIndex::new(0)).unwrap();
        assert_eq!(color.red(), 255);
        assert_eq!(color.green(), 0);
        assert_eq!(color.blue(), 0);
    }

    #[test]
    fn style_lookup_para_alignment() {
        use hwpforge_core::StyleLookup;
        let store = style_lookup_test_store();
        assert_eq!(store.para_alignment(ParaShapeIndex::new(0)), Some(Alignment::Center));
    }

    #[test]
    fn style_lookup_style_name() {
        use hwpforge_core::StyleLookup;
        use hwpforge_foundation::StyleIndex;
        let store = style_lookup_test_store();
        assert_eq!(store.style_name(StyleIndex::new(0)), Some("개요 2"));
    }

    #[test]
    fn style_lookup_style_heading_level() {
        use hwpforge_core::StyleLookup;
        use hwpforge_foundation::StyleIndex;
        let store = style_lookup_test_store();
        assert_eq!(store.style_heading_level(StyleIndex::new(0)), Some(2));
    }

    #[test]
    fn style_lookup_para_heading_level_reads_outline_para_shape() {
        use hwpforge_core::StyleLookup;
        use hwpforge_foundation::ParaShapeIndex;

        let mut store = style_lookup_test_store();
        store.para_shapes[0].heading_type = HeadingType::Outline;
        store.para_shapes[0].heading_level = 4;

        assert_eq!(store.para_heading_level(ParaShapeIndex::new(0)), Some(4));
    }

    #[test]
    fn style_lookup_style_heading_level_prefers_outline_para_shape() {
        use hwpforge_core::StyleLookup;
        use hwpforge_foundation::StyleIndex;

        let mut store = style_lookup_test_store();
        store.styles[0].name = "맞춤 제목".to_string();
        store.para_shapes[0].heading_type = HeadingType::Outline;
        store.para_shapes[0].heading_level = 3;

        assert_eq!(store.style_heading_level(StyleIndex::new(0)), Some(3));
    }

    #[test]
    fn style_lookup_out_of_bounds_returns_none() {
        use hwpforge_core::StyleLookup;
        use hwpforge_foundation::StyleIndex;
        let store = HwpxStyleStore::new();
        assert!(store.char_bold(CharShapeIndex::new(99)).is_none());
        assert!(store.para_alignment(ParaShapeIndex::new(99)).is_none());
        assert!(store.style_name(StyleIndex::new(99)).is_none());
        assert!(store.style_heading_level(StyleIndex::new(99)).is_none());
    }

    #[test]
    fn style_lookup_image_data_always_none() {
        use hwpforge_core::StyleLookup;
        let store = style_lookup_test_store();
        assert!(store.image_data("anything.png").is_none());
    }

    #[test]
    fn style_lookup_default_char_shape_not_bold() {
        use hwpforge_core::StyleLookup;
        let mut store = HwpxStyleStore::new();
        store.push_char_shape(HwpxCharShape::default());
        assert_eq!(store.char_bold(CharShapeIndex::new(0)), Some(false));
        assert_eq!(store.char_italic(CharShapeIndex::new(0)), Some(false));
        assert_eq!(store.char_strikeout(CharShapeIndex::new(0)), Some(false));
        assert_eq!(store.char_underline(CharShapeIndex::new(0)), Some(UnderlineType::None));
    }

    // ── parse_heading_level_from_name tests ─────────────────────

    #[test]
    fn heading_level_outline_1_to_6() {
        assert_eq!(parse_heading_level_from_name("개요 1"), Some(1));
        assert_eq!(parse_heading_level_from_name("개요 2"), Some(2));
        assert_eq!(parse_heading_level_from_name("개요 3"), Some(3));
        assert_eq!(parse_heading_level_from_name("개요 4"), Some(4));
        assert_eq!(parse_heading_level_from_name("개요 5"), Some(5));
        assert_eq!(parse_heading_level_from_name("개요 6"), Some(6));
    }

    #[test]
    fn heading_level_outline_clamped() {
        // 개요 7+ clamped to 6
        assert_eq!(parse_heading_level_from_name("개요 7"), Some(6));
        assert_eq!(parse_heading_level_from_name("개요 10"), Some(6));
    }

    #[test]
    fn heading_level_title_styles() {
        assert_eq!(parse_heading_level_from_name("+제목"), Some(1));
        assert_eq!(parse_heading_level_from_name("타이들"), Some(1));
        assert_eq!(parse_heading_level_from_name("큰제목"), Some(1));
    }

    #[test]
    fn heading_level_non_heading() {
        assert_eq!(parse_heading_level_from_name("바탕글"), None);
        assert_eq!(parse_heading_level_from_name("본문"), None);
        assert_eq!(parse_heading_level_from_name(""), None);
    }

    #[test]
    fn heading_level_whitespace_trimmed() {
        assert_eq!(parse_heading_level_from_name("  개요 3  "), Some(3));
        assert_eq!(parse_heading_level_from_name("  +제목  "), Some(1));
    }
}
