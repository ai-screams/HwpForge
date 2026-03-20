//! HWPX decoding pipeline.
//!
//! Submodules handle individual stages:
//! - `package` — ZIP extraction and file access
//! - `header` — `header.xml` parsing → [`HwpxStyleStore`]
//! - `section` — `section*.xml` parsing → paragraphs + page settings

pub(crate) mod chart;
pub(crate) mod header;
pub(crate) mod package;
pub(crate) mod section;
pub(crate) mod shapes;

use std::path::Path;

use hwpforge_core::document::{Document, Draft};
use hwpforge_core::image::ImageStore;
use hwpforge_core::section::{MasterPage, Section};
use hwpforge_core::PageSettings;
use hwpforge_foundation::ApplyPageType;

use crate::error::HwpxResult;
use crate::style_store::HwpxStyleStore;

// ── HwpxDocument ─────────────────────────────────────────────────

/// The result of decoding an HWPX file.
///
/// Contains the Core document (structure), the HWPX-specific style
/// store (fonts, char shapes, para shapes from `header.xml`), and
/// binary image data extracted from `BinData/` entries.
#[derive(Debug)]
#[non_exhaustive]
pub struct HwpxDocument {
    /// The decoded document in Core's DOM.
    pub document: Document<Draft>,
    /// Style information parsed from `header.xml`.
    pub style_store: HwpxStyleStore,
    /// Binary image data extracted from `BinData/` ZIP entries.
    pub image_store: ImageStore,
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

        // Step 2: Parse header (style store + begin_num)
        let header_xml = pkg.read_header_xml()?;
        let header_result = header::parse_header(&header_xml)?;
        let style_store = header_result.style_store;
        let begin_num = header_result.begin_num;

        // Step 3: Extract chart XMLs from ZIP
        let chart_xmls = pkg.read_chart_xmls()?;

        // Step 4: Extract masterpage XMLs from ZIP and parse them
        let masterpage_xmls = pkg.read_masterpage_xmls()?;
        let parsed_masterpages = parse_masterpages(masterpage_xmls);

        // Step 5: Parse sections
        let mut document = Document::<Draft>::new();
        let section_count = pkg.section_count();
        // Track how many masterpages have been assigned across sections
        let mut masterpage_cursor = 0usize;

        for i in 0..section_count {
            let section_xml = pkg.read_section_xml(i)?;
            let result = section::parse_section(&section_xml, i, &chart_xmls)?;

            let page_settings = result.page_settings.unwrap_or_else(PageSettings::a4);

            // Determine how many masterpages this section owns by scanning
            // the section XML for masterPageCnt attribute (avoids modifying section.rs).
            // Fall back to result.master_pages (parsed inline) if no ZIP files were found.
            let mp_cnt = extract_master_page_cnt(&section_xml);
            let section_master_pages: Option<Vec<MasterPage>> = if mp_cnt > 0 {
                let end = (masterpage_cursor + mp_cnt).min(parsed_masterpages.len());
                let slice = parsed_masterpages[masterpage_cursor..end].to_vec();
                masterpage_cursor = end;
                if slice.is_empty() {
                    result.master_pages
                } else {
                    Some(slice)
                }
            } else {
                result.master_pages
            };

            let section = Section {
                paragraphs: result.paragraphs,
                page_settings,
                header: result.header,
                footer: result.footer,
                page_number: result.page_number,
                column_settings: result.column_settings,
                visibility: result.visibility,
                line_number_shape: result.line_number_shape,
                page_border_fills: result.page_border_fills,
                master_pages: section_master_pages,
                // Per-section startNum from secPr; merge footnote/endnote
                // from header.xml for the first section.
                begin_num: {
                    let mut bn = result.begin_num;
                    if i == 0 {
                        if let (Some(ref mut section_bn), Some(ref header_bn)) =
                            (&mut bn, &begin_num)
                        {
                            section_bn.footnote = header_bn.footnote;
                            section_bn.endnote = header_bn.endnote;
                        } else if bn.is_none() {
                            bn = begin_num;
                        }
                    }
                    bn
                },
                text_direction: result.text_direction,
            };

            document.add_section(section);
        }

        // Step 6: Extract binary image data from BinData/
        let image_store = pkg.read_all_bindata()?;

        Ok(HwpxDocument { document, style_store, image_store })
    }

    /// Decodes an HWPX file from a filesystem path.
    pub fn decode_file(path: impl AsRef<Path>) -> HwpxResult<HwpxDocument> {
        let bytes = std::fs::read(path.as_ref()).map_err(crate::error::HwpxError::Io)?;
        Self::decode(&bytes)
    }
}

// ── Masterpage helpers ────────────────────────────────────────────

/// Parses all masterpage XML strings into [`MasterPage`] structs.
///
/// Input is a map from global masterpage index to raw XML.
/// Returns a `Vec` sorted by index so masterpage 0 comes first.
fn parse_masterpages(xmls: std::collections::HashMap<usize, String>) -> Vec<MasterPage> {
    let mut entries: Vec<(usize, String)> = xmls.into_iter().collect();
    entries.sort_by_key(|(idx, _)| *idx);
    entries.into_iter().map(|(_, xml)| parse_masterpage_xml(&xml)).collect()
}

/// Parses a single masterpage XML string into a [`MasterPage`].
///
/// Extracts the `applyPageType` attribute from the root `<masterPage>` element
/// and the paragraph text from `<hp:subList><hp:p><hp:run><hp:t>` descendants.
/// Unknown `applyPageType` values fall back to `Both`.
fn parse_masterpage_xml(xml: &str) -> MasterPage {
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::{Run, RunContent};
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

    // Extract applyPageType attribute
    let apply_page_type = extract_masterpage_apply_type(xml);

    // Extract paragraphs: find all <hp:p> elements with their attributes.
    // This is a lightweight scan — masterpage paragraphs typically contain
    // minimal or no text content.
    let mut paragraphs = Vec::new();
    let mut search = xml;
    while let Some(p_start) = search.find("<hp:p ").or_else(|| search.find("<hp:p>")) {
        let after_p = &search[p_start..];
        // Find the end of the opening <hp:p ...> tag
        let Some(tag_end) = after_p.find('>') else { break };
        let open_tag = &after_p[..tag_end];
        let after_tag = &after_p[tag_end + 1..];
        let Some(p_close) = after_tag.find("</hp:p>") else { break };
        let p_content = &after_tag[..p_close];

        // Extract paraPrIDRef from the <hp:p> tag
        let para_pr_id = extract_attr_u32(open_tag, "paraPrIDRef");

        // Collect all text runs within this paragraph
        let mut runs = Vec::new();
        let mut run_search = p_content;
        while let Some(r_start) =
            run_search.find("<hp:run ").or_else(|| run_search.find("<hp:run>"))
        {
            let after_r = &run_search[r_start..];
            let Some(r_tag_end) = after_r.find('>') else { break };
            let run_open = &after_r[..r_tag_end];
            let char_pr_id = extract_attr_u32(run_open, "charPrIDRef");

            // Find text within this run
            let after_run_tag = &after_r[r_tag_end + 1..];
            if let Some(t_start) = after_run_tag.find("<hp:t>") {
                let after_t = &after_run_tag[t_start + "<hp:t>".len()..];
                if let Some(t_end) = after_t.find("</hp:t>") {
                    let text = &after_t[..t_end];
                    if !text.is_empty() {
                        runs.push(Run {
                            content: RunContent::Text(text.to_string()),
                            char_shape_id: CharShapeIndex::new(char_pr_id as usize),
                        });
                    }
                }
            }

            // Advance past this run
            let run_end_tag = "</hp:run>";
            if let Some(re) = after_r.find(run_end_tag) {
                run_search = &after_r[re + run_end_tag.len()..];
            } else {
                break;
            }
        }

        let mut para = Paragraph::new(ParaShapeIndex::new(para_pr_id as usize));
        for run in runs {
            para.runs.push(run);
        }
        paragraphs.push(para);

        // Advance past this </hp:p>
        search = &after_tag[p_close + "</hp:p>".len()..];
    }

    MasterPage { apply_page_type, paragraphs }
}

/// Extracts a named u32 attribute value from an XML open-tag string.
///
/// Returns 0 if the attribute is not found or cannot be parsed.
fn extract_attr_u32(open_tag: &str, attr_name: &str) -> u32 {
    let needle = format!("{attr_name}=\"");
    if let Some(pos) = open_tag.find(&needle) {
        let after = &open_tag[pos + needle.len()..];
        if let Some(end) = after.find('"') {
            return after[..end].parse().unwrap_or(0);
        }
    }
    0
}

/// Extracts the `applyPageType` attribute value from a masterpage XML root element.
fn extract_masterpage_apply_type(xml: &str) -> ApplyPageType {
    // Look for type="BOTH"|"EVEN"|"ODD" in the <masterPage ...> opening tag.
    // The encoder writes: <masterPage ... type="BOTH">
    if let Some(pos) = xml.find("type=\"") {
        let after = &xml[pos + "type=\"".len()..];
        if let Some(end) = after.find('"') {
            return match &after[..end] {
                "BOTH" => ApplyPageType::Both,
                "EVEN" => ApplyPageType::Even,
                "ODD" => ApplyPageType::Odd,
                _ => ApplyPageType::Both,
            };
        }
    }
    ApplyPageType::Both
}

/// Extracts `masterPageCnt` from the `<hp:secPr>` element in a section XML string.
///
/// Scans the raw XML for `masterPageCnt="N"` without re-parsing the full XML.
/// Returns 0 if the attribute is absent or unparseable.
fn extract_master_page_cnt(section_xml: &str) -> usize {
    let needle = "masterPageCnt=\"";
    if let Some(pos) = section_xml.find(needle) {
        let after = &section_xml[pos + needle.len()..];
        if let Some(end) = after.find('"') {
            return after[..end].parse().unwrap_or(0);
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_foundation::{HeadingType, NumberFormatType};
    use std::io::{Cursor, Write};
    use std::path::PathBuf;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    /// Creates a complete minimal HWPX for testing.
    fn make_test_hwpx(header_xml: &str, section_xmls: &[&str]) -> Vec<u8> {
        let buf = Vec::new();
        let mut zip = ZipWriter::new(Cursor::new(buf));

        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
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

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name)
    }

    fn decode_fixture(name: &str) -> HwpxDocument {
        let path = fixture_path(name);
        let bytes =
            std::fs::read(&path).unwrap_or_else(|_| panic!("fixture should exist: {path:?}"));
        HwpxDecoder::decode(&bytes).unwrap_or_else(|_| panic!("fixture should decode: {path:?}"))
    }

    fn collect_body_heading_triples(doc: &HwpxDocument) -> Vec<(HeadingType, u32, u32)> {
        doc.document
            .sections()
            .iter()
            .flat_map(|section| section.paragraphs.iter())
            .map(|paragraph| {
                let shape = doc
                    .style_store
                    .para_shape(paragraph.para_shape_id)
                    .expect("paragraph para shape should exist");
                (shape.heading_type, shape.heading_id_ref, shape.heading_level)
            })
            .collect()
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

    // ── Header / Footer / PageNum decode tests ──────────────────

    #[test]
    fn decode_section_with_header_ctrl() {
        let section = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <header id="0" applyPageType="BOTH">
                            <subList id="0" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP"
                                     linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0">
                                <p paraPrIDRef="0">
                                    <run charPrIDRef="0"><t>Page Header</t></run>
                                </p>
                            </subList>
                        </header>
                    </ctrl>
                    <t>Body text</t>
                </run>
            </p>
        </sec>"#;
        let bytes = make_test_hwpx(HEADER, &[section]);
        let result = HwpxDecoder::decode(&bytes).unwrap();

        let sec = &result.document.sections()[0];
        let header = sec.header.as_ref().expect("section should have header");
        assert_eq!(header.apply_page_type, hwpforge_foundation::ApplyPageType::Both);
        assert_eq!(header.paragraphs.len(), 1);
        assert_eq!(header.paragraphs[0].runs[0].content.as_text(), Some("Page Header"));
    }

    #[test]
    fn decode_section_with_footer_and_pagenum() {
        let section = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <footer id="0" applyPageType="ODD">
                            <subList id="0" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP"
                                     linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0">
                                <p paraPrIDRef="0">
                                    <run charPrIDRef="0"><t>Footer</t></run>
                                </p>
                            </subList>
                        </footer>
                    </ctrl>
                    <ctrl>
                        <pageNum pos="BOTTOM_CENTER" formatType="DIGIT" sideChar="- "/>
                    </ctrl>
                    <t>Body</t>
                </run>
            </p>
        </sec>"#;
        let bytes = make_test_hwpx(HEADER, &[section]);
        let result = HwpxDecoder::decode(&bytes).unwrap();

        let sec = &result.document.sections()[0];
        let footer = sec.footer.as_ref().expect("section should have footer");
        assert_eq!(footer.apply_page_type, hwpforge_foundation::ApplyPageType::Odd);
        assert_eq!(footer.paragraphs[0].runs[0].content.as_text(), Some("Footer"));

        let pn = sec.page_number.as_ref().expect("section should have page number");
        assert_eq!(pn.position, hwpforge_foundation::PageNumberPosition::BottomCenter);
        assert_eq!(pn.number_format, hwpforge_foundation::NumberFormatType::Digit);
        assert_eq!(pn.decoration, "- ");
    }

    // ── Image binary roundtrip test ─────────────────────────────

    #[test]
    fn decode_extracts_bindata_images() {
        let buf = Vec::new();
        let mut zip = ZipWriter::new(Cursor::new(buf));
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflate = SimpleFileOptions::default();

        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/hwp+zip").unwrap();

        zip.start_file("Contents/header.xml", deflate).unwrap();
        zip.write_all(HEADER.as_bytes()).unwrap();

        let section = r#"<sec><p paraPrIDRef="0"><run charPrIDRef="0"><t>Body</t></run></p></sec>"#;
        zip.start_file("Contents/section0.xml", deflate).unwrap();
        zip.write_all(section.as_bytes()).unwrap();

        // Add a BinData image
        let fake_png = vec![0x89, 0x50, 0x4E, 0x47]; // PNG magic bytes
        zip.start_file("BinData/logo.png", stored).unwrap();
        zip.write_all(&fake_png).unwrap();

        let bytes = zip.finish().unwrap().into_inner();
        let result = HwpxDecoder::decode(&bytes).unwrap();

        assert!(!result.image_store.is_empty(), "image store should contain extracted images");
        let data = result.image_store.get("logo.png").expect("should find logo.png");
        assert_eq!(data, &fake_png);
    }

    #[test]
    fn decode_user_sample_bullet_list_preserves_bullet_semantics() {
        let decoded = decode_fixture("user_samples/sample-bullet-list.hwpx");
        let headings = collect_body_heading_triples(&decoded);

        assert!(headings.contains(&(HeadingType::Bullet, 1, 0)));
        assert_eq!(decoded.style_store.bullet_count(), 1);
        assert_eq!(decoded.style_store.numbering_count(), 1);
        assert_eq!(decoded.style_store.iter_bullets().next().map(|bullet| bullet.id), Some(1));
    }

    #[test]
    fn decode_user_sample_numbered_list_preserves_numbering_semantics() {
        let decoded = decode_fixture("user_samples/sample-numbered-list.hwpx");
        let headings = collect_body_heading_triples(&decoded);

        assert!(headings.contains(&(HeadingType::Number, 2, 0)));
        assert!(decoded.style_store.numbering_count() >= 2);
    }

    #[test]
    fn decode_user_sample_mixed_lists_with_outline_preserves_all_list_kinds() {
        let decoded = decode_fixture("user_samples/sample-mixed-lists-with-outline.hwpx");
        let headings = collect_body_heading_triples(&decoded);

        assert!(headings.contains(&(HeadingType::Outline, 0, 1)));
        assert!(headings.contains(&(HeadingType::Outline, 0, 2)));
        assert!(headings.contains(&(HeadingType::Bullet, 1, 0)));
        assert!(headings.contains(&(HeadingType::Number, 2, 0)));
        assert!(headings.contains(&(HeadingType::Number, 3, 0)));
        assert_eq!(decoded.style_store.bullet_count(), 1);
        assert!(decoded.style_store.numbering_count() >= 3);
    }

    #[test]
    fn decode_user_sample_numbered_custom_formats_preserves_distinct_numbering_ids() {
        let decoded = decode_fixture("user_samples/sample-numbered-list-custom-formats.hwpx");
        let headings = collect_body_heading_triples(&decoded);

        for id_ref in [2, 3, 4, 5] {
            assert!(headings.contains(&(HeadingType::Number, id_ref, 0)));
        }
        assert!(decoded.style_store.numbering_count() >= 5);
        let numberings: Vec<_> = decoded.style_store.iter_numberings().collect();
        assert_eq!(numberings[1].levels[0].text, "^1)");
        assert_eq!(numberings[2].levels[0].text, "(^1)");
        assert_eq!(numberings[4].levels[6].num_format, NumberFormatType::CircledLatinSmall);
    }
}
