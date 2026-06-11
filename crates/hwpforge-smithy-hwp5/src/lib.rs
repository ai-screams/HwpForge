//! HWP5 binary format decoder and semantic audit surface for HwpForge.
//!
//! This crate reads HWP5 files (OLE2 compound binary format, `.hwp`),
//! converting them into HwpForge Core's document types so they can be
//! re-encoded as HWPX or Markdown. It also exposes a semantic IR and
//! parser-only audit contracts used to validate structural reconstruction
//! before Core or HWPX projection is involved.
//!
//! # Architecture
//!
//! **Decoding** (HWP5 → Core):
//! 1. Open OLE2 container with `cfb`, locate streams
//! 2. Parse `FileHeader` → version, flags, password status
//! 3. Decompress DEFLATE-compressed streams (`flate2`)
//! 4. Read binary records (`schema`) — tag-length-value format
//! 5. Parse `DocInfo` stream → style definitions (`Hwp5StyleStore`)
//! 6. Parse `BodyText/Section{N}` streams → paragraphs
//! 7. Materialize semantic/audit contracts for structure-first validation
//! 8. Assemble `Document<Draft>` via projection layer
//!
//! # Quick Start
//!
//! ```no_run
//! use hwpforge_smithy_hwp5::Hwp5Decoder;
//!
//! let bytes = std::fs::read("document.hwp").unwrap();
//! let result = Hwp5Decoder::decode(&bytes).unwrap();
//! println!("Sections: {}", result.document.sections().len());
//! ```
//!
//! # Supported Content
//!
//! Currently supports T1 (text + styles), T2 (tables), and a narrow parser-backed
//! image slice covering `gso ` + `ShapePicture` anchored in body/header/footer/
//! table/textbox subtrees.

#![deny(missing_docs)]
#![deny(unsafe_code)]

mod ctrl_ids;
pub mod decoder;
pub mod error;
mod layout_hint_patch;
mod numeric;
pub mod projection;
pub mod schema;
pub mod semantic;
mod semantic_adapter;
pub mod style_store;
mod style_store_border_fill;
mod style_store_convert;
mod table_cell_vertical_align;
mod table_page_break;
#[cfg(test)]
/// Test-only helpers for resolving shared workspace fixtures.
pub(crate) mod test_support;
mod warning_utils;

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::numeric::positive_i32_from_u32;
use crate::warning_utils::push_projection_fallback;
use hwpforge_core::document::{Document, Draft};
use hwpforge_core::image::ImageStore;

pub use decoder::{Hwp5Decoder, Hwp5Document, Hwp5Warning};
pub use error::{Hwp5Error, Hwp5ErrorCode, Hwp5Result};
pub use semantic::{
    Hwp5ParserAuditContainerControlCount, Hwp5ParserAuditContainerCount,
    Hwp5ParserAuditControlCount, Hwp5ParserAuditOptionalContainerCount, Hwp5ParserAuditSection,
    Hwp5ParserAuditSnapshot, Hwp5SemanticConfidence, Hwp5SemanticContainerKind,
    Hwp5SemanticContainerPath, Hwp5SemanticControlEdge, Hwp5SemanticControlEdgeKind,
    Hwp5SemanticControlId, Hwp5SemanticControlKind, Hwp5SemanticControlNode,
    Hwp5SemanticControlPayload, Hwp5SemanticDocInfo, Hwp5SemanticDocument,
    Hwp5SemanticGraphIntegrityIssue, Hwp5SemanticImageFormat, Hwp5SemanticImagePayload,
    Hwp5SemanticNamedStyleRef, Hwp5SemanticOlePayload, Hwp5SemanticPackageMeta,
    Hwp5SemanticParagraph, Hwp5SemanticParagraphId, Hwp5SemanticSection, Hwp5SemanticSectionId,
    Hwp5SemanticTableCellEvidence, Hwp5SemanticTableCellMargin, Hwp5SemanticTableCellVerticalAlign,
    Hwp5SemanticTablePageBreak, Hwp5SemanticTablePayload, Hwp5SemanticUnresolvedId,
    Hwp5UnresolvedItem, Hwp5UnresolvedReason,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hwp5JoinedImageAsset {
    pub payload: Hwp5SemanticImagePayload,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hwp5JoinedImageAssetPlan {
    pub ordered_assets: Vec<Hwp5JoinedImageAsset>,
    pub assets_by_binary_data_id: BTreeMap<u16, Hwp5JoinedImageAsset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Hwp5ImageGeometryHint {
    width_hwp: i32,
    height_hwp: i32,
}

impl Hwp5JoinedImageAssetPlan {
    pub(crate) fn asset_for_binary_data_id(
        &self,
        binary_data_id: u16,
    ) -> Option<&Hwp5JoinedImageAsset> {
        self.assets_by_binary_data_id.get(&binary_data_id)
    }
}

/// Per-document plan of OLE-backed BinData entries, keyed by `binary_data_id`.
///
/// This carries the raw (still DEFLATE-compressed) `/BinData/BIN*.OLE` bytes
/// so the projection layer can attempt chart extraction without re-opening
/// the source CFB. Non-OLE entries are excluded; image entries are handled
/// separately via [`Hwp5JoinedImageAssetPlan`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Hwp5OleAssetPlan {
    /// Raw `/BinData/*` bytes by `binary_data_id`. Always DEFLATE-compressed
    /// (HWP5 OLE entries set `should_decompress=true`); the consumer
    /// (`decoder::chart_ole::extract_chart_payload`) handles inflation.
    pub assets_by_binary_data_id: BTreeMap<u16, Vec<u8>>,
}

impl Hwp5OleAssetPlan {
    pub(crate) fn bytes_for_binary_data_id(&self, binary_data_id: u16) -> Option<&[u8]> {
        self.assets_by_binary_data_id.get(&binary_data_id).map(|v| v.as_slice())
    }
}

/// Inspect summary for an HWP5 source document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwp5InspectSummary {
    /// HWP5 file format version (for example, `5.0.2.5`).
    pub version: String,
    /// Number of non-fatal warnings emitted while decoding and projecting
    /// inspectable style/Core semantics.
    pub warning_count: usize,
    /// Validation issue encountered after projection, if any.
    pub validation_error: Option<String>,
    /// DocInfo-derived style and font counts.
    pub doc_info: Hwp5DocInfoSummary,
    /// Aggregate projected document counts.
    pub totals: Hwp5DocumentSummary,
    /// Per-section projected summaries.
    pub sections: Vec<Hwp5SectionSummary>,
}

/// DocInfo-level counts extracted from the HWP5 binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwp5DocInfoSummary {
    /// Total number of raw `FaceName` records found in `DocInfo`.
    pub font_faces: usize,
    /// Per-language font bucket counts from `IdMappings`, when available.
    pub font_buckets: Option<Hwp5FontBucketSummary>,
    /// Number of character shape records.
    pub char_shapes: usize,
    /// Number of paragraph shape records.
    pub para_shapes: usize,
    /// Number of named style records.
    pub styles: usize,
}

/// Per-language font bucket counts for an HWP5 document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwp5FontBucketSummary {
    /// Number of Hangul fonts.
    pub hangul: usize,
    /// Number of Latin fonts.
    pub latin: usize,
    /// Number of Hanja fonts.
    pub hanja: usize,
    /// Number of Japanese fonts.
    pub japanese: usize,
    /// Number of Other-script fonts.
    pub other: usize,
    /// Number of Symbol fonts.
    pub symbol: usize,
    /// Number of User-defined fonts.
    pub user: usize,
}

/// Aggregate projected document counts for HWP5 inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwp5DocumentSummary {
    /// Number of sections in the projected document.
    pub sections: usize,
    /// Total paragraph count across every section.
    pub paragraphs: usize,
    /// Number of non-empty paragraphs across every section.
    pub non_empty_paragraphs: usize,
    /// Number of projected tables across every section.
    pub tables: usize,
    /// Number of sections with headers.
    pub headers: usize,
    /// Number of sections with footers.
    pub footers: usize,
    /// Number of sections with page numbers.
    pub page_numbers: usize,
    /// Number of sections marked landscape.
    pub landscape_sections: usize,
}

/// Projected section summary for HWP5 inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hwp5SectionSummary {
    /// Zero-based section index.
    pub index: usize,
    /// Number of paragraphs in the section.
    pub paragraphs: usize,
    /// Number of non-empty paragraphs in the section.
    pub non_empty_paragraphs: usize,
    /// Number of projected tables in the section.
    pub tables: usize,
    /// Whether the section has a header.
    pub has_header: bool,
    /// Whether the section has a footer.
    pub has_footer: bool,
    /// Whether the section has a page number.
    pub has_page_number: bool,
    /// Whether the section uses landscape page settings.
    pub landscape: bool,
    /// First non-empty paragraph text, if any.
    pub first_non_empty_text: Option<String>,
}

/// Raw fixture census for an HWP5 package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5CensusReport {
    /// HWP5 file format version (for example, `5.1.1.0`).
    pub version: String,
    /// Whether BodyText and DocInfo streams are document-level compressed.
    pub compressed: bool,
    /// All package entries discovered in the CFB container.
    pub package_entries: Vec<Hwp5PackageEntry>,
    /// Raw DocInfo stream inventory.
    pub doc_info: Hwp5StreamCensus,
    /// Raw BodyText section inventories.
    pub sections: Vec<Hwp5SectionCensus>,
    /// `/BinData/*` stream inventory from the package.
    pub bin_data_streams: Vec<Hwp5BinDataStream>,
}

/// Metadata for a single package entry in an HWP5 CFB container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5PackageEntry {
    /// Full CFB path (for example, `/BodyText/Section0`).
    pub path: String,
    /// Entry kind.
    pub kind: Hwp5PackageEntryKind,
    /// Raw size in bytes from the CFB directory entry.
    pub size: u64,
}

/// Entry type inside an HWP5 CFB container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Hwp5PackageEntryKind {
    /// The root storage object.
    Root,
    /// A storage/directory entry.
    Storage,
    /// A stream/file entry.
    Stream,
}

/// Raw record census for one decompressed HWP5 stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5StreamCensus {
    /// Decompressed byte length used for record parsing.
    pub decoded_size_bytes: usize,
    /// Number of TLV records found in the stream.
    pub record_count: usize,
    /// Aggregated record counts by tag.
    pub tag_counts: Vec<Hwp5TagCount>,
    /// Parsed `BinData` records for streams that contain them.
    pub bin_data_records: Vec<Hwp5BinDataRecordSummary>,
}

/// Raw record census for one BodyText section stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5SectionCensus {
    /// Zero-based section index.
    pub index: usize,
    /// Decompressed byte length used for record parsing.
    pub decoded_size_bytes: usize,
    /// Number of TLV records found in the stream.
    pub record_count: usize,
    /// Aggregated record counts by tag.
    pub tag_counts: Vec<Hwp5TagCount>,
    /// `CtrlHeader` IDs seen in the section.
    pub ctrl_ids: Vec<Hwp5CtrlIdCount>,
}

/// Count of a single tag ID in a decompressed HWP5 stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5TagCount {
    /// Numeric tag ID from the record header.
    pub tag_id: u16,
    /// Debug-style tag name (`ParaHeader`, `BinData`, `Unknown(0x999)`...).
    pub tag_name: String,
    /// Number of occurrences.
    pub count: usize,
}

/// Count of one `ctrl_id` in a BodyText section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5CtrlIdCount {
    /// Hex-encoded canonical control ID (`0x74626C20`).
    pub ctrl_id_hex: String,
    /// Printable ASCII rendering when available (`tbl `).
    pub ctrl_id_ascii: String,
    /// Number of `CtrlHeader` records with this ID.
    pub count: usize,
    /// Distinct record nesting levels at which the control occurred.
    pub record_levels: Vec<u16>,
}

/// Summary for a single `/BinData/*` stream entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5BinDataStream {
    /// Stream name relative to `/BinData/`.
    pub name: String,
    /// Raw byte length from the package.
    pub size_bytes: usize,
}

/// Summary for a single `DocInfo/BinData` record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5BinDataRecordSummary {
    /// 1-based binary item ID.
    pub binary_data_id: u16,
    /// Expected `/BinData/*` storage name derived from the record.
    pub storage_name: String,
    /// File extension from the record payload.
    pub extension: String,
    /// Storage mode (`Embedding`, `Link`, ...).
    pub data_type: String,
    /// Compression mode (`Default`, `Compress`, ...).
    pub compression: String,
    /// Internal decode hint telling image/OLE join paths whether the raw
    /// `/BinData/*` stream must be DEFLATE-decoded before use.
    #[serde(skip_serializing)]
    pub(crate) should_decompress: bool,
}

/// Inspects an HWP5 document from bytes and returns a compact audit summary.
///
/// This is a decode-side helper for tools that need to compare source HWP5
/// structure with converted HWPX output without re-parsing private modules.
///
/// # Errors
///
/// Returns [`Hwp5Error`] if the bytes cannot be opened as a valid HWP5
/// compound document or any required stream cannot be decoded.
pub fn inspect_hwp5(bytes: &[u8]) -> Hwp5Result<Hwp5InspectSummary> {
    let intermediate = decoder::decode_intermediate(bytes)?;
    inspect_decoded_hwp5_intermediate(intermediate)
}

fn inspect_decoded_hwp5_intermediate(
    intermediate: decoder::DecodedHwp5Intermediate,
) -> Hwp5Result<Hwp5InspectSummary> {
    let crate::decoder::DecodedHwp5Intermediate { version, sections, doc_info, warnings, .. } =
        intermediate;
    let mut warnings = warnings;
    let (_, _, style_warnings) = project_doc_info_styles_with_warnings(&doc_info);
    warnings.extend(style_warnings);

    let (document, projection_warnings) = projection::project_to_core(sections)?;
    warnings.extend(projection_warnings);

    let sections = summarize_sections(&document);
    let totals = summarize_document(&sections);
    let validation_error = document.validate().err().map(|err| err.to_string());
    let doc_info = Hwp5DocInfoSummary {
        font_faces: doc_info.fonts.len(),
        font_buckets: doc_info.id_mappings.as_ref().map(|m| Hwp5FontBucketSummary {
            hangul: m.hangul_font_count.max(0) as usize,
            latin: m.english_font_count.max(0) as usize,
            hanja: m.hanja_font_count.max(0) as usize,
            japanese: m.japanese_font_count.max(0) as usize,
            other: m.other_font_count.max(0) as usize,
            symbol: m.symbol_font_count.max(0) as usize,
            user: m.user_font_count.max(0) as usize,
        }),
        char_shapes: doc_info.char_shapes.len(),
        para_shapes: doc_info.para_shapes.len(),
        styles: doc_info.styles.len(),
    };

    Ok(Hwp5InspectSummary {
        version,
        warning_count: warnings.len(),
        validation_error,
        doc_info,
        totals,
        sections,
    })
}

fn project_doc_info_styles_with_warnings(
    doc_info: &crate::decoder::header::DocInfoResult,
) -> (style_store::Hwp5StyleStore, hwpforge_smithy_hwpx::HwpxStyleStore, Vec<Hwp5Warning>) {
    let hwp5_styles = style_store::Hwp5StyleStore::from_doc_info(doc_info);
    let (hwpx_style_store, style_warnings) = hwp5_styles.to_hwpx_style_store_with_warnings();
    (hwp5_styles, hwpx_style_store, style_warnings)
}

/// Builds a raw fixture census for an HWP5 document.
///
/// Unlike [`inspect_hwp5`], this function stays close to the binary package
/// structure. It inventories CFB entries, raw TLV tags, control IDs, and
/// `/BinData/*` streams before any projection to Core/HWPX.
///
/// # Errors
///
/// Returns [`Hwp5Error`] if the package cannot be opened or any decompressed
/// stream fails TLV parsing.
pub fn census_hwp5(bytes: &[u8]) -> Hwp5Result<Hwp5CensusReport> {
    use std::collections::{BTreeMap, BTreeSet};
    use std::io::Cursor;

    use decoder::package::PackageReader;
    use schema::record::{Record, TagId};

    let pkg = PackageReader::open(bytes)?;
    let package_entries = collect_package_entries(bytes)?;

    let doc_info_records = Record::parse_stream(&mut Cursor::new(pkg.doc_info_data()))?;
    let doc_info = Hwp5StreamCensus {
        decoded_size_bytes: pkg.doc_info_data().len(),
        record_count: doc_info_records.len(),
        tag_counts: summarize_tag_counts(&doc_info_records),
        bin_data_records: Vec::new(),
    };

    let mut sections = Vec::with_capacity(pkg.sections_data().len());
    for (index, section_data) in pkg.sections_data().iter().enumerate() {
        let records = Record::parse_stream(&mut Cursor::new(section_data))?;
        let mut ctrl_counts: BTreeMap<u32, (usize, BTreeSet<u16>)> = BTreeMap::new();
        for record in &records {
            if matches!(TagId::from(record.header.tag_id), TagId::CtrlHeader) {
                let ctrl_id = parse_ctrl_id(&record.data);
                let entry = ctrl_counts.entry(ctrl_id).or_insert_with(|| (0, BTreeSet::new()));
                entry.0 += 1;
                entry.1.insert(record.header.level);
            }
        }

        sections.push(Hwp5SectionCensus {
            index,
            decoded_size_bytes: section_data.len(),
            record_count: records.len(),
            tag_counts: summarize_tag_counts(&records),
            ctrl_ids: ctrl_counts
                .into_iter()
                .map(|(ctrl_id, (count, levels))| Hwp5CtrlIdCount {
                    ctrl_id_hex: format!("0x{ctrl_id:08X}"),
                    ctrl_id_ascii: ctrl_id_ascii(ctrl_id),
                    count,
                    record_levels: levels.into_iter().collect(),
                })
                .collect(),
        });
    }

    let bin_data_records = summarize_doc_info_bin_data_records(
        pkg.doc_info_data(),
        pkg.file_header().flags.compressed,
    )?;
    let bin_data_streams = summarize_package_bin_data_streams(&pkg);

    Ok(Hwp5CensusReport {
        version: pkg.file_header().version.to_string(),
        compressed: pkg.file_header().flags.compressed,
        package_entries,
        doc_info: Hwp5StreamCensus {
            decoded_size_bytes: doc_info.decoded_size_bytes,
            record_count: doc_info.record_count,
            tag_counts: doc_info.tag_counts,
            bin_data_records,
        },
        sections,
        bin_data_streams,
    })
}

/// Builds a raw fixture census for an HWP5 document on disk.
///
/// # Errors
///
/// Returns [`Hwp5Error`] if the file cannot be read or decoded.
pub fn census_hwp5_file(path: impl AsRef<Path>) -> Hwp5Result<Hwp5CensusReport> {
    let bytes = std::fs::read(path.as_ref()).map_err(Hwp5Error::Io)?;
    census_hwp5(&bytes)
}

/// Inspects an HWP5 document from a filesystem path.
///
/// # Errors
///
/// Returns [`Hwp5Error`] if the file cannot be read or decoded.
pub fn inspect_hwp5_file(path: impl AsRef<Path>) -> Hwp5Result<Hwp5InspectSummary> {
    let bytes = std::fs::read(path.as_ref()).map_err(Hwp5Error::Io)?;
    inspect_hwp5(&bytes)
}

/// Builds the current semantic HWP5 document from raw bytes.
///
/// This helper exposes the parser-side semantic reconstruction before any
/// Core or HWPX projection is involved. The current semantic slice is
/// intentionally limited to package metadata, DocInfo references, structural
/// subtrees, and the current narrow semantic image slice.
///
/// # Errors
///
/// Returns [`Hwp5Error`] if the package cannot be opened or required streams
/// cannot be decoded.
pub fn build_hwp5_semantic(bytes: &[u8]) -> Hwp5Result<Hwp5SemanticDocument> {
    let decoded = decoder::decode_intermediate(bytes)?;
    let image_assets = join_hwp5_image_assets(bytes, &decoded)?;
    Ok(semantic_adapter::adapt_to_semantic(&decoded, &image_assets))
}

/// Builds the current semantic HWP5 document from a filesystem path.
///
/// # Errors
///
/// Returns [`Hwp5Error`] if the file cannot be read or decoded.
pub fn build_hwp5_semantic_file(path: impl AsRef<Path>) -> Hwp5Result<Hwp5SemanticDocument> {
    let bytes = std::fs::read(path.as_ref()).map_err(Hwp5Error::Io)?;
    build_hwp5_semantic(&bytes)
}

/// Decodes an HWP5 document with the current image slice enabled.
///
/// Unlike [`Hwp5Decoder::decode`], this helper populates `ImageStore` from
/// joined HWP5 `BinData` evidence and projects paragraph-local image runs in
/// the current narrow slice.
///
/// # Errors
///
/// Returns [`Hwp5Error`] if package decoding, image asset joining, or Core
/// projection fails.
pub fn decode_hwp5_with_images(bytes: &[u8]) -> Hwp5Result<Hwp5Document> {
    let intermediate = decoder::decode_intermediate(bytes)?;
    let image_assets = join_hwp5_image_assets(bytes, &intermediate)?;
    let mut warnings = intermediate.warnings;
    let metadata = intermediate.metadata;
    let (mut document, image_store, proj_warnings) =
        projection::project_to_core_with_images(intermediate.sections, &image_assets)?;
    document.set_metadata(metadata);
    warnings.extend(proj_warnings);

    Ok(Hwp5Document { document, image_store, warnings })
}

/// Decodes an HWP5 file from disk with the current image slice enabled.
///
/// # Errors
///
/// Returns [`Hwp5Error`] if the file cannot be read or decoded.
pub fn decode_hwp5_with_images_file(path: impl AsRef<Path>) -> Hwp5Result<Hwp5Document> {
    let bytes = std::fs::read(path.as_ref()).map_err(Hwp5Error::Io)?;
    decode_hwp5_with_images(&bytes)
}

/// Converts an HWP5 file to HWPX format.
///
/// This is the primary convenience function for HWP5 → HWPX conversion.
/// Internally it decodes the HWP5 binary, builds a style store, validates
/// the document, and re-encodes as HWPX.
///
/// # Examples
///
/// ```no_run
/// use hwpforge_smithy_hwp5::hwp5_to_hwpx;
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
/// use hwpforge_smithy_hwp5::hwp5_to_hwpx_bytes;
///
/// let hwp5_bytes = std::fs::read("input.hwp").unwrap();
/// let (hwpx_bytes, warnings) = hwp5_to_hwpx_bytes(&hwp5_bytes).unwrap();
/// println!("Produced {} bytes with {} warnings", hwpx_bytes.len(), warnings.len());
/// ```
pub fn hwp5_to_hwpx_bytes(bytes: &[u8]) -> Hwp5Result<(Vec<u8>, Vec<Hwp5Warning>)> {
    let intermediate = decoder::decode_intermediate(bytes)?;
    let image_assets = join_hwp5_image_assets(bytes, &intermediate)?;
    let ole_assets = join_hwp5_ole_assets(bytes, &intermediate)?;
    let layout_hints = layout_hint_patch::capture_layout_hints(&intermediate.sections);
    let mut warnings = intermediate.warnings;
    // Wave 12o Phase 3 — forward HWP5 SummaryInformation metadata into
    // the projected Core Document so the downstream HWPX encoder emits
    // a populated `<opf:metadata>` block.
    let metadata = intermediate.metadata;

    let (hwp5_styles, hwpx_style_store, style_warnings) =
        project_doc_info_styles_with_warnings(&intermediate.doc_info);
    warnings.extend(style_warnings);

    let (mut document, mut image_store, proj_warnings) =
        projection::project_to_core_with_images_and_ole(
            intermediate.sections,
            &image_assets,
            &ole_assets,
        )?;
    document.set_metadata(metadata);
    warnings.extend(proj_warnings);
    supplement_border_fill_image_assets(
        &hwp5_styles,
        &image_assets,
        &mut image_store,
        &mut warnings,
    );

    let validated = document.validate().map_err(Hwp5Error::Core)?;
    let hwpx_bytes =
        hwpforge_smithy_hwpx::HwpxEncoder::encode(&validated, &hwpx_style_store, &image_store)
            .map_err(|e| Hwp5Error::Cfb { detail: format!("HWPX encoding failed: {e}") })?;
    let hwpx_bytes = layout_hint_patch::patch_hwpx_layout_hints(&hwpx_bytes, &layout_hints)?;

    Ok((hwpx_bytes, warnings))
}

fn supplement_border_fill_image_assets(
    hwp5_styles: &style_store::Hwp5StyleStore,
    image_assets: &Hwp5JoinedImageAssetPlan,
    image_store: &mut ImageStore,
    warnings: &mut Vec<Hwp5Warning>,
) {
    for binary_data_id in hwp5_styles.border_fill_image_binary_ids() {
        let Some(asset) = image_assets.asset_for_binary_data_id(binary_data_id) else {
            push_projection_fallback(
                warnings,
                "style.border_fill.image",
                format!("missing_image_asset_for_binary_data_id={binary_data_id}"),
            );
            continue;
        };
        image_store.insert(asset.payload.storage_name.clone(), asset.bytes.clone());
    }
}

fn join_hwp5_image_assets(
    bytes: &[u8],
    intermediate: &decoder::DecodedHwp5Intermediate,
) -> Hwp5Result<Hwp5JoinedImageAssetPlan> {
    use decoder::package::PackageReader;

    let pkg = PackageReader::open(bytes)?;
    let geometry_hints: BTreeMap<u16, Hwp5ImageGeometryHint> =
        collect_image_geometry_hints(&intermediate.sections);
    let mut ordered_assets: Vec<Hwp5JoinedImageAsset> = Vec::new();
    let mut assets_by_binary_data_id: BTreeMap<u16, Hwp5JoinedImageAsset> = BTreeMap::new();

    for record in &intermediate.bin_data_records {
        let extension = record.extension.to_ascii_lowercase();
        if !is_supported_image_extension(&extension) {
            continue;
        }

        let Some(raw_data) = pkg.bin_data().get(&record.storage_name) else {
            continue;
        };
        let data: Vec<u8> = decode_bin_data_payload(raw_data, record, &record.storage_name)?;

        let asset = Hwp5JoinedImageAsset {
            payload: Hwp5SemanticImagePayload {
                binary_data_id: record.binary_data_id,
                storage_name: record.storage_name.clone(),
                package_path: format!("BinData/{}", record.storage_name),
                format: semantic_image_format_from_extension(&extension),
                width_hwp: geometry_hints.get(&record.binary_data_id).map(|hint| hint.width_hwp),
                height_hwp: geometry_hints.get(&record.binary_data_id).map(|hint| hint.height_hwp),
            },
            bytes: data,
        };
        assets_by_binary_data_id.insert(record.binary_data_id, asset.clone());
        ordered_assets.push(asset);
    }

    Ok(Hwp5JoinedImageAssetPlan { ordered_assets, assets_by_binary_data_id })
}

fn join_hwp5_ole_assets(
    bytes: &[u8],
    intermediate: &decoder::DecodedHwp5Intermediate,
) -> Hwp5Result<Hwp5OleAssetPlan> {
    use decoder::package::PackageReader;

    let pkg = PackageReader::open(bytes)?;
    let mut assets_by_binary_data_id: BTreeMap<u16, Vec<u8>> = BTreeMap::new();
    for record in &intermediate.bin_data_records {
        let extension = record.extension.to_ascii_lowercase();
        if extension != "ole" {
            continue;
        }
        let Some(raw_data) = pkg.bin_data().get(&record.storage_name) else {
            continue;
        };
        assets_by_binary_data_id.insert(record.binary_data_id, raw_data.clone());
    }
    Ok(Hwp5OleAssetPlan { assets_by_binary_data_id })
}

fn decode_bin_data_payload(
    raw_data: &[u8],
    record: &Hwp5BinDataRecordSummary,
    stream_name: &str,
) -> Hwp5Result<Vec<u8>> {
    if !record.should_decompress {
        return Ok(raw_data.to_vec());
    }

    decoder::package::decompress_stream(raw_data).map_err(|_| Hwp5Error::RecordParse {
        offset: 0,
        detail: format!("BinData '{stream_name}' decompression failed"),
    })
}

fn collect_image_geometry_hints(
    sections: &[decoder::section::SectionResult],
) -> BTreeMap<u16, Hwp5ImageGeometryHint> {
    let mut hints: BTreeMap<u16, Hwp5ImageGeometryHint> = BTreeMap::new();
    for section in sections {
        collect_image_geometry_hints_in_paragraphs(&section.paragraphs, &mut hints);
    }
    hints
}

fn collect_image_geometry_hints_in_paragraphs(
    paragraphs: &[decoder::section::Hwp5Paragraph],
    hints: &mut BTreeMap<u16, Hwp5ImageGeometryHint>,
) {
    for paragraph in paragraphs {
        collect_image_geometry_hints_in_controls(&paragraph.controls, hints);
    }
}

fn collect_image_geometry_hints_in_controls(
    controls: &[decoder::section::Hwp5Control],
    hints: &mut BTreeMap<u16, Hwp5ImageGeometryHint>,
) {
    for control in controls {
        match control {
            decoder::section::Hwp5Control::Image(image) => {
                record_image_geometry_hint(
                    image.binary_data_id,
                    image.geometry.width,
                    image.geometry.height,
                    hints,
                );
            }
            decoder::section::Hwp5Control::Table(table) => {
                for cell in &table.cells {
                    collect_image_geometry_hints_in_paragraphs(&cell.paragraphs, hints);
                }
            }
            decoder::section::Hwp5Control::Header(subtree)
            | decoder::section::Hwp5Control::Footer(subtree)
            | decoder::section::Hwp5Control::Footnote(subtree)
            | decoder::section::Hwp5Control::Endnote(subtree) => {
                collect_image_geometry_hints_in_paragraphs(&subtree.paragraphs, hints);
            }
            decoder::section::Hwp5Control::TextBox(textbox) => {
                collect_image_geometry_hints_in_paragraphs(&textbox.paragraphs, hints);
            }
            decoder::section::Hwp5Control::Line(_)
            | decoder::section::Hwp5Control::Rect(_)
            | decoder::section::Hwp5Control::Polygon(_)
            | decoder::section::Hwp5Control::Ellipse(_)
            | decoder::section::Hwp5Control::Arc(_)
            | decoder::section::Hwp5Control::Curve(_)
            | decoder::section::Hwp5Control::ConnectLine(_)
            | decoder::section::Hwp5Control::Equation(_)
            | decoder::section::Hwp5Control::Memo(_)
            | decoder::section::Hwp5Control::Dutmal(_)
            | decoder::section::Hwp5Control::Compose(_)
            | decoder::section::Hwp5Control::IndexMark(_)
            | decoder::section::Hwp5Control::ClickHere(_)
            | decoder::section::Hwp5Control::SummeryField(_)
            | decoder::section::Hwp5Control::DateCodeField(_)
            | decoder::section::Hwp5Control::PathField(_)
            | decoder::section::Hwp5Control::CrossRef(_)
            | decoder::section::Hwp5Control::InlinePageNumber(_)
            | decoder::section::Hwp5Control::OleObject(_)
            | decoder::section::Hwp5Control::Unknown { .. } => {}
        }
    }
}

fn record_image_geometry_hint(
    binary_data_id: u16,
    width_hwp: u32,
    height_hwp: u32,
    hints: &mut BTreeMap<u16, Hwp5ImageGeometryHint>,
) {
    let Some(width_hwp): Option<i32> = positive_i32_from_u32(width_hwp) else {
        return;
    };
    let Some(height_hwp): Option<i32> = positive_i32_from_u32(height_hwp) else {
        return;
    };
    hints.entry(binary_data_id).or_insert(Hwp5ImageGeometryHint { width_hwp, height_hwp });
}

fn is_supported_image_extension(extension: &str) -> bool {
    matches!(extension, "png" | "jpg" | "jpeg" | "gif" | "bmp" | "wmf" | "emf")
}

fn semantic_image_format_from_extension(extension: &str) -> Hwp5SemanticImageFormat {
    match extension {
        "png" => Hwp5SemanticImageFormat::Png,
        "jpg" | "jpeg" => Hwp5SemanticImageFormat::Jpeg,
        "gif" => Hwp5SemanticImageFormat::Gif,
        "bmp" => Hwp5SemanticImageFormat::Bmp,
        "wmf" => Hwp5SemanticImageFormat::Wmf,
        "emf" => Hwp5SemanticImageFormat::Emf,
        other => Hwp5SemanticImageFormat::Unknown(other.to_string()),
    }
}

fn summarize_sections(document: &Document<Draft>) -> Vec<Hwp5SectionSummary> {
    document
        .sections()
        .iter()
        .enumerate()
        .map(|(index, section)| {
            let counts = section.content_counts();
            let non_empty_paragraphs = section
                .paragraphs
                .iter()
                .filter(|para| !para.text_content().trim().is_empty())
                .count();
            let first_non_empty_text = first_visible_text_in_paragraphs(&section.paragraphs);

            Hwp5SectionSummary {
                index,
                paragraphs: section.paragraphs.len(),
                non_empty_paragraphs,
                tables: counts.tables,
                has_header: !section.headers.is_empty(),
                has_footer: !section.footers.is_empty(),
                has_page_number: section.page_number.is_some(),
                landscape: section.page_settings.landscape,
                first_non_empty_text,
            }
        })
        .collect()
}

fn collect_package_entries(bytes: &[u8]) -> Hwp5Result<Vec<Hwp5PackageEntry>> {
    let comp =
        cfb::OpenOptions::new().open_with(std::io::Cursor::new(bytes)).map_err(Hwp5Error::Io)?;

    let mut entries: Vec<Hwp5PackageEntry> = comp
        .walk()
        .map(|entry| Hwp5PackageEntry {
            path: entry.path().display().to_string(),
            kind: if entry.is_root() {
                Hwp5PackageEntryKind::Root
            } else if entry.is_storage() {
                Hwp5PackageEntryKind::Storage
            } else {
                Hwp5PackageEntryKind::Stream
            },
            size: entry.len(),
        })
        .collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

fn summarize_doc_info_bin_data_records(
    doc_info_data: &[u8],
    file_is_compressed: bool,
) -> Hwp5Result<Vec<Hwp5BinDataRecordSummary>> {
    use std::io::Cursor;

    use schema::header::Hwp5RawBinData;
    use schema::record::{Record, TagId};

    let records = Record::parse_stream(&mut Cursor::new(doc_info_data))?;
    Ok(records
        .iter()
        .filter(|record| matches!(TagId::from(record.header.tag_id), TagId::BinData))
        .filter_map(|record| Hwp5RawBinData::parse(&record.data).ok())
        .map(|record| Hwp5BinDataRecordSummary {
            binary_data_id: record.binary_data_id,
            storage_name: record.storage_name(),
            extension: record.extension,
            data_type: format!("{:?}", record.data_type),
            compression: format!("{:?}", record.compression),
            should_decompress: record.compression.should_decompress(file_is_compressed),
        })
        .collect())
}

fn summarize_package_bin_data_streams(
    pkg: &decoder::package::PackageReader,
) -> Vec<Hwp5BinDataStream> {
    let mut bin_data_streams: Vec<Hwp5BinDataStream> = pkg
        .bin_data()
        .iter()
        .map(|(name, data)| Hwp5BinDataStream { name: name.clone(), size_bytes: data.len() })
        .collect();
    bin_data_streams.sort_by(|a, b| a.name.cmp(&b.name));
    bin_data_streams
}

fn summarize_tag_counts(records: &[schema::record::Record]) -> Vec<Hwp5TagCount> {
    use std::collections::BTreeMap;

    let mut counts: BTreeMap<u16, usize> = BTreeMap::new();
    for record in records {
        *counts.entry(record.header.tag_id).or_default() += 1;
    }

    counts
        .into_iter()
        .map(|(tag_id, count)| Hwp5TagCount { tag_id, tag_name: tag_name(tag_id), count })
        .collect()
}

fn tag_name(tag_id: u16) -> String {
    match schema::record::TagId::from(tag_id) {
        schema::record::TagId::Unknown(_) => format!("Unknown(0x{tag_id:04X})"),
        tag => format!("{tag:?}"),
    }
}

fn parse_ctrl_id(data: &[u8]) -> u32 {
    if data.len() < 4 {
        return 0;
    }
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

fn ctrl_id_ascii(ctrl_id: u32) -> String {
    let bytes = ctrl_id.to_be_bytes();
    bytes
        .iter()
        .map(|byte| if (0x20..=0x7E).contains(byte) { char::from(*byte) } else { '.' })
        .collect()
}

fn first_visible_text_in_paragraphs(
    paragraphs: &[hwpforge_core::paragraph::Paragraph],
) -> Option<String> {
    paragraphs.iter().find_map(first_visible_text_in_paragraph)
}

fn first_visible_text_in_paragraph(para: &hwpforge_core::paragraph::Paragraph) -> Option<String> {
    para.runs.iter().find_map(|run| match &run.content {
        // Text + InlineText share the "first visible trimmed text"
        // semantic via the unified `plain_text()` accessor — see
        // debug doc §3a-A9.
        hwpforge_core::run::RunContent::Text(_) | hwpforge_core::run::RunContent::InlineText(_) => {
            let cow = run.content.plain_text()?;
            let trimmed = cow.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        hwpforge_core::run::RunContent::Table(table) => first_visible_text_in_table(table),
        _ => None,
    })
}

fn first_visible_text_in_table(table: &hwpforge_core::table::Table) -> Option<String> {
    table.rows.iter().find_map(|row| {
        row.cells.iter().find_map(|cell| first_visible_text_in_paragraphs(&cell.paragraphs))
    })
}

fn summarize_document(sections: &[Hwp5SectionSummary]) -> Hwp5DocumentSummary {
    Hwp5DocumentSummary {
        sections: sections.len(),
        paragraphs: sections.iter().map(|section| section.paragraphs).sum(),
        non_empty_paragraphs: sections.iter().map(|section| section.non_empty_paragraphs).sum(),
        tables: sections.iter().map(|section| section.tables).sum(),
        headers: sections.iter().filter(|section| section.has_header).count(),
        footers: sections.iter().filter(|section| section.has_footer).count(),
        page_numbers: sections.iter().filter(|section| section.has_page_number).count(),
        landscape_sections: sections.iter().filter(|section| section.landscape).count(),
    }
}

#[cfg(test)]
mod tests;
