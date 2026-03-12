//! Bridge that combines [`HwpxStyleStore`] and [`ImageStore`] into a
//! single [`StyleLookup`] implementor.
//!
//! [`HwpxStyleLookup`] delegates style queries to the store and image
//! queries to the image store, giving downstream consumers (like the
//! Markdown encoder) a single `&dyn StyleLookup` to work with.
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::{ImageStore, StyleLookup};
//! use hwpforge_smithy_hwpx::style_store::HwpxStyleStore;
//! use hwpforge_smithy_hwpx::HwpxStyleLookup;
//!
//! let store = HwpxStyleStore::new();
//! let images = ImageStore::new();
//! let lookup = HwpxStyleLookup::new(&store, &images);
//!
//! // All queries delegate through the bridge
//! assert!(lookup.image_data("logo.png").is_none());
//! ```

use hwpforge_core::{ImageStore, StyleLookup};
use hwpforge_foundation::{
    Alignment, CharShapeIndex, Color, HwpUnit, ParaShapeIndex, StyleIndex, UnderlineType,
};

use crate::style_store::HwpxStyleStore;

/// A bridge combining [`HwpxStyleStore`] (style queries) and [`ImageStore`]
/// (binary image data) into a single [`StyleLookup`] implementor.
///
/// Style methods delegate to [`HwpxStyleStore`]'s `StyleLookup` impl.
/// [`image_data`](StyleLookup::image_data) delegates to [`ImageStore::get`].
pub struct HwpxStyleLookup<'a> {
    /// The HWPX style store (fonts, char shapes, para shapes, styles).
    pub styles: &'a HwpxStyleStore,
    /// The image binary data store.
    pub images: &'a ImageStore,
}

impl<'a> HwpxStyleLookup<'a> {
    /// Creates a new bridge from a style store and an image store.
    pub fn new(styles: &'a HwpxStyleStore, images: &'a ImageStore) -> Self {
        Self { styles, images }
    }
}

impl StyleLookup for HwpxStyleLookup<'_> {
    fn char_bold(&self, id: CharShapeIndex) -> Option<bool> {
        self.styles.char_bold(id)
    }

    fn char_italic(&self, id: CharShapeIndex) -> Option<bool> {
        self.styles.char_italic(id)
    }

    fn char_underline(&self, id: CharShapeIndex) -> Option<UnderlineType> {
        self.styles.char_underline(id)
    }

    fn char_strikeout(&self, id: CharShapeIndex) -> Option<bool> {
        self.styles.char_strikeout(id)
    }

    fn char_superscript(&self, id: CharShapeIndex) -> Option<bool> {
        self.styles.char_superscript(id)
    }

    fn char_subscript(&self, id: CharShapeIndex) -> Option<bool> {
        self.styles.char_subscript(id)
    }

    fn char_font_name(&self, id: CharShapeIndex) -> Option<&str> {
        self.styles.char_font_name(id)
    }

    fn char_font_size(&self, id: CharShapeIndex) -> Option<HwpUnit> {
        self.styles.char_font_size(id)
    }

    fn char_text_color(&self, id: CharShapeIndex) -> Option<Color> {
        self.styles.char_text_color(id)
    }

    fn para_alignment(&self, id: ParaShapeIndex) -> Option<Alignment> {
        self.styles.para_alignment(id)
    }

    fn para_list_type(&self, id: ParaShapeIndex) -> Option<&str> {
        self.styles.para_list_type(id)
    }

    fn style_name(&self, id: StyleIndex) -> Option<&str> {
        self.styles.style_name(id)
    }

    fn style_heading_level(&self, id: StyleIndex) -> Option<u8> {
        self.styles.style_heading_level(id)
    }

    fn image_resolve_filename(&self, key: &str) -> Option<&str> {
        let stripped = key.strip_prefix("BinData/").unwrap_or(key);
        // Always return a store-owned key (not the input) to satisfy lifetimes.
        self.images
            .iter()
            .find(|(k, _)| {
                *k == stripped || k.rsplit_once('.').is_some_and(|(stem, _)| stem == stripped)
            })
            .map(|(k, _)| k)
    }

    fn image_data(&self, key: &str) -> Option<&[u8]> {
        let resolved = self.image_resolve_filename(key)?;
        self.images.get(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style_store::{HwpxCharShape, HwpxFont, HwpxStyleStore};
    use hwpforge_core::ImageStore;
    use hwpforge_foundation::CharShapeIndex;

    #[test]
    fn bridge_delegates_style_queries() {
        let mut store = HwpxStyleStore::new();
        store.push_font(HwpxFont::new(0, "함초롬돋움", "HANGUL"));
        store.push_char_shape(HwpxCharShape { bold: true, ..Default::default() });

        let images = ImageStore::new();
        let lookup = HwpxStyleLookup::new(&store, &images);

        assert_eq!(lookup.char_bold(CharShapeIndex::new(0)), Some(true));
        assert_eq!(lookup.char_font_name(CharShapeIndex::new(0)), Some("함초롬돋움"));
    }

    #[test]
    fn bridge_delegates_image_data() {
        let store = HwpxStyleStore::new();
        let mut images = ImageStore::new();
        images.insert("logo.png", vec![0x89, 0x50, 0x4E, 0x47]);

        let lookup = HwpxStyleLookup::new(&store, &images);

        assert_eq!(lookup.image_data("logo.png"), Some(&[0x89, 0x50, 0x4E, 0x47][..]));
        assert!(lookup.image_data("missing.png").is_none());
    }

    #[test]
    fn bridge_as_trait_object() {
        let store = HwpxStyleStore::new();
        let images = ImageStore::new();
        let lookup = HwpxStyleLookup::new(&store, &images);

        let dyn_lookup: &dyn StyleLookup = &lookup;
        assert!(dyn_lookup.char_bold(CharShapeIndex::new(0)).is_none());
    }

    #[test]
    fn bridge_style_out_of_bounds_returns_none() {
        let store = HwpxStyleStore::new();
        let images = ImageStore::new();
        let lookup = HwpxStyleLookup::new(&store, &images);

        assert!(lookup.char_bold(CharShapeIndex::new(99)).is_none());
        assert!(lookup.char_font_name(CharShapeIndex::new(99)).is_none());
    }
}
