//! Cross-format conversion orchestrator for HwpForge.
//!
//! This crate sits *above* the format-specific Smithy crates and wires them
//! together through the neutral Core IR: `decode(format A) -> Core Document ->
//! encode(format B)`. Keeping orchestration here lets each Smithy crate depend
//! only on Core (peer-equality) and opens a path for additional output formats
//! without modifying the decoders.
#![deny(missing_docs)]

mod layout_hint_patch;
mod style_store_border_fill;
mod style_store_convert;
mod warning_utils;

use std::path::Path;

use hwpforge_foundation::{HeadingType, ParaShapeIndex};
use hwpforge_smithy_hwp5::schema::header::{Hwp5RawStyle, Hwp5RawTabDef, Hwp5TabDefSlot};
use hwpforge_smithy_hwp5::style_store::Hwp5StyleStore;
use hwpforge_smithy_hwp5::{decode_hwp5_to_core, Hwp5Error, Hwp5Result, Hwp5Warning};
use hwpforge_smithy_hwpx::{HwpxEncoder, HwpxStyleStore};

use crate::style_store_border_fill::{push_hwp5_border_fills, push_required_border_fills};
use crate::style_store_convert::{
    hwp5_char_shape_to_hwpx_with_counts_and_warnings,
    hwp5_para_shape_to_hwpx_with_tab_id_and_warnings, hwp5_style_to_hwpx, hwp5_tab_def_to_hwpx,
    push_fonts, resolved_font_group_counts,
};
use crate::warning_utils::push_projection_fallback;

/// Converts an HWP5 file to HWPX format.
///
/// This is the primary convenience function for HWP5 → HWPX conversion.
/// Internally it decodes the HWP5 binary to the neutral Core IR, maps the
/// HWP5 style store onto HWPX styles, validates the document, and re-encodes
/// as HWPX.
///
/// # Examples
///
/// ```no_run
/// use hwpforge_convert::hwp5_to_hwpx;
///
/// let warnings = hwp5_to_hwpx("input.hwp", "output.hwpx").unwrap();
/// println!("Conversion complete with {} warnings", warnings.len());
/// ```
///
/// # Errors
///
/// Returns [`Hwp5Error`] if the input file cannot be read, decoded, or
/// the output file cannot be written.
pub fn hwp5_to_hwpx(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
) -> Hwp5Result<Vec<Hwp5Warning>> {
    let bytes = std::fs::read(input.as_ref()).map_err(Hwp5Error::Io)?;
    let (hwpx_bytes, warnings) = hwp5_to_hwpx_bytes(&bytes)?;
    std::fs::write(output.as_ref(), hwpx_bytes).map_err(Hwp5Error::Io)?;
    Ok(warnings)
}

/// Convert HWP5 bytes to HWPX bytes in memory.
///
/// In-memory variant of [`hwp5_to_hwpx`]. Useful for chaining conversions
/// (e.g. HWP5 -> HWPX -> Markdown) without touching the filesystem.
///
/// Returns the HWPX bytes alongside any non-fatal warnings encountered during
/// decoding, projection, and style mapping.
///
/// # Examples
///
/// ```no_run
/// use hwpforge_convert::hwp5_to_hwpx_bytes;
///
/// let hwp5_bytes = std::fs::read("input.hwp").unwrap();
/// let (hwpx_bytes, warnings) = hwp5_to_hwpx_bytes(&hwp5_bytes).unwrap();
/// println!("Produced {} bytes with {} warnings", hwpx_bytes.len(), warnings.len());
/// ```
///
/// # Errors
///
/// Returns [`Hwp5Error`] if the bytes cannot be decoded, the document fails
/// validation, or HWPX encoding fails.
pub fn hwp5_to_hwpx_bytes(bytes: &[u8]) -> Hwp5Result<(Vec<u8>, Vec<Hwp5Warning>)> {
    let decoded = decode_hwp5_to_core(bytes)?;
    let (hwpx_style_store, style_warnings) = hwp5_style_store_to_hwpx(&decoded.style_store);
    let mut warnings = decoded.warnings;
    warnings.extend(style_warnings);

    let validated = decoded.document.validate().map_err(Hwp5Error::Core)?;
    let hwpx_bytes = HwpxEncoder::encode(&validated, &hwpx_style_store, &decoded.image_store)
        .map_err(|e| Hwp5Error::Cfb { detail: format!("HWPX encoding failed: {e}") })?;
    let hwpx_bytes =
        layout_hint_patch::patch_hwpx_layout_hints(&hwpx_bytes, &decoded.layout_hints)?;

    Ok((hwpx_bytes, warnings))
}

/// Maps a format-neutral [`Hwp5StyleStore`] onto an [`HwpxStyleStore`].
///
/// This is a best-effort conversion. Fields that cannot be mapped cleanly use
/// defaults from the `"default"` preset, and unsupported style payloads surface
/// as explicit projection-fallback warnings instead of silent lies.
pub fn hwp5_style_store_to_hwpx(store: &Hwp5StyleStore) -> (HwpxStyleStore, Vec<Hwp5Warning>) {
    let mut out = HwpxStyleStore::new();
    let mut warnings: Vec<Hwp5Warning> = Vec::new();
    let border_fills = store.border_fills();
    if border_fills.is_empty() {
        push_required_border_fills(&mut out);
    } else {
        push_hwp5_border_fills(&mut out, border_fills, &mut warnings);
    }
    push_fonts(&mut out, store);
    let font_group_counts = resolved_font_group_counts(store);
    let tab_id_map = Hwp5TabIdMap::from_doc_info(&store.tab_defs);

    // Map character shapes.
    for (raw_id, raw) in store.char_shapes.iter().enumerate() {
        out.push_char_shape(hwp5_char_shape_to_hwpx_with_counts_and_warnings(
            raw,
            font_group_counts,
            raw_id,
            &mut warnings,
        ));
    }

    // Map numbering definitions before paragraph shapes so references are stable.
    append_numbering_definition_integrity_warning(store, &mut warnings);
    for slot in &store.numberings {
        match slot.numbering.as_ref() {
            Some(raw) => out.push_numbering(raw.to_core_numbering_def(slot.id)),
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
    append_bullet_definition_integrity_warning(store, &mut warnings);
    for slot in &store.bullets {
        match slot.bullet.as_ref() {
            Some(raw) => out.push_bullet(raw.to_core_bullet_def(slot.id)),
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
    for (raw_id, raw) in store.para_shapes.iter().enumerate() {
        let tab_pr_id_ref = tab_id_map.map_para_shape_ref(raw.tab_def_id, &mut warnings);
        out.push_para_shape(hwp5_para_shape_to_hwpx_with_tab_id_and_warnings(
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
    apply_outline_style_level_overrides(&mut out, &store.styles, &mut warnings);

    append_tab_definition_integrity_warning(store, &mut warnings);
    for slot in &store.tab_defs {
        match slot.tab_def.as_ref() {
            Some(raw) => {
                append_tab_projection_warnings(slot.raw_id, raw, &mut warnings);
                out.push_tab(hwp5_tab_def_to_hwpx(slot.raw_id, raw));
            }
            None => {
                warnings.push(Hwp5Warning::ParserFallback {
                    subject: "tab_def.slot",
                    reason: format!(
                        "tab definition slot {} failed to parse earlier; emitting empty placeholder to preserve raw ids",
                        slot.raw_id
                    ),
                });
                out.push_tab(empty_placeholder_tab_def(slot.raw_id));
            }
        }
    }

    for (idx, raw) in store.styles.iter().enumerate() {
        out.push_style(hwp5_style_to_hwpx(idx as u32, raw, store.styles.len()));
    }

    (out, warnings)
}

#[derive(Debug, Clone)]
struct Hwp5TabIdMap {
    known_slots: std::collections::BTreeSet<u32>,
}

impl Hwp5TabIdMap {
    fn from_doc_info(tab_defs: &[Hwp5TabDefSlot]) -> Self {
        let known_slots = tab_defs.iter().map(|slot| slot.raw_id).collect();
        Self { known_slots }
    }

    fn map_para_shape_ref(&self, raw_id: u16, warnings: &mut Vec<Hwp5Warning>) -> u32 {
        let raw_id = raw_id as u32;
        if hwpforge_core::TabDef::reference_is_known(raw_id, self.known_slots.iter().copied()) {
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

fn empty_placeholder_tab_def(id: u32) -> hwpforge_core::TabDef {
    hwpforge_core::TabDef { id, auto_tab_left: false, auto_tab_right: false, stops: Vec::new() }
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
    styles: &[Hwp5RawStyle],
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
        let Ok(para_shape) = store.para_shape_mut(ParaShapeIndex::new(para_shape_id)) else {
            continue;
        };
        // Codex §5: only override outline-kind paraPrs; never repurpose
        // Number/Bullet/None.
        if !matches!(para_shape.heading_type, HeadingType::Outline) {
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

#[cfg(test)]
mod style_store_tests;
#[cfg(test)]
mod tests;
