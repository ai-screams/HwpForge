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
        let result = section::parse_body_text(section_data, &pkg.file_header().version)?;
        warnings.extend(result.warnings.iter().cloned());
        sections.push(result);
    }

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
        let intermediate = decode_intermediate(bytes)?;
        let image_assets = crate::join_hwp5_image_assets(bytes, &intermediate)?;
        let mut warnings = intermediate.warnings;
        let metadata = intermediate.metadata;

        // Stage 4: Projection — HWP5 IR → Core Document
        let (mut document, image_store, proj_warnings) =
            crate::projection::project_to_core_with_images(intermediate.sections, &image_assets)?;
        warnings.extend(proj_warnings);

        // Codex(architect) Wave 12o-fixup §Top-4: forward
        // `\x05HwpSummaryInformation` derived metadata so the canonical
        // entry point honors the same wire contract as
        // `decode_hwp5_with_images` / `hwp5_to_hwpx_bytes`. Without
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
    use crate::schema::section::TextSegment;
    use std::io::Write;

    /// Build a minimal valid CFB file with FileHeader + DocInfo + Section0.
    fn make_test_cfb(doc_info: &[u8], section0: &[u8]) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut comp = cfb::CompoundFile::create(cursor).unwrap();

        // FileHeader (256 bytes) — v5.0.2.5, uncompressed
        let mut header_buf = vec![0u8; 256];
        header_buf[..18].copy_from_slice(b"HWP Document File\0");
        let version: u32 = (5 << 24) | (2 << 8) | 5;
        header_buf[32..36].copy_from_slice(&version.to_le_bytes());
        header_buf[36..40].copy_from_slice(&0u32.to_le_bytes()); // flags=0: uncompressed
        let mut stream = comp.create_stream("/FileHeader").unwrap();
        stream.write_all(&header_buf).unwrap();
        drop(stream);

        // DocInfo
        let mut stream = comp.create_stream("/DocInfo").unwrap();
        stream.write_all(doc_info).unwrap();
        drop(stream);

        // BodyText/Section0
        comp.create_storage("/BodyText").unwrap();
        let mut stream = comp.create_stream("/BodyText/Section0").unwrap();
        stream.write_all(section0).unwrap();
        drop(stream);

        comp.into_inner().into_inner()
    }

    fn make_record(tag_id: u16, level: u16, data: &[u8]) -> Vec<u8> {
        let size = data.len() as u32;
        let word = (tag_id as u32) | ((level as u32) << 10) | (size.min(0xFFE) << 20);
        let mut buf = word.to_le_bytes().to_vec();
        if size > 0xFFE {
            let word = (tag_id as u32) | ((level as u32) << 10) | (0xFFF << 20);
            buf = word.to_le_bytes().to_vec();
            buf.extend_from_slice(&size.to_le_bytes());
        }
        buf.extend_from_slice(data);
        buf
    }

    fn para_header_data() -> Vec<u8> {
        let mut buf = vec![0u8; 22];
        buf[0..4].copy_from_slice(&5u32.to_le_bytes()); // char_count=5
        buf
    }

    fn para_text_data(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect()
    }

    #[test]
    fn decode_minimal_document() {
        // Build a section stream with one paragraph
        let mut section = Vec::new();
        section.extend(make_record(0x42, 0, &para_header_data())); // ParaHeader
        section.extend(make_record(0x43, 0, &para_text_data("Hello"))); // ParaText

        let bytes = make_test_cfb(&[], &section);
        let result = Hwp5Decoder::decode(&bytes).unwrap();
        assert!(!result.document.sections().is_empty());
    }

    #[test]
    fn decode_intermediate_collects_shared_parse_output() {
        let mut section = Vec::new();
        section.extend(make_record(0x42, 0, &para_header_data()));
        section.extend(make_record(0x43, 0, &para_text_data("Hello")));

        let bytes = make_test_cfb(&[], &section);
        let intermediate = decode_intermediate(&bytes).unwrap();
        assert_eq!(intermediate.version, "5.0.2.5");
        assert!(!intermediate.compressed);
        assert_eq!(intermediate.sections.len(), 1);
        assert_eq!(intermediate.sections[0].paragraphs.len(), 1);
        assert_eq!(intermediate.sections[0].paragraphs[0].text, "Hello");
        assert!(!intermediate.package_entries.is_empty());
    }

    #[test]
    fn decode_empty_doc_info_and_section() {
        let bytes = make_test_cfb(&[], &[]);
        let result = Hwp5Decoder::decode(&bytes).unwrap();
        // Even empty sections should produce at least one section with default paragraph
        assert!(!result.document.sections().is_empty());
    }

    #[test]
    fn decode_file_not_found() {
        let err = Hwp5Decoder::decode_file("/nonexistent/path.hwp").unwrap_err();
        assert!(matches!(err, crate::error::Hwp5Error::Io(_)));
    }

    /// Golden test: `hwp5_00.hwp` contains 5 paragraphs of Lorem ipsum with
    /// bold, italic, and underline formatting.
    #[test]
    fn golden_hwp5_00_lorem_ipsum() {
        let fixture = crate::test_support::workspace_fixture_path("hwp5_00.hwp");
        if !fixture.exists() {
            eprintln!("Skipping: fixture not found at {:?}", fixture);
            return;
        }

        let doc = Hwp5Decoder::decode_file(&fixture).expect("Failed to decode hwp5_00.hwp");

        // Should produce exactly 1 section.
        assert_eq!(doc.document.sections().len(), 1);

        let sec = &doc.document.sections()[0];

        // 5 text paragraphs + 6 empty separator paragraphs = 11 total.
        assert_eq!(sec.paragraphs.len(), 11);

        // Collect non-empty paragraphs — should be exactly 5.
        let text_paras: Vec<String> =
            sec.paragraphs.iter().map(|p| p.text_content()).filter(|t| !t.is_empty()).collect();
        assert_eq!(text_paras.len(), 5, "Expected 5 text paragraphs");

        // All 5 should contain "Lorem ipsum".
        for (i, text) in text_paras.iter().enumerate() {
            assert!(
                text.contains("Lorem ipsum"),
                "Para {} should contain 'Lorem ipsum', got: {:?}",
                i,
                &text[..text.len().min(80)]
            );
        }

        // Only minor DocInfo warnings (unknown tags), no ParaHeader failures.
        for w in &doc.warnings {
            if let Hwp5Warning::UnsupportedTag { tag_id, .. } = w {
                // Should NOT have tag_id 0x42 (ParaHeader) warnings.
                assert_ne!(*tag_id, 0x42, "ParaHeader should not produce warnings");
            }
        }
    }

    /// Wave 12n Step 8 — end-to-end regression gate for the five
    /// auto-field variants. `sample-field-docsummary.hwp` is a 한컴
    /// Office-authored fixture (macOS 12.30) that exercises:
    ///
    /// - `%smr` SUMMERY → `Control::Field { field_type: ModifiedTime
    ///   / LastSavedBy / CreatedTime / Author / Title }`
    /// - `%pat` PATH → `Control::PathField`
    /// - `atno` inline page-number → `Control::InlinePageNumber`
    ///
    /// The fixture was the driver for the Wave 12n Phase 2 wire dump
    /// (`.docs/research/2026-06-02_auto_field_wire_dump.md`) and is
    /// the same one used for the Step 6 PATH wire byte-parity test
    /// against `sample-field-docsummary.hwpx`.
    #[test]
    fn golden_sample_field_docsummary_decodes_all_auto_field_variants() {
        use hwpforge_core::control::{Control, InlinePageKind};
        use hwpforge_core::run::RunContent;
        use hwpforge_foundation::FieldType;

        let Some(doc) = load_fixture("sample-field-docsummary.hwp") else {
            return;
        };

        // Collect every Control variant across all paragraphs/runs.
        let mut field_modified = 0usize;
        let mut field_last_saved = 0usize;
        let mut field_created = 0usize;
        let mut field_author = 0usize;
        let mut field_title = 0usize;
        let mut path_field = 0usize;
        let mut autonum_current = 0usize;
        let mut autonum_total = 0usize;

        for section in doc.document.sections() {
            for paragraph in &section.paragraphs {
                for run in &paragraph.runs {
                    let RunContent::Control(ctrl) = &run.content else {
                        continue;
                    };
                    match ctrl.as_ref() {
                        Control::Field { field_type, .. } => match field_type {
                            FieldType::ModifiedTime => field_modified += 1,
                            FieldType::LastSavedBy => field_last_saved += 1,
                            FieldType::CreatedTime => field_created += 1,
                            FieldType::Author => field_author += 1,
                            FieldType::Title => field_title += 1,
                            _ => {}
                        },
                        Control::PathField { .. } => path_field += 1,
                        Control::InlinePageNumber { kind, .. } => match kind {
                            InlinePageKind::CurrentPage => autonum_current += 1,
                            InlinePageKind::TotalPages => autonum_total += 1,
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
        }

        // Counts derived from the fixture's section0.xml wire dump.
        assert_eq!(field_modified, 1, "$modifiedtime (Field::ModifiedTime)");
        assert_eq!(field_last_saved, 1, "$lastsaveby (Field::LastSavedBy)");
        assert_eq!(field_created, 2, "$createtime (Field::CreatedTime, standalone + inline)");
        assert_eq!(field_author, 2, "$author (Field::Author, standalone + inline)");
        assert_eq!(field_title, 1, "$title (Field::Title)");
        assert_eq!(path_field, 1, "$P$F (PathField)");
        assert!(
            autonum_current >= 2,
            "1[쪽 번호] (InlinePageNumber::CurrentPage), got {autonum_current}"
        );
        assert_eq!(autonum_total, 1, "1[전체 쪽수] (InlinePageNumber::TotalPages)");
    }

    // ── Helper: load fixture, skip if missing ─────────────────────────
    fn load_fixture(name: &str) -> Option<Hwp5Document> {
        let path = crate::test_support::workspace_fixture_path(name);
        if !path.exists() {
            eprintln!("Skipping: fixture not found at {:?}", path);
            return None;
        }
        Some(
            Hwp5Decoder::decode_file(&path)
                .unwrap_or_else(|e| panic!("Failed to decode {name}: {e:?}")),
        )
    }

    fn load_intermediate_fixture(name: &str) -> Option<DecodedHwp5Intermediate> {
        let path = crate::test_support::workspace_fixture_path(name);
        if !path.exists() {
            eprintln!("Skipping: fixture not found at {:?}", path);
            return None;
        }
        let bytes = std::fs::read(&path).expect("read hwp fixture");
        Some(
            decode_intermediate(&bytes)
                .unwrap_or_else(|error| panic!("Failed to decode intermediate {name}: {error:?}")),
        )
    }

    fn assert_no_para_header_warnings(warnings: &[Hwp5Warning]) {
        for w in warnings {
            if let Hwp5Warning::UnsupportedTag { tag_id, .. } = w {
                assert_ne!(*tag_id, 0x42, "ParaHeader should not produce warnings");
            }
        }
    }

    /// hwp5_01: Two tables with nested cell paragraphs and merged spans.
    #[test]
    fn golden_hwp5_01_table() {
        let doc = match load_fixture("hwp5_01.hwp") {
            Some(d) => d,
            None => return,
        };
        assert_eq!(doc.document.sections().len(), 1);
        let sec = &doc.document.sections()[0];
        assert_eq!(sec.paragraphs.len(), 5);

        let tables: Vec<&hwpforge_core::table::Table> = sec
            .paragraphs
            .iter()
            .flat_map(|para| para.runs.iter())
            .filter_map(|run| run.content.as_table())
            .collect();
        assert_eq!(tables.len(), 2, "Expected 2 table runs");

        let first = tables[0];
        assert_eq!(first.row_count(), 3);
        assert_eq!(first.col_count(), 4);
        assert_eq!(first.rows[0].cells[0].paragraphs[0].text_content(), "안녕");
        assert_eq!(first.rows[2].cells[3].paragraphs[0].text_content(), "적어요");

        let second = tables[1];
        assert_eq!(second.row_count(), 3);
        assert_eq!(second.col_count(), 3, "merged rows should still keep 3 physical rows");
        assert_eq!(second.rows[0].cells[0].col_span, 2);
        assert_eq!(second.rows[0].cells[1].row_span, 2);
        assert_eq!(second.rows[2].cells[0].col_span, 4);

        for para in &sec.paragraphs {
            assert!(
                !para.text_content().contains('\u{FFFC}'),
                "table control placeholder should not leak into projected text"
            );
        }
        assert_no_para_header_warnings(&doc.warnings);
    }

    /// hwp5_02: Multiple fonts/sizes — 바탕 12pt, 돋음 16pt, 맑은고딕 10pt, 기타등등.
    #[test]
    fn golden_hwp5_02_fonts() {
        let doc = match load_fixture("hwp5_02.hwp") {
            Some(d) => d,
            None => return,
        };
        assert_eq!(doc.document.sections().len(), 1);
        let sec = &doc.document.sections()[0];

        let texts: Vec<String> =
            sec.paragraphs.iter().map(|p| p.text_content()).filter(|t| !t.is_empty()).collect();
        assert_eq!(texts.len(), 4, "Expected 4 text paragraphs");
        assert!(texts[0].contains("바탕"));
        assert!(texts[1].contains("돋음") || texts[1].contains("돋움"));
        assert!(texts[2].contains("맑은고딕"));
        assert!(texts[3].contains("기타등등"));
        assert_no_para_header_warnings(&doc.warnings);
    }

    #[test]
    fn decode_intermediate_user_sample_text_tab_linebreak_preserves_raw_segments() {
        let decoded =
            match load_intermediate_fixture("user_samples/sample-text-tab-linebreak-basic.hwp") {
                Some(decoded) => decoded,
                None => return,
            };

        let paragraph = &decoded.sections[0].paragraphs[0];
        assert_eq!(paragraph.text, "LEFT\tRIGHT\nNEXT-LINE");
        let text_runs: Vec<&str> = paragraph
            .text_segments
            .iter()
            .filter_map(|segment| match segment {
                TextSegment::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text_runs, vec!["LEFT", "RIGHT", "NEXT-LINE"]);
        assert!(paragraph
            .text_segments
            .iter()
            .any(|segment| matches!(segment, TextSegment::Tab { .. })));
        assert!(paragraph
            .text_segments
            .iter()
            .any(|segment| matches!(segment, TextSegment::LineBreak)));
    }

    #[test]
    fn decode_intermediate_user_sample_hyperlink_preserves_field_markers() {
        let decoded =
            match load_intermediate_fixture("user_samples/sample-field-hyperlink-basic.hwp") {
                Some(decoded) => decoded,
                None => return,
            };

        let paragraph = &decoded.sections[0].paragraphs[0];
        assert_eq!(paragraph.text, "OpenAI");
        assert_eq!(
            paragraph
                .text_segments
                .iter()
                .filter(|segment| matches!(segment, TextSegment::FieldBegin { .. }))
                .count(),
            1,
            "hyperlink paragraph should preserve one field-begin marker",
        );
        assert_eq!(
            paragraph
                .text_segments
                .iter()
                .filter(|segment| matches!(segment, TextSegment::FieldEnd))
                .count(),
            1,
            "hyperlink paragraph should preserve one field-end marker",
        );
    }

    /// hwp5_03: 5 paragraphs with different alignments (left/center/right/distribute/split).
    #[test]
    fn golden_hwp5_03_alignment() {
        let doc = match load_fixture("hwp5_03.hwp") {
            Some(d) => d,
            None => return,
        };
        assert_eq!(doc.document.sections().len(), 1);
        let sec = &doc.document.sections()[0];
        assert_eq!(sec.paragraphs.len(), 11, "5 text + 6 empty separator paragraphs");

        let texts: Vec<String> =
            sec.paragraphs.iter().map(|p| p.text_content()).filter(|t| !t.is_empty()).collect();
        assert_eq!(texts.len(), 5, "Expected 5 text paragraphs");
        for (i, text) in texts.iter().enumerate() {
            assert!(
                text.contains("Lorem ipsum"),
                "Para {i} should contain 'Lorem ipsum', got: {:?}",
                &text[..text.len().min(60)]
            );
        }
        assert_no_para_header_warnings(&doc.warnings);
    }

    /// hwp5_04: 2 sections — section 0 landscape, section 1 portrait with custom margins.
    #[test]
    fn golden_hwp5_04_landscape() {
        let doc = match load_fixture("hwp5_04.hwp") {
            Some(d) => d,
            None => return,
        };
        assert_eq!(doc.document.sections().len(), 2);

        // Both sections have 5 text paragraphs each.
        for (si, sec) in doc.document.sections().iter().enumerate() {
            let texts: Vec<String> =
                sec.paragraphs.iter().map(|p| p.text_content()).filter(|t| !t.is_empty()).collect();
            assert_eq!(texts.len(), 5, "Section {si} should have 5 text paragraphs");
        }
        assert_no_para_header_warnings(&doc.warnings);
    }

    /// hwp5_04 intermediate: verify PageDef landscape and margins via parse_body_text.
    #[test]
    fn golden_hwp5_04_page_def() {
        let path = crate::test_support::workspace_fixture_path("hwp5_04.hwp");
        if !path.exists() {
            return;
        }
        let bytes = std::fs::read(&path).unwrap();
        let pkg = package::PackageReader::open(&bytes).unwrap();
        let version = &pkg.file_header().version;

        let sections_data = pkg.sections_data();
        assert_eq!(sections_data.len(), 2);

        // Section 0: landscape
        let sec0 = section::parse_body_text(&sections_data[0], version).unwrap();
        let pd0 = sec0.page_def.expect("Section 0 should have PageDef");
        assert!(pd0.landscape, "Section 0 should be landscape");

        // Section 1: portrait with custom margins
        let sec1 = section::parse_body_text(&sections_data[1], version).unwrap();
        let pd1 = sec1.page_def.expect("Section 1 should have PageDef");
        assert!(!pd1.landscape, "Section 1 should be portrait");

        // Margins in HwpUnit (1mm ≈ 283 HwpUnit). Allow ±50 tolerance.
        let margin_checks = [
            ("left", pd1.margin_left, 33.0_f64),
            ("right", pd1.margin_right, 32.0),
            ("top", pd1.margin_top, 22.0),
            ("bottom", pd1.margin_bottom, 16.0),
            ("header", pd1.header_margin, 13.0),
            ("footer", pd1.footer_margin, 17.0),
            ("gutter", pd1.gutter, 2.0),
        ];
        for (name, actual, expected_mm) in margin_checks {
            let expected_hwpunit = (expected_mm * 283.0) as u32;
            let diff = (actual as i64 - expected_hwpunit as i64).unsigned_abs();
            assert!(
                diff <= 50,
                "Section 1 {name}: expected ~{expected_hwpunit} ({}mm), got {actual} (diff={diff})",
                expected_mm
            );
        }
    }

    /// hwp5_05: Empty document — new file saved immediately.
    #[test]
    fn golden_hwp5_05_empty() {
        let doc = match load_fixture("hwp5_05.hwp") {
            Some(d) => d,
            None => return,
        };
        assert_eq!(doc.document.sections().len(), 1);
        let sec = &doc.document.sections()[0];
        assert_eq!(sec.paragraphs.len(), 1, "Empty doc should have 1 paragraph");
        assert!(sec.paragraphs[0].text_content().is_empty(), "Paragraph should be empty");
        assert_no_para_header_warnings(&doc.warnings);
    }

    #[test]
    fn golden_hwp5_02_doc_info_preserves_font_buckets_and_style_fields() {
        let fixture = crate::test_support::workspace_fixture_path("hwp5_02.hwp");
        if !fixture.exists() {
            return;
        }

        let bytes = std::fs::read(&fixture).unwrap();
        let pkg = package::PackageReader::open(&bytes).unwrap();
        let doc_info =
            header::parse_doc_info(pkg.doc_info_data(), &pkg.file_header().version).unwrap();
        let mappings = doc_info.id_mappings.as_ref().expect("hwp5_02 should have IdMappings");

        assert_eq!(doc_info.fonts.len(), 56);
        assert_eq!(mappings.hangul_font_count, 8);
        assert_eq!(mappings.english_font_count, 8);
        assert_eq!(mappings.hanja_font_count, 8);
        assert_eq!(mappings.japanese_font_count, 8);
        assert_eq!(mappings.other_font_count, 8);
        assert_eq!(mappings.symbol_font_count, 8);
        assert_eq!(mappings.user_font_count, 8);
        assert_eq!(mappings.char_shape_count, 14);
        assert_eq!(mappings.para_shape_count, 20);
        assert_eq!(mappings.style_count, 22);

        assert_eq!(doc_info.styles.len(), 22);
        assert_eq!(doc_info.styles[0].lang_id, 1042);
        assert_eq!(doc_info.styles[1].next_style_id, 1);
        assert_eq!(doc_info.styles[1].para_shape_id, 1);
        assert_eq!(doc_info.styles[12].kind, 1);
        assert_eq!(doc_info.styles[12].char_shape_id, 1);
    }
}
