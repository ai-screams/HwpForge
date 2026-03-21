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

macro_rules! delegate_style_lookup_to_store {
    ($(fn $name:ident(&self, $arg:ident : $arg_ty:ty) -> $ret:ty;)+) => {
        $(
            fn $name(&self, $arg: $arg_ty) -> $ret {
                self.styles.$name($arg)
            }
        )+
    };
}

impl StyleLookup for HwpxStyleLookup<'_> {
    delegate_style_lookup_to_store! {
        fn char_bold(&self, id: CharShapeIndex) -> Option<bool>;
        fn char_italic(&self, id: CharShapeIndex) -> Option<bool>;
        fn char_underline(&self, id: CharShapeIndex) -> Option<UnderlineType>;
        fn char_strikeout(&self, id: CharShapeIndex) -> Option<bool>;
        fn char_superscript(&self, id: CharShapeIndex) -> Option<bool>;
        fn char_subscript(&self, id: CharShapeIndex) -> Option<bool>;
        fn char_font_name(&self, id: CharShapeIndex) -> Option<&str>;
        fn char_font_size(&self, id: CharShapeIndex) -> Option<HwpUnit>;
        fn char_text_color(&self, id: CharShapeIndex) -> Option<Color>;
        fn para_alignment(&self, id: ParaShapeIndex) -> Option<Alignment>;
        fn para_list_type(&self, id: ParaShapeIndex) -> Option<&str>;
        fn para_list_level(&self, id: ParaShapeIndex) -> Option<u8>;
        fn para_checked_state(&self, id: ParaShapeIndex) -> Option<bool>;
        fn para_style_name(&self, id: ParaShapeIndex) -> Option<&str>;
        fn para_heading_level(&self, id: ParaShapeIndex) -> Option<u8>;
        fn style_name(&self, id: StyleIndex) -> Option<&str>;
        fn style_heading_level(&self, id: StyleIndex) -> Option<u8>;
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
    use crate::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore};
    use hwpforge_core::ImageStore;
    use hwpforge_foundation::{CharShapeIndex, HeadingType, NumberFormatType, ParaShapeIndex};

    #[test]
    fn bridge_delegates_style_queries() {
        let mut store = HwpxStyleStore::new();
        store.push_font(HwpxFont::new(0, "함초롬돋움", "HANGUL"));
        store.push_char_shape(HwpxCharShape { bold: true, ..Default::default() });
        store.push_para_shape(HwpxParaShape {
            heading_type: HeadingType::Outline,
            heading_level: 1,
            ..Default::default()
        });
        store.push_bullet(hwpforge_core::BulletDef {
            id: 7,
            bullet_char: "☐".into(),
            checked_char: Some("☑".into()),
            use_image: false,
            para_head: hwpforge_core::ParaHead {
                start: 0,
                level: 1,
                num_format: NumberFormatType::Digit,
                text: String::new(),
                checkable: true,
            },
        });
        store.push_para_shape(HwpxParaShape {
            heading_type: HeadingType::Bullet,
            heading_id_ref: 7,
            checked: true,
            ..Default::default()
        });

        let images = ImageStore::new();
        let lookup = HwpxStyleLookup::new(&store, &images);

        assert_eq!(lookup.char_bold(CharShapeIndex::new(0)), Some(true));
        assert_eq!(lookup.char_font_name(CharShapeIndex::new(0)), Some("함초롬돋움"));
        assert_eq!(lookup.para_heading_level(ParaShapeIndex::new(0)), Some(2));
        assert_eq!(lookup.para_checked_state(ParaShapeIndex::new(1)), Some(true));
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
