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
        fn char_font_axis_names(&self, id: CharShapeIndex) -> Vec<&str>;
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
        fn char_style_shape(&self, name: &str) -> Option<CharShapeIndex>;
        fn border_fill_lines(&self, id: u32) -> Option<hwpforge_core::BorderFillLines>;
        fn border_fill_face(&self, id: u32) -> Option<hwpforge_core::FillKind>;
    }

    fn image_resolve_filename(&self, key: &str) -> Option<&str> {
        let stripped = key.strip_prefix("BinData/").unwrap_or(key);
        // W2a hardening (§3 disposition M5): 종전 단일 iter().find() 는
        // exact 와 stem 을 OR 로 섞어 HashMap 순서에 따라 다른 자산을
        // 돌려줄 수 있었다 (비결정). 우선순위 고정:
        // ① exact 일치 → ② stem 일치가 **유일**할 때만 → ③ 복수 stem =
        // None (모호 — 추측 금지). 실측 양 경로(HWPX sanitize 키·HWP5
        // storage_name)는 exact 로 정합하므로 stem 은 방어층이다.
        if let Some((k, _)) = self.images.iter().find(|(k, _)| *k == stripped) {
            return Some(k);
        }
        let mut stem_matches = self
            .images
            .iter()
            .filter(|(k, _)| k.rsplit_once('.').is_some_and(|(stem, _)| stem == stripped))
            .map(|(k, _)| k);
        let first = stem_matches.next()?;
        if stem_matches.next().is_some() {
            return None; // 복수 stem — 모호
        }
        Some(first)
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

    #[test]
    fn image_resolver_prefers_exact_over_stem() {
        // W2a M5: "logo.png" 와 "logo.png.jpg" 공존 시 exact 가 항상 이긴다
        // (종전 단일 find 는 HashMap 순서 비결정).
        let store = HwpxStyleStore::new();
        let mut images = ImageStore::new();
        images.insert("logo.png".to_string(), vec![1]);
        images.insert("logo.png.jpg".to_string(), vec![2]);
        let lookup = HwpxStyleLookup::new(&store, &images);
        assert_eq!(lookup.image_data("BinData/logo.png"), Some(&[1u8][..]));
    }

    #[test]
    fn image_resolver_unique_stem_matches() {
        // HWPX 디코더 키 관례: path="BinData/image1" ↔ 키 "image1.png".
        let store = HwpxStyleStore::new();
        let mut images = ImageStore::new();
        images.insert("image1.png".to_string(), vec![7]);
        let lookup = HwpxStyleLookup::new(&store, &images);
        assert_eq!(lookup.image_data("BinData/image1"), Some(&[7u8][..]));
    }

    #[test]
    fn image_resolver_ambiguous_stem_returns_none() {
        // 같은 stem 다른 확장자 2개 = 모호 — 추측하지 않는다.
        let store = HwpxStyleStore::new();
        let mut images = ImageStore::new();
        images.insert("logo.png".to_string(), vec![1]);
        images.insert("logo.jpg".to_string(), vec![2]);
        let lookup = HwpxStyleLookup::new(&store, &images);
        assert_eq!(lookup.image_data("BinData/logo"), None);
    }
}
