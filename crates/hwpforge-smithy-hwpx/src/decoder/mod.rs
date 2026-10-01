//! HWPX decoding pipeline.
//!
//! Submodules handle individual stages:
//! - `package` — ZIP extraction and file access
//! - `header` — `header.xml` parsing → [`HwpxStyleStore`]
//! - `section` — `section*.xml` parsing → paragraphs + page settings

pub(crate) mod chart;
pub(crate) mod header;
pub(crate) mod metadata;
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

// ── serde decode entry point ─────────────────────────────────────

/// Serde recursion budget for the HWPX parts this crate deserializes with
/// serde (`header.xml`, `section*.xml`, and the patch path's section
/// reparse; `content.hpf` and chart XML use the pull reader instead).
///
/// quick-xml 0.42 caps serde nesting at 128 by default, which rejects
/// tables nested 21 deep — well inside our own
/// [`MAX_NESTING_DEPTH`](section::MAX_NESTING_DEPTH) guard of 32, which the
/// encoder also honours. A nested table spans six XML element levels and a
/// text box five (measured on real fixtures), so seven per nesting level —
/// `7 * 32 = 224` — keeps even the 33rd level inside the budget: that level
/// is rejected by our structural nesting guards rather than by the serde
/// budget, the boundary quick-xml 0.41 had.
///
/// **Provisional:** the value may change in a minor release once more real
/// documents have been measured. Deep inputs still need stack; the measured
/// minimum per build profile is on [`HwpxDecoder::decode`].
pub(crate) const XML_RECURSION_LIMIT: usize = 7 * section::MAX_NESTING_DEPTH;

/// Upper bound on namespace bindings in scope while deserializing.
///
/// quick-xml 0.42 counts bindings in scope (not per element as 0.41 did) and
/// defaults to 128. The value is set explicitly so a future quick-xml bump
/// cannot move it silently. The largest count seen in real documents is 15.
pub(crate) const XML_MAX_NAMESPACE_BINDINGS: usize = 128;

/// Deserializes one HWPX XML part with this crate's explicit limits.
///
/// Every serde decode in this crate goes through here so the recursion and
/// namespace limits are applied uniformly; `quick_xml::de::from_str` is
/// rejected by the workspace `.clippy.toml` (`disallowed-methods`).
pub(crate) fn xml_from_str<'de, T: serde::Deserialize<'de>>(
    xml: &'de str,
) -> Result<T, quick_xml::DeError> {
    #[allow(clippy::disallowed_methods)] // the one place that sets the limits
    let mut de = quick_xml::de::Deserializer::from_str(xml);
    de.recursion_limit(XML_RECURSION_LIMIT);
    de.resolver_mut().set_max_namespace_bindings(XML_MAX_NAMESPACE_BINDINGS);
    T::deserialize(&mut de)
}

/// The `detail` text for a failed [`xml_from_str`], as users see it in
/// `DECODE_FAILED`.
///
/// quick-xml's own text for the namespace limit tells the reader to call
/// `NamespaceResolver::set_max_namespace_bindings`, which no caller of the
/// CLI, MCP server or Python binding can do; it is replaced by a sentence
/// that states the limit. Every other error keeps quick-xml's text.
pub(crate) fn xml_error_detail(error: &quick_xml::DeError) -> String {
    use quick_xml::name::NamespaceError;
    match error {
        quick_xml::DeError::InvalidXml(quick_xml::Error::Namespace(
            NamespaceError::TooManyBindings(limit),
        )) => format!(
            "more than {limit} namespace bindings in scope; HwpForge does not read \
             documents that declare more"
        ),
        other => other.to_string(),
    }
}

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
    /// 디코드 중 표면화된 비치명 경고 (W5-α M4 — warning-first).
    pub warnings: Vec<DecodeWarning>,
}

/// 디코드 중 표면화된 비치명 경고.
///
/// 디코더가 미지 wire 값을 기본값으로 **조용히** 폴백하면 하류(렌더러
/// 등)의 warning-first 는 증거가 세탁된 뒤라 무력해진다 (W5-α M4) —
/// 폴백은 유지하되 사실을 표면화한다. 속성 결측(빈 문자열)은 기본값
/// 적용이 정상이므로 경고 대상이 아니다.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecodeWarning {
    /// 알 수 없는 enum 문자열을 기본값으로 폴백함.
    UnknownEnumValue {
        /// 요소@속성 경로 (예: `hp:header@applyPageType`).
        attribute: &'static str,
        /// 원본 wire 문자열.
        raw: String,
        /// 폴백한 값의 wire 표기.
        fallback: &'static str,
    },
    /// 문단의 linesegarray 캐시가 Core 로 승격되지 않음 — wire→Core
    /// 좌표 ledger 구축 실패 또는 textpos 정규화 실패 (W1b fail-closed,
    /// 추측 좌표 승격 금지).
    LayoutCacheDropped {
        /// 경고가 난 문단의 중첩 경로.
        path: ParagraphPath,
        /// 구체 사유 (ledger 실패 원인 또는 문제 textpos).
        reason: String,
    },
}

/// 디코드 경고가 가리키는 문단의 중첩 경로 (§1g v5 변경 5).
///
/// `(section, paragraph)` 튜플로는 셀/머리말/각주 내부 문단을 표현할 수
/// 없어 ordered segment vector 로 기록한다. `Section` 으로 시작해 최종
/// 문단 segment 로 끝난다 — sublist 진입 시 인덱스를 리셋하거나 depth
/// 만 전달하지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParagraphPath(pub Vec<PathSeg>);

/// [`ParagraphPath`] 의 segment 하나.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathSeg {
    /// 구역 인덱스.
    Section(usize),
    /// 본문 문단 인덱스.
    BodyParagraph(usize),
    /// 머리말 인덱스.
    Header(usize),
    /// 꼬리말 인덱스.
    Footer(usize),
    /// run 인덱스.
    Run(usize),
    /// 표 셀 (행, 셀).
    TableCell {
        /// 행 인덱스.
        row: usize,
        /// 셀 인덱스.
        cell: usize,
    },
    /// 캡션 내부.
    Caption,
    /// 각주 내부.
    Footnote,
    /// 미주 내부.
    Endnote,
    /// 글상자 내부.
    TextBox,
    /// 메모 내부.
    Memo,
    /// 묶음 객체 자식 인덱스.
    GroupChild(usize),
    /// 중첩 sublist 문단 인덱스 (컨테이너 내부 문단).
    NestedParagraph(usize),
}

impl std::fmt::Display for ParagraphPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, seg) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, ".")?;
            }
            match seg {
                PathSeg::Section(n) => write!(f, "section[{n}]")?,
                PathSeg::BodyParagraph(n) => write!(f, "para[{n}]")?,
                PathSeg::Header(n) => write!(f, "header[{n}]")?,
                PathSeg::Footer(n) => write!(f, "footer[{n}]")?,
                PathSeg::Run(n) => write!(f, "run[{n}]")?,
                PathSeg::TableCell { row, cell } => write!(f, "cell[{row}][{cell}]")?,
                PathSeg::Caption => write!(f, "caption")?,
                PathSeg::Footnote => write!(f, "footnote")?,
                PathSeg::Endnote => write!(f, "endnote")?,
                PathSeg::TextBox => write!(f, "textbox")?,
                PathSeg::Memo => write!(f, "memo")?,
                PathSeg::GroupChild(n) => write!(f, "group[{n}]")?,
                PathSeg::NestedParagraph(n) => write!(f, "npara[{n}]")?,
            }
        }
        Ok(())
    }
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
    ///
    /// # Limits
    ///
    /// `header.xml` and `section*.xml` are deserialized with a nesting budget
    /// of 224 XML levels (provisional: it may change in a minor release once
    /// more real documents have been measured), enough for tables, text boxes or
    /// table/text-box mixes nested 32 deep. The 33rd level is rejected as
    /// [`HwpxError::InvalidStructure`](crate::HwpxError::InvalidStructure) by
    /// our structural nesting guards. A part deeper than the budget, or one
    /// with more than 128 namespace bindings in scope, fails with
    /// [`HwpxError::XmlParse`](crate::HwpxError::XmlParse). Other parts
    /// (`content.hpf`, charts) are read without these two limits.
    ///
    /// # Stack
    ///
    /// Deeply nested input recurses on the calling thread's stack, and an
    /// overflow aborts the process instead of returning an error. Measured
    /// through this function on macOS arm64, with packages holding nested
    /// tables and text boxes up to and past the budget:
    ///
    /// | Build | Stack that decoded every input | Overflowed at |
    /// | -- | -- | -- |
    /// | release (optimized) | 1 MiB | 512 KiB |
    /// | `opt-level = 1` (the `cargo test` profile) | 1 MiB | 768 KiB |
    /// | `opt-level = 0` (unoptimized dev build) | 4 MiB | 3 MiB |
    ///
    /// So in an optimized build give the calling thread 2 MiB of stack.
    /// 1 MiB is the measured minimum, but how much a build needs depends on
    /// how the compiler inlines the recursive decode: a test binary that
    /// compiled extra code into the same crate needed about twice as much
    /// for the same input. Rust's default 2 MiB spawned threads qualify, and
    /// so does the main thread on Linux and macOS (typically 8 MiB). On
    /// Windows the main thread defaults to 1 MiB in total, part of it
    /// already used by the caller, so decode deeply nested input on a
    /// spawned thread there. An unoptimized build needs about 4 MiB, which
    /// Rust's default spawned threads do not have. The 1 MiB release figure
    /// is also checked by a test on Linux x86_64 in CI; other platforms are
    /// not measured.
    pub fn decode(bytes: &[u8]) -> HwpxResult<HwpxDocument> {
        // Step 1: Open package
        let mut pkg = package::PackageReader::new(bytes)?;

        // Step 2: Parse header (style store + begin_num)
        let header_xml = pkg.read_header_xml()?;
        let header_result = header::parse_header(&header_xml)?;
        let style_store = header_result.style_store;
        let begin_num = header_result.begin_num;

        // Step 2b (Wave 12o): Parse Contents/content.hpf metadata.
        // Missing or malformed metadata downgrades to default rather
        // than failing the whole decode — Hancom-emitted XML always
        // has a metadata block, but third-party authoring tools may not.
        let metadata = match pkg.read_text_entry("Contents/content.hpf") {
            Ok(xml) => metadata::parse_content_hpf_metadata(&xml).unwrap_or_default(),
            Err(_) => hwpforge_core::metadata::Metadata::default(),
        };

        // Step 3: Extract chart XMLs from ZIP
        let chart_xmls = pkg.read_chart_xmls()?;

        // Step 4: Extract masterpage XMLs from ZIP and parse them
        let masterpage_xmls = pkg.read_masterpage_xmls()?;
        let parsed_masterpages = parse_masterpages(masterpage_xmls);

        // Step 5: Parse sections
        let mut document = Document::<Draft>::with_metadata(metadata);
        let section_count = pkg.section_count();
        // Track how many masterpages have been assigned across sections
        let mut masterpage_cursor = 0usize;
        let mut warnings: Vec<DecodeWarning> = Vec::new();

        for i in 0..section_count {
            let section_xml = pkg.read_section_xml(i)?;
            let mut result = section::parse_section(&section_xml, i, &chart_xmls)?;
            warnings.append(&mut result.warnings);

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
                // ADR-002 cardinality — 파서가 wire 순서 그대로의 Vec 을
                // 돌려준다 (W5-α C1: ODD/EVEN 다중 머리말 보존).
                headers: result.headers,
                footers: result.footers,
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

        Ok(HwpxDocument { document, style_store, image_store, warnings })
    }

    /// Decodes an HWPX file from a filesystem path.
    ///
    /// The nesting and namespace limits of [`decode`](Self::decode) apply,
    /// including its provisional 224-level budget and its stack advice for
    /// the calling thread (2 MiB recommended, 1 MiB measured).
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
        let header = sec.headers.first().expect("section should have header");
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
        let footer = sec.footers.first().expect("section should have footer");
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
        let decoded = decode_fixture("user_samples/lists/sample-bullet-list.hwpx");
        let headings = collect_body_heading_triples(&decoded);

        assert!(headings.contains(&(HeadingType::Bullet, 1, 0)));
        assert_eq!(decoded.style_store.bullet_count(), 1);
        assert_eq!(decoded.style_store.numbering_count(), 1);
        assert_eq!(decoded.style_store.iter_bullets().next().map(|bullet| bullet.id), Some(1));
    }

    #[test]
    fn decode_user_sample_numbered_list_preserves_numbering_semantics() {
        let decoded = decode_fixture("user_samples/lists/sample-numbered-list.hwpx");
        let headings = collect_body_heading_triples(&decoded);

        assert!(headings.contains(&(HeadingType::Number, 2, 0)));
        assert!(decoded.style_store.numbering_count() >= 2);
    }

    #[test]
    fn decode_user_sample_mixed_lists_with_outline_preserves_all_list_kinds() {
        let decoded = decode_fixture("user_samples/lists/sample-mixed-lists-with-outline.hwpx");
        let headings = collect_body_heading_triples(&decoded);

        assert!(headings.contains(&(HeadingType::Outline, 0, 0)));
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
        let decoded = decode_fixture("user_samples/lists/sample-numbered-list-custom-formats.hwpx");
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

    #[test]
    fn decode_user_sample_checkable_bullet_basic_preserves_checked_glyph_and_item_state() {
        let decoded = decode_fixture("user_samples/lists/sample-checkable-bullet-basic.hwpx");
        let paragraphs = &decoded.document.sections()[0].paragraphs;

        let unchecked = paragraphs
            .iter()
            .find(|paragraph| paragraph.text_content().contains("unchecked item A"))
            .expect("fixture should contain unchecked item A");
        let checked = paragraphs
            .iter()
            .find(|paragraph| paragraph.text_content().contains("checked item B"))
            .expect("fixture should contain checked item B");

        let unchecked_shape = decoded.style_store.para_shape(unchecked.para_shape_id).unwrap();
        let checked_shape = decoded.style_store.para_shape(checked.para_shape_id).unwrap();
        let bullet = decoded
            .style_store
            .iter_bullets()
            .find(|bullet| bullet.id == unchecked_shape.heading_id_ref)
            .expect("checkable bullet definition should exist");

        assert_eq!(unchecked_shape.heading_type, HeadingType::Bullet);
        assert_eq!(checked_shape.heading_type, HeadingType::Bullet);
        assert!(bullet.is_checkable());
        assert_eq!(bullet.checked_char.as_deref(), Some("☑"));
        assert!(!unchecked_shape.checked);
        assert!(checked_shape.checked);
    }

    #[test]
    fn decode_user_sample_checkable_bullet_nested_preserves_depth() {
        let decoded = decode_fixture("user_samples/lists/sample-checkable-bullet-nested.hwpx");
        let paragraphs = &decoded.document.sections()[0].paragraphs;

        let level1 = paragraphs
            .iter()
            .find(|paragraph| paragraph.text_content().contains("level 1 unchecked"))
            .expect("fixture should contain level 1 item");
        let level2 = paragraphs
            .iter()
            .find(|paragraph| paragraph.text_content().contains("level 2 checked"))
            .expect("fixture should contain level 2 item");
        let level3 = paragraphs
            .iter()
            .find(|paragraph| paragraph.text_content().contains("level 3 unchecked"))
            .expect("fixture should contain level 3 item");

        assert_eq!(decoded.style_store.para_shape(level1.para_shape_id).unwrap().heading_level, 0);
        assert_eq!(decoded.style_store.para_shape(level2.para_shape_id).unwrap().heading_level, 1);
        assert_eq!(decoded.style_store.para_shape(level3.para_shape_id).unwrap().heading_level, 2);
    }
}

/// Limits of the shared serde entry point [`xml_from_str`]: our structural
/// nesting guards must stay the error that fires at 33 levels (not the serde
/// recursion budget), and the namespace-binding cap is 128 in scope.
///
/// Boundary cases run on a 64 MiB thread so they hold in every build
/// profile (an unoptimized build overflows 2 MiB at 32 nested tables, and an
/// overflow aborts the test process). The documented 1 MiB minimum for
/// optimized builds is checked by `tests/decode_stack.rs`, which links the
/// library as it ships.
#[cfg(test)]
mod xml_limit_tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::section::parse_section;
    use crate::error::HwpxError;

    /// The A1 cell run of `tables/table_01_basic_2x2.hwpx`.
    const TABLE_ANCHOR: &str = r#"<hp:run charPrIDRef="0"><hp:t>A1</hp:t></hp:run>"#;
    /// The drawText run of `images/textbox_anchored.hwpx`.
    const TEXTBOX_ANCHOR: &str =
        r#"<hp:run charPrIDRef="0"><hp:t>앵커형 글상자입니다.</hp:t></hp:run>"#;
    /// Text placed in the innermost level; a decode that drops it is a
    /// silent loss, not a success.
    const DEEPEST: &str = "XML_LIMIT_DEEPEST";

    fn fixture_section(rel: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(rel);
        let bytes = std::fs::read(&path).expect("read fixture");
        super::package::PackageReader::new(&bytes)
            .expect("open fixture package")
            .read_section_xml(0)
            .expect("read section0.xml")
    }

    /// Byte range of the first `<open …>…</close>` element.
    fn first_element(xml: &str, open: &str, close: &str) -> std::ops::Range<usize> {
        let start = xml.find(open).expect("element start");
        let end = xml.find(close).expect("element end") + close.len();
        start..end
    }

    /// The table fixture's section with its table replaced by `levels`
    /// nested levels following `pattern` (`T` = table, `B` = textbox), the
    /// outermost level being `pattern[0]`. Real fixture fragments keep the
    /// per-level element depth equal to the measured one (T 6, B 5).
    fn nested_section(pattern: &str, levels: usize) -> String {
        let section = fixture_section("tables/table_01_basic_2x2.hwpx");
        let table_span = first_element(&section, "<hp:tbl", "</hp:tbl>");
        let table = &section[table_span.clone()];
        assert_eq!(table.matches(TABLE_ANCHOR).count(), 1, "table anchor");
        let box_section = fixture_section("images/textbox_anchored.hwpx");
        let textbox = &box_section[first_element(&box_section, "<hp:rect", "</hp:rect>")];
        assert_eq!(textbox.matches(TEXTBOX_ANCHOR).count(), 1, "textbox anchor");

        let kinds: Vec<char> = pattern.chars().cycle().take(levels).collect();
        let mut fragment = String::new();
        for (index, kind) in kinds.iter().rev().enumerate() {
            let text = if index == 0 { DEEPEST } else { "outer" };
            let run = format!(r#"<hp:run charPrIDRef="0">{fragment}<hp:t>{text}</hp:t></hp:run>"#);
            fragment = match kind {
                'T' => table.replacen(TABLE_ANCHOR, &run, 1),
                'B' => textbox.replacen(TEXTBOX_ANCHOR, &run, 1),
                other => panic!("unknown nesting kind {other}"),
            };
        }
        format!("{}{}{}", &section[..table_span.start], fragment, &section[table_span.end..])
    }

    /// Decodes on a 64 MiB thread; returns the paragraphs' debug text.
    fn decode_on_big_stack(xml: String) -> Result<String, HwpxError> {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(move || {
                parse_section(&xml, 0, &HashMap::new()).map(|r| format!("{:?}", r.paragraphs))
            })
            .expect("spawn decode thread")
            .join()
            .expect("decode thread panicked")
    }

    fn assert_decodes_to_deepest(pattern: &str, levels: usize) {
        match decode_on_big_stack(nested_section(pattern, levels)) {
            Ok(debug) => assert!(debug.contains(DEEPEST), "{pattern} x{levels}: deepest text lost"),
            Err(err) => panic!("{pattern} x{levels} must decode, got: {err:?}"),
        }
    }

    /// `guard` is `"table"` or `"sublist"`: which structural guard fires.
    fn assert_nesting_guard(pattern: &str, levels: usize, guard: &str) {
        let expected = format!("{guard} nesting depth 32 exceeds limit of 32");
        match decode_on_big_stack(nested_section(pattern, levels)) {
            Err(HwpxError::InvalidStructure { detail }) if detail == expected => {}
            other => panic!("{pattern} x{levels} must hit the {guard} guard, got: {other:?}"),
        }
    }

    // 이것을 실패시키는 것: `xml_from_str` 에서 `recursion_limit` 호출을 빼기
    // (0.42 기본 128 → XmlParse "recursion limit of 128 exceeded"), 또는
    // section.rs 가 `quick_xml::de::from_str` 를 직접 부르기.
    #[test]
    fn xml_limit_table_nesting_32_decodes() {
        assert_decodes_to_deepest("T", 32);
    }

    // 이것을 실패시키는 것: `XML_RECURSION_LIMIT` 를 200 으로 낮추기 — 가드보다
    // serde 재귀 한도가 먼저 걸려 XmlParse 가 된다. 표 가드(section.rs
    // `convert_table`)를 `depth > MAX_NESTING_DEPTH` 로 바꾸기.
    #[test]
    fn xml_limit_table_nesting_33_hits_nesting_guard() {
        assert_nesting_guard("T", 33, "table");
    }

    // 이것을 실패시키는 것: `recursion_limit` 호출을 빼기 (TB 32 = 요소 깊이 180).
    #[test]
    fn xml_limit_table_textbox_nesting_32_decodes() {
        assert_decodes_to_deepest("TB", 32);
    }

    // TB 33 은 표 가드가 먼저 걸린다(실측).
    // 이것을 실패시키는 것: `XML_RECURSION_LIMIT` 를 180 으로 낮추기 — TB 33 에서
    // 가드보다 serde 재귀 한도가 먼저 걸린다. 표 가드를 `>` 로 바꾸기.
    #[test]
    fn xml_limit_table_textbox_nesting_33_hits_nesting_guard() {
        assert_nesting_guard("TB", 33, "table");
    }

    // 이것을 실패시키는 것: `recursion_limit` 호출을 빼기 (B 32 = 요소 깊이 164).
    #[test]
    fn xml_limit_textbox_nesting_32_decodes() {
        assert_decodes_to_deepest("B", 32);
    }

    // 글상자만 겹치면 표가 아니라 subList 가드가 먼저 걸린다.
    // 이것을 실패시키는 것: `XML_RECURSION_LIMIT` 를 160 으로 낮추기 — B 33 (요소
    // 깊이 169) 에서 가드보다 serde 재귀 한도가 먼저 걸린다. subList 가드
    // (section.rs `decode_sublist_paragraphs_skipping`)를 `>` 로 바꾸기.
    #[test]
    fn xml_limit_textbox_nesting_33_hits_sublist_guard() {
        assert_nesting_guard("B", 33, "sublist");
    }

    // 이것을 실패시키는 것: `XML_RECURSION_LIMIT` 를 크게 올리기(예: 10000) —
    // 표 42겹이 serde 를 통과해 가드 오류로 바뀐다.
    #[test]
    fn xml_limit_table_nesting_42_fails_closed_on_recursion_limit() {
        match decode_on_big_stack(nested_section("T", 42)) {
            Err(HwpxError::XmlParse { detail, .. })
                if detail.contains("recursion limit of 224 exceeded") => {}
            other => panic!("table x42 must fail on the serde recursion limit, got: {other:?}"),
        }
    }

    /// `n` namespace declarations with distinct prefixes starting `tag`.
    fn namespace_decls(tag: &str, n: usize) -> String {
        (0..n).map(|k| format!(r#" xmlns:{tag}{k}="urn:hwpforge-test:{tag}{k}""#)).collect()
    }

    /// A minimal section whose `sec`, `p` and `run` elements declare the
    /// given numbers of namespace bindings, all in scope at the run.
    fn namespace_section(on_sec: usize, on_p: usize, on_run: usize) -> String {
        format!(
            r#"<sec{}><p paraPrIDRef="0"{}><run charPrIDRef="0"{}><t>ns</t></run></p></sec>"#,
            namespace_decls("s", on_sec),
            namespace_decls("p", on_p),
            namespace_decls("r", on_run),
        )
    }

    fn assert_namespace_limit_rejects(xml: &str) {
        match parse_section(xml, 0, &HashMap::new()) {
            // 이것을 실패시키는 것: `xml_error_detail` 없이 quick-xml 문구를 그대로 쓰는 것 —
            // 사용자가 부를 수 없는 `NamespaceResolver` API 를 안내한다.
            Err(HwpxError::XmlParse { detail, .. })
                if detail.contains("more than 128 namespace bindings")
                    && !detail.contains("NamespaceResolver") => {}
            other => panic!("129 bindings in scope must be rejected, got: {other:?}"),
        }
    }

    // quick-xml 0.42 의 기본값도 128 이라 `set_max_namespace_bindings` 호출을
    // 빼는 변이는 오늘 이 테스트들을 빨갛게 만들지 못한다(no-op). 호출은 다음
    // bump 에서 기본값이 움직이는 것을 막으려고 둔다.
    // 이것을 실패시키는 것: `XML_MAX_NAMESPACE_BINDINGS` 를 127 로 낮추기.
    #[test]
    fn xml_limit_namespace_bindings_128_on_root_decode() {
        parse_section(&namespace_section(128, 0, 0), 0, &HashMap::new()).expect("128 bindings");
    }

    // 이것을 실패시키는 것: `XML_MAX_NAMESPACE_BINDINGS` 를 256 (또는 usize::MAX)
    // 으로 올리기.
    #[test]
    fn xml_limit_namespace_bindings_129_on_root_rejected() {
        assert_namespace_limit_rejects(&namespace_section(129, 0, 0));
    }

    // 요소 하나당이 아니라 범위 안 합계를 센다(0.41 은 요소당 256).
    // 이것을 실패시키는 것: `XML_MAX_NAMESPACE_BINDINGS` 를 127 로 낮추기.
    #[test]
    fn xml_limit_namespace_bindings_128_spread_decode() {
        parse_section(&namespace_section(43, 43, 42), 0, &HashMap::new())
            .expect("128 bindings spread over sec/p/run");
    }

    // 이것을 실패시키는 것: `XML_MAX_NAMESPACE_BINDINGS` 를 256 으로 올리기.
    #[test]
    fn xml_limit_namespace_bindings_129_spread_rejected() {
        assert_namespace_limit_rejects(&namespace_section(43, 43, 43));
    }
}
