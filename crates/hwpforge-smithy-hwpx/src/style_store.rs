//! HWPX-specific style storage.
//!
//! [`HwpxStyleStore`] is the **smithy-hwpx** analogue of Blueprint's
//! `StyleRegistry`, but much simpler: it stores only what was actually
//! found in `header.xml`, with zero inheritance logic.
//!
//! All fields use Foundation types (`Color`, `HwpUnit`, `Alignment`)
//! so downstream code never touches raw XML strings.

use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_foundation::{
    Alignment, BorderFillIndex, BreakType, CharShapeIndex, Color, EmbossType, EngraveType,
    FontIndex, HwpUnit, LineSpacingType, OutlineType, ParaShapeIndex, ShadowType, StrikeoutShape,
    UnderlineType, VerticalPosition, WordBreakType,
};

use crate::default_styles::HancomStyleSet;
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
    /// The 한글 version style set used when injecting default styles.
    style_set: HancomStyleSet,
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
        for (idx, &lang) in langs.iter().enumerate() {
            store.push_font(HwpxFont::new(idx as u32, font_name, lang));
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
    pub fn from_registry(registry: &StyleRegistry) -> Self {
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
    pub fn from_registry_with(registry: &StyleRegistry, style_set: HancomStyleSet) -> Self {
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

        // Step 2: Ensure at least one char shape and para shape exist
        let has_shapes = !registry.char_shapes.is_empty() && !registry.para_shapes.is_empty();

        if !has_shapes {
            // Inject minimal default char shape (10pt, black, no formatting)
            store.push_char_shape(HwpxCharShape {
                font_ref: HwpxFontRef::default(),    // All point to font 0
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
            });

            // Inject minimal default para shape (justified, 160% line spacing)
            store.push_para_shape(HwpxParaShape {
                alignment: Alignment::Justify,
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
                widow_orphan: true,
                break_latin_word: WordBreakType::KeepWord,
                break_non_latin_word: WordBreakType::KeepWord,
                border_fill_id: None,
            });
        }

        // CharShapes: Blueprint CharShape → HwpxCharShape
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
            });
        }

        // ParaShapes: Blueprint ParaShape → HwpxParaShape
        for ps in &registry.para_shapes {
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
            });
        }

        // Step 3: Inject default styles from the configured style set.
        // The order and IDs must match exactly what 한글 expects for this version.
        let defaults = store.style_set.default_styles();
        for (idx, entry) in defaults.iter().enumerate() {
            let next_style_id_ref = if entry.is_char_style() { 0 } else { idx as u32 };
            store.push_style(HwpxStyle {
                id: idx as u32,
                style_type: entry.style_type.to_string(),
                name: entry.name.to_string(),
                eng_name: entry.eng_name.to_string(),
                para_pr_id_ref: 0,
                char_pr_id_ref: 0,
                next_style_id_ref,
                lang_id: 1042, // Korean
            });
        }

        // Step 4: Add user's styles from registry (starting after defaults)
        let offset = defaults.len();
        for (i, (name, entry)) in registry.style_entries.iter().enumerate() {
            store.push_style(HwpxStyle {
                id: (offset + i) as u32,
                style_type: "PARA".to_string(),
                name: name.clone(),
                eng_name: name.clone(),
                para_pr_id_ref: entry.para_shape_id.get() as u32,
                char_pr_id_ref: entry.char_shape_id.get() as u32,
                next_style_id_ref: 0,
                lang_id: 1042, // Korean
            });
        }

        store
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
    } else {
        Alignment::Left
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_blueprint::builtins::builtin_default;
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

    // ── from_registry bridge tests ──────────────────────────────

    #[test]
    fn from_registry_empty_produces_empty_store() {
        let registry: StyleRegistry = serde_json::from_str(
            r#"{"fonts":[],"char_shapes":[],"para_shapes":[],"style_entries":{}}"#,
        )
        .unwrap();
        let store = HwpxStyleStore::from_registry(&registry);

        // Empty registry injects 한글-compatible defaults:
        // 1 font × 7 language groups, 1 default char shape, 1 default para shape,
        // 22 required styles (Modern default set)
        assert_eq!(store.font_count(), 7);
        assert_eq!(store.char_shape_count(), 1);
        assert_eq!(store.para_shape_count(), 1);
        assert_eq!(store.style_count(), 22);
    }

    #[test]
    fn from_registry_preserves_counts() {
        let template = builtin_default().unwrap();
        let registry = StyleRegistry::from_template(&template).unwrap();
        let store = HwpxStyleStore::from_registry(&registry);

        // Fonts are mirrored across 7 language groups (HANGUL, LATIN, HANJA, JAPANESE, OTHER, SYMBOL, USER)
        assert_eq!(store.font_count(), registry.font_count() * 7);
        assert_eq!(store.char_shape_count(), registry.char_shape_count());
        assert_eq!(store.para_shape_count(), registry.para_shape_count());
        // +22 for injected Modern default styles (the default HancomStyleSet)
        assert_eq!(store.style_count(), registry.style_count() + 22);
    }

    #[test]
    fn from_registry_font_face_names_match() {
        let template = builtin_default().unwrap();
        let registry = StyleRegistry::from_template(&template).unwrap();
        let store = HwpxStyleStore::from_registry(&registry);

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
        let store = HwpxStyleStore::from_registry(&registry);

        for (i, bp_cs) in registry.char_shapes.iter().enumerate() {
            let hwpx_cs = store.char_shape(CharShapeIndex::new(i)).unwrap();
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
        let store = HwpxStyleStore::from_registry(&registry);

        for (i, bp_ps) in registry.para_shapes.iter().enumerate() {
            let hwpx_ps = store.para_shape(ParaShapeIndex::new(i)).unwrap();
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
    fn from_registry_style_entries_reference_valid_indices() {
        let template = builtin_default().unwrap();
        let registry = StyleRegistry::from_template(&template).unwrap();
        let store = HwpxStyleStore::from_registry(&registry);

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
    fn from_registry_with_classic_style_set() {
        let registry: StyleRegistry = serde_json::from_str(
            r#"{"fonts":[],"char_shapes":[],"para_shapes":[],"style_entries":{}}"#,
        )
        .unwrap();
        let store = HwpxStyleStore::from_registry_with(&registry, HancomStyleSet::Classic);
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
}
