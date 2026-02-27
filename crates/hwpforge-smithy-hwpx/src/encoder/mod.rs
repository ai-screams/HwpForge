//! HWPX encoder pipeline.
//!
//! Submodules handle individual stages:
//! - `header` — [`HwpxStyleStore`] → `header.xml` serialization
//! - `section` — Core `Section` → `section*.xml` serialization
//! - `package` — ZIP assembly (mimetype, metadata, content files)
//!
//! The public entry point is [`HwpxEncoder`], which orchestrates
//! the full pipeline: header → sections → ZIP packaging.

pub(crate) mod chart;
pub(crate) mod header;
pub(crate) mod package;
pub(crate) mod section;

use std::path::Path;

use hwpforge_core::document::{Document, Validated};
use hwpforge_core::image::ImageStore;

use crate::error::{HwpxError, HwpxResult};
use crate::style_store::HwpxStyleStore;

use self::header::encode_header;
use self::package::PackageWriter;
use self::section::encode_section;

// ── HwpxEncoder ─────────────────────────────────────────────────

/// Encodes Core documents to HWPX format (ZIP + XML).
///
/// This is the reverse of [`crate::HwpxDecoder`]: it takes a validated
/// document and an [`HwpxStyleStore`] and produces a valid HWPX archive.
///
/// # Round-trip
///
/// ```no_run
/// use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};
///
/// let bytes = std::fs::read("input.hwpx").unwrap();
/// let result = HwpxDecoder::decode(&bytes).unwrap();
/// let validated = result.document.validate().unwrap();
/// let output = HwpxEncoder::encode(&validated, &result.style_store, &result.image_store).unwrap();
/// std::fs::write("output.hwpx", &output).unwrap();
/// ```
///
/// # Image Binary Support
///
/// The encoder embeds binary image data from [`ImageStore`] into
/// `BinData/` entries in the ZIP archive. Image paths in the document
/// (e.g. `"BinData/image1.png"`) are matched against the store keys.
/// Images not found in the store are silently skipped (XML reference
/// only, no binary data).
#[derive(Debug, Clone, Copy)]
pub struct HwpxEncoder;

impl HwpxEncoder {
    /// Encodes a validated document with its style store and images to HWPX bytes.
    ///
    /// The returned bytes form a valid ZIP archive that can be written
    /// to a `.hwpx` file or decoded back with [`crate::HwpxDecoder`].
    ///
    /// # Pipeline
    ///
    /// 1. Serialize `HwpxStyleStore` → `header.xml`
    /// 2. Serialize each section → `section{N}.xml`
    /// 3. Collect image binaries from `ImageStore`
    /// 4. Package into ZIP with metadata files + BinData/
    ///
    /// # Errors
    ///
    /// - [`HwpxError::XmlSerialize`] if quick-xml serialization fails
    /// - [`HwpxError::InvalidStructure`] if table nesting exceeds limits
    /// - [`HwpxError::Zip`] if ZIP archive creation fails
    pub fn encode(
        document: &Document<Validated>,
        style_store: &HwpxStyleStore,
        image_store: &ImageStore,
    ) -> HwpxResult<Vec<u8>> {
        let sections = document.sections();
        let sec_cnt = sections.len() as u32;

        // Step 1: Encode header
        let header_xml = encode_header(style_store, sec_cnt)?;

        // Step 2: Encode sections (each produces XML + chart entries)
        // chart_offset tracks the global chart index across sections to avoid
        // duplicate Chart/chartN.xml filenames in the ZIP archive.
        let mut chart_offset = 0usize;
        let mut section_results = Vec::with_capacity(sections.len());
        for (i, section) in sections.iter().enumerate() {
            let result = encode_section(section, i, chart_offset)?;
            chart_offset += result.charts.len();
            section_results.push(result);
        }

        let section_xmls: Vec<String> = section_results.iter().map(|r| r.xml.clone()).collect();
        let charts: Vec<(String, String)> =
            section_results.into_iter().flat_map(|r| r.charts).collect();

        // Step 3: Collect image binaries
        let images: Vec<(String, Vec<u8>)> =
            image_store.iter().map(|(key, data)| (key.to_string(), data.to_vec())).collect();

        // Step 4: Package into ZIP with images and charts
        PackageWriter::write_hwpx(&header_xml, &section_xmls, &images, &charts)
    }

    /// Encodes a validated document and writes it to a file.
    ///
    /// Convenience wrapper around [`encode`](Self::encode) +
    /// [`std::fs::write`].
    ///
    /// # Errors
    ///
    /// Returns [`HwpxError::Io`] if the file cannot be written, or any
    /// error from [`encode`](Self::encode).
    pub fn encode_file(
        path: impl AsRef<Path>,
        document: &Document<Validated>,
        style_store: &HwpxStyleStore,
        image_store: &ImageStore,
    ) -> HwpxResult<()> {
        let bytes = Self::encode(document, style_store, image_store)?;
        std::fs::write(path.as_ref(), bytes).map_err(HwpxError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HwpxDecoder;
    use hwpforge_core::image::ImageStore;
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::Section;
    use hwpforge_core::PageSettings;
    use hwpforge_foundation::{
        Alignment, CharShapeIndex, Color, EmbossType, EngraveType, FontIndex, HwpUnit,
        LineSpacingType, OutlineType, ParaShapeIndex, ShadowType, StrikeoutShape, UnderlineType,
        VerticalPosition,
    };

    use crate::style_store::{HwpxCharShape, HwpxFont, HwpxFontRef, HwpxParaShape};

    /// Creates a minimal validated document + style store for testing.
    fn minimal_doc_and_store() -> (Document<Validated>, HwpxStyleStore) {
        let mut store = HwpxStyleStore::new();
        store.push_font(HwpxFont {
            id: 0, face_name: "함초롬돋움".into(), lang: "HANGUL".into()
        });
        store.push_char_shape(HwpxCharShape {
            font_ref: HwpxFontRef::default(),
            height: HwpUnit::new(1000).unwrap(),
            text_color: Color::BLACK,
            shade_color: None,
            bold: false,
            italic: false,
            underline_type: UnderlineType::None,
            underline_color: None,
            strikeout_shape: StrikeoutShape::None,
            strikeout_color: None,
            vertical_position: VerticalPosition::Normal,
            outline_type: OutlineType::None,
            shadow_type: ShadowType::None,
            emboss_type: EmbossType::None,
            engrave_type: EngraveType::None,
        });
        store.push_para_shape(HwpxParaShape {
            alignment: Alignment::Left,
            margin_left: HwpUnit::ZERO,
            margin_right: HwpUnit::ZERO,
            indent: HwpUnit::ZERO,
            spacing_before: HwpUnit::ZERO,
            spacing_after: HwpUnit::ZERO,
            line_spacing: 160,
            line_spacing_type: LineSpacingType::Percentage,
            ..Default::default()
        });

        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("안녕하세요", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let validated = doc.validate().unwrap();
        (validated, store)
    }

    // ── 1. Basic encode produces valid ZIP ──────────────────────

    #[test]
    fn encode_produces_valid_zip() {
        let (doc, store) = minimal_doc_and_store();
        let bytes = HwpxEncoder::encode(&doc, &store, &ImageStore::new()).unwrap();

        // Must be a valid ZIP (starts with PK magic bytes)
        assert_eq!(&bytes[0..2], b"PK", "output must be a ZIP archive");
        assert!(bytes.len() > 100, "ZIP too small: {} bytes", bytes.len());
    }

    // ── 2. Full encode → decode roundtrip ──────────────────────

    #[test]
    fn encode_decode_roundtrip() {
        let (doc, store) = minimal_doc_and_store();
        let bytes = HwpxEncoder::encode(&doc, &store, &ImageStore::new()).unwrap();

        // Decode the encoded output
        let decoded = HwpxDecoder::decode(&bytes).unwrap();

        // Document structure preserved
        assert_eq!(decoded.document.sections().len(), 1);
        let section = &decoded.document.sections()[0];
        assert_eq!(section.paragraphs.len(), 1);
        assert_eq!(section.paragraphs[0].runs[0].content.as_text(), Some("안녕하세요"),);

        // Style store preserved (fonts expanded to 7 language groups: 1 × 7 = 7)
        assert_eq!(decoded.style_store.font_count(), 7);
        let font = decoded.style_store.font(FontIndex::new(0)).unwrap();
        assert_eq!(font.face_name, "함초롬돋움");
        assert_eq!(font.lang, "HANGUL");

        assert_eq!(decoded.style_store.char_shape_count(), store.char_shape_count());
        let cs = decoded.style_store.char_shape(CharShapeIndex::new(0)).unwrap();
        assert_eq!(cs.height.as_i32(), 1000);
        assert!(!cs.bold);

        assert_eq!(decoded.style_store.para_shape_count(), store.para_shape_count());
        let ps = decoded.style_store.para_shape(ParaShapeIndex::new(0)).unwrap();
        assert_eq!(ps.alignment, Alignment::Left);
        assert_eq!(ps.line_spacing, 160);
    }

    // ── 3. Multi-section roundtrip ─────────────────────────────

    #[test]
    fn multi_section_roundtrip() {
        let (_, store) = minimal_doc_and_store();

        let mut doc = Document::new();
        for i in 0..3 {
            doc.add_section(Section::with_paragraphs(
                vec![Paragraph::with_runs(
                    vec![Run::text(format!("Section {i}"), CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                PageSettings::a4(),
            ));
        }
        let validated = doc.validate().unwrap();

        let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).unwrap();
        let decoded = HwpxDecoder::decode(&bytes).unwrap();

        assert_eq!(decoded.document.sections().len(), 3);
        for i in 0..3 {
            let text =
                decoded.document.sections()[i].paragraphs[0].runs[0].content.as_text().unwrap();
            assert_eq!(text, &format!("Section {i}"));
        }
    }

    // ── 4. Page settings roundtrip ─────────────────────────────

    #[test]
    fn page_settings_roundtrip() {
        let (_, store) = minimal_doc_and_store();

        let custom_ps = PageSettings {
            width: HwpUnit::new(59528).unwrap(),
            height: HwpUnit::new(84188).unwrap(),
            margin_left: HwpUnit::new(8504).unwrap(),
            margin_right: HwpUnit::new(8504).unwrap(),
            margin_top: HwpUnit::new(5668).unwrap(),
            margin_bottom: HwpUnit::new(4252).unwrap(),
            header_margin: HwpUnit::new(4252).unwrap(),
            footer_margin: HwpUnit::new(4252).unwrap(),
        };

        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("Content", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            custom_ps,
        ));
        let validated = doc.validate().unwrap();

        let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).unwrap();
        let decoded = HwpxDecoder::decode(&bytes).unwrap();

        let decoded_ps = &decoded.document.sections()[0].page_settings;
        assert_eq!(decoded_ps.width.as_i32(), 59528);
        assert_eq!(decoded_ps.height.as_i32(), 84188);
        assert_eq!(decoded_ps.margin_left.as_i32(), 8504);
        assert_eq!(decoded_ps.margin_right.as_i32(), 8504);
        assert_eq!(decoded_ps.margin_top.as_i32(), 5668);
        assert_eq!(decoded_ps.margin_bottom.as_i32(), 4252);
    }

    // ── 5. Table roundtrip ─────────────────────────────────────

    #[test]
    fn table_roundtrip() {
        use hwpforge_core::table::{Table, TableCell, TableRow};

        let (_, store) = minimal_doc_and_store();

        let cell1 = TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("A", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::new(5000).unwrap(),
        );
        let cell2 = TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("B", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::new(5000).unwrap(),
        );
        let table = Table::new(vec![TableRow { cells: vec![cell1, cell2], height: None }]);

        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let validated = doc.validate().unwrap();

        let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).unwrap();
        let decoded = HwpxDecoder::decode(&bytes).unwrap();

        let run = &decoded.document.sections()[0].paragraphs[0].runs[0];
        let t = run.content.as_table().unwrap();
        assert_eq!(t.rows.len(), 1);
        assert_eq!(t.rows[0].cells.len(), 2);
        assert_eq!(t.rows[0].cells[0].paragraphs[0].runs[0].content.as_text(), Some("A"),);
        assert_eq!(t.rows[0].cells[1].paragraphs[0].runs[0].content.as_text(), Some("B"),);
    }

    // ── 6. Rich styles roundtrip ───────────────────────────────

    #[test]
    fn rich_styles_roundtrip() {
        let mut store = HwpxStyleStore::new();
        store.push_font(HwpxFont {
            id: 0, face_name: "함초롬돋움".into(), lang: "HANGUL".into()
        });
        store.push_font(HwpxFont { id: 0, face_name: "Arial".into(), lang: "LATIN".into() });
        store.push_char_shape(HwpxCharShape {
            font_ref: HwpxFontRef {
                hangul: FontIndex::new(0),
                latin: FontIndex::new(1),
                ..Default::default()
            },
            height: HwpUnit::new(2400).unwrap(),
            text_color: Color::from_rgb(255, 0, 0),
            shade_color: None,
            bold: true,
            italic: true,
            underline_type: UnderlineType::Bottom,
            underline_color: None,
            strikeout_shape: StrikeoutShape::None,
            strikeout_color: None,
            vertical_position: VerticalPosition::Normal,
            outline_type: OutlineType::None,
            shadow_type: ShadowType::None,
            emboss_type: EmbossType::None,
            engrave_type: EngraveType::None,
        });
        store.push_char_shape(HwpxCharShape::default());
        store.push_para_shape(HwpxParaShape {
            alignment: Alignment::Justify,
            margin_left: HwpUnit::new(200).unwrap(),
            margin_right: HwpUnit::new(100).unwrap(),
            indent: HwpUnit::new(300).unwrap(),
            spacing_before: HwpUnit::new(150).unwrap(),
            spacing_after: HwpUnit::new(50).unwrap(),
            line_spacing: 200,
            line_spacing_type: LineSpacingType::Percentage,
            ..Default::default()
        });

        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![
                    Run::text("Bold+Italic", CharShapeIndex::new(0)),
                    Run::text("Normal", CharShapeIndex::new(1)),
                ],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let validated = doc.validate().unwrap();

        let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).unwrap();
        let decoded = HwpxDecoder::decode(&bytes).unwrap();

        // Fonts: expanded to 7 language groups (1+1+1×5 = 7)
        assert_eq!(decoded.style_store.font_count(), 7);
        assert_eq!(decoded.style_store.font(FontIndex::new(0)).unwrap().face_name, "함초롬돋움");
        assert_eq!(decoded.style_store.font(FontIndex::new(1)).unwrap().face_name, "Arial");

        // Rich char shape
        let cs = decoded.style_store.char_shape(CharShapeIndex::new(0)).unwrap();
        assert_eq!(cs.height.as_i32(), 2400);
        assert_eq!(cs.text_color, Color::from_rgb(255, 0, 0));
        assert!(cs.bold);
        assert!(cs.italic);
        assert_eq!(cs.underline_type, UnderlineType::Bottom);

        // Para shape
        let ps = decoded.style_store.para_shape(ParaShapeIndex::new(0)).unwrap();
        assert_eq!(ps.alignment, Alignment::Justify);
        assert_eq!(ps.margin_left.as_i32(), 200);
        assert_eq!(ps.line_spacing, 200);
    }

    // ── 7. encode_file roundtrip ───────────────────────────────

    #[test]
    fn encode_file_roundtrip() {
        let (doc, store) = minimal_doc_and_store();

        let dir = std::env::temp_dir().join("hwpforge_test_encode_file");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test_output.hwpx");

        HwpxEncoder::encode_file(&path, &doc, &store, &ImageStore::new()).unwrap();

        // Decode the file
        let decoded = HwpxDecoder::decode_file(&path).unwrap();
        assert_eq!(decoded.document.sections().len(), 1);
        assert_eq!(
            decoded.document.sections()[0].paragraphs[0].runs[0].content.as_text(),
            Some("안녕하세요"),
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── 8. encode_file error on bad path ───────────────────────

    #[test]
    fn encode_file_bad_path() {
        let (doc, store) = minimal_doc_and_store();
        let err = HwpxEncoder::encode_file(
            "/nonexistent/dir/test.hwpx",
            &doc,
            &store,
            &ImageStore::new(),
        )
        .unwrap_err();
        assert!(matches!(err, HwpxError::Io(_)));
    }

    // ── 9. Empty style store produces valid output ─────────────

    #[test]
    fn empty_style_store_encode() {
        let store = HwpxStyleStore::new();
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("text", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let validated = doc.validate().unwrap();

        // Should still produce a valid ZIP (no style data, but valid structure)
        let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).unwrap();
        assert_eq!(&bytes[0..2], b"PK");
    }

    // ── 10. Encoded output is decodable ────────────────────────

    #[test]
    fn encoded_output_is_decodable_by_decoder() {
        let (doc, store) = minimal_doc_and_store();
        let bytes = HwpxEncoder::encode(&doc, &store, &ImageStore::new()).unwrap();

        // The key test: the decoder accepts encoder output
        let result = HwpxDecoder::decode(&bytes);
        assert!(result.is_ok(), "Decoder failed on encoder output: {:?}", result.err());
    }
}
