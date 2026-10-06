//! Header/footer anchors (§19.17 F2, X4 fixtures — Hancom-authored).
//!
//! Hancom applies a header/footer from the page its control sits on, and a
//! control in a table cell as if it sat in the top-level paragraph that holds
//! the table (x4c). The decoder records that paragraph as
//! `HeaderFooter::anchor_paragraph`; the encoder writes the control into the
//! first run of that paragraph instead of the first paragraph.
//!
//! - x4a: header A in top-level paragraph 3, header B in 28 (PDF: pages 1–2
//!   A, 3–5 B).
//! - x4b: header C alone in paragraph 14 (PDF: no header on page 1).
//! - x4c: footer D in a cell of the 1×1 table held by paragraph 6, the cell
//!   paragraph on page 2 (PDF: footer from page 1).

use std::io::{Cursor, Read};

use hwpforge_core::image::ImageStore;
use hwpforge_core::run::Run;
use hwpforge_core::section::{HeaderFooter, Section};
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{EncodeOptions, HwpxDecoder, HwpxEncoder};

fn fixture(path: &str) -> Vec<u8> {
    let path = format!("{}/../../tests/fixtures/{path}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn section_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name("Contents/section0.xml").expect("section0").read_to_string(&mut out).unwrap();
    out
}

fn reencode_with(bytes: &[u8], options: EncodeOptions) -> Vec<u8> {
    let doc = HwpxDecoder::decode(bytes).expect("decode");
    HwpxEncoder::encode_with_options(
        &doc.document.validate().expect("validate"),
        &doc.style_store,
        &doc.image_store,
        options,
    )
    .expect("encode")
}

fn reencode(bytes: &[u8]) -> Vec<u8> {
    reencode_with(bytes, EncodeOptions::default())
}

/// The top-level `<hp:p>` elements of a section part, in order (nested
/// paragraphs stay inside their parent's slice).
fn top_level_paragraphs(xml: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < xml.len() {
        let rest = &xml[i..];
        if rest.starts_with("<hp:p ") || rest.starts_with("<hp:p>") {
            if depth == 0 {
                start = i;
            }
            depth += 1;
            i += 5;
        } else if rest.starts_with("</hp:p>") {
            depth -= 1;
            i += 7;
            if depth == 0 {
                out.push(&xml[start..i]);
            }
        } else {
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    out
}

/// Top-level paragraph indices holding `needle`, in order.
fn paragraphs_with(xml: &str, needle: &str) -> Vec<usize> {
    top_level_paragraphs(xml)
        .iter()
        .enumerate()
        .filter(|(_, p)| p.contains(needle))
        .map(|(i, _)| i)
        .collect()
}

fn header_texts(doc: &Document) -> Vec<(Option<usize>, String)> {
    doc.sections()[0]
        .headers
        .iter()
        .map(|h| (h.anchor_paragraph, h.paragraphs.iter().map(Paragraph::text_content).collect()))
        .collect()
}

// F-T8. 이것을 실패시키는 것: 인코더가 모두 첫 문단에 주입(앵커 무시) /
// 디코더가 앵커를 버림.
#[test]
fn x4a_headers_stay_in_their_paragraphs() {
    let source = fixture("header_footer/x4a-later-header.hwpx");
    let decoded = HwpxDecoder::decode(&source).expect("decode");
    let headers = header_texts(&decoded.document);
    assert_eq!(headers.len(), 2);
    assert_eq!(headers[0].0, Some(3));
    assert_eq!(headers[1].0, Some(28));
    assert!(headers[0].1.contains('A') && headers[1].1.contains('B'), "{headers:?}");

    let xml = section_xml(&reencode(&source));
    assert_eq!(paragraphs_with(&xml, "<hp:header "), [3, 28]);
    assert_eq!(
        top_level_paragraphs(&xml).len(),
        decoded.document.sections()[0].paragraphs.len(),
        "the placeholder run must not add a paragraph"
    );
}

// F-T9. 이것을 실패시키는 것: 같은 두 변이 — 문단 0 에 머리말이 생긴다.
#[test]
fn x4b_header_is_not_moved_to_the_first_paragraph() {
    let source = fixture("header_footer/x4b-header-only-later.hwpx");
    let decoded = HwpxDecoder::decode(&source).expect("decode");
    assert_eq!(header_texts(&decoded.document)[0].0, Some(14));

    let xml = section_xml(&reencode(&source));
    assert_eq!(paragraphs_with(&xml, "<hp:header "), [14]);
}

// F-T10. 이것을 실패시키는 것: 칸 안 것을 앵커 없이 올리기(문단 0 으로) /
// 앵커를 칸 문단 index 로(범위 밖·다른 문단) / 칸 안 수집 제거(꼬리말 소실).
#[test]
fn x4c_cell_footer_goes_to_the_paragraph_holding_the_table() {
    let source = fixture("header_footer/x4c-footer-in-spanning-cell.hwpx");
    let decoded = HwpxDecoder::decode(&source).expect("decode");
    let section = &decoded.document.sections()[0];
    assert_eq!(section.footers.len(), 1);
    assert_eq!(section.footers[0].anchor_paragraph, Some(6));
    let text: String = section.footers[0].paragraphs.iter().map(Paragraph::text_content).collect();
    assert!(text.contains('D'), "{text}");

    let xml = section_xml(&reencode(&source));
    assert_eq!(paragraphs_with(&xml, "<hp:footer "), [6]);
    let para6 = top_level_paragraphs(&xml)[6];
    let first_run_end = para6.find("</hp:run>").expect("run");
    assert!(para6[..first_run_end].contains("<hp:footer "), "footer not in the first run");
    let table = &para6[para6.find("<hp:tbl ").expect("table")..];
    assert!(!table.contains("<hp:footer "), "footer left inside the cell");
}

// x4c 의 칸 문단(꼬리말이 빠짐)과 문단 6(꼬리말 ctrl 이 앞에 들어감), x4a 의
// 여러 줄 문단 3·28(머리말 ctrl 이 앞에 들어감)의 줄 조판 캐시가 왕복에서
// 그대로다. 이것을 실패시키는 것: 앵커 문단에 선행 ctrl 수 0 을 넘기기(x4a
// 문단 3 의 둘째 줄부터 textpos 가 8 유닛 어긋남).
#[test]
fn anchored_footer_keeps_both_layout_caches() {
    for name in ["x4c-footer-in-spanning-cell.hwpx", "x4a-later-header.hwpx"] {
        let source = fixture(&format!("header_footer/{name}"));
        let first = HwpxDecoder::decode(&source).expect("decode");
        let out = reencode_with(&source, EncodeOptions::default().with_emit_layout_cache(true));
        let second = HwpxDecoder::decode(&out).expect("decode again");
        assert!(
            !second.warnings.iter().any(|w| format!("{w:?}").contains("LayoutCacheDropped")),
            "{name}: {:?}",
            second.warnings
        );
        let caches = |doc: &Document| -> Vec<String> {
            let mut out = Vec::new();
            doc.sections()[0].for_each_paragraph(|p| out.push(format!("{:?}", p.layout_cache)));
            out
        };
        let (a, b) = (caches(&first.document), caches(&second.document));
        assert_eq!(a.len(), b.len(), "{name}");
        assert!(a.iter().any(|c| c != "None"), "{name}");
        assert_eq!(a, b, "{name}");
    }
    let x4a = HwpxDecoder::decode(&fixture("header_footer/x4a-later-header.hwpx")).unwrap();
    let lines =
        x4a.document.sections()[0].paragraphs[3].layout_cache.as_ref().map_or(0, |c| c.lines.len());
    assert!(lines > 1, "paragraph 3 must span lines for this test to see an offset");
}

fn api_doc(anchor: Option<usize>) -> Vec<u8> {
    let para = |t: &str| {
        Paragraph::with_runs(vec![Run::text(t, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
    };
    let mut section = Section::with_paragraphs(vec![para("one"), para("two")], PageSettings::a4());
    let mut header = HeaderFooter::all_pages(vec![para("head")]);
    header.anchor_paragraph = anchor;
    section.headers.push(header);
    let mut doc = Document::new();
    doc.add_section(section);
    let mut styles = HwpxStyleStore::new();
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
        .expect("encode")
}

// F-T12: 앵커 없는 API 문서는 첫 문단에 쓰이고, `Some(0)`(JSON 으로 올 수
// 있음)도 같은 뜻이다. 이것을 실패시키는 것: `None` 을 0 이 아닌 문단으로
// 처리 / `Some(0)` 에 자리표시 run 을 만들기.
#[test]
fn unanchored_header_is_written_into_the_first_paragraph() {
    let none = section_xml(&api_doc(None));
    let zero = section_xml(&api_doc(Some(0)));
    let strip_ids = |x: &str| {
        let re_free: String =
            x.split(" id=\"").map(|s| s.split_once('"').map_or(s, |(_, r)| r)).collect();
        re_free
    };
    assert_eq!(paragraphs_with(&none, "<hp:header "), [0]);
    assert!(
        none.find("<hp:colPr").unwrap() < none.find("<hp:header ").unwrap(),
        "paragraph 0 headers follow the column control, as before"
    );
    assert_eq!(strip_ids(&none), strip_ids(&zero));
    let one = section_xml(&api_doc(Some(1)));
    assert_eq!(paragraphs_with(&one, "<hp:header "), [1]);
}
