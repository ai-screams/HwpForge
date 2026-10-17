//! Preserve-first HWPX patching utilities.
//!
//! Unlike [`crate::HwpxEncoder`], this module is intentionally conservative:
//! it keeps untouched ZIP entries byte-for-byte and only patches section XML
//! when the semantic change is text-only and a stable preservation plan exists.

use std::collections::BTreeMap;
use std::io::{Cursor, Read as _, Write as _};
use std::ops::Range;

use crate::decoder::xml_from_str;
use quick_xml::events::Event;
use quick_xml::Reader;
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use hwpforge_core::caption::Caption;
use hwpforge_core::control::Control;
use hwpforge_core::document::Validated;
use hwpforge_core::image::Image;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::{HeaderFooter, Section};
use hwpforge_core::table::{Table, TableCell, TableRow};

use crate::decoder::package::{PackageReader, MAX_ENTRY_SIZE, MAX_TOTAL_SIZE};
use crate::error::{HwpxError, HwpxResult};
use crate::exchange::{
    PreservedTextSlot, SectionPreservation, TextLocator, SECTION_PRESERVATION_VERSION,
};
use crate::inline_text::encode_inline_text_xml;
use crate::schema::section::{
    HxCaption, HxCtrl, HxFieldBegin, HxFootNote, HxHeaderFooter, HxParagraph, HxPic, HxRun,
    HxSection, HxSubList, HxTable, HxTableCell, HxTableRow,
};
use crate::schema::shapes::{HxConnectLine, HxCurve, HxEllipse, HxLine, HxPolygon, HxRect};
use crate::{HwpxDecoder, HwpxStyleStore};

/// Preserve-first patch engine for section-level edits.
///
/// This engine is intentionally narrower than full document regeneration:
/// it accepts only text-only section edits and refuses structural/style
/// mutations until a preserving structural patcher exists.
#[derive(Debug, Clone, Copy)]
pub struct HwpxPatcher;

impl HwpxPatcher {
    /// Builds preservation metadata for a section export.
    ///
    /// `to-json --section` uses this to embed stable text locators so a later
    /// `patch` call can modify only the touched `<hp:t>` payloads.
    ///
    /// # Errors
    ///
    /// Every package entry is read and counted, including ones the decoder
    /// ignores: an entry that decompresses past 50 MiB, or a package past
    /// 500 MiB in total, is refused with [`HwpxError::InvalidStructure`].
    pub fn export_section_preservation(
        base_bytes: &[u8],
        section_idx: usize,
        section: &Section,
    ) -> HwpxResult<SectionPreservation> {
        let raw_package = RawPackage::read(base_bytes)?;
        let section_path = raw_package.section_path(section_idx)?;
        let section_xml = raw_package.read_text_entry(&section_path)?;
        build_section_preservation(&section_xml, &section_path, section)
    }

    /// Patches a single section while preserving untouched package entries.
    ///
    /// The preserving path currently supports text-only edits. Any structural
    /// change (style changes, table geometry changes, control changes, etc.)
    /// returns [`HwpxError::InvalidStructure`].
    ///
    /// # Errors
    ///
    /// Every package entry is read and counted, including ones the decoder
    /// ignores: an entry that decompresses past 50 MiB, or a package past
    /// 500 MiB in total, is refused with [`HwpxError::InvalidStructure`].
    /// Beyond that, see [`HwpxPatcher::patch_section_preserving_with_diagnostics`].
    pub fn patch_section_preserving(
        base_bytes: &[u8],
        section_idx: usize,
        replacement: &Section,
        styles: Option<&HwpxStyleStore>,
        preservation: Option<&SectionPreservation>,
    ) -> HwpxResult<Vec<u8>> {
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
    ) -> HwpxResult<crate::diagnostics::WithDecodeWarnings<Vec<u8>>> {
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
    /// characters are removed is kept too (with the warning).
    ///
    /// # Errors
    ///
    /// [`HwpxError::InvalidStructure`] when the preservation metadata is
    /// missing, stale or aimed at another section, or when the replacement
    /// makes a change the preserving path cannot express; decode failures
    /// surface as themselves.
    ///
    /// Every package entry is read and counted, including ones the decoder
    /// ignores: an entry that decompresses past 50 MiB, or a package past
    /// 500 MiB in total, is refused with [`HwpxError::InvalidStructure`].
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
    ) -> HwpxResult<crate::diagnostics::WithCodecWarnings<Vec<u8>>> {
        let preservation = preservation.ok_or_else(|| HwpxError::InvalidStructure {
            detail:
                "missing preservation metadata; re-export the section with the current to-json command"
                    .into(),
        })?;

        let mut raw_package = RawPackage::read(base_bytes)?;
        let expected_section_path = raw_package.section_path(section_idx)?;
        if preservation.section_path != expected_section_path {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "preservation metadata targets '{}' but patch requested '{}'",
                    preservation.section_path, expected_section_path
                ),
            });
        }

        if preservation.preservation_version != SECTION_PRESERVATION_VERSION {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "preservation metadata version {} is unsupported; re-export the section with the current to-json command",
                    preservation.preservation_version
                ),
            });
        }

        let mut decoded = HwpxDecoder::decode(base_bytes)?;
        let decode_warnings = decoded.warnings.clone();
        let base_section =
            decoded.document.sections().get(section_idx).cloned().ok_or_else(|| {
                HwpxError::InvalidStructure {
                    detail: format!(
                        "section {section_idx} out of range (document has {} sections)",
                        decoded.document.sections().len()
                    ),
                }
            })?;

        if let Some(patch_styles) = styles {
            if patch_styles != &decoded.style_store {
                return Err(HwpxError::InvalidStructure {
                    detail: "preserving patch does not support style-store changes yet".into(),
                });
            }
        }

        if base_section.master_pages != replacement.master_pages {
            return Err(HwpxError::InvalidStructure {
                detail:
                    "preserving patch does not support master page edits yet; master pages live outside section XML"
                        .into(),
            });
        }

        let mut base_redacted = base_section.clone();
        let mut replacement_redacted = replacement.clone();
        redact_section_texts(&mut base_redacted);
        redact_section_texts(&mut replacement_redacted);
        if base_redacted != replacement_redacted {
            return Err(HwpxError::InvalidStructure {
                detail:
                    "preserving patch currently supports text-only section edits; structural change detected"
                        .into(),
            });
        }

        // Validate the semantic result before touching raw bytes.
        let sections = decoded.document.sections_mut();
        sections[section_idx] = replacement.clone();
        let _: hwpforge_core::document::Document<Validated> = decoded.document.validate()?;

        let base_section_xml = raw_package.read_text_entry(&expected_section_path)?;
        let base_hash = sha256_hex(base_section_xml.as_bytes());
        if preservation.section_sha256 != base_hash {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "preservation metadata hash mismatch for '{}': expected {} actual {}",
                    expected_section_path, preservation.section_sha256, base_hash
                ),
            });
        }

        let rebuilt_preservation =
            build_section_preservation(&base_section_xml, &expected_section_path, &base_section)?;
        if preservation != &rebuilt_preservation {
            return Err(HwpxError::InvalidStructure {
                detail:
                    "stale or tampered preservation metadata; re-export the section with the current to-json command"
                        .into(),
            });
        }

        let replacement_slots = collect_semantic_text_slots(replacement);
        if replacement_slots.len() != rebuilt_preservation.text_slots.len() {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "replacement semantic text-slot count mismatch: expected {} actual {}",
                    rebuilt_preservation.text_slots.len(),
                    replacement_slots.len()
                ),
            });
        }

        for (index, (replacement_slot, preserved_slot)) in
            replacement_slots.iter().zip(&rebuilt_preservation.text_slots).enumerate()
        {
            if replacement_slot.path != preserved_slot.path {
                return Err(HwpxError::InvalidStructure {
                    detail: format!(
                        "replacement text-slot path mismatch at index {index}: expected '{}' actual '{}'",
                        preserved_slot.path, replacement_slot.path
                    ),
                });
            }
        }

        // Decide every slot before splicing any: the caller's text is first
        // compared as given, so a slot handed back unchanged is never looked
        // at (the base may itself hold characters XML cannot carry); only a
        // real change candidate is cleaned, and the cleaned text is compared
        // again so a value that only differed by those characters is kept.
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
            new_texts.push((text != preserved_slot.original_text).then_some(text));
        }

        let mut edits: Vec<(&TextLocator, &str)> = Vec::new();
        for (new_text, preserved_slot) in new_texts.iter().zip(&rebuilt_preservation.text_slots) {
            let Some(new_text) = new_text else {
                continue;
            };

            if preserved_slot.has_inline_markup {
                return Err(HwpxError::InvalidStructure {
                    detail: format!(
                        "preserving patch does not support editing text-slot '{}' with inline HWPX markup yet",
                        preserved_slot.path
                    ),
                });
            }

            edits.push((&preserved_slot.locator, new_text.as_str()));
        }

        // Branch review A3: a changed text slot makes its paragraph's line
        // cache stale, and the caches of the later paragraphs of its flow and
        // of the host chain — the same set `fill` drops (`cache_flow`). Text
        // replacements and cache removals are written in one pass, which
        // refuses overlapping spans.
        // 이것을 실패시키는 것: 이 제거 삭제(`patch_drops_the_changed_flow_tail_and_host_chain`,
        // `a_shortened_patch_drops_the_stale_line_start`).
        let mut replacements = Vec::with_capacity(edits.len());
        if !edits.is_empty() {
            let flow = crate::cache_flow::ParagraphFlow::scan(&base_section_xml)?;
            let mut changed = Vec::with_capacity(edits.len());
            for (locator, text) in &edits {
                let at = text_locator_start(locator);
                changed.push(flow.paragraph_at(at).ok_or_else(|| HwpxError::InvalidStructure {
                    detail: format!(
                        "text slot at byte {at} of '{expected_section_path}' is outside every paragraph"
                    ),
                })?);
                replacements.push(text_locator_replacement(&base_section_xml, locator, text)?);
            }
            replacements
                .extend(flow.stale_caches(changed).into_iter().map(|range| (range, String::new())));
        }
        let patched_section_xml = crate::cache_flow::splice(&base_section_xml, replacements)?;

        if patched_section_xml == base_section_xml {
            return Ok(crate::diagnostics::WithCodecWarnings::new(
                base_bytes.to_vec(),
                decode_warnings,
                encode_warnings,
            ));
        }

        raw_package.replace_text_entry(&expected_section_path, patched_section_xml);
        Ok(crate::diagnostics::WithCodecWarnings::new(
            raw_package.write()?,
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

#[derive(Debug, Default)]
struct RawSectionSlots {
    body: Vec<PreservedTextSlot>,
    /// One entry per `<hp:header>`, in wire order.
    header: Vec<Vec<PreservedTextSlot>>,
    /// One entry per `<hp:footer>`, in wire order.
    footer: Vec<Vec<PreservedTextSlot>>,
}

impl RawSectionSlots {
    fn into_slots(self) -> Vec<PreservedTextSlot> {
        let mut slots = self.body;
        slots.extend(numbered_header_footer_slots(self.header, "header", "headers"));
        slots.extend(numbered_header_footer_slots(self.footer, "footer", "footers"));
        slots
    }
}

/// Names header/footer slots the way the semantic side does
/// (`collect_semantic_text_slots`): `header.…` when the section has one,
/// `headers[i].…` when it has several.
///
/// 이것을 실패시키는 것: 여럿일 때도 `header.…` 로 두기
/// (`preservation_slots_number_multiple_footers`).
fn numbered_header_footer_slots(
    groups: Vec<Vec<PreservedTextSlot>>,
    single: &str,
    plural: &str,
) -> Vec<PreservedTextSlot> {
    let several = groups.len() > 1;
    let mut out = Vec::new();
    for (i, group) in groups.into_iter().enumerate() {
        for mut slot in group {
            if several {
                if let Some(rest) = slot.path.strip_prefix(single) {
                    slot.path = format!("{plural}[{i}]{rest}");
                }
            }
            out.push(slot);
        }
    }
    out
}

struct RawSlotSink<'a> {
    body_slots: &'a mut Vec<PreservedTextSlot>,
    section_slots: &'a mut RawSectionSlots,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TextElementSpan {
    element_start: usize,
    element_end: usize,
    content_start: Option<usize>,
    content_end: Option<usize>,
    has_inline_markup: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct RawPackage {
    pub(crate) order: crate::decoder::package_order::PackageOrder,
    entries: Vec<RawPackageEntry>,
    index_by_path: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
struct RawPackageEntry {
    path: String,
    bytes: Vec<u8>,
    compression: CompressionMethod,
}

impl RawPackage {
    /// Resolve a Core section index using the decoder's package reading order.
    /// 이것을 실패시키는 것: section index를 파일 번호로 사용하거나 범위 밖을 폴백하기.
    pub(crate) fn section_path(&self, section_idx: usize) -> HwpxResult<String> {
        self.order.sections.get(section_idx).cloned().ok_or_else(|| HwpxError::InvalidStructure {
            detail: format!(
                "section {section_idx} out of range (package has {} sections)",
                self.order.sections.len()
            ),
        })
    }

    pub(crate) fn read(bytes: &[u8]) -> HwpxResult<Self> {
        let reader = PackageReader::new(bytes)?;
        let mut package = Self::read_capped(bytes, MAX_ENTRY_SIZE, MAX_TOTAL_SIZE)?;
        package.order = reader.order;
        Ok(package)
    }

    /// Reads every entry, bounding decompression per entry and in total.
    ///
    /// The ZIP header size is attacker-controlled, so it is not used to size
    /// buffers at all: many entries declaring 50 MiB while holding a few
    /// bytes would otherwise reserve gigabytes that the cumulative budget
    /// (which counts real bytes) never sees. Buffers grow with the data, and
    /// the reader is limited with `take(max_entry + 1)` so a bomb stops one
    /// byte past the limit.
    fn read_capped(bytes: &[u8], max_entry: u64, max_total: u64) -> HwpxResult<Self> {
        let cursor = Cursor::new(bytes);
        let mut archive = ZipArchive::new(cursor).map_err(|e| HwpxError::Zip(e.to_string()))?;
        let mut entries: Vec<RawPackageEntry> = Vec::with_capacity(archive.len());
        let mut index_by_path: BTreeMap<String, usize> = BTreeMap::new();
        let mut total_read: u64 = 0;

        for index in 0..archive.len() {
            let mut file = archive.by_index(index).map_err(|e| HwpxError::Zip(e.to_string()))?;
            let path = file.name().to_string();
            let mut data: Vec<u8> = Vec::new();
            (&mut file)
                .take(max_entry + 1)
                .read_to_end(&mut data)
                .map_err(|e| HwpxError::Zip(format!("read '{path}': {e}")))?;
            if data.len() as u64 > max_entry {
                return Err(HwpxError::InvalidStructure {
                    detail: format!(
                        "entry '{path}' decompressed to {} bytes, exceeds limit of {max_entry}",
                        data.len(),
                    ),
                });
            }
            total_read += data.len() as u64;
            if total_read > max_total {
                return Err(HwpxError::InvalidStructure {
                    detail: format!(
                        "total decompressed data ({total_read} bytes) exceeds limit of {max_total}",
                    ),
                });
            }
            index_by_path.insert(path.clone(), entries.len());
            entries.push(RawPackageEntry { path, bytes: data, compression: file.compression() });
        }

        Ok(Self { entries, index_by_path, order: Default::default() })
    }

    pub(crate) fn entry_paths(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.path.as_str())
    }

    pub(crate) fn read_entry(&self, path: &str) -> HwpxResult<&[u8]> {
        self.index_by_path
            .get(path)
            .map(|&i| self.entries[i].bytes.as_slice())
            .ok_or_else(|| HwpxError::MissingFile { path: path.into() })
    }

    pub(crate) fn read_text_entry(&self, path: &str) -> HwpxResult<String> {
        let index = self
            .index_by_path
            .get(path)
            .copied()
            .ok_or_else(|| HwpxError::MissingFile { path: path.to_string() })?;
        String::from_utf8(self.entries[index].bytes.clone())
            .map_err(|e| HwpxError::Zip(format!("entry '{path}' is not valid UTF-8: {e}")))
    }

    pub(crate) fn replace_text_entry(&mut self, path: &str, content: String) {
        if let Some(index) = self.index_by_path.get(path).copied() {
            self.entries[index].bytes = content.into_bytes();
        }
    }

    pub(crate) fn write(&self) -> HwpxResult<Vec<u8>> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut zip = ZipWriter::new(cursor);

        for entry in &self.entries {
            // W0: fixed entry time (the ZIP epoch), never the clock.
            let options = SimpleFileOptions::default()
                .compression_method(entry.compression)
                .last_modified_time(zip::DateTime::default());
            zip.start_file(&entry.path, options).map_err(|e| HwpxError::Zip(e.to_string()))?;
            zip.write_all(&entry.bytes).map_err(|e| HwpxError::Zip(e.to_string()))?;
        }

        let cursor = zip.finish().map_err(|e| HwpxError::Zip(e.to_string()))?;
        Ok(cursor.into_inner())
    }
}

fn build_section_preservation(
    section_xml: &str,
    section_path: &str,
    section: &Section,
) -> HwpxResult<SectionPreservation> {
    // Parsed as the decoder parses it: a whitespace-only `<hp:t>` (the " "
    // after a caption number) keeps its text instead of reading as empty, so
    // the raw slots match the decoded runs. Element order is unchanged, so
    // the spans below still come from the original XML.
    // 이것을 실패시키는 것: 이 정규화를 지우기(F17 캡션의 " " 슬롯이 사라져
    // 보존 슬롯 수 불일치 — `caption_text_after_its_number_is_patched`).
    let normalized = crate::wire_xml::preserve_ws_only_text(section_xml).map_err(|error| {
        HwpxError::XmlParse {
            file: section_path.to_string(),
            detail: crate::decoder::xml_error_detail(&error.into()),
        }
    })?;
    let hx_section: HxSection = xml_from_str(&normalized).map_err(|error| HwpxError::XmlParse {
        file: section_path.to_string(),
        detail: crate::decoder::xml_error_detail(&error),
    })?;

    let raw_slots = collect_raw_text_slots(section_xml, &hx_section)?;
    let semantic_slots = collect_semantic_text_slots(section);
    if raw_slots.len() != semantic_slots.len() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "preservation slot count mismatch: raw={} semantic={}",
                raw_slots.len(),
                semantic_slots.len()
            ),
        });
    }

    let mut text_slots: Vec<PreservedTextSlot> = Vec::with_capacity(raw_slots.len());
    for (index, (raw_slot, semantic_slot)) in raw_slots.into_iter().zip(semantic_slots).enumerate()
    {
        if raw_slot.path != semantic_slot.path {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "preservation slot path mismatch at index {index}: raw='{}' semantic='{}'",
                    raw_slot.path, semantic_slot.path
                ),
            });
        }
        if raw_slot.original_text != semantic_slot.text {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "preservation slot text mismatch at index {index} path '{}': raw='{}' semantic='{}'",
                    raw_slot.path, raw_slot.original_text, semantic_slot.text
                ),
            });
        }
        text_slots.push(raw_slot);
    }

    Ok(SectionPreservation {
        preservation_version: SECTION_PRESERVATION_VERSION,
        section_path: section_path.to_string(),
        section_sha256: sha256_hex(section_xml.as_bytes()),
        text_slots,
    })
}

// pub(crate): E6 stamp plan 단계가 같은 커버리지로 placeholder 를 탐지한다
// (세 번째 순회 복제 금지 — fill/patch 거울 규칙과 동일 계열).
pub(crate) fn collect_semantic_text_slots(section: &Section) -> Vec<SemanticTextSlot> {
    let mut slots: Vec<SemanticTextSlot> = Vec::new();
    collect_semantic_paragraph_slots(&section.paragraphs, "paragraphs", &mut slots);
    // ADR-002: emit a slot per `<hp:header>` / `<hp:footer>` so multi-
    // cardinality page features (ODD/EVEN/BOTH) are addressable. Path
    // is indexed by position to keep stable identity across patches.
    for (idx, header) in section.headers.iter().enumerate() {
        let prefix = if section.headers.len() == 1 {
            "header".to_string()
        } else {
            format!("headers[{idx}]")
        };
        collect_semantic_header_footer_slots(header, &prefix, &mut slots);
    }
    for (idx, footer) in section.footers.iter().enumerate() {
        let prefix = if section.footers.len() == 1 {
            "footer".to_string()
        } else {
            format!("footers[{idx}]")
        };
        collect_semantic_header_footer_slots(footer, &prefix, &mut slots);
    }
    slots
}

fn collect_semantic_header_footer_slots(
    value: &HeaderFooter,
    prefix: &str,
    slots: &mut Vec<SemanticTextSlot>,
) {
    let paragraphs_prefix = format!("{prefix}.paragraphs");
    collect_semantic_paragraph_slots(&value.paragraphs, &paragraphs_prefix, slots);
}

fn collect_semantic_caption_slots(
    caption: &Caption,
    prefix: &str,
    slots: &mut Vec<SemanticTextSlot>,
) {
    let paragraphs_prefix = format!("{prefix}.caption.paragraphs");
    collect_semantic_paragraph_slots(&caption.paragraphs, &paragraphs_prefix, slots);
}

fn collect_semantic_paragraph_slots(
    paragraphs: &[Paragraph],
    prefix: &str,
    slots: &mut Vec<SemanticTextSlot>,
) {
    for (paragraph_idx, paragraph) in paragraphs.iter().enumerate() {
        let paragraph_prefix = format!("{prefix}[{paragraph_idx}]");
        for (run_idx, run) in paragraph.runs.iter().enumerate() {
            let run_prefix = format!("{paragraph_prefix}.runs[{run_idx}]");
            match &run.content {
                RunContent::Text(text) => slots.push(SemanticTextSlot {
                    path: format!("{run_prefix}.text"),
                    text: text.clone(),
                    inline: false,
                }),
                // `RunContent::InlineText` carries `<hp:tab>`
                // attributes that the slot model cannot represent;
                // expose the tab-equivalent plain string so the slot
                // is still discoverable and editable. Editing through
                // the slot interface downgrades the run to plain
                // `Text(String)` on apply — see the
                // `apply_*_slot` helpers and debug doc §3a-C16.
                RunContent::InlineText(it) => slots.push(SemanticTextSlot {
                    path: format!("{run_prefix}.text"),
                    text: it.plain_text(),
                    inline: true,
                }),
                RunContent::Table(table) => {
                    collect_semantic_table_slots(table, &format!("{run_prefix}.table"), slots);
                }
                RunContent::Image(image) => {
                    collect_semantic_image_slots(image, &format!("{run_prefix}.image"), slots);
                }
                RunContent::Control(control) => {
                    collect_semantic_control_slots(
                        control,
                        &format!("{run_prefix}.control"),
                        slots,
                    );
                }
                _ => {}
            }
        }
    }
}

fn collect_semantic_table_slots(table: &Table, prefix: &str, slots: &mut Vec<SemanticTextSlot>) {
    if let Some(caption) = &table.caption {
        collect_semantic_caption_slots(caption, prefix, slots);
    }
    for (row_idx, row) in table.rows.iter().enumerate() {
        collect_semantic_row_slots(row, &format!("{prefix}.rows[{row_idx}]"), slots);
    }
}

fn collect_semantic_row_slots(row: &TableRow, prefix: &str, slots: &mut Vec<SemanticTextSlot>) {
    for (cell_idx, cell) in row.cells.iter().enumerate() {
        collect_semantic_cell_slots(cell, &format!("{prefix}.cells[{cell_idx}]"), slots);
    }
}

fn collect_semantic_cell_slots(cell: &TableCell, prefix: &str, slots: &mut Vec<SemanticTextSlot>) {
    let paragraphs_prefix = format!("{prefix}.paragraphs");
    collect_semantic_paragraph_slots(&cell.paragraphs, &paragraphs_prefix, slots);
}

fn collect_semantic_image_slots(image: &Image, prefix: &str, slots: &mut Vec<SemanticTextSlot>) {
    if let Some(caption) = &image.caption {
        collect_semantic_caption_slots(caption, prefix, slots);
    }
}

fn collect_semantic_control_slots(
    control: &Control,
    prefix: &str,
    slots: &mut Vec<SemanticTextSlot>,
) {
    match control {
        Control::TextBox { paragraphs, caption, .. } => {
            let paragraphs_prefix = format!("{prefix}.textbox.paragraphs");
            collect_semantic_paragraph_slots(paragraphs, &paragraphs_prefix, slots);
            if let Some(caption) = caption {
                collect_semantic_caption_slots(caption, &format!("{prefix}.textbox"), slots);
            }
        }
        Control::Footnote { paragraphs, .. } => {
            let paragraphs_prefix = format!("{prefix}.footnote.paragraphs");
            collect_semantic_paragraph_slots(paragraphs, &paragraphs_prefix, slots);
        }
        Control::Endnote { paragraphs, .. } => {
            let paragraphs_prefix = format!("{prefix}.endnote.paragraphs");
            collect_semantic_paragraph_slots(paragraphs, &paragraphs_prefix, slots);
        }
        Control::Ellipse { paragraphs, caption, .. } => {
            let paragraphs_prefix = format!("{prefix}.ellipse.paragraphs");
            collect_semantic_paragraph_slots(paragraphs, &paragraphs_prefix, slots);
            if let Some(caption) = caption {
                collect_semantic_caption_slots(caption, &format!("{prefix}.ellipse"), slots);
            }
        }
        Control::Polygon { paragraphs, caption, .. } => {
            let paragraphs_prefix = format!("{prefix}.polygon.paragraphs");
            collect_semantic_paragraph_slots(paragraphs, &paragraphs_prefix, slots);
            if let Some(caption) = caption {
                collect_semantic_caption_slots(caption, &format!("{prefix}.polygon"), slots);
            }
        }
        Control::Rect { caption: Some(caption), .. } => {
            collect_semantic_caption_slots(caption, &format!("{prefix}.rect"), slots);
        }
        Control::Line { caption: Some(caption), .. } => {
            collect_semantic_caption_slots(caption, &format!("{prefix}.line"), slots);
        }
        Control::Arc { caption: Some(caption), .. } => {
            collect_semantic_caption_slots(caption, &format!("{prefix}.arc"), slots);
        }
        Control::Curve { caption: Some(caption), .. } => {
            collect_semantic_caption_slots(caption, &format!("{prefix}.curve"), slots);
        }
        Control::ConnectLine { caption: Some(caption), .. } => {
            collect_semantic_caption_slots(caption, &format!("{prefix}.connect_line"), slots);
        }
        // 누름틀(ClickHere) 본문은 사용자가 채우는 값 — 텍스트 슬롯으로
        // 노출한다 (Epic 1). ClickHere 로만 게이트: SUMMERY/DATE 류의
        // display_text 는 한컴이 재계산하는 캐시 값이라 슬롯이 없고, redact
        // 도 지우지 않으므로 편집 시 structural change 로 거부된다 —
        // 슬롯 없이 redact 만 지우면 변경이 무음 드롭되는 부패 경로가 생긴다.
        Control::Field {
            field_type: hwpforge_foundation::FieldType::ClickHere,
            display_text,
            ..
        } => {
            if !display_text.is_empty() {
                slots.push(SemanticTextSlot {
                    path: format!("{prefix}.field"),
                    text: display_text.clone(),
                    inline: false,
                });
            }
        }
        _ => {}
    }
}

fn collect_raw_text_slots(xml: &str, section: &HxSection) -> HwpxResult<Vec<PreservedTextSlot>> {
    let root_span = find_root_span(xml, "hs:sec")?;
    let mut slots = RawSectionSlots::default();
    let mut body_slots: Vec<PreservedTextSlot> = Vec::new();
    let mut sink = RawSlotSink { body_slots: &mut body_slots, section_slots: &mut slots };
    collect_raw_paragraph_list_slots(
        xml,
        root_span,
        &section.paragraphs,
        "paragraphs",
        &mut sink,
        true,
    )?;
    slots.body = body_slots;
    Ok(slots.into_slots())
}

fn collect_raw_paragraph_list_slots(
    xml: &str,
    parent_span: Range<usize>,
    paragraphs: &[HxParagraph],
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
    allow_section_header_footer: bool,
) -> HwpxResult<()> {
    let paragraph_spans = collect_direct_child_outer_spans(xml, parent_span, "hp:p")?;
    if paragraph_spans.len() != paragraphs.len() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "paragraph count mismatch while building preservation metadata: raw={} semantic={}",
                paragraph_spans.len(),
                paragraphs.len()
            ),
        });
    }

    for (paragraph_idx, (paragraph, paragraph_span)) in
        paragraphs.iter().zip(paragraph_spans.into_iter()).enumerate()
    {
        let paragraph_prefix = format!("{prefix}[{paragraph_idx}]");
        collect_raw_paragraph_slots(
            xml,
            paragraph_span,
            paragraph,
            &paragraph_prefix,
            sink,
            allow_section_header_footer,
        )?;
    }

    Ok(())
}

fn collect_raw_paragraph_slots(
    xml: &str,
    paragraph_span: Range<usize>,
    paragraph: &HxParagraph,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
    allow_section_header_footer: bool,
) -> HwpxResult<()> {
    let run_spans = collect_direct_child_outer_spans(xml, paragraph_span.clone(), "hp:run")?;
    if run_spans.len() != paragraph.runs.len() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "run count mismatch while building preservation metadata for {prefix}: raw={} semantic={}",
                run_spans.len(),
                paragraph.runs.len()
            ),
        });
    }

    let mut semantic_run_idx: usize = 0;
    for (run, run_span) in paragraph.runs.iter().zip(run_spans.iter()) {
        semantic_run_idx = collect_raw_run_slots(
            xml,
            run_span.clone(),
            run,
            prefix,
            semantic_run_idx,
            sink,
            allow_section_header_footer,
        )?;
    }

    if semantic_run_idx == 0 {
        let Some(first_run_span) = run_spans.first() else {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "empty paragraph normalization is unsupported without a raw <hp:run> anchor at {prefix}"
                ),
            });
        };
        sink.body_slots.push(PreservedTextSlot {
            path: format!("{prefix}.runs[0].text"),
            original_text: String::new(),
            has_inline_markup: false,
            locator: TextLocator::EmptyRun {
                run_start: first_run_span.start,
                run_end: first_run_span.end,
            },
        });
    }

    Ok(())
}

fn collect_raw_run_slots(
    xml: &str,
    run_span: Range<usize>,
    run: &HxRun,
    prefix: &str,
    mut semantic_run_idx: usize,
    sink: &mut RawSlotSink<'_>,
    allow_section_header_footer: bool,
) -> HwpxResult<usize> {
    let text_locators = collect_direct_text_elements(xml, run_span.clone())?;
    if text_locators.len() != run.texts.len() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "text element count mismatch while building preservation metadata for {prefix}: raw={} semantic={}",
                text_locators.len(),
                run.texts.len()
            ),
        });
    }

    // W1a: 디코더가 자식을 문서 순서(child_order 사이드카)로 평탄화하므로
    // raw walker 도 같은 순서로 semantic_run_idx 를 전진시킨다 — 종전
    // by-kind 재현(texts→tables→pictures→ctrls→shapes)은 인터리브 run 에서
    // 경로 주소가 어긋난다. field 본문도 순서 기반: begin/end **사이**의
    // 텍스트가 본문 (gotcha #30 모호성 게이트 철폐 — 디코더와 거울).
    let kind_spans = |name: &str, expect: usize, label: &str| -> HwpxResult<Vec<Range<usize>>> {
        let spans = collect_direct_child_outer_spans(xml, run_span.clone(), name)?;
        if spans.len() != expect {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "{label} count mismatch while building preservation metadata for {prefix}: raw={} semantic={}",
                    spans.len(),
                    expect
                ),
            });
        }
        Ok(spans)
    };
    let table_spans = kind_spans("hp:tbl", run.tables.len(), "table")?;
    let picture_spans = kind_spans("hp:pic", run.pictures.len(), "picture")?;
    let ctrl_spans = kind_spans("hp:ctrl", run.ctrls.len(), "ctrl")?;
    let rect_spans = kind_spans("hp:rect", run.rects.len(), "rect")?;
    let line_spans = kind_spans("hp:line", run.lines.len(), "line")?;
    let ellipse_spans = kind_spans("hp:ellipse", run.ellipses.len(), "ellipse")?;
    let polygon_spans = kind_spans("hp:polygon", run.polygons.len(), "polygon")?;
    let curve_spans = kind_spans("hp:curve", run.curves.len(), "curve")?;
    let connect_line_spans = kind_spans("hp:connectLine", run.connect_lines.len(), "connectLine")?;

    let order = if run.child_order.is_empty() {
        crate::schema::section::legacy_child_order(run)
    } else {
        run.child_order.clone()
    };

    let mut pending_field_begin: Option<&HxFieldBegin> = None;
    // begin/end 사이 본문 텍스트 추적: (locator 인덱스, 사이 텍스트 개수).
    // 슬롯은 본문이 정확히 1개 `<hp:t>` 일 때만 만든다 (단일 요소 교체
    // locator 모델의 한계 — 0/2+ 개는 편집 슬롯 없음, 디코더 display_text
    // 는 이어붙여 carry).
    let mut field_body_locator: Option<usize> = None;
    let mut field_body_count = 0usize;
    let mut field_body_text = String::new();

    for &(kind, idx) in &order {
        use crate::schema::section::HxRunChildKind as K;
        match kind {
            K::Text => {
                if pending_field_begin.is_some() {
                    field_body_locator = Some(idx);
                    field_body_count += 1;
                    field_body_text.push_str(&run.texts[idx].text());
                    continue;
                }
                let text_content = run.texts[idx].text();
                if !text_content.is_empty() {
                    sink.body_slots.push(PreservedTextSlot {
                        path: format!("{prefix}.runs[{semantic_run_idx}].text"),
                        original_text: text_content,
                        has_inline_markup: text_locators[idx].has_inline_markup,
                        locator: text_locator(&text_locators[idx]),
                    });
                    semantic_run_idx += 1;
                }
            }
            K::Table => {
                let table_prefix = format!("{prefix}.runs[{semantic_run_idx}].table");
                collect_raw_table_slots(
                    xml,
                    table_spans[idx].clone(),
                    &run.tables[idx],
                    &table_prefix,
                    sink,
                    allow_section_header_footer,
                )?;
                semantic_run_idx += 1;
            }
            K::Picture => {
                if picture_has_semantic_run(&run.pictures[idx]) {
                    let picture_prefix = format!("{prefix}.runs[{semantic_run_idx}].image");
                    collect_raw_picture_slots(
                        xml,
                        picture_spans[idx].clone(),
                        &run.pictures[idx],
                        &picture_prefix,
                        sink,
                    )?;
                    semantic_run_idx += 1;
                }
            }
            K::Ctrl => {
                semantic_run_idx = collect_raw_ctrl_child_slots(
                    xml,
                    ctrl_spans[idx].clone(),
                    &run.ctrls[idx],
                    prefix,
                    semantic_run_idx,
                    sink,
                    allow_section_header_footer,
                    &mut pending_field_begin,
                    &mut field_body_locator,
                    &mut field_body_count,
                    &mut field_body_text,
                    &text_locators,
                )?;
            }
            K::Rect => {
                if run.rects[idx].draw_text.is_some() {
                    let rect_prefix = format!("{prefix}.runs[{semantic_run_idx}].control.textbox");
                    collect_raw_rect_slots(
                        xml,
                        rect_spans[idx].clone(),
                        &run.rects[idx],
                        &rect_prefix,
                        sink,
                        allow_section_header_footer,
                    )?;
                    semantic_run_idx += 1;
                }
            }
            K::Line => {
                let line_prefix = format!("{prefix}.runs[{semantic_run_idx}].control.line");
                collect_raw_line_slots(
                    xml,
                    line_spans[idx].clone(),
                    &run.lines[idx],
                    &line_prefix,
                    sink,
                )?;
                semantic_run_idx += 1;
            }
            K::Ellipse => {
                let ellipse = &run.ellipses[idx];
                let control_name = if ellipse.has_arc_pr == 1 { "arc" } else { "ellipse" };
                let ellipse_prefix =
                    format!("{prefix}.runs[{semantic_run_idx}].control.{control_name}");
                collect_raw_ellipse_slots(
                    xml,
                    ellipse_spans[idx].clone(),
                    ellipse,
                    control_name,
                    &ellipse_prefix,
                    sink,
                )?;
                semantic_run_idx += 1;
            }
            K::Polygon => {
                let polygon_prefix = format!("{prefix}.runs[{semantic_run_idx}].control.polygon");
                collect_raw_polygon_slots(
                    xml,
                    polygon_spans[idx].clone(),
                    &run.polygons[idx],
                    &polygon_prefix,
                    sink,
                )?;
                semantic_run_idx += 1;
            }
            K::Curve => {
                let curve_prefix = format!("{prefix}.runs[{semantic_run_idx}].control.curve");
                collect_raw_curve_slots(
                    xml,
                    curve_spans[idx].clone(),
                    &run.curves[idx],
                    &curve_prefix,
                    sink,
                )?;
                semantic_run_idx += 1;
            }
            K::ConnectLine => {
                let connect_line_prefix =
                    format!("{prefix}.runs[{semantic_run_idx}].control.connect_line");
                collect_raw_connect_line_slots(
                    xml,
                    connect_line_spans[idx].clone(),
                    &run.connect_lines[idx],
                    &connect_line_prefix,
                    sink,
                )?;
                semantic_run_idx += 1;
            }
            // 디코더가 문서 순서 위치에서 run 을 방출하는 슬롯-없는 종류 —
            // 인덱스만 전진 (equation/dutmal/compose + W1a 신규: container/
            // textart — 종전 walker 는 이 둘을 아예 세지 않던 잠복 결함).
            K::Equation | K::Dutmal | K::Compose | K::Container | K::TextArt => {
                semantic_run_idx += 1;
            }
            // Switch(차트) 는 디코더가 문단 끝에 append — run 꼬리 집계 유지.
            // secPr/titleMark/Other/StrayText 는 run 방출 없음.
            K::Switch | K::SecPr | K::TitleMark | K::Other | K::Ole | K::StrayText => {}
        }
    }
    // trailing self-closing fieldBegin (BOOKMARK span start) — 디코더 거울.
    if pending_field_begin.take().is_some_and(|fb| fb.field_type == "BOOKMARK") {
        semantic_run_idx += 1;
    }
    // Switch(차트): 디코더가 문단 끝에 append 하므로 종전대로 run 꼬리 집계.
    semantic_run_idx += run
        .switches
        .iter()
        .filter(|switch_case| {
            switch_case.case.as_ref().and_then(|case| case.chart.as_ref()).is_some()
        })
        .count();

    Ok(semantic_run_idx)
}

/// `<hp:ctrl>` 자식 하나의 raw 슬롯/인덱스 처리 (W1a — 디코더
/// `convert_ctrl_child` 와 거울상. field 본문은 순서 기반: begin/end 사이
/// 텍스트가 정확히 1개일 때만 편집 슬롯을 만든다).
#[allow(clippy::too_many_arguments)]
fn collect_raw_ctrl_child_slots<'a>(
    xml: &str,
    ctrl_span: Range<usize>,
    ctrl: &'a HxCtrl,
    prefix: &str,
    mut semantic_run_idx: usize,
    sink: &mut RawSlotSink<'_>,
    allow_section_header_footer: bool,
    pending_field_begin: &mut Option<&'a HxFieldBegin>,
    field_body_locator: &mut Option<usize>,
    field_body_count: &mut usize,
    field_body_text: &mut String,
    text_locators: &[TextElementSpan],
) -> HwpxResult<usize> {
    {
        if allow_section_header_footer {
            if let Some(header) = &ctrl.header {
                collect_raw_header_footer_slots(
                    xml,
                    ctrl_span.clone(),
                    header,
                    "header",
                    &mut sink.section_slots.header,
                )?;
            }
            if let Some(footer) = &ctrl.footer {
                collect_raw_header_footer_slots(
                    xml,
                    ctrl_span.clone(),
                    footer,
                    "footer",
                    &mut sink.section_slots.footer,
                )?;
            }
        }
        if let Some(footnote) = &ctrl.foot_note {
            let footnote_prefix = format!("{prefix}.runs[{semantic_run_idx}].control.footnote");
            collect_raw_footnote_slots(xml, ctrl_span.clone(), footnote, &footnote_prefix, sink)?;
            semantic_run_idx += 1;
        }
        if let Some(endnote) = &ctrl.end_note {
            let endnote_prefix = format!("{prefix}.runs[{semantic_run_idx}].control.endnote");
            collect_raw_footnote_slots(xml, ctrl_span.clone(), endnote, &endnote_prefix, sink)?;
            semantic_run_idx += 1;
        }
        if ctrl.bookmark.is_some() {
            semantic_run_idx += 1;
        }
        if ctrl.indexmark.is_some() {
            semantic_run_idx += 1;
        }
        if let Some(field_begin) = &ctrl.field_begin {
            *pending_field_begin = Some(field_begin);
            *field_body_locator = None;
            *field_body_count = 0;
            field_body_text.clear();
        }
        if ctrl.field_end.is_some() {
            if let Some(field_begin) = pending_field_begin.take() {
                // 누름틀(CLICK_HERE) 본문 = begin/end **사이** 텍스트 (W1a
                // 문서 순서 — 종전 `<hp:t> 1개` 모호성 게이트 철폐, 디코더와
                // 거울). 편집 슬롯은 단일 요소 교체 locator 모델이라 사이
                // 텍스트가 정확히 1개일 때만 만든다 (0/2+ 는 슬롯 없음 —
                // 디코더 display_text 는 이어붙여 carry).
                if field_begin.field_type == "CLICK_HERE" && *field_body_count == 1 {
                    let li = field_body_locator.expect("count==1 이면 locator 존재");
                    if !field_body_text.is_empty() {
                        sink.body_slots.push(PreservedTextSlot {
                            path: format!("{prefix}.runs[{semantic_run_idx}].control.field"),
                            original_text: field_body_text.clone(),
                            has_inline_markup: text_locators[li].has_inline_markup,
                            locator: text_locator(&text_locators[li]),
                        });
                    }
                }
                *field_body_locator = None;
                *field_body_count = 0;
                field_body_text.clear();
                semantic_run_idx += 1;
            }
        }
        if let Some(auto_num) = &ctrl.auto_num {
            // 디코더는 PAGE 와 TOTAL_PAGE 를 InlinePageNumber run 으로
            // 복원한다 (decoder/section.rs) — 카운터가 PAGE 만 세면 이후
            // 슬롯 인덱스가 밀린다 (W2 독립 설계리뷰에서 발견된 잠복 결함).
            // §19.16 J (review r1): a caption number the decoder turns into
            // a `Control::AutoNumber` run counts too — by the decoder's own
            // acceptance, so a format it refuses (no run) does not advance.
            // 이것을 실패시키는 것: 이 전진을 지우기(F17 캡션 번호 뒤 슬롯이 밀려
            // 보존 편집 실패 — `caption_text_after_its_number_is_patched`).
            let caption_number =
                matches!(crate::decoder::section::auto_number_control(auto_num), Ok(Some(_)));
            if auto_num.num_type == "PAGE" || auto_num.num_type == "TOTAL_PAGE" || caption_number {
                semantic_run_idx += 1;
            }
        }
        // W2: 디코더가 모든 `<hp:newNum>` 을 Control::NewNumber run 으로
        // 복원하므로 (미지 numType 도 Unknown kind 로 carry) 무조건 전진.
        if ctrl.new_num.is_some() {
            semantic_run_idx += 1;
        }
        // W3: `<hp:pageHiding>` 도 Control::PageHiding run 으로 복원 — 전진.
        if ctrl.page_hiding.is_some() {
            semantic_run_idx += 1;
        }
    }
    Ok(semantic_run_idx)
}

fn collect_raw_table_slots(
    xml: &str,
    table_span: Range<usize>,
    table: &HxTable,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
    lift_header_footer: bool,
) -> HwpxResult<()> {
    if let Some(caption) = &table.caption {
        let caption_span =
            single_optional_direct_outer_span(xml, table_span.clone(), "hp:caption")?.ok_or_else(
                || HwpxError::InvalidStructure {
                    detail: format!("table caption span missing for {prefix}"),
                },
            )?;
        collect_raw_caption_slots(xml, caption_span, caption, prefix, sink)?;
    }

    let row_spans = collect_direct_child_outer_spans(xml, table_span, "hp:tr")?;
    if row_spans.len() != table.rows.len() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "table row count mismatch while building preservation metadata for {prefix}: raw={} semantic={}",
                row_spans.len(),
                table.rows.len()
            ),
        });
    }
    for (row_idx, (row, row_span)) in table.rows.iter().zip(row_spans.into_iter()).enumerate() {
        collect_raw_row_slots(
            xml,
            row_span,
            row,
            &format!("{prefix}.rows[{row_idx}]"),
            sink,
            lift_header_footer,
        )?;
    }
    Ok(())
}

fn collect_raw_row_slots(
    xml: &str,
    row_span: Range<usize>,
    row: &HxTableRow,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
    lift_header_footer: bool,
) -> HwpxResult<()> {
    let cell_spans = collect_direct_child_outer_spans(xml, row_span, "hp:tc")?;
    if cell_spans.len() != row.cells.len() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "table cell count mismatch while building preservation metadata for {prefix}: raw={} semantic={}",
                cell_spans.len(),
                row.cells.len()
            ),
        });
    }
    for (cell_idx, (cell, cell_span)) in row.cells.iter().zip(cell_spans.into_iter()).enumerate() {
        collect_raw_cell_slots(
            xml,
            cell_span,
            cell,
            &format!("{prefix}.cells[{cell_idx}]"),
            sink,
            lift_header_footer,
        )?;
    }
    Ok(())
}

fn collect_raw_cell_slots(
    xml: &str,
    cell_span: Range<usize>,
    cell: &HxTableCell,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
    lift_header_footer: bool,
) -> HwpxResult<()> {
    let Some(sub_list) = &cell.sub_list else {
        return Ok(());
    };
    let sub_list_span = single_optional_direct_outer_span(xml, cell_span, "hp:subList")?
        .ok_or_else(|| HwpxError::InvalidStructure {
            detail: format!("cell subList span missing for {prefix}"),
        })?;
    // 디코더(`lift_header_footers`)가 표 칸 안 머리말/꼬리말을 구역 목록으로
    // 올리므로 같은 자리에서 슬롯도 센다.
    collect_raw_sublist_slots(
        xml,
        sub_list_span,
        sub_list,
        &format!("{prefix}.paragraphs"),
        sink,
        lift_header_footer,
    )
}

fn collect_raw_picture_slots(
    xml: &str,
    picture_span: Range<usize>,
    picture: &HxPic,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
) -> HwpxResult<()> {
    if let Some(caption) = &picture.caption {
        let caption_span = single_optional_direct_outer_span(xml, picture_span, "hp:caption")?
            .ok_or_else(|| HwpxError::InvalidStructure {
                detail: format!("picture caption span missing for {prefix}"),
            })?;
        collect_raw_caption_slots(xml, caption_span, caption, prefix, sink)?;
    }
    Ok(())
}

fn collect_raw_rect_slots(
    xml: &str,
    rect_span: Range<usize>,
    rect: &HxRect,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
    lift_header_footer: bool,
) -> HwpxResult<()> {
    if let Some(draw_text) = &rect.draw_text {
        let draw_text_span =
            single_optional_direct_outer_span(xml, rect_span.clone(), "hp:drawText")?.ok_or_else(
                || HwpxError::InvalidStructure {
                    detail: format!("textbox drawText span missing for {prefix}"),
                },
            )?;
        collect_raw_draw_text_slots(
            xml,
            draw_text_span,
            draw_text,
            prefix,
            sink,
            lift_header_footer,
        )?;
    }
    if let Some(caption) = &rect.caption {
        let caption_span = single_optional_direct_outer_span(xml, rect_span, "hp:caption")?
            .ok_or_else(|| HwpxError::InvalidStructure {
                detail: format!("textbox caption span missing for {prefix}"),
            })?;
        collect_raw_caption_slots(xml, caption_span, caption, prefix, sink)?;
    }
    Ok(())
}

fn collect_raw_line_slots(
    xml: &str,
    line_span: Range<usize>,
    line: &HxLine,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
) -> HwpxResult<()> {
    if let Some(caption) = &line.caption {
        let caption_span = single_optional_direct_outer_span(xml, line_span, "hp:caption")?
            .ok_or_else(|| HwpxError::InvalidStructure {
                detail: format!("line caption span missing for {prefix}"),
            })?;
        collect_raw_caption_slots(xml, caption_span, caption, prefix, sink)?;
    }
    Ok(())
}

fn collect_raw_ellipse_slots(
    xml: &str,
    ellipse_span: Range<usize>,
    ellipse: &HxEllipse,
    control_name: &str,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
) -> HwpxResult<()> {
    if control_name == "ellipse" {
        if let Some(draw_text) = &ellipse.draw_text {
            let draw_text_span =
                single_optional_direct_outer_span(xml, ellipse_span.clone(), "hp:drawText")?
                    .ok_or_else(|| HwpxError::InvalidStructure {
                        detail: format!("ellipse drawText span missing for {prefix}"),
                    })?;
            collect_raw_draw_text_slots(xml, draw_text_span, draw_text, prefix, sink, false)?;
        }
    }
    if let Some(caption) = &ellipse.caption {
        let caption_span = single_optional_direct_outer_span(xml, ellipse_span, "hp:caption")?
            .ok_or_else(|| HwpxError::InvalidStructure {
                detail: format!("{control_name} caption span missing for {prefix}"),
            })?;
        collect_raw_caption_slots(xml, caption_span, caption, prefix, sink)?;
    }
    Ok(())
}

fn collect_raw_polygon_slots(
    xml: &str,
    polygon_span: Range<usize>,
    polygon: &HxPolygon,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
) -> HwpxResult<()> {
    if let Some(draw_text) = &polygon.draw_text {
        let draw_text_span =
            single_optional_direct_outer_span(xml, polygon_span.clone(), "hp:drawText")?
                .ok_or_else(|| HwpxError::InvalidStructure {
                    detail: format!("polygon drawText span missing for {prefix}"),
                })?;
        collect_raw_draw_text_slots(xml, draw_text_span, draw_text, prefix, sink, false)?;
    }
    if let Some(caption) = &polygon.caption {
        let caption_span = single_optional_direct_outer_span(xml, polygon_span, "hp:caption")?
            .ok_or_else(|| HwpxError::InvalidStructure {
                detail: format!("polygon caption span missing for {prefix}"),
            })?;
        collect_raw_caption_slots(xml, caption_span, caption, prefix, sink)?;
    }
    Ok(())
}

fn collect_raw_curve_slots(
    xml: &str,
    curve_span: Range<usize>,
    curve: &HxCurve,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
) -> HwpxResult<()> {
    if let Some(caption) = &curve.caption {
        let caption_span = single_optional_direct_outer_span(xml, curve_span, "hp:caption")?
            .ok_or_else(|| HwpxError::InvalidStructure {
                detail: format!("curve caption span missing for {prefix}"),
            })?;
        collect_raw_caption_slots(xml, caption_span, caption, prefix, sink)?;
    }
    Ok(())
}

fn collect_raw_connect_line_slots(
    xml: &str,
    connect_line_span: Range<usize>,
    connect_line: &HxConnectLine,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
) -> HwpxResult<()> {
    if let Some(caption) = &connect_line.caption {
        let caption_span = single_optional_direct_outer_span(xml, connect_line_span, "hp:caption")?
            .ok_or_else(|| HwpxError::InvalidStructure {
                detail: format!("connect line caption span missing for {prefix}"),
            })?;
        collect_raw_caption_slots(xml, caption_span, caption, prefix, sink)?;
    }
    Ok(())
}

fn collect_raw_draw_text_slots(
    xml: &str,
    draw_text_span: Range<usize>,
    draw_text: &crate::schema::shapes::HxDrawText,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
    lift_header_footer: bool,
) -> HwpxResult<()> {
    let sub_list_span = single_optional_direct_outer_span(xml, draw_text_span, "hp:subList")?
        .ok_or_else(|| HwpxError::InvalidStructure {
            detail: format!("drawText subList span missing for {prefix}"),
        })?;
    collect_raw_sublist_slots(
        xml,
        sub_list_span,
        &draw_text.sub_list,
        &format!("{prefix}.paragraphs"),
        sink,
        lift_header_footer,
    )
}

fn collect_raw_header_footer_slots(
    xml: &str,
    ctrl_span: Range<usize>,
    value: &HxHeaderFooter,
    prefix: &str,
    groups: &mut Vec<Vec<PreservedTextSlot>>,
) -> HwpxResult<()> {
    groups.push(Vec::new());
    let slots = groups.last_mut().expect("just pushed");
    let tag = if prefix == "header" { "hp:header" } else { "hp:footer" };
    let header_footer_span =
        single_optional_direct_outer_span(xml, ctrl_span, tag)?.ok_or_else(|| {
            HwpxError::InvalidStructure {
                detail: format!("{prefix} span missing while building preservation metadata"),
            }
        })?;
    let Some(sub_list) = &value.sub_list else {
        return Ok(());
    };
    let sub_list_span = single_optional_direct_outer_span(xml, header_footer_span, "hp:subList")?
        .ok_or_else(|| HwpxError::InvalidStructure {
        detail: format!("{prefix} subList span missing while building preservation metadata"),
    })?;
    let mut section_sink = RawSectionSlots::default();
    let mut sink = RawSlotSink { body_slots: slots, section_slots: &mut section_sink };
    collect_raw_sublist_slots(
        xml,
        sub_list_span,
        sub_list,
        &format!("{prefix}.paragraphs"),
        &mut sink,
        false,
    )
}

fn collect_raw_footnote_slots(
    xml: &str,
    ctrl_span: Range<usize>,
    note: &HxFootNote,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
) -> HwpxResult<()> {
    let tag = if prefix.ends_with(".footnote") { "hp:footNote" } else { "hp:endNote" };
    let note_span = single_optional_direct_outer_span(xml, ctrl_span, tag)?.ok_or_else(|| {
        HwpxError::InvalidStructure { detail: format!("note span missing for {prefix}") }
    })?;
    let sub_list_span = single_optional_direct_outer_span(xml, note_span, "hp:subList")?
        .ok_or_else(|| HwpxError::InvalidStructure {
            detail: format!("note subList span missing for {prefix}"),
        })?;
    // The decoder drops the number head the encoder injects
    // ([`injected_head_range`](crate::decoder::section::injected_head_range)):
    // its " " spacer is no run of Core's, so it is no slot either — the
    // spacer's text is blanked for the walk (its element stays, so the run
    // spans still line up).
    // 이것을 실패시키는 것: 이 비우기를 지우기(각주가 있는 문서의 보존 슬롯 수
    // 불일치 — `every_container_fill_visits_maps_to_its_own_paragraph`).
    let mut sub_list = note.sub_list.clone();
    if crate::decoder::section::injected_head_range(&sub_list).is_some() {
        for text in &mut sub_list.paragraphs[0].runs[1].texts {
            *text = crate::schema::section::HxText::new(String::new());
        }
    }
    collect_raw_sublist_slots(
        xml,
        sub_list_span,
        &sub_list,
        &format!("{prefix}.paragraphs"),
        sink,
        false,
    )
}

fn collect_raw_caption_slots(
    xml: &str,
    caption_span: Range<usize>,
    caption: &HxCaption,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
) -> HwpxResult<()> {
    let sub_list_span = single_optional_direct_outer_span(xml, caption_span, "hp:subList")?
        .ok_or_else(|| HwpxError::InvalidStructure {
            detail: format!("caption subList span missing for {prefix}"),
        })?;
    collect_raw_sublist_slots(
        xml,
        sub_list_span,
        &caption.sub_list,
        &format!("{prefix}.caption.paragraphs"),
        sink,
        false,
    )
}

fn collect_raw_sublist_slots(
    xml: &str,
    sub_list_span: Range<usize>,
    sub_list: &HxSubList,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
    allow_section_header_footer: bool,
) -> HwpxResult<()> {
    collect_raw_paragraph_list_slots(
        xml,
        sub_list_span,
        &sub_list.paragraphs,
        prefix,
        sink,
        allow_section_header_footer,
    )
}

fn picture_has_semantic_run(picture: &HxPic) -> bool {
    matches!(picture.img.as_ref(), Some(img) if !img.binary_item_id_ref.is_empty())
}

pub(crate) fn find_root_span(xml: &str, tag: &str) -> HwpxResult<Range<usize>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf: Vec<u8> = Vec::new();
    let mut root_start: Option<usize> = None;
    let mut depth: usize = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) if event.name().as_ref() == tag => {
                let end = buffer_position(&reader)?;
                let start = event_start(end, event.len(), false)?;
                if root_start.is_none() {
                    root_start = Some(start);
                }
                depth += 1;
            }
            Ok(Event::Empty(event)) if event.name().as_ref() == tag => {
                let end = buffer_position(&reader)?;
                let start = event_start(end, event.len(), true)?;
                if depth == 0 {
                    return Ok(start..end);
                }
            }
            Ok(Event::Start(_)) => depth += 1,
            Ok(Event::End(event)) if event.name().as_ref() == tag => {
                let end = buffer_position(&reader)?;
                if depth == 1 {
                    let start = root_start.ok_or_else(|| HwpxError::XmlParse {
                        file: "section.xml".into(),
                        detail: "root element closed before start was recorded".into(),
                    })?;
                    return Ok(start..end);
                }
                depth = depth.saturating_sub(1);
            }
            Ok(Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(HwpxError::XmlParse {
                    file: "section.xml".into(),
                    detail: error.to_string(),
                });
            }
        }
        buf.clear();
    }

    Err(HwpxError::InvalidStructure { detail: format!("root element '{tag}' not found") })
}

pub(crate) fn collect_direct_child_outer_spans(
    xml: &str,
    parent_span: Range<usize>,
    tag: &str,
) -> HwpxResult<Vec<Range<usize>>> {
    let fragment = &xml[parent_span.clone()];
    let mut reader = Reader::from_str(fragment);
    reader.config_mut().trim_text(false);

    let mut buf: Vec<u8> = Vec::new();
    let mut results: Vec<Range<usize>> = Vec::new();
    let mut open_indices: Vec<usize> = Vec::new();
    let mut depth: usize = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => {
                if depth == 1 && event.name().as_ref() == tag {
                    let end = buffer_position(&reader)?;
                    let start = parent_span.start + event_start(end, event.len(), false)?;
                    results.push(start..0);
                    open_indices.push(results.len() - 1);
                }
                depth += 1;
            }
            Ok(Event::Empty(event)) => {
                if depth == 1 && event.name().as_ref() == tag {
                    let end = parent_span.start + buffer_position(&reader)?;
                    let start = parent_span.start
                        + event_start(end - parent_span.start, event.len(), true)?;
                    results.push(start..end);
                }
            }
            Ok(Event::End(event)) => {
                if depth == 2 && event.name().as_ref() == tag {
                    let end = parent_span.start + buffer_position(&reader)?;
                    let index = open_indices.pop().ok_or_else(|| HwpxError::XmlParse {
                        file: "section.xml".into(),
                        detail: "closing tag without matching direct-child start".into(),
                    })?;
                    results[index].end = end;
                }
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(HwpxError::XmlParse {
                    file: "section.xml".into(),
                    detail: error.to_string(),
                });
            }
        }
        buf.clear();
    }

    Ok(results)
}

fn single_optional_direct_outer_span(
    xml: &str,
    parent_span: Range<usize>,
    tag: &str,
) -> HwpxResult<Option<Range<usize>>> {
    let spans = collect_direct_child_outer_spans(xml, parent_span, tag)?;
    match spans.len() {
        0 => Ok(None),
        1 => Ok(spans.into_iter().next()),
        count => Err(HwpxError::InvalidStructure {
            detail: format!("expected at most one direct '{tag}' child but found {count}"),
        }),
    }
}

fn collect_direct_text_elements(
    xml: &str,
    parent_span: Range<usize>,
) -> HwpxResult<Vec<TextElementSpan>> {
    let fragment = &xml[parent_span.clone()];
    let mut reader = Reader::from_str(fragment);
    reader.config_mut().trim_text(false);

    let mut buf: Vec<u8> = Vec::new();
    let mut results: Vec<TextElementSpan> = Vec::new();
    let mut open_indices: Vec<usize> = Vec::new();
    let mut depth: usize = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => {
                if depth == 1 && event.name().as_ref() == "hp:t" {
                    let end_local = buffer_position(&reader)?;
                    let start_local = event_start(end_local, event.len(), false)?;
                    results.push(TextElementSpan {
                        element_start: parent_span.start + start_local,
                        element_end: 0,
                        content_start: Some(parent_span.start + end_local),
                        content_end: Some(0),
                        has_inline_markup: false,
                    });
                    open_indices.push(results.len() - 1);
                }
                if event.name().as_ref() != "hp:t" {
                    if let Some(index) = open_indices.last().copied() {
                        results[index].has_inline_markup = true;
                    }
                }
                depth += 1;
            }
            Ok(Event::Empty(event)) => {
                if depth == 1 && event.name().as_ref() == "hp:t" {
                    let end_local = buffer_position(&reader)?;
                    let start_local = event_start(end_local, event.len(), true)?;
                    results.push(TextElementSpan {
                        element_start: parent_span.start + start_local,
                        element_end: parent_span.start + end_local,
                        content_start: None,
                        content_end: None,
                        has_inline_markup: false,
                    });
                } else if let Some(index) = open_indices.last().copied() {
                    results[index].has_inline_markup = true;
                }
            }
            Ok(Event::End(event)) => {
                if depth == 2 && event.name().as_ref() == "hp:t" {
                    let end_local = buffer_position(&reader)?;
                    let close_start_local = end_tag_start(end_local, event.len())?;
                    let index = open_indices.pop().ok_or_else(|| HwpxError::XmlParse {
                        file: "section.xml".into(),
                        detail: "closing </hp:t> without matching start".into(),
                    })?;
                    results[index].element_end = parent_span.start + end_local;
                    results[index].content_end = Some(parent_span.start + close_start_local);
                }
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(HwpxError::XmlParse {
                    file: "section.xml".into(),
                    detail: error.to_string(),
                });
            }
        }
        buf.clear();
    }

    Ok(results)
}

fn text_locator(span: &TextElementSpan) -> TextLocator {
    TextLocator::TextElement {
        element_start: span.element_start,
        element_end: span.element_end,
        content_start: span.content_start,
        content_end: span.content_end,
    }
}

pub(crate) fn event_start(end: usize, raw_len: usize, is_empty: bool) -> HwpxResult<usize> {
    let extra = if is_empty { 3 } else { 2 };
    end.checked_sub(raw_len + extra).ok_or_else(|| HwpxError::XmlParse {
        file: "section.xml".into(),
        detail: "invalid XML event span".into(),
    })
}

fn end_tag_start(end: usize, raw_len: usize) -> HwpxResult<usize> {
    end.checked_sub(raw_len + 3).ok_or_else(|| HwpxError::XmlParse {
        file: "section.xml".into(),
        detail: "invalid XML end-tag span".into(),
    })
}

pub(crate) fn buffer_position(reader: &Reader<&[u8]>) -> HwpxResult<usize> {
    usize::try_from(reader.buffer_position())
        .map_err(|_| HwpxError::Zip("buffer position overflow".into()))
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
    let (range, replacement) = text_locator_replacement(xml, locator, new_text)?;
    let mut patched = xml.to_string();
    patched.replace_range(range, &replacement);
    Ok(patched)
}

/// The span of `xml` a [`TextLocator`] replaces and what goes there.
fn text_locator_replacement(
    xml: &str,
    locator: &TextLocator,
    new_text: &str,
) -> HwpxResult<(Range<usize>, String)> {
    match locator {
        TextLocator::TextElement { element_start, element_end, content_start, content_end } => {
            if let (Some(start), Some(end)) = (*content_start, *content_end) {
                Ok((start..end, encode_text_inner(new_text)?))
            } else {
                let original = &xml[*element_start..*element_end];
                Ok((*element_start..*element_end, patch_text_element_xml(original, new_text)?))
            }
        }
        TextLocator::EmptyRun { run_start, run_end } => {
            let original = &xml[*run_start..*run_end];
            Ok((*run_start..*run_end, patch_text_run_xml(original, new_text)?))
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

fn patch_text_element_xml(element_xml: &str, new_text: &str) -> HwpxResult<String> {
    let encoded = encode_text_inner(new_text)?;

    if let Some(prefix) = element_xml.strip_suffix("/>") {
        if new_text.is_empty() {
            return Ok(element_xml.to_string());
        }
        return Ok(format!("{prefix}>{encoded}</hp:t>"));
    }

    let open_end = element_xml.find('>').ok_or_else(|| HwpxError::XmlParse {
        file: "section.xml".into(),
        detail: "unterminated <hp:t> while patching text element".into(),
    })?;
    let close_start = element_xml.rfind("</hp:t>").ok_or_else(|| HwpxError::XmlParse {
        file: "section.xml".into(),
        detail: "missing </hp:t> while patching text element".into(),
    })?;

    let mut patched = String::with_capacity(element_xml.len() + encoded.len());
    patched.push_str(&element_xml[..open_end + 1]);
    patched.push_str(&encoded);
    patched.push_str(&element_xml[close_start..]);
    Ok(patched)
}

fn patch_text_run_xml(run_xml: &str, new_text: &str) -> HwpxResult<String> {
    let encoded = encode_text_inner(new_text)?;

    if let Some(text_start_rel) = run_xml.find("<hp:t") {
        let text_start = text_start_rel;
        let open_end_rel = run_xml[text_start..].find('>').ok_or_else(|| HwpxError::XmlParse {
            file: "section.xml".into(),
            detail: "unterminated <hp:t> tag while patching run".into(),
        })?;
        let open_end = text_start + open_end_rel;

        if run_xml.as_bytes().get(open_end.saturating_sub(1)) == Some(&b'/') {
            if new_text.is_empty() {
                return Ok(run_xml.to_string());
            }
            let open_tag = &run_xml[text_start..open_end - 1];
            let replacement = format!("{open_tag}>{encoded}</hp:t>");
            let mut patched = String::with_capacity(run_xml.len() + encoded.len() + 8);
            patched.push_str(&run_xml[..text_start]);
            patched.push_str(&replacement);
            patched.push_str(&run_xml[open_end + 1..]);
            return Ok(patched);
        }

        let close_rel =
            run_xml[open_end + 1..].find("</hp:t>").ok_or_else(|| HwpxError::XmlParse {
                file: "section.xml".into(),
                detail: "missing </hp:t> while patching run".into(),
            })?;
        let close = open_end + 1 + close_rel;
        let mut patched = String::with_capacity(run_xml.len() + encoded.len());
        patched.push_str(&run_xml[..open_end + 1]);
        patched.push_str(&encoded);
        patched.push_str(&run_xml[close..]);
        return Ok(patched);
    }

    if let Some(prefix) = run_xml.strip_suffix("/>") {
        if new_text.is_empty() {
            return Ok(run_xml.to_string());
        }
        return Ok(format!("{prefix}><hp:t>{encoded}</hp:t></hp:run>"));
    }

    let end_run = run_xml.rfind("</hp:run>").ok_or_else(|| HwpxError::XmlParse {
        file: "section.xml".into(),
        detail: "missing </hp:run> while patching run".into(),
    })?;
    let mut patched = String::with_capacity(run_xml.len() + encoded.len() + 16);
    patched.push_str(&run_xml[..end_run]);
    patched.push_str("<hp:t>");
    patched.push_str(&encoded);
    patched.push_str("</hp:t>");
    patched.push_str(&run_xml[end_run..]);
    Ok(patched)
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

fn redact_section_texts(section: &mut Section) {
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
            // structure that the redactor is trying to remove. See
            // debug doc §3a-C17.
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
        Control::Field {
            field_type: hwpforge_foundation::FieldType::ClickHere,
            display_text,
            ..
        } => display_text.clear(),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
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
        assert!(
            matches!(error, HwpxError::XmlParse { file, .. } if file == "Contents/section2.xml")
        );
    }

    /// Builds an in-memory ZIP whose entries are `(name, size)` runs of zeros.
    fn zeros_zip(entries: &[(&str, usize)]) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::<u8>::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, size) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(&vec![0u8; *size]).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    /// Rewrites the declared uncompressed size in every central-directory
    /// record, leaving the compressed data (and its CRC) untouched.
    fn forge_declared_size(zip: &mut [u8], declared: u32) {
        let mut at = 0;
        while at + 28 <= zip.len() {
            if zip[at..at + 4] == [0x50, 0x4b, 0x01, 0x02] {
                zip[at + 24..at + 28].copy_from_slice(&declared.to_le_bytes());
                at += 46;
            } else {
                at += 1;
            }
        }
    }

    /// W0: a rewritten package keeps every entry at the fixed ZIP epoch.
    /// 이것을 실패시키는 것: 시각 고정 삭제 — `--features zip/time` 으로 돌릴 때
    /// (그 기능이 꺼진 빌드에서는 zip 기본값이 같은 epoch 라 통과한다).
    #[test]
    fn rewritten_entries_keep_a_fixed_time() {
        let bytes = zeros_zip(&[("a.bin", 8), ("b.bin", 8)]);
        let out = RawPackage::read_capped(&bytes, 1024, u64::MAX).unwrap().write().unwrap();
        let mut archive = ZipArchive::new(Cursor::new(out)).unwrap();
        for i in 0..archive.len() {
            let entry = archive.by_index(i).unwrap();
            assert_eq!(entry.last_modified(), Some(zip::DateTime::default()), "{}", entry.name());
        }
    }

    // 이것을 실패시키는 것: 버퍼 용량을 헤더 크기로 예약 (`Vec::with_capacity(file.size().min(max_entry) as usize)`)
    #[test]
    fn read_capped_does_not_reserve_capacity_from_the_forged_header_size() {
        let mut bytes = zeros_zip(&[("a.bin", 8), ("b.bin", 8), ("c.bin", 8)]);
        forge_declared_size(&mut bytes, 10 * 1024 * 1024);
        // Guard the forgery itself: the archive must really declare 10 MiB.
        let mut archive = ZipArchive::new(Cursor::new(&bytes[..])).unwrap();
        assert_eq!(archive.by_index(0).unwrap().size(), 10 * 1024 * 1024);
        drop(archive);

        let package = RawPackage::read_capped(&bytes, 20 * 1024 * 1024, u64::MAX).unwrap();

        for entry in &package.entries {
            assert_eq!(entry.bytes.len(), 8);
            assert!(entry.bytes.capacity() < 4096, "capacity {}", entry.bytes.capacity());
        }
    }

    // 이것을 실패시키는 것: `file.take(max_entry + 1)` 제거 (해제량이 cap+1 이 아니라 cap*10 으로 보고됨)
    #[test]
    fn read_capped_stops_decompression_one_byte_past_the_entry_cap() {
        let cap: u64 = 1024;
        let bytes = zeros_zip(&[("bomb.bin", cap as usize * 10)]);

        let err = RawPackage::read_capped(&bytes, cap, u64::MAX).unwrap_err();

        match err {
            HwpxError::InvalidStructure { detail } => assert_eq!(
                detail,
                format!(
                    "entry 'bomb.bin' decompressed to {} bytes, exceeds limit of {cap}",
                    cap + 1
                )
            ),
            other => panic!("expected InvalidStructure, got {other:?}"),
        }
    }

    // 이것을 실패시키는 것: 누적 총량 검사(`total > max_total`) 제거
    #[test]
    fn read_capped_rejects_entries_that_are_small_alone_but_large_together() {
        let bytes = zeros_zip(&[("a.bin", 600), ("b.bin", 600)]);

        let err = RawPackage::read_capped(&bytes, 1024, 1000).unwrap_err();

        match err {
            HwpxError::InvalidStructure { detail } => {
                assert_eq!(detail, "total decompressed data (1200 bytes) exceeds limit of 1000")
            }
            other => panic!("expected InvalidStructure, got {other:?}"),
        }
    }

    // 이것을 실패시키는 것: 엔트리 검사 `>` 를 `>=` 로 바꿈 (정확히 cap 바이트가 거부됨)
    // 총량 검사 `>` → `>=` 도 같은 테스트(total == 상한)가 잡는다.
    #[test]
    fn read_capped_accepts_an_entry_of_exactly_the_cap() {
        let bytes = zeros_zip(&[("exact.bin", 1024)]);

        let package = RawPackage::read_capped(&bytes, 1024, 1024).unwrap();

        assert_eq!(package.entries[0].bytes.len(), 1024);
    }

    /// A decodable package can still hold a document Core rejects.
    ///
    /// This is the reachability question behind `ops::validate`'s `ok: false`
    /// branch (W1b review R2, finding 10). The encoder cannot produce such a
    /// package — `HwpxEncoder::encode` takes a `Document<Validated>`, so the
    /// type state makes an invalid document unencodable — but a package from
    /// another producer can carry a section with no paragraphs, and the
    /// decoder reports it faithfully rather than inventing a paragraph.
    ///
    /// The failed verdict is therefore a real branch, not dead code.
    ///
    /// The neighbouring case is the opposite: a `<hp:p>` stripped of its runs
    /// decodes with a synthesised run, so `EmptyParagraph` is **not**
    /// reachable this way. Both are asserted, so a decoder change that starts
    /// synthesising sections (or stops synthesising runs) is caught here.
    ///
    /// It lives in this module because `RawPackage` is the crate's only
    /// package writer and is `pub(crate)`.
    #[test]
    fn a_section_without_paragraphs_decodes_into_a_document_core_rejects() {
        use crate::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
        use crate::HwpxEncoder;
        use hwpforge_core::image::ImageStore;
        use hwpforge_core::run::Run;
        use hwpforge_core::{Document, PageSettings, Paragraph};
        use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

        let para = |text: &str| {
            Paragraph::with_runs(
                vec![Run::text(text, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )
        };
        let mut document = Document::new();
        document.add_section(Section::with_paragraphs(
            vec![para("hello"), para("world")],
            PageSettings::a4(),
        ));
        let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
        styles.push_char_shape(HwpxCharShape::default());
        styles.push_para_shape(HwpxParaShape::default());
        let bytes = HwpxEncoder::encode(&document.validate().unwrap(), &styles, &ImageStore::new())
            .unwrap();
        let section_entry = RawPackage::read(&bytes).unwrap().section_path(0).unwrap();

        // ── a paragraph with no runs: the decoder fills one in ──────
        let mut pkg = RawPackage::read(&bytes).unwrap();
        let xml = pkg.read_text_entry(&section_entry).unwrap();
        let second_para = r#"<hp:p id="1" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0">"#;
        let runless = xml.replace(
            &format!("{second_para}<hp:run charPrIDRef=\"0\"><hp:t>world</hp:t></hp:run></hp:p>"),
            &format!("{second_para}</hp:p>"),
        );
        assert_ne!(runless, xml, "the fixture XML shape changed; update this test");
        pkg.replace_text_entry(&section_entry, runless);
        let decoded = crate::HwpxDecoder::decode(&pkg.write().unwrap()).unwrap();
        assert!(
            decoded.document.sections()[0].paragraphs.iter().all(|p| !p.runs.is_empty()),
            "the decoder synthesises a run, so EmptyParagraph is unreachable this way"
        );
        assert!(decoded.document.validate().is_ok());

        // ── a section with no paragraphs: Core rejects it ───────────
        let mut pkg = RawPackage::read(&bytes).unwrap();
        let xml = pkg.read_text_entry(&section_entry).unwrap();
        let first_para = xml.find("><hp:p").map(|i| i + 1).unwrap();
        let sec_close = xml.rfind("</hs:sec>").unwrap();
        pkg.replace_text_entry(
            &section_entry,
            format!("{}{}", &xml[..first_para], &xml[sec_close..]),
        );

        let decoded = crate::HwpxDecoder::decode(&pkg.write().unwrap())
            .expect("an empty section still decodes");

        assert_eq!(decoded.document.sections().len(), 1);
        assert!(decoded.document.sections()[0].paragraphs.is_empty());
        assert!(
            matches!(
                decoded.document.validate(),
                Err(hwpforge_core::CoreError::Validation(
                    hwpforge_core::ValidationError::EmptySection { section_index: 0 }
                ))
            ),
            "`ops::validate` reports ok:false for exactly this document"
        );
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

    #[test]
    fn patch_text_element_expands_self_closing_tag() {
        let patched = patch_text_element_xml(r#"<hp:t/>"#, "filled").unwrap();
        assert_eq!(patched, "<hp:t>filled</hp:t>");
    }

    #[test]
    fn patch_text_run_inserts_text_into_empty_run() {
        let patched = patch_text_run_xml(r#"<hp:run charPrIDRef="0"/>"#, "value").unwrap();
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

    // §19.7 F1 계약 변경: 칸 안 꼬리말은 구역 꼬리말로 올라가고(옛 계약
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

    // F-T6: 최상위 꼬리말 + 뒤 문단 칸 안 꼬리말 = 둘 → semantic 쪽 `footers[i]`
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
