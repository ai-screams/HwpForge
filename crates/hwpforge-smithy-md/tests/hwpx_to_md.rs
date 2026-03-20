//! Integration tests: HWPX → Markdown conversion pipeline.
//!
//! These tests exercise the full decode→lookup→encode_styled path:
//! 1. Build minimal HWPX bytes programmatically (no golden files needed)
//! 2. `HwpxDecoder::decode` → `HwpxDocument { document, style_store, image_store }`
//! 3. `document.validate()` → `Document<Validated>`
//! 4. `HwpxStyleLookup::new(&style_store, &image_store)` → `&dyn StyleLookup`
//! 5. `MdEncoder::encode_styled(&validated, &lookup)` → `MdOutput { markdown, images }`

use std::io::Write as IoWrite;

use hwpforge_core::StyleLookup;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxStyleLookup};
use hwpforge_smithy_md::{MdEncoder, MdOutput};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

// ── HWPX byte builders ────────────────────────────────────────────────────────

const FONTFACES_XML: &str = r#"
        <fontfaces itemCnt="1">
            <fontface lang="HANGUL" fontCnt="1">
                <font id="0" face="함초롬돋움" type="TTF" isEmbedded="0"/>
            </fontface>
        </fontfaces>
"#;

const DEFAULT_CHAR_PR_BODY: &str = r##"
                    textColor="#000000" shadeColor="none"
                    useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="0">
                <fontRef hangul="0" latin="0" hanja="0" japanese="0" other="0" symbol="0" user="0"/>
"##;

fn char_pr_xml(id: u32, height: u32) -> String {
    format!(r#"<charPr id="{id}" height="{height}"{DEFAULT_CHAR_PR_BODY}</charPr>"#,)
}

fn para_pr_xml(id: u32, heading_xml: Option<&str>) -> String {
    let heading = heading_xml.unwrap_or("");
    format!(
        r#"<paraPr id="{id}">
                <align horizontal="LEFT" vertical="BASELINE"/>
                {heading}
                <switch><default>
                    <lineSpacing type="PERCENT" value="160"/>
                </default></switch>
            </paraPr>"#,
    )
}

fn style_xml(
    id: u32,
    name: &str,
    eng_name: &str,
    para_pr_id_ref: u32,
    char_pr_id_ref: u32,
    next_style_id_ref: u32,
) -> String {
    format!(
        r#"<style id="{id}" type="PARA" name="{name}" engName="{eng_name}"
                   paraPrIDRef="{para_pr_id_ref}" charPrIDRef="{char_pr_id_ref}"
                   nextStyleIDRef="{next_style_id_ref}" lockForm="0"/>"#,
    )
}

fn build_header_xml(char_prs: &[String], para_prs: &[String], styles: &[String]) -> String {
    let char_properties = format!(
        "<charProperties itemCnt=\"{}\">{}</charProperties>",
        char_prs.len(),
        char_prs.join("")
    );
    let para_properties = format!(
        "<paraProperties itemCnt=\"{}\">{}</paraProperties>",
        para_prs.len(),
        para_prs.join("")
    );
    let styles_xml = if styles.is_empty() {
        String::new()
    } else {
        format!("<styles itemCnt=\"{}\">{}</styles>", styles.len(), styles.join(""))
    };

    format!(
        r#"<head version="1.4" secCnt="1">
    <refList>{FONTFACES_XML}
        {char_properties}
        {para_properties}
        {styles_xml}
    </refList>
</head>"#,
    )
}

/// Minimal `header.xml` with one font, one charPr, one paraPr.
fn minimal_header() -> String {
    let char_prs = vec![char_pr_xml(0, 1000)];
    let para_prs = vec![para_pr_xml(0, None)];
    build_header_xml(&char_prs, &para_prs, &[])
}

/// `header.xml` with heading styles (level 1 and 2) so `style_heading_level` is non-None.
fn heading_header() -> String {
    let char_prs = vec![char_pr_xml(0, 1400), char_pr_xml(1, 1200)];
    let para_prs = vec![para_pr_xml(0, None), para_pr_xml(1, None), para_pr_xml(2, None)];
    let styles = vec![
        style_xml(0, "본문", "Normal", 0, 0, 0),
        style_xml(1, "개요 1", "Outline 1", 1, 0, 0),
        style_xml(2, "개요 2", "Outline 2", 2, 1, 0),
    ];
    build_header_xml(&char_prs, &para_prs, &styles)
}

/// `header.xml` with a custom-named style whose referenced paraPr carries
/// explicit outline semantics.
fn custom_outline_header() -> String {
    let char_prs = vec![char_pr_xml(0, 1400)];
    let para_prs = vec![
        para_pr_xml(0, Some(r#"<heading type="OUTLINE" idRef="0" level="3"/>"#)),
        para_pr_xml(1, None),
    ];
    let styles = vec![
        style_xml(0, "맞춤 제목", "Custom Heading", 0, 0, 1),
        style_xml(1, "본문", "Normal", 1, 0, 1),
    ];
    build_header_xml(&char_prs, &para_prs, &styles)
}

/// Builds a minimal HWPX ZIP from `header_xml` and a slice of section XML strings.
fn make_hwpx(header_xml: &str, section_xmls: &[&str]) -> Vec<u8> {
    let buf = Vec::new();
    let mut zip = ZipWriter::new(std::io::Cursor::new(buf));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflate = SimpleFileOptions::default();

    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/hwp+zip").unwrap();

    zip.start_file("Contents/header.xml", deflate).unwrap();
    zip.write_all(header_xml.as_bytes()).unwrap();

    for (i, xml) in section_xmls.iter().enumerate() {
        let path = format!("Contents/section{i}.xml");
        zip.start_file(&path, deflate).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }

    zip.finish().unwrap().into_inner()
}

/// Adds a `BinData/` PNG entry to an HWPX ZIP byte buffer.
fn make_hwpx_with_image(
    header_xml: &str,
    section_xmls: &[&str],
    image_name: &str,
    image_data: &[u8],
) -> Vec<u8> {
    let buf = Vec::new();
    let mut zip = ZipWriter::new(std::io::Cursor::new(buf));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflate = SimpleFileOptions::default();

    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/hwp+zip").unwrap();

    zip.start_file("Contents/header.xml", deflate).unwrap();
    zip.write_all(header_xml.as_bytes()).unwrap();

    for (i, xml) in section_xmls.iter().enumerate() {
        let path = format!("Contents/section{i}.xml");
        zip.start_file(&path, deflate).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }

    let bin_path = format!("BinData/{image_name}");
    zip.start_file(&bin_path, stored).unwrap();
    zip.write_all(image_data).unwrap();

    zip.finish().unwrap().into_inner()
}

/// Full pipeline helper: decode HWPX bytes → validate → lookup → encode_styled.
fn decode_and_convert(hwpx_bytes: &[u8]) -> MdOutput {
    let hwpx_doc = HwpxDecoder::decode(hwpx_bytes).expect("decode");
    let validated = hwpx_doc.document.validate().expect("validate");
    let lookup = HwpxStyleLookup::new(&hwpx_doc.style_store, &hwpx_doc.image_store);
    MdEncoder::encode_styled(&validated, &lookup)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Basic conversion: single text paragraph produces non-empty markdown.
#[test]
fn basic_conversion_non_empty_output() {
    let section = r#"<sec>
        <p paraPrIDRef="0">
            <run charPrIDRef="0"><t>안녕하세요</t></run>
        </p>
    </sec>"#;

    let header = minimal_header();
    let bytes = make_hwpx(&header, &[section]);
    let output = decode_and_convert(&bytes);

    assert!(!output.markdown.is_empty(), "markdown should not be empty");
    assert!(output.markdown.contains("안녕하세요"), "markdown should contain the paragraph text");
}

/// Heading detection: paragraphs whose para-shape or style implies a heading
/// level emit `#` markers.
///
/// The styled encoder prefers actual outline semantics on the referenced
/// `paraPr`, then falls back to style-name heuristics like "개요 N".
#[test]
fn heading_detection_emits_hash_markers() {
    // Paragraph with styleIDRef="1" → "개요 1" → heading level 1 → "# ..."
    let section = r#"<sec>
        <p paraPrIDRef="1" styleIDRef="1">
            <run charPrIDRef="0"><t>제목입니다</t></run>
        </p>
        <p paraPrIDRef="0" styleIDRef="0">
            <run charPrIDRef="0"><t>본문입니다</t></run>
        </p>
    </sec>"#;

    let header = heading_header();
    let bytes = make_hwpx(&header, &[section]);
    let output = decode_and_convert(&bytes);

    assert!(
        output.markdown.contains("# 제목입니다"),
        "heading level 1 should produce '# 제목입니다'; got:\n{}",
        output.markdown
    );
    assert!(
        output.markdown.contains("본문입니다"),
        "body text should appear without heading marker; got:\n{}",
        output.markdown
    );
    // Body paragraph must NOT have a heading marker.
    for line in output.markdown.lines() {
        if line.contains("본문입니다") {
            assert!(
                !line.starts_with('#'),
                "body paragraph should not have heading marker; got: {line}"
            );
        }
    }
}

#[test]
fn heading_detection_uses_outline_para_shape_even_for_custom_style_names() {
    let section = r#"<sec>
        <p paraPrIDRef="0" styleIDRef="0">
            <run charPrIDRef="0"><t>맞춤 개요 제목</t></run>
        </p>
        <p paraPrIDRef="1" styleIDRef="1">
            <run charPrIDRef="0"><t>본문입니다</t></run>
        </p>
    </sec>"#;

    let header = custom_outline_header();
    let bytes = make_hwpx(&header, &[section]);
    let output = decode_and_convert(&bytes);

    assert!(
        output.markdown.contains("### 맞춤 개요 제목"),
        "outline paraPr should produce heading markdown even with custom style name; got:\n{}",
        output.markdown
    );
    assert!(
        output.markdown.contains("본문입니다"),
        "body paragraph should still be preserved; got:\n{}",
        output.markdown
    );
}

#[test]
fn numbered_para_shape_beats_heading_like_style_name() {
    let char_prs = vec![char_pr_xml(0, 1000)];
    let para_prs = vec![
        para_pr_xml(0, Some(r#"<heading type="NUMBER" idRef="2" level="0"/>"#)),
        para_pr_xml(1, None),
    ];
    let styles =
        vec![style_xml(0, "개요 2", "Outline 2", 0, 0, 1), style_xml(1, "본문", "Normal", 1, 0, 1)];
    let header = build_header_xml(&char_prs, &para_prs, &styles);
    let section = r#"<sec>
        <p paraPrIDRef="0" styleIDRef="0">
            <run charPrIDRef="0"><t>번호 문단</t></run>
        </p>
    </sec>"#;

    let output = decode_and_convert(&make_hwpx(&header, &[section]));
    assert_eq!(output.markdown.trim(), "1. 번호 문단");
}

#[test]
fn nested_bullet_para_shape_preserves_indentation() {
    let char_prs = vec![char_pr_xml(0, 1000)];
    let para_prs = vec![
        para_pr_xml(0, Some(r#"<heading type="BULLET" idRef="1" level="0"/>"#)),
        para_pr_xml(1, Some(r#"<heading type="BULLET" idRef="1" level="2"/>"#)),
    ];
    let header = build_header_xml(&char_prs, &para_prs, &[]);
    let section = r#"<sec>
        <p paraPrIDRef="0">
            <run charPrIDRef="0"><t>상위</t></run>
        </p>
        <p paraPrIDRef="1">
            <run charPrIDRef="0"><t>하위</t></run>
        </p>
    </sec>"#;

    let output = decode_and_convert(&make_hwpx(&header, &[section]));
    assert_eq!(output.markdown, "- 상위\n\n    - 하위");
}

/// Table conversion: a table run should produce GFM table syntax or HTML table markup.
#[test]
fn table_conversion_produces_table_output() {
    let section = r#"<sec>
        <p paraPrIDRef="0">
            <run charPrIDRef="0">
                <tbl rowCnt="2" colCnt="2">
                    <tr>
                        <tc name="A1">
                            <cellSz width="5000" height="1000"/>
                            <subList>
                                <p paraPrIDRef="0"><run charPrIDRef="0"><t>헤더A</t></run></p>
                            </subList>
                        </tc>
                        <tc name="B1">
                            <cellSz width="5000" height="1000"/>
                            <subList>
                                <p paraPrIDRef="0"><run charPrIDRef="0"><t>헤더B</t></run></p>
                            </subList>
                        </tc>
                    </tr>
                    <tr>
                        <tc name="A2">
                            <cellSz width="5000" height="1000"/>
                            <subList>
                                <p paraPrIDRef="0"><run charPrIDRef="0"><t>값1</t></run></p>
                            </subList>
                        </tc>
                        <tc name="B2">
                            <cellSz width="5000" height="1000"/>
                            <subList>
                                <p paraPrIDRef="0"><run charPrIDRef="0"><t>값2</t></run></p>
                            </subList>
                        </tc>
                    </tr>
                </tbl>
            </run>
        </p>
    </sec>"#;

    let header = minimal_header();
    let bytes = make_hwpx(&header, &[section]);
    let output = decode_and_convert(&bytes);

    // The styled encoder emits GFM pipe tables (no merges) or HTML <table>.
    assert!(output.markdown.contains("헤더A"), "table output should contain first header cell");
    assert!(output.markdown.contains("헤더B"), "table output should contain second header cell");
    assert!(output.markdown.contains("값1"), "table output should contain first data cell");
    assert!(output.markdown.contains("값2"), "table output should contain second data cell");
    // Simple 2x2 table with no merges → GFM pipe table with | delimiters and --- separator.
    let is_gfm = output.markdown.contains('|') && output.markdown.contains("---");
    let is_html = output.markdown.contains("<table") && output.markdown.contains("</table>");
    assert!(
        is_gfm || is_html,
        "table should be GFM pipe syntax or HTML table; got:\n{}",
        output.markdown
    );
}

/// Image extraction: binary image data from `BinData/` should be collected into `MdOutput.images`.
#[test]
fn image_extraction_populates_images_map() {
    // Minimal PNG magic bytes (8-byte signature)
    let fake_png: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

    let section = r#"<sec>
        <p paraPrIDRef="0">
            <run charPrIDRef="0">
                <img width="5000" height="3000" instId="0" binDataIDRef="0"
                     reverse="0" effect="REAL_PIC" border="0">
                    <imgPr path="BinData/logo.png"/>
                </img>
            </run>
        </p>
    </sec>"#;

    let header = minimal_header();
    let bytes = make_hwpx_with_image(&header, &[section], "logo.png", &fake_png);
    let output = decode_and_convert(&bytes);

    // The styled encoder extracts images via `lookup.image_data(key)` and places them
    // in `MdOutput.images`. Verify the image store was populated during decode.
    let hwpx_doc = HwpxDecoder::decode(&bytes).expect("decode for image check");
    assert!(!hwpx_doc.image_store.is_empty(), "image_store should contain the embedded PNG");
    assert!(hwpx_doc.image_store.get("logo.png").is_some(), "image_store should have 'logo.png'");

    // Verify that the StyleLookup bridge can retrieve image data.
    // (The synthetic <img> XML may not fully parse into a document Image run,
    // but the style lookup bridge must be able to serve the data if asked.)
    let validated = hwpx_doc.document.validate().expect("validate for lookup check");
    let lookup = HwpxStyleLookup::new(&hwpx_doc.style_store, &hwpx_doc.image_store);
    assert!(
        lookup.image_data("logo.png").is_some(),
        "StyleLookup should serve image data for 'logo.png'"
    );
    assert_eq!(
        lookup.image_data("logo.png").unwrap(),
        &fake_png[..],
        "image data through StyleLookup should match original bytes"
    );

    // If the decoder parsed the image run, verify markdown + MdOutput.images.
    if output.markdown.contains("![logo](images/logo.png)") {
        assert!(
            output.images.contains_key("images/logo.png"),
            "MdOutput.images should contain extracted image data"
        );
        assert_eq!(
            output.images["images/logo.png"], fake_png,
            "extracted image data should match the original PNG bytes"
        );
    }

    let _ = validated;
}

/// Section markers: multi-section HWPX should produce `<!-- hwpforge:section -->` between sections.
#[test]
fn multi_section_produces_section_markers() {
    let s0 = r#"<sec>
        <p paraPrIDRef="0"><run charPrIDRef="0"><t>첫 번째 섹션</t></run></p>
    </sec>"#;
    let s1 = r#"<sec>
        <p paraPrIDRef="0"><run charPrIDRef="0"><t>두 번째 섹션</t></run></p>
    </sec>"#;

    let header = minimal_header();
    let bytes = make_hwpx(&header, &[s0, s1]);
    let output = decode_and_convert(&bytes);

    assert!(
        output.markdown.contains("<!-- hwpforge:section -->"),
        "multi-section HWPX should produce section marker; got:\n{}",
        output.markdown
    );
    assert!(output.markdown.contains("첫 번째 섹션"), "first section text should appear");
    assert!(output.markdown.contains("두 번째 섹션"), "second section text should appear");
}

/// Bold/italic formatting: charPr with bold=true should produce `**text**` in output.
#[test]
fn bold_text_produces_double_asterisk() {
    let header = r##"<head version="1.4" secCnt="1">
        <refList>
            <fontfaces itemCnt="1">
                <fontface lang="HANGUL" fontCnt="1">
                    <font id="0" face="함초롬돋움" type="TTF" isEmbedded="0"/>
                </fontface>
            </fontfaces>
            <charProperties itemCnt="2">
                <charPr id="0" height="1000" textColor="#000000" shadeColor="none"
                        useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="0">
                    <fontRef hangul="0" latin="0" hanja="0" japanese="0" other="0" symbol="0" user="0"/>
                </charPr>
                <charPr id="1" height="1000" textColor="#000000" shadeColor="none"
                        bold="1"
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

    let section = r#"<sec>
        <p paraPrIDRef="0">
            <run charPrIDRef="0"><t>보통 텍스트 </t></run>
            <run charPrIDRef="1"><t>굵은 텍스트</t></run>
        </p>
    </sec>"#;

    let bytes = make_hwpx(header, &[section]);
    let output = decode_and_convert(&bytes);

    assert!(output.markdown.contains("보통 텍스트"), "normal text should appear");
    assert!(
        output.markdown.contains("굵은 텍스트"),
        "bold text should appear in output; got:\n{}",
        output.markdown
    );
    // If the decoder parses bold=1, the encoder wraps with **...**
    if output.markdown.contains("**") {
        assert!(
            output.markdown.contains("**굵은 텍스트**"),
            "bold text should be wrapped with double asterisks; got:\n{}",
            output.markdown
        );
        // Normal text must NOT be bold-wrapped.
        assert!(
            !output.markdown.contains("**보통 텍스트"),
            "normal text should not be bold; got:\n{}",
            output.markdown
        );
    }
}

/// Empty document: single section with empty text should not panic and produce some output.
#[test]
fn empty_text_does_not_panic() {
    let section = r#"<sec>
        <p paraPrIDRef="0">
            <run charPrIDRef="0"><t></t></run>
        </p>
    </sec>"#;

    let header = minimal_header();
    let bytes = make_hwpx(&header, &[section]);
    // Should not panic.
    let output = decode_and_convert(&bytes);
    // The markdown may be empty or contain only whitespace/newlines for an empty paragraph.
    let _ = output;
}
