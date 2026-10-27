//! Preserve-first HWPX patching utilities.
//!
//! Unlike [`crate::HwpxEncoder`], this module is intentionally conservative:
//! it keeps untouched ZIP entries byte-for-byte and only patches section XML
//! when the semantic change is text-only and a stable preservation plan exists.

use std::collections::BTreeMap;
use std::ops::Range;

use sha2::{Digest, Sha256};

use hwpforge_core::caption::Caption;
use hwpforge_core::control::Control;
use hwpforge_core::document::Validated;
use hwpforge_core::image::Image;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::{HeaderFooter, Section};
use hwpforge_core::table::{Table, TableCell, TableRow};

use crate::error::{CodecFailure, CodecStage, HwpxError, HwpxResult};
use crate::exchange::{SectionPreservation, TextLocator, SECTION_PRESERVATION_VERSION};
use crate::inline_text::encode_inline_text_xml;
pub(crate) use crate::raw_package::RawPackage;
use crate::{HwpxDecoder, HwpxStyleStore};

/// Preserve-first patch engine for section-level edits.
///
/// This engine is intentionally narrower than full document regeneration:
/// it accepts only text-only section edits and refuses structural/style
/// mutations until a preserving structural patcher exists.
#[derive(Debug, Clone, Copy)]
pub struct HwpxPatcher;

/// Why a preserving section patch, or the preservation-metadata export it
/// relies on, failed.
///
/// The variant says which kind of failure it was, so a caller can tell a
/// request the preserving path refuses ([`PatchError::Rejected`]) from a
/// codec failure at a known stage ([`PatchError::Codec`]) or a broken
/// internal invariant.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PatchError {
    /// A codec step failed: decoding the base, validating the document or
    /// encoding the output. The stage says which; the message is the
    /// original error's.
    #[error("{}", .0.detail)]
    Codec(CodecFailure),
    /// The preserving path refuses the request: the preservation metadata
    /// is missing, stale or aimed at another section, or the replacement
    /// makes a change a text-only splice cannot express.
    #[error(transparent)]
    Rejected(HwpxError),
    /// The requested section does not exist in the base package.
    #[error("section {requested} out of range (document has {sections} sections)")]
    SectionOutOfRange {
        /// Requested section index.
        requested: usize,
        /// Number of sections in the base.
        sections: usize,
    },
    /// The replacement changes the value or the range of a field the patch
    /// may not edit.
    #[error("{detail}")]
    ProtectedFieldRange {
        /// What changed.
        detail: String,
    },
    /// An invariant the earlier checks established did not hold (a decoder
    /// source map or text locator was missing or inconsistent). This is a
    /// defect, not a property of the input.
    #[error("internal invariant violated: {detail}")]
    Invariant {
        /// What did not hold.
        detail: String,
    },
}

/// Wraps an HWPX error as a codec failure at `stage`; the stage literal stays
/// at the call site.
fn codec(stage: CodecStage) -> impl FnOnce(HwpxError) -> PatchError {
    move |error| PatchError::Codec(CodecFailure::hwpx(stage, &error))
}

/// Wraps a Core error met directly as a codec failure at `stage`.
fn core(stage: CodecStage) -> impl FnOnce(hwpforge_core::CoreError) -> PatchError {
    move |error| PatchError::Codec(CodecFailure::core(stage, &error))
}

/// The package path of section `section_idx`, after checking the index
/// against the package's section count (so the range failure is typed, not
/// read out of a message).
fn section_path(package: &RawPackage, section_idx: usize) -> Result<String, PatchError> {
    let sections = package.order.sections.len();
    if section_idx >= sections {
        return Err(PatchError::SectionOutOfRange { requested: section_idx, sections });
    }
    package.section_path(section_idx).map_err(codec(CodecStage::Decode))
}

impl HwpxPatcher {
    /// Builds preservation metadata for a section export.
    ///
    /// `to-json --section` uses this to embed stable text locators so a later
    /// `patch` call can modify only the touched `<hp:t>` payloads.
    ///
    /// # Errors
    ///
    /// - [`PatchError::Codec`] at [`CodecStage::Validation`] when `section`
    ///   nests deeper than the Core limit, and at [`CodecStage::Decode`] when
    ///   the base cannot be read or decoded. Every package entry is read and
    ///   counted, including ones the decoder ignores: an entry that
    ///   decompresses past 50 MiB, or a package past 500 MiB in total, is a
    ///   decode failure.
    /// - [`PatchError::SectionOutOfRange`] when the base has no such section.
    /// - [`PatchError::Rejected`] when `section` holds text at a place the
    ///   base has no text run for.
    /// - [`PatchError::Invariant`] when the decoder's source map is missing
    ///   or disagrees with `section`.
    pub fn export_section_preservation(
        base_bytes: &[u8],
        section_idx: usize,
        section: &Section,
    ) -> Result<SectionPreservation, PatchError> {
        section.check_nesting_depth().map_err(core(CodecStage::Validation))?;
        let raw_package = RawPackage::read(base_bytes).map_err(codec(CodecStage::Decode))?;
        let section_path = section_path(&raw_package, section_idx)?;
        let section_xml =
            raw_package.read_text_entry(&section_path).map_err(codec(CodecStage::Decode))?;
        let decoded = HwpxDecoder::decode(base_bytes).map_err(codec(CodecStage::Decode))?;
        let map = decoded
            .source_maps
            .get(section_idx)
            .ok_or_else(|| PatchError::Invariant { detail: "missing decoder source map".into() })?;
        preservation_from_map(&section_xml, &section_path, section, map, section_idx)
    }

    /// Patches a single section while preserving untouched package entries.
    ///
    /// The preserving path currently supports text-only edits. Any structural
    /// change (style changes, table geometry changes, control changes, etc.)
    /// returns [`PatchError::Rejected`].
    ///
    /// # Errors
    ///
    /// See [`HwpxPatcher::patch_section_preserving_with_codec_diagnostics`].
    pub fn patch_section_preserving(
        base_bytes: &[u8],
        section_idx: usize,
        replacement: &Section,
        styles: Option<&HwpxStyleStore>,
        preservation: Option<&SectionPreservation>,
    ) -> Result<Vec<u8>, PatchError> {
        Self::patch_section_preserving_with_diagnostics(
            base_bytes,
            section_idx,
            replacement,
            styles,
            preservation,
        )
        .map(crate::diagnostics::WithDecodeWarnings::into_value)
    }

    /// Patches a single section preservingly, keeping the decoder's warnings.
    ///
    /// Same bytes as [`HwpxPatcher::patch_section_preserving`], which is a
    /// thin wrapper over this function. It is in turn a thin wrapper over
    /// [`HwpxPatcher::patch_section_preserving_with_codec_diagnostics`] that
    /// drops the encode channel — the warnings for characters removed from
    /// the edited text. Use that twin to see them.
    ///
    /// # Errors
    ///
    /// See [`HwpxPatcher::patch_section_preserving_with_codec_diagnostics`].
    pub fn patch_section_preserving_with_diagnostics(
        base_bytes: &[u8],
        section_idx: usize,
        replacement: &Section,
        styles: Option<&HwpxStyleStore>,
        preservation: Option<&SectionPreservation>,
    ) -> Result<crate::diagnostics::WithDecodeWarnings<Vec<u8>>, PatchError> {
        Self::patch_section_preserving_with_codec_diagnostics(
            base_bytes,
            section_idx,
            replacement,
            styles,
            preservation,
        )
        .map(|diagnosed| {
            crate::diagnostics::WithDecodeWarnings::new(diagnosed.value, diagnosed.decode_warnings)
        })
    }

    /// Patches a single section preservingly, keeping both warning channels.
    ///
    /// Same bytes as [`HwpxPatcher::patch_section_preserving`]. The patch
    /// splices XML rather than re-encoding the package, so the decode
    /// warnings come from decoding the base to check the replacement against
    /// it, and the encode warnings describe only the edited text slots: a
    /// replacement text that holds characters XML 1.0 cannot represent (the
    /// C0 controls other than TAB, LF, CR and U+001F, plus U+FFFE/U+FFFF) is
    /// written without them, and each such slot adds one
    /// [`EncodeWarning::XmlForbiddenCharsRemoved`](crate::EncodeWarning::XmlForbiddenCharsRemoved)
    /// whose `location` is the slot path.
    ///
    /// Only slots whose text differs from the base are looked at. A slot
    /// handed back unchanged is kept byte for byte even when the base itself
    /// holds such characters, and a slot whose text equals the base once the
    /// characters are removed is kept too (with the warning). Changed candidates
    /// also use the shared body line-break encoding rule before comparison.
    ///
    /// # Errors
    ///
    /// The checks run in this order and the first failure is returned:
    ///
    /// 1. [`PatchError::Codec`] at [`CodecStage::Validation`] when the
    ///    replacement nests deeper than the Core limit;
    /// 2. [`PatchError::Rejected`] when the preservation metadata is missing;
    /// 3. [`PatchError::Codec`] at [`CodecStage::Decode`] when the base
    ///    package cannot be read. Every package entry is read and counted,
    ///    including ones the decoder ignores: an entry that decompresses past
    ///    50 MiB, or a package past 500 MiB in total, is a decode failure;
    /// 4. [`PatchError::SectionOutOfRange`] when the base has no such section;
    /// 5. [`PatchError::Rejected`] when the metadata is aimed at another
    ///    section or is of an unsupported version;
    /// 6. [`PatchError::Codec`] at [`CodecStage::Decode`] when the base cannot
    ///    be decoded;
    /// 7. [`PatchError::Codec`] at [`CodecStage::Validation`] when the base's
    ///    style measures fail output validation;
    /// 8. [`PatchError::Rejected`] for a style-store or master-page change;
    /// 9. [`PatchError::ProtectedFieldRange`] when the value or range of a
    ///    field the patch may not edit changes;
    /// 10. [`PatchError::Rejected`] for any other structural change;
    /// 11. [`PatchError::Codec`] at [`CodecStage::Validation`] when the
    ///     patched document fails validation;
    /// 12. [`PatchError::Rejected`] when the metadata is stale (hash, slots)
    ///     or an edited slot holds inline markup the patch cannot recreate;
    /// 13. [`PatchError::Codec`] at [`CodecStage::Encode`] when the output
    ///     cannot be written.
    ///
    /// [`PatchError::Invariant`] can come from any step after 6, when a
    /// decoder source map or text locator the earlier checks vouched for is
    /// missing or inconsistent.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxPatcher};
    ///
    /// let base = std::fs::read("form.hwpx")?;
    /// let section = HwpxDecoder::decode(&base)?.document.sections()[0].clone();
    /// let preservation = HwpxPatcher::export_section_preservation(&base, 0, &section)?;
    /// // `section` would normally carry the caller's text edits here.
    /// let diagnosed = HwpxPatcher::patch_section_preserving_with_codec_diagnostics(
    ///     &base,
    ///     0,
    ///     &section,
    ///     None,
    ///     Some(&preservation),
    /// )?;
    /// for warning in &diagnosed.encode_warnings {
    ///     eprintln!("{warning}");
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn patch_section_preserving_with_codec_diagnostics(
        base_bytes: &[u8],
        section_idx: usize,
        replacement: &Section,
        styles: Option<&HwpxStyleStore>,
        preservation: Option<&SectionPreservation>,
    ) -> Result<crate::diagnostics::WithCodecWarnings<Vec<u8>>, PatchError> {
        // 이것을 실패시키는 것: replacement.clone() 앞 관문 삭제.
        replacement.check_nesting_depth().map_err(core(CodecStage::Validation))?;
        let preservation = preservation.ok_or_else(|| {
            PatchError::Rejected(HwpxError::InvalidStructure {
                detail:
                    "missing preservation metadata; re-export the section with the current to-json command"
                        .into(),
            })
        })?;

        let mut raw_package = RawPackage::read(base_bytes).map_err(codec(CodecStage::Decode))?;
        let expected_section_path = section_path(&raw_package, section_idx)?;
        if preservation.section_path != expected_section_path {
            return Err(PatchError::Rejected(HwpxError::InvalidStructure {
                detail: format!(
                    "preservation metadata targets '{}' but patch requested '{}'",
                    preservation.section_path, expected_section_path
                ),
            }));
        }

        if preservation.preservation_version != SECTION_PRESERVATION_VERSION {
            return Err(PatchError::Rejected(HwpxError::InvalidStructure {
                detail: format!(
                    "preservation metadata version {} is unsupported; re-export the section with the current to-json command",
                    preservation.preservation_version
                ),
            }));
        }

        let mut decoded = HwpxDecoder::decode(base_bytes).map_err(codec(CodecStage::Decode))?;
        crate::paragraph_measure::validate_measure_output(&decoded.style_store)
            .map_err(codec(CodecStage::Validation))?;
        let normalize_measures = decoded.requires_header_normalization();
        let decode_warnings = decoded.warnings.clone();
        let mut replacement = replacement.clone();
        let mut authored = decoded.document.sections().to_vec();
        if let Some(section) = authored.get_mut(section_idx) {
            *section = replacement;
        }
        hwpforge_core::field_inventory::FieldInventory::restore_source_provenance(
            &mut authored,
            decoded.document.sections(),
        );
        let sections = decoded.document.sections().len();
        replacement = authored
            .get(section_idx)
            .cloned()
            .ok_or(PatchError::SectionOutOfRange { requested: section_idx, sections })?;
        let replacement = &replacement;
        let base_section = decoded
            .document
            .sections()
            .get(section_idx)
            .cloned()
            .ok_or(PatchError::SectionOutOfRange { requested: section_idx, sections })?;

        if let Some(patch_styles) = styles {
            if patch_styles != &decoded.style_store {
                return Err(PatchError::Rejected(HwpxError::InvalidStructure {
                    detail: "preserving patch does not support style-store changes yet".into(),
                }));
            }
        }

        if base_section.master_pages != replacement.master_pages {
            return Err(PatchError::Rejected(HwpxError::InvalidStructure {
                detail:
                    "preserving patch does not support master page edits yet; master pages live outside section XML"
                        .into(),
            }));
        }

        // 이것을 실패시키는 것: 경계형 자동 필드 본문을 일반 텍스트로 지우기.
        let before_fields = hwpforge_core::field_inventory::FieldInventory::from_sections_at(
            std::slice::from_ref(&base_section),
            section_idx,
        );
        let after_fields = hwpforge_core::field_inventory::FieldInventory::from_sections_at(
            std::slice::from_ref(replacement),
            section_idx,
        );
        if before_fields.fields().len() != after_fields.fields().len()
            || before_fields.fields().iter().zip(after_fields.fields()).any(|(a, b)| {
                a.editability() != hwpforge_core::field_inventory::FieldEditability::Editable
                    && a != b
            })
        {
            return Err(PatchError::ProtectedFieldRange {
                detail: "protected field value or range changed".into(),
            });
        }
        let mut base_redacted = base_section.clone();
        let mut replacement_redacted = replacement.clone();
        redact_section_texts(&mut base_redacted, section_idx);
        redact_section_texts(&mut replacement_redacted, section_idx);
        if base_redacted != replacement_redacted {
            return Err(PatchError::Rejected(HwpxError::InvalidStructure {
                detail:
                    "preserving patch currently supports text-only section edits; structural change detected"
                        .into(),
            }));
        }

        // Validate the semantic result before touching raw bytes.
        let sections = decoded.document.sections_mut();
        sections[section_idx] = replacement.clone();
        HwpxDecoder::verify_document_source(&decoded.document)
            .map_err(codec(CodecStage::Validation))?;
        let validated: hwpforge_core::document::Document<Validated> =
            decoded.document.validate().map_err(core(CodecStage::Validation))?;

        let base_section_xml = raw_package
            .read_text_entry(&expected_section_path)
            .map_err(codec(CodecStage::Decode))?;
        let base_hash = sha256_hex(base_section_xml.as_bytes());
        if preservation.section_sha256 != base_hash {
            return Err(PatchError::Rejected(HwpxError::InvalidStructure {
                detail: format!(
                    "preservation metadata hash mismatch for '{}': expected {} actual {}",
                    expected_section_path, preservation.section_sha256, base_hash
                ),
            }));
        }

        let rebuilt_preservation = preservation_from_map(
            &base_section_xml,
            &expected_section_path,
            &base_section,
            decoded.source_maps.get(section_idx).ok_or_else(|| PatchError::Invariant {
                detail: "missing decoder source map".into(),
            })?,
            section_idx,
        )?;
        if preservation != &rebuilt_preservation {
            return Err(PatchError::Rejected(HwpxError::InvalidStructure {
                detail:
                    "stale or tampered preservation metadata; re-export the section with the current to-json command"
                        .into(),
            }));
        }

        let replacement_slots = collect_preserving_text_slots(
            replacement,
            section_idx,
            &decoded.source_maps[section_idx],
        )?;
        if replacement_slots.len() != rebuilt_preservation.text_slots.len() {
            return Err(PatchError::Rejected(HwpxError::InvalidStructure {
                detail: format!(
                    "replacement semantic text-slot count mismatch: expected {} actual {}",
                    rebuilt_preservation.text_slots.len(),
                    replacement_slots.len()
                ),
            }));
        }

        for (index, (replacement_slot, preserved_slot)) in
            replacement_slots.iter().zip(&rebuilt_preservation.text_slots).enumerate()
        {
            if replacement_slot.path != preserved_slot.path {
                return Err(PatchError::Rejected(HwpxError::InvalidStructure {
                    detail: format!(
                        "replacement text-slot path mismatch at index {index}: expected '{}' actual '{}'",
                        preserved_slot.path, replacement_slot.path
                    ),
                }));
            }
        }

        // Decide every slot before splicing any: the caller's text is first
        // compared as given, so a slot handed back unchanged is never looked
        // at (the base may itself hold characters XML cannot carry); only a
        // real change candidate is cleaned and normalized to HWPX body line
        // breaks before comparing again, so an equivalent value is kept.
        let mut encode_warnings = Vec::new();
        let mut new_texts: Vec<Option<String>> = Vec::with_capacity(replacement_slots.len());
        for (replacement_slot, preserved_slot) in
            replacement_slots.iter().zip(&rebuilt_preservation.text_slots)
        {
            if replacement_slot.text == preserved_slot.original_text {
                new_texts.push(None);
                continue;
            }
            let text = crate::wire_xml::sanitize_edit_text(
                &replacement_slot.text,
                &expected_section_path,
                &preserved_slot.path,
                &mut encode_warnings,
            );
            let text = crate::wire_xml::body_text_line_breaks(&text, true).into_owned();
            new_texts.push((text != preserved_slot.original_text).then_some(text));
        }

        let mut edits: Vec<(&TextLocator, &str)> = Vec::new();
        for (new_text, preserved_slot) in new_texts.iter().zip(&rebuilt_preservation.text_slots) {
            let Some(new_text) = new_text else {
                continue;
            };

            if preserved_slot.has_inline_markup {
                // Use the decoder's element and range; never rediscover slot identity.
                let element = decoded.source_maps[section_idx]
                    .text_elements
                    .get(&text_locator_start(&preserved_slot.locator))
                    .ok_or_else(|| PatchError::Invariant {
                        detail: "missing decoder text element".into(),
                    })?;
                let inner = element.inner().ok_or_else(|| PatchError::Invariant {
                    detail: "missing decoder inline text content".into(),
                })?;
                if let Err(reason) = crate::inline_text::validate_recreatable_inline_markup(
                    element.qualified_name(),
                    &base_section_xml[inner],
                ) {
                    return Err(PatchError::Rejected(HwpxError::InvalidStructure {
                        detail: format!(
                            "preserving patch does not support editing text-slot '{}' with inline HWPX markup ({reason})",
                            preserved_slot.path
                        ),
                    }));
                }
            }

            edits.push((&preserved_slot.locator, new_text.as_str()));
        }

        // A changed text slot makes its paragraph's line
        // cache stale, and the caches of the later paragraphs of its flow and
        // of the host chain — the same set `fill` drops (`cache_flow`). Text
        // replacements and cache removals are written in one pass, which
        // refuses overlapping spans.
        // 이것을 실패시키는 것: 이 제거 삭제(`patch_drops_the_changed_flow_tail_and_host_chain`,
        // `a_shortened_patch_drops_the_stale_line_start`).
        let mut replacements = Vec::with_capacity(edits.len());
        if !edits.is_empty() {
            let flow = &decoded.source_maps[section_idx].cache_flow;
            let mut changed = Vec::with_capacity(edits.len());
            for (locator, text) in &edits {
                let at = text_locator_start(locator);
                changed.push(flow.paragraph_at(at).ok_or_else(|| PatchError::Invariant {
                    detail: format!(
                        "text slot at byte {at} of '{expected_section_path}' is outside every paragraph"
                    ),
                })?);
                // The locator and its element came from the decoder and passed
                // the source-map checks above, so a failure here is a defect.
                replacements.push(
                    text_locator_replacement(
                        &base_section_xml,
                        locator,
                        text,
                        &decoded.source_maps[section_idx].text_elements,
                    )
                    .map_err(|error| PatchError::Invariant { detail: error.to_string() })?,
                );
            }
            replacements
                .extend(flow.stale_caches(changed).into_iter().map(|range| (range, String::new())));
        }
        let patched_section_xml = crate::cache_flow::splice(&base_section_xml, replacements)
            .map_err(|error| PatchError::Invariant { detail: error.to_string() })?;

        // The text-only proof still applies, but an unnormalised header cannot
        // escape through raw ZIP copying. Reuse the ordinary output/cache planner.
        if normalize_measures
            || crate::header_references::HeaderReferences::build(&decoded.style_store)
                .map_err(codec(CodecStage::Encode))?
                .style_index(0)
                .is_err()
        {
            let outcome = crate::HwpxEncoder::encode_with_base(
                base_bytes,
                &validated,
                &decoded.style_store,
                &decoded.image_store,
                crate::EncodeOptions::default(),
            )
            .map_err(codec(CodecStage::Encode))?;
            encode_warnings.extend(outcome.warnings);
            return Ok(crate::diagnostics::WithCodecWarnings::new(
                outcome.bytes,
                decode_warnings,
                encode_warnings,
            ));
        }

        if patched_section_xml == base_section_xml {
            return Ok(crate::diagnostics::WithCodecWarnings::new(
                base_bytes.to_vec(),
                decode_warnings,
                encode_warnings,
            ));
        }

        raw_package.replace_text_entry(&expected_section_path, patched_section_xml);
        Ok(crate::diagnostics::WithCodecWarnings::new(
            raw_package.write().map_err(codec(CodecStage::Encode))?,
            decode_warnings,
            encode_warnings,
        ))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SemanticTextSlot {
    pub(crate) path: String,
    pub(crate) text: String,
    /// True when the payload is a tab-bearing `RunContent::InlineText`
    /// (plain-string projection). Patch editing downgrades such runs to
    /// `Text` on apply; E6 stamping excludes them instead — splitting a
    /// tab-bearing run is lossy (Wave 1A).
    pub(crate) inline: bool,
}

fn preservation_from_map(
    xml: &str,
    part: &str,
    section: &Section,
    map: &crate::decoder::source_map::SourceMap,
    section_idx: usize,
) -> Result<SectionPreservation, PatchError> {
    let recorded: BTreeMap<_, _> = map.text_slots().iter().map(|s| (s.path.as_str(), s)).collect();
    let mut slots = Vec::new();
    for slot in collect_preserving_text_slots(section, section_idx, map)? {
        let original = recorded.get(slot.path.as_str()).ok_or_else(|| PatchError::Invariant {
            detail: format!(
                "internal decoder source-map defect: no original slot for {}",
                slot.path
            ),
        })?;
        if original.original_text != slot.text {
            return Err(PatchError::Invariant {
                detail: format!(
                    "internal decoder source-map defect: original differs at {}",
                    slot.path
                ),
            });
        }
        slots.push((*original).clone());
    }
    Ok(SectionPreservation {
        preservation_version: SECTION_PRESERVATION_VERSION,
        section_path: part.into(),
        section_sha256: sha256_hex(xml.as_bytes()),
        text_slots: slots,
    })
}

// Both export and patch project only real wire text slots. Absence alone
// never authorizes skipping a slot: the decoder must identify a placeholder.
fn collect_preserving_text_slots(
    section: &Section,
    section_idx: usize,
    map: &crate::decoder::source_map::SourceMap,
) -> Result<Vec<SemanticTextSlot>, PatchError> {
    let mut slots = Vec::new();
    for slot in collect_semantic_text_slots(section, section_idx) {
        if map.synthetic_empty_texts.contains(&slot.path) {
            if !slot.text.is_empty() {
                return Err(PatchError::Rejected(HwpxError::InvalidStructure {
                    detail: format!(
                        "preserving patch cannot insert text at '{}': no original run; structural insertion is required",
                        slot.path
                    ),
                }));
            }
        } else {
            slots.push(slot);
        }
    }
    Ok(slots)
}

#[cfg(test)]
fn build_section_preservation(
    xml: &str,
    part: &str,
    section: &Section,
) -> Result<SectionPreservation, PatchError> {
    let parsed = crate::decoder::section::parse_section_with_images(
        xml,
        0,
        part,
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
    )
    .map_err(codec(CodecStage::Decode))?;
    let mut decoded =
        Section::with_paragraphs(parsed.paragraphs, hwpforge_core::PageSettings::a4());
    decoded.headers = parsed.headers;
    decoded.footers = parsed.footers;
    let map = parsed.source.finish(&decoded, 0, part);
    preservation_from_map(xml, part, section, &map, 0)
}

// pub(crate): E6 stamp plan 단계가 같은 커버리지로 placeholder 를 탐지한다
// (컨테이너 순회와 필드 정책은 Core의 FieldInventory가 소유한다).
pub(crate) fn collect_semantic_text_slots(
    section: &Section,
    section_idx: usize,
) -> Vec<SemanticTextSlot> {
    collect_text_slots(section, section_idx, true)
}

// Stamp policy must operate on actual text slots, including folded Memo
// anchors. Filtering them as patch targets first would hide policy regressions.
pub(crate) fn collect_stamp_text_slots(
    section: &Section,
    section_idx: usize,
) -> Vec<SemanticTextSlot> {
    collect_text_slots(section, section_idx, false)
}

fn text_runs(
    runs: &[Run],
    prefix: &str,
    section: usize,
    inventory: &hwpforge_core::field_inventory::FieldInventory<'_>,
    patchable_only: bool,
    slots: &mut Vec<SemanticTextSlot>,
) {
    let mut path = prefix.to_owned();
    let mut stack: Vec<_> =
        runs.iter().enumerate().rev().map(|(ri, run)| (run, ri, path.len())).collect();
    while let Some((run, ri, restore)) = stack.pop() {
        // 텍스트나 메모 앵커가 없는 run은 텍스트 슬롯 탐색에서 제외한다.
        if !has_text_or_anchor(run) {
            continue;
        }
        path.truncate(restore);
        use std::fmt::Write;
        write!(path, ".runs[{ri}]").expect("write to String");
        if let Some(text) = run.content.plain_text() {
            path.push_str(".text");
            if !patchable_only || !inventory.protects(section, &path) {
                slots.push(SemanticTextSlot {
                    path: path.clone(),
                    text: text.into_owned(),
                    inline: matches!(run.content, RunContent::InlineText(_)),
                });
            }
        }
        if let Some(Control::Memo { anchor_runs, .. }) = run.content.as_control() {
            path.push_str(".control.memo.anchor[0]");
            let restore = path.len();
            stack.extend(anchor_runs.iter().enumerate().rev().map(|(ri, run)| (run, ri, restore)));
        }
    }
}

fn has_text_or_anchor(run: &Run) -> bool {
    matches!(run.content, RunContent::Text(_) | RunContent::InlineText(_))
        || matches!(run.content.as_control(), Some(Control::Memo { .. }))
}

fn collect_text_slots(
    section: &Section,
    section_idx: usize,
    patchable_only: bool,
) -> Vec<SemanticTextSlot> {
    use hwpforge_core::field_inventory::{FieldEditability, FieldInventory};
    let sections = std::slice::from_ref(section);
    let inventory = FieldInventory::from_sections_at(sections, section_idx);
    let mut slots = Vec::new();
    FieldInventory::visit_flows(sections, |_, paragraphs, prefix| {
        // External master-page parts are not section patch slots.
        if prefix.starts_with("master_pages[") {
            return;
        }
        for (pi, paragraph) in paragraphs.iter().enumerate() {
            // No full paragraph path for a container-only flow.
            if !paragraph.runs.iter().any(has_text_or_anchor) {
                continue;
            }
            text_runs(
                &paragraph.runs,
                &format!("{prefix}[{pi}]"),
                section_idx,
                &inventory,
                patchable_only,
                &mut slots,
            );
        }
    });
    for field in inventory.fields().iter().filter(|f| {
        (!patchable_only || f.editability() == FieldEditability::Editable)
            && !f.current().is_empty()
    }) {
        for body in field.body().iter().filter(|b| b.path().ends_with(".control.field")) {
            slots.push(SemanticTextSlot {
                path: body.path().into(),
                text: field.current(),
                inline: false,
            });
        }
    }
    // Source and semantic projections use addresses, not enumeration position.
    slots.sort_by_cached_key(|s| {
        let priority = if s.path.starts_with("paragraphs[") {
            0
        } else if s.path.starts_with("header") {
            1
        } else {
            2
        };
        let mut path = String::new();
        for part in s.path.split_inclusive('[') {
            if let Some((number, rest)) = part.split_once(']') {
                if let Ok(number) = number.parse::<usize>() {
                    path.push_str(&format!("{number:020}]{rest}"));
                    continue;
                }
            }
            path.push_str(part);
        }
        (priority, path)
    });
    slots
}

/// Byte offset where the span a [`TextLocator`] replaces begins.
fn text_locator_start(locator: &TextLocator) -> usize {
    match locator {
        TextLocator::TextElement { element_start, .. } => *element_start,
        TextLocator::EmptyRun { run_start, .. } => *run_start,
    }
}

#[cfg(test)]
fn apply_text_locator(xml: &str, locator: &TextLocator, new_text: &str) -> HwpxResult<String> {
    let (_, source) = hwpforge_core::source_xml::XmlSource::read(xml).unwrap();
    let elements = source.elements().iter().map(|e| (e.outer().start, e.clone())).collect();
    let (range, replacement) = text_locator_replacement(xml, locator, new_text, &elements)?;
    let mut patched = xml.to_string();
    patched.replace_range(range, &replacement);
    Ok(patched)
}

/// The span of `xml` a [`TextLocator`] replaces and what goes there.
fn text_locator_replacement(
    xml: &str,
    locator: &TextLocator,
    new_text: &str,
    elements: &BTreeMap<usize, hwpforge_core::source_xml::XmlElement>,
) -> HwpxResult<(Range<usize>, String)> {
    let element = elements.get(&text_locator_start(locator)).ok_or_else(|| {
        HwpxError::InvalidStructure { detail: "missing decoder text element".into() }
    })?;
    let name = element.qualified_name();
    let prefix = name.split_once(':').map_or(String::new(), |(prefix, _)| format!("{prefix}:"));
    let encoded = encode_text_inner(new_text)?
        .replace("<hp:", &format!("<{prefix}"))
        .replace("</hp:", &format!("</{prefix}"));
    match locator {
        TextLocator::TextElement { content_start: Some(start), content_end: Some(end), .. } => {
            Ok((*start..*end, encoded))
        }
        TextLocator::TextElement { .. } | TextLocator::EmptyRun { .. } => {
            let encoded = if matches!(locator, TextLocator::EmptyRun { .. }) {
                format!("<{prefix}t>{encoded}</{prefix}t>")
            } else {
                encoded
            };
            if let Some(inner) = element.inner() {
                // Empty Core run, whose exact insertion point was recorded by
                // the decoder. Never search for tags in the original run.
                Ok((inner.end..inner.end, encoded))
            } else {
                let outer = element.outer();
                let opening = &xml[outer.start..outer.end - 2];
                Ok((outer, format!("{opening}>{encoded}</{name}>")))
            }
        }
    }
}

/// Encodes the text a patch splices into a section, refusing a fragment that
/// would leave the part malformed.
///
/// Callers clean the text first ([`crate::wire_xml::sanitize_edit_text`]), so
/// this check never fires on their path; it is the invariant that keeps a
/// future splice path which forgets to clean from writing a character XML 1.0
/// forbids (the inline escape writes them as they are). Only the new
/// fragment is checked — the rest of the section is the caller's input,
/// preserved as it was.
fn encode_text_inner(text: &str) -> HwpxResult<String> {
    let encoded = encode_inline_text_xml(text);
    if let Some(c) = encoded.chars().find(|&c| crate::wire_xml::is_xml_forbidden_char(c)) {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "patched text would write U+{:04X}, which XML 1.0 cannot represent",
                u32::from(c)
            ),
        });
    }
    Ok(encoded)
}

// pub(crate): E6 stamp manifest 가 source/output 해시에 재사용.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn redact_section_texts(section: &mut Section, section_idx: usize) {
    hwpforge_core::field_inventory::FieldInventory::clear_editable_values(
        std::slice::from_mut(section),
        section_idx,
    );
    redact_paragraphs(&mut section.paragraphs);
    // Redact every header/footer in the Vec (ADR-002 multi-cardinality).
    for header in section.headers.iter_mut() {
        redact_header_footer(header);
    }
    for footer in section.footers.iter_mut() {
        redact_header_footer(footer);
    }
}

fn redact_header_footer(value: &mut HeaderFooter) {
    redact_paragraphs(&mut value.paragraphs);
}

fn redact_caption(caption: &mut Caption) {
    redact_paragraphs(&mut caption.paragraphs);
}

fn redact_paragraphs(paragraphs: &mut [Paragraph]) {
    for paragraph in paragraphs {
        redact_runs(&mut paragraph.runs);
    }
}

fn redact_runs(runs: &mut [Run]) {
    for run in runs {
        match &mut run.content {
            RunContent::Text(text) => text.clear(),
            // Redaction wipes visible payload only. Downgrade
            // `InlineText` to an empty `Text(String)` — the tab
            // attributes are intentionally lost because keeping a
            // structured `<hp:tab>` next to redacted text would leak
            // structure that the redactor is trying to remove.
            RunContent::InlineText(_) => run.content = RunContent::Text(String::new()),
            RunContent::Table(table) => redact_table(table),
            RunContent::Image(image) => redact_image(image),
            RunContent::Control(control) => redact_control(control),
            _ => {}
        }
    }
}

fn redact_table(table: &mut Table) {
    if let Some(caption) = table.caption.as_mut() {
        redact_caption(caption);
    }
    for row in &mut table.rows {
        redact_row(row);
    }
}

fn redact_row(row: &mut TableRow) {
    for cell in &mut row.cells {
        redact_cell(cell);
    }
}

fn redact_cell(cell: &mut TableCell) {
    redact_paragraphs(&mut cell.paragraphs);
}

fn redact_image(image: &mut Image) {
    if let Some(caption) = image.caption.as_mut() {
        redact_caption(caption);
    }
}

fn redact_control(control: &mut Control) {
    match control {
        Control::TextBox { paragraphs, caption, .. } => {
            redact_paragraphs(paragraphs);
            if let Some(caption) = caption.as_mut() {
                redact_caption(caption);
            }
        }
        Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
            redact_paragraphs(paragraphs);
        }
        Control::Ellipse { paragraphs, caption, .. }
        | Control::Polygon { paragraphs, caption, .. } => {
            redact_paragraphs(paragraphs);
            if let Some(caption) = caption.as_mut() {
                redact_caption(caption);
            }
        }
        Control::Rect { caption, .. }
        | Control::Line { caption, .. }
        | Control::Arc { caption, .. }
        | Control::Curve { caption, .. }
        | Control::ConnectLine { caption, .. } => {
            if let Some(caption) = caption.as_mut() {
                redact_caption(caption);
            }
        }
        // ClickHere 본문은 편집 가능한 텍스트 슬롯이므로 redact 대상.
        // 다른 Field 타입(SUMMERY/DATE 류)은 지우지 않는다 — 슬롯이 없어
        // 변경이 무음 드롭되는 대신 structural change 로 거부되게 한다.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    mod small_stack {
        include!("../tests/support/small_stack.rs");
    }
    // 이것을 실패시키는 것: text_runs의 Memo anchor 재귀 복원 또는 leaf 누락.
    // Field policy is tested separately at depth 33; this test isolates slot traversal.
    #[test]
    fn deep_anchor_slots_small_stack() {
        small_stack::small_stack_child("patch::tests::deep_anchor_slots_small_stack", || {
            let mut run = Run::text("@ ", hwpforge_foundation::CharShapeIndex::new(0));
            for _ in 0..10_000 {
                run = Run::control(
                    Control::memo_with_anchor(vec![], vec![run]),
                    hwpforge_foundation::CharShapeIndex::new(0),
                );
            }
            let mut slots = Vec::new();
            text_runs(
                &[run],
                "paragraphs[0]",
                0,
                &hwpforge_core::field_inventory::FieldInventory::default(),
                false,
                &mut slots,
            );
            assert_eq!(slots.len(), 1);
            assert_eq!(slots[0].text, "@ ");
            assert_eq!(slots[0].path.matches(".control.memo.anchor[0]").count(), 10_000);
        });
    }

    use super::*;
    use crate::decoder::section::parse_section;
    use std::collections::HashMap;

    // 이것을 실패시키는 것: 보존 슬롯 전처리 생략(원문 표식 손실), 또는
    // 전처리의 네임스페이스 오류 무시.
    #[test]
    fn source_marks_and_namespace_errors_in_preservation() {
        let xml =
            "<hs:sec><hp:p><hp:run><hp:t>&#xE000; <hp:fwSpace/>끝</hp:t></hp:run></hp:p></hs:sec>";
        let section = section_from_parsed(parse_section(xml, 0, &HashMap::new()).unwrap());
        let preservation =
            build_section_preservation(xml, "Contents/section0.xml", &section).unwrap();
        assert_eq!(preservation.text_slots[0].original_text, "\u{E000} \u{1f}끝");
        let xml = "<q:sec xmlns:q='urn:&unknown;'/>";
        let section = Section::with_paragraphs(Vec::new(), hwpforge_core::PageSettings::a4());
        let error = build_section_preservation(xml, "Contents/section2.xml", &section).unwrap_err();
        assert!(matches!(
            &error,
            PatchError::Codec(CodecFailure { stage: CodecStage::Decode, code, detail })
                if *code == crate::error::HwpxErrorCode::XmlParse
                    && detail.contains("Contents/section2.xml")
        ));
    }

    #[test]
    fn encode_text_inner_converts_inline_text_to_hpx_mixed_content() {
        assert_eq!(
            encode_text_inner("line 1\tline 2\nline 3\u{00A0}& < 4").unwrap(),
            "line 1<hp:tab/>line 2<hp:lineBreak/>line 3<hp:nbSpace/>&amp; &lt; 4"
        );
    }

    // The new-fragment invariant: a text that reaches the splice uncleaned
    // must not put a character XML 1.0 forbids (the inline escape writes it as
    // it is) into the section — NUL, another C0 control, U+FFFE or U+FFFF.
    // 이것을 실패시키는 것: `encode_text_inner` 의 금지 문자 검사를 빼는 것,
    // 또는 그 판정을 NUL 만(`c == '\0'`)으로 좁히는 것.
    #[test]
    fn a_fragment_holding_a_forbidden_character_is_refused_for_every_splice_shape() {
        let xml = "<hp:run><hp:t>old</hp:t></hp:run><hp:run/><hp:run><hp:t/></hp:run>";
        let content = TextLocator::TextElement {
            element_start: 8,
            element_end: 24,
            content_start: Some(14),
            content_end: Some(17),
        };
        let empty_run = TextLocator::EmptyRun { run_start: 33, run_end: 42 };
        let self_closing = TextLocator::TextElement {
            element_start: 50,
            element_end: 57,
            content_start: None,
            content_end: None,
        };

        for locator in [&content, &empty_run, &self_closing] {
            for (c, code) in [
                ('\u{0}', "U+0000"),
                ('\u{1}', "U+0001"),
                ('\u{FFFE}', "U+FFFE"),
                ('\u{FFFF}', "U+FFFF"),
            ] {
                let error = apply_text_locator(xml, locator, &format!("A{c}B")).unwrap_err();
                assert!(
                    matches!(&error, HwpxError::InvalidStructure { detail } if detail.contains(code)),
                    "{code} {locator:?}: {error:?}"
                );
            }
            assert_eq!(apply_text_locator(xml, locator, "AB").unwrap().matches("AB").count(), 1);
        }
    }

    /// 이것을 실패시키는 것: 빈 요소·run의 원본 태그 재검색 또는 hp 접두사 강제.
    #[test]
    fn patch_uses_recorded_empty_element_positions_and_namespaces() {
        for (xml, locator, expected) in [
            (
                "<para:t/>",
                TextLocator::TextElement {
                    element_start: 0,
                    element_end: 9,
                    content_start: None,
                    content_end: None,
                },
                "<para:t>hp:value<para:tab/>next</para:t>",
            ),
            (
                "<para:run/>",
                TextLocator::EmptyRun { run_start: 0, run_end: 11 },
                "<para:run><para:t>hp:value<para:tab/>next</para:t></para:run>",
            ),
            (
                "<para:run></para:run>",
                TextLocator::EmptyRun { run_start: 0, run_end: 21 },
                "<para:run><para:t>hp:value<para:tab/>next</para:t></para:run>",
            ),
        ] {
            assert_eq!(apply_text_locator(xml, &locator, "hp:value\tnext").unwrap(), expected);
        }
    }

    #[test]
    fn patch_text_element_expands_self_closing_tag() {
        let patched = apply_text_locator(
            r#"<hp:t/>"#,
            &TextLocator::TextElement {
                element_start: 0,
                element_end: 7,
                content_start: None,
                content_end: None,
            },
            "filled",
        )
        .unwrap();
        assert_eq!(patched, "<hp:t>filled</hp:t>");
    }

    #[test]
    fn patch_text_run_inserts_text_into_empty_run() {
        let patched = apply_text_locator(
            r#"<hp:run charPrIDRef="0"/>"#,
            &TextLocator::EmptyRun { run_start: 0, run_end: 23 },
            "value",
        )
        .unwrap();
        assert_eq!(patched, r#"<hp:run charPrIDRef="0"><hp:t>value</hp:t></hp:run>"#);
    }

    #[test]
    fn build_section_preservation_for_simple_table_cell() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:tbl rowCnt="1" colCnt="1" cellSpacing="0" borderFillIDRef="1">
                <hp:tr>
                  <hp:tc borderFillIDRef="1">
                    <hp:subList vertAlign="TOP">
                      <hp:p id="1" paraPrIDRef="0">
                        <hp:run charPrIDRef="0"><hp:t>cell</hp:t></hp:run>
                      </hp:p>
                    </hp:subList>
                  </hp:tc>
                </hp:tr>
              </hp:tbl>
            </hp:run>
          </hp:p>
        </hs:sec>
        "#;

        let parsed = parse_section(xml, 0, &HashMap::new()).unwrap();
        let section = Section {
            paragraphs: parsed.paragraphs,
            page_settings: parsed.page_settings.unwrap_or_default(),
            headers: parsed.headers,
            footers: parsed.footers,
            page_number: parsed.page_number,
            column_settings: parsed.column_settings,
            visibility: parsed.visibility,
            line_number_shape: parsed.line_number_shape,
            page_border_fills: parsed.page_border_fills,
            master_pages: None,
            begin_num: parsed.begin_num,
            text_direction: parsed.text_direction,
        };

        let preservation =
            build_section_preservation(xml, "Contents/section0.xml", &section).unwrap();
        assert_eq!(preservation.preservation_version, SECTION_PRESERVATION_VERSION);
        assert_eq!(preservation.text_slots.len(), 1);
        assert_eq!(
            preservation.text_slots[0].path,
            "paragraphs[0].runs[0].table.rows[0].cells[0].paragraphs[0].runs[0].text"
        );
        assert_eq!(preservation.text_slots[0].original_text, "cell");
    }

    fn section_from_parsed(parsed: crate::decoder::section::SectionParseResult) -> Section {
        Section {
            paragraphs: parsed.paragraphs,
            page_settings: parsed.page_settings.unwrap_or_default(),
            headers: parsed.headers,
            footers: parsed.footers,
            page_number: parsed.page_number,
            column_settings: parsed.column_settings,
            visibility: parsed.visibility,
            line_number_shape: parsed.line_number_shape,
            page_border_fills: parsed.page_border_fills,
            master_pages: None,
            begin_num: parsed.begin_num,
            text_direction: parsed.text_direction,
        }
    }

    const NESTED_FOOTER_TABLE: &str = r#"
              <hp:tbl rowCnt="1" colCnt="1" cellSpacing="0" borderFillIDRef="1">
                <hp:tr>
                  <hp:tc borderFillIDRef="1">
                    <hp:subList vertAlign="TOP">
                      <hp:p id="1" paraPrIDRef="0">
                        <hp:run charPrIDRef="0">
                          <hp:ctrl>
                            <hp:footer id="" applyPageType="BOTH">
                              <hp:subList vertAlign="BOTTOM">
                                <hp:p id="2" paraPrIDRef="0">
                                  <hp:run charPrIDRef="0"><hp:t>nested foot</hp:t></hp:run>
                                </hp:p>
                              </hp:subList>
                            </hp:footer>
                          </hp:ctrl>
                        </hp:run>
                        <hp:run charPrIDRef="0"><hp:t>body</hp:t></hp:run>
                      </hp:p>
                    </hp:subList>
                  </hp:tc>
                </hp:tr>
              </hp:tbl>"#;

    // 계약: 칸 안 꼬리말은 구역 꼬리말로 올라가고(옛 계약
    // "구역 꼬리말이 아니다" 를 뒤집음), raw 슬롯도 같은 자리에서 센다.
    // 이것을 실패시키는 것: raw 표 칸 슬롯에 `false` 를 넘기기(슬롯 수 불일치).
    #[test]
    fn build_section_preservation_lifts_nested_footer_controls() {
        let xml = format!(
            r#"<hs:sec><hp:p id="0" paraPrIDRef="0"><hp:run charPrIDRef="0">{NESTED_FOOTER_TABLE}</hp:run></hp:p></hs:sec>"#
        );
        let parsed = parse_section(&xml, 0, &HashMap::new()).unwrap();
        assert_eq!(parsed.footers.len(), 1, "nested footer becomes a section footer");
        assert_eq!(parsed.footers[0].anchor_paragraph, None);
        let section = section_from_parsed(parsed);

        let preservation =
            build_section_preservation(&xml, "Contents/section0.xml", &section).unwrap();
        let paths: Vec<&str> = preservation.text_slots.iter().map(|s| s.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "paragraphs[0].runs[0].table.rows[0].cells[0].paragraphs[0].runs[0].text",
                "footer.paragraphs[0].runs[0].text",
            ]
        );
        assert_eq!(preservation.text_slots[1].original_text, "nested foot");
    }

    // 글상자 안 머리말도 디코더처럼 슬롯을 센다. 이것을 실패시키는 것:
    // rect 의 drawText 슬롯에 `false` 를 넘기기(슬롯 수 불일치).
    #[test]
    fn build_section_preservation_lifts_header_in_text_box() {
        let xml = r#"<hs:sec><hp:p id="0" paraPrIDRef="0"><hp:run charPrIDRef="0">
            <hp:rect id="1"><hp:sz width="1000" height="1000"/><hp:pos treatAsChar="1"/>
              <hp:drawText><hp:subList><hp:p id="1" paraPrIDRef="0"><hp:run charPrIDRef="0">
                <hp:ctrl><hp:header id="" applyPageType="BOTH"><hp:subList vertAlign="TOP">
                  <hp:p id="2" paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:t>boxed head</hp:t></hp:run></hp:p>
                </hp:subList></hp:header></hp:ctrl><hp:t>box</hp:t></hp:run></hp:p></hp:subList></hp:drawText>
            </hp:rect></hp:run></hp:p></hs:sec>"#;
        let parsed = parse_section(xml, 0, &HashMap::new()).unwrap();
        assert_eq!(parsed.headers.len(), 1);
        let section = section_from_parsed(parsed);
        let preservation =
            build_section_preservation(xml, "Contents/section0.xml", &section).unwrap();
        assert!(
            preservation.text_slots.iter().any(|s| s.path == "header.paragraphs[0].runs[0].text"),
            "{:?}",
            preservation.text_slots.iter().map(|s| &s.path).collect::<Vec<_>>()
        );
    }

    // 최상위 꼬리말 + 뒤 문단 칸 안 꼬리말 = 둘 → semantic 쪽 `footers[i]`
    // 와 같은 이름. 이것을 실패시키는 것: `numbered_header_footer_slots` 가
    // 여럿일 때도 `footer.` 로 두기(경로 불일치 오류).
    #[test]
    fn preservation_slots_number_multiple_footers() {
        let xml = format!(
            r#"<hs:sec>
          <hp:p id="0" paraPrIDRef="0"><hp:run charPrIDRef="0">
            <hp:ctrl><hp:footer id="" applyPageType="BOTH"><hp:subList vertAlign="BOTTOM">
              <hp:p id="3" paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:t>top foot</hp:t></hp:run></hp:p>
            </hp:subList></hp:footer></hp:ctrl>
            <hp:t>first</hp:t></hp:run></hp:p>
          <hp:p id="4" paraPrIDRef="0"><hp:run charPrIDRef="0">{NESTED_FOOTER_TABLE}</hp:run></hp:p>
        </hs:sec>"#
        );
        let parsed = parse_section(&xml, 0, &HashMap::new()).unwrap();
        let anchors: Vec<Option<usize>> =
            parsed.footers.iter().map(|f| f.anchor_paragraph).collect();
        assert_eq!(anchors, [None, Some(1)]);
        let section = section_from_parsed(parsed);
        let preservation =
            build_section_preservation(&xml, "Contents/section0.xml", &section).unwrap();
        let paths: Vec<&str> = preservation.text_slots.iter().map(|s| s.path.as_str()).collect();
        assert!(paths.contains(&"footers[0].paragraphs[0].runs[0].text"), "{paths:?}");
        assert!(paths.contains(&"footers[1].paragraphs[0].runs[0].text"), "{paths:?}");
    }

    #[test]
    fn build_section_preservation_uses_single_textbox_prefix() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0">
            <hp:run charPrIDRef="3">
              <hp:rect id="" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM"
                       textFlow="BOTH_SIDES" lock="0" dropcapstyle="None"
                       href="" groupLevel="0" instid="12345" ratio="0">
                <hp:sz width="14000" height="8000" widthRelTo="ABSOLUTE" heightRelTo="ABSOLUTE" protect="0"/>
                <hp:pos treatAsChar="1" affectLSpacing="0" flowWithText="0" allowOverlap="0"
                        holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="PARA"
                        vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/>
                <hp:drawText lastWidth="13434" name="" editable="0">
                  <hp:subList id="0" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP"
                              linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0">
                    <hp:p id="1" paraPrIDRef="0">
                      <hp:run charPrIDRef="0"><hp:t>Box content</hp:t></hp:run>
                    </hp:p>
                  </hp:subList>
                </hp:drawText>
              </hp:rect>
            </hp:run>
          </hp:p>
        </hs:sec>
        "#;

        let parsed = parse_section(xml, 0, &HashMap::new()).unwrap();
        let section = Section {
            paragraphs: parsed.paragraphs,
            page_settings: parsed.page_settings.unwrap_or_default(),
            headers: parsed.headers,
            footers: parsed.footers,
            page_number: parsed.page_number,
            column_settings: parsed.column_settings,
            visibility: parsed.visibility,
            line_number_shape: parsed.line_number_shape,
            page_border_fills: parsed.page_border_fills,
            master_pages: None,
            begin_num: parsed.begin_num,
            text_direction: parsed.text_direction,
        };

        let preservation =
            build_section_preservation(xml, "Contents/section0.xml", &section).unwrap();
        assert_eq!(preservation.preservation_version, SECTION_PRESERVATION_VERSION);
        assert_eq!(preservation.text_slots.len(), 1);
        assert_eq!(
            preservation.text_slots[0].path,
            "paragraphs[0].runs[0].control.textbox.paragraphs[0].runs[0].text"
        );
    }

    // 이것을 실패시키는 것: 바뀐 slot 을 바이트 위치가 아니라 slot 순서의 역순으로
    // 적용 — 캡션(앞쪽 바이트)을 먼저 고쳐 내부 문단 slot 의 위치가 밀린다.
    // 한글이면 문자 경계 panic, ASCII 면 비정형 XML 을 성공으로 쓴다.
    #[test]
    fn edits_apply_in_byte_order_when_slot_order_differs() {
        use hwpforge_core::caption::{Caption, CaptionSide};
        use hwpforge_core::document::Document;
        use hwpforge_core::image::ImageStore;
        use hwpforge_core::run::{Run, RunContent};
        use hwpforge_foundation::{CharShapeIndex, FieldType, HwpUnit, ParaShapeIndex};

        let field = |name: &str, current: &str| {
            let mut para = Paragraph::new(ParaShapeIndex::new(0));
            para.runs.push(Run::control(
                Control::Field {
                    field_type: FieldType::ClickHere,
                    hint_text: Some(current.to_string()),
                    help_text: None,
                    name: Some(name.to_string()),
                    display_text: current.to_string(),
                },
                CharShapeIndex::new(0),
            ));
            para
        };
        let size = |v: i32| HwpUnit::new(v).unwrap();
        let mut ellipse =
            Control::ellipse_with_text(size(6000), size(3000), vec![field("inner", "aaaa")]);
        if let Control::Ellipse { caption, .. } = &mut ellipse {
            *caption = Some(Caption::new(vec![field("cap", "bbbb")], CaptionSide::Bottom));
        }
        let mut body = Paragraph::new(ParaShapeIndex::new(0));
        body.runs.push(Run::control(ellipse, CharShapeIndex::new(0)));
        let mut doc = Document::new();
        let mut section = Section::new(hwpforge_core::PageSettings::default());
        section.paragraphs.push(body);
        doc.add_section(section);
        let styles = crate::style_store_for_preset("default").unwrap();
        let base =
            crate::HwpxEncoder::encode(&doc.validate().unwrap(), &styles, &ImageStore::default())
                .unwrap();

        let original = HwpxDecoder::decode(&base).unwrap().document.sections()[0].clone();
        let mut edited = original.clone();
        let set = |control: &mut Control, value: &str| {
            if let Control::Field { display_text, .. } = control {
                *display_text = value.to_string();
            }
        };
        let RunContent::Control(shape) = &mut edited.paragraphs[0].runs[0].content else {
            panic!("ellipse run");
        };
        let Control::Ellipse { paragraphs, caption: Some(caption), .. } = shape.as_mut() else {
            panic!("ellipse with caption");
        };
        for (paragraphs, value) in [(paragraphs, "X"), (&mut caption.paragraphs, "Y")] {
            let RunContent::Control(field) = &mut paragraphs[0].runs[0].content else {
                panic!("field run");
            };
            set(field, value);
        }

        let preservation = HwpxPatcher::export_section_preservation(&base, 0, &original).unwrap();
        let patched =
            HwpxPatcher::patch_section_preserving(&base, 0, &edited, None, Some(&preservation))
                .unwrap();
        let fields = crate::HwpxFiller::list_fields(&patched).unwrap();
        let values: Vec<(Option<String>, String)> =
            fields.into_iter().map(|f| (f.name, f.current)).collect();
        assert_eq!(
            values,
            vec![(Some("inner".into()), "X".into()), (Some("cap".into()), "Y".into())]
        );
    }
}

#[cfg(test)]
#[path = "../tests/support/field_path_contract.rs"]
mod field_path_contract;
