//! D: HWP5 table captions against the HWPX Hancom saved from the same
//! document. Two Hancom (macOS) fixtures: `sample-table-caption-fullsz`
//! (two 3×2 tables, a bottom caption without and with `fullSz`) and
//! `sample-table-caption-split` (two 60-row tables split over pages, a
//! bottom and a top caption).

use std::io::{Cursor, Read};
use std::path::PathBuf;

use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};

fn fixture(name: &str, ext: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../tests/fixtures/user_samples/{name}.{ext}"));
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn section(hwpx: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(hwpx)).expect("zip");
    let mut xml = String::new();
    zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut xml).unwrap();
    xml
}

/// Every `<hp:caption>…</hp:caption>` with the subList's own attributes
/// left out (they are the encoder's, not the caption's).
fn captions(section: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = section;
    while let Some(at) = rest.find("<hp:caption ") {
        let end = rest[at..].find("</hp:caption>").expect("closed") + at;
        let element = &rest[at..end];
        let sub = element.find("<hp:subList").expect("subList");
        let sub_end = sub + element[sub..].find('>').expect("tag end");
        out.push(format!("{}{}", &element[..sub], &element[sub_end + 1..]));
        rest = &rest[end..];
    }
    out
}

/// The caption elements — attributes, paragraphs and their line cache —
/// equal Hancom's for both fixtures when the layout cache is carried, and
/// the conversion succeeds without it too.
/// 이것을 실패시키는 것: 캡션 디코드 삭제(0 개) · `fullSz`·`lastWidth` 인코드
/// 복귀 · 캡션 문단 hint 를 칸 뒤로(변환 실패) · 캡션 줄 캐시 누락.
#[test]
fn table_caption_converts_with_its_line_cache() {
    for name in ["sample-table-caption-fullsz", "sample-table-caption-split"] {
        let hancom = captions(&section(&fixture(name, "hwpx")));
        assert_eq!(hancom.len(), 2, "{name}");
        let options = ConvertOptions::default().with_carry_layout_cache(true);
        let (ours, _) =
            hwp5_to_hwpx_bytes_with_diagnostics(&fixture(name, "hwp"), options).expect("convert");
        assert_eq!(captions(&section(&ours)), hancom, "{name}");
        let (plain, _) =
            hwp5_to_hwpx_bytes_with_diagnostics(&fixture(name, "hwp"), ConvertOptions::default())
                .expect("convert without the cache");
        assert_eq!(captions(&section(&plain)).len(), 2, "{name}");
    }
}
