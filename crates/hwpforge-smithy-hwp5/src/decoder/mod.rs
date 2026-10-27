//! HWP5 decoding pipeline.
//!
//! Submodules handle individual stages:
//! - `package` — OLE2/CFB extraction and stream access
//! - `header` — `DocInfo` stream parsing → [`crate::style_store::Hwp5StyleStore`]
//! - `section` — `BodyText/Section{N}` stream parsing → paragraphs

pub(crate) mod chart_ole;
pub(crate) mod header;
pub(crate) mod package;
pub(crate) mod section;

use std::path::Path;

use hwpforge_core::document::{Document, Draft};
use hwpforge_core::image::ImageStore;

use crate::error::Hwp5Result;
use crate::{
    collect_package_entries, summarize_doc_info_bin_data_records,
    summarize_package_bin_data_streams, Hwp5BinDataRecordSummary, Hwp5BinDataStream,
    Hwp5PackageEntry,
};

use self::header::DocInfoResult;
use self::section::SectionResult;

// ── Hwp5Document ─────────────────────────────────────────────────

/// The result of decoding an HWP5 file.
///
/// Contains the Core document (structure), the HWP5-specific style
/// store (fonts, char shapes, para shapes from the `DocInfo` stream),
/// and any warnings encountered during decoding.
#[derive(Debug)]
#[non_exhaustive]
pub struct Hwp5Document {
    /// The decoded document in Core's DOM.
    pub document: Document<Draft>,
    /// Binary image data extracted from `BinData` streams.
    pub image_store: ImageStore,
    /// Non-fatal warnings encountered during decoding.
    pub warnings: Vec<Hwp5Warning>,
}

/// A non-fatal warning emitted during HWP5 decoding.
///
/// Warnings indicate content that could not be fully decoded but
/// did not prevent the overall document from being parsed.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Hwp5Warning {
    /// A record tag was encountered but is not yet supported.
    UnsupportedTag {
        /// The numeric tag ID.
        tag_id: u16,
        /// The byte offset of the record in the stream.
        offset: usize,
    },
    /// A stream was present but skipped (e.g. encrypted or unknown).
    SkippedStream {
        /// The OLE2 stream name.
        name: String,
    },
    /// A decoded control was intentionally dropped because projection support
    /// for it is not implemented or because required assets were missing.
    DroppedControl {
        /// Stable control family name such as `rect`, `ole_object`, or `image`.
        control: &'static str,
        /// Concrete reason for the drop.
        reason: String,
    },
    /// Projection had to fall back to a default because the source value
    /// could not be represented directly.
    ProjectionFallback {
        /// Stable projection subject such as `table.page_break`.
        subject: &'static str,
        /// Concrete fallback detail.
        reason: String,
    },
    /// Parser had to fall back because source structure could not be attached
    /// without inventing parentage.
    ParserFallback {
        /// Stable parser subject such as `table.nested_attach`.
        subject: &'static str,
        /// Concrete fallback detail.
        reason: String,
    },
    /// A paragraph's line-layout cache was not promoted to Core because the
    /// wire→Core coordinate ledger could not be built or a lineseg `textpos`
    /// could not be normalized (W1b fail-closed — no guessed coordinates).
    LayoutCacheDropped {
        /// Concrete drop reason (ledger failure or offending textpos).
        reason: String,
    },
    /// Group (묶음 객체) content Core cannot carry as the source has it: a
    /// child the shared fold cannot place exactly (shear, collapse, residual,
    /// a field the Core variant lacks), a matrix chain that cannot be read, a
    /// picture or OLE child, the text of a shape that does not keep it, a
    /// directional fill, an object inside a grouped text box, or a group left
    /// with nothing. Same name and wire code (`CONTENT_LOST`) as the HWPX
    /// decode warning.
    ContentLost {
        /// What was lost (`group_child_transform`, `pic`, `group`, …).
        kind: String,
        /// Concrete reason.
        reason: String,
        /// The group's number in document order and the child's position
        /// among its group's children, e.g. `group[3].child[2].child[0]`.
        location: String,
    },
}

/// Shared parser output before Core projection or semantic adaptation.
#[derive(Debug)]
pub(crate) struct DecodedHwp5Intermediate {
    /// HWP5 version string from `FileHeader`.
    pub version: String,
    /// Whether the main HWP5 streams are document-level compressed.
    pub compressed: bool,
    /// Raw package entry inventory.
    pub package_entries: Vec<Hwp5PackageEntry>,
    /// Parsed `DocInfo/BinData` record summaries.
    pub bin_data_records: Vec<Hwp5BinDataRecordSummary>,
    /// Raw `/BinData/*` stream inventory.
    pub bin_data_streams: Vec<Hwp5BinDataStream>,
    /// Parsed `DocInfo` result.
    pub doc_info: DocInfoResult,
    /// Parsed `BodyText/Section{N}` results.
    pub sections: Vec<SectionResult>,
    /// Document metadata parsed from `\x05HwpSummaryInformation`
    /// (Wave 12o Phase 3). Empty `Metadata::default()` when the stream
    /// is absent or undecodable.
    pub metadata: hwpforge_core::metadata::Metadata,
    /// Non-fatal warnings collected during parsing.
    pub warnings: Vec<Hwp5Warning>,
}

/// Parse an HWP5 package into a shared intermediate result.
///
/// This removes the repeated `PackageReader -> parse_doc_info ->
/// parse_body_text(section*)` path from decoder helpers and higher-level
/// convenience functions without changing projection or style-store behavior.
pub(crate) fn decode_intermediate(bytes: &[u8]) -> Hwp5Result<DecodedHwp5Intermediate> {
    let pkg = package::PackageReader::open(bytes)?;
    let mut warnings: Vec<Hwp5Warning> = Vec::new();

    let package_entries = collect_package_entries(bytes)?;
    let bin_data_records = summarize_doc_info_bin_data_records(
        pkg.doc_info_data(),
        pkg.file_header().flags.compressed,
    )?;
    let bin_data_streams = summarize_package_bin_data_streams(&pkg);

    let doc_info = header::parse_doc_info(pkg.doc_info_data(), &pkg.file_header().version)?;
    warnings.extend(doc_info.warnings.iter().cloned());

    let mut sections: Vec<SectionResult> = Vec::with_capacity(pkg.sections_data().len());
    for section_data in pkg.sections_data() {
        section::validate_body_style_references(section_data, &doc_info)?;
        let mut result = section::parse_body_text(section_data, &pkg.file_header().version)?;
        // Transfer ownership: projection must not report these events again.
        warnings.append(&mut result.warnings);
        sections.push(result);
    }

    // Projection also synthesizes char/para 0 for missing runs or empty sections.
    // Those implicit references still need original definitions; otherwise a
    // decoded document could reach the encoder's new-document-only exception.
    // 이것을 실패시키는 것: 아래 기본 인덱스 원본 검사 제거(implicit_zero_decode_gate).
    hwpforge_core::style_lookup::lookup(doc_info.para_shapes.first(), "paraPr", 0)?;
    hwpforge_core::style_lookup::lookup(doc_info.char_shapes.first(), "charPr", 0)?;

    // Wave 12o Phase 3: parse \x05HwpSummaryInformation PropertySet
    // when present. Errors are demoted to warnings + empty metadata so
    // a malformed summary stream cannot break the body-text decode
    // path (the user-visible content is far more important than the
    // metadata sidecar).
    let metadata = match pkg.summary_info_data() {
        Some(raw) => match crate::schema::summary_info::parse_summary_information(raw) {
            Ok(m) => m,
            Err(e) => {
                warnings.push(Hwp5Warning::ParserFallback {
                    subject: "hwp_summary_information",
                    reason: format!(
                        "HwpSummaryInformation parse failed ({e}); using default metadata"
                    ),
                });
                hwpforge_core::metadata::Metadata::default()
            }
        },
        None => hwpforge_core::metadata::Metadata::default(),
    };

    Ok(DecodedHwp5Intermediate {
        version: pkg.file_header().version.to_string(),
        compressed: pkg.file_header().flags.compressed,
        package_entries,
        bin_data_records,
        bin_data_streams,
        doc_info,
        sections,
        metadata,
        warnings,
    })
}

// ── Hwp5Decoder ──────────────────────────────────────────────────

/// Decodes HWP5 files (OLE2 compound binary) into Core's `Document<Draft>`.
///
/// # Examples
///
/// ```no_run
/// use hwpforge_smithy_hwp5::Hwp5Decoder;
///
/// let bytes = std::fs::read("document.hwp").unwrap();
/// let result = Hwp5Decoder::decode(&bytes).unwrap();
/// println!("Sections: {}", result.document.sections().len());
/// ```
pub struct Hwp5Decoder;

impl Hwp5Decoder {
    /// Decodes an HWP5 file from raw bytes.
    ///
    /// Pipeline:
    /// 1. Open OLE2 container, validate HWP5 signature
    /// 2. Read `FileHeader` — version, flags, password check
    /// 3. Parse `DocInfo` stream → style store
    /// 4. Parse `BodyText/Section{N}` streams → paragraphs
    /// 5. Join `DocInfo/BinData` with `/BinData/*` image assets
    /// 6. Assemble `Document<Draft>` via projection
    pub fn decode(bytes: &[u8]) -> Hwp5Result<Hwp5Document> {
        Self::decode_with_budget_limit(bytes, package::MAX_TOTAL_DECOMPRESSED)
    }

    /// [`decode`](Self::decode) under an explicit document decompression
    /// budget.
    pub(crate) fn decode_with_budget_limit(
        bytes: &[u8],
        budget_limit: u64,
    ) -> Hwp5Result<Hwp5Document> {
        let intermediate = decode_intermediate(bytes)?;
        let (image_assets, budget) =
            crate::join_hwp5_image_assets(bytes, &intermediate, budget_limit)?;
        let mut warnings = intermediate.warnings;
        let metadata = intermediate.metadata;

        // Stage 4: Projection — HWP5 IR → Core Document
        let (mut document, image_store, proj_warnings) =
            crate::projection::project_to_core_with_images(
                intermediate.sections,
                &image_assets,
                budget,
            )?;
        warnings.extend(proj_warnings);

        // Forward `\x05HwpSummaryInformation` derived metadata so the canonical
        // entry point honors the same wire contract as
        // `decode_hwp5_with_images` / `decode_hwp5_to_core`. Without
        // this the public API silently dropped metadata for any
        // caller using `Hwp5Decoder::decode` directly (CLI/MCP/tests).
        document.set_metadata(metadata);

        Ok(Hwp5Document { document, image_store, warnings })
    }

    /// Decodes an HWP5 file from a filesystem path.
    pub fn decode_file(path: impl AsRef<Path>) -> Hwp5Result<Hwp5Document> {
        let bytes = std::fs::read(path.as_ref()).map_err(crate::error::Hwp5Error::Io)?;
        Self::decode(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod wire {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/support/hwp5.rs"));
    }
    use wire::{make_record, make_test_cfb};

    fn para_header_data() -> Vec<u8> {
        let mut buf = vec![0u8; 22];
        buf[0..4].copy_from_slice(&5u32.to_le_bytes()); // char_count=5
        buf
    }

    fn para_text_data(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect()
    }

    #[test]
    fn implicit_zero_decode_gate() {
        // 이것을 실패시키는 것: 원본 빈 표를 projection 기본 0으로 수복하도록 허용.
        let char_def = make_record(0x15, 0, &[0; 68]);
        let para_def = make_record(0x19, 0, &[0; 42]);
        let style_def = make_record(0x1a, 0, &[0; 14]);
        for (info, kind) in [(para_def.clone(), "charPr"), (char_def.clone(), "paraPr")] {
            let bytes = make_test_cfb(&info, &[]);
            for error in [
                Hwp5Decoder::decode(&bytes).unwrap_err(),
                crate::decode_hwp5_to_core(&bytes).unwrap_err(),
            ] {
                assert!(
                    error.to_string().contains(&format!("broken {kind} reference ID 0")),
                    "{error}"
                );
            }
        }
        let mut info = para_def;
        info.extend(style_def);
        let section = make_record(0x42, 0, &para_header_data());
        let bytes = make_test_cfb(&info, &section);
        assert!(Hwp5Decoder::decode(&bytes).unwrap_err().to_string().contains("charPr"));
        info.extend(char_def);
        let bytes = make_test_cfb(&info, &section);
        Hwp5Decoder::decode(&bytes).unwrap();
        crate::decode_hwp5_to_core(&bytes).unwrap();
    }

    #[test]
    fn empty_definitions_zero_decode_gate() {
        // 이것을 실패시키는 것: decode_intermediate의 원본 본문 참조 검사 제거.
        let mut header = para_header_data();
        header[12..14].copy_from_slice(&1u16.to_le_bytes());
        let mut section = make_record(0x42, 0, &header);
        section.extend(make_record(0x43, 1, &para_text_data("Hello")));
        let mut char_ref = 0u32.to_le_bytes().to_vec();
        char_ref.extend_from_slice(&0u32.to_le_bytes());
        section.extend(make_record(0x44, 1, &char_ref));
        let parsed =
            section::parse_body_text(&section, &crate::schema::header::HwpVersion::new(5, 0, 2, 5))
                .unwrap();
        assert_eq!(parsed.paragraphs[0].char_shape_runs[0].char_shape_id, 0);
        let bytes = make_test_cfb(&[], &section);
        for error in [
            Hwp5Decoder::decode(&bytes).unwrap_err(),
            crate::decode_hwp5_to_core(&bytes).unwrap_err(),
        ] {
            assert!(error.to_string().contains("broken paraPr reference ID 0"), "{error}");
        }
    }

    #[test]
    fn public_decoders_reject_undefined_body_style_ids() {
        // 이것을 실패시키는 것: decode_intermediate의 본문 lookup 호출 제거.
        for kind in ["charPr", "paraPr", "style"] {
            let mut header = para_header_data();
            header[12..14].copy_from_slice(&1u16.to_le_bytes());
            if kind == "paraPr" {
                header[8..10].copy_from_slice(&1u16.to_le_bytes());
            }
            if kind == "style" {
                header[10] = 1;
            }
            let mut section = make_record(0x42, 0, &header);
            section.extend(make_record(0x43, 1, &para_text_data("Hello")));
            let mut run = 0u32.to_le_bytes().to_vec();
            run.extend_from_slice(&u32::from(kind == "charPr").to_le_bytes());
            section.extend(make_record(0x44, 1, &run));
            let bytes = make_test_cfb(&minimal_doc_info(), &section);
            for error in [
                Hwp5Decoder::decode(&bytes).unwrap_err(),
                crate::decode_hwp5_to_core(&bytes).unwrap_err(),
            ] {
                assert!(
                    error.to_string().contains(&format!("broken {kind} reference ID 1")),
                    "{error}"
                );
            }
        }
    }

    fn minimal_doc_info() -> Vec<u8> {
        // 이것을 실패시키는 것: 합성 입력의 원본 char/para/style 정의 제거.
        let mut info = make_record(0x15, 0, &[0; 68]);
        info.extend(make_record(0x19, 0, &[0; 42]));
        info.extend(make_record(0x1a, 0, &[0; 14]));
        info
    }

    #[test]
    fn decode_minimal_document() {
        // 이것을 실패시키는 것: minimal_doc_info의 원본 스타일 정의 제거.
        // Build a section stream with one paragraph
        let mut section = Vec::new();
        section.extend(make_record(0x42, 0, &para_header_data())); // ParaHeader
        section.extend(make_record(0x43, 0, &para_text_data("Hello"))); // ParaText

        let bytes = make_test_cfb(&minimal_doc_info(), &section);
        let result = Hwp5Decoder::decode(&bytes).unwrap();
        assert!(!result.document.sections().is_empty());
    }

    #[test]
    fn decode_intermediate_collects_shared_parse_output() {
        // 이것을 실패시키는 것: minimal_doc_info의 원본 스타일 정의 제거.
        let mut section = Vec::new();
        section.extend(make_record(0x42, 0, &para_header_data()));
        section.extend(make_record(0x43, 0, &para_text_data("Hello")));

        let bytes = make_test_cfb(&minimal_doc_info(), &section);
        let intermediate = decode_intermediate(&bytes).unwrap();
        assert_eq!(intermediate.version, "5.0.2.5");
        assert!(!intermediate.compressed);
        assert_eq!(intermediate.sections.len(), 1);
        assert_eq!(intermediate.sections[0].paragraphs.len(), 1);
        assert_eq!(intermediate.sections[0].paragraphs[0].text, "Hello");
        assert!(!intermediate.package_entries.is_empty());
    }

    #[test]
    fn decode_empty_section_with_defined_styles() {
        // 이것을 실패시키는 것: 빈 구역의 합성 0이 참조할 원본 정의 제거.
        let bytes = make_test_cfb(&minimal_doc_info(), &[]);
        let result = Hwp5Decoder::decode(&bytes).unwrap();
        // Even empty sections should produce at least one section with default paragraph
        assert!(!result.document.sections().is_empty());
    }

    #[test]
    fn decode_file_not_found() {
        let err = Hwp5Decoder::decode_file("/nonexistent/path.hwp").unwrap_err();
        assert!(matches!(err, crate::error::Hwp5Error::Io(_)));
    }
}
