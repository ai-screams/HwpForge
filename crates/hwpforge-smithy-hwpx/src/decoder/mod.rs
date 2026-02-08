//! HWPX decoding pipeline.
//!
//! Submodules handle individual stages:
//! - `package` — ZIP extraction and file access
//! - `header` — `header.xml` parsing → [`HwpxStyleStore`]
//! - `section` — `section*.xml` parsing → paragraphs + page settings

pub(crate) mod header;
pub(crate) mod package;
pub(crate) mod section;

use std::path::Path;

use hwpforge_core::document::{Document, Draft};
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;

use crate::error::HwpxResult;
use crate::style_store::HwpxStyleStore;

// ── HwpxDocument ─────────────────────────────────────────────────

/// The result of decoding an HWPX file.
///
/// Contains both the Core document (structure) and the HWPX-specific
/// style store (fonts, char shapes, para shapes from `header.xml`).
#[derive(Debug)]
#[non_exhaustive]
pub struct HwpxDocument {
    /// The decoded document in Core's DOM.
    pub document: Document<Draft>,
    /// Style information parsed from `header.xml`.
    pub style_store: HwpxStyleStore,
}

// ── HwpxDecoder ──────────────────────────────────────────────────

/// Decodes HWPX files (ZIP + XML) into Core's `Document<Draft>`.
///
/// # Examples
///
/// ```no_run
/// use hwpforge_smithy_hwpx::HwpxDecoder;
///
/// let bytes = std::fs::read("document.hwpx").unwrap();
/// let result = HwpxDecoder::decode(&bytes).unwrap();
/// println!("Sections: {}", result.document.sections().len());
/// ```
pub struct HwpxDecoder;

impl HwpxDecoder {
    /// Decodes an HWPX file from raw bytes.
    ///
    /// Pipeline:
    /// 1. Open ZIP archive, validate mimetype
    /// 2. Parse `Contents/header.xml` → `HwpxStyleStore`
    /// 3. Parse `Contents/section*.xml` → paragraphs + page settings
    /// 4. Assemble `Document<Draft>` with sections
    pub fn decode(bytes: &[u8]) -> HwpxResult<HwpxDocument> {
        // Step 1: Open package
        let mut pkg = package::PackageReader::new(bytes)?;

        // Step 2: Parse header
        let header_xml = pkg.read_header_xml()?;
        let style_store = header::parse_header(&header_xml)?;

        // Step 3: Parse sections
        let mut document = Document::<Draft>::new();
        let section_count = pkg.section_count();

        for i in 0..section_count {
            let section_xml = pkg.read_section_xml(i)?;
            let result = section::parse_section(&section_xml, i)?;

            let page_settings = result
                .page_settings
                .unwrap_or_else(PageSettings::a4);

            let section = Section {
                paragraphs: result.paragraphs,
                page_settings,
            };

            document.add_section(section);
        }

        Ok(HwpxDocument { document, style_store })
    }

    /// Decodes an HWPX file from a filesystem path.
    pub fn decode_file(path: impl AsRef<Path>) -> HwpxResult<HwpxDocument> {
        let bytes = std::fs::read(path.as_ref())
            .map_err(crate::error::HwpxError::Io)?;
        Self::decode(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    /// Creates a complete minimal HWPX for testing.
    fn make_test_hwpx(
        header_xml: &str,
        section_xmls: &[&str],
    ) -> Vec<u8> {
        let buf = Vec::new();
        let mut zip = ZipWriter::new(Cursor::new(buf));

        let stored = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let deflate = SimpleFileOptions::default();

        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/hwp+zip").unwrap();

        zip.start_file("Contents/header.xml", deflate).unwrap();
        zip.write_all(header_xml.as_bytes()).unwrap();

        for (i, xml) in section_xmls.iter().enumerate() {
            let path = format!("Contents/section{}.xml", i);
            zip.start_file(&path, deflate).unwrap();
            zip.write_all(xml.as_bytes()).unwrap();
        }

        zip.finish().unwrap().into_inner()
    }

    const HEADER: &str = r##"<head version="1.4" secCnt="1">
        <refList>
            <fontfaces itemCnt="1">
                <fontface lang="HANGUL" fontCnt="1">
                    <font id="0" face="함초롬돋움" type="TTF" isEmbedded="0"/>
                </fontface>
            </fontfaces>
            <charProperties itemCnt="1">
                <charPr id="0" height="1000" textColor="#000000" shadeColor="none"
                        useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="0">
                    <fontRef hangul="0" latin="0" hanja="0" japanese="0" other="0" symbol="0" user="0"/>
                </charPr>
            </charProperties>
            <paraProperties itemCnt="1">
                <paraPr id="0">
                    <align horizontal="LEFT" vertical="BASELINE"/>
                    <switch><default>
                        <lineSpacing type="PERCENT" value="160"/>
                    </default></switch>
                </paraPr>
            </paraProperties>
        </refList>
    </head>"##;

    const SECTION_TEXT: &str = r#"<sec>
        <p paraPrIDRef="0">
            <run charPrIDRef="0">
                <secPr textDirection="HORIZONTAL">
                    <pagePr landscape="WIDELY" width="59528" height="84188">
                        <margin header="4252" footer="4252" gutter="0"
                                left="8504" right="8504" top="5668" bottom="4252"/>
                    </pagePr>
                </secPr>
                <t>안녕하세요</t>
            </run>
        </p>
    </sec>"#;

    // ── Full pipeline tests ──────────────────────────────────────

    #[test]
    fn decode_minimal_hwpx() {
        let bytes = make_test_hwpx(HEADER, &[SECTION_TEXT]);
        let result = HwpxDecoder::decode(&bytes).unwrap();

        // Document structure
        assert_eq!(result.document.sections().len(), 1);
        let section = &result.document.sections()[0];
        assert_eq!(section.paragraphs.len(), 1);

        // Text content
        let text = section.paragraphs[0].runs[0].content.as_text();
        assert_eq!(text, Some("안녕하세요"));

        // Page settings
        assert_eq!(section.page_settings.width.as_i32(), 59528);
        assert_eq!(section.page_settings.height.as_i32(), 84188);

        // Style store
        assert_eq!(result.style_store.font_count(), 1);
        assert_eq!(result.style_store.char_shape_count(), 1);
        assert_eq!(result.style_store.para_shape_count(), 1);
    }

    #[test]
    fn decode_multiple_sections() {
        let s0 = r#"<sec><p paraPrIDRef="0"><run charPrIDRef="0"><t>Section 0</t></run></p></sec>"#;
        let s1 = r#"<sec><p paraPrIDRef="0"><run charPrIDRef="0"><t>Section 1</t></run></p></sec>"#;
        let bytes = make_test_hwpx(HEADER, &[s0, s1]);
        let result = HwpxDecoder::decode(&bytes).unwrap();
        assert_eq!(result.document.sections().len(), 2);
    }

    #[test]
    fn decode_with_table() {
        let section = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <tbl rowCnt="1" colCnt="1">
                        <tr>
                            <tc name="A1">
                                <cellSz width="5000" height="1000"/>
                                <subList><p paraPrIDRef="0"><run charPrIDRef="0"><t>Cell</t></run></p></subList>
                            </tc>
                        </tr>
                    </tbl>
                </run>
            </p>
        </sec>"#;
        let bytes = make_test_hwpx(HEADER, &[section]);
        let result = HwpxDecoder::decode(&bytes).unwrap();
        let run = &result.document.sections()[0].paragraphs[0].runs[0];
        assert!(run.content.is_table());
    }

    #[test]
    fn decode_section_without_secpr_uses_a4_defaults() {
        let section = r#"<sec><p paraPrIDRef="0"><run charPrIDRef="0"><t>Text</t></run></p></sec>"#;
        let bytes = make_test_hwpx(HEADER, &[section]);
        let result = HwpxDecoder::decode(&bytes).unwrap();
        let ps = &result.document.sections()[0].page_settings;
        assert_eq!(*ps, PageSettings::a4());
    }

    #[test]
    fn decode_not_a_zip() {
        let err = HwpxDecoder::decode(b"not a zip").unwrap_err();
        assert!(matches!(err, crate::error::HwpxError::Zip(_)));
    }

    #[test]
    fn decode_file_nonexistent() {
        let err = HwpxDecoder::decode_file("/nonexistent/path.hwpx").unwrap_err();
        assert!(matches!(err, crate::error::HwpxError::Io(_)));
    }
}
