//! Format-agnostic style querying trait.
//!
//! [`StyleLookup`] provides a uniform interface for retrieving character,
//! paragraph, and style properties by index. Each format-specific style
//! store (e.g. `HwpxStyleStore`) implements this trait so that downstream
//! consumers (e.g. the Markdown encoder) can query styles without knowing
//! the underlying format.
//!
//! All methods have default implementations returning `None`, so
//! implementors only need to override the methods they can support.

use hwpforge_foundation::{
    Alignment, CharShapeIndex, Color, HwpUnit, ParaShapeIndex, StyleIndex, UnderlineType,
};

/// Trait for querying resolved style properties by index.
///
/// This is the bridge between format-specific style stores and
/// format-independent consumers (like the Markdown encoder). Each method
/// takes a branded index and returns `Option<T>`, where `None` means the
/// property is unavailable or unsupported.
///
/// # Default Implementations
///
/// Every method defaults to `None`, so an empty implementation is valid:
///
/// ```
/// use hwpforge_core::StyleLookup;
/// use hwpforge_foundation::CharShapeIndex;
///
/// struct NoopStore;
/// impl StyleLookup for NoopStore {}
///
/// let store = NoopStore;
/// assert!(store.char_bold(CharShapeIndex::new(0)).is_none());
/// ```
pub trait StyleLookup {
    /// Returns whether the character shape at `id` is bold.
    fn char_bold(&self, _id: CharShapeIndex) -> Option<bool> {
        None
    }

    /// Returns whether the character shape at `id` is italic.
    fn char_italic(&self, _id: CharShapeIndex) -> Option<bool> {
        None
    }

    /// Returns the underline type of the character shape at `id`.
    fn char_underline(&self, _id: CharShapeIndex) -> Option<UnderlineType> {
        None
    }

    /// Returns whether the character shape at `id` has strikeout.
    fn char_strikeout(&self, _id: CharShapeIndex) -> Option<bool> {
        None
    }

    /// Returns whether the character shape at `id` is superscript.
    fn char_superscript(&self, _id: CharShapeIndex) -> Option<bool> {
        None
    }

    /// Returns whether the character shape at `id` is subscript.
    fn char_subscript(&self, _id: CharShapeIndex) -> Option<bool> {
        None
    }

    /// Returns the font name of the character shape at `id`.
    fn char_font_name(&self, _id: CharShapeIndex) -> Option<&str> {
        None
    }

    /// Returns the **distinct** font face names referenced across the
    /// per-language axes (hangul/latin/hanja/…) of the character shape.
    ///
    /// Formats with per-language font references (HWPX `fontRef`) override
    /// this to surface axis mismatches — a result longer than 1 means the
    /// character shape renders with different fonts per script, which a
    /// single-font consumer cannot reproduce faithfully. The first element
    /// matches [`char_font_name`](Self::char_font_name) when both resolve.
    ///
    /// The default implementation returns the single
    /// [`char_font_name`](Self::char_font_name) (no axis information).
    fn char_font_axis_names(&self, id: CharShapeIndex) -> Vec<&str> {
        self.char_font_name(id).into_iter().collect()
    }

    /// Returns the font size (in [`HwpUnit`]) of the character shape at `id`.
    fn char_font_size(&self, _id: CharShapeIndex) -> Option<HwpUnit> {
        None
    }

    /// Returns the char shape referenced by the named **character** style
    /// (style table `type="CHAR"`), matching either the localized or the
    /// English style name.
    ///
    /// Hancom renders page numbers (`hp:pageNum`) with the dedicated
    /// "쪽 번호"/"Page Number" CHAR style rather than the document default —
    /// fixture-verified (rules-pagenum, 2026-08-10). Consumers that
    /// synthesize such text need the style's char shape to match Hancom
    /// output. The default implementation reports no style table.
    fn char_style_shape(&self, _name: &str) -> Option<CharShapeIndex> {
        None
    }

    /// Returns the text color of the character shape at `id`.
    fn char_text_color(&self, _id: CharShapeIndex) -> Option<Color> {
        None
    }

    /// Returns the horizontal alignment of the paragraph shape at `id`.
    fn para_alignment(&self, _id: ParaShapeIndex) -> Option<Alignment> {
        None
    }

    /// Returns the left indent of the paragraph shape at `id`.
    fn para_indent_left(&self, _id: ParaShapeIndex) -> Option<HwpUnit> {
        None
    }

    /// Returns the first-line indent of the paragraph shape at `id`.
    fn para_indent_first_line(&self, _id: ParaShapeIndex) -> Option<HwpUnit> {
        None
    }

    /// Returns the list type for a paragraph shape: `"BULLET"`, `"NUMBER"`, or `None`.
    ///
    /// Returns `None` if the paragraph has no list heading or if the heading
    /// type is `NONE` / `OUTLINE`.
    fn para_list_type(&self, _id: ParaShapeIndex) -> Option<&str> {
        None
    }

    /// Returns the zero-based list nesting level for a paragraph shape.
    ///
    /// This is only meaningful for numbered/bulleted list semantics. Outline
    /// headings should use [`para_heading_level`](Self::para_heading_level)
    /// instead.
    fn para_list_level(&self, _id: ParaShapeIndex) -> Option<u8> {
        None
    }

    /// Returns the checkbox state for a paragraph shape when it is a checkable bullet.
    ///
    /// `Some(true)` means a checked checkbox item, `Some(false)` means an
    /// unchecked checkbox item, and `None` means the paragraph is not a
    /// checkable bullet.
    fn para_checked_state(&self, _id: ParaShapeIndex) -> Option<bool> {
        None
    }

    /// Returns the preferred style name associated with the paragraph shape.
    ///
    /// This is useful for encoders that need to recover semantics carried by a
    /// dedicated paragraph shape even when the paragraph itself has no explicit
    /// `style_id`.
    fn para_style_name(&self, _id: ParaShapeIndex) -> Option<&str> {
        None
    }

    /// Returns the heading level (1–6) implied by the paragraph shape at `id`.
    ///
    /// This is the format-agnostic truth source for paragraph-level outline
    /// semantics. Implementors that can inspect real paragraph-shape outline
    /// metadata should override this method; downstream styled export paths use
    /// it before style-name heuristics whenever both are available.
    fn para_heading_level(&self, _id: ParaShapeIndex) -> Option<u8> {
        None
    }

    /// Returns the Korean name of the style at `id`.
    fn style_name(&self, _id: StyleIndex) -> Option<&str> {
        None
    }

    /// Returns the heading level (1–6) of the style at `id`, if it is
    /// a heading style. Returns `None` for non-heading styles.
    fn style_heading_level(&self, _id: StyleIndex) -> Option<u8> {
        None
    }

    /// Resolves a `binaryItemIDRef` (e.g. `"BinData/image1"`) to the actual
    /// filename with extension (e.g. `"image1.png"`).
    ///
    /// Returns `None` if no matching image is found.
    fn image_resolve_filename(&self, _key: &str) -> Option<&str> {
        None
    }

    /// Returns the raw binary data for the image identified by `key`.
    ///
    /// `key` is typically a path like `"image1.jpg"`. Returns `None` if
    /// the image is not available or if the implementor does not store
    /// image data.
    fn image_data(&self, _key: &str) -> Option<&[u8]> {
        None
    }

    /// Returns the four rendered border edges of the `borderFill` at `id`
    /// (1-based wire reference, e.g. [`crate::table::TableCell::border_fill_id`]).
    ///
    /// `None` means the id is not registered in this store.
    fn border_fill_lines(&self, _id: u32) -> Option<BorderFillLines> {
        None
    }

    /// Returns the face-fill verdict of the `borderFill` at `id` (1-based
    /// wire reference).
    ///
    /// `None` means the id is not registered — distinguish this from
    /// [`FillKind::None`] (registered, but transparent) and
    /// [`FillKind::Unsupported`] (registered, but not renderable — warn).
    fn border_fill_face(&self, _id: u32) -> Option<FillKind> {
        None
    }
}

/// Render kind of one border edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BorderLineKind {
    /// No line on this edge.
    None,
    /// Solid stroke.
    Solid,
    /// Any other style (dashed, double, …) or an unparsable width/color —
    /// consumers must warn and skip instead of guessing (no fake support).
    Other,
}

/// One rendered border edge of a `borderFill`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct BorderLine {
    /// Render kind of this edge.
    pub kind: BorderLineKind,
    /// Stroke width ([`HwpUnit::ZERO`] when [`BorderLineKind::None`]/`Other`).
    pub width: HwpUnit,
    /// Stroke color.
    pub color: Color,
}

impl BorderLine {
    /// Creates a border line.
    #[must_use]
    pub fn new(kind: BorderLineKind, width: HwpUnit, color: Color) -> Self {
        Self { kind, width, color }
    }
}

/// The four rendered edges of a `borderFill`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct BorderFillLines {
    /// Left edge.
    pub left: BorderLine,
    /// Right edge.
    pub right: BorderLine,
    /// Top edge.
    pub top: BorderLine,
    /// Bottom edge.
    pub bottom: BorderLine,
}

impl BorderFillLines {
    /// Creates the four edges.
    #[must_use]
    pub fn new(left: BorderLine, right: BorderLine, top: BorderLine, bottom: BorderLine) -> Self {
        Self { left, right, top, bottom }
    }
}

/// Face-fill verdict of a `borderFill` — distinguishes "no fill" from
/// "unsupported fill": only the latter warrants a consumer warning.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum FillKind {
    /// No fill (transparent) — normal, no warning.
    None,
    /// Solid color fill.
    Solid(Color),
    /// Gradient/image/hatch fill or an unparsable color — consumers must
    /// warn and skip instead of guessing (no fake support).
    Unsupported,
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_foundation::{ParaShapeIndex, StyleIndex};

    struct NoopStore;
    impl StyleLookup for NoopStore {}

    #[test]
    fn noop_store_returns_none_for_all_methods() {
        let store = NoopStore;
        let cs = CharShapeIndex::new(0);
        let ps = ParaShapeIndex::new(0);
        let si = StyleIndex::new(0);

        assert!(store.char_bold(cs).is_none());
        assert!(store.char_italic(cs).is_none());
        assert!(store.char_underline(cs).is_none());
        assert!(store.char_strikeout(cs).is_none());
        assert!(store.char_superscript(cs).is_none());
        assert!(store.char_subscript(cs).is_none());
        assert!(store.char_font_name(cs).is_none());
        assert!(store.char_font_size(cs).is_none());
        assert!(store.char_text_color(cs).is_none());
        assert!(store.para_alignment(ps).is_none());
        assert!(store.para_indent_left(ps).is_none());
        assert!(store.para_indent_first_line(ps).is_none());
        assert!(store.para_list_type(ps).is_none());
        assert!(store.para_list_level(ps).is_none());
        assert!(store.para_checked_state(ps).is_none());
        assert!(store.para_style_name(ps).is_none());
        assert!(store.para_heading_level(ps).is_none());
        assert!(store.style_name(si).is_none());
        assert!(store.style_heading_level(si).is_none());
        assert!(store.image_data("image1.jpg").is_none());
        assert!(store.border_fill_lines(1).is_none());
        assert!(store.border_fill_face(1).is_none());
    }

    #[test]
    fn partial_impl_returns_some_for_overridden_methods() {
        struct BoldOnly;
        impl StyleLookup for BoldOnly {
            fn char_bold(&self, _id: CharShapeIndex) -> Option<bool> {
                Some(true)
            }
        }

        let store = BoldOnly;
        assert_eq!(store.char_bold(CharShapeIndex::new(0)), Some(true));
        // Non-overridden methods still return None
        assert!(store.char_italic(CharShapeIndex::new(0)).is_none());
    }

    #[test]
    fn trait_object_works() {
        let store: &dyn StyleLookup = &NoopStore;
        assert!(store.char_bold(CharShapeIndex::new(0)).is_none());
    }

    #[test]
    fn default_axis_names_mirror_single_font_name() {
        // 기본 구현 = char_font_name 단일 원소 (축 정보 없는 포맷).
        let cs = CharShapeIndex::new(0);
        assert!(NoopStore.char_font_axis_names(cs).is_empty());

        struct OneFont;
        impl StyleLookup for OneFont {
            fn char_font_name(&self, _id: CharShapeIndex) -> Option<&str> {
                Some("함초롬바탕")
            }
        }
        assert_eq!(OneFont.char_font_axis_names(cs), vec!["함초롬바탕"]);
    }
}
