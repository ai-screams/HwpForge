//! HWP5 style store — parsed style definitions from the `DocInfo` stream.
//!
//! [`Hwp5StyleStore`] holds font tables, character property arrays, paragraph
//! property arrays, list definitions, and named styles extracted from the HWP5
//! `DocInfo` binary stream. It is a **format-neutral** snapshot of HWP5 style
//! data; mapping it onto an output format (for example HWPX) lives in the
//! `hwpforge-convert` orchestrator crate, not here.

use crate::decoder::header::{
    DocInfoResult, Hwp5DocInfoBorderFillSlot, Hwp5DocInfoBulletSlot, Hwp5DocInfoNumberingSlot,
};
use crate::schema::border_fill::Hwp5RawBorderFillFill;
use crate::schema::header::{
    Hwp5RawCharShape, Hwp5RawFaceName, Hwp5RawIdMappings, Hwp5RawParaShape, Hwp5RawStyle,
    Hwp5TabDefSlot,
};
use std::collections::BTreeSet;

/// Intermediate style data parsed from HWP5's DocInfo stream.
///
/// Holds all font, character shape, paragraph shape, and named style
/// definitions in their HWP5-native form. Conversion to an output format's
/// style store (for example `HwpxStyleStore`) is performed by the
/// `hwpforge-convert` orchestrator.
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
    /// Numbering definition slots.
    ///
    /// These are projected into shared `NumberingDef` entries before HWPX
    /// header serialization.
    pub numberings: Vec<Hwp5DocInfoNumberingSlot>,
    /// Bullet definition slots.
    pub bullets: Vec<Hwp5DocInfoBulletSlot>,
    /// Tab definition slots preserved in DocInfo order.
    pub tab_defs: Vec<Hwp5TabDefSlot>,
    /// Named style records.
    pub styles: Vec<Hwp5RawStyle>,
    /// Border/fill definition slots, preserved in DocInfo record order.
    pub border_fills: Vec<Hwp5DocInfoBorderFillSlot>,
}

impl Hwp5StyleStore {
    /// Construct a [`Hwp5StyleStore`] from a parsed [`DocInfoResult`].
    pub fn from_doc_info(doc_info: &DocInfoResult) -> Self {
        Self {
            id_mappings: doc_info.id_mappings.clone(),
            fonts: doc_info.fonts.clone(),
            char_shapes: doc_info.char_shapes.clone(),
            para_shapes: doc_info.para_shapes.clone(),
            numberings: doc_info.numberings.clone(),
            bullets: doc_info.bullets.clone(),
            tab_defs: doc_info.tab_defs.clone(),
            styles: doc_info.styles.clone(),
            border_fills: doc_info.border_fills.clone(),
        }
    }

    /// Border/fill definition slots parsed from DocInfo, in record order.
    ///
    /// Exposed so output-format orchestrators (for example `hwpforge-convert`)
    /// can map HWP5 border/fill definitions onto their own style store without
    /// reaching into private fields.
    pub fn border_fills(&self) -> &[Hwp5DocInfoBorderFillSlot] {
        &self.border_fills
    }

    pub(crate) fn border_fill_image_binary_ids(&self) -> BTreeSet<u16> {
        self.border_fills
            .iter()
            .filter_map(|slot| match slot.fill.as_ref()?.fill {
                Hwp5RawBorderFillFill::Image(ref fill) => Some(fill.bindata_id),
                _ => None,
            })
            .collect()
    }
}
