//! HWPX-specific style storage.
//!
//! [`HwpxStyleStore`] is the **smithy-hwpx** analogue of Blueprint's
//! `StyleRegistry`, but much simpler: it stores only what was actually
//! found in `header.xml`, with zero inheritance logic.
//!
//! All fields use Foundation types (`Color`, `HwpUnit`, `Alignment`)
//! so downstream code never touches raw XML strings.

use hwpforge_foundation::{Alignment, CharShapeIndex, Color, FontIndex, HwpUnit, ParaShapeIndex};

use crate::error::{HwpxError, HwpxResult};

// ── Font ─────────────────────────────────────────────────────────

/// A resolved font from `<hh:fontface>` → `<hh:font>`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HwpxFont {
    /// Original `id` attribute from XML.
    pub id: u32,
    /// Face name (e.g. `"함초롬돋움"`, `"Times New Roman"`).
    pub face_name: String,
    /// Language group this font belongs to (e.g. `"HANGUL"`, `"LATIN"`).
    pub lang: String,
}

// ── Per-language font references ─────────────────────────────────

/// Per-language font index references from `<hh:fontRef>`.
///
/// Each field is a [`FontIndex`] pointing into the store's font list
/// for that language group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HwpxCharShape {
    /// Per-language font references.
    pub font_ref: HwpxFontRef,
    /// Font height in HwpUnit (height attribute × 1, already HWPUNIT).
    pub height: HwpUnit,
    /// Text color (from `textColor` attribute, e.g. `"#000000"`).
    pub text_color: Color,
    /// Background shade color (from `shadeColor`, `"none"` → BLACK).
    pub shade_color: Color,
    /// Bold formatting.
    pub bold: bool,
    /// Italic formatting.
    pub italic: bool,
    /// Underline type string (e.g. `"NONE"`, `"BOTTOM"`).
    pub underline_type: String,
    /// Strikeout shape string (e.g. `"NONE"`, `"SLASH"`).
    pub strikeout_shape: String,
}

impl Default for HwpxCharShape {
    fn default() -> Self {
        Self {
            font_ref: HwpxFontRef::default(),
            height: HwpUnit::ZERO,
            text_color: Color::BLACK,
            shade_color: Color::BLACK,
            bold: false,
            italic: false,
            underline_type: String::from("NONE"),
            strikeout_shape: String::from("NONE"),
        }
    }
}

// ── Style ────────────────────────────────────────────────────────

/// Resolved style definition from `<hh:style>`.
///
/// Stores style metadata like names and references to character/paragraph
/// properties. This enables full roundtrip of style names like "바탕글",
/// "본문", etc.
#[derive(Debug, Clone, PartialEq, Eq)]
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
}

// ── Paragraph Shape ──────────────────────────────────────────────

/// Resolved paragraph properties from `<hh:paraPr>`.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Line spacing type (e.g. `"PERCENT"`, `"FIXED"`).
    pub line_spacing_type: String,
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
            line_spacing_type: String::from("PERCENT"),
        }
    }
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
#[derive(Debug, Clone, Default)]
pub struct HwpxStyleStore {
    fonts: Vec<HwpxFont>,
    char_shapes: Vec<HwpxCharShape>,
    para_shapes: Vec<HwpxParaShape>,
    styles: Vec<HwpxStyle>,
}

impl HwpxStyleStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
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
}

// ── Thread safety assertions ─────────────────────────────────────

#[allow(dead_code)]
const _: () = {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    fn assertions() {
        assert_send::<HwpxStyleStore>();
        assert_sync::<HwpxStyleStore>();
    }
};

// ── Color parsing helper ─────────────────────────────────────────

/// Parses a HWPX hex color string (`"#RRGGBB"`) into a [`Color`].
///
/// Returns `Color::BLACK` for `"none"`, empty strings, or invalid formats.
/// This is intentionally lenient: real-world HWPX files sometimes contain
/// non-standard color values, and rejecting them would make the decoder
/// unusable for slightly malformed documents.
pub(crate) fn parse_hex_color(s: &str) -> Color {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") {
        return Color::BLACK;
    }
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 {
        return Color::BLACK;
    }
    let Ok(rgb) = u32::from_str_radix(hex, 16) else {
        return Color::BLACK;
    };
    let r = ((rgb >> 16) & 0xFF) as u8;
    let g = ((rgb >> 8) & 0xFF) as u8;
    let b = (rgb & 0xFF) as u8;
    Color::from_rgb(r, g, b)
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
    } else if s.eq_ignore_ascii_case("JUSTIFY") {
        Alignment::Justify
    } else {
        Alignment::Left
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_foundation::{CharShapeIndex, FontIndex, ParaShapeIndex};

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
        assert_eq!(cs.height, HwpUnit::ZERO);
        assert_eq!(cs.text_color, Color::BLACK);
        assert!(!cs.bold);
        assert!(!cs.italic);
        assert_eq!(cs.underline_type, "NONE");
        assert_eq!(cs.strikeout_shape, "NONE");
    }

    // ── HwpxParaShape default ────────────────────────────────────

    #[test]
    fn para_shape_default_values() {
        let ps = HwpxParaShape::default();
        assert_eq!(ps.alignment, Alignment::Left);
        assert_eq!(ps.margin_left, HwpUnit::ZERO);
        assert_eq!(ps.indent, HwpUnit::ZERO);
        assert_eq!(ps.line_spacing, 160);
        assert_eq!(ps.line_spacing_type, "PERCENT");
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
        });
        let names: Vec<&str> = store.iter_styles().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["바탕글", "본문"]);
    }
}
