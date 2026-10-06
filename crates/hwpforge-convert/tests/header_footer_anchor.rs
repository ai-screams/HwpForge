//! HWP5 header/footer anchors (§19.17 F2) against the X4 fixtures.
//!
//! x4a: header A in top-level paragraph 3, B in 28. x4c: footer D inside the
//! cell of the table held by paragraph 6 — before, its paragraph was read as
//! one more cell paragraph and the control was dropped as an unknown
//! control.

use std::io::{Cursor, Read};
use std::path::PathBuf;

use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};
use hwpforge_core::Paragraph;
use hwpforge_smithy_hwpx::HwpxDecoder;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/header_footer")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn section0(hwpx: &[u8]) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(hwpx)).expect("zip");
    let mut xml = String::new();
    archive.by_name("Contents/section0.xml").expect("section").read_to_string(&mut xml).unwrap();
    xml
}

/// Top-level paragraph indices whose XML holds `needle`.
fn paragraphs_with(xml: &str, needle: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let (mut depth, mut index, mut start, mut i) = (0usize, 0usize, 0usize, 0usize);
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
                if xml[start..i].contains(needle) {
                    found.push(index);
                }
                index += 1;
            }
        } else {
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    found
}

/// F-T11. 이것을 실패시키는 것: 투영이 앵커를 채우지 않기(둘 다 문단 0) /
/// 앵커를 HWP5 문단 순번이 아닌 다른 값으로.
#[test]
fn x4a_hwp5_headers_land_in_their_paragraphs() {
    let (hwpx, _) = hwp5_to_hwpx_bytes_with_diagnostics(
        &fixture("x4a-later-header.hwp"),
        ConvertOptions::default(),
    )
    .expect("convert");
    assert_eq!(paragraphs_with(&section0(&hwpx), "<hp:header "), [3, 28]);
    let doc = HwpxDecoder::decode(&hwpx).expect("decode");
    let anchors: Vec<_> =
        doc.document.sections()[0].headers.iter().map(|h| h.anchor_paragraph).collect();
    assert_eq!(anchors, [Some(3), Some(28)]);
}

/// F-T4 + F-T7: the cell footer reaches the section anchored to paragraph 6,
/// its text no longer sits in the cell, the "unsupported ctrl" warning is
/// gone, and the conversion succeeds with and without the layout cache (the
/// hint queues still line up).
///
/// 이것을 실패시키는 것: 디코더의 칸 머리말/꼬리말 갈래 삭제(꼬리말 0,
/// "꼬리말 D" 가 칸 문단으로) / 투영의 표 칸 재귀 삭제(꼬리말 0) / layout
/// hint 의 칸 문단을 본문 scope 로(hint 수 부족 → 변환 실패).
#[test]
fn cell_footer_converts_with_and_without_the_layout_cache() {
    for carry in [false, true] {
        let (hwpx, warnings) = hwp5_to_hwpx_bytes_with_diagnostics(
            &fixture("x4c-footer-in-spanning-cell.hwp"),
            ConvertOptions::default().with_carry_layout_cache(carry),
        )
        .unwrap_or_else(|e| panic!("carry={carry}: {e}"));
        let text: Vec<String> = warnings.iter().map(|w| format!("{w:?}")).collect();
        assert!(!text.iter().any(|w| w.contains("unsupported ctrl 'foot'")), "{text:?}");

        let xml = section0(&hwpx);
        assert_eq!(paragraphs_with(&xml, "<hp:footer "), [6], "carry={carry}");
        let footer_at = xml.find("<hp:footer ").expect("footer");
        let footer_end = xml.find("</hp:footer>").expect("footer end");
        let d_at = xml.find("꼬리말 D").expect("footer text");
        assert!(footer_at < d_at && d_at < footer_end, "footer text outside the footer");
        assert_eq!(xml.matches("꼬리말 D").count(), 1, "footer text duplicated in the cell");

        let doc = HwpxDecoder::decode(&hwpx).expect("decode");
        let section = &doc.document.sections()[0];
        assert_eq!(section.footers.len(), 1);
        assert_eq!(section.footers[0].anchor_paragraph, Some(6));
        let footer: String =
            section.footers[0].paragraphs.iter().map(Paragraph::text_content).collect();
        assert_eq!(footer, "꼬리말 D");
    }
}
