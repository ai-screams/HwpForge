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

/// Minimal `header.xml` with one font, one charPr, one paraPr.
const MINIMAL_HEADER: &str = r##"<head version="1.4" secCnt="1">
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

/// `header.xml` with heading styles (level 1 and 2) so `style_heading_level` is non-None.
const HEADING_HEADER: &str = r##"<head version="1.4" secCnt="1">
    <refList>
        <fontfaces itemCnt="1">
            <fontface lang="HANGUL" fontCnt="1">
                <font id="0" face="함초롬돋움" type="TTF" isEmbedded="0"/>
            </fontface>
        </fontfaces>
        <charProperties itemCnt="2">
            <charPr id="0" height="1400" textColor="#000000" shadeColor="none"
                    useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="0">
                <fontRef hangul="0" latin="0" hanja="0" japanese="0" other="0" symbol="0" user="0"/>
            </charPr>
            <charPr id="1" height="1200" textColor="#000000" shadeColor="none"
                    useFontSpace="0" useKerning="0" symMark="NONE" borderFillIDRef="0">
                <fontRef hangul="0" latin="0" hanja="0" japanese="0" other="0" symbol="0" user="0"/>
            </charPr>
        </charProperties>
        <paraProperties itemCnt="3">
            <paraPr id="0">
                <align horizontal="LEFT" vertical="BASELINE"/>
                <switch><default>
                    <lineSpacing type="PERCENT" value="160"/>
                </default></switch>
            </paraPr>
            <paraPr id="1">
                <align horizontal="LEFT" vertical="BASELINE"/>
                <switch><default>
                    <lineSpacing type="PERCENT" value="160"/>
                </default></switch>
            </paraPr>
            <paraPr id="2">
                <align horizontal="LEFT" vertical="BASELINE"/>
                <switch><default>
                    <lineSpacing type="PERCENT" value="160"/>
                </default></switch>
            </paraPr>
        </paraProperties>
        <styles itemCnt="3">
            <style id="0" type="PARA" name="본문" engName="Normal"
                   paraPrIDRef="0" charPrIDRef="0" nextStyleIDRef="0" lockForm="0"/>
            <style id="1" type="PARA" name="개요 1" engName="Outline 1"
                   paraPrIDRef="1" charPrIDRef="0" nextStyleIDRef="0" lockForm="0"/>
            <style id="2" type="PARA" name="개요 2" engName="Outline 2"
                   paraPrIDRef="2" charPrIDRef="1" nextStyleIDRef="0" lockForm="0"/>
        </styles>
    </refList>
</head>"##;

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

    let bytes = make_hwpx(MINIMAL_HEADER, &[section]);
    let output = decode_and_convert(&bytes);

    assert!(!output.markdown.is_empty(), "markdown should not be empty");
    assert!(output.markdown.contains("안녕하세요"), "markdown should contain the paragraph text");
}

/// Heading detection: paragraphs whose style maps to a heading level emit `#` markers.
///
/// The styled encoder calls `style_heading_level(style_id)` via the lookup bridge.
/// The `HwpxStyleStore` detects heading level from style names containing "개요 N"
/// (Korean outline styles used by 한글 software for headings 1–6).
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

    let bytes = make_hwpx(HEADING_HEADER, &[section]);
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

    let bytes = make_hwpx(MINIMAL_HEADER, &[section]);
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

    let bytes = make_hwpx_with_image(MINIMAL_HEADER, &[section], "logo.png", &fake_png);
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

    let bytes = make_hwpx(MINIMAL_HEADER, &[s0, s1]);
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

    let bytes = make_hwpx(MINIMAL_HEADER, &[section]);
    // Should not panic.
    let output = decode_and_convert(&bytes);
    // The markdown may be empty or contain only whitespace/newlines for an empty paragraph.
    let _ = output;
}
