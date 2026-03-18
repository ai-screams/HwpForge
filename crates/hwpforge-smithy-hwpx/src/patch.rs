//! Preserve-first HWPX patching utilities.
//!
//! Unlike [`crate::HwpxEncoder`], this module is intentionally conservative:
//! it keeps untouched ZIP entries byte-for-byte and only patches section XML
//! when the semantic change is text-only and a stable preservation plan exists.

use std::collections::BTreeMap;
use std::io::{Cursor, Read as _, Write as _};
use std::ops::Range;

use quick_xml::de::from_str;
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

use crate::decoder::package::PackageReader;
use crate::encoder::escape_xml;
use crate::error::{HwpxError, HwpxResult};
use crate::exchange::{PreservedTextSlot, SectionPreservation, TextLocator};
use crate::schema::section::{
    HxCaption, HxFieldBegin, HxFootNote, HxHeaderFooter, HxParagraph, HxPic, HxRun, HxSection,
    HxSubList, HxTable, HxTableCell, HxTableRow,
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
    pub fn export_section_preservation(
        base_bytes: &[u8],
        section_idx: usize,
        section: &Section,
    ) -> HwpxResult<SectionPreservation> {
        let raw_package = RawPackage::read(base_bytes)?;
        let section_path = section_path(section_idx);
        let section_xml = raw_package.read_text_entry(&section_path)?;
        build_section_preservation(&section_xml, &section_path, section)
    }

    /// Patches a single section while preserving untouched package entries.
    ///
    /// The preserving path currently supports text-only edits. Any structural
    /// change (style changes, table geometry changes, control changes, etc.)
    /// returns [`HwpxError::InvalidStructure`].
    pub fn patch_section_preserving(
        base_bytes: &[u8],
        section_idx: usize,
        replacement: &Section,
        styles: Option<&HwpxStyleStore>,
        preservation: Option<&SectionPreservation>,
    ) -> HwpxResult<Vec<u8>> {
        let preservation = preservation.ok_or_else(|| HwpxError::InvalidStructure {
            detail:
                "missing preservation metadata; re-export the section with the current to-json command"
                    .into(),
        })?;

        let expected_section_path = section_path(section_idx);
        if preservation.section_path != expected_section_path {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "preservation metadata targets '{}' but patch requested '{}'",
                    preservation.section_path, expected_section_path
                ),
            });
        }

        let mut decoded = HwpxDecoder::decode(base_bytes)?;
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

        let mut raw_package = RawPackage::read(base_bytes)?;
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

        let base_slots = collect_semantic_text_slots(&base_section);
        validate_preservation_slots(preservation, &base_slots)?;

        let replacement_slots = collect_semantic_text_slots(replacement);
        if replacement_slots.len() != preservation.text_slots.len() {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "replacement semantic text-slot count mismatch: expected {} actual {}",
                    preservation.text_slots.len(),
                    replacement_slots.len()
                ),
            });
        }

        for (index, (replacement_slot, preserved_slot)) in
            replacement_slots.iter().zip(&preservation.text_slots).enumerate()
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

        let mut patched_section_xml = base_section_xml.clone();
        for (replacement_slot, preserved_slot) in
            replacement_slots.iter().zip(&preservation.text_slots).rev()
        {
            if replacement_slot.text == preserved_slot.original_text {
                continue;
            }

            if preserved_slot.has_inline_markup {
                return Err(HwpxError::InvalidStructure {
                    detail: format!(
                        "preserving patch does not support editing text-slot '{}' with inline HWPX markup yet",
                        preserved_slot.path
                    ),
                });
            }

            patched_section_xml = apply_text_locator(
                &patched_section_xml,
                &preserved_slot.locator,
                &replacement_slot.text,
            )?;
        }

        if patched_section_xml == base_section_xml {
            return Ok(base_bytes.to_vec());
        }

        raw_package.replace_text_entry(&expected_section_path, patched_section_xml);
        raw_package.write()
    }
}

#[derive(Debug, Clone)]
struct SemanticTextSlot {
    path: String,
    text: String,
}

#[derive(Debug, Default)]
struct RawSectionSlots {
    body: Vec<PreservedTextSlot>,
    header: Vec<PreservedTextSlot>,
    footer: Vec<PreservedTextSlot>,
}

impl RawSectionSlots {
    fn into_slots(self) -> Vec<PreservedTextSlot> {
        let mut slots = self.body;
        slots.extend(self.header);
        slots.extend(self.footer);
        slots
    }
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
struct RawPackage {
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
    fn read(bytes: &[u8]) -> HwpxResult<Self> {
        let _ = PackageReader::new(bytes)?;

        let cursor = Cursor::new(bytes);
        let mut archive = ZipArchive::new(cursor).map_err(|e| HwpxError::Zip(e.to_string()))?;
        let mut entries: Vec<RawPackageEntry> = Vec::with_capacity(archive.len());
        let mut index_by_path: BTreeMap<String, usize> = BTreeMap::new();

        for index in 0..archive.len() {
            let mut file = archive.by_index(index).map_err(|e| HwpxError::Zip(e.to_string()))?;
            let mut data: Vec<u8> = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut data)
                .map_err(|e| HwpxError::Zip(format!("read '{}': {e}", file.name())))?;
            let path = file.name().to_string();
            index_by_path.insert(path.clone(), entries.len());
            entries.push(RawPackageEntry { path, bytes: data, compression: file.compression() });
        }

        Ok(Self { entries, index_by_path })
    }

    fn read_text_entry(&self, path: &str) -> HwpxResult<String> {
        let index = self
            .index_by_path
            .get(path)
            .copied()
            .ok_or_else(|| HwpxError::MissingFile { path: path.to_string() })?;
        String::from_utf8(self.entries[index].bytes.clone())
            .map_err(|e| HwpxError::Zip(format!("entry '{path}' is not valid UTF-8: {e}")))
    }

    fn replace_text_entry(&mut self, path: &str, content: String) {
        if let Some(index) = self.index_by_path.get(path).copied() {
            self.entries[index].bytes = content.into_bytes();
        }
    }

    fn write(&self) -> HwpxResult<Vec<u8>> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut zip = ZipWriter::new(cursor);

        for entry in &self.entries {
            let options = SimpleFileOptions::default().compression_method(entry.compression);
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
    let hx_section: HxSection = from_str(section_xml).map_err(|error| HwpxError::XmlParse {
        file: section_path.to_string(),
        detail: error.to_string(),
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
        section_path: section_path.to_string(),
        section_sha256: sha256_hex(section_xml.as_bytes()),
        text_slots,
    })
}

fn validate_preservation_slots(
    preservation: &SectionPreservation,
    semantic_slots: &[SemanticTextSlot],
) -> HwpxResult<()> {
    if preservation.text_slots.len() != semantic_slots.len() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "preservation metadata text-slot count mismatch: expected {} actual {}",
                preservation.text_slots.len(),
                semantic_slots.len()
            ),
        });
    }

    for (index, (preserved_slot, semantic_slot)) in
        preservation.text_slots.iter().zip(semantic_slots).enumerate()
    {
        if preserved_slot.path != semantic_slot.path {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "preservation metadata path mismatch at index {index}: expected '{}' actual '{}'",
                    preserved_slot.path, semantic_slot.path
                ),
            });
        }
        if preserved_slot.original_text != semantic_slot.text {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "preservation metadata original_text mismatch at index {index} path '{}': expected '{}' actual '{}'",
                    preserved_slot.path, preserved_slot.original_text, semantic_slot.text
                ),
            });
        }
    }

    Ok(())
}

fn collect_semantic_text_slots(section: &Section) -> Vec<SemanticTextSlot> {
    let mut slots: Vec<SemanticTextSlot> = Vec::new();
    collect_semantic_paragraph_slots(&section.paragraphs, "paragraphs", &mut slots);
    if let Some(header) = &section.header {
        collect_semantic_header_footer_slots(header, "header", &mut slots);
    }
    if let Some(footer) = &section.footer {
        collect_semantic_header_footer_slots(footer, "footer", &mut slots);
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
        _ => {}
    }
}

fn collect_raw_text_slots(xml: &str, section: &HxSection) -> HwpxResult<Vec<PreservedTextSlot>> {
    let root_span = find_root_span(xml, b"hs:sec")?;
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
    let paragraph_spans = collect_direct_child_outer_spans(xml, parent_span, b"hp:p")?;
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
    let run_spans = collect_direct_child_outer_spans(xml, paragraph_span.clone(), b"hp:run")?;
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
    let has_field_pair = run.ctrls.iter().any(|ctrl| ctrl.field_begin.is_some())
        && run.ctrls.iter().any(|ctrl| ctrl.field_end.is_some());

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

    if !has_field_pair {
        for (text, locator) in run.texts.iter().zip(text_locators.iter()) {
            let text_content = text.text();
            if !text_content.is_empty() {
                sink.body_slots.push(PreservedTextSlot {
                    path: format!("{prefix}.runs[{semantic_run_idx}].text"),
                    original_text: text_content,
                    has_inline_markup: locator.has_inline_markup,
                    locator: text_locator(locator),
                });
                semantic_run_idx += 1;
            }
        }
    }

    let table_spans = collect_direct_child_outer_spans(xml, run_span.clone(), b"hp:tbl")?;
    if table_spans.len() != run.tables.len() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "table count mismatch while building preservation metadata for {prefix}: raw={} semantic={}",
                table_spans.len(),
                run.tables.len()
            ),
        });
    }
    for (table, table_span) in run.tables.iter().zip(table_spans.into_iter()) {
        let table_prefix = format!("{prefix}.runs[{semantic_run_idx}].table");
        collect_raw_table_slots(xml, table_span, table, &table_prefix, sink)?;
        semantic_run_idx += 1;
    }

    let picture_spans = collect_direct_child_outer_spans(xml, run_span.clone(), b"hp:pic")?;
    if picture_spans.len() != run.pictures.len() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "picture count mismatch while building preservation metadata for {prefix}: raw={} semantic={}",
                picture_spans.len(),
                run.pictures.len()
            ),
        });
    }
    for (picture, picture_span) in run.pictures.iter().zip(picture_spans.into_iter()) {
        if picture_has_semantic_run(picture) {
            let picture_prefix = format!("{prefix}.runs[{semantic_run_idx}].image");
            collect_raw_picture_slots(xml, picture_span, picture, &picture_prefix, sink)?;
            semantic_run_idx += 1;
        }
    }

    let ctrl_spans = collect_direct_child_outer_spans(xml, run_span.clone(), b"hp:ctrl")?;
    if ctrl_spans.len() != run.ctrls.len() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "ctrl count mismatch while building preservation metadata for {prefix}: raw={} semantic={}",
                ctrl_spans.len(),
                run.ctrls.len()
            ),
        });
    }
    let mut pending_field_begin: Option<&HxFieldBegin> = None;
    for (ctrl, ctrl_span) in run.ctrls.iter().zip(ctrl_spans.into_iter()) {
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
            pending_field_begin = Some(field_begin);
        }
        if ctrl.field_end.is_some() && pending_field_begin.take().is_some() {
            semantic_run_idx += 1;
        }
        if let Some(auto_num) = &ctrl.auto_num {
            if auto_num.num_type == "PAGE" {
                semantic_run_idx += 1;
            }
        }
    }
    if let Some(field_begin) = pending_field_begin.take() {
        if field_begin.field_type == "BOOKMARK" {
            semantic_run_idx += 1;
        }
    }

    let rect_spans = collect_direct_child_outer_spans(xml, run_span.clone(), b"hp:rect")?;
    if rect_spans.len() != run.rects.len() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "rect count mismatch while building preservation metadata for {prefix}: raw={} semantic={}",
                rect_spans.len(),
                run.rects.len()
            ),
        });
    }
    for (rect, rect_span) in run.rects.iter().zip(rect_spans.into_iter()) {
        if rect.draw_text.is_some() {
            let rect_prefix = format!("{prefix}.runs[{semantic_run_idx}].control.textbox");
            collect_raw_rect_slots(xml, rect_span, rect, &rect_prefix, sink)?;
            semantic_run_idx += 1;
        }
    }

    let line_spans = collect_direct_child_outer_spans(xml, run_span.clone(), b"hp:line")?;
    if line_spans.len() != run.lines.len() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "line count mismatch while building preservation metadata for {prefix}: raw={} semantic={}",
                line_spans.len(),
                run.lines.len()
            ),
        });
    }
    for (line, line_span) in run.lines.iter().zip(line_spans.into_iter()) {
        let line_prefix = format!("{prefix}.runs[{semantic_run_idx}].control.line");
        collect_raw_line_slots(xml, line_span, line, &line_prefix, sink)?;
        semantic_run_idx += 1;
    }

    let ellipse_spans = collect_direct_child_outer_spans(xml, run_span.clone(), b"hp:ellipse")?;
    if ellipse_spans.len() != run.ellipses.len() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "ellipse count mismatch while building preservation metadata for {prefix}: raw={} semantic={}",
                ellipse_spans.len(),
                run.ellipses.len()
            ),
        });
    }
    for (ellipse, ellipse_span) in run.ellipses.iter().zip(ellipse_spans.into_iter()) {
        let control_name = if ellipse.has_arc_pr == 1 { "arc" } else { "ellipse" };
        let ellipse_prefix = format!("{prefix}.runs[{semantic_run_idx}].control.{control_name}");
        collect_raw_ellipse_slots(xml, ellipse_span, ellipse, control_name, &ellipse_prefix, sink)?;
        semantic_run_idx += 1;
    }

    let polygon_spans = collect_direct_child_outer_spans(xml, run_span.clone(), b"hp:polygon")?;
    if polygon_spans.len() != run.polygons.len() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "polygon count mismatch while building preservation metadata for {prefix}: raw={} semantic={}",
                polygon_spans.len(),
                run.polygons.len()
            ),
        });
    }
    for (polygon, polygon_span) in run.polygons.iter().zip(polygon_spans.into_iter()) {
        let polygon_prefix = format!("{prefix}.runs[{semantic_run_idx}].control.polygon");
        collect_raw_polygon_slots(xml, polygon_span, polygon, &polygon_prefix, sink)?;
        semantic_run_idx += 1;
    }

    let curve_spans = collect_direct_child_outer_spans(xml, run_span.clone(), b"hp:curve")?;
    if curve_spans.len() != run.curves.len() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "curve count mismatch while building preservation metadata for {prefix}: raw={} semantic={}",
                curve_spans.len(),
                run.curves.len()
            ),
        });
    }
    for (curve, curve_span) in run.curves.iter().zip(curve_spans.into_iter()) {
        let curve_prefix = format!("{prefix}.runs[{semantic_run_idx}].control.curve");
        collect_raw_curve_slots(xml, curve_span, curve, &curve_prefix, sink)?;
        semantic_run_idx += 1;
    }

    let connect_line_spans =
        collect_direct_child_outer_spans(xml, run_span.clone(), b"hp:connectLine")?;
    if connect_line_spans.len() != run.connect_lines.len() {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "connectLine count mismatch while building preservation metadata for {prefix}: raw={} semantic={}",
                connect_line_spans.len(),
                run.connect_lines.len()
            ),
        });
    }
    for (connect_line, connect_line_span) in
        run.connect_lines.iter().zip(connect_line_spans.into_iter())
    {
        let connect_line_prefix = format!("{prefix}.runs[{semantic_run_idx}].control.connect_line");
        collect_raw_connect_line_slots(
            xml,
            connect_line_span,
            connect_line,
            &connect_line_prefix,
            sink,
        )?;
        semantic_run_idx += 1;
    }

    semantic_run_idx += run.equations.len();
    semantic_run_idx += run
        .switches
        .iter()
        .filter(|switch_case| {
            switch_case.case.as_ref().and_then(|case| case.chart.as_ref()).is_some()
        })
        .count();
    semantic_run_idx += run.dutmals.len();
    semantic_run_idx += run.composes.len();

    Ok(semantic_run_idx)
}

fn collect_raw_table_slots(
    xml: &str,
    table_span: Range<usize>,
    table: &HxTable,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
) -> HwpxResult<()> {
    if let Some(caption) = &table.caption {
        let caption_span =
            single_optional_direct_outer_span(xml, table_span.clone(), b"hp:caption")?.ok_or_else(
                || HwpxError::InvalidStructure {
                    detail: format!("table caption span missing for {prefix}"),
                },
            )?;
        collect_raw_caption_slots(xml, caption_span, caption, prefix, sink)?;
    }

    let row_spans = collect_direct_child_outer_spans(xml, table_span, b"hp:tr")?;
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
        collect_raw_row_slots(xml, row_span, row, &format!("{prefix}.rows[{row_idx}]"), sink)?;
    }
    Ok(())
}

fn collect_raw_row_slots(
    xml: &str,
    row_span: Range<usize>,
    row: &HxTableRow,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
) -> HwpxResult<()> {
    let cell_spans = collect_direct_child_outer_spans(xml, row_span, b"hp:tc")?;
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
        collect_raw_cell_slots(xml, cell_span, cell, &format!("{prefix}.cells[{cell_idx}]"), sink)?;
    }
    Ok(())
}

fn collect_raw_cell_slots(
    xml: &str,
    cell_span: Range<usize>,
    cell: &HxTableCell,
    prefix: &str,
    sink: &mut RawSlotSink<'_>,
) -> HwpxResult<()> {
    let Some(sub_list) = &cell.sub_list else {
        return Ok(());
    };
    let sub_list_span = single_optional_direct_outer_span(xml, cell_span, b"hp:subList")?
        .ok_or_else(|| HwpxError::InvalidStructure {
            detail: format!("cell subList span missing for {prefix}"),
        })?;
    collect_raw_sublist_slots(
        xml,
        sub_list_span,
        sub_list,
        &format!("{prefix}.paragraphs"),
        sink,
        false,
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
        let caption_span = single_optional_direct_outer_span(xml, picture_span, b"hp:caption")?
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
) -> HwpxResult<()> {
    if let Some(draw_text) = &rect.draw_text {
        let draw_text_span =
            single_optional_direct_outer_span(xml, rect_span.clone(), b"hp:drawText")?.ok_or_else(
                || HwpxError::InvalidStructure {
                    detail: format!("textbox drawText span missing for {prefix}"),
                },
            )?;
        collect_raw_draw_text_slots(xml, draw_text_span, draw_text, prefix, sink)?;
    }
    if let Some(caption) = &rect.caption {
        let caption_span = single_optional_direct_outer_span(xml, rect_span, b"hp:caption")?
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
        let caption_span = single_optional_direct_outer_span(xml, line_span, b"hp:caption")?
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
                single_optional_direct_outer_span(xml, ellipse_span.clone(), b"hp:drawText")?
                    .ok_or_else(|| HwpxError::InvalidStructure {
                        detail: format!("ellipse drawText span missing for {prefix}"),
                    })?;
            collect_raw_draw_text_slots(xml, draw_text_span, draw_text, prefix, sink)?;
        }
    }
    if let Some(caption) = &ellipse.caption {
        let caption_span = single_optional_direct_outer_span(xml, ellipse_span, b"hp:caption")?
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
            single_optional_direct_outer_span(xml, polygon_span.clone(), b"hp:drawText")?
                .ok_or_else(|| HwpxError::InvalidStructure {
                    detail: format!("polygon drawText span missing for {prefix}"),
                })?;
        collect_raw_draw_text_slots(xml, draw_text_span, draw_text, prefix, sink)?;
    }
    if let Some(caption) = &polygon.caption {
        let caption_span = single_optional_direct_outer_span(xml, polygon_span, b"hp:caption")?
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
        let caption_span = single_optional_direct_outer_span(xml, curve_span, b"hp:caption")?
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
        let caption_span =
            single_optional_direct_outer_span(xml, connect_line_span, b"hp:caption")?.ok_or_else(
                || HwpxError::InvalidStructure {
                    detail: format!("connect line caption span missing for {prefix}"),
                },
            )?;
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
) -> HwpxResult<()> {
    let sub_list_span = single_optional_direct_outer_span(xml, draw_text_span, b"hp:subList")?
        .ok_or_else(|| HwpxError::InvalidStructure {
            detail: format!("drawText subList span missing for {prefix}"),
        })?;
    collect_raw_sublist_slots(
        xml,
        sub_list_span,
        &draw_text.sub_list,
        &format!("{prefix}.paragraphs"),
        sink,
        false,
    )
}

fn collect_raw_header_footer_slots(
    xml: &str,
    ctrl_span: Range<usize>,
    value: &HxHeaderFooter,
    prefix: &str,
    slots: &mut Vec<PreservedTextSlot>,
) -> HwpxResult<()> {
    let tag = if prefix == "header" { b"hp:header".as_slice() } else { b"hp:footer".as_slice() };
    let header_footer_span =
        single_optional_direct_outer_span(xml, ctrl_span, tag)?.ok_or_else(|| {
            HwpxError::InvalidStructure {
                detail: format!("{prefix} span missing while building preservation metadata"),
            }
        })?;
    let Some(sub_list) = &value.sub_list else {
        return Ok(());
    };
    let sub_list_span = single_optional_direct_outer_span(xml, header_footer_span, b"hp:subList")?
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
    let tag = if prefix.ends_with(".footnote") {
        b"hp:footNote".as_slice()
    } else {
        b"hp:endNote".as_slice()
    };
    let note_span = single_optional_direct_outer_span(xml, ctrl_span, tag)?.ok_or_else(|| {
        HwpxError::InvalidStructure { detail: format!("note span missing for {prefix}") }
    })?;
    let sub_list_span = single_optional_direct_outer_span(xml, note_span, b"hp:subList")?
        .ok_or_else(|| HwpxError::InvalidStructure {
            detail: format!("note subList span missing for {prefix}"),
        })?;
    collect_raw_sublist_slots(
        xml,
        sub_list_span,
        &note.sub_list,
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
    let sub_list_span = single_optional_direct_outer_span(xml, caption_span, b"hp:subList")?
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

fn find_root_span(xml: &str, tag: &[u8]) -> HwpxResult<Range<usize>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf: Vec<u8> = Vec::new();
    let mut root_start: Option<usize> = None;
    let mut depth: usize = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) if event.name().as_ref() == tag => {
                let end = buffer_position(&reader)?;
                let start = event_start(end, (&event as &[u8]).len(), false)?;
                if root_start.is_none() {
                    root_start = Some(start);
                }
                depth += 1;
            }
            Ok(Event::Empty(event)) if event.name().as_ref() == tag => {
                let end = buffer_position(&reader)?;
                let start = event_start(end, (&event as &[u8]).len(), true)?;
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

    Err(HwpxError::InvalidStructure {
        detail: format!("root element '{}' not found", String::from_utf8_lossy(tag)),
    })
}

fn collect_direct_child_outer_spans(
    xml: &str,
    parent_span: Range<usize>,
    tag: &[u8],
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
                    let start =
                        parent_span.start + event_start(end, (&event as &[u8]).len(), false)?;
                    results.push(start..0);
                    open_indices.push(results.len() - 1);
                }
                depth += 1;
            }
            Ok(Event::Empty(event)) => {
                if depth == 1 && event.name().as_ref() == tag {
                    let end = parent_span.start + buffer_position(&reader)?;
                    let start = parent_span.start
                        + event_start(end - parent_span.start, (&event as &[u8]).len(), true)?;
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
    tag: &[u8],
) -> HwpxResult<Option<Range<usize>>> {
    let spans = collect_direct_child_outer_spans(xml, parent_span, tag)?;
    match spans.len() {
        0 => Ok(None),
        1 => Ok(spans.into_iter().next()),
        count => Err(HwpxError::InvalidStructure {
            detail: format!(
                "expected at most one direct '{}' child but found {}",
                String::from_utf8_lossy(tag),
                count
            ),
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
                if depth == 1 && event.name().as_ref() == b"hp:t" {
                    let end_local = buffer_position(&reader)?;
                    let start_local = event_start(end_local, (&event as &[u8]).len(), false)?;
                    results.push(TextElementSpan {
                        element_start: parent_span.start + start_local,
                        element_end: 0,
                        content_start: Some(parent_span.start + end_local),
                        content_end: Some(0),
                        has_inline_markup: false,
                    });
                    open_indices.push(results.len() - 1);
                }
                if event.name().as_ref() != b"hp:t" {
                    if let Some(index) = open_indices.last().copied() {
                        results[index].has_inline_markup = true;
                    }
                }
                depth += 1;
            }
            Ok(Event::Empty(event)) => {
                if depth == 1 && event.name().as_ref() == b"hp:t" {
                    let end_local = buffer_position(&reader)?;
                    let start_local = event_start(end_local, (&event as &[u8]).len(), true)?;
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
                if depth == 2 && event.name().as_ref() == b"hp:t" {
                    let end_local = buffer_position(&reader)?;
                    let close_start_local = end_tag_start(end_local, (&event as &[u8]).len())?;
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

fn event_start(end: usize, raw_len: usize, is_empty: bool) -> HwpxResult<usize> {
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

fn buffer_position(reader: &Reader<&[u8]>) -> HwpxResult<usize> {
    usize::try_from(reader.buffer_position())
        .map_err(|_| HwpxError::Zip("buffer position overflow".into()))
}

fn apply_text_locator(xml: &str, locator: &TextLocator, new_text: &str) -> HwpxResult<String> {
    let mut patched = xml.to_string();
    match locator {
        TextLocator::TextElement { element_start, element_end, content_start, content_end } => {
            if let (Some(start), Some(end)) = (*content_start, *content_end) {
                patched.replace_range(start..end, &encode_text_inner(new_text));
                Ok(patched)
            } else {
                let original = &patched[*element_start..*element_end];
                let replaced = patch_text_element_xml(original, new_text)?;
                patched.replace_range(*element_start..*element_end, &replaced);
                Ok(patched)
            }
        }
        TextLocator::EmptyRun { run_start, run_end } => {
            let original = &patched[*run_start..*run_end];
            let replaced = patch_text_run_xml(original, new_text)?;
            patched.replace_range(*run_start..*run_end, &replaced);
            Ok(patched)
        }
    }
}

fn encode_text_inner(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    let mut encoded = String::new();
    for (index, part) in text.split('\n').enumerate() {
        if index > 0 {
            encoded.push_str("<hp:lineBreak/>");
        }
        encoded.push_str(&escape_xml(part));
    }
    encoded
}

fn patch_text_element_xml(element_xml: &str, new_text: &str) -> HwpxResult<String> {
    let encoded = encode_text_inner(new_text);

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
    let encoded = encode_text_inner(new_text);

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

fn section_path(section_idx: usize) -> String {
    format!("Contents/section{section_idx}.xml")
}

fn sha256_hex(bytes: &[u8]) -> String {
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
    if let Some(header) = section.header.as_mut() {
        redact_header_footer(header);
    }
    if let Some(footer) = section.footer.as_mut() {
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
        Control::Line { caption, .. }
        | Control::Arc { caption, .. }
        | Control::Curve { caption, .. }
        | Control::ConnectLine { caption, .. } => {
            if let Some(caption) = caption.as_mut() {
                redact_caption(caption);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::section::parse_section;
    use std::collections::HashMap;

    #[test]
    fn encode_text_inner_converts_newlines_to_line_breaks() {
        assert_eq!(
            encode_text_inner("line 1\nline 2 & < 3"),
            "line 1<hp:lineBreak/>line 2 &amp; &lt; 3"
        );
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
            header: parsed.header,
            footer: parsed.footer,
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
        assert_eq!(preservation.text_slots.len(), 1);
        assert_eq!(
            preservation.text_slots[0].path,
            "paragraphs[0].runs[0].table.rows[0].cells[0].paragraphs[0].runs[0].text"
        );
        assert_eq!(preservation.text_slots[0].original_text, "cell");
    }

    #[test]
    fn build_section_preservation_ignores_nested_footer_controls() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0">
            <hp:run charPrIDRef="0">
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
                                  <hp:run charPrIDRef="0"/>
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
              </hp:tbl>
            </hp:run>
          </hp:p>
        </hs:sec>
        "#;

        let parsed = parse_section(xml, 0, &HashMap::new()).unwrap();
        assert!(parsed.footer.is_none(), "nested footer must not become section footer");

        let section = Section {
            paragraphs: parsed.paragraphs,
            page_settings: parsed.page_settings.unwrap_or_default(),
            header: parsed.header,
            footer: parsed.footer,
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
        assert_eq!(preservation.text_slots.len(), 1);
        assert_eq!(
            preservation.text_slots[0].path,
            "paragraphs[0].runs[0].table.rows[0].cells[0].paragraphs[0].runs[0].text"
        );
        assert!(!preservation.text_slots.iter().any(|slot| slot.path.starts_with("footer.")));
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
            header: parsed.header,
            footer: parsed.footer,
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
        assert_eq!(preservation.text_slots.len(), 1);
        assert_eq!(
            preservation.text_slots[0].path,
            "paragraphs[0].runs[0].control.textbox.paragraphs[0].runs[0].text"
        );
    }
}
