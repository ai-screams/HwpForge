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

    /// Returns the font size (in [`HwpUnit`]) of the character shape at `id`.
    fn char_font_size(&self, _id: CharShapeIndex) -> Option<HwpUnit> {
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

    /// Returns the list type for a paragraph shape: `"BULLET"`, `"NUMBER"`, or `None`.
    ///
    /// Returns `None` if the paragraph has no list heading or if the heading
    /// type is `NONE` / `OUTLINE`.
    fn para_list_type(&self, _id: ParaShapeIndex) -> Option<&str> {
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
        assert!(store.para_list_type(ps).is_none());
        assert!(store.style_name(si).is_none());
        assert!(store.style_heading_level(si).is_none());
        assert!(store.image_data("image1.jpg").is_none());
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
}
