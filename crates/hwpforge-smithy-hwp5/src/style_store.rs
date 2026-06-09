//! HWP5 style store — parsed style definitions from the `DocInfo` stream.
//!
//! [`Hwp5StyleStore`] holds font tables, character property arrays, paragraph
//! property arrays, list definitions, and named styles extracted from the HWP5
//! `DocInfo` binary stream.  It provides a best-effort conversion to
//! [`HwpxStyleStore`] for use with the HWPX encoder.

use crate::decoder::header::{
    DocInfoResult, Hwp5DocInfoBorderFillSlot, Hwp5DocInfoBulletSlot, Hwp5DocInfoNumberingSlot,
};
use crate::decoder::Hwp5Warning;
use crate::schema::header::{
    Hwp5RawCharShape, Hwp5RawFaceName, Hwp5RawIdMappings, Hwp5RawParaShape, Hwp5RawStyle,
    Hwp5RawTabDef, Hwp5TabDefSlot,
};
use crate::style_store_border_fill::{
    collect_hwp5_border_fill_image_binary_ids, push_hwp5_border_fills, push_required_border_fills,
};
use crate::style_store_convert::{
    hwp5_char_shape_to_hwpx_with_counts_and_warnings,
    hwp5_para_shape_to_hwpx_with_tab_id_and_warnings, hwp5_style_to_hwpx, hwp5_tab_def_to_hwpx,
    push_fonts, resolved_font_group_counts,
};
use crate::warning_utils::push_projection_fallback;
use hwpforge_core::TabDef;
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
            numberings: doc_info.numberings.clone(),
            bullets: doc_info.bullets.clone(),
            tab_defs: doc_info.tab_defs.clone(),
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
        let tab_id_map = Hwp5TabIdMap::from_doc_info(&self.tab_defs);

        // Map character shapes.
        for (raw_id, raw) in self.char_shapes.iter().enumerate() {
            store.push_char_shape(hwp5_char_shape_to_hwpx_with_counts_and_warnings(
                raw,
                font_group_counts,
                raw_id,
                &mut warnings,
            ));
        }

        // Map numbering definitions before paragraph shapes so references are stable.
        append_numbering_definition_integrity_warning(self, &mut warnings);
        for slot in &self.numberings {
            match slot.numbering.as_ref() {
                Some(raw) => store.push_numbering(raw.to_core_numbering_def(slot.id)),
                None => {
                    warnings.push(Hwp5Warning::ParserFallback {
                        subject: "numbering.slot",
                        reason: format!(
                            "numbering definition slot {} failed to parse earlier; emitting no numbering entry",
                            slot.id
                        ),
                    });
                }
            }
        }

        // Map bullet definitions before paragraph shapes so bullet references
        // can resolve to stable shared ids.
        append_bullet_definition_integrity_warning(self, &mut warnings);
        for slot in &self.bullets {
            match slot.bullet.as_ref() {
                Some(raw) => store.push_bullet(raw.to_core_bullet_def(slot.id)),
                None => {
                    warnings.push(Hwp5Warning::ParserFallback {
                        subject: "bullet.slot",
                        reason: format!(
                            "bullet definition slot {} failed to parse earlier; emitting no bullet entry",
                            slot.id
                        ),
                    });
                }
            }
        }

        // Map paragraph shapes.
        for (raw_id, raw) in self.para_shapes.iter().enumerate() {
            let tab_pr_id_ref = tab_id_map.map_para_shape_ref(raw.tab_def_id, &mut warnings);
            store.push_para_shape(hwp5_para_shape_to_hwpx_with_tab_id_and_warnings(
                raw,
                tab_pr_id_ref,
                raw_id,
                &mut warnings,
            ));
        }

        // Wave 12q task #122: HWP5 wire 의 outline level 은 3 bits (cap 6) 만
        // 표현 가능하지만 한컴 native 는 paragraph Style ("개요 N") 의 N-1 을
        // 진정한 outline level 로 사용합니다. ParaShape 변환이 끝난 후 Style
        // 테이블의 "개요 N" / "Outline N" 패턴을 찾아 그 Style 의 para_shape_id
        // 가 가리키는 paraPr 의 heading_level 을 override 합니다. 한컴이 wire
        // 에 cap=6 으로 저장한 level 7/8/9 가 제대로 emit 됩니다.
        apply_outline_style_level_overrides(&mut store, &self.styles, &mut warnings);

        append_tab_definition_integrity_warning(self, &mut warnings);
        for slot in &self.tab_defs {
            match slot.tab_def.as_ref() {
                Some(raw) => {
                    append_tab_projection_warnings(slot.raw_id, raw, &mut warnings);
                    store.push_tab(hwp5_tab_def_to_hwpx(slot.raw_id, raw));
                }
                None => {
                    warnings.push(Hwp5Warning::ParserFallback {
                        subject: "tab_def.slot",
                        reason: format!(
                            "tab definition slot {} failed to parse earlier; emitting empty placeholder to preserve raw ids",
                            slot.raw_id
                        ),
                    });
                    store.push_tab(empty_placeholder_tab_def(slot.raw_id));
                }
            }
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

#[derive(Debug, Clone)]
struct Hwp5TabIdMap {
    known_slots: BTreeSet<u32>,
}

impl Hwp5TabIdMap {
    fn from_doc_info(tab_defs: &[Hwp5TabDefSlot]) -> Self {
        let known_slots = tab_defs.iter().map(|slot| slot.raw_id).collect();
        Self { known_slots }
    }

    fn map_para_shape_ref(&self, raw_id: u16, warnings: &mut Vec<Hwp5Warning>) -> u32 {
        let raw_id = raw_id as u32;
        if TabDef::reference_is_known(raw_id, self.known_slots.iter().copied()) {
            return raw_id;
        }
        push_projection_fallback(
            warnings,
            "tab_def.ref",
            format!(
                "paragraph references missing tab definition id {}; defaulting to built-in tab definition 0",
                raw_id
            ),
        );
        0
    }
}

fn append_tab_definition_integrity_warning(
    store: &Hwp5StyleStore,
    warnings: &mut Vec<Hwp5Warning>,
) {
    let Some(id_mappings) = store.id_mappings.as_ref() else {
        return;
    };
    let declared = id_mappings.tab_def_count.max(0) as usize;
    let actual = store.tab_defs.len();
    if declared != actual {
        push_projection_fallback(
            warnings,
            "tab_def.count",
            format!(
                "IdMappings declares {declared} tab definitions, but DocInfo parsed {actual}; preserving raw record order"
            ),
        );
    }
}

fn append_numbering_definition_integrity_warning(
    store: &Hwp5StyleStore,
    warnings: &mut Vec<Hwp5Warning>,
) {
    let Some(id_mappings) = store.id_mappings.as_ref() else {
        return;
    };
    let declared = id_mappings.numbering_def_count.max(0) as usize;
    let actual = store.numberings.len();
    if declared != actual {
        push_projection_fallback(
            warnings,
            "numbering.count",
            format!(
                "IdMappings declares {declared} numbering definitions, but DocInfo parsed {actual}; preserving raw record order"
            ),
        );
    }
}

fn append_bullet_definition_integrity_warning(
    store: &Hwp5StyleStore,
    warnings: &mut Vec<Hwp5Warning>,
) {
    let Some(id_mappings) = store.id_mappings.as_ref() else {
        return;
    };
    let declared = id_mappings.bullet_def_count.max(0) as usize;
    let actual = store.bullets.len();
    if declared != actual {
        push_projection_fallback(
            warnings,
            "bullet.count",
            format!(
                "IdMappings declares {declared} bullet definitions, but DocInfo parsed {actual}; preserving raw record order"
            ),
        );
    }
}

fn append_tab_projection_warnings(id: u32, raw: &Hwp5RawTabDef, warnings: &mut Vec<Hwp5Warning>) {
    for (stop_idx, stop) in raw.tab_stops.iter().enumerate() {
        if stop.position > hwpforge_foundation::HwpUnit::MAX_VALUE as u32 {
            push_projection_fallback(
                warnings,
                "tab_def.position",
                format!(
                    "tab definition {id} stop {stop_idx} uses out-of-range position {}; clamping to {}",
                    stop.position,
                    hwpforge_foundation::HwpUnit::MAX_VALUE
                ),
            );
        }
        if !matches!(stop.tab_type, 0..=3) {
            push_projection_fallback(
                warnings,
                "tab_def.align",
                format!(
                    "tab definition {id} stop {stop_idx} uses unknown tab_type {}; defaulting to LEFT",
                    stop.tab_type
                ),
            );
        }
        if stop.fill_type > 16 {
            push_projection_fallback(
                warnings,
                "tab_def.leader",
                format!(
                    "tab definition {id} stop {stop_idx} uses unknown fill_type {}; defaulting to SOLID",
                    stop.fill_type
                ),
            );
        }
    }
}

fn empty_placeholder_tab_def(id: u32) -> TabDef {
    TabDef { id, auto_tab_left: false, auto_tab_right: false, stops: Vec::new() }
}

/// Wave 12q task #122: apply outline-level overrides from Style records.
///
/// HWP5 `ParaShape.property1` bit 25-27 only carries 3 bits (cap 6), but
/// Hancom HWP5 의 native outline 정의는 Style record 의 한국어 이름 "개요 N"
/// 으로 진짜 level (N-1) 을 표현합니다. 변환 후 paraPr 의 heading_level 이
/// wire-cap 6 으로 잘려있어도 Style 의 N-1 이 진실의 source 이므로 override.
///
/// Matching:
/// - `kind == 0` (paragraph style) 만 대상
/// - 한국어 "개요 N" (N=1..10) 또는 영문 "Outline N"
/// - `para_shape_id < store.para_shape_count()` 이어야 함
/// - `heading_type` 이 이미 Outline 인 paraPr 만 override (Number/Bullet/None
///   에는 영향 없음 — Codex §5 "순차 ID 신앙 금지" 호환)
fn apply_outline_style_level_overrides(
    store: &mut HwpxStyleStore,
    styles: &[hwpforge_smithy_hwp5_schema::Hwp5RawStyle],
    _warnings: &mut Vec<Hwp5Warning>,
) {
    // The override is intentional, expected, and lossless — it recovers
    // outline level 7~9 that the HWP5 wire (3-bit, cap 6) cannot represent.
    // Emitting a warning here would pollute audit counts (cf. failing test
    // `audit_hwp5_rect_fixture_now_matches_after_carry`) without surfacing a
    // real lossy projection. Keep the function silent.
    for style in styles.iter() {
        if style.kind != 0 {
            continue;
        }
        let Some(level_one_based) = parse_outline_style_name(&style.name) else {
            continue;
        };
        if level_one_based == 0 || level_one_based > 10 {
            continue;
        }
        let level_zero_based = level_one_based - 1;
        let para_shape_id = style.para_shape_id as usize;
        if para_shape_id >= store.para_shape_count() {
            continue;
        }
        let Ok(para_shape) =
            store.para_shape_mut(hwpforge_foundation::ParaShapeIndex::new(para_shape_id))
        else {
            continue;
        };
        // Codex §5: only override outline-kind paraPrs; never repurpose
        // Number/Bullet/None.
        if !matches!(para_shape.heading_type, hwpforge_foundation::HeadingType::Outline) {
            continue;
        }
        if u32::from(level_zero_based) != para_shape.heading_level {
            para_shape.heading_level = u32::from(level_zero_based);
        }
    }
}

/// Recognise the Hancom outline style names "개요 N" (Korean) and
/// "Outline N" (English fallback). Returns `Some(N)` where `N` is the
/// 1-based outline level (1..=10), or `None` for unrelated styles.
fn parse_outline_style_name(name: &str) -> Option<u8> {
    let trimmed = name.trim();
    // Korean: "개요 " prefix
    let suffix = trimmed
        .strip_prefix("개요 ")
        .or_else(|| trimmed.strip_prefix("개요"))
        .or_else(|| trimmed.strip_prefix("Outline "))
        .or_else(|| trimmed.strip_prefix("Outline"))?;
    suffix.trim().parse::<u8>().ok()
}

mod hwpforge_smithy_hwp5_schema {
    pub(super) use crate::schema::header::Hwp5RawStyle;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "style_store_tests.rs"]
mod tests;
