//! HWP5 style store — parsed style definitions from the `DocInfo` stream.
//!
//! [`Hwp5StyleStore`] holds font tables, character property arrays, paragraph
//! property arrays, and named styles extracted from the HWP5 `DocInfo`
//! binary stream.  It provides a best-effort conversion to [`HwpxStyleStore`]
//! for use with the HWPX encoder.

use crate::decoder::header::{DocInfoResult, Hwp5DocInfoBorderFillSlot};
use crate::decoder::Hwp5Warning;
use crate::schema::header::{
    Hwp5RawCharShape, Hwp5RawFaceName, Hwp5RawIdMappings, Hwp5RawParaShape, Hwp5RawStyle,
};
use crate::style_store_border_fill::{
    collect_hwp5_border_fill_image_binary_ids, push_hwp5_border_fills, push_required_border_fills,
};
use crate::style_store_convert::{
    hwp5_char_shape_to_hwpx_with_counts, hwp5_para_shape_to_hwpx, hwp5_style_to_hwpx, push_fonts,
    resolved_font_group_counts,
};
use hwpforge_smithy_hwpx::HwpxStyleStore;
use std::collections::BTreeSet;

/// Intermediate style data parsed from HWP5's DocInfo stream.
///
/// Holds all font, character shape, paragraph shape, and named style
/// definitions. Provides conversion to [`HwpxStyleStore`] for HWPX output.
#[derive(Debug, Clone)]
pub struct Hwp5StyleStore {
    /// Optional IdMappings record used to reconstruct font buckets.
    pub id_mappings: Option<Hwp5RawIdMappings>,
    /// Font face name records (one per FaceName record in DocInfo).
    pub fonts: Vec<Hwp5RawFaceName>,
    /// Character shape records.
    pub char_shapes: Vec<Hwp5RawCharShape>,
    /// Paragraph shape records.
    pub para_shapes: Vec<Hwp5RawParaShape>,
    /// Named style records.
    pub styles: Vec<Hwp5RawStyle>,
    /// Border/fill records.
    pub(crate) border_fills: Vec<Hwp5DocInfoBorderFillSlot>,
}

impl Hwp5StyleStore {
    /// Construct a [`Hwp5StyleStore`] from parsed DocInfo.
    pub(crate) fn from_doc_info(doc_info: &DocInfoResult) -> Self {
        Self {
            id_mappings: doc_info.id_mappings.clone(),
            fonts: doc_info.fonts.clone(),
            char_shapes: doc_info.char_shapes.clone(),
            para_shapes: doc_info.para_shapes.clone(),
            styles: doc_info.styles.clone(),
            border_fills: doc_info.border_fills.clone(),
        }
    }

    /// Convert to [`HwpxStyleStore`] for use with the HWPX encoder.
    ///
    /// This is a best-effort conversion. Fields that cannot be mapped cleanly
    /// use defaults from the `"default"` preset. This convenience wrapper
    /// discards non-fatal conversion warnings; the main `hwp5_to_hwpx()`
    /// pipeline uses the warning-aware internal variant so unsupported style
    /// payloads can surface as explicit fallbacks instead of silent lies.
    pub fn to_hwpx_style_store(&self) -> HwpxStyleStore {
        self.to_hwpx_style_store_with_warnings().0
    }

    pub(crate) fn to_hwpx_style_store_with_warnings(&self) -> (HwpxStyleStore, Vec<Hwp5Warning>) {
        let mut store = HwpxStyleStore::new();
        let mut warnings: Vec<Hwp5Warning> = Vec::new();
        if self.border_fills.is_empty() {
            push_required_border_fills(&mut store);
        } else {
            push_hwp5_border_fills(&mut store, &self.border_fills, &mut warnings);
        }
        push_fonts(&mut store, self);
        let font_group_counts = resolved_font_group_counts(self);

        // Map character shapes.
        for raw in &self.char_shapes {
            store.push_char_shape(hwp5_char_shape_to_hwpx_with_counts(raw, font_group_counts));
        }

        // Map paragraph shapes.
        for raw in &self.para_shapes {
            store.push_para_shape(hwp5_para_shape_to_hwpx(raw));
        }

        for (idx, raw) in self.styles.iter().enumerate() {
            store.push_style(hwp5_style_to_hwpx(idx as u32, raw, self.styles.len()));
        }

        (store, warnings)
    }

    pub(crate) fn border_fill_image_binary_ids(&self) -> BTreeSet<u16> {
        collect_hwp5_border_fill_image_binary_ids(&self.border_fills)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "style_store_tests.rs"]
mod tests;
