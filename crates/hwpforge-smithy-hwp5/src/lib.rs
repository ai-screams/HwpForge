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
    let (document, image_store, proj_warnings) =
        projection::project_to_core_with_images(intermediate.sections, &image_assets)?;
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

    let (hwp5_styles, hwpx_style_store, style_warnings) =
        project_doc_info_styles_with_warnings(&intermediate.doc_info);
    warnings.extend(style_warnings);

    let (document, mut image_store, proj_warnings) =
        projection::project_to_core_with_images_and_ole(
            intermediate.sections,
            &image_assets,
            &ole_assets,
        )?;
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
mod tests {
    use super::*;

    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use hwpforge_core::control::Control;
    use hwpforge_core::image::Image;
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::Run;
    use hwpforge_core::table::Table;
    use hwpforge_foundation::{
        BookmarkType, HeadingType, HwpUnit, NumberFormatType, PageNumberPosition,
    };
    use hwpforge_smithy_hwpx::{HwpxDecoder, PackageReader};

    fn default_test_char_shape() -> crate::schema::header::Hwp5RawCharShape {
        crate::schema::header::Hwp5RawCharShape {
            font_ids: [0; 7],
            font_ratios: [100; 7],
            font_spacings: [0; 7],
            font_rel_sizes: [100; 7],
            font_offsets: [0; 7],
            height: 1000,
            property: 0,
            shadow_gap_x: 0,
            shadow_gap_y: 0,
            text_color: 0x000000,
            underline_color: 0x000000,
            shade_color: 0xFFFF_FFFF,
            shadow_color: 0x000000,
            border_fill_id: None,
            strike_color: None,
        }
    }

    fn doc_info_with_style_projection_warning() -> crate::decoder::header::DocInfoResult {
        let mut raw = default_test_char_shape();
        raw.property =
            (1 << 2) | (1 << 4) | (2 << 11) | (1 << 13) | (1 << 15) | (1 << 18) | (7 << 26);
        raw.font_ratios[1] = 90;
        raw.font_spacings[2] = 5;

        crate::decoder::header::DocInfoResult {
            id_mappings: None,
            fonts: vec![crate::schema::header::Hwp5RawFaceName {
                property: 0,
                face_name: "함초롬바탕".into(),
                alternate_font_type: None,
                alternate_font_name: None,
                panose1: None,
                default_font_name: None,
            }],
            char_shapes: vec![raw],
            para_shapes: vec![],
            numberings: vec![],
            bullets: vec![],
            tab_defs: vec![],
            styles: vec![],
            border_fills: vec![],
            warnings: vec![],
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct ImageFixtureExpectation {
        name: &'static str,
        expected_storage_names: &'static [&'static str],
        expected_gso_count: usize,
        expected_shape_picture_count: usize,
        expected_table_count_after_convert: usize,
        expected_body_images_after_convert: usize,
        expected_header_images_after_convert: usize,
        expected_footer_images_after_convert: usize,
        expected_table_cell_images_after_convert: usize,
        expected_textbox_images_after_convert: usize,
        expected_textbox_controls_after_convert: usize,
    }

    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    struct DecodedImageLayout {
        body_images: usize,
        header_images: usize,
        footer_images: usize,
        table_cell_images: usize,
        textbox_images: usize,
        textbox_controls: usize,
    }

    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    struct DecodedShapeLayout {
        lines: usize,
        polygons: usize,
        textboxes: usize,
        rects: usize,
        ellipses: usize,
        arcs: usize,
        curves: usize,
        connect_lines: usize,
        equations: usize,
        memos: usize,
        dutmals: usize,
        composes: usize,
        index_marks: usize,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum DecodedImageLocation {
        Body,
        Header,
        Footer,
        TableCell,
        TextBox,
    }

    fn fixture_path(name: &str) -> PathBuf {
        crate::test_support::workspace_fixture_path(name)
    }

    fn unique_temp_path(file_name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("hwpforge-hwp5-image-slice-{stamp}-{file_name}"))
    }

    fn read_section_xml(path: &Path, index: usize) -> String {
        let bytes = std::fs::read(path).expect("converted hwpx should be readable");
        let mut package =
            PackageReader::new(&bytes).expect("converted hwpx should open as package");
        package.read_section_xml(index).expect("section xml should exist")
    }

    fn joined_text_runs<'a>(runs: impl IntoIterator<Item = &'a Run>) -> String {
        runs.into_iter().filter_map(|run| run.content.as_text()).collect()
    }

    #[test]
    fn inspect_summary_counts_style_projection_warnings() {
        let doc_info = doc_info_with_style_projection_warning();
        let (_, _, style_warnings) = project_doc_info_styles_with_warnings(&doc_info);
        assert!(
            !style_warnings.is_empty(),
            "synthetic doc info must trigger style projection warnings"
        );

        let intermediate = crate::decoder::DecodedHwp5Intermediate {
            version: "5.0.2.5".to_string(),
            compressed: false,
            package_entries: vec![],
            bin_data_records: vec![],
            bin_data_streams: vec![],
            doc_info,
            sections: vec![crate::decoder::section::SectionResult {
                paragraphs: vec![],
                page_def: None,
                section_def_properties: None,
                page_border_fills: Vec::new(),
                warnings: vec![],
            }],
            warnings: vec![],
        };

        let summary =
            inspect_decoded_hwp5_intermediate(intermediate).expect("inspect summary should build");
        assert_eq!(
            summary.warning_count,
            style_warnings.len(),
            "inspect must count the same style projection warnings as conversion"
        );
    }

    fn shape_picture_count(report: &Hwp5CensusReport) -> usize {
        report
            .sections
            .iter()
            .flat_map(|section| section.tag_counts.iter())
            .filter(|entry| entry.tag_name == "ShapePicture")
            .map(|entry| entry.count)
            .sum()
    }

    fn ctrl_count(report: &Hwp5CensusReport, ctrl_id_ascii: &str) -> usize {
        report
            .sections
            .iter()
            .flat_map(|section| section.ctrl_ids.iter())
            .filter(|entry| entry.ctrl_id_ascii == ctrl_id_ascii)
            .map(|entry| entry.count)
            .sum()
    }

    fn storage_names(report: &Hwp5CensusReport) -> Vec<String> {
        let mut names: Vec<String> = report
            .doc_info
            .bin_data_records
            .iter()
            .map(|record| record.storage_name.clone())
            .collect();
        names.sort();
        names
    }

    fn stream_names(report: &Hwp5CensusReport) -> Vec<String> {
        let mut names: Vec<String> =
            report.bin_data_streams.iter().map(|stream| stream.name.clone()).collect();
        names.sort();
        names
    }

    fn joined_asset_storage_names(plan: &Hwp5JoinedImageAssetPlan) -> Vec<String> {
        let mut names: Vec<String> =
            plan.ordered_assets.iter().map(|asset| asset.payload.storage_name.clone()).collect();
        names.sort();
        names
    }

    fn decoded_image_store_names(decoded: &hwpforge_smithy_hwpx::HwpxDocument) -> Vec<String> {
        let mut names: Vec<String> =
            decoded.image_store.iter().map(|(name, _)| name.to_string()).collect();
        names.sort();
        names
    }

    fn collect_decoded_image_layout(
        decoded: &hwpforge_smithy_hwpx::HwpxDocument,
    ) -> DecodedImageLayout {
        let mut layout = DecodedImageLayout::default();

        for section in decoded.document.sections() {
            count_images_in_paragraphs(
                &section.paragraphs,
                DecodedImageLocation::Body,
                &mut layout,
            );
            // ADR-002: walk every header/footer in the multi-cardinality Vec.
            for header in &section.headers {
                count_images_in_paragraphs(
                    &header.paragraphs,
                    DecodedImageLocation::Header,
                    &mut layout,
                );
            }
            for footer in &section.footers {
                count_images_in_paragraphs(
                    &footer.paragraphs,
                    DecodedImageLocation::Footer,
                    &mut layout,
                );
            }
        }

        layout
    }

    fn count_images_in_paragraphs(
        paragraphs: &[Paragraph],
        location: DecodedImageLocation,
        layout: &mut DecodedImageLayout,
    ) {
        for paragraph in paragraphs {
            for run in &paragraph.runs {
                count_images_in_run(run, location, layout);
            }
        }
    }

    fn count_images_in_run(
        run: &Run,
        location: DecodedImageLocation,
        layout: &mut DecodedImageLayout,
    ) {
        match &run.content {
            // Image counters skip text-bearing runs entirely; both
            // variants carry no images. Explicit list rather than `_`
            // wildcard to keep intent visible (debug doc §3a-A10).
            hwpforge_core::run::RunContent::Text(_)
            | hwpforge_core::run::RunContent::InlineText(_) => {}
            hwpforge_core::run::RunContent::Image(_) => match location {
                DecodedImageLocation::Body => layout.body_images += 1,
                DecodedImageLocation::Header => layout.header_images += 1,
                DecodedImageLocation::Footer => layout.footer_images += 1,
                DecodedImageLocation::TableCell => layout.table_cell_images += 1,
                DecodedImageLocation::TextBox => layout.textbox_images += 1,
            },
            hwpforge_core::run::RunContent::Table(table) => {
                count_images_in_table(table, layout);
            }
            hwpforge_core::run::RunContent::Control(control) => {
                count_images_in_control(control.as_ref(), layout);
            }
            _ => {}
        }
    }

    fn count_images_in_table(table: &Table, layout: &mut DecodedImageLayout) {
        for row in &table.rows {
            for cell in &row.cells {
                count_images_in_paragraphs(
                    &cell.paragraphs,
                    DecodedImageLocation::TableCell,
                    layout,
                );
            }
        }
    }

    fn count_images_in_control(control: &Control, layout: &mut DecodedImageLayout) {
        match control {
            Control::TextBox { paragraphs, .. } => {
                layout.textbox_controls += 1;
                count_images_in_paragraphs(paragraphs, DecodedImageLocation::TextBox, layout);
            }
            Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
                count_images_in_paragraphs(paragraphs, DecodedImageLocation::Body, layout);
            }
            _ => {}
        }
    }

    fn first_image_in_paragraphs(paragraphs: &[Paragraph]) -> Option<&Image> {
        for paragraph in paragraphs {
            for run in &paragraph.runs {
                if let Some(image) = first_image_in_run(run) {
                    return Some(image);
                }
            }
        }
        None
    }

    fn first_image_in_run(run: &Run) -> Option<&Image> {
        match &run.content {
            hwpforge_core::run::RunContent::Image(image) => Some(image),
            hwpforge_core::run::RunContent::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        if let Some(image) = first_image_in_paragraphs(&cell.paragraphs) {
                            return Some(image);
                        }
                    }
                }
                None
            }
            hwpforge_core::run::RunContent::Control(control) => first_image_in_control(control),
            _ => None,
        }
    }

    fn first_image_in_control(control: &Control) -> Option<&Image> {
        match control {
            Control::TextBox { paragraphs, .. }
            | Control::Footnote { paragraphs, .. }
            | Control::Endnote { paragraphs, .. } => first_image_in_paragraphs(paragraphs),
            _ => None,
        }
    }

    fn assert_valid_hwpx(path: &Path) {
        let bytes = std::fs::read(path).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        assert!(
            !decoded.document.sections().is_empty(),
            "converted hwpx should contain at least one section"
        );

        let mut package = PackageReader::new(&bytes).expect("converted hwpx should be a package");
        let entry_paths: Vec<String> = package
            .list_entries()
            .expect("list hwpx entries")
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        for path in entry_paths {
            if !(path.ends_with(".xml") || path.ends_with(".hpf")) {
                continue;
            }

            let content = package
                .read_text_entry(&path)
                .unwrap_or_else(|err| panic!("read xml-ish zip entry {path}: {err}"));
            assert!(!content.contains('\0'), "xml entry {} contains NUL byte", path);
        }
    }

    fn collect_decoded_shape_layout(
        decoded: &hwpforge_smithy_hwpx::HwpxDocument,
    ) -> DecodedShapeLayout {
        let mut layout = DecodedShapeLayout::default();
        for section in decoded.document.sections() {
            count_shapes_in_paragraphs(&section.paragraphs, &mut layout);
            // ADR-002: same multi-cardinality walk for shape counters.
            for header in &section.headers {
                count_shapes_in_paragraphs(&header.paragraphs, &mut layout);
            }
            for footer in &section.footers {
                count_shapes_in_paragraphs(&footer.paragraphs, &mut layout);
            }
        }
        layout
    }

    fn count_shapes_in_paragraphs(paragraphs: &[Paragraph], layout: &mut DecodedShapeLayout) {
        for paragraph in paragraphs {
            for run in &paragraph.runs {
                count_shapes_in_run(run, layout);
            }
        }
    }

    fn collect_decoded_body_heading_triples(
        decoded: &hwpforge_smithy_hwpx::HwpxDocument,
    ) -> Vec<(HeadingType, u32, u32)> {
        decoded
            .document
            .sections()
            .iter()
            .flat_map(|section| section.paragraphs.iter())
            .map(|paragraph| {
                let shape = decoded
                    .style_store
                    .para_shape(paragraph.para_shape_id)
                    .expect("paragraph para shape should exist");
                (shape.heading_type, shape.heading_id_ref, shape.heading_level)
            })
            .collect()
    }

    fn count_shapes_in_run(run: &Run, layout: &mut DecodedShapeLayout) {
        match &run.content {
            hwpforge_core::run::RunContent::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        count_shapes_in_paragraphs(&cell.paragraphs, layout);
                    }
                }
            }
            hwpforge_core::run::RunContent::Control(control) => {
                count_shapes_in_control(control.as_ref(), layout);
            }
            _ => {}
        }
    }

    fn count_shapes_in_control(control: &Control, layout: &mut DecodedShapeLayout) {
        match control {
            Control::Line { .. } => layout.lines += 1,
            Control::Rect { .. } => layout.rects += 1,
            Control::Polygon { .. } => layout.polygons += 1,
            Control::Ellipse { .. } => layout.ellipses += 1,
            Control::Arc { .. } => layout.arcs += 1,
            Control::Curve { .. } => layout.curves += 1,
            Control::ConnectLine { .. } => layout.connect_lines += 1,
            Control::Equation { .. } => layout.equations += 1,
            Control::Memo { content, .. } => {
                layout.memos += 1;
                count_shapes_in_paragraphs(content, layout);
            }
            Control::Dutmal { .. } => layout.dutmals += 1,
            Control::Compose { .. } => layout.composes += 1,
            Control::IndexMark { .. } => layout.index_marks += 1,
            Control::TextBox { paragraphs, .. } => {
                layout.textboxes += 1;
                count_shapes_in_paragraphs(paragraphs, layout);
            }
            Control::Footnote { paragraphs, .. } | Control::Endnote { paragraphs, .. } => {
                count_shapes_in_paragraphs(paragraphs, layout);
            }
            _ => {}
        }
    }

    #[test]
    fn census_image_fixture_matrix_reports_expected_bindata_and_gso_inventory() {
        let cases: [ImageFixtureExpectation; 8] = [
            ImageFixtureExpectation {
                name: "img_01_single_png_inline.hwp",
                expected_storage_names: &["BIN0001.png"],
                expected_gso_count: 1,
                expected_shape_picture_count: 1,
                expected_table_count_after_convert: 0,
                expected_body_images_after_convert: 1,
                expected_header_images_after_convert: 0,
                expected_footer_images_after_convert: 0,
                expected_table_cell_images_after_convert: 0,
                expected_textbox_images_after_convert: 0,
                expected_textbox_controls_after_convert: 0,
            },
            ImageFixtureExpectation {
                name: "img_03_two_images_png_jpg.hwp",
                expected_storage_names: &["BIN0001.png", "BIN0002.jpeg"],
                expected_gso_count: 2,
                expected_shape_picture_count: 2,
                expected_table_count_after_convert: 0,
                expected_body_images_after_convert: 2,
                expected_header_images_after_convert: 0,
                expected_footer_images_after_convert: 0,
                expected_table_cell_images_after_convert: 0,
                expected_textbox_images_after_convert: 0,
                expected_textbox_controls_after_convert: 0,
            },
            ImageFixtureExpectation {
                name: "img_05_image_in_table_cell.hwp",
                expected_storage_names: &["BIN0001.png"],
                expected_gso_count: 1,
                expected_shape_picture_count: 1,
                expected_table_count_after_convert: 1,
                expected_body_images_after_convert: 0,
                expected_header_images_after_convert: 0,
                expected_footer_images_after_convert: 0,
                expected_table_cell_images_after_convert: 1,
                expected_textbox_images_after_convert: 0,
                expected_textbox_controls_after_convert: 0,
            },
            ImageFixtureExpectation {
                name: "mixed_02a_header_image_footer_text_real.hwp",
                expected_storage_names: &["BIN0001.png"],
                expected_gso_count: 1,
                expected_shape_picture_count: 1,
                expected_table_count_after_convert: 0,
                expected_body_images_after_convert: 0,
                expected_header_images_after_convert: 1,
                expected_footer_images_after_convert: 0,
                expected_table_cell_images_after_convert: 0,
                expected_textbox_images_after_convert: 0,
                expected_textbox_controls_after_convert: 0,
            },
            ImageFixtureExpectation {
                name: "mixed_02b_textbox_with_image_real.hwp",
                expected_storage_names: &["BIN0001.png"],
                expected_gso_count: 2,
                expected_shape_picture_count: 1,
                expected_table_count_after_convert: 0,
                expected_body_images_after_convert: 0,
                expected_header_images_after_convert: 0,
                expected_footer_images_after_convert: 0,
                expected_table_cell_images_after_convert: 0,
                expected_textbox_images_after_convert: 1,
                expected_textbox_controls_after_convert: 1,
            },
            ImageFixtureExpectation {
                name: "floating_image_not_treat_as_char.hwp",
                expected_storage_names: &["BIN0001.png"],
                expected_gso_count: 1,
                expected_shape_picture_count: 1,
                expected_table_count_after_convert: 0,
                expected_body_images_after_convert: 1,
                expected_header_images_after_convert: 0,
                expected_footer_images_after_convert: 0,
                expected_table_cell_images_after_convert: 0,
                expected_textbox_images_after_convert: 0,
                expected_textbox_controls_after_convert: 0,
            },
            ImageFixtureExpectation {
                name: "two_same_image_refs_different_places.hwp",
                expected_storage_names: &["BIN0001.png"],
                expected_gso_count: 2,
                expected_shape_picture_count: 2,
                expected_table_count_after_convert: 0,
                expected_body_images_after_convert: 2,
                expected_header_images_after_convert: 0,
                expected_footer_images_after_convert: 0,
                expected_table_cell_images_after_convert: 0,
                expected_textbox_images_after_convert: 0,
                expected_textbox_controls_after_convert: 0,
            },
            ImageFixtureExpectation {
                name: "real_crop_vs_original_two_objects.hwp",
                expected_storage_names: &["BIN0001.png"],
                expected_gso_count: 2,
                expected_shape_picture_count: 2,
                expected_table_count_after_convert: 0,
                expected_body_images_after_convert: 2,
                expected_header_images_after_convert: 0,
                expected_footer_images_after_convert: 0,
                expected_table_cell_images_after_convert: 0,
                expected_textbox_images_after_convert: 0,
                expected_textbox_controls_after_convert: 0,
            },
        ];

        for case in cases {
            let path = fixture_path(case.name);
            if !path.exists() {
                continue;
            }

            let report = census_hwp5_file(&path).expect("fixture census should succeed");
            let expected_storage_names: Vec<String> =
                case.expected_storage_names.iter().map(|value| (*value).to_string()).collect();

            assert_eq!(storage_names(&report), expected_storage_names, "fixture={}", case.name);
            assert_eq!(stream_names(&report), expected_storage_names, "fixture={}", case.name);
            assert_eq!(
                ctrl_count(&report, "gso "),
                case.expected_gso_count,
                "fixture={}",
                case.name
            );
            assert_eq!(
                shape_picture_count(&report),
                case.expected_shape_picture_count,
                "fixture={}",
                case.name
            );
        }
    }

    #[test]
    fn join_hwp5_image_assets_matches_fixture_bindata_inventory() {
        let cases: [(&str, &[&str]); 2] = [
            ("img_01_single_png_inline.hwp", &["BIN0001.png"]),
            ("img_03_two_images_png_jpg.hwp", &["BIN0001.png", "BIN0002.jpeg"]),
        ];

        for (name, expected_storage_names) in cases {
            let path = fixture_path(name);
            if !path.exists() {
                continue;
            }

            let bytes = std::fs::read(&path).expect("fixture bytes should be readable");
            let intermediate =
                crate::decoder::decode_intermediate(&bytes).expect("fixture intermediate decode");
            let image_assets =
                join_hwp5_image_assets(&bytes, &intermediate).expect("image assets should join");
            let expected_storage_names: Vec<String> =
                expected_storage_names.iter().map(|value| (*value).to_string()).collect();

            assert_eq!(
                joined_asset_storage_names(&image_assets),
                expected_storage_names,
                "fixture={name}"
            );
            assert!(
                image_assets.ordered_assets.iter().all(|asset| {
                    asset.payload.width_hwp.is_some_and(|width| width > 0)
                        && asset.payload.height_hwp.is_some_and(|height| height > 0)
                }),
                "joined image assets should preserve positive geometry hints: fixture={name}"
            );
            assert!(
                image_assets.ordered_assets.iter().all(|asset| !asset.bytes.is_empty()),
                "fixture={name}"
            );
        }
    }

    #[test]
    fn join_hwp5_image_assets_decompresses_full_report_png_payload() {
        let path = fixture_path("full_report.hwp");
        if !path.exists() {
            return;
        }

        let bytes = std::fs::read(&path).expect("fixture bytes should be readable");
        let intermediate =
            crate::decoder::decode_intermediate(&bytes).expect("fixture intermediate decode");
        let image_assets =
            join_hwp5_image_assets(&bytes, &intermediate).expect("image assets should join");
        let first_asset = image_assets
            .asset_for_binary_data_id(1)
            .expect("full_report should expose binary image id 1");

        assert!(
            first_asset.bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "full_report joined image bytes must be actual PNG payload, not compressed raw data"
        );
    }

    #[test]
    fn hwp5_to_hwpx_full_report_keeps_leading_image_non_zero() {
        let source = fixture_path("full_report.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("full_report.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out).expect("full_report conversion should succeed");
        assert!(
            !warnings.iter().any(|warning| matches!(
                warning,
                Hwp5Warning::DroppedControl { control, .. } if *control == "image"
            )),
            "full_report should not drop its leading image"
        );

        assert_valid_hwpx(&out);

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let layout = collect_decoded_image_layout(&decoded);
        let shape_layout = collect_decoded_shape_layout(&decoded);
        let section0 = &decoded.document.sections()[0];
        let first_body_image = first_image_in_paragraphs(&section0.paragraphs)
            .expect("section 0 should contain an image");

        // The HWPX decoder's `image_store` returns every `BinData/*` entry,
        // which now legitimately includes the Wave 4c chart-carry OLE blobs
        // (`ole{N}.ole`). Assert only on image (`BIN*.png`/`.jpg`/…) names
        // here; the OLE entries are exercised by the chart-carry golden test.
        let image_only_names: Vec<String> = decoded_image_store_names(&decoded)
            .into_iter()
            .filter(|name| !name.to_ascii_lowercase().ends_with(".ole"))
            .collect();
        assert_eq!(image_only_names, vec!["BIN0001.png".to_string()]);
        assert_eq!(layout.body_images, 1);
        assert_eq!(layout.header_images, 0);
        assert_eq!(layout.footer_images, 0);
        assert_eq!(shape_layout.lines, 4);
        assert_eq!(shape_layout.polygons, 1);
        assert!(!section0.headers.is_empty(), "full_report should keep header");
        assert!(!section0.footers.is_empty(), "full_report should keep footer");
        assert_eq!(first_body_image.path, "BinData/BIN0001");
        assert_ne!(first_body_image.width, HwpUnit::ZERO);
        assert_ne!(first_body_image.height, HwpUnit::ZERO);

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_image_fixture_matrix_emits_valid_hwpx_packages() {
        let cases: [ImageFixtureExpectation; 8] = [
            ImageFixtureExpectation {
                name: "img_01_single_png_inline.hwp",
                expected_storage_names: &["BIN0001.png"],
                expected_gso_count: 1,
                expected_shape_picture_count: 1,
                expected_table_count_after_convert: 0,
                expected_body_images_after_convert: 1,
                expected_header_images_after_convert: 0,
                expected_footer_images_after_convert: 0,
                expected_table_cell_images_after_convert: 0,
                expected_textbox_images_after_convert: 0,
                expected_textbox_controls_after_convert: 0,
            },
            ImageFixtureExpectation {
                name: "img_03_two_images_png_jpg.hwp",
                expected_storage_names: &["BIN0001.png", "BIN0002.jpeg"],
                expected_gso_count: 2,
                expected_shape_picture_count: 2,
                expected_table_count_after_convert: 0,
                expected_body_images_after_convert: 2,
                expected_header_images_after_convert: 0,
                expected_footer_images_after_convert: 0,
                expected_table_cell_images_after_convert: 0,
                expected_textbox_images_after_convert: 0,
                expected_textbox_controls_after_convert: 0,
            },
            ImageFixtureExpectation {
                name: "img_05_image_in_table_cell.hwp",
                expected_storage_names: &["BIN0001.png"],
                expected_gso_count: 1,
                expected_shape_picture_count: 1,
                expected_table_count_after_convert: 1,
                expected_body_images_after_convert: 0,
                expected_header_images_after_convert: 0,
                expected_footer_images_after_convert: 0,
                expected_table_cell_images_after_convert: 1,
                expected_textbox_images_after_convert: 0,
                expected_textbox_controls_after_convert: 0,
            },
            ImageFixtureExpectation {
                name: "mixed_02a_header_image_footer_text_real.hwp",
                expected_storage_names: &["BIN0001.png"],
                expected_gso_count: 1,
                expected_shape_picture_count: 1,
                expected_table_count_after_convert: 0,
                expected_body_images_after_convert: 0,
                expected_header_images_after_convert: 1,
                expected_footer_images_after_convert: 0,
                expected_table_cell_images_after_convert: 0,
                expected_textbox_images_after_convert: 0,
                expected_textbox_controls_after_convert: 0,
            },
            ImageFixtureExpectation {
                name: "mixed_02b_textbox_with_image_real.hwp",
                expected_storage_names: &["BIN0001.png"],
                expected_gso_count: 2,
                expected_shape_picture_count: 1,
                expected_table_count_after_convert: 0,
                expected_body_images_after_convert: 0,
                expected_header_images_after_convert: 0,
                expected_footer_images_after_convert: 0,
                expected_table_cell_images_after_convert: 0,
                expected_textbox_images_after_convert: 1,
                expected_textbox_controls_after_convert: 1,
            },
            ImageFixtureExpectation {
                name: "floating_image_not_treat_as_char.hwp",
                expected_storage_names: &["BIN0001.png"],
                expected_gso_count: 1,
                expected_shape_picture_count: 1,
                expected_table_count_after_convert: 0,
                expected_body_images_after_convert: 1,
                expected_header_images_after_convert: 0,
                expected_footer_images_after_convert: 0,
                expected_table_cell_images_after_convert: 0,
                expected_textbox_images_after_convert: 0,
                expected_textbox_controls_after_convert: 0,
            },
            ImageFixtureExpectation {
                name: "two_same_image_refs_different_places.hwp",
                expected_storage_names: &["BIN0001.png"],
                expected_gso_count: 2,
                expected_shape_picture_count: 2,
                expected_table_count_after_convert: 0,
                expected_body_images_after_convert: 2,
                expected_header_images_after_convert: 0,
                expected_footer_images_after_convert: 0,
                expected_table_cell_images_after_convert: 0,
                expected_textbox_images_after_convert: 0,
                expected_textbox_controls_after_convert: 0,
            },
            ImageFixtureExpectation {
                name: "real_crop_vs_original_two_objects.hwp",
                expected_storage_names: &["BIN0001.png"],
                expected_gso_count: 2,
                expected_shape_picture_count: 2,
                expected_table_count_after_convert: 0,
                expected_body_images_after_convert: 2,
                expected_header_images_after_convert: 0,
                expected_footer_images_after_convert: 0,
                expected_table_cell_images_after_convert: 0,
                expected_textbox_images_after_convert: 0,
                expected_textbox_controls_after_convert: 0,
            },
        ];

        for case in cases {
            let source = fixture_path(case.name);
            if !source.exists() {
                continue;
            }

            let out = unique_temp_path(&format!("{}.hwpx", case.name.trim_end_matches(".hwp")));
            let warnings = hwp5_to_hwpx(&source, &out).expect("fixture conversion should succeed");
            assert!(
                warnings.is_empty(),
                "controlled image fixture should convert without warnings: {}",
                case.name
            );

            assert_valid_hwpx(&out);

            let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
            let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
            let layout = collect_decoded_image_layout(&decoded);
            let expected_storage_names: Vec<String> =
                case.expected_storage_names.iter().map(|value| (*value).to_string()).collect();
            let total_tables: usize = decoded
                .document
                .sections()
                .iter()
                .map(|section| section.content_counts().tables)
                .sum();
            assert_eq!(
                total_tables, case.expected_table_count_after_convert,
                "fixture={}",
                case.name
            );
            assert_eq!(
                decoded_image_store_names(&decoded),
                expected_storage_names,
                "fixture={}",
                case.name
            );
            assert_eq!(
                layout.body_images, case.expected_body_images_after_convert,
                "fixture={}",
                case.name
            );
            assert_eq!(
                layout.header_images, case.expected_header_images_after_convert,
                "fixture={}",
                case.name
            );
            assert_eq!(
                layout.footer_images, case.expected_footer_images_after_convert,
                "fixture={}",
                case.name
            );
            assert_eq!(
                layout.table_cell_images, case.expected_table_cell_images_after_convert,
                "fixture={}",
                case.name
            );
            assert_eq!(
                layout.textbox_images, case.expected_textbox_images_after_convert,
                "fixture={}",
                case.name
            );
            assert_eq!(
                layout.textbox_controls, case.expected_textbox_controls_after_convert,
                "fixture={}",
                case.name
            );

            let _ = std::fs::remove_file(&out);
        }
    }

    #[test]
    fn hwp5_to_hwpx_non_image_gso_fixture_matrix_emits_visible_line_and_polygon() {
        let cases: [(&str, usize, usize); 2] =
            [("line_simple.hwp", 1, 0), ("polygon_simple.hwp", 0, 1)];

        for (name, expected_lines, expected_polygons) in cases {
            let source = fixture_path(name);
            if !source.exists() {
                continue;
            }

            let out = unique_temp_path(&format!("{}.hwpx", name.trim_end_matches(".hwp")));
            let warnings = hwp5_to_hwpx(&source, &out).expect("fixture conversion should succeed");
            assert!(
                warnings.is_empty(),
                "controlled non-image gso fixture should convert without warnings: {name}"
            );

            assert_valid_hwpx(&out);

            let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
            let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
            let layout = collect_decoded_shape_layout(&decoded);
            assert_eq!(layout.lines, expected_lines, "fixture={name}");
            assert_eq!(layout.polygons, expected_polygons, "fixture={name}");

            let _ = std::fs::remove_file(&out);
        }
    }

    #[test]
    fn hwp5_to_hwpx_rect_fixture_carries_rect_without_warning() {
        let source = fixture_path("rect_simple.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("rect_simple.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out).expect("fixture conversion should succeed");
        assert!(
            !warnings.iter().any(|warning| matches!(
                warning,
                Hwp5Warning::DroppedControl { control, .. } if *control == "rect"
            )),
            "rect projection should no longer surface a DroppedControl:rect warning: {warnings:?}"
        );

        assert_valid_hwpx(&out);

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let layout = collect_decoded_shape_layout(&decoded);
        assert_eq!(layout.lines, 0);
        assert_eq!(layout.polygons, 0);
        assert_eq!(layout.textboxes, 0);
        assert!(layout.rects >= 1, "expected at least one decoded Control::Rect in section runs");

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_gso_ellipse_carries_ellipse() {
        // Wave 12a: a plain ellipse (gso ShapeComponentEllipse 0x50, property 0)
        // used to fall through to Hwp5Control::Unknown and silently empty its
        // paragraph. It must now carry as Control::Ellipse → <hp:ellipse>.
        let source = fixture_path("user_samples/sample-gso-ellipse.hwp");
        if !source.exists() {
            return;
        }
        let out = unique_temp_path("user-sample-gso-ellipse.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out).expect("ellipse conversion should succeed");
        assert!(
            !warnings.iter().any(|warning| matches!(warning, Hwp5Warning::DroppedControl { .. })),
            "gso ellipse must not drop any control: {warnings:?}"
        );
        assert_valid_hwpx(&out);

        let section_xml = read_section_xml(&out, 0);
        assert!(section_xml.contains("<hp:ellipse"), "converted xml must emit <hp:ellipse>");
        assert!(
            section_xml.contains(r#"hasArcPr="0""#),
            "a plain ellipse must not advertise arc properties"
        );

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let layout = collect_decoded_shape_layout(&decoded);
        assert_eq!(layout.ellipses, 1, "exactly one Control::Ellipse should round-trip");
        assert_eq!(layout.arcs, 0);
        assert_eq!(layout.curves, 0);

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_gso_arc_carries_arc() {
        // Wave 12a: 한컴 stores arcs in the same ShapeComponentEllipse (0x50)
        // record with arc fields set, so the arc must carry as Control::Arc →
        // <hp:ellipse hasArcPr="1">.
        let source = fixture_path("user_samples/sample-gso-arc.hwp");
        if !source.exists() {
            return;
        }
        let out = unique_temp_path("user-sample-gso-arc.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out).expect("arc conversion should succeed");
        assert!(
            !warnings.iter().any(|warning| matches!(warning, Hwp5Warning::DroppedControl { .. })),
            "gso arc must not drop any control: {warnings:?}"
        );
        assert_valid_hwpx(&out);

        let section_xml = read_section_xml(&out, 0);
        assert!(section_xml.contains("<hp:ellipse"), "an arc is emitted as <hp:ellipse>");
        assert!(
            section_xml.contains(r#"hasArcPr="1""#),
            "an arc must advertise arc properties via hasArcPr=1"
        );

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let layout = collect_decoded_shape_layout(&decoded);
        assert_eq!(layout.arcs, 1, "exactly one Control::Arc should round-trip");
        assert_eq!(layout.ellipses, 0);
        assert_eq!(layout.curves, 0);

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_gso_curve_carries_curve() {
        // Wave 12a: a bezier curve (gso ShapeComponentCurve 0x53) must carry as
        // Control::Curve → <hp:curve> instead of being dropped.
        let source = fixture_path("user_samples/sample-gso-curve.hwp");
        if !source.exists() {
            return;
        }
        let out = unique_temp_path("user-sample-gso-curve.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out).expect("curve conversion should succeed");
        assert!(
            !warnings.iter().any(|warning| matches!(warning, Hwp5Warning::DroppedControl { .. })),
            "gso curve must not drop any control: {warnings:?}"
        );
        assert_valid_hwpx(&out);

        let section_xml = read_section_xml(&out, 0);
        assert!(section_xml.contains("<hp:curve"), "converted xml must emit <hp:curve>");

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let layout = collect_decoded_shape_layout(&decoded);
        assert_eq!(layout.curves, 1, "exactly one Control::Curve should round-trip");
        assert_eq!(layout.ellipses, 0);
        assert_eq!(layout.arcs, 0);

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_gso_connectline_carries_connect_line() {
        // Wave 12b: 한컴 stores a connector in the same 0x4E ShapeComponentLine
        // record as a plain line; only the ShapeComponent "$col" type tag tells
        // them apart. The connector must carry as Control::ConnectLine →
        // <hp:connectLine>, while its two anchor rectangles stay Control::Rect.
        let source = fixture_path("user_samples/sample-gso-connectline-native.hwp");
        if !source.exists() {
            return;
        }
        let out = unique_temp_path("user-sample-gso-connectline-native.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out).expect("connect-line conversion should succeed");
        assert!(
            !warnings.iter().any(|warning| matches!(warning, Hwp5Warning::DroppedControl { .. })),
            "connect line must not drop any control: {warnings:?}"
        );
        assert_valid_hwpx(&out);

        let section_xml = read_section_xml(&out, 0);
        assert!(
            section_xml.contains("<hp:connectLine"),
            "the connector must emit <hp:connectLine>, not a plain <hp:line>"
        );

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let layout = collect_decoded_shape_layout(&decoded);
        assert_eq!(layout.connect_lines, 1, "exactly one Control::ConnectLine should round-trip");
        assert_eq!(layout.lines, 0, "the connector must not be reclassified as a plain line");
        assert_eq!(layout.rects, 2, "the two anchor rectangles must carry as Control::Rect");

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_equation_carries_script() {
        // Wave 12d: the eqed ctrl + HWPTAG_EQEDIT(0x58) script must carry as
        // Control::Equation → <hp:equation> with the HancomEQN script intact,
        // instead of eqed falling through to Unknown and being dropped.
        let source = fixture_path("user_samples/sample-equation-basic.hwp");
        if !source.exists() {
            return;
        }
        let out = unique_temp_path("user-sample-equation-basic.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out).expect("equation conversion should succeed");
        assert!(
            !warnings.iter().any(|warning| matches!(warning, Hwp5Warning::DroppedControl { .. })),
            "equation must not drop any control: {warnings:?}"
        );
        assert_valid_hwpx(&out);

        let section_xml = read_section_xml(&out, 0);
        assert!(section_xml.contains("<hp:equation"), "converted xml must emit <hp:equation>");
        assert!(
            section_xml.contains("<hp:script>{a + b} over {c + d}</hp:script>"),
            "the HancomEQN script must be preserved verbatim"
        );

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let layout = collect_decoded_shape_layout(&decoded);
        assert_eq!(layout.equations, 1, "exactly one Control::Equation should round-trip");

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_memo_basic_preserves_body_and_carries_memo() {
        // Wave 12e-Memo: the inline `%unk MEMO/.../.../...` ctrl + its
        // `HWPTAG_MEMO_LIST` (0x5D) cluster at the section's last body
        // paragraph must carry as `Control::Memo` instead of falling through
        // to `Unknown` (which previously let the cluster's lvl=2 ParaText
        // overwrite the body text — corpus corruption bug).
        let source = fixture_path("user_samples/sample-memo-basic.hwp");
        if !source.exists() {
            return;
        }
        let out = unique_temp_path("user-sample-memo-basic.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out).expect("memo conversion should succeed");
        assert!(
            !warnings.iter().any(|warning| matches!(
                warning,
                Hwp5Warning::DroppedControl { control: "memo" | "memo_content_cluster", .. }
            )),
            "memo must not drop placeholder or cluster: {warnings:?}"
        );
        assert_valid_hwpx(&out);

        let section_xml = read_section_xml(&out, 0);
        // The body text is split across two runs in the encoded HWPX
        // (`메모 대상 문장` + `입니다.`) because 한컴 changed char shape mid-line;
        // assert both fragments exist rather than the joined string.
        assert!(
            section_xml.contains("메모 대상 문장") && section_xml.contains("입니다"),
            "body anchor text must survive the memo cluster (not be overwritten by lvl=2 ParaText): {section_xml}"
        );
        assert!(
            section_xml.contains(r#"<hp:fieldBegin"#) && section_xml.contains(r#"type="MEMO""#),
            "memo must emit <hp:fieldBegin type=\"MEMO\"> in HWPX"
        );
        assert!(
            section_xml.contains("Claude야 여기가 메모야"),
            "memo body content must carry into the <hp:subList>"
        );

        // Wave 12h: full 7-parameter `<hp:parameters cnt="7">` block.
        // Without these 한컴 mis-classifies the field and renders
        // `[메모 시작][필드 끝]` in 조판부호 view.
        assert!(
            section_xml.contains(r#"<hp:parameters cnt="7""#),
            "memo must emit the 7-parameter block (Wave 12h)"
        );
        assert!(
            section_xml.contains(r#"editable="1""#)
                && section_xml.contains(r#"dirty="1""#)
                && section_xml.contains(r#"zorder="1""#),
            "fieldBegin must carry editable/dirty/zorder = 1 (Wave 12h)"
        );
        assert!(
            section_xml.contains(r#"<hp:stringParam name="Command">MEMO/65535/1/"#),
            "Command parameter must mirror wire command verbatim (Wave 12h)"
        );
        assert!(
            section_xml.contains(r#"<hp:stringParam name="ID">memo1</hp:stringParam>"#),
            "ID parameter must derive `memo{{number}}` from wire memo_id (Wave 12h)"
        );
        assert!(
            section_xml.contains(r#"<hp:stringParam name="Author">hanyul</hp:stringParam>"#),
            "Author parameter must come from wire slash[5] (Wave 12h)"
        );
        assert!(
            section_xml.contains(r#"<hp:stringParam name="MemoShapeIDRef">65535</hp:stringParam>"#),
            "MemoShapeIDRef parameter must come from wire slash[1] (Wave 12h)"
        );
        assert!(
            section_xml.contains(r#"<hp:stringParam name="CreateDateTime">"#)
                && section_xml.contains("Z</hp:stringParam>"),
            "CreateDateTime auto-generated as ISO 8601 UTC (Wave 12h)"
        );

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let layout = collect_decoded_shape_layout(&decoded);
        assert_eq!(layout.memos, 1, "exactly one Control::Memo should round-trip");

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_memo_multiple_matches_clusters_by_id() {
        // Wave 12e-Memo: multiple memos in the same section. Content clusters
        // are stored *together* at the end of the last body paragraph and are
        // matched back to each inline placeholder by `memo_id` (not document
        // position). This catches accidental position-based matching that
        // would only surface when there is more than one memo.
        let source = fixture_path("user_samples/sample-memo-multiple.hwp");
        if !source.exists() {
            return;
        }
        let out = unique_temp_path("user-sample-memo-multiple.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out).expect("multi-memo conversion should succeed");
        assert!(
            !warnings.iter().any(|warning| matches!(
                warning,
                Hwp5Warning::DroppedControl { control: "memo" | "memo_content_cluster", .. }
            )),
            "multi-memo must not drop placeholder or cluster: {warnings:?}"
        );
        assert_valid_hwpx(&out);

        let section_xml = read_section_xml(&out, 0);
        // Body anchors of both memos must survive.
        assert!(
            section_xml.contains("이 단어에") && section_xml.contains("메모1"),
            "first memo anchor body text must survive"
        );
        assert!(
            section_xml.contains("저 단어에") && section_xml.contains("메모2"),
            "second memo anchor body text must survive"
        );
        // Each cluster is matched to the right placeholder by memo_id — if
        // matching were positional, swapping the cluster order would silently
        // mislabel the bodies.
        assert!(section_xml.contains("첫 번째"), "first memo body content must carry");
        assert!(section_xml.contains("두번째"), "second memo body content must carry");

        // Wave 12h: both memos emit full 7-parameter blocks with distinct
        // ID/Number derived from each wire `memo_id`.
        assert!(
            section_xml.matches(r#"<hp:parameters cnt="7""#).count() == 2,
            "each memo must emit a 7-parameter block (Wave 12h)"
        );
        assert!(
            section_xml.contains(r#"<hp:stringParam name="ID">memo1</hp:stringParam>"#)
                && section_xml.contains(r#"<hp:stringParam name="ID">memo2</hp:stringParam>"#),
            "ID parameters derive from wire memo_id distinctly (memo1, memo2)"
        );

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let layout = collect_decoded_shape_layout(&decoded);
        assert_eq!(layout.memos, 2, "both memos should round-trip");

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_dutmal_basic_carries_option_and_preserves_spacing() {
        // Wave 12i covers two bugs that surface on a section-leading paragraph
        // with two adjacent dutmals separated by a body-text space.
        //
        // Bug A — option attribute carry. HWP5 stores `option_raw` at
        //   `tail[8..12]` of the `tdut` ctrl payload (see
        //   `.docs/algorithms/2026-06-01_dutmal_carry.md`); 한컴 emits the same
        //   integer back into `<hp:dutmal option=…>`. The encoder previously
        //   hard-coded `option="0"` so a fixture with `option="4"` lost
        //   fidelity.
        //
        // Bug B — flat projection `control_iter` filter. The flat paragraph
        //   projection path used to iterate every control (including the
        //   `secd`/`cold` Unknown ctrls that lead every first-section
        //   paragraph); each inline `\u{FFFC}` ControlRef position then
        //   popped the *wrong* control, dropping it. The real dutmal/shape
        //   controls leaked to the end-of-paragraph drain loop, reordering
        //   any text node that sat *between* two inline controls.
        let source = fixture_path("user_samples/sample-dutmal-basic.hwp");
        if !source.exists() {
            return;
        }
        let out = unique_temp_path("user-sample-dutmal-basic.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out).expect("dutmal conversion should succeed");
        assert!(
            !warnings.iter().any(|warning| matches!(warning, Hwp5Warning::DroppedControl { .. })),
            "dutmal must not drop any control: {warnings:?}"
        );
        assert_valid_hwpx(&out);

        let section_xml = read_section_xml(&out, 0);

        // Both dutmals must reach HWPX with their main/sub text intact.
        assert!(
            section_xml.contains("<hp:mainText>한국어</hp:mainText>")
                && section_xml.contains("<hp:subText>Korean</hp:subText>"),
            "first dutmal main/sub text must carry: {section_xml}"
        );
        assert!(
            section_xml.contains("<hp:mainText>韓字</hp:mainText>")
                && section_xml.contains("<hp:subText>한자</hp:subText>"),
            "second dutmal main/sub text must carry: {section_xml}"
        );

        // Bug A: `option_raw` must round-trip verbatim. The fixture has
        // option=0 on the first dutmal and option=4 on the second.
        assert!(
            section_xml.contains(r#"posType="TOP""#) && section_xml.contains(r#"option="0""#),
            "TOP dutmal must keep option=0 (Wave 12i Bug A): {section_xml}"
        );
        assert!(
            section_xml.contains(r#"posType="BOTTOM""#) && section_xml.contains(r#"option="4""#),
            "BOTTOM dutmal must carry option=4 verbatim (Wave 12i Bug A): {section_xml}"
        );

        // Bug B: the wire layout is `[TOP-marker][space][BOTTOM-marker]`,
        // so the body text space must remain *between* the two dutmals,
        // not jump ahead of the first one. Match the literal element
        // ordering on the wire — failing the order check is exactly the
        // visual regression the flat-path filter was added to prevent.
        let top_pos =
            section_xml.find(r#"<hp:dutmal posType="TOP""#).expect("TOP dutmal element must exist");
        let bottom_pos = section_xml
            .find(r#"<hp:dutmal posType="BOTTOM""#)
            .expect("BOTTOM dutmal element must exist");
        let space_pos = section_xml[top_pos..bottom_pos]
            .find("<hp:t> </hp:t>")
            .map(|rel| rel + top_pos)
            .expect("body space <hp:t> </hp:t> must sit between the two dutmals");
        assert!(
            top_pos < space_pos && space_pos < bottom_pos,
            "element order must be TOP-dutmal → space → BOTTOM-dutmal (Wave 12i Bug B)"
        );

        // HWPX → Core round-trip preserves both dutmals (counter added to
        // `DecodedShapeLayout` alongside this test).
        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let layout = collect_decoded_shape_layout(&decoded);
        assert_eq!(layout.dutmals, 2, "both dutmals should round-trip");

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_compose_basic_carries_composetext_and_char_pr_overrides() {
        // Wave 12j: a natively-authored 한컴 fixture with a single compose
        // (글자겹침) — `composeText="한韓"`, `circleType="SHAPE_CIRCLE"`,
        // `charSz="-3"`, `composeType="SPREAD"`, and 10 `<hp:charPr>` slots
        // with the first one carrying `prIDRef="7"` (a real override) and
        // the remaining 9 holding `u32::MAX` (the "no override" sentinel).
        //
        // This guards two earlier gaps that Wave 12j closed:
        //   1. The HWP5 leg used to drop `tcps` ctrls silently because they
        //      fell through to `Hwp5Control::Unknown`; the host paragraph
        //      ended up empty.
        //   2. The Core `Control::Compose` variant did not carry
        //      `char_pr_ids`, so even with the HWP5 leg working, the first
        //      slot's `prIDRef=7` was overwritten with `u32::MAX` at HWPX
        //      emit time.
        let source = fixture_path("user_samples/sample-compose-basic.hwp");
        if !source.exists() {
            return;
        }
        let out = unique_temp_path("user-sample-compose-basic.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out).expect("compose conversion should succeed");
        assert!(
            !warnings.iter().any(|warning| matches!(warning, Hwp5Warning::DroppedControl { .. })),
            "compose must not drop any control: {warnings:?}"
        );
        assert_valid_hwpx(&out);

        let section_xml = read_section_xml(&out, 0);
        assert!(
            section_xml.contains(r#"<hp:compose circleType="SHAPE_CIRCLE""#),
            "compose must emit circleType=SHAPE_CIRCLE: {section_xml}"
        );
        assert!(
            section_xml.contains(r#"charSz="-3""#)
                && section_xml.contains(r#"composeType="SPREAD""#)
                && section_xml.contains(r#"charPrCnt="10""#),
            "compose metadata attributes must carry: {section_xml}"
        );
        assert!(
            section_xml.contains(r#"composeText="한韓""#),
            "compose body text must carry verbatim: {section_xml}"
        );
        // Wave 12j Phase 2: char_pr_ids round-trip. The first slot was
        // `prIDRef="7"` in the truth fixture; the encoder used to emit
        // 10 × `u32::MAX` regardless of input.
        assert!(
            section_xml.contains(r#"<hp:charPr prIDRef="7"/>"#),
            "first charPr override (prIDRef=7) must round-trip: {section_xml}"
        );
        let placeholder_count = section_xml.matches(r#"<hp:charPr prIDRef="4294967295"/>"#).count();
        assert_eq!(
            placeholder_count, 9,
            "remaining 9 charPr slots must stay at u32::MAX: {section_xml}"
        );

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let layout = collect_decoded_shape_layout(&decoded);
        assert_eq!(layout.composes, 1, "exactly one Control::Compose should round-trip");

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_compose_all_shapes_handles_packed_wire_variant() {
        // Wave 12j Phase 3 regression gate. The `gen_compose_variants`
        // example authored an HWPX with every OWPML `circleType × composeType`
        // combination (14 × 2 = 28 compose elements, all sharing
        // `composeText="한韓"`); 한컴 saved that HWPX back to HWP5 round-trip.
        //
        // 27 of the 28 wire entries use the "unpacked" `tcps` layout
        // (composeText fully in `data[8..]`, `properties.low = 0x0003`).
        // The **CHAR + OVERLAP** combination is the only one 한컴 emitted
        // with the "packed" layout: `composeText[0]` is in
        // `properties.high` and `properties.low = 0x0002` doubles as the
        // text length. See
        // `.docs/algorithms/2026-06-01_compose_carry.md` for the full
        // discriminator table.
        //
        // Before the discriminator was added, the parser treated the
        // packed variant as unpacked and silently lost the first char
        // ("한" missing → `composeText="韓"`). This test asserts every
        // 28 entry round-trips its full `composeText="한韓"`, which
        // exactly catches that regression.
        let source = fixture_path("user_samples/sample-compose-all-shapes.hwp");
        if !source.exists() {
            return;
        }
        let out = unique_temp_path("user-sample-compose-all-shapes.hwpx");
        let warnings =
            hwp5_to_hwpx(&source, &out).expect("all-shapes compose conversion should succeed");
        assert!(
            !warnings.iter().any(|warning| matches!(warning, Hwp5Warning::DroppedControl { .. })),
            "no compose variant must drop: {warnings:?}"
        );
        assert_valid_hwpx(&out);

        let section_xml = read_section_xml(&out, 0);
        let compose_count = section_xml.matches("<hp:compose ").count();
        assert_eq!(compose_count, 28, "all 28 circleType × composeType variants must round-trip");

        // The packed-variant fix means *every* compose must keep the
        // full composeText. A single "한韓" miss would fail this check.
        let full_text_count = section_xml.matches(r#"composeText="한韓""#).count();
        assert_eq!(
            full_text_count, 28,
            "every compose must keep composeText=\"한韓\" (packed-variant regression gate)"
        );
        // Inverse check: zero variants should be stripped to single-char
        // "韓" (the pre-fix bug only affected the CHAR + OVERLAP entry).
        assert_eq!(
            section_xml.matches(r#"composeText="韓""#).count(),
            0,
            "no compose should be reduced to just \"韓\" (lost-first-char bug signature)"
        );

        // Round-trip through HWPX decoder confirms Core sees all 28 too.
        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let layout = collect_decoded_shape_layout(&decoded);
        assert_eq!(layout.composes, 28, "all 28 composes should reach Core::Compose");

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_indexmark_basic_carries_primary_only_entries() {
        // Wave 12k: a natively-authored 한컴 fixture with 2 IndexMarks,
        // both primary-only ("테스트" and "문장"). Guards three earlier
        // gaps Wave 12k closed in lock-step:
        //   1. `0x16` inline marker was silently consumed in
        //      `Hwp5ParaText::parse` — the indexmark Run would have
        //      ended up drained to end-of-paragraph instead of sitting
        //      at the body anchor.
        //   2. `idxm` CtrlHeader fell through to `Hwp5Control::Unknown`,
        //      so the projection had nothing to dispatch even when the
        //      marker reached it.
        //   3. Malformed `idxm` payloads used to bubble up as the
        //      generic `UnsupportedTag(0x47)` warning; the new path
        //      emits `DroppedControl { control: "indexmark", … }`
        //      instead (asserted indirectly by the absence of dropped
        //      controls on well-formed input).
        let source = fixture_path("user_samples/sample-indexmark-basic.hwp");
        if !source.exists() {
            return;
        }
        let out = unique_temp_path("user-sample-indexmark-basic.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out).expect("indexmark conversion should succeed");
        assert!(
            !warnings.iter().any(|warning| matches!(warning, Hwp5Warning::DroppedControl { .. })),
            "indexmark must not drop any control: {warnings:?}"
        );
        assert_valid_hwpx(&out);

        let section_xml = read_section_xml(&out, 0);
        assert!(
            section_xml.contains("<hp:indexmark><hp:firstKey>테스트</hp:firstKey></hp:indexmark>"),
            "first indexmark primary must carry verbatim: {section_xml}"
        );
        assert!(
            section_xml.contains("<hp:indexmark><hp:firstKey>문장</hp:firstKey></hp:indexmark>"),
            "second indexmark primary must carry verbatim: {section_xml}"
        );
        // Neither indexmark has a secondary in the truth fixture.
        assert!(
            !section_xml.contains("<hp:secondKey>"),
            "neither indexmark should emit <hp:secondKey>: {section_xml}"
        );

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let layout = collect_decoded_shape_layout(&decoded);
        assert_eq!(layout.index_marks, 2, "both indexmarks should round-trip");
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_indexmark_multi_preserves_order_and_secondary_keys() {
        // Wave 12k Phase 2 regression gate. The `gen_indexmark_variants`
        // example authored an HWPX with 8 IndexMarks across 7
        // paragraphs covering all the wire shapes that could regress:
        //   * primary-only / primary+secondary
        //   * 한글 / 영문 / 한자 / 혼합 키워드
        //   * two IndexMarks in the same paragraph (order regression
        //     for `0x16` ParaText + object_controls queue alignment)
        //   * source `Some("")` secondary normalized by 한컴 to None
        //     on HWP5 save
        // 한컴 saved that HWPX as HWP5; the assertions below confirm
        // every variant reaches HWPX intact and that the
        // CPU-then-GPU order survives the round trip.
        let source = fixture_path("user_samples/sample-indexmark-multi.hwp");
        if !source.exists() {
            return;
        }
        let out = unique_temp_path("user-sample-indexmark-multi.hwpx");
        let warnings =
            hwp5_to_hwpx(&source, &out).expect("multi indexmark conversion should succeed");
        assert!(
            !warnings.iter().any(|warning| matches!(warning, Hwp5Warning::DroppedControl { .. })),
            "no indexmark variant must drop: {warnings:?}"
        );
        assert_valid_hwpx(&out);

        let section_xml = read_section_xml(&out, 0);
        let indexmark_count = section_xml.matches("<hp:indexmark>").count();
        assert_eq!(indexmark_count, 8, "all 8 indexmark variants must round-trip");

        // Primary-only entries.
        for primary in ["컴퓨터", "韓國", "CPU", "GPU", "네트워크"] {
            let needle = format!("<hp:firstKey>{primary}</hp:firstKey>");
            assert!(
                section_xml.contains(&needle),
                "primary '{primary}' must reach HWPX: {section_xml}"
            );
        }

        // Primary + secondary pairings carry both keys.
        for (primary, secondary) in [("컴퓨터", "하드웨어"), ("Memory", "RAM"), ("운영체제", "OS")]
        {
            let needle = format!(
                "<hp:firstKey>{primary}</hp:firstKey><hp:secondKey>{secondary}</hp:secondKey>"
            );
            assert!(
                section_xml.contains(&needle),
                "primary+secondary pair '{primary}/{secondary}' must carry: {section_xml}"
            );
        }

        // The source for "네트워크" was `Some("")`; 한컴 normalized it
        // to `None` on save, so the output must NOT contain a
        // `<hp:secondKey>` for that primary.
        let net_idx = section_xml
            .find(r#"<hp:firstKey>네트워크</hp:firstKey>"#)
            .expect("네트워크 indexmark must exist");
        let after_net = &section_xml[net_idx..];
        let close_tag = after_net.find("</hp:indexmark>").expect("close tag");
        let net_element = &after_net[..close_tag];
        assert!(
            !net_element.contains("<hp:secondKey>"),
            "Some(\"\") secondary should normalize to absent <hp:secondKey>: {net_element}"
        );

        // Order regression: CPU-then-GPU on the same paragraph. If
        // the `0x16` marker handling or the projection queue dispatch
        // drifts, those two would swap or collapse onto end-of-paragraph.
        let cpu_pos =
            section_xml.find("<hp:firstKey>CPU</hp:firstKey>").expect("CPU indexmark must exist");
        let gpu_pos =
            section_xml.find("<hp:firstKey>GPU</hp:firstKey>").expect("GPU indexmark must exist");
        assert!(
            cpu_pos < gpu_pos,
            "CPU indexmark must appear before GPU (queue-order regression gate)"
        );

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let layout = collect_decoded_shape_layout(&decoded);
        assert_eq!(layout.index_marks, 8, "all 8 indexmarks must reach Core");
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_equation_native_carries_complex_scripts() {
        // Wave 12d: a richer, natively-authored 한컴 document with TWO equations
        // (Fourier series + binomial expansion). Stresses sum/subscript/
        // superscript/fraction syntax, backtick spacing markers, and the U+2026
        // ellipsis — confirming the EQEDIT script parse holds for native
        // authoring and special characters, not just our round-tripped fixture.
        let source = fixture_path("user_samples/sample-equation-native.hwp");
        if !source.exists() {
            return;
        }
        let out = unique_temp_path("user-sample-equation-native.hwpx");
        let warnings =
            hwp5_to_hwpx(&source, &out).expect("native equation conversion should succeed");
        assert!(
            !warnings.iter().any(|warning| matches!(warning, Hwp5Warning::DroppedControl { .. })),
            "native equations must not drop any control: {warnings:?}"
        );
        assert_valid_hwpx(&out);

        let section_xml = read_section_xml(&out, 0);
        assert!(
            section_xml.contains("sum _{n=1} ^{INF"),
            "sum + subscript + superscript script must carry"
        );
        assert!(section_xml.contains("over {L}"), "fraction script must carry");
        assert!(
            section_xml.contains('…'),
            "the U+2026 ellipsis must survive the UTF-16 → UTF-8 round trip"
        );

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let layout = collect_decoded_shape_layout(&decoded);
        assert_eq!(layout.equations, 2, "both native equations should round-trip");

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_page_border_fill_references_visible_border() {
        // Wave 7: the section's BOTH page border must reference the real
        // (solid) borderFill, not the invisible default (id=1) the encoder
        // fabricated before the secd HWPTAG_PAGE_BORDER_FILL records were
        // carried. See `.docs/debug/2026-05-29_hwp5_page_border_fill.md`.
        let source = fixture_path("user_samples/pages/sample-page-border-fill.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-page-border-fill.hwpx");
        hwp5_to_hwpx(&source, &out).expect("page border fill conversion should succeed");
        assert_valid_hwpx(&out);

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let section = &decoded.document.sections()[0];
        let entries =
            section.page_border_fills.as_ref().expect("section should carry page border fills");
        let both = entries
            .iter()
            .find(|entry| entry.apply_type == "BOTH")
            .expect("a BOTH page border fill entry should exist");
        let border_fill = decoded
            .style_store
            .border_fill(both.border_fill_id)
            .expect("the referenced page border fill must exist in the style store");
        assert!(
            [&border_fill.left, &border_fill.right, &border_fill.top, &border_fill.bottom]
                .iter()
                .any(|side| side.line_type != "NONE"),
            "BOTH page border must reference a visible (non-NONE) border fill: {border_fill:?}"
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_page_border_odd_even_distinct_line_types() {
        // Locks two things at once:
        // 1. EVEN/ODD page-border-fill records are mapped in the right order
        //    (the truth has EVEN = dotted, ODD = solid — distinct, so a swap
        //    would be caught).
        // 2. The HWP5 border line kind 2 = 점선 decodes to DOT, not DASH
        //    (from_raw codes 2/3 were swapped). See task #41.
        let source = fixture_path("user_samples/pages/sample-page-border-odd-even.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-page-border-odd-even.hwpx");
        hwp5_to_hwpx(&source, &out).expect("odd/even border conversion should succeed");
        assert_valid_hwpx(&out);

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let entries = decoded.document.sections()[0]
            .page_border_fills
            .as_ref()
            .expect("section should carry page border fills");

        let line_type_of = |apply: &str| -> String {
            let entry =
                entries.iter().find(|e| e.apply_type == apply).expect("apply_type entry exists");
            decoded
                .style_store
                .border_fill(entry.border_fill_id)
                .expect("referenced border fill exists")
                .top
                .line_type
                .clone()
        };

        assert_eq!(line_type_of("EVEN"), "DOT", "EVEN page border must be dotted (점선 → DOT)");
        assert_eq!(line_type_of("ODD"), "SOLID", "ODD page border must be solid (실선)");

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_page_border_pattern_carries_double_line_and_gradient() {
        // Regression lock: a double-line (이중선) border + gradient background
        // carry through DocInfo borderFill decode (line family + gradient fill).
        let source = fixture_path("user_samples/pages/sample-page-border-pattern.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-page-border-pattern.hwpx");
        hwp5_to_hwpx(&source, &out).expect("pattern border conversion should succeed");
        assert_valid_hwpx(&out);

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let entries = decoded.document.sections()[0]
            .page_border_fills
            .as_ref()
            .expect("section should carry page border fills");
        let both = entries
            .iter()
            .find(|e| e.apply_type == "BOTH")
            .expect("a BOTH page border fill entry should exist");
        let border_fill = decoded
            .style_store
            .border_fill(both.border_fill_id)
            .expect("referenced border fill exists");
        assert_eq!(
            border_fill.top.line_type, "DOUBLE_SLIM",
            "double-line border (이중선) must carry as DOUBLE_SLIM"
        );
        assert!(
            border_fill.gradient_fill.is_some(),
            "gradient background must carry as a gradient fill: {border_fill:?}"
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_multi_section_preserves_sections_and_orientation() {
        // Regression lock: HWP5 multi-section already carries (two sections,
        // second one landscape). Keep it that way.
        let source = fixture_path("user_samples/pages/sample-multi-section.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-multi-section.hwpx");
        hwp5_to_hwpx(&source, &out).expect("multi-section conversion should succeed");
        assert_valid_hwpx(&out);

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let sections = decoded.document.sections();
        assert_eq!(sections.len(), 2, "expected two sections");
        assert!(!sections[0].page_settings.landscape, "first section should stay portrait");
        assert!(sections[1].page_settings.landscape, "second section should be landscape");

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_char_line_variants_carry_line_families() {
        // Regression lock: underline/strikeout line families (double, wave,
        // dot, dash) already carry through HwpxCharShape. Keep it that way.
        use hwpforge_foundation::{StrikeoutShape, UnderlineShape};

        let source = fixture_path("user_samples/sample-char-line-variants.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-char-line-variants.hwpx");
        hwp5_to_hwpx(&source, &out).expect("char line variants conversion should succeed");
        assert_valid_hwpx(&out);

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let underline_shapes: Vec<UnderlineShape> =
            decoded.style_store.iter_char_shapes().map(|cs| cs.underline_shape).collect();
        let strikeout_shapes: Vec<StrikeoutShape> =
            decoded.style_store.iter_char_shapes().map(|cs| cs.strikeout_shape).collect();
        assert!(
            underline_shapes.contains(&UnderlineShape::DoubleSlim),
            "double underline must carry: {underline_shapes:?}"
        );
        assert!(
            underline_shapes.contains(&UnderlineShape::Wave),
            "wave underline must carry: {underline_shapes:?}"
        );
        assert!(
            strikeout_shapes.contains(&StrikeoutShape::DoubleSlim),
            "double strikeout must carry: {strikeout_shapes:?}"
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_fwspace_carries_fixed_width_space() {
        // Truth fixture is a single paragraph: FWLEFT<hp:fwSpace/>FWRIGHT.
        // Before the fix, the HWP5 wire-byte 0x1F was silently consumed and
        // the surrounding text was concatenated into "FWLEFTFWRIGHT".
        let source = fixture_path("user_samples/text/sample-fwspace-fixed.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-fwspace.hwpx");
        let warnings =
            hwp5_to_hwpx(&source, &out).expect("user sample fwspace conversion should succeed");
        assert!(
            warnings.is_empty(),
            "fwspace fixture should convert without warnings: {warnings:?}",
        );

        assert_valid_hwpx(&out);

        // Round-trip through the Core DOM: text run must carry the U+001F
        // sentinel between the two marker strings.
        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let para = &decoded.document.sections()[0].paragraphs[0];
        let visible_text: String = para.runs.iter().filter_map(|r| r.content.as_text()).collect();
        assert_eq!(
            visible_text, "FWLEFT\u{001F}FWRIGHT",
            "Core text must carry the U+001F sentinel between the markers"
        );

        // Wire-level check: exactly one `<hp:fwSpace/>` is emitted inline.
        let section_xml = read_section_xml(&out, 0);
        let fwspace_count = section_xml.matches("<hp:fwSpace").count();
        assert_eq!(
            fwspace_count, 1,
            "expected exactly 1 <hp:fwSpace/> element to match the truth fixture; got {fwspace_count} in:\n{section_xml}"
        );
        assert!(
            section_xml.contains("<hp:t>FWLEFT<hp:fwSpace/>FWRIGHT</hp:t>"),
            "fwSpace must be emitted inline inside the same <hp:t> as the surrounding text"
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_tab_preserves_inline_tab_text_and_custom_tab_def() {
        let source = fixture_path("user_samples/tabs/sample-tab.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-tab.hwpx");
        let warnings =
            hwp5_to_hwpx(&source, &out).expect("user sample tab conversion should succeed");
        // Phase 2 closes the inline-tab carry: every `<hp:tab>` with
        // non-default `width` / `leader` / `tab_type` now rides through
        // Core via `RunContent::InlineText`, so the conversion should
        // produce zero warnings again.
        assert!(
            warnings.is_empty(),
            "controlled tab fixture should convert without warnings, saw {warnings:?}"
        );

        assert_valid_hwpx(&out);

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        assert!(
            decoded.style_store.iter_tabs().any(|tab| tab.id > 2 && !tab.stops.is_empty()),
            "converted HWP5 tab fixture should keep an explicit custom tab definition"
        );

        let para = &decoded.document.sections()[0].paragraphs[0];
        // Wave 4 Phase 3: HWP5→HWPX→Core round-trip now upgrades the
        // run to `InlineText` because the tab carries non-default
        // attributes. Use `plain_text()` to validate the visible
        // string and `as_inline_text()` to confirm the attributes
        // survive both encode and decode steps.
        assert_eq!(para.runs[0].content.plain_text().as_deref(), Some("LEFT\tRIGHT"));
        let inline = para.runs[0]
            .content
            .as_inline_text()
            .expect("non-default inline tab should land in `RunContent::InlineText`");
        let tab = inline
            .segments
            .iter()
            .find_map(|seg| match seg {
                hwpforge_core::inline::InlineSegment::Tab(attr) => Some(attr),
                _ => None,
            })
            .expect("InlineText should keep a Tab segment");
        assert_eq!(tab.width.as_i32(), 12488, "inline tab width should round-trip");
        assert_eq!(tab.leader, 3, "inline tab leader should round-trip");
        assert_eq!(tab.tab_type, 1, "inline tab type should round-trip");

        let para_shape =
            decoded.style_store.para_shape(para.para_shape_id).expect("para shape should exist");
        assert!(
            para_shape.tab_pr_id_ref > 2,
            "paragraph should reference a converted custom tab definition"
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_table_tab_preserves_inline_tab_text_in_cell() {
        let source = fixture_path("user_samples/tabs/sample-table-tab.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-table-tab.hwpx");
        let warnings =
            hwp5_to_hwpx(&source, &out).expect("user sample table tab conversion should succeed");
        // Phase 2: inline tab carry restored full parity here too.
        assert!(
            warnings.is_empty(),
            "controlled table-tab fixture should convert without warnings, saw {warnings:?}"
        );

        assert_valid_hwpx(&out);

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let table = decoded.document.sections()[0]
            .paragraphs
            .iter()
            .flat_map(|para| &para.runs)
            .find_map(|run| run.content.as_table())
            .expect("expected a table");
        // Wave 4 Phase 3 round-trip parity for cell-level inline tabs.
        assert_eq!(
            table.rows[0].cells[0].paragraphs[0].runs[0].content.plain_text().as_deref(),
            Some("CELLLEFT\tCELLRIGHT")
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_bullet_list_preserves_bullet_semantics() {
        let source = fixture_path("user_samples/lists/sample-bullet-list.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-bullet-list.hwpx");
        let warnings =
            hwp5_to_hwpx(&source, &out).expect("user sample bullet list conversion should succeed");
        assert!(warnings.is_empty(), "bullet list fixture should convert without warnings");

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let headings = collect_decoded_body_heading_triples(&decoded);
        assert!(headings.contains(&(HeadingType::Bullet, 1, 0)));
        assert_eq!(decoded.style_store.bullet_count(), 1);

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_checkable_multiline_preserves_item_state_and_continuation() {
        let source = fixture_path("user_samples/sample-checkable-bullet-multiline.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-checkable-bullet-multiline.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out)
            .expect("user sample checkable multiline conversion should succeed");
        assert!(warnings.is_empty(), "checkable multiline fixture should convert without warnings");

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let paragraphs = &decoded.document.sections()[0].paragraphs;
        assert_eq!(paragraphs.len(), 3, "fixture should stay as exactly 3 body paragraphs");

        let unchecked = paragraphs
            .iter()
            .find(|paragraph| paragraph.text_content().contains("unchecked item A first paragraph"))
            .expect("fixture should contain unchecked task item");
        let continuation = paragraphs
            .iter()
            .find(|paragraph| {
                paragraph.text_content().contains("second paragraph of the same item")
            })
            .expect("fixture should contain continuation paragraph");
        let checked = paragraphs
            .iter()
            .find(|paragraph| paragraph.text_content().contains("checked item B"))
            .expect("fixture should contain checked task item");

        let unchecked_shape = decoded.style_store.para_shape(unchecked.para_shape_id).unwrap();
        let continuation_shape =
            decoded.style_store.para_shape(continuation.para_shape_id).unwrap();
        let checked_shape = decoded.style_store.para_shape(checked.para_shape_id).unwrap();
        assert_eq!(unchecked_shape.heading_type, HeadingType::Bullet);
        assert_eq!(unchecked_shape.heading_level, 0);
        assert!(!unchecked_shape.checked);
        assert_eq!(checked_shape.heading_type, HeadingType::Bullet);
        assert_eq!(checked_shape.heading_level, 0);
        assert!(checked_shape.checked);
        assert_eq!(continuation_shape.heading_type, HeadingType::None);
        assert_eq!(continuation_shape.heading_id_ref, 0);
        assert!(!continuation_shape.checked);
        assert!(continuation_shape.margin_left.as_i32() > 0);

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_numbered_list_preserves_numbering_semantics() {
        let source = fixture_path("user_samples/lists/sample-numbered-list.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-numbered-list.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out)
            .expect("user sample numbered list conversion should succeed");
        assert!(warnings.is_empty(), "numbered list fixture should convert without warnings");

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let headings = collect_decoded_body_heading_triples(&decoded);
        assert!(headings.contains(&(HeadingType::Number, 2, 0)));
        assert!(decoded.style_store.numbering_count() >= 2);

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_hyperlink_preserves_field_control_and_surrounding_text() {
        let source = fixture_path("user_samples/sample-field-hyperlink-surrounding-text-basic.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-field-hyperlink-surrounding-text-basic.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out)
            .expect("user sample hyperlink surrounding text conversion should succeed");
        assert!(
            warnings.is_empty(),
            "hyperlink surrounding text fixture should convert without warnings: {warnings:?}"
        );

        assert_valid_hwpx(&out);

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let hyperlink_paragraph = decoded.document.sections()[0]
            .paragraphs
            .iter()
            .find(|paragraph| {
                paragraph.runs.iter().any(|run| {
                    matches!(
                        run.content.as_control(),
                        Some(Control::Hyperlink { text, url })
                            if text == "OpenAI" && url == "https://openai.com"
                    )
                })
            })
            .expect("fixture should produce a hyperlink control paragraph");
        let hyperlink_index = hyperlink_paragraph
            .runs
            .iter()
            .position(|run| {
                matches!(
                    run.content.as_control(),
                    Some(Control::Hyperlink { text, url })
                        if text == "OpenAI" && url == "https://openai.com"
                )
            })
            .expect("paragraph must contain the hyperlink control");
        assert_eq!(
            joined_text_runs(hyperlink_paragraph.runs[..hyperlink_index].iter()),
            "링크: ",
            "plain text before the hyperlink must remain outside the field control"
        );
        assert_eq!(
            joined_text_runs(hyperlink_paragraph.runs[hyperlink_index + 1..].iter()),
            " 바로가기",
            "plain text after the hyperlink must remain outside the field control"
        );

        let section_xml = read_section_xml(&out, 0);
        assert!(
            section_xml.contains("<hp:fieldBegin") && section_xml.contains(r#"type="HYPERLINK""#),
            "converted section xml must carry an HYPERLINK fieldBegin"
        );
        assert!(
            section_xml.contains("<hp:t>OpenAI</hp:t>"),
            "converted section xml must keep hyperlink display text"
        );
        assert!(
            section_xml
                .contains(r#"<hp:stringParam name="Path">https://openai.com</hp:stringParam>"#,),
            "converted section xml must keep hyperlink url"
        );
        assert!(
            section_xml.contains("<hp:fieldBegin")
                && section_xml.contains(r#"type="BOOKMARK""#)
                && section_xml.contains(r#"name="target_span_1""#),
            "combined fixture must preserve the span bookmark field begin"
        );
        assert!(
            section_xml.contains("<hp:fieldBegin") && section_xml.contains(r#"type="CROSSREF""#),
            "combined fixture must preserve the cross-reference field"
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_footnote_carries_four_footnotes() {
        let source = fixture_path("user_samples/sample-field-footnote.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-field-footnote.hwpx");
        let warnings =
            hwp5_to_hwpx(&source, &out).expect("user sample footnote conversion should succeed");
        assert!(
            warnings.is_empty(),
            "footnote fixture should convert without warnings: {warnings:?}"
        );

        assert_valid_hwpx(&out);

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");

        // Count Control::Footnote instances across the projected Core document.
        let footnote_count: usize = decoded.document.sections()[0]
            .paragraphs
            .iter()
            .flat_map(|paragraph| &paragraph.runs)
            .filter(|run| matches!(run.content.as_control(), Some(Control::Footnote { .. })))
            .count();
        assert_eq!(
            footnote_count, 4,
            "fixture sample-field-footnote.hwp should round-trip exactly four footnote controls"
        );

        // Also assert the encoded HWPX carries four <hp:footNote> elements
        // (separate from the <hp:footNotePr> section property).
        let section_xml = read_section_xml(&out, 0);
        let footnote_element_count = section_xml.matches("<hp:footNote ").count()
            + section_xml.matches("<hp:footNote>").count();
        assert_eq!(
            footnote_element_count, 4,
            "converted hwpx must emit four <hp:footNote> elements"
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_bookmark_crossref_preserves_controls() {
        let source = fixture_path("user_samples/sample-field-bookmark-crossref-basic.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-field-bookmark-crossref-basic.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out)
            .expect("user sample bookmark/crossref conversion should succeed");
        assert!(
            warnings.is_empty(),
            "bookmark/crossref fixture should convert without warnings: {warnings:?}"
        );

        assert_valid_hwpx(&out);

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let controls: Vec<&Control> = decoded.document.sections()[0]
            .paragraphs
            .iter()
            .flat_map(|paragraph| &paragraph.runs)
            .filter_map(|run| run.content.as_control())
            .collect();
        assert!(
            controls.iter().any(|control| {
                matches!(
                    control,
                    Control::Bookmark {
                        name,
                        bookmark_type: BookmarkType::Point,
                    } if name == "target1"
                )
            }),
            "fixture should produce a point bookmark control named target1"
        );
        let section_xml = read_section_xml(&out, 0);
        assert!(
            section_xml.contains(r#"<hp:bookmark name="target1"/>"#),
            "converted section xml must keep the point bookmark"
        );
        assert!(
            section_xml.contains("<hp:fieldBegin") && section_xml.contains(r#"type="CROSSREF""#),
            "converted section xml must keep the cross-reference field"
        );
        assert!(
            section_xml.contains(r#"<hp:stringParam name="RefPath">?target1;</hp:stringParam>"#),
            "converted cross-reference must target the bookmark name"
        );
        assert!(
            section_xml
                .contains(r#"<hp:stringParam name="RefType">TARGET_BOOKMARK</hp:stringParam>"#),
            "converted cross-reference must keep bookmark reference type"
        );
        assert!(
            section_xml.contains(
                r#"<hp:stringParam name="RefContentType">OBJECT_TYPE_PAGE</hp:stringParam>"#,
            ),
            "converted cross-reference must keep page content type"
        );
        assert!(
            section_xml.contains("<hp:t>1</hp:t>"),
            "converted cross-reference must keep its visible text"
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_crossref_emits_nonzero_fieldid() {
        let source = fixture_path("user_samples/sample-field-bookmark-crossref-basic.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-crossref-nonzero-fieldid.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out)
            .expect("user sample bookmark/crossref conversion should succeed");
        assert!(
            warnings.is_empty(),
            "bookmark/crossref fixture should convert without warnings: {warnings:?}"
        );

        let section_xml = read_section_xml(&out, 0);

        // fieldid="0" makes Hancom treat the CROSSREF as an invalid instance
        // (F9 refresh / Ctrl+click jump break). It must never be emitted.
        assert!(
            !section_xml.contains(r#"fieldid="0""#),
            "converted section xml must not emit fieldid=0 anywhere"
        );

        // The CROSSREF fieldBegin and its matching fieldEnd must carry the
        // same non-zero fieldid.
        let begin_marker = "<hp:fieldBegin ";
        let begin_pos =
            section_xml.find(begin_marker).expect("section xml must contain a fieldBegin");
        let begin_tag_end = section_xml[begin_pos..]
            .find('>')
            .map(|rel| begin_pos + rel)
            .expect("fieldBegin tag must be terminated");
        let begin_tag = &section_xml[begin_pos..=begin_tag_end];
        assert!(begin_tag.contains(r#"type="CROSSREF""#), "first field must be CROSSREF");

        let extract_fieldid = |tag: &str| -> String {
            let needle = r#"fieldid=""#;
            let start =
                tag.find(needle).expect("tag must carry a fieldid attribute") + needle.len();
            let end = tag[start..].find('"').expect("fieldid attribute must be quoted") + start;
            tag[start..end].to_string()
        };

        let begin_fieldid = extract_fieldid(begin_tag);
        assert_ne!(begin_fieldid, "0", "CROSSREF fieldBegin fieldid must be non-zero");

        let end_marker = "<hp:fieldEnd ";
        let end_pos = section_xml.find(end_marker).expect("section xml must contain a fieldEnd");
        let end_tag_end = section_xml[end_pos..]
            .find('>')
            .map(|rel| end_pos + rel)
            .expect("fieldEnd tag must be terminated");
        let end_tag = &section_xml[end_pos..=end_tag_end];
        let end_fieldid = extract_fieldid(end_tag);

        assert_eq!(
            begin_fieldid, end_fieldid,
            "CROSSREF fieldBegin and fieldEnd must share the same fieldid"
        );

        // Hancom reads `fieldBegin id` as a signed 32-bit integer. A value at
        // or above 2^31 wraps negative and the field is no longer recognized
        // (click / F9 refresh / Ctrl+click jump silently fail). The id must be
        // a positive integer strictly below i32::MAX + 1.
        let extract_attr = |tag: &str, attr: &str| -> String {
            let needle = format!("{attr}=\"");
            let start =
                tag.find(&needle).unwrap_or_else(|| panic!("tag must carry a {attr} attribute"))
                    + needle.len();
            let end =
                tag[start..].find('"').unwrap_or_else(|| panic!("{attr} attribute must be quoted"))
                    + start;
            tag[start..end].to_string()
        };

        let begin_id: i64 = extract_attr(begin_tag, "id")
            .parse()
            .expect("CROSSREF fieldBegin id must be an integer");
        assert!(begin_id > 0, "CROSSREF fieldBegin id must be a positive integer: {begin_id}");
        assert!(
            begin_id < 2_147_483_648,
            "CROSSREF fieldBegin id must be below 2^31 (signed i32 range): {begin_id}"
        );

        let begin_id_ref = extract_attr(end_tag, "beginIDRef");
        assert_eq!(
            begin_id_ref,
            begin_id.to_string(),
            "CROSSREF fieldEnd beginIDRef must reference the fieldBegin id"
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_page_number_preserves_section_page_number() {
        let source = fixture_path("user_samples/sample-field-page-number-basic.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-field-page-number-basic.hwpx");
        let warnings =
            hwp5_to_hwpx(&source, &out).expect("user sample page number conversion should succeed");
        assert!(
            warnings.is_empty(),
            "page number fixture should convert without warnings: {warnings:?}"
        );

        assert_valid_hwpx(&out);

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let section = &decoded.document.sections()[0];
        let page_number = section.page_number.as_ref().expect("section must carry a page number");
        assert_eq!(page_number.position, PageNumberPosition::BottomCenter);
        assert_eq!(page_number.number_format, NumberFormatType::Digit);
        assert_eq!(page_number.decoration, "-");

        let section_xml = read_section_xml(&out, 0);
        assert!(
            section_xml
                .contains(r#"<hp:pageNum pos="BOTTOM_CENTER" formatType="DIGIT" sideChar="-""#),
            "converted section xml must inject a page number control"
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_mixed_lists_preserves_all_list_kinds() {
        let source = fixture_path("user_samples/lists/sample-mixed-lists-with-outline.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-mixed-lists.hwpx");
        let warnings =
            hwp5_to_hwpx(&source, &out).expect("user sample mixed list conversion should succeed");
        assert!(warnings.is_empty(), "mixed list fixture should convert without warnings");

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let headings = collect_decoded_body_heading_triples(&decoded);
        assert!(headings.contains(&(HeadingType::Outline, 0, 0)));
        assert!(headings.contains(&(HeadingType::Bullet, 1, 0)));
        assert!(headings.contains(&(HeadingType::Number, 2, 0)));
        assert!(headings.contains(&(HeadingType::Number, 3, 0)));
        assert_eq!(decoded.style_store.bullet_count(), 1);
        assert!(decoded.style_store.numbering_count() >= 3);

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_custom_number_formats_keep_distinct_ids() {
        let source = fixture_path("user_samples/lists/sample-numbered-list-custom-formats.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-numbered-custom-formats.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out)
            .expect("user sample custom numbering conversion should succeed");
        assert!(warnings.is_empty(), "custom numbering fixture should convert without warnings");

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let headings = collect_decoded_body_heading_triples(&decoded);
        for id_ref in [2, 3, 4, 5] {
            assert!(headings.contains(&(HeadingType::Number, id_ref, 0)));
        }
        assert!(decoded.style_store.numbering_count() >= 5);
        let numberings: Vec<_> = decoded.style_store.iter_numberings().collect();
        assert_eq!(numberings[1].levels[0].text, "^1)");
        assert_eq!(numberings[2].levels[0].text, "(^1)");
        assert_eq!(numberings[3].levels[2].text, "(^3)");
        assert_eq!(numberings[4].levels[0].num_format, NumberFormatType::LatinCapital);
        assert_eq!(numberings[4].levels[6].num_format, NumberFormatType::CircledLatinSmall);
        assert_eq!(numberings[4].levels[6].text, "^7");

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_multilevel_list_projects_as_outline_levels() {
        let source = fixture_path("user_samples/lists/sample-numbered-list-multilevel.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-numbered-multilevel.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out)
            .expect("user sample multilevel numbering conversion should succeed");
        assert!(
            warnings.is_empty(),
            "multilevel numbering fixture should convert without warnings"
        );

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let headings = collect_decoded_body_heading_triples(&decoded);
        let outline_levels: Vec<_> = headings
            .iter()
            .filter_map(|(heading_type, id_ref, level)| {
                if *heading_type == HeadingType::Outline && *id_ref == 0 {
                    Some(*level)
                } else {
                    None
                }
            })
            .collect();
        assert!(
            outline_levels.len() >= 2,
            "fixture should project at least two outline paragraphs"
        );
        assert!(outline_levels.contains(&0), "fixture should preserve first outline level as 0");
        assert!(
            outline_levels.iter().any(|level| *level > 0),
            "fixture should preserve nested outline levels"
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_underline_variants_preserves_all_shapes() {
        use hwpforge_foundation::UnderlineShape;

        let source = fixture_path("user_samples/sample-char-underline-variants.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-underline-variants.hwpx");
        let warnings =
            hwp5_to_hwpx(&source, &out).expect("underline variants conversion should succeed");

        // After Wave 1b the underline_shape warning is replaced by actual carry.
        assert!(
            !warnings.iter().any(|w| matches!(
                w,
                crate::decoder::Hwp5Warning::ProjectionFallback { subject, .. }
                    if *subject == "style.char_shape.underline_shape"
            )),
            "underline_shape ProjectionFallback must not fire after Wave 1b carry"
        );

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let paragraphs = &decoded.document.sections()[0].paragraphs;

        // Fixture has 5 paragraphs: SOLID, DOUBLE_SLIM, DASH, WAVE, SLIM_THICK.
        assert!(
            paragraphs.len() >= 5,
            "fixture should have at least 5 paragraphs, got {}",
            paragraphs.len()
        );

        // Actual shape ordering as encoded in the HWP5 fixture (verified by inspection):
        // para[0]=Solid, para[1]=DoubleSlim, para[2]=Dot, para[3]=Wave, para[4]=SlimThick
        let expected_shapes = [
            UnderlineShape::Solid,
            UnderlineShape::DoubleSlim,
            UnderlineShape::Dot,
            UnderlineShape::Wave,
            UnderlineShape::SlimThick,
        ];

        for (i, expected) in expected_shapes.iter().enumerate() {
            let para = &paragraphs[i];
            let run = para.runs.first().expect("paragraph must have at least one run");
            let cs =
                decoded.style_store.char_shape(run.char_shape_id).expect("char shape must exist");
            assert_eq!(
                cs.underline_shape, *expected,
                "paragraph {} (0-based) expected underline_shape {:?}, got {:?}",
                i, expected, cs.underline_shape
            );
        }

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_strike_variants_preserves_line_family() {
        use hwpforge_foundation::StrikeoutShape;

        let source = fixture_path("user_samples/sample-char-strike-variants.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-strike-variants.hwpx");
        let warnings =
            hwp5_to_hwpx(&source, &out).expect("strike variants conversion should succeed");

        // Wave 1c: the strike line family is now carried, so the projection
        // fallback warning for strike_shape must not fire.
        assert!(
            !warnings.iter().any(|w| matches!(
                w,
                crate::decoder::Hwp5Warning::ProjectionFallback { subject, .. }
                    if *subject == "style.char_shape.strike_shape"
            )),
            "style.char_shape.strike_shape ProjectionFallback must not fire after Wave 1c carry"
        );

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");

        // The fixture defines three strike variants on char shapes 7/8/9:
        //   charPr 7 = "단일선" → SOLID
        //   charPr 8 = "이중선" → DOUBLE_SLIM (HWP5 raw shape = 7)
        //   charPr 9 = "빨간선" → SOLID with non-black strike_color
        // Verify the style store carries the line family for each.
        let cs7 = decoded
            .style_store
            .char_shape(hwpforge_foundation::CharShapeIndex::new(7))
            .expect("char shape 7 must exist");
        assert_eq!(cs7.strikeout_shape, StrikeoutShape::Solid);

        let cs8 = decoded
            .style_store
            .char_shape(hwpforge_foundation::CharShapeIndex::new(8))
            .expect("char shape 8 must exist");
        assert_eq!(
            cs8.strikeout_shape,
            StrikeoutShape::DoubleSlim,
            "char shape 8 should carry DoubleSlim (raw=7) after Wave 1c"
        );

        let cs9 = decoded
            .style_store
            .char_shape(hwpforge_foundation::CharShapeIndex::new(9))
            .expect("char shape 9 must exist");
        assert_eq!(cs9.strikeout_shape, StrikeoutShape::Solid);

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_breakwordlatin_variants_preserves_hyphenation() {
        use hwpforge_foundation::{ParaShapeIndex, WordBreakType};

        let source = fixture_path("user_samples/sample-char-breakwordlatin-variants.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-breakwordlatin-variants.hwpx");
        let warnings =
            hwp5_to_hwpx(&source, &out).expect("breakwordlatin variants conversion should succeed");

        // After Wave 1d carry, the break_latin_word projection warning is gone
        // for the raw=1 (HYPHENATION) and raw=2 (BREAK_WORD) cases.
        assert!(
            !warnings.iter().any(|w| matches!(
                w,
                crate::decoder::Hwp5Warning::ProjectionFallback { subject, .. }
                    if *subject == "style.para_shape.break_latin_word"
            )),
            "style.para_shape.break_latin_word ProjectionFallback must not fire after Wave 1d carry"
        );

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");

        // The .hwp fixture defines 21 paragraph shapes:
        //   - indices 0..=19 carry the default raw=0 (KEEP_WORD)
        //   - index 20 carries raw=2 (BREAK_WORD per the HWP5 spec bits 5-6:
        //     0=Word, 1=Hyphenate, 2=Character)
        // Raw=1 (HYPHENATION) is therefore not exercised by this fixture's
        // HWP5 body, but the projection now carries it through whenever a
        // raw=1 shape appears (the foundation enum and HWPX encoder/decoder
        // are wired end-to-end and covered by the foundation unit tests).
        let shape_19 = decoded
            .style_store
            .para_shape(ParaShapeIndex::new(19))
            .expect("para shape 19 must exist");
        assert_eq!(shape_19.break_latin_word, WordBreakType::KeepWord);

        let shape_20 = decoded
            .style_store
            .para_shape(ParaShapeIndex::new(20))
            .expect("para shape 20 must exist");
        assert_eq!(shape_20.break_latin_word, WordBreakType::BreakWord);

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_line_spacing_preserves_all_modes() {
        use hwpforge_foundation::{LineSpacingType, ParaShapeIndex};

        let source = fixture_path("user_samples/sample-para-line-spacing.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-line-spacing.hwpx");
        let warnings = hwp5_to_hwpx(&source, &out).expect("line-spacing conversion should succeed");

        // Wave 2a: AtLeast is now a first-class variant, so the
        // ProjectionFallback warning for raw=3 must no longer fire.
        assert!(
            !warnings.iter().any(|w| matches!(
                w,
                crate::decoder::Hwp5Warning::ProjectionFallback { subject, .. }
                    if *subject == "style.para_shape.line_spacing"
            )),
            "style.para_shape.line_spacing ProjectionFallback must not fire after Wave 2a carry"
        );

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");

        // The HWPX fixture defines paraPr entries with three distinct line
        // spacing modes:
        //   paraPr  0 → PERCENT 160 (default)
        //   paraPr 20 → FIXED 2000  (20pt)
        //   paraPr 21 → AT_LEAST 2400 (24pt minimum)
        // The new AtLeast variant must round-trip through the encoder.
        let shape_0 = decoded
            .style_store
            .para_shape(ParaShapeIndex::new(0))
            .expect("para shape 0 must exist");
        assert_eq!(shape_0.line_spacing_type, LineSpacingType::Percentage);

        let shape_20 = decoded
            .style_store
            .para_shape(ParaShapeIndex::new(20))
            .expect("para shape 20 must exist");
        assert_eq!(shape_20.line_spacing_type, LineSpacingType::Fixed);

        let shape_21 = decoded
            .style_store
            .para_shape(ParaShapeIndex::new(21))
            .expect("para shape 21 must exist");
        assert_eq!(
            shape_21.line_spacing_type,
            LineSpacingType::AtLeast,
            "para shape 21 should carry AtLeast (HWP5 raw=3) after Wave 2a"
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_alignment_preserves_all_six_variants() {
        use hwpforge_foundation::{Alignment, ParaShapeIndex};

        let source = fixture_path("user_samples/sample-para-alignments-all.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-alignments-all.hwpx");
        let _warnings = hwp5_to_hwpx(&source, &out).expect("alignment conversion should succeed");

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");

        // HWPX fixture paraPr id → expected Alignment (verified by header.xml inspection).
        let expected: &[(usize, Alignment)] = &[
            (20, Alignment::Justify),
            (21, Alignment::Left),
            (22, Alignment::Right),
            (23, Alignment::Center),
            (24, Alignment::Distribute),
            (25, Alignment::DistributeFlush),
        ];
        for (idx, exp) in expected {
            let shape = decoded
                .style_store
                .para_shape(ParaShapeIndex::new(*idx))
                .unwrap_or_else(|err| panic!("para shape {idx} must exist after Wave 2b: {err}"));
            assert_eq!(
                shape.alignment, *exp,
                "para shape {idx} expected {:?}, got {:?}",
                exp, shape.alignment
            );
        }

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_indent_preserves_four_variants() {
        use hwpforge_foundation::ParaShapeIndex;

        let source = fixture_path("user_samples/sample-para-indent-variants.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-indent-variants.hwpx");
        let _warnings = hwp5_to_hwpx(&source, &out).expect("indent conversion should succeed");

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");

        // paraPr 20: 왼쪽 들여쓰기 (margin_left > 0, others 0)
        let s20 = decoded.style_store.para_shape(ParaShapeIndex::new(20)).expect("para shape 20");
        assert!(
            s20.margin_left.as_i32() > 0,
            "para 20 (왼쪽) expects positive margin_left, got {}",
            s20.margin_left.as_i32()
        );
        assert_eq!(s20.indent.as_i32(), 0, "para 20 indent should be 0");

        // paraPr 21: 오른쪽 들여쓰기 (margin_right > 0)
        let s21 = decoded.style_store.para_shape(ParaShapeIndex::new(21)).expect("para shape 21");
        assert!(
            s21.margin_right.as_i32() > 0,
            "para 21 (오른쪽) expects positive margin_right, got {}",
            s21.margin_right.as_i32()
        );

        // paraPr 22: 첫 줄 들여쓰기 (indent > 0)
        let s22 = decoded.style_store.para_shape(ParaShapeIndex::new(22)).expect("para shape 22");
        assert!(
            s22.indent.as_i32() > 0,
            "para 22 (첫 줄) expects positive indent, got {}",
            s22.indent.as_i32()
        );

        // paraPr 23: 내어쓰기 (indent < 0 / hanging)
        let s23 = decoded.style_store.para_shape(ParaShapeIndex::new(23)).expect("para shape 23");
        assert!(
            s23.indent.as_i32() < 0,
            "para 23 (내어쓰기) expects negative hanging indent, got {}",
            s23.indent.as_i32()
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_page_break_preserves_break_and_keep_flags() {
        use hwpforge_foundation::{BreakType, ParaShapeIndex};

        let source = fixture_path("user_samples/sample-para-page-break.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-page-break.hwpx");
        let _warnings = hwp5_to_hwpx(&source, &out).expect("page-break conversion should succeed");

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");

        // paraPr 20: 다음 쪽에서 시작 (page break before)
        let s20 = decoded.style_store.para_shape(ParaShapeIndex::new(20)).expect("para shape 20");
        assert_eq!(
            s20.break_type,
            BreakType::Page,
            "para 20 (다음 쪽에서 시작) expects BreakType::Page, got {:?}",
            s20.break_type
        );

        // paraPr 21: 다음 문단과 함께 (keep with next)
        let s21 = decoded.style_store.para_shape(ParaShapeIndex::new(21)).expect("para shape 21");
        assert!(s21.keep_with_next, "para 21 (다음 문단과 함께) expects keep_with_next = true");

        // paraPr 22: 같은 쪽에 두기 (keep lines together)
        let s22 = decoded.style_store.para_shape(ParaShapeIndex::new(22)).expect("para shape 22");
        assert!(
            s22.keep_lines_together,
            "para 22 (같은 쪽에 두기) expects keep_lines_together = true"
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_border_shading_carries_border_fill_per_paragraph() {
        use hwpforge_foundation::ParaShapeIndex;

        let source = fixture_path("user_samples/sample-para-border-shading.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-border-shading.hwpx");
        let _warnings =
            hwp5_to_hwpx(&source, &out).expect("border-shading conversion should succeed");

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");

        // Each of the three used paraPrs (20=사방 / 21=위아래 / 22=배경) must
        // reference a non-default borderFill (id 0/1/2 are the built-in defaults
        // for page / char-background / table; user borderFills start at id 3+).
        for (idx, label) in &[(20, "사방"), (21, "위아래"), (22, "배경")] {
            let shape = decoded
                .style_store
                .para_shape(ParaShapeIndex::new(*idx))
                .unwrap_or_else(|err| panic!("para shape {idx} ({label}) must exist: {err}"));
            let border_fill_id = shape
                .border_fill_id
                .unwrap_or_else(|| panic!("para {idx} ({label}) must carry a borderFillIDRef"));
            let raw = border_fill_id.get();
            assert!(
                raw >= 3,
                "para {idx} ({label}) expected non-default borderFillIDRef (>= 3), got {raw}"
            );
            // The referenced BorderFill must be resolvable via style_store.
            let _ = decoded
                .style_store
                .border_fill(raw as u32)
                .unwrap_or_else(|err| panic!("borderFill {raw} must resolve: {err}"));
        }

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_checkable_bullet_basic_decodes_per_paragraph_checked_state() {
        use hwpforge_foundation::ParaShapeIndex;

        let source = fixture_path("user_samples/lists/sample-checkable-bullet-basic.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-checkable-bullet-basic.hwpx");
        let _warnings =
            hwp5_to_hwpx(&source, &out).expect("checkable-bullet-basic conversion should succeed");

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");

        // Wave 3 — Per-paragraph checked state must decode end-to-end.
        // The HWPX fixture defines paraPr 20 (unchecked) and paraPr 21 (checked)
        // both pointing at the same BULLET heading definition.
        let s20 = decoded.style_store.para_shape(ParaShapeIndex::new(20)).expect("para shape 20");
        assert!(!s20.checked, "para 20 (unchecked bullet) expects checked = false, got true");

        let s21 = decoded.style_store.para_shape(ParaShapeIndex::new(21)).expect("para shape 21");
        assert!(
            s21.checked,
            "para 21 (checked bullet) expects checked = true — \
             per-paragraph checked state must round-trip through HWP5 → HWPX"
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_user_sample_checkable_bullet_basic_carries_definition_level_checkable() {
        let source = fixture_path("user_samples/lists/sample-checkable-bullet-basic.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("user-sample-checkable-bullet-basic-defn.hwpx");
        let _warnings =
            hwp5_to_hwpx(&source, &out).expect("checkable-bullet-basic conversion should succeed");

        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");

        // Definition-level checkable truth: CLAUDE.md gotcha #8 requires both
        // `bullet.checkedChar` and `bullet.paraHead.checkable` to carry through.
        let bullet = decoded
            .style_store
            .iter_bullets()
            .next()
            .expect("converted hwpx should contain a bullet definition");
        assert_eq!(
            bullet.checked_char.as_deref(),
            Some("☑"),
            "bullet definition must carry checkedChar from the HWP5 record"
        );
        assert!(
            bullet.para_head.checkable,
            "bullet paraHead must carry checkable=true from the HWP5 attribute bit"
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_chart_fixture_emits_embedded_chart_switch_block() {
        // Wave 4c carry: a HWP5 chart-bearing fixture must round-trip as
        //   * `Control::EmbeddedChart` run in the projected Core document
        //   * `Chart/chart1.xml` ZIP entry containing the OOXML chartSpace
        //   * `BinData/ole1.ole` ZIP entry containing the inner OLE2 bytes
        //   * `Contents/content.hpf` opf:manifest entry for the OLE blob
        //   * `Contents/section0.xml` with `<hp:switch>` carrying both a
        //     `<hp:case>` chart reference and `<hp:default>` OLE fallback
        let source = fixture_path("charts/chart_01_single_column.hwp");
        if !source.exists() {
            return;
        }

        let out = unique_temp_path("chart_01_single_column.hwpx");
        let warnings =
            hwp5_to_hwpx(&source, &out).expect("chart fixture conversion should succeed");
        assert!(
            !warnings.iter().any(|warning| matches!(
                warning,
                Hwp5Warning::DroppedControl { control, .. } if *control == "ole_object"
            )),
            "chart fixture should no longer surface DroppedControl:ole_object: {warnings:?}"
        );

        assert_valid_hwpx(&out);

        // Inspect ZIP contents directly: 한글 needs Chart/chart1.xml and
        // BinData/ole1.ole side-by-side and a matching opf:item entry.
        let bytes = std::fs::read(&out).expect("converted hwpx should be readable");
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes))
            .expect("converted hwpx should open as zip");
        let names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
        assert!(
            names.iter().any(|n| n == "Chart/chart1.xml"),
            "expected Chart/chart1.xml in zip, got {names:?}"
        );
        assert!(
            names.iter().any(|n| n == "BinData/ole1.ole"),
            "expected BinData/ole1.ole in zip, got {names:?}"
        );

        let mut chart_xml = String::new();
        {
            use std::io::Read;
            archive
                .by_name("Chart/chart1.xml")
                .expect("Chart/chart1.xml should be present")
                .read_to_string(&mut chart_xml)
                .expect("Chart/chart1.xml should be UTF-8");
        }
        assert!(
            chart_xml.contains("<c:chartSpace"),
            "Chart/chart1.xml should carry an OOXML <c:chartSpace> root"
        );

        let mut ole_bytes: Vec<u8> = Vec::new();
        {
            use std::io::Read;
            archive
                .by_name("BinData/ole1.ole")
                .expect("BinData/ole1.ole should be present")
                .read_to_end(&mut ole_bytes)
                .expect("BinData/ole1.ole should be readable");
        }
        assert!(
            ole_bytes.len() > 1024,
            "BinData/ole1.ole should carry a non-trivial OLE2 payload, got {} bytes",
            ole_bytes.len()
        );
        assert_eq!(
            &ole_bytes[..4],
            b"\xD0\xCF\x11\xE0",
            "BinData/ole1.ole must start with OLE2 magic"
        );

        let mut hpf_xml = String::new();
        {
            use std::io::Read;
            archive
                .by_name("Contents/content.hpf")
                .expect("Contents/content.hpf should be present")
                .read_to_string(&mut hpf_xml)
                .expect("content.hpf should be UTF-8");
        }
        assert!(
            hpf_xml.contains(r#"href="BinData/ole1.ole""#)
                && hpf_xml.contains(r#"media-type="application/ole""#),
            "content.hpf must register BinData/ole1.ole as application/ole"
        );

        let section_xml = read_section_xml(&out, 0);
        assert!(section_xml.contains("<hp:switch"), "section must contain <hp:switch>");
        assert!(section_xml.contains("<hp:chart"), "section must contain <hp:chart> in case arm");
        assert!(
            section_xml.contains("<hp:default>") && section_xml.contains("<hp:ole "),
            "section must contain <hp:default><hp:ole …> fallback"
        );
        assert!(
            section_xml.contains(r#"chartIDRef="Chart/chart1.xml""#),
            "section chart must reference Chart/chart1.xml"
        );
        assert!(
            section_xml.contains(r#"binaryItemIDRef="ole1""#),
            "section OLE fallback must reference ole1 binary item id"
        );

        // Decode round-trip: the HWPX decoder's structured Chart parser does
        // not know about the EmbeddedChart variant, so the chart shows up as
        // Control::Chart there. We only assert the OLE binary made it into
        // the decoded image_store, since that proves the manifest+BinData
        // wiring is consistent.
        let decoded = HwpxDecoder::decode(&bytes).expect("converted hwpx should decode");
        let has_ole_in_store =
            decoded.image_store.iter().any(|(name, _)| name.eq_ignore_ascii_case("ole1.ole"));
        assert!(has_ole_in_store, "decoded image_store should retain ole1.ole binary entry");

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hwp5_to_hwpx_bytes_matches_file_based_api() {
        let source = fixture_path("sample-text-char-runs-basic.hwp");
        if !source.exists() {
            return;
        }

        let bytes = std::fs::read(&source).expect("source bytes should be readable");
        let (inmem_bytes, inmem_warnings) =
            hwp5_to_hwpx_bytes(&bytes).expect("in-memory conversion should succeed");

        let file_out = unique_temp_path("inmem_compare.hwpx");
        let file_warnings =
            hwp5_to_hwpx(&source, &file_out).expect("file-based conversion should succeed");
        let file_bytes = std::fs::read(&file_out).expect("file output should be readable");

        assert_eq!(
            inmem_bytes, file_bytes,
            "in-memory and file-based hwp5_to_hwpx should produce identical bytes"
        );
        assert_eq!(
            inmem_warnings.len(),
            file_warnings.len(),
            "in-memory and file-based variants should emit the same warning count"
        );
        HwpxDecoder::decode(&inmem_bytes)
            .expect("in-memory hwpx bytes should round-trip through HwpxDecoder");

        let _ = std::fs::remove_file(&file_out);
    }
}
